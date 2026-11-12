//! `reset` / `step` over one course (plan §4).
//!
//! Per tick, in order: integrate the view angles from the action (clamped), build the `UserCmd`, run
//! `process_movement`, append the command to the episode's `Recording`, advance the `CourseRun`, update
//! the tracked takeoff facts, and score the tick. The `Recording` alone reproduces the episode bit for
//! bit through `Recording::replay`, which builds its mode and config the same way `reset` does.

use std::sync::Arc;

use course::{Course, CourseRun, Status};
use movement::replay::Recording;
use movement::{process_movement, tick_interval, MovementConfig, MovementMode, NullObserver, PlayerState, Vec3};

use crate::action::Action;
use crate::config::EnvConfig;
use crate::obs::{GoalVector, ObsCtx, ObsProvider, ObsSpec, Proprioception};
use crate::reward::{BaseReward, Reward, RewardCtx};

/// Per-episode facts the sim state does not carry, kept for observations.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Track {
    /// Ticks run this episode.
    pub episode_ticks: u32,
    /// The last action as applied (view deltas after clamping).
    pub prev_action: Action,
    pub last_yaw_delta: f32,
    pub last_pitch_delta: f32,
    /// Ticks since the last ground-to-air transition; `None` before the first one.
    pub ticks_since_takeoff: Option<u32>,
    /// Horizontal speed after the takeoff tick.
    pub last_takeoff_speed: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepInfo {
    /// Sum of the per-tick rewards over the ticks this step ran.
    pub reward: f32,
    /// Completed, hazard, or out of bounds.
    pub terminated: bool,
    /// Hit `max_episode_ticks` without terminating.
    pub truncated: bool,
    pub status: Status,
    /// Timer ticks of the run (0 while idle in the start zone).
    pub run_ticks: u32,
    pub episode_ticks: u32,
    /// The run's time in ticks, on the step that completes it.
    pub completed_ticks: Option<u32>,
    /// Straight-line distance from the origin to the end target, if the end has one.
    pub distance_to_goal: Option<f32>,
}

/// Everything that determines how an episode continues (plan §4): sim state, mode-private state, view
/// angles, timer, tracked facts, reward state, and the commands so far. Restoring it and stepping on
/// equals never having stopped, so it serves curriculum starts and mid-course training. The recording is
/// kept whole, so an episode resumed from a snapshot still replays from its true start.
pub struct Snapshot {
    course: Arc<Course>,
    mode: Box<dyn MovementMode + Send + Sync>,
    state: PlayerState,
    view_angles: Vec3,
    run: CourseRun,
    track: Track,
    reward: Box<dyn Reward>,
    recording: Recording,
    seed: u64,
    done: bool,
}

impl Clone for Snapshot {
    fn clone(&self) -> Self {
        Self {
            course: self.course.clone(),
            mode: self.mode.fork(),
            state: self.state.clone(),
            view_angles: self.view_angles,
            run: self.run,
            track: self.track,
            reward: self.reward.fork(),
            recording: self.recording.clone(),
            seed: self.seed,
            done: self.done,
        }
    }
}

impl Snapshot {
    pub fn state(&self) -> &PlayerState {
        &self.state
    }
    pub fn run(&self) -> &CourseRun {
        &self.run
    }
    pub fn episode_ticks(&self) -> u32 {
        self.track.episode_ticks
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub obs: Vec<f32>,
    pub info: StepInfo,
}

pub struct Env {
    course: Arc<Course>,
    cfg: EnvConfig,
    tickrate: u32,
    dt: f32,
    move_cfg: MovementConfig,
    mode: Box<dyn MovementMode + Send + Sync>,
    state: PlayerState,
    view_angles: Vec3,
    run: CourseRun,
    track: Track,
    providers: Vec<Box<dyn ObsProvider>>,
    spec: ObsSpec,
    reward: Box<dyn Reward>,
    recording: Recording,
    seed: u64,
    done: bool,
}

impl Env {
    /// An env with the default observation (proprioception + goal vector) and the base reward. Call
    /// `reset` before the first `step`.
    pub fn new(course: Arc<Course>, cfg: EnvConfig) -> Result<Self, String> {
        let mode = cfg.mode.create();
        let tickrate = match (cfg.tickrate, mode.required_tickrate()) {
            (Some(t), Some(r)) if t != r => {
                return Err(format!("{} requires tickrate {r}, got {t}", cfg.mode.name()));
            }
            (Some(t), _) => t,
            (None, Some(r)) => r,
            (None, None) => 128,
        };
        if cfg.action_repeat == 0 {
            return Err("action_repeat must be at least 1".into());
        }
        let mut move_cfg = *mode.config();
        move_cfg.autobhop = cfg.autobhop;
        let providers: Vec<Box<dyn ObsProvider>> = vec![Box::new(Proprioception), Box::new(GoalVector)];
        let spec = ObsSpec::from_blocks(providers.iter().map(|p| p.spec()));
        let state = PlayerState::new(course.spawn.0);
        let recording = Recording::new(cfg.mode, tickrate, state.clone());
        Ok(Self {
            view_angles: Vec3::new(0.0, course.spawn.1, 0.0),
            course,
            cfg,
            tickrate,
            dt: tick_interval(tickrate),
            move_cfg,
            mode,
            state,
            run: CourseRun::new(),
            track: Track::default(),
            providers,
            spec,
            reward: Box::new(BaseReward::default()),
            recording,
            seed: 0,
            done: true,
        })
    }

    pub fn with_providers(mut self, providers: Vec<Box<dyn ObsProvider>>) -> Self {
        self.spec = ObsSpec::from_blocks(providers.iter().map(|p| p.spec()));
        self.providers = providers;
        self
    }

    pub fn with_reward(mut self, reward: Box<dyn Reward>) -> Self {
        self.reward = reward;
        self
    }

    pub fn spec(&self) -> &ObsSpec {
        &self.spec
    }
    pub fn course(&self) -> &Course {
        &self.course
    }
    pub fn config(&self) -> &EnvConfig {
        &self.cfg
    }
    pub fn tickrate(&self) -> u32 {
        self.tickrate
    }
    pub fn state(&self) -> &PlayerState {
        &self.state
    }
    pub fn run(&self) -> &CourseRun {
        &self.run
    }
    pub fn view_angles(&self) -> Vec3 {
        self.view_angles
    }
    pub fn track(&self) -> &Track {
        &self.track
    }
    /// The seed passed to the last `reset`. Nothing is randomized yet; the seed is kept so randomized
    /// resets (toy courses, R3) slot in without an API change.
    pub fn seed(&self) -> u64 {
        self.seed
    }
    /// This episode so far as a replayable recording: start state and every command sent.
    pub fn recording(&self) -> &Recording {
        &self.recording
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            course: self.course.clone(),
            mode: self.mode.fork(),
            state: self.state.clone(),
            view_angles: self.view_angles,
            run: self.run,
            track: self.track,
            reward: self.reward.fork(),
            recording: self.recording.clone(),
            seed: self.seed,
            done: self.done,
        }
    }

    /// Restore `snap` and write its observation into `obs`. The snapshot must come from an env on the
    /// same course (same `Arc`) with the same mode and config; anything else is an error and leaves the
    /// env unchanged.
    pub fn restore_into(&mut self, snap: &Snapshot, obs: &mut [f32]) -> Result<(), String> {
        if !Arc::ptr_eq(&snap.course, &self.course) {
            return Err("snapshot is from a different course".into());
        }
        if snap.mode.kind() != self.cfg.mode
            || snap.recording.tickrate != self.tickrate
            || snap.recording.autobhop != self.cfg.autobhop
        {
            return Err("snapshot is from a different mode, tickrate, or autobhop setting".into());
        }
        let s = snap.clone();
        self.mode = s.mode;
        self.state = s.state;
        self.view_angles = s.view_angles;
        self.run = s.run;
        self.track = s.track;
        self.reward = s.reward;
        self.recording = s.recording;
        self.seed = s.seed;
        self.done = s.done;
        self.write_obs(obs);
        Ok(())
    }

    pub fn restore(&mut self, snap: &Snapshot) -> Result<Vec<f32>, String> {
        let mut obs = vec![0.0; self.spec.len()];
        self.restore_into(snap, &mut obs)?;
        Ok(obs)
    }

    pub fn reset(&mut self, seed: u64) -> Vec<f32> {
        let mut obs = vec![0.0; self.spec.len()];
        self.reset_into(seed, &mut obs);
        obs
    }

    /// Reset and write the first observation into `obs` (length `spec().len()`).
    pub fn reset_into(&mut self, seed: u64, obs: &mut [f32]) {
        self.seed = seed;
        self.mode = self.cfg.mode.create();
        self.state = PlayerState::new(self.course.spawn.0);
        self.view_angles = Vec3::new(0.0, self.course.spawn.1, 0.0);
        self.run = CourseRun::new();
        self.track = Track::default();
        self.reward.reset();
        let mut rec = Recording::new(self.cfg.mode, self.tickrate, self.state.clone());
        rec.autobhop = self.cfg.autobhop;
        rec.cmds.reserve(self.cfg.max_episode_ticks.min(1 << 16) as usize);
        self.recording = rec;
        self.done = false;
        self.write_obs(obs);
    }

    pub fn step(&mut self, action: Action) -> Step {
        let mut obs = vec![0.0; self.spec.len()];
        let info = self.step_into(action, &mut obs);
        Step { obs, info }
    }

    /// Run `action_repeat` ticks (fewer if the episode ends) and write the observation into `obs`.
    ///
    /// Panics if the episode has ended or `reset` was never called.
    pub fn step_into(&mut self, action: Action, obs: &mut [f32]) -> StepInfo {
        assert!(!self.done, "step after the episode ended; call reset");
        let mut reward = 0.0;
        let mut truncated = false;
        let mut completed_ticks = None;
        for _ in 0..self.cfg.action_repeat {
            reward += self.tick(&action);
            if self.run.status == Status::Completed && completed_ticks.is_none() {
                completed_ticks = Some(self.run.elapsed_ticks);
            }
            if self.run.status.is_terminal() {
                break;
            }
            if self.track.episode_ticks >= self.cfg.max_episode_ticks {
                truncated = true;
                break;
            }
        }
        let terminated = self.run.status.is_terminal();
        self.done = terminated || truncated;
        self.write_obs(obs);
        StepInfo {
            reward,
            terminated,
            truncated,
            status: self.run.status,
            run_ticks: self.run.elapsed_ticks,
            episode_ticks: self.track.episode_ticks,
            completed_ticks,
            distance_to_goal: self.course.end.target().map(|t| t.sub(self.state.origin).length()),
        }
    }

    fn tick(&mut self, action: &Action) -> f32 {
        let yaw_delta = clamp_turn(action.yaw_delta, self.cfg.max_yaw_per_tick);
        let pitch_delta = clamp_turn(action.pitch_delta, self.cfg.max_pitch_per_tick);
        self.view_angles.y = (self.view_angles.y + yaw_delta).rem_euclid(360.0);
        self.view_angles.x = (self.view_angles.x + pitch_delta).clamp(-89.0, 89.0);

        let cmd = action.to_cmd(self.track.episode_ticks, self.view_angles);
        let prev = self.state.clone();
        process_movement(
            &self.move_cfg,
            self.mode.as_mut(),
            &self.course.world,
            &mut self.state,
            &cmd,
            &mut NullObserver,
            self.dt,
        );
        self.recording.cmds.push(cmd);

        let prev_status = self.run.status;
        self.run.update(&self.course, &self.state);

        let t = &mut self.track;
        t.episode_ticks += 1;
        t.prev_action = Action { yaw_delta, pitch_delta, ..*action };
        t.last_yaw_delta = yaw_delta;
        t.last_pitch_delta = pitch_delta;
        if prev.on_ground() && !self.state.on_ground() {
            t.ticks_since_takeoff = Some(0);
            t.last_takeoff_speed = self.state.horizontal_speed();
        } else if let Some(n) = t.ticks_since_takeoff.as_mut() {
            *n += 1;
        }

        self.reward.reward(&RewardCtx {
            course: &self.course,
            prev: &prev,
            next: &self.state,
            action,
            prev_status,
            status: self.run.status,
        })
    }

    fn write_obs(&self, obs: &mut [f32]) {
        assert_eq!(obs.len(), self.spec.len(), "observation buffer length");
        let ctx = ObsCtx {
            course: &self.course,
            cfg: &self.cfg,
            state: &self.state,
            run: &self.run,
            view_angles: self.view_angles,
            track: &self.track,
        };
        for (p, (b, o)) in self.providers.iter().zip(&self.spec.blocks) {
            p.write(&ctx, &mut obs[*o..*o + b.len()]);
        }
    }
}

/// Clamp a per-tick turn to `max`; a non-finite request turns nothing.
fn clamp_turn(d: f32, max: f32) -> f32 {
    if d.is_finite() {
        d.clamp(-max, max)
    } else {
        0.0
    }
}

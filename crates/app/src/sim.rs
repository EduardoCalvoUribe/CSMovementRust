//! The fixed-tick system that runs the movement crate (plan §6.1). `PlayerState` here is authoritative.

use std::collections::VecDeque;
use std::time::Duration;

use bevy::prelude::*;
use movement::instrument::{BumpEvent, MoveObserver, Tee};
use movement::jumpstats::{JumpReport, JumpTracker};
use movement::pipeline::MoveData;
use movement::{
    process_movement, tick_interval, ModeKind, MovementConfig, MovementMode, PlayerState, PrimitiveWorld,
    TechniqueDetector, TechniqueFlags, UserCmd, Vec3 as SVec3,
};

use crate::input::{build_cmd, HeldInput, ViewAngles};
use crate::record::Recorder;

/// Ticks allowed per rendered frame before excess time is discarded (plan §6.1 catch-up cap).
pub const MAX_TICKS_PER_FRAME: u32 = 4;
/// Ticks of speed history kept for the HUD graph.
pub const SPEED_HISTORY: usize = 256;
/// Commands kept for replay export.
pub const CMD_RING: usize = 64 * 120;

/// Collects the last command's bump traces for the debug view.
#[derive(Default)]
pub struct TraceCollector {
    pub bumps: Vec<BumpEvent>,
}

impl MoveObserver for TraceCollector {
    fn on_cmd_start(&mut self, _s: &PlayerState, _c: &UserCmd) {
        self.bumps.clear();
    }
    fn on_try_player_move(&mut self, ev: &BumpEvent) {
        self.bumps.push(*ev);
    }
}

#[derive(Resource)]
pub struct Sim {
    pub kind: ModeKind,
    pub mode: Box<dyn MovementMode + Send + Sync>,
    pub cfg: MovementConfig,
    pub world: PrimitiveWorld,
    pub state: PlayerState,
    pub prev_state: PlayerState,
    pub tickrate: u32,
    pub tick: u32,
    pub spawn: (SVec3, f32),
    pub checkpoint: Option<(PlayerState, ViewAngles)>,
    pub autohop: bool,
    pub paused: bool,
    pub step_once: bool,
    pub debug_draw: bool,
    pub detector: TechniqueDetector,
    pub traces: TraceCollector,
    pub tracker: JumpTracker,
    pub last_report: Option<JumpReport>,
    pub last_flags: TechniqueFlags,
    pub last_mv: MoveData,
    pub last_cmd: UserCmd,
    pub speed_history: VecDeque<f32>,
    pub cmd_ring: VecDeque<UserCmd>,
    pub discarded_time_events: u32,
    pub recorder: Recorder,
}

impl Sim {
    pub fn new(world: PrimitiveWorld, spawn: (SVec3, f32)) -> Self {
        let kind = ModeKind::Vanilla;
        let mode = kind.create();
        let cfg = *mode.config();
        let state = PlayerState::new(spawn.0);
        Self {
            kind,
            mode,
            cfg,
            world,
            prev_state: state.clone(),
            state,
            tickrate: 64,
            tick: 0,
            spawn,
            checkpoint: None,
            autohop: false,
            paused: false,
            step_once: false,
            debug_draw: false,
            detector: TechniqueDetector::default(),
            traces: TraceCollector::default(),
            tracker: JumpTracker::new(),
            last_report: None,
            last_flags: TechniqueFlags::default(),
            last_mv: MoveData::default(),
            last_cmd: UserCmd::default(),
            speed_history: VecDeque::with_capacity(SPEED_HISTORY),
            cmd_ring: VecDeque::with_capacity(CMD_RING),
            discarded_time_events: 0,
            recorder: Recorder::default(),
        }
    }

    pub fn dt(&self) -> f32 {
        tick_interval(self.tickrate)
    }

    /// Mode changes happen between commands and reset mode-private state (plan §5.6).
    pub fn set_mode(&mut self, kind: ModeKind) {
        self.recorder.stop_if_recording();
        self.kind = kind;
        self.mode = kind.create();
        self.cfg = *self.mode.config();
        if let Some(rate) = self.mode.required_tickrate() {
            self.tickrate = rate;
        }
        self.tracker.reset();
    }

    pub fn set_tickrate(&mut self, rate: u32) {
        if self.mode.required_tickrate().is_some_and(|r| r != rate) {
            return;
        }
        self.recorder.stop_if_recording();
        self.tickrate = rate;
    }

    pub fn teleport(&mut self, origin: SVec3) {
        let mut s = PlayerState::new(origin);
        s.duck_speed = self.cfg.duck_speed_ideal;
        self.state = s;
        self.prev_state = self.state.clone();
        self.mode.reset();
        self.tracker.reset();
    }

    /// Run one command.
    pub fn step(&mut self, cmd: UserCmd) {
        let mut cfg = self.cfg;
        cfg.autobhop = self.autohop;
        let dt = self.dt();
        let before = self.state.clone();
        self.prev_state = before.clone();
        let mut obs = Tee(&mut self.detector, &mut self.traces);
        self.last_mv = process_movement(&cfg, self.mode.as_mut(), &self.world, &mut self.state, &cmd, &mut obs, dt);
        self.last_flags = self.detector.last;
        self.last_cmd = cmd;
        self.tick = self.tick.wrapping_add(1);

        if let Some(r) = self.tracker.tick(&self.world, &before, &self.state, &cmd, self.last_flags, cfg.gravity, dt) {
            self.last_report = Some(r.clone());
        }
        if self.speed_history.len() == SPEED_HISTORY {
            self.speed_history.pop_front();
        }
        self.speed_history.push_back(self.state.horizontal_speed());
        if self.cmd_ring.len() == CMD_RING {
            self.cmd_ring.pop_front();
        }
        self.cmd_ring.push_back(cmd);
        let state = self.state.clone();
        self.recorder.on_tick(&cmd, &state);
    }
}

/// FixedUpdate: build one command from the current input and run it. With a ghost capture loaded, the
/// capture's commands drive the sim instead, looping from its recorded start.
pub fn fixed_tick(
    mut sim: ResMut<Sim>,
    mut angles: ResMut<ViewAngles>,
    mut input: ResMut<HeldInput>,
    ghost: Option<ResMut<crate::ghost::Ghost>>,
) {
    if sim.paused && !sim.step_once {
        return;
    }
    sim.step_once = false;
    if let Some(mut g) = ghost {
        let cmd = match g.next_cmd() {
            Some(c) => c,
            None => {
                let _ = g.reset(&mut sim);
                return;
            }
        };
        *angles = ViewAngles { pitch: cmd.view_angles.x, yaw: cmd.view_angles.y };
        sim.step(cmd);
        input.consume();
        return;
    }
    let cmd = build_cmd(sim.tick, *angles, input.buttons());
    sim.step(cmd);
    input.consume();
}

/// Keep `Time<Fixed>` on the mode's tickrate and cap catch-up ticks per frame.
pub fn sync_timestep(
    sim: Res<Sim>,
    mut fixed: ResMut<Time<Fixed>>,
    mut virt: ResMut<Time<Virtual>>,
) {
    let want = Duration::from_secs_f64(1.0 / sim.tickrate as f64);
    if fixed.timestep() != want {
        fixed.set_timestep(want);
    }
    let cap = want * MAX_TICKS_PER_FRAME;
    if virt.max_delta() != cap {
        virt.set_max_delta(cap);
    }
}

/// Log when a frame stall made Bevy discard simulation time. During a recording that run is no longer
/// comparable to a headless replay of real time, so it is flagged on the HUD.
pub fn detect_discarded_time(mut sim: ResMut<Sim>, real: Res<Time<Real>>, virt: Res<Time<Virtual>>) {
    if real.delta() <= virt.max_delta() || sim.tick == 0 {
        return;
    }
    let lost = real.delta() - virt.max_delta();
    if sim.recorder.is_recording() {
        sim.discarded_time_events += 1;
        warn!("frame stall: capped at {MAX_TICKS_PER_FRAME} ticks, discarded {lost:?} of sim time during a recording");
    } else {
        info!("frame stall: capped at {MAX_TICKS_PER_FRAME} ticks, discarded {lost:?} of sim time");
    }
}

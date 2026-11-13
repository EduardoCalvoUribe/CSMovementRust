//! Per-episode timer and termination state.
//!
//! Call [`CourseRun::update`] once after every `process_movement` with the command and the resulting
//! state. Time is counted in ticks (seconds = ticks / tickrate), so a completion time is an exact integer.
//!
//! Rules:
//! - Zone start: the timer starts on the first command whose resulting state is outside the start zone;
//!   that command counts as tick 1, since the movement it performed is part of the run. Returning to the
//!   start zone while running resets the timer (and waypoint progress) to idle.
//! - Button start (GOKZ, measured on the real server; docs/use-rules.md): a `+use` press of the start
//!   button starts the run at tick 0 if the player is on the ground and landed more than
//!   `KzRules::land_ticks` commands ago (both judged before the press command's movement). The press
//!   leaves a *virtual start button* at the origin the player pressed from: after every later command,
//!   while the origin is within `KzRules::virtual_range` of it (in line of sight), on the ground, settled
//!   after landing, and within `KzRules::use_window_ticks` of the last `+use` press, the run restarts at
//!   tick 0 on that command. So in practice a run starts when the player leaves that radius.
//! - Each later command adds one tick, and the command that finishes the run is included: a run started
//!   (or restarted) on command `s` and finished by a press on command `e` takes `e - s` ticks, which is
//!   GOKZ's time.
//! - Checks run in this order, so the safest outcome wins a tie: out of bounds, hazard, end.
//! - The end only counts while the timer is running, so a run cannot finish without leaving the start.
//! - Waypoints are reached in order while running; reaching them never ends the run.
//! - Once a run is terminal it stays terminal and `elapsed_ticks` stops changing.
//!
//! `+use` presses (`use_hits`) are judged from the state *before* the command's movement, as the engine
//! does; `CourseRun` keeps the previous command's resulting state for that. Start it with
//! [`CourseRun::starting_at`] so the first command has it too.

use movement::math::angle_vectors;
use movement::{Buttons, EntityId, Hull, PlayerState, TraceWorld, UserCmd, Vec3};

use crate::course::{eye, Course, EndSpec, StartSpec, USE_RANGE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// In the start zone, timer not running.
    Idle,
    Running,
    Completed,
    Hazard,
    OutOfBounds,
}

impl Status {
    pub fn is_terminal(self) -> bool {
        matches!(self, Status::Completed | Status::Hazard | Status::OutOfBounds)
    }
}

/// GOKZ timer rules for button starts (gokz-core 3.6.4; docs/use-rules.md). The defaults are SimpleKZ at
/// 128 tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KzRules {
    /// Virtual start button radius, origin to origin (GOKZ `gF_ModeVirtualButtonRanges`: vanilla 0,
    /// SimpleKZ 32, KZTimer 70). 0 disables virtual restarts.
    pub virtual_range: f32,
    /// A `+use` press keeps virtual restarts going for this many commands
    /// (`GOKZ_VIRTUAL_BUTTON_USE_DETECTION_TIME`, 2 s).
    pub use_window_ticks: u32,
    /// Starting is refused until more than this many commands after landing
    /// (`GOKZ_TIMER_START_GROUND_TICKS`).
    pub land_ticks: u32,
}

impl Default for KzRules {
    fn default() -> Self {
        Self::for_mode(movement::ModeKind::SimpleKz, 128)
    }
}

impl KzRules {
    pub fn for_mode(mode: movement::ModeKind, tickrate: u32) -> Self {
        let virtual_range = match mode {
            movement::ModeKind::Vanilla => 0.0,
            movement::ModeKind::SimpleKz => 32.0,
            movement::ModeKind::KzTimer => 70.0,
        };
        Self { virtual_range, use_window_ticks: 2 * tickrate, land_ticks: 4 }
    }
}

/// What `+use` and the GOKZ rules need from the state before a command.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Before {
    state_origin: Vec3,
    eye: Vec3,
    on_ground: bool,
}

impl Before {
    fn of(state: &PlayerState) -> Self {
        Self { state_origin: state.origin, eye: eye(state), on_ground: state.on_ground() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CourseRun {
    pub status: Status,
    pub elapsed_ticks: u32,
    /// Index of the next waypoint to reach (= waypoints reached so far).
    pub next_waypoint: usize,
    /// `+use` was held on the previous command, so holding it does not press again.
    pub use_held: bool,
    pub rules: KzRules,
    /// Commands seen so far (the index of the next command).
    commands: u32,
    before: Option<Before>,
    /// The command whose movement last landed the player.
    landed_on: Option<u32>,
    /// The last command with a fresh `+use` press.
    last_use: Option<u32>,
    /// Where the player pressed the start button from (GOKZ's virtual start button).
    virtual_start: Option<Vec3>,
    /// The last command a map teleport fired in.
    teleported_on: Option<u32>,
}

impl Default for CourseRun {
    fn default() -> Self {
        Self::new()
    }
}

impl CourseRun {
    /// A run whose first command's "before" state is unknown: that command's `+use` is judged from the
    /// state after it. Prefer [`CourseRun::starting_at`].
    pub fn new() -> Self {
        Self {
            status: Status::Idle,
            elapsed_ticks: 0,
            next_waypoint: 0,
            use_held: false,
            rules: KzRules::default(),
            commands: 0,
            before: None,
            landed_on: None,
            last_use: None,
            virtual_start: None,
            teleported_on: None,
        }
    }

    /// A run whose first command starts from `state`.
    pub fn starting_at(state: &PlayerState) -> Self {
        Self { before: Some(Before::of(state)), ..Self::new() }
    }

    pub fn with_rules(mut self, rules: KzRules) -> Self {
        self.rules = rules;
        self
    }

    /// A map teleport fired in the command about to be passed to `update` (`Course::touch_teleports`).
    pub fn teleported(&mut self) {
        self.teleported_on = Some(self.commands);
    }

    /// GOKZ `TimerStart`'s ground checks for command `k`: on the ground, settled after landing, and not
    /// just teleported (`JustTeleported`: within `land_ticks` commands of a teleport, judged like landing).
    fn can_start(&self, on_ground: bool, k: u32) -> bool {
        on_ground
            && self.landed_on.is_none_or(|l| k - l > self.rules.land_ticks)
            && self.teleported_on.is_none_or(|t| k - t > self.rules.land_ticks)
    }

    fn restart(&mut self) {
        self.status = Status::Running;
        self.elapsed_ticks = 0;
        self.next_waypoint = 0;
    }

    /// Everything the course does after a command's movement, in the engine's order: map teleports
    /// (`Course::touch_teleports`, which may move `state`), then [`CourseRun::update`]. Returns the view
    /// angles a teleport set, for the caller's view (the next command's angles).
    pub fn after_command(&mut self, course: &Course, state: &mut PlayerState, cmd: &UserCmd) -> Option<Vec3> {
        let mut view = None;
        if let Some(t) = course.touch_teleports(state) {
            self.teleported();
            view = t.angles;
        }
        self.update(course, state, cmd);
        view
    }

    /// Advance by one command. `state` is the player state after `cmd` ran (and after any teleport:
    /// prefer [`CourseRun::after_command`]).
    pub fn update(&mut self, course: &Course, state: &PlayerState, cmd: &UserCmd) -> Status {
        let k = self.commands;
        let before = self.before.unwrap_or_else(|| Before::of(state));
        let use_down = cmd.buttons.contains(Buttons::USE);
        let use_pressed = use_down && !self.use_held;
        self.use_held = use_down;
        self.commands += 1;
        self.before = Some(Before::of(state));
        if !before.on_ground && state.on_ground() {
            self.landed_on = Some(k);
        }
        if use_pressed {
            self.last_use = Some(k);
        }
        if self.status.is_terminal() {
            return self.status;
        }
        let hull = state.hull();

        if state.origin.z < course.kill_z {
            self.status = Status::OutOfBounds;
            return self.status;
        }
        if course.hazards.iter().any(|z| z.overlaps(state.origin, hull)) {
            self.status = Status::Hazard;
            return self.status;
        }

        let pressed = if use_pressed { use_target(course, before.eye, cmd.view_angles) } else { None };
        let mut started_now = false;
        match &course.start {
            StartSpec::Zone(zone) => {
                let in_start = zone.overlaps(state.origin, hull);
                match self.status {
                    Status::Idle => {
                        if !in_start {
                            self.status = Status::Running;
                            self.elapsed_ticks = 1;
                        }
                    }
                    Status::Running => {
                        if in_start {
                            self.status = Status::Idle;
                            self.elapsed_ticks = 0;
                            self.next_waypoint = 0;
                        } else {
                            self.elapsed_ticks += 1;
                        }
                    }
                    _ => unreachable!("terminal states return above"),
                }
            }
            StartSpec::Button { entity } => {
                if pressed == Some(*entity) && self.can_start(before.on_ground, k) {
                    self.restart();
                    self.virtual_start = Some(before.state_origin);
                    started_now = true;
                } else if self.status == Status::Running {
                    self.elapsed_ticks += 1;
                }
            }
        }

        if self.status == Status::Running && !started_now {
            while course.waypoints.get(self.next_waypoint).is_some_and(|w| w.reached_by(state.origin)) {
                self.next_waypoint += 1;
            }
            let done = match &course.end {
                EndSpec::Touch(zone) => zone.overlaps(state.origin, hull),
                EndSpec::Use { entity, .. } => pressed == Some(*entity),
            };
            if done {
                self.status = Status::Completed;
                return self.status;
            }
            // GOKZ's virtual start button, checked after the command (its post-think).
            if let Some(v) = self.virtual_start {
                let r = self.rules;
                if r.virtual_range > 0.0
                    && self.last_use.is_some_and(|u| k - u <= r.use_window_ticks)
                    && state.origin.sub(v).length() <= r.virtual_range
                    && self.can_start(state.on_ground(), k)
                    && clear_line(course, state.origin, v)
                {
                    self.restart();
                }
            }
        }
        self.status
    }
}

/// A point trace from `a` to `b` hits nothing.
fn clear_line(course: &Course, a: Vec3, b: Vec3) -> bool {
    let tr = course.world.trace_hull(a, b, Hull::new(Vec3::ZERO, Vec3::ZERO));
    !tr.start_solid && tr.fraction >= 1.0
}

/// The button a `+use` from `eye` with `view_angles` presses, measured on CS:GO 1.38.8.1
/// (docs/use-rules.md):
/// - in reach when the largest per-axis gap from the eye to the button's bounding box is below
///   `USE_RANGE` (a box, not a sphere);
/// - in any view direction (KZ buttons have `min_use_angle -1`; other values are not modelled);
/// - only in line of sight: a trace from the eye to the box's nearest point must reach the button;
/// - of several, the one whose nearest point is closest to the view line (Source's choice; not measured).
pub fn use_target(course: &Course, eye: Vec3, view_angles: Vec3) -> Option<EntityId> {
    let forward = angle_vectors(view_angles).forward;
    let mut best: Option<(f32, EntityId)> = None;
    for e in course.buttons() {
        let Some((lo, hi)) = course.entity_bounds(e) else { continue };
        let near = Vec3::new(eye.x.clamp(lo.x, hi.x), eye.y.clamp(lo.y, hi.y), eye.z.clamp(lo.z, hi.z));
        let gap = near.sub(eye);
        if gap.x.abs().max(gap.y.abs()).max(gap.z.abs()) >= USE_RANGE {
            continue;
        }
        if near != eye {
            let tr = course.world.trace_hull(eye, near, Hull::new(Vec3::ZERO, Vec3::ZERO));
            if tr.start_solid || (tr.fraction < 1.0 && tr.hit_entity != Some(e)) {
                continue;
            }
        }
        // Distance from the nearest point to the view line.
        let along = gap.dot(forward);
        let off = gap.sub(forward.scale(along)).length();
        if best.is_none_or(|(d, _)| off < d) {
            best = Some((off, e));
        }
    }
    best.map(|(_, e)| e)
}

/// The button a `+use` pressed in `state` (the state before the command's movement) reaches.
pub fn use_hits(course: &Course, state: &PlayerState, view_angles: Vec3) -> Option<EntityId> {
    use_target(course, eye(state), view_angles)
}

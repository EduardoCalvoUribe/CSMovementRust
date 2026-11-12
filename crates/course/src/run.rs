//! Per-episode timer and termination state.
//!
//! Call [`CourseRun::update`] once after every `process_movement` with the command and the resulting
//! state. Time is counted in ticks (seconds = ticks / tickrate), so a completion time is an exact integer.
//!
//! Rules:
//! - Zone start: the timer starts on the first command whose resulting state is outside the start zone;
//!   that command counts as tick 1, since the movement it performed is part of the run. Returning to the
//!   start zone while running resets the timer (and waypoint progress) to idle.
//! - Button start: a fresh `+use` press on the start button starts (or restarts) the run at tick 0.
//! - Each later command adds one tick. The command that finishes the run is included.
//! - Checks run in this order, so the safest outcome wins a tie: out of bounds, hazard, end.
//! - The end only counts while the timer is running, so a run cannot finish without leaving the start.
//! - Waypoints are reached in order while running; reaching them never ends the run.
//! - Once a run is terminal it stays terminal and `elapsed_ticks` stops changing.

use movement::math::angle_vectors;
use movement::{Buttons, Hull, PlayerState, TraceWorld, UserCmd, Vec3};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CourseRun {
    pub status: Status,
    pub elapsed_ticks: u32,
    /// Index of the next waypoint to reach (= waypoints reached so far).
    pub next_waypoint: usize,
    /// `+use` was held on the previous command, so holding it does not press again.
    pub use_held: bool,
}

impl Default for CourseRun {
    fn default() -> Self {
        Self::new()
    }
}

impl CourseRun {
    pub fn new() -> Self {
        Self { status: Status::Idle, elapsed_ticks: 0, next_waypoint: 0, use_held: false }
    }

    /// Advance by one command. `state` is the player state after `cmd` ran.
    pub fn update(&mut self, course: &Course, state: &PlayerState, cmd: &UserCmd) -> Status {
        let use_down = cmd.buttons.contains(Buttons::USE);
        let use_pressed = use_down && !self.use_held;
        self.use_held = use_down;
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
                // The press command is tick 0; each later command adds one, so the run time is the number
                // of commands from the start press to the end press.
                if use_pressed && use_hits(course, state, cmd.view_angles) == Some(*entity) {
                    self.status = Status::Running;
                    self.elapsed_ticks = 0;
                    self.next_waypoint = 0;
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
                EndSpec::Use { entity, .. } => use_pressed && use_hits(course, state, cmd.view_angles) == Some(*entity),
            };
            if done {
                self.status = Status::Completed;
            }
        }
        self.status
    }
}

/// The entity a `+use` from `state` with `view_angles` would reach: a straight zero-size trace from the
/// eye, `USE_RANGE` long.
pub fn use_hits(course: &Course, state: &PlayerState, view_angles: Vec3) -> Option<movement::EntityId> {
    let from = eye(state);
    let to = from.ma(USE_RANGE, angle_vectors(view_angles).forward);
    let tr = course.world.trace_hull(from, to, Hull::new(Vec3::ZERO, Vec3::ZERO));
    if tr.start_solid || tr.fraction >= 1.0 {
        None
    } else {
        tr.hit_entity
    }
}

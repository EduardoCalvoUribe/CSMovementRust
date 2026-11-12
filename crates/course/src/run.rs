//! Per-episode timer and termination state.
//!
//! Call [`CourseRun::update`] once after every `process_movement` with the resulting state. Time is
//! counted in ticks (seconds = ticks / tickrate), so a completion time is an exact integer.
//!
//! Rules:
//! - The timer starts on the first command whose resulting state is outside the start zone; that command
//!   counts as tick 1, since the movement it performed is part of the run.
//! - Returning to the start zone while running resets the timer to idle.
//! - Each later command adds one tick. The command that finishes the run is included.
//! - Checks run in this order, so the safest outcome wins a tie: out of bounds, hazard, end.
//! - The end only counts while the timer is running, so a run cannot finish without leaving the start.
//! - Once a run is terminal it stays terminal and `elapsed_ticks` stops changing.

use movement::PlayerState;

use crate::course::{Course, EndSpec};

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
}

impl Default for CourseRun {
    fn default() -> Self {
        Self::new()
    }
}

impl CourseRun {
    pub fn new() -> Self {
        Self { status: Status::Idle, elapsed_ticks: 0 }
    }

    /// Advance by one command. `state` is the player state after that command.
    pub fn update(&mut self, course: &Course, state: &PlayerState) -> Status {
        if self.status.is_terminal() {
            return self.status;
        }
        let hull = state.hull();
        let in_start = course.start_zone.overlaps(state.origin, hull);

        if state.origin.z < course.kill_z {
            self.status = Status::OutOfBounds;
            return self.status;
        }
        if course.hazards.iter().any(|z| z.overlaps(state.origin, hull)) {
            self.status = Status::Hazard;
            return self.status;
        }

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
                } else {
                    self.elapsed_ticks += 1;
                }
            }
            _ => unreachable!("terminal states return above"),
        }

        if self.status == Status::Running {
            let EndSpec::Touch(zone) = &course.end;
            if zone.overlaps(state.origin, hull) {
                self.status = Status::Completed;
            }
        }
        self.status
    }
}

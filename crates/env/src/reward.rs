//! Pluggable rewards (plan §6). A reward sees one tick: the state before and after, the action, and the
//! run status before and after. `StepInfo` always reports the raw quantities, whatever reward is used.

use course::{Course, Status};
use movement::PlayerState;

use crate::action::Action;

pub struct RewardCtx<'a> {
    pub course: &'a Course,
    pub prev: &'a PlayerState,
    pub next: &'a PlayerState,
    pub action: &'a Action,
    pub prev_status: Status,
    pub status: Status,
}

pub trait Reward: Send {
    /// Reward for one tick.
    fn reward(&mut self, ctx: &RewardCtx) -> f32;
    /// Called from `Env::reset`.
    fn reset(&mut self) {}
    /// An independent copy including any internal state, for `Env::snapshot`.
    fn fork(&self) -> Box<dyn Reward>;
}

/// Plan §6 base reward: a bonus on completion, a cost per tick, and penalties by failure reason. The
/// default magnitudes are placeholders for the learner to tune; nothing in the env depends on them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BaseReward {
    pub completion_bonus: f32,
    /// Charged every tick of the episode, including ticks spent idle in the start zone, so waiting is never
    /// free.
    pub tick_cost: f32,
    pub hazard_penalty: f32,
    pub out_of_bounds_penalty: f32,
}

impl Default for BaseReward {
    fn default() -> Self {
        Self { completion_bonus: 10.0, tick_cost: 1.0 / 128.0, hazard_penalty: 0.0, out_of_bounds_penalty: 0.0 }
    }
}

impl Reward for BaseReward {
    fn reward(&mut self, ctx: &RewardCtx) -> f32 {
        let mut r = -self.tick_cost;
        if ctx.status != ctx.prev_status {
            r += match ctx.status {
                Status::Completed => self.completion_bonus,
                Status::Hazard => -self.hazard_penalty,
                Status::OutOfBounds => -self.out_of_bounds_penalty,
                Status::Idle | Status::Running => 0.0,
            };
        }
        r
    }

    fn fork(&self) -> Box<dyn Reward> {
        Box::new(*self)
    }
}

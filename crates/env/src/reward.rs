//! Pluggable rewards (plan §6). A reward sees one tick: the state before and after, the action, and the
//! run before and after. `StepInfo` always reports the raw quantities, whatever reward is used. Which
//! shaping helps is an experiment; these are the building blocks, combined with [`SumReward`].

use course::{Course, CourseRun, Status};
use movement::PlayerState;

use crate::action::Action;

pub struct RewardCtx<'a> {
    pub course: &'a Course,
    pub prev: &'a PlayerState,
    pub next: &'a PlayerState,
    pub action: &'a Action,
    pub prev_status: Status,
    pub status: Status,
    pub prev_run: &'a CourseRun,
    pub run: &'a CourseRun,
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

/// Potential-based progress shaping: `scale * (gamma * phi(next) - phi(prev))` with
/// `phi = -Course::remaining_route(origin, next_waypoint)`. With `gamma = 1` the sum over an episode
/// telescopes to `scale * (phi(last) - phi(first))`: route progress, whatever path was taken. Set `gamma`
/// to the learner's discount to keep the optimal policy unchanged (Ng et al. 1999). A run reset by
/// returning to the start zone resets waypoint progress, which this charges as lost progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProgressShaping {
    pub scale: f32,
    pub gamma: f32,
}

impl Default for ProgressShaping {
    /// Placeholder scale: 1 per 100 units of route.
    fn default() -> Self {
        Self { scale: 0.01, gamma: 1.0 }
    }
}

impl ProgressShaping {
    pub fn potential(course: &Course, state: &PlayerState, run: &CourseRun) -> f32 {
        -course.remaining_route(state.origin, run.next_waypoint)
    }
}

impl Reward for ProgressShaping {
    fn reward(&mut self, ctx: &RewardCtx) -> f32 {
        let before = Self::potential(ctx.course, ctx.prev, ctx.prev_run);
        let after = Self::potential(ctx.course, ctx.next, ctx.run);
        self.scale * (self.gamma * after - before)
    }

    fn fork(&self) -> Box<dyn Reward> {
        Box::new(*self)
    }
}

/// A fixed bonus each time the next waypoint is reached.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaypointBonus {
    pub bonus: f32,
}

impl Reward for WaypointBonus {
    fn reward(&mut self, ctx: &RewardCtx) -> f32 {
        let reached = ctx.run.next_waypoint.saturating_sub(ctx.prev_run.next_waypoint);
        self.bonus * reached as f32
    }

    fn fork(&self) -> Box<dyn Reward> {
        Box::new(*self)
    }
}

/// The sum of several rewards, evaluated in order.
pub struct SumReward(pub Vec<Box<dyn Reward>>);

impl Reward for SumReward {
    fn reward(&mut self, ctx: &RewardCtx) -> f32 {
        self.0.iter_mut().map(|r| r.reward(ctx)).sum()
    }
    fn reset(&mut self) {
        self.0.iter_mut().for_each(|r| r.reset());
    }
    fn fork(&self) -> Box<dyn Reward> {
        Box::new(SumReward(self.0.iter().map(|r| r.fork()).collect()))
    }
}

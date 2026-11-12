//! Reinforcement-learning environment over the movement sim (`docs/reinforcement-plan.md` §4-§6).
//! Pure Rust on top of `movement` and `course`: no Bevy, no I/O, no hidden randomness.
//!
//! The learning algorithm and network are deliberately not chosen here. Observations and rewards are
//! pluggable traits so whichever approach the research settles on can pick or combine them.

pub mod action;
pub mod config;
pub mod env;
pub mod obs;
pub mod rays;
pub mod reward;
pub mod vec;

pub use action::Action;
pub use config::EnvConfig;
pub use env::{Env, Snapshot, Step, StepInfo, Track};
pub use obs::{BlockSpec, GoalVector, ObsCtx, ObsProvider, ObsSpec, Proprioception};
pub use rays::{ClassEncoding, RayGrid, RayHit, SurfaceClass};
pub use reward::{BaseReward, ProgressShaping, Reward, RewardCtx, SumReward, WaypointBonus};
pub use vec::{episode_seed, VecEnv};

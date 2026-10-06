//! Engine-agnostic CS:GO (Source 1) movement. No Bevy dependency, no I/O, time, or randomness.

// Named Vec3 methods keep every operation explicit; `-1.0 *` mirrors Source's AngleVectors; reference
// constants keep the digits the reference quotes.
#![allow(clippy::should_implement_trait, clippy::neg_multiply, clippy::excessive_precision)]

pub mod categorize;
pub mod cmd;
pub mod collide;
pub mod config;
pub mod duck;
pub mod instrument;
pub mod jump;
pub mod jumpstats;
pub mod ladder;
pub mod math;
pub mod modes;
pub mod physics;
pub mod pipeline;
pub mod replay;
pub mod script;
pub mod stamina;
pub mod state;
pub mod trace;
pub mod world;

pub use cmd::{Buttons, UserCmd};
pub use config::MovementConfig;
pub use instrument::{MoveObserver, NullObserver, TechniqueDetector, TechniqueFlags};
pub use math::Vec3;
pub use modes::{ModeKind, MovementMode};
pub use pipeline::{process_movement, MoveData};
pub use state::{MoveType, PlayerState};
pub use trace::{EntityId, Hull, TraceResult, TraceWorld};
pub use world::PrimitiveWorld;

/// Tick interval for a tickrate [Ref §2].
pub fn tick_interval(tickrate: u32) -> f32 {
    1.0 / tickrate as f32
}

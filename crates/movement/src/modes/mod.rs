//! Movement modes: config plus hooks over one core [Ref §20, plan §5.6].

use crate::cmd::UserCmd;
use crate::config::MovementConfig;
use crate::state::PlayerState;

pub mod kztimer;
pub mod simplekz;
pub mod vanilla;

pub use kztimer::KzTimer;
pub use simplekz::SimpleKz;
pub use vanilla::Vanilla;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ModeKind {
    Vanilla,
    KzTimer,
    SimpleKz,
}

impl ModeKind {
    pub fn create(self) -> Box<dyn MovementMode + Send + Sync> {
        match self {
            ModeKind::Vanilla => Box::new(Vanilla::new()),
            ModeKind::KzTimer => Box::new(KzTimer::new()),
            ModeKind::SimpleKz => Box::new(SimpleKz::new()),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ModeKind::Vanilla => "Vanilla",
            ModeKind::KzTimer => "KZTimer",
            ModeKind::SimpleKz => "SimpleKZ",
        }
    }
}

pub trait MovementMode {
    fn kind(&self) -> ModeKind;
    fn config(&self) -> &MovementConfig;
    /// The only tickrate this mode allows, if it locks one (SimpleKZ: 128) [Ref §20.2].
    fn required_tickrate(&self) -> Option<u32> {
        None
    }
    /// Called before the command runs. May edit the command (KZTimer jump+duck suppression).
    fn pre_command(&mut self, _state: &mut PlayerState, _cmd: &mut UserCmd, _dt: f32) {}
    fn post_command(&mut self, _state: &mut PlayerState, _cmd: &UserCmd) {}
    /// Called inside a successful jump, after the impulse and before the move. Perf detection and
    /// takeoff adjustment. `ground_z` is the height of the support under the takeoff, if any.
    fn on_jump(&mut self, _state: &mut PlayerState, _ground_speed: f32, _ground_z: Option<f32>) {}
    /// Called from landing processing.
    fn on_land(&mut self, _state: &mut PlayerState) {}
    /// Adjust the move-data maximum speed (prestrafe).
    fn modify_wish_speed(&self, _state: &PlayerState, base: f32) -> f32 {
        base
    }
    /// Drop mode-private state (mode switch, reset to spawn).
    fn reset(&mut self) {}
    /// Current prestrafe multiplier, for the HUD.
    fn prestrafe_multiplier(&self) -> f32 {
        1.0
    }
}

/// Counts consecutive grounded commands, used for perfect-hop recognition by both KZ modes.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GroundTicks {
    pub ticks: u32,
}

impl GroundTicks {
    pub fn update(&mut self, state: &PlayerState) {
        if state.on_ground() {
            self.ticks = self.ticks.saturating_add(1);
        } else {
            self.ticks = 0;
        }
    }
}

/// Signed smallest yaw change between two angles, in degrees.
pub(crate) fn yaw_delta(prev: f32, cur: f32) -> f32 {
    let mut d = cur - prev;
    while d > 180.0 {
        d -= 360.0;
    }
    while d < -180.0 {
        d += 360.0;
    }
    d
}

/// Scale horizontal velocity down to `cap`, leaving vertical velocity alone.
pub(crate) fn cap_horizontal(state: &mut PlayerState, cap: f32) {
    let speed = state.velocity.length_2d();
    if speed > cap && speed > 0.0 {
        let k = cap / speed;
        state.velocity.x *= k;
        state.velocity.y *= k;
    }
}

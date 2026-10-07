//! Movement modes: config plus hooks over one core [Ref §20, plan §5.6].

use crate::cmd::UserCmd;
use crate::config::MovementConfig;
use crate::math::Vec3;
use crate::pipeline::MoveData;
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
    /// Called before the command runs (GOKZ's `OnPlayerRunCmd`). May edit the command and the state.
    fn pre_command(&mut self, _state: &mut PlayerState, _cmd: &mut UserCmd, _dt: f32) {}
    /// Called after the command (MovementAPI's post-think bookkeeping). `mv` holds this command's move
    /// data, including where the walk and air moves happened.
    fn post_command(&mut self, _state: &mut PlayerState, _cmd: &UserCmd, _mv: &MoveData) {}
    /// The player's maximum speed (`GetPlayerMaxSpeed`); the move data starts from it before the crops.
    fn player_max_speed(&self, base: f32) -> f32 {
        base
    }
    /// The wish speed `AirAccelerate` receives.
    fn air_wish_speed(&self, wish_speed: f32) -> f32 {
        wish_speed
    }
    /// Override `CanUnduck`: `Some(false)` keeps the player ducked.
    fn can_unduck(&self, _state: &PlayerState) -> Option<bool> {
        None
    }
    /// Called inside a successful jump, after the impulse and before the move. Perf detection and
    /// takeoff adjustment. `ground_z` is the height of the support under the takeoff, if any.
    fn on_jump(&mut self, _state: &mut PlayerState, _ground_speed: f32, _ground_z: Option<f32>) {}
    /// Called after every `CategorizePosition`, with whether the player was grounded before it and the
    /// normal of the ground found (MovementAPI's categorize-position hook).
    fn on_categorize(&mut self, _state: &mut PlayerState, _mv: &MoveData, _was_on_ground: bool, _ground_normal: Option<Vec3>) {}
    /// Called from landing processing.
    fn on_land(&mut self, _state: &mut PlayerState) {}
    /// Drop mode-private state (mode switch, reset to spawn).
    fn reset(&mut self) {}
    /// Current prestrafe multiplier, for the HUD.
    fn prestrafe_multiplier(&self) -> f32 {
        1.0
    }
}

/// SourceMod's `SetVectorHorizontalLength` as GOKZ uses it: normalize the horizontal part (Source's
/// `VectorNormalize`, with its `FLT_EPSILON`), scale it to `length`, keep the vertical part.
pub(crate) fn set_horizontal_length(v: Vec3, length: f32) -> Vec3 {
    let mut h = Vec3::new(v.x, v.y, 0.0);
    h.normalize_in_place();
    Vec3::new(h.x * length, h.y * length, v.z)
}

/// `GetVectorHorizontalLength`.
pub(crate) fn horizontal_length(v: Vec3) -> f32 {
    (v.x * v.x + v.y * v.y).sqrt()
}

/// GOKZ's `CalcDeltaAngle`: `b - a` wrapped into (-180, 180].
pub(crate) fn calc_delta_angle(a: f32, b: f32) -> f32 {
    let d = b - a;
    if d > 180.0 {
        d - 360.0
    } else if d <= -180.0 {
        d + 360.0
    } else {
        d
    }
}

/// MovementAPI's turning flags, from the eye yaw of two consecutive commands.
pub(crate) fn turning(old_yaw: f32, yaw: f32) -> (bool, bool) {
    let turning = yaw != old_yaw;
    let left = yaw < old_yaw - 180.0 || (yaw > old_yaw && yaw < old_yaw + 180.0);
    (turning, left)
}

/// GOKZ's `SlopeFix` (by Mev and Blacky, modified by DanZay; GPL-3.0), run by both KZ modes on landing:
/// on a walkable slope that isn't flat, the landing velocity clipped to the slope replaces the velocity
/// when that is faster. Returns the new landing velocity when it applies. GOKZ traces the ducked hull
/// straight down for the slope; this uses the plane categorization grounded the player on.
pub(crate) fn slope_fix(state: &mut PlayerState, mv: &MoveData, normal: Option<Vec3>) -> Option<Vec3> {
    let n = normal?;
    if !(0.7 <= n.z && n.z < 1.0) {
        return None;
    }
    let last = if mv.air_accelerated { mv.post_aa_velocity } else { state.velocity };
    let back_off = last.dot(n);
    let mut v = Vec3::new(last.x - n.x * back_off, last.y - n.y * back_off, 0.0);
    let adjust = v.dot(n);
    if adjust < 0.0 {
        v.x -= n.x * adjust;
        v.y -= n.y * adjust;
    }
    let last_h = Vec3::new(last.x, last.y, 0.0);
    if v.length() > last_h.length() {
        state.velocity = v;
        Some(v)
    } else {
        None
    }
}

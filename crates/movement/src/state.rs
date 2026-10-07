//! Player state that persists across commands [Ref §1].

use crate::cmd::Buttons;
use crate::math::Vec3;
use crate::trace::{EntityId, Hull};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MoveType {
    #[default]
    Walk,
    Ladder,
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlayerState {
    pub tick: u32,
    pub origin: Vec3,
    pub velocity: Vec3,
    pub base_velocity: Vec3,
    pub ground_entity: Option<EntityId>,
    pub move_type: MoveType,
    /// Buttons of the previous command, as edited by the jump logic.
    pub old_buttons: Buttons,
    /// Stored surface friction: written by categorization, read by the next acceleration [Ref §7].
    pub surface_friction: f32,
    /// Stamina penalty accumulator [Ref §8].
    pub stamina: f32,
    /// 0 = standing, 1 = fully crouched; eye/animation fraction [Ref §10.3].
    pub duck_amount: f32,
    /// Rate at which duck_amount moves; reduced by duck spam [Ref §10.3].
    pub duck_speed: f32,
    /// Origin at the start of the last command that ended with full duck speed; moving far from it
    /// speeds up duck speed recovery (docs/divergences.md D14).
    pub duck_speed_anchor: Vec3,
    /// The duck hull is in use (`FL_DUCKING` / `m_bDucked`).
    pub ducked: bool,
    /// A duck or unduck transition is in progress (`m_bDucking`).
    pub ducking: bool,
    /// Stored fall velocity, consumed by landing processing [Ref §1, §8].
    pub fall_velocity: f32,
    pub ladder_normal: Vec3,
    /// Seconds remaining during which jump cannot detach from a newly entered ladder [Ref §18].
    pub ladder_jump_ignore: f32,
}

impl PlayerState {
    pub fn new(origin: Vec3) -> Self {
        Self {
            tick: 0,
            origin,
            velocity: Vec3::ZERO,
            base_velocity: Vec3::ZERO,
            ground_entity: None,
            move_type: MoveType::Walk,
            old_buttons: Buttons::NONE,
            surface_friction: 1.0,
            stamina: 0.0,
            duck_amount: 0.0,
            duck_speed: 8.0,
            duck_speed_anchor: origin,
            ducked: false,
            ducking: false,
            fall_velocity: 0.0,
            ladder_normal: Vec3::ZERO,
            ladder_jump_ignore: 0.0,
        }
    }

    pub fn on_ground(&self) -> bool {
        self.ground_entity.is_some()
    }

    pub fn hull(&self) -> Hull {
        if self.ducked {
            Hull::DUCK
        } else {
            Hull::STAND
        }
    }

    pub fn horizontal_speed(&self) -> f32 {
        self.velocity.length_2d()
    }
}

//! GOKZ KZTimer mode [Ref §20, §20.1]. Hook algorithms are our own designs that hit the reference's
//! numbers (prestrafe max 1.104, perf cap 380); see `docs/modes-notes.md`.

use super::{cap_horizontal, yaw_delta, GroundTicks, ModeKind, MovementMode};
use crate::cmd::{Buttons, UserCmd};
use crate::config::MovementConfig;
use crate::state::PlayerState;

/// Prestrafe modifier ceiling: 276 from 250 [Ref §20.1].
pub const PRESTRAFE_MAX: f32 = 1.104;
/// Perfect-hop horizontal speed cap [Ref §20.1].
pub const PERF_SPEED_CAP: f32 = 380.0;
/// Modifier change per second while building or losing prestrafe (unverified).
pub const PRESTRAFE_RATE: f32 = 0.208;

pub struct KzTimer {
    cfg: MovementConfig,
    modifier: f32,
    last_yaw: Option<f32>,
    ground: GroundTicks,
}

impl KzTimer {
    pub fn new() -> Self {
        let mut cfg = MovementConfig::vanilla();
        cfg.accelerate = 6.5;
        cfg.air_accelerate = 100.0;
        cfg.friction = 5.0;
        cfg.weapon_speed_scaling = false;
        cfg.stamina_jump_cost = 0.0;
        cfg.stamina_land_cost = 0.0;
        cfg.enable_bunnyhopping = true;
        cfg.ladder_scale = 1.0;
        cfg.max_component_velocity = 2000.0;
        cfg.ledge_helper = false;
        Self { cfg, modifier: 1.0, last_yaw: None, ground: GroundTicks::default() }
    }
}

impl Default for KzTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl MovementMode for KzTimer {
    fn kind(&self) -> ModeKind {
        ModeKind::KzTimer
    }
    fn config(&self) -> &MovementConfig {
        &self.cfg
    }

    fn pre_command(&mut self, state: &mut PlayerState, cmd: &mut UserCmd, dt: f32) {
        // Suppress a fresh grounded jump+duck on the same command [Ref §20.1]: the jump wins.
        let fresh = |b: Buttons| cmd.buttons.contains(b) && !state.old_buttons.contains(b);
        if state.on_ground() && fresh(Buttons::JUMP) && fresh(Buttons::DUCK) {
            cmd.buttons.remove(Buttons::DUCK);
        }

        let yaw = cmd.view_angles.y;
        let turning = self.last_yaw.is_some_and(|p| yaw_delta(p, yaw) != 0.0);
        self.last_yaw = Some(yaw);
        if state.on_ground() {
            let strafing = cmd.side_move != 0.0 && cmd.forward_move != 0.0;
            if turning && strafing {
                self.modifier = (self.modifier + PRESTRAFE_RATE * dt).min(PRESTRAFE_MAX);
            } else {
                self.modifier = (self.modifier - PRESTRAFE_RATE * dt).max(1.0);
            }
        }
    }

    fn post_command(&mut self, state: &mut PlayerState, _cmd: &UserCmd) {
        self.ground.update(state);
    }

    fn on_jump(&mut self, state: &mut PlayerState, _ground_speed: f32, _ground_z: Option<f32>) {
        if self.ground.ticks <= 1 {
            cap_horizontal(state, PERF_SPEED_CAP);
        }
    }

    fn modify_wish_speed(&self, state: &PlayerState, base: f32) -> f32 {
        if state.on_ground() {
            base * self.modifier
        } else {
            base
        }
    }

    fn reset(&mut self) {
        self.modifier = 1.0;
        self.last_yaw = None;
        self.ground = GroundTicks::default();
    }

    fn prestrafe_multiplier(&self) -> f32 {
        self.modifier
    }
}

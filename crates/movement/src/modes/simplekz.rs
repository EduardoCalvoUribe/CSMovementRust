//! GOKZ SimpleKZ mode [Ref §20, §20.2]. 128 tick only. The takeoff rule is the reference's formula; the
//! bonus growth/decay/grace rates are our own designs (see `docs/modes-notes.md`).

use super::{cap_horizontal, yaw_delta, GroundTicks, ModeKind, MovementMode};
use crate::cmd::UserCmd;
use crate::config::MovementConfig;
use crate::state::PlayerState;

pub const TICKRATE: u32 = 128;
/// Prestrafe bonus ceiling, as a multiplier (unverified; same 276 target as KZTimer).
pub const BONUS_MAX: f32 = 0.104;
/// Bonus gained per second while turning with strafe input (unverified).
pub const BONUS_GROWTH: f32 = 0.26;
/// Bonus lost per second once the grace interval runs out (unverified).
pub const BONUS_DECAY: f32 = 0.35;
/// Seconds without turning before the bonus starts to decay (unverified).
pub const BONUS_GRACE: f32 = 0.1;
/// Grounded commands within which a jump still counts as perfect (unverified).
pub const PERF_TICKS: u32 = 2;

/// Takeoff speed after a perfect hop: `min(V, (0.2 V + 200) M_pre)` [Ref §20.2].
pub fn takeoff_speed(landing_speed: f32, m_pre: f32) -> f32 {
    landing_speed.min((0.2 * landing_speed + 200.0) * m_pre)
}

pub struct SimpleKz {
    cfg: MovementConfig,
    bonus: f32,
    grace_left: f32,
    last_yaw: Option<f32>,
    ground: GroundTicks,
    landing_speed: f32,
}

impl SimpleKz {
    pub fn new() -> Self {
        let mut cfg = MovementConfig::vanilla();
        cfg.accelerate = 6.5;
        cfg.air_accelerate = 100.0;
        cfg.friction = 5.2;
        cfg.weapon_speed_scaling = false;
        cfg.stamina_jump_cost = 0.0;
        cfg.stamina_land_cost = 0.0;
        cfg.enable_bunnyhopping = true;
        cfg.ladder_scale = 1.0;
        cfg.max_component_velocity = 3500.0;
        cfg.ledge_helper = false;
        Self { cfg, bonus: 0.0, grace_left: 0.0, last_yaw: None, ground: GroundTicks::default(), landing_speed: 0.0 }
    }

    pub fn multiplier(&self) -> f32 {
        1.0 + self.bonus
    }
}

impl Default for SimpleKz {
    fn default() -> Self {
        Self::new()
    }
}

impl MovementMode for SimpleKz {
    fn kind(&self) -> ModeKind {
        ModeKind::SimpleKz
    }
    fn config(&self) -> &MovementConfig {
        &self.cfg
    }
    fn required_tickrate(&self) -> Option<u32> {
        Some(TICKRATE)
    }

    fn pre_command(&mut self, state: &mut PlayerState, cmd: &mut UserCmd, dt: f32) {
        let yaw = cmd.view_angles.y;
        let turning = self.last_yaw.is_some_and(|p| yaw_delta(p, yaw) != 0.0);
        self.last_yaw = Some(yaw);
        if !state.on_ground() {
            return;
        }
        if turning && cmd.side_move != 0.0 {
            self.bonus = (self.bonus + BONUS_GROWTH * dt).min(BONUS_MAX);
            self.grace_left = BONUS_GRACE;
        } else if self.grace_left > 0.0 {
            self.grace_left = (self.grace_left - dt).max(0.0);
        } else {
            self.bonus = (self.bonus - BONUS_DECAY * dt).max(0.0);
        }
    }

    fn post_command(&mut self, state: &mut PlayerState, _cmd: &UserCmd) {
        self.ground.update(state);
    }

    fn on_land(&mut self, state: &mut PlayerState) {
        self.landing_speed = state.velocity.length_2d();
    }

    fn on_jump(&mut self, state: &mut PlayerState, _ground_speed: f32) {
        if self.ground.ticks <= PERF_TICKS {
            cap_horizontal(state, takeoff_speed(self.landing_speed, self.multiplier()));
        }
    }

    fn modify_wish_speed(&self, state: &PlayerState, base: f32) -> f32 {
        if state.on_ground() {
            base * self.multiplier()
        } else {
            base
        }
    }

    fn reset(&mut self) {
        self.bonus = 0.0;
        self.grace_left = 0.0;
        self.last_yaw = None;
        self.ground = GroundTicks::default();
        self.landing_speed = 0.0;
    }

    fn prestrafe_multiplier(&self) -> f32 {
        self.multiplier()
    }
}

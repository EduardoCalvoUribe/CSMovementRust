//! GOKZ SimpleKZ mode [Ref §20, §20.2], ported from GOKZ 3.6.4 `gokz-mode-simplekz.sp` (GPL-3.0,
//! KZGlobalTeam), with the player state it reads from MovementAPI 2.4.4 (GPL-3.0, DanZay). 128 tick
//! only. See `docs/modes-notes.md` for what is and isn't ported.
//!
//! The prestrafe is a bonus speed earned per grounded command by turning (up to 90 degrees per second
//! counts) with movement keys held, capped by the turn rate and the base speed, and lost in the air.
//! Perfect hops keep a takeoff speed computed from the landing speed.

use super::{calc_delta_angle, horizontal_length, set_horizontal_length, turning, ModeKind, MovementMode};
use crate::cmd::{Buttons, UserCmd};
use crate::config::MovementConfig;
use crate::math::Vec3;
use crate::pipeline::MoveData;
use crate::state::PlayerState;

pub const TICKRATE: u32 = 128;
/// `SPEED_NORMAL`.
pub const SPEED_NORMAL: f32 = 250.0;
/// `PS_MAX_REWARD_TURN_RATE`: degrees per tick (90 per second).
pub const PS_MAX_REWARD_TURN_RATE: f32 = 0.703125;
/// `PS_MAX_TURN_RATE_DECREMENT`: degrees per tick (2 per second).
pub const PS_MAX_TURN_RATE_DECREMENT: f32 = 0.015625;
/// `PS_SPEED_MAX`: bonus units.
pub const PS_SPEED_MAX: f32 = 26.54321;
/// `PS_SPEED_INCREMENT`: units per tick.
pub const PS_SPEED_INCREMENT: f32 = 0.35;
/// `PS_SPEED_DECREMENT_MIDAIR`: units per tick.
pub const PS_SPEED_DECREMENT_MIDAIR: f32 = 0.2824;
/// `PS_GRACE_TICKS`.
pub const PS_GRACE_TICKS: i32 = 3;
pub const DUCK_SPEED_NORMAL: f32 = 8.0;
/// `DUCK_SPEED_MINIMUM`: duck speed after a single duck or unduck.
pub const DUCK_SPEED_MINIMUM: f32 = 6.0234375;
/// GOKZ's `EPSILON`.
const EPSILON: f32 = 0.000001;

/// `CalcTweakedTakeoffSpeed`: after a perfect hop, `min(V, (0.2 V + 200) M)` above 250 [Ref §20.2].
pub fn takeoff_speed(landing_speed: f32, landing_vel_mod: f32) -> f32 {
    if landing_speed > SPEED_NORMAL {
        landing_speed.min((0.2 * landing_speed + 200.0) * landing_vel_mod)
    } else {
        landing_speed
    }
}

pub struct SimpleKz {
    cfg: MovementConfig,
    /// `gF_PSBonusSpeed`, `gF_PSVelMod`, `gF_PSVelModLanding`, `gB_PSTurningLeft`, `gF_PSTurnRate`,
    /// `gI_PSTicksSinceIncrement`.
    bonus_speed: f32,
    vel_mod: f32,
    vel_mod_landing: f32,
    ps_turning_left: bool,
    turn_rate: f32,
    ticks_since_increment: i32,
    /// Command numbers: this one, the landing, the last that held jump.
    cmd: i64,
    landing_cmd: i64,
    last_jump_button_cmd: i64,
    /// Buttons of the previous command as the game ran them (`gI_OldButtons`, `m_nButtons`).
    old_buttons: Buttons,
    /// `gF_OldAngles`: eye angles stored at the end of the previous `OnPlayerRunCmd`.
    old_angles: Vec3,
    /// MovementAPI: eye angles after the last command and the turning flags they gave; landing
    /// velocity; the last takeoff's horizontal speed.
    eye_angles: Vec3,
    turning: bool,
    turning_left: bool,
    landing_velocity: Vec3,
    takeoff_speed: f32,
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
        Self {
            cfg,
            bonus_speed: 0.0,
            vel_mod: 1.0,
            vel_mod_landing: 1.0,
            ps_turning_left: false,
            turn_rate: 0.0,
            ticks_since_increment: 0,
            cmd: 0,
            landing_cmd: i64::MIN / 2,
            last_jump_button_cmd: i64::MIN / 2,
            old_buttons: Buttons::NONE,
            old_angles: Vec3::ZERO,
            eye_angles: Vec3::ZERO,
            turning: false,
            turning_left: false,
            landing_velocity: Vec3::ZERO,
            takeoff_speed: 0.0,
        }
    }

    pub fn multiplier(&self) -> f32 {
        self.vel_mod
    }

    fn reset_vel_mod(&mut self) {
        self.bonus_speed = 0.0;
        self.vel_mod = 1.0;
        self.turn_rate = 0.0;
    }

    /// `ValidPrestrafeButtons`: one of forward/back, or one of left/right.
    fn valid_prestrafe_buttons(b: Buttons) -> bool {
        let fb = (b.contains(Buttons::FORWARD) || b.contains(Buttons::BACK))
            && !(b.contains(Buttons::FORWARD) && b.contains(Buttons::BACK));
        let lr = (b.contains(Buttons::LEFT) || b.contains(Buttons::RIGHT))
            && !(b.contains(Buttons::LEFT) && b.contains(Buttons::RIGHT));
        fb || lr
    }

    /// `CalcPreRewardSpeed`.
    fn reward(yaw_diff: f32, base_speed: f32) -> f32 {
        let reward = if yaw_diff >= PS_MAX_REWARD_TURN_RATE {
            PS_SPEED_INCREMENT
        } else {
            PS_SPEED_INCREMENT * (yaw_diff / PS_MAX_REWARD_TURN_RATE)
        };
        reward * base_speed / SPEED_NORMAL
    }

    /// `CalcPrestrafeVelMod`.
    fn calc_prestrafe_vel_mod(&mut self, state: &PlayerState, angles: Vec3) {
        self.ticks_since_increment += 1;
        let speed = horizontal_length(state.velocity);
        if speed < EPSILON {
            self.reset_vel_mod();
            return;
        }
        let base_speed = SPEED_NORMAL.min(speed / self.vel_mod);
        let mut new_bonus = self.bonus_speed;

        if !state.on_ground() {
            new_bonus -= PS_SPEED_DECREMENT_MIDAIR;
        } else if self.turning && Self::valid_prestrafe_buttons(self.old_buttons) {
            let turning_right = self.turning && !self.turning_left;
            if self.turning_left && !self.ps_turning_left || turning_right && self.ps_turning_left {
                self.reset_vel_mod();
                new_bonus = 0.0;
            }
            self.ps_turning_left = self.turning_left;
            let mut new_turn_rate = calc_delta_angle(self.old_angles.y, angles.y).abs();
            if self.ticks_since_increment <= PS_GRACE_TICKS {
                let ticks = self.ticks_since_increment as f32;
                new_turn_rate = PS_MAX_REWARD_TURN_RATE.min(new_turn_rate / ticks);
                self.turn_rate = new_turn_rate.max(self.turn_rate - PS_MAX_TURN_RATE_DECREMENT * ticks);
                new_bonus += Self::reward(self.turn_rate, base_speed) * ticks;
            } else {
                new_turn_rate = PS_MAX_REWARD_TURN_RATE.min(new_turn_rate);
                self.turn_rate = new_turn_rate.max(self.turn_rate - PS_MAX_TURN_RATE_DECREMENT);
                new_bonus += Self::reward(self.turn_rate, base_speed);
            }
            self.ticks_since_increment = 0;
        } else if self.ticks_since_increment > PS_GRACE_TICKS {
            self.turn_rate = 0.0f32.max(self.turn_rate - PS_MAX_TURN_RATE_DECREMENT);
        }

        if new_bonus < 0.0 {
            new_bonus = 0.0;
        } else {
            let base_scale = base_speed / SPEED_NORMAL;
            let turn_scale = 1.0f32.min(self.turn_rate / PS_MAX_REWARD_TURN_RATE);
            let scaled_max = PS_SPEED_MAX * base_scale * turn_scale;
            new_bonus = new_bonus.min(scaled_max);
        }
        self.bonus_speed = new_bonus;
        self.vel_mod = 1.0 + (new_bonus / base_speed);
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

    fn pre_command(&mut self, state: &mut PlayerState, cmd: &mut UserCmd, _dt: f32) {
        self.cmd += 1;
        // RemoveCrouchJumpBind.
        if state.on_ground()
            && cmd.buttons.contains(Buttons::JUMP)
            && !self.old_buttons.contains(Buttons::JUMP)
            && !self.old_buttons.contains(Buttons::DUCK)
        {
            cmd.buttons.remove(Buttons::DUCK);
        }
        // ReduceDuckSlowdown: full duck speed outside a transition, never below one duck's worth.
        if !state.ducking && state.duck_speed < DUCK_SPEED_NORMAL - EPSILON {
            state.duck_speed = DUCK_SPEED_NORMAL;
        } else if state.duck_speed < DUCK_SPEED_MINIMUM - EPSILON {
            state.duck_speed = DUCK_SPEED_MINIMUM;
        }
        self.calc_prestrafe_vel_mod(state, cmd.view_angles);
        self.old_buttons = cmd.buttons;
        self.old_angles = self.eye_angles;
    }

    fn post_command(&mut self, _state: &mut PlayerState, cmd: &UserCmd, _mv: &MoveData) {
        let (t, left) = turning(self.eye_angles.y, cmd.view_angles.y);
        self.turning = t;
        self.turning_left = left;
        self.eye_angles = cmd.view_angles;
        if cmd.buttons.contains(Buttons::JUMP) {
            self.last_jump_button_cmd = self.cmd;
        }
    }

    fn player_max_speed(&self, _base: f32) -> f32 {
        SPEED_NORMAL * self.vel_mod
    }

    fn air_wish_speed(&self, wish_speed: f32) -> f32 {
        if self.vel_mod > 1.0 {
            wish_speed / self.vel_mod
        } else {
            wish_speed
        }
    }

    fn can_unduck(&self, state: &PlayerState) -> Option<bool> {
        let landed_last = self.landing_cmd + 1 == self.cmd;
        (landed_last && state.duck_amount >= 1.0 && state.ducked).then_some(false)
    }

    /// `TweakJump`.
    fn on_jump(&mut self, state: &mut PlayerState, _ground_speed: f32, ground_z: Option<f32>) {
        let since_landing = self.cmd - self.landing_cmd;
        let hit = since_landing <= 1 || since_landing <= 3 && self.cmd - self.last_jump_button_cmd <= 3;
        if hit {
            // NerfRealPerf: take off from the ground, not from wherever the landing left the player
            // hovering inside the ground probe.
            if since_landing <= 1 && state.velocity.z >= EPSILON {
                if let Some(z) = ground_z {
                    state.origin.z = z;
                }
            }
            // ApplyTweakedTakeoffSpeed: the landing direction at the tweaked speed.
            let landing_speed = horizontal_length(self.landing_velocity);
            let v = set_horizontal_length(self.landing_velocity, takeoff_speed(landing_speed, self.vel_mod_landing));
            let v = v.add(state.base_velocity);
            state.velocity.x = v.x;
            state.velocity.y = v.y;
            if since_landing > 1 || self.takeoff_speed > SPEED_NORMAL {
                // Restore prestrafe lost while briefly on the ground.
                self.vel_mod = self.vel_mod_landing;
            }
        }
        // MovementAPI records the takeoff after the jump.
        self.takeoff_speed = horizontal_length(state.velocity);
    }

    fn on_categorize(&mut self, state: &mut PlayerState, mv: &MoveData, was_on_ground: bool, ground_normal: Option<Vec3>) {
        if !was_on_ground && state.on_ground() {
            self.landing_cmd = self.cmd;
            self.landing_velocity = if mv.air_accelerated { mv.post_aa_velocity } else { state.velocity };
            self.vel_mod_landing = self.vel_mod;
            if let Some(v) = super::slope_fix(state, mv, ground_normal) {
                self.landing_velocity = v;
            }
        } else if was_on_ground && !state.on_ground() && !mv.jumped {
            self.takeoff_speed = horizontal_length(if mv.walk_moved { mv.post_walk_velocity } else { state.velocity });
        }
    }

    fn reset(&mut self) {
        let cfg = self.cfg;
        *self = Self::new();
        self.cfg = cfg;
    }

    fn prestrafe_multiplier(&self) -> f32 {
        self.vel_mod
    }
}

//! GOKZ KZTimer mode [Ref §20, §20.1], ported from GOKZ 3.6.4 `gokz-mode-kztimer.sp` (GPL-3.0,
//! KZGlobalTeam; its prestrafe is itself adapted from KZTimerGlobal), with the player state it reads
//! from MovementAPI 2.4.4 (GPL-3.0, DanZay). See `docs/modes-notes.md` for what is and isn't ported.
//!
//! The prestrafe is a velocity modifier on the player's maximum speed, updated before each command
//! while grounded, turning and holding a strafe key. Air acceleration divides it back out.

use super::{set_horizontal_length, turning, ModeKind, MovementMode};
use crate::cmd::{Buttons, UserCmd};
use crate::config::MovementConfig;
use crate::math::{angle_vectors, Vec3};
use crate::pipeline::MoveData;
use crate::state::{MoveType, PlayerState};

/// `SPEED_NORMAL`: GOKZ's player maximum speed.
pub const SPEED_NORMAL: f32 = 250.0;
/// `PRE_VELMOD_MAX`: 276 / 250 [Ref §20.1].
pub const PRE_VELMOD_MAX: f32 = 1.104;
/// `PERF_SPEED_CAP` [Ref §20.1].
pub const PERF_SPEED_CAP: f32 = 380.0;
/// `DUCK_SPEED_NORMAL`.
pub const DUCK_SPEED_NORMAL: f32 = 8.0;

pub struct KzTimer {
    cfg: MovementConfig,
    /// `gF_PreVelMod`, `gF_PreVelModLastChange`, `gF_RealPreVelMod`, `gI_PreTickCounter`.
    pre_vel_mod: f32,
    pre_vel_mod_last_change: f32,
    real_pre_vel_mod: f32,
    pre_tick_counter: i32,
    /// Stands in for `GetEngineTime()`: one tick interval per command.
    engine_time: f32,
    /// Buttons of the previous command as the game ran them (`gI_OldButtons`, `m_nButtons`).
    old_buttons: Buttons,
    /// The command before that, for `m_afButtonReleased`.
    older_buttons: Buttons,
    /// MovementAPI: eye angles after the last command, and the turning flags they gave.
    eye_angles: Vec3,
    turning: bool,
    turning_left: bool,
    /// MovementAPI: whether the last command walk-moved (no perf after a walk) and its move type.
    old_walk_moved: bool,
    old_move_type: MoveType,
    /// Commands run, and the one during which the player last landed.
    cmd: u64,
    landing_cmd: Option<u64>,
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
        Self {
            cfg,
            pre_vel_mod: 1.0,
            pre_vel_mod_last_change: 0.0,
            real_pre_vel_mod: 1.0,
            pre_tick_counter: 0,
            engine_time: 0.0,
            old_buttons: Buttons::NONE,
            older_buttons: Buttons::NONE,
            eye_angles: Vec3::ZERO,
            turning: false,
            turning_left: false,
            old_walk_moved: false,
            old_move_type: MoveType::Walk,
            cmd: 0,
            landing_cmd: None,
        }
    }

    /// `GetClientMovingDirection(client, false)`: velocity direction against the view direction, with
    /// pitch clamped to ±70.
    fn moving_direction(&self, state: &PlayerState) -> f32 {
        let mut angles = self.eye_angles;
        angles.x = angles.x.clamp(-70.0, 70.0);
        let mut view = angle_vectors(angles).forward;
        let mut vel = state.velocity;
        vel.normalize_in_place();
        view.normalize_in_place();
        vel.dot(view)
    }

    /// `CalcPrestrafeVelMod`.
    fn calc_prestrafe_vel_mod(&mut self, state: &PlayerState) -> f32 {
        if !state.on_ground() {
            return self.pre_vel_mod;
        }
        let buttons = self.old_buttons;
        let turning_right = self.turning && !self.turning_left;
        let turning_left = self.turning_left;
        if !self.turning {
            if self.engine_time - self.pre_vel_mod_last_change > 0.2 {
                self.pre_vel_mod = 1.0;
                self.pre_vel_mod_last_change = self.engine_time;
            } else if self.pre_vel_mod > PRE_VELMOD_MAX + 0.007 {
                // Returning without setting the modifier is intentional (as in GOKZ).
                return PRE_VELMOD_MAX - 0.001;
            }
        } else if (buttons.contains(Buttons::LEFT) || buttons.contains(Buttons::RIGHT))
            && super::horizontal_length(state.velocity) > 248.9
        {
            let increment = if self.pre_vel_mod > 1.04 { 0.001 } else { 0.0009 };
            let forwards = self.moving_direction(state) > 0.0;
            if (buttons.contains(Buttons::RIGHT) && turning_right || turning_left && !forwards)
                || (buttons.contains(Buttons::LEFT) && turning_left || turning_right && !forwards)
            {
                self.pre_tick_counter += 1;
                if self.pre_tick_counter < 75 {
                    self.pre_vel_mod += increment;
                    if self.pre_vel_mod > PRE_VELMOD_MAX {
                        if self.pre_vel_mod > PRE_VELMOD_MAX + 0.007 {
                            self.pre_vel_mod = PRE_VELMOD_MAX - 0.001;
                        } else {
                            self.pre_vel_mod -= 0.007;
                        }
                    }
                    self.pre_vel_mod += increment;
                } else {
                    self.pre_vel_mod -= 0.0045;
                    self.pre_tick_counter -= 2;
                    if self.pre_vel_mod < 1.0 {
                        self.pre_vel_mod = 1.0;
                        self.pre_tick_counter = 0;
                    }
                }
            } else {
                self.pre_vel_mod -= 0.04;
                if self.pre_vel_mod < 1.0 {
                    self.pre_vel_mod = 1.0;
                }
            }
            self.pre_vel_mod_last_change = self.engine_time;
        } else {
            self.pre_tick_counter = 0;
            // Returning without setting the modifier is intentional (as in GOKZ).
            return 1.0;
        }
        self.pre_vel_mod
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
    /// GOKZ's KZTimer refuses to load on anything but 128 tick.
    fn required_tickrate(&self) -> Option<u32> {
        Some(128)
    }

    fn pre_command(&mut self, state: &mut PlayerState, cmd: &mut UserCmd, dt: f32) {
        self.cmd += 1;
        self.engine_time += dt;
        // RemoveCrouchJumpBind: a fresh grounded jump drops a duck that wasn't already held.
        if state.on_ground()
            && cmd.buttons.contains(Buttons::JUMP)
            && !self.old_buttons.contains(Buttons::JUMP)
            && !self.old_buttons.contains(Buttons::DUCK)
        {
            cmd.buttons.remove(Buttons::DUCK);
        }
        self.real_pre_vel_mod = self.calc_prestrafe_vel_mod(state);
        // ReduceDuckSlowdown: releasing duck restores full duck speed.
        if self.older_buttons.contains(Buttons::DUCK) && !self.old_buttons.contains(Buttons::DUCK) {
            state.duck_speed = DUCK_SPEED_NORMAL;
        }
        self.older_buttons = self.old_buttons;
        self.old_buttons = cmd.buttons;
    }

    fn post_command(&mut self, state: &mut PlayerState, cmd: &UserCmd, mv: &MoveData) {
        let (t, left) = turning(self.eye_angles.y, cmd.view_angles.y);
        self.turning = t;
        self.turning_left = left;
        self.eye_angles = cmd.view_angles;
        self.old_walk_moved = mv.walk_moved;
        self.old_move_type = state.move_type;
    }

    fn player_max_speed(&self, _base: f32) -> f32 {
        SPEED_NORMAL * self.real_pre_vel_mod
    }

    fn air_wish_speed(&self, wish_speed: f32) -> f32 {
        if self.pre_vel_mod > 1.0 {
            wish_speed / self.pre_vel_mod
        } else {
            wish_speed
        }
    }

    fn can_unduck(&self, state: &PlayerState) -> Option<bool> {
        // Just landed fully ducked: no unduck on the next command.
        let landed_last = self.landing_cmd.is_some_and(|c| c + 1 == self.cmd);
        (landed_last && state.duck_amount >= 1.0 && state.ducked).then_some(false)
    }

    fn on_jump(&mut self, state: &mut PlayerState, _ground_speed: f32, _ground_z: Option<f32>) {
        // MovementAPI: a perf is a jump on a command after one that didn't walk-move.
        let hit_perf = self.old_move_type != MoveType::Ladder && !self.old_walk_moved;
        if hit_perf && super::horizontal_length(state.velocity) > PERF_SPEED_CAP {
            state.velocity = set_horizontal_length(state.velocity, PERF_SPEED_CAP);
        }
    }

    fn on_categorize(&mut self, state: &mut PlayerState, mv: &MoveData, was_on_ground: bool, ground_normal: Option<Vec3>) {
        if !was_on_ground && state.on_ground() {
            self.landing_cmd = Some(self.cmd);
            super::slope_fix(state, mv, ground_normal);
        }
    }

    fn reset(&mut self) {
        let cfg = self.cfg;
        *self = Self::new();
        self.cfg = cfg;
    }

    fn prestrafe_multiplier(&self) -> f32 {
        self.real_pre_vel_mod
    }
}

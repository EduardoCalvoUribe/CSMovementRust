//! `ProcessMovement` / `PlayerMove` / `FullWalkMove` ordering [Ref §2].
//!
//! Each Source routine is one method on `Mover`, spread across the modules named in the plan. The call
//! order below is the behavior; keep it even where it looks redundant.

use crate::cmd::{Buttons, UserCmd};
use crate::config::MovementConfig;
use crate::instrument::{MoveObserver, Phase};
use crate::math::{angle_vectors, Basis, Vec3};
use crate::modes::MovementMode;
use crate::state::{MoveType, PlayerState};
use crate::trace::TraceWorld;

/// Per-command working copy, as Source's `CMoveData`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MoveData {
    pub buttons: Buttons,
    pub old_buttons: Buttons,
    pub forward_move: f32,
    pub side_move: f32,
    pub up_move: f32,
    /// Move-data maximum speed after all crops [Ref §3].
    pub max_speed: f32,
    /// Movement angles (view angles with roll removed).
    pub angles: Vec3,
    pub basis: Basis,
    pub out_wish_vel: Vec3,
    pub out_jump_vel: Vec3,
    pub out_step_height: f32,
    // Debug values for the HUD.
    pub wish_dir: Vec3,
    pub wish_speed: f32,
    pub accel_budget: f32,
}

pub(crate) struct Mover<'a, W: TraceWorld + ?Sized, O: MoveObserver + ?Sized> {
    pub cfg: &'a MovementConfig,
    pub mode: &'a mut dyn MovementMode,
    pub world: &'a W,
    pub state: &'a mut PlayerState,
    pub mv: MoveData,
    pub obs: &'a mut O,
    pub dt: f32,
    pub phase: Phase,
}

/// Run one command. `dt` is the tick interval (1/64 or 1/128) [Ref §2], never the render frame time.
pub fn process_movement<W: TraceWorld + ?Sized, O: MoveObserver + ?Sized>(
    cfg: &MovementConfig,
    mode: &mut dyn MovementMode,
    world: &W,
    state: &mut PlayerState,
    cmd: &UserCmd,
    obs: &mut O,
    dt: f32,
) -> MoveData {
    let mut cmd = *cmd;
    if cfg.autobhop {
        state.old_buttons.remove(Buttons::JUMP);
    }
    mode.pre_command(state, &mut cmd, dt);
    obs.on_cmd_start(state, &cmd);

    let mv = MoveData {
        buttons: cmd.buttons,
        old_buttons: state.old_buttons,
        forward_move: cmd.forward_move,
        side_move: cmd.side_move,
        up_move: cmd.up_move,
        max_speed: cfg.weapon_max_speed,
        angles: cmd.view_angles,
        ..MoveData::default()
    };
    let mut m = Mover { cfg, mode, world, state, mv, obs, dt, phase: Phase::PreMove };
    m.player_move();

    // FinishMove
    m.state.old_buttons = m.mv.buttons;
    m.state.tick = m.state.tick.wrapping_add(1);
    let mv = m.mv;
    mode.post_command(state, &cmd);
    obs.on_cmd_end(state, &cmd);
    mv
}

impl<W: TraceWorld + ?Sized, O: MoveObserver + ?Sized> Mover<'_, W, O> {
    pub(crate) fn motion(&self) -> crate::instrument::Motion {
        crate::instrument::Motion { origin: self.state.origin, velocity: self.state.velocity }
    }

    fn player_move(&mut self) {
        self.check_parameters();
        self.mv.out_wish_vel = Vec3::ZERO;
        self.mv.out_jump_vel = Vec3::ZERO;
        self.reduce_timers();
        self.mv.basis = angle_vectors(self.mv.angles);

        // Optimized initial categorization: only unground when launched upward fast [Ref §2].
        self.phase = Phase::PreMove;
        if self.state.move_type != MoveType::Walk {
            self.categorize_position();
        } else if self.state.velocity.z > self.cfg.unground_velocity {
            self.set_ground_entity(None);
        }

        // Store off the fall velocity before duck processing can ground us [Ref §15.1].
        if !self.state.on_ground() {
            self.state.fall_velocity = -self.state.velocity.z;
        }

        self.phase = Phase::Duck;
        self.duck();

        self.phase = Phase::Ladder;
        if !self.ladder_move() && self.state.move_type == MoveType::Ladder {
            self.state.move_type = MoveType::Walk;
        }

        match self.state.move_type {
            MoveType::Walk => self.full_walk_move(),
            MoveType::Ladder => self.full_ladder_move(),
        }
    }

    /// `FullWalkMove`, dry land only [Ref §2].
    fn full_walk_move(&mut self) {
        self.start_gravity();

        self.phase = Phase::Jump;
        if self.mv.buttons.contains(Buttons::JUMP) {
            self.check_jump_button();
        } else {
            self.mv.old_buttons.remove(Buttons::JUMP);
        }

        // Friction runs after jump, so a jump on the first grounded command skips it [Ref §11.1].
        self.phase = Phase::Move;
        if self.state.on_ground() {
            self.state.velocity.z = 0.0;
            self.state.fall_velocity = 0.0;
            self.friction();
        }

        self.check_velocity();

        if self.state.on_ground() {
            self.walk_move();
        } else {
            self.air_move();
        }

        self.phase = Phase::PostMove;
        self.categorize_position();
        self.check_velocity();
        self.finish_gravity();
        if self.state.on_ground() {
            self.state.velocity.z = 0.0;
        }
        self.check_falling();
    }

    /// `StartGravity`: half a gravity step before movement [Ref §9].
    pub(crate) fn start_gravity(&mut self) {
        let ent_gravity = 1.0f32;
        self.state.velocity.z -= ent_gravity * self.cfg.gravity * 0.5 * self.dt;
        self.state.velocity.z += self.state.base_velocity.z * self.dt;
        self.state.base_velocity.z = 0.0;
        self.check_velocity();
    }

    /// `FinishGravity`: the other half step [Ref §9].
    pub(crate) fn finish_gravity(&mut self) {
        let ent_gravity = 1.0f32;
        self.state.velocity.z -= ent_gravity * self.cfg.gravity * self.dt * 0.5;
        self.check_velocity();
    }

    /// `CheckVelocity`: NaN guard and per-component limit [Ref §3].
    pub(crate) fn check_velocity(&mut self) {
        let lim = self.cfg.max_component_velocity;
        for i in 0..3 {
            let v = self.state.velocity.get(i);
            if v.is_nan() {
                self.state.velocity.set(i, 0.0);
            } else if v > lim {
                self.state.velocity.set(i, lim);
            } else if v < -lim {
                self.state.velocity.set(i, -lim);
            }
            if self.state.origin.get(i).is_nan() {
                self.state.origin.set(i, 0.0);
            }
        }
    }

    /// `ReduceTimers` with CS stamina recovery [Ref §8].
    fn reduce_timers(&mut self) {
        if self.state.stamina > 0.0 {
            self.state.stamina = crate::stamina::recover(self.cfg, self.state.stamina, self.dt);
        }
        self.state.duck_speed = crate::math::approach(
            self.cfg.duck_speed_ideal,
            self.state.duck_speed,
            self.dt * self.cfg.duck_speed_recovery,
        );
        if self.state.ladder_jump_ignore > 0.0 {
            self.state.ladder_jump_ignore = (self.state.ladder_jump_ignore - self.dt).max(0.0);
        }
    }

    /// `CheckParameters` with the CS input crops [Ref §5.2, §8, §10.3].
    fn check_parameters(&mut self) {
        let scale_inputs = |mv: &mut MoveData, k: f32| {
            mv.forward_move *= k;
            mv.side_move *= k;
            mv.up_move *= k;
            mv.max_speed *= k;
        };

        let d = self.state.duck_amount;
        if d > 0.0 {
            scale_inputs(&mut self.mv, 1.0 - DUCK_CROP * d);
        }
        if self.mv.buttons.contains(Buttons::WALK) && !self.state.ducked {
            scale_inputs(&mut self.mv, self.cfg.walk_modifier);
        }
        // Sampled before ReduceTimers recovers stamina [Ref §8].
        if self.state.stamina > 0.0 {
            scale_inputs(&mut self.mv, crate::stamina::speed_scale(self.cfg, self.state.stamina));
        }
        self.mv.max_speed = self.mode.modify_wish_speed(self.state, self.mv.max_speed);

        let spd = self.mv.forward_move * self.mv.forward_move
            + self.mv.side_move * self.mv.side_move
            + self.mv.up_move * self.mv.up_move;
        if spd != 0.0 && spd > self.mv.max_speed * self.mv.max_speed {
            let ratio = self.mv.max_speed / spd.sqrt();
            self.mv.forward_move *= ratio;
            self.mv.side_move *= ratio;
            self.mv.up_move *= ratio;
        }

        // sv_rollangle is 0, so the movement roll is 0.
        self.mv.angles.z = 0.0;
        if self.mv.angles.y > 180.0 {
            self.mv.angles.y -= 360.0;
        }
    }
}

/// Duck input crop coefficient: inputs scale by `1 - 0.66 d` [Ref §10.3].
pub const DUCK_CROP: f32 = 0.66;

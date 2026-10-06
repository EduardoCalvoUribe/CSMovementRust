//! `CheckJumpButton`, `PreventBunnyJumping`, and the CS jump stamina [Ref §8, §9, §11].

use crate::cmd::Buttons;
use crate::instrument::{JumpBranch, JumpEvent, MoveObserver};
use crate::pipeline::Mover;
use crate::trace::TraceWorld;

impl<W: TraceWorld + ?Sized, O: MoveObserver + ?Sized> Mover<'_, W, O> {
    /// Returns true if a jump happened.
    pub(crate) fn check_jump_button(&mut self) -> bool {
        let before = self.motion();
        let no_jump = |m: &mut Self| {
            let ev = JumpEvent { jumped: false, branch: None, impulse: 0.0, before, after: m.motion() };
            m.obs.on_jump_button(&ev);
            false
        };

        // An airborne press consumes the button transition [Ref §11].
        if !self.state.on_ground() {
            self.mv.old_buttons.insert(Buttons::JUMP);
            return no_jump(self);
        }
        // Don't pogo stick.
        if self.mv.old_buttons.contains(Buttons::JUMP) {
            return no_jump(self);
        }

        if !self.cfg.enable_bunnyhopping {
            self.prevent_bunny_jumping();
        }

        self.set_ground_entity(None);

        // Ground surface jump factor; every primitive surface uses 1.
        let ground_factor = 1.0f32;
        let start_z = self.state.velocity.z;
        let branch = if self.state.ducking || self.state.ducked {
            self.state.velocity.z = ground_factor * self.cfg.jump_impulse;
            JumpBranch::DuckReset
        } else {
            self.state.velocity.z += ground_factor * self.cfg.jump_impulse;
            JumpBranch::Standing
        };
        if self.state.stamina > 0.0 {
            self.state.velocity.z *= crate::stamina::jump_scale(self.cfg, self.state.stamina);
        }

        self.finish_gravity();

        let impulse = self.state.velocity.z - start_z;
        self.mv.out_jump_vel.z += impulse;
        self.mv.out_step_height += 0.15;

        // CS OnJump: jump stamina from the measured impulse [Ref §8].
        self.state.stamina = crate::stamina::after_jump(self.cfg, self.state.stamina, self.mv.out_jump_vel.z);
        let ground_speed = self.state.velocity.length_2d();
        self.mode.on_jump(self.state, ground_speed);

        self.mv.old_buttons.insert(Buttons::JUMP);
        let ev = JumpEvent { jumped: true, branch: Some(branch), impulse, before, after: self.motion() };
        self.obs.on_jump_button(&ev);
        true
    }

    /// Stock anti-bhop: rescale the 3D velocity to 1.1x the player's max-speed field [Ref §11.2].
    pub(crate) fn prevent_bunny_jumping(&mut self) {
        let max_scaled = self.cfg.bunnyjump_max_speed_factor * self.cfg.player_max_speed;
        if max_scaled <= 0.0 {
            return;
        }
        let spd = self.state.velocity.length();
        if spd <= max_scaled {
            return;
        }
        let fraction = max_scaled / spd;
        self.state.velocity = self.state.velocity.scale(fraction);
    }
}

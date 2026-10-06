//! `LadderMove` and `FullLadderMove` [Ref §18].
//!
//! Ladders are non-solid volumes found with the ladder trace. Detection reaches 2 units when entering and
//! 10 while attached; a fresh attach starts a jump-ignore interval; a detach jump sets velocity to 270 n.

use crate::cmd::Buttons;
use crate::instrument::MoveObserver;
use crate::math::Vec3;
use crate::pipeline::Mover;
use crate::state::MoveType;
use crate::trace::{Hull, TraceWorld};

impl<W: TraceWorld + ?Sized, O: MoveObserver + ?Sized> Mover<'_, W, O> {
    /// Returns true while the player is on a ladder this command.
    pub(crate) fn ladder_move(&mut self) -> bool {
        let attached = self.state.move_type == MoveType::Ladder;
        let wish_dir = if attached {
            self.state.ladder_normal.neg()
        } else if self.mv.forward_move != 0.0 || self.mv.side_move != 0.0 {
            let f = self.mv.basis.forward;
            let r = self.mv.basis.right;
            let fm = self.mv.forward_move;
            let sm = self.mv.side_move;
            Vec3::new(f.x * fm + r.x * sm, f.y * fm + r.y * sm, f.z * fm + r.z * sm).normalized()
        } else {
            return false;
        };

        let dist = if attached { self.cfg.ladder_distance_attached } else { self.cfg.ladder_distance };
        let end = self.state.origin.ma(dist, wish_dir);
        let pm = self.world.trace_ladder(self.state.origin, end, self.state.hull());
        if pm.fraction == 1.0 {
            return false;
        }

        if !attached {
            self.state.ladder_jump_ignore = self.cfg.ladder_jump_ignore_time;
        }
        self.state.move_type = MoveType::Ladder;
        self.state.ladder_normal = pm.plane_normal;
        let n = pm.plane_normal;

        let mut floor = self.state.origin;
        floor.z += self.state.hull().mins.z - 1.0;
        let point = Hull::new(Vec3::ZERO, Vec3::ZERO);
        let on_floor = self.world.point_solid(floor, point) || self.state.on_ground();

        let climb_speed = self.cfg.max_climb_speed * self.cfg.ladder_scale;
        let mut forward_speed = 0.0f32;
        let mut right_speed = 0.0f32;
        let b = self.mv.buttons;
        if b.contains(Buttons::BACK) {
            forward_speed -= climb_speed;
        }
        if b.contains(Buttons::FORWARD) {
            forward_speed += climb_speed;
        }
        if b.contains(Buttons::LEFT) {
            right_speed -= climb_speed;
        }
        if b.contains(Buttons::RIGHT) {
            right_speed += climb_speed;
        }

        if b.contains(Buttons::JUMP) && self.state.ladder_jump_ignore <= 0.0 {
            self.state.move_type = MoveType::Walk;
            self.state.velocity = n.scale(self.cfg.ladder_jump_velocity);
        } else if forward_speed != 0.0 || right_speed != 0.0 {
            let velocity = self.mv.basis.forward.scale(forward_speed).ma(right_speed, self.mv.basis.right);
            let perp = Vec3::UP.cross(n).normalized();
            let normal = velocity.dot(n);
            let cross = n.scale(normal);
            let lateral = velocity.sub(cross);
            // Velocity into the ladder face becomes motion along the ladder's vertical direction.
            let tmp = n.cross(perp);
            self.state.velocity = lateral.ma(-normal, tmp);
            if on_floor && normal > 0.0 {
                self.state.velocity = self.state.velocity.ma(self.cfg.max_climb_speed, n);
            }
        } else {
            self.state.velocity = Vec3::ZERO;
        }
        true
    }

    pub(crate) fn full_ladder_move(&mut self) {
        self.phase = crate::instrument::Phase::Jump;
        if self.mv.buttons.contains(Buttons::JUMP) {
            self.check_jump_button();
        } else {
            self.mv.old_buttons.remove(Buttons::JUMP);
        }
        self.phase = crate::instrument::Phase::Move;
        self.state.velocity = self.state.velocity.add(self.state.base_velocity);
        self.try_player_move();
        self.state.velocity = self.state.velocity.sub(self.state.base_velocity);
    }
}

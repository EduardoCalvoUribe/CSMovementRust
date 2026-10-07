//! `TryPlayerMove`, `StepMove`, `StayOnGround` [Ref ?12].

use crate::instrument::{BumpEvent, MoveObserver};
use crate::math::Vec3;
use crate::physics::clip_velocity;
use crate::pipeline::Mover;
use crate::state::MoveType;
use crate::trace::{TraceWorld, DIST_EPSILON};

pub const MAX_BUMPS: u32 = 4;
pub const MAX_CLIP_PLANES: usize = 5;
/// Source's network coordinate resolution, used by `StayOnGround`.
pub(crate) const COORD_RESOLUTION: f32 = 1.0 / 32.0;

impl<W: TraceWorld + ?Sized, O: MoveObserver + ?Sized> Mover<'_, W, O> {
    /// Slide the hull along the velocity for the remaining command time, clipping against up to
    /// `MAX_CLIP_PLANES` planes over `MAX_BUMPS` bumps [Ref ?12.1, ?12.2]. Returns the blocked flags.
    pub(crate) fn try_player_move(&mut self) -> u32 {
        let mut blocked = 0u32;
        let mut num_planes = 0usize;
        let mut planes = [Vec3::ZERO; MAX_CLIP_PLANES];
        let mut original_velocity = self.state.velocity;
        let primal_velocity = self.state.velocity;
        let mut all_fraction = 0.0f32;
        let mut time_left = self.dt;
        let mut new_velocity = Vec3::ZERO;
        let hull = self.state.hull();

        for bump in 0..MAX_BUMPS {
            if self.state.velocity.length() == 0.0 {
                break;
            }
            let start = self.state.origin;
            let end = start.ma(time_left, self.state.velocity);
            let pm = self.world.trace_hull(start, end, hull);
            let velocity_before = self.state.velocity;
            all_fraction += pm.fraction;

            let emit = |m: &mut Self, tl: f32| {
                let ev = BumpEvent {
                    phase: m.phase,
                    bump,
                    start,
                    end,
                    trace: pm,
                    time_left: tl,
                    velocity_before,
                    velocity_after: m.state.velocity,
                };
                m.obs.on_try_player_move(&ev);
            };

            if pm.all_solid {
                self.state.velocity = Vec3::ZERO;
                emit(self, time_left);
                return 4;
            }

            if pm.fraction > 0.0 {
                if pm.fraction == 1.0 {
                    // A swept box can succeed while the end position is embedded; re-check unswept.
                    let stuck = self.world.trace_hull(pm.end_pos, pm.end_pos, hull);
                    if stuck.start_solid || stuck.fraction != 1.0 {
                        self.state.velocity = Vec3::ZERO;
                        emit(self, time_left);
                        break;
                    }
                }
                self.state.origin = pm.end_pos;
                original_velocity = self.state.velocity;
                num_planes = 0;
            }

            if pm.fraction == 1.0 {
                emit(self, time_left);
                break;
            }

            if pm.plane_normal.z > 0.7 {
                blocked |= 1;
            }
            if pm.plane_normal.z == 0.0 {
                blocked |= 2;
            }

            time_left -= time_left * pm.fraction;

            if num_planes >= MAX_CLIP_PLANES {
                self.state.velocity = Vec3::ZERO;
                emit(self, time_left);
                break;
            }

            planes[num_planes] = pm.plane_normal;
            num_planes += 1;

            if num_planes == 1 && self.state.move_type == MoveType::Walk && !self.state.on_ground() {
                // First impact while airborne: reflect (overbounce 1 on floors and, with sv_bounce 0, walls).
                for plane in planes.iter().take(num_planes) {
                    if plane.z > 0.7 {
                        new_velocity = clip_velocity(original_velocity, *plane, 1.0).0;
                        original_velocity = new_velocity;
                    } else {
                        let overbounce = 1.0 + self.cfg.bounce * (1.0 - self.state.surface_friction);
                        new_velocity = clip_velocity(original_velocity, *plane, overbounce).0;
                    }
                }
                self.state.velocity = new_velocity;
                original_velocity = new_velocity;
            } else {
                let mut i = 0;
                while i < num_planes {
                    self.state.velocity = clip_velocity(original_velocity, planes[i], 1.0).0;
                    let mut j = 0;
                    while j < num_planes {
                        if j != i && self.state.velocity.dot(planes[j]) < 0.0 {
                            break;
                        }
                        j += 1;
                    }
                    if j == num_planes {
                        break;
                    }
                    i += 1;
                }
                if i == num_planes {
                    // Go along the crease.
                    if num_planes != 2 {
                        self.state.velocity = Vec3::ZERO;
                        emit(self, time_left);
                        break;
                    }
                    let dir = planes[0].cross(planes[1]).normalized();
                    let d = dir.dot(self.state.velocity);
                    self.state.velocity = dir.scale(d);
                }
                // If velocity turned against the original, stop dead to avoid oscillation in corners.
                let d = self.state.velocity.dot(primal_velocity);
                if d <= 0.0 {
                    self.state.velocity = Vec3::ZERO;
                    emit(self, time_left);
                    break;
                }
            }
            emit(self, time_left);
        }

        if all_fraction == 0.0 {
            self.state.velocity = Vec3::ZERO;
        }
        blocked
    }

    /// Compare the plain slide with up-over-down stepping and keep the farther one [Ref ?12.3].
    pub(crate) fn step_move(&mut self, dest: Vec3) {
        let _ = dest; // The first trace is recomputed inside try_player_move; results are identical.
        let hull = self.state.hull();
        let pos = self.state.origin;
        let vel = self.state.velocity;

        // Slide move down.
        self.try_player_move();
        let down_pos = self.state.origin;
        let down_vel = self.state.velocity;

        // Reset and move up a stair height.
        self.state.origin = pos;
        self.state.velocity = vel;
        let mut end = self.state.origin;
        end.z += self.cfg.step_size + DIST_EPSILON;
        let tr = self.world.trace_hull(self.state.origin, end, hull);
        if !tr.start_solid && !tr.all_solid {
            self.state.origin = tr.end_pos;
        }

        // Slide move up.
        self.try_player_move();

        // Move down a stair (attempt to).
        let mut end = self.state.origin;
        end.z -= self.cfg.step_size + DIST_EPSILON;
        let tr = self.world.trace_hull(self.state.origin, end, hull);

        // Not on ground any more: use the original movement attempt.
        if tr.plane_normal.z < 0.7 {
            self.state.origin = down_pos;
            self.state.velocity = down_vel;
            let step = self.state.origin.z - pos.z;
            if step > 0.0 {
                self.mv.out_step_height += step;
            }
            return;
        }

        if !tr.start_solid && !tr.all_solid {
            self.state.origin = tr.end_pos;
        }
        let up_pos = self.state.origin;

        let down_dist = (down_pos.x - pos.x) * (down_pos.x - pos.x) + (down_pos.y - pos.y) * (down_pos.y - pos.y);
        let up_dist = (up_pos.x - pos.x) * (up_pos.x - pos.x) + (up_pos.y - pos.y) * (up_pos.y - pos.y);
        if down_dist > up_dist {
            self.state.origin = down_pos;
            self.state.velocity = down_vel;
        } else {
            // Copy z value from the slide move.
            self.state.velocity.z = down_vel.z;
        }
        let step = self.state.origin.z - pos.z;
        if step > 0.0 {
            self.mv.out_step_height += step;
        }
    }

    /// Snap down to walkable ground within a step after a grounded move (walking down stairs/slopes).
    pub(crate) fn stay_on_ground(&mut self) {
        let hull = self.state.hull();
        let mut start = self.state.origin;
        let mut end = self.state.origin;
        start.z += 2.0;
        end.z -= self.cfg.step_size;

        let tr = self.world.trace_hull(self.state.origin, start, hull);
        let start = tr.end_pos;

        let tr = self.world.trace_hull(start, end, hull);
        if tr.fraction > 0.0 && tr.fraction < 1.0 && !tr.start_solid && tr.plane_normal.z >= 0.7 {
            let delta = (self.state.origin.z - tr.end_pos.z).abs();
            if delta > 0.5 * COORD_RESOLUTION {
                self.state.origin = tr.end_pos;
            }
        }
    }
}

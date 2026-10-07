//! `CategorizePosition`, `SetGroundEntity`, `CheckFalling` [Ref ?7, ?12.4, ?14].

use crate::instrument::{CategorizeEvent, LandingEvent, MoveObserver, SetGroundEvent};
use crate::math::Vec3;
use crate::pipeline::Mover;
use crate::state::MoveType;
use crate::trace::{Hull, TraceResult, TraceWorld};

impl<W: TraceWorld + ?Sized, O: MoveObserver + ?Sized> Mover<'_, W, O> {
    /// Decide whether the player is supported, and store the surface friction the next acceleration will
    /// use, including the airborne deadstrafe factor [Ref ?7].
    pub(crate) fn categorize_position(&mut self) {
        let ground_before = self.state.ground_entity;
        self.state.surface_friction = 1.0;

        let origin = self.state.origin;
        // A player who is already grounded and walking probes a step further, and is snapped onto
        // walkable ground the main probe finds [Ref ?12.4]; measured, docs/divergences.md D4.
        let grounded_walk = ground_before.is_some() && self.state.move_type == MoveType::Walk;
        let probe = if grounded_walk { self.cfg.ground_probe + self.cfg.step_size } else { self.cfg.ground_probe };
        let point = Vec3::new(origin.x, origin.y, origin.z - probe);

        let zvel = self.state.velocity.z;
        let moving_up = zvel > 0.0;
        // Ground entities are static, so the moving-ground correction of this test is zero.
        let moving_up_rapidly = zvel > self.cfg.non_jump_velocity;

        if moving_up_rapidly || (moving_up && self.state.move_type == MoveType::Ladder) {
            self.set_ground_entity(None);
        } else {
            let hull = self.state.hull();
            let mut pm = self.world.trace_hull(origin, point, hull);
            let main = pm;
            if pm.hit_entity.is_none() || pm.plane_normal.z < self.cfg.walkable_normal {
                self.try_touch_ground_in_quadrants(origin, point, hull, &mut pm);
                if pm.hit_entity.is_none() || pm.plane_normal.z < self.cfg.walkable_normal {
                    self.set_ground_entity(None);
                    if self.state.velocity.z > 0.0 {
                        self.state.surface_friction = self.cfg.deadstrafe_friction;
                    }
                } else {
                    self.set_ground_entity(Some(&pm));
                }
            } else {
                self.set_ground_entity(Some(&pm));
            }
            // Still grounded after a grounded walk: snap down to whatever the main probe hit, walkable or
            // not (support may come from a quadrant probe). A probe that starts inside the 1/32 gap has
            // fraction 0 and doesn't move the player (docs/divergences.md D4).
            if grounded_walk && self.state.on_ground() && main.fraction > 0.0 && main.fraction < 1.0 && !main.start_solid {
                self.state.origin = main.end_pos;
            }
        }

        let ev = CategorizeEvent {
            phase: self.phase,
            ground_before,
            ground_after: self.state.ground_entity,
            surface_friction: self.state.surface_friction,
            motion: self.motion(),
        };
        self.obs.on_categorize(&ev);
    }

    /// Probe with each quarter of the hull footprint, so an edge of a steep surface can't hide shallower
    /// ground under another corner.
    fn try_touch_ground_in_quadrants(&mut self, start: Vec3, end: Vec3, hull: Hull, pm: &mut TraceResult) {
        let fraction = pm.fraction;
        let end_pos = pm.end_pos;
        let (mn, mx) = (hull.mins, hull.maxs);
        let quadrants = [
            Hull::new(mn, Vec3::new(mx.x.min(0.0), mx.y.min(0.0), mx.z)),
            Hull::new(Vec3::new(mn.x.max(0.0), mn.y.max(0.0), mn.z), mx),
            Hull::new(Vec3::new(mn.x, mn.y.max(0.0), mn.z), Vec3::new(mx.x.min(0.0), mx.y, mx.z)),
            Hull::new(Vec3::new(mn.x.max(0.0), mn.y, mn.z), Vec3::new(mx.x, mx.y.min(0.0), mx.z)),
        ];
        for q in quadrants {
            *pm = self.world.trace_hull(start, end, q);
            if pm.hit_entity.is_some() && pm.plane_normal.z >= self.cfg.walkable_normal {
                break;
            }
        }
        pm.fraction = fraction;
        pm.end_pos = end_pos;
    }

    /// `SetGroundEntity`. Landing zeroes vertical velocity without moving the origin [Ref ?12.4].
    pub(crate) fn set_ground_entity(&mut self, pm: Option<&TraceResult>) {
        let new = pm.and_then(|t| t.hit_entity);
        let old = self.state.ground_entity;
        // Static ground has zero velocity, so only the vertical base velocity is reset on transitions.
        if old.is_none() != new.is_none() {
            self.state.base_velocity.z = 0.0;
        }
        self.state.ground_entity = new;
        if let Some(e) = new {
            self.state.surface_friction = self.world.surface_friction_at(e).min(1.0);
            self.state.velocity.z = 0.0;
        }
        let ev = SetGroundEvent { phase: self.phase, old, new, motion: self.motion() };
        self.obs.on_set_ground(&ev);
    }

    /// `CheckFalling`: landing processing, only when grounded with stored fall velocity [Ref ?8, ?14].
    pub(crate) fn check_falling(&mut self) {
        if !self.state.on_ground() || self.state.fall_velocity <= 0.0 {
            return;
        }
        let fall = self.state.fall_velocity;
        // CS OnLand: landing stamina [Ref ?8].
        self.state.stamina = crate::stamina::after_land(self.cfg, self.state.stamina, fall);
        self.mode.on_land(self.state);
        let ev = LandingEvent { fall_velocity: fall, motion: self.motion() };
        self.obs.on_landing(&ev);
        self.state.fall_velocity = 0.0;
    }
}

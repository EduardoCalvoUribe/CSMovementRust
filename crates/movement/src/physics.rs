//! Friction, acceleration, and walk/air movement [Ref ?4?7, ?5.3].
//!
//! The free functions are the math cores, written in Source's operation order. The `Mover` methods are the
//! routines (`Friction`, `Accelerate`, `AirAccelerate`, `WalkMove`, `AirMove`) that read and write state.

use crate::config::MovementConfig;
use crate::instrument::{AccelEvent, MoveObserver};
use crate::math::Vec3;
use crate::pipeline::Mover;
use crate::trace::TraceWorld;

/// Ground friction [Ref ?5.1]. `surface_friction` is mu.
pub fn friction(cfg: &MovementConfig, v: Vec3, surface_friction: f32, dt: f32) -> Vec3 {
    let speed = v.length();
    if speed < 0.1 {
        return v;
    }
    let friction = cfg.friction * surface_friction;
    let control = if speed < cfg.stop_speed { cfg.stop_speed } else { speed };
    let drop = control * friction * dt;
    let mut new_speed = speed - drop;
    if new_speed < 0.0 {
        new_speed = 0.0;
    }
    if new_speed != speed {
        new_speed /= speed;
        return v.scale(new_speed);
    }
    v
}

/// Inputs to the CS ground acceleration override [Ref ?5.2].
#[derive(Clone, Copy, Debug)]
pub struct GroundAccel {
    pub wish_dir: Vec3,
    pub wish_speed: f32,
    pub accel: f32,
    pub surface_friction: f32,
    pub dt: f32,
    /// The duck hull is in use.
    pub ducked: bool,
    /// A duck transition is in progress (m_bDucking).
    pub ducking: bool,
    pub walking: bool,
}

/// CS ground acceleration. Returns the new velocity and the budget before the room clamp [Ref ?5.2].
pub fn ground_accelerate(cfg: &MovementConfig, v: Vec3, a: GroundAccel) -> (Vec3, f32) {
    let current = v.dot(a.wish_dir);
    let add_speed = a.wish_speed - current;
    if add_speed <= 0.0 {
        return (v, 0.0);
    }

    let mut accel_scale = a.wish_speed.max(250.0);
    let mut goal_speed = accel_scale;
    // The scoped-sniper branch is implemented as a flag that is never set (plan ?13 item 3).
    let slow_scoped = false;
    if cfg.weapon_speed_scaling {
        let k = (cfg.weapon_max_speed / 250.0).min(1.0);
        goal_speed *= k;
        if (!a.walking && !a.ducked && !a.ducking) || slow_scoped {
            accel_scale *= k;
        }
    }
    if a.ducked || a.ducking {
        let k = if a.ducked { cfg.ducked_modifier } else { cfg.duck_modifier };
        if !slow_scoped {
            accel_scale *= k;
        }
        goal_speed *= k;
    }
    if a.walking {
        if !slow_scoped {
            accel_scale *= cfg.walk_modifier;
        }
        goal_speed *= cfg.walk_modifier;
        // Taper over the last 5 units/s below the walk goal speed.
        let speed = v.length();
        if speed > goal_speed - 5.0 {
            accel_scale *= ((goal_speed - speed) / 5.0).clamp(0.0, 1.0);
        }
    }

    let budget = a.accel * a.dt * accel_scale * a.surface_friction;
    let accel_speed = if budget > add_speed { add_speed } else { budget };
    (v.ma(accel_speed, a.wish_dir), budget)
}

/// Air acceleration [Ref ?6]. The directional limit uses the CAPPED wish speed, the budget the UNCAPPED one.
/// Returns the new velocity and the budget.
pub fn air_accelerate(
    cfg: &MovementConfig,
    v: Vec3,
    wish_dir: Vec3,
    wish_speed: f32,
    surface_friction: f32,
    dt: f32,
) -> (Vec3, f32) {
    let wishspd = if wish_speed > cfg.air_wish_cap { cfg.air_wish_cap } else { wish_speed };
    let current = v.dot(wish_dir);
    let add_speed = wishspd - current;
    if add_speed <= 0.0 {
        return (v, 0.0);
    }
    let budget = cfg.air_accelerate * wish_speed * dt * surface_friction;
    let accel_speed = if budget > add_speed { add_speed } else { budget };
    (v.ma(accel_speed, wish_dir), budget)
}

/// `ClipVelocity` [Ref ?12.2]. Returns the clipped velocity and the blocked flags.
/// Velocity added along the plane normal when a clip still points into the plane (D16).
pub const CLIP_PUSH: f32 = 1.0 / 32.0;

pub fn clip_velocity(v: Vec3, normal: Vec3, overbounce: f32) -> (Vec3, u32) {
    let angle = normal.z;
    let mut blocked = 0;
    if angle > 0.0 {
        blocked |= 1;
    }
    if angle == 0.0 {
        blocked |= 2;
    }
    let backoff = v.dot(normal) * overbounce;
    let mut out = Vec3::new(v.x - normal.x * backoff, v.y - normal.y * backoff, v.z - normal.z * backoff);
    // Make sure we aren't still moving through the plane. CS:GO pushes off by 1/32 unit/s along the
    // normal instead of removing exactly the residual (SDK 2013 subtracts `normal * adjust`); measured
    // bit for bit on slope slides, docs/divergences.md D16.
    let adjust = out.dot(normal);
    if adjust < 0.0 {
        out = out.add(normal.scale(CLIP_PUSH));
    }
    (out, blocked)
}

impl<W: TraceWorld + ?Sized, O: MoveObserver + ?Sized> Mover<'_, W, O> {
    /// `Friction`.
    pub(crate) fn friction(&mut self) {
        if !self.state.on_ground() {
            return;
        }
        self.state.velocity = friction(self.cfg, self.state.velocity, self.state.surface_friction, self.dt);
    }

    /// Wish direction and speed from the horizontal view basis [Ref ?4].
    fn wish(&mut self, always_normalize: bool) -> (Vec3, Vec3, f32) {
        let mut f = self.mv.basis.forward;
        let mut r = self.mv.basis.right;
        if always_normalize {
            f.z = 0.0;
            r.z = 0.0;
            f.normalize_in_place();
            r.normalize_in_place();
        } else {
            if f.z != 0.0 {
                f.z = 0.0;
                f.normalize_in_place();
            }
            if r.z != 0.0 {
                r.z = 0.0;
                r.normalize_in_place();
            }
        }
        let fm = self.mv.forward_move;
        let sm = self.mv.side_move;
        let mut wish_vel = Vec3::new(f.x * fm + r.x * sm, f.y * fm + r.y * sm, 0.0);
        let mut wish_dir = wish_vel;
        let mut wish_speed = wish_dir.normalize_in_place();
        if wish_speed != 0.0 && wish_speed > self.mv.max_speed {
            wish_vel = wish_vel.scale(self.mv.max_speed / wish_speed);
            wish_speed = self.mv.max_speed;
        }
        self.mv.wish_dir = wish_dir;
        self.mv.wish_speed = wish_speed;
        (wish_vel, wish_dir, wish_speed)
    }

    /// CS `Accelerate` [Ref ?5.2].
    fn accelerate(&mut self, wish_dir: Vec3, wish_speed: f32, accel: f32) {
        let before = self.motion();
        // The duck multiplier applies from the first command of a duck transition (m_bDucking), not only
        // once the duck hull is in: measured, docs/divergences.md D13.
        let walking = self.mv.buttons.contains(crate::cmd::Buttons::WALK) && !self.state.ducked && !self.state.ducking;
        let (v, budget) = ground_accelerate(
            self.cfg,
            self.state.velocity,
            GroundAccel {
                wish_dir,
                wish_speed,
                accel,
                surface_friction: self.state.surface_friction,
                dt: self.dt,
                ducked: self.state.ducked,
                ducking: self.state.ducking,
                walking,
            },
        );
        self.state.velocity = v;
        self.mv.accel_budget = budget;
        let ev = AccelEvent {
            wish_dir,
            wish_speed,
            budget,
            surface_friction: self.state.surface_friction,
            before,
            after: self.motion(),
        };
        self.obs.on_walk_move(&ev);
    }

    /// `AirAccelerate` [Ref ?6, ?7].
    fn air_accelerate(&mut self, wish_dir: Vec3, wish_speed: f32) {
        let before = self.motion();
        let (v, budget) = air_accelerate(
            self.cfg,
            self.state.velocity,
            wish_dir,
            wish_speed,
            self.state.surface_friction,
            self.dt,
        );
        let delta = v.sub(self.state.velocity);
        self.state.velocity = v;
        self.mv.out_wish_vel = self.mv.out_wish_vel.add(delta);
        self.mv.accel_budget = budget;
        let ev = AccelEvent {
            wish_dir,
            wish_speed,
            budget,
            surface_friction: self.state.surface_friction,
            before,
            after: self.motion(),
        };
        self.obs.on_air_accelerate(&ev);
    }

    /// `WalkMove` with the CS total-speed clamp [Ref ?5.3].
    pub(crate) fn walk_move(&mut self) {
        let (_wish_vel, wish_dir, wish_speed) = self.wish(false);
        let old_ground = self.state.ground_entity;

        self.state.velocity.z = 0.0;
        self.accelerate(wish_dir, wish_speed, self.cfg.accelerate);
        self.state.velocity.z = 0.0;

        let spd = self.state.velocity.length();
        if spd > self.mv.max_speed {
            let scale = self.mv.max_speed / spd;
            self.state.velocity.x *= scale;
            self.state.velocity.y *= scale;
        }

        self.state.velocity = self.state.velocity.add(self.state.base_velocity);
        let spd = self.state.velocity.length();
        if spd < 1.0 {
            self.state.velocity = Vec3::ZERO;
            self.state.velocity = self.state.velocity.sub(self.state.base_velocity);
            return;
        }

        let o = self.state.origin;
        let dest = Vec3::new(o.x + self.state.velocity.x * self.dt, o.y + self.state.velocity.y * self.dt, o.z);
        let pm = self.world.trace_hull(o, dest, self.state.hull());
        self.mv.out_wish_vel = self.mv.out_wish_vel.add(wish_dir.scale(wish_speed));

        if pm.fraction == 1.0 {
            self.state.origin = pm.end_pos;
            self.state.velocity = self.state.velocity.sub(self.state.base_velocity);
            self.stay_on_ground();
            return;
        }

        // Don't walk up stairs if not on ground.
        if old_ground.is_none() {
            self.state.velocity = self.state.velocity.sub(self.state.base_velocity);
            return;
        }

        self.step_move(dest);
        self.state.velocity = self.state.velocity.sub(self.state.base_velocity);
        self.stay_on_ground();
    }

    /// `AirMove` [Ref ?6].
    pub(crate) fn air_move(&mut self) {
        let (_wish_vel, wish_dir, wish_speed) = self.wish(true);
        self.air_accelerate(wish_dir, wish_speed);
        self.state.velocity = self.state.velocity.add(self.state.base_velocity);
        self.try_player_move();
        self.state.velocity = self.state.velocity.sub(self.state.base_velocity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT64: f32 = 1.0 / 64.0;
    const DT128: f32 = 1.0 / 128.0;

    fn approx(a: f32, b: f32, eps: f32) {
        assert!((a - b).abs() < eps, "{a} != {b}");
    }

    #[test]
    fn friction_step_at_250() {
        // [Ref ?5.1]: 20.3125 at 64 tick, 10.15625 at 128 tick.
        let cfg = MovementConfig::vanilla();
        let v = friction(&cfg, Vec3::new(250.0, 0.0, 0.0), 1.0, DT64);
        approx(250.0 - v.length(), 20.3125, 1e-3);
        let v = friction(&cfg, Vec3::new(250.0, 0.0, 0.0), 1.0, DT128);
        approx(250.0 - v.length(), 10.15625, 1e-3);
    }

    #[test]
    fn ground_budget_at_250() {
        // [Ref ?5.2]: A_g = 21.484375 at 64 tick, 10.7421875 at 128 tick.
        let cfg = MovementConfig::vanilla();
        for (dt, want) in [(DT64, 21.484_375), (DT128, 10.742_187_5)] {
            let (v, budget) = ground_accelerate(
                &cfg,
                Vec3::ZERO,
                GroundAccel {
                    wish_dir: Vec3::new(1.0, 0.0, 0.0),
                    wish_speed: 250.0,
                    accel: cfg.accelerate,
                    surface_friction: 1.0,
                    dt,
                    ducked: false,
                    ducking: false,
                    walking: false,
                },
            );
            assert_eq!(budget, want);
            assert_eq!(v.x, want);
        }
    }

    #[test]
    fn perpendicular_air_strafe_gain() {
        // [Ref ?6.1]: V=W=250, wish perpendicular, one step -> ~251.793566 (64 tick).
        let cfg = MovementConfig::vanilla();
        let (v, _) =
            air_accelerate(&cfg, Vec3::new(250.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), 250.0, 1.0, DT64);
        approx(v.length(), 251.793_566, 1e-3);
    }

    #[test]
    fn optimal_angle_128_tick() {
        // [Ref ?6.2]: p* = 6.5625 at 128 tick, gain 1.708032.
        let cfg = MovementConfig::vanilla();
        let p = 6.5625_f32;
        let theta = (p / 250.0).acos();
        let wish = Vec3::new(theta.cos(), theta.sin(), 0.0);
        let (v, _) = air_accelerate(&cfg, Vec3::new(250.0, 0.0, 0.0), wish, 250.0, 1.0, DT128);
        approx(v.length() - 250.0, 1.708_032, 1e-3);
    }

    #[test]
    fn deadstrafe_budget_is_quarter() {
        // [Ref ?7]: 0.25 surface friction -> 11.71875 budget at 64 tick.
        let cfg = MovementConfig::vanilla();
        let (_, budget) =
            air_accelerate(&cfg, Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), 250.0, 0.25, DT64);
        assert_eq!(budget, 11.718_75);
        let (_, budget) = air_accelerate(&cfg, Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), 250.0, 1.0, DT64);
        assert_eq!(budget, 46.875);
    }

    #[test]
    fn clip_removes_normal_component() {
        // [Ref ?12.2]: v' = v - (v.n) n.
        let (out, blocked) = clip_velocity(Vec3::new(100.0, 50.0, 0.0), Vec3::new(-1.0, 0.0, 0.0), 1.0);
        assert_eq!(out, Vec3::new(0.0, 50.0, 0.0));
        assert_eq!(blocked, 2);
    }
}

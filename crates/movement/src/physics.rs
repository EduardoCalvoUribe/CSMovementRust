//! Friction and acceleration (reference doc sections 5-6).
//! Horizontal-plane helpers; callers pass velocity with z handled separately.

use crate::config::MovementConfig;
use crate::math::Vec3;

/// Ground friction. `surface_friction` is mu.
pub fn friction(cfg: &MovementConfig, v: Vec3, surface_friction: f32, dt: f32) -> Vec3 {
    let speed = v.length();
    if speed < 0.1 {
        return v;
    }
    let control = speed.max(cfg.stop_speed);
    let drop = control * cfg.friction * surface_friction * dt;
    let new_speed = (speed - drop).max(0.0);
    v.scale(new_speed / speed)
}

/// Projection-limited acceleration with an explicit budget (ground and air share this core).
fn accelerate_core(v: Vec3, wish_dir: Vec3, limit: f32, budget: f32) -> Vec3 {
    let current = v.dot(wish_dir);
    let room = limit - current;
    if room <= 0.0 {
        return v;
    }
    let q = budget.min(room);
    v.add(wish_dir.scale(q))
}

/// Simple 250-style ground acceleration: budget = accel * wish_speed * mu * dt.
pub fn ground_accelerate(
    cfg: &MovementConfig,
    v: Vec3,
    wish_dir: Vec3,
    wish_speed: f32,
    surface_friction: f32,
    dt: f32,
) -> Vec3 {
    let budget = cfg.accelerate * wish_speed * surface_friction * dt;
    accelerate_core(v, wish_dir, wish_speed, budget)
}

/// Air acceleration: the directional limit uses the CAPPED wish speed,
/// the budget uses the UNCAPPED one.
pub fn air_accelerate(
    cfg: &MovementConfig,
    v: Vec3,
    wish_dir: Vec3,
    wish_speed: f32,
    surface_friction: f32,
    dt: f32,
) -> Vec3 {
    let capped = wish_speed.min(cfg.air_wish_cap);
    let budget = cfg.air_accelerate * wish_speed * surface_friction * dt;
    accelerate_core(v, wish_dir, capped, budget)
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
        let cfg = MovementConfig::vanilla();
        let v = friction(&cfg, Vec3::new(250.0, 0.0, 0.0), 1.0, DT64);
        approx(250.0 - v.length(), 20.3125, 1e-3);
        let v = friction(&cfg, Vec3::new(250.0, 0.0, 0.0), 1.0, DT128);
        approx(250.0 - v.length(), 10.15625, 1e-3);
    }

    #[test]
    fn perpendicular_air_strafe_gain() {
        // Doc 6.1/6.2: V=W=250, wish perpendicular, one step -> ~251.793566 (64 tick).
        let cfg = MovementConfig::vanilla();
        let v = air_accelerate(
            &cfg,
            Vec3::new(250.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            250.0,
            1.0,
            DT64,
        );
        approx(v.length(), 251.793_566, 1e-3);
    }

    #[test]
    fn optimal_angle_128_tick() {
        // Doc 6.2: p* = 6.5625 at 128 tick, gain 1.708032.
        let cfg = MovementConfig::vanilla();
        let p = 6.5625_f32;
        let theta = (p / 250.0).acos();
        let wish = Vec3::new(theta.cos(), theta.sin(), 0.0);
        let v = air_accelerate(&cfg, Vec3::new(250.0, 0.0, 0.0), wish, 250.0, 1.0, DT128);
        approx(v.length() - 250.0, 1.708_032, 1e-3);
    }

    #[test]
    fn deadstrafe_budget_is_quarter() {
        // Doc 7: 0.25 surface friction -> 11.71875 budget at 64 tick (perpendicular, p=0).
        let cfg = MovementConfig::vanilla();
        let v = air_accelerate(
            &cfg,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            250.0,
            0.25,
            DT64,
        );
        approx(v.length(), 11.718_75, 1e-4);
    }
}

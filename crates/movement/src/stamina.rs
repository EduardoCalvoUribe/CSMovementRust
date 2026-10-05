//! Stamina penalty accumulator (reference doc section 8).

use crate::config::MovementConfig;

pub fn recover(cfg: &MovementConfig, s: f32, dt: f32) -> f32 {
    (s - cfg.stamina_recovery_rate * dt).max(0.0)
}

pub fn after_jump(cfg: &MovementConfig, s: f32, impulse: f32) -> f32 {
    (s + cfg.stamina_jump_cost * impulse).clamp(0.0, cfg.stamina_max)
}

pub fn after_land(cfg: &MovementConfig, s: f32, fall_velocity: f32) -> f32 {
    (s + cfg.stamina_land_cost * fall_velocity).clamp(0.0, cfg.stamina_max)
}

/// Multiplier on movement speed: (1 - S/100)^2.
pub fn speed_scale(cfg: &MovementConfig, s: f32) -> f32 {
    let k = 1.0 - s / cfg.stamina_range;
    k * k
}

/// Multiplier on jump velocity: clamp(1 - S/100, 0, 1).
pub fn jump_scale(cfg: &MovementConfig, s: f32) -> f32 {
    (1.0 - s / cfg.stamina_range).clamp(0.0, 1.0)
}

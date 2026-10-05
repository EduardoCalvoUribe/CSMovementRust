//! Movement constants (reference doc section 3, GOKZ Vanilla values) as a preset-able struct.

#[derive(Clone, Debug)]
pub struct MovementConfig {
    pub gravity: f32,
    pub jump_impulse: f32,
    pub accelerate: f32,
    pub air_accelerate: f32,
    pub air_wish_cap: f32,
    pub friction: f32,
    pub stop_speed: f32,
    pub step_size: f32,
    pub walkable_normal: f32,
    pub max_component_velocity: f32,
    pub stamina_jump_cost: f32,
    pub stamina_land_cost: f32,
    pub stamina_recovery_rate: f32,
    pub stamina_max: f32,
    pub stamina_range: f32,
}

impl MovementConfig {
    pub fn vanilla() -> Self {
        Self {
            gravity: 800.0,
            jump_impulse: 301.993_377,
            accelerate: 5.5,
            air_accelerate: 12.0,
            air_wish_cap: 30.0,
            friction: 5.2,
            stop_speed: 80.0,
            step_size: 18.0,
            walkable_normal: 0.7,
            max_component_velocity: 3500.0,
            stamina_jump_cost: 0.080,
            stamina_land_cost: 0.050,
            stamina_recovery_rate: 60.0,
            stamina_max: 80.0,
            stamina_range: 100.0,
        }
    }
}

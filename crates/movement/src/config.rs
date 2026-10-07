//! Movement constants [Ref §3] and mode knobs [Ref §20] as a preset-able struct.
//!
//! Values marked "unverified" are not stated in the reference; they are best estimates recorded in
//! `docs/divergences.md` and are config so a capture can correct them without touching code.

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MovementConfig {
    // [Ref §3]
    pub gravity: f32,
    pub jump_impulse: f32,
    pub accelerate: f32,
    pub air_accelerate: f32,
    pub air_wish_cap: f32,
    pub friction: f32,
    pub stop_speed: f32,
    pub step_size: f32,
    pub walkable_normal: f32,
    /// `sv_maxvelocity`: the per-component safeguard.
    pub max_component_velocity: f32,
    pub stamina_jump_cost: f32,
    pub stamina_land_cost: f32,
    pub stamina_recovery_rate: f32,
    pub stamina_max: f32,
    pub stamina_range: f32,
    pub walk_modifier: f32,
    /// Acceleration multiplier while a duck transition is in progress (hull not yet ducked).
    pub duck_modifier: f32,
    /// Acceleration multiplier with the duck hull in use.
    pub ducked_modifier: f32,
    pub ladder_scale: f32,

    // Speed sources [Ref §3 "maximum speed is overloaded", §5.2, §11.2]
    /// Active weapon running speed (knife = 250).
    pub weapon_max_speed: f32,
    /// The player's own max-speed field, used by the anti-bhop cap.
    pub player_max_speed: f32,
    /// Weapon-speed acceleration scaling [Ref §5.2, §20].
    pub weapon_speed_scaling: bool,
    /// `sv_enablebunnyhopping`. When false, `prevent_bunny_jumping` runs [Ref §11.2].
    pub enable_bunnyhopping: bool,
    pub bunnyjump_max_speed_factor: f32,
    /// Jump re-arms every command while held (KZ autobhop / app toggle).
    pub autobhop: bool,

    // Grounding [Ref §7, §12.4]
    pub ground_probe: f32,
    /// Upward speed above which categorization never grounds (`NON_JUMP_VELOCITY`).
    pub non_jump_velocity: f32,
    /// Airborne surface friction while 0 < vz <= non_jump_velocity [Ref §7].
    pub deadstrafe_friction: f32,
    /// With optimized movement, the pre-move categorization only ungrounds above this vz.
    pub unground_velocity: f32,
    /// `sv_bounce`.
    pub bounce: f32,

    // Ladder [Ref §18]
    pub max_climb_speed: f32,
    pub ladder_jump_velocity: f32,
    pub ladder_distance: f32,
    pub ladder_distance_attached: f32,
    pub ladder_jump_ignore_time: f32,
    /// Ledge-catch helper [Ref §18, §20]. Recorded but not simulated (see docs/divergences.md).
    pub ledge_helper: bool,

    // Duck [Ref §10.3]. Measured on CS:GO 1.38.8.1 (docs/divergences.md D5).
    pub duck_speed_ideal: f32,
    /// Duck speed lost on each duck press or release.
    pub duck_speed_penalty: f32,
    /// Floor of duck speed after the penalty.
    pub duck_speed_min: f32,
    /// Duck speed regained per second, every command.
    pub duck_speed_recovery: f32,
    /// Grounded duck-down rate is `duck_speed * duck_down_scale` per second.
    pub duck_down_scale: f32,
    /// Unducking (and a refused duck) moves at `max(duck_speed, unduck_speed_min)` per second.
    pub unduck_speed_min: f32,
    /// A duck press is ignored while duck speed (before this command's recovery) is below this.
    pub duck_refuse_below: f32,
    /// A grounded unduck restores the standing hull once `duck_amount` falls to this or below.
    pub unduck_hull_amount: f32,
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
            walk_modifier: 0.52,
            duck_modifier: 0.34,
            // 1 - 0.66 evaluated in f32 (0.33999997): with the duck hull in, 250 times it is 84.99999,
            // which captures match bit for bit; mid-transition the literal 0.34 applies (D13).
            ducked_modifier: 1.0 - 0.66,
            ladder_scale: 0.78,

            weapon_max_speed: 250.0,
            player_max_speed: 250.0,
            weapon_speed_scaling: true,
            enable_bunnyhopping: false,
            bunnyjump_max_speed_factor: 1.1,
            autobhop: false,

            ground_probe: 2.0,
            non_jump_velocity: 140.0,
            deadstrafe_friction: 0.25,
            unground_velocity: 250.0,
            bounce: 0.0,

            max_climb_speed: 200.0,
            ladder_jump_velocity: 270.0,
            ladder_distance: 2.0,
            ladder_distance_attached: 10.0,
            ladder_jump_ignore_time: 0.2,
            ledge_helper: true,

            duck_speed_ideal: 8.0,
            duck_speed_penalty: 2.0,
            duck_speed_min: 0.0,
            duck_speed_recovery: 3.0,
            duck_down_scale: 0.8,
            unduck_speed_min: 1.5,
            duck_refuse_below: 1.5,
            unduck_hull_amount: 0.75,
        }
    }
}

impl Default for MovementConfig {
    fn default() -> Self {
        Self::vanilla()
    }
}

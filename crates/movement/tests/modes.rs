//! M7 gate: mode presets against [Ref §20] and the mode-specific numbers.

mod common;
use common::*;
use movement::modes::{KzTimer, SimpleKz};
use movement::*;

#[test]
fn config_table_matches_reference() {
    // [Ref §20] table: Vanilla / KZTimer / SimpleKZ.
    let v = *ModeKind::Vanilla.create().config();
    let k = *ModeKind::KzTimer.create().config();
    let s = *ModeKind::SimpleKz.create().config();

    assert_eq!((v.accelerate, k.accelerate, s.accelerate), (5.5, 6.5, 6.5));
    assert_eq!((v.air_accelerate, k.air_accelerate, s.air_accelerate), (12.0, 100.0, 100.0));
    assert_eq!((v.friction, k.friction, s.friction), (5.2, 5.0, 5.2));
    assert_eq!((v.weapon_speed_scaling, k.weapon_speed_scaling, s.weapon_speed_scaling), (true, false, false));
    assert!(v.stamina_jump_cost > 0.0 && v.stamina_land_cost > 0.0);
    assert_eq!((k.stamina_jump_cost, k.stamina_land_cost), (0.0, 0.0));
    assert_eq!((s.stamina_jump_cost, s.stamina_land_cost), (0.0, 0.0));
    // Stock anti-bhop enabled only in Vanilla.
    assert_eq!((v.enable_bunnyhopping, k.enable_bunnyhopping, s.enable_bunnyhopping), (false, true, true));
    assert_eq!((v.ladder_scale, k.ladder_scale, s.ladder_scale), (0.78, 1.0, 1.0));
    assert_eq!(
        (v.max_component_velocity, k.max_component_velocity, s.max_component_velocity),
        (3500.0, 2000.0, 3500.0)
    );
    assert_eq!((v.ledge_helper, k.ledge_helper, s.ledge_helper), (true, false, false));
}

#[test]
fn kz_modes_require_128_tick() {
    // [Ref §20.2] for SimpleKZ; GOKZ 3.6.4 also refuses KZTimer below 128 tick.
    assert_eq!(ModeKind::SimpleKz.create().required_tickrate(), Some(128));
    assert_eq!(ModeKind::KzTimer.create().required_tickrate(), Some(128));
    assert_eq!(ModeKind::Vanilla.create().required_tickrate(), None);
}

fn prestrafe(kind: ModeKind, tickrate: u32) -> (f32, f32) {
    let mut s = Sim::flat(kind, tickrate);
    let mut max: f32 = 0.0;
    let mut yaw = 0.0;
    for _ in 0..(tickrate * 4) {
        yaw += 0.6;
        s.press(Buttons::FORWARD | Buttons::LEFT, yaw);
        max = max.max(s.state.horizontal_speed());
    }
    (max, s.state.horizontal_speed())
}

#[test]
fn kztimer_prestrafe_caps_near_276() {
    // [Ref §20.1]: modifier max 1.104, 276 from 250. GOKZ's increments overshoot the cap by up to two
    // steps before pulling back, so the peak sits just above 276.
    let (max, _) = prestrafe(ModeKind::KzTimer, 128);
    assert!(max > 275.0 && max < 276.5, "{max}");
    // Vanilla has no bonus: the ground clamp holds 250.
    let (max, _) = prestrafe(ModeKind::Vanilla, 64);
    assert!(max <= 250.0 + 1e-3);
}

#[test]
fn kztimer_prestrafe_decays_after_75_turning_commands() {
    // GOKZ: after 75 commands of building, the modifier loses 0.0045 per command while still turning,
    // stepping its counter back so it alternates with gains; the speed settles below the peak. The
    // exact curve is checked against the real game by the S20-*_kztimer_128 captures.
    let mut s = Sim::flat(ModeKind::KzTimer, 128);
    let mut yaw = 0.0;
    let mut peak: f32 = 0.0;
    for _ in 0..400 {
        yaw += 0.6;
        s.press(Buttons::FORWARD | Buttons::LEFT, yaw);
        peak = peak.max(s.state.horizontal_speed());
    }
    assert!(s.state.horizontal_speed() < peak - 0.5, "{} vs peak {peak}", s.state.horizontal_speed());
}

#[test]
fn simplekz_prestrafe_bonus_grows_and_decays() {
    // GOKZ: the bonus tops out at PS_SPEED_MAX over 250 (276.54, as captured in S20-*_simplekz_128).
    let (max, end) = prestrafe(ModeKind::SimpleKz, 128);
    approx(max, 250.0 + movement::modes::simplekz::PS_SPEED_MAX, 0.01);
    approx(end, 250.0 + movement::modes::simplekz::PS_SPEED_MAX, 0.01);
    // Stop turning: the bonus is capped by the falling turn rate, and speed returns to 250.
    let mut s = Sim::flat(ModeKind::SimpleKz, 128);
    let mut yaw = 0.0;
    for _ in 0..512 {
        yaw += 0.6;
        s.press(Buttons::FORWARD | Buttons::LEFT, yaw);
    }
    s.hold(Buttons::FORWARD, yaw, 512);
    approx(s.state.horizontal_speed(), 250.0, 1e-3);
}

fn hop_from(kind: ModeKind, tickrate: u32, speed: f32) -> f32 {
    let mut s = Sim::flat(kind, tickrate);
    s.state.velocity = Vec3::new(speed, 0.0, 0.0);
    // Land once so the next jump is a perfect hop.
    s.press(Buttons::JUMP, 0.0);
    s.state.velocity.x = speed;
    while !s.state.on_ground() {
        s.idle(1);
    }
    s.press(Buttons::JUMP, 0.0);
    assert!(!s.state.on_ground());
    s.state.horizontal_speed()
}

#[test]
fn kztimer_perf_cap_380() {
    // [Ref §20.1]: perfect hop speed cap 380.
    approx(hop_from(ModeKind::KzTimer, 128, 500.0), 380.0, 1e-3);
    approx(hop_from(ModeKind::KzTimer, 128, 350.0), 350.0, 1e-3);
    // Vanilla instead uses the 3D 1.1 x maxspeed restriction [Ref §11.2].
    assert!(hop_from(ModeKind::Vanilla, 64, 500.0) < 276.0);
}

#[test]
fn simplekz_takeoff_formula() {
    // [Ref §20.2]: min(V, (0.2 V + 200) M_pre).
    use movement::modes::simplekz::takeoff_speed;
    assert_eq!(takeoff_speed(250.0, 1.0), 250.0);
    assert_eq!(takeoff_speed(400.0, 1.0), 280.0);
    approx(takeoff_speed(400.0, 1.104), 309.12, 1e-3);
    // In the sim: a perfect hop landing at 400 takes off at 280 (no prestrafe bonus).
    approx(hop_from(ModeKind::SimpleKz, 128, 400.0), 280.0, 1e-2);
}

#[test]
fn kztimer_suppresses_simultaneous_jump_and_duck() {
    // [Ref §20.1]: a fresh grounded jump+duck jumps without ducking, so the standing branch applies.
    let mut s = Sim::flat(ModeKind::KzTimer, 128);
    s.press(Buttons::JUMP | Buttons::DUCK, 0.0);
    assert!(!s.state.ducking && !s.state.ducked);
    let mut v = Sim::flat(ModeKind::Vanilla, 64);
    v.press(Buttons::JUMP | Buttons::DUCK, 0.0);
    assert!(v.state.ducking || v.state.ducked);
}

#[test]
fn mode_reset_clears_private_state() {
    let mut k = KzTimer::new();
    let mut s = SimpleKz::new();
    let mut st = PlayerState::new(Vec3::ZERO);
    st.ground_entity = Some(EntityId::WORLD);
    st.velocity = Vec3::new(250.0, 0.0, 0.0);
    let mut cmd = UserCmd::from_buttons(0, Vec3::ZERO, Buttons::FORWARD | Buttons::LEFT);
    let mv = movement::pipeline::MoveData::default();
    for i in 0..100 {
        cmd.view_angles.y = i as f32 * 0.5;
        k.pre_command(&mut st, &mut cmd, DT128);
        k.post_command(&mut st, &cmd, &mv);
        s.pre_command(&mut st, &mut cmd, DT128);
        s.post_command(&mut st, &cmd, &mv);
    }
    assert!(k.prestrafe_multiplier() > 1.0 && s.prestrafe_multiplier() > 1.0);
    k.reset();
    s.reset();
    assert_eq!((k.prestrafe_multiplier(), s.prestrafe_multiplier()), (1.0, 1.0));
}

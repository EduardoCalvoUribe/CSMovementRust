//! M1 gate: grounded movement on a flat floor.

mod common;
use common::*;
use movement::physics::friction;
use movement::trace::DIST_EPSILON;
use movement::*;

#[test]
fn standing_still_stays_put() {
    // S0: the resting origin sits DIST_EPSILON above the floor and doesn't drift.
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    s.idle(100);
    assert_eq!(s.state.origin, Vec3::new(0.0, 0.0, DIST_EPSILON));
    assert_eq!(s.state.velocity, Vec3::ZERO);
    assert!(s.state.on_ground());
}

/// Ticks of holding W from rest until horizontal speed first reaches 250, and the speed trace.
fn accelerate_from_rest(tickrate: u32) -> (u32, Vec<f32>) {
    let mut s = Sim::flat(ModeKind::Vanilla, tickrate);
    let mut speeds = Vec::new();
    for t in 1..=200 {
        s.press(Buttons::FORWARD, 0.0);
        let v = s.state.horizontal_speed();
        speeds.push(v);
        if v >= 250.0 {
            return (t, speeds);
        }
    }
    panic!("never reached 250: {speeds:?}");
}

#[test]
fn accelerate_from_rest_golden() {
    // [Ref §5.1, §5.2]: friction then a 21.484375 (64) / 10.7421875 (128) budget per tick, capped at 250.
    // Golden tick counts recorded from this implementation.
    let (t64, v64) = accelerate_from_rest(64);
    let (t128, _) = accelerate_from_rest(128);
    assert_eq!(v64[0], 21.484_375, "first tick from rest is the full budget");
    assert_eq!(t64, GOLDEN_TICKS_TO_250_AT_64, "{v64:?}");
    assert_eq!(t128, GOLDEN_TICKS_TO_250_AT_128, "128");
    // Holding W keeps exactly 250: friction removes 20.3125 and acceleration refills it [Ref §5.1].
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    s.hold(Buttons::FORWARD, 0.0, 200);
    assert_eq!(s.state.horizontal_speed(), 250.0);
}

const GOLDEN_TICKS_TO_250_AT_64: u32 = 35;
const GOLDEN_TICKS_TO_250_AT_128: u32 = 72;

#[test]
fn release_decelerates_with_friction_model() {
    // S1 release: speed follows friction alone, including the stop-speed region [Ref §5.1].
    let cfg = MovementConfig::vanilla();
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    s.hold(Buttons::FORWARD, 0.0, 100);
    let mut model = Vec3::new(250.0, 0.0, 0.0);
    for _ in 0..40 {
        s.idle(1);
        model = friction(&cfg, model, 1.0, DT64);
        approx(s.state.velocity.x, model.x, 1e-4);
    }
    assert_eq!(s.state.velocity, Vec3::ZERO);
}

#[test]
fn counter_strafe_stops_when_model_predicts() {
    // S2: running right at 250, then pressing left. Model: friction, then the full opposite budget
    // (the room is 250 + v, always larger), until the velocity crosses zero [Ref §5.4].
    let cfg = MovementConfig::vanilla();
    let budget = cfg.accelerate * DT64 * 250.0 * 1.0;
    let mut v = 250.0f32;
    let mut predicted = 0;
    while v > 0.0 {
        v = friction(&cfg, Vec3::new(v, 0.0, 0.0), 1.0, DT64).x;
        v -= budget;
        predicted += 1;
    }

    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    s.hold(Buttons::RIGHT, 0.0, 100);
    // Facing +x, "right" is -y.
    approx(s.state.velocity.y, -250.0, 1e-3);
    let mut ticks = 0;
    while s.state.velocity.y < 0.0 {
        s.press(Buttons::LEFT, 0.0);
        ticks += 1;
        assert!(ticks < 100);
    }
    assert_eq!(ticks, predicted);
}

#[test]
fn walk_and_duck_speeds() {
    // S3: walk caps at 250 * 0.52, full crouch at 250 * 0.34 [Ref §3, §10.3].
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    s.hold(Buttons::FORWARD | Buttons::WALK, 0.0, 200);
    approx(s.state.horizontal_speed(), 250.0 * 0.52, 0.05);

    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    s.hold(Buttons::FORWARD | Buttons::DUCK, 0.0, 200);
    assert!(s.state.ducked);
    approx(s.state.horizontal_speed(), 250.0 * 0.34, 0.05);
}

#[test]
fn diagonal_input_is_not_faster() {
    // [Ref §4]: W+A only changes direction; the wish speed is capped.
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    s.hold(Buttons::FORWARD | Buttons::LEFT, 0.0, 200);
    approx(s.state.horizontal_speed(), 250.0, 1e-3);
}

//! M4 gate: duck/unduck, hull shifts, crouch-jump heights, low ceilings.

mod common;
use common::*;
use movement::instrument::{DuckEvent, DuckKind, MoveObserver};
use movement::trace::DIST_EPSILON;
use movement::world::Brush;
use movement::*;

/// Max feet height above the takeoff origin for a jump, given the buttons on the jump command and after.
fn apex(s: &mut Sim<PrimitiveWorld>, on_jump: Buttons, after: Buttons) -> f32 {
    let z0 = s.state.origin.z;
    s.press(on_jump | Buttons::JUMP, 0.0);
    let mut max = s.state.origin.z;
    for _ in 0..200 {
        s.press(after, 0.0);
        max = max.max(s.state.origin.z);
        if s.state.on_ground() {
            break;
        }
    }
    max - z0
}

const STANDING_64: f32 = 54.653_766;
const RESET: f32 = 56.997_516;

#[test]
fn crouch_jump_cases_match_reference() {
    // [Ref §10.2] examples, 64 tick. Heights are feet (origin) above takeoff.
    // Jump standing, then crouch airborne: standing branch plus the +9 hull shift.
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    approx(apex(&mut s, Buttons::NONE, Buttons::DUCK), STANDING_64 + 9.0, 1e-3);

    // Begin ducking on the jump command: ducking flag selects the reset branch, then +9 in the air.
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    approx(apex(&mut s, Buttons::DUCK, Buttons::DUCK), RESET + 9.0, 1e-3);

    // Start fully crouched and stay crouched: reset branch, no new shift.
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    s.hold(Buttons::DUCK, 0.0, 64);
    approx(apex(&mut s, Buttons::DUCK, Buttons::DUCK), RESET, 1e-3);

    // Jump fully crouched, then stand in the air: feet drop 9 (minijump geometry).
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    s.hold(Buttons::DUCK, 0.0, 64);
    let z0 = s.state.origin.z;
    s.press(Buttons::DUCK | Buttons::JUMP, 0.0);
    let mut max = s.state.origin.z;
    while !s.state.on_ground() {
        s.idle(1);
        max = max.max(s.state.origin.z);
    }
    // The first airborne command unducks before moving: the peak is the reset trajectory minus 9.
    approx(max - z0, RESET - 9.0, 1e-3);
}

#[derive(Default)]
struct DuckProbe(Vec<DuckEvent>);
impl MoveObserver for DuckProbe {
    fn on_duck(&mut self, ev: &DuckEvent) {
        self.0.push(*ev);
    }
}

#[test]
fn airborne_duck_shifts_nine_units() {
    // [Ref §10.1]: airborne duck raises the origin by 9, airborne unduck lowers it by 9.
    let mut s = Sim::new(PrimitiveWorld::flat_floor(0.0), ModeKind::Vanilla, 64, Vec3::new(0.0, 0.0, 300.0));
    let mut probe = DuckProbe::default();
    s.step_obs(UserCmd::from_buttons(0, Vec3::ZERO, Buttons::DUCK), &mut probe);
    s.step_obs(UserCmd::from_buttons(0, Vec3::ZERO, Buttons::NONE), &mut probe);
    assert_eq!(probe.0.len(), 2);
    assert_eq!(probe.0[0].kind, DuckKind::FinishDuck);
    assert!(probe.0[0].airborne);
    assert_eq!(probe.0[0].after.origin.z - probe.0[0].before.origin.z, 9.0);
    assert_eq!(probe.0[1].kind, DuckKind::FinishUnduck);
    assert_eq!(probe.0[1].after.origin.z - probe.0[1].before.origin.z, -9.0);
}

#[test]
fn grounded_duck_keeps_feet_and_takes_time() {
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    let z = s.state.origin.z;
    s.press(Buttons::DUCK, 0.0);
    assert!(s.state.ducking && !s.state.ducked);
    s.hold(Buttons::DUCK, 0.0, 30);
    assert!(s.state.ducked);
    assert_eq!(s.state.duck_amount, 1.0);
    assert_eq!(s.state.origin.z, z);
    s.hold(Buttons::NONE, 0.0, 30);
    assert!(!s.state.ducked);
    assert_eq!(s.state.duck_amount, 0.0);
    assert_eq!(s.state.origin.z, z);
}

#[test]
fn unduck_blocked_under_low_ceiling() {
    // [Ref §10.2]: CanUnduck traces the standing hull; a 60-unit gap only fits the duck hull.
    let mut world = PrimitiveWorld::flat_floor(0.0);
    world.add(Brush::cuboid(Vec3::new(64.0, -256.0, 60.0), Vec3::new(512.0, 256.0, 120.0)));
    let mut s = Sim::new(world, ModeKind::Vanilla, 64, Vec3::new(0.0, 0.0, DIST_EPSILON));
    s.idle(4);
    s.hold(Buttons::DUCK, 0.0, 40);
    // Crouched acceleration from rest is slow: a 7.3 budget against 6.5 of stop-speed friction [Ref §5].
    s.hold(Buttons::DUCK | Buttons::FORWARD, 0.0, 300);
    assert!(s.state.origin.x > 200.0, "crawled under: {:?}", s.state.origin);
    s.hold(Buttons::NONE, 0.0, 40);
    assert!(s.state.ducked, "stood up into the ceiling");
    assert!(!s.world.point_solid(s.state.origin, s.state.hull()));
    // Walking back out lets it stand.
    s.hold(Buttons::BACK, 0.0, 400);
    assert!(!s.state.ducked);
}

#[test]
fn duck_spam_slows_transitions() {
    // [Ref §10.3]: press/release transitions penalize duck speed; it recovers over time.
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    let fresh = s.state.duck_speed;
    for _ in 0..4 {
        s.press(Buttons::DUCK, 0.0);
        s.press(Buttons::NONE, 0.0);
    }
    assert!(s.state.duck_speed < fresh);
    s.idle(256);
    assert_eq!(s.state.duck_speed, fresh);
}

#[test]
fn ducking_crops_air_wish_speed() {
    // [Ref §10.3]: inputs and the move maximum scale by 1 - 0.66 d, in the air too.
    let mut s = Sim::new(PrimitiveWorld::flat_floor(0.0), ModeKind::Vanilla, 64, Vec3::new(0.0, 0.0, 500.0));
    s.hold(Buttons::DUCK, 0.0, 2);
    assert_eq!(s.state.duck_amount, 1.0);
    let cmd = UserCmd::from_buttons(0, Vec3::ZERO, Buttons::DUCK | Buttons::FORWARD);
    let mv = process_movement(&s.cfg, s.mode.as_mut(), &s.world, &mut s.state, &cmd, &mut NullObserver, s.dt);
    approx(mv.wish_speed, 250.0 * (1.0 - 0.66), 1e-3);
}

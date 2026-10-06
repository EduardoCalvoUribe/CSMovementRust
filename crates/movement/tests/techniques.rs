//! M6 gate: jumpbug, duckbug, edgebug reproduced on test geometry and detected on exactly those
//! commands; ladders.

mod common;
use common::*;
use movement::state::MoveType;
use movement::world::{Brush, Contents};
use movement::*;

fn falling_ducked(height: f32) -> Sim<PrimitiveWorld> {
    let mut s = Sim::new(PrimitiveWorld::flat_floor(0.0), ModeKind::Vanilla, 64, Vec3::new(0.0, 0.0, height));
    s.press(Buttons::DUCK, 0.0);
    assert!(s.state.ducked);
    s
}

/// Fall ducked from `height`, holding duck for `k` commands, then send `release` (duck released).
/// Returns the flags of every command and the state after `release`.
fn drop_then(height: f32, k: u32, release: Buttons) -> Option<(Vec<TechniqueFlags>, PlayerState, PlayerState)> {
    let mut s = falling_ducked(height);
    let mut flags = Vec::new();
    for _ in 0..k {
        flags.push(s.press(Buttons::DUCK, 0.0));
        if s.state.on_ground() {
            return None;
        }
    }
    let before = s.state.clone();
    flags.push(s.press(release, 0.0));
    Some((flags, before, s.state.clone()))
}

#[test]
fn jumpbug_reproduces_and_is_detected_on_that_command() {
    // [Ref §15.2]: unduck creates support during duck processing; jump uses it; no landing.
    let mut found = 0;
    for k in 0..200 {
        let Some((flags, before, after)) = drop_then(150.0, k, Buttons::JUMP) else { break };
        let last = *flags.last().unwrap();
        // Only the final command may carry the flag.
        assert!(flags[..flags.len() - 1].iter().all(|f| !f.jumpbug && !f.duckbug));
        if last.jumpbug {
            found += 1;
            assert!(last.jumped && !last.landed);
            assert!(after.velocity.z > 0.0, "jumpbug must launch upward");
            assert!(!after.on_ground());
            // Window estimate [Ref §15.3]: feet roughly 9-11 units above the floor before the unduck.
            let clearance = before.origin.z;
            assert!((8.9..=11.5).contains(&clearance), "clearance {clearance}");
        }
    }
    assert!(found >= 1, "no jumpbug command found");
}

#[test]
fn duckbug_avoids_landing_processing() {
    // [Ref §15.1]: grounded during duck processing; the ordinary path clears fall velocity, so the
    // landing (and its stamina cost) never runs.
    let mut found = 0;
    for k in 0..200 {
        let Some((flags, _before, after)) = drop_then(150.0, k, Buttons::NONE) else { break };
        let last = *flags.last().unwrap();
        if last.duckbug {
            found += 1;
            assert!(!last.landed && !last.jumped);
            assert!(after.on_ground());
            assert_eq!(after.stamina, 0.0, "no landing stamina");
        } else if after.on_ground() {
            // An ordinary landing pays stamina.
            assert!(last.landed);
            assert!(after.stamina > 0.0);
        }
    }
    assert!(found >= 1);
}

#[test]
fn perfect_bhop_is_not_a_jumpbug() {
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    s.cfg.autobhop = true;
    for _ in 0..300 {
        let f = s.press(Buttons::JUMP, 0.0);
        assert!(!f.jumpbug && !f.duckbug && !f.edgebug);
    }
}

#[test]
fn edgebug_reproduces_with_signature() {
    // [Ref §14]: the sweep hits the top of a ledge, the rest of the command carries the hull past the
    // edge, final categorization finds no ground, and no landing runs. Final vz is -6.25 at 64 tick.
    let mut world = PrimitiveWorld::flat_floor(-2000.0);
    world.add(Brush::cuboid(Vec3::new(-1024.0, -1024.0, -64.0), Vec3::new(0.0, 1024.0, 0.0)));
    let mut found = 0;
    for i in 0..200 {
        // A 96-unit fall at -300 u/s lasts about 0.24 s, drifting about 60 units at 250 u/s.
        let x0 = -80.0 + i as f32 * 0.25;
        let mut s = Sim::new(world.clone(), ModeKind::Vanilla, 64, Vec3::new(x0, 0.0, 96.0));
        s.state.velocity = Vec3::new(250.0, 0.0, -300.0);
        for _ in 0..40 {
            let f = s.press(Buttons::NONE, 0.0);
            if s.state.on_ground() {
                assert!(!f.edgebug);
                break;
            }
            if f.edgebug {
                found += 1;
                assert!(!f.landed);
                assert_eq!(s.state.velocity.z, -6.25);
                assert!(s.state.origin.x > 16.0);
                approx(s.state.velocity.x, 250.0, 1e-3);
                break;
            }
            if s.state.origin.z < -200.0 {
                break;
            }
        }
    }
    assert!(found >= 1, "no edgebug produced");
}

fn ladder_world() -> PrimitiveWorld {
    let mut w = PrimitiveWorld::flat_floor(0.0);
    w.add(Brush::cuboid(Vec3::new(4.0, -64.0, 0.0), Vec3::new(36.0, 64.0, 400.0)));
    w.add(Brush::cuboid(Vec3::new(0.0, -32.0, 0.0), Vec3::new(4.0, 32.0, 400.0)).with_contents(Contents::Ladder));
    w
}

fn look(s: &mut Sim<PrimitiveWorld>, buttons: Buttons, pitch: f32) -> TechniqueFlags {
    s.step(UserCmd::from_buttons(0, Vec3::new(pitch, 0.0, 0.0), buttons))
}

#[test]
fn ladder_attach_climb_and_detach() {
    // [Ref §18].
    let mut s = Sim::new(ladder_world(), ModeKind::Vanilla, 64, Vec3::new(-40.0, 0.0, 0.031_25));
    s.idle(4);
    for _ in 0..40 {
        look(&mut s, Buttons::FORWARD, 0.0);
        if s.state.move_type == MoveType::Ladder {
            break;
        }
    }
    assert_eq!(s.state.move_type, MoveType::Ladder);
    assert_eq!(s.state.ladder_normal, Vec3::new(-1.0, 0.0, 0.0));
    approx(s.state.ladder_jump_ignore, 0.2, 0.02);

    // Looking up and pressing forward climbs.
    let z = s.state.origin.z;
    for _ in 0..30 {
        look(&mut s, Buttons::FORWARD, -45.0);
    }
    assert!(s.state.origin.z > z + 50.0, "{:?}", s.state.origin);
    assert_eq!(s.state.move_type, MoveType::Ladder);

    // Looking down and pressing forward descends: pitch changes ladder motion [Ref §18].
    look(&mut s, Buttons::FORWARD, 45.0);
    assert!(s.state.velocity.z < 0.0);

    // Hysteresis: holding no keys while attached stays attached (10-unit probe) and stops.
    look(&mut s, Buttons::NONE, 0.0);
    assert_eq!(s.state.move_type, MoveType::Ladder);
    assert_eq!(s.state.velocity, Vec3::ZERO);

    // Detach jump: 270 n.
    look(&mut s, Buttons::JUMP, 0.0);
    assert_eq!(s.state.move_type, MoveType::Walk);
    approx(s.state.velocity.x, -270.0, 1e-3);
}

#[test]
fn ladder_jump_ignored_right_after_attach() {
    let mut s = Sim::new(ladder_world(), ModeKind::Vanilla, 64, Vec3::new(-40.0, 0.0, 0.031_25));
    s.idle(4);
    while s.state.move_type != MoveType::Ladder {
        look(&mut s, Buttons::FORWARD, 0.0);
    }
    look(&mut s, Buttons::JUMP | Buttons::FORWARD, -45.0);
    assert_eq!(s.state.move_type, MoveType::Ladder, "jump inside the ignore interval must not detach");
}

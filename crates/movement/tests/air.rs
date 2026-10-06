//! M2 gate: split gravity, jump branches, stamina, bhop, deadstrafe, anti-bhop.
#![allow(clippy::excessive_precision)]

mod common;
use common::*;
use movement::instrument::{AccelEvent, CategorizeEvent, MoveObserver, Phase};
use movement::*;

/// Max sampled height above the takeoff origin for a jump with `buttons` held, from rest.
fn apex(sim: &mut Sim<PrimitiveWorld>, buttons: Buttons) -> f32 {
    let z0 = sim.state.origin.z;
    sim.press(buttons | Buttons::JUMP, 0.0);
    let mut max = sim.state.origin.z;
    while !sim.state.on_ground() {
        sim.press(buttons, 0.0);
        max = max.max(sim.state.origin.z);
    }
    max - z0
}

#[test]
fn standing_jump_apex_matches_reference_table() {
    // [Ref §9] ordinary standing branch, no stamina: 54.653766 (64) / 55.825641 (128).
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    approx(apex(&mut s, Buttons::NONE), 54.653_766, 1e-3);
    let mut s = Sim::flat(ModeKind::Vanilla, 128);
    approx(apex(&mut s, Buttons::NONE), 55.825_641, 1e-3);
}

#[test]
fn ducked_jump_apex_matches_reset_branch() {
    // [Ref §9] duck/reset branch, no hull shift because the hull is already ducked: 56.997516 at both.
    for tickrate in [64, 128] {
        let mut s = Sim::flat(ModeKind::Vanilla, tickrate);
        s.hold(Buttons::DUCK, 0.0, 64);
        assert!(s.state.ducked);
        approx(apex(&mut s, Buttons::DUCK), 56.997_516, 1e-3);
    }
}

#[test]
fn jump_velocity_sequence_uses_split_gravity() {
    // [Ref §9 table]: standing branch ends the jump command at J - 2 h_g (h_g = 6.25 at 64 tick).
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    s.press(Buttons::JUMP, 0.0);
    let cfg = MovementConfig::vanilla();
    approx(s.state.velocity.z, cfg.jump_impulse - 2.0 * 6.25 - 6.25, 1e-4);
}

#[test]
fn jump_stamina_cost_is_about_24() {
    // [Ref §8]: cost = 0.080 * (J - g dt / 2) for an unpenalized standing takeoff, about 24.
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    s.press(Buttons::JUMP, 0.0);
    let want = 0.080 * (301.993_377f32 - 6.25);
    approx(s.state.stamina, want, 1e-3);
    assert!((s.state.stamina - 24.0).abs() < 0.5);
    // Recovers at 60/s [Ref §8].
    let before = s.state.stamina;
    s.idle(1);
    approx(s.state.stamina, before - 60.0 * DT64, 1e-4);
}

#[test]
fn landing_stamina_uses_fall_velocity() {
    // [Ref §8]: landing adds 0.050 * stored fall velocity.
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    s.state.origin.z = 100.0;
    s.state.ground_entity = None;
    let mut last_vz = 0.0;
    loop {
        let start_vz = s.state.velocity.z;
        let stamina_before = s.state.stamina;
        s.idle(1);
        if s.state.on_ground() {
            approx(s.state.stamina, (stamina_before + 0.05 * -start_vz).min(80.0), 1e-3);
            break;
        }
        last_vz = start_vz;
    }
    assert!(last_vz < 0.0);
}

fn run_bhops(cfg_tweak: impl Fn(&mut MovementConfig), late: bool) -> Vec<f32> {
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    cfg_tweak(&mut s.cfg);
    s.state.velocity = Vec3::new(250.0, 0.0, 0.0);
    let mut takeoff_speeds = Vec::new();
    s.press(Buttons::JUMP, 0.0);
    for _ in 0..5 {
        takeoff_speeds.push(s.state.horizontal_speed());
        // Fall back down with no input.
        while !s.state.on_ground() {
            s.idle(1);
        }
        if late {
            s.idle(1);
        }
        s.press(Buttons::JUMP, 0.0);
        assert!(!s.state.on_ground());
    }
    takeoff_speeds
}

#[test]
fn perfect_bhop_chain_keeps_speed() {
    // S7 / [Ref §11.1]: jumping on the first grounded command skips friction and the ground clamp.
    let speeds = run_bhops(|_| {}, false);
    for v in &speeds {
        assert_eq!(*v, 250.0, "{speeds:?}");
    }
}

#[test]
fn late_bhop_gets_friction() {
    // S8: one grounded command of friction before each jump.
    let speeds = run_bhops(|_| {}, true);
    for w in speeds.windows(2) {
        assert!(w[1] < w[0], "{speeds:?}");
    }
}

#[test]
fn anti_bhop_caps_3d_speed_at_takeoff() {
    // [Ref §11.2]: 3D speed rescaled to 1.1 * player max speed (275) before the impulse.
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    s.state.velocity = Vec3::new(400.0, 0.0, 0.0);
    s.press(Buttons::JUMP, 0.0);
    // At the cap, the velocity was (400, 0, -h_g) scaled to length 275.
    let want = 275.0 * 400.0 / (400.0f32 * 400.0 + 6.25 * 6.25).sqrt();
    approx(s.state.horizontal_speed(), want, 1e-2);

    // Without the restriction (sv_enablebunnyhopping 1) the speed is kept.
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    s.cfg.enable_bunnyhopping = true;
    s.state.velocity = Vec3::new(400.0, 0.0, 0.0);
    s.press(Buttons::JUMP, 0.0);
    approx(s.state.horizontal_speed(), 400.0, 1e-3);
}

#[test]
fn held_jump_does_not_rehop_but_autobhop_does() {
    // [Ref §11]: holding jump needs a release; autobhop clears the old jump bit.
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    s.press(Buttons::JUMP, 0.0);
    while !s.state.on_ground() {
        s.press(Buttons::JUMP, 0.0);
    }
    s.press(Buttons::JUMP, 0.0);
    assert!(s.state.on_ground(), "held jump must not hop again");

    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    s.cfg.autobhop = true;
    s.press(Buttons::JUMP, 0.0);
    while !s.state.on_ground() {
        s.press(Buttons::JUMP, 0.0);
    }
    unreachable_if_grounded(&mut s);
}

fn unreachable_if_grounded(s: &mut Sim<PrimitiveWorld>) {
    // With autobhop the landing command ends grounded, and the next one jumps.
    s.press(Buttons::JUMP, 0.0);
    assert!(!s.state.on_ground());
}

#[derive(Default)]
struct DeadstrafeProbe {
    /// (post-move categorize vz, airborne?) of the previous command.
    last_categorize: Option<(f32, bool)>,
    pairs: Vec<(f32, f32)>,
}

impl MoveObserver for DeadstrafeProbe {
    fn on_categorize(&mut self, ev: &CategorizeEvent) {
        if ev.phase == Phase::PostMove {
            self.last_categorize = Some((ev.motion.velocity.z, ev.ground_after.is_none()));
        }
    }
    fn on_air_accelerate(&mut self, ev: &AccelEvent) {
        if let Some((vz, true)) = self.last_categorize {
            self.pairs.push((vz, ev.surface_friction));
        }
    }
}

#[test]
fn deadstrafe_band_quarters_air_budget() {
    // [Ref §7]: surface friction stored by categorization is 0.25 while 0 < vz <= 140, and the next
    // command's air acceleration uses it.
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    let mut probe = DeadstrafeProbe::default();
    let jump = UserCmd::from_buttons(0, Vec3::ZERO, Buttons::JUMP | Buttons::LEFT);
    s.step_obs(jump, &mut probe);
    while !s.state.on_ground() {
        s.step_obs(UserCmd::from_buttons(0, Vec3::ZERO, Buttons::LEFT), &mut probe);
    }
    assert!(probe.pairs.len() > 40);
    let mut saw_dead = false;
    for (vz, sf) in &probe.pairs {
        let expect = if *vz > 0.0 && *vz <= 140.0 { 0.25 } else { 1.0 };
        assert_eq!(*sf, expect, "vz={vz}");
        saw_dead |= *sf == 0.25;
    }
    assert!(saw_dead);
}

#[test]
fn air_strafe_gains_speed() {
    // S5/S6: turning with a strafe key adds speed above 250 [Ref §6.1].
    let mut s = Sim::flat(ModeKind::Vanilla, 64);
    s.hold(Buttons::FORWARD, 0.0, 100);
    s.press(Buttons::JUMP, 0.0);
    let mut yaw = 0.0;
    while !s.state.on_ground() {
        // Turn left at roughly the optimal rate for this speed.
        yaw += 1.2;
        s.press(Buttons::LEFT, yaw);
    }
    assert!(s.state.horizontal_speed() > 260.0, "{}", s.state.horizontal_speed());
}

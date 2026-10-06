//! M8 (automated part): jump classification, distance conventions, sync.

mod common;
use common::*;
use movement::jumpstats::{JumpReport, JumpTracker, JumpType, DISTANCE_OFFSET};
use movement::world::Brush;
use movement::*;

struct Tracked {
    sim: Sim<PrimitiveWorld>,
    tracker: JumpTracker,
    reports: Vec<JumpReport>,
}

impl Tracked {
    fn new(sim: Sim<PrimitiveWorld>) -> Self {
        Self { sim, tracker: JumpTracker::new(), reports: Vec::new() }
    }
    fn press(&mut self, b: Buttons, yaw: f32) {
        let before = self.sim.state.clone();
        let cmd = UserCmd::from_buttons(0, Vec3::new(0.0, yaw, 0.0), b);
        let flags = self.sim.step(cmd);
        let (g, dt) = (self.sim.cfg.gravity, self.sim.dt);
        if let Some(r) = self.tracker.tick(&self.sim.world, &before, &self.sim.state, &cmd, flags, g, dt) {
            self.reports.push(r.clone());
        }
    }
    fn hold(&mut self, b: Buttons, yaw: f32, n: u32) {
        for _ in 0..n {
            self.press(b, yaw);
        }
    }
}

#[test]
fn long_jump_report() {
    let mut t = Tracked::new(Sim::flat(ModeKind::Vanilla, 64));
    t.hold(Buttons::FORWARD, 0.0, 100);
    t.press(Buttons::JUMP, 0.0);
    let mut yaw = 0.0;
    for i in 0..60 {
        let left = (i / 12) % 2 == 0;
        yaw += if left { 1.0 } else { -1.0 };
        t.press(if left { Buttons::LEFT } else { Buttons::RIGHT }, yaw);
    }
    assert_eq!(t.reports.len(), 1);
    let r = &t.reports[0];
    assert_eq!(r.jump_type, JumpType::LongJump);
    // +32 reporting convention [Ref §16].
    approx(r.distance - r.distance_no_offset, DISTANCE_OFFSET, 1e-4);
    // Zigzag strafing curves the path, so the straight-line distance is under 250 * airtime.
    assert!(r.distance > 200.0 && r.distance < 280.0, "{}", r.distance);
    approx(r.pre_speed, 250.0, 1e-3);
    // The landing correction moves the point back by at most one tick of travel [Ref §16].
    let shift = r.landing_origin.sub(r.landing_origin_raw);
    assert!(shift.length_2d() <= r.max_speed * DT64 + 1e-3, "{shift:?}");
    // Grounding can happen up to 2 units above the floor; the corrected landing is on the floor.
    assert!(r.landing_origin_raw.z - r.landing_origin.z >= 0.0 && r.landing_origin_raw.z - r.landing_origin.z <= 2.0);
    approx(r.block_height, 0.0, 1e-3);
    assert!(r.airtime_ticks > 40 && r.airtime_ticks < 55, "{}", r.airtime_ticks);
    assert!(r.strafes.len() >= 4);
    assert!(r.sync > 0.0 && r.sync <= 100.0);
    approx(r.height, 54.653_766, 1e-3);
    assert!(r.max_speed > 250.0);
}

#[test]
fn bhop_and_multibhop_classification() {
    let mut t = Tracked::new(Sim::flat(ModeKind::Vanilla, 64));
    t.sim.cfg.autobhop = true;
    t.hold(Buttons::FORWARD, 0.0, 100);
    for _ in 0..200 {
        t.press(Buttons::JUMP, 0.0);
    }
    let types: Vec<JumpType> = t.reports.iter().map(|r| r.jump_type).collect();
    assert!(types.len() >= 3, "{types:?}");
    assert_eq!(types[0], JumpType::LongJump);
    assert_eq!(types[1], JumpType::Bhop);
    assert!(types[2..].iter().all(|t| *t == JumpType::MultiBhop), "{types:?}");
    assert!(t.reports[1].perfect);
}

#[test]
fn weird_jump_after_drop() {
    let mut world = PrimitiveWorld::flat_floor(0.0);
    world.add(Brush::cuboid(Vec3::new(-512.0, -256.0, 0.0), Vec3::new(0.0, 256.0, 32.0)));
    let mut sim = Sim::new(world, ModeKind::Vanilla, 64, Vec3::new(-100.0, 0.0, 32.031_25));
    sim.cfg.autobhop = true;
    sim.idle(4);
    let mut t = Tracked::new(sim);
    // Walk off the ledge (a fall), then hold jump: autobhop hops on the first grounded command.
    while t.sim.state.on_ground() {
        t.press(Buttons::FORWARD, 0.0);
    }
    for _ in 0..120 {
        t.press(Buttons::FORWARD | Buttons::JUMP, 0.0);
    }
    assert!(!t.reports.is_empty());
    assert_eq!(t.reports[0].jump_type, JumpType::WeirdJump);
}

#[test]
fn standing_jump_in_place_has_zero_distance_plus_offset() {
    let mut t = Tracked::new(Sim::flat(ModeKind::Vanilla, 64));
    t.press(Buttons::JUMP, 0.0);
    t.hold(Buttons::NONE, 0.0, 80);
    assert_eq!(t.reports.len(), 1);
    approx(t.reports[0].distance, DISTANCE_OFFSET, 1e-4);
    assert_eq!(t.reports[0].sync, 0.0);
}

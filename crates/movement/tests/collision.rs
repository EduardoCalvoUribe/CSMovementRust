//! M3 gate: walls, creases, steps, slopes, and no tunneling.

mod common;
use common::*;
use movement::trace::DIST_EPSILON;
use movement::world::{Brush, RiseDir};
use movement::*;

fn floor_with(brushes: Vec<Brush>) -> PrimitiveWorld {
    let mut w = PrimitiveWorld::flat_floor(0.0);
    for b in brushes {
        w.add(b);
    }
    w
}

fn grounded_sim(world: PrimitiveWorld, origin: Vec3) -> Sim<PrimitiveWorld> {
    let mut s = Sim::new(world, ModeKind::Vanilla, 64, origin);
    s.idle(8);
    s
}

#[test]
fn slides_along_wall_on_ground() {
    // S10 / [Ref §12.2]: the into-wall component is removed, the tangential part kept.
    let wall = Brush::cuboid(Vec3::new(64.0, -1024.0, 0.0), Vec3::new(96.0, 1024.0, 256.0));
    let mut s = grounded_sim(floor_with(vec![wall]), Vec3::new(0.0, 0.0, DIST_EPSILON));
    s.hold(Buttons::FORWARD, 45.0, 60);
    assert!(s.state.origin.x <= 48.0, "penetrated: {:?}", s.state.origin);
    assert!(s.state.origin.x > 47.9);
    assert!(s.state.velocity.x.abs() < 1e-3, "{:?}", s.state.velocity);
    assert!(s.state.velocity.y > 100.0);
}

#[test]
fn slides_along_wall_in_air() {
    let wall = Brush::cuboid(Vec3::new(64.0, -1024.0, 0.0), Vec3::new(96.0, 1024.0, 256.0));
    let mut s = Sim::new(floor_with(vec![wall]), ModeKind::Vanilla, 64, Vec3::new(0.0, 0.0, 100.0));
    s.state.velocity = Vec3::new(300.0, 200.0, 0.0);
    for _ in 0..20 {
        s.idle(1);
        assert!(s.state.origin.x <= 48.0);
    }
    assert_eq!(s.state.velocity.x, 0.0);
    approx(s.state.velocity.y, 200.0, 1e-3);
}

#[test]
fn slides_along_crease() {
    // S11 / [Ref §12.2]: a steep ramp and a wall; motion is confined to their crease.
    let ramp = Brush::ramp(Vec3::new(100.0, -1024.0, 0.0), 200.0, 2048.0, 60.0, RiseDir::PosX);
    let wall = Brush::cuboid(Vec3::new(-1024.0, 64.0, 0.0), Vec3::new(1024.0, 96.0, 1024.0));
    let mut s = Sim::new(floor_with(vec![ramp.clone(), wall]), ModeKind::Vanilla, 64, Vec3::new(60.0, 30.0, 40.0));
    s.state.velocity = Vec3::new(400.0, 300.0, 0.0);
    let n = ramp.planes.last().unwrap().normal;
    let mut confined = 0;
    for _ in 0..12 {
        s.idle(1);
        assert!(!s.world.point_solid(s.state.origin, s.state.hull()));
        let o = s.state.origin;
        if o.y > 47.0 && s.state.velocity.length() > 1.0 {
            assert!(s.state.velocity.y.abs() < 1e-3, "{:?}", s.state.velocity);
            // Against the ramp too: no velocity into it.
            if s.state.velocity.dot(n) < 1e-2 && o.x > 100.0 {
                confined += 1;
            }
        }
    }
    assert!(confined > 0, "never slid along the crease");
}

fn run_at_step(height: f32) -> PlayerState {
    let step = Brush::cuboid(Vec3::new(64.0, -256.0, 0.0), Vec3::new(400.0, 256.0, height));
    let mut s = grounded_sim(floor_with(vec![step]), Vec3::new(0.0, 0.0, DIST_EPSILON));
    s.hold(Buttons::FORWARD, 0.0, 40);
    s.state
}

#[test]
fn steps_up_18_but_not_19() {
    // S12 / [Ref §12.3]: step size 18.
    for h in [16.0, 18.0] {
        let st = run_at_step(h);
        assert!(st.origin.x > 80.0, "h={h}: {:?}", st.origin);
        approx(st.origin.z, h + DIST_EPSILON, 1e-3);
        assert!(st.on_ground());
    }
    let st = run_at_step(19.0);
    assert!(st.origin.x <= 48.0);
    approx(st.origin.z, DIST_EPSILON, 1e-4);
}

#[test]
fn walks_down_stairs_staying_grounded() {
    // StayOnGround snaps down within a step.
    let step = Brush::cuboid(Vec3::new(-256.0, -256.0, 0.0), Vec3::new(64.0, 256.0, 16.0));
    let mut s = grounded_sim(floor_with(vec![step]), Vec3::new(0.0, 0.0, 16.0 + DIST_EPSILON));
    for _ in 0..40 {
        s.press(Buttons::FORWARD, 0.0);
        assert!(s.state.on_ground(), "{:?}", s.state.origin);
    }
    approx(s.state.origin.z, DIST_EPSILON, 1e-3);
}

fn on_slope(angle: f32) -> Sim<PrimitiveWorld> {
    let ramp = Brush::ramp(Vec3::new(0.0, -1024.0, 0.0), 600.0, 2048.0, angle, RiseDir::PosX);
    let top = 300.0 * movement::math::deg2rad(angle).tan() + 40.0;
    Sim::new(floor_with(vec![ramp]), ModeKind::Vanilla, 64, Vec3::new(300.0, 0.0, top))
}

#[test]
fn stands_on_slope_below_threshold() {
    // S13 / [Ref §12.4, §13.1]: 40 deg has n_z = 0.766 >= 0.7, walkable.
    let mut s = on_slope(40.0);
    s.idle(60);
    assert!(s.state.on_ground());
    let o = s.state.origin;
    s.idle(60);
    assert!(s.state.origin.sub(o).length() < 1e-3, "drifted on a walkable slope");
}

#[test]
fn slides_off_slope_above_threshold() {
    // [Ref §13.1]: 50 deg has n_z = 0.643 < 0.7; the player stays airborne and slides downhill.
    let mut s = on_slope(50.0);
    s.idle(60);
    assert!(!s.state.on_ground());
    assert!(s.state.velocity.x < -50.0, "{:?}", s.state.velocity);
    assert!(!s.world.point_solid(s.state.origin, s.state.hull()));
}

#[test]
fn threshold_angles_45_and_46() {
    // arccos(0.7) = 45.573 deg [Ref §13.1].
    let mut s = on_slope(45.0);
    s.idle(60);
    assert!(s.state.on_ground());
    let mut s = on_slope(46.0);
    s.idle(60);
    assert!(!s.state.on_ground());
}

mod props {
    use super::*;
    use proptest::prelude::*;

    fn arb_box() -> impl Strategy<Value = Brush> {
        (-256.0f32..256.0, -256.0f32..256.0, 0.0f32..200.0, 8.0f32..200.0, 8.0f32..200.0, 8.0f32..120.0)
            .prop_map(|(x, y, z, sx, sy, sz)| Brush::cuboid(Vec3::new(x, y, z), Vec3::new(x + sx, y + sy, z + sz)))
    }

    fn arb_ramp() -> impl Strategy<Value = Brush> {
        (-256.0f32..256.0, -256.0f32..256.0, 20.0f32..200.0, 10.0f32..70.0, 0usize..4).prop_map(|(x, y, run, a, d)| {
            let dir = [RiseDir::PosX, RiseDir::NegX, RiseDir::PosY, RiseDir::NegY][d];
            Brush::ramp(Vec3::new(x, y, 0.0), run, 96.0, a, dir)
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(96))]

        /// Plan §11: never inside a brush, never NaN, component limit respected, and deterministic.
        #[test]
        fn no_tunneling(
            boxes in prop::collection::vec(arb_box(), 0..6),
            ramps in prop::collection::vec(arb_ramp(), 0..3),
            start in (-300.0f32..300.0, -300.0f32..300.0, 1.0f32..300.0),
            vel in (-3000.0f32..3000.0, -3000.0f32..3000.0, -3000.0f32..3000.0),
            inputs in prop::collection::vec((0u32..256, -180.0f32..180.0, -89.0f32..89.0), 48),
        ) {
            let mut world = PrimitiveWorld::flat_floor(0.0);
            for b in boxes.into_iter().chain(ramps) {
                world.add(b);
            }
            let origin = Vec3::new(start.0, start.1, start.2);
            prop_assume!(!world.point_solid(origin, Hull::STAND));

            let cmds: Vec<UserCmd> = inputs
                .iter()
                .enumerate()
                .map(|(i, (b, yaw, pitch))| UserCmd::from_buttons(i as u32, Vec3::new(*pitch, *yaw, 0.0), Buttons(*b)))
                .collect();

            let run = |world: &PrimitiveWorld| {
                let mut s = Sim::new(world.clone(), ModeKind::Vanilla, 64, origin);
                s.state.velocity = Vec3::new(vel.0, vel.1, vel.2);
                let mut states = Vec::new();
                for c in &cmds {
                    s.step(*c);
                    let st = &s.state;
                    assert!(st.origin.is_finite() && st.velocity.is_finite());
                    for i in 0..3 {
                        assert!(st.velocity.get(i).abs() <= s.cfg.max_component_velocity);
                    }
                    assert!(!s.world.point_solid(st.origin, st.hull()), "inside solid at {:?} ducked={}", st.origin, st.ducked);
                    states.push(st.clone());
                }
                states
            };
            let a = run(&world);
            let b = run(&world);
            prop_assert_eq!(a, b);
        }

        /// The swept trace agrees with sampling the same segment at fine steps: no sample before the
        /// reported fraction is inside solid.
        #[test]
        fn trace_agrees_with_sampling(
            boxes in prop::collection::vec(arb_box(), 1..5),
            start in (-300.0f32..300.0, -300.0f32..300.0, 1.0f32..300.0),
            delta in (-400.0f32..400.0, -400.0f32..400.0, -400.0f32..400.0),
        ) {
            let mut world = PrimitiveWorld::new();
            for b in boxes {
                world.add(b);
            }
            let a = Vec3::new(start.0, start.1, start.2);
            prop_assume!(!world.point_solid(a, Hull::STAND));
            let b = a.add(Vec3::new(delta.0, delta.1, delta.2));
            let tr = world.trace_hull(a, b, Hull::STAND);
            prop_assert!(!world.point_solid(tr.end_pos, Hull::STAND));
            let n = 200;
            for i in 0..n {
                let t = tr.fraction * i as f32 / n as f32;
                let p = a.add(b.sub(a).scale(t));
                prop_assert!(!world.point_solid(p, Hull::STAND), "solid at t={} < fraction {}", t, tr.fraction);
            }
            if tr.fraction < 1.0 {
                // Pushed through the reported plane by twice the epsilon gap, the hull is in solid.
                let p = tr.end_pos.sub(tr.plane_normal.scale(2.0 * movement::trace::DIST_EPSILON));
                prop_assert!(world.point_solid(p, Hull::STAND), "plane {:?} not solid behind", tr.plane_normal);
            }
        }
    }
}

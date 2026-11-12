//! R4 gate (docs/reinforcement-plan.md §11): ray-grid hits agree with brute-force sampling on the
//! primitive world, classes included; the observation spec matches what is written.

use std::sync::Arc;

use course::{StartSpec, Course, CourseMeta, EndSpec, Zone};
use env::{ClassEncoding, Env, EnvConfig, GoalVector, ObsProvider, Proprioception, RayGrid, SurfaceClass};
use movement::trace::DIST_EPSILON;
use movement::world::{Brush, Contents, PrimitiveWorld, RiseDir, World};
use movement::{PlayerState, Vec3};

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

/// Floor, boxes, a walkable 30-degree ramp, a steep 60-degree ramp, a ceiling slab, a ladder, plus a
/// goal and a hazard volume.
fn scene() -> (Course, Vec<Brush>, Zone, Zone) {
    let brushes = vec![
        Brush::cuboid(v(-2048.0, -2048.0, -64.0), v(2048.0, 2048.0, 0.0)),
        Brush::cuboid(v(200.0, -100.0, 0.0), v(300.0, 100.0, 120.0)),
        Brush::cuboid(v(-400.0, 300.0, 0.0), v(-300.0, 340.0, 400.0)),
        Brush::ramp(v(0.0, 400.0, 0.0), 256.0, 200.0, 30.0, RiseDir::PosX),
        Brush::ramp(v(-600.0, -500.0, 0.0), 128.0, 300.0, 60.0, RiseDir::NegX),
        Brush::cuboid(v(-200.0, -300.0, 260.0), v(200.0, 0.0, 300.0)),
        Brush::cuboid(v(500.0, -40.0, 0.0), v(504.0, 40.0, 300.0)).with_contents(Contents::Ladder),
    ];
    let mut w = PrimitiveWorld::new();
    for b in &brushes {
        w.add(b.clone());
    }
    let goal = Zone::cuboid(v(700.0, 200.0, 0.0), v(800.0, 300.0, 128.0));
    let hazard = Zone::cuboid(v(-700.0, 400.0, 0.0), v(-600.0, 500.0, 64.0));
    let course = Course {
        world: World::Primitive(w),
        spawn: (v(0.0, 0.0, DIST_EPSILON), 0.0),
        start: StartSpec::Zone(Zone::cuboid(v(-32.0, -32.0, -8.0), v(32.0, 32.0, 128.0))),
        end: EndSpec::Touch(goal.clone()),
        hazards: vec![hazard.clone()],
        kill_z: -256.0,
        waypoints: Vec::new(),
        meta: CourseMeta { name: "rays".into() },
    };
    (course, brushes, goal, hazard)
}

fn inside_brush(b: &Brush, p: Vec3) -> bool {
    p.x >= b.mins.x - 0.01
        && p.x <= b.maxs.x + 0.01
        && p.y >= b.mins.y - 0.01
        && p.y <= b.maxs.y + 0.01
        && p.z >= b.mins.z - 0.01
        && p.z <= b.maxs.z + 0.01
        && b.planes.iter().all(|pl| p.dot(pl.normal) - pl.dist <= 0.0)
}

fn inside_box(lo: Vec3, hi: Vec3, p: Vec3) -> bool {
    p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y && p.z >= lo.z && p.z <= hi.z
}

#[derive(Debug)]
enum Hit {
    Brush(usize),
    Goal,
    Hazard,
}

/// What, if anything, contains `p`. Solid wins over zones: zones are seen through only up to solid.
fn what_at(brushes: &[Brush], goal: (Vec3, Vec3), hazard: (Vec3, Vec3), p: Vec3) -> Option<Hit> {
    if let Some(i) = brushes.iter().position(|b| inside_brush(b, p)) {
        return Some(Hit::Brush(i));
    }
    if inside_box(goal.0, goal.1, p) {
        return Some(Hit::Goal);
    }
    if inside_box(hazard.0, hazard.1, p) {
        return Some(Hit::Hazard);
    }
    None
}

/// Brute force: march in 1-unit steps, then bisect the first step that enters something.
fn brute(brushes: &[Brush], goal: (Vec3, Vec3), hazard: (Vec3, Vec3), eye: Vec3, dir: Vec3, range: f32) -> Option<(f32, Hit)> {
    let at = |t: f32| eye.ma(t, dir);
    let mut prev = 0.0;
    let mut t = 1.0;
    while t <= range {
        if what_at(brushes, goal, hazard, at(t)).is_some() {
            let (mut lo, mut hi) = (prev, t);
            for _ in 0..30 {
                let mid = 0.5 * (lo + hi);
                if what_at(brushes, goal, hazard, at(mid)).is_some() {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            return Some((hi, what_at(brushes, goal, hazard, at(hi)).unwrap()));
        }
        prev = t;
        t += 1.0;
    }
    None
}

/// The face of `b` the point `p` (on its surface) lies on, and how clearly: the gap to the next face.
fn face(b: &Brush, p: Vec3) -> (Vec3, f32) {
    let mut d: Vec<(f32, Vec3)> = b.planes.iter().map(|pl| (p.dot(pl.normal) - pl.dist, pl.normal)).collect();
    d.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    (d[0].1, d[0].0 - d[1].0)
}

/// |cos| between `dir` and the face of the box `b` that `p` lies on, or `None` near an edge.
fn box_face_cos(b: (Vec3, Vec3), p: Vec3, dir: Vec3) -> Option<f32> {
    let mut d = [
        ((p.x - b.0.x).abs().min((p.x - b.1.x).abs()), dir.x.abs()),
        ((p.y - b.0.y).abs().min((p.y - b.1.y).abs()), dir.y.abs()),
        ((p.z - b.0.z).abs().min((p.z - b.1.z).abs()), dir.z.abs()),
    ];
    d.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    (d[1].0 - d[0].0 >= 0.5).then_some(d[0].1)
}

/// 64-bit LCG in [0, 1).
struct Lcg(u64);
impl Lcg {
    fn f(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[test]
fn rays_agree_with_brute_force_sampling() {
    let (course, brushes, goal, hazard) = scene();
    let (gb, hb) = (goal.bounds().unwrap(), hazard.bounds().unwrap());
    let grid = RayGrid { rows: 7, cols: 11, h_fov: 300.0, v_fov: 150.0, max_distance: 1500.0, ..RayGrid::default() };
    let mut rng = Lcg(42);
    let (mut checked, mut classes_seen) = (0, std::collections::HashSet::new());
    for _ in 0..60 {
        let mut state = PlayerState::new(v(rng.f() * 1400.0 - 700.0, rng.f() * 1400.0 - 700.0, DIST_EPSILON));
        state.duck_amount = rng.f();
        let eye = RayGrid::eye(&state);
        if what_at(&brushes, gb, hb, eye).is_some() {
            continue;
        }
        let view = v(rng.f() * 120.0 - 60.0, rng.f() * 360.0, 0.0);
        for (dir, hit) in grid.directions(view).into_iter().zip(grid.cast_all(&course, &state, view)) {
            let expect = brute(&brushes, gb, hb, eye, dir, grid.max_distance);
            let Some((t, what)) = expect else {
                assert_eq!(hit.class, SurfaceClass::None, "eye {eye:?} dir {dir:?}");
                assert_eq!(hit.distance, 1.0);
                checked += 1;
                continue;
            };
            let got = hit.distance * grid.max_distance;
            let p = eye.ma(t, dir);
            let (class, cos) = match what {
                Hit::Goal => (SurfaceClass::Goal, box_face_cos(gb, p, dir)),
                Hit::Hazard => (SurfaceClass::Hazard, box_face_cos(hb, p, dir)),
                Hit::Brush(i) => {
                    let (n, clarity) = face(&brushes[i], p);
                    if clarity < 0.5 {
                        continue; // on an edge: either face is right
                    }
                    let class = if brushes[i].contents == Contents::Ladder {
                        SurfaceClass::Ladder
                    } else {
                        SurfaceClass::from_normal(n)
                    };
                    (class, Some(n.dot(dir).abs()))
                }
            };
            // The trace stops DIST_EPSILON short along the face normal. Edges of zone boxes are skipped.
            let Some(cos) = cos else { continue };
            if cos < 0.1 {
                continue; // grazing: the epsilon stretches without bound
            }
            let tol = DIST_EPSILON / cos + 0.01;
            assert!(got <= t + 0.01 && got >= t - tol, "eye {eye:?} dir {dir:?}: trace {got} vs brute {t} ({what:?})");
            assert_eq!(hit.class, class, "eye {eye:?} dir {dir:?} at {t}");
            classes_seen.insert(class);
            checked += 1;
        }
    }
    assert!(checked > 2500, "only {checked} rays checked");
    for c in [
        SurfaceClass::Floor,
        SurfaceClass::Steep,
        SurfaceClass::Wall,
        SurfaceClass::Ceiling,
        SurfaceClass::Ladder,
        SurfaceClass::Goal,
        SurfaceClass::Hazard,
    ] {
        assert!(classes_seen.contains(&c), "scene never produced {c:?}");
    }
}

#[test]
fn straight_down_hits_the_floor_at_eye_height() {
    let (course, ..) = scene();
    let grid = RayGrid { rows: 1, cols: 1, ..RayGrid::default() };
    let mut state = PlayerState::new(v(-1000.0, -1000.0, DIST_EPSILON));
    let hit = grid.cast_all(&course, &state, v(89.9, 0.0, 0.0))[0];
    assert_eq!(hit.class, SurfaceClass::Floor);
    let d = hit.distance * grid.max_distance;
    let expect = (64.0 + DIST_EPSILON) / 89.9f32.to_radians().sin();
    assert!((d - expect).abs() < 0.05, "{d} vs {expect}");
    // Fully ducked: 46.
    state.duck_amount = 1.0;
    let d = grid.cast_all(&course, &state, v(89.9, 0.0, 0.0))[0].distance * grid.max_distance;
    assert!((d - (46.0 + DIST_EPSILON) / 89.9f32.to_radians().sin()).abs() < 0.05, "{d}");
}

#[test]
fn grid_layout_is_top_left_first() {
    let grid = RayGrid { rows: 3, cols: 3, h_fov: 90.0, v_fov: 60.0, ..RayGrid::default() };
    let d = grid.directions(v(0.0, 0.0, 0.0));
    // Top-left looks up (+z) and left (+y); bottom-right looks down and right.
    assert!(d[0].z > 0.0 && d[0].y > 0.0, "{:?}", d[0]);
    assert!(d[8].z < 0.0 && d[8].y < 0.0, "{:?}", d[8]);
    assert!((d[4].x - 1.0).abs() < 1e-6);
    // Level grid ignores view pitch.
    let level = RayGrid { follow_pitch: false, ..grid };
    assert_eq!(level.directions(v(45.0, 0.0, 0.0)), grid.directions(v(0.0, 0.0, 0.0)));
}

#[test]
fn spec_matches_what_is_written() {
    let (course, ..) = scene();
    for encoding in [ClassEncoding::OneHot, ClassEncoding::Index] {
        let grid = RayGrid { rows: 4, cols: 6, encoding, ..RayGrid::default() };
        let spec = grid.spec();
        assert_eq!(spec.shape, vec![4, 6, grid.channels()]);
        assert_eq!(spec.fields.len(), grid.channels());
        let env = Env::new(Arc::new(course.clone()), EnvConfig::default()).unwrap();
        let mut env = env.with_providers(vec![Box::new(Proprioception), Box::new(GoalVector), Box::new(grid)]);
        let obs = env.reset(0);
        assert_eq!(obs.len(), env.spec().len());
        let rays = env.spec().block(&obs, "rays").unwrap();
        assert_eq!(rays.len(), 4 * 6 * grid.channels());
        let hits = grid.cast_all(&course, env.state(), env.view_angles());
        for (cell, hit) in rays.chunks(grid.channels()).zip(&hits) {
            assert_eq!(cell[0], hit.distance);
            match encoding {
                ClassEncoding::OneHot => {
                    assert_eq!(cell[1..].iter().sum::<f32>(), 1.0);
                    assert_eq!(cell[1 + hit.class as usize], 1.0);
                }
                ClassEncoding::Index => assert_eq!(cell[1], hit.class as u8 as f32),
            }
        }
    }
}

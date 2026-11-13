//! Throughput measurement (plan §7), not a gate. Run with
//! `cargo test --release -p env --test throughput -- --ignored --nocapture`.

use std::sync::Arc;
use std::time::Instant;

use course::{StartSpec, Course, CourseMeta, EndSpec, Zone};
use env::{Action, Env, EnvConfig};
use movement::trace::DIST_EPSILON;
use movement::world::{Brush, PrimitiveWorld, World};
use movement::Vec3;

#[test]
#[ignore]
fn single_env_steps_per_second() {
    let mut w = PrimitiveWorld::flat_floor(0.0);
    w.add(Brush::cuboid(Vec3::new(150.0, -100.0, 0.0), Vec3::new(250.0, 100.0, 40.0)));
    let course = Course {
        world: World::Primitive(w),
        spawn: (Vec3::new(0.0, 0.0, DIST_EPSILON), 0.0),
        start: StartSpec::Zone(Zone::cuboid(Vec3::new(-64.0, -64.0, -8.0), Vec3::new(64.0, 64.0, 256.0))),
        end: EndSpec::Touch(Zone::cuboid(Vec3::new(5000.0, -64.0, -8.0), Vec3::new(5100.0, 64.0, 256.0))),
        hazards: Vec::new(),
        kill_z: -256.0,
        waypoints: Vec::new(),
        teleports: Vec::new(),
        meta: CourseMeta { name: "bench".into() },
    };
    let mut env = Env::new(Arc::new(course), EnvConfig { max_episode_ticks: 2000, ..EnvConfig::default() }).unwrap();
    let mut obs = vec![0.0; env.spec().len()];
    env.reset_into(0, &mut obs);
    let n = 2_000_000u32;
    let t0 = Instant::now();
    for i in 0..n {
        let a = Action { forward: 1, side: if i % 64 < 32 { 1 } else { -1 }, jump: i % 50 == 0, yaw_delta: 1.5, ..Action::IDLE };
        let info = env.step_into(a, &mut obs);
        if info.terminated || info.truncated {
            env.reset_into(0, &mut obs);
        }
    }
    let s = t0.elapsed().as_secs_f64();
    println!("THROUGHPUT {n} steps in {s:.2}s = {:.0} steps/s (one core, primitive world, 2 brushes)", n as f64 / s);
}

/// Default observation plus the default ray grid (8 x 16 = 128 rays) on a toy course.
#[test]
#[ignore]
fn single_env_with_rays_steps_per_second() {
    use course::toy::{generate, Stage};
    use env::{GoalVector, Proprioception, RayGrid};
    let toy = generate(Stage::Heights, 1);
    let brushes = match &toy.course.world {
        World::Primitive(w) => w.brushes.len(),
        _ => unreachable!(),
    };
    for (name, grid) in [("default 8x16", RayGrid::default()), ("4x8", RayGrid { rows: 4, cols: 8, ..RayGrid::default() })] {
        let mut env = Env::new(Arc::new(toy.course.clone()), EnvConfig::default())
            .unwrap()
            .with_providers(vec![Box::new(Proprioception), Box::new(GoalVector), Box::new(grid)]);
        let mut obs = vec![0.0; env.spec().len()];
        env.reset_into(0, &mut obs);
        let n = 100_000u32;
        let t0 = Instant::now();
        let mut i = 0usize;
        for _ in 0..n {
            let c = &toy.solution[i];
            let a = Action { forward: 1, jump: c.buttons.contains(movement::Buttons::JUMP), ..Action::IDLE };
            let info = env.step_into(a, &mut obs);
            i += 1;
            if info.terminated || info.truncated || i == toy.solution.len() {
                env.reset_into(0, &mut obs);
                i = 0;
            }
        }
        let s = t0.elapsed().as_secs_f64();
        println!("THROUGHPUT rays {name}: {:.0} steps/s ({brushes} brushes)", n as f64 / s);
    }
}

/// `VecEnv` total steps/s by env count, without and with the default ray grid.
#[test]
#[ignore]
fn vec_env_steps_per_second() {
    use course::toy::{generate, Stage};
    use env::{GoalVector, ObsProvider, Proprioception, RayGrid, VecEnv};
    let course = Arc::new(generate(Stage::Heights, 1).course);
    for rays in [false, true] {
        for n in [1usize, 4, 8, 16, 64] {
            let make = || {
                let mut p: Vec<Box<dyn ObsProvider>> = vec![Box::new(Proprioception), Box::new(GoalVector)];
                if rays {
                    p.push(Box::new(RayGrid::default()));
                }
                Env::new(course.clone(), EnvConfig { max_episode_ticks: 1000, ..EnvConfig::default() }).unwrap().with_providers(p)
            };
            let mut v = VecEnv::new((0..n).map(|_| make()).collect()).unwrap();
            v.reset(0);
            let steps = if rays { 400_000 } else { 4_000_000 } / n;
            let acts: Vec<Action> = (0..n).map(|i| Action { forward: 1, jump: i % 2 == 0, yaw_delta: 0.5, ..Action::IDLE }).collect();
            let t0 = Instant::now();
            for _ in 0..steps {
                v.step(&acts);
            }
            let s = t0.elapsed().as_secs_f64();
            println!("THROUGHPUT vec rays={rays} n={n}: {:.0} env-steps/s", (steps * n) as f64 / s);
        }
    }
}

//! Throughput measurement (plan §7), not a gate. Run with
//! `cargo test --release -p env --test throughput -- --ignored --nocapture`.

use std::sync::Arc;
use std::time::Instant;

use course::{Course, CourseMeta, EndSpec, Zone};
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
        start_zone: Zone::cuboid(Vec3::new(-64.0, -64.0, -8.0), Vec3::new(64.0, 64.0, 256.0)),
        end: EndSpec::Touch(Zone::cuboid(Vec3::new(5000.0, -64.0, -8.0), Vec3::new(5100.0, 64.0, 256.0))),
        hazards: Vec::new(),
        kill_z: -256.0,
        waypoints: Vec::new(),
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

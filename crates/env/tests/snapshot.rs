//! R2 gate (docs/reinforcement-plan.md §11): restore then continue equals running straight through, for
//! all three modes, including the exported recording.

use std::sync::Arc;

use course::{Course, CourseMeta, EndSpec, Zone};
use env::{Action, Env, EnvConfig};
use movement::replay::state_fields;
use movement::trace::DIST_EPSILON;
use movement::world::{Brush, PrimitiveWorld, World};
use movement::{ModeKind, Vec3};

fn course() -> Arc<Course> {
    let mut w = PrimitiveWorld::flat_floor(0.0);
    w.add(Brush::cuboid(Vec3::new(150.0, -100.0, 0.0), Vec3::new(250.0, 100.0, 40.0)));
    Arc::new(Course {
        world: World::Primitive(w),
        spawn: (Vec3::new(0.0, 0.0, DIST_EPSILON), 0.0),
        start_zone: Zone::cuboid(Vec3::new(-64.0, -64.0, -8.0), Vec3::new(64.0, 64.0, 256.0)),
        end: EndSpec::Touch(Zone::cuboid(Vec3::new(3000.0, -64.0, -8.0), Vec3::new(3100.0, 64.0, 256.0))),
        hazards: Vec::new(),
        kill_z: -256.0,
        meta: CourseMeta { name: "snapshot".into() },
    })
}

/// Strafe-and-turn actions with occasional jumps and ducks (64-bit LCG).
fn actions(seed: u64, n: u32) -> Vec<Action> {
    let mut x = seed;
    let mut next = move || {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (x >> 33) as u32
    };
    (0..n)
        .map(|i| {
            let r = next();
            let right = (i / 40) % 2 == 0;
            Action {
                forward: 1,
                side: if right { 1 } else { -1 },
                jump: r % 37 == 0,
                duck: r % 53 == 0,
                use_: false,
                yaw_delta: if right { -1.5 } else { 1.5 } + (next() % 100) as f32 / 100.0 - 0.5,
                pitch_delta: 0.0,
            }
        })
        .collect()
}

type Trace = Vec<(Vec<String>, Vec<u32>, u32)>;

fn step_all(env: &mut Env, acts: &[Action]) -> Trace {
    acts.iter()
        .map_while(|a| {
            if env.track().episode_ticks >= env.config().max_episode_ticks || env.run().status.is_terminal() {
                return None;
            }
            let s = env.step(*a);
            Some((state_fields(env.state()), s.obs.iter().map(|f| f.to_bits()).collect(), s.info.reward.to_bits()))
        })
        .collect()
}

#[test]
fn restore_then_continue_equals_straight_through() {
    let c = course();
    let all = actions(5, 1000);
    for mode in [ModeKind::Vanilla, ModeKind::KzTimer, ModeKind::SimpleKz] {
        let cfg = EnvConfig { mode, ..EnvConfig::default() };
        let mut straight = Env::new(c.clone(), cfg).unwrap();
        straight.reset(3);
        let reference = step_all(&mut straight, &all);
        assert_eq!(reference.len(), all.len(), "{mode:?}: episode ended early");

        for cut in [0usize, 1, 250, 777, 999] {
            let mut env = Env::new(c.clone(), cfg).unwrap();
            env.reset(3);
            step_all(&mut env, &all[..cut]);
            let snap = env.snapshot();
            assert_eq!(snap.episode_ticks() as usize, cut);

            // Wander off, then restore into the same env; and separately into a fresh env.
            step_all(&mut env, &actions(77, 200));
            env.restore(&snap).unwrap();
            let rest = step_all(&mut env, &all[cut..]);
            assert_eq!(rest, reference[cut..], "{mode:?} cut {cut}: restore into same env");
            assert_eq!(env.recording(), straight.recording(), "{mode:?} cut {cut}: recording");

            let mut fresh = Env::new(c.clone(), cfg).unwrap();
            fresh.reset(9);
            fresh.restore(&snap).unwrap();
            assert_eq!(fresh.seed(), 3);
            let rest = step_all(&mut fresh, &all[cut..]);
            assert_eq!(rest, reference[cut..], "{mode:?} cut {cut}: restore into fresh env");

            // A restored snapshot can be restored again: restoring does not consume it.
            fresh.restore(&snap).unwrap();
            assert_eq!(step_all(&mut fresh, &all[cut..]), reference[cut..]);
        }
    }
}

#[test]
fn restoring_into_a_mismatched_env_is_an_error() {
    let c = course();
    let mut kz = Env::new(c.clone(), EnvConfig::default()).unwrap();
    kz.reset(0);
    let snap = kz.snapshot();
    let mut vanilla = Env::new(c.clone(), EnvConfig { mode: ModeKind::Vanilla, ..EnvConfig::default() }).unwrap();
    assert!(vanilla.restore(&snap).is_err());
    let mut other_course = Env::new(course(), EnvConfig::default()).unwrap();
    assert!(other_course.restore(&snap).is_err());
}

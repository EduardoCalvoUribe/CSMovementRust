//! R1 gate (docs/reinforcement-plan.md §11): determinism, bit-exact replay of the exported `Recording`,
//! and agreement with R0's scripted walk. SimpleKZ at 128 Hz.

use std::sync::Arc;

use course::{StartSpec, Course, CourseMeta, EndSpec, Status, Zone};
use env::{Action, BaseReward, Env, EnvConfig};
use movement::replay::{state_fields, Recording};
use movement::trace::DIST_EPSILON;
use movement::world::{Brush, PrimitiveWorld, World};
use movement::{ModeKind, Vec3};

fn zone(min: (f32, f32, f32), max: (f32, f32, f32)) -> Zone {
    Zone::cuboid(Vec3::new(min.0, min.1, min.2), Vec3::new(max.0, max.1, max.2))
}

/// Same course as the R0 gate (`crates/course/tests/run.rs`).
fn flat_course() -> Course {
    Course {
        world: World::Primitive(PrimitiveWorld::flat_floor(0.0)),
        spawn: (Vec3::new(0.0, 0.0, DIST_EPSILON), 0.0),
        start: StartSpec::Zone(zone((-64.0, -64.0, -8.0), (64.0, 64.0, 256.0))),
        end: EndSpec::Touch(zone((600.0, -64.0, -8.0), (700.0, 64.0, 256.0))),
        hazards: Vec::new(),
        kill_z: -256.0,
        waypoints: Vec::new(),
        teleports: Vec::new(),
        meta: CourseMeta { name: "flat".into() },
    }
}

/// A course with something to collide with: a block to jump onto and a wall, so the replay test
/// exercises steps, jumps, and clipping, not only flat ground.
fn bumpy_course() -> Course {
    let mut w = PrimitiveWorld::flat_floor(0.0);
    w.add(Brush::cuboid(Vec3::new(150.0, -100.0, 0.0), Vec3::new(250.0, 100.0, 40.0)));
    w.add(Brush::cuboid(Vec3::new(-300.0, 200.0, 0.0), Vec3::new(800.0, 232.0, 200.0)));
    Course { world: World::Primitive(w), ..flat_course() }
}

fn new_env(course: Course) -> Env {
    Env::new(Arc::new(course), EnvConfig::default()).unwrap()
}

/// Deterministic pseudo-random actions (64-bit LCG), so the tests need no RNG crate.
fn actions(seed: u64, n: usize) -> Vec<Action> {
    let mut x = seed;
    let mut next = move || {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (x >> 33) as u32
    };
    (0..n)
        .map(|_| {
            let r = next();
            Action {
                forward: (r % 3) as i8 - 1,
                side: ((r / 3) % 3) as i8 - 1,
                jump: (r / 9) % 5 == 0,
                duck: (r / 45) % 7 == 0,
                use_: false,
                yaw_delta: (next() % 2001) as f32 / 100.0 - 10.0,
                pitch_delta: (next() % 201) as f32 / 100.0 - 1.0,
            }
        })
        .collect()
}

fn bits(v: &[f32]) -> Vec<u32> {
    v.iter().map(|f| f.to_bits()).collect()
}

/// Run until done or the actions run out; returns per-step (state fields, obs bits, reward bits).
fn rollout(env: &mut Env, acts: &[Action]) -> Vec<(Vec<String>, Vec<u32>, u32)> {
    let obs = env.reset(7);
    let mut out = vec![(state_fields(env.state()), bits(&obs), 0)];
    for a in acts {
        let s = env.step(*a);
        out.push((state_fields(env.state()), bits(&s.obs), s.info.reward.to_bits()));
        if s.info.terminated || s.info.truncated {
            break;
        }
    }
    out
}

#[test]
fn env_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<Env>();
}

#[test]
fn same_actions_give_bit_identical_episodes() {
    let acts = actions(1, 1500);
    let a = rollout(&mut new_env(bumpy_course()), &acts);
    let b = rollout(&mut new_env(bumpy_course()), &acts);
    assert!(a.len() > 1000, "episode ran long enough to mean something ({})", a.len());
    assert_eq!(a, b);
    // Resetting the same env reproduces it too: no state leaks between episodes.
    let mut env = new_env(bumpy_course());
    let first = rollout(&mut env, &acts);
    let second = rollout(&mut env, &acts);
    assert_eq!(first, second);
}

#[test]
fn exported_recording_replays_bit_for_bit() {
    let course = bumpy_course();
    let mut env = new_env(course.clone());
    env.reset(0);
    let mut states = Vec::new();
    for a in actions(2, 2000) {
        let s = env.step(a);
        states.push(state_fields(env.state()));
        if s.info.terminated || s.info.truncated {
            break;
        }
    }
    let rec = env.recording().clone();
    assert_eq!(rec.cmds.len(), states.len());
    assert_eq!((rec.mode, rec.tickrate, rec.autobhop), (ModeKind::SimpleKz, 128, false));

    // Every intermediate state, not just the last.
    let mut replayed = Vec::new();
    let end = rec.replay_with(&course.world, &mut movement::NullObserver, |s| replayed.push(state_fields(s)));
    assert_eq!(replayed, states);
    assert_eq!(state_fields(&end), state_fields(env.state()));

    // And through the text format the viewer and capture rig read.
    let back = Recording::from_text(&rec.to_text()).unwrap();
    assert_eq!(back, rec);
    assert_eq!(state_fields(&back.replay(&course.world)), state_fields(env.state()));
}

#[test]
fn holding_forward_matches_the_r0_scripted_walk() {
    // R0 golden (`straight_walk_golden_tick_count`): command index 312 completes the run at 259 ticks.
    let mut env = new_env(flat_course());
    env.reset(0);
    let fwd = Action { forward: 1, ..Action::IDLE };
    let mut steps = 0;
    let info = loop {
        let s = env.step(fwd);
        steps += 1;
        if s.info.terminated || s.info.truncated {
            break s.info;
        }
    };
    assert_eq!(steps, 313);
    assert_eq!(info.status, Status::Completed);
    assert!(info.terminated && !info.truncated);
    assert_eq!(info.completed_ticks, Some(259));
    assert_eq!((info.run_ticks, info.episode_ticks), (259, 313));
}

#[test]
fn base_reward_is_bonus_minus_tick_cost() {
    let r = BaseReward::default();
    let mut env = new_env(flat_course());
    env.reset(0);
    let mut total = 0.0f32;
    let mut expected = 0.0f32;
    loop {
        let s = env.step(Action { forward: 1, ..Action::IDLE });
        total += s.info.reward;
        expected += -r.tick_cost + if s.info.status == Status::Completed { r.completion_bonus } else { 0.0 };
        if s.info.terminated {
            break;
        }
    }
    assert_eq!(total, expected);
}

#[test]
fn tick_limit_truncates_without_terminating() {
    let cfg = EnvConfig { max_episode_ticks: 100, ..EnvConfig::default() };
    let mut env = Env::new(Arc::new(flat_course()), cfg).unwrap();
    env.reset(0);
    for i in 1..=100 {
        let s = env.step(Action::IDLE);
        assert_eq!(s.info.truncated, i == 100);
        assert!(!s.info.terminated);
    }
}

#[test]
#[should_panic(expected = "call reset")]
fn stepping_a_finished_episode_panics() {
    let cfg = EnvConfig { max_episode_ticks: 1, ..EnvConfig::default() };
    let mut env = Env::new(Arc::new(flat_course()), cfg).unwrap();
    env.reset(0);
    env.step(Action::IDLE);
    env.step(Action::IDLE);
}

#[test]
fn action_repeat_equals_single_steps() {
    let acts = actions(3, 200);
    let mut single = new_env(bumpy_course());
    single.reset(0);
    let cfg = EnvConfig { action_repeat: 4, ..EnvConfig::default() };
    let mut repeated = Env::new(Arc::new(bumpy_course()), cfg).unwrap();
    repeated.reset(0);
    for a in &acts {
        let mut r = 0.0;
        for _ in 0..4 {
            r += single.step(*a).info.reward;
        }
        let s = repeated.step(*a);
        assert_eq!(s.info.reward, r);
        assert_eq!(state_fields(repeated.state()), state_fields(single.state()));
        if s.info.terminated || s.info.truncated {
            break;
        }
    }
    assert_eq!(repeated.recording(), single.recording());
}

#[test]
fn turn_is_clamped_and_yaw_wraps() {
    let mut env = new_env(flat_course());
    env.reset(0);
    env.step(Action { yaw_delta: 1000.0, pitch_delta: -1000.0, ..Action::IDLE });
    assert_eq!(env.view_angles(), Vec3::new(-10.0, 10.0, 0.0));
    env.step(Action { yaw_delta: -10.0, ..Action::IDLE });
    env.step(Action { yaw_delta: -10.0, ..Action::IDLE });
    assert_eq!(env.view_angles().y, 350.0);
    env.step(Action { yaw_delta: f32::NAN, ..Action::IDLE });
    assert_eq!(env.view_angles().y, 350.0);
    // The observation reports the applied delta, not the requested one.
    let s = env.step(Action { yaw_delta: 99.0, ..Action::IDLE });
    assert_eq!(env.spec().block(&s.obs, "proprioception").unwrap()[12], 10.0);
}

#[test]
fn mode_tickrate_mismatch_is_an_error() {
    let cfg = EnvConfig { tickrate: Some(64), ..EnvConfig::default() };
    assert!(Env::new(Arc::new(flat_course()), cfg).is_err());
    let cfg = EnvConfig { mode: ModeKind::Vanilla, tickrate: Some(64), ..EnvConfig::default() };
    assert_eq!(Env::new(Arc::new(flat_course()), cfg).unwrap().tickrate(), 64);
}

#[test]
fn observation_layout_and_goal_vector() {
    let mut env = new_env(flat_course());
    let obs = env.reset(0);
    let spec = env.spec();
    assert_eq!(obs.len(), spec.len());
    let names: Vec<_> = spec.blocks.iter().map(|(b, _)| b.name.as_str()).collect();
    assert_eq!(names, ["proprioception", "goal"]);
    for (b, _) in &spec.blocks {
        assert_eq!(b.fields.len(), b.len());
    }
    // Spawn at the origin facing +x; the end zone's center is (650, 0, 124).
    let g = spec.block(&obs, "goal").unwrap();
    assert_eq!(g[0], 650.0);
    assert!(g[1].abs() < 1e-3);
    assert_eq!(g[2], 124.0 - DIST_EPSILON);
    assert_eq!(g[3], 650.0);
    // Turned 90 degrees left, the goal is to the right.
    let mut env = new_env(Course { spawn: (Vec3::new(0.0, 0.0, DIST_EPSILON), 90.0), ..flat_course() });
    let obs = env.reset(0);
    let g = env.spec().block(&obs, "goal").unwrap();
    assert!(g[0].abs() < 1e-3 && (g[1] - 650.0).abs() < 1e-3, "{g:?}");
}

#[test]
fn takeoff_is_tracked() {
    let mut env = new_env(flat_course());
    let obs = env.reset(0);
    let p = |o: &[f32]| env_block(o);
    assert_eq!(p(&obs)[9], -1.0, "no takeoff yet");
    // Let the spawn settle onto the floor, then jump.
    let mut env2 = new_env(flat_course());
    env2.reset(0);
    for _ in 0..10 {
        env2.step(Action::IDLE);
    }
    assert!(env2.state().on_ground());
    let s = env2.step(Action { jump: true, ..Action::IDLE });
    assert!(!env2.state().on_ground());
    assert_eq!(p(&s.obs)[9], 0.0);
    let s = env2.step(Action::IDLE);
    assert_eq!(p(&s.obs)[9], 1.0);
}

fn env_block(obs: &[f32]) -> Vec<f32> {
    // proprioception is the first block.
    obs[..21].to_vec()
}

#[test]
fn toy_solutions_complete_through_the_env() {
    use course::toy::{generate, Stage};
    for stage in Stage::ALL {
        for seed in 0..20 {
            let t = generate(stage, seed);
            let mut env = new_env(t.course.clone());
            env.reset(seed);
            let mut last = None;
            for (i, a) in Action::from_toy(&t).into_iter().enumerate() {
                let info = env.step(a).info;
                if info.terminated || info.truncated {
                    assert_eq!(i + 1, t.solution.len(), "{stage:?} seed {seed}: ended early {info:?}");
                }
                last = Some(info);
            }
            let info = last.unwrap();
            assert_eq!(info.status, Status::Completed, "{stage:?} seed {seed}");
            assert_eq!(info.completed_ticks, t.reference_ticks, "{stage:?} seed {seed}");
            assert_eq!(env.recording().cmds, t.solution, "{stage:?} seed {seed}");
        }
    }
}

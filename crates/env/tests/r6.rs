//! R6 gate (docs/reinforcement-plan.md §11): a scripted run presses the end button; shaping rewards
//! telescope to route progress; waypoints are reached in order and observed.

use std::sync::Arc;

use course::toy::{generate, generate_with_end, EndKind, Stage, SOLUTION_MODE, SOLUTION_TICKRATE};
use course::{use_hits, CourseRun, EndSpec, Status, USE_RANGE};
use env::{
    Action, BaseReward, Env, EnvConfig, ProgressShaping, RayGrid, Reward, SumReward, SurfaceClass, WaypointBonus,
};
use movement::{process_movement, tick_interval, Buttons, NullObserver, PlayerState, UserCmd, Vec3};


/// Run `cmds` through the sim and a `CourseRun`; the status and tick after each command.
fn run(course: &course::Course, cmds: &[UserCmd]) -> Vec<(Status, u32)> {
    let mut mode = SOLUTION_MODE.create();
    let cfg = *mode.config();
    let dt = tick_interval(SOLUTION_TICKRATE);
    let mut state = PlayerState::new(course.spawn.0);
    let mut cr = CourseRun::new();
    cmds.iter()
        .map(|c| {
            process_movement(&cfg, mode.as_mut(), &course.world, &mut state, c, &mut NullObserver, dt);
            cr.update(course, &state, c);
            (cr.status, cr.elapsed_ticks)
        })
        .collect()
}

#[test]
fn scripted_runs_press_the_button() {
    for stage in Stage::ALL {
        for seed in 0..100 {
            let t = generate_with_end(stage, seed, EndKind::Button);
            let reference = t.reference_ticks.unwrap_or_else(|| panic!("{stage:?} seed {seed}: controller failed"));
            assert!(matches!(t.course.end, EndSpec::Use { .. }));
            let log = run(&t.course, &t.solution);
            let done = log.iter().position(|&(s, _)| s == Status::Completed).expect("completes");
            assert_eq!(done + 1, t.solution.len(), "{stage:?} seed {seed}");
            assert_eq!(log[done].1, reference);
            // The finishing command is a fresh +use press.
            assert!(t.solution[done].buttons.contains(Buttons::USE));
            assert!(!t.solution[done - 1].buttons.contains(Buttons::USE));
            // Same layout as the Touch course.
            assert_eq!(t.blocks, generate(stage, seed).blocks);
        }
    }
}

/// Walk into the button on a Walk course, with `use_at(tick)` deciding when +use is down.
fn walk_and_use(use_at: impl Fn(u32) -> bool, ticks: u32) -> Vec<(Status, u32)> {
    let t = generate_with_end(Stage::Walk, 0, EndKind::Button);
    let cmds: Vec<UserCmd> = (0..ticks)
        .map(|i| {
            let mut b = Buttons::FORWARD;
            if use_at(i) {
                b.insert(Buttons::USE);
            }
            UserCmd::from_buttons(i, Vec3::ZERO, b)
        })
        .collect();
    run(&t.course, &cmds)
}

#[test]
fn use_needs_a_fresh_press_in_range() {
    // Held from the first tick: never a fresh press near the button.
    let held = walk_and_use(|_| true, 600);
    assert!(held.iter().all(|&(s, _)| s != Status::Completed));
    // Released and pressed again near the button: completes.
    let repress = walk_and_use(|i| !(10..400).contains(&i), 600);
    assert!(repress.iter().any(|&(s, _)| s == Status::Completed));
    // Tapping while still far away does nothing until in range.
    let t = generate_with_end(Stage::Walk, 0, EndKind::Button);
    let EndSpec::Use { target, entity } = t.course.end else { unreachable!() };
    let far = PlayerState::new(Vec3::new(target.x - 200.0, 0.0, 0.03125));
    assert_eq!(use_hits(&t.course, &far, Vec3::ZERO), None);
    let near = PlayerState::new(Vec3::new(target.x - USE_RANGE + 8.0, 0.0, 0.03125));
    assert_eq!(use_hits(&t.course, &near, Vec3::ZERO), Some(entity));
    // KZ buttons (min_use_angle -1) press in any view direction (docs/use-rules.md).
    assert_eq!(use_hits(&t.course, &near, Vec3::new(0.0, 180.0, 0.0)), Some(entity));
}

/// The measured reach is a box around the button's bounds, not a sphere (docs/use-rules.md): the gap
/// along x decides, even with the button 64 units to the side.
#[test]
fn use_reach_is_a_box_and_needs_line_of_sight() {
    let t = generate_with_end(Stage::Walk, 0, EndKind::Button);
    let EndSpec::Use { entity, .. } = t.course.end else { unreachable!() };
    let (lo, _) = t.course.entity_bounds(entity).unwrap();
    let eye_at = |gap_x: f32, side: f32| Vec3::new(lo.x - gap_x, side, lo.z + 8.0);
    assert_eq!(course::use_target(&t.course, eye_at(USE_RANGE - 0.02, 0.0), Vec3::ZERO), Some(entity));
    assert_eq!(course::use_target(&t.course, eye_at(USE_RANGE + 0.02, 0.0), Vec3::ZERO), None);
    // 64 units beside the button's side face: still the x gap that decides.
    let side = 16.0 + 64.0;
    assert_eq!(course::use_target(&t.course, eye_at(USE_RANGE - 0.02, side), Vec3::ZERO), Some(entity));
    assert_eq!(course::use_target(&t.course, eye_at(USE_RANGE + 0.02, side), Vec3::ZERO), None);
    // From inside the end platform's solid, within reach by the box test: no line of sight. (The real
    // wall case is the probe's `Uo_slow` capture, checked by `compare useprobe-check`.)
    let below = Vec3::new(lo.x + 8.0, 0.0, lo.z - 60.0);
    assert_eq!(course::use_target(&t.course, below, Vec3::ZERO), None);
}

#[test]
fn rays_see_the_button_as_goal() {
    let t = generate_with_end(Stage::Walk, 0, EndKind::Button);
    let EndSpec::Use { target, .. } = t.course.end else { unreachable!() };
    let grid = RayGrid { rows: 1, cols: 1, ..RayGrid::default() };
    let state = PlayerState::new(Vec3::new(target.x - 100.0, 0.0, 0.03125));
    let hit = grid.cast_all(&t.course, &state, Vec3::ZERO)[0];
    assert_eq!(hit.class, SurfaceClass::Goal);
    assert!((hit.distance * grid.max_distance - (100.0 - 8.0)).abs() < 0.1, "{hit:?}");
    // Turned away, the same spot sees the start side of the course, not the goal.
    let back = grid.cast_all(&t.course, &state, Vec3::new(0.0, 180.0, 0.0))[0];
    assert_ne!(back.class, SurfaceClass::Goal);
}

/// Pseudo-random actions (64-bit LCG), forward-biased so runs make progress and sometimes fall.
fn random_actions(seed: u64, n: usize) -> Vec<Action> {
    let mut x = seed;
    let mut next = move || {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (x >> 33) as u32
    };
    (0..n)
        .map(|_| {
            let r = next();
            Action {
                forward: if r % 5 == 0 { -1 } else { 1 },
                side: (r / 5 % 3) as i8 - 1,
                jump: r / 15 % 11 == 0,
                yaw_delta: (next() % 201) as f32 / 100.0 - 1.0,
                ..Action::IDLE
            }
        })
        .collect()
}

#[test]
fn progress_shaping_telescopes() {
    let shaping = ProgressShaping { scale: 0.01, gamma: 1.0 };
    for stage in Stage::ALL {
        for seed in 0..10 {
            let t = generate(stage, seed);
            let solution: Vec<Action> = Action::from_toy(&t);
            for acts in [solution, random_actions(seed, 3000)] {
                let mut env = Env::new(Arc::new(t.course.clone()), EnvConfig::default()).unwrap().with_reward(Box::new(shaping));
                env.reset(seed);
                let start = ProgressShaping::potential(env.course(), env.state(), env.run());
                let mut total = 0.0f64;
                for a in &acts {
                    let s = env.step(*a);
                    total += s.info.reward as f64;
                    if s.info.terminated || s.info.truncated {
                        break;
                    }
                }
                let end = ProgressShaping::potential(env.course(), env.state(), env.run());
                let expect = shaping.scale as f64 * (end as f64 - start as f64);
                // Per-tick f32 rounding of the potential differences accumulates; the bound is loose
                // compared with a route of thousands of units.
                assert!((total - expect).abs() < 1e-3 + 1e-4 * expect.abs(), "{stage:?} seed {seed}: {total} vs {expect}");
            }
        }
    }
}

#[test]
fn waypoints_are_reached_in_order_and_rewarded() {
    for stage in Stage::ALL {
        for seed in 0..20 {
            let t = generate(stage, seed);
            let reward = SumReward(vec![Box::new(BaseReward { tick_cost: 0.0, completion_bonus: 0.0, ..BaseReward::default() }), Box::new(WaypointBonus { bonus: 1.0 })]);
            let mut env = Env::new(Arc::new(t.course.clone()), EnvConfig::default()).unwrap().with_reward(Box::new(reward));
            env.reset(0);
            let (mut total, mut last_reached, mut info) = (0.0, 0, None);
            for a in Action::from_toy(&t) {
                let s = env.step(a);
                assert!(s.info.waypoints_reached >= last_reached, "progress never goes back on a clean run");
                assert!(s.info.waypoints_reached <= last_reached + 1 || stage == Stage::Walk, "one at a time");
                last_reached = s.info.waypoints_reached;
                total += s.info.reward;
                info = Some(s.info);
            }
            let info = info.unwrap();
            assert_eq!(info.status, Status::Completed);
            // Every waypoint before the end platform's. That last one can stay unreached: the Touch end
            // completes the moment the hull touches the end volume, which can be before the origin is within
            // the platform's waypoint (dropping from above, more than `WAYPOINT_ABOVE` over it; or arriving
            // off-centre after a strafed long jump, outside its radius). Waypoints never decide the outcome.
            let n = t.course.waypoints.len();
            assert!(info.waypoints_reached == n || info.waypoints_reached == n - 1, "{stage:?} seed {seed}");
            if info.waypoints_reached == n - 1 {
                assert!(matches!(t.course.end, EndSpec::Touch(_)), "{stage:?} seed {seed}");
                assert!(!t.course.waypoints[n - 1].reached_by(env.state().origin), "{stage:?} seed {seed}");
            }
            assert_eq!(total, info.waypoints_reached as f32);
        }
    }
}

#[test]
fn goal_block_reports_the_next_waypoint() {
    let t = generate(Stage::GapChain, 3);
    let mut env = Env::new(Arc::new(t.course.clone()), EnvConfig::default()).unwrap();
    let obs = env.reset(0);
    let g = env.spec().block(&obs, "goal").unwrap().to_vec();
    assert_eq!(g.len(), 9);
    let wp = t.course.waypoints[0].pos;
    let spawn = t.course.spawn.0;
    assert_eq!(g[4], 1.0, "has_waypoint");
    assert_eq!(g[5], wp.x - spawn.x);
    assert!(g[6].abs() < 1e-3);
    // Once every waypoint is reached, the waypoint fields repeat the goal.
    let mut info = None;
    for a in Action::from_toy(&t) {
        info = Some(env.step(a));
    }
    let s = info.unwrap();
    let g = env.spec().block(&s.obs, "goal").unwrap();
    assert_eq!(g[4], 0.0);
    assert_eq!(&g[5..9], &g[0..4]);
}

#[test]
fn snapshot_forks_combined_rewards() {
    let t = generate(Stage::Heights, 2);
    let reward = || -> Box<dyn Reward> {
        Box::new(SumReward(vec![Box::new(BaseReward::default()), Box::new(ProgressShaping::default()), Box::new(WaypointBonus { bonus: 0.5 })]))
    };
    let mut a = Env::new(Arc::new(t.course.clone()), EnvConfig::default()).unwrap().with_reward(reward());
    a.reset(0);
    let acts: Vec<Action> = Action::from_toy(&t);
    for x in &acts[..200] {
        a.step(*x);
    }
    let snap = a.snapshot();
    let straight: Vec<u32> = acts[200..].iter().map(|x| a.step(*x).info.reward.to_bits()).collect();
    a.restore(&snap).unwrap();
    let resumed: Vec<u32> = acts[200..].iter().map(|x| a.step(*x).info.reward.to_bits()).collect();
    assert_eq!(straight, resumed);
}

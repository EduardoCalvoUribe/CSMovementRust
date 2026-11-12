//! R3 gate (docs/reinforcement-plan.md §11): every generated stage is solved by its reference solution,
//! replayed from scratch; the same seed gives the same course.

use course::toy::{generate, reference_solution, Stage, SOLUTION_MODE, SOLUTION_TICKRATE};
use course::{CourseRun, Status, ToyCourse};
use movement::{process_movement, tick_interval, Buttons, NullObserver, PlayerState};

const SEEDS: u64 = 300;

/// Replay the solution with a fresh mode and timer, independent of the generator's own run.
fn replay(t: &ToyCourse) -> (Status, u32, usize) {
    let mut mode = SOLUTION_MODE.create();
    let cfg = *mode.config();
    let dt = tick_interval(SOLUTION_TICKRATE);
    let mut state = PlayerState::new(t.course.spawn.0);
    let mut run = CourseRun::new();
    for (i, cmd) in t.solution.iter().enumerate() {
        process_movement(&cfg, mode.as_mut(), &t.course.world, &mut state, cmd, &mut NullObserver, dt);
        if run.update(&t.course, &state, cmd).is_terminal() {
            return (run.status, run.elapsed_ticks, i + 1);
        }
    }
    (run.status, run.elapsed_ticks, t.solution.len())
}

#[test]
fn every_stage_is_solved_by_its_solution() {
    for stage in Stage::ALL {
        let mut times = Vec::new();
        for seed in 0..SEEDS {
            let t = generate(stage, seed);
            let reference = t.reference_ticks.unwrap_or_else(|| panic!("{stage:?} seed {seed}: controller failed"));
            let (status, ticks, used) = replay(&t);
            assert_eq!(status, Status::Completed, "{stage:?} seed {seed}");
            assert_eq!(ticks, reference, "{stage:?} seed {seed}");
            assert_eq!(used, t.solution.len(), "{stage:?} seed {seed}: solution has commands past the finish");
            times.push(reference);
        }
        println!("{stage:?}: reference ticks {}..={}", times.iter().min().unwrap(), times.iter().max().unwrap());
    }
}

#[test]
fn same_seed_same_course() {
    for stage in Stage::ALL {
        for seed in [0, 1, 12345, u64::MAX] {
            let (a, b) = (generate(stage, seed), generate(stage, seed));
            assert_eq!(a.blocks, b.blocks);
            assert_eq!(a.solution, b.solution);
            assert_eq!(a.reference_ticks, b.reference_ticks);
            assert_eq!(a.course.waypoints, b.course.waypoints);
        }
    }
}

#[test]
fn seeds_vary_the_layout() {
    for stage in [Stage::Gap, Stage::GapChain, Stage::ShortBlocks, Stage::Heights] {
        let layouts: std::collections::HashSet<String> =
            (0..50).map(|s| format!("{:?}", generate(stage, s).blocks)).collect();
        assert!(layouts.len() >= 30, "{stage:?}: only {} distinct layouts in 50 seeds", layouts.len());
    }
    // Stages with the same seed differ too.
    assert_ne!(format!("{:?}", generate(Stage::Gap, 5).blocks), format!("{:?}", generate(Stage::GapChain, 5).blocks));
}

#[test]
fn geometry_is_on_integer_coordinates() {
    for stage in Stage::ALL {
        for seed in 0..50 {
            for b in generate(stage, seed).blocks {
                for v in [b.min_x, b.max_x, b.top] {
                    assert_eq!(v, v.round(), "{stage:?} seed {seed}: {b:?}");
                }
            }
        }
    }
}

#[test]
fn stages_need_what_they_claim() {
    // Walk has no gap; every other stage has at least one, and Heights changes height.
    for seed in 0..50 {
        let w = generate(Stage::Walk, seed);
        assert_eq!(w.blocks[1].min_x, w.blocks[0].max_x);
        for stage in [Stage::Gap, Stage::GapChain, Stage::ShortBlocks, Stage::Heights] {
            let t = generate(stage, seed);
            assert!(t.blocks.windows(2).all(|p| p[1].min_x > p[0].max_x), "{stage:?} seed {seed}");
        }
        let h = generate(Stage::Heights, seed);
        assert!(h.blocks.iter().any(|b| b.top != 0.0));
    }
}

#[test]
fn short_blocks_are_short_and_rejumped_on_landing() {
    // What the stage claims (see `Stage::ShortBlocks`): stage blocks are at most 128 long and the solution
    // jumps exactly once per gap. Jumping only at edges is not reliable here: landings drift toward the
    // far edge and eventually overshoot a block (seed 15 does, at its third block).
    for seed in 0..50 {
        let t = generate(Stage::ShortBlocks, seed);
        let n = t.blocks.len();
        assert!(t.blocks[1..n - 1].iter().all(|b| b.max_x - b.min_x <= 128.0), "seed {seed}");
        let jumps = t.solution.iter().filter(|c| c.buttons.contains(Buttons::JUMP)).count();
        assert_eq!(jumps, n - 1, "seed {seed}");
    }
    let t = generate(Stage::ShortBlocks, 15);
    assert_eq!(reference_solution(&t.course, &t.blocks, false).1, None);
}

#[test]
fn long_jump_needs_strafing() {
    // What the stage claims (see `Stage::LongJump`): the gap is 220-250, the unstrafed controller (hold W,
    // jump at the edge; reach ~217) cannot clear it, and the strafing solution does, turning at most
    // `SOLUTION_MAX_TURN` per tick.
    for seed in 0..60 {
        let t = generate(Stage::LongJump, seed);
        let gap = t.blocks[1].min_x - t.blocks[0].max_x;
        assert!((220.0..=250.0).contains(&gap), "seed {seed}: gap {gap}");
        assert!(t.reference_ticks.is_some(), "seed {seed}: strafing solution failed");
        let (_, walk_only) = reference_solution(&t.course, &t.blocks, false);
        assert_eq!(walk_only, None, "seed {seed}: cleared without strafing");
        assert!(t.view_deltas.iter().any(|d| d.0 != 0.0), "seed {seed}: the solution never turns");
        assert!(t.view_deltas.iter().all(|d| d.0.abs() <= course::toy::SOLUTION_MAX_TURN));
        assert_eq!(t.view_deltas.len(), t.solution.len());
    }
}

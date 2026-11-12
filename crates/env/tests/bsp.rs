//! `Env` on an imported KZ map (R7): episodes on a BSP course verify bit for bit through their text form,
//! and the ray grid works on BSP collision. Uses the map at `CSMOVE_KZ_MAP` (default
//! `D:\Programs\csgo_workshop\kz\kz_longjumps_v4096.bsp`), outside the repo; prints SKIPPED without it.

use std::sync::Arc;
use std::time::Instant;

use course::bsp::{load, ImportOptions};
use env::{Action, CourseSource, Env, EnvConfig, Episode, GoalVector, Proprioception, RayGrid, SurfaceClass};

fn map_path() -> Option<String> {
    let p = std::env::var("CSMOVE_KZ_MAP").unwrap_or_else(|_| r"D:\Programs\csgo_workshop\kz\kz_longjumps_v4096.bsp".into());
    if std::path::Path::new(&p).exists() {
        Some(p)
    } else {
        println!("SKIPPED: no KZ map at {p} (set CSMOVE_KZ_MAP)");
        None
    }
}

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
                forward: if r % 5 == 0 { -1 } else { 1 },
                side: (r / 5 % 3) as i8 - 1,
                jump: r / 15 % 9 == 0,
                duck: r / 135 % 13 == 0,
                use_: r / 1755 % 4 == 0,
                yaw_delta: (next() % 801) as f32 / 100.0 - 4.0,
                pitch_delta: (next() % 101) as f32 / 100.0 - 0.5,
            }
        })
        .collect()
}

#[test]
fn bsp_episodes_verify_through_text() {
    let Some(path) = map_path() else { return };
    let opts = ImportOptions { allow_unsupported: true };
    let (course, _, hash) = load(std::path::Path::new(&path), opts).unwrap();
    let course = Arc::new(course);
    let grid = RayGrid { rows: 4, cols: 8, ..RayGrid::default() };
    for seed in 0..4 {
        let mut env = Env::new(course.clone(), EnvConfig { max_episode_ticks: 1500, ..EnvConfig::default() })
            .unwrap()
            .with_providers(vec![Box::new(Proprioception), Box::new(GoalVector), Box::new(grid)]);
        env.reset(seed);
        let mut classes = std::collections::HashSet::new();
        for a in actions(seed, 1500) {
            let s = env.step(a);
            let rays = env.spec().block(&s.obs, "rays").unwrap();
            for cell in rays.chunks(grid.channels()) {
                classes.insert(cell[1..].iter().position(|&v| v == 1.0).unwrap());
            }
            if s.info.terminated || s.info.truncated {
                break;
            }
        }
        assert!(classes.contains(&(SurfaceClass::Floor as usize)) && classes.contains(&(SurfaceClass::Wall as usize)), "{classes:?}");
        let ep = Episode::from_env(&env, CourseSource::Bsp { path: path.clone(), hash, allow_unsupported: true });
        let back = Episode::from_text(&ep.to_text()).unwrap();
        assert_eq!(back, ep);
        println!("{}", back.verify().unwrap());
    }
}

#[test]
fn bsp_episode_with_wrong_hash_is_rejected() {
    let Some(path) = map_path() else { return };
    let ep_source = CourseSource::Bsp { path, hash: 1, allow_unsupported: true };
    assert!(ep_source.build().unwrap_err().contains("hash"));
}

/// Steps/s on the BSP map, without and with the default ray grid (plan §7). Run with
/// `cargo test --release -p env --test bsp -- --ignored --nocapture`.
#[test]
#[ignore]
fn bsp_steps_per_second() {
    let Some(path) = map_path() else { return };
    let (course, ..) = load(std::path::Path::new(&path), ImportOptions { allow_unsupported: true }).unwrap();
    let course = Arc::new(course);
    let d = RayGrid::default();
    let variants: [(&str, Option<RayGrid>); 6] = [
        ("no rays", None),
        ("default 8x16, 2048, ladders", Some(d)),
        ("8x16, 2048, no ladders", Some(RayGrid { detect_ladders: false, ..d })),
        ("8x16, 1024, no ladders", Some(RayGrid { detect_ladders: false, max_distance: 1024.0, ..d })),
        ("8x16, 512, no ladders", Some(RayGrid { detect_ladders: false, max_distance: 512.0, ..d })),
        ("4x8, 1024, no ladders", Some(RayGrid { rows: 4, cols: 8, detect_ladders: false, max_distance: 1024.0, ..d })),
    ];
    for (name, grid) in variants {
        let rays = grid.is_some();
        let mut env = Env::new(course.clone(), EnvConfig { max_episode_ticks: 3000, ..EnvConfig::default() }).unwrap();
        if let Some(g) = grid {
            env = env.with_providers(vec![Box::new(Proprioception), Box::new(GoalVector), Box::new(g)]);
        }
        let mut obs = vec![0.0; env.spec().len()];
        env.reset_into(0, &mut obs);
        let acts = actions(1, 3000);
        let n = if rays { 50_000 } else { 500_000 };
        let t0 = Instant::now();
        for i in 0..n {
            let info = env.step_into(acts[i % acts.len()], &mut obs);
            if info.terminated || info.truncated {
                env.reset_into(0, &mut obs);
            }
        }
        let s = t0.elapsed().as_secs_f64();
        println!("THROUGHPUT bsp {name}: {:.0} steps/s", n as f64 / s);
    }
}

//! R5 gate (docs/reinforcement-plan.md §11): `VecEnv` gives the same results in parallel as serially,
//! whatever the thread count, and auto-reset matches a lone `Env`.

use std::sync::Arc;

use course::toy::{generate, Stage};
use env::{episode_seed, Action, Env, EnvConfig, GoalVector, ObsProvider, Proprioception, RayGrid, VecEnv};
use movement::replay::state_fields;

const N: usize = 12;

fn providers() -> Vec<Box<dyn ObsProvider>> {
    vec![Box::new(Proprioception), Box::new(GoalVector), Box::new(RayGrid { rows: 3, cols: 5, ..RayGrid::default() })]
}

/// Different toy courses per env, short episodes so auto-reset happens often.
fn envs() -> Vec<Env> {
    (0..N)
        .map(|i| {
            let toy = generate(Stage::ALL[i % Stage::ALL.len()], i as u64);
            let cfg = EnvConfig { max_episode_ticks: 150 + 37 * i as u32, ..EnvConfig::default() };
            Env::new(Arc::new(toy.course), cfg).unwrap().with_providers(providers())
        })
        .collect()
}

/// Per-step action batches from a 64-bit LCG.
fn batches(seed: u64, steps: usize) -> Vec<Vec<Action>> {
    let mut x = seed;
    let mut next = move || {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (x >> 33) as u32
    };
    (0..steps)
        .map(|_| {
            (0..N)
                .map(|_| {
                    let r = next();
                    Action {
                        forward: if r % 4 == 0 { 0 } else { 1 },
                        side: (r / 4 % 3) as i8 - 1,
                        jump: r / 12 % 9 == 0,
                        duck: r / 108 % 13 == 0,
                        yaw_delta: (next() % 601) as f32 / 100.0 - 3.0,
                        ..Action::IDLE
                    }
                })
                .collect()
        })
        .collect()
}

fn bits(v: &[f32]) -> Vec<u32> {
    v.iter().map(|f| f.to_bits()).collect()
}

type Trace = Vec<(Vec<u32>, Vec<u32>, Vec<String>)>;

fn trace(mut v: VecEnv, acts: &[Vec<Action>]) -> (Trace, Vec<Vec<String>>) {
    let mut out = vec![(bits(v.reset(17)), Vec::new(), Vec::new())];
    for a in acts {
        let (obs, infos) = v.step(a);
        let obs = bits(obs);
        let infos = infos.iter().map(|i| format!("{i:?}")).collect();
        out.push((obs, bits(v.final_obs()), infos));
    }
    let states = v.envs().iter().map(|e| state_fields(e.state())).collect();
    (out, states)
}

#[test]
fn parallel_equals_serial() {
    let acts = batches(3, 700);
    let serial = trace(VecEnv::new(envs()).unwrap().serial(), &acts);
    let global = trace(VecEnv::new(envs()).unwrap(), &acts);
    let three = trace(VecEnv::new(envs()).unwrap().with_threads(3).unwrap(), &acts);
    assert_eq!(global, serial);
    assert_eq!(three, serial);
    // Episodes actually ended along the way, so auto-reset was exercised.
    let ended = serial.0.iter().flat_map(|(_, _, i)| i).filter(|s| s.contains("terminated: true") || s.contains("truncated: true")).count();
    assert!(ended > 20, "only {ended} episode ends");
}

#[test]
fn auto_reset_matches_a_lone_env() {
    let acts = batches(8, 900);
    let mut v = VecEnv::new(envs()).unwrap();
    v.reset(5);
    let i = 4;
    let mut lone = envs().swap_remove(i);
    let mut episode = 0;
    assert_eq!(bits(&lone.reset(episode_seed(5, i, episode))), bits(&v.obs()[i * v.obs_len()..][..v.obs_len()]));
    let mut ends = 0;
    for a in &acts {
        let (obs, infos) = v.step(a);
        let mine = &obs[i * lone.spec().len()..][..lone.spec().len()];
        let s = lone.step(a[i]);
        assert_eq!(infos[i], s.info);
        if s.info.terminated || s.info.truncated {
            ends += 1;
            assert_eq!(bits(&v.final_obs()[i * v.obs_len()..][..v.obs_len()]), bits(&s.obs));
            episode += 1;
            let fresh = lone.reset(episode_seed(5, i, episode));
            assert_eq!(bits(&v.obs()[i * v.obs_len()..][..v.obs_len()]), bits(&fresh));
            assert_eq!(v.envs()[i].seed(), episode_seed(5, i, episode));
        } else {
            assert_eq!(bits(mine), bits(&s.obs));
        }
    }
    assert!(ends >= 2, "env {i} ended only {ends} episodes");
}

#[test]
fn without_auto_reset_finished_envs_wait_for_reset_one() {
    let mut v = VecEnv::new(envs()).unwrap().with_auto_reset(false);
    v.reset(0);
    let idle = vec![Action::IDLE; N];
    // Env 0 truncates at 150 ticks; step it there, then reset only it.
    for _ in 0..150 {
        v.step(&idle);
    }
    assert!(v.infos()[0].truncated);
    assert_eq!(v.envs()[0].track().episode_ticks, 150);
    v.reset_one(0);
    assert_eq!(v.envs()[0].track().episode_ticks, 0);
    assert_eq!(v.envs()[0].seed(), episode_seed(0, 0, 1));
}

#[test]
fn mismatched_specs_are_rejected() {
    let mut e = envs();
    e[1] = Env::new(Arc::new(generate(Stage::Gap, 0).course), EnvConfig::default()).unwrap();
    assert!(VecEnv::new(e).is_err());
    assert!(VecEnv::new(Vec::new()).is_err());
}

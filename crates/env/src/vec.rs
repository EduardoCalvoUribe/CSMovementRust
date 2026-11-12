//! `VecEnv` (plan §7, R5): N independent `Env`s stepped in parallel with rayon, writing into buffers
//! allocated once. Results do not depend on thread count or scheduling: each env only touches its own
//! slice, and auto-reset seeds come from a fixed per-env sequence.

use rayon::prelude::*;
use rayon::ThreadPool;

use crate::action::Action;
use crate::env::{Env, StepInfo};
use crate::obs::ObsSpec;

pub struct VecEnv {
    envs: Vec<Env>,
    obs_len: usize,
    /// `envs.len() * obs_len`, env-major.
    obs: Vec<f32>,
    /// The last observation of an episode that just ended, for envs whose `infos[i]` ended it; the env
    /// itself has already been reset into `obs` when `auto_reset` is on.
    final_obs: Vec<f32>,
    infos: Vec<StepInfo>,
    /// Episodes started per env, for the reset seed sequence.
    episodes: Vec<u64>,
    base_seed: u64,
    auto_reset: bool,
    pool: Option<ThreadPool>,
    parallel: bool,
}

/// Seed for env `i`'s episode `k` (SplitMix64 finalizer over the mixed inputs).
pub fn episode_seed(base: u64, env: usize, episode: u64) -> u64 {
    let mut z = base ^ (env as u64).wrapping_mul(0x9e3779b97f4a7c15) ^ episode.wrapping_mul(0xd6e8feb86659fd93);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

impl VecEnv {
    /// All envs must share one observation spec. Auto-reset is on and stepping uses rayon's global pool.
    pub fn new(envs: Vec<Env>) -> Result<Self, String> {
        let first = envs.first().ok_or("VecEnv needs at least one env")?;
        let spec = first.spec().clone();
        if envs.iter().any(|e| *e.spec() != spec) {
            return Err("all envs in a VecEnv must have the same observation spec".into());
        }
        let n = envs.len();
        let obs_len = spec.len();
        let empty = StepInfo {
            reward: 0.0,
            terminated: false,
            truncated: false,
            status: course::Status::Idle,
            run_ticks: 0,
            episode_ticks: 0,
            completed_ticks: None,
            distance_to_goal: None,
            waypoints_reached: 0,
        };
        Ok(Self {
            envs,
            obs_len,
            obs: vec![0.0; n * obs_len],
            final_obs: vec![0.0; n * obs_len],
            infos: vec![empty; n],
            episodes: vec![0; n],
            base_seed: 0,
            auto_reset: true,
            pool: None,
            parallel: true,
        })
    }

    /// Run on a private pool of `threads` threads instead of the global one.
    pub fn with_threads(mut self, threads: usize) -> Result<Self, String> {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().map_err(|e| e.to_string())?;
        self.pool = Some(pool);
        Ok(self)
    }

    /// Step the envs one after another on the calling thread (for comparison and debugging).
    pub fn serial(mut self) -> Self {
        self.parallel = false;
        self
    }

    /// With auto-reset off, a finished env must be reset with `reset_one` before it is stepped again.
    pub fn with_auto_reset(mut self, on: bool) -> Self {
        self.auto_reset = on;
        self
    }

    pub fn len(&self) -> usize {
        self.envs.len()
    }
    pub fn is_empty(&self) -> bool {
        self.envs.is_empty()
    }
    pub fn obs_len(&self) -> usize {
        self.obs_len
    }
    pub fn spec(&self) -> &ObsSpec {
        self.envs[0].spec()
    }
    pub fn envs(&self) -> &[Env] {
        &self.envs
    }
    pub fn obs(&self) -> &[f32] {
        &self.obs
    }
    pub fn final_obs(&self) -> &[f32] {
        &self.final_obs
    }
    pub fn infos(&self) -> &[StepInfo] {
        &self.infos
    }

    /// Reset every env, starting each env's seed sequence from `base_seed`.
    pub fn reset(&mut self, base_seed: u64) -> &[f32] {
        self.base_seed = base_seed;
        self.episodes.fill(0);
        let base = self.base_seed;
        let len = self.obs_len;
        let job = |(i, (env, (obs, ep))): (usize, (&mut Env, (&mut [f32], &mut u64)))| {
            env.reset_into(episode_seed(base, i, *ep), obs);
            *ep += 1;
        };
        let (envs, obs, eps, pool) = (&mut self.envs, &mut self.obs, &mut self.episodes, self.pool.as_ref());
        if self.parallel {
            let mut par = || envs.par_iter_mut().zip(obs.par_chunks_mut(len).zip(eps.par_iter_mut())).enumerate().for_each(job);
            match pool {
                Some(p) => p.install(par),
                None => par(),
            }
        } else {
            envs.iter_mut().zip(obs.chunks_mut(len).zip(eps.iter_mut())).enumerate().for_each(job);
        }
        &self.obs
    }

    /// Reset one env (needed only with auto-reset off).
    pub fn reset_one(&mut self, i: usize) {
        let seed = episode_seed(self.base_seed, i, self.episodes[i]);
        self.episodes[i] += 1;
        self.envs[i].reset_into(seed, &mut self.obs[i * self.obs_len..(i + 1) * self.obs_len]);
    }

    /// Step env `i` with `actions[i]`. Returns the observations (after any auto-reset) and infos.
    pub fn step(&mut self, actions: &[Action]) -> (&[f32], &[StepInfo]) {
        assert_eq!(actions.len(), self.envs.len(), "one action per env");
        let (base, len, auto) = (self.base_seed, self.obs_len, self.auto_reset);
        type Item<'a> = (usize, ((&'a mut Env, &'a Action), ((&'a mut [f32], &'a mut [f32]), (&'a mut StepInfo, &'a mut u64))));
        let job = |(i, ((env, a), ((obs, fin), (info, ep)))): Item| {
            *info = env.step_into(*a, obs);
            if auto && (info.terminated || info.truncated) {
                fin.copy_from_slice(obs);
                env.reset_into(episode_seed(base, i, *ep), obs);
                *ep += 1;
            }
        };
        let (envs, obs, fin, infos, eps, pool) =
            (&mut self.envs, &mut self.obs, &mut self.final_obs, &mut self.infos, &mut self.episodes, self.pool.as_ref());
        if self.parallel {
            let mut par = || {
                envs.par_iter_mut()
                    .zip(actions.par_iter())
                    .zip(obs.par_chunks_mut(len).zip(fin.par_chunks_mut(len)).zip(infos.par_iter_mut().zip(eps.par_iter_mut())))
                    .enumerate()
                    .for_each(job)
            };
            match pool {
                Some(p) => p.install(par),
                None => par(),
            }
        } else {
            envs.iter_mut()
                .zip(actions.iter())
                .zip(obs.chunks_mut(len).zip(fin.chunks_mut(len)).zip(infos.iter_mut().zip(eps.iter_mut())))
                .enumerate()
                .for_each(job);
        }
        (&self.obs, &self.infos)
    }
}

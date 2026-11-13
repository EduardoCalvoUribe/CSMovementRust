//! Episode files (plan §4, R8): one agent episode as the single artifact for the viewer, regression tests,
//! and later real-server validation. It names how to rebuild the course, records the outcome and final
//! state, and embeds the bit-exact `Recording`.
//!
//! Format (text):
//!
//! ```text
//! csmove-episode 1
//! course toy <Stage> <seed> <Touch|Button>        (or)
//! course bsp <fnv64 hex> <allow_unsupported 0|1> <path to .bsp>
//! outcome <Status> <run ticks> <waypoints reached>
//! final <PlayerState fields, as in replay.rs>
//! csmove-replay 1
//! ... the Recording, verbatim ...
//! ```

use course::toy::{generate_with_end, EndKind, Stage};
use course::{Course, CourseRun, KzRules, Status};
use movement::replay::{state_fields, Recording};
use movement::{process_movement, tick_interval, NullObserver};

use crate::env::Env;

/// How to rebuild an episode's course.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CourseSource {
    Toy { stage: Stage, seed: u64, end: EndKind },
    /// An imported map (`course::bsp`). `hash` is the file's FNV-1a 64; a different file is an error.
    Bsp { path: String, hash: u64, allow_unsupported: bool },
}

impl CourseSource {
    pub fn build(&self) -> Result<Course, String> {
        match self {
            CourseSource::Toy { stage, seed, end } => Ok(generate_with_end(*stage, *seed, *end).course),
            CourseSource::Bsp { path, hash, allow_unsupported } => {
                let opts = course::bsp::ImportOptions { allow_unsupported: *allow_unsupported };
                let (course, _, got) = course::bsp::load(std::path::Path::new(path), opts)?;
                if got != *hash {
                    return Err(format!("{path}: map hash {got:016x} differs from the episode's {hash:016x}"));
                }
                Ok(course)
            }
        }
    }

    pub fn describe(&self) -> String {
        match self {
            CourseSource::Toy { stage, seed, end } => format!("toy {stage:?} seed {seed} ({end:?} end)"),
            CourseSource::Bsp { path, .. } => format!("map {path}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Episode {
    pub source: CourseSource,
    pub status: Status,
    pub run_ticks: u32,
    pub waypoints_reached: usize,
    /// `state_fields` of the final state.
    pub final_state: Vec<String>,
    pub recording: Recording,
}

/// What replaying an episode produced.
#[derive(Clone, Debug, PartialEq)]
pub struct Replayed {
    pub status: Status,
    pub run_ticks: u32,
    pub waypoints_reached: usize,
    pub final_state: Vec<String>,
}

impl Episode {
    /// The env's current episode. `source` must be what the env's course was built from.
    pub fn from_env(env: &Env, source: CourseSource) -> Self {
        let run = env.run();
        Self {
            source,
            status: run.status,
            run_ticks: run.elapsed_ticks,
            waypoints_reached: run.next_waypoint,
            final_state: state_fields(env.state()),
            recording: env.recording().clone(),
        }
    }

    /// Rebuild the course and replay the commands with a fresh mode and timer.
    pub fn replay(&self) -> Result<Replayed, String> {
        let course = self.source.build()?;
        Ok(replay_on(&course, &self.recording))
    }

    /// Replay and compare with the recorded outcome and final state, bit for bit.
    pub fn verify(&self) -> Result<String, String> {
        let r = self.replay()?;
        let recorded = Replayed {
            status: self.status,
            run_ticks: self.run_ticks,
            waypoints_reached: self.waypoints_reached,
            final_state: self.final_state.clone(),
        };
        if r == recorded {
            Ok(format!(
                "BIT_EXACT: {} commands on {}, {:?} at {} run ticks, final state identical",
                self.recording.cmds.len(),
                self.source.describe(),
                r.status,
                r.run_ticks
            ))
        } else {
            Err(format!(
                "MISMATCH after {} commands on {}\n recorded: {:?} {} ticks, {} waypoints\n           {}\n replayed: {:?} {} ticks, {} waypoints\n           {}",
                self.recording.cmds.len(),
                self.source.describe(),
                recorded.status,
                recorded.run_ticks,
                recorded.waypoints_reached,
                recorded.final_state.join(" "),
                r.status,
                r.run_ticks,
                r.waypoints_reached,
                r.final_state.join(" ")
            ))
        }
    }

    pub fn to_text(&self) -> String {
        let course = match &self.source {
            CourseSource::Toy { stage, seed, end } => format!("toy {stage:?} {seed} {end:?}"),
            CourseSource::Bsp { path, hash, allow_unsupported } => {
                format!("bsp {hash:016x} {} {path}", *allow_unsupported as u8)
            }
        };
        format!(
            "csmove-episode 1\ncourse {course}\noutcome {:?} {} {}\nfinal {}\n{}",
            self.status,
            self.run_ticks,
            self.waypoints_reached,
            self.final_state.join(" "),
            self.recording.to_text()
        )
    }

    pub fn from_text(text: &str) -> Result<Self, String> {
        let split = text.find("csmove-replay 1").ok_or("no embedded recording")?;
        let (header, rec) = text.split_at(split);
        let mut lines = header.lines();
        if lines.next().map(str::trim) != Some("csmove-episode 1") {
            return Err("not a csmove-episode v1 file".into());
        }
        let (mut source, mut outcome, mut final_state) = (None, None, None);
        for line in lines {
            let f: Vec<&str> = line.split_whitespace().collect();
            match f.first().copied() {
                Some("course") if f.get(1) == Some(&"bsp") => {
                    // The path is the rest of the line and may contain spaces.
                    let mut parts = line.trim_start().splitn(5, char::is_whitespace);
                    let (_, _, hash, allow, path) = (parts.next(), parts.next(), parts.next(), parts.next(), parts.next());
                    let hash = hash.and_then(|h| u64::from_str_radix(h, 16).ok()).ok_or_else(|| format!("bad course line `{line}`"))?;
                    let allow_unsupported = match allow {
                        Some("0") => false,
                        Some("1") => true,
                        _ => return Err(format!("bad course line `{line}`")),
                    };
                    let path = path.map(str::trim).filter(|p| !p.is_empty()).ok_or_else(|| format!("bad course line `{line}`"))?;
                    source = Some(CourseSource::Bsp { path: path.to_string(), hash, allow_unsupported });
                }
                Some("course") => {
                    if f.len() != 5 || f[1] != "toy" {
                        return Err(format!("bad course line `{line}`"));
                    }
                    let stage = Stage::ALL
                        .into_iter()
                        .find(|s| format!("{s:?}") == f[2])
                        .ok_or_else(|| format!("unknown stage `{}`", f[2]))?;
                    let seed = f[3].parse().map_err(|_| format!("bad seed `{}`", f[3]))?;
                    let end = match f[4] {
                        "Touch" => EndKind::Touch,
                        "Button" => EndKind::Button,
                        e => return Err(format!("unknown end `{e}`")),
                    };
                    source = Some(CourseSource::Toy { stage, seed, end });
                }
                Some("outcome") => {
                    if f.len() != 4 {
                        return Err(format!("bad outcome line `{line}`"));
                    }
                    let status = [Status::Idle, Status::Running, Status::Completed, Status::Hazard, Status::OutOfBounds]
                        .into_iter()
                        .find(|s| format!("{s:?}") == f[1])
                        .ok_or_else(|| format!("unknown status `{}`", f[1]))?;
                    let ticks = f[2].parse().map_err(|_| "bad run ticks")?;
                    let wps = f[3].parse().map_err(|_| "bad waypoint count")?;
                    outcome = Some((status, ticks, wps));
                }
                Some("final") => final_state = Some(f[1..].iter().map(|s| s.to_string()).collect()),
                None => {}
                Some(other) => return Err(format!("unknown record `{other}`")),
            }
        }
        let (status, run_ticks, waypoints_reached) = outcome.ok_or("missing outcome")?;
        Ok(Self {
            source: source.ok_or("missing course")?,
            status,
            run_ticks,
            waypoints_reached,
            final_state: final_state.ok_or("missing final state")?,
            recording: Recording::from_text(rec)?,
        })
    }
}

/// Replay `rec` on `course` with a fresh mode and `CourseRun`, as `Env` ran it.
pub fn replay_on(course: &Course, rec: &Recording) -> Replayed {
    let mut mode = rec.mode.create();
    let mut cfg = *mode.config();
    cfg.autobhop = rec.autobhop;
    let dt = tick_interval(rec.tickrate);
    let mut state = rec.start.clone();
    let mut run = CourseRun::starting_at(&state).with_rules(KzRules::for_mode(rec.mode, rec.tickrate));
    for cmd in &rec.cmds {
        process_movement(&cfg, mode.as_mut(), &course.world, &mut state, cmd, &mut NullObserver, dt);
        run.update(course, &state, cmd);
    }
    Replayed {
        status: run.status,
        run_ticks: run.elapsed_ticks,
        waypoints_reached: run.next_waypoint,
        final_state: state_fields(&state),
    }
}

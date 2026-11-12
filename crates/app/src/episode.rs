//! Agent episode viewer (docs/reinforcement-plan.md R8): `csmove --episode <file>` rebuilds the episode's
//! course, drives the sim with its recorded commands (looping, like `--ghost`), and draws the start zone,
//! end (volume or button), hazards, and waypoints. `csmove --check-episode <file>` is the headless
//! bit-exact check. Display only: `env::Episode::verify` is the source of truth.

use bevy::prelude::*;
use course::{Course, CourseRun, EndSpec, Status, Zone};
use env::Episode;
use movement::world::World;
use movement::{UserCmd, Vec3 as SVec3};
use testlevel::{Area, Level};

use crate::camera::to_bevy;
use crate::sim::Sim;

/// Ticks to hold on the last frame before looping.
const HOLD_TICKS: u32 = 128;

#[derive(Resource)]
pub struct EpisodeView {
    pub ep: Episode,
    pub course: Course,
    pub name: String,
    /// Index of the next command to run.
    pub cursor: usize,
    /// The run as replayed live in the viewer.
    pub run: CourseRun,
    hold: u32,
}

impl EpisodeView {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let ep = Episode::from_text(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path.file_name().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned());
        Ok(Self::new(ep, name))
    }

    pub fn new(ep: Episode, name: String) -> Self {
        let course = ep.source.build();
        Self { ep, course, name, cursor: 0, run: CourseRun::new(), hold: 0 }
    }

    /// The course as a level the app can render: its brushes and one area at the spawn.
    pub fn level(&self) -> Result<Level, String> {
        let World::Primitive(w) = &self.course.world else {
            return Err("only primitive-world courses can be viewed so far".into());
        };
        Ok(Level {
            brushes: w.brushes.clone(),
            signs: Vec::new(),
            areas: vec![Area { name: "Episode start", spawn: self.course.spawn.0, yaw: self.course.spawn.1 }],
        })
    }

    /// Put the sim at the recording's start, in its mode, tickrate, and autobhop setting.
    pub fn reset(&mut self, sim: &mut Sim) {
        let rec = &self.ep.recording;
        sim.set_mode(rec.mode);
        sim.tickrate = rec.tickrate;
        sim.cfg = *sim.mode.config();
        sim.autohop = rec.autobhop;
        sim.state = rec.start.clone();
        sim.prev_state = rec.start.clone();
        sim.spawn = (rec.start.origin, self.course.spawn.1);
        sim.tick = 0;
        self.cursor = 0;
        self.run = CourseRun::new();
        self.hold = 0;
    }

    /// The next command, or `None` once the recording (and the hold after it) is over.
    pub fn next_cmd(&mut self) -> Option<UserCmd> {
        if let Some(c) = self.ep.recording.cmds.get(self.cursor).copied() {
            self.cursor += 1;
            return Some(c);
        }
        None
    }

    /// Count a held frame after the last command; true when it is time to loop.
    pub fn hold_done(&mut self) -> bool {
        self.hold += 1;
        self.hold > HOLD_TICKS
    }
}

fn draw_box(gizmos: &mut Gizmos, lo: SVec3, hi: SVec3, c: Color) {
    let v = |x: f32, y: f32, z: f32| to_bevy(SVec3::new(x, y, z));
    for &z in &[lo.z, hi.z] {
        gizmos.line(v(lo.x, lo.y, z), v(hi.x, lo.y, z), c);
        gizmos.line(v(hi.x, lo.y, z), v(hi.x, hi.y, z), c);
        gizmos.line(v(hi.x, hi.y, z), v(lo.x, hi.y, z), c);
        gizmos.line(v(lo.x, hi.y, z), v(lo.x, lo.y, z), c);
    }
    for &x in &[lo.x, hi.x] {
        for &y in &[lo.y, hi.y] {
            gizmos.line(v(x, y, lo.z), v(x, y, hi.z), c);
        }
    }
}

fn draw_zone(gizmos: &mut Gizmos, z: &Zone, c: Color) {
    if let Some((lo, hi)) = z.bounds() {
        draw_box(gizmos, lo, hi, c);
    }
}

pub fn episode_gizmos(view: Option<Res<EpisodeView>>, mut gizmos: Gizmos) {
    let Some(view) = view else { return };
    let c = &view.course;
    draw_zone(&mut gizmos, &c.start_zone, Color::srgb(0.2, 0.9, 0.3));
    let gold = Color::srgb(1.0, 0.8, 0.1);
    match &c.end {
        EndSpec::Touch(z) => draw_zone(&mut gizmos, z, gold),
        EndSpec::Use { entity, .. } => {
            if let World::Primitive(w) = &c.world {
                for b in w.brushes.iter().filter(|b| b.entity == *entity) {
                    draw_box(&mut gizmos, b.mins, b.maxs, gold);
                }
            }
        }
    }
    for z in &c.hazards {
        draw_zone(&mut gizmos, z, Color::srgb(1.0, 0.2, 0.2));
    }
    for (i, w) in c.waypoints.iter().enumerate() {
        let reached = i < view.run.next_waypoint;
        let col = if reached { Color::srgb(0.4, 0.4, 0.4) } else { Color::srgb(0.3, 0.8, 1.0) };
        let p = to_bevy(w.pos);
        let up = to_bevy(w.pos.add(SVec3::new(0.0, 0.0, 48.0)));
        gizmos.line(p, up, col);
        gizmos.circle(Isometry3d::new(p, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)), w.radius, col);
    }
}

/// HUD lines for the episode.
pub fn panel(view: &EpisodeView) -> String {
    let rec = &view.ep.recording;
    let secs = |t: u32| t as f32 / rec.tickrate as f32;
    let running = match view.run.status {
        Status::Idle => "idle (in start zone)".to_string(),
        Status::Running => format!("running {:.3}s", secs(view.run.elapsed_ticks)),
        s => format!("{s:?} at {:.3}s ({} ticks)", secs(view.run.elapsed_ticks), view.run.elapsed_ticks),
    };
    format!(
        "EPISODE {}  {}\ntick {}/{}  {} {} Hz autobhop {}\nrun: {running}  waypoints {}/{}\nrecorded: {:?} in {} ticks\n",
        view.name,
        view.ep.source.describe(),
        view.cursor,
        rec.cmds.len(),
        rec.mode.name(),
        rec.tickrate,
        if rec.autobhop { "on" } else { "off" },
        view.run.next_waypoint,
        view.course.waypoints.len(),
        view.ep.status,
        view.ep.run_ticks,
    )
}

/// Headless check for `--check-episode`.
pub fn check(path: &std::path::Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Episode::from_text(&text)?.verify()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use course::toy::{generate_with_end, EndKind, Stage};
    use env::{Action, CourseSource, Env, EnvConfig, Episode};
    use movement::replay::state_fields;
    use movement::Buttons;

    use super::EpisodeView;

    /// The viewer's tick path (what `sim::fixed_tick` does with an episode loaded) ends on the episode's
    /// recorded outcome and final state, bit for bit, and loops back to the start.
    #[test]
    fn viewer_replays_episodes_bit_for_bit() {
        for (stage, end) in [(Stage::Heights, EndKind::Touch), (Stage::ShortBlocks, EndKind::Button)] {
            let toy = generate_with_end(stage, 3, end);
            let mut env = Env::new(Arc::new(toy.course.clone()), EnvConfig::default()).unwrap();
            env.reset(3);
            for c in &toy.solution {
                let key = |m: f32| (m > 0.0) as i8 - (m < 0.0) as i8;
                env.step(Action {
                    forward: key(c.forward_move),
                    side: key(c.side_move),
                    jump: c.buttons.contains(Buttons::JUMP),
                    use_: c.buttons.contains(Buttons::USE),
                    ..Action::IDLE
                });
            }
            let ep = Episode::from_env(&env, CourseSource::Toy { stage, seed: 3, end });
            let mut view = EpisodeView::new(ep.clone(), "test".into());
            let level = crate::level::from_episode(&view).unwrap();
            let mut sim = crate::sim::Sim::new(level.world, (level.info.areas[0].spawn, level.info.areas[0].yaw));
            for lap in 0..2 {
                view.reset(&mut sim);
                assert_eq!(sim.kind, ep.recording.mode);
                assert_eq!((sim.tickrate, sim.autohop), (ep.recording.tickrate, ep.recording.autobhop));
                while let Some(cmd) = view.next_cmd() {
                    sim.step(cmd);
                    view.run.update(&view.course, &sim.state, &cmd);
                }
                assert_eq!(state_fields(&sim.state), ep.final_state, "{stage:?} lap {lap}");
                assert_eq!((view.run.status, view.run.elapsed_ticks), (ep.status, ep.run_ticks));
                assert_eq!(view.run.next_waypoint, ep.waypoints_reached);
            }
        }
    }
}
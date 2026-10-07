//! In-app ghost overlay (plan §9.6): `csmove --ghost <capture>` drives the sim with a real-server
//! capture's command stream and draws the captured hull next to ours, with our value, the captured
//! value and the delta on the HUD. For visual debugging only; `compare` is the source of truth.

use bevy::prelude::*;
use compare::capture::{Capture, Row};
use compare::diff::state_from_row;
use movement::trace::Hull;
use movement::{PlayerState, UserCmd, Vec3 as SVec3};

use crate::camera::to_bevy;
use crate::sim::Sim;

#[derive(Resource)]
pub struct Ghost {
    pub cap: Capture,
    /// Index of the next command to run.
    pub cursor: usize,
    pub start: PlayerState,
}

impl Ghost {
    pub fn load(dir: &std::path::Path) -> Result<Ghost, String> {
        let cap = Capture::load(dir)?;
        let start = state_from_row(&cap.pre[0], &PlayerState::new(SVec3::ZERO));
        Ok(Ghost { cap, cursor: 0, start })
    }

    /// Put the sim at the capture's recorded start, in the capture's mode, tickrate and cvars.
    pub fn reset(&mut self, sim: &mut Sim) -> Result<(), String> {
        sim.set_mode(self.cap.mode()?);
        sim.tickrate = self.cap.tickrate()?;
        sim.cfg = self.cap.config()?;
        sim.autohop = sim.cfg.autobhop;
        sim.state = self.start.clone();
        sim.prev_state = self.start.clone();
        sim.spawn = (self.start.origin, self.cap.cmds[0].view_angles.y);
        sim.tick = 0;
        self.cursor = 0;
        Ok(())
    }

    /// The next command, or `None` at the end of the capture.
    pub fn next_cmd(&mut self) -> Option<UserCmd> {
        let c = self.cap.cmds.get(self.cursor).copied();
        if c.is_some() {
            self.cursor += 1;
        }
        c
    }

    /// Captured state matching the sim's current state (after the last command run).
    pub fn current(&self) -> &Row {
        if self.cursor == 0 {
            &self.cap.pre[0]
        } else {
            &self.cap.post[self.cursor - 1]
        }
    }
}

pub fn ghost_gizmos(ghost: Option<Res<Ghost>>, mut gizmos: Gizmos) {
    let Some(ghost) = ghost else { return };
    let r = ghost.current();
    let hull = if r.ducked == Some(true) { Hull::DUCK } else { Hull::STAND };
    let c = Color::srgb(1.0, 0.25, 0.85);
    let (lo, hi) = (r.origin.add(hull.mins), r.origin.add(hull.maxs));
    let v = |x: f32, y: f32, z: f32| to_bevy(SVec3::new(x, y, z));
    let xs = [lo.x, hi.x];
    let ys = [lo.y, hi.y];
    let zs = [lo.z, hi.z];
    for &z in &zs {
        gizmos.line(v(lo.x, lo.y, z), v(hi.x, lo.y, z), c);
        gizmos.line(v(hi.x, lo.y, z), v(hi.x, hi.y, z), c);
        gizmos.line(v(hi.x, hi.y, z), v(lo.x, hi.y, z), c);
        gizmos.line(v(lo.x, hi.y, z), v(lo.x, lo.y, z), c);
    }
    for &x in &xs {
        for &y in &ys {
            gizmos.line(v(x, y, lo.z), v(x, y, hi.z), c);
        }
    }
}

/// HUD lines: ours, captured, delta.
pub fn panel(ghost: &Ghost, s: &PlayerState) -> String {
    let r = ghost.current();
    let row = |name: &str, ours: f32, theirs: Option<f32>| match theirs {
        Some(t) => format!("{name:<8}{ours:>13.5}{t:>13.5}{:>+12.2e}\n", ours as f64 - t as f64),
        None => format!("{name:<8}{ours:>13.5}{:>13}\n", "n/a"),
    };
    let mut t = format!(
        "GHOST {}  tick {}/{}\n{:<8}{:>13}{:>13}{:>12}\n",
        ghost.cap.name(),
        ghost.cursor,
        ghost.cap.cmds.len(),
        "",
        "ours",
        "captured",
        "delta"
    );
    t += &row("speed", s.velocity.length_2d(), Some(r.velocity.length_2d()));
    t += &row("vz", s.velocity.z, Some(r.velocity.z));
    t += &row("x", s.origin.x, Some(r.origin.x));
    t += &row("y", s.origin.y, Some(r.origin.y));
    t += &row("z", s.origin.z, Some(r.origin.z));
    t += &row("stamina", s.stamina, r.stamina);
    t += &row("duck", s.duck_amount, r.duck_amount);
    t += &format!("ground  {:>13}{:>13}\n", s.on_ground(), r.ground >= 0);
    t
}

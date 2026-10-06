//! F5 recording: the `UserCmd` stream (bit-exact replay file), a per-tick CSV, and the final live state,
//! so `csmove --check <file.replay>` can confirm a headless replay matches the live run (M5 gate).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use movement::replay::{state_fields, Recording};
use movement::{ModeKind, PlayerState, UserCmd};

pub const DIR: &str = "recordings";

struct Active {
    rec: Recording,
    csv: String,
    last: PlayerState,
}

#[derive(Default)]
pub struct Recorder {
    active: Option<Active>,
    pub last_saved: Option<PathBuf>,
}

impl Recorder {
    pub fn is_recording(&self) -> bool {
        self.active.is_some()
    }

    pub fn start(&mut self, kind: ModeKind, tickrate: u32, autobhop: bool, start: &PlayerState) {
        let mut csv = String::from(
            "tick,origin_x,origin_y,origin_z,vel_x,vel_y,vel_z,ground,move_type,duck_amount,ducked,stamina,\
             surface_friction,fall_velocity,forward_move,side_move,buttons,pitch,yaw\n",
        );
        csv.reserve(1 << 16);
        let mut rec = Recording::new(kind, tickrate, start.clone());
        rec.autobhop = autobhop;
        self.active = Some(Active { rec, csv, last: start.clone() });
    }

    pub fn on_tick(&mut self, cmd: &UserCmd, state: &PlayerState) {
        let Some(a) = &mut self.active else { return };
        let mut c = *cmd;
        c.tick = a.rec.cmds.len() as u32;
        a.rec.cmds.push(c);
        let o = state.origin;
        let v = state.velocity;
        let _ = writeln!(
            a.csv,
            "{},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{},{:?},{:.9},{},{:.9},{:.9},{:.9},{},{},{},{:.9},{:.9}",
            c.tick,
            o.x,
            o.y,
            o.z,
            v.x,
            v.y,
            v.z,
            state.ground_entity.map_or(-1, |e| e.0 as i64),
            state.move_type,
            state.duck_amount,
            state.ducked as u8,
            state.stamina,
            state.surface_friction,
            state.fall_velocity,
            c.forward_move,
            c.side_move,
            c.buttons.0,
            c.view_angles.x,
            c.view_angles.y
        );
        a.last = state.clone();
    }

    /// Stop and write the files. Returns the `.replay` path.
    pub fn stop_if_recording(&mut self) -> Option<PathBuf> {
        let a = self.active.take()?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let dir = Path::new(DIR);
        if let Err(e) = std::fs::create_dir_all(dir) {
            bevy::log::error!("cannot create {DIR}: {e}");
            return None;
        }
        let base = dir.join(format!("{stamp}_{}", a.rec.mode.name()));
        let replay = base.with_extension("replay");
        let result = std::fs::write(&replay, a.rec.to_text())
            .and_then(|_| std::fs::write(base.with_extension("csv"), &a.csv))
            .and_then(|_| std::fs::write(base.with_extension("final"), state_fields(&a.last).join(" ")));
        match result {
            Ok(()) => {
                bevy::log::info!("recorded {} ticks to {}", a.rec.cmds.len(), replay.display());
                self.last_saved = Some(replay.clone());
                Some(replay)
            }
            Err(e) => {
                bevy::log::error!("writing recording failed: {e}");
                None
            }
        }
    }
}

/// Headless check: replay a recording against the test level and compare with the saved final state.
pub fn check(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let rec = Recording::from_text(&text)?;
    let level = crate::level::describe();
    let world = crate::level::build_world(&level);
    let end = rec.replay(&world);
    let got = state_fields(&end).join(" ");
    let final_path = path.with_extension("final");
    let want = std::fs::read_to_string(&final_path).map_err(|e| format!("{}: {e}", final_path.display()))?;
    if got == want.trim() {
        Ok(format!("BIT_EXACT: {} commands replayed, final state identical", rec.cmds.len()))
    } else {
        Err(format!("MISMATCH after {} commands\n live:   {}\n replay: {}", rec.cmds.len(), want.trim(), got))
    }
}

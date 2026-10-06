//! Record and replay `UserCmd` streams. Floats are stored as raw bit patterns so a replay is bit-exact.
//!
//! Format (text, one record per line):
//!
//! ```text
//! csmove-replay 1
//! mode Vanilla
//! tickrate 64
//! autobhop 0
//! state <PlayerState fields, see `state_fields`>
//! cmd <tick> <forward> <side> <up> <buttons> <pitch> <yaw> <roll>
//! ```

use crate::cmd::{Buttons, UserCmd};
use crate::instrument::MoveObserver;
use crate::math::Vec3;
use crate::modes::ModeKind;
use crate::pipeline::process_movement;
use crate::state::{MoveType, PlayerState};
use crate::trace::{EntityId, TraceWorld};

#[derive(Clone, Debug, PartialEq)]
pub struct Recording {
    pub mode: ModeKind,
    pub tickrate: u32,
    /// Config autobhop flag in effect for the whole recording.
    pub autobhop: bool,
    pub start: PlayerState,
    pub cmds: Vec<UserCmd>,
}

impl Recording {
    pub fn new(mode: ModeKind, tickrate: u32, start: PlayerState) -> Self {
        Self { mode, tickrate, autobhop: false, start, cmds: Vec::new() }
    }

    /// Replay from the start state with a fresh mode, calling `per_tick` after every command.
    pub fn replay_with<W: TraceWorld + ?Sized, O: MoveObserver>(
        &self,
        world: &W,
        obs: &mut O,
        mut per_tick: impl FnMut(&PlayerState),
    ) -> PlayerState {
        let mut mode = self.mode.create();
        let mut cfg = *mode.config();
        cfg.autobhop = self.autobhop;
        let dt = crate::tick_interval(self.tickrate);
        let mut state = self.start.clone();
        for cmd in &self.cmds {
            process_movement(&cfg, mode.as_mut(), world, &mut state, cmd, obs, dt);
            per_tick(&state);
        }
        state
    }

    pub fn replay<W: TraceWorld + ?Sized>(&self, world: &W) -> PlayerState {
        self.replay_with(world, &mut crate::instrument::NullObserver, |_| {})
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("csmove-replay 1\n");
        out.push_str(&format!("mode {:?}\n", self.mode));
        out.push_str(&format!("tickrate {}\n", self.tickrate));
        out.push_str(&format!("autobhop {}\n", self.autobhop as u8));
        out.push_str("state");
        for f in state_fields(&self.start) {
            out.push(' ');
            out.push_str(&f);
        }
        out.push('\n');
        for c in &self.cmds {
            out.push_str(&format!(
                "cmd {} {} {} {} {} {} {} {}\n",
                c.tick,
                hex(c.forward_move),
                hex(c.side_move),
                hex(c.up_move),
                c.buttons.0,
                hex(c.view_angles.x),
                hex(c.view_angles.y),
                hex(c.view_angles.z)
            ));
        }
        out
    }

    pub fn from_text(text: &str) -> Result<Recording, String> {
        let mut lines = text.lines();
        if lines.next().map(str::trim) != Some("csmove-replay 1") {
            return Err("not a csmove-replay v1 file".into());
        }
        let mut mode = None;
        let mut tickrate = None;
        let mut autobhop = false;
        let mut start = None;
        let mut cmds = Vec::new();
        for line in lines {
            let mut p = line.split_whitespace();
            match p.next() {
                Some("mode") => {
                    mode = Some(match p.next() {
                        Some("Vanilla") => ModeKind::Vanilla,
                        Some("KzTimer") => ModeKind::KzTimer,
                        Some("SimpleKz") => ModeKind::SimpleKz,
                        m => return Err(format!("unknown mode {m:?}")),
                    })
                }
                Some("tickrate") => tickrate = Some(p.next().ok_or("tickrate")?.parse::<u32>().map_err(|e| e.to_string())?),
                Some("autobhop") => autobhop = p.next() == Some("1"),
                Some("state") => start = Some(parse_state(&p.collect::<Vec<_>>())?),
                Some("cmd") => {
                    let f: Vec<&str> = p.collect();
                    if f.len() != 8 {
                        return Err(format!("bad cmd line `{line}`"));
                    }
                    cmds.push(UserCmd {
                        tick: f[0].parse().map_err(|_| "tick")?,
                        forward_move: unhex(f[1])?,
                        side_move: unhex(f[2])?,
                        up_move: unhex(f[3])?,
                        buttons: Buttons(f[4].parse().map_err(|_| "buttons")?),
                        view_angles: Vec3::new(unhex(f[5])?, unhex(f[6])?, unhex(f[7])?),
                    });
                }
                None => {}
                Some(other) => return Err(format!("unknown record `{other}`")),
            }
        }
        Ok(Recording {
            mode: mode.ok_or("missing mode")?,
            tickrate: tickrate.ok_or("missing tickrate")?,
            autobhop,
            start: start.ok_or("missing state")?,
            cmds,
        })
    }
}

pub fn hex(f: f32) -> String {
    format!("{:08x}", f.to_bits())
}

pub fn unhex(s: &str) -> Result<f32, String> {
    u32::from_str_radix(s, 16).map(f32::from_bits).map_err(|e| format!("`{s}`: {e}"))
}

fn vec_fields(v: Vec3) -> [String; 3] {
    [hex(v.x), hex(v.y), hex(v.z)]
}

/// PlayerState as an ordered list of tokens.
pub fn state_fields(s: &PlayerState) -> Vec<String> {
    let mut f = vec![s.tick.to_string()];
    f.extend(vec_fields(s.origin));
    f.extend(vec_fields(s.velocity));
    f.extend(vec_fields(s.base_velocity));
    f.push(s.ground_entity.map_or("-".to_string(), |e| e.0.to_string()));
    f.push(match s.move_type {
        MoveType::Walk => "walk".into(),
        MoveType::Ladder => "ladder".into(),
    });
    f.push(s.old_buttons.0.to_string());
    for x in [s.surface_friction, s.stamina, s.duck_amount, s.duck_speed] {
        f.push(hex(x));
    }
    f.push((s.ducked as u8).to_string());
    f.push((s.ducking as u8).to_string());
    f.push(hex(s.fall_velocity));
    f.extend(vec_fields(s.ladder_normal));
    f.push(hex(s.ladder_jump_ignore));
    f
}

fn parse_state(f: &[&str]) -> Result<PlayerState, String> {
    if f.len() != 24 {
        return Err(format!("state needs 24 fields, got {}", f.len()));
    }
    let v = |i: usize| -> Result<Vec3, String> { Ok(Vec3::new(unhex(f[i])?, unhex(f[i + 1])?, unhex(f[i + 2])?)) };
    let b = |i: usize| f[i] == "1";
    Ok(PlayerState {
        tick: f[0].parse().map_err(|_| "tick")?,
        origin: v(1)?,
        velocity: v(4)?,
        base_velocity: v(7)?,
        ground_entity: if f[10] == "-" { None } else { Some(EntityId(f[10].parse().map_err(|_| "ground")?)) },
        move_type: if f[11] == "ladder" { MoveType::Ladder } else { MoveType::Walk },
        old_buttons: Buttons(f[12].parse().map_err(|_| "old_buttons")?),
        surface_friction: unhex(f[13])?,
        stamina: unhex(f[14])?,
        duck_amount: unhex(f[15])?,
        duck_speed: unhex(f[16])?,
        ducked: b(17),
        ducking: b(18),
        fall_velocity: unhex(f[19])?,
        ladder_normal: v(20)?,
        ladder_jump_ignore: unhex(f[23])?,
    })
}

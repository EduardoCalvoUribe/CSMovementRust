//! Capture folder format, version 1 (plan §9.3).
//!
//! ```text
//! <capture>/meta.toml      build, map, tickrate, mode, versions, unavailable columns, [cvars]
//! <capture>/scenario.toml  requested start, geometry, settle ticks
//! <capture>/cmds.csv       the injected stream, Source button bits
//! <capture>/states.csv     a `pre` and a `post` row per injected command
//! ```
//!
//! Every float column `x` is written twice: `x` (decimal, for reading) and `x_bits` (raw IEEE-754 bits
//! as 8 hex digits). Readers use the bits when present, so there is no text-rounding loss.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use movement::cmd::{Buttons, UserCmd};
use movement::{ModeKind, MovementConfig, Vec3};

use crate::kv::Kv;

pub const FORMAT: u32 = 1;

/// Source `IN_*` button bits (SDK 2013 `in_buttons.h`).
pub mod in_bits {
    pub const ATTACK: u32 = 1 << 0;
    pub const JUMP: u32 = 1 << 1;
    pub const DUCK: u32 = 1 << 2;
    pub const FORWARD: u32 = 1 << 3;
    pub const BACK: u32 = 1 << 4;
    pub const USE: u32 = 1 << 5;
    pub const MOVELEFT: u32 = 1 << 9;
    pub const MOVERIGHT: u32 = 1 << 10;
    /// Shift-walk in CS:GO.
    pub const SPEED: u32 = 1 << 17;
}

const BUTTON_MAP: [(Buttons, u32); 8] = [
    (Buttons::JUMP, in_bits::JUMP),
    (Buttons::DUCK, in_bits::DUCK),
    (Buttons::FORWARD, in_bits::FORWARD),
    (Buttons::BACK, in_bits::BACK),
    (Buttons::LEFT, in_bits::MOVELEFT),
    (Buttons::RIGHT, in_bits::MOVERIGHT),
    (Buttons::WALK, in_bits::SPEED),
    (Buttons::USE, in_bits::USE),
];

pub fn to_source_buttons(b: Buttons) -> u32 {
    BUTTON_MAP.iter().filter(|(ours, _)| b.contains(*ours)).fold(0, |acc, (_, src)| acc | src)
}

pub fn from_source_buttons(bits: u32) -> Buttons {
    BUTTON_MAP.iter().filter(|(_, src)| bits & src != 0).fold(Buttons::NONE, |acc, (ours, _)| acc | *ours)
}

pub fn hex(f: f32) -> String {
    format!("{:08x}", f.to_bits())
}

/// Source `MoveType_t` values that matter here.
pub const MOVETYPE_WALK: i32 = 2;
pub const MOVETYPE_LADDER: i32 = 9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Pre,
    Post,
}

/// One logged player state. `None` = the column was unavailable (empty cell).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub tick: u32,
    pub cmdnum: Option<i64>,
    pub server_tick: Option<i64>,
    pub origin: Vec3,
    pub velocity: Vec3,
    pub base_velocity: Option<Vec3>,
    pub angles: Option<Vec3>,
    /// Ground entity index, -1 for none.
    pub ground: i32,
    pub flags: Option<i32>,
    pub move_type: Option<i32>,
    pub ducked: Option<bool>,
    pub ducking: Option<bool>,
    pub duck_amount: Option<f32>,
    pub duck_speed: Option<f32>,
    pub stamina: Option<f32>,
    pub surface_friction: Option<f32>,
    pub max_speed: Option<f32>,
    pub fall_velocity: Option<f32>,
    pub old_buttons: Option<u32>,
    pub ladder_normal: Option<Vec3>,
}

#[derive(Clone, Debug)]
pub struct Capture {
    pub dir: PathBuf,
    pub meta: Kv,
    pub scenario: Kv,
    pub cmds: Vec<UserCmd>,
    pub pre: Vec<Row>,
    pub post: Vec<Row>,
}

impl Capture {
    pub fn name(&self) -> String {
        self.dir.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    }

    pub fn tickrate(&self) -> Result<u32, String> {
        self.meta.u32("tickrate")
    }

    pub fn mode(&self) -> Result<ModeKind, String> {
        parse_mode(self.meta.req("mode")?)
    }

    pub fn load(dir: &Path) -> Result<Capture, String> {
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).map_err(|e| format!("{}: {e}", dir.join(f).display()));
        let meta = Kv::parse(&read("meta.toml")?)?;
        let scenario = Kv::parse(&read("scenario.toml")?)?;
        let fmt = meta.u32("format")?;
        if fmt != FORMAT {
            return Err(format!("capture format {fmt}, expected {FORMAT}"));
        }
        let cmds = parse_cmds(&read("cmds.csv")?)?;
        let (pre, post) = parse_states(&read("states.csv")?)?;
        let c = Capture { dir: dir.to_path_buf(), meta, scenario, cmds, pre, post };
        c.validate()?;
        Ok(c)
    }

    /// Plan §9.6 step 1: columns, tick contiguity, no dropped commands.
    pub fn validate(&self) -> Result<(), String> {
        let n = self.cmds.len();
        if self.pre.len() != n || self.post.len() != n {
            return Err(format!("{} cmds but {} pre and {} post rows", n, self.pre.len(), self.post.len()));
        }
        for (k, (a, b)) in self.pre.iter().zip(&self.post).enumerate() {
            if a.tick as usize != k || b.tick as usize != k || self.cmds[k].tick as usize != k {
                return Err(format!("tick {k}: rows or cmds are not contiguous"));
            }
        }
        // A command-number gap means the server dropped a command [plan §9.4]. Server ticks may repeat
        // or skip for a real client (two commands in one frame), so only command numbers are checked.
        for k in 1..n {
            if let (Some(p), Some(c)) = (self.post[k - 1].cmdnum, self.post[k].cmdnum) {
                if c != p + 1 {
                    return Err(format!("cmdnum gap at tick {k}: {p} -> {c} (dropped command, discard run)"));
                }
            }
        }
        Ok(())
    }

    /// Movement config from the recorded cvar dump, so a server with different values still compares
    /// fairly (plan §9.6 step 2).
    pub fn config(&self) -> Result<MovementConfig, String> {
        let mut cfg = *self.mode()?.create().config();
        apply_cvars(&mut cfg, self.meta.section("cvars"))?;
        if let Ok(v) = self.meta.f32("weapon_max_speed") {
            cfg.weapon_max_speed = v;
        }
        if let Some(v) = self.pre.first().and_then(|r| r.max_speed) {
            cfg.player_max_speed = v;
        }
        Ok(cfg)
    }

    /// Columns the server could not provide, as listed in `meta.toml`.
    pub fn unavailable(&self) -> Vec<String> {
        self.meta
            .get("unavailable")
            .map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
            .unwrap_or_default()
    }
}

pub fn parse_mode(s: &str) -> Result<ModeKind, String> {
    match s.to_ascii_lowercase().as_str() {
        "vanilla" => Ok(ModeKind::Vanilla),
        "kztimer" => Ok(ModeKind::KzTimer),
        "simplekz" => Ok(ModeKind::SimpleKz),
        m => Err(format!("unknown mode `{m}`")),
    }
}

pub fn mode_name(m: ModeKind) -> &'static str {
    match m {
        ModeKind::Vanilla => "vanilla",
        ModeKind::KzTimer => "kztimer",
        ModeKind::SimpleKz => "simplekz",
    }
}

fn apply_cvars<'a>(cfg: &mut MovementConfig, cvars: impl Iterator<Item = (&'a str, &'a str)>) -> Result<(), String> {
    for (name, value) in cvars {
        let f = || value.parse::<f32>().map_err(|e| format!("cvar {name} = `{value}`: {e}"));
        match name {
            "sv_gravity" => cfg.gravity = f()?,
            "sv_jump_impulse" => cfg.jump_impulse = f()?,
            "sv_accelerate" => cfg.accelerate = f()?,
            "sv_airaccelerate" => cfg.air_accelerate = f()?,
            "sv_air_max_wishspeed" => cfg.air_wish_cap = f()?,
            "sv_friction" => cfg.friction = f()?,
            "sv_stopspeed" => cfg.stop_speed = f()?,
            "sv_stepsize" => cfg.step_size = f()?,
            "sv_maxvelocity" => cfg.max_component_velocity = f()?,
            "sv_staminajumpcost" => cfg.stamina_jump_cost = f()?,
            "sv_staminalandcost" => cfg.stamina_land_cost = f()?,
            "sv_staminarecoveryrate" => cfg.stamina_recovery_rate = f()?,
            "sv_staminamax" => cfg.stamina_max = f()?,
            "sv_enablebunnyhopping" => cfg.enable_bunnyhopping = f()? != 0.0,
            "sv_autobunnyhopping" => cfg.autobhop = f()? != 0.0,
            "sv_ladder_scale_speed" => cfg.ladder_scale = f()?,
            "sv_bounce" => cfg.bounce = f()?,
            "sv_accelerate_use_weapon_speed" => cfg.weapon_speed_scaling = f()? != 0.0,
            _ => {}
        }
    }
    Ok(())
}

/// CSV with a header; returns rows as maps from column name to cell.
/// A CSV's header and its rows, each row keyed by column name.
type Table = (Vec<String>, Vec<HashMap<String, String>>);

fn parse_csv(text: &str) -> Result<Table, String> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header: Vec<String> = lines.next().ok_or("empty csv")?.split(',').map(|s| s.trim().to_string()).collect();
    let mut rows = Vec::new();
    for (n, line) in lines.enumerate() {
        let cells: Vec<&str> = line.split(',').collect();
        if cells.len() != header.len() {
            return Err(format!("row {}: {} cells, header has {}", n + 1, cells.len(), header.len()));
        }
        rows.push(header.iter().cloned().zip(cells.iter().map(|c| c.trim().to_string())).collect());
    }
    Ok((header, rows))
}

struct Cells<'a>(&'a HashMap<String, String>);

impl Cells<'_> {
    fn raw(&self, k: &str) -> Option<&str> {
        self.0.get(k).map(String::as_str).filter(|s| !s.is_empty())
    }
    /// Float from `k_bits` if present, else from the decimal column. `None` if unavailable.
    fn f(&self, k: &str) -> Result<Option<f32>, String> {
        if let Some(b) = self.raw(&format!("{k}_bits")) {
            return u32::from_str_radix(b, 16).map(|u| Some(f32::from_bits(u))).map_err(|e| format!("{k}_bits `{b}`: {e}"));
        }
        match self.raw(k) {
            Some(s) => s.parse::<f32>().map(Some).map_err(|e| format!("{k} `{s}`: {e}")),
            None => Ok(None),
        }
    }
    fn freq(&self, k: &str) -> Result<f32, String> {
        self.f(k)?.ok_or_else(|| format!("missing column {k}"))
    }
    fn v(&self, p: &str) -> Result<Option<Vec3>, String> {
        match (self.f(&format!("{p}_x"))?, self.f(&format!("{p}_y"))?, self.f(&format!("{p}_z"))?) {
            (Some(x), Some(y), Some(z)) => Ok(Some(Vec3::new(x, y, z))),
            _ => Ok(None),
        }
    }
    fn vreq(&self, p: &str) -> Result<Vec3, String> {
        self.v(p)?.ok_or_else(|| format!("missing column {p}_x/y/z"))
    }
    fn i(&self, k: &str) -> Result<Option<i64>, String> {
        self.raw(k).map(|s| s.parse::<i64>().map_err(|e| format!("{k} `{s}`: {e}"))).transpose()
    }
    fn ireq(&self, k: &str) -> Result<i64, String> {
        self.i(k)?.ok_or_else(|| format!("missing column {k}"))
    }
}

pub fn parse_cmds(text: &str) -> Result<Vec<UserCmd>, String> {
    let (_, rows) = parse_csv(text)?;
    rows.iter()
        .map(|r| {
            let c = Cells(r);
            Ok(UserCmd {
                tick: c.ireq("tick")? as u32,
                forward_move: c.freq("forward_move")?,
                side_move: c.freq("side_move")?,
                up_move: c.freq("up_move")?,
                buttons: from_source_buttons(c.ireq("buttons")? as u32),
                view_angles: Vec3::new(c.freq("pitch")?, c.freq("yaw")?, c.freq("roll")?),
            })
        })
        .collect()
}

pub fn write_cmds(cmds: &[UserCmd]) -> String {
    let mut s = String::from(
        "tick,forward_move,side_move,up_move,buttons,pitch,yaw,roll,impulse,\
         forward_move_bits,side_move_bits,up_move_bits,pitch_bits,yaw_bits,roll_bits\n",
    );
    for c in cmds {
        let a = c.view_angles;
        let _ = writeln!(
            s,
            "{},{:.9},{:.9},{:.9},{},{:.9},{:.9},{:.9},0,{},{},{},{},{},{}",
            c.tick,
            c.forward_move,
            c.side_move,
            c.up_move,
            to_source_buttons(c.buttons),
            a.x,
            a.y,
            a.z,
            hex(c.forward_move),
            hex(c.side_move),
            hex(c.up_move),
            hex(a.x),
            hex(a.y),
            hex(a.z)
        );
    }
    s
}

pub fn parse_states(text: &str) -> Result<(Vec<Row>, Vec<Row>), String> {
    let (_, rows) = parse_csv(text)?;
    let mut pre = Vec::new();
    let mut post = Vec::new();
    for r in &rows {
        let c = Cells(r);
        let b = |k: &str| -> Result<Option<bool>, String> { Ok(c.i(k)?.map(|v| v != 0)) };
        let row = Row {
            tick: c.ireq("tick")? as u32,
            cmdnum: c.i("cmdnum")?,
            server_tick: c.i("server_tick")?,
            origin: c.vreq("origin")?,
            velocity: c.vreq("vel")?,
            base_velocity: c.v("basevel")?,
            angles: match (c.f("pitch")?, c.f("yaw")?, c.f("roll")?) {
                (Some(p), Some(y), Some(r)) => Some(Vec3::new(p, y, r)),
                _ => None,
            },
            ground: c.ireq("ground")? as i32,
            flags: c.i("flags")?.map(|v| v as i32),
            move_type: c.i("move_type")?.map(|v| v as i32),
            ducked: b("ducked")?,
            ducking: b("ducking")?,
            duck_amount: c.f("duck_amount")?,
            duck_speed: c.f("duck_speed")?,
            stamina: c.f("stamina")?,
            surface_friction: c.f("surface_friction")?,
            max_speed: c.f("max_speed")?,
            fall_velocity: c.f("fall_velocity")?,
            old_buttons: c.i("old_buttons")?.map(|v| v as u32),
            ladder_normal: c.v("ladder_n")?,
        };
        match c.raw("phase") {
            Some("pre") => pre.push(row),
            Some("post") => post.push(row),
            p => return Err(format!("bad phase {p:?}")),
        }
    }
    Ok((pre, post))
}

/// Header of `states.csv`, shared by the plugin and by our own writer.
pub const STATES_HEADER: &str = "tick,phase,cmdnum,server_tick,\
origin_x,origin_y,origin_z,vel_x,vel_y,vel_z,basevel_x,basevel_y,basevel_z,pitch,yaw,roll,\
ground,flags,move_type,ducked,ducking,duck_amount,duck_speed,stamina,surface_friction,max_speed,fall_velocity,\
old_buttons,ladder_n_x,ladder_n_y,ladder_n_z,\
origin_x_bits,origin_y_bits,origin_z_bits,vel_x_bits,vel_y_bits,vel_z_bits,basevel_x_bits,basevel_y_bits,basevel_z_bits,\
pitch_bits,yaw_bits,roll_bits,duck_amount_bits,duck_speed_bits,stamina_bits,surface_friction_bits,max_speed_bits,\
fall_velocity_bits,ladder_n_x_bits,ladder_n_y_bits,ladder_n_z_bits";

/// Write rows in the capture format (used for synthetic captures in tests and for our side's dump).
pub fn write_states(pre: &[Row], post: &[Row]) -> String {
    let mut s = String::from(STATES_HEADER);
    s.push('\n');
    let of = |v: Option<f32>| v.map_or(String::new(), |x| format!("{x:.9}"));
    let ob = |v: Option<f32>| v.map_or(String::new(), hex);
    let oi = |v: Option<i64>| v.map_or(String::new(), |x| x.to_string());
    for (k, (a, b)) in pre.iter().zip(post).enumerate() {
        for (phase, r) in [("pre", a), ("post", b)] {
            let bv = r.base_velocity;
            let an = r.angles;
            let ln = r.ladder_normal;
            let floats = [
                Some(r.origin.x),
                Some(r.origin.y),
                Some(r.origin.z),
                Some(r.velocity.x),
                Some(r.velocity.y),
                Some(r.velocity.z),
                bv.map(|v| v.x),
                bv.map(|v| v.y),
                bv.map(|v| v.z),
                an.map(|v| v.x),
                an.map(|v| v.y),
                an.map(|v| v.z),
            ];
            let tail = [r.duck_amount, r.duck_speed, r.stamina, r.surface_friction, r.max_speed, r.fall_velocity];
            let lnf = [ln.map(|v| v.x), ln.map(|v| v.y), ln.map(|v| v.z)];
            let mut cells: Vec<String> = vec![k.to_string(), phase.into(), oi(r.cmdnum), oi(r.server_tick)];
            cells.extend(floats.iter().map(|v| of(*v)));
            cells.push(r.ground.to_string());
            cells.push(oi(r.flags.map(i64::from)));
            cells.push(oi(r.move_type.map(i64::from)));
            cells.push(oi(r.ducked.map(i64::from)));
            cells.push(oi(r.ducking.map(i64::from)));
            cells.extend(tail.iter().map(|v| of(*v)));
            cells.push(oi(r.old_buttons.map(i64::from)));
            cells.extend(lnf.iter().map(|v| of(*v)));
            cells.extend(floats.iter().map(|v| ob(*v)));
            cells.extend(tail.iter().map(|v| ob(*v)));
            cells.extend(lnf.iter().map(|v| ob(*v)));
            s.push_str(&cells.join(","));
            s.push('\n');
        }
    }
    s
}

/// Drop the decimal copy of every float column that also has a `_bits` column.
pub fn compact_csv(text: &str) -> String {
    let mut lines = text.lines();
    let Some(header) = lines.next() else { return String::new() };
    let cols: Vec<&str> = header.split(',').collect();
    let keep: Vec<bool> = cols.iter().map(|c| !cols.contains(&format!("{c}_bits").as_str())).collect();
    let pick = |line: &str| -> String {
        line.split(',').zip(&keep).filter(|(_, k)| **k).map(|(c, _)| c).collect::<Vec<_>>().join(",")
    };
    let mut out = pick(header);
    out.push('\n');
    for l in lines.filter(|l| !l.trim().is_empty()) {
        out.push_str(&pick(l));
        out.push('\n');
    }
    out
}

/// FNV-1a over the level description, so a capture records which geometry it ran on.
pub fn level_hash(level: &testlevel::Level) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in &level.brushes {
        for byte in format!("{:?}|{:?};", b.shape, b.contents).bytes() {
            h ^= byte as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_round_trip() {
        for bits in 0..256u32 {
            let b = Buttons(bits);
            assert_eq!(from_source_buttons(to_source_buttons(b)), b);
        }
        assert_eq!(to_source_buttons(Buttons::JUMP | Buttons::WALK), in_bits::JUMP | in_bits::SPEED);
    }

    #[test]
    fn cmds_round_trip_bit_exact() {
        let cmds = vec![UserCmd {
            tick: 0,
            view_angles: Vec3::new(-0.1, 123.456_79, 0.0),
            forward_move: 450.0,
            side_move: -0.000_1,
            up_move: 0.0,
            buttons: Buttons::FORWARD | Buttons::JUMP,
        }];
        assert_eq!(parse_cmds(&write_cmds(&cmds)).unwrap(), cmds);
    }

    #[test]
    fn states_round_trip_bit_exact() {
        let r = Row {
            tick: 0,
            cmdnum: Some(5),
            server_tick: Some(77),
            origin: Vec3::new(1.0e-7, -3.3, 0.031_25),
            velocity: Vec3::new(250.0, 0.0, -6.25),
            ground: 0,
            stamina: Some(0.1),
            ducked: Some(true),
            ..Row::default()
        };
        let (pre, post) = parse_states(&write_states(&[r.clone()], &[r.clone()])).unwrap();
        assert_eq!(pre[0], r);
        assert_eq!(post[0], r);
    }
}

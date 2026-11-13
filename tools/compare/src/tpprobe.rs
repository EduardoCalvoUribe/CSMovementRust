//! `trigger_teleport` measurement on the real server (docs/reinforcement-plan.md R7: KZ maps chain their
//! stages with teleports).
//!
//! A flat map with `trigger_teleport`s using the keyvalues of kz_longjumps_v4096's stage teleports
//! (`spawnflags 4097`, an `info_teleport_destination` target one unit above the floor), and scenarios that
//! walk, run, jump, and duck into them. The plugin logs the trigger's `OnStartTouch` with the command it
//! happened in and the eye angles after it (events.csv); the per-command states show where the player
//! ended up and with what velocity. `check` replays every scenario through our sim plus
//! `course::Course::touch_teleports` and diffs it against the capture, and checks GOKZ's timer across a
//! teleport.

use std::path::Path;

use movement::world::Brush;
use movement::{Buttons, ModeKind, UserCmd, Vec3};

use crate::scenarios::Scenario;
use crate::vmf::{BrushEntity, PointEntity, TRIGGER_MATERIAL};

pub const MAP: &str = "csmove_tpprobe";

const fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3 { x, y, z }
}

pub struct Teleport {
    pub name: &'static str,
    pub min: Vec3,
    pub max: Vec3,
    pub spawnflags: u32,
    pub dest: Vec3,
    /// Destination angles (pitch yaw roll).
    pub angles: Vec3,
    /// The y of the lane the player walks along.
    pub lane_y: f32,
}

/// Clients only (1) plus 4096, as on kz_longjumps_v4096; 32 = preserve angles.
const KZ_FLAGS: u32 = 4097;

pub const TELEPORTS: [Teleport; 5] = [
    // A wall-like volume across lane 0, landing facing +y.
    Teleport { name: "a", min: v(400.0, -64.0, 0.0), max: v(432.0, 64.0, 128.0), spawnflags: KZ_FLAGS, dest: v(1500.0, 0.0, 1.0), angles: v(0.0, 90.0, 0.0), lane_y: 0.0 },
    // Preserve angles.
    Teleport { name: "b", min: v(400.0, 336.0, 0.0), max: v(432.0, 464.0, 128.0), spawnflags: KZ_FLAGS | 32, dest: v(1500.0, 400.0, 1.0), angles: v(0.0, 180.0, 0.0), lane_y: 400.0 },
    // A 4-unit floor slab (the KZ map's stage teleports are 8-unit slabs), destination with a pitch.
    Teleport { name: "c", min: v(400.0, -464.0, 0.0), max: v(832.0, -336.0, 4.0), spawnflags: KZ_FLAGS, dest: v(1500.0, -400.0, 1.0), angles: v(30.0, 45.0, 0.0), lane_y: -400.0 },
    // GOKZ: run from the start button through a teleport to the end button.
    Teleport { name: "g", min: v(400.0, -864.0, 0.0), max: v(432.0, -736.0, 128.0), spawnflags: KZ_FLAGS, dest: v(1500.0, -800.0, 1.0), angles: v(0.0, 0.0, 0.0), lane_y: -800.0 },
    // GOKZ: arrive next to a start button and press it right away.
    Teleport { name: "h", min: v(400.0, 736.0, 0.0), max: v(432.0, 864.0, 128.0), spawnflags: KZ_FLAGS, dest: v(1500.0, 800.0, 1.0), angles: v(0.0, 0.0, 0.0), lane_y: 800.0 },
];

/// GOKZ buttons: (name, min, max). Two start buttons share the name, as GOKZ allows.
pub const BUTTONS: [(&str, Vec3, Vec3); 3] = [
    ("climb_startbutton", v(0.0, -808.0, 40.0), v(16.0, -792.0, 56.0)),
    ("climb_endbutton", v(1700.0, -808.0, 40.0), v(1716.0, -792.0, 56.0)),
    ("climb_startbutton", v(1540.0, 792.0, 40.0), v(1556.0, 808.0, 56.0)),
];

pub fn teleport(name: &str) -> &'static Teleport {
    TELEPORTS.iter().find(|t| t.name == name).unwrap()
}

fn floor() -> Brush {
    Brush::cuboid(v(-640.0, -960.0, -16.0), v(2048.0, 1152.0, 0.0))
}

pub fn level() -> (testlevel::Level, Vec<BrushEntity>, Vec<PointEntity>) {
    let level = testlevel::Level {
        brushes: vec![floor()],
        signs: Vec::new(),
        areas: vec![testlevel::Area { name: "teleport probe", spawn: v(150.0, 0.0, 0.03125), yaw: 0.0 }],
    };
    let mut ents: Vec<BrushEntity> = TELEPORTS
        .iter()
        .map(|t| BrushEntity {
            class: "trigger_teleport",
            keys: vec![
                ("targetname", format!("tp_{}", t.name)),
                ("target", format!("dest_{}", t.name)),
                ("spawnflags", t.spawnflags.to_string()),
                ("StartDisabled", "0".into()),
                ("origin", format!("{} {} {}", (t.min.x + t.max.x) / 2.0, (t.min.y + t.max.y) / 2.0, (t.min.z + t.max.z) / 2.0)),
            ],
            boxes: vec![(t.min, t.max)],
            material: Some(TRIGGER_MATERIAL),
        })
        .collect();
    for (name, min, max) in BUTTONS {
        ents.push(BrushEntity {
            class: "func_button",
            keys: vec![
                ("targetname", name.to_string()),
                ("spawnflags", "1025".into()),
                ("wait", "0.0001".into()),
                ("speed", "5".into()),
                ("sounds", "0".into()),
                ("movedir", "0 0 0".into()),
                ("min_use_angle", "-1".into()),
            ],
            boxes: vec![(min, max)],
            material: None,
        });
    }
    let points = TELEPORTS
        .iter()
        .map(|t| PointEntity {
            class: "info_teleport_destination",
            keys: vec![
                ("targetname", format!("dest_{}", t.name)),
                ("origin", format!("{} {} {}", t.dest.x, t.dest.y, t.dest.z)),
                ("angles", format!("{} {} {}", t.angles.x, t.angles.y, t.angles.z)),
            ],
        })
        .collect();
    (level, ents, points)
}

fn cmd(k: u32, yaw: f32, fwd: f32, buttons: Buttons) -> UserCmd {
    UserCmd { tick: k, view_angles: Vec3::new(0.0, yaw, 0.0), forward_move: fwd, side_move: 0.0, up_move: 0.0, buttons }
}

fn scenario(id: &str, x: f32, lane: f32, yaw: f32, cmds: Vec<UserCmd>, isolates: &'static str) -> Scenario {
    Scenario {
        id: id.into(),
        mode: ModeKind::SimpleKz,
        tickrate: 128,
        origin: Vec3::new(x, lane, 1.0),
        yaw,
        settle: 64,
        cmds,
        isolates,
        map: Some(MAP.into()),
    }
}

/// Start at `x` on `t`'s lane facing the trigger and hold forward at `fwd` for `n` commands, with
/// `extra(k)` buttons.
fn walk(id: &str, t: &Teleport, x: f32, fwd: f32, n: u32, extra: impl Fn(u32) -> Buttons, why: &'static str) -> Scenario {
    let yaw = if x > t.max.x { 180.0 } else { 0.0 };
    let cmds = (0..n).map(|k| cmd(k, yaw, fwd, extra(k))).collect();
    scenario(id, x, t.lane_y, yaw, cmds, why)
}

fn tap(k: u32, phase: u32) -> Buttons {
    if k % 2 == phase {
        Buttons::USE
    } else {
        Buttons::NONE
    }
}

pub fn scenarios() -> Vec<Scenario> {
    let (a, b, c) = (teleport("a"), teleport("b"), teleport("c"));
    let none = |_| Buttons::NONE;
    let mut out = vec![
        walk("TPa_slow", a, 300.0, 40.0, 500, none, "teleport: touch boundary, slow walk-in"),
        walk("TPa_crawl", a, 376.0, 10.0, 400, none, "teleport: touch boundary, 0.08 units per command"),
        walk("TPa_run", a, 100.0, 250.0, 300, none, "teleport: velocity and ground state after a teleport"),
        walk("TPa_jump", a, 100.0, 250.0, 300, |k| if k == 135 { Buttons::JUMP } else { Buttons::NONE }, "teleport: entered in the air"),
        walk("TPa_duck", a, 300.0, 250.0, 400, |_| Buttons::DUCK, "teleport: entered ducked"),
        walk("TPa_back", a, 520.0, 40.0, 500, none, "teleport: touch boundary from the far side"),
        walk("TPb_run", b, 100.0, 250.0, 300, none, "teleport: preserve-angles flag"),
        walk("TPc_walk", c, 300.0, 250.0, 300, none, "teleport: walking onto a floor slab"),
        walk("TPc_jump", c, 100.0, 250.0, 300, |k| if k == 115 { Buttons::JUMP } else { Buttons::NONE }, "teleport: landing on a floor slab"),
    ];
    // GOKZ: press the start button behind, run through the teleport, press the end button.
    let g = teleport("g");
    let cmds = (0..520)
        .map(|k| match k {
            0..=7 => cmd(k, 0.0, 0.0, Buttons::NONE),
            8 => cmd(k, 0.0, 0.0, Buttons::USE),
            _ => cmd(k, 0.0, 250.0, if k >= 20 { tap(k, 0) } else { Buttons::NONE }),
        })
        .collect();
    out.push(scenario("TPg_timer", 60.0, g.lane_y, 0.0, cmds, "GOKZ: the timer runs on through a teleport"));
    // GOKZ: arrive next to a start button tapping +use (both phases).
    let h = teleport("h");
    for phase in 0..2 {
        let cmds = (0..400).map(|k| cmd(k, 0.0, if k < 200 { 100.0 } else { 0.0 }, if k >= 20 { tap(k, phase) } else { Buttons::NONE })).collect();
        out.push(scenario(&format!("TPh_start{phase}"), 300.0, h.lane_y, 0.0, cmds, "GOKZ: start presses right after a teleport"));
    }
    out
}

/// The probe map as a course (teleports, the GOKZ buttons), for `check`.
pub fn probe_course() -> course::Course {
    use movement::world::{PrimitiveWorld, World};
    let mut w = PrimitiveWorld::new();
    w.add(floor());
    let mut ids = Vec::new();
    for (i, (name, min, max)) in BUTTONS.iter().enumerate() {
        let id = movement::EntityId(i as u32 + 1);
        let mut brush = Brush::cuboid(*min, *max);
        brush.entity = id;
        w.add(brush);
        ids.push((*name, id));
    }
    // Spawnflag 32 is "only clients in vehicles": the server never teleported the player (TPb).
    let teleports = TELEPORTS
        .iter()
        .filter(|t| t.spawnflags & 32 == 0)
        .map(|t| course::Teleport { zone: course::Zone::cuboid(t.min, t.max), origin: t.dest, angles: Some(t.angles) })
        .collect();
    // Start buttons 1 and 3 share a name: the course presses either (StartSpec takes one entity, so
    // the scenario's own start button is chosen in `check`).
    course::Course {
        world: World::Primitive(w),
        spawn: (Vec3::ZERO, 0.0),
        start: course::StartSpec::Button { entity: ids[0].1 },
        end: course::EndSpec::Use { entity: ids[1].1, target: BUTTONS[1].1.add(BUTTONS[1].2).scale(0.5) },
        hazards: Vec::new(),
        kill_z: -1000.0,
        waypoints: Vec::new(),
        teleports,
        meta: Default::default(),
    }
}

/// Every teleport probe capture under `root`: our sim plus `Course::touch_teleports` against the server's
/// per-command states, free-running from the captured start and re-synced to the captured state before
/// every command (which isolates the teleport itself); the command each teleport fired in against the
/// plugin's `OnStartTouch`; and GOKZ's timer against `CourseRun`. The eye angles after a teleport are
/// printed, not checked: the plugin overrides the command angles, so the server's snap is not visible.
pub fn check(root: &Path) -> Result<(), String> {
    use crate::diff;
    let mut ok = true;
    let mut seen = 0;
    for dir in crate::capture_dirs(root) {
        let cap = crate::capture::Capture::load(&dir)?;
        let id = cap.scenario.req("scenario")?.split('_').take_while(|s| *s != "simplekz").collect::<Vec<_>>().join("_");
        if !id.starts_with("TP") {
            continue;
        }
        seen += 1;
        let mut course = probe_course();
        if id.starts_with("TPh") {
            course.start = course::StartSpec::Button { entity: movement::EntityId(3) };
        }
        let tol = diff::Tolerance::default();
        let mut runs = Vec::new();
        let mut teleported_on = Vec::new();
        for sync in [diff::Sync::Free, diff::Sync::Resync] {
            let mut tps = Vec::new();
            let mut k = 0u32;
            let r = diff::run_with(&cap, &course.world, sync, 0, tol, &mut |state: &mut movement::PlayerState| {
                if course.touch_teleports(state).is_some() {
                    tps.push(k);
                }
                k += 1;
            })?;
            if sync == diff::Sync::Free {
                teleported_on = tps;
            }
            runs.push(r);
        }
        let events = crate::useprobe::parse_events(&std::fs::read_to_string(dir.join("events.csv")).unwrap_or_default())?;
        let theirs_tp: Vec<u32> =
            events.iter().filter(|e| e.what == "OnStartTouch" && e.name.starts_with("tp_")).map(|e| e.tick).collect();
        let tp_ok = teleported_on == theirs_tp && events.iter().filter(|e| e.what == "OnStartTouch").all(|e| e.in_cmd);
        let angles: Vec<String> = teleported_on
            .iter()
            .filter_map(|&k| events.iter().find(|e| e.tick == k && e.what == "eye_post"))
            .map(|e| format!("({} {})", e.angles.x, e.angles.y))
            .collect();
        let timer = if id.starts_with("TPg") || id.starts_with("TPh") {
            crate::useprobe::timer_check(&cap, &events, &course, Some(&teleported_on)).map(|(ok, line)| {
                // The start button behind TPg's player stays in reach while +use is tapped, so its refire
                // delay (not modelled, docs/use-rules.md) moves the last restart; the end press must agree.
                if ok || !id.starts_with("TPg") {
                    return (ok, line);
                }
                let end = timer_end_command(&cap, &events);
                let ours = ours_end_command(&cap, &course, &teleported_on);
                let same = end.is_some() && end == ours;
                (same, format!("{line}
  end press on command ours {ours:?} server {end:?} (last restart moved by the refire delay)"))
            })
        } else {
            None
        };
        let [free, resync] = [&runs[0], &runs[1]];
        println!(
            "{id:12} free {:11} re-synced {:11} teleports ours {teleported_on:?} server {theirs_tp:?}, eye angles after {}{}",
            free.verdict.label(),
            resync.verdict.label(),
            angles.join(" "),
            timer.as_ref().map_or(String::new(), |(ok, l)| format!("
             {}{l}", if *ok { "" } else { "TIMER MISMATCH " }))
        );
        for (label, r) in [("free", free), ("re-synced", resync)] {
            if let Some((t, col, o, th)) = r.first {
                println!("             {label}: first divergence: command {t} {col}: ours {o} server {th}");
            }
        }
        let passed = |r: &diff::RunResult| !matches!(r.verdict, diff::Verdict::Failed(_));
        ok &= passed(free) && passed(resync) && tp_ok && timer.is_none_or(|(ok, _)| ok);
    }
    if seen == 0 {
        return Err(format!("no teleport probe captures under {}", root.display()));
    }
    if ok {
        println!("TELEPORT RULES MATCH");
        Ok(())
    } else {
        Err("teleport rules do not match the captures".into())
    }
}

/// The command whose `+use` ended GOKZ's timer: the `timer_end` event's command.
fn timer_end_command(_cap: &crate::capture::Capture, events: &[crate::useprobe::Event]) -> Option<u32> {
    events.iter().find(|e| e.what == "timer_end").map(|e| e.tick)
}

/// The command `CourseRun` completes the run on, fed the captured states.
fn ours_end_command(cap: &crate::capture::Capture, course: &course::Course, teleports: &[u32]) -> Option<u32> {
    let mut run = course::CourseRun::starting_at(&crate::useprobe::row_state(&cap.pre[0]));
    for (k, c) in cap.cmds.iter().enumerate() {
        if teleports.contains(&(k as u32)) {
            run.teleported();
        }
        if run.update(course, &crate::useprobe::row_state(&cap.post[k]), c) == course::Status::Completed {
            return Some(k as u32);
        }
    }
    None
}

//! `+use` measurement on the real server (docs/reinforcement-plan.md R6: the placeholder `+use` rules).
//!
//! A flat map with `func_button`s copied from a real KZ map's start button (`wait 0.0001`,
//! `spawnflags 1025` = don't move + use activates, `min_use_angle -1`), and scenarios that walk into
//! them, sweep the view across them, and hold or tap `+use`. The plugin logs every `OnPressed` with the
//! command it happened in and the server's eye position each tick (events.csv). `report` turns that
//! into one row per fresh press: did it fire, and the eye-to-button distances and view angles before and
//! after that command's movement, so the range, the angle rule, and which position is used can be read
//! off directly. A GOKZ lane (`climb_startbutton`/`climb_endbutton`) checks the timer rules on top.

use std::fmt::Write as _;
use std::path::Path;

use movement::math::angle_vectors;
use movement::world::Brush;
use movement::{Buttons, ModeKind, UserCmd, Vec3};

use crate::scenarios::Scenario;
use crate::vmf::BrushEntity;

pub const MAP: &str = "csmove_useprobe";

pub struct Button {
    pub name: &'static str,
    pub min: Vec3,
    pub max: Vec3,
    pub min_use_angle: &'static str,
    /// The y of the line the player walks along (x is the walking direction).
    pub walk_y: f32,
}

impl Button {
    pub fn center(&self) -> Vec3 {
        self.min.add(self.max).scale(0.5)
    }

    /// The point of the box nearest to `p`.
    pub fn nearest(&self, p: Vec3) -> Vec3 {
        Vec3::new(p.x.clamp(self.min.x, self.max.x), p.y.clamp(self.min.y, self.max.y), p.z.clamp(self.min.z, self.max.z))
    }
}

const fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3 { x, y, z }
}

/// Lanes along +x: the player starts west of the button and faces it (yaw 0).
pub const BUTTONS: [Button; 16] = [
    // 16-unit cube at eye height.
    Button { name: "probe_small", min: v(400.0, -8.0, 56.0), max: v(416.0, 8.0, 72.0), min_use_angle: "-1", walk_y: 0.0 },
    // 64-unit cube: the face is 32 units in front of the centre.
    Button { name: "probe_big", min: v(400.0, 368.0, 32.0), max: v(464.0, 432.0, 96.0), min_use_angle: "-1", walk_y: 400.0 },
    // Small cube behind a wall (x 380..384).
    Button { name: "probe_occluded", min: v(400.0, 792.0, 56.0), max: v(416.0, 808.0, 72.0), min_use_angle: "-1", walk_y: 800.0 },
    // Small cube with an angle limit.
    Button { name: "probe_angle", min: v(400.0, -808.0, 56.0), max: v(416.0, -792.0, 72.0), min_use_angle: "0.8", walk_y: -800.0 },
    // GOKZ: start ahead of the player, end behind.
    Button { name: "climb_startbutton", min: v(400.0, -408.0, 40.0), max: v(416.0, -392.0, 56.0), min_use_angle: "-1", walk_y: -400.0 },
    // Range shape (second batch): 16-unit cubes offset from the walking line at eye height (z 64).
    // Above the eye by 16..64, below by 48 (on the floor); at x 1000.
    Button { name: "probe_v16", min: v(1000.0, -808.0, 80.0), max: v(1016.0, -792.0, 96.0), min_use_angle: "-1", walk_y: -800.0 },
    Button { name: "probe_v32", min: v(1000.0, -408.0, 96.0), max: v(1016.0, -392.0, 112.0), min_use_angle: "-1", walk_y: -400.0 },
    Button { name: "probe_v48", min: v(1000.0, -8.0, 112.0), max: v(1016.0, 8.0, 128.0), min_use_angle: "-1", walk_y: 0.0 },
    Button { name: "probe_v64", min: v(1000.0, 392.0, 128.0), max: v(1016.0, 408.0, 144.0), min_use_angle: "-1", walk_y: 400.0 },
    Button { name: "probe_d48", min: v(1000.0, 792.0, 0.0), max: v(1016.0, 808.0, 16.0), min_use_angle: "-1", walk_y: 800.0 },
    // Beside the line by 24..64, diagonal (32 beside and 32 above), and at eye height for a ducked walk; at x 1600.
    Button { name: "probe_l24", min: v(1600.0, -776.0, 56.0), max: v(1616.0, -760.0, 72.0), min_use_angle: "-1", walk_y: -800.0 },
    Button { name: "probe_l48", min: v(1600.0, -352.0, 56.0), max: v(1616.0, -336.0, 72.0), min_use_angle: "-1", walk_y: -400.0 },
    Button { name: "probe_l64", min: v(1600.0, 64.0, 56.0), max: v(1616.0, 80.0, 72.0), min_use_angle: "-1", walk_y: 0.0 },
    Button { name: "probe_x32", min: v(1600.0, 432.0, 96.0), max: v(1616.0, 448.0, 112.0), min_use_angle: "-1", walk_y: 400.0 },
    Button { name: "probe_k", min: v(1600.0, 792.0, 56.0), max: v(1616.0, 808.0, 72.0), min_use_angle: "-1", walk_y: 800.0 },
    Button { name: "climb_endbutton", min: v(0.0, -408.0, 40.0), max: v(16.0, -392.0, 56.0), min_use_angle: "-1", walk_y: -400.0 },
];

fn button(name: &str) -> &'static Button {
    BUTTONS.iter().find(|b| b.name == name).unwrap()
}

/// The button a scenario is about, from its id prefix.
pub fn lane_button(id: &str) -> Option<&'static Button> {
    let name = match id.split('_').next()? {
        "Us" => "probe_small",
        "Ub" => "probe_big",
        "Uo" => "probe_occluded",
        "Ua" => "probe_angle",
        "G" => "climb_startbutton",
        // Second batch: `R<tag>_...` is about `probe_<tag>`.
        t if t.starts_with('R') => return BUTTONS.iter().find(|b| b.name.strip_prefix("probe_") == Some(&t[1..])),
        _ => return None,
    };
    Some(button(name))
}

fn lane_y(b: &Button) -> f32 {
    b.walk_y
}

pub fn level() -> (testlevel::Level, Vec<BrushEntity>) {
    let brushes = vec![
        Brush::cuboid(v(-640.0, -960.0, -16.0), v(2048.0, 1152.0, 0.0)),
        // The occluding wall.
        Brush::cuboid(v(380.0, 736.0, 0.0), v(384.0, 864.0, 128.0)),
    ];
    let level = testlevel::Level {
        brushes,
        signs: Vec::new(),
        areas: vec![testlevel::Area { name: "use probe", spawn: v(150.0, 0.0, 0.03125), yaw: 0.0 }],
    };
    let ents = BUTTONS
        .iter()
        .map(|b| BrushEntity {
            class: "func_button",
            keys: vec![
                ("targetname", b.name.to_string()),
                ("spawnflags", "1025".into()),
                ("wait", "0.0001".into()),
                ("speed", "5".into()),
                ("sounds", "0".into()),
                ("movedir", "0 0 0".into()),
                ("min_use_angle", b.min_use_angle.into()),
                ("rendermode", "0".into()),
                ("renderamt", "255".into()),
                ("rendercolor", "255 255 255".into()),
            ],
            boxes: vec![(b.min, b.max)],
            material: None,
        })
        .collect();
    (level, ents)
}

fn cmd(k: u32, pitch: f32, yaw: f32, fwd: f32, buttons: Buttons) -> UserCmd {
    UserCmd { tick: k, view_angles: Vec3::new(pitch, yaw, 0.0), forward_move: fwd, side_move: 0.0, up_move: 0.0, buttons }
}

fn scenario(id: String, origin: Vec3, yaw: f32, cmds: Vec<UserCmd>, isolates: &'static str) -> Scenario {
    Scenario {
        id,
        mode: ModeKind::SimpleKz,
        tickrate: 128,
        origin: Vec3::new(origin.x, origin.y, 1.0),
        yaw,
        settle: 64,
        cmds,
        isolates,
        map: Some(MAP.into()),
    }
}

/// `+use` down on every other command (`k % 2 == phase`): each down command is a fresh press.
fn tap(k: u32, phase: u32) -> Buttons {
    if k % 2 == phase {
        Buttons::USE
    } else {
        Buttons::NONE
    }
}

/// Walk at yaw 0 towards `b` from `x`, with `buttons(k)` held (`+use`, `+duck`).
fn approach(id: &str, b: &Button, x: f32, fwd: f32, n: u32, buttons: impl Fn(u32) -> Buttons, why: &'static str) -> Scenario {
    let cmds = (0..n).map(|k| cmd(k, 0.0, 0.0, fwd, buttons(k))).collect();
    scenario(id.into(), v(x, lane_y(b), 0.0), 0.0, cmds, why)
}

/// Stand `d` units in front of `b`'s face, sweep one view angle from `from` to `to` (0.5 degrees per
/// command), tapping `+use`.
fn sweep(id: String, b: &Button, d: f32, pitch: bool, from: f32, to: f32) -> Scenario {
    let n = ((to - from).abs() / 0.5) as u32 + 1;
    let step = (to - from).signum() * 0.5;
    let cmds = (0..n + 16)
        .map(|k| {
            let a = from + step * k.min(n - 1) as f32;
            let (p, y) = if pitch { (a, 0.0) } else { (0.0, a) };
            cmd(k, p, y, 0.0, tap(k, 1))
        })
        .collect();
    let start_yaw = if pitch { 0.0 } else { from };
    scenario(id, v(b.min.x - d, lane_y(b), 0.0), start_yaw, cmds, if pitch { "+use: pitch sweep" } else { "+use: yaw sweep" })
}

pub fn scenarios() -> Vec<Scenario> {
    let small = button("probe_small");
    let big = button("probe_big");
    let mut out = vec![
        approach("Us_slow", small, 150.0, 40.0, 900, |k| tap(k, 1), "+use: range, slow walk-in"),
        approach("Ub_slow", big, 150.0, 40.0, 900, |k| tap(k, 1), "+use: range to face or centre"),
        approach("Us_duck", small, 250.0, 100.0, 1400, |k| tap(k, 1) | Buttons::DUCK, "+use: range ducked (eye height)"),
        approach("Us_hold", small, 150.0, 250.0, 300, |_| Buttons::USE, "+use: held, never a fresh press"),
        approach("Uo_slow", button("probe_occluded"), 150.0, 40.0, 900, |k| tap(k, 1), "+use: through a wall"),
        approach("Ua_slow", button("probe_angle"), 150.0, 40.0, 900, |k| tap(k, 1), "+use: min_use_angle 0.8"),
    ];
    // Fast walk-ins at several offsets and both tap phases: the press fires on the position before or
    // after that command's movement, which the slow walk-in's threshold decides.
    for (b, tag) in [(small, "Us"), (big, "Ub")] {
        for o in 0..3 {
            for phase in 0..2 {
                let x = 100.0 + 1.25 * o as f32;
                out.push(approach(&format!("{tag}_fast{o}{phase}"), b, x, 250.0, 300, move |k| tap(k, phase), "+use: fast walk-in"));
            }
        }
    }
    for (b, tag) in [(small, "Us"), (big, "Ub"), (button("probe_angle"), "Ua")] {
        for d in [24.0f32, 48.0] {
            out.push(sweep(format!("{tag}_yawL{}", d as u32), b, d, false, 180.0, 0.0));
            out.push(sweep(format!("{tag}_yawR{}", d as u32), b, d, false, -180.0, 0.0));
        }
    }
    out.push(sweep("Us_pitchD48".into(), small, 48.0, true, -89.0, 89.0));
    out.push(sweep("Us_pitchU48".into(), small, 48.0, true, 89.0, -89.0));

    // Range shape: very slow walk-ins (about 0.08 units per command, 0.16 between presses) from 92 units.
    for b in BUTTONS.iter().filter(|b| b.min.x >= 1000.0) {
        let tag = b.name.strip_prefix("probe_").unwrap();
        let duck = tag == "k";
        let fwd = if duck { 30.0 } else { 10.0 };
        let extra = if duck { Buttons::DUCK } else { Buttons::NONE };
        out.push(approach(&format!("R{tag}_slow"), b, b.min.x - 92.0, fwd, 1100, move |k| tap(k, 1) | extra, "+use: range shape"));
    }

    // GOKZ: start on the ground, run back to the end button.
    let start = button("climb_startbutton");
    let ly = lane_y(start);
    let ground: Vec<UserCmd> = (0..400)
        .map(|k| match k {
            0..=7 => cmd(k, 0.0, 0.0, 0.0, Buttons::NONE),
            8 => cmd(k, 0.0, 0.0, 0.0, Buttons::USE),
            _ => cmd(k, 0.0, 180.0, 250.0, if k >= 20 { tap(k, 0) } else { Buttons::NONE }),
        })
        .collect();
    out.push(scenario("G_ground".into(), v(360.0, ly, 0.0), 0.0, ground, "GOKZ: start on ground, end button"));
    // Jump in place and keep tapping the start button through the landing.
    for phase in 0..2 {
        let air: Vec<UserCmd> = (0..220)
            .map(|k| {
                let b = if k == 8 { Buttons::JUMP } else if k >= 10 { tap(k, phase) } else { Buttons::NONE };
                cmd(k, 0.0, 0.0, 0.0, b)
            })
            .collect();
        out.push(scenario(format!("G_air{phase}"), v(360.0, ly, 0.0), 0.0, air, "GOKZ: start presses in the air and after landing"));
    }
    out
}

// ---------------------------------------------------------------------------------------------------
// Report

/// One events.csv row (plugin 1.2+).
pub struct Event {
    pub tick: u32,
    pub in_cmd: bool,
    pub what: String,
    pub name: String,
    pub value: f32,
    pub eye: Vec3,
    /// Eye angles (pitch yaw roll) when the event was logged.
    pub angles: Vec3,
}

fn bits(s: &str) -> Result<f32, String> {
    u32::from_str_radix(s, 16).map(f32::from_bits).map_err(|_| format!("bad float bits `{s}`"))
}

pub fn parse_events(text: &str) -> Result<Vec<Event>, String> {
    let mut lines = text.lines();
    let head: Vec<&str> = lines.next().ok_or("empty events.csv")?.split(',').collect();
    let col = |n: &str| head.iter().position(|h| *h == n).ok_or_else(|| format!("events.csv: no column {n}"));
    let (t, ic, ev, nm, vb, ex, ey, ez) =
        (col("tick")?, col("in_cmd")?, col("event")?, col("name")?, col("value_bits")?, col("eye_x_bits")?, col("eye_y_bits")?, col("eye_z_bits")?);
    let (ap, ay, ar) = (col("pitch_bits")?, col("yaw_bits")?, col("roll_bits")?);
    lines
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let c: Vec<&str> = l.trim().split(',').collect();
            Ok(Event {
                tick: c[t].parse().map_err(|_| format!("bad tick in `{l}`"))?,
                in_cmd: c[ic] == "1",
                what: c[ev].to_string(),
                name: c[nm].to_string(),
                value: bits(c[vb])?,
                eye: Vec3::new(bits(c[ex])?, bits(c[ey])?, bits(c[ez])?),
                angles: Vec3::new(bits(c[ap])?, bits(c[ay])?, bits(c[ar])?),
            })
        })
        .collect()
}

fn angle_deg(forward: Vec3, to: Vec3) -> f32 {
    let d = to.normalized();
    forward.dot(d).clamp(-1.0, 1.0).acos().to_degrees()
}

/// One row per fresh `+use` press in every capture under `root` that has events.csv; GOKZ timer events
/// listed after. Written to `out` as CSV, with a per-scenario summary on stdout.
pub fn report(root: &Path, out: &Path) -> Result<(), String> {
    let mut csv = String::from(
        "scenario,k,fired,in_cmd,on_ground,duck_amount,pitch,yaw,d_near_pre,d_near_post,d_center_pre,d_center_post,\
         dx_face_pre,dx_face_post,ang_center_pre,ang_center_post,eye_z_pre,eye_z_post\n",
    );
    let mut summary = String::new();
    for dir in crate::capture_dirs(root) {
        let Ok(text) = std::fs::read_to_string(dir.join("events.csv")) else { continue };
        let cap = crate::capture::Capture::load(&dir)?;
        let id = cap.scenario.req("scenario")?.split('_').take_while(|s| *s != "simplekz").collect::<Vec<_>>().join("_");
        let Some(b) = lane_button(&id) else { continue };
        let events = parse_events(&text)?;
        let eye = |k: u32, what: &str| events.iter().find(|e| e.tick == k && e.what == what).map(|e| e.eye);
        let (mut fired_n, mut presses) = (0, 0);
        let (mut max_hit, mut min_miss) = ([f32::MIN; 2], [f32::MAX; 2]);
        for (k, c) in cap.cmds.iter().enumerate() {
            let k = k as u32;
            let down = c.buttons.contains(Buttons::USE);
            let before = k > 0 && cap.cmds[k as usize - 1].buttons.contains(Buttons::USE);
            if !down || before {
                continue;
            }
            presses += 1;
            let (Some(pre), Some(post)) = (eye(k, "eye_pre"), eye(k, "eye_post")) else { continue };
            let hit = events.iter().find(|e| e.tick == k && e.what == "OnPressed" && e.name == b.name);
            let fired = hit.is_some();
            fired_n += fired as u32;
            let fwd = angle_vectors(c.view_angles).forward;
            let dn = |p: Vec3| b.nearest(p).sub(p).length();
            let dc = |p: Vec3| b.center().sub(p).length();
            let post_row = &cap.post[k as usize];
            for (i, p) in [pre, post].into_iter().enumerate() {
                if fired {
                    max_hit[i] = max_hit[i].max(dn(p));
                } else {
                    min_miss[i] = min_miss[i].min(dn(p));
                }
            }
            let _ = writeln!(
                csv,
                "{id},{k},{},{},{},{},{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.3},{:.3},{:.4},{:.4}",
                fired as u8,
                hit.map_or(String::new(), |e| (e.in_cmd as u8).to_string()),
                (post_row.ground >= 0) as u8,
                post_row.duck_amount.unwrap_or(0.0),
                c.view_angles.x,
                c.view_angles.y,
                dn(pre),
                dn(post),
                dc(pre),
                dc(post),
                b.min.x - pre.x,
                b.min.x - post.x,
                angle_deg(fwd, b.center().sub(pre)),
                angle_deg(fwd, b.center().sub(post)),
                pre.z,
                post.z,
            );
        }
        let other: Vec<String> = events
            .iter()
            .filter(|e| !e.what.starts_with("eye_") && !(e.what == "OnPressed" && e.name == b.name))
            .map(|e| format!("{}@{}{} {} {}", e.what, e.tick, if e.in_cmd { "" } else { "(outside)" }, e.name, e.value))
            .collect();
        let _ = writeln!(
            summary,
            "{id:12} presses {presses:4} fired {fired_n:4}  nearest-point distance: fired <= {:.3} / {:.3}, missed >= {:.3} / {:.3} (pre / post){}",
            max_hit[0],
            max_hit[1],
            min_miss[0],
            min_miss[1],
            if other.is_empty() { String::new() } else { format!("\n             other: {}", other.join(", ")) }
        );
    }
    std::fs::write(out, csv).map_err(|e| e.to_string())?;
    print!("{summary}");
    println!("per-press rows: {}", out.display());
    Ok(())
}

// ---------------------------------------------------------------------------------------------------
// Check: the course crate's `+use` and GOKZ rules against the probe captures

/// The probe map as a course whose start button is `start` and whose end is `end` (a button) or, for
/// `None`, a far-away touch volume.
fn probe_course(start: &str, end: Option<&str>) -> (course::Course, Vec<(&'static str, movement::EntityId)>) {
    use movement::world::{PrimitiveWorld, World};
    let (level, _) = level();
    let mut w = PrimitiveWorld::new();
    for b in level.brushes {
        w.add(b);
    }
    let mut ids = Vec::new();
    for (i, b) in BUTTONS.iter().enumerate() {
        let id = movement::EntityId(i as u32 + 1);
        let mut brush = Brush::cuboid(b.min, b.max);
        brush.entity = id;
        w.add(brush);
        ids.push((b.name, id));
    }
    let id = |n: &str| ids.iter().find(|(m, _)| *m == n).unwrap().1;
    let course = course::Course {
        world: World::Primitive(w),
        spawn: (Vec3::ZERO, 0.0),
        start: course::StartSpec::Button { entity: id(start) },
        end: match end {
            Some(e) => course::EndSpec::Use { entity: id(e), target: button(e).center() },
            None => course::EndSpec::Touch(course::Zone::cuboid(v(-600.0, 1100.0, 0.0), v(-590.0, 1110.0, 10.0))),
        },
        hazards: Vec::new(),
        kill_z: -1000.0,
        waypoints: Vec::new(),
        teleports: Vec::new(),
        meta: Default::default(),
    };
    (course, ids)
}

pub fn row_state(r: &crate::capture::Row) -> movement::PlayerState {
    let mut s = movement::PlayerState::new(r.origin);
    s.velocity = r.velocity;
    s.ground_entity = (r.ground >= 0).then_some(movement::EntityId(0));
    s.ducked = r.ducked.unwrap_or(false);
    s.duck_amount = r.duck_amount.unwrap_or(0.0);
    s
}

/// Every fresh press in the probe captures under `root`, predicted with `course::use_target` from the
/// server's eye position, against what the server did; and the GOKZ scenarios' timer events against
/// `CourseRun` on the captured states. Errors if anything is unexplained.
/// GOKZ's timer starts (and restarts) and end time in a capture against `CourseRun` fed the captured
/// states. `teleports` lists the commands a map teleport fired in, if any. `None` when the capture has
/// no timer events and none are predicted.
pub fn timer_check(
    cap: &crate::capture::Capture,
    events: &[Event],
    course: &course::Course,
    teleports: Option<&[u32]>,
) -> Option<(bool, String)> {
    let id = cap.scenario.get("scenario").unwrap_or("?");
    let mut run = course::CourseRun::starting_at(&row_state(&cap.pre[0]));
    let mut ours = Vec::new();
    for (k, c) in cap.cmds.iter().enumerate() {
        let before = (run.status, run.elapsed_ticks);
        if teleports.is_some_and(|t| t.contains(&(k as u32))) {
            run.teleported();
        }
        run.update(course, &row_state(&cap.post[k]), c);
        // The last command is left out: the server's after-command event for it is never logged.
        if run.status == course::Status::Running && run.elapsed_ticks == 0 && k + 1 < cap.cmds.len() {
            ours.push(k as u32);
        }
        if run.status == course::Status::Completed && before.0 != course::Status::Completed {
            ours.push(100_000 + run.elapsed_ticks);
        }
    }
    // Server: a start inside command k, or an outside one tagged k (after command k - 1).
    let mut theirs: Vec<u32> = events
        .iter()
        .filter_map(|e| match e.what.as_str() {
            "timer_start" if e.in_cmd => Some(e.tick),
            "timer_start" => e.tick.checked_sub(1),
            "timer_end" => Some(100_000 + (e.value * 128.0).round() as u32),
            _ => None,
        })
        .collect();
    theirs.sort();
    theirs.dedup();
    // Starts before the first command belong to the previous scenario's virtual button.
    let first_cmd_start = ours.first().copied().unwrap_or(0);
    let last = cap.cmds.len() as u32 - 1;
    theirs.retain(|&t| t >= first_cmd_start && t != last);
    if ours.is_empty() && theirs.is_empty() {
        return None;
    }
    let line = if ours == theirs {
        format!("{id}: timer starts and end match ({} events)", ours.len())
    } else {
        format!("{id}: timer MISMATCH
  ours   {ours:?}
  server {theirs:?}")
    };
    Some((ours == theirs, line))
}

pub fn check(root: &Path) -> Result<(), String> {
    let mut agree = 0u32;
    let mut refire = 0u32;
    let mut bad = Vec::new();
    let mut gokz = Vec::new();
    for dir in crate::capture_dirs(root) {
        let Ok(text) = std::fs::read_to_string(dir.join("events.csv")) else { continue };
        let cap = crate::capture::Capture::load(&dir)?;
        let id = cap.scenario.req("scenario")?.split('_').take_while(|s| *s != "simplekz").collect::<Vec<_>>().join("_");
        let Some(b) = lane_button(&id) else { continue };
        // `min_use_angle` other than -1 is not modelled (KZ buttons use -1).
        if b.min_use_angle != "-1" {
            continue;
        }
        let is_g = id.starts_with('G');
        let (course, ids) = if is_g { probe_course(b.name, Some("climb_endbutton")) } else { probe_course(b.name, None) };
        let name_of = |e: movement::EntityId| ids.iter().find(|(_, i)| *i == e).unwrap().0;
        let events = parse_events(&text)?;
        let mut last_fire: Option<u32> = None;
        for (k, c) in cap.cmds.iter().enumerate() {
            let k = k as u32;
            let fresh = c.buttons.contains(Buttons::USE) && !(k > 0 && cap.cmds[k as usize - 1].buttons.contains(Buttons::USE));
            if !fresh {
                continue;
            }
            let Some(eye) = events.iter().find(|e| e.tick == k && e.what == "eye_pre").map(|e| e.eye) else { continue };
            let actual = events.iter().find(|e| e.tick == k && e.what == "OnPressed").map(|e| e.name.as_str());
            let predicted = course::use_target(&course, eye, c.view_angles).map(name_of);
            if actual == predicted {
                agree += 1;
            } else if actual.is_none() && last_fire.is_some_and(|f| k - f <= 8) {
                // The button had fired within 8 commands and was not ready again (docs/use-rules.md).
                refire += 1;
            } else {
                bad.push(format!("{id} k {k}: server {actual:?}, predicted {predicted:?}"));
            }
            if actual.is_some() {
                last_fire = Some(k);
            }
        }
        if is_g {
            gokz.extend(timer_check(&cap, &events, &course, None));
        }
    }
    println!("presses: {agree} agree, {refire} explained by the refire delay, {} unexplained", bad.len());
    for b in bad.iter().take(20) {
        println!("  {b}");
    }
    for (_, l) in &gokz {
        println!("{l}");
    }
    if bad.is_empty() && gokz.iter().all(|(ok, _)| *ok) {
        println!("USE RULES MATCH");
        Ok(())
    } else {
        Err("use rules do not match the captures".into())
    }
}

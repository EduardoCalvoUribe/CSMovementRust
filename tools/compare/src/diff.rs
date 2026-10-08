//! Replay a capture's command stream through the movement crate and diff it against the captured
//! states, tick by tick and column by column (plan §9.6 steps 2-6).

use std::fmt::Write as _;

use movement::instrument::NullObserver;
use movement::state::MoveType;
use movement::trace::EntityId;
use movement::{process_movement, tick_interval, PlayerState, TraceWorld, Vec3};

use crate::capture::{from_source_buttons, Capture, Row, MOVETYPE_LADDER};

/// Source `FL_DUCKING`: the duck hull is in use.
const FL_DUCKING: i32 = 1 << 1;

#[derive(Clone, Copy, Debug)]
pub struct Tolerance {
    /// Largest ULP distance still reported as `ULP(n)` rather than compared against `eps`.
    pub max_ulp: u32,
    /// Absolute tolerance for `CLOSE` (units, or units/s).
    pub eps: f64,
}

impl Default for Tolerance {
    fn default() -> Self {
        Self { max_ulp: 4, eps: 1e-3 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Exact,
    Ulp(u32),
    Close,
    Diverged,
}

/// Distance in units in the last place, treating +0 and -0 as equal.
pub fn ulp_distance(a: f32, b: f32) -> u64 {
    fn ordered(f: f32) -> i64 {
        let i = f.to_bits() as i32 as i64;
        if i < 0 {
            i64::from(i32::MIN) - i
        } else {
            i
        }
    }
    (ordered(a) - ordered(b)).unsigned_abs()
}

pub fn classify(ours: f32, theirs: f32, tol: Tolerance) -> Class {
    if ours.to_bits() == theirs.to_bits() || (ours == 0.0 && theirs == 0.0) {
        return Class::Exact;
    }
    let n = ulp_distance(ours, theirs);
    if n <= tol.max_ulp as u64 {
        return Class::Ulp(n as u32);
    }
    if ((ours as f64) - (theirs as f64)).abs() <= tol.eps {
        Class::Close
    } else {
        Class::Diverged
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    BitExact,
    WithinUlp,
    WithinEps,
    Failed(u32),
}

impl Verdict {
    pub fn label(self) -> String {
        match self {
            Verdict::BitExact => "BIT_EXACT".into(),
            Verdict::WithinUlp => "WITHIN_ULP".into(),
            Verdict::WithinEps => "WITHIN_EPS".into(),
            Verdict::Failed(t) => format!("FAILED @ tick {t}"),
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Verdict::BitExact => 0,
            Verdict::WithinUlp => 1,
            Verdict::WithinEps => 2,
            Verdict::Failed(_) => 3,
        }
    }

    pub fn parse_min(s: &str) -> Result<Verdict, String> {
        match s {
            "BIT_EXACT" => Ok(Verdict::BitExact),
            "WITHIN_ULP" => Ok(Verdict::WithinUlp),
            "WITHIN_EPS" => Ok(Verdict::WithinEps),
            _ => Err(format!("unknown verdict `{s}`")),
        }
    }
}

/// Compared columns. Booleans are compared as 0/1.
pub const COLUMNS: [&str; 15] = [
    "origin_x",
    "origin_y",
    "origin_z",
    "vel_x",
    "vel_y",
    "vel_z",
    "on_ground",
    "on_ladder",
    "ducked",
    "duck_amount",
    "duck_speed",
    "stamina",
    "surface_friction",
    "fall_velocity",
    "speed_2d",
];

const BOOL_COLUMNS: [&str; 3] = ["on_ground", "on_ladder", "ducked"];

fn ours_value(s: &PlayerState, col: &str) -> f32 {
    match col {
        "origin_x" => s.origin.x,
        "origin_y" => s.origin.y,
        "origin_z" => s.origin.z,
        "vel_x" => s.velocity.x,
        "vel_y" => s.velocity.y,
        "vel_z" => s.velocity.z,
        "on_ground" => s.on_ground() as u8 as f32,
        "on_ladder" => (s.move_type == MoveType::Ladder) as u8 as f32,
        "ducked" => s.ducked as u8 as f32,
        "duck_amount" => s.duck_amount,
        "duck_speed" => s.duck_speed,
        "stamina" => s.stamina,
        "surface_friction" => s.surface_friction,
        "fall_velocity" => s.fall_velocity,
        "speed_2d" => s.velocity.length_2d(),
        _ => unreachable!("{col}"),
    }
}

fn theirs_value(r: &Row, col: &str) -> Option<f32> {
    match col {
        "origin_x" => Some(r.origin.x),
        "origin_y" => Some(r.origin.y),
        "origin_z" => Some(r.origin.z),
        "vel_x" => Some(r.velocity.x),
        "vel_y" => Some(r.velocity.y),
        "vel_z" => Some(r.velocity.z),
        "on_ground" => Some((r.ground >= 0) as u8 as f32),
        "on_ladder" => r.move_type.map(|m| (m == MOVETYPE_LADDER) as u8 as f32),
        // Our `ducked` is the duck hull, which is FL_DUCKING; m_bDucked clears as soon as an unduck starts.
        "ducked" => r.flags.map(|f| (f & FL_DUCKING != 0) as u8 as f32).or(r.ducked.map(|b| b as u8 as f32)),
        "duck_amount" => r.duck_amount,
        "duck_speed" => r.duck_speed,
        "stamina" => r.stamina,
        "surface_friction" => r.surface_friction,
        "fall_velocity" => r.fall_velocity,
        "speed_2d" => Some(r.velocity.length_2d()),
        _ => unreachable!("{col}"),
    }
}

/// Our start state from a captured row (the settled `pre` row of tick 0, or any row when re-syncing).
/// Fields the capture lacks are taken from `fallback`.
pub fn state_from_row(r: &Row, fallback: &PlayerState) -> PlayerState {
    let mut s = fallback.clone();
    s.origin = r.origin;
    s.velocity = r.velocity;
    if let Some(v) = r.base_velocity {
        s.base_velocity = v;
    }
    s.ground_entity = if r.ground >= 0 { Some(EntityId(r.ground as u32)) } else { None };
    if let Some(m) = r.move_type {
        s.move_type = if m == MOVETYPE_LADDER { MoveType::Ladder } else { MoveType::Walk };
    }
    if let Some(f) = r.flags {
        s.ducked = f & FL_DUCKING != 0;
    } else if let Some(v) = r.ducked {
        s.ducked = v;
    }
    if let Some(v) = r.ducking {
        s.ducking = v;
    }
    if let Some(v) = r.duck_amount {
        s.duck_amount = v;
    }
    if let Some(v) = r.duck_speed {
        s.duck_speed = v;
    }
    if let Some(v) = r.stamina {
        s.stamina = v;
    }
    if let Some(v) = r.surface_friction {
        s.surface_friction = v;
    }
    if let Some(v) = r.fall_velocity {
        s.fall_velocity = v;
    }
    if let Some(b) = r.old_buttons {
        // CS:GO strips IN_DUCK from the saved buttons when a duck press is refused (crouch spam), but
        // its press/release detection still sees the held key; our old_buttons keeps the held key, so
        // the duck bit is carried over from our own state rather than the capture.
        let duck = s.old_buttons.contains(movement::cmd::Buttons::DUCK);
        s.old_buttons = from_source_buttons(b);
        s.old_buttons.remove(movement::cmd::Buttons::DUCK);
        if duck {
            s.old_buttons.insert(movement::cmd::Buttons::DUCK);
        }
    }
    if let Some(n) = r.ladder_normal {
        s.ladder_normal = n;
    }
    s
}

#[derive(Clone, Debug, Default)]
pub struct ColumnStats {
    pub compared: u32,
    pub exact: u32,
    pub ulp: u32,
    pub close: u32,
    pub diverged: u32,
    pub max_ulp: u64,
    pub max_err: f64,
    pub max_err_tick: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    Jump,
    Land,
    LeaveGround,
    Duck,
    Unduck,
    LadderOn,
    LadderOff,
    /// Fall damage on this command.
    Damage,
}

/// Fall speed above which a landing hurts (`PLAYER_MAX_SAFE_FALL_SPEED`, public SDK 2013).
pub const MAX_SAFE_FALL_SPEED: f32 = 580.0;

/// Events derived identically from both sides' pre/post rows, so the lists are comparable even
/// without routine-level hooks on the server (plan §9.4 item 4).
pub fn derive_events(pre: &[Row], post: &[Row]) -> Vec<(u32, EventKind)> {
    let mut ev = Vec::new();
    for (a, b) in pre.iter().zip(post) {
        let t = b.tick;
        let ladder = |r: &Row| r.move_type == Some(MOVETYPE_LADDER);
        if b.velocity.z - a.velocity.z > 100.0 {
            ev.push((t, EventKind::Jump));
        }
        if a.ground < 0 && b.ground >= 0 {
            ev.push((t, EventKind::Land));
        }
        if a.ground >= 0 && b.ground < 0 {
            ev.push((t, EventKind::LeaveGround));
        }
        let hull = |r: &Row| r.flags.map(|f| f & FL_DUCKING != 0).or(r.ducked);
        match (hull(a), hull(b)) {
            (Some(false), Some(true)) => ev.push((t, EventKind::Duck)),
            (Some(true), Some(false)) => ev.push((t, EventKind::Unduck)),
            _ => {}
        }
        if !ladder(a) && ladder(b) {
            ev.push((t, EventKind::LadderOn));
        }
        if ladder(a) && !ladder(b) {
            ev.push((t, EventKind::LadderOff));
        }
        // Health where it was logged; otherwise (our side, older captures) Source's rule: a landing
        // whose fall speed, set from -vz at the start of the command, exceeds the safe speed.
        let hurt = match (a.health, b.health) {
            (Some(h0), Some(h1)) => h1 < h0,
            _ => a.ground < 0 && b.ground >= 0 && -a.velocity.z > MAX_SAFE_FALL_SPEED,
        };
        if hurt {
            ev.push((t, EventKind::Damage));
        }
    }
    ev
}

/// Our side expressed as capture rows, so both sides go through the same event derivation.
pub fn row_from_state(tick: u32, s: &PlayerState) -> Row {
    Row {
        tick,
        origin: s.origin,
        velocity: s.velocity,
        base_velocity: Some(s.base_velocity),
        ground: s.ground_entity.map_or(-1, |e| e.0 as i32),
        move_type: Some(if s.move_type == MoveType::Ladder { MOVETYPE_LADDER } else { crate::capture::MOVETYPE_WALK }),
        flags: Some(if s.ducked { FL_DUCKING } else { 0 }),
        ducked: Some(s.ducked),
        ducking: Some(s.ducking),
        duck_amount: Some(s.duck_amount),
        duck_speed: Some(s.duck_speed),
        stamina: Some(s.stamina),
        surface_friction: Some(s.surface_friction),
        fall_velocity: Some(s.fall_velocity),
        ladder_normal: Some(s.ladder_normal),
        ..Row::default()
    }
}

#[derive(Clone, Debug)]
pub struct RunResult {
    pub verdict: Verdict,
    /// First diverged (tick, column, ours, theirs).
    pub first: Option<(u32, &'static str, f32, f32)>,
    pub stats: Vec<(&'static str, ColumnStats)>,
    pub ours_pre: Vec<Row>,
    pub ours_post: Vec<Row>,
    pub events_ours: Vec<(u32, EventKind)>,
    pub events_theirs: Vec<(u32, EventKind)>,
    /// Captured `pre(k+1)` differs from `post(k)`: something outside movement touched the player.
    pub rig_gaps: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sync {
    /// Our state evolves on its own from the recorded start.
    Free,
    /// Our state is overwritten with the captured `pre` row before every command (plan §9.6 step 6).
    Resync,
}

/// Replay and diff. `shift` delays our command stream by that many ticks (phase check, plan §9.7 step 4;
/// the first `shift` commands are empty).
pub fn run(cap: &Capture, world: &impl TraceWorld, sync: Sync, shift: u32, tol: Tolerance) -> Result<RunResult, String> {
    let cfg = cap.config()?;
    let mut mode = cap.mode()?.create();
    let dt = tick_interval(cap.tickrate()?);
    let template = PlayerState::new(Vec3::ZERO);
    let mut state = state_from_row(&cap.pre[0], &template);
    // Not logged: assume duck speed was last full where the capture starts.
    state.duck_speed_anchor = state.origin;
    let unavailable = cap.unavailable();

    let mut stats: Vec<(&'static str, ColumnStats)> = COLUMNS.iter().map(|c| (*c, ColumnStats::default())).collect();
    let mut first = None;
    let mut ours_pre = Vec::new();
    let mut ours_post = Vec::new();
    let mut rig_gaps = Vec::new();

    for k in 0..cap.cmds.len() {
        if sync == Sync::Resync {
            state = state_from_row(&cap.pre[k], &state);
        }
        if k > 0 && !rows_equal(&cap.post[k - 1], &cap.pre[k]) {
            rig_gaps.push(k as u32);
        }
        ours_pre.push(row_from_state(k as u32, &state));
        let mut cmd = if (k as u32) < shift { Default::default() } else { cap.cmds[k - shift as usize] };
        cmd.tick = k as u32;
        process_movement(&cfg, mode.as_mut(), world, &mut state, &cmd, &mut NullObserver, dt);
        ours_post.push(row_from_state(k as u32, &state));

        let theirs = &cap.post[k];
        for (col, st) in stats.iter_mut() {
            if unavailable.iter().any(|u| u == col) {
                continue;
            }
            let Some(t) = theirs_value(theirs, col) else { continue };
            let o = ours_value(&state, col);
            st.compared += 1;
            let class = if BOOL_COLUMNS.contains(col) {
                if o == t {
                    Class::Exact
                } else {
                    Class::Diverged
                }
            } else {
                classify(o, t, tol)
            };
            match class {
                Class::Exact => st.exact += 1,
                Class::Ulp(n) => {
                    st.ulp += 1;
                    st.max_ulp = st.max_ulp.max(n as u64);
                }
                Class::Close => st.close += 1,
                Class::Diverged => st.diverged += 1,
            }
            let err = ((o as f64) - (t as f64)).abs();
            if err > st.max_err {
                st.max_err = err;
                st.max_err_tick = k as u32;
            }
            if class == Class::Diverged && first.is_none() {
                first = Some((k as u32, *col, o, t));
            }
        }
    }

    let worst = stats
        .iter()
        .map(|(_, s)| if s.diverged > 0 { 3 } else if s.close > 0 { 2 } else if s.ulp > 0 { 1 } else { 0 })
        .max()
        .unwrap_or(0);
    let verdict = match (first, worst) {
        (Some((t, ..)), _) => Verdict::Failed(t),
        (None, 0) => Verdict::BitExact,
        (None, 1) => Verdict::WithinUlp,
        _ => Verdict::WithinEps,
    };
    let events_ours = derive_events(&ours_pre, &ours_post);
    let events_theirs = derive_events(&cap.pre, &cap.post);
    Ok(RunResult { verdict, first, stats, ours_pre, ours_post, events_ours, events_theirs, rig_gaps })
}

fn rows_equal(a: &Row, b: &Row) -> bool {
    let bits = |v: Vec3| (v.x.to_bits(), v.y.to_bits(), v.z.to_bits());
    bits(a.origin) == bits(b.origin)
        && bits(a.velocity) == bits(b.velocity)
        && a.ground == b.ground
        && a.ducked == b.ducked
        && a.stamina.map(f32::to_bits) == b.stamina.map(f32::to_bits)
        && a.duck_amount.map(f32::to_bits) == b.duck_amount.map(f32::to_bits)
}

/// Text report (plan §9.6 step 5).
pub fn text_report(cap: &Capture, free: &RunResult, resync: &RunResult, shifted: &[(i32, Verdict)]) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "capture   {}", cap.name());
    let _ = writeln!(
        s,
        "build {}  map {}  tickrate {}  mode {}  ticks {}",
        cap.meta.get("build").unwrap_or("?"),
        cap.meta.get("map").unwrap_or("?"),
        cap.meta.get("tickrate").unwrap_or("?"),
        cap.meta.get("mode").unwrap_or("?"),
        cap.cmds.len()
    );
    let _ = writeln!(s, "verdict   {}   (re-synced: {})", free.verdict.label(), resync.verdict.label());
    if !shifted.is_empty() {
        let list: Vec<String> = shifted.iter().map(|(k, v)| format!("{k:+}: {}", v.label())).collect();
        let _ = writeln!(s, "phase     {}", list.join("   "));
    }
    if !free.rig_gaps.is_empty() {
        let _ = writeln!(s, "RIG       captured pre(k) != post(k-1) at ticks {:?} (state touched outside movement)", free.rig_gaps);
    }
    for (label, r) in [("free-running", free), ("re-synced", resync)] {
        let _ = writeln!(s, "\n== {label} ==");
        let _ = writeln!(s, "{:<17}{:>7}{:>7}{:>7}{:>7}{:>7}{:>9}{:>14}{:>7}", "column", "n", "exact", "ulp", "close", "div", "max_ulp", "max_err", "@tick");
        for (col, st) in &r.stats {
            if st.compared == 0 {
                let _ = writeln!(s, "{col:<17}   (unavailable)");
                continue;
            }
            let _ = writeln!(
                s,
                "{:<17}{:>7}{:>7}{:>7}{:>7}{:>7}{:>9}{:>14.6e}{:>7}",
                col, st.compared, st.exact, st.ulp, st.close, st.diverged, st.max_ulp, st.max_err, st.max_err_tick
            );
        }
        if let Some((t, col, o, th)) = r.first {
            let _ = writeln!(s, "\nfirst divergence: tick {t}, column {col}: ours {o:.9} ({:08x}) theirs {th:.9} ({:08x})", o.to_bits(), th.to_bits());
            let k = t as usize;
            if k > 0 {
                let _ = writeln!(s, "  previous captured post: {}", fmt_row(&cap.post[k - 1]));
            }
            let _ = writeln!(s, "  start (captured pre):   {}", fmt_row(&cap.pre[k]));
            let _ = writeln!(s, "  start (ours):           {}", fmt_row(&r.ours_pre[k]));
            let c = cap.cmds[k];
            let _ = writeln!(
                s,
                "  command: fwd {} side {} buttons {:?} angles ({}, {}, {})",
                c.forward_move, c.side_move, c.buttons, c.view_angles.x, c.view_angles.y, c.view_angles.z
            );
            let _ = writeln!(s, "  end (captured post):    {}", fmt_row(&cap.post[k]));
            let _ = writeln!(s, "  end (ours):             {}", fmt_row(&r.ours_post[k]));
            let win = |ev: &[(u32, EventKind)]| -> Vec<String> {
                ev.iter().filter(|(et, _)| (*et as i64 - t as i64).abs() <= 3).map(|(et, e)| format!("{et}:{e:?}")).collect()
            };
            let _ = writeln!(s, "  events +/-3 ours:   {:?}", win(&r.events_ours));
            let _ = writeln!(s, "  events +/-3 theirs: {:?}", win(&r.events_theirs));
        }
    }
    let _ = writeln!(s, "\nevents ours == theirs: {}", free.events_ours == free.events_theirs);
    if free.events_ours != free.events_theirs {
        let _ = writeln!(s, "  ours:   {:?}", free.events_ours);
        let _ = writeln!(s, "  theirs: {:?}", free.events_theirs);
    }
    s
}

fn fmt_row(r: &Row) -> String {
    format!(
        "o=({:.6}, {:.6}, {:.6}) v=({:.6}, {:.6}, {:.6}) gnd={} duck={:?} da={:?} st={:?} sf={:?} fv={:?}",
        r.origin.x,
        r.origin.y,
        r.origin.z,
        r.velocity.x,
        r.velocity.y,
        r.velocity.z,
        r.ground,
        r.ducked,
        r.duck_amount,
        r.stamina,
        r.surface_friction,
        r.fall_velocity
    )
}

/// Residuals CSV: our value, theirs, and the difference for each compared column.
pub fn residuals_csv(cap: &Capture, r: &RunResult) -> String {
    let mut s = String::from("tick");
    for c in COLUMNS {
        let _ = write!(s, ",{c}_ours,{c}_theirs,{c}_diff");
    }
    s.push('\n');
    for k in 0..cap.cmds.len() {
        let _ = write!(s, "{k}");
        for c in COLUMNS {
            let o = theirs_value(&r.ours_post[k], c).unwrap_or(f32::NAN);
            match theirs_value(&cap.post[k], c) {
                Some(t) => {
                    let _ = write!(s, ",{o:.9},{t:.9},{:.9e}", (o as f64) - (t as f64));
                }
                None => {
                    let _ = write!(s, ",{o:.9},,");
                }
            }
        }
        s.push('\n');
    }
    s
}

/// Self-contained HTML page with speed, z and origin-error plots (plan §9.6 step 7).
pub fn html_report(cap: &Capture, r: &RunResult, text: &str) -> String {
    let n = cap.cmds.len();
    let ours_speed: Vec<f64> = r.ours_post.iter().map(|x| x.velocity.length_2d() as f64).collect();
    let their_speed: Vec<f64> = cap.post.iter().map(|x| x.velocity.length_2d() as f64).collect();
    let ours_z: Vec<f64> = r.ours_post.iter().map(|x| x.origin.z as f64).collect();
    let their_z: Vec<f64> = cap.post.iter().map(|x| x.origin.z as f64).collect();
    let err: Vec<f64> = (0..n)
        .map(|k| {
            let d = r.ours_post[k].origin.sub(cap.post[k].origin);
            ((d.x as f64).powi(2) + (d.y as f64).powi(2) + (d.z as f64).powi(2)).sqrt().max(1e-9).log10()
        })
        .collect();
    let esc = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{name}</title><style>
:root{{--bg:#fff;--fg:#1d1d1f;--mute:#6e6e73;--grid:#e5e5ea;--ours:#2f6fdf;--theirs:#d9822b;--err:#c4314b}}
@media (prefers-color-scheme:dark){{:root{{--bg:#16161a;--fg:#ececf1;--mute:#9a9aa3;--grid:#2c2c33;--ours:#6d9ef7;--theirs:#f0a35e;--err:#f06b84}}}}
body{{background:var(--bg);color:var(--fg);font:14px/1.45 system-ui,sans-serif;margin:0 auto;max-width:1000px;padding:16px}}
h2{{font-size:15px;margin:22px 0 6px}} svg{{width:100%;height:auto;display:block}} pre{{overflow-x:auto;font-size:12px;color:var(--fg)}}
.k{{display:inline-block;width:10px;height:3px;margin:0 4px 3px 10px;vertical-align:middle}}
</style></head><body>
<h1 style="font-size:18px">{name}: {verdict}</h1>
<div style="color:var(--mute)"><span class="k" style="background:var(--ours)"></span>ours<span class="k" style="background:var(--theirs)"></span>captured</div>
<h2>Horizontal speed (u/s)</h2>{speed}
<h2>Origin z</h2>{z}
<h2>log10 origin error (units)</h2>{errp}
<h2>Report</h2><pre>{esc}</pre></body></html>"#,
        name = cap.name(),
        verdict = r.verdict.label(),
        speed = svg_plot(&[(&ours_speed, "var(--ours)"), (&their_speed, "var(--theirs)")]),
        z = svg_plot(&[(&ours_z, "var(--ours)"), (&their_z, "var(--theirs)")]),
        errp = svg_plot(&[(&err, "var(--err)")]),
    )
}

fn svg_plot(series: &[(&Vec<f64>, &str)]) -> String {
    let (w, h, pad) = (960.0, 180.0, 36.0);
    let n = series.iter().map(|(s, _)| s.len()).max().unwrap_or(1).max(2);
    let all = series.iter().flat_map(|(s, _)| s.iter().copied()).filter(|v| v.is_finite());
    let (mut lo, mut hi) = all.fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v), b.max(v)));
    if lo > hi {
        (lo, hi) = (0.0, 1.0);
    }
    if hi - lo < 1e-9 {
        hi = lo + 1.0;
    }
    let x = |i: usize| pad + (w - pad - 8.0) * i as f64 / (n - 1) as f64;
    let y = |v: f64| h - 20.0 - (h - 30.0) * (v - lo) / (hi - lo);
    let mut s = format!(r#"<svg viewBox="0 0 {w} {h}" role="img">"#);
    for v in [lo, (lo + hi) / 2.0, hi] {
        let (x1, y0, ty) = (w - 8.0, y(v), y(v) + 3.0);
        let _ = write!(
            s,
            r#"<line x1="{pad}" x2="{x1}" y1="{y0:.1}" y2="{y0:.1}" stroke="var(--grid)"/><text x="2" y="{ty:.1}" font-size="10" fill="var(--mute)">{v:.3}</text>"#
        );
    }
    let _ = write!(s, r#"<text x="{pad}" y="{}" font-size="10" fill="var(--mute)">tick 0</text>"#, h - 4.0);
    let _ = write!(s, r#"<text x="{}" y="{}" font-size="10" fill="var(--mute)" text-anchor="end">tick {}</text>"#, w - 8.0, h - 4.0, n - 1);
    for (data, color) in series {
        let pts: Vec<String> = data.iter().enumerate().filter(|(_, v)| v.is_finite()).map(|(i, v)| format!("{:.1},{:.1}", x(i), y(*v))).collect();
        let _ = write!(s, r#"<polyline fill="none" stroke="{color}" stroke-width="1.5" points="{}"/>"#, pts.join(" "));
    }
    s.push_str("</svg>");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ulp_and_classes() {
        assert_eq!(ulp_distance(1.0, 1.0), 0);
        assert_eq!(ulp_distance(1.0, f32::from_bits(1.0f32.to_bits() + 3)), 3);
        assert_eq!(ulp_distance(-0.0, 0.0), 0);
        assert_eq!(ulp_distance(f32::from_bits(1), -f32::from_bits(1)), 2);
        let t = Tolerance::default();
        assert_eq!(classify(250.0, 250.0, t), Class::Exact);
        assert_eq!(classify(250.0, f32::from_bits(250.0f32.to_bits() + 2), t), Class::Ulp(2));
        assert_eq!(classify(250.0, 250.0005, t), Class::Close);
        assert_eq!(classify(250.0, 250.01, t), Class::Diverged);
    }
}

//! Text HUD (plan §7): always-on stats, per-jump panel, and a debug panel. Plus debug gizmos.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::text::{FontSize, FontSource};
use movement::jumpstats::JumpReport;
use movement::state::MoveType;
use movement::trace::Hull;

use crate::camera::to_bevy;
use crate::input::ViewAngles;
use crate::modes::HELP;
use crate::sim::Sim;

#[derive(Component)]
pub struct StatsText;
#[derive(Component)]
pub struct SpeedText;
#[derive(Component)]
pub struct JumpText;
#[derive(Component)]
pub struct DebugText;

fn mono(size: f32) -> TextFont {
    TextFont::from(FontSource::Monospace).with_font_size(FontSize::Px(size))
}

pub fn spawn_hud(mut commands: Commands) {
    let panel = |left: Option<f32>, right: Option<f32>, top: Option<f32>, bottom: Option<f32>| Node {
        position_type: PositionType::Absolute,
        left: left.map_or(Val::Auto, Val::Px),
        right: right.map_or(Val::Auto, Val::Px),
        top: top.map_or(Val::Auto, Val::Px),
        bottom: bottom.map_or(Val::Auto, Val::Px),
        padding: UiRect::all(Val::Px(6.0)),
        ..Default::default()
    };
    let bg = BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45));
    commands.spawn((Text::new(""), mono(15.0), panel(Some(8.0), None, Some(8.0), None), bg, StatsText));
    commands.spawn((Text::new(""), mono(15.0), panel(None, Some(8.0), Some(8.0), None), bg, JumpText));
    commands.spawn((Text::new(""), mono(14.0), panel(Some(8.0), None, None, Some(8.0)), bg, DebugText));
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            bottom: Val::Px(90.0),
            justify_content: JustifyContent::Center,
            ..Default::default()
        })
        .with_children(|p| {
            p.spawn((Text::new(""), mono(30.0), TextColor(Color::srgb(1.0, 1.0, 1.0)), SpeedText));
        });
    // Crosshair.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..Default::default()
        })
        .with_children(|p| {
            p.spawn((Node { width: Val::Px(4.0), height: Val::Px(4.0), ..Default::default() }, BackgroundColor(Color::srgb(0.2, 1.0, 0.3))));
        });
}

fn sparkline(values: impl Iterator<Item = f32>, width: usize) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let v: Vec<f32> = values.collect();
    if v.is_empty() {
        return String::new();
    }
    let chunk = v.len().div_ceil(width).max(1);
    let pts: Vec<f32> = v.chunks(chunk).map(|c| c.iter().cloned().fold(0.0, f32::max)).collect();
    let max = pts.iter().cloned().fold(1.0, f32::max);
    pts.iter().map(|x| BARS[((x / max) * 7.0).round().clamp(0.0, 7.0) as usize]).collect()
}

fn jump_panel(r: &JumpReport) -> String {
    let mut s = format!(
        "{} {:.4}   (no +32: {:.4})\nraw final-cmd origin: {:.4}\n",
        r.jump_type.short(),
        r.distance,
        r.distance_no_offset,
        r.distance_raw
    );
    s += &format!(
        "pre {:.2}  takeoff {:.2}  max {:.2}\nheight {:.3}  block {:.3}  air {} t\nstrafes {}  sync {:.1}%  {}\n",
        r.pre_speed,
        r.takeoff_speed,
        r.max_speed,
        r.height,
        r.block_height,
        r.airtime_ticks,
        r.strafes.len(),
        r.sync,
        if r.perfect { "perf" } else { "not perf" }
    );
    let mut flags = Vec::new();
    if r.jumpbug {
        flags.push("JUMPBUG");
    }
    if r.edgebug_in_air {
        flags.push("EDGEBUG");
    }
    if r.duckbug_landing {
        flags.push("DUCKBUG");
    }
    if !flags.is_empty() {
        s += &format!("{}\n", flags.join(" "));
    }
    for (i, st) in r.strafes.iter().enumerate().take(12) {
        let sync = if st.ticks > 0 { st.sync_ticks as f32 / st.ticks as f32 * 100.0 } else { 0.0 };
        s += &format!("  {:>2}: +{:6.2} -{:6.2} {:3}t {:5.1}%\n", i + 1, st.gain, st.loss, st.ticks, sync);
    }
    s
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_hud(
    sim: Res<Sim>,
    angles: Res<ViewAngles>,
    diagnostics: Res<DiagnosticsStore>,
    mut stats: Single<&mut Text, (With<StatsText>, Without<SpeedText>, Without<JumpText>, Without<DebugText>)>,
    mut speed: Single<&mut Text, (With<SpeedText>, Without<JumpText>, Without<DebugText>)>,
    mut jump: Single<&mut Text, (With<JumpText>, Without<DebugText>)>,
    mut debug: Single<&mut Text, With<DebugText>>,
    ghost: Option<Res<crate::ghost::Ghost>>,
) {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let s = &sim.state;
    let o = s.origin;
    let mut flags = Vec::new();
    if sim.autohop {
        flags.push("AUTOHOP");
    }
    if sim.paused {
        flags.push("PAUSED");
    }
    if sim.recorder.is_recording() {
        flags.push("REC");
    }
    if sim.discarded_time_events > 0 {
        flags.push("TIME-DISCARDED");
    }
    stats.0 = format!(
        "{} {} tick  {:.0} fps  {}\n\
         speed {:.2}  vz {:.2}  pre {:.2}\n\
         pos {:.3} {:.3} {:.3}\n\
         ang {:.3} {:.3}\n\
         ground {}  ent {:?}  move {:?}\n\
         duck {:.3} {}  dspeed {:.2}\n\
         sf {:.2}  stamina {:.3}  prestrafe x{:.4}\n\
         tick {}  {}\n{}",
        sim.kind.name(),
        sim.tickrate,
        fps,
        flags.join(" "),
        s.horizontal_speed(),
        s.velocity.z,
        sim.tracker.last_takeoff_speed,
        o.x,
        o.y,
        o.z,
        angles.pitch,
        angles.yaw,
        if s.on_ground() { "yes" } else { "no" },
        s.ground_entity.map(|e| e.0),
        s.move_type,
        s.duck_amount,
        if s.ducked { "(hull)" } else { "" },
        s.duck_speed,
        s.surface_friction,
        s.stamina,
        sim.mode.prestrafe_multiplier(),
        sim.tick,
        sim.recorder.last_saved.as_ref().map_or(String::new(), |p| format!("saved {}", p.display())),
        HELP,
    );
    speed.0 = format!("{:.0}", s.horizontal_speed());
    jump.0 = sim.last_report.as_ref().map_or("no jump yet".to_string(), jump_panel);

    if sim.debug_draw {
        let mv = &sim.last_mv;
        let mut t = format!(
            "wish dir {:.3} {:.3}  wish speed {:.2}  max {:.2}\nbudget {:.4}  flags {:?}\n",
            mv.wish_dir.x, mv.wish_dir.y, mv.wish_speed, mv.max_speed, mv.accel_budget, sim.last_flags
        );
        for b in &sim.traces.bumps {
            t += &format!(
                "bump {:?}#{} frac {:.5} n ({:.3} {:.3} {:.3}) solid {}/{}\n",
                b.phase,
                b.bump,
                b.trace.fraction,
                b.trace.plane_normal.x,
                b.trace.plane_normal.y,
                b.trace.plane_normal.z,
                b.trace.start_solid,
                b.trace.all_solid
            );
        }
        t += &format!("speed {}\n", sparkline(sim.speed_history.iter().copied(), 64));
        debug.0 = t;
    } else {
        debug.0 = String::new();
    }
    if let Some(g) = ghost {
        debug.0 = crate::ghost::panel(&g, s) + &debug.0;
    }
}

pub fn debug_gizmos(sim: Res<Sim>, mut gizmos: Gizmos) {
    if !sim.debug_draw {
        return;
    }
    let s = &sim.state;
    let hull = if s.ducked { Hull::DUCK } else { Hull::STAND };
    let color = if s.move_type == MoveType::Ladder { Color::srgb(1.0, 0.6, 0.1) } else { Color::srgb(0.2, 1.0, 0.4) };
    // Only the footprint and corner posts: the full box would surround the first-person camera.
    let corners = [(hull.mins.x, hull.mins.y), (hull.maxs.x, hull.mins.y), (hull.maxs.x, hull.maxs.y), (hull.mins.x, hull.maxs.y)];
    for i in 0..4 {
        let (ax, ay) = corners[i];
        let (bx, by) = corners[(i + 1) % 4];
        let a = s.origin.add(movement::Vec3::new(ax, ay, 0.0));
        let b = s.origin.add(movement::Vec3::new(bx, by, 0.0));
        gizmos.line(to_bevy(a), to_bevy(b), color);
        gizmos.line(to_bevy(a), to_bevy(a.add(movement::Vec3::new(0.0, 0.0, 8.0))), color);
    }
    for b in &sim.traces.bumps {
        gizmos.line(to_bevy(b.start), to_bevy(b.trace.end_pos), Color::srgb(1.0, 1.0, 0.0));
        if b.trace.fraction < 1.0 {
            let p = to_bevy(b.trace.end_pos);
            gizmos.arrow(p, p + to_bevy(b.trace.plane_normal) * 24.0, Color::srgb(1.0, 0.2, 0.2));
        }
    }
    // Velocity vector from the feet.
    let feet = to_bevy(s.origin);
    gizmos.line(feet, feet + to_bevy(s.velocity) * 0.1, Color::srgb(0.3, 0.6, 1.0));
}

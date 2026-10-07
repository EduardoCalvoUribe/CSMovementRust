//! Hotkeys: mode switching, tickrate, reset, checkpoints, toggles, recording, teleports (plan §6.4).

use bevy::prelude::*;
use movement::ModeKind;

use crate::input::ViewAngles;
use crate::level::Level;
use crate::sim::Sim;

#[derive(Resource)]
pub struct LevelInfo(pub Level);

pub const HELP: &str = "F1/F2/F3 mode  T 64/128  R spawn  F6 save  F7 load  H autohop  G debug  F5 record  \
P pause  . step  1-9 areas  LMB grab  Esc release";

pub fn hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    mut sim: ResMut<Sim>,
    mut angles: ResMut<ViewAngles>,
    level: Res<LevelInfo>,
    ghost: Option<ResMut<crate::ghost::Ghost>>,
) {
    if let Some(mut g) = ghost {
        // A ghost run owns the sim: R restarts the capture; mode and teleport keys are ignored.
        if keys.just_pressed(KeyCode::KeyR) {
            let _ = g.reset(&mut sim);
        }
        if keys.just_pressed(KeyCode::KeyP) {
            sim.paused = !sim.paused;
        }
        if keys.just_pressed(KeyCode::Period) {
            sim.step_once = true;
        }
        if keys.just_pressed(KeyCode::KeyG) {
            sim.debug_draw = !sim.debug_draw;
        }
        return;
    }
    let mode_keys = [(KeyCode::F1, ModeKind::Vanilla), (KeyCode::F2, ModeKind::KzTimer), (KeyCode::F3, ModeKind::SimpleKz)];
    for (key, kind) in mode_keys {
        if keys.just_pressed(key) && sim.kind != kind {
            sim.set_mode(kind);
            info!("mode {} at {} tick", kind.name(), sim.tickrate);
        }
    }
    if keys.just_pressed(KeyCode::KeyT) {
        let rate = if sim.tickrate == 64 { 128 } else { 64 };
        sim.set_tickrate(rate);
    }
    if keys.just_pressed(KeyCode::KeyR) {
        let (origin, yaw) = sim.spawn;
        sim.teleport(origin);
        *angles = ViewAngles { pitch: 0.0, yaw };
    }
    if keys.just_pressed(KeyCode::F6) {
        sim.checkpoint = Some((sim.state.clone(), *angles));
    }
    if keys.just_pressed(KeyCode::F7) {
        if let Some((state, a)) = sim.checkpoint.clone() {
            sim.teleport(state.origin);
            sim.state = state;
            sim.state.velocity = movement::Vec3::ZERO;
            sim.prev_state = sim.state.clone();
            *angles = a;
        }
    }
    if keys.just_pressed(KeyCode::KeyH) {
        sim.recorder.stop_if_recording();
        sim.autohop = !sim.autohop;
    }
    if keys.just_pressed(KeyCode::KeyG) {
        sim.debug_draw = !sim.debug_draw;
    }
    if keys.just_pressed(KeyCode::KeyP) {
        sim.paused = !sim.paused;
    }
    if keys.just_pressed(KeyCode::Period) && sim.paused {
        sim.step_once = true;
    }
    if keys.just_pressed(KeyCode::F5) {
        if sim.recorder.is_recording() {
            sim.recorder.stop_if_recording();
        } else {
            let (kind, rate, auto, start) = (sim.kind, sim.tickrate, sim.autohop, sim.state.clone());
            sim.discarded_time_events = 0;
            sim.recorder.start(kind, rate, auto, &start);
        }
    }
    let digits = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    for (i, key) in digits.into_iter().enumerate() {
        if keys.just_pressed(key) {
            if let Some(area) = level.0.areas.get(i) {
                sim.teleport(area.spawn);
                *angles = ViewAngles { pitch: 0.0, yaw: area.yaw };
                info!("teleport: {}", area.name);
            }
        }
    }
}

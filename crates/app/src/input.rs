//! Keyboard and mouse to view angles and held buttons (plan §6.1).
//!
//! Input-per-tick contract: `Update`-rate systems only accumulate. The fixed tick builds exactly one
//! `UserCmd` from the current angles, held keys and latched edges, then clears the latches.

use bevy::input::mouse::{AccumulatedMouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use movement::cmd::Buttons;
use movement::{UserCmd, Vec3 as SVec3};

/// Source-style mouse settings: degrees per count = sensitivity * m_yaw (0.022, to confirm in S12).
#[derive(Resource)]
pub struct MouseSettings {
    pub sensitivity: f32,
    pub m_yaw: f32,
    pub m_pitch: f32,
}

impl Default for MouseSettings {
    fn default() -> Self {
        Self { sensitivity: 2.0, m_yaw: 0.022, m_pitch: 0.022 }
    }
}

#[derive(Resource, Default, Clone, Copy)]
pub struct ViewAngles {
    pub pitch: f32,
    pub yaw: f32,
}

/// Keys held right now, plus edges latched since the last tick so a press and release between two
/// ticks (a scroll-wheel jump) still lands on exactly one command.
#[derive(Resource, Default)]
pub struct HeldInput {
    pub held: Buttons,
    pub latched: Buttons,
}

impl HeldInput {
    /// Buttons for the next command. Call `consume` after the tick runs.
    pub fn buttons(&self) -> Buttons {
        self.held | self.latched
    }
    pub fn consume(&mut self) {
        self.latched = Buttons::NONE;
    }
}

pub fn build_cmd(tick: u32, angles: ViewAngles, buttons: Buttons) -> UserCmd {
    UserCmd::from_buttons(tick, SVec3::new(angles.pitch, angles.yaw, 0.0), buttons)
}

pub fn grab_cursor(
    mut cursor: Single<&mut CursorOptions>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if mouse.just_pressed(MouseButton::Left) {
        cursor.visible = false;
        cursor.grab_mode = CursorGrabMode::Locked;
    }
    if keys.just_pressed(KeyCode::Escape) {
        cursor.visible = true;
        cursor.grab_mode = CursorGrabMode::None;
    }
}

pub fn accumulate_input(
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    cursor: Single<&CursorOptions>,
    settings: Res<MouseSettings>,
    mut angles: ResMut<ViewAngles>,
    mut input: ResMut<HeldInput>,
) {
    let grabbed = cursor.grab_mode != CursorGrabMode::None;
    if grabbed {
        let d = motion.delta;
        angles.yaw -= d.x * settings.sensitivity * settings.m_yaw;
        angles.pitch = (angles.pitch + d.y * settings.sensitivity * settings.m_pitch).clamp(-89.0, 89.0);
        angles.yaw = angles.yaw.rem_euclid(360.0);
    }

    let map = [
        (KeyCode::KeyW, Buttons::FORWARD),
        (KeyCode::KeyS, Buttons::BACK),
        (KeyCode::KeyA, Buttons::LEFT),
        (KeyCode::KeyD, Buttons::RIGHT),
        (KeyCode::Space, Buttons::JUMP),
        (KeyCode::ControlLeft, Buttons::DUCK),
        (KeyCode::ShiftLeft, Buttons::WALK),
        (KeyCode::KeyE, Buttons::USE),
    ];
    let mut held = Buttons::NONE;
    for (key, b) in map {
        if keys.pressed(key) {
            held.insert(b);
        }
        if keys.just_pressed(key) {
            input.latched.insert(b);
        }
    }
    input.held = held;

    // Every wheel notch is a jump press and release inside this frame window.
    if wheel.read().count() > 0 {
        input.latched.insert(Buttons::JUMP);
    }
}

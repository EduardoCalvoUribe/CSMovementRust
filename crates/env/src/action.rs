//! What a player controls each tick (plan §4).

use movement::{Buttons, UserCmd, Vec3};

/// One tick of player input.
///
/// - `forward`: +1 forward (W), -1 back (S), 0 none. `side`: +1 right (D), -1 left (A). Only the sign
///   is used, so any `i8` is valid; the sim receives +-450 as from keys (`UserCmd::from_buttons`).
/// - `yaw_delta` / `pitch_delta`: degrees to turn this tick. Positive yaw turns left (Source yaw grows
///   counter-clockwise); positive pitch looks down. The env clamps them to the configured per-tick limit.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Action {
    pub forward: i8,
    pub side: i8,
    pub jump: bool,
    pub duck: bool,
    pub use_: bool,
    pub yaw_delta: f32,
    pub pitch_delta: f32,
}

impl Action {
    pub const IDLE: Action =
        Action { forward: 0, side: 0, jump: false, duck: false, use_: false, yaw_delta: 0.0, pitch_delta: 0.0 };

    pub fn buttons(&self) -> Buttons {
        let mut b = Buttons::NONE;
        match self.forward.signum() {
            1 => b.insert(Buttons::FORWARD),
            -1 => b.insert(Buttons::BACK),
            _ => {}
        }
        match self.side.signum() {
            1 => b.insert(Buttons::RIGHT),
            -1 => b.insert(Buttons::LEFT),
            _ => {}
        }
        if self.jump {
            b.insert(Buttons::JUMP);
        }
        if self.duck {
            b.insert(Buttons::DUCK);
        }
        if self.use_ {
            b.insert(Buttons::USE);
        }
        b
    }

    /// The command for this action at already-integrated view angles (pitch, yaw, roll).
    pub fn to_cmd(&self, tick: u32, view_angles: Vec3) -> UserCmd {
        UserCmd::from_buttons(tick, view_angles, self.buttons())
    }
}

impl Action {
    /// The action that sends `cmd`'s keys with the given view turn (`yaw_delta`, `pitch_delta`).
    pub fn from_cmd(cmd: &UserCmd, (yaw_delta, pitch_delta): (f32, f32)) -> Action {
        let key = |m: f32| (m > 0.0) as i8 - (m < 0.0) as i8;
        Action {
            forward: key(cmd.forward_move),
            side: key(cmd.side_move),
            jump: cmd.buttons.contains(Buttons::JUMP),
            duck: cmd.buttons.contains(Buttons::DUCK),
            use_: cmd.buttons.contains(Buttons::USE),
            yaw_delta,
            pitch_delta,
        }
    }

    /// A toy course's reference solution as actions. Through a default `Env` they reproduce the
    /// solution's commands bit for bit.
    pub fn from_toy(toy: &course::ToyCourse) -> Vec<Action> {
        toy.solution.iter().zip(&toy.view_deltas).map(|(c, d)| Action::from_cmd(c, *d)).collect()
    }
}

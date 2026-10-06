//! User commands: the only way input reaches the simulation [Ref §2].

use crate::math::Vec3;

/// Button bitflags. Bit values are our own; only the set membership matters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Buttons(pub u32);

impl Buttons {
    pub const NONE: Buttons = Buttons(0);
    pub const JUMP: Buttons = Buttons(1 << 0);
    pub const DUCK: Buttons = Buttons(1 << 1);
    pub const FORWARD: Buttons = Buttons(1 << 2);
    pub const BACK: Buttons = Buttons(1 << 3);
    pub const LEFT: Buttons = Buttons(1 << 4);
    pub const RIGHT: Buttons = Buttons(1 << 5);
    pub const WALK: Buttons = Buttons(1 << 6);
    pub const USE: Buttons = Buttons(1 << 7);

    pub fn contains(self, b: Buttons) -> bool {
        self.0 & b.0 == b.0
    }
    pub fn intersects(self, b: Buttons) -> bool {
        self.0 & b.0 != 0
    }
    pub fn insert(&mut self, b: Buttons) {
        self.0 |= b.0;
    }
    pub fn remove(&mut self, b: Buttons) {
        self.0 &= !b.0;
    }
    pub fn with(mut self, b: Buttons) -> Buttons {
        self.insert(b);
        self
    }
}

impl core::ops::BitOr for Buttons {
    type Output = Buttons;
    fn bitor(self, rhs: Buttons) -> Buttons {
        Buttons(self.0 | rhs.0)
    }
}

/// Source movement input magnitude for a full key press (`cl_forwardspeed` and friends).
pub const MOVE_INPUT: f32 = 450.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UserCmd {
    pub tick: u32,
    /// (pitch, yaw, roll) in degrees.
    pub view_angles: Vec3,
    pub forward_move: f32,
    pub side_move: f32,
    pub up_move: f32,
    pub buttons: Buttons,
}

impl UserCmd {
    /// Build forward/side moves from the direction buttons, as the client input code does.
    pub fn from_buttons(tick: u32, view_angles: Vec3, buttons: Buttons) -> Self {
        let mut forward_move = 0.0;
        let mut side_move = 0.0;
        if buttons.contains(Buttons::FORWARD) {
            forward_move += MOVE_INPUT;
        }
        if buttons.contains(Buttons::BACK) {
            forward_move -= MOVE_INPUT;
        }
        if buttons.contains(Buttons::RIGHT) {
            side_move += MOVE_INPUT;
        }
        if buttons.contains(Buttons::LEFT) {
            side_move -= MOVE_INPUT;
        }
        Self { tick, view_angles, forward_move, side_move, up_move: 0.0, buttons }
    }
}

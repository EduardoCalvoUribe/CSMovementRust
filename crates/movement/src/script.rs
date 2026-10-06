//! Scenario scripts as data: a small command DSL (plan §11).
//!
//! Text form, one step per line or separated by `;`:
//!
//! ```text
//! look 90 0          # set yaw=90, pitch=0
//! W 40               # hold W for 40 ticks
//! WJ 1               # W + jump for 1 tick
//! A 30 yaw 0.8       # hold A for 30 ticks, turning yaw +0.8 deg per tick
//! - 10               # no input for 10 ticks
//! ```
//!
//! Keys: `W A S D` move, `J` jump, `C` crouch, `H` walk (shift).

use crate::cmd::{Buttons, UserCmd};
use crate::math::Vec3;

#[derive(Clone, Debug, Default)]
pub struct Script {
    cmds: Vec<UserCmd>,
    angles: Vec3,
    tick: u32,
}

impl Script {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn look(mut self, yaw: f32, pitch: f32) -> Self {
        self.angles = Vec3::new(pitch, yaw, 0.0);
        self
    }

    pub fn hold(self, buttons: Buttons, ticks: u32) -> Self {
        self.hold_turning(buttons, ticks, 0.0)
    }

    pub fn hold_turning(mut self, buttons: Buttons, ticks: u32, yaw_per_tick: f32) -> Self {
        for _ in 0..ticks {
            self.angles.y += yaw_per_tick;
            self.cmds.push(UserCmd::from_buttons(self.tick, self.angles, buttons));
            self.tick += 1;
        }
        self
    }

    pub fn idle(self, ticks: u32) -> Self {
        self.hold(Buttons::NONE, ticks)
    }

    pub fn push(mut self, mut cmd: UserCmd) -> Self {
        cmd.tick = self.tick;
        self.angles = cmd.view_angles;
        self.cmds.push(cmd);
        self.tick += 1;
        self
    }

    pub fn build(self) -> Vec<UserCmd> {
        self.cmds
    }

    pub fn parse(text: &str) -> Result<Script, String> {
        let mut s = Script::new();
        for raw in text.split(['\n', ';']) {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            let num = |i: usize| -> Result<f32, String> {
                parts
                    .get(i)
                    .ok_or_else(|| format!("missing number in `{line}`"))?
                    .parse::<f32>()
                    .map_err(|e| format!("`{line}`: {e}"))
            };
            if parts[0] == "look" {
                s = s.look(num(1)?, num(2)?);
                continue;
            }
            let buttons = parse_keys(parts[0])?;
            let ticks = num(1)? as u32;
            let yaw = if parts.get(2) == Some(&"yaw") { num(3)? } else { 0.0 };
            s = s.hold_turning(buttons, ticks, yaw);
        }
        Ok(s)
    }
}

pub fn parse_keys(keys: &str) -> Result<Buttons, String> {
    let mut b = Buttons::NONE;
    if keys == "-" {
        return Ok(b);
    }
    for c in keys.chars() {
        b.insert(match c.to_ascii_uppercase() {
            'W' => Buttons::FORWARD,
            'A' => Buttons::LEFT,
            'S' => Buttons::BACK,
            'D' => Buttons::RIGHT,
            'J' => Buttons::JUMP,
            'C' => Buttons::DUCK,
            'H' => Buttons::WALK,
            _ => return Err(format!("unknown key `{c}` in `{keys}`")),
        });
    }
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_steps() {
        let cmds = Script::parse("look 90 0; W 3\nA 2 yaw 1.5 # comment\n- 1").unwrap().build();
        assert_eq!(cmds.len(), 6);
        assert!(cmds[0].buttons.contains(Buttons::FORWARD));
        assert_eq!(cmds[0].forward_move, 450.0);
        assert_eq!(cmds[4].view_angles.y, 93.0);
        assert_eq!(cmds[4].side_move, -450.0);
        assert_eq!(cmds[5].buttons, Buttons::NONE);
        assert_eq!(cmds[5].tick, 5);
    }
}

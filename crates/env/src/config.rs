//! Environment settings (plan §4).

use movement::ModeKind;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvConfig {
    pub mode: ModeKind,
    /// `None` uses the mode's required tickrate, or 128 if it has none. A tickrate the mode forbids is an
    /// error in `Env::new`.
    pub tickrate: Option<u32>,
    /// Decided off (plan §10.1): the agent must release and press jump around landings.
    pub autobhop: bool,
    /// Largest yaw change per tick, degrees. Stands in for mouse sensitivity x counts per tick; the default
    /// is a placeholder, not a measured human limit.
    pub max_yaw_per_tick: f32,
    /// Largest pitch change per tick, degrees (placeholder, as above).
    pub max_pitch_per_tick: f32,
    /// Ticks each `step` runs with the same action. 1 hides nothing about timing.
    pub action_repeat: u32,
    /// Episode length limit in ticks; reaching it truncates (not terminates) the episode.
    pub max_episode_ticks: u32,
}

impl Default for EnvConfig {
    fn default() -> Self {
        Self {
            mode: ModeKind::SimpleKz,
            tickrate: None,
            autobhop: false,
            max_yaw_per_tick: 10.0,
            max_pitch_per_tick: 10.0,
            action_repeat: 1,
            max_episode_ticks: 128 * 120,
        }
    }
}

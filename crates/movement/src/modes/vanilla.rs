//! GOKZ Vanilla: stock rules, config only [Ref §20].

use super::{ModeKind, MovementMode};
use crate::config::MovementConfig;

pub struct Vanilla {
    cfg: MovementConfig,
}

impl Vanilla {
    pub fn new() -> Self {
        Self { cfg: MovementConfig::vanilla() }
    }
}

impl Default for Vanilla {
    fn default() -> Self {
        Self::new()
    }
}

impl MovementMode for Vanilla {
    fn kind(&self) -> ModeKind {
        ModeKind::Vanilla
    }
    fn config(&self) -> &MovementConfig {
        &self.cfg
    }
}

//! GOKZ Vanilla: stock rules, config only [Ref §20].

use super::{ModeKind, MovementMode};
use crate::config::MovementConfig;

#[derive(Clone)]
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
    fn fork(&self) -> Box<dyn MovementMode + Send + Sync> {
        Box::new(self.clone())
    }
    fn kind(&self) -> ModeKind {
        ModeKind::Vanilla
    }
    fn config(&self) -> &MovementConfig {
        &self.cfg
    }
}

#![allow(dead_code)]

use movement::instrument::{EventLog, MoveObserver, Tee, TechniqueDetector};
use movement::trace::DIST_EPSILON;
use movement::*;

pub const DT64: f32 = 1.0 / 64.0;
pub const DT128: f32 = 1.0 / 128.0;

pub struct Sim<W: TraceWorld> {
    pub mode: Box<dyn MovementMode + Send + Sync>,
    pub cfg: MovementConfig,
    pub world: W,
    pub state: PlayerState,
    pub dt: f32,
    pub det: TechniqueDetector,
    pub tick: u32,
}

impl Sim<PrimitiveWorld> {
    /// Flat floor with its top at z = 0, player resting on it.
    pub fn flat(kind: ModeKind, tickrate: u32) -> Self {
        let mut s = Sim::new(PrimitiveWorld::flat_floor(0.0), kind, tickrate, Vec3::new(0.0, 0.0, DIST_EPSILON));
        s.idle(4);
        assert!(s.state.on_ground());
        s
    }
}

impl<W: TraceWorld> Sim<W> {
    pub fn new(world: W, kind: ModeKind, tickrate: u32, origin: Vec3) -> Self {
        let mode = kind.create();
        let cfg = *mode.config();
        Self {
            mode,
            cfg,
            world,
            state: PlayerState::new(origin),
            dt: tick_interval(tickrate),
            det: TechniqueDetector::default(),
            tick: 0,
        }
    }

    pub fn step(&mut self, mut cmd: UserCmd) -> TechniqueFlags {
        cmd.tick = self.tick;
        self.tick += 1;
        process_movement(&self.cfg, self.mode.as_mut(), &self.world, &mut self.state, &cmd, &mut self.det, self.dt);
        self.det.last
    }

    pub fn step_obs<O: MoveObserver>(&mut self, mut cmd: UserCmd, obs: &mut O) -> TechniqueFlags {
        cmd.tick = self.tick;
        self.tick += 1;
        let mut tee = Tee(&mut self.det, obs);
        process_movement(&self.cfg, self.mode.as_mut(), &self.world, &mut self.state, &cmd, &mut tee, self.dt);
        self.det.last
    }

    pub fn step_logged(&mut self, cmd: UserCmd) -> (TechniqueFlags, Vec<String>) {
        let mut log = EventLog::default();
        let f = self.step_obs(cmd, &mut log);
        (f, log.lines)
    }

    pub fn press(&mut self, buttons: Buttons, yaw: f32) -> TechniqueFlags {
        self.step(UserCmd::from_buttons(0, Vec3::new(0.0, yaw, 0.0), buttons))
    }

    pub fn hold(&mut self, buttons: Buttons, yaw: f32, ticks: u32) {
        for _ in 0..ticks {
            self.press(buttons, yaw);
        }
    }

    pub fn idle(&mut self, ticks: u32) {
        self.hold(Buttons::NONE, 0.0, ticks);
    }

    pub fn run(&mut self, cmds: &[UserCmd]) -> Vec<(PlayerState, TechniqueFlags)> {
        cmds.iter().map(|c| (self.step(*c), self.state.clone())).map(|(f, s)| (s, f)).collect()
    }
}

pub fn approx(a: f32, b: f32, eps: f32) {
    assert!((a - b).abs() <= eps, "{a} != {b} (eps {eps})");
}

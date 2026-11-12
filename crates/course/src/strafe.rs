//! A strafing reference driver for toy-course solutions that need speed above 250 u/s (long jumps,
//! bhop chains). Closed-loop over our own sim, frozen into a `UserCmd` list like every other solution.
//!
//! The strafe logic follows the capture rig's scenario generator (`tools/compare/src/scenarios.rs`:
//! `optimal_yaw`, `run_and_jump_at_edge`, `sync_strafes`): each air tick the view is set so the strafe
//! key's wish direction sits at the max-gain angle to the velocity [Ref §6], and strafes alternate sides
//! every `half` ticks, with a shorter first strafe so the S-shaped path stays centred. `fork` uses the
//! modes' `fork()` (R2), so probing a command costs one tick.

use movement::{process_movement, tick_interval, Buttons, MovementConfig, MovementMode, NullObserver, PlayerState, TraceWorld, UserCmd, Vec3};

use crate::toy::{SOLUTION_MAX_TURN, SOLUTION_MODE, SOLUTION_TICKRATE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn other(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
}

pub struct Driver<'w, W: TraceWorld + ?Sized> {
    world: &'w W,
    cfg: MovementConfig,
    mode: Box<dyn MovementMode + Send + Sync>,
    dt: f32,
    pub state: PlayerState,
    pub cmds: Vec<UserCmd>,
    /// The (yaw, pitch) change applied on each command, as `env::Action` deltas.
    pub deltas: Vec<(f32, f32)>,
    pub yaw: f32,
    pub pitch: f32,
    /// Turn applied on the next `step`, already clamped.
    pending_yaw: f32,
}

impl<'w, W: TraceWorld + ?Sized> Driver<'w, W> {
    /// A driver in `SOLUTION_MODE` at `SOLUTION_TICKRATE`, autobhop off, from `PlayerState::new(origin)`.
    pub fn new(world: &'w W, origin: Vec3, yaw: f32) -> Self {
        let mode = SOLUTION_MODE.create();
        let cfg = *mode.config();
        Self {
            world,
            cfg,
            mode,
            dt: tick_interval(SOLUTION_TICKRATE),
            state: PlayerState::new(origin),
            cmds: Vec::new(),
            deltas: Vec::new(),
            yaw,
            pitch: 0.0,
            pending_yaw: 0.0,
        }
    }

    /// Turn toward `target` on the next step, by at most `SOLUTION_MAX_TURN`. The new yaw is computed the
    /// way `env::Env` integrates a yaw delta (clamp, add, `rem_euclid(360)`), so feeding `deltas` to an
    /// `Env` reproduces these commands bit for bit.
    pub fn turn_to(&mut self, target: f32) {
        let mut d = (target - self.yaw).rem_euclid(360.0);
        if d > 180.0 {
            d -= 360.0;
        }
        self.pending_yaw = d.clamp(-SOLUTION_MAX_TURN, SOLUTION_MAX_TURN);
    }

    pub fn fork(&self) -> Self {
        Self {
            world: self.world,
            cfg: self.cfg,
            mode: self.mode.fork(),
            dt: self.dt,
            state: self.state.clone(),
            cmds: self.cmds.clone(),
            deltas: self.deltas.clone(),
            yaw: self.yaw,
            pitch: self.pitch,
            pending_yaw: self.pending_yaw,
        }
    }

    pub fn step(&mut self, b: Buttons) {
        let d = self.pending_yaw;
        self.pending_yaw = 0.0;
        self.yaw = (self.yaw + d).rem_euclid(360.0);
        let c = UserCmd::from_buttons(self.cmds.len() as u32, Vec3::new(self.pitch, self.yaw, 0.0), b);
        process_movement(&self.cfg, self.mode.as_mut(), self.world, &mut self.state, &c, &mut NullObserver, self.dt);
        self.cmds.push(c);
        self.deltas.push((d, 0.0));
    }

    /// Whether `b` this tick would leave the player airborne, without committing it.
    pub fn would_leave_ground(&self, b: Buttons) -> bool {
        let mut probe = self.fork();
        probe.step(b);
        !probe.state.on_ground()
    }

    /// Hold `b` until the next tick would leave the ground, then jump on the last grounded tick.
    /// Returns false if that never happened within `max_ticks`.
    pub fn run_and_jump_at_edge(&mut self, b: Buttons, max_ticks: u32) -> bool {
        for _ in 0..max_ticks {
            if self.state.on_ground() && self.would_leave_ground(b) {
                self.step(b | Buttons::JUMP);
                return true;
            }
            self.step(b);
        }
        false
    }

    /// Yaw that puts the strafe key's wish direction at the max-gain angle to the velocity [Ref §6]. With
    /// SimpleKZ's air accelerate one tick's acceleration exceeds the 30 u/s wish cap, so the angle is 90
    /// degrees: the wish direction is perpendicular to the velocity.
    pub fn optimal_yaw(&self, side: Side) -> f32 {
        let v = self.state.velocity;
        let speed = v.length_2d();
        if speed < 1.0 {
            return self.yaw;
        }
        let accel = self.cfg.air_accelerate * self.cfg.weapon_max_speed * self.dt * self.state.surface_friction;
        let cos = ((self.cfg.air_wish_cap - accel).max(0.0) / speed).clamp(-1.0, 1.0);
        let theta = cos.acos().to_degrees();
        let vel_yaw = v.y.atan2(v.x).to_degrees();
        let yaw = match side {
            // D: wish = right vector (yaw - 90); turns clockwise.
            Side::Right => vel_yaw - theta + 90.0,
            // A: wish = -right (yaw + 90); turns counter-clockwise.
            Side::Left => vel_yaw + theta - 90.0,
        };
        yaw.rem_euclid(360.0)
    }

    /// One optimal air-strafe tick on `side`, plus `extra` buttons.
    pub fn strafe_tick(&mut self, side: Side, extra: Buttons) {
        let target = self.optimal_yaw(side);
        self.turn_to(target);
        let key = if side == Side::Right { Buttons::RIGHT } else { Buttons::LEFT };
        self.step(key | extra);
    }

    /// Alternate optimal strafes, the first lasting `first` ticks and later ones `half`, until the player
    /// lands or `max_ticks` pass. Starts on `side`.
    pub fn sync_strafes(&mut self, side: Side, first: u32, half: u32, max_ticks: u32) {
        let mut side = side;
        let mut len = first.max(1);
        let mut n = 0;
        while n < max_ticks {
            for _ in 0..len {
                if self.state.on_ground() || n >= max_ticks {
                    return;
                }
                self.strafe_tick(side, Buttons::NONE);
                n += 1;
            }
            len = half.max(1);
            side = side.other();
        }
    }
}

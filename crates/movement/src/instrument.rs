//! Observation points inside the tick pipeline [Ref §22, plan §5.5].
//!
//! All methods default to no-ops; `NullObserver` compiles away. Events carry the before/after origin and
//! velocity so detectors can see intermediate states that a pre/post-command log would miss [Ref §15.2].

use crate::cmd::UserCmd;
use crate::math::Vec3;
use crate::state::PlayerState;
use crate::trace::{EntityId, TraceResult};

/// Which part of the command an event happened in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    PreMove,
    Duck,
    Ladder,
    Jump,
    Move,
    PostMove,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    pub origin: Vec3,
    pub velocity: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JumpBranch {
    /// Adds the impulse to existing vertical velocity [Ref §9].
    Standing,
    /// Assigns the impulse (ducking / ducked) [Ref §9, §10.2].
    DuckReset,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JumpEvent {
    pub jumped: bool,
    pub branch: Option<JumpBranch>,
    /// Upward velocity change credited to the jump (stamina cost input) [Ref §8].
    pub impulse: f32,
    pub before: Motion,
    pub after: Motion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuckKind {
    FinishDuck,
    FinishUnduck,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DuckEvent {
    pub kind: DuckKind,
    pub airborne: bool,
    pub before: Motion,
    pub after: Motion,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AccelEvent {
    pub wish_dir: Vec3,
    pub wish_speed: f32,
    pub budget: f32,
    pub surface_friction: f32,
    pub before: Motion,
    pub after: Motion,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BumpEvent {
    pub phase: Phase,
    pub bump: u32,
    pub start: Vec3,
    pub end: Vec3,
    pub trace: TraceResult,
    pub time_left: f32,
    pub velocity_before: Vec3,
    pub velocity_after: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CategorizeEvent {
    pub phase: Phase,
    pub ground_before: Option<EntityId>,
    pub ground_after: Option<EntityId>,
    pub surface_friction: f32,
    pub motion: Motion,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SetGroundEvent {
    pub phase: Phase,
    pub old: Option<EntityId>,
    pub new: Option<EntityId>,
    pub motion: Motion,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LandingEvent {
    pub fall_velocity: f32,
    pub motion: Motion,
}

pub trait MoveObserver {
    fn on_cmd_start(&mut self, _state: &PlayerState, _cmd: &UserCmd) {}
    fn on_duck(&mut self, _ev: &DuckEvent) {}
    fn on_jump_button(&mut self, _ev: &JumpEvent) {}
    fn on_air_accelerate(&mut self, _ev: &AccelEvent) {}
    fn on_walk_move(&mut self, _ev: &AccelEvent) {}
    fn on_try_player_move(&mut self, _ev: &BumpEvent) {}
    fn on_categorize(&mut self, _ev: &CategorizeEvent) {}
    fn on_set_ground(&mut self, _ev: &SetGroundEvent) {}
    fn on_landing(&mut self, _ev: &LandingEvent) {}
    fn on_cmd_end(&mut self, _state: &PlayerState, _cmd: &UserCmd) {}
}

pub struct NullObserver;
impl MoveObserver for NullObserver {}

/// Forward every event to two observers.
pub struct Tee<'a, A: MoveObserver + ?Sized, B: MoveObserver + ?Sized>(pub &'a mut A, pub &'a mut B);

impl<A: MoveObserver + ?Sized, B: MoveObserver + ?Sized> MoveObserver for Tee<'_, A, B> {
    fn on_cmd_start(&mut self, s: &PlayerState, c: &UserCmd) {
        self.0.on_cmd_start(s, c);
        self.1.on_cmd_start(s, c);
    }
    fn on_duck(&mut self, e: &DuckEvent) {
        self.0.on_duck(e);
        self.1.on_duck(e);
    }
    fn on_jump_button(&mut self, e: &JumpEvent) {
        self.0.on_jump_button(e);
        self.1.on_jump_button(e);
    }
    fn on_air_accelerate(&mut self, e: &AccelEvent) {
        self.0.on_air_accelerate(e);
        self.1.on_air_accelerate(e);
    }
    fn on_walk_move(&mut self, e: &AccelEvent) {
        self.0.on_walk_move(e);
        self.1.on_walk_move(e);
    }
    fn on_try_player_move(&mut self, e: &BumpEvent) {
        self.0.on_try_player_move(e);
        self.1.on_try_player_move(e);
    }
    fn on_categorize(&mut self, e: &CategorizeEvent) {
        self.0.on_categorize(e);
        self.1.on_categorize(e);
    }
    fn on_set_ground(&mut self, e: &SetGroundEvent) {
        self.0.on_set_ground(e);
        self.1.on_set_ground(e);
    }
    fn on_landing(&mut self, e: &LandingEvent) {
        self.0.on_landing(e);
        self.1.on_landing(e);
    }
    fn on_cmd_end(&mut self, s: &PlayerState, c: &UserCmd) {
        self.0.on_cmd_end(s, c);
        self.1.on_cmd_end(s, c);
    }
}

/// Techniques detected from internal events of one command [Ref §14, §15].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TechniqueFlags {
    pub jumped: bool,
    pub landed: bool,
    pub jumpbug: bool,
    pub duckbug: bool,
    pub edgebug: bool,
}

/// Per-command technique detector in the style MovementAPI uses: it looks at *where* grounding happened,
/// not only at the start/end state.
#[derive(Clone, Debug, Default)]
pub struct TechniqueDetector {
    /// Flags of the command that just ended.
    pub last: TechniqueFlags,
    cur: TechniqueFlags,
    started_airborne: bool,
    grounded_in_duck: bool,
    walkable_hit_in_move: bool,
    fall_velocity_at_start: f32,
}

impl MoveObserver for TechniqueDetector {
    fn on_cmd_start(&mut self, state: &PlayerState, _cmd: &UserCmd) {
        self.cur = TechniqueFlags::default();
        self.started_airborne = !state.on_ground();
        self.grounded_in_duck = false;
        self.walkable_hit_in_move = false;
        self.fall_velocity_at_start = -state.velocity.z;
    }
    fn on_set_ground(&mut self, ev: &SetGroundEvent) {
        if ev.phase == Phase::Duck && ev.old.is_none() && ev.new.is_some() {
            self.grounded_in_duck = true;
        }
    }
    fn on_jump_button(&mut self, ev: &JumpEvent) {
        if ev.jumped {
            self.cur.jumped = true;
        }
    }
    fn on_try_player_move(&mut self, ev: &BumpEvent) {
        if ev.phase == Phase::Move && ev.trace.fraction < 1.0 && ev.trace.plane_normal.z >= 0.7 {
            self.walkable_hit_in_move = true;
        }
    }
    fn on_landing(&mut self, _ev: &LandingEvent) {
        self.cur.landed = true;
    }
    fn on_cmd_end(&mut self, state: &PlayerState, _cmd: &UserCmd) {
        if self.started_airborne && self.grounded_in_duck && self.fall_velocity_at_start > 0.0 {
            if self.cur.jumped {
                self.cur.jumpbug = true;
            } else if !self.cur.landed {
                self.cur.duckbug = true;
            }
        }
        if self.started_airborne
            && !self.grounded_in_duck
            && self.walkable_hit_in_move
            && !state.on_ground()
            && !self.cur.jumped
        {
            self.cur.edgebug = true;
        }
        self.last = self.cur;
    }
}

/// Records every event as text, for debugging and trace dumps.
#[derive(Clone, Debug, Default)]
pub struct EventLog {
    pub lines: Vec<String>,
}

impl MoveObserver for EventLog {
    fn on_cmd_start(&mut self, s: &PlayerState, c: &UserCmd) {
        self.lines.push(format!("cmd_start tick={} origin={:?} vel={:?} buttons={:?}", c.tick, s.origin, s.velocity, c.buttons));
    }
    fn on_duck(&mut self, e: &DuckEvent) {
        self.lines.push(format!("duck {:?} airborne={} {:?}->{:?}", e.kind, e.airborne, e.before.origin, e.after.origin));
    }
    fn on_jump_button(&mut self, e: &JumpEvent) {
        self.lines.push(format!("jump_button jumped={} branch={:?} impulse={}", e.jumped, e.branch, e.impulse));
    }
    fn on_try_player_move(&mut self, e: &BumpEvent) {
        self.lines.push(format!(
            "bump {:?}#{} frac={} normal={:?} vel {:?}->{:?}",
            e.phase, e.bump, e.trace.fraction, e.trace.plane_normal, e.velocity_before, e.velocity_after
        ));
    }
    fn on_categorize(&mut self, e: &CategorizeEvent) {
        self.lines.push(format!("categorize {:?} {:?}->{:?} sf={}", e.phase, e.ground_before, e.ground_after, e.surface_friction));
    }
    fn on_set_ground(&mut self, e: &SetGroundEvent) {
        self.lines.push(format!("set_ground {:?} {:?}->{:?}", e.phase, e.old, e.new));
    }
    fn on_landing(&mut self, e: &LandingEvent) {
        self.lines.push(format!("landing fall_velocity={}", e.fall_velocity));
    }
    fn on_cmd_end(&mut self, s: &PlayerState, _c: &UserCmd) {
        self.lines.push(format!("cmd_end origin={:?} vel={:?} ground={:?}", s.origin, s.velocity, s.ground_entity));
    }
}

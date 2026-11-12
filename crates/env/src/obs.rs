//! Pluggable observation providers (plan §5). Each provider writes one named, fixed-shape block of `f32`
//! into a caller-owned slice, so a vectorized env can fill one preallocated buffer. Values are in sim
//! units (u, u/s, degrees, ticks); scaling and normalization are left to the learner.

use course::{Course, CourseRun, Status};
use movement::math::angle_vectors;
use movement::{MoveType, PlayerState, Vec3};

use crate::config::EnvConfig;
use crate::env::Track;

#[derive(Clone, Debug, PartialEq)]
pub struct BlockSpec {
    pub name: String,
    pub shape: Vec<usize>,
    /// Names for logging and debugging: one per element for 1-D blocks, one per channel (last axis) for
    /// grids.
    pub fields: Vec<String>,
}

impl BlockSpec {
    pub fn len(&self) -> usize {
        self.shape.iter().product()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn vector(name: &str, fields: &[&str]) -> Self {
        Self { name: name.into(), shape: vec![fields.len()], fields: fields.iter().map(|s| s.to_string()).collect() }
    }
}

/// The layout of the flat observation vector: blocks in order, each at its offset.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObsSpec {
    pub blocks: Vec<(BlockSpec, usize)>,
}

impl ObsSpec {
    pub fn from_blocks(blocks: impl IntoIterator<Item = BlockSpec>) -> Self {
        let mut offset = 0;
        let blocks = blocks
            .into_iter()
            .map(|b| {
                let o = offset;
                offset += b.len();
                (b, o)
            })
            .collect();
        Self { blocks }
    }
    pub fn len(&self) -> usize {
        self.blocks.last().map_or(0, |(b, o)| o + b.len())
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// The slice of `obs` holding block `name`.
    pub fn block<'a>(&self, obs: &'a [f32], name: &str) -> Option<&'a [f32]> {
        self.blocks.iter().find(|(b, _)| b.name == name).map(|(b, o)| &obs[*o..*o + b.len()])
    }
}

/// Everything a provider may read. Built by the env after each step.
pub struct ObsCtx<'a> {
    pub course: &'a Course,
    pub cfg: &'a EnvConfig,
    pub state: &'a PlayerState,
    pub run: &'a CourseRun,
    /// (pitch, yaw, roll) in degrees, as sent in the last command.
    pub view_angles: Vec3,
    pub track: &'a Track,
}

impl ObsCtx<'_> {
    /// Horizontal view frame: forward and right at the current yaw with zero pitch and roll, and world up.
    pub fn yaw_frame(&self) -> (Vec3, Vec3, Vec3) {
        let b = angle_vectors(Vec3::new(0.0, self.view_angles.y, 0.0));
        (b.forward, b.right, Vec3::UP)
    }
}

pub trait ObsProvider: Send + Sync {
    fn spec(&self) -> BlockSpec;
    /// Write exactly `spec().len()` values.
    fn write(&self, ctx: &ObsCtx, out: &mut [f32]);
}

/// Plan §5.1: the player's own body state, previous action, and timer.
#[derive(Clone, Copy, Debug, Default)]
pub struct Proprioception;

impl Proprioception {
    const FIELDS: [&'static str; 21] = [
        "vel_forward",
        "vel_right",
        "vel_up",
        "speed_h",
        "grounded",
        "on_ladder",
        "ducked",
        "duck_amount",
        "stamina",
        // -1 before the first takeoff of the episode.
        "ticks_since_takeoff",
        "last_takeoff_speed",
        "pitch",
        // Applied (clamped) yaw change of the last tick.
        "yaw_rate",
        "prev_forward",
        "prev_side",
        "prev_jump",
        "prev_duck",
        "prev_use",
        "prev_pitch_delta",
        "timer_running",
        "elapsed_ticks",
    ];
}

fn flag(b: bool) -> f32 {
    if b {
        1.0
    } else {
        0.0
    }
}

impl ObsProvider for Proprioception {
    fn spec(&self) -> BlockSpec {
        BlockSpec::vector("proprioception", &Self::FIELDS)
    }

    fn write(&self, ctx: &ObsCtx, out: &mut [f32]) {
        let s = ctx.state;
        let t = ctx.track;
        let (f, r, u) = ctx.yaw_frame();
        let a = t.prev_action;
        let v = [
            s.velocity.dot(f),
            s.velocity.dot(r),
            s.velocity.dot(u),
            s.horizontal_speed(),
            flag(s.on_ground()),
            flag(s.move_type == MoveType::Ladder),
            flag(s.ducked),
            s.duck_amount,
            s.stamina,
            t.ticks_since_takeoff.map_or(-1.0, |n| n as f32),
            t.last_takeoff_speed,
            ctx.view_angles.x,
            t.last_yaw_delta,
            a.forward.signum() as f32,
            a.side.signum() as f32,
            flag(a.jump),
            flag(a.duck),
            flag(a.use_),
            t.last_pitch_delta,
            flag(ctx.run.status == Status::Running),
            ctx.run.elapsed_ticks as f32,
        ];
        out.copy_from_slice(&v);
    }
}

/// Plan §5.2: the end target relative to the player, in the horizontal view frame. Waypoints join this
/// block when courses carry them (R6).
#[derive(Clone, Copy, Debug, Default)]
pub struct GoalVector;

impl ObsProvider for GoalVector {
    fn spec(&self) -> BlockSpec {
        BlockSpec::vector("goal", &["goal_forward", "goal_right", "goal_up", "goal_dist_h"])
    }

    fn write(&self, ctx: &ObsCtx, out: &mut [f32]) {
        let Some(target) = ctx.course.end.target() else {
            out.fill(0.0);
            return;
        };
        let d = target.sub(ctx.state.origin);
        let (f, r, u) = ctx.yaw_frame();
        out.copy_from_slice(&[d.dot(f), d.dot(r), d.dot(u), d.length_2d()]);
    }
}

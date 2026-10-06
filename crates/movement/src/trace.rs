//! Collision interface. Movement code only sees the world through `TraceWorld` [plan §5.4].

use crate::math::Vec3;

/// Source's `DIST_EPSILON`: traces stop this far short of a surface.
pub const DIST_EPSILON: f32 = 0.031_25;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EntityId(pub u32);

impl EntityId {
    pub const WORLD: EntityId = EntityId(0);
}

/// Axis-aligned box relative to the origin [Ref §1].
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Hull {
    pub mins: Vec3,
    pub maxs: Vec3,
}

impl Hull {
    pub const STAND: Hull = Hull { mins: Vec3::new(-16.0, -16.0, 0.0), maxs: Vec3::new(16.0, 16.0, 72.0) };
    pub const DUCK: Hull = Hull { mins: Vec3::new(-16.0, -16.0, 0.0), maxs: Vec3::new(16.0, 16.0, 54.0) };

    pub const fn new(mins: Vec3, maxs: Vec3) -> Self {
        Self { mins, maxs }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraceResult {
    /// 1.0 = no hit.
    pub fraction: f32,
    pub end_pos: Vec3,
    pub plane_normal: Vec3,
    pub start_solid: bool,
    pub all_solid: bool,
    pub hit_entity: Option<EntityId>,
}

impl TraceResult {
    pub fn empty(end: Vec3) -> Self {
        Self {
            fraction: 1.0,
            end_pos: end,
            plane_normal: Vec3::ZERO,
            start_solid: false,
            all_solid: false,
            hit_entity: None,
        }
    }
}

pub trait TraceWorld {
    /// Sweep `hull` from `start` to `end` against player-solid geometry.
    fn trace_hull(&self, start: Vec3, end: Vec3, hull: Hull) -> TraceResult;

    /// Sweep against ladder volumes only (Source's ladder mask) [Ref §18].
    fn trace_ladder(&self, _start: Vec3, end: Vec3, _hull: Hull) -> TraceResult {
        TraceResult::empty(end)
    }

    /// True if the hull at `p` overlaps solid geometry. Used for stuck checks.
    fn point_solid(&self, p: Vec3, hull: Hull) -> bool {
        self.trace_hull(p, p, hull).start_solid
    }

    /// Ground-surface friction factor of an entity, already scaled as `CategorizeGroundSurface` does.
    fn surface_friction_at(&self, _e: EntityId) -> f32 {
        1.0
    }
}

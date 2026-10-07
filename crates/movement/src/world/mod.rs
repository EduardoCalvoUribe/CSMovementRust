//! Collision worlds implementing `TraceWorld`.

pub mod bsp;
pub mod primitive;

pub use bsp::{BspMap, BspWorld};
pub use primitive::{Brush, Contents, Plane, PrimitiveWorld, RiseDir, Shape};

use crate::math::Vec3;
use crate::trace::{EntityId, Hull, TraceResult, TraceWorld};

/// Either collision backend, so a host can swap maps at runtime (plan §5.4: primitive brushes and BSP
/// are interchangeable behind `TraceWorld`).
#[derive(Clone, Debug)]
pub enum World {
    Primitive(PrimitiveWorld),
    Bsp(Box<BspWorld>),
}

impl From<PrimitiveWorld> for World {
    fn from(w: PrimitiveWorld) -> Self {
        World::Primitive(w)
    }
}

impl From<BspWorld> for World {
    fn from(w: BspWorld) -> Self {
        World::Bsp(Box::new(w))
    }
}

impl TraceWorld for World {
    fn trace_hull(&self, start: Vec3, end: Vec3, hull: Hull) -> TraceResult {
        match self {
            World::Primitive(w) => w.trace_hull(start, end, hull),
            World::Bsp(w) => w.trace_hull(start, end, hull),
        }
    }

    fn trace_ladder(&self, start: Vec3, end: Vec3, hull: Hull) -> TraceResult {
        match self {
            World::Primitive(w) => w.trace_ladder(start, end, hull),
            World::Bsp(w) => w.trace_ladder(start, end, hull),
        }
    }

    fn point_solid(&self, p: Vec3, hull: Hull) -> bool {
        match self {
            World::Primitive(w) => w.point_solid(p, hull),
            World::Bsp(w) => w.point_solid(p, hull),
        }
    }

    fn surface_friction_at(&self, e: EntityId) -> f32 {
        match self {
            World::Primitive(w) => w.surface_friction_at(e),
            World::Bsp(w) => w.surface_friction_at(e),
        }
    }
}

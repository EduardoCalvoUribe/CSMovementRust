//! The course description every environment runs on. The toy generator and the BSP importer both produce
//! this struct, so nothing downstream depends on where a course came from.

use movement::trace::EntityId;
use movement::world::World;
use movement::{PlayerState, Vec3};

use crate::zone::Zone;

/// Eye height above the origin, standing and fully ducked, interpolated by `duck_amount`. Same values as
/// the app's camera (`crates/app/src/camera.rs`), which marks them as still to confirm against captures.
pub const EYE_STAND: f32 = 64.0;
pub const EYE_DUCK: f32 = 46.0;

/// Reach of `+use` for an end button. **Placeholder** (docs/reinforcement-plan.md §10.2) until confirmed
/// against the real game.
pub const USE_RANGE: f32 = 64.0;

/// The eye position of a player state.
pub fn eye(state: &PlayerState) -> Vec3 {
    let h = EYE_STAND - (EYE_STAND - EYE_DUCK) * state.duck_amount;
    Vec3::new(state.origin.x, state.origin.y, state.origin.z + h)
}

/// How a run ends successfully.
#[derive(Clone, Debug)]
pub enum EndSpec {
    /// The player's hull overlaps the volume.
    Touch(Zone),
    /// A fresh `+use` press whose view trace (from the eye, up to `USE_RANGE`) hits `entity`, the
    /// button's solid brush entity. `target` is the point goal observations aim at (the button's center).
    ///
    /// Placeholder rules, to confirm against GOKZ/Source (plan §3): Source finds use targets with a more
    /// forgiving search than one straight trace, and the check here runs on the state after the tick's
    /// movement.
    Use { entity: EntityId, target: Vec3 },
}

impl EndSpec {
    /// The point a goal-relative observation aims at.
    pub fn target(&self) -> Option<Vec3> {
        match self {
            EndSpec::Touch(z) => z.center(),
            EndSpec::Use { target, .. } => Some(*target),
        }
    }
}

/// A point along the route, reached when the origin is within `radius` horizontally and between 8 units
/// below and `WAYPOINT_ABOVE` above it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Waypoint {
    pub pos: Vec3,
    pub radius: f32,
}

/// How far above a waypoint the origin may be and still reach it (a standing hull's height).
pub const WAYPOINT_ABOVE: f32 = 72.0;

impl Waypoint {
    pub fn reached_by(&self, origin: Vec3) -> bool {
        let d = origin.sub(self.pos);
        d.length_2d() <= self.radius && d.z >= -8.0 && d.z <= WAYPOINT_ABOVE
    }
}

#[derive(Clone, Debug, Default)]
pub struct CourseMeta {
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Course {
    pub world: World,
    /// Spawn origin and yaw in degrees.
    pub spawn: (Vec3, f32),
    pub start_zone: Zone,
    pub end: EndSpec,
    /// Volumes that end the run as a failure (`trigger_hurt` equivalents).
    pub hazards: Vec<Zone>,
    /// Origin heights below this end the run as out of bounds.
    pub kill_z: f32,
    /// Ordered points along the route, authored by the toy generator; empty for imported maps. They are
    /// reached in order (`CourseRun::next_waypoint`) and feed observations and shaping rewards, never the
    /// run's outcome.
    pub waypoints: Vec<Waypoint>,
    pub meta: CourseMeta,
}

impl Course {
    /// Route length left from `origin` when `next` waypoints have been reached: straight to the next
    /// waypoint, then along the remaining waypoints, then to the end target. 0 once every waypoint is
    /// reached and the course has no end target. A potential for shaping rewards (plan §6); straight-line
    /// distance alone misleads on KZ routes.
    pub fn remaining_route(&self, origin: Vec3, next: usize) -> f32 {
        let mut points = self.waypoints.iter().skip(next).map(|w| w.pos).chain(self.end.target());
        let Some(first) = points.next() else { return 0.0 };
        let mut total = first.sub(origin).length();
        let mut prev = first;
        for p in points {
            total += p.sub(prev).length();
            prev = p;
        }
        total
    }
}

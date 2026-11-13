//! The course description every environment runs on. The toy generator and the BSP importer both produce
//! this struct, so nothing downstream depends on where a course came from.

use movement::trace::EntityId;
use movement::world::World;
use movement::{PlayerState, Vec3};

use crate::zone::Zone;

/// Eye height above the origin, standing and fully ducked, interpolated by `duck_amount`. The end values
/// match the server's eye position in the `+use` probe captures (docs/use-rules.md); the interpolation in
/// between is not measured.
pub const EYE_STAND: f32 = 64.0;
pub const EYE_DUCK: f32 = 46.0;

/// Reach of `+use`, measured on CS:GO 1.38.8.1 (docs/use-rules.md): a button is in reach when the
/// largest per-axis gap between the eye and the button's bounding box is below this (a box test, not a
/// sphere). The measured cutoff lies in (82.029, 82.078].
pub const USE_RANGE: f32 = 82.05;

/// The eye position of a player state.
pub fn eye(state: &PlayerState) -> Vec3 {
    let h = EYE_STAND - (EYE_STAND - EYE_DUCK) * state.duck_amount;
    Vec3::new(state.origin.x, state.origin.y, state.origin.z + h)
}

/// How a run's timer starts.
#[derive(Clone, Debug)]
pub enum StartSpec {
    /// On the first command that ends with the hull outside the zone; re-entering it resets the run.
    Zone(Zone),
    /// On a `+use` press of `entity` (a KZ map's `climb_startbutton`), under GOKZ's start rules; pressing
    /// it again restarts the run. See `CourseRun` and docs/use-rules.md.
    Button { entity: EntityId },
}

/// How a run ends successfully.
#[derive(Clone, Debug)]
pub enum EndSpec {
    /// The player's hull overlaps the volume.
    Touch(Zone),
    /// A `+use` press of `entity`, the button's solid brush entity (`use_hits` has the measured rules).
    /// `target` is the point goal observations aim at (the button's center).
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

/// A map teleport: a `trigger_teleport` volume and its `info_teleport_destination`.
#[derive(Clone, Debug)]
pub struct Teleport {
    pub zone: Zone,
    /// Where the origin is put (the destination's origin; the player's mins.z is 0).
    pub origin: Vec3,
    /// The view angles the player is given (the destination's angles), or `None` to keep the view.
    pub angles: Option<Vec3>,
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
    pub start: StartSpec,
    pub end: EndSpec,
    /// Volumes that end the run as a failure (`trigger_hurt` equivalents).
    pub hazards: Vec<Zone>,
    /// Origin heights below this end the run as out of bounds.
    pub kill_z: f32,
    /// Ordered points along the route, authored by the toy generator; empty for imported maps. They are
    /// reached in order (`CourseRun::next_waypoint`) and feed observations and shaping rewards, never the
    /// run's outcome.
    pub waypoints: Vec<Waypoint>,
    /// Map teleports, applied after each command's movement (`touch_teleports`).
    pub teleports: Vec<Teleport>,
    pub meta: CourseMeta,
}

impl Course {
    /// Apply the first teleport whose volume the player's hull overlaps after a command's movement, as
    /// the engine's trigger touch does: the origin moves to the destination and the player leaves the
    /// ground; velocity is kept (Source SDK 2013 `CTriggerTeleport::Touch` passes no velocity). The
    /// caller sets the view to the returned teleport's `angles`. Rules measured on the real server:
    /// docs/teleport-rules.md.
    pub fn touch_teleports(&self, state: &mut PlayerState) -> Option<&Teleport> {
        let hull = state.hull();
        let t = self.teleports.iter().find(|t| t.zone.overlaps(state.origin, hull))?;
        state.origin = t.origin;
        state.ground_entity = None;
        state.game_moved = true;
        Some(t)
    }

    /// The entities `+use` can press on this course: the start and end buttons.
    pub fn buttons(&self) -> impl Iterator<Item = EntityId> + '_ {
        let start = match &self.start {
            StartSpec::Button { entity } => Some(*entity),
            StartSpec::Zone(_) => None,
        };
        let end = match &self.end {
            EndSpec::Use { entity, .. } => Some(*entity),
            EndSpec::Touch(_) => None,
        };
        start.into_iter().chain(end)
    }

    /// Bounding box of an entity's brushes, if it has any.
    pub fn entity_bounds(&self, e: EntityId) -> Option<(Vec3, Vec3)> {
        let grow = |acc: Option<(Vec3, Vec3)>, (lo, hi): (Vec3, Vec3)| {
            Some(match acc {
                None => (lo, hi),
                Some((a, b)) => (
                    Vec3::new(a.x.min(lo.x), a.y.min(lo.y), a.z.min(lo.z)),
                    Vec3::new(b.x.max(hi.x), b.y.max(hi.y), b.z.max(hi.z)),
                ),
            })
        };
        match &self.world {
            World::Primitive(w) => w.brushes.iter().filter(|b| b.entity == e).map(|b| (b.mins, b.maxs)).fold(None, grow),
            World::Bsp(w) => w.entity_brushes(e).map(|b| (b.mins, b.maxs)).fold(None, grow),
        }
    }

    /// The start zone, for courses that start by leaving one.
    pub fn start_zone(&self) -> Option<&Zone> {
        match &self.start {
            StartSpec::Zone(z) => Some(z),
            StartSpec::Button { .. } => None,
        }
    }

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

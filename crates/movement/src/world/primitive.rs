//! Primitive-brush world: convex solids given as bounding planes [plan §5.4].
//!
//! A hull sweep against a brush expands each plane by the hull's support distance (the Minkowski sum)
//! and clips the segment against the expanded plane set, stopping `DIST_EPSILON` short of the surface.
//! Every brush also carries its axial bounding planes ("bevels"), which make the expansion exact for
//! boxes and for wedges extruded along an axis.

use crate::math::Vec3;
use crate::trace::{EntityId, Hull, TraceResult, TraceWorld, DIST_EPSILON};

/// Approach distance below which a sweep that starts in front of a plane is treated as tangent to it.
/// Much smaller than `DIST_EPSILON`; it absorbs dot-product rounding in slides along non-axial planes
/// (see docs/divergences.md).
pub const TANGENT_TOLERANCE: f32 = 1.0e-4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub normal: Vec3,
    pub dist: f32,
}

/// Direction a wedge's slope rises toward.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiseDir {
    PosX,
    NegX,
    PosY,
    NegY,
}

/// The authoring shape a brush was built from. The app renders from this, so render and collision agree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Box { min: Vec3, max: Vec3 },
    /// Occupies the box `min..max`; the slope runs from `min.z` at the low edge to `max.z` at the high edge.
    Wedge { min: Vec3, max: Vec3, rise: RiseDir },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contents {
    Solid,
    Ladder,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Brush {
    pub planes: Vec<Plane>,
    pub mins: Vec3,
    pub maxs: Vec3,
    pub shape: Shape,
    pub contents: Contents,
    pub entity: EntityId,
}

impl Brush {
    pub fn from_shape(shape: Shape) -> Self {
        let (min, max) = match shape {
            Shape::Box { min, max } | Shape::Wedge { min, max, .. } => (min, max),
        };
        let mut planes = vec![
            Plane { normal: Vec3::new(1.0, 0.0, 0.0), dist: max.x },
            Plane { normal: Vec3::new(-1.0, 0.0, 0.0), dist: -min.x },
            Plane { normal: Vec3::new(0.0, 1.0, 0.0), dist: max.y },
            Plane { normal: Vec3::new(0.0, -1.0, 0.0), dist: -min.y },
            Plane { normal: Vec3::new(0.0, 0.0, 1.0), dist: max.z },
            Plane { normal: Vec3::new(0.0, 0.0, -1.0), dist: -min.z },
        ];
        if let Shape::Wedge { rise, .. } = shape {
            let h = max.z - min.z;
            // The slope passes through the low bottom edge and the high top edge.
            let (normal, point) = match rise {
                RiseDir::PosX => (Vec3::new(-h, 0.0, max.x - min.x), Vec3::new(min.x, 0.0, min.z)),
                RiseDir::NegX => (Vec3::new(h, 0.0, max.x - min.x), Vec3::new(max.x, 0.0, min.z)),
                RiseDir::PosY => (Vec3::new(0.0, -h, max.y - min.y), Vec3::new(0.0, min.y, min.z)),
                RiseDir::NegY => (Vec3::new(0.0, h, max.y - min.y), Vec3::new(0.0, max.y, min.z)),
            };
            let normal = normal.normalized();
            planes.push(Plane { normal, dist: normal.dot(point) });
        }
        Self { planes, mins: min, maxs: max, shape, contents: Contents::Solid, entity: EntityId::WORLD }
    }

    pub fn cuboid(min: Vec3, max: Vec3) -> Self {
        Self::from_shape(Shape::Box { min, max })
    }

    /// A wedge whose slope rises at `angle_deg` from the low edge over `run` units.
    pub fn ramp(low_corner: Vec3, run: f32, width: f32, angle_deg: f32, rise: RiseDir) -> Self {
        let h = run * crate::math::deg2rad(angle_deg).tan();
        let (sx, sy) = match rise {
            RiseDir::PosX | RiseDir::NegX => (run, width),
            RiseDir::PosY | RiseDir::NegY => (width, run),
        };
        let min = low_corner;
        let max = Vec3::new(min.x + sx, min.y + sy, min.z + h);
        Self::from_shape(Shape::Wedge { min, max, rise })
    }

    pub fn with_contents(mut self, c: Contents) -> Self {
        self.contents = c;
        self
    }

    /// Clip a hull sweep against this brush, tightening `tr` if this brush is hit earlier.
    fn clip_hull(&self, start: Vec3, end: Vec3, hull: Hull, tr: &mut TraceResult) {
        let mut enter_frac = -1.0f32;
        let mut leave_frac = 1.0f32;
        let mut clip_plane: Option<Vec3> = None;
        let mut get_out = false;
        let mut start_out = false;
        let delta = end.sub(start);

        for p in &self.planes {
            // Point of the hull that reaches furthest against the plane normal.
            let ofs = Vec3::new(
                if p.normal.x < 0.0 { hull.maxs.x } else { hull.mins.x },
                if p.normal.y < 0.0 { hull.maxs.y } else { hull.mins.y },
                if p.normal.z < 0.0 { hull.maxs.z } else { hull.mins.z },
            );
            let dist = p.dist - ofs.dot(p.normal);
            let d1 = start.dot(p.normal) - dist;
            // From d1 plus the projected motion, so a move tangent to the plane keeps d2 == d1 exactly
            // instead of picking up rounding noise (see docs/divergences.md).
            let d2 = d1 + delta.dot(p.normal);

            if d2 > 0.0 {
                get_out = true;
            }
            if d1 > 0.0 {
                start_out = true;
            }
            // Entirely in front of this plane: no contact with the brush. A move that stays inside the
            // epsilon gap and approaches by less than TANGENT_TOLERANCE counts as tangent.
            if d1 > 0.0 && (d2 >= DIST_EPSILON || d2 >= d1 - TANGENT_TOLERANCE) {
                return;
            }
            // Entirely behind this plane: this plane doesn't limit the sweep.
            if d1 <= 0.0 && d2 <= 0.0 {
                continue;
            }
            if d1 > d2 {
                let f = ((d1 - DIST_EPSILON) / (d1 - d2)).max(0.0);
                if f > enter_frac {
                    enter_frac = f;
                    clip_plane = Some(p.normal);
                }
            } else {
                let f = ((d1 + DIST_EPSILON) / (d1 - d2)).min(1.0);
                if f < leave_frac {
                    leave_frac = f;
                }
            }
        }

        if !start_out {
            tr.start_solid = true;
            if !get_out {
                tr.all_solid = true;
                tr.fraction = 0.0;
                tr.hit_entity = Some(self.entity);
            }
            return;
        }
        if enter_frac < leave_frac && enter_frac > -1.0 && enter_frac < tr.fraction {
            tr.fraction = enter_frac.max(0.0);
            tr.plane_normal = clip_plane.unwrap_or(Vec3::ZERO);
            tr.hit_entity = Some(self.entity);
        }
    }

    fn may_touch(&self, lo: Vec3, hi: Vec3) -> bool {
        lo.x <= self.maxs.x
            && hi.x >= self.mins.x
            && lo.y <= self.maxs.y
            && hi.y >= self.mins.y
            && lo.z <= self.maxs.z
            && hi.z >= self.mins.z
    }
}

#[derive(Clone, Debug, Default)]
pub struct PrimitiveWorld {
    pub brushes: Vec<Brush>,
}

impl PrimitiveWorld {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, b: Brush) -> &mut Self {
        self.brushes.push(b);
        self
    }

    /// A single flat floor whose top face is at `z`, large enough to act as infinite.
    pub fn flat_floor(z: f32) -> Self {
        let mut w = Self::new();
        w.add(Brush::cuboid(Vec3::new(-65536.0, -65536.0, z - 64.0), Vec3::new(65536.0, 65536.0, z)));
        w
    }

    fn trace(&self, start: Vec3, end: Vec3, hull: Hull, contents: Contents) -> TraceResult {
        let mut tr = TraceResult::empty(end);
        let margin = 1.0;
        let lo = Vec3::new(
            start.x.min(end.x) + hull.mins.x - margin,
            start.y.min(end.y) + hull.mins.y - margin,
            start.z.min(end.z) + hull.mins.z - margin,
        );
        let hi = Vec3::new(
            start.x.max(end.x) + hull.maxs.x + margin,
            start.y.max(end.y) + hull.maxs.y + margin,
            start.z.max(end.z) + hull.maxs.z + margin,
        );
        for b in &self.brushes {
            if b.contents != contents || !b.may_touch(lo, hi) {
                continue;
            }
            b.clip_hull(start, end, hull, &mut tr);
            if tr.all_solid {
                break;
            }
        }
        if tr.fraction == 1.0 {
            tr.end_pos = end;
        } else {
            tr.end_pos = Vec3::new(
                start.x + tr.fraction * (end.x - start.x),
                start.y + tr.fraction * (end.y - start.y),
                start.z + tr.fraction * (end.z - start.z),
            );
        }
        tr
    }
}

impl TraceWorld for PrimitiveWorld {
    fn trace_hull(&self, start: Vec3, end: Vec3, hull: Hull) -> TraceResult {
        self.trace(start, end, hull, Contents::Solid)
    }

    fn trace_ladder(&self, start: Vec3, end: Vec3, hull: Hull) -> TraceResult {
        self.trace(start, end, hull, Contents::Ladder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_onto_floor_and_stops_dist_epsilon_short() {
        let w = PrimitiveWorld::flat_floor(0.0);
        let tr = w.trace_hull(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, -10.0), Hull::STAND);
        assert!(tr.fraction < 1.0);
        assert_eq!(tr.plane_normal, Vec3::new(0.0, 0.0, 1.0));
        assert!((tr.end_pos.z - DIST_EPSILON).abs() < 1e-4, "{}", tr.end_pos.z);
    }

    #[test]
    fn resting_on_floor_is_not_solid() {
        let w = PrimitiveWorld::flat_floor(0.0);
        assert!(!w.point_solid(Vec3::new(0.0, 0.0, DIST_EPSILON), Hull::STAND));
        // Exact contact counts as inside, as in brush tracing.
        assert!(w.point_solid(Vec3::new(0.0, 0.0, 0.0), Hull::STAND));
        assert!(w.point_solid(Vec3::new(0.0, 0.0, -1.0), Hull::STAND));
    }

    #[test]
    fn ramp_normal_matches_angle() {
        let b = Brush::ramp(Vec3::ZERO, 100.0, 64.0, 30.0, RiseDir::PosX);
        let slope = b.planes.last().unwrap().normal;
        assert!((slope.z - crate::math::deg2rad(30.0).cos()).abs() < 1e-5);
        assert!(slope.x < 0.0);
    }
}

//! Primitive-brush world: convex solids given as bounding planes [plan ?5.4].
//!
//! A hull sweep against a brush expands each plane by the hull's support distance (the Minkowski sum)
//! and clips the segment against the expanded plane set, stopping `DIST_EPSILON` short of the surface.
//! Every brush also carries its axial bounding planes ("bevels"), which make the expansion exact for
//! boxes and for wedges extruded along an axis.

use crate::math::Vec3;
use crate::trace::{EntityId, Hull, TraceResult, TraceWorld, DIST_EPSILON};



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

/// An upward-facing plane through three points, built the way the BSP compiler builds brush planes
/// (vbsp `PlaneFromPoints` on 32-bit x87 builds): cross product of the edges, length rounded to f32, an
/// f32 reciprocal, each component multiplied and rounded, and the distance from the rounded normal.
/// This is map compilation, not simulation; it reproduces every slope plane of the compiled test map
/// bit for bit, which plain f32 or f64 arithmetic does not (docs/divergences.md D15).
fn plane_from_points(p0: [f32; 3], p1: [f32; 3], p2: [f32; 3]) -> Plane {
    let cross = |p0: [f32; 3], p1: [f32; 3], p2: [f32; 3]| {
        let u = [p0[0] as f64 - p1[0] as f64, p0[1] as f64 - p1[1] as f64, p0[2] as f64 - p1[2] as f64];
        let v = [p2[0] as f64 - p1[0] as f64, p2[1] as f64 - p1[1] as f64, p2[2] as f64 - p1[2] as f64];
        [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
    };
    // The map writer orders the points so the normal faces out (up for a slope); the first point then
    // anchors the distance.
    let (a, c) = {
        let c = cross(p0, p1, p2);
        if c[2] < 0.0 {
            (p2, cross(p2, p1, p0))
        } else {
            (p0, c)
        }
    };
    let length = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt() as f32;
    let inv = (1.0 / length as f64) as f32;
    let n = [(c[0] * inv as f64) as f32, (c[1] * inv as f64) as f32, (c[2] * inv as f64) as f32];
    let dist = a[0] as f64 * n[0] as f64 + a[1] as f64 * n[1] as f64 + a[2] as f64 * n[2] as f64;
    Plane { normal: Vec3::new(n[0], n[1], n[2]), dist: dist as f32 }
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
            // The slope passes through the low bottom edge and the high top edge.
            let (low, along, high) = match rise {
                RiseDir::PosX => ([min.x, min.y, min.z], [min.x, max.y, min.z], [max.x, max.y, max.z]),
                RiseDir::NegX => ([max.x, min.y, min.z], [max.x, max.y, min.z], [min.x, max.y, max.z]),
                RiseDir::PosY => ([min.x, min.y, min.z], [max.x, min.y, min.z], [max.x, max.y, max.z]),
                RiseDir::NegY => ([min.x, max.y, min.z], [max.x, max.y, min.z], [max.x, min.y, max.z]),
            };
            planes.push(plane_from_points(low, along, high));
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

    /// Clip a box sweep against this brush, tightening `tr` if this brush is hit earlier. The box is
    /// given as its centre `start`, half-size `extents`, and motion `delta` (Source's `Ray_t`).
    fn clip_box(&self, start: Vec3, delta: Vec3, extents: Vec3, tr: &mut TraceResult) {
        let mut enter_frac = -1.0f32;
        let mut leave_frac = 1.0f32;
        let mut clip_plane: Option<Vec3> = None;
        let mut get_out = false;
        let mut start_out = false;

        for p in &self.planes {
            // Corner of the box that reaches furthest against the plane normal.
            let ofs = Vec3::new(
                if p.normal.x < 0.0 { extents.x } else { -extents.x },
                if p.normal.y < 0.0 { extents.y } else { -extents.y },
                if p.normal.z < 0.0 { extents.z } else { -extents.z },
            );
            let dist = p.dist - ofs.dot(p.normal);
            let d1 = start.dot(p.normal) - dist;
            // From the end point, as Source does; deriving it from d1 plus the projected motion puts slope
            // contacts a few ULP off (measured, docs/divergences.md D1).
            let d2 = start.add(delta).dot(p.normal) - dist;

            if d2 > 0.0 {
                get_out = true;
            }
            if d1 > 0.0 {
                start_out = true;
            }
            // Both ends in front of this plane: no contact with the brush. The epsilon only enters the
            // fraction, so a sweep may end inside the 1/32 gap (measured, docs/divergences.md D2).
            if d1 > 0.0 && d2 > 0.0 {
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

    /// Source traces a hull as a box centred on `origin + (mins + maxs) / 2` and converts the end point
    /// back by subtracting that offset (`Ray_t::Init`, public SDK `cmodel.h`). The arithmetic happens at
    /// the centre's magnitude, so its rounding is part of the behavior: a resting player at
    /// z = 0.03124994 comes back from any trace at exactly 0.03125 (see docs/divergences.md).
    fn trace(&self, start: Vec3, end: Vec3, hull: Hull, contents: Contents) -> TraceResult {
        let mut tr = TraceResult::empty(end);
        let delta = end.sub(start);
        let extents = hull.maxs.sub(hull.mins).scale(0.5);
        let start_offset = hull.mins.add(hull.maxs).scale(0.5);
        let ray_start = start.add(start_offset);
        let start_offset = start_offset.scale(-1.0);
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
        let mut hit_box = false;
        for b in &self.brushes {
            // Ladder brushes block the player as well as marking the ladder (CS:GO's invisible-ladder
            // brushes are player-solid; measured, docs/divergences.md D9).
            let wanted = b.contents == contents || (contents == Contents::Solid && b.contents == Contents::Ladder);
            if !wanted || !b.may_touch(lo, hi) {
                continue;
            }
            let before = tr.fraction;
            b.clip_box(ray_start, delta, extents, &mut tr);
            if tr.fraction < before {
                hit_box = matches!(b.shape, Shape::Box { .. });
            }
            if tr.all_solid {
                break;
            }
        }
        // VectorMA(m_Start, fraction, m_Delta), then back from the box centre to the origin. Which
        // arithmetic the engine uses depends on what ended the sweep: a stop on an axis-aligned box brush
        // (BSP "box brushes", traced by their own routine) rounds in f32; a stop on a general brush, or no
        // stop at all, keeps the sum in extended precision (32-bit x87) and rounds once. Both measured bit
        // for bit (docs/divergences.md D17). The extended branch is the one place collision arithmetic is
        // wider than f32.
        let frac = tr.fraction;
        let end = |c: f32, d: f32, o: f32| {
            if hit_box && frac < 1.0 {
                (c + frac * d) + o
            } else {
                ((c as f64 + frac as f64 * d as f64) + o as f64) as f32
            }
        };
        tr.end_pos = Vec3::new(
            end(ray_start.x, delta.x, start_offset.x),
            end(ray_start.y, delta.y, start_offset.y),
            end(ray_start.z, delta.z, start_offset.z),
        );
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

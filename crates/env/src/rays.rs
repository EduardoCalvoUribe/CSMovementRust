//! Raycast "screen" observation (plan §5.3, R4): a grid of rays from the eye, each reporting how far it
//! got and what kind of surface stopped it. Rays are zero-size hull traces through the course's own
//! `TraceWorld`, so they see exactly the geometry the sim collides with, on any map. No pixels.

use course::Course;
use movement::math::angle_vectors;
use movement::{Hull, PlayerState, TraceWorld, Vec3};

use crate::obs::{BlockSpec, ObsCtx, ObsProvider};


/// Ground the player can stand on: plane normal z at least this (Source's 0.7) [Ref §6].
pub const WALKABLE_NORMAL_Z: f32 = 0.7;
/// Normals with |z| below this count as vertical walls.
pub const WALL_NORMAL_Z: f32 = 0.01;

/// What stopped a ray.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SurfaceClass {
    /// Nothing within range.
    None = 0,
    /// Walkable (normal z >= 0.7).
    Floor = 1,
    /// Upward-facing but too steep to stand on (surf/slide).
    Steep = 2,
    /// Vertical. Also used for a ray whose start is inside solid.
    Wall = 3,
    /// Downward-facing.
    Ceiling = 4,
    Ladder = 5,
    /// The end volume (seen through; it is not solid), or the end button's brush.
    Goal = 6,
    /// A hazard volume.
    Hazard = 7,
}

impl SurfaceClass {
    pub const COUNT: usize = 8;
    pub const NAMES: [&'static str; Self::COUNT] =
        ["none", "floor", "steep", "wall", "ceiling", "ladder", "goal", "hazard"];

    pub fn from_normal(n: Vec3) -> Self {
        if n.z >= WALKABLE_NORMAL_Z {
            SurfaceClass::Floor
        } else if n.z >= WALL_NORMAL_Z {
            SurfaceClass::Steep
        } else if n.z > -WALL_NORMAL_Z {
            SurfaceClass::Wall
        } else {
            SurfaceClass::Ceiling
        }
    }
}

/// How each ray's class is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassEncoding {
    /// Channels: distance, then one per class (1.0 for the hit class).
    OneHot,
    /// Channels: distance, class index as f32.
    Index,
}

/// One ray's result.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    /// Fraction of `max_distance` travelled, 1.0 when nothing was hit.
    pub distance: f32,
    pub class: SurfaceClass,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayGrid {
    pub rows: usize,
    pub cols: usize,
    /// Total angular width and height of the grid, degrees. A single column or row looks straight ahead.
    pub h_fov: f32,
    pub v_fov: f32,
    pub max_distance: f32,
    /// The grid pitches with the view (what a player sees); otherwise it stays level.
    pub follow_pitch: bool,
    /// One extra ladder trace per hit ray tells ladders from walls.
    pub detect_ladders: bool,
    /// Rays stop at the end volume and at hazards (two extra small traces per ray).
    pub see_zones: bool,
    pub encoding: ClassEncoding,
}

impl Default for RayGrid {
    /// Resolution, field of view, and range are placeholders for the learner to tune; cost scales with
    /// `rows * cols` (plan §7).
    fn default() -> Self {
        Self {
            rows: 8,
            cols: 16,
            h_fov: 120.0,
            v_fov: 60.0,
            max_distance: 2048.0,
            follow_pitch: true,
            detect_ladders: true,
            see_zones: true,
            encoding: ClassEncoding::OneHot,
        }
    }
}

impl RayGrid {
    pub fn channels(&self) -> usize {
        match self.encoding {
            ClassEncoding::OneHot => 1 + SurfaceClass::COUNT,
            ClassEncoding::Index => 2,
        }
    }

    /// The eye position rays start from (`course::eye`: 64 standing, 46 ducked).
    pub fn eye(state: &PlayerState) -> Vec3 {
        course::eye(state)
    }

    /// Unit direction of the ray at `row` (0 = top) and `col` (0 = left), evenly spaced in angle.
    pub fn direction(&self, view_angles: Vec3, row: usize, col: usize) -> Vec3 {
        let frac = |i: usize, n: usize| if n > 1 { i as f32 / (n - 1) as f32 - 0.5 } else { 0.0 };
        let pitch = if self.follow_pitch { view_angles.x } else { 0.0 };
        // Source pitch is positive looking down; yaw grows to the left.
        let p = pitch + self.v_fov * frac(row, self.rows);
        let y = view_angles.y - self.h_fov * frac(col, self.cols);
        angle_vectors(Vec3::new(p, y, 0.0)).forward
    }

    /// All ray directions, row-major.
    pub fn directions(&self, view_angles: Vec3) -> Vec<Vec3> {
        (0..self.rows).flat_map(|r| (0..self.cols).map(move |c| (r, c))).map(|(r, c)| self.direction(view_angles, r, c)).collect()
    }

    /// Cast one ray from `eye` along unit `dir`.
    pub fn cast(&self, course: &Course, eye: Vec3, dir: Vec3) -> RayHit {
        let end = eye.ma(self.max_distance, dir);
        let point = Hull::new(Vec3::ZERO, Vec3::ZERO);
        let tr = course.world.trace_hull(eye, end, point);
        let mut hit = if tr.start_solid {
            RayHit { distance: 0.0, class: SurfaceClass::Wall }
        } else if tr.fraction < 1.0 {
            let button = matches!(course.end, course::EndSpec::Use { entity, .. } if tr.hit_entity == Some(entity));
            let class = if button {
                SurfaceClass::Goal
            } else if self.detect_ladders && course.world.trace_ladder(eye, end, point).fraction <= tr.fraction {
                SurfaceClass::Ladder
            } else {
                SurfaceClass::from_normal(tr.plane_normal)
            };
            RayHit { distance: tr.fraction, class }
        } else {
            RayHit { distance: 1.0, class: SurfaceClass::None }
        };
        if self.see_zones && !tr.start_solid {
            let end = eye.ma(self.max_distance * hit.distance, dir);
            let nearer = |zone: &course::Zone, class: SurfaceClass, hit: &mut RayHit| {
                if let Some(f) = zone.ray_entry(eye, end) {
                    // `f` is along the shortened segment.
                    let d = f * hit.distance;
                    if d < hit.distance {
                        *hit = RayHit { distance: d, class };
                    }
                }
            };
            if let course::EndSpec::Touch(z) = &course.end {
                nearer(z, SurfaceClass::Goal, &mut hit);
            }
            for z in &course.hazards {
                nearer(z, SurfaceClass::Hazard, &mut hit);
            }
        }
        hit
    }

    pub fn cast_all(&self, course: &Course, state: &PlayerState, view_angles: Vec3) -> Vec<RayHit> {
        let eye = Self::eye(state);
        self.directions(view_angles).into_iter().map(|d| self.cast(course, eye, d)).collect()
    }
}

impl ObsProvider for RayGrid {
    fn spec(&self) -> BlockSpec {
        let mut fields = vec!["distance".to_string()];
        match self.encoding {
            ClassEncoding::OneHot => fields.extend(SurfaceClass::NAMES.iter().map(|n| format!("is_{n}"))),
            ClassEncoding::Index => fields.push("class".into()),
        }
        BlockSpec { name: "rays".into(), shape: vec![self.rows, self.cols, self.channels()], fields }
    }

    fn write(&self, ctx: &ObsCtx, out: &mut [f32]) {
        let ch = self.channels();
        let eye = Self::eye(ctx.state);
        for r in 0..self.rows {
            for c in 0..self.cols {
                let hit = self.cast(ctx.course, eye, self.direction(ctx.view_angles, r, c));
                let o = &mut out[(r * self.cols + c) * ch..][..ch];
                o[0] = hit.distance;
                match self.encoding {
                    ClassEncoding::OneHot => {
                        o[1..].fill(0.0);
                        o[1 + hit.class as usize] = 1.0;
                    }
                    ClassEncoding::Index => o[1] = hit.class as u8 as f32,
                }
            }
        }
    }
}

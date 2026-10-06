//! Minimal f32 vector type. Kept dependency-free so operation order stays explicit.
//!
//! Every method spells out its arithmetic in the same order as the Source SDK 2013 mathlib equivalent.
//! No `mul_add`, no reassociation.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// Source's `FLT_EPSILON`, used by `VectorNormalize` to avoid dividing by zero.
pub const FLT_EPSILON: f32 = f32::EPSILON;

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
    pub const UP: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 1.0 };

    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn dot(self, o: Vec3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    pub fn length_2d(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    pub fn scale(self, s: f32) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }

    pub fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }

    pub fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }

    pub fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }

    /// `VectorMA(self, s, dir)`: `self + dir * s`, computed per component.
    pub fn ma(self, s: f32, dir: Vec3) -> Vec3 {
        Vec3::new(self.x + dir.x * s, self.y + dir.y * s, self.z + dir.z * s)
    }

    /// Source `VectorNormalize`: divides by `length + FLT_EPSILON` and returns the original length.
    pub fn normalize_in_place(&mut self) -> f32 {
        let radius = (self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        let iradius = 1.0 / (radius + FLT_EPSILON);
        self.x *= iradius;
        self.y *= iradius;
        self.z *= iradius;
        radius
    }

    pub fn normalized(mut self) -> Vec3 {
        self.normalize_in_place();
        self
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    pub fn get(self, i: usize) -> f32 {
        match i {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }

    pub fn set(&mut self, i: usize, v: f32) {
        match i {
            0 => self.x = v,
            1 => self.y = v,
            _ => self.z = v,
        }
    }
}

/// Degrees to radians in f32, the way Source's `DEG2RAD` macro evaluates for float input.
pub fn deg2rad(deg: f32) -> f32 {
    deg * (core::f32::consts::PI / 180.0)
}

/// Basis vectors of a view orientation.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Basis {
    pub forward: Vec3,
    pub right: Vec3,
    pub up: Vec3,
}

/// Source `AngleVectors` for (pitch, yaw, roll) in degrees [Ref §4].
pub fn angle_vectors(angles: Vec3) -> Basis {
    let (sy, cy) = deg2rad(angles.y).sin_cos();
    let (sp, cp) = deg2rad(angles.x).sin_cos();
    let (sr, cr) = deg2rad(angles.z).sin_cos();

    let forward = Vec3::new(cp * cy, cp * sy, -sp);
    let right = Vec3::new(
        -1.0 * sr * sp * cy + -1.0 * cr * -sy,
        -1.0 * sr * sp * sy + -1.0 * cr * cy,
        -1.0 * sr * cp,
    );
    let up = Vec3::new(cr * sp * cy + -sr * -sy, cr * sp * sy + -sr * cy, cr * cp);
    Basis { forward, right, up }
}

/// Move `value` toward `target` by at most `speed` (Source `Approach`).
pub fn approach(target: f32, value: f32, speed: f32) -> f32 {
    let delta = target - value;
    if delta > speed {
        value + speed
    } else if delta < -speed {
        value - speed
    } else {
        target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angle_vectors_yaw_90() {
        let b = angle_vectors(Vec3::new(0.0, 90.0, 0.0));
        assert!((b.forward.y - 1.0).abs() < 1e-6);
        assert!(b.forward.x.abs() < 1e-6);
        // Right is +x when facing +y (forward x up, with z up).
        assert!((b.right.x - 1.0).abs() < 1e-6);
    }

    #[test]
    fn normalize_returns_length() {
        let mut v = Vec3::new(3.0, 4.0, 0.0);
        let l = v.normalize_in_place();
        assert_eq!(l, 5.0);
        assert!((v.length() - 1.0).abs() < 1e-6);
    }
}

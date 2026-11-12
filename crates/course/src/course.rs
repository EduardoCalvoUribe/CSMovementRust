//! The course description every environment runs on. The toy generator and the BSP importer both produce
//! this struct, so nothing downstream depends on where a course came from.

use movement::world::World;
use movement::Vec3;

use crate::zone::Zone;

/// How a run ends successfully.
#[derive(Clone, Debug)]
pub enum EndSpec {
    /// The player's hull overlaps the volume.
    Touch(Zone),
}

impl EndSpec {
    /// The point a goal-relative observation aims at.
    pub fn target(&self) -> Option<Vec3> {
        match self {
            EndSpec::Touch(z) => z.center(),
        }
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
    pub meta: CourseMeta,
}

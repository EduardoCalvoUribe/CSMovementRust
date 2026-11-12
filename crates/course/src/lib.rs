//! Course description, run timer, and toy-course generator for the reinforcement-learning environment
//! (`docs/reinforcement-plan.md` §3, §8). Pure Rust on top of `movement`: no Bevy, no I/O. The only
//! randomness is the toy generator's, from a caller-supplied seed.

pub mod course;
pub mod run;
pub mod toy;
pub mod zone;

pub use course::{eye, Course, CourseMeta, EndSpec, Waypoint, EYE_DUCK, EYE_STAND, USE_RANGE, WAYPOINT_ABOVE};
pub use run::{use_hits, CourseRun, Status};
pub use toy::{EndKind, Stage, ToyCourse};
pub use zone::Zone;

//! Course description and run timer for the reinforcement-learning environment
//! (`docs/reinforcement-plan.md` §3). Pure Rust on top of `movement`: no Bevy, no I/O, no randomness.

pub mod course;
pub mod run;
pub mod zone;

pub use course::{Course, CourseMeta, EndSpec};
pub use run::{CourseRun, Status};
pub use zone::Zone;

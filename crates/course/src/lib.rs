//! Course description, run timer, and toy-course generator for the reinforcement-learning environment
//! (`docs/reinforcement-plan.md` §3, §8). Pure Rust on top of `movement`: no Bevy, no I/O. The only
//! randomness is the toy generator's, from a caller-supplied seed.

pub mod course;
pub mod run;
pub mod toy;
pub mod zone;

pub use course::{Course, CourseMeta, EndSpec};
pub use run::{CourseRun, Status};
pub use toy::{Stage, ToyCourse};
pub use zone::Zone;

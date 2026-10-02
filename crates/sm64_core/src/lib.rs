#![forbid(unsafe_code)]

pub mod action;
pub mod mario;
pub mod math;
pub mod types;

pub use action::*;
pub use mario::MarioState;
pub use math::{Vec3f, Vec3s};
pub use types::{ObjectId, SurfaceId};

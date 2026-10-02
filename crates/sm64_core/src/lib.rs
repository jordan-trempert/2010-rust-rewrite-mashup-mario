#![forbid(unsafe_code)]

pub mod action;
pub mod collision;
pub mod mario;
pub mod mario_step;
pub mod math;
pub mod surface;
mod trig_tables;
pub mod types;

pub use action::*;
pub use mario::MarioState;
pub use math::{Vec3f, Vec3s};
pub use types::{ObjectId, SurfaceId};

pub use collision::{CollisionWorld, SurfaceHit};
pub use surface::{Surface, SurfaceNormal};

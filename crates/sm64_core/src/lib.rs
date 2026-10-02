#![forbid(unsafe_code)]

pub mod action;
pub mod actions;
pub mod automatic;
pub mod collision;
pub mod ground_motion;
pub mod mario;
pub mod mario_action;
pub mod mario_step;
pub mod object;
pub mod math;
pub mod surface;
pub mod submerged;
pub mod surface_types;
pub mod surface_props;
mod trig_tables;
pub mod types;

pub use action::*;
pub use mario::MarioState;
pub use math::{Vec3f, Vec3s};
pub use types::{ObjectId, SurfaceId};

pub use collision::{CollisionWorld, EnvironmentRegion, SurfaceHit};
pub use surface::{Surface, SurfaceNormal};
pub use surface_types::*;
pub use surface_props::*;
pub use ground_motion::*;

pub use object::{ObjectList, Sm64Object};

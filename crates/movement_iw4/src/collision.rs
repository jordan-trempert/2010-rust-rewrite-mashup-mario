use trace_iw4::Trace;

use crate::GroundTraceInput;
use crate::correct_solid::{CorrectSolidOutcome, correct_solid};

pub trait CollisionBackend {
    fn trace(&self, input: GroundTraceInput) -> Trace;

    /// Maximum ledge height ordinary walking may step over. IW4 maps use the
    /// original 18-unit value; imported worlds can opt into their authored
    /// stair height without changing movement on normal multiplayer maps.
    fn step_size(&self) -> f32 {
        18.0
    }

    fn correct_solid(
        &self,
        origin: [f32; 3],
        mins: [f32; 3],
        maxs: [f32; 3],
        tracemask: u32,
    ) -> Option<CorrectSolidOutcome> {
        correct_solid(origin, mins, maxs, tracemask, self)
    }

    fn touch_entity(&self, _entity: i32) {}
}

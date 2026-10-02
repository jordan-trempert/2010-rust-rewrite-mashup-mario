/// Stable simulation handle replacing a C pointer to an SM64 object.
///
/// The original game stores direct pointers in MarioState. The Rust port keeps
/// equivalent relationships by ID so snapshots/clones remain deterministic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ObjectId(pub u32);

/// Stable simulation handle replacing a C pointer to an SM64 collision surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SurfaceId(pub u32);

use bevy::prelude::*;
use sm64_sim::{SM64_TICK_SECONDS, Sm64Input, Sm64Snapshot, Sm64World};

pub struct Sm64Plugin;

#[derive(Resource, Debug, Clone, Copy)]
pub struct Sm64Enabled(pub bool);

impl Default for Sm64Enabled {
    fn default() -> Self { Self(false) }
}

#[derive(Resource, Default)]
pub struct Sm64Runtime {
    pub world: Sm64World,
    accumulator: f64,
    pub latest: Option<Sm64Snapshot>,
}

#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct Sm64ControllerInput(pub Sm64Input);

#[derive(Component, Debug, Clone, Copy)]
pub struct Sm64MarioPresentation;

impl Plugin for Sm64Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Sm64Enabled>()
            .init_resource::<Sm64Runtime>()
            .init_resource::<Sm64ControllerInput>()
            .add_systems(Update, advance_sm64_runtime);
    }
}

fn advance_sm64_runtime(
    time: Res<Time>,
    enabled: Res<Sm64Enabled>,
    input: Res<Sm64ControllerInput>,
    mut runtime: ResMut<Sm64Runtime>,
) {
    if !enabled.0 { return; }
    runtime.accumulator += time.delta_secs_f64();
    // Bound catch-up so pausing/debugging cannot spiral one render frame into
    // thousands of original-game frames.
    let mut steps=0;
    while runtime.accumulator >= SM64_TICK_SECONDS && steps < 8 {
        runtime.accumulator -= SM64_TICK_SECONDS;
        runtime.latest=Some(runtime.world.step(input.0));
        steps += 1;
    }
}

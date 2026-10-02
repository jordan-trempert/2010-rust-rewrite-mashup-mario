use bevy::log::{error, info};
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

#[derive(Resource, Debug, Clone)]
pub struct Sm64LoadStatus {
    pub level: String,
    pub area: u8,
    pub source_root: Option<std::path::PathBuf>,
    pub loaded: bool,
    pub message: String,
}

impl Default for Sm64LoadStatus {
    fn default() -> Self {
        Self {
            level: "bob".to_owned(),
            area: 1,
            source_root: None,
            loaded: false,
            message: "SM64 decomp root not configured".to_owned(),
        }
    }
}

#[derive(Component, Debug, Clone, Copy)]
pub struct Sm64MarioPresentation;

impl Plugin for Sm64Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Sm64Enabled>()
            .init_resource::<Sm64Runtime>()
            .init_resource::<Sm64ControllerInput>()
            .init_resource::<Sm64LoadStatus>()
            .add_systems(Startup, load_sm64_from_decomp)
            .add_systems(Update, advance_sm64_runtime);
    }
}

fn load_sm64_from_decomp(
    mut enabled: ResMut<Sm64Enabled>,
    mut runtime: ResMut<Sm64Runtime>,
    mut status: ResMut<Sm64LoadStatus>,
) {
    let Some(root)=std::env::var_os("SM64_DECOMP_ROOT").map(std::path::PathBuf::from) else {
        return;
    };
    let level=std::env::var("SM64_LEVEL").unwrap_or_else(|_|"bob".to_owned());
    let area=std::env::var("SM64_AREA")
        .ok()
        .and_then(|v|v.parse::<u8>().ok())
        .filter(|v|*v!=0)
        .unwrap_or(1);

    status.source_root=Some(root.clone());
    status.level=level.clone();
    status.area=area;

    let parsed=match sm64_assets::load_level_collision(&root,&level,area) {
        Ok(parsed)=>parsed,
        Err(error)=>{
            status.message=format!("SM64 collision load failed: {error}");
            error!("{}",status.message);
            return;
        }
    };
    let spawn=match sm64_assets::load_mario_spawn(&root,&level,area) {
        Ok(spawn)=>spawn,
        Err(error)=>{
            status.message=format!("SM64 spawn load failed: {error}");
            error!("{}",status.message);
            return;
        }
    };
    let area_settings=match sm64_assets::load_area_settings(&root,&level,area) {
        Ok(settings)=>settings,
        Err(error)=>{
            status.message=format!("SM64 area metadata load failed: {error}");
            error!("{}",status.message);
            return;
        }
    };

    let surface_count=parsed.world.surfaces.len();
    let special_count=parsed.specials.len();
    runtime.world.collision=parsed.world;
    runtime.world.mario.terrain_type=area_settings.terrain_type;
    runtime.world.spawn_mario(
        [spawn.pos[0] as f32,spawn.pos[1] as f32,spawn.pos[2] as f32],
        spawn.yaw_sm64(),
    );
    runtime.latest=Some(runtime.world.snapshot());
    enabled.0=std::env::var("SM64_ENABLED")
        .map(|v|v!="0" && !v.eq_ignore_ascii_case("false"))
        .unwrap_or(true);

    status.loaded=true;
    status.message=format!(
        "SM64 {level} area {area}: {surface_count} surfaces, {special_count} special objects, Mario at {:?}",
        spawn.pos
    );
    info!("{}",status.message);
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

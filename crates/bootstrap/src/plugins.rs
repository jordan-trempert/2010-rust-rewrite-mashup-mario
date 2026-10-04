use assets::AssetPlugin;
use audio::AudioPlugin;
use bevy::{
    log::LogPlugin,
    prelude::*,
    render::{
        RenderPlugin as BevyRenderPlugin,
        pipelined_rendering::PipelinedRenderingPlugin,
        settings::{RenderCreation, WgpuFeatures, WgpuSettings},
    },
};
use bots::BotsPlugin;
use console::ConsolePlugin;
use frame::RuntimeRole;
use hud::HudPlugin;
use net::NetPlugin;
use render::RenderPlugin;
use replay::ReplayPlugin;
use session::SessionPlugin;
use sm64_bevy::Sm64Plugin;
use ui::UiPlugin;

#[derive(Resource, Debug, Clone, Copy)]
struct Sm64CodSpawn {
    origin: [f32; 3],
    view: [f32; 3],
}

pub fn add_runtime_plugins(app: &mut App) {
    add_runtime_plugins_with_role(app, RuntimeRole::Listen);
}

pub fn add_runtime_plugins_with_role(app: &mut App, role: RuntimeRole) {
    let net = match role {
        RuntimeRole::Listen => NetPlugin::listen(),
        RuntimeRole::Dedicated => NetPlugin::dedicated(),
        RuntimeRole::Client => NetPlugin::client(),
        RuntimeRole::Replay => NetPlugin::replay(),
    };
    if crate::bench::enabled() {
        app.insert_resource(bevy::winit::WinitSettings::continuous());
    }
    app.add_plugins(AssetPlugin)
        .add_plugins(UiPlugin)
        .add_plugins(ConsolePlugin)
        .add_plugins(net)
        .add_plugins((BotsPlugin, HudPlugin))
        .add_plugins(AudioPlugin)
        .add_plugins(ReplayPlugin)
        .add_plugins(RenderPlugin)
        .add_plugins(SessionPlugin)
        .add_plugins(Sm64Plugin)
        .add_systems(Update, (
            launch_installed_sm64_cod_map,
            place_sm64_cod_player_on_life_started,
            sync_cod_player_into_sm64,
            suppress_sm64_player_view,
            clear_sm64_cod_on_return,
        ));

    app.edit_schedule(Update, |schedule| {
        schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
    });
    app.edit_schedule(First, |schedule| {
        schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
    });
    app.edit_schedule(PreUpdate, |schedule| {
        schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
    });
    app.edit_schedule(PostUpdate, |schedule| {
        schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
    });
    app.edit_schedule(Last, |schedule| {
        schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
    });

    if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
        render_app.edit_schedule(bevy::render::renderer::RenderGraph, |schedule| {
            schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        });
        render_app.edit_schedule(bevy::core_pipeline::Core3d, |schedule| {
            schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        });
        render_app.edit_schedule(bevy::core_pipeline::Core2d, |schedule| {
            schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        });
        if !(cfg!(target_os = "macos") && pipelined_rendering()) {
            render_app.edit_schedule(bevy::render::Render, |schedule| {
                schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
            });
        }
        render_app.edit_schedule(bevy::render::ExtractSchedule, |schedule| {
            schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        });
    }
}

fn launch_installed_sm64_cod_map(
    mut installed: MessageReader<frame::MatchInstalled>,
    pending: Option<Res<sm64_bevy::Sm64CodMapRequest>>,
    mut authority: Option<ResMut<net::AuthorityWorld>>,
    mut commands: Commands,
) {
    if installed.read().last().is_none() {
        return;
    }
    let Some(pending) = pending else {
        return;
    };
    let level = pending.level.clone();
    let area = pending.area;

    let Some(root)=std::env::var_os("SM64_DECOMP_ROOT").map(std::path::PathBuf::from) else {
        diag::warn!(World, "SM64 COD map: SM64_DECOMP_ROOT is not configured");
        return;
    };

    let parsed=match sm64_assets::load_level_collision(&root,&level,area) {
        Ok(parsed)=>parsed,
        Err(error)=>{
            diag::warn!(World, "SM64 COD map: collision load failed for {level}: {error}");
            return;
        }
    };
    let spawn=match sm64_assets::load_mario_spawn(&root,&level,area) {
        Ok(spawn)=>spawn,
        Err(error)=>{
            diag::warn!(World, "SM64 COD map: spawn load failed for {level}: {error}");
            return;
        }
    };

    if let Some(authority)=authority.as_deref_mut() {
        let mut verts=Vec::with_capacity(parsed.world.surfaces.len()*3);
        for surface in &parsed.world.surfaces {
            // SM64 is Y-up and IW4 is Z-up. The Y/Z swap changes handedness,
            // so reverse v2/v3 as well; otherwise walkable SM64 floors become
            // back-facing triangles and a falling COD capsule passes through.
            for vertex in [surface.vertex1,surface.vertex3,surface.vertex2] {
                verts.push([
                    vertex[0] as f32,
                    vertex[2] as f32,
                    vertex[1] as f32,
                ]);
            }
        }
        let mesh=sim::SimClipMesh::from_linear_triangles(verts);
        let content=authority.0.content().with_clip_mesh(mesh);
        authority.0.install_content(content);

        // with_clip_mesh intentionally removes the donor BSP brushes. The
        // imported SM64 triangle mesh is already complete at this point, so
        // the mp_rust preparation hold must not keep authority/admission
        // frozen waiting on donor collision that is no longer in use.
        commands.insert_resource(net::AuthorityLoadHold(false));

        let floor_y=parsed.world
            .find_floor(spawn.pos[0] as f32, 30_000.0, spawn.pos[2] as f32)
            .map(|hit|hit.height)
            .unwrap_or(spawn.pos[1] as f32);
        // Start well above the detected SM64 floor. This gives COD's
        // capsule several frames to settle onto the imported collision and
        // makes a bad floor/collision transform obvious in the log.
        const SM64_COD_SPAWN_CLEARANCE: f32 = 256.0;
        let spawn_cod=[
            spawn.pos[0] as f32,
            spawn.pos[2] as f32,
            floor_y + SM64_COD_SPAWN_CLEARANCE,
        ];
        let sm64_yaw=spawn.yaw_sm64() as u16 as f32 * 360.0 / 65536.0;
        let view=[0.0,90.0-sm64_yaw,0.0];
        commands.insert_resource(Sm64CodSpawn {
            origin: spawn_cod,
            view,
        });
        diag::info!(
            World,
            "SM64 COD map: installed {} collision triangles; detected floor_y={:.1}; COD spawn armed at {:?} (+{:.0} above floor)",
            parsed.world.surfaces.len(),
            floor_y,
            spawn_cod,
            SM64_COD_SPAWN_CLEARANCE
        );
    } else {
        diag::warn!(World, "SM64 COD map: authority world unavailable while attaching {level}");
    }

    commands.insert_resource(sm64_bevy::Sm64LaunchRequest::new(level.clone(), area));
    commands.insert_resource(sm64_bevy::Sm64CodActive {
        level: level.clone(),
        area,
    });
    commands.remove_resource::<sm64_bevy::Sm64CodMapRequest>();
    diag::info!(World, "SM64 COD map: attaching `{level}` area {area} to installed IW4 proxy match");
}

fn place_sm64_cod_player_on_life_started(
    active: Option<Res<sm64_bevy::Sm64CodActive>>,
    spawn: Option<Res<Sm64CodSpawn>>,
    mut lives: MessageReader<frame::LifeStarted>,
    mut authority: Option<ResMut<net::AuthorityWorld>>,
) {
    if active.is_none() {
        return;
    }
    let (Some(spawn),Some(authority))=(spawn,authority.as_deref_mut()) else {
        return;
    };
    for life in lives.read() {
        let id=sim::ClientId(life.client);
        authority.0.teleport(id,spawn.origin);
        authority.0.set_viewangles(id,spawn.view);
        diag::info!(
            World,
            "SM64 COD map: life {} client {} spawned at {:?}",
            life.life,
            life.client,
            spawn.origin
        );
    }
}
fn sync_cod_player_into_sm64(
    active: Option<Res<sm64_bevy::Sm64CodActive>>,
    authority: Option<Res<net::AuthorityWorld>>,
    mut external: ResMut<sm64_bevy::Sm64ExternalPlayer>,
) {
    if active.is_none() {
        external.active=false;
        return;
    }
    let Some(authority)=authority else {
        external.active=false;
        return;
    };
    let mut first=None;
    authority.0.visit_players(|_,player|{
        if first.is_none() {
            first=Some(player.origin);
        }
    });
    let Some(origin)=first else {
        external.active=false;
        return;
    };
    external.sm64_pos=[origin[0],origin[2],origin[1]];
    external.active=true;
}

fn suppress_sm64_player_view(
    mut commands: Commands,
    active: Option<Res<sm64_bevy::Sm64CodActive>>,
    mario: Query<Entity, With<sm64_bevy::Sm64MarioPresentation>>,
    cameras: Query<(Entity, &Camera), With<Camera3d>>,
) {
    if active.is_none() {
        return;
    }

    for entity in &mario {
        commands.entity(entity).despawn();
    }
    for (entity, camera) in &cameras {
        if camera.order == 1000 {
            commands.entity(entity).despawn();
        }
    }
}

fn clear_sm64_cod_on_return(
    mut returned: MessageReader<frame::ReturnedToMenu>,
    mut commands: Commands,
) {
    if returned.read().next().is_none() {
        return;
    }
    commands.remove_resource::<sm64_bevy::Sm64CodMapRequest>();
    commands.remove_resource::<sm64_bevy::Sm64CodActive>();
    commands.remove_resource::<Sm64CodSpawn>();
    commands.remove_resource::<frame::ExternalWorldPresentation>();
    commands.remove_resource::<sm64_bevy::Sm64LaunchRequest>();
}

pub fn assemble_listen_app() -> App {
    let mut app = App::new();
    app.add_plugins(default_plugins_with_quiet_log(WindowPlugin {
        primary_window: None,
        ..default()
    }));
    add_runtime_plugins(&mut app);
    app
}

pub fn default_plugins_with_quiet_log(mut window: WindowPlugin) -> bevy::app::PluginGroupBuilder {
    if let Some(primary) = window.primary_window.as_mut() {
        primary.desired_maximum_frame_latency = core::num::NonZeroU32::new(frame_latency());
    }
    let mut wgpu = WgpuSettings::default();
    wgpu.features |= WgpuFeatures::TEXTURE_FORMAT_16BIT_NORM
        | WgpuFeatures::TEXTURE_COMPRESSION_BC
        | WgpuFeatures::POLYGON_MODE_LINE
        | WgpuFeatures::TEXTURE_BINDING_ARRAY
        | WgpuFeatures::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
        | WgpuFeatures::PARTIALLY_BOUND_BINDING_ARRAY;
    let plugins = DefaultPlugins
        .set(window)
        .set(LogPlugin {
            filter: "warn,iw4l=info,sm64_bevy=info,sm64_sim=info".into(),
            level: bevy::log::Level::WARN,
            ..default()
        })
        .set(BevyRenderPlugin {
            render_creation: RenderCreation::Automatic(Box::new(wgpu)),
            ..default()
        });
    if pipelined_rendering() {
        plugins
    } else {
        plugins.disable::<PipelinedRenderingPlugin>()
    }
}

const PIPELINED_RENDERING_ENV: &str = "IW4L_PIPELINED_RENDERING";

fn pipelined_rendering() -> bool {
    match std::env::var_os(PIPELINED_RENDERING_ENV) {
        None => true,
        Some(_) => perf::switch(PIPELINED_RENDERING_ENV),
    }
}

const FRAME_LATENCY_ENV: &str = "IW4L_FRAME_LATENCY";

pub(crate) fn frame_latency() -> u32 {
    const DEFAULT: u32 = if cfg!(target_os = "macos") { 2 } else { 1 };
    static FRAMES: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *FRAMES.get_or_init(|| {
        let Some(asked) = std::env::var_os(FRAME_LATENCY_ENV) else {
            return DEFAULT;
        };
        match asked.to_str().map(str::trim).and_then(|v| v.parse().ok()) {
            Some(frames) if frames > 0 => frames,
            _ => {
                diag::warn!(
                    Launch,
                    "{FRAME_LATENCY_ENV}={} is not a frame count; using {DEFAULT}",
                    asked.to_string_lossy(),
                );
                DEFAULT
            }
        }
    })
}

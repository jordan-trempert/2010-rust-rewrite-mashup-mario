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
            for vertex in [surface.vertex1,surface.vertex2,surface.vertex3] {
                // SM64 is Y-up. IW4 simulation is Z-up. Swapping Y/Z also
                // flips handedness, which gives clipmap_iw4 the winding it
                // expects for one-sided world triangles.
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

        let spawn_cod=[
            spawn.pos[0] as f32,
            spawn.pos[2] as f32,
            spawn.pos[1] as f32 + 64.0,
        ];
        let sm64_yaw=spawn.yaw_sm64() as u16 as f32 * 360.0 / 65536.0;
        let view=[0.0,90.0-sm64_yaw,0.0];
        let mut players=Vec::new();
        authority.0.visit_players(|id,_|players.push(id));
        for id in players {
            authority.0.teleport(id,spawn_cod);
            authority.0.set_viewangles(id,view);
        }
        diag::info!(
            World,
            "SM64 COD map: installed {} collision triangles and teleported {} COD player(s) to {:?}",
            parsed.world.surfaces.len(),
            authority.0.player_count(),
            spawn_cod
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

fn suppress_sm64_player_view(
    mut commands: Commands,
    active: Option<Res<sm64_bevy::Sm64CodActive>>,
    mario: Query<Entity, With<sm64_bevy::Sm64MarioPresentation>>,
    cameras: Query<(Entity, &Camera), With<Camera3d>>,
    mut proxy_world: Option<ResMut<render_frontend::prepare::scene::world::WorldScene>>,
) {
    if active.is_none() {
        return;
    }

    // The IW4 proxy exists only to supply scripts, weapons, bodies and shared
    // match content. Never draw its Rust geometry underneath the SM64 course.
    if let Some(world) = proxy_world.as_deref_mut()
        && (world.spawned || !world.batches.is_empty() || !world.static_model_meshes.is_empty())
    {
        *world = render_frontend::prepare::scene::world::WorldScene::default();
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

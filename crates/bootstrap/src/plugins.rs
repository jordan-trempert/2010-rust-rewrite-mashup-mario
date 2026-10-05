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

#[derive(Resource, Debug, Default, Clone)]
struct Sm64CodCollisionState {
    static_vertices: Vec<[f32;3]>,
    last_dynamic_tick: u32,
    last_dynamic_triangles: Vec<[[f32;3];3]>,
}

#[derive(Resource, Debug, Clone, Copy)]
struct Sm64CodSpawn {
    origin: [f32; 3],
    view: [f32; 3],
}

#[derive(Resource, Debug, Default, Clone, Copy)]
struct Sm64CodNativeState {
    last_sm64_health: Option<i32>,
    last_action: u32,
    last_weapon_shot_count: Option<i32>,
    last_attack_down: bool,
    last_use_down: bool,
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
    app.init_resource::<Sm64CodNativeState>();
    app.init_resource::<Sm64CodCollisionState>();
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
            apply_sm64_native_player_output,
            apply_sm64_dynamic_collision,
            suppress_sm64_player_view,
            clear_sm64_cod_on_return,
        ).chain());

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
    mut collision_state: ResMut<Sm64CodCollisionState>,
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
            // Proper rotation: SM64 (X,Y-up,Z) -> IW4 (X,-Z,Y-up).
            // clipmap_iw4 uses the opposite triangle cross-product convention,
            // so reverse v2/v3 here to keep SM64 floor normals facing upward.
            for vertex in [surface.vertex1,surface.vertex3,surface.vertex2] {
                let s=sm64_core::SM64_TO_IW4_SCALE;
                verts.push([
                    vertex[0] as f32*s,
                    -(vertex[2] as f32)*s,
                    vertex[1] as f32*s,
                ]);
            }
        }
        collision_state.static_vertices=verts.clone();
        collision_state.last_dynamic_tick=0;
        collision_state.last_dynamic_triangles.clear();
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
        const SM64_COD_SPAWN_CLEARANCE: f32 = 128.0;
        let s=sm64_core::SM64_TO_IW4_SCALE;
        let spawn_cod=[
            spawn.pos[0] as f32*s,
            -(spawn.pos[2] as f32)*s,
            floor_y*s + SM64_COD_SPAWN_CLEARANCE,
        ];
        let sm64_yaw=spawn.yaw_sm64() as u16 as f32 * 360.0 / 65536.0;
        let view=[0.0,sm64_yaw-90.0,0.0];

        // Validate the exact collision backend COD movement will use before
        // admitting the player. This is a swept standing-player capsule from
        // the spawn down through the expected Battlefield floor.
        let probe_end=[
            spawn_cod[0],
            spawn_cod[1],
            floor_y*sm64_core::SM64_TO_IW4_SCALE - 96.0,
        ];
        let probe=authority.0.trace_world(
            spawn_cod,
            probe_end,
            sim::PLAYER_MINS,
            sim::PLAYER_MAXS,
            sim::MASK_PLAYER_SOLID,
        );
        diag::info!(
            World,
            "SM64 COD collision probe: start={:?} end={:?} fraction={:.4} normal={:?} walkable={} startsolid={} allsolid={} contents=0x{:08x}",
            spawn_cod,
            probe_end,
            probe.fraction,
            probe.normal,
            probe.walkable,
            probe.startsolid,
            probe.allsolid,
            probe.contents
        );

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

    commands.insert_resource(Sm64CodNativeState::default());
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
    mut bridge_state: ResMut<Sm64CodNativeState>,
    mut external: ResMut<sm64_bevy::Sm64ExternalPlayer>,
) {
    if active.is_none() {
        external.active=false;
        external.attack_flags=0;
        bridge_state.last_weapon_shot_count=None;
        bridge_state.last_attack_down=false;
        bridge_state.last_use_down=false;
        return;
    }
    let Some(authority)=authority else {
        external.active=false;
        external.attack_flags=0;
        bridge_state.last_weapon_shot_count=None;
        bridge_state.last_attack_down=false;
        bridge_state.last_use_down=false;
        return;
    };
    let mut first=None;
    authority.0.visit_players(|id,player|{
        if first.is_none() {
            first=Some((
                id,
                player.origin,
                player.velocity,
                player.viewangles,
                player.health,
                player.weapon_shot_count,
            ));
        }
    });
    let Some((id,origin,velocity,viewangles,health,weapon_shot_count))=first else {
        external.active=false;
        external.attack_flags=0;
        bridge_state.last_weapon_shot_count=None;
        bridge_state.last_attack_down=false;
        bridge_state.last_use_down=false;
        return;
    };
    let inv_scale=1.0/sm64_core::SM64_TO_IW4_SCALE;
    external.sm64_pos=[
        origin[0]*inv_scale,
        origin[2]*inv_scale,
        -origin[1]*inv_scale,
    ];
    // IW4 velocities are units/second; SM64 stores velocity per 30 Hz tick.
    external.sm64_vel=[
        velocity[0]*inv_scale/sm64_sim::SM64_TICK_HZ as f32,
        velocity[2]*inv_scale/sm64_sim::SM64_TICK_HZ as f32,
        -velocity[1]*inv_scale/sm64_sim::SM64_TICK_HZ as f32,
    ];
    let sm64_yaw_degrees=viewangles[1]+90.0;
    external.sm64_yaw=((sm64_yaw_degrees/360.0)*65536.0) as i32 as i16;
    // IW4 pitch is positive downward; SM64 cannon pitch is positive upward.
    external.sm64_pitch=((-viewangles[0]/360.0)*65536.0) as i32 as i16;
    let command_buttons=authority.0.command_buttons(id);
    let attack_down=(command_buttons & 0x1)!=0;
    let weapon_fired=bridge_state
        .last_weapon_shot_count
        .is_some_and(|previous|weapon_shot_count!=previous);
    let use_down=(command_buttons & 0x8)!=0;
    let use_pressed=use_down && !bridge_state.last_use_down;

    // Bit 0: external weapon/ground-pound attack. Keep it asserted while fire
    // is held so the 30 Hz native bridge cannot miss a short COD input pulse.
    // Bit 1: one-shot native B/use press for NPCs, signs and dialog advance.
    external.attack_flags=0;
    if attack_down || weapon_fired {
        external.attack_flags|=1;
    }
    if use_pressed {
        external.attack_flags|=2;
    }

    bridge_state.last_attack_down=attack_down;
    bridge_state.last_use_down=use_down;
    bridge_state.last_weapon_shot_count=Some(weapon_shot_count);
    external.health=health;
    external.active=true;
}

fn apply_sm64_dynamic_collision(
    active: Option<Res<sm64_bevy::Sm64CodActive>>,
    dynamic: Res<sm64_bevy::Sm64NativeDynamicCollision>,
    mut collision_state: ResMut<Sm64CodCollisionState>,
    mut authority: Option<ResMut<net::AuthorityWorld>>,
) {
    if active.is_none() {
        return;
    }
    if dynamic.tick==0 || dynamic.tick==collision_state.last_dynamic_tick {
        return;
    }
    collision_state.last_dynamic_tick=dynamic.tick;

    // Rebuilding/installing the entire clip mesh every 30 Hz native snapshot
    // is expensive. Most frames have identical moving-platform collision, so
    // only touch the authority collision backend when the triangles actually
    // changed.
    if dynamic.triangles==collision_state.last_dynamic_triangles {
        return;
    }
    collision_state.last_dynamic_triangles=dynamic.triangles.clone();

    let Some(authority)=authority.as_deref_mut() else {return;};

    let s=sm64_core::SM64_TO_IW4_SCALE;
    let mut verts=collision_state.static_vertices.clone();
    verts.reserve(dynamic.triangles.len()*3);

    for tri in &dynamic.triangles {
        // Same handedness/winding conversion used by the static course mesh.
        for vertex in [tri[0],tri[2],tri[1]] {
            verts.push([
                vertex[0]*s,
                -vertex[2]*s,
                vertex[1]*s,
            ]);
        }
    }

    let mesh=sim::SimClipMesh::from_linear_triangles(verts);
    let content=authority.0.content().with_clip_mesh(mesh);
    authority.0.install_content(content);
}

fn apply_sm64_native_player_output(
    active: Option<Res<sm64_bevy::Sm64CodActive>>,
    output: Res<sm64_bevy::Sm64NativePlayerOutput>,
    mut bridge_state: ResMut<Sm64CodNativeState>,
    mut authority: Option<ResMut<net::AuthorityWorld>>,
) {
    if active.is_none() {
        bridge_state.last_sm64_health=None;
        bridge_state.last_action=0;
        return;
    }
    let Some(authority)=authority.as_deref_mut() else {return;};

    let mut first=None;
    authority.0.visit_players(|id,player|{
        if first.is_none() {
            first=Some((id,*player));
        }
    });
    let Some((id,player))=first else {return;};

    if !output.active {
        authority.0.set_external_motion(id,false);
        bridge_state.last_sm64_health=None;
        bridge_state.last_action=0;
        return;
    }

    // Preserve ordinary COD damage while layering SM64 damage/healing on top.
    // SM64's normal full health is 0x880, so apply the *delta* rather than
    // replacing COD health with an unrelated absolute scale every frame.
    if let Some(previous)=bridge_state.last_sm64_health {
        let sm64_delta=output.health-previous;
        if sm64_delta!=0 {
            let scaled=((sm64_delta as f32)*(player.max_health.max(1) as f32)/0x880 as f32).round() as i32;
            if scaled!=0 {
                authority.0.set_health(id,player.health.saturating_add(scaled));
            }
        }
    }
    bridge_state.last_sm64_health=Some(output.health);

    let group=output.action & sm64_core::ACT_GROUP_MASK;
    let owns_motion=matches!(
        group,
        sm64_core::ACT_GROUP_OBJECT
            | sm64_core::ACT_GROUP_AUTOMATIC
            | sm64_core::ACT_GROUP_CUTSCENE
    ) || matches!(
        output.action,
        sm64_core::ACT_SHOT_FROM_CANNON
            | sm64_core::ACT_TORNADO_TWIRLING
            | sm64_core::ACT_GRABBED
            | sm64_core::ACT_RIDING_HOOT
            | sm64_core::ACT_WARP_DOOR_SPAWN
            | sm64_core::ACT_EMERGE_FROM_PIPE
            | sm64_core::ACT_SPAWN_SPIN_AIRBORNE
            | sm64_core::ACT_SPAWN_NO_SPIN_AIRBORNE
            | sm64_core::ACT_TELEPORT_FADE_OUT
            | sm64_core::ACT_TELEPORT_FADE_IN
            | sm64_core::ACT_EXIT_AIRBORNE
            | sm64_core::ACT_SPECIAL_EXIT_AIRBORNE
    );

    authority.0.set_external_motion(id,owns_motion);
    if owns_motion {
        let s=sm64_core::SM64_TO_IW4_SCALE;
        let origin=[
            output.sm64_pos[0]*s,
            -output.sm64_pos[2]*s,
            output.sm64_pos[1]*s,
        ];
        let velocity=[
            output.sm64_vel[0]*s*sm64_sim::SM64_TICK_HZ as f32,
            -output.sm64_vel[2]*s*sm64_sim::SM64_TICK_HZ as f32,
            output.sm64_vel[1]*s*sm64_sim::SM64_TICK_HZ as f32,
        ];
        authority.0.set_origin(id,origin);
        authority.0.set_velocity(id,velocity);

        let sm64_yaw_degrees=output.sm64_yaw as u16 as f32*360.0/65536.0;
        let mut view=player.viewangles;
        view[1]=sm64_yaw_degrees-90.0;
        authority.0.set_viewangles(id,view);
    }

    if output.action!=bridge_state.last_action {
        diag::info!(
            World,
            "SM64 native player action 0x{:08x} -> 0x{:08x} (external_motion={})",
            bridge_state.last_action,
            output.action,
            owns_motion
        );
        bridge_state.last_action=output.action;
    }
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
    commands.insert_resource(Sm64CodCollisionState::default());
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

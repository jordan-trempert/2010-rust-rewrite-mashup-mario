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
    static_vertices: Vec<[f32; 3]>,
    normal_static_vertices: Vec<[f32; 3]>,
    vanish_static_vertices: Vec<[f32; 3]>,
    vanish_active: bool,
    level: String,
    area: u8,
    last_dynamic_tick: u32,
    last_dynamic_triangles: Vec<[[f32; 3]; 3]>,
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
    last_area: u32,
    force_native_reposition: bool,
    last_weapon_shot_count: Option<i32>,
    last_attack_down: bool,
    last_use_down: bool,
}

#[derive(Resource, Debug, Default, Clone, Copy)]
struct Sm64CodAudioState {
    last_tick: u32,
}

#[derive(Resource, Debug, Default, Clone, Copy)]
struct Sm64CodMoveState {
    last_jump_down: bool,
    last_pound_down: bool,
    last_grounded: bool,
    jump_stage: u8,
    ground_pounding: bool,
}

fn sm64_static_collision_vertices(
    parsed: &sm64_assets::ParsedCollision,
    vanish_cap: bool,
) -> Vec<[f32; 3]> {
    let s = sm64_core::SM64_TO_IW4_SCALE;
    /*
     * clipmap_iw4 triangle sweeps are front-face only. SM64's collision
     * routines are not: interior walls and room boundaries may be approached
     * from either side depending on the current room. Emit both windings so
     * the imported world behaves as solid geometry in COD instead of allowing
     * the player to fall/walk through the back face of a valid SM64 surface.
     */
    let mut verts = Vec::with_capacity(parsed.world.surfaces.len() * 6);
    for surface in &parsed.world.surfaces {
        if vanish_cap && surface.surface_type as i32 == sm64_core::SURFACE_VANISH_CAP_WALLS {
            continue;
        }

        let convert = |vertex: [i16; 3]| {
            [
                vertex[0] as f32 * s,
                -(vertex[2] as f32) * s,
                vertex[1] as f32 * s,
            ]
        };

        let v1 = convert(surface.vertex1);
        let v2 = convert(surface.vertex2);
        let v3 = convert(surface.vertex3);

        // Front face in clipmap_iw4's reversed cross-product convention.
        verts.extend_from_slice(&[v1, v3, v2]);
        // Matching back face so SM64 collision remains solid from either side.
        verts.extend_from_slice(&[v1, v2, v3]);
    }
    verts
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
    app.init_resource::<Sm64CodAudioState>();
    app.init_resource::<Sm64CodMoveState>();
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
        .add_systems(
            Update,
            (
                arm_sm64_cod_external_presentation,
                launch_installed_sm64_cod_map,
                place_sm64_cod_player_on_life_started,
                sync_cod_player_into_sm64.before(sm64_bevy::Sm64RuntimeStep),
                apply_sm64_native_player_output.after(sm64_bevy::Sm64RuntimeStep),
                sync_sm64_cod_area_collision,
                sync_sm64_cap_collision,
                apply_sm64_dynamic_collision,
                handle_sm64_cod_level_transition,
                publish_sm64_native_audio,
                publish_sm64_cod_hud,
                suppress_sm64_player_view,
                clear_sm64_cod_on_return,
            )
                .chain(),
        );

    app.add_systems(
        FixedUpdate,
        apply_sm64_cod_movement_abilities
            .after(net::AuthoritySet::Gather)
            .before(net::AuthoritySet::Step),
    );

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

fn arm_sm64_cod_external_presentation(
    pending: Option<Res<sm64_bevy::Sm64CodMapRequest>>,
    active: Option<Res<sm64_bevy::Sm64CodActive>>,
    external: Option<Res<frame::ExternalWorldPresentation>>,
    mut commands: Commands,
) {
    if pending.is_some() || active.is_some() {
        if !external.is_some_and(|value| value.0) {
            // Arm this before MatchInstalled/world-spawn processing so the
            // donor Rust scene never gets a visible frame ahead of BOB.
            commands.insert_resource(frame::ExternalWorldPresentation(true));
        }
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

    let Some(root) = std::env::var_os("SM64_DECOMP_ROOT").map(std::path::PathBuf::from) else {
        diag::warn!(World, "SM64 COD map: SM64_DECOMP_ROOT is not configured");
        return;
    };

    let parsed = match sm64_assets::load_level_collision(&root, &level, area) {
        Ok(parsed) => parsed,
        Err(error) => {
            diag::warn!(
                World,
                "SM64 COD map: collision load failed for {level}: {error}"
            );
            return;
        }
    };
    let spawn = match sm64_assets::load_mario_spawn(&root, &level, area) {
        Ok(spawn) => spawn,
        Err(error) => {
            diag::warn!(
                World,
                "SM64 COD map: spawn load failed for {level}: {error}"
            );
            return;
        }
    };

    if let Some(authority) = authority.as_deref_mut() {
        let verts = sm64_static_collision_vertices(&parsed, false);
        let vanish_verts = sm64_static_collision_vertices(&parsed, true);
        collision_state.static_vertices = verts.clone();
        collision_state.normal_static_vertices = verts.clone();
        collision_state.vanish_static_vertices = vanish_verts;
        collision_state.vanish_active = false;
        collision_state.level = level.clone();
        collision_state.area = area;
        collision_state.last_dynamic_tick = 0;
        collision_state.last_dynamic_triangles.clear();
        let mesh = sim::SimClipMesh::from_linear_triangles(verts);
        let content = authority.0.content().with_clip_mesh(mesh);
        authority.0.install_content(content);

        // with_clip_mesh intentionally removes the donor BSP brushes. The
        // imported SM64 triangle mesh is already complete at this point, so
        // the mp_rust preparation hold must not keep authority/admission
        // frozen waiting on donor collision that is no longer in use.
        commands.insert_resource(net::AuthorityLoadHold(false));

        let floor_y = parsed
            .world
            .find_floor(spawn.pos[0] as f32, 30_000.0, spawn.pos[2] as f32)
            .map(|hit| hit.height)
            .unwrap_or(spawn.pos[1] as f32);
        // Start well above the detected SM64 floor. This gives COD's
        // capsule several frames to settle onto the imported collision and
        // makes a bad floor/collision transform obvious in the log.
        const SM64_COD_SPAWN_CLEARANCE: f32 = 128.0;
        let s = sm64_core::SM64_TO_IW4_SCALE;
        let spawn_cod = [
            spawn.pos[0] as f32 * s,
            -(spawn.pos[2] as f32) * s,
            floor_y * s + SM64_COD_SPAWN_CLEARANCE,
        ];
        let sm64_yaw = spawn.yaw_sm64() as u16 as f32 * 360.0 / 65536.0;
        let view = [0.0, sm64_yaw - 90.0, 0.0];

        // Validate the exact collision backend COD movement will use before
        // admitting the player. This is a swept standing-player capsule from
        // the spawn down through the expected Battlefield floor.
        let probe_end = [
            spawn_cod[0],
            spawn_cod[1],
            floor_y * sm64_core::SM64_TO_IW4_SCALE - 96.0,
        ];
        let probe = authority.0.trace_world(
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
        diag::warn!(
            World,
            "SM64 COD map: authority world unavailable while attaching {level}"
        );
    }

    commands.insert_resource(Sm64CodNativeState::default());
    commands.insert_resource(sm64_bevy::Sm64LaunchRequest::new(level.clone(), area));
    commands.insert_resource(sm64_bevy::Sm64CodActive {
        level: level.clone(),
        area,
    });
    // SM64/Bevy is the visible world for this mode. Tell session admission not
    // to wait forever for the hidden donor IW4 WorldScene's GPU quiet frames.
    // clear_sm64_cod_on_return removes this resource when leaving the map.
    commands.insert_resource(frame::ExternalWorldPresentation(true));
    commands.remove_resource::<sm64_bevy::Sm64CodMapRequest>();
    diag::info!(
        World,
        "SM64 COD map: attaching `{level}` area {area} to installed IW4 proxy match"
    );
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
    let (Some(spawn), Some(authority)) = (spawn, authority.as_deref_mut()) else {
        return;
    };
    for life in lives.read() {
        let id = sim::ClientId(life.client);
        authority.0.teleport(id, spawn.origin);
        authority.0.set_viewangles(id, spawn.view);
        diag::info!(
            World,
            "SM64 COD map: life {} client {} spawned at {:?}",
            life.life,
            life.client,
            spawn.origin
        );
    }
}
fn sm64_cod_native_owns_motion(action: u32) -> bool {
    /*
     * Dialogs, stars and area transitions keep their native state machines,
     * but they never suppress COD pmove. Only mechanics whose movement itself
     * is the gameplay hand control of the player transform to native SM64.
     */
    matches!(
        action,
        sm64_core::ACT_SHOT_FROM_CANNON
            | sm64_core::ACT_TORNADO_TWIRLING
            | sm64_core::ACT_GRABBED
            | sm64_core::ACT_RIDING_HOOT
    )
}

fn sm64_cod_native_reposition_action(action: u32) -> bool {
    matches!(
        action,
        sm64_core::ACT_WARP_DOOR_SPAWN
            | sm64_core::ACT_EMERGE_FROM_PIPE
            | sm64_core::ACT_SPAWN_SPIN_AIRBORNE
            | sm64_core::ACT_SPAWN_NO_SPIN_AIRBORNE
            | sm64_core::ACT_TELEPORT_FADE_OUT
            | sm64_core::ACT_TELEPORT_FADE_IN
            | sm64_core::ACT_EXIT_AIRBORNE
            | sm64_core::ACT_SPECIAL_EXIT_AIRBORNE
    )
}

fn apply_sm64_cod_movement_abilities(
    active: Option<Res<sm64_bevy::Sm64CodActive>>,
    output: Res<sm64_bevy::Sm64NativePlayerOutput>,
    local: Option<Res<net::LocalPresentClient>>,
    pending: Res<net::PendingAuthorityInput>,
    mut state: ResMut<Sm64CodMoveState>,
    mut external: ResMut<sm64_bevy::Sm64ExternalPlayer>,
    mut authority: Option<ResMut<net::AuthorityWorld>>,
) {
    const ENTITYNUM_NONE: i32 = 1023;
    const BUTTON_PRONE: u32 = 0x100;
    const BUTTON_CROUCH: u32 = 0x200;
    const BUTTON_JUMP: u32 = 0x400;
    const DOUBLE_JUMP_VELOCITY: f32 = 360.0;
    const TRIPLE_JUMP_VELOCITY: f32 = 500.0;
    const GROUND_POUND_VELOCITY: f32 = -900.0;
    const WING_ASCENT_VELOCITY: f32 = 220.0;
    const WING_GLIDE_FALL_VELOCITY: f32 = -110.0;

    if active.is_none() {
        *state = Sm64CodMoveState::default();
        external.pending_ground_pound = false;
        return;
    }
    let Some(authority) = authority.as_deref_mut() else {
        return;
    };
    let Some(input) = pending.0.as_ref() else {
        return;
    };

    let target = local
        .as_ref()
        .map(|id| id.0)
        .or_else(|| input.cmds.first().map(|(id, _)| *id));
    let Some(id) = target else {
        return;
    };
    let Some((_, cmd)) = input.cmds.iter().rev().find(|(client, _)| *client == id) else {
        return;
    };
    let Some(player) = authority.0.player(id).copied() else {
        return;
    };

    let jump_down = (cmd.buttons & BUTTON_JUMP) != 0;
    let pound_down = (cmd.buttons & (BUTTON_PRONE | BUTTON_CROUCH)) != 0;
    let jump_pressed = jump_down && !state.last_jump_down;
    let pound_pressed = pound_down && !state.last_pound_down;
    let grounded = player.ground_entity_num != ENTITYNUM_NONE;
    let just_left_ground = state.last_grounded && !grounded;

    // Cannons/grabs/etc. truly own native motion. Dialogs, stars and warps do
    // not block these abilities or ordinary COD movement.
    if player.health <= 0 || (output.active && sm64_cod_native_owns_motion(output.action)) {
        state.ground_pounding = false;
        state.jump_stage = 0;
        state.last_jump_down = jump_down;
        state.last_pound_down = pound_down;
        state.last_grounded = grounded;
        return;
    }

    if grounded {
        if state.ground_pounding {
            external.pending_ground_pound = true;
            diag::info!(World, "SM64 COD ground pound landed");
        }
        state.ground_pounding = false;

        // Let COD's normal pmove perform jump #1. This only arms the chain.
        if jump_pressed {
            state.jump_stage = 1;
        } else if !jump_down {
            state.jump_stage = 0;
        }
    } else {
        if just_left_ground && state.jump_stage == 0 {
            state.jump_stage = 1;
        }

        if pound_pressed && !state.ground_pounding {
            let mut velocity = player.velocity;
            velocity[0] *= 0.45;
            velocity[1] *= 0.45;
            velocity[2] = GROUND_POUND_VELOCITY;
            authority.0.set_velocity(id, velocity);
            state.ground_pounding = true;
            diag::info!(World, "SM64 COD ground pound started");
        } else if state.ground_pounding {
            let mut velocity = player.velocity;
            velocity[0] *= 0.9;
            velocity[1] *= 0.9;
            velocity[2] = velocity[2].min(GROUND_POUND_VELOCITY);
            authority.0.set_velocity(id, velocity);
        } else if output.active && (output.mario_flags & sm64_core::MARIO_WING_CAP) != 0 {
            // COD owns ordinary locomotion, so translate the active native
            // Wing Cap into controllable ascent/gliding while preserving COD
            // air steering. The native timer, music and visual cap still own
            // the duration and presentation.
            let mut velocity = player.velocity;
            velocity[2] = if jump_down {
                velocity[2].max(WING_ASCENT_VELOCITY)
            } else {
                velocity[2].max(WING_GLIDE_FALL_VELOCITY)
            };
            authority.0.set_velocity(id, velocity);
        } else if jump_pressed && !just_left_ground {
            let (next_stage, vertical) = match state.jump_stage {
                0 | 1 => (2, DOUBLE_JUMP_VELOCITY),
                2 => (3, TRIPLE_JUMP_VELOCITY),
                _ => (state.jump_stage, player.velocity[2]),
            };
            if next_stage != state.jump_stage {
                let mut velocity = player.velocity;
                velocity[2] = vertical.max(velocity[2]);
                authority.0.set_velocity(id, velocity);
                state.jump_stage = next_stage;
                diag::info!(
                    World,
                    "SM64 COD {} jump vz={:.1}",
                    if next_stage == 2 { "double" } else { "triple" },
                    velocity[2]
                );
            }
        }
    }

    state.last_jump_down = jump_down;
    state.last_pound_down = pound_down;
    state.last_grounded = grounded;
}

fn sync_cod_player_into_sm64(
    active: Option<Res<sm64_bevy::Sm64CodActive>>,
    authority: Option<Res<net::AuthorityWorld>>,
    mut bridge_state: ResMut<Sm64CodNativeState>,
    mut external: ResMut<sm64_bevy::Sm64ExternalPlayer>,
) {
    if active.is_none() {
        external.active = false;
        external.attack_flags = 0;
        external.pending_use = false;
        external.pending_fire = false;
        external.pending_ground_pound = false;
        bridge_state.last_area = 0;
        bridge_state.last_weapon_shot_count = None;
        bridge_state.last_attack_down = false;
        bridge_state.last_use_down = false;
        return;
    }
    let Some(authority) = authority else {
        external.active = false;
        external.attack_flags = 0;
        external.pending_use = false;
        external.pending_fire = false;
        external.pending_ground_pound = false;
        bridge_state.last_area = 0;
        bridge_state.last_weapon_shot_count = None;
        bridge_state.last_attack_down = false;
        bridge_state.last_use_down = false;
        return;
    };
    let mut first = None;
    authority.0.visit_players(|id, player| {
        if first.is_none() {
            first = Some((
                id,
                player.origin,
                player.velocity,
                player.viewangles,
                player.health,
                player.weapon_shot_count,
            ));
        }
    });
    let Some((id, origin, velocity, viewangles, health, weapon_shot_count)) = first else {
        external.active = false;
        external.attack_flags = 0;
        external.pending_use = false;
        external.pending_fire = false;
        external.pending_ground_pound = false;
        bridge_state.last_area = 0;
        bridge_state.last_weapon_shot_count = None;
        bridge_state.last_attack_down = false;
        bridge_state.last_use_down = false;
        return;
    };
    let inv_scale = 1.0 / sm64_core::SM64_TO_IW4_SCALE;
    external.sm64_pos = [
        origin[0] * inv_scale,
        origin[2] * inv_scale,
        -origin[1] * inv_scale,
    ];
    // IW4 velocities are units/second; SM64 stores velocity per 30 Hz tick.
    external.sm64_vel = [
        velocity[0] * inv_scale / sm64_sim::SM64_TICK_HZ as f32,
        velocity[2] * inv_scale / sm64_sim::SM64_TICK_HZ as f32,
        -velocity[1] * inv_scale / sm64_sim::SM64_TICK_HZ as f32,
    ];
    let sm64_yaw_degrees = viewangles[1] + 90.0;
    external.sm64_yaw = ((sm64_yaw_degrees / 360.0) * 65536.0) as i32 as i16;
    // IW4 pitch is positive downward; SM64 cannon pitch is positive upward.
    external.sm64_pitch = ((-viewangles[0] / 360.0) * 65536.0) as i32 as i16;
    let command_buttons = authority.0.command_buttons(id);
    let attack_down = (command_buttons & 0x1) != 0;
    let weapon_fired = bridge_state
        .last_weapon_shot_count
        .is_some_and(|previous| weapon_shot_count != previous);
    // IW4 USE and controller/contextual USE_RELOAD both advance native dialogs.
    let use_down = (command_buttons & (0x8 | 0x20)) != 0;
    let use_pressed = use_down && !bridge_state.last_use_down;

    // Bit 0: held external weapon attack.
    // Bit 1: one-shot native B/A use press for NPCs, signs and dialog advance.
    // Bit 2 is added downstream for a discrete weapon shot; bit 3 is the
    // movement system's one-shot ground-pound landing pulse.
    // One-shot presses are latched until a 30 Hz native step consumes them
    // because the render/update rate is normally higher than the SM64 tick.
    external.attack_flags = if attack_down { 1 } else { 0 };
    if weapon_fired || (attack_down && !bridge_state.last_attack_down) {
        external.pending_fire = true;
    }
    if use_pressed {
        external.pending_use = true;
    }

    bridge_state.last_attack_down = attack_down;
    bridge_state.last_use_down = use_down;
    bridge_state.last_weapon_shot_count = Some(weapon_shot_count);
    external.health = health;
    external.active = true;
}

fn handle_sm64_cod_level_transition(
    mut transition: ResMut<sm64_bevy::Sm64NativeTransitionOutput>,
    active: Option<Res<sm64_bevy::Sm64CodActive>>,
    mut bridge_state: ResMut<Sm64CodNativeState>,
    mut move_state: ResMut<Sm64CodMoveState>,
    mut collision_state: ResMut<Sm64CodCollisionState>,
    mut native_dynamic: ResMut<sm64_bevy::Sm64NativeDynamicCollision>,
    mut authority: Option<ResMut<net::AuthorityWorld>>,
    mut commands: Commands,
) {
    let Some(target) = transition.pending.take() else {
        return;
    };
    let Some(current) = active else {
        return;
    };
    if target.level.is_empty() {
        return;
    }

    let area = target.area.max(1);
    let Some(root) = std::env::var_os("SM64_DECOMP_ROOT").map(std::path::PathBuf::from) else {
        diag::warn!(
            World,
            "SM64 COD level transition {} -> {} refused: SM64_DECOMP_ROOT is not configured",
            current.level,
            target.level
        );
        return;
    };

    let parsed = match sm64_assets::load_level_collision(&root, &target.level, area) {
        Ok(parsed) => parsed,
        Err(error) => {
            diag::warn!(
                World,
                "SM64 COD level transition {} -> {} area {} collision failed: {}",
                current.level,
                target.level,
                area,
                error
            );
            return;
        }
    };
    let spawn = sm64_assets::load_mario_spawn(&root, &target.level, area).ok();

    if let Some(authority) = authority.as_deref_mut() {
        let s = sm64_core::SM64_TO_IW4_SCALE;
        let verts = sm64_static_collision_vertices(&parsed, false);
        let vanish_verts = sm64_static_collision_vertices(&parsed, true);
        collision_state.static_vertices = verts.clone();
        collision_state.normal_static_vertices = verts.clone();
        collision_state.vanish_static_vertices = vanish_verts;
        collision_state.vanish_active = false;
        collision_state.level = target.level.clone();
        collision_state.area = area;
        collision_state.last_dynamic_tick = 0;
        collision_state.last_dynamic_triangles.clear();
        native_dynamic.tick = 0;
        native_dynamic.triangles.clear();

        let static_triangle_count = verts.len() / 3;
        let mesh = sim::SimClipMesh::from_linear_triangles(verts);
        let content = authority.0.content().with_clip_mesh(mesh);
        authority.0.install_content(content);
        diag::info!(
            World,
            "SM64 COD level transition collision: installed {} static triangles for {} area {}",
            static_triangle_count,
            target.level,
            area
        );

        let fallback_spawn = spawn.as_ref().map(|spawn| {
            let floor_y = parsed
                .world
                .find_floor(spawn.pos[0] as f32, 30_000.0, spawn.pos[2] as f32)
                .map(|hit| hit.height)
                .unwrap_or(spawn.pos[1] as f32);
            let spawn_cod = [
                spawn.pos[0] as f32 * s,
                -(spawn.pos[2] as f32) * s,
                floor_y * s + 128.0,
            ];
            let sm64_yaw = spawn.yaw_sm64() as u16 as f32 * 360.0 / 65536.0;
            Sm64CodSpawn {
                origin: spawn_cod,
                view: [0.0, sm64_yaw - 90.0, 0.0],
            }
        });
        if let Some(fallback) = fallback_spawn {
            commands.insert_resource(fallback);
        }

        let mut first = None;
        authority.0.visit_players(|id, _| {
            if first.is_none() {
                first = Some(id);
            }
        });
        if let Some(id) = first {
            /*
             * Do not move the player to the level's generic MARIO_POS here.
             * Cross-level doors and paintings have their own destination warp
             * nodes, and the native DLL has already resolved the exact spawn.
             * Hold COD movement for the handoff frame; the first native
             * snapshot will reposition us to that exact point.
             */
            authority.0.set_external_motion(id, true);
            authority.0.set_velocity(id, [0.0; 3]);
        }
    }

    bridge_state.last_sm64_health = None;
    bridge_state.last_action = 0;
    bridge_state.last_area = 0;
    bridge_state.force_native_reposition = true;
    *move_state = Sm64CodMoveState::default();

    commands.insert_resource(sm64_bevy::Sm64CodActive {
        level: target.level.clone(),
        area,
    });
    commands.insert_resource(sm64_bevy::Sm64LaunchRequest::with_warp(
        target.level.clone(),
        area,
        target.node,
        target.arg,
    ));

    diag::info!(
        World,
        "SM64 COD host transition: {} -> {} area={} node={} arg={}",
        current.level,
        target.level,
        area,
        target.node,
        target.arg
    );
}

fn sync_sm64_cod_area_collision(
    active: Option<Res<sm64_bevy::Sm64CodActive>>,
    output: Res<sm64_bevy::Sm64NativePlayerOutput>,
    mut authority: Option<ResMut<net::AuthorityWorld>>,
    mut collision_state: ResMut<Sm64CodCollisionState>,
) {
    let (Some(active), Some(authority)) = (active, authority.as_deref_mut()) else {
        return;
    };
    let Ok(area) = u8::try_from(output.area_index) else {
        return;
    };
    if !output.active
        || area == 0
        || (active.level == collision_state.level && area == collision_state.area)
    {
        return;
    }

    let Some(root) = std::env::var_os("SM64_DECOMP_ROOT").map(std::path::PathBuf::from) else {
        diag::warn!(
            World,
            "SM64 COD area transition: SM64_DECOMP_ROOT is not configured"
        );
        return;
    };
    let parsed = match sm64_assets::load_level_collision(&root, &active.level, area) {
        Ok(parsed) => parsed,
        Err(error) => {
            diag::warn!(
                World,
                "SM64 COD area transition: collision load failed for {} area {}: {}",
                active.level,
                area,
                error
            );
            return;
        }
    };

    let verts = sm64_static_collision_vertices(&parsed, false);
    let vanish_verts = sm64_static_collision_vertices(&parsed, true);

    collision_state.static_vertices = verts.clone();
    collision_state.normal_static_vertices = verts.clone();
    collision_state.vanish_static_vertices = vanish_verts;
    collision_state.vanish_active = false;
    collision_state.level = active.level.clone();
    collision_state.area = area;
    collision_state.last_dynamic_tick = 0;
    collision_state.last_dynamic_triangles.clear();

    let mesh = sim::SimClipMesh::from_linear_triangles(verts);
    let content = authority.0.content().with_clip_mesh(mesh);
    authority.0.install_content(content);
    diag::info!(
        World,
        "SM64 COD area transition: installed {} static triangles for {} area {}",
        parsed.world.surfaces.len(),
        active.level,
        area
    );
}

fn sync_sm64_cap_collision(
    active: Option<Res<sm64_bevy::Sm64CodActive>>,
    output: Res<sm64_bevy::Sm64NativePlayerOutput>,
    mut collision_state: ResMut<Sm64CodCollisionState>,
    mut authority: Option<ResMut<net::AuthorityWorld>>,
) {
    if active.is_none() || !output.active {
        return;
    }
    let vanish_active = (output.mario_flags & sm64_core::MARIO_VANISH_CAP) != 0;
    if vanish_active == collision_state.vanish_active {
        return;
    }
    let Some(authority) = authority.as_deref_mut() else {
        return;
    };

    collision_state.vanish_active = vanish_active;
    collision_state.static_vertices = if vanish_active {
        collision_state.vanish_static_vertices.clone()
    } else {
        collision_state.normal_static_vertices.clone()
    };

    let s = sm64_core::SM64_TO_IW4_SCALE;
    let mut verts = collision_state.static_vertices.clone();
    verts.reserve(collision_state.last_dynamic_triangles.len() * 3);
    for tri in &collision_state.last_dynamic_triangles {
        for vertex in [tri[0], tri[2], tri[1]] {
            verts.push([vertex[0] * s, -vertex[2] * s, vertex[1] * s]);
        }
    }
    let mesh = sim::SimClipMesh::from_linear_triangles(verts);
    let content = authority.0.content().with_clip_mesh(mesh);
    authority.0.install_content(content);
    diag::info!(
        World,
        "SM64 COD Vanish Cap collision {} (timer={})",
        if vanish_active { "enabled" } else { "restored" },
        output.cap_timer
    );
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
    if dynamic.tick == 0 || dynamic.tick == collision_state.last_dynamic_tick {
        return;
    }
    collision_state.last_dynamic_tick = dynamic.tick;

    // Rebuilding/installing the entire clip mesh every 30 Hz native snapshot
    // is expensive. Most frames have identical moving-platform collision, so
    // only touch the authority collision backend when the triangles actually
    // changed.
    if dynamic.triangles == collision_state.last_dynamic_triangles {
        return;
    }
    collision_state.last_dynamic_triangles = dynamic.triangles.clone();

    let Some(authority) = authority.as_deref_mut() else {
        return;
    };

    let s = sm64_core::SM64_TO_IW4_SCALE;
    let mut verts = collision_state.static_vertices.clone();
    verts.reserve(dynamic.triangles.len() * 3);

    for tri in &dynamic.triangles {
        let v0 = [tri[0][0] * s, -tri[0][2] * s, tri[0][1] * s];
        let v1 = [tri[1][0] * s, -tri[1][2] * s, tri[1][1] * s];
        let v2 = [tri[2][0] * s, -tri[2][2] * s, tri[2][1] * s];

        // Keep moving platforms/doors solid from both sides as well.
        verts.extend_from_slice(&[v0, v2, v1]);
        verts.extend_from_slice(&[v0, v1, v2]);
    }

    let mesh = sim::SimClipMesh::from_linear_triangles(verts);
    let content = authority.0.content().with_clip_mesh(mesh);
    authority.0.install_content(content);
}

#[derive(Debug, Clone, Copy)]
struct Sm64TransitionFloorProbe {
    hit: bool,
    end: [f32; 3],
    normal: [f32; 3],
    startsolid: u8,
    allsolid: u8,
}

fn sm64_safe_transition_origin(
    authority: &sim::SimWorld,
    native_origin: [f32; 3],
) -> ([f32; 3], Sm64TransitionFloorProbe) {
    /*
     * Native SM64 warp nodes often place Mario exactly on the floor plane.
     * IW4's imported triangle world is one-sided, so starting the COD capsule
     * exactly on (or a tiny float-rounding amount below) that plane can let the
     * first gravity step pass through it. Probe downward from safely above the
     * native point and only snap when the discovered floor is already very
     * close to the native Y. Airborne painting/course spawns stay untouched.
     */
    const PROBE_UP: f32 = 48.0;
    const PROBE_DOWN: f32 = 96.0;
    const GROUNDED_TOLERANCE: f32 = 24.0;
    const FLOOR_EPSILON: f32 = 1.0;

    let start = [
        native_origin[0],
        native_origin[1],
        native_origin[2] + PROBE_UP,
    ];
    let end = [
        native_origin[0],
        native_origin[1],
        native_origin[2] - PROBE_DOWN,
    ];
    let probe = authority.trace_world(
        start,
        end,
        [0.0; 3],
        [0.0; 3],
        sim::MASK_PLAYER_SOLID,
    );
    let summary = Sm64TransitionFloorProbe {
        hit: probe.fraction < 1.0,
        end: probe.endpos,
        normal: probe.normal,
        startsolid: probe.startsolid,
        allsolid: probe.allsolid,
    };

    if summary.hit
        && summary.startsolid == 0
        && summary.allsolid == 0
        && summary.normal[2] >= 0.5
        && (native_origin[2] - summary.end[2]).abs() <= GROUNDED_TOLERANCE
    {
        let mut safe = native_origin;
        safe[2] = summary.end[2] + FLOOR_EPSILON;
        (safe, summary)
    } else {
        (native_origin, summary)
    }
}

fn apply_sm64_native_player_output(
    active: Option<Res<sm64_bevy::Sm64CodActive>>,
    output: Res<sm64_bevy::Sm64NativePlayerOutput>,
    mut bridge_state: ResMut<Sm64CodNativeState>,
    mut spawn: Option<ResMut<Sm64CodSpawn>>,
    mut authority: Option<ResMut<net::AuthorityWorld>>,
) {
    if active.is_none() {
        bridge_state.last_sm64_health = None;
        bridge_state.last_action = 0;
        bridge_state.last_area = 0;
        bridge_state.force_native_reposition = false;
        return;
    }
    let Some(authority) = authority.as_deref_mut() else {
        return;
    };

    let mut first = None;
    authority.0.visit_players(|id, player| {
        if first.is_none() {
            first = Some((id, *player));
        }
    });
    let Some((id, player)) = first else {
        return;
    };

    if !output.active {
        authority.0.set_external_motion(id, false);
        bridge_state.last_sm64_health = None;
        bridge_state.last_action = 0;
        bridge_state.last_area = 0;
        return;
    }

    // Preserve ordinary COD damage while layering SM64 damage/healing on top.
    // SM64's normal full health is 0x880, so apply the *delta* rather than
    // replacing COD health with an unrelated absolute scale every frame.
    //
    // Use full native health as the first baseline. If Mario is hit on the
    // first native frame we see, initializing the baseline to that already-
    // damaged value would silently discard the first enemy hit.
    let previous = bridge_state.last_sm64_health.unwrap_or(0x880);
    if output.health < 0x100 && player.health > 0 {
        authority.0.damage_from_environment(id, player.health);
    }
    {
        let sm64_delta = output.health - previous;
        if sm64_delta != 0 {
            let mut scaled = ((sm64_delta as f32) * (player.max_health.max(1) as f32)
                / 0x880 as f32)
                .round() as i32;

            // Never round real native contact damage away completely.
            if sm64_delta < 0 && scaled == 0 {
                scaled = -1;
            } else if sm64_delta > 0 && scaled == 0 {
                scaled = 1;
            }

            if scaled != 0 && player.health > 0 && output.health >= 0x100 {
                let next = player
                    .health
                    .saturating_add(scaled)
                    .clamp(0, player.max_health.max(1));
                if scaled < 0 {
                    authority
                        .0
                        .damage_from_environment(id, scaled.saturating_neg());
                } else {
                    authority.0.set_health(id, next);
                }
                diag::info!(
                    World,
                    "SM64 native health delta {} -> COD {} ({} -> {})",
                    sm64_delta,
                    scaled,
                    player.health,
                    next
                );
            }
        }
    }
    bridge_state.last_sm64_health = Some(output.health);

    let owns_motion = sm64_cod_native_owns_motion(output.action);
    let area_changed = bridge_state.last_area != 0
        && output.area_index != 0
        && output.area_index != bridge_state.last_area;
    let s = sm64_core::SM64_TO_IW4_SCALE;
    let native_origin = [
        output.sm64_pos[0] * s,
        -output.sm64_pos[2] * s,
        output.sm64_pos[1] * s,
    ];

    authority.0.set_external_motion(id, owns_motion);
    if owns_motion {
        let velocity = [
            output.sm64_vel[0] * s * sm64_sim::SM64_TICK_HZ as f32,
            -output.sm64_vel[2] * s * sm64_sim::SM64_TICK_HZ as f32,
            output.sm64_vel[1] * s * sm64_sim::SM64_TICK_HZ as f32,
        ];
        authority.0.set_origin(id, native_origin);
        authority.0.set_velocity(id, velocity);

        let sm64_yaw_degrees = output.sm64_yaw as u16 as f32 * 360.0 / 65536.0;
        let mut view = player.viewangles;
        view[1] = sm64_yaw_degrees - 90.0;
        authority.0.set_viewangles(id, view);
    } else if bridge_state.force_native_reposition
        || area_changed
        || sm64_cod_native_reposition_action(output.action)
    {
        let dx = native_origin[0] - player.origin[0];
        let dy = native_origin[1] - player.origin[1];
        let dz = native_origin[2] - player.origin[2];
        let distance2 = dx * dx + dy * dy + dz * dz;
        if bridge_state.force_native_reposition || area_changed || distance2 > 48.0 * 48.0 {
            let (teleport_origin, floor_probe) = if bridge_state.force_native_reposition {
                let (origin, probe) = sm64_safe_transition_origin(&authority.0, native_origin);
                (origin, Some(probe))
            } else {
                (native_origin, None)
            };
            authority.0.teleport(id, teleport_origin);
            authority.0.set_velocity(id, [0.0; 3]);

            if bridge_state.force_native_reposition {
                let sm64_yaw_degrees =
                    output.sm64_yaw as u16 as f32 * 360.0 / 65536.0;
                if let Some(spawn) = spawn.as_deref_mut() {
                    spawn.origin = teleport_origin;
                    spawn.view = [0.0, sm64_yaw_degrees - 90.0, 0.0];
                }

                if let Some(probe) = floor_probe {
                    diag::info!(
                        World,
                        "SM64 COD transition floor probe native={:?} chosen={:?} hit={} end={:?} normal={:?} startsolid={} allsolid={}",
                        native_origin,
                        teleport_origin,
                        probe.hit,
                        probe.end,
                        probe.normal,
                        probe.startsolid,
                        probe.allsolid
                    );
                }
            }

            bridge_state.force_native_reposition = false;
            diag::info!(
                World,
                "SM64 COD transition reposition area {} -> {} action=0x{:08x} native={:?} origin={:?}",
                bridge_state.last_area,
                output.area_index,
                output.action,
                native_origin,
                teleport_origin
            );
        }
    }
    if output.area_index != 0 {
        bridge_state.last_area = output.area_index;
    }

    if output.action != bridge_state.last_action {
        diag::info!(
            World,
            "SM64 native player action 0x{:08x} -> 0x{:08x} (external_motion={})",
            bridge_state.last_action,
            output.action,
            owns_motion
        );
        bridge_state.last_action = output.action;
    }
}

fn publish_sm64_native_audio(
    active: Option<Res<sm64_bevy::Sm64CodActive>>,
    frame: Res<sm64_bevy::Sm64NativeAudioFrame>,
    mut state: ResMut<Sm64CodAudioState>,
    mut chunks: MessageWriter<audio::ExternalPcmChunk>,
) {
    if active.is_none() {
        state.last_tick = 0;
        return;
    }
    if frame.tick == 0 || frame.tick == state.last_tick || frame.samples.is_empty() {
        return;
    }
    state.last_tick = frame.tick;
    chunks.write(audio::ExternalPcmChunk {
        // Stable stream identity for the currently installed SM64 runtime.
        // Match teardown resets the audio-side queue before another map starts.
        stream_id: 0x534D_3634,
        sample_rate: frame.sample_rate,
        channels: frame.channels,
        samples: std::sync::Arc::from(frame.samples.clone()),
    });
}

fn publish_sm64_cod_hud(
    active: Option<Res<sm64_bevy::Sm64CodActive>>,
    output: Res<sm64_bevy::Sm64NativePlayerOutput>,
    dialog: Res<sm64_bevy::Sm64NativeDialogOutput>,
    mut hud: ResMut<frame::Sm64HudView>,
) {
    if active.is_none() || !output.active {
        *hud = frame::Sm64HudView::default();
        return;
    }
    *hud = frame::Sm64HudView {
        active: true,
        health: output.health,
        coins: output.coins,
        dialog_id: dialog.id,
        dialog_text: dialog.text.clone(),
    };
}

fn suppress_sm64_player_view(
    mut commands: Commands,
    active: Option<Res<sm64_bevy::Sm64CodActive>>,
    mario: Query<Entity, With<sm64_bevy::Sm64MarioPresentation>>,
) {
    if active.is_none() {
        return;
    }

    // The sm64cod path never spawns Sm64DebugCamera. Do not delete arbitrary
    // Camera3d entities by render order here: the COD first-person/viewmodel
    // path owns its own overlay camera and was being removed along with Mario.
    for entity in &mario {
        commands.entity(entity).despawn();
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
    commands.insert_resource(Sm64CodAudioState::default());
    commands.insert_resource(Sm64CodMoveState::default());
    commands.remove_resource::<frame::ExternalWorldPresentation>();
    commands.remove_resource::<sm64_bevy::Sm64LaunchRequest>();
    commands.insert_resource(frame::Sm64HudView::default());
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

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
    last_native_static_tick: u32,
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
    last_platform_tick: u32,
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
    chain_grace_ticks: u8,
    ground_pounding: bool,
    last_weapon_shot_count: Option<i32>,
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

fn sm64_native_collision_vertices(
    triangles: &[[[f32; 3]; 3]],
) -> Vec<[f32; 3]> {
    let s = sm64_core::SM64_TO_IW4_SCALE;
    let mut verts = Vec::with_capacity(triangles.len() * 6);
    for tri in triangles {
        let v0 = [tri[0][0] * s, -tri[0][2] * s, tri[0][1] * s];
        let v1 = [tri[1][0] * s, -tri[1][2] * s, tri[1][1] * s];
        let v2 = [tri[2][0] * s, -tri[2][2] * s, tri[2][1] * s];

        // Match the handedness expected by clipmap_iw4, then duplicate the
        // reverse face so room walls/floors are solid from either side.
        verts.extend_from_slice(&[v0, v2, v1]);
        verts.extend_from_slice(&[v0, v1, v2]);
    }
    verts
}

fn install_sm64_cod_collision(
    authority: &mut sim::SimWorld,
    collision_state: &mut Sm64CodCollisionState,
    level: &str,
    area: u8,
    parsed: &sm64_assets::ParsedCollision,
) -> usize {
    let verts = sm64_static_collision_vertices(parsed, false);
    let vanish_verts = sm64_static_collision_vertices(parsed, true);
    let triangle_count = verts.len() / 3;

    collision_state.static_vertices = verts.clone();
    collision_state.normal_static_vertices = verts.clone();
    collision_state.vanish_static_vertices = vanish_verts;
    collision_state.vanish_active = false;
    collision_state.level.clear();
    collision_state.level.push_str(level);
    collision_state.area = area;
    collision_state.last_native_static_tick = 0;
    collision_state.last_dynamic_tick = 0;
    collision_state.last_dynamic_triangles.clear();

    let mesh = sim::SimClipMesh::from_linear_triangles(verts);
    let content = authority.content().with_clip_mesh(mesh);
    authority.install_content(content);

    triangle_count
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
                sync_sm64_cod_area_collision.after(sm64_bevy::Sm64RuntimeStep),
                apply_sm64_native_player_output.after(sm64_bevy::Sm64RuntimeStep),
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
        let installed_triangles = install_sm64_cod_collision(
            &mut authority.0,
            &mut collision_state,
            &level,
            area,
            &parsed,
        );

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
            installed_triangles,
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
    const DOUBLE_JUMP_VELOCITY: f32 = 390.0;
    const TRIPLE_JUMP_VELOCITY: f32 = 540.0;
    const GROUND_POUND_VELOCITY: f32 = -900.0;
    const JUMP_CHAIN_GRACE_TICKS: u8 = 12;
    const GROUND_POUND_AIM_DEGREES: f32 = 60.0;
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
    let just_landed = !state.last_grounded && grounded;
    let just_left_ground = state.last_grounded && !grounded;
    let weapon_shot = state
        .last_weapon_shot_count
        .is_some_and(|previous| player.weapon_shot_count != previous);
    let pitch = {
        let mut pitch = player.viewangles[0] % 360.0;
        if pitch > 180.0 {
            pitch -= 360.0;
        } else if pitch < -180.0 {
            pitch += 360.0;
        }
        pitch
    };
    let shoot_down_pound = weapon_shot && pitch >= GROUND_POUND_AIM_DEGREES;

    // Cannons/grabs/etc. truly own native motion.
    if player.health <= 0 || (output.active && sm64_cod_native_owns_motion(output.action)) {
        state.ground_pounding = false;
        state.jump_stage = 0;
        state.chain_grace_ticks = 0;
        state.last_jump_down = jump_down;
        state.last_pound_down = pound_down;
        state.last_grounded = grounded;
        state.last_weapon_shot_count = Some(player.weapon_shot_count);
        return;
    }

    if grounded {
        if state.ground_pounding {
            external.pending_ground_pound = true;
            diag::info!(World, "SM64 COD ground pound landed");
        }
        state.ground_pounding = false;

        if just_landed {
            if matches!(state.jump_stage, 1 | 2) {
                state.chain_grace_ticks = JUMP_CHAIN_GRACE_TICKS;
            } else if state.jump_stage >= 3 {
                state.jump_stage = 0;
                state.chain_grace_ticks = 0;
            }
        }

        if jump_pressed {
            state.jump_stage = if state.chain_grace_ticks > 0 {
                match state.jump_stage {
                    1 => 2,
                    2 => 3,
                    _ => 1,
                }
            } else {
                1
            };
            state.chain_grace_ticks = 0;
        } else if state.chain_grace_ticks > 0 {
            state.chain_grace_ticks -= 1;
        } else if state.last_grounded && !jump_down {
            state.jump_stage = 0;
        }
    } else {
        // COD pmove performs the actual takeoff. Once it has left the floor,
        // upgrade that takeoff if this was the second/third chained jump.
        if just_left_ground {
            let boost = match state.jump_stage {
                2 => Some(DOUBLE_JUMP_VELOCITY),
                3 => Some(TRIPLE_JUMP_VELOCITY),
                _ => None,
            };
            if let Some(vertical) = boost {
                let mut velocity = player.velocity;
                velocity[2] = velocity[2].max(vertical);
                authority.0.set_velocity(id, velocity);
                diag::info!(
                    World,
                    "SM64 COD {} jump vz={:.1}",
                    if state.jump_stage == 2 { "double" } else { "triple" },
                    velocity[2]
                );
            }
        }

        let pound_requested = pound_pressed || shoot_down_pound;
        if pound_requested && !state.ground_pounding {
            let mut velocity = player.velocity;
            velocity[0] *= 0.45;
            velocity[1] *= 0.45;
            velocity[2] = GROUND_POUND_VELOCITY;
            authority.0.set_velocity(id, velocity);
            state.ground_pounding = true;
            state.jump_stage = 0;
            state.chain_grace_ticks = 0;
            diag::info!(
                World,
                "SM64 COD ground pound started ({})",
                if shoot_down_pound { "aim-down fire" } else { "crouch/prone" }
            );
        } else if state.ground_pounding {
            let mut velocity = player.velocity;
            velocity[0] *= 0.9;
            velocity[1] *= 0.9;
            velocity[2] = velocity[2].min(GROUND_POUND_VELOCITY);
            authority.0.set_velocity(id, velocity);
        } else if output.active && (output.mario_flags & sm64_core::MARIO_WING_CAP) != 0 {
            let mut velocity = player.velocity;
            velocity[2] = if jump_down {
                velocity[2].max(WING_ASCENT_VELOCITY)
            } else {
                velocity[2].max(WING_GLIDE_FALL_VELOCITY)
            };
            authority.0.set_velocity(id, velocity);
        }
    }

    state.last_jump_down = jump_down;
    state.last_pound_down = pound_down;
    state.last_grounded = grounded;
    state.last_weapon_shot_count = Some(player.weapon_shot_count);
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
    mut native_static: ResMut<sm64_bevy::Sm64NativeStaticCollision>,
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
    native_static.tick = 0;
    native_static.triangles.clear();
    native_dynamic.tick = 0;
    native_dynamic.triangles.clear();

    /*
     * Do not install a source-parsed approximation of the destination here.
     * The fresh native DLL will export the exact static surfaces SM64 actually
     * loaded for the destination area on its first snapshot. Keep COD frozen
     * until that snapshot arrives; sync_sm64_cod_area_collision installs those
     * exact native surfaces before apply_sm64_native_player_output teleports the
     * player to Mario's resolved warp-node position.
     */
    collision_state.level.clear();
    collision_state.area = 0;
    collision_state.last_native_static_tick = 0;
    collision_state.last_dynamic_tick = 0;
    collision_state.last_dynamic_triangles.clear();

    if let Some(authority) = authority.as_deref_mut() {
        let mut first = None;
        authority.0.visit_players(|id, _| {
            if first.is_none() {
                first = Some(id);
            }
        });
        if let Some(id) = first {
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
        "SM64 COD host transition: {} -> {} area={} node={} arg={} (waiting for native static collision + Mario spawn)",
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
    native_static: Res<sm64_bevy::Sm64NativeStaticCollision>,
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
        || native_static.tick == 0
        || native_static.triangles.is_empty()
    {
        return;
    }

    let level_or_area_changed =
        collision_state.level != active.level || collision_state.area != area;
    if !level_or_area_changed && collision_state.last_native_static_tick != 0 {
        return;
    }

    let verts = sm64_native_collision_vertices(&native_static.triangles);
    let triangle_count = verts.len() / 3;

    collision_state.static_vertices = verts.clone();
    collision_state.normal_static_vertices = verts.clone();
    // Native snapshot currently exports exact geometry but not surface types.
    // Keep the exact geometry for vanish mode too rather than restoring stale
    // parser collision from another level/area.
    collision_state.vanish_static_vertices = verts.clone();
    collision_state.vanish_active = false;
    collision_state.level = active.level.clone();
    collision_state.area = area;
    collision_state.last_native_static_tick = native_static.tick;
    collision_state.last_dynamic_tick = 0;
    collision_state.last_dynamic_triangles.clear();

    let mesh = sim::SimClipMesh::from_linear_triangles(verts);
    let content = authority.0.content().with_clip_mesh(mesh);
    authority.0.install_content(content);

    diag::info!(
        World,
        "SM64 COD native collision installed: level={} area={} native_surfaces={} IW4_triangles={} tick={}",
        active.level,
        area,
        native_static.triangles.len(),
        triangle_count,
        native_static.tick
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

    let mut verts = collision_state.static_vertices.clone();
    verts.reserve(collision_state.last_dynamic_triangles.len() * 6);
    for tri in &collision_state.last_dynamic_triangles {
        let [v0, v1, v2] = sm64_dynamic_triangle_to_iw4(tri);
        verts.extend_from_slice(&[v0, v2, v1]);
        verts.extend_from_slice(&[v0, v1, v2]);
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

fn sm64_dynamic_triangle_to_iw4(tri: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let s = sm64_core::SM64_TO_IW4_SCALE;
    [
        [tri[0][0] * s, -tri[0][2] * s, tri[0][1] * s],
        [tri[1][0] * s, -tri[1][2] * s, tri[1][1] * s],
        [tri[2][0] * s, -tri[2][2] * s, tri[2][1] * s],
    ]
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

    let Some(authority) = authority.as_deref_mut() else {
        collision_state.last_dynamic_triangles = dynamic.triangles.clone();
        return;
    };

    collision_state.last_dynamic_triangles = dynamic.triangles.clone();

    let mut verts = collision_state.static_vertices.clone();
    verts.reserve(dynamic.triangles.len() * 6);

    for tri in &dynamic.triangles {
        let [v0, v1, v2] = sm64_dynamic_triangle_to_iw4(tri);

        // Keep moving platforms/doors solid from both sides as well.
        verts.extend_from_slice(&[v0, v2, v1]);
        verts.extend_from_slice(&[v0, v1, v2]);
    }

    let mesh = sim::SimClipMesh::from_linear_triangles(verts);
    let content = authority.0.content().with_clip_mesh(mesh);
    authority.0.install_content(content);
}

fn apply_sm64_native_player_output(
    active: Option<Res<sm64_bevy::Sm64CodActive>>,
    output: Res<sm64_bevy::Sm64NativePlayerOutput>,
    collision_state: Res<Sm64CodCollisionState>,
    mut bridge_state: ResMut<Sm64CodNativeState>,
    mut spawn: Option<ResMut<Sm64CodSpawn>>,
    mut authority: Option<ResMut<net::AuthorityWorld>>,
) {
    let Some(active) = active else {
        bridge_state.last_sm64_health = None;
        bridge_state.last_action = 0;
        bridge_state.last_area = 0;
        bridge_state.force_native_reposition = false;
        return;
    };
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
        // During a cross-level handoff the previous DLL has been dropped but
        // the destination DLL may not have produced its first 30 Hz snapshot
        // yet. Keep COD frozen at the source door until that snapshot exists.
        authority
            .0
            .set_external_motion(id, bridge_state.force_native_reposition);
        if bridge_state.force_native_reposition {
            authority.0.set_velocity(id, [0.0; 3]);
        }
        bridge_state.last_sm64_health = None;
        bridge_state.last_action = 0;
        bridge_state.last_area = 0;
        bridge_state.last_platform_tick = 0;
        return;
    }

    if bridge_state.force_native_reposition {
        let destination_ready = u8::try_from(output.area_index).ok().is_some_and(|area| {
            collision_state.level == active.level
                && collision_state.area == area
                && collision_state.last_native_static_tick != 0
        });

        if !destination_ready {
            /*
             * Critical transition barrier: never teleport from a snapshot
             * unless the exact native static collision installed for that same
             * destination level/area is already active. This prevents the last
             * Castle Grounds ACT_PUSHING_DOOR snapshot from being mistaken for
             * the first Castle Inside spawn snapshot.
             */
            authority.0.set_external_motion(id, true);
            authority.0.set_velocity(id, [0.0; 3]);
            return;
        }
    }

    // Preserve ordinary COD damage while layering SM64 damage/healing on top.
    // SM64's normal full health is 0x880, so apply the *delta* rather than
    // replacing COD health with an unrelated absolute scale every frame.
    //
    // Use full native health as the first baseline. If Mario is hit on the
    // first native frame we see, initializing the baseline to that already-
    // damaged value would silently discard the first enemy hit.
    let previous = bridge_state.last_sm64_health.unwrap_or(0x880);
    if output.health < 0x100 {
        /*
         * The native bridge hands Mario deaths to the course's authored
         * WARP_NODE_DEATH. Do not also kill the COD pawn here: that creates a
         * second independent respawn while the old native death action is still
         * alive, which is the death loop this mode used to enter.
         */
        bridge_state.last_sm64_health = Some(output.health);
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

    let mut carried_origin = player.origin;
    if !owns_motion
        && !bridge_state.force_native_reposition
        && output.platform_active
        && output.tick != 0
        && output.tick != bridge_state.last_platform_tick
    {
        let delta = [
            output.platform_displacement[0] * s,
            -output.platform_displacement[2] * s,
            output.platform_displacement[1] * s,
        ];

        // Native SM64 already computed this displacement from gMarioPlatform,
        // including platform translation and rotation. Apply it once, as a
        // nudge, without touching COD's own movement velocity.
        if delta[0].is_finite()
            && delta[1].is_finite()
            && delta[2].is_finite()
            && delta[0].abs() <= 256.0
            && delta[1].abs() <= 256.0
            && delta[2].abs() <= 256.0
        {
            authority.0.gate_nudge_origin(id, delta);
            carried_origin[0] += delta[0];
            carried_origin[1] += delta[1];
            carried_origin[2] += delta[2];
        }
        bridge_state.last_platform_tick = output.tick;
    } else if output.tick != 0 && output.tick != bridge_state.last_platform_tick && !output.platform_active {
        bridge_state.last_platform_tick = output.tick;
    }

    authority
        .0
        .set_external_motion(id, owns_motion || bridge_state.force_native_reposition);
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
        let dx = native_origin[0] - carried_origin[0];
        let dy = native_origin[1] - carried_origin[1];
        let dz = native_origin[2] - carried_origin[2];
        let distance2 = dx * dx + dy * dy + dz * dz;
        if bridge_state.force_native_reposition || area_changed || distance2 > 48.0 * 48.0 {
            /*
             * A cross-level door/painting has one authoritative spawn: Mario's
             * position from the freshly loaded native destination. Put COD
             * there exactly. No generic level spawn, no floor probe, no offset.
             */
            authority.0.teleport(id, native_origin);
            authority.0.set_velocity(id, [0.0; 3]);

            if bridge_state.force_native_reposition {
                let sm64_yaw_degrees =
                    output.sm64_yaw as u16 as f32 * 360.0 / 65536.0;
                let native_view = [0.0, sm64_yaw_degrees - 90.0, 0.0];
                authority.0.set_viewangles(id, native_view);
                if let Some(spawn) = spawn.as_deref_mut() {
                    spawn.origin = native_origin;
                    spawn.view = native_view;
                }
            }

            bridge_state.force_native_reposition = false;
            diag::info!(
                World,
                "SM64 COD transition: COD placed exactly at destination native Mario position level={} area={} action=0x{:08x} origin={:?} collision_tick={}",
                active.level,
                output.area_index,
                output.action,
                native_origin,
                collision_state.last_native_static_tick
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

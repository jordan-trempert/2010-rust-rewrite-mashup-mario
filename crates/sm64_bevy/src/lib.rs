use bevy::{
    asset::RenderAssetUsages,
    camera::ClearColorConfig,
    log::{error, info},
    prelude::*,
    render::render_resource::PrimitiveTopology,
};
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

#[derive(Component, Debug, Clone, Copy)]
struct Sm64DebugWorld;

#[derive(Component, Debug, Clone, Copy)]
struct Sm64DebugCamera;

#[derive(Resource, Debug, Clone, Copy)]
struct Sm64DebugView(pub bool);

impl Default for Sm64DebugView {
    fn default() -> Self {
        let enabled=std::env::var("SM64_DEBUG_VIEW")
            .map(|v|v!="0" && !v.eq_ignore_ascii_case("false"))
            .unwrap_or(true);
        Self(enabled)
    }
}

impl Plugin for Sm64Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Sm64Enabled>()
            .init_resource::<Sm64Runtime>()
            .init_resource::<Sm64ControllerInput>()
            .init_resource::<Sm64LoadStatus>()
            .init_resource::<Sm64DebugView>()
            .add_systems(Startup, load_sm64_from_decomp)
            .add_systems(
                Update,
                (
                    update_debug_input,
                    advance_sm64_runtime,
                    sync_mario_presentation.after(advance_sm64_runtime),
                    follow_mario_camera.after(advance_sm64_runtime),
                ),
            );
    }
}

fn load_sm64_from_decomp(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut enabled: ResMut<Sm64Enabled>,
    debug_view: Res<Sm64DebugView>,
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

    if debug_view.0 {
        spawn_debug_scene(
            &mut commands,
            &mut meshes,
            &mut materials,
            &parsed.world,
            [spawn.pos[0] as f32,spawn.pos[1] as f32,spawn.pos[2] as f32],
        );
    }

    runtime.world.collision=parsed.world;
    runtime.world.spawn_mario(
        [spawn.pos[0] as f32,spawn.pos[1] as f32,spawn.pos[2] as f32],
        spawn.yaw_sm64(),
    );
    runtime.world.mario.terrain_type=area_settings.terrain_type;
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

fn spawn_debug_scene(
    commands:&mut Commands,
    meshes:&mut Assets<Mesh>,
    materials:&mut Assets<StandardMaterial>,
    world:&sm64_core::CollisionWorld,
    mario_spawn:[f32;3],
) {
    let mut positions=Vec::<[f32;3]>::with_capacity(world.surfaces.len()*3);
    let mut normals=Vec::<[f32;3]>::with_capacity(world.surfaces.len()*3);

    for surface in &world.surfaces {
        positions.push([
            surface.vertex1[0] as f32,
            surface.vertex1[1] as f32,
            surface.vertex1[2] as f32,
        ]);
        positions.push([
            surface.vertex2[0] as f32,
            surface.vertex2[1] as f32,
            surface.vertex2[2] as f32,
        ]);
        positions.push([
            surface.vertex3[0] as f32,
            surface.vertex3[1] as f32,
            surface.vertex3[2] as f32,
        ]);
        let n=[surface.normal.x,surface.normal.y,surface.normal.z];
        normals.extend_from_slice(&[n,n,n]);
    }

    let mesh=Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION,positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL,normals);

    commands.spawn((
        Name::new("SM64 debug collision world"),
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color:Color::srgb(0.35,0.68,0.35),
            perceptual_roughness:1.0,
            unlit:true,
            cull_mode:None,
            ..default()
        })),
        Sm64DebugWorld,
    ));

    commands.spawn((
        Name::new("SM64 Mario debug marker"),
        Mesh3d(meshes.add(Cuboid::new(80.0,160.0,80.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color:Color::srgb(0.9,0.12,0.08),
            unlit:true,
            ..default()
        })),
        Transform::from_xyz(mario_spawn[0],mario_spawn[1]+80.0,mario_spawn[2]),
        Sm64MarioPresentation,
    ));

    let target=Vec3::new(mario_spawn[0],mario_spawn[1]+80.0,mario_spawn[2]);
    let camera_pos=target+Vec3::new(0.0,650.0,1200.0);
    commands.spawn((
        Name::new("SM64 debug camera"),
        Camera3d::default(),
        Camera {
            order:1000,
            clear_color:ClearColorConfig::Custom(Color::srgb(0.08,0.12,0.2)),
            ..default()
        },
        Transform::from_translation(camera_pos).looking_at(target,Vec3::Y),
        Sm64DebugCamera,
    ));
}

fn update_debug_input(
    keyboard:Res<ButtonInput<KeyCode>>,
    enabled:Res<Sm64Enabled>,
    debug_view:Res<Sm64DebugView>,
    mut input:ResMut<Sm64ControllerInput>,
) {
    if !enabled.0 || !debug_view.0 {return;}

    let x=i8::from(keyboard.pressed(KeyCode::KeyD))*64
        - i8::from(keyboard.pressed(KeyCode::KeyA))*64;
    let y=i8::from(keyboard.pressed(KeyCode::KeyS))*64
        - i8::from(keyboard.pressed(KeyCode::KeyW))*64;

    input.0=Sm64Input {
        stick_x:x.clamp(-64,64),
        stick_y:y.clamp(-64,64),
        camera_yaw:0,
        button_a:keyboard.pressed(KeyCode::Space),
        button_b:keyboard.pressed(KeyCode::KeyB),
        button_z:keyboard.pressed(KeyCode::KeyZ),
        button_start:keyboard.pressed(KeyCode::Enter),
    };
}

fn advance_sm64_runtime(
    time: Res<Time>,
    enabled: Res<Sm64Enabled>,
    input: Res<Sm64ControllerInput>,
    mut runtime: ResMut<Sm64Runtime>,
) {
    if !enabled.0 { return; }
    runtime.accumulator += time.delta_secs_f64();
    let mut steps=0;
    while runtime.accumulator >= SM64_TICK_SECONDS && steps < 8 {
        runtime.accumulator -= SM64_TICK_SECONDS;
        runtime.latest=Some(runtime.world.step(input.0));
        steps += 1;
    }
}

fn sync_mario_presentation(
    runtime:Res<Sm64Runtime>,
    mut query:Query<&mut Transform,With<Sm64MarioPresentation>>,
) {
    let Some(snapshot)=runtime.latest.as_ref() else {return;};
    for mut transform in &mut query {
        transform.translation=Vec3::new(
            snapshot.mario.pos[0],
            snapshot.mario.pos[1]+80.0,
            snapshot.mario.pos[2],
        );
        let yaw=snapshot.mario.face_angle[1] as u16 as f32
            * core::f32::consts::TAU / 65536.0;
        transform.rotation=Quat::from_rotation_y(yaw);
    }
}

fn follow_mario_camera(
    runtime:Res<Sm64Runtime>,
    mut query:Query<&mut Transform,With<Sm64DebugCamera>>,
) {
    let Some(snapshot)=runtime.latest.as_ref() else {return;};
    let yaw=snapshot.mario.face_angle[1];
    let forward=Vec3::new(sm64_core::math::sins(yaw),0.0,sm64_core::math::coss(yaw));
    let target=Vec3::new(
        snapshot.mario.pos[0],
        snapshot.mario.pos[1]+100.0,
        snapshot.mario.pos[2],
    );
    let camera_pos=target-forward*1200.0+Vec3::Y*650.0;
    for mut transform in &mut query {
        *transform=Transform::from_translation(camera_pos).looking_at(target,Vec3::Y);
    }
}

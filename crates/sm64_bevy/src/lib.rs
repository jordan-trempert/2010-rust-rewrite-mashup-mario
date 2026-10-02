use bevy::{
    asset::RenderAssetUsages,
    camera::ClearColorConfig,
    log::{error, info, warn},
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

#[derive(Resource, Debug, Clone)]
pub struct Sm64LaunchRequest {
    pub level: String,
    pub area: u8,
}

impl Sm64LaunchRequest {
    pub fn new(level: impl Into<String>, area: u8) -> Self {
        Self { level: level.into(), area: area.max(1) }
    }

    pub fn from_map_key(map: &str) -> Option<Self> {
        let level=map.strip_prefix("sm64:")?;
        (!level.is_empty()).then(||Self::new(level,1))
    }
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
            level: String::new(),
            area: 1,
            source_root: None,
            loaded: false,
            message: "No SM64 map loaded".to_owned(),
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
            .add_systems(
                Update,
                (
                    launch_requested_sm64_map,
                    update_debug_input.after(launch_requested_sm64_map),
                    advance_sm64_runtime.after(launch_requested_sm64_map),
                    sync_mario_presentation.after(advance_sm64_runtime),
                    follow_mario_camera.after(advance_sm64_runtime),
                ),
            );
    }
}

fn launch_requested_sm64_map(
    mut commands: Commands,
    request: Option<Res<Sm64LaunchRequest>>,
    existing: Query<
        Entity,
        Or<(
            With<Sm64DebugWorld>,
            With<Sm64MarioPresentation>,
            With<Sm64DebugCamera>,
        )>,
    >,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut enabled: ResMut<Sm64Enabled>,
    debug_view: Res<Sm64DebugView>,
    mut runtime: ResMut<Sm64Runtime>,
    mut status: ResMut<Sm64LoadStatus>,
) {
    let Some(request)=request else {return;};

    for entity in &existing {
        commands.entity(entity).despawn();
    }

    let Some(root)=std::env::var_os("SM64_DECOMP_ROOT").map(std::path::PathBuf::from) else {
        enabled.0=false;
        status.loaded=false;
        status.message="SM64_DECOMP_ROOT is not configured".into();
        error!("{}",status.message);
        commands.remove_resource::<Sm64LaunchRequest>();
        return;
    };

    let level=request.level.clone();
    let area=request.area;
    status.source_root=Some(root.clone());
    status.level=level.clone();
    status.area=area;
    status.loaded=false;

    let parsed=match sm64_assets::load_level_collision(&root,&level,area) {
        Ok(parsed)=>parsed,
        Err(error)=>{
            enabled.0=false;
            status.message=format!("SM64 collision load failed: {error}");
            error!("{}",status.message);
            commands.remove_resource::<Sm64LaunchRequest>();
            return;
        }
    };
    let spawn=match sm64_assets::load_mario_spawn(&root,&level,area) {
        Ok(spawn)=>spawn,
        Err(error)=>{
            enabled.0=false;
            status.message=format!("SM64 spawn load failed: {error}");
            error!("{}",status.message);
            commands.remove_resource::<Sm64LaunchRequest>();
            return;
        }
    };
    let area_settings=match sm64_assets::load_area_settings(&root,&level,area) {
        Ok(settings)=>settings,
        Err(error)=>{
            enabled.0=false;
            status.message=format!("SM64 area metadata load failed: {error}");
            error!("{}",status.message);
            commands.remove_resource::<Sm64LaunchRequest>();
            return;
        }
    };

    let render_geometry=match sm64_assets::load_level_render_geometry(&root,&level,area) {
        Ok(geometry)=>Some(geometry),
        Err(error)=>{
            warn!("SM64 render geometry load failed; using collision fallback: {error}");
            None
        }
    };

    let surface_count=parsed.world.surfaces.len();
    let special_count=parsed.specials.len();
    let render_triangles=render_geometry.as_ref().map_or(0,|g|g.triangle_count);
    let render_batches=render_geometry.as_ref().map_or(0,|g|g.batches.len());

    if debug_view.0 {
        spawn_debug_scene(
            &mut commands,
            &mut meshes,
            &mut materials,
            &parsed.world,
            render_geometry.as_ref(),
            [spawn.pos[0] as f32,spawn.pos[1] as f32,spawn.pos[2] as f32],
        );
    }

    runtime.world=Sm64World::new();
    runtime.world.collision=parsed.world;
    runtime.world.spawn_mario(
        [spawn.pos[0] as f32,spawn.pos[1] as f32,spawn.pos[2] as f32],
        spawn.yaw_sm64(),
    );
    runtime.world.mario.terrain_type=area_settings.terrain_type;
    runtime.accumulator=0.0;
    runtime.latest=Some(runtime.world.snapshot());
    enabled.0=std::env::var("SM64_ENABLED")
        .map(|v|v!="0" && !v.eq_ignore_ascii_case("false"))
        .unwrap_or(true);

    status.loaded=true;
    status.message=format!(
        "SM64 {level} area {area}: {surface_count} collision surfaces, {special_count} special objects, {render_triangles} render triangles in {render_batches} batches, Mario at {:?}",
        spawn.pos
    );
    info!("{}",status.message);
    commands.remove_resource::<Sm64LaunchRequest>();
}

fn spawn_debug_scene(
    commands:&mut Commands,
    meshes:&mut Assets<Mesh>,
    materials:&mut Assets<StandardMaterial>,
    world:&sm64_core::CollisionWorld,
    render_geometry:Option<&sm64_assets::ParsedRenderGeometry>,
    mario_spawn:[f32;3],
) {
    if let Some(geometry)=render_geometry.filter(|geometry|geometry.triangle_count>0) {
        spawn_display_list_geometry(commands,meshes,materials,geometry);
    } else {
        spawn_collision_fallback(commands,meshes,materials,world);
    }

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

fn spawn_display_list_geometry(
    commands:&mut Commands,
    meshes:&mut Assets<Mesh>,
    materials:&mut Assets<StandardMaterial>,
    geometry:&sm64_assets::ParsedRenderGeometry,
) {
    for (batch_index,batch) in geometry.batches.iter().enumerate() {
        if batch.vertices.len()<3 {continue;}

        let mut positions=Vec::<[f32;3]>::with_capacity(batch.vertices.len());
        let mut normals=Vec::<[f32;3]>::with_capacity(batch.vertices.len());
        let mut uvs=Vec::<[f32;2]>::with_capacity(batch.vertices.len());

        for triangle in batch.vertices.chunks_exact(3) {
            let a=Vec3::from_array(triangle[0].position);
            let b=Vec3::from_array(triangle[1].position);
            let c=Vec3::from_array(triangle[2].position);
            let normal=(b-a).cross(c-a).try_normalize().unwrap_or(Vec3::Y).to_array();

            for vertex in triangle {
                positions.push(vertex.position);
                normals.push(normal);
                let [width,height]=batch.texture_size.unwrap_or([32,32]);
                let u=vertex.texcoord[0] as f32/(32.0*width.max(1) as f32);
                let v=vertex.texcoord[1] as f32/(32.0*height.max(1) as f32);
                uvs.push([u,v]);
            }
        }

        let mesh=Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION,positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL,normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0,uvs);

        let base_color=debug_texture_color(batch.texture_symbol.as_deref(),batch_index);
        commands.spawn((
            Name::new(format!(
                "SM64 display list batch {} {}",
                batch_index,
                batch.texture_symbol.as_deref().unwrap_or("untextured")
            )),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color,
                perceptual_roughness:1.0,
                unlit:true,
                cull_mode:None,
                ..default()
            })),
            Sm64DebugWorld,
        ));
    }
}

fn spawn_collision_fallback(
    commands:&mut Commands,
    meshes:&mut Assets<Mesh>,
    materials:&mut Assets<StandardMaterial>,
    world:&sm64_core::CollisionWorld,
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
        Name::new("SM64 collision fallback"),
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
}

fn debug_texture_color(symbol:Option<&str>,batch_index:usize)->Color {
    let mut hash=0x811C9DC5u32;
    for byte in symbol.unwrap_or("untextured").bytes() {
        hash^=byte as u32;
        hash=hash.wrapping_mul(0x01000193);
    }
    hash^=batch_index as u32;
    let channel=|shift:u32| 0.28+(((hash>>shift)&0xFF) as f32/255.0)*0.58;
    Color::srgb(channel(0),channel(8),channel(16))
}

fn update_debug_input(
    keyboard:Res<ButtonInput<KeyCode>>,
    enabled:Res<Sm64Enabled>,
    debug_view:Res<Sm64DebugView>,
    mut input:ResMut<Sm64ControllerInput>,
) {
    if !enabled.0 || !debug_view.0 {return;}

    let x=(keyboard.pressed(KeyCode::KeyD) as i8)*64
        - (keyboard.pressed(KeyCode::KeyA) as i8)*64;
    let y=(keyboard.pressed(KeyCode::KeyS) as i8)*64
        - (keyboard.pressed(KeyCode::KeyW) as i8)*64;

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

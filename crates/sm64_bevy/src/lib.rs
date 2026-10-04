use std::collections::HashMap;

use bevy::{
    asset::RenderAssetUsages,
    camera::ClearColorConfig,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    log::{error, info, warn},
    prelude::*,
    render::render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat},
};
use sm64_sim::{SM64_TICK_SECONDS, Sm64Input, Sm64Snapshot, Sm64World};

const MARIO_MODEL_ROOTS:&[(&str,&str)]=&[
    ("MARIO_BUTT","mario_butt"),
    ("MARIO_TORSO","mario_torso"),
    ("MARIO_HEAD","mario_cap_on_eyes_front"),
    ("MARIO_LEFT_ARM","mario_left_arm"),
    ("MARIO_LEFT_FOREARM","mario_left_forearm_shared_dl"),
    ("MARIO_LEFT_HAND","mario_left_hand_closed"),
    ("MARIO_RIGHT_ARM","mario_right_arm"),
    ("MARIO_RIGHT_FOREARM","mario_right_forearm_shared_dl"),
    ("MARIO_RIGHT_HAND","mario_right_hand_closed"),
    ("MARIO_LEFT_THIGH","mario_left_thigh"),
    ("MARIO_LEFT_LEG","mario_left_leg_shared_dl"),
    ("MARIO_LEFT_FOOT","mario_left_foot"),
    ("MARIO_RIGHT_THIGH","mario_right_thigh"),
    ("MARIO_RIGHT_LEG","mario_right_leg_shared_dl"),
    ("MARIO_RIGHT_FOOT","mario_right_foot"),
];


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

#[derive(Resource, Debug, Clone)]
pub struct Sm64CodMapRequest {
    pub level: String,
    pub area: u8,
}

impl Sm64CodMapRequest {
    pub fn new(level: impl Into<String>, area: u8) -> Self {
        Self { level: level.into(), area: area.max(1) }
    }

    pub fn from_map_key(map: &str) -> Option<Self> {
        let level=map.strip_prefix("sm64cod:")
            .or_else(||map.strip_prefix("sm64:"))?;
        (!level.is_empty()).then(||Self::new(level,1))
    }

    pub fn map_key(&self) -> String {
        format!("sm64cod:{}",self.level)
    }
}

#[derive(Resource, Debug, Clone)]
pub struct Sm64CodActive {
    pub level: String,
    pub area: u8,
}

#[derive(Resource, Default)]
pub struct Sm64Runtime {
    pub world: Sm64World,
    accumulator: f64,
    pub latest: Option<Sm64Snapshot>,
}

#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct Sm64ControllerInput(pub Sm64Input);

#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct Sm64ExternalPlayer {
    pub sm64_pos: [f32;3],
    /// SM64-coordinate velocity in units per original 30 Hz tick.
    pub sm64_vel: [f32;3],
    /// SM64 binary-angle yaw used by object behaviors that face/track Mario.
    pub sm64_yaw: i16,
    pub health: i32,
    pub active: bool,
}

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
struct Sm64ObjectPresentation {
    id: sm64_core::ObjectId,
}

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
            .init_resource::<Sm64ExternalPlayer>()
            .init_resource::<Sm64LoadStatus>()
            .init_resource::<Sm64DebugView>()
            .add_systems(
                Update,
                (
                    launch_requested_sm64_map,
                    update_debug_input.after(launch_requested_sm64_map),
                    advance_sm64_runtime.after(launch_requested_sm64_map),
                    sync_mario_presentation.after(advance_sm64_runtime),
                    sync_object_presentations.after(advance_sm64_runtime),
                    follow_mario_camera.after(advance_sm64_runtime),
                ),
            );
    }
}

fn launch_requested_sm64_map(
    mut commands: Commands,
    request: Option<Res<Sm64LaunchRequest>>,
    cod_active: Option<Res<Sm64CodActive>>,
    existing: Query<
        Entity,
        Or<(
            With<Sm64DebugWorld>,
            With<Sm64MarioPresentation>,
            With<Sm64ObjectPresentation>,
            With<Sm64DebugCamera>,
        )>,
    >,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
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
    let texture_sources=match sm64_assets::load_texture_sources(&root,&level) {
        Ok(sources)=>sources,
        Err(error)=>{
            warn!("SM64 texture source resolution failed: {error}");
            HashMap::new()
        }
    };
    let mario_hierarchy=sm64_assets::resolve_geo_model_parts(&root,"mario_geo")
        .map_err(|error|{
            warn!("SM64 Mario GeoLayout hierarchy failed; using legacy part placement: {error}");
            error
        })
        .ok();
    let mario_geometry=match sm64_assets::load_model_display_lists(
        &root,
        "actors/mario/model.inc.c",
        MARIO_MODEL_ROOTS,
    ) {
        Ok(geometry)=>Some(geometry),
        Err(error)=>{
            warn!("SM64 Mario model load failed; using cuboid fallback: {error}");
            None
        }
    };
    let mario_texture_sources=sm64_assets::load_actor_texture_sources(&root,"mario")
        .unwrap_or_else(|error|{
            warn!("SM64 Mario texture resolution failed: {error}");
            HashMap::new()
        });
    let object_spawns=sm64_assets::load_level_object_spawns(&root,&level,area)
        .unwrap_or_else(|error|{
            warn!("SM64 object spawn load failed: {error}");
            sm64_assets::ParsedObjectSpawns::default()
        });
    let behavior_lists=sm64_assets::load_behavior_object_lists(&root)
        .unwrap_or_else(|error|{
            warn!("SM64 behavior list load failed: {error}");
            HashMap::new()
        });
    let selected_act=std::env::var("SM64_ACT")
        .ok()
        .and_then(|value|value.parse::<u8>().ok())
        .filter(|value|(1..=6).contains(value))
        .unwrap_or(1);

    let surface_count=parsed.world.surfaces.len();
    let special_count=parsed.specials.len();
    let render_triangles=render_geometry.as_ref().map_or(0,|g|g.triangle_count);
    let render_batches=render_geometry.as_ref().map_or(0,|g|g.batches.len());

    if debug_view.0 || cod_active.is_some() {
        spawn_debug_scene(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            &parsed.world,
            render_geometry.as_ref(),
            &texture_sources,
            mario_hierarchy.as_ref(),
            mario_geometry.as_ref(),
            &mario_texture_sources,
            [spawn.pos[0] as f32,spawn.pos[1] as f32,spawn.pos[2] as f32],
            debug_view.0 && cod_active.is_none(),
        );
    }

    runtime.world=Sm64World::new();
    runtime.world.collision=parsed.world;
    runtime.world.spawn_mario(
        [spawn.pos[0] as f32,spawn.pos[1] as f32,spawn.pos[2] as f32],
        spawn.yaw_sm64(),
    );
    runtime.world.mario.terrain_type=area_settings.terrain_type;
    runtime.world.clear_objects();
    populate_sm64_objects(
        &mut runtime.world,
        &object_spawns,
        &behavior_lists,
        selected_act,
    );
    if debug_view.0 || cod_active.is_some() {
        spawn_runtime_object_presentations(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            &root,
            &level,
            &runtime.world.objects,
        );
    }
    runtime.accumulator=0.0;
    runtime.latest=Some(runtime.world.snapshot());
    enabled.0=std::env::var("SM64_ENABLED")
        .map(|v|v!="0" && !v.eq_ignore_ascii_case("false"))
        .unwrap_or(true);

    status.loaded=true;
    status.message=format!(
        "SM64 {level} area {area} act {selected_act}: {surface_count} collision surfaces, {special_count} special collision objects, {render_triangles} render triangles in {render_batches} batches, {} runtime objects, Mario at {:?}",
        runtime.world.objects.len(),
        spawn.pos
    );
    info!("{}",status.message);
    commands.remove_resource::<Sm64LaunchRequest>();
}

fn populate_sm64_objects(
    world:&mut Sm64World,
    spawns:&sm64_assets::ParsedObjectSpawns,
    behavior_lists:&HashMap<String,String>,
    selected_act:u8,
) {
    let act_bit=1u8<<(selected_act.saturating_sub(1));

    for spawn in &spawns.level_objects {
        if spawn.act_mask & act_bit == 0 {continue;}
        let list=behavior_lists.get(&spawn.behavior)
            .map_or(sm64_core::ObjectList::Default,|symbol|sm64_core::ObjectList::from_symbol(symbol));
        let id=world.spawn_object(
            spawn.model.clone(),
            spawn.behavior.clone(),
            list,
            [
                spawn.position[0] as f32,
                spawn.position[1] as f32,
                spawn.position[2] as f32,
            ],
            [
                degrees_to_sm64_angle(spawn.angles_deg[0]),
                degrees_to_sm64_angle(spawn.angles_deg[1]),
                degrees_to_sm64_angle(spawn.angles_deg[2]),
            ],
        );
        if let Some(object)=world.object_mut(id) {
            object.behavior_params_expr=spawn.behavior_param_expr.clone();
        }
    }

    for spawn in &spawns.macro_objects {
        let Some(definition)=spawn.resolved.as_ref() else {continue;};
        let list=behavior_lists.get(&definition.behavior)
            .map_or(sm64_core::ObjectList::Default,|symbol|sm64_core::ObjectList::from_symbol(symbol));
        let id=world.spawn_object(
            definition.model.clone(),
            definition.behavior.clone(),
            list,
            [
                spawn.position[0] as f32,
                spawn.position[1] as f32,
                spawn.position[2] as f32,
            ],
            [0,degrees_to_sm64_angle(spawn.yaw_deg),0],
        );
        if let Some(object)=world.object_mut(id) {
            object.behavior_params_expr=spawn.behavior_param_expr.clone()
                .unwrap_or_else(||definition.default_behavior_param_expr.clone());
        }
    }
}

#[inline]
fn spawn_runtime_object_presentations(
    commands:&mut Commands,
    meshes:&mut Assets<Mesh>,
    materials:&mut Assets<StandardMaterial>,
    images:&mut Assets<Image>,
    decomp_root:&std::path::Path,
    level:&str,
    objects:&[sm64_core::Sm64Object],
) {
    let registry=match sm64_assets::load_model_registry(decomp_root,level) {
        Ok(registry)=>registry,
        Err(error)=>{
            warn!("SM64 model registry load failed: {error}");
            return;
        }
    };
    let mut geo_cache=HashMap::<
        String,
        (sm64_assets::ResolvedGeoModel,HashMap<String,std::path::PathBuf>)
    >::new();
    let mut dl_cache=HashMap::<
        String,
        (sm64_assets::ParsedRenderGeometry,HashMap<String,std::path::PathBuf>)
    >::new();

    for object in objects {
        if object.model=="MODEL_NONE" {continue;}
        let Some(source)=registry.get(&object.model) else {
            warn!("SM64 object model {} is not registered",object.model);
            continue;
        };

        let root=commands.spawn((
            Name::new(format!("SM64 object {} {}",object.id.0,object.model)),
            Transform::from_translation(sm64_render_vec3(object.pos)),
            Visibility::default(),
            Sm64ObjectPresentation{id:object.id},
        )).id();

        match source {
            sm64_assets::ModelSource::Geo{geo_symbol}=>{
                if !geo_cache.contains_key(&object.model) {
                    match sm64_assets::resolve_geo_model_parts(decomp_root,geo_symbol) {
                        Ok(model)=>{
                            let textures=sm64_assets::load_actor_texture_sources(
                                decomp_root,&model.actor_name
                            ).unwrap_or_default();
                            geo_cache.insert(object.model.clone(),(model,textures));
                        }
                        Err(error)=>{
                            warn!(
                                "SM64 object model {} ({geo_symbol}) failed: {error}",
                                object.model
                            );
                            commands.entity(root).despawn();
                            continue;
                        }
                    }
                }
                let Some((model,textures))=geo_cache.get(&object.model) else {continue;};
                spawn_hierarchical_object_geometry(
                    commands,meshes,materials,images,root,model,textures,
                );
            }
            sm64_assets::ModelSource::DisplayList{display_list,layer}=>{
                if !dl_cache.contains_key(&object.model) {
                    match sm64_assets::resolve_display_list_model_geometry(
                        decomp_root,level,display_list,layer,
                    ) {
                        Ok(resolved)=>{
                            dl_cache.insert(object.model.clone(),resolved);
                        }
                        Err(error)=>{
                            warn!(
                                "SM64 display-list model {} ({display_list}) failed: {error}",
                                object.model
                            );
                            commands.entity(root).despawn();
                            continue;
                        }
                    }
                }
                let Some((geometry,textures))=dl_cache.get(&object.model) else {continue;};
                spawn_flat_object_geometry(
                    commands,meshes,materials,images,root,geometry,textures,
                );
            }
        }
    }
}
fn spawn_hierarchical_object_geometry(
    commands:&mut Commands,
    meshes:&mut Assets<Mesh>,
    materials:&mut Assets<StandardMaterial>,
    images:&mut Assets<Image>,
    parent:Entity,
    model:&sm64_assets::ResolvedGeoModel,
    texture_sources:&HashMap<String,std::path::PathBuf>,
) {
    let mut texture_cache=HashMap::<String,Handle<Image>>::new();
    for (part_index,part) in model.parts.iter().enumerate() {
      for (batch_index,batch) in part.geometry.batches.iter().enumerate() {
        if batch.vertices.len()<3 {continue;}
        let mut positions=Vec::with_capacity(batch.vertices.len());
        let mut normals=Vec::with_capacity(batch.vertices.len());
        let mut uvs=Vec::with_capacity(batch.vertices.len());
        for triangle in batch.vertices.chunks_exact(3) {
            let a=sm64_render_vec3(triangle[0].position);
            let b=sm64_render_vec3(triangle[1].position);
            let d=sm64_render_vec3(triangle[2].position);
            let n=(b-a).cross(d-a).try_normalize().unwrap_or(Vec3::Y).to_array();
            for vertex in triangle {
                positions.push(sm64_render_pos(vertex.position));
                normals.push(n);
                let [w,h]=batch.texture_size.unwrap_or([32,32]);
                uvs.push([
                    vertex.texcoord[0] as f32/(32.0*w.max(1) as f32),
                    vertex.texcoord[1] as f32/(32.0*h.max(1) as f32),
                ]);
            }
        }
        let mesh=Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD|RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION,positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL,normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0,uvs);

        let texture=batch.texture_symbol.as_ref().and_then(|symbol|{
            if let Some(handle)=texture_cache.get(symbol){return Some(handle.clone());}
            let path=texture_sources.get(symbol)?;
            let handle=load_sm64_png(path,images).ok()?;
            texture_cache.insert(symbol.clone(),handle.clone());
            Some(handle)
        });
        commands.spawn((
            Name::new(format!("SM64 object part {part_index} mesh {batch_index}")),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial{
                base_color:Color::WHITE,
                base_color_texture:texture,
                unlit:true,
                cull_mode:None,
                alpha_mode:if batch.layer.contains("TRANSPARENT"){AlphaMode::Blend}
                    else if batch.layer.contains("ALPHA"){AlphaMode::Mask(0.5)}
                    else{AlphaMode::Opaque},
                ..default()
            })),
            Transform {
                translation:sm64_render_vec3(part.spec.translation),
                rotation:sm64_render_rotation(part.spec.rotation_deg),
                scale:Vec3::splat(part.spec.scale),
            },
            ChildOf(parent),
            Sm64DebugWorld,
        ));
      }
    }
}

fn spawn_flat_object_geometry(
    commands:&mut Commands,
    meshes:&mut Assets<Mesh>,
    materials:&mut Assets<StandardMaterial>,
    images:&mut Assets<Image>,
    parent:Entity,
    geometry:&sm64_assets::ParsedRenderGeometry,
    texture_sources:&HashMap<String,std::path::PathBuf>,
) {
    let mut texture_cache=HashMap::<String,Handle<Image>>::new();
    for (batch_index,batch) in geometry.batches.iter().enumerate() {
        if batch.vertices.len()<3 {continue;}
        let mut positions=Vec::with_capacity(batch.vertices.len());
        let mut normals=Vec::with_capacity(batch.vertices.len());
        let mut uvs=Vec::with_capacity(batch.vertices.len());
        for triangle in batch.vertices.chunks_exact(3) {
            let a=sm64_render_vec3(triangle[0].position);
            let b=sm64_render_vec3(triangle[1].position);
            let d=sm64_render_vec3(triangle[2].position);
            let n=(b-a).cross(d-a).try_normalize().unwrap_or(Vec3::Z).to_array();
            for vertex in triangle {
                positions.push(sm64_render_pos(vertex.position));
                normals.push(n);
                let [w,h]=batch.texture_size.unwrap_or([32,32]);
                uvs.push([
                    vertex.texcoord[0] as f32/(32.0*w.max(1) as f32),
                    vertex.texcoord[1] as f32/(32.0*h.max(1) as f32),
                ]);
            }
        }
        let mesh=Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD|RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION,positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL,normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0,uvs);

        let texture=batch.texture_symbol.as_ref().and_then(|symbol|{
            if let Some(handle)=texture_cache.get(symbol){return Some(handle.clone());}
            let path=texture_sources.get(symbol)?;
            let handle=load_sm64_png(path,images).ok()?;
            texture_cache.insert(symbol.clone(),handle.clone());
            Some(handle)
        });

        commands.spawn((
            Name::new(format!("SM64 flat object mesh {batch_index}")),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial{
                base_color:Color::WHITE,
                base_color_texture:texture,
                unlit:true,
                alpha_mode:if batch.layer.contains("TRANSPARENT"){AlphaMode::Blend}
                    else if batch.layer.contains("ALPHA"){AlphaMode::Mask(0.5)}
                    else{AlphaMode::Opaque},
                ..default()
            })),
            Transform::IDENTITY,
            ChildOf(parent),
            Sm64DebugWorld,
        ));
    }
}

fn degrees_to_sm64_angle(degrees:i16)->i16 {
    (((degrees as i32)*0x10000/360) as u16) as i16
}

fn spawn_debug_scene(
    commands:&mut Commands,
    meshes:&mut Assets<Mesh>,
    materials:&mut Assets<StandardMaterial>,
    images:&mut Assets<Image>,
    world:&sm64_core::CollisionWorld,
    render_geometry:Option<&sm64_assets::ParsedRenderGeometry>,
    texture_sources:&HashMap<String,std::path::PathBuf>,
    mario_hierarchy:Option<&sm64_assets::ResolvedGeoModel>,
    mario_geometry:Option<&sm64_assets::ParsedRenderGeometry>,
    mario_texture_sources:&HashMap<String,std::path::PathBuf>,
    mario_spawn:[f32;3],
    spawn_debug_player:bool,
) {
    if let Some(geometry)=render_geometry.filter(|geometry|geometry.triangle_count>0) {
        spawn_display_list_geometry(
            commands,
            meshes,
            materials,
            images,
            geometry,
            texture_sources,
        );
    } else {
        spawn_collision_fallback(commands,meshes,materials,world);
    }

    if spawn_debug_player {
        let mario_root=commands.spawn((
            Name::new("SM64 Mario"),
            Transform::from_xyz(mario_spawn[0],mario_spawn[1],mario_spawn[2]),
            Visibility::default(),
            Sm64MarioPresentation,
        )).id();
    
        if let Some(model)=mario_hierarchy.filter(|model|!model.parts.is_empty()) {
            spawn_mario_hierarchy(
                commands,
                meshes,
                materials,
                images,
                mario_root,
                model,
                mario_texture_sources,
            );
        } else if let Some(geometry)=mario_geometry.filter(|geometry|geometry.triangle_count>0) {
            spawn_mario_geometry(
                commands,
                meshes,
                materials,
                images,
                mario_root,
                geometry,
                mario_texture_sources,
            );
        } else {
            commands.spawn((
                Name::new("SM64 Mario fallback"),
                Mesh3d(meshes.add(Cuboid::new(80.0,160.0,80.0))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color:Color::srgb(0.9,0.12,0.08),
                    unlit:true,
                    ..default()
                })),
                Transform::from_xyz(0.0,80.0,0.0),
                ChildOf(mario_root),
                Sm64DebugWorld,
            ));
        }
    
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
}

#[inline]
fn sm64_render_pos(v:[f32;3])->[f32;3] {
    [v[0],-v[2],v[1]]
}

#[inline]
fn sm64_render_vec3(v:[f32;3])->Vec3 {
    Vec3::new(v[0],-v[2],v[1])
}

fn sm64_render_rotation(degrees:[f32;3])->Quat {
    let old=Quat::from_euler(
        EulerRot::XYZ,
        degrees[0].to_radians(),
        degrees[1].to_radians(),
        degrees[2].to_radians(),
    );
    let basis=Mat3::from_cols(
        Vec3::X,
        Vec3::Z,
        -Vec3::Y,
    );
    Quat::from_mat3(&(basis * Mat3::from_quat(old) * basis))
}

fn spawn_display_list_geometry(
    commands:&mut Commands,
    meshes:&mut Assets<Mesh>,
    materials:&mut Assets<StandardMaterial>,
    images:&mut Assets<Image>,
    geometry:&sm64_assets::ParsedRenderGeometry,
    texture_sources:&HashMap<String,std::path::PathBuf>,
) {
    let mut texture_cache=HashMap::<String,Handle<Image>>::new();
    for (batch_index,batch) in geometry.batches.iter().enumerate() {
        if batch.vertices.len()<3 {continue;}

        let mut positions=Vec::<[f32;3]>::with_capacity(batch.vertices.len());
        let mut normals=Vec::<[f32;3]>::with_capacity(batch.vertices.len());
        let mut uvs=Vec::<[f32;2]>::with_capacity(batch.vertices.len());

        for triangle in batch.vertices.chunks_exact(3) {
            let a=sm64_render_vec3(triangle[0].position);
            let b=sm64_render_vec3(triangle[1].position);
            let c=sm64_render_vec3(triangle[2].position);
            let normal=(b-a).cross(c-a).try_normalize().unwrap_or(Vec3::Z).to_array();

            for vertex in triangle {
                positions.push(sm64_render_pos(vertex.position));
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

        let texture_handle=batch.texture_symbol.as_ref().and_then(|symbol|{
            if let Some(handle)=texture_cache.get(symbol) {
                return Some(handle.clone());
            }
            let path=texture_sources.get(symbol)?;
            match load_sm64_png(path,images) {
                Ok(handle)=>{
                    texture_cache.insert(symbol.clone(),handle.clone());
                    Some(handle)
                }
                Err(error)=>{
                    warn!("SM64 texture {} could not load from {}: {error}",symbol,path.display());
                    None
                }
            }
        });
        let base_color=if texture_handle.is_some() {
            Color::WHITE
        } else {
            debug_texture_color(batch.texture_symbol.as_deref(),batch_index)
        };

        let alpha_mode=if batch.layer.contains("TRANSPARENT") {
            AlphaMode::Blend
        } else if batch.layer.contains("ALPHA") {
            AlphaMode::Mask(0.5)
        } else {
            AlphaMode::Opaque
        };
        commands.spawn((
            Name::new(format!(
                "SM64 display list batch {} {}",
                batch_index,
                batch.texture_symbol.as_deref().unwrap_or("untextured")
            )),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color,
                base_color_texture:texture_handle,
                alpha_mode,
                perceptual_roughness:1.0,
                unlit:true,
                ..default()
            })),
            Sm64DebugWorld,
        ));
    }
}


fn spawn_mario_hierarchy(
    commands:&mut Commands,
    meshes:&mut Assets<Mesh>,
    materials:&mut Assets<StandardMaterial>,
    images:&mut Assets<Image>,
    mario_root:Entity,
    model:&sm64_assets::ResolvedGeoModel,
    texture_sources:&HashMap<String,std::path::PathBuf>,
) {
    let mut texture_cache=HashMap::<String,Handle<Image>>::new();

    for (part_index,part) in model.parts.iter().enumerate() {
        for (batch_index,batch) in part.geometry.batches.iter().enumerate() {
            if batch.vertices.len()<3 {continue;}
            let mut positions=Vec::with_capacity(batch.vertices.len());
            let mut normals=Vec::with_capacity(batch.vertices.len());
            let mut uvs=Vec::with_capacity(batch.vertices.len());

            for triangle in batch.vertices.chunks_exact(3) {
                let a=sm64_render_vec3(triangle[0].position);
                let b=sm64_render_vec3(triangle[1].position);
                let d=sm64_render_vec3(triangle[2].position);
                let face_normal=(b-a).cross(d-a).try_normalize().unwrap_or(Vec3::Y);
                for vertex in triangle {
                    positions.push(sm64_render_pos(vertex.position));
                    let n=Vec3::new(
                        vertex.attributes[0] as i8 as f32,
                        vertex.attributes[2] as i8 as f32,
                        vertex.attributes[1] as i8 as f32,
                    ).try_normalize().unwrap_or(face_normal);
                    normals.push(n.to_array());
                    let [w,h]=batch.texture_size.unwrap_or([32,32]);
                    uvs.push([
                        vertex.texcoord[0] as f32/(32.0*w.max(1) as f32),
                        vertex.texcoord[1] as f32/(32.0*h.max(1) as f32),
                    ]);
                }
            }

            let mesh=Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::MAIN_WORLD|RenderAssetUsages::RENDER_WORLD,
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION,positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL,normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0,uvs);

            let texture=batch.texture_symbol.as_ref().and_then(|symbol|{
                if let Some(handle)=texture_cache.get(symbol){return Some(handle.clone());}
                let path=texture_sources.get(symbol)?;
                let handle=load_sm64_png(path,images).ok()?;
                texture_cache.insert(symbol.clone(),handle.clone());
                Some(handle)
            });
            let base_color=if texture.is_some(){Color::WHITE}else{mario_batch_color(batch)};

            commands.spawn((
                Name::new(format!("SM64 Mario hierarchy part {part_index} batch {batch_index}")),
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(materials.add(StandardMaterial{
                    base_color,
                    base_color_texture:texture,
                    unlit:true,
                    cull_mode:None,
                    alpha_mode:if batch.layer.contains("TRANSPARENT"){AlphaMode::Blend}
                        else if batch.layer.contains("ALPHA"){AlphaMode::Mask(0.5)}
                        else{AlphaMode::Opaque},
                    ..default()
                })),
                Transform {
                    translation:Vec3::from_array(part.spec.translation),
                    rotation:Quat::from_euler(
                        EulerRot::XYZ,
                        part.spec.rotation_deg[0].to_radians(),
                        part.spec.rotation_deg[1].to_radians(),
                        part.spec.rotation_deg[2].to_radians(),
                    ),
                    scale:Vec3::splat(part.spec.scale),
                },
                ChildOf(mario_root),
                Sm64DebugWorld,
            ));
        }
    }
}

fn spawn_mario_geometry(
    commands:&mut Commands,
    meshes:&mut Assets<Mesh>,
    materials:&mut Assets<StandardMaterial>,
    images:&mut Assets<Image>,
    mario_root:Entity,
    geometry:&sm64_assets::ParsedRenderGeometry,
    texture_sources:&HashMap<String,std::path::PathBuf>,
) {
    let mut texture_cache=HashMap::<String,Handle<Image>>::new();

    for (batch_index,batch) in geometry.batches.iter().enumerate() {
        if batch.vertices.len()<3 {continue;}
        let mut positions=Vec::<[f32;3]>::with_capacity(batch.vertices.len());
        let mut normals=Vec::<[f32;3]>::with_capacity(batch.vertices.len());
        let mut uvs=Vec::<[f32;2]>::with_capacity(batch.vertices.len());

        for triangle in batch.vertices.chunks_exact(3) {
            let a=sm64_render_vec3(triangle[0].position);
            let b=sm64_render_vec3(triangle[1].position);
            let d=sm64_render_vec3(triangle[2].position);
            let face_normal=(b-a).cross(d-a).try_normalize().unwrap_or(Vec3::Y);
            for vertex in triangle {
                positions.push(sm64_render_pos(vertex.position));
                // Mario's Vtx payload stores signed normals in the RGB bytes
                // while lighting is active. Prefer them over a flat face normal.
                let n=Vec3::new(
                    vertex.attributes[0] as i8 as f32,
                    vertex.attributes[1] as i8 as f32,
                    vertex.attributes[2] as i8 as f32,
                ).try_normalize().unwrap_or(face_normal);
                normals.push(n.to_array());
                let [width,height]=batch.texture_size.unwrap_or([32,32]);
                uvs.push([
                    vertex.texcoord[0] as f32/(32.0*width.max(1) as f32),
                    vertex.texcoord[1] as f32/(32.0*height.max(1) as f32),
                ]);
            }
        }

        let mesh=Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION,positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL,normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0,uvs);

        let texture_handle=batch.texture_symbol.as_ref().and_then(|symbol|{
            if let Some(handle)=texture_cache.get(symbol) {
                return Some(handle.clone());
            }
            let path=texture_sources.get(symbol)?;
            match load_sm64_png(path,images) {
                Ok(handle)=>{
                    texture_cache.insert(symbol.clone(),handle.clone());
                    Some(handle)
                }
                Err(error)=>{
                    warn!("Mario texture {} could not load from {}: {error}",symbol,path.display());
                    None
                }
            }
        });
        let base_color=if texture_handle.is_some() {
            Color::WHITE
        } else {
            mario_batch_color(batch)
        };

        commands.spawn((
            Name::new(format!("SM64 Mario {} batch {}",batch.layer,batch_index)),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color,
                base_color_texture:texture_handle,
                perceptual_roughness:1.0,
                unlit:true,
                cull_mode:None,
                ..default()
            })),
            Transform::from_translation(mario_part_translation(&batch.layer)),
            ChildOf(mario_root),
            Sm64DebugWorld,
        ));
    }
}

fn mario_part_translation(layer:&str)->Vec3 {
    match layer {
        "MARIO_BUTT"=>Vec3::ZERO,
        "MARIO_TORSO"=>Vec3::new(68.0,0.0,0.0),
        "MARIO_HEAD"=>Vec3::new(155.0,0.0,0.0),
        "MARIO_LEFT_ARM"=>Vec3::new(135.0,-10.0,79.0),
        "MARIO_LEFT_FOREARM"=>Vec3::new(200.0,-10.0,79.0),
        "MARIO_LEFT_HAND"=>Vec3::new(260.0,-10.0,79.0),
        "MARIO_RIGHT_ARM"=>Vec3::new(136.0,-10.0,-79.0),
        "MARIO_RIGHT_FOREARM"=>Vec3::new(201.0,-10.0,-79.0),
        "MARIO_RIGHT_HAND"=>Vec3::new(261.0,-10.0,-79.0),
        "MARIO_LEFT_THIGH"=>Vec3::new(13.0,-8.0,42.0),
        "MARIO_LEFT_LEG"=>Vec3::new(102.0,-8.0,42.0),
        "MARIO_LEFT_FOOT"=>Vec3::new(169.0,-8.0,42.0),
        "MARIO_RIGHT_THIGH"=>Vec3::new(13.0,-8.0,-42.0),
        "MARIO_RIGHT_LEG"=>Vec3::new(102.0,-8.0,-42.0),
        "MARIO_RIGHT_FOOT"=>Vec3::new(169.0,-8.0,-42.0),
        _=>Vec3::ZERO,
    }
}

fn mario_batch_color(batch:&sm64_assets::RenderBatch)->Color {
    if let Some(light)=batch.light_symbol.as_deref() {
        return match light {
            "mario_blue_lights_group"=>Color::srgb(0.0,0.0,1.0),
            "mario_red_lights_group"=>Color::srgb(1.0,0.0,0.0),
            "mario_white_lights_group"=>Color::WHITE,
            "mario_brown1_lights_group"=>Color::srgb(0x72 as f32/255.0,0x1c as f32/255.0,0x0e as f32/255.0),
            "mario_beige_lights_group"=>Color::srgb(0xfe as f32/255.0,0xc1 as f32/255.0,0x79 as f32/255.0),
            "mario_brown2_lights_group"=>Color::srgb(0x73 as f32/255.0,0x06 as f32/255.0,0.0),
            _=>debug_texture_color(Some(light),0),
        };
    }

    if batch.layer.contains("ARM") || batch.layer.contains("FOREARM") {
        Color::srgb(1.0,0.0,0.0)
    } else if batch.layer.contains("HAND") {
        Color::WHITE
    } else if batch.layer.contains("FOOT") {
        Color::srgb(0x72 as f32/255.0,0x1c as f32/255.0,0x0e as f32/255.0)
    } else if batch.layer.contains("HEAD") {
        Color::srgb(0xfe as f32/255.0,0xc1 as f32/255.0,0x79 as f32/255.0)
    } else {
        Color::srgb(0.0,0.0,1.0)
    }
}

fn load_sm64_png(
    path:&std::path::Path,
    images:&mut Assets<Image>,
)->Result<Handle<Image>,String>{
    let decoded=image::open(path)
        .map_err(|error|error.to_string())?
        .into_rgba8();
    let (width,height)=decoded.dimensions();
    let mut image=Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers:1,
        },
        TextureDimension::D2,
        decoded.into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    let mut sampler=ImageSamplerDescriptor::nearest();
    sampler.address_mode_u=ImageAddressMode::Repeat;
    sampler.address_mode_v=ImageAddressMode::Repeat;
    sampler.address_mode_w=ImageAddressMode::Repeat;
    image.sampler=ImageSampler::Descriptor(sampler);
    Ok(images.add(image))
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
            surface.vertex1[2] as f32,
            surface.vertex1[1] as f32,
        ]);
        positions.push([
            surface.vertex2[0] as f32,
            surface.vertex2[2] as f32,
            surface.vertex2[1] as f32,
        ]);
        positions.push([
            surface.vertex3[0] as f32,
            surface.vertex3[2] as f32,
            surface.vertex3[1] as f32,
        ]);
        let n=[surface.normal.x,surface.normal.z,surface.normal.y];
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
    external: Res<Sm64ExternalPlayer>,
    mut runtime: ResMut<Sm64Runtime>,
) {
    if !enabled.0 { return; }
    runtime.accumulator += time.delta_secs_f64();
    let mut steps=0;
    while runtime.accumulator >= SM64_TICK_SECONDS && steps < 8 {
        runtime.accumulator -= SM64_TICK_SECONDS;
        runtime.latest=Some(if external.active {
            runtime.world.step_external_player(
                external.sm64_pos,
                external.sm64_vel,
                external.sm64_yaw,
            )
        } else {
            runtime.world.step(input.0)
        });
        steps += 1;
    }
}

fn sync_mario_presentation(
    runtime:Res<Sm64Runtime>,
    mut query:Query<&mut Transform,With<Sm64MarioPresentation>>,
) {
    let Some(snapshot)=runtime.latest.as_ref() else {return;};
    for mut transform in &mut query {
        transform.translation=sm64_render_vec3(snapshot.mario.pos);
        let yaw=snapshot.mario.face_angle[1] as u16 as f32
            * core::f32::consts::TAU / 65536.0;
        transform.rotation=Quat::from_rotation_z(yaw);
    }
}

fn sync_object_presentations(
    mut commands:Commands,
    runtime:Res<Sm64Runtime>,
    mut query:Query<(Entity,&Sm64ObjectPresentation,&mut Transform)>,
) {
    let Some(snapshot)=runtime.latest.as_ref() else {return;};
    let by_id=snapshot.objects.iter()
        .map(|object|(object.id,object))
        .collect::<HashMap<_,_>>();

    for (entity,presentation,mut transform) in &mut query {
        let Some(object)=by_id.get(&presentation.id) else {
            commands.entity(entity).despawn();
            continue;
        };
        transform.translation=sm64_render_vec3(object.pos);
        let yaw=object.face_angle[1] as u16 as f32*core::f32::consts::TAU/65536.0;
        transform.rotation=Quat::from_rotation_z(yaw);
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

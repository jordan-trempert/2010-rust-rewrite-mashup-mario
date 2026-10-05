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

#[derive(Default)]
struct Sm64NativeRuntime {
    client: Option<sm64_native::NativeClient>,
    model_symbols: HashMap<i32,String>,
    active: bool,
}

#[derive(Resource, Default)]
struct Sm64PresentationCache {
    level:String,
    registry:HashMap<String,sm64_assets::ModelSource>,
    geo:HashMap<
        (String,i32),
        (sm64_assets::ResolvedGeoModel,HashMap<String,std::path::PathBuf>)
    >,
    dl:HashMap<
        String,
        (sm64_assets::ParsedRenderGeometry,HashMap<String,std::path::PathBuf>)
    >,
}


#[derive(Resource, Default, Debug, Clone)]
pub struct Sm64NativeDynamicCollision {
    pub triangles: Vec<[[f32;3];3]>,
    pub tick: u32,
}

#[derive(Resource, Default, Debug, Clone)]
pub struct Sm64NativeRenderFrame {
    pub tick:u32,
    pub triangles:Vec<sm64_native::NativeRenderTriangle>,
    pub texture_updates:Vec<sm64_native::NativeTextureUpdate>,
}

#[derive(Resource, Default)]
struct Sm64NativeRenderCache {
    textures:HashMap<u32,Handle<Image>>,
    batches:HashMap<(u32,bool),NativeRenderBatchHandles>,
}

struct NativeRenderBatchHandles {
    entity:Entity,
    mesh:Handle<Mesh>,
    material:Handle<StandardMaterial>,
}

#[derive(Component, Debug, Clone, Copy)]
struct Sm64NativeRenderPresentation;

#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct Sm64NativePlayerOutput {
    pub active: bool,
    pub sm64_pos: [f32;3],
    pub sm64_vel: [f32;3],
    pub sm64_yaw: i16,
    pub action: u32,
    pub health: i32,
    pub coins: i32,
}

#[derive(Resource, Default, Debug, Clone)]
pub struct Sm64NativeDialogOutput {
    pub id: i16,
    pub text: String,
}

#[derive(Component, Debug, Clone, Copy)]
struct Sm64DialogRoot;

#[derive(Component, Debug, Clone, Copy)]
struct Sm64DialogText;

#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct Sm64ExternalPlayer {
    pub sm64_pos: [f32;3],
    /// SM64-coordinate velocity in units per original 30 Hz tick.
    pub sm64_vel: [f32;3],
    /// SM64 binary-angle yaw used by object behaviors that face/track Mario.
    pub sm64_yaw: i16,
    pub sm64_pitch: i16,
    pub attack_flags: u32,
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

#[derive(Component, Debug, Clone)]
struct Sm64ObjectPresentation {
    id: sm64_core::ObjectId,
    model:String,
    anim_state:i32,
}

#[derive(Component, Debug, Clone, Copy)]
struct Sm64BillboardPart;

#[derive(Component, Debug, Clone, Copy)]
struct Sm64BillboardObject;

#[derive(Component, Debug, Clone, Copy)]
struct Sm64DebugWorld;

#[derive(Component, Debug, Clone, Copy)]
struct Sm64SkyboxPresentation;

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
            .init_resource::<Sm64NativePlayerOutput>()
            .init_resource::<Sm64NativeDialogOutput>()
            .init_resource::<Sm64NativeDynamicCollision>()
            .init_resource::<Sm64NativeRenderFrame>()
            .init_resource::<Sm64NativeRenderCache>()
            .init_resource::<Sm64PresentationCache>()
            .init_resource::<Sm64LoadStatus>()
            .init_resource::<Sm64DebugView>();
        app.insert_non_send_resource(Sm64NativeRuntime::default());
        app.add_systems(
            Update,
            (
                launch_requested_sm64_map,
                update_debug_input.after(launch_requested_sm64_map),
                advance_sm64_runtime.after(launch_requested_sm64_map),
                sync_sm64_dialog_overlay.after(advance_sm64_runtime),
                sync_native_render_frame.after(advance_sm64_runtime),
                sync_mario_presentation.after(advance_sm64_runtime),
                sync_object_presentations.after(advance_sm64_runtime),
                face_sm64_billboard_objects.after(sync_object_presentations),
                face_sm64_billboards.after(sync_object_presentations),
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
            With<Sm64SkyboxPresentation>,
            With<Sm64DialogRoot>,
            With<Sm64NativeRenderPresentation>,
        )>,
    >,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut enabled: ResMut<Sm64Enabled>,
    debug_view: Res<Sm64DebugView>,
    mut runtime: ResMut<Sm64Runtime>,
    mut native_render: ResMut<Sm64NativeRenderFrame>,
    mut native_render_cache: ResMut<Sm64NativeRenderCache>,
    mut native: NonSendMut<Sm64NativeRuntime>,
    mut presentation_cache: ResMut<Sm64PresentationCache>,
    mut status: ResMut<Sm64LoadStatus>,
) {
    let Some(request)=request else {return;};

    for entity in &existing {
        commands.entity(entity).despawn();
    }
    native.client=None;
    native.model_symbols.clear();
    native.active=false;
    native_render.tick=0;
    native_render.triangles.clear();
    native_render.texture_updates.clear();
    native_render_cache.textures.clear();
    native_render_cache.batches.clear();

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
    let skybox_name=sm64_assets::load_skybox_name(&root,&level)
        .unwrap_or_else(|error|{
            warn!("SM64 skybox discovery failed: {error}");
            None
        });
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
    let (object_spawns,behavior_lists)=if cod_active.is_none() {
        (
            sm64_assets::load_level_object_spawns(&root,&level,area)
                .unwrap_or_else(|error|{
                    warn!("SM64 object spawn load failed: {error}");
                    sm64_assets::ParsedObjectSpawns::default()
                }),
            sm64_assets::load_behavior_object_lists(&root)
                .unwrap_or_else(|error|{
                    warn!("SM64 behavior list load failed: {error}");
                    HashMap::new()
                }),
        )
    } else {
        // sm64cod:* never constructs the old Rust gameplay object world.
        (sm64_assets::ParsedObjectSpawns::default(),HashMap::new())
    };
    let selected_act=std::env::var("SM64_ACT")
        .ok()
        .and_then(|value|value.parse::<u8>().ok())
        .filter(|value|(1..=6).contains(value))
        .unwrap_or(1);

    if cod_active.is_some() {
        native.model_symbols=sm64_assets::load_model_id_symbols(&root,&level)
            .unwrap_or_else(|error|{
                warn!("SM64 native model-id mapping failed: {error}");
                HashMap::new()
            });
        match sm64_native::NativeClient::launch(&root,&level,area,selected_act) {
            Ok(client)=>{
                let loaded_path=client.module_path().to_path_buf();
                native.client=Some(client);
                native.active=true;
                info!(
                    "SM64 COD map: EMBEDDED DLL LOADED in-process: {} | level={level} area={area} act={selected_act}",
                    loaded_path.display()
                );
            }
            Err(error)=>{
                // sm64cod:* is the native-decomp mode. Never silently drop back
                // to the incomplete Rust behavior subset here; doing so makes
                // it impossible to tell whether COD is actually using the DLL.
                enabled.0=false;
                native.client=None;
                native.active=false;
                status.loaded=false;
                status.message=format!(
                    "SM64 COD map REFUSED TO START because the embedded DLL did not load: {error}. Build tools/sm64_native_bridge/build.ps1 against sm64-port or set SM64_NATIVE_MODULE to iw4l-sm64-native.dll."
                );
                error!("{}",status.message);
                commands.remove_resource::<Sm64LaunchRequest>();
                return;
            }
        }
    }

    let surface_count=parsed.world.surfaces.len();
    let special_count=parsed.specials.len();
    let render_triangles=render_geometry.as_ref().map_or(0,|g|g.triangle_count);
    let render_batches=render_geometry.as_ref().map_or(0,|g|g.batches.len());

    if debug_view.0 || cod_active.is_some() {
        if let Some(skybox)=skybox_name.as_deref() {
            let path=root.join("textures").join("skyboxes").join(format!("{skybox}.png"));
            if let Err(error)=spawn_sm64_skybox(
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut images,
                &path,
            ) {
                warn!("SM64 skybox {skybox} failed: {error}");
            }
        }
    }
    if debug_view.0 && cod_active.is_none() {
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
            true,
        );
    }

    runtime.world=Sm64World::new();
    runtime.accumulator=0.0;
    runtime.latest=None;

    if cod_active.is_some() {
        /*
         * Native-only gameplay path. Keep only a tiny Mario carrier so the
         * presentation adapter has a previous frame to clone before the first
         * DLL snapshot arrives. No Rust SM64 collision/object simulation is
         * installed or stepped for sm64cod:*.
         */
        runtime.world.mario.pos=[
            spawn.pos[0] as f32,
            spawn.pos[1] as f32,
            spawn.pos[2] as f32,
        ];
        runtime.world.mario.face_angle[1]=spawn.yaw_sm64();
        runtime.world.mario.terrain_type=area_settings.terrain_type;

        info!(
            "SM64 COD map: NATIVE-ONLY GAMEPLAY enabled; Rust Sm64World objects=0, Rust fallback simulation=disabled"
        );
    } else {
        // Standalone/debug sm64:* mode keeps the Rust simulation.
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
        if debug_view.0 {
            spawn_runtime_object_presentations(
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut images,
                &root,
                &level,
                &runtime.world.objects,
                &mut presentation_cache,
            );
        }
        runtime.latest=Some(runtime.world.snapshot());
    }

    enabled.0=std::env::var("SM64_ENABLED")
        .map(|v|v!="0" && !v.eq_ignore_ascii_case("false"))
        .unwrap_or(true);

    status.loaded=true;
    status.message=if cod_active.is_some() {
        format!(
            "SM64 {level} area {area} act {selected_act}: native DLL gameplay only; {surface_count} static collision surfaces, {render_triangles} presentation triangles in {render_batches} batches; Rust gameplay objects=0"
        )
    } else {
        format!(
            "SM64 {level} area {area} act {selected_act}: {surface_count} collision surfaces, {special_count} special collision objects, {render_triangles} render triangles in {render_batches} batches, {} Rust runtime objects, Mario at {:?}",
            runtime.world.objects.len(),
            spawn.pos
        )
    };
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
    cache:&mut Sm64PresentationCache,
) {
    if cache.level!=level {
        cache.level=level.to_owned();
        cache.geo.clear();
        cache.dl.clear();
        cache.registry=match sm64_assets::load_model_registry(decomp_root,level) {
            Ok(registry)=>registry,
            Err(error)=>{
                warn!("SM64 model registry load failed: {error}");
                HashMap::new()
            }
        };
    }

    for object in objects {
        if object.model=="MODEL_NONE" {continue;}

        // Always create the presentation root first. If geometry resolution
        // fails, the root remains as an invisible placeholder so the same
        // missing model is not reparsed and re-warned every frame.
        let mut root_entity=commands.spawn((
            Name::new(format!("SM64 object {} {}",object.id.0,object.model)),
            Transform {
                translation:sm64_render_vec3(object.pos),
                rotation:sm64_presentation_rotation(&object.model,object.face_angle),
                scale:Vec3::from_array(object.scale),
            },
            if (object.render_flags & 1)!=0 && (object.render_flags & (1<<4))==0 {
                Visibility::Visible
            } else {
                Visibility::Hidden
            },
            Sm64ObjectPresentation{
                id:object.id,
                model:object.model.clone(),
                anim_state:object.anim_state,
            },
        ));
        if (object.render_flags & (1<<2))!=0 {
            root_entity.insert(Sm64BillboardObject);
        }
        let root=root_entity.id();

        let Some(source)=cache.registry.get(&object.model).cloned() else {
            warn!("SM64 object model {} is not registered",object.model);
            continue;
        };

        match source {
            sm64_assets::ModelSource::Geo{geo_symbol}=>{
                let geo_key=(object.model.clone(),object.anim_state);
                if !cache.geo.contains_key(&geo_key) {
                    match sm64_assets::resolve_geo_model_parts_for_level_state(
                        decomp_root,level,&geo_symbol,object.anim_state
                    ) {
                        Ok(model)=>{
                            let mut textures=sm64_assets::load_actor_texture_sources(
                                decomp_root,&model.actor_name
                            ).unwrap_or_default();
                            if textures.is_empty() {
                                textures=sm64_assets::load_level_object_texture_sources(
                                    decomp_root,level,&model.actor_name
                                ).unwrap_or_default();
                            }
                            if textures.is_empty() {
                                textures=sm64_assets::load_texture_sources(
                                    decomp_root,level
                                ).unwrap_or_default();
                            }
                            cache.geo.insert(geo_key.clone(),(model,textures));
                        }
                        Err(hierarchy_error)=>{
                            // Some generated decomp GeoLayouts are simple
                            // enough for the direct display-list resolver even
                            // when hierarchy parsing fails. Use that before
                            // dropping the model entirely (notably stars).
                            match sm64_assets::resolve_geo_model_geometry(
                                decomp_root,&geo_symbol
                            ) {
                                Ok((geometry,actor_name))=>{
                                    let mut textures=sm64_assets::load_actor_texture_sources(
                                        decomp_root,&actor_name
                                    ).unwrap_or_default();
                                    if textures.is_empty() {
                                        textures=sm64_assets::load_texture_sources(
                                            decomp_root,level
                                        ).unwrap_or_default();
                                    }
                                    cache.dl.insert(
                                        object.model.clone(),
                                        (geometry,textures),
                                    );
                                }
                                Err(flat_error)=>{
                                    warn!(
                                        "SM64 object model {} ({geo_symbol}) failed: {hierarchy_error}; flat fallback: {flat_error}",
                                        object.model
                                    );
                                    continue;
                                }
                            }
                        }
                    }
                }
                if let Some((model,textures))=cache.geo.get(&geo_key) {
                    spawn_hierarchical_object_geometry(
                        commands,meshes,materials,images,root,model,textures,
                    );
                } else if let Some((geometry,textures))=cache.dl.get(&object.model) {
                    spawn_flat_object_geometry(
                        commands,meshes,materials,images,root,geometry,textures,
                        sm64_flat_geo_scale(&object.model),
                    );
                }
            }
            sm64_assets::ModelSource::DisplayList{display_list,layer}=>{
                if !cache.dl.contains_key(&object.model) {
                    match sm64_assets::resolve_display_list_model_geometry(
                        decomp_root,level,&display_list,&layer,
                    ) {
                        Ok(resolved)=>{
                            cache.dl.insert(object.model.clone(),resolved);
                        }
                        Err(error)=>{
                            warn!(
                                "SM64 display-list model {} ({display_list}) failed: {error}",
                                object.model
                            );
                            continue;
                        }
                    }
                }
                let Some((geometry,textures))=cache.dl.get(&object.model) else {continue;};
                spawn_flat_object_geometry(
                    commands,meshes,materials,images,root,geometry,textures,1.0,
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
            let handle=load_sm64_png_clamped(path,images).ok()?;
            texture_cache.insert(symbol.clone(),handle.clone());
            Some(handle)
        });
        let mut entity=commands.spawn((
            Name::new(format!("SM64 object part {part_index} mesh {batch_index}")),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial{
                base_color:if texture.is_some() {
                    Color::WHITE
                } else if let Some([r,g,b])=batch.light_color {
                    Color::srgb(
                        r as f32/255.0,
                        g as f32/255.0,
                        b as f32/255.0,
                    )
                } else {
                    Color::WHITE
                },
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
                scale:Vec3::splat(
                    if matches!(model.actor_name.as_str(),"star"|"transparent_star") {
                        0.25
                    } else {
                        part.spec.scale
                    }
                ),
            },
            ChildOf(parent),
            Sm64DebugWorld,
        ));
        if part.spec.billboard {
            entity.insert(Sm64BillboardPart);
        }
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
    local_scale:f32,
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
            let n={ let n=(b-a).cross(d-a).normalize_or_zero(); if n.length_squared()>0.0 {n} else {Vec3::Z} }.to_array();
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
            let handle=load_sm64_png_clamped(path,images).ok()?;
            texture_cache.insert(symbol.clone(),handle.clone());
            Some(handle)
        });

        commands.spawn((
            Name::new(format!("SM64 flat object mesh {batch_index}")),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial{
                base_color:if texture.is_some() {
                    Color::WHITE
                } else if let Some([r,g,b])=batch.light_color {
                    Color::srgb(
                        r as f32/255.0,
                        g as f32/255.0,
                        b as f32/255.0,
                    )
                } else {
                    Color::WHITE
                },
                base_color_texture:texture,
                unlit:true,
                // Many SM64 props (notably BOB's bubbly trees) are authored as
                // thin textured planes. N64 render state allows them to remain
                // visible from the gameplay camera; never back-face cull them.
                cull_mode:None,
                alpha_mode:if batch.layer.contains("TRANSPARENT"){AlphaMode::Blend}
                    else if batch.layer.contains("ALPHA"){AlphaMode::Mask(0.5)}
                    else{AlphaMode::Opaque},
                ..default()
            })),
            Transform::from_scale(Vec3::splat(local_scale)),
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
    let s=sm64_core::SM64_TO_IW4_SCALE;
    [v[0]*s,-v[2]*s,v[1]*s]
}

#[inline]
fn sm64_render_vec3(v:[f32;3])->Vec3 {
    let s=sm64_core::SM64_TO_IW4_SCALE;
    Vec3::new(v[0]*s,-v[2]*s,v[1]*s)
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
    Quat::from_mat3(&(basis * Mat3::from_quat(old) * basis.transpose()))
}

#[inline]
fn sm64_object_rotation(angle:[i16;3])->Quat {
    let to_deg=|value:i16| value as u16 as f32*360.0/65536.0;
    sm64_render_rotation([
        to_deg(angle[0]),
        to_deg(angle[1]),
        to_deg(angle[2]),
    ])
}

#[inline]
fn sm64_upright_actor(model:&str)->bool {
    model.contains("GOOMBA")
        || model.contains("BOBOMB")
        || model.contains("BOB_OMB")
}

#[inline]
fn sm64_presentation_rotation(model:&str,angle:[i16;3])->Quat {
    if sm64_upright_actor(model) {
        /*
         * Goomba and Bob-omb animation tables carry a constant 0x3FFF
         * (~+90°) root-Y channel. We do not yet evaluate skeletal animation
         * channels, so without this root channel the whole actor is presented
         * sideways. Apply that authored root yaw here while keeping transient
         * pitch/roll out of the rigid presentation.
         */
        let yaw=(angle[1] as u16).wrapping_add(0x4000) as i16;
        sm64_object_rotation([0,yaw,0])
    } else {
        sm64_object_rotation(angle)
    }
}

#[inline]
fn sm64_flat_geo_scale(model:&str)->f32 {
    // star_geo carries GEO_SCALE(0x00, 16384). When hierarchical GeoLayout
    // resolution falls back to the flat display-list resolver that scale node
    // is otherwise lost, producing a 4x oversized star.
    if matches!(model,"MODEL_STAR"|"MODEL_TRANSPARENT_STAR") {
        0.25
    } else {
        1.0
    }
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

fn load_sm64_png_clamped(
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
    sampler.address_mode_u=ImageAddressMode::ClampToEdge;
    sampler.address_mode_v=ImageAddressMode::ClampToEdge;
    sampler.address_mode_w=ImageAddressMode::ClampToEdge;
    image.sampler=ImageSampler::Descriptor(sampler);
    Ok(images.add(image))
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

fn spawn_sm64_skybox(
    commands:&mut Commands,
    meshes:&mut Assets<Mesh>,
    materials:&mut Assets<StandardMaterial>,
    images:&mut Assets<Image>,
    path:&std::path::Path,
)->Result<(),String>{
    let decoded=image::open(path)
        .map_err(|error|format!("{}: {error}",path.display()))?
        .into_rgba8();
    let (width,height)=decoded.dimensions();
    let mut image=Image::new(
        Extent3d {width,height,depth_or_array_layers:1},
        TextureDimension::D2,
        decoded.into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    let mut sampler=ImageSamplerDescriptor::nearest();
    sampler.address_mode_u=ImageAddressMode::Repeat;
    sampler.address_mode_v=ImageAddressMode::ClampToEdge;
    sampler.address_mode_w=ImageAddressMode::ClampToEdge;
    image.sampler=ImageSampler::Descriptor(sampler);
    let texture=images.add(image);

    const LON:usize=48;
    const LAT:usize=24;
    const R:f32=30000.0;
    let mut positions=Vec::<[f32;3]>::with_capacity(LON*LAT*6);
    let mut normals=Vec::<[f32;3]>::with_capacity(LON*LAT*6);
    let mut uvs=Vec::<[f32;2]>::with_capacity(LON*LAT*6);

    let point=|u:f32,v:f32| {
        let theta=u*core::f32::consts::TAU;
        let phi=(v-0.5)*core::f32::consts::PI;
        let cp=phi.cos();
        [R*cp*theta.cos(),R*cp*theta.sin(),R*phi.sin()]
    };

    for y in 0..LAT {
        let v0=y as f32/LAT as f32;
        let v1=(y+1) as f32/LAT as f32;
        for x in 0..LON {
            let u0=x as f32/LON as f32;
            let u1=(x+1) as f32/LON as f32;
            let p00=point(u0,v0);
            let p10=point(u1,v0);
            let p11=point(u1,v1);
            let p01=point(u0,v1);
            for (p,uv) in [
                (p00,[u0,1.0-v0]),(p11,[u1,1.0-v1]),(p10,[u1,1.0-v0]),
                (p00,[u0,1.0-v0]),(p01,[u0,1.0-v1]),(p11,[u1,1.0-v1]),
            ] {
                positions.push(p);
                normals.push([0.0,0.0,1.0]);
                uvs.push(uv);
            }
        }
    }

    let mesh=Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION,positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL,normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0,uvs);

    commands.spawn((
        Name::new("SM64 skybox"),
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial{
            base_color:Color::WHITE,
            base_color_texture:Some(texture),
            unlit:true,
            cull_mode:None,
            ..default()
        })),
        Transform::IDENTITY,
        Sm64SkyboxPresentation,
        Sm64DebugWorld,
    ));
    Ok(())
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
    cod_active: Option<Res<Sm64CodActive>>,
    external: Res<Sm64ExternalPlayer>,
    mut native: NonSendMut<Sm64NativeRuntime>,
    mut native_output: ResMut<Sm64NativePlayerOutput>,
    mut native_dialog: ResMut<Sm64NativeDialogOutput>,
    mut native_collision: ResMut<Sm64NativeDynamicCollision>,
    mut native_render: ResMut<Sm64NativeRenderFrame>,
    mut runtime: ResMut<Sm64Runtime>,
) {
    if !enabled.0 { return; }
    runtime.accumulator += time.delta_secs_f64();
    let mut steps=0;
    while runtime.accumulator >= SM64_TICK_SECONDS && steps < 8 {
        runtime.accumulator -= SM64_TICK_SECONDS;
        if cod_active.is_some() {
            // sm64cod:* is strictly native DLL gameplay. Never execute the old
            // Rust Sm64World as a fallback, even while waiting for the COD
            // player to spawn.
            if !native.active {
                native_output.active=false;
                native_dialog.id=-1;
                native_dialog.text.clear();
                native_collision.triangles.clear();
                steps += 1;
                continue;
            }
            if !external.active {
                native_output.active=false;
                steps += 1;
                continue;
            }

            let Some(client)=native.client.as_mut() else {
                error!("SM64 native-only mode lost its loaded DLL client; refusing Rust fallback");
                native.active=false;
                native_output.active=false;
                steps += 1;
                continue;
            };

            match client.step(sm64_native::NativePlayerProxy {
                pos:external.sm64_pos,
                vel:external.sm64_vel,
                yaw:external.sm64_yaw,
                pitch:external.sm64_pitch,
                health:external.health,
                attack_flags:external.attack_flags,
            }) {
                Ok(snapshot)=>{
                    let native_object_count=snapshot.objects.len();
                    native_dialog.id=snapshot.dialog_id;
                    native_dialog.text=snapshot.dialog_text.clone();
                    native_collision.tick=snapshot.tick;
                    native_collision.triangles=snapshot.dynamic_surfaces.clone();
                    native_render.tick=snapshot.tick;
                    native_render.triangles=snapshot.render_triangles.clone();
                    native_render.texture_updates=snapshot.texture_updates.clone();
                    native_output.active=true;
                    native_output.sm64_pos=snapshot.mario_pos;
                    native_output.sm64_vel=snapshot.mario_vel;
                    native_output.sm64_yaw=snapshot.mario_yaw;
                    native_output.action=snapshot.mario_action;
                    native_output.health=snapshot.mario_health;
                    native_output.coins=snapshot.coins;
                    runtime.latest=Some(native_snapshot_to_sm64(
                        snapshot,
                        &runtime.world.mario,
                        &native.model_symbols,
                    ));
                    if runtime.latest.as_ref().is_some_and(|snapshot|snapshot.tick<=3 || snapshot.tick%300==0) {
                        info!(
                            "SM64 COD native DLL snapshot tick={} objects={} (Rust gameplay objects=0)",
                            runtime.latest.as_ref().map_or(0,|snapshot|snapshot.tick),
                            native_object_count
                        );
                    }
                }
                Err(error)=>{
                    error!("SM64 embedded native gameplay runtime failed: {error}");
                    native.client=None;
                    native.active=false;
                    native_output.active=false;
                    native_dialog.id=-1;
                    native_dialog.text.clear();
                    native_collision.triangles.clear();
                    native_render.triangles.clear();
                    native_render.texture_updates.clear();
                    runtime.latest=None;
                }
            }
        } else {
            // Standalone/debug sm64:* mode only.
            native_output.active=false;
            native_dialog.id=-1;
            native_dialog.text.clear();
            native_collision.triangles.clear();
            runtime.latest=Some(runtime.world.step(input.0));
        }
        steps += 1;
    }
}

fn sync_native_render_frame(
    mut commands:Commands,
    cod_active:Option<Res<Sm64CodActive>>,
    frame:Res<Sm64NativeRenderFrame>,
    mut cache:ResMut<Sm64NativeRenderCache>,
    mut meshes:ResMut<Assets<Mesh>>,
    mut materials:ResMut<Assets<StandardMaterial>>,
    mut images:ResMut<Assets<Image>>,
    mut last_tick:Local<u32>,
) {
    if cod_active.is_none() || frame.tick==0 || frame.tick==*last_tick {
        return;
    }
    *last_tick=frame.tick;

    for update in &frame.texture_updates {
        let mut image=Image::new(
            Extent3d {
                width:update.width,
                height:update.height,
                depth_or_array_layers:1,
            },
            TextureDimension::D2,
            update.rgba.clone(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        let mut sampler=ImageSamplerDescriptor::nearest();
        sampler.address_mode_u=ImageAddressMode::Repeat;
        sampler.address_mode_v=ImageAddressMode::Repeat;
        sampler.address_mode_w=ImageAddressMode::ClampToEdge;
        image.sampler=ImageSampler::Descriptor(sampler);

        if let Some(handle)=cache.textures.get(&update.id) {
            if let Some(existing)=images.get_mut(handle.id()) {
                *existing=image;
            }
        } else {
            let handle=images.add(image);
            cache.textures.insert(update.id,handle);
        }
    }

    let mut groups=HashMap::<(u32,bool),Vec<&sm64_native::NativeRenderTriangle>>::new();
    for triangle in &frame.triangles {
        let texture_id=triangle.texture_id.unwrap_or(u32::MAX);
        groups.entry((texture_id,triangle.alpha)).or_default().push(triangle);
    }

    let active_keys=groups.keys().copied().collect::<std::collections::HashSet<_>>();
    let stale=cache.batches.keys()
        .filter(|key|!active_keys.contains(key))
        .copied()
        .collect::<Vec<_>>();
    for key in stale {
        if let Some(batch)=cache.batches.remove(&key) {
            commands.entity(batch.entity).despawn();
            meshes.remove(batch.mesh.id());
            materials.remove(batch.material.id());
        }
    }

    for (key,triangles) in groups {
        let mut positions=Vec::<[f32;3]>::with_capacity(triangles.len()*3);
        let mut normals=Vec::<[f32;3]>::with_capacity(triangles.len()*3);
        let mut uvs=Vec::<[f32;2]>::with_capacity(triangles.len()*3);
        let mut colors=Vec::<[f32;4]>::with_capacity(triangles.len()*3);

        for triangle in triangles {
            let converted=[
                sm64_render_vec3(triangle.pos[0]),
                sm64_render_vec3(triangle.pos[1]),
                sm64_render_vec3(triangle.pos[2]),
            ];
            let a=Vec3::from_array(converted[0]);
            let b=Vec3::from_array(converted[1]);
            let d=Vec3::from_array(converted[2]);
            let normal=(b-a).cross(d-a).try_normalize().unwrap_or(Vec3::Z).to_array();

            for i in 0..3 {
                positions.push(converted[i]);
                normals.push(normal);
                uvs.push(triangle.uv[i]);
                colors.push([
                    triangle.rgba[i][0] as f32/255.0,
                    triangle.rgba[i][1] as f32/255.0,
                    triangle.rgba[i][2] as f32/255.0,
                    triangle.rgba[i][3] as f32/255.0,
                ]);
            }
        }

        let mesh_data=Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION,positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL,normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0,uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR,colors);

        if let Some(existing)=cache.batches.get(&key) {
            if let Some(mesh)=meshes.get_mut(&existing.mesh) {
                *mesh=mesh_data;
            }
            continue;
        }

        let texture=if key.0==u32::MAX {
            None
        } else {
            cache.textures.get(&key.0).cloned()
        };
        let material=materials.add(StandardMaterial {
            base_color:Color::WHITE,
            base_color_texture:texture,
            unlit:true,
            alpha_mode:if key.1 {AlphaMode::Blend} else {AlphaMode::Opaque},
            cull_mode:None,
            ..default()
        });
        let mesh=meshes.add(mesh_data);
        let entity=commands.spawn((
            Name::new(format!("SM64 native render batch tex={} alpha={}",key.0,key.1)),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::IDENTITY,
            Sm64NativeRenderPresentation,
            Sm64DebugWorld,
        )).id();
        cache.batches.insert(key,NativeRenderBatchHandles {
            entity,
            mesh,
            material,
        });
    }

    if frame.tick<=3 || frame.tick%300==0 {
        info!(
            "SM64 native renderer frame tick={} triangles={} textures={} batches={}",
            frame.tick,
            frame.triangles.len(),
            cache.textures.len(),
            cache.batches.len()
        );
    }
}

fn sync_sm64_dialog_overlay(
    mut commands:Commands,
    dialog:Res<Sm64NativeDialogOutput>,
    roots:Query<Entity,With<Sm64DialogRoot>>,
    mut texts:Query<&mut Text,With<Sm64DialogText>>,
) {
    let visible=dialog.id>=0 && !dialog.text.trim().is_empty();

    if !visible {
        for entity in &roots {
            commands.entity(entity).despawn();
        }
        return;
    }

    if let Some(mut text)=texts.iter_mut().next() {
        if dialog.is_changed() {
            *text=Text::new(format!(
                "{}\n\n[Use] Continue",
                dialog.text.trim()
            ));
        }
        return;
    }

    commands.spawn((
        Sm64DialogRoot,
        Node {
            position_type:PositionType::Absolute,
            left:Val::Percent(18.0),
            right:Val::Percent(18.0),
            bottom:Val::Percent(8.0),
            padding:UiRect::all(Val::Px(20.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.02,0.02,0.03,0.88)),
        GlobalZIndex(50_000),
    )).with_children(|parent|{
        parent.spawn((
            Sm64DialogText,
            Text::new(format!(
                "{}\n\n[Use] Continue",
                dialog.text.trim()
            )),
            TextFont {
                font_size:bevy::text::FontSize::Px(24.0),
                ..default()
            },
            TextColor(Color::WHITE),
            TextLayout::justify(Justify::Left),
        ));
    });
}

fn face_sm64_billboard_objects(
    cameras:Query<&GlobalTransform,With<Camera3d>>,
    mut billboards:Query<(&GlobalTransform,&mut Transform),With<Sm64BillboardObject>>,
) {
    let Some(camera)=cameras.iter().next() else {return;};
    let camera_pos=camera.translation();

    for (global,mut local) in &mut billboards {
        let delta=camera_pos-global.translation();
        let flat=Vec2::new(delta.x,delta.y);
        if flat.length_squared()<1e-6 {
            continue;
        }
        // Runtime GRAPH_RENDER_BILLBOARD is object-level and replaces the
        // object's authored facing with a camera-facing yaw.
        let yaw=flat.y.atan2(flat.x)-core::f32::consts::FRAC_PI_2;
        local.rotation=Quat::from_rotation_z(yaw);
    }
}

fn face_sm64_billboards(
    cameras:Query<&GlobalTransform,With<Camera3d>>,
    parent_transforms:Query<&GlobalTransform,Without<Sm64BillboardPart>>,
    mut billboards:Query<(&ChildOf,&GlobalTransform,&mut Transform),With<Sm64BillboardPart>>,
) {
    let Some(camera)=cameras.iter().next() else {return;};
    let camera_pos=camera.translation();

    for (child_of,global,mut local) in &mut billboards {
        let Ok(parent)=parent_transforms.get(child_of.parent()) else {continue;};
        let delta=camera_pos-global.translation();
        let flat=Vec2::new(delta.x,delta.y);
        if flat.length_squared()<1e-6 {
            continue;
        }

        // Billboard around IW4/Bevy Z-up while preserving the object's parent
        // translation/scale. This matches SM64's camera-facing sprite pieces.
        let yaw=flat.y.atan2(flat.x)-core::f32::consts::FRAC_PI_2;
        local.rotation=parent.rotation().inverse()*Quat::from_rotation_z(yaw);
    }
}

fn native_snapshot_to_sm64(
    native:sm64_native::NativeSnapshot,
    previous_mario:&sm64_core::MarioState,
    model_symbols:&HashMap<i32,String>,
)->Sm64Snapshot {
    let mut mario=previous_mario.clone();
    mario.global_timer=native.tick;
    mario.health=native.mario_health.clamp(i16::MIN as i32,i16::MAX as i32) as i16;
    mario.num_coins=native.coins.clamp(i16::MIN as i32,i16::MAX as i32) as i16;
    mario.action=native.mario_action;
    mario.pos=native.mario_pos;
    mario.vel=native.mario_vel;
    mario.face_angle[1]=native.mario_yaw;

    let objects=native.objects.into_iter().map(|source|{
        let model=model_symbols.get(&source.model_id)
            .cloned()
            .unwrap_or_else(||if source.model_id<=0 {
                // Native objects with no shared graph node are logic-only
                // objects (MODEL_NONE). Do not try to materialize them as
                // renderable MODEL_NATIVE_FFFFFFFF placeholders.
                "MODEL_NONE".to_owned()
            } else {
                format!("MODEL_NATIVE_{:02X}",source.model_id)
            });
        let mut object=sm64_core::Sm64Object::new(
            sm64_core::ObjectId(source.id),
            model,
            "native_decomp",
            sm64_core::ObjectList::Default,
            source.pos,
            source.face_angle,
        );
        object.scale=source.scale;
        object.render_flags=source.render_flags;
        object.anim_id=source.anim_id;
        object.anim_frame=source.anim_frame;
        object.anim_state=source.anim_state;
        object.interact_status=source.interact_status;
        object.damage_or_coin_value=source.damage_or_coin_value;
        object.active=source.active_flags!=0;
        object
    }).collect();

    Sm64Snapshot {
        tick:native.tick as u64,
        mario,
        objects,
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
    cod_active:Option<Res<Sm64CodActive>>,
    runtime:Res<Sm64Runtime>,
    mut last_tick:Local<u64>,
    status:Res<Sm64LoadStatus>,
    mut meshes:ResMut<Assets<Mesh>>,
    mut materials:ResMut<Assets<StandardMaterial>>,
    mut images:ResMut<Assets<Image>>,
    mut presentation_cache:ResMut<Sm64PresentationCache>,
    mut query:Query<(Entity,&Sm64ObjectPresentation,&mut Transform,&mut Visibility)>,
) {
    if cod_active.is_some() {
        return;
    }
    let Some(snapshot)=runtime.latest.as_ref() else {return;};
    if snapshot.tick==*last_tick {
        return;
    }
    *last_tick=snapshot.tick;
    let by_id=snapshot.objects.iter()
        .map(|object|(object.id,object))
        .collect::<HashMap<_,_>>();
    let mut presented=std::collections::HashSet::new();

    for (entity,presentation,mut transform,mut visibility) in &mut query {
        let Some(object)=by_id.get(&presentation.id) else {
            commands.entity(entity).despawn();
            continue;
        };

        // Geo switch cases and even the shared model can change at runtime.
        // Rebuild only when that authored render state changes; otherwise keep
        // the cached meshes and just update transform/visibility.
        if presentation.model!=object.model || presentation.anim_state!=object.anim_state {
            commands.entity(entity).despawn();
            continue;
        }

        presented.insert(presentation.id);
        transform.translation=sm64_render_vec3(object.pos);
        transform.rotation=sm64_presentation_rotation(&object.model,object.face_angle);
        transform.scale=Vec3::from_array(object.scale);
        *visibility=if (object.render_flags & 1)!=0 && (object.render_flags & (1<<4))==0 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }

    // Native SM64 behaviors create objects at runtime (coin formations,
    // particles, stars, enemy children, etc.). Initial-load-only presentation
    // silently made those objects invisible, so materialize any newly observed
    // simulation object here.
    let (Some(root),level)=(status.source_root.as_ref(),status.level.as_str()) else {return;};
    let missing=snapshot.objects.iter()
        .filter(|object|!presented.contains(&object.id) && object.model!="MODEL_NONE")
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        // Resolve the model registry and geometry caches once for the whole
        // native snapshot instead of reparsing the decomp once per object.
        spawn_runtime_object_presentations(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            root,
            level,
            &missing,
            &mut presentation_cache,
        );
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

use std::{
    env,
    ffi::{CStr, CString, c_char},
    fmt,
    path::{Path, PathBuf},
    slice,
};

use libloading::Library;

const ABI_VERSION: u32 = 5;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct NativePlayerProxy {
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    pub yaw: i16,
    pub pitch: i16,
    pub health: i32,
    pub attack_flags: u32,
}

#[derive(Clone, Debug, Default)]
pub struct NativeObject {
    pub id: u32,
    pub model_id: i32,
    pub pos: [f32; 3],
    pub face_angle: [i16; 3],
    pub scale: [f32; 3],
    pub active_flags: u16,
    pub render_flags: u16,
    pub anim_id: i16,
    pub anim_frame: i16,
    pub anim_state: i32,
    pub interact_status: u32,
    pub damage_or_coin_value: i32,
}

#[derive(Clone, Debug, Default)]
pub struct NativeRenderTriangle {
    pub pos: [[f32;3];3],
    pub uv: [[f32;2];3],
    pub rgba: [[u8;4];3],
    pub texture_id: Option<u32>,
    pub alpha: bool,
    pub screen_space: bool,
    /// Native N64 tile wrap bits (G_TX_MIRROR=1, G_TX_CLAMP=2).
    pub wrap_s: u8,
    pub wrap_t: u8,
}

#[derive(Clone, Debug)]
pub struct NativeTextureUpdate {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub generation: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct NativeSnapshot {
    pub tick: u32,
    pub area_index: u32,
    pub mario_health: i32,
    pub coins: i32,
    pub mario_action: u32,
    pub mario_pos: [f32; 3],
    pub mario_vel: [f32; 3],
    pub mario_yaw: i16,
    pub dialog_id: i16,
    pub dialog_text: String,
    pub objects: Vec<NativeObject>,
    pub dynamic_surfaces: Vec<[[f32; 3]; 3]>,
    pub render_triangles: Vec<NativeRenderTriangle>,
    pub texture_updates: Vec<NativeTextureUpdate>,
    pub audio_samples: Vec<i16>,
    pub audio_sample_rate: u32,
    pub audio_channels: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NativeObjectView {
    id: u32,
    model_id: i32,
    pos: [f32; 3],
    face_angle: [i16; 3],
    scale: [f32; 3],
    active_flags: u16,
    render_flags: u16,
    anim_id: i16,
    anim_frame: i16,
    anim_state: i32,
    interact_status: u32,
    damage_or_coin_value: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NativeTriangleView {
    vertices: [f32; 9],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NativeRenderTriangleView {
    pos: [f32;9],
    uv: [f32;6],
    rgba: [u8;12],
    texture_id: u32,
    textured: u8,
    alpha: u8,
    wrap_s: u8,
    wrap_t: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NativeTextureView {
    id: u32,
    width: u32,
    height: u32,
    generation: u32,
    rgba: *const u8,
}

#[repr(C)]
struct NativeSnapshotView {
    abi_version: u32,
    tick: u32,
    area_index: u32,
    mario_health: i32,
    coins: i32,
    mario_action: u32,
    mario_pos: [f32; 3],
    mario_vel: [f32; 3],
    mario_yaw: i16,
    dialog_id: i16,
    dialog_text: *const c_char,
    dialog_text_len: u32,
    objects: *const NativeObjectView,
    object_count: u32,
    dynamic_surfaces: *const NativeTriangleView,
    dynamic_surface_count: u32,
    render_triangles: *const NativeRenderTriangleView,
    render_triangle_count: u32,
    textures: *const NativeTextureView,
    texture_count: u32,
    audio_samples: *const i16,
    audio_frame_count: u32,
    audio_sample_rate: u32,
    audio_channels: u16,
    audio_reserved: u16,
}

type AbiVersionFn = unsafe extern "C" fn() -> u32;
type InitFn = unsafe extern "C" fn(*const c_char, i32, i32) -> i32;
type StepFn = unsafe extern "C" fn(*const NativePlayerProxy) -> *const NativeSnapshotView;
type ShutdownFn = unsafe extern "C" fn();
type LastErrorFn = unsafe extern "C" fn() -> *const c_char;

#[derive(Debug)]
pub struct NativeBridgeError(String);

impl NativeBridgeError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for NativeBridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NativeBridgeError {}

pub fn native_root(asset_root: impl AsRef<Path>) -> PathBuf {
    if let Some(path) = env::var_os("SM64_NATIVE_ROOT") {
        return PathBuf::from(path);
    }

    let asset_root = asset_root.as_ref();
    if asset_root.join("src").join("pc").join("pc_main.c").is_file() {
        return asset_root.to_path_buf();
    }

    if let Some(parent) = asset_root.parent() {
        for name in ["sm64-port", "sm64_port", "sm64-port-master"] {
            let candidate = parent.join(name);
            if candidate.join("src").join("pc").join("pc_main.c").is_file() {
                return candidate;
            }
        }
    }

    asset_root.to_path_buf()
}

pub fn default_module_path(asset_root: impl AsRef<Path>) -> PathBuf {
    if let Some(path) = env::var_os("SM64_NATIVE_MODULE") {
        return PathBuf::from(path);
    }
    if let Some(path) = env::var_os("SM64_NATIVE_BRIDGE") {
        // Keep the old environment variable useful during the migration when
        // it is explicitly pointed at the new DLL.
        let path = PathBuf::from(path);
        if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("dll")) {
            return path;
        }
    }

    native_root(asset_root)
        .join("build")
        .join("us_bridge")
        .join("iw4l-sm64-native.dll")
}

/// Compatibility alias for callers that still use the previous helper name.
pub fn default_bridge_path(asset_root: impl AsRef<Path>) -> PathBuf {
    default_module_path(asset_root)
}

pub struct NativeClient {
    // Keep the module loaded for at least as long as every copied function
    // pointer below.
    _library: Library,
    step_fn: StepFn,
    shutdown_fn: ShutdownFn,
    last_error_fn: LastErrorFn,
    module_path: PathBuf,
    texture_generations: std::collections::HashMap<u32,u32>,
}

impl NativeClient {
    pub fn available(decomp_root: impl AsRef<Path>) -> bool {
        default_module_path(decomp_root).is_file()
    }

    pub fn launch(
        decomp_root: impl AsRef<Path>,
        level: &str,
        area: u8,
        act: u8,
    ) -> Result<Self, NativeBridgeError> {
        let asset_root = decomp_root.as_ref();
        let module_path = default_module_path(asset_root);
        if !module_path.is_file() {
            return Err(NativeBridgeError::new(format!(
                "embedded SM64 native module not found at {}. Rebuild it with tools/sm64_native_bridge/build.ps1",
                module_path.display()
            )));
        }

        let level = CString::new(level)
            .map_err(|_| NativeBridgeError::new("SM64 level name contains an interior NUL"))?;

        // SAFETY: the module is produced by this repository's build script and
        // all symbols are checked before any call is made.
        let library = unsafe { Library::new(&module_path) }.map_err(|error| {
            NativeBridgeError::new(format!(
                "failed to load embedded SM64 module {}: {error}",
                module_path.display()
            ))
        })?;

        let (abi_version_fn, init_fn, step_fn, shutdown_fn, last_error_fn) = unsafe {
            let abi = *library
                .get::<AbiVersionFn>(b"iw4l_sm64_abi_version\0")
                .map_err(|error| symbol_error(&module_path, "iw4l_sm64_abi_version", error))?;
            let init = *library
                .get::<InitFn>(b"iw4l_sm64_init\0")
                .map_err(|error| symbol_error(&module_path, "iw4l_sm64_init", error))?;
            let step = *library
                .get::<StepFn>(b"iw4l_sm64_step\0")
                .map_err(|error| symbol_error(&module_path, "iw4l_sm64_step", error))?;
            let shutdown = *library
                .get::<ShutdownFn>(b"iw4l_sm64_shutdown\0")
                .map_err(|error| symbol_error(&module_path, "iw4l_sm64_shutdown", error))?;
            let last_error = *library
                .get::<LastErrorFn>(b"iw4l_sm64_last_error\0")
                .map_err(|error| symbol_error(&module_path, "iw4l_sm64_last_error", error))?;
            (abi, init, step, shutdown, last_error)
        };

        let abi = unsafe { abi_version_fn() };
        if abi != ABI_VERSION {
            return Err(NativeBridgeError::new(format!(
                "embedded SM64 ABI mismatch: Rust expects {ABI_VERSION}, module reports {abi}"
            )));
        }

        let initialized = unsafe { init_fn(level.as_ptr(), i32::from(area), i32::from(act)) };
        if initialized == 0 {
            return Err(NativeBridgeError::new(format!(
                "embedded SM64 initialization failed: {}",
                unsafe { native_error(last_error_fn) }
            )));
        }

        Ok(Self {
            _library: library,
            step_fn,
            shutdown_fn,
            last_error_fn,
            module_path,
            texture_generations: std::collections::HashMap::new(),
        })
    }

    pub fn module_path(&self) -> &Path {
        &self.module_path
    }

    pub fn step(
        &mut self,
        player: NativePlayerProxy,
    ) -> Result<NativeSnapshot, NativeBridgeError> {
        let view = unsafe { (self.step_fn)(&raw const player) };
        if view.is_null() {
            return Err(NativeBridgeError::new(format!(
                "embedded SM64 step failed in {}: {}",
                self.module_path.display(),
                unsafe { native_error(self.last_error_fn) }
            )));
        }

        // SAFETY: the native module owns this stable snapshot view and its
        // backing arrays until the next step. We copy everything before
        // returning to Bevy.
        let view = unsafe { &*view };
        if view.abi_version != ABI_VERSION {
            return Err(NativeBridgeError::new(format!(
                "embedded SM64 snapshot ABI mismatch: expected {ABI_VERSION}, got {}",
                view.abi_version
            )));
        }
        if view.object_count > 4096 {
            return Err(NativeBridgeError::new(format!(
                "embedded SM64 returned impossible object count {}",
                view.object_count
            )));
        }
        if view.dynamic_surface_count > 8192 {
            return Err(NativeBridgeError::new(format!(
                "embedded SM64 returned impossible dynamic surface count {}",
                view.dynamic_surface_count
            )));
        }
        if view.render_triangle_count > 65_536 {
            return Err(NativeBridgeError::new(format!(
                "embedded SM64 returned impossible render triangle count {}",
                view.render_triangle_count
            )));
        }
        if view.texture_count > 512 {
            return Err(NativeBridgeError::new(format!(
                "embedded SM64 returned impossible texture count {}",
                view.texture_count
            )));
        }
        if view.dialog_text_len > 4095 {
            return Err(NativeBridgeError::new(format!(
                "embedded SM64 returned impossible dialog length {}",
                view.dialog_text_len
            )));
        }

        if view.audio_frame_count > 2048
            || view.audio_channels > 2
            || (view.audio_frame_count != 0
                && !(8_000..=192_000).contains(&view.audio_sample_rate))
        {
            return Err(NativeBridgeError::new(format!(
                "embedded SM64 returned invalid audio chunk frames={} rate={} channels={}",
                view.audio_frame_count, view.audio_sample_rate, view.audio_channels
            )));
        }

        let dialog_text = if view.dialog_text.is_null() || view.dialog_text_len == 0 {
            String::new()
        } else {
            let bytes = unsafe {
                slice::from_raw_parts(view.dialog_text.cast::<u8>(), view.dialog_text_len as usize)
            };
            String::from_utf8_lossy(bytes).into_owned()
        };

        let object_views = if view.objects.is_null() || view.object_count == 0 {
            &[][..]
        } else {
            unsafe { slice::from_raw_parts(view.objects, view.object_count as usize) }
        };
        let objects = object_views
            .iter()
            .map(|source| NativeObject {
                id: source.id,
                model_id: source.model_id,
                pos: source.pos,
                face_angle: source.face_angle,
                scale: source.scale,
                active_flags: source.active_flags,
                render_flags: source.render_flags,
                anim_id: source.anim_id,
                anim_frame: source.anim_frame,
                anim_state: source.anim_state,
                interact_status: source.interact_status,
                damage_or_coin_value: source.damage_or_coin_value,
            })
            .collect();

        let triangle_views = if view.dynamic_surfaces.is_null() || view.dynamic_surface_count == 0 {
            &[][..]
        } else {
            unsafe {
                slice::from_raw_parts(
                    view.dynamic_surfaces,
                    view.dynamic_surface_count as usize,
                )
            }
        };
        let dynamic_surfaces = triangle_views
            .iter()
            .map(|source| {
                [
                    [source.vertices[0], source.vertices[1], source.vertices[2]],
                    [source.vertices[3], source.vertices[4], source.vertices[5]],
                    [source.vertices[6], source.vertices[7], source.vertices[8]],
                ]
            })
            .collect();

        let render_views = if view.render_triangles.is_null() || view.render_triangle_count == 0 {
            &[][..]
        } else {
            unsafe {
                slice::from_raw_parts(
                    view.render_triangles,
                    view.render_triangle_count as usize,
                )
            }
        };
        let render_triangles=render_views.iter().map(|source|{
            NativeRenderTriangle {
                pos:[
                    [source.pos[0],source.pos[1],source.pos[2]],
                    [source.pos[3],source.pos[4],source.pos[5]],
                    [source.pos[6],source.pos[7],source.pos[8]],
                ],
                uv:[
                    [source.uv[0],source.uv[1]],
                    [source.uv[2],source.uv[3]],
                    [source.uv[4],source.uv[5]],
                ],
                rgba:[
                    [source.rgba[0],source.rgba[1],source.rgba[2],source.rgba[3]],
                    [source.rgba[4],source.rgba[5],source.rgba[6],source.rgba[7]],
                    [source.rgba[8],source.rgba[9],source.rgba[10],source.rgba[11]],
                ],
                texture_id:(source.textured!=0 && source.texture_id!=u32::MAX)
                    .then_some(source.texture_id),
                alpha:source.alpha & 1 != 0,
                screen_space:source.alpha & 2 != 0,
                wrap_s:source.wrap_s,
                wrap_t:source.wrap_t,
            }
        }).collect();

        let texture_views = if view.textures.is_null() || view.texture_count == 0 {
            &[][..]
        } else {
            unsafe { slice::from_raw_parts(view.textures,view.texture_count as usize) }
        };
        let mut texture_updates=Vec::new();
        for source in texture_views {
            let known=self.texture_generations.get(&source.id).copied().unwrap_or(0);
            if source.generation==known || source.rgba.is_null() || source.width==0 || source.height==0 {
                continue;
            }
            let byte_len=(source.width as usize)
                .saturating_mul(source.height as usize)
                .saturating_mul(4);
            if byte_len==0 || byte_len>64*1024*1024 {
                continue;
            }
            let rgba=unsafe { slice::from_raw_parts(source.rgba,byte_len) }.to_vec();
            self.texture_generations.insert(source.id,source.generation);
            texture_updates.push(NativeTextureUpdate {
                id:source.id,
                width:source.width,
                height:source.height,
                generation:source.generation,
                rgba,
            });
        }

        let audio_sample_count = (view.audio_frame_count as usize)
            .saturating_mul(view.audio_channels as usize);
        let audio_samples = if audio_sample_count == 0 || view.audio_samples.is_null() {
            Vec::new()
        } else {
            unsafe { slice::from_raw_parts(view.audio_samples, audio_sample_count) }.to_vec()
        };

        Ok(NativeSnapshot {
            tick: view.tick,
            area_index: view.area_index,
            mario_health: view.mario_health,
            coins: view.coins,
            mario_action: view.mario_action,
            mario_pos: view.mario_pos,
            mario_vel: view.mario_vel,
            mario_yaw: view.mario_yaw,
            dialog_id: view.dialog_id,
            dialog_text,
            objects,
            dynamic_surfaces,
            render_triangles,
            texture_updates,
            audio_samples,
            audio_sample_rate: view.audio_sample_rate,
            audio_channels: view.audio_channels,
        })
    }
}

impl Drop for NativeClient {
    fn drop(&mut self) {
        unsafe {
            (self.shutdown_fn)();
        }
    }
}

fn symbol_error(
    module: &Path,
    symbol: &str,
    error: libloading::Error,
) -> NativeBridgeError {
    NativeBridgeError::new(format!(
        "embedded SM64 module {} is missing {symbol}: {error}",
        module.display()
    ))
}

unsafe fn native_error(last_error_fn: LastErrorFn) -> String {
    let ptr = unsafe { last_error_fn() };
    if ptr.is_null() {
        return "unknown native error".to_owned();
    }
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}

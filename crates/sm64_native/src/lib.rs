use std::{
    env,
    ffi::{CStr, CString, c_char},
    fmt,
    path::{Path, PathBuf},
    slice,
};

use libloading::Library;

const ABI_VERSION: u32 = 1;

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
pub struct NativeSnapshot {
    pub tick: u32,
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
struct NativeSnapshotView {
    abi_version: u32,
    tick: u32,
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
        })
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
        if view.dialog_text_len > 4095 {
            return Err(NativeBridgeError::new(format!(
                "embedded SM64 returned impossible dialog length {}",
                view.dialog_text_len
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

        Ok(NativeSnapshot {
            tick: view.tick,
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

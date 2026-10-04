use std::{
    env,
    fmt,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

const REQUEST_MAGIC: u32 = 0x3151_4D53; // "SMQ1"
const SNAPSHOT_MAGIC: u32 = 0x3153_4D53; // "SMS1"
const OP_STEP: u32 = 1;
const OP_SHUTDOWN: u32 = 2;

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
    pub objects: Vec<NativeObject>,
}

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
    env::var_os("SM64_NATIVE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| asset_root.as_ref().to_path_buf())
}

pub fn default_bridge_path(asset_root: impl AsRef<Path>) -> PathBuf {
    if let Some(path) = env::var_os("SM64_NATIVE_BRIDGE") {
        return PathBuf::from(path);
    }
    let root = native_root(asset_root);
    #[cfg(windows)]
    {
        root.join("build")
            .join("us_bridge")
            .join("iw4l-sm64-bridge.exe")
    }
    #[cfg(not(windows))]
    {
        root.join("build").join("us_bridge").join("iw4l-sm64-bridge")
    }
}

pub struct NativeClient {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl NativeClient {
    pub fn available(decomp_root: impl AsRef<Path>) -> bool {
        default_bridge_path(decomp_root).is_file()
    }

    pub fn launch(
        decomp_root: impl AsRef<Path>,
        level: &str,
        area: u8,
        act: u8,
    ) -> Result<Self, NativeBridgeError> {
        let asset_root = decomp_root.as_ref();
        let root = native_root(asset_root);
        let bridge = default_bridge_path(asset_root);
        if !bridge.is_file() {
            return Err(NativeBridgeError::new(format!(
                "native SM64 bridge not found at {}",
                bridge.display()
            )));
        }
        let mut child = Command::new(&bridge)
            .arg("--level")
            .arg(level)
            .arg("--area")
            .arg(area.to_string())
            .arg("--act")
            .arg(act.to_string())
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| {
                NativeBridgeError::new(format!("failed to launch {}: {error}", bridge.display()))
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| NativeBridgeError::new("native bridge stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| NativeBridgeError::new("native bridge stdout unavailable"))?;
        Ok(Self {
            child,
            stdin,
            stdout,
        })
    }

    pub fn step(
        &mut self,
        player: NativePlayerProxy,
    ) -> Result<NativeSnapshot, NativeBridgeError> {
        write_u32(&mut self.stdin, REQUEST_MAGIC)?;
        write_u32(&mut self.stdin, OP_STEP)?;
        for value in player.pos {
            write_f32(&mut self.stdin, value)?;
        }
        for value in player.vel {
            write_f32(&mut self.stdin, value)?;
        }
        write_i16(&mut self.stdin, player.yaw)?;
        write_i16(&mut self.stdin, player.pitch)?;
        write_i32(&mut self.stdin, player.health)?;
        write_u32(&mut self.stdin, player.attack_flags)?;
        self.stdin
            .flush()
            .map_err(|error| NativeBridgeError::new(format!("native bridge flush failed: {error}")))?;

        let magic = read_u32(&mut self.stdout)?;
        if magic != SNAPSHOT_MAGIC {
            return Err(NativeBridgeError::new(format!(
                "native bridge protocol mismatch: expected 0x{SNAPSHOT_MAGIC:08x}, got 0x{magic:08x}"
            )));
        }
        let tick = read_u32(&mut self.stdout)?;
        let object_count = read_u32(&mut self.stdout)? as usize;
        let mario_health = read_i32(&mut self.stdout)?;
        let coins = read_i32(&mut self.stdout)?;
        let mario_action = read_u32(&mut self.stdout)?;
        let mut mario_pos=[0.0;3];
        for value in &mut mario_pos {
            *value=read_f32(&mut self.stdout)?;
        }
        let mut mario_vel=[0.0;3];
        for value in &mut mario_vel {
            *value=read_f32(&mut self.stdout)?;
        }
        let mario_yaw=read_i16(&mut self.stdout)?;
        let _reserved=read_u16(&mut self.stdout)?;

        if object_count > 4096 {
            return Err(NativeBridgeError::new(format!(
                "native bridge returned impossible object count {object_count}"
            )));
        }

        let mut objects = Vec::with_capacity(object_count);
        for _ in 0..object_count {
            let id = read_u32(&mut self.stdout)?;
            let model_id = read_i32(&mut self.stdout)?;
            let mut pos = [0.0; 3];
            for value in &mut pos {
                *value = read_f32(&mut self.stdout)?;
            }
            let mut face_angle = [0; 3];
            for value in &mut face_angle {
                *value = read_i16(&mut self.stdout)?;
            }
            let mut scale = [0.0; 3];
            for value in &mut scale {
                *value = read_f32(&mut self.stdout)?;
            }
            let active_flags = read_u16(&mut self.stdout)?;
            let interact_status = read_u32(&mut self.stdout)?;
            let damage_or_coin_value = read_i32(&mut self.stdout)?;
            objects.push(NativeObject {
                id,
                model_id,
                pos,
                face_angle,
                scale,
                active_flags,
                interact_status,
                damage_or_coin_value,
            });
        }

        Ok(NativeSnapshot {
            tick,
            mario_health,
            coins,
            mario_action,
            mario_pos,
            mario_vel,
            mario_yaw,
            objects,
        })
    }
}

impl Drop for NativeClient {
    fn drop(&mut self) {
        let _ = write_u32(&mut self.stdin, REQUEST_MAGIC);
        let _ = write_u32(&mut self.stdin, OP_SHUTDOWN);
        for _ in 0..3 {
            let _ = write_f32(&mut self.stdin, 0.0);
        }
        for _ in 0..3 {
            let _ = write_f32(&mut self.stdin, 0.0);
        }
        let _ = write_i16(&mut self.stdin, 0);
        let _ = write_u16(&mut self.stdin, 0);
        let _ = write_i32(&mut self.stdin, 0);
        let _ = write_u32(&mut self.stdin, 0);
        let _ = self.stdin.flush();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn write_u32(writer: &mut impl Write, value: u32) -> Result<(), NativeBridgeError> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(io_error)
}

fn write_i32(writer: &mut impl Write, value: i32) -> Result<(), NativeBridgeError> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(io_error)
}

fn write_u16(writer: &mut impl Write, value: u16) -> Result<(), NativeBridgeError> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(io_error)
}

fn write_i16(writer: &mut impl Write, value: i16) -> Result<(), NativeBridgeError> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(io_error)
}

fn write_f32(writer: &mut impl Write, value: f32) -> Result<(), NativeBridgeError> {
    writer
        .write_all(&value.to_bits().to_le_bytes())
        .map_err(io_error)
}

fn read_u32(reader: &mut impl Read) -> Result<u32, NativeBridgeError> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes).map_err(io_error)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_i32(reader: &mut impl Read) -> Result<i32, NativeBridgeError> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes).map_err(io_error)?;
    Ok(i32::from_le_bytes(bytes))
}

fn read_u16(reader: &mut impl Read) -> Result<u16, NativeBridgeError> {
    let mut bytes = [0; 2];
    reader.read_exact(&mut bytes).map_err(io_error)?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_i16(reader: &mut impl Read) -> Result<i16, NativeBridgeError> {
    let mut bytes = [0; 2];
    reader.read_exact(&mut bytes).map_err(io_error)?;
    Ok(i16::from_le_bytes(bytes))
}

fn read_f32(reader: &mut impl Read) -> Result<f32, NativeBridgeError> {
    Ok(f32::from_bits(read_u32(reader)?))
}

fn io_error(error: std::io::Error) -> NativeBridgeError {
    NativeBridgeError::new(format!("native bridge I/O failed: {error}"))
}

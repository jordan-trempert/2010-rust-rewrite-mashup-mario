use std::{
    collections::VecDeque,
    env,
    fmt,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
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
    pub objects: Vec<NativeObject>,
    pub dynamic_surfaces: Vec<[[f32; 3]; 3]>,
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
    if let Some(path)=env::var_os("SM64_NATIVE_ROOT") {
        return PathBuf::from(path);
    }

    let asset_root=asset_root.as_ref();
    if asset_root.join("src").join("pc").join("pc_main.c").is_file() {
        return asset_root.to_path_buf();
    }

    if let Some(parent)=asset_root.parent() {
        for name in ["sm64-port","sm64_port","sm64-port-master"] {
            let candidate=parent.join(name);
            if candidate.join("src").join("pc").join("pc_main.c").is_file() {
                return candidate;
            }
        }
    }

    asset_root.to_path_buf()
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
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    bridge_path: PathBuf,
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
            .stderr(Stdio::piped())
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
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| NativeBridgeError::new("native bridge stderr unavailable"))?;
        let stderr_tail=Arc::new(Mutex::new(VecDeque::with_capacity(64)));
        let stderr_tail_writer=Arc::clone(&stderr_tail);
        thread::Builder::new()
            .name("sm64-native-stderr".to_owned())
            .spawn(move || {
                let reader=BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    eprintln!("{line}");
                    if let Ok(mut tail)=stderr_tail_writer.lock() {
                        if tail.len()>=64 {
                            tail.pop_front();
                        }
                        tail.push_back(line);
                    }
                }
            })
            .map_err(|error| NativeBridgeError::new(format!(
                "failed to start native bridge stderr reader: {error}"
            )))?;
        Ok(Self {
            child,
            stdin,
            stdout,
            stderr_tail,
            bridge_path: bridge,
        })
    }

    pub fn step(
        &mut self,
        player: NativePlayerProxy,
    ) -> Result<NativeSnapshot, NativeBridgeError> {
        match self.step_inner(player) {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                let child_state = match self.child.try_wait() {
                    Ok(Some(status)) => format!("; native bridge exited with {status}"),
                    Ok(None) => "; native bridge is still running".to_owned(),
                    Err(status_error) => format!(
                        "; failed to query native bridge process status: {status_error}"
                    ),
                };
                let tail_lines=self.stderr_tail
                    .lock()
                    .ok()
                    .map(|tail|tail.iter().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();
                let native_tail=if tail_lines.is_empty() {
                    String::new()
                } else {
                    format!("; native stderr tail: {}",tail_lines.join(" | "))
                };
                let symbolized=symbolize_native_fault(&self.bridge_path,&tail_lines)
                    .map(|value|format!("; native symbol: {value}"))
                    .unwrap_or_default();
                Err(NativeBridgeError::new(format!(
                    "{error}{child_state}{native_tail}{symbolized}"
                )))
            }
        }
    }

    fn step_inner(
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
        let dynamic_surface_count = read_u32(&mut self.stdout)? as usize;
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
        if dynamic_surface_count > 8192 {
            return Err(NativeBridgeError::new(format!(
                "native bridge returned impossible dynamic surface count {dynamic_surface_count}"
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
            let render_flags = read_u16(&mut self.stdout)?;
            let anim_id = read_i16(&mut self.stdout)?;
            let anim_frame = read_i16(&mut self.stdout)?;
            let anim_state = read_i32(&mut self.stdout)?;
            let interact_status = read_u32(&mut self.stdout)?;
            let damage_or_coin_value = read_i32(&mut self.stdout)?;
            objects.push(NativeObject {
                id,
                model_id,
                pos,
                face_angle,
                scale,
                active_flags,
                render_flags,
                anim_id,
                anim_frame,
                anim_state,
                interact_status,
                damage_or_coin_value,
            });
        }

        let mut dynamic_surfaces=Vec::with_capacity(dynamic_surface_count);
        for _ in 0..dynamic_surface_count {
            let mut tri=[[0.0;3];3];
            for vertex in &mut tri {
                for axis in vertex {
                    *axis=read_f32(&mut self.stdout)?;
                }
            }
            dynamic_surfaces.push(tri);
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
            dynamic_surfaces,
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


fn symbolize_native_fault(
    bridge:&Path,
    stderr_lines:&[String],
)->Option<String>{
    let address=stderr_lines.iter().rev().find_map(|line|{
        let marker="address=";
        let start=line.find(marker)?+marker.len();
        let tail=&line[start..];
        let end=tail.find(char::is_whitespace).unwrap_or(tail.len());
        let value=&tail[..end];
        if value.starts_with("0x") || value.starts_with("0X") {
            Some(value.to_owned())
        } else if value.chars().all(|ch|ch.is_ascii_hexdigit()) {
            Some(format!("0x{value}"))
        } else {
            None
        }
    })?;

    let mut candidates=vec![address.clone()];
    // MinGW PE executables normally load at 0x140000000. Some addr2line
    // builds expect the RVA rather than the process virtual address.
    if let Some(hex)=address.strip_prefix("0x").or_else(||address.strip_prefix("0X")) {
        if let Ok(value)=u64::from_str_radix(hex,16) {
            const PE_IMAGE_BASE:u64=0x1_4000_0000;
            if value>=PE_IMAGE_BASE {
                candidates.push(format!("0x{:x}",value-PE_IMAGE_BASE));
            }
        }
    }

    for tool in ["addr2line","x86_64-w64-mingw32-addr2line"] {
        for candidate in &candidates {
            let output=Command::new(tool)
                .arg("-f")
                .arg("-C")
                .arg("-e")
                .arg(bridge)
                .arg(candidate)
                .output();
            let Ok(output)=output else {continue;};
            if !output.status.success() {continue;}
            let text=String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|line|!line.is_empty())
                .collect::<Vec<_>>()
                .join(" @ ");
            if !text.is_empty() && !text.contains("??") {
                return Some(format!("{address} ({candidate}) => {text}"));
            }
        }
    }
    None
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

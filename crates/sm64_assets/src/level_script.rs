use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarioSpawn {
    pub area: u8,
    pub yaw_degrees: i16,
    pub pos: [i16; 3],
}

impl MarioSpawn {
    pub fn yaw_sm64(self) -> i16 {
        ((self.yaw_degrees as i32 * 0x10000 / 360) as u16) as i16
    }
}

pub fn load_mario_spawn(
    decomp_root: impl AsRef<Path>,
    level: &str,
    area: u8,
) -> Result<MarioSpawn, crate::CollisionParseError> {
    let path=decomp_root.as_ref().join("levels").join(level).join("script.c");
    let text=std::fs::read_to_string(&path)
        .map_err(|e|crate::CollisionParseError::new(format!("{}: {e}",path.display())))?;
    parse_mario_spawn(&text,area).ok_or_else(||crate::CollisionParseError::new(
        format!("{}: no MARIO_POS for area {area}",path.display())
    ))
}

pub fn parse_mario_spawn(source:&str, area:u8)->Option<MarioSpawn> {
    for raw in source.lines() {
        let line=remove_block_comments(raw);
        let line=line.trim();
        let args=macro_args(line,"MARIO_POS")?;
        let parts=args.split(',').map(str::trim).collect::<Vec<_>>();
        if parts.len()!=5 { continue; }
        let parsed_area=parse_i16(parts[0])?;
        if parsed_area as u8 != area { continue; }
        return Some(MarioSpawn {
            area,
            yaw_degrees: parse_i16(parts[1])?,
            pos: [parse_i16(parts[2])?,parse_i16(parts[3])?,parse_i16(parts[4])?],
        });
    }
    None
}

fn macro_args<'a>(line:&'a str,name:&str)->Option<&'a str>{
    let rest=line.strip_prefix(name)?.trim_start().strip_prefix('(')?;
    let end=rest.rfind(')')?;
    Some(&rest[..end])
}

fn parse_i16(text:&str)->Option<i16>{
    let t=text.trim();
    if let Some(hex)=t.strip_prefix("0x") {
        i32::from_str_radix(hex,16).ok().and_then(|v|i16::try_from(v).ok())
    } else {
        t.parse::<i16>().ok()
    }
}

fn remove_block_comments(line:&str)->String {
    let mut out=String::with_capacity(line.len());
    let mut rest=line;
    loop {
        let Some(start)=rest.find("/*") else { out.push_str(rest); break; };
        out.push_str(&rest[..start]);
        let after=&rest[start+2..];
        let Some(end)=after.find("*/") else { break; };
        rest=&after[end+2..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_decomp_mario_pos() {
        let src="MARIO_POS(/*area*/ 1, /*yaw*/ 135, /*pos*/ -6558, 0, 6464),";
        let spawn=parse_mario_spawn(src,1).unwrap();
        assert_eq!(spawn.pos,[-6558,0,6464]);
        assert_eq!(spawn.yaw_degrees,135);
        assert_eq!(spawn.yaw_sm64(),0x6000);
    }
}

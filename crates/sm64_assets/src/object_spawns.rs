use std::{fs, path::Path};

use crate::CollisionParseError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelObjectSpawn {
    pub model: String,
    pub position: [i16;3],
    pub angles_deg: [i16;3],
    pub behavior_param_expr: String,
    pub behavior: String,
    pub acts_expr: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroObjectSpawn {
    pub preset: String,
    pub yaw_deg: i16,
    pub position: [i16;3],
    pub behavior_param_expr: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ParsedObjectSpawns {
    pub level_objects: Vec<LevelObjectSpawn>,
    pub macro_objects: Vec<MacroObjectSpawn>,
}

pub fn load_level_object_spawns(
    decomp_root: impl AsRef<Path>,
    level:&str,
    area:u8,
)->Result<ParsedObjectSpawns,CollisionParseError>{
    let root=decomp_root.as_ref();
    let script_path=root.join("levels").join(level).join("script.c");
    let macro_path=root.join("levels").join(level).join("areas").join(area.to_string()).join("macro.inc.c");

    let script=fs::read_to_string(&script_path)
        .map_err(|e|CollisionParseError::new(format!("{}: {e}",script_path.display())))?;
    let macro_source=fs::read_to_string(&macro_path)
        .map_err(|e|CollisionParseError::new(format!("{}: {e}",macro_path.display())))?;

    Ok(ParsedObjectSpawns{
        level_objects:parse_level_script_objects(&script),
        macro_objects:parse_macro_objects(&macro_source),
    })
}

pub fn parse_level_script_objects(source:&str)->Vec<LevelObjectSpawn>{
    let cleaned=strip_comments(source);
    cleaned.lines().filter_map(|raw|{
        let line=raw.trim();
        let (args,with_acts)=if let Some(args)=macro_args(line,"OBJECT_WITH_ACTS"){
            (args,true)
        }else if let Some(args)=macro_args(line,"OBJECT"){
            (args,false)
        }else{
            return None;
        };

        let parts=split_args(args);
        let required=if with_acts{10}else{9};
        if parts.len()<required{return None;}

        Some(LevelObjectSpawn{
            model:parts[0].trim().to_owned(),
            position:[
                parse_i16(parts[1])?,
                parse_i16(parts[2])?,
                parse_i16(parts[3])?,
            ],
            angles_deg:[
                parse_i16(parts[4])?,
                parse_i16(parts[5])?,
                parse_i16(parts[6])?,
            ],
            behavior_param_expr:parts[7].trim().to_owned(),
            behavior:parts[8].trim().to_owned(),
            acts_expr:with_acts.then(||parts[9].trim().to_owned()),
        })
    }).collect()
}

pub fn parse_macro_objects(source:&str)->Vec<MacroObjectSpawn>{
    let cleaned=strip_comments(source);
    cleaned.lines().filter_map(|raw|{
        let line=raw.trim();
        if let Some(args)=macro_args(line,"MACRO_OBJECT_WITH_BHV_PARAM"){
            let parts=split_args(args);
            if parts.len()<6{return None;}
            return Some(MacroObjectSpawn{
                preset:parts[0].trim().to_owned(),
                yaw_deg:parse_i16(parts[1])?,
                position:[
                    parse_i16(parts[2])?,
                    parse_i16(parts[3])?,
                    parse_i16(parts[4])?,
                ],
                behavior_param_expr:Some(parts[5].trim().to_owned()),
            });
        }
        if let Some(args)=macro_args(line,"MACRO_OBJECT"){
            let parts=split_args(args);
            if parts.len()<5{return None;}
            return Some(MacroObjectSpawn{
                preset:parts[0].trim().to_owned(),
                yaw_deg:parse_i16(parts[1])?,
                position:[
                    parse_i16(parts[2])?,
                    parse_i16(parts[3])?,
                    parse_i16(parts[4])?,
                ],
                behavior_param_expr:None,
            });
        }
        None
    }).collect()
}

fn strip_comments(source:&str)->String{
    let mut out=String::with_capacity(source.len());
    let mut chars=source.chars().peekable();
    let mut in_block=false;
    let mut in_line=false;
    while let Some(ch)=chars.next(){
        if in_line{
            if ch=='\n'{in_line=false;out.push(ch);}
            continue;
        }
        if in_block{
            if ch=='*' && chars.peek()==Some(&'/'){chars.next();in_block=false;}
            continue;
        }
        if ch=='/' && chars.peek()==Some(&'/'){chars.next();in_line=true;continue;}
        if ch=='/' && chars.peek()==Some(&'*'){chars.next();in_block=true;continue;}
        out.push(ch);
    }
    out
}

fn macro_args<'a>(line:&'a str,name:&str)->Option<&'a str>{
    let rest=line.strip_prefix(name)?.trim_start().strip_prefix('(')?;
    let mut depth=1i32;
    for (i,ch) in rest.char_indices(){
        match ch{
            '('=>depth+=1,
            ')'=>{
                depth-=1;
                if depth==0{return Some(&rest[..i]);}
            }
            _=>{}
        }
    }
    None
}

fn split_args(args:&str)->Vec<&str>{
    let mut out=Vec::new();
    let mut start=0usize;
    let mut depth=0i32;
    for (i,ch) in args.char_indices(){
        match ch{
            '('=>depth+=1,
            ')'=>depth-=1,
            ',' if depth==0=>{
                out.push(args[start..i].trim());
                start=i+1;
            }
            _=>{}
        }
    }
    out.push(args[start..].trim());
    out
}

fn parse_i16(text:&str)->Option<i16>{
    let t=text.trim();
    if let Some(rest)=t.strip_prefix("-0x"){
        i32::from_str_radix(rest,16).ok()
            .and_then(|v|i16::try_from(-v).ok())
    }else if let Some(rest)=t.strip_prefix("0x"){
        i32::from_str_radix(rest,16).ok()
            .and_then(|v|i16::try_from(v).ok())
    }else{
        t.parse::<i16>().ok()
    }
}

#[cfg(test)]
mod tests{
    use super::*;

    #[test]
    fn parses_object_with_acts(){
        let src=r#"OBJECT_WITH_ACTS(MODEL_KING_BOBOMB, 1636, 4242, -5567, 0, -147, 0, BPARAM1(STAR_INDEX_ACT_1), bhvKingBobomb, ACT_1),"#;
        let parsed=parse_level_script_objects(src);
        assert_eq!(parsed.len(),1);
        assert_eq!(parsed[0].model,"MODEL_KING_BOBOMB");
        assert_eq!(parsed[0].position,[1636,4242,-5567]);
        assert_eq!(parsed[0].behavior,"bhvKingBobomb");
    }

    #[test]
    fn parses_macro_object(){
        let src="MACRO_OBJECT(macro_goomba, 0, -2713, 152, 5778),";
        let parsed=parse_macro_objects(src);
        assert_eq!(parsed.len(),1);
        assert_eq!(parsed[0].preset,"macro_goomba");
    }
}

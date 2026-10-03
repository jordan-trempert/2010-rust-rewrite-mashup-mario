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
    pub act_mask: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroObjectSpawn {
    pub preset: String,
    pub yaw_deg: i16,
    pub position: [i16;3],
    pub behavior_param_expr: Option<String>,
    pub resolved: Option<MacroPresetDefinition>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroPresetDefinition {
    pub behavior: String,
    pub model: String,
    pub default_behavior_param_expr: String,
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
    let presets_path=root.join("include").join("macro_presets.inc.c");
    let presets_source=fs::read_to_string(&presets_path)
        .map_err(|e|CollisionParseError::new(format!("{}: {e}",presets_path.display())))?;
    let preset_defs=parse_macro_preset_definitions(&presets_source);

    let mut macro_objects=parse_macro_objects(&macro_source);
    for object in &mut macro_objects {
        object.resolved=preset_defs.get(&object.preset).cloned();
    }

    Ok(ParsedObjectSpawns{
        level_objects:parse_level_script_objects(&script),
        macro_objects,
    })
}

pub fn parse_act_mask(expr:&str)->u8{
    let expr=expr.trim();
    if expr=="ALL_ACTS"{return 0x3F;}
    let mut mask=0u8;
    for part in expr.split('|').map(str::trim){
        mask|=match part{
            "ACT_1"=>1<<0,
            "ACT_2"=>1<<1,
            "ACT_3"=>1<<2,
            "ACT_4"=>1<<3,
            "ACT_5"=>1<<4,
            "ACT_6"=>1<<5,
            _=>0,
        };
    }
    mask
}

pub fn parse_macro_preset_definitions(
    source:&str,
)->std::collections::HashMap<String,MacroPresetDefinition>{
    let mut out=std::collections::HashMap::new();
    for raw in source.lines(){
        let line=raw.trim();
        let Some(comment_start)=line.find("/*") else {continue;};
        let Some(comment_end)=line[comment_start+2..].find("*/") else {continue;};
        let comment_end=comment_start+2+comment_end;
        let preset=line[comment_start+2..comment_end].trim();
        if !preset.starts_with("macro_"){continue;}

        let Some(open)=line[comment_end+2..].find('{') else {continue;};
        let rest=&line[comment_end+2+open+1..];
        let Some(close)=rest.find('}') else {continue;};
        let parts=split_args(&rest[..close]);
        if parts.len()<3{continue;}

        out.insert(preset.to_owned(),MacroPresetDefinition{
            behavior:parts[0].trim().to_owned(),
            model:parts[1].trim().to_owned(),
            default_behavior_param_expr:parts[2].trim().to_owned(),
        });
    }
    out
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
            act_mask:if with_acts { parse_act_mask(parts[9]) } else { 0x3F },
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
                resolved:None,
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
                resolved:None,
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
        assert_eq!(parsed[0].act_mask,1);
    }

    #[test]
    fn parses_act_masks(){
        assert_eq!(parse_act_mask("ACT_1 | ACT_3 | ACT_6"),0b100101);
        assert_eq!(parse_act_mask("ALL_ACTS"),0x3F);
    }

    #[test]
    fn parses_macro_object(){
        let src="MACRO_OBJECT(macro_goomba, 0, -2713, 152, 5778),";
        let parsed=parse_macro_objects(src);
        assert_eq!(parsed.len(),1);
        assert_eq!(parsed[0].preset,"macro_goomba");
        assert!(parsed[0].resolved.is_none());
    }

    #[test]
    fn resolves_macro_preset_table_row(){
        let src="/* macro_goomba */ { bhvGoomba, MODEL_GOOMBA, GOOMBA_SIZE_REGULAR },";
        let defs=parse_macro_preset_definitions(src);
        let def=defs.get("macro_goomba").unwrap();
        assert_eq!(def.behavior,"bhvGoomba");
        assert_eq!(def.model,"MODEL_GOOMBA");
    }
}

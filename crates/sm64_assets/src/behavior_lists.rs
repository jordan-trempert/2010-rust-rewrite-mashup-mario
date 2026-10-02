use std::{collections::HashMap, fs, path::Path};

use crate::CollisionParseError;

pub fn load_behavior_object_lists(
    decomp_root: impl AsRef<Path>,
)->Result<HashMap<String,String>,CollisionParseError>{
    let path=decomp_root.as_ref().join("data").join("behavior_data.c");
    let source=fs::read_to_string(&path)
        .map_err(|e|CollisionParseError::new(format!("{}: {e}",path.display())))?;
    Ok(parse_behavior_object_lists(&source))
}

pub fn parse_behavior_object_lists(source:&str)->HashMap<String,String>{
    let mut out=HashMap::new();
    let mut current:Option<String>=None;

    for raw in source.lines(){
        let line=raw.trim();

        if let Some(rest)=line.strip_prefix("const BehaviorScript "){
            if let Some(bracket)=rest.find('['){
                let name=rest[..bracket].trim();
                if !name.is_empty(){
                    current=Some(name.to_owned());
                }
            }
            continue;
        }

        let Some(name)=current.as_ref() else {continue;};
        if let Some(args)=macro_args(line,"BEGIN"){
            out.insert(name.clone(),args.trim().to_owned());
            current=None;
        }else if line.starts_with("};"){
            current=None;
        }
    }

    out
}

fn macro_args<'a>(line:&'a str,name:&str)->Option<&'a str>{
    let rest=line.strip_prefix(name)?.trim_start().strip_prefix('(')?;
    let end=rest.find(')')?;
    Some(&rest[..end])
}

#[cfg(test)]
mod tests{
    use super::*;

    #[test]
    fn reads_object_list_from_behavior(){
        let src=r#"
const BehaviorScript bhvGoomba[] = {
    BEGIN(OBJ_LIST_PUSHABLE),
    OR_INT(oFlags, OBJ_FLAG_UPDATE_GFX_POS_AND_ANGLE),
};
"#;
        let parsed=parse_behavior_object_lists(src);
        assert_eq!(parsed.get("bhvGoomba").map(String::as_str),Some("OBJ_LIST_PUSHABLE"));
    }
}

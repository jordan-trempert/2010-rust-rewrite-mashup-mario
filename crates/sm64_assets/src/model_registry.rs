use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use crate::{CollisionParseError, ParsedRenderGeometry, load_model_display_lists};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelSource {
    Geo { geo_symbol:String },
    DisplayList { display_list:String, layer:String },
}

pub fn load_model_registry(
    decomp_root:impl AsRef<Path>,
    level:&str,
)->Result<HashMap<String,ModelSource>,CollisionParseError>{
    let root=decomp_root.as_ref();
    let mut registry=HashMap::new();

    for path in [
        root.join("levels").join("scripts.c"),
        root.join("levels").join(level).join("script.c"),
    ] {
        if !path.exists(){continue;}
        let source=fs::read_to_string(&path)
            .map_err(|e|CollisionParseError::new(format!("{}: {e}",path.display())))?;
        parse_model_registry_source(&source,&mut registry);
    }
    Ok(registry)
}

pub fn resolve_geo_model_geometry(
    decomp_root:impl AsRef<Path>,
    geo_symbol:&str,
)->Result<(ParsedRenderGeometry,String),CollisionParseError>{
    let root=decomp_root.as_ref();
    let actors=root.join("actors");
    let mut actor_dirs=Vec::new();
    collect_dirs_with_geo(&actors,&mut actor_dirs)
        .map_err(|e|CollisionParseError::new(format!("{}: {e}",actors.display())))?;

    for dir in actor_dirs {
        let geo_path=dir.join("geo.inc.c");
        let Ok(source)=fs::read_to_string(&geo_path) else {continue;};
        let Some(body)=extract_named_array_body(&source,"GeoLayout",geo_symbol) else {continue;};
        let roots=parse_geo_roots(&body);
        if roots.is_empty(){
            return Err(CollisionParseError::new(format!(
                "{}: GeoLayout {geo_symbol} has no display lists",geo_path.display()
            )));
        }
        let model_path=dir.join("model.inc.c");
        let relative=model_path.strip_prefix(root)
            .map_err(|_|CollisionParseError::new(format!("{} is outside decomp root",model_path.display())))?
            .to_string_lossy()
            .replace('\\',"/");
        let root_pairs=roots.iter()
            .map(|(layer,name)|(layer.as_str(),name.as_str()))
            .collect::<Vec<_>>();
        let geometry=load_model_display_lists(root,&relative,&root_pairs)?;
        let actor_name=dir.file_name()
            .and_then(|v|v.to_str())
            .unwrap_or("unknown")
            .to_owned();
        return Ok((geometry,actor_name));
    }

    Err(CollisionParseError::new(format!(
        "could not find actor GeoLayout {geo_symbol}"
    )))
}

fn parse_model_registry_source(source:&str,out:&mut HashMap<String,ModelSource>){
    let cleaned=strip_comments(source);
    for raw in cleaned.lines(){
        let line=raw.trim();
        if let Some(args)=macro_args(line,"LOAD_MODEL_FROM_GEO"){
            let parts=split_args(args);
            if parts.len()>=2{
                out.insert(parts[0].to_owned(),ModelSource::Geo{
                    geo_symbol:parts[1].to_owned(),
                });
            }
        }else if let Some(args)=macro_args(line,"LOAD_MODEL_FROM_DL"){
            let parts=split_args(args);
            if parts.len()>=3{
                out.insert(parts[0].to_owned(),ModelSource::DisplayList{
                    display_list:parts[1].to_owned(),
                    layer:parts[2].to_owned(),
                });
            }
        }
    }
}

fn parse_geo_roots(body:&str)->Vec<(String,String)>{
    body.lines().filter_map(|raw|{
        let line=raw.trim();
        let args=macro_args(line,"GEO_DISPLAY_LIST")?;
        let parts=split_args(args);
        (parts.len()>=2).then(||(parts[0].to_owned(),parts[1].to_owned()))
    }).collect()
}

fn collect_dirs_with_geo(dir:&Path,out:&mut Vec<PathBuf>)->std::io::Result<()>{
    for entry in fs::read_dir(dir)?{
        let entry=entry?;
        let path=entry.path();
        if path.is_dir(){
            if path.join("geo.inc.c").exists(){
                out.push(path.clone());
            }
            collect_dirs_with_geo(&path,out)?;
        }
    }
    Ok(())
}

fn extract_named_array_body(source:&str,ty:&str,name:&str)->Option<String>{
    let needles=[
        format!("const {ty} {name}[]"),
        format!("static const {ty} {name}[]"),
    ];
    let start=needles.iter().find_map(|needle|source.find(needle))?;
    let eq=source[start..].find('=')?+start;
    let open=source[eq..].find('{')?+eq;
    let mut depth=0i32;
    for (relative,ch) in source[open..].char_indices(){
        match ch{
            '{'=>depth+=1,
            '}'=>{
                depth-=1;
                if depth==0{
                    let end=open+relative;
                    return Some(source[open+1..end].to_owned());
                }
            }
            _=>{}
        }
    }
    None
}

fn strip_comments(source:&str)->String{
    let mut out=String::with_capacity(source.len());
    let mut chars=source.chars().peekable();
    let mut block=false;
    let mut line=false;
    while let Some(ch)=chars.next(){
        if line{
            if ch=='\n'{line=false;out.push(ch);}
            continue;
        }
        if block{
            if ch=='*'&&chars.peek()==Some(&'/'){chars.next();block=false;}
            continue;
        }
        if ch=='/'&&chars.peek()==Some(&'/'){chars.next();line=true;continue;}
        if ch=='/'&&chars.peek()==Some(&'*'){chars.next();block=true;continue;}
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

fn split_args(args:&str)->Vec<String>{
    let mut out=Vec::new();
    let mut start=0usize;
    let mut depth=0i32;
    for (i,ch) in args.char_indices(){
        match ch{
            '('=>depth+=1,
            ')'=>depth-=1,
            ',' if depth==0=>{
                out.push(args[start..i].trim().to_owned());
                start=i+1;
            }
            _=>{}
        }
    }
    out.push(args[start..].trim().to_owned());
    out
}

#[cfg(test)]
mod tests{
    use super::*;

    #[test]
    fn parses_model_registry(){
        let mut out=HashMap::new();
        parse_model_registry_source(
            "LOAD_MODEL_FROM_GEO(MODEL_GOOMBA, goomba_geo),\nLOAD_MODEL_FROM_DL(MODEL_COIN, coin_dl, LAYER_ALPHA),",
            &mut out,
        );
        assert_eq!(
            out.get("MODEL_GOOMBA"),
            Some(&ModelSource::Geo{geo_symbol:"goomba_geo".into()})
        );
    }
}

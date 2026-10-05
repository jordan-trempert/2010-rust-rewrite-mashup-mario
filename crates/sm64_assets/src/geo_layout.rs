use std::{collections::HashMap, fs, path::{Path,PathBuf}};

use crate::{CollisionParseError, ParsedRenderGeometry, load_model_display_lists};

#[derive(Clone, Debug, PartialEq)]
pub struct GeoRenderPart {
    pub layer: String,
    pub display_list: String,
    pub translation: [f32;3],
    pub rotation_deg: [f32;3],
    pub scale: f32,
    pub billboard: bool,
}

#[derive(Clone, Debug)]
pub struct ResolvedGeoPart {
    pub spec: GeoRenderPart,
    pub geometry: ParsedRenderGeometry,
}

#[derive(Clone, Debug)]
pub struct ResolvedGeoModel {
    pub actor_name: String,
    pub parts: Vec<ResolvedGeoPart>,
}

#[derive(Clone, Debug, Default)]
struct RawNode {
    parent: Option<usize>,
    children: Vec<usize>,
    translation: [f32;3],
    rotation_deg: [f32;3],
    scale: f32,
    billboard: bool,
    switch: bool,
    display: Option<(String,String)>,
    branch: Option<String>,
}

#[derive(Clone, Copy, Debug)]
struct Accum {
    translation: [f32;3],
    rotation_deg: [f32;3],
    scale: f32,
    billboard: bool,
}

impl Default for Accum {
    fn default()->Self{
        Self{
            translation:[0.0;3],
            rotation_deg:[0.0;3],
            scale:1.0,
            billboard:false,
        }
    }
}

pub fn parse_geo_render_parts(source:&str,geo_symbol:&str)->Result<Vec<GeoRenderPart>,CollisionParseError>{
    parse_geo_render_parts_with_switch(source,geo_symbol,0)
}

pub fn parse_geo_render_parts_with_switch(
    source:&str,
    geo_symbol:&str,
    switch_case:i32,
)->Result<Vec<GeoRenderPart>,CollisionParseError>{
    let layouts=extract_geo_layout_bodies(source);
    let mut out=Vec::new();
    let mut visiting=Vec::new();
    flatten_layout(
        &layouts,
        geo_symbol,
        Accum::default(),
        switch_case,
        &mut visiting,
        &mut out,
    )?;
    Ok(out)
}

pub fn resolve_geo_model_parts(
    decomp_root:impl AsRef<Path>,
    geo_symbol:&str,
)->Result<ResolvedGeoModel,CollisionParseError>{
    resolve_geo_model_parts_with_switch(decomp_root,geo_symbol,0)
}

pub fn resolve_geo_model_parts_with_switch(
    decomp_root:impl AsRef<Path>,
    geo_symbol:&str,
    switch_case:i32,
)->Result<ResolvedGeoModel,CollisionParseError>{
    let root=decomp_root.as_ref();
    let actors=root.join("actors");
    let mut actor_dirs=Vec::new();
    collect_dirs_with_geo(&actors,&mut actor_dirs)
        .map_err(|e|CollisionParseError::new(format!("{}: {e}",actors.display())))?;

    for dir in actor_dirs {
        let geo_path=dir.join("geo.inc.c");
        let Ok(source)=fs::read_to_string(&geo_path) else {continue;};
        if !source.contains(&format!("{geo_symbol}[]")){continue;}

        let specs=parse_geo_render_parts_with_switch(&source,geo_symbol,switch_case)?;
        let model_path=dir.join("model.inc.c");
        let relative=model_path.strip_prefix(root)
            .map_err(|_|CollisionParseError::new(format!("{} is outside decomp root",model_path.display())))?
            .to_string_lossy()
            .replace('\\',"/");
        let mut parts=Vec::new();

        for spec in specs {
            let root_pair=[(spec.layer.as_str(),spec.display_list.as_str())];
            match load_model_display_lists(root,&relative,&root_pair) {
                Ok(geometry) if geometry.triangle_count>0=>{
                    parts.push(ResolvedGeoPart{spec,geometry});
                }
                Ok(_)=>{}
                Err(error)=>{
                    // Some display lists referenced by actor geos live in sibling
                    // include files. Keep resolving other parts rather than making
                    // the entire actor disappear.
                    let _=error;
                }
            }
        }

        let actor_name=dir.file_name()
            .and_then(|v|v.to_str())
            .unwrap_or("unknown")
            .to_owned();
        return Ok(ResolvedGeoModel{actor_name,parts});
    }

    Err(CollisionParseError::new(format!("could not find actor GeoLayout {geo_symbol}")))
}

pub fn resolve_geo_model_parts_for_level(
    decomp_root:impl AsRef<Path>,
    level:&str,
    geo_symbol:&str,
)->Result<ResolvedGeoModel,CollisionParseError>{
    resolve_geo_model_parts_for_level_state(decomp_root,level,geo_symbol,0)
}

pub fn resolve_geo_model_parts_for_level_state(
    decomp_root:impl AsRef<Path>,
    level:&str,
    geo_symbol:&str,
    switch_case:i32,
)->Result<ResolvedGeoModel,CollisionParseError>{
    let root=decomp_root.as_ref();

    // Shared actor geometry first.
    if let Ok(model)=resolve_geo_model_parts_with_switch(root,geo_symbol,switch_case) {
        return Ok(model);
    }

    // Course-specific objects (for example BOB's chain-chomp gate and
    // seesaw platform) live under levels/<level>/* rather than actors/*.
    let level_root=root.join("levels").join(level);
    let mut dirs=Vec::new();
    collect_dirs_with_geo(&level_root,&mut dirs)
        .map_err(|e|CollisionParseError::new(format!("{}: {e}",level_root.display())))?;

    for dir in dirs {
        let geo_path=dir.join("geo.inc.c");
        let Ok(source)=fs::read_to_string(&geo_path) else {continue;};
        if !source.contains(geo_symbol) {continue;}

        let specs=parse_geo_render_parts_with_switch(&source,geo_symbol,switch_case)?;
        let mut parts=Vec::new();

        // Most level-local object geos have a sibling model.inc.c. Area
        // geometry may fan out into numbered subdirectories, so try those too.
        let mut model_paths=Vec::new();
        let sibling=dir.join("model.inc.c");
        if sibling.exists() {
            model_paths.push(sibling);
        }
        if let Ok(entries)=fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path=entry.path().join("model.inc.c");
                if path.exists() {
                    model_paths.push(path);
                }
            }
        }

        for spec in specs {
            let root_pair=[(spec.layer.as_str(),spec.display_list.as_str())];
            for model_path in &model_paths {
                let relative=model_path.strip_prefix(root)
                    .map_err(|_|CollisionParseError::new(format!(
                        "{} is outside decomp root",model_path.display()
                    )))?
                    .to_string_lossy()
                    .replace('\\',"/");
                match load_model_display_lists(root,&relative,&root_pair) {
                    Ok(geometry) if geometry.triangle_count>0=>{
                        parts.push(ResolvedGeoPart{spec:spec.clone(),geometry});
                        break;
                    }
                    _=>{}
                }
            }
        }

        let actor_name=dir.file_name()
            .and_then(|v|v.to_str())
            .unwrap_or(level)
            .to_owned();
        if !parts.is_empty() {
            return Ok(ResolvedGeoModel{actor_name,parts});
        }
    }

    Err(CollisionParseError::new(format!(
        "could not find GeoLayout {geo_symbol} in actors or level {level}"
    )))
}

fn flatten_layout(
    layouts:&HashMap<String,String>,
    symbol:&str,
    inherited:Accum,
    switch_case:i32,
    visiting:&mut Vec<String>,
    out:&mut Vec<GeoRenderPart>,
)->Result<(),CollisionParseError>{
    if visiting.iter().any(|entry|entry==symbol){
        return Err(CollisionParseError::new(format!("recursive GeoLayout branch {symbol}")));
    }
    let body=layouts.get(symbol)
        .ok_or_else(||CollisionParseError::new(format!("missing GeoLayout {symbol}")))?;
    visiting.push(symbol.to_owned());

    let (nodes,roots)=parse_raw_tree(body);
    for root in roots {
        flatten_node(&nodes,root,inherited,layouts,switch_case,visiting,out)?;
    }

    visiting.pop();
    Ok(())
}

fn flatten_node(
    nodes:&[RawNode],
    index:usize,
    inherited:Accum,
    layouts:&HashMap<String,String>,
    switch_case:i32,
    visiting:&mut Vec<String>,
    out:&mut Vec<GeoRenderPart>,
)->Result<(),CollisionParseError>{
    let node=&nodes[index];
    let mut accum=inherited;

    let local_translation=rotate_euler_xyz(
        [
            node.translation[0]*inherited.scale,
            node.translation[1]*inherited.scale,
            node.translation[2]*inherited.scale,
        ],
        inherited.rotation_deg,
    );
    accum.translation=[
        inherited.translation[0]+local_translation[0],
        inherited.translation[1]+local_translation[1],
        inherited.translation[2]+local_translation[2],
    ];
    accum.rotation_deg=[
        inherited.rotation_deg[0]+node.rotation_deg[0],
        inherited.rotation_deg[1]+node.rotation_deg[1],
        inherited.rotation_deg[2]+node.rotation_deg[2],
    ];
    accum.scale=inherited.scale*node.scale;
    accum.billboard=inherited.billboard||node.billboard;

    if let Some((layer,display_list))=&node.display {
        if display_list!="NULL" {
            out.push(GeoRenderPart{
                layer:layer.clone(),
                display_list:display_list.clone(),
                translation:accum.translation,
                rotation_deg:accum.rotation_deg,
                scale:accum.scale,
                billboard:accum.billboard,
            });
        }
    }

    if let Some(branch)=&node.branch {
        if layouts.contains_key(branch) {
            flatten_layout(layouts,branch,accum,switch_case,visiting,out)?;
        }
    }

    if node.switch {
        if !node.children.is_empty() {
            let selected=switch_case.rem_euclid(node.children.len() as i32) as usize;
            flatten_node(
                nodes,
                node.children[selected],
                accum,
                layouts,
                switch_case,
                visiting,
                out,
            )?;
        }
    }else{
        for child in &node.children {
            flatten_node(nodes,*child,accum,layouts,switch_case,visiting,out)?;
        }
    }

    Ok(())
}

fn parse_raw_tree(body:&str)->(Vec<RawNode>,Vec<usize>){
    let mut nodes=Vec::<RawNode>::new();
    let mut roots=Vec::new();
    let mut current_parent:Option<usize>=None;
    let mut last_node:Option<usize>=None;

    for raw in body.lines(){
        let line=strip_comments(raw).trim().to_owned();
        if line.is_empty(){continue;}

        if line.starts_with("GEO_OPEN_NODE"){
            if let Some(last)=last_node{
                current_parent=Some(last);
            }
            continue;
        }
        if line.starts_with("GEO_CLOSE_NODE"){
            current_parent=current_parent.and_then(|parent|nodes[parent].parent);
            last_node=current_parent;
            continue;
        }
        if line.starts_with("GEO_END")||line.starts_with("GEO_RETURN"){
            continue;
        }

        let mut node=RawNode{
            parent:current_parent,
            scale:1.0,
            ..RawNode::default()
        };

        if let Some(args)=macro_args(&line,"GEO_SCALE"){
            let parts=split_args(args);
            if parts.len()>=2{
                node.scale=parse_i32(&parts[1]).map_or(1.0,|v|v as f32/65536.0);
            }
        }else if let Some(args)=macro_args(&line,"GEO_ANIMATED_PART"){
            let parts=split_args(args);
            if parts.len()>=5{
                node.translation=[
                    parse_i32(&parts[1]).unwrap_or(0) as f32,
                    parse_i32(&parts[2]).unwrap_or(0) as f32,
                    parse_i32(&parts[3]).unwrap_or(0) as f32,
                ];
                node.display=Some((parts[0].clone(),parts[4].clone()));
            }
        }else if let Some(args)=macro_args(&line,"GEO_DISPLAY_LIST"){
            let parts=split_args(args);
            if parts.len()>=2{
                node.display=Some((parts[0].clone(),parts[1].clone()));
            }
        }else if let Some(args)=macro_args(&line,"GEO_TRANSLATE_NODE"){
            let parts=split_args(args);
            if parts.len()>=4{
                node.translation=[
                    parse_i32(&parts[1]).unwrap_or(0) as f32,
                    parse_i32(&parts[2]).unwrap_or(0) as f32,
                    parse_i32(&parts[3]).unwrap_or(0) as f32,
                ];
            }
        }else if let Some(args)=macro_args(&line,"GEO_TRANSLATE_ROTATE"){
            let parts=split_args(args);
            if parts.len()>=7{
                node.translation=[
                    parse_i32(&parts[1]).unwrap_or(0) as f32,
                    parse_i32(&parts[2]).unwrap_or(0) as f32,
                    parse_i32(&parts[3]).unwrap_or(0) as f32,
                ];
                node.rotation_deg=[
                    parse_i32(&parts[4]).unwrap_or(0) as f32,
                    parse_i32(&parts[5]).unwrap_or(0) as f32,
                    parse_i32(&parts[6]).unwrap_or(0) as f32,
                ];
            }
        }else if let Some(args)=macro_args(&line,"GEO_ROTATION_NODE"){
            let parts=split_args(args);
            if parts.len()>=4{
                node.rotation_deg=[
                    parse_i32(&parts[1]).unwrap_or(0) as f32,
                    parse_i32(&parts[2]).unwrap_or(0) as f32,
                    parse_i32(&parts[3]).unwrap_or(0) as f32,
                ];
            }
        }else if line.starts_with("GEO_BILLBOARD"){
            node.billboard=true;
        }else if line.starts_with("GEO_SWITCH_CASE"){
            node.switch=true;
        }else if let Some(args)=macro_args(&line,"GEO_BRANCH"){
            let parts=split_args(args);
            if parts.len()>=2{
                node.branch=Some(parts[1].clone());
            }
        }

        let index=nodes.len();
        nodes.push(node);
        if let Some(parent)=current_parent{
            nodes[parent].children.push(index);
        }else{
            roots.push(index);
        }
        last_node=Some(index);
    }

    (nodes,roots)
}

fn extract_geo_layout_bodies(source:&str)->HashMap<String,String>{
    let mut out=HashMap::new();
    let mut cursor=0usize;
    let needle="GeoLayout ";

    while let Some(relative)=source[cursor..].find(needle){
        let start=cursor+relative+needle.len();
        let Some(bracket_rel)=source[start..].find('[') else{break;};
        let bracket=start+bracket_rel;
        let name=source[start..bracket].trim().trim_start_matches("const ").trim();
        if name.is_empty(){cursor=bracket+1;continue;}

        let Some(eq_rel)=source[bracket..].find('=') else{cursor=bracket+1;continue;};
        let eq=bracket+eq_rel;
        let Some(open_rel)=source[eq..].find('{') else{cursor=eq+1;continue;};
        let open=eq+open_rel;
        let mut depth=0i32;
        let mut end=None;
        for (relative,ch) in source[open..].char_indices(){
            match ch{
                '{'=>depth+=1,
                '}'=>{
                    depth-=1;
                    if depth==0{end=Some(open+relative);break;}
                }
                _=>{}
            }
        }
        let Some(end)=end else{break;};
        out.insert(name.to_owned(),source[open+1..end].to_owned());
        cursor=end+1;
    }
    out
}

fn rotate_euler_xyz(v:[f32;3],degrees:[f32;3])->[f32;3]{
    let [rx,ry,rz]=degrees.map(f32::to_radians);
    let (sx,cx)=rx.sin_cos();
    let (sy,cy)=ry.sin_cos();
    let (sz,cz)=rz.sin_cos();

    let x1=v[0];
    let y1=v[1]*cx-v[2]*sx;
    let z1=v[1]*sx+v[2]*cx;
    let x2=x1*cy+z1*sy;
    let y2=y1;
    let z2=-x1*sy+z1*cy;
    [
        x2*cz-y2*sz,
        x2*sz+y2*cz,
        z2,
    ]
}

fn collect_dirs_with_geo(dir:&Path,out:&mut Vec<PathBuf>)->std::io::Result<()>{
    for entry in fs::read_dir(dir)?{
        let entry=entry?;
        let path=entry.path();
        if path.is_dir(){
            if path.join("geo.inc.c").exists(){out.push(path.clone());}
            collect_dirs_with_geo(&path,out)?;
        }
    }
    Ok(())
}

fn strip_comments(line:&str)->String{
    let mut out=String::new();
    let mut rest=line;
    loop{
        let no_line=rest.split("//").next().unwrap_or(rest);
        let Some(start)=no_line.find("/*") else{out.push_str(no_line);break;};
        out.push_str(&no_line[..start]);
        let after=&no_line[start+2..];
        let Some(end)=after.find("*/") else{break;};
        rest=&after[end+2..];
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

fn parse_i32(text:&str)->Option<i32>{
    let text=text.trim();
    if let Some(hex)=text.strip_prefix("-0x"){
        i32::from_str_radix(hex,16).ok().map(|v|-v)
    }else if let Some(hex)=text.strip_prefix("0x"){
        i32::from_str_radix(hex,16).ok()
    }else{
        text.parse().ok()
    }
}

#[cfg(test)]
mod tests{
    use super::*;

    #[test]
    fn parses_goomba_style_hierarchy(){
        let source=r#"
const GeoLayout test_geo[] = {
    GEO_SCALE(0x00, 16384),
    GEO_OPEN_NODE(),
        GEO_ANIMATED_PART(LAYER_OPAQUE, 10, 0, 0, body_dl),
        GEO_OPEN_NODE(),
            GEO_ANIMATED_PART(LAYER_OPAQUE, 20, 0, 0, foot_dl),
        GEO_CLOSE_NODE(),
    GEO_CLOSE_NODE(),
    GEO_END(),
};
"#;
        let parts=parse_geo_render_parts(source,"test_geo").unwrap();
        assert_eq!(parts.len(),2);
        assert!((parts[0].scale-0.25).abs()<0.001);
        assert_eq!(parts[0].translation,[2.5,0.0,0.0]);
        assert_eq!(parts[1].translation,[7.5,0.0,0.0]);
    }

    #[test]
    fn chooses_first_switch_state(){
        let source=r#"
const GeoLayout test_geo[] = {
    GEO_SWITCH_CASE(2, geo_switch_anim_state),
    GEO_OPEN_NODE(),
        GEO_DISPLAY_LIST(LAYER_ALPHA, first_dl),
        GEO_DISPLAY_LIST(LAYER_ALPHA, second_dl),
    GEO_CLOSE_NODE(),
    GEO_END(),
};
"#;
        let parts=parse_geo_render_parts(source,"test_geo").unwrap();
        assert_eq!(parts.len(),1);
        assert_eq!(parts[0].display_list,"first_dl");
    }
}

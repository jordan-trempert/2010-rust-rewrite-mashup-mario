use sm64_core::{CollisionWorld, EnvironmentRegion, Surface};

use crate::surface_type_by_name;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpecialObject {
    pub preset: String,
    pub values: Vec<i16>,
}

#[derive(Clone, Debug)]
pub struct ParsedCollision {
    pub world: CollisionWorld,
    pub specials: Vec<SpecialObject>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollisionParseError(String);

impl CollisionParseError {
    pub(crate) fn new(message:String)->Self {Self(message)}
}
impl core::fmt::Display for CollisionParseError {
    fn fmt(&self,f:&mut core::fmt::Formatter<'_>)->core::fmt::Result {f.write_str(&self.0)}
}
impl std::error::Error for CollisionParseError {}

pub fn parse_collision_source(source:&str)->Result<ParsedCollision,CollisionParseError> {
    let mut vertices:Vec<[i16;3]>=Vec::new();
    let mut world=CollisionWorld::default();
    let mut specials=Vec::new();
    let mut surface_type:Option<i16>=None;

    for (line_no,raw) in source.lines().enumerate() {
        let cleaned=strip_comments(raw);
        let line=cleaned.trim();
        if line.is_empty() {continue;}
        if let Some(args)=macro_args(line,"COL_VERTEX") {
            let n=parse_numbers(args,line_no)?;
            if n.len()!=3 {return Err(err(line_no,"COL_VERTEX expects 3 numbers"));}
            vertices.push([n[0],n[1],n[2]]);
        } else if let Some(args)=macro_args(line,"COL_TRI_INIT") {
            let parts=split_args(args);
            let Some(name)=parts.first() else {return Err(err(line_no,"COL_TRI_INIT missing surface type"));};
            surface_type=Some(
                surface_type_by_name(name)
                    .or_else(||parse_i16(name).ok())
                    .ok_or_else(||err(line_no,&format!("unknown surface type {name}")))?
            );
        } else if let Some(args)=macro_args(line,"COL_TRI_SPECIAL") {
            let n=parse_numbers(args,line_no)?;
            if n.len()!=4 {return Err(err(line_no,"COL_TRI_SPECIAL expects v1,v2,v3,force"));}
            push_triangle(&mut world,&vertices,surface_type,n[0],n[1],n[2],n[3],line_no)?;
        } else if let Some(args)=macro_args(line,"COL_TRI") {
            let n=parse_numbers(args,line_no)?;
            if n.len()!=3 {return Err(err(line_no,"COL_TRI expects v1,v2,v3"));}
            push_triangle(&mut world,&vertices,surface_type,n[0],n[1],n[2],0,line_no)?;
        } else if let Some(args)=macro_args(line,"COL_WATER_BOX") {
            let parts=split_args(args);
            if parts.len()!=6 {return Err(err(line_no,"COL_WATER_BOX expects kind,x1,z1,x2,z2,y"));}
            let kind=water_kind(parts[0]).or_else(||parse_i16(parts[0]).ok())
                .ok_or_else(||err(line_no,&format!("unknown environment kind {}",parts[0])))?;
            let vals=parts[1..].iter().map(|v|parse_i16(v)).collect::<Result<Vec<_>,_>>()
                .map_err(|e|err(line_no,&e))?;
            world.push_environment_region(EnvironmentRegion{
                kind,lo_x:vals[0],lo_z:vals[1],hi_x:vals[2],hi_z:vals[3],height:vals[4],
            });
        } else if let Some(args)=macro_args(line,"SPECIAL_OBJECT_WITH_YAW") {
            specials.push(parse_special(args,true,line_no)?);
        } else if let Some(args)=macro_args(line,"SPECIAL_OBJECT") {
            specials.push(parse_special(args,false,line_no)?);
        }
    }

    Ok(ParsedCollision{world,specials})
}

fn push_triangle(
    world:&mut CollisionWorld, vertices:&[[i16;3]], surface_type:Option<i16>,
    a:i16,b:i16,c:i16,force:i16,line_no:usize
)->Result<(),CollisionParseError>{
    let ty=surface_type.ok_or_else(||err(line_no,"COL_TRI before COL_TRI_INIT"))?;
    let get=|i:i16| vertices.get(i as usize).copied().ok_or_else(||err(line_no,&format!("vertex index {i} out of range")));
    let v1=get(a)?; let v2=get(b)?; let v3=get(c)?;
    if let Some(surface)=Surface::from_triangle(ty,force,0,0,v1,v2,v3) {
        world.push_surface(surface);
    }
    Ok(())
}

fn parse_special(args:&str,_with_yaw:bool,line_no:usize)->Result<SpecialObject,CollisionParseError>{
    let parts=split_args(args);
    let Some(preset)=parts.first() else {return Err(err(line_no,"SPECIAL_OBJECT missing preset"));};
    let mut values=Vec::new();
    for value in &parts[1..] {
        if let Ok(n)=parse_i16(value) {values.push(n);}
    }
    Ok(SpecialObject{preset:preset.trim().to_owned(),values})
}

fn water_kind(name:&str)->Option<i16>{
    match name.trim() {
        "WATER_BOX" => Some(0),
        "TOXIC_HAZE" => Some(50),
        _ => None,
    }
}

fn strip_comments(line:&str)->String {
    let no_line=line.split("//").next().unwrap_or(line);
    let mut out=String::with_capacity(no_line.len());
    let mut rest=no_line;
    loop {
        let Some(start)=rest.find("/*") else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..start]);
        let after=&rest[start+2..];
        let Some(end)=after.find("*/") else { break; };
        rest=&after[end+2..];
    }
    out
}

fn macro_args<'a>(line:&'a str,name:&str)->Option<&'a str>{
    let rest=line.strip_prefix(name)?.trim_start();
    let rest=rest.strip_prefix('(')?;
    let end=rest.rfind(')')?;
    Some(&rest[..end])
}

fn split_args(args:&str)->Vec<&str>{
    args.split(',').map(|v|v.trim()).filter(|v|!v.is_empty()).collect()
}

fn parse_numbers(args:&str,line_no:usize)->Result<Vec<i16>,CollisionParseError>{
    split_args(args).into_iter().map(|v|parse_i16(v).map_err(|e|err(line_no,&e))).collect()
}

fn parse_i16(text:&str)->Result<i16,String>{
    let t=text.trim();
    let value=if let Some(hex)=t.strip_prefix("0x") {
        i32::from_str_radix(hex,16).map_err(|e|e.to_string())?
    } else {
        t.parse::<i32>().map_err(|e|e.to_string())?
    };
    i16::try_from(value).map_err(|_|format!("{text} does not fit i16"))
}

fn err(line_no:usize,msg:&str)->CollisionParseError {
    CollisionParseError::new(format!("collision source line {}: {msg}",line_no+1))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_collision_macros() {
        let src=r#"
            COL_VERTEX_INIT(3),
            COL_VERTEX(-100, 0, -100),
            COL_VERTEX(100, 0, -100),
            COL_VERTEX(0, 0, 100),
            COL_TRI_INIT(SURFACE_DEFAULT, 1),
            COL_TRI(0, 1, 2),
            COL_TRI_STOP(),
        "#;
        let parsed=parse_collision_source(src).unwrap();
        assert_eq!(parsed.world.surfaces.len(),1);
        assert!(parsed.world.find_floor(0.0,50.0,0.0).is_some());
    }
}

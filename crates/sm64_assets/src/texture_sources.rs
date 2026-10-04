use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use crate::CollisionParseError;

pub fn load_texture_sources(
    decomp_root: impl AsRef<Path>,
    level: &str,
) -> Result<HashMap<String, PathBuf>, CollisionParseError> {
    let root=decomp_root.as_ref();
    let mut out=HashMap::new();

    // Segment 9 is a per-course texture bank. BOB loads "generic", WF loads
    // "grass", CCM loads "snow", JRB loads "water", etc. Using generic.c for
    // every level makes non-BOB display lists resolve to the wrong or missing
    // texture symbols.
    if let Some(bank)=level_texture_bank(root,level)? {
        let bank_path=root.join("bin").join(format!("{bank}.c"));
        if bank_path.exists() {
            let source=fs::read_to_string(&bank_path)
                .map_err(|e|CollisionParseError::new(format!("{}: {e}",bank_path.display())))?;
            parse_texture_source_file(root,&source,&mut out);
        }
    }

    let level_texture=root.join("levels").join(level).join("texture.inc.c");
    if level_texture.exists() {
        let source=fs::read_to_string(&level_texture)
            .map_err(|e|CollisionParseError::new(format!("{}: {e}",level_texture.display())))?;
        parse_texture_source_file(root,&source,&mut out);
    }

    Ok(out)
}

fn level_texture_bank(
    root:&Path,
    level:&str,
)->Result<Option<String>,CollisionParseError>{
    let script_path=root.join("levels").join(level).join("script.c");
    if !script_path.exists() {
        return Ok(None);
    }
    let source=fs::read_to_string(&script_path)
        .map_err(|e|CollisionParseError::new(format!("{}: {e}",script_path.display())))?;

    for raw in source.lines() {
        let line=raw.trim();
        if !line.contains("LOAD_MIO0_TEXTURE") || !line.contains("0x09") {
            continue;
        }
        let Some(start)=line.find('_') else {continue;};
        let tail=&line[start+1..];
        let Some(end)=tail.find("_mio0SegmentRomStart") else {continue;};
        let bank=&tail[..end];
        if !bank.is_empty() {
            return Ok(Some(bank.to_owned()));
        }
    }
    Ok(None)
}

fn parse_texture_source_file(
    root:&Path,
    source:&str,
    out:&mut HashMap<String,PathBuf>,
) {
    let mut pending_symbol:Option<String>=None;

    for raw in source.lines() {
        let line=raw.trim();

        if let Some(index)=line.find("const Texture ") {
            let rest=&line[index+"const Texture ".len()..];
            if let Some(bracket)=rest.find('[') {
                let symbol=rest[..bracket].trim();
                if !symbol.is_empty() {
                    pending_symbol=Some(symbol.to_owned());
                }
            }
            continue;
        }

        let Some(symbol)=pending_symbol.as_ref() else {continue;};
        let Some(include_start)=line.find("#include \"") else {continue;};
        let after=&line[include_start+"#include \"".len()..];
        let Some(end)=after.find('"') else {continue;};
        let include_path=&after[..end];

        let png_rel=include_path
            .strip_suffix(".inc.c")
            .map_or_else(||PathBuf::from(include_path),|base|PathBuf::from(format!("{base}.png")));
        out.insert(symbol.clone(),root.join(png_rel));
        pending_symbol=None;
    }
}

pub fn load_level_object_texture_sources(
    decomp_root: impl AsRef<Path>,
    level: &str,
    object_dir: &str,
) -> Result<HashMap<String, PathBuf>, CollisionParseError> {
    let root=decomp_root.as_ref();
    let dir=root.join("levels").join(level).join(object_dir);
    let mut out=HashMap::new();

    for path in [
        dir.join("model.inc.c"),
        dir.join("texture.inc.c"),
    ] {
        if !path.exists() {
            continue;
        }
        let source=fs::read_to_string(&path)
            .map_err(|e|CollisionParseError::new(format!("{}: {e}",path.display())))?;
        parse_texture_source_file(root,&source,&mut out);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_texture_symbol_to_png() {
        let mut out=HashMap::new();
        let source=r#"
ALIGNED8 const Texture generic_09005800[] = {
#include "textures/generic/bob_textures.05800.rgba16.inc.c"
};
"#;
        parse_texture_source_file(Path::new("/tmp/sm64"),source,&mut out);
        assert_eq!(
            out.get("generic_09005800").unwrap(),
            &PathBuf::from("/tmp/sm64/textures/generic/bob_textures.05800.rgba16.png")
        );
    }
}


pub fn load_actor_texture_sources(
    decomp_root: impl AsRef<Path>,
    actor: &str,
) -> Result<HashMap<String, PathBuf>, CollisionParseError> {
    let root=decomp_root.as_ref();
    let actor_dir=root.join("actors").join(actor);
    let mut out=HashMap::new();

    for path in [
        actor_dir.join("model.inc.c"),
        actor_dir.join("texture.inc.c"),
    ] {
        if !path.exists() {
            continue;
        }
        let source=fs::read_to_string(&path)
            .map_err(|e|CollisionParseError::new(format!("{}: {e}",path.display())))?;
        parse_texture_source_file(root,&source,&mut out);
    }

    Ok(out)
}

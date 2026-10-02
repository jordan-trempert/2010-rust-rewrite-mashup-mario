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

    for path in [
        root.join("bin").join("generic.c"),
        root.join("levels").join(level).join("texture.inc.c"),
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

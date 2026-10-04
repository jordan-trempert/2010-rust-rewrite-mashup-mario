use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use regex::Regex;

use crate::CollisionParseError;

#[derive(Clone, Debug)]
pub struct RenderVertex {
    pub position: [f32; 3],
    /// Raw N64 texture coordinates in S10.5 units.
    pub texcoord: [i16; 2],
    /// Raw Vtx attribute bytes. With lighting enabled these are signed normals
    /// plus alpha; with lighting disabled they are RGBA vertex color.
    pub attributes: [u8; 4],
}

#[derive(Clone, Debug)]
pub struct RenderBatch {
    pub layer: String,
    pub texture_symbol: Option<String>,
    pub texture_size: Option<[u32; 2]>,
    /// Active N64 light/color group when this batch was emitted.
    pub light_symbol: Option<String>,
    /// Flattened triangle vertices, three entries per triangle.
    pub vertices: Vec<RenderVertex>,
}

#[derive(Clone, Debug, Default)]
pub struct ParsedRenderGeometry {
    pub batches: Vec<RenderBatch>,
    pub display_lists_executed: usize,
    pub triangle_count: usize,
}

#[derive(Clone, Debug)]
enum GfxCommand {
    SetTexture(String),
    ClearTexture,
    SetLight(String),
    SetTileSize([u32; 2]),
    Vertex {
        array: String,
        count: usize,
        first_slot: usize,
    },
    Triangle([usize; 3]),
    DisplayList(String),
    End,
}

#[derive(Clone, Debug)]
struct GeoDisplayList {
    layer: String,
    name: String,
}

#[derive(Clone, Debug)]
struct ExecState {
    cache: Vec<Option<RenderVertex>>,
    layer: String,
    texture_symbol: Option<String>,
    texture_size: Option<[u32; 2]>,
    light_symbol: Option<String>,
}

impl ExecState {
    fn new(layer: String) -> Self {
        Self {
            cache: vec![None; 32],
            layer,
            texture_symbol: None,
            texture_size: None,
            light_symbol: None,
        }
    }
}

pub fn load_level_render_geometry(
    decomp_root: impl AsRef<Path>,
    level: &str,
    area: u8,
) -> Result<ParsedRenderGeometry, CollisionParseError> {
    let root = decomp_root.as_ref();
    let area_dir = root
        .join("levels")
        .join(level)
        .join("areas")
        .join(area.to_string());

    let geo_path = area_dir.join("geo.inc.c");
    let geo_source = fs::read_to_string(&geo_path).map_err(|error| {
        CollisionParseError::new(format!("{}: {error}", geo_path.display()))
    })?;
    let roots = parse_geo_display_lists(&geo_source);
    if roots.is_empty() {
        return Err(CollisionParseError::new(format!(
            "{}: no GEO_DISPLAY_LIST entries found",
            geo_path.display()
        )));
    }

    let mut model_files = Vec::new();
    collect_model_files(&area_dir, &mut model_files).map_err(|error| {
        CollisionParseError::new(format!("{}: {error}", area_dir.display()))
    })?;

    let mut vertex_arrays = HashMap::<String, Vec<RenderVertex>>::new();
    let mut display_lists = HashMap::<String, Vec<GfxCommand>>::new();

    for path in model_files {
        let source = fs::read_to_string(&path).map_err(|error| {
            CollisionParseError::new(format!("{}: {error}", path.display()))
        })?;

        for (name, body) in extract_arrays(&source, "Vtx") {
            vertex_arrays.insert(name, parse_vertex_array(&body)?);
        }
        for (name, body) in extract_arrays(&source, "Gfx") {
            display_lists.insert(name, parse_display_list(&body)?);
        }
    }

    let mut output = ParsedRenderGeometry::default();
    for root in roots {
        let mut state = ExecState::new(root.layer);
        let mut recursion = HashSet::new();
        execute_display_list(
            &root.name,
            &display_lists,
            &vertex_arrays,
            &mut state,
            &mut output,
            &mut recursion,
        )?;
    }
    coalesce_opaque_batches(&mut output);
    Ok(output)
}

fn coalesce_opaque_batches(output: &mut ParsedRenderGeometry) {
    let batches = std::mem::take(&mut output.batches);
    let mut merged = Vec::<RenderBatch>::with_capacity(batches.len());
    let mut by_key =
        HashMap::<(String, Option<String>, Option<[u32; 2]>, Option<String>), usize>::new();

    for batch in batches {
        // Preserve ordering for blended geometry; opaque and alpha-tested
        // batches can be safely grouped by render state.
        if batch.layer.contains("TRANSPARENT") {
            merged.push(batch);
            continue;
        }
        let key = (
            batch.layer.clone(),
            batch.texture_symbol.clone(),
            batch.texture_size,
            batch.light_symbol.clone(),
        );
        if let Some(&index) = by_key.get(&key) {
            merged[index].vertices.extend(batch.vertices);
        } else {
            by_key.insert(key, merged.len());
            merged.push(batch);
        }
    }
    output.batches = merged;
}


pub fn load_model_display_lists(
    decomp_root: impl AsRef<Path>,
    relative_model_path: impl AsRef<Path>,
    roots: &[(&str, &str)],
) -> Result<ParsedRenderGeometry, CollisionParseError> {
    let path=decomp_root.as_ref().join(relative_model_path.as_ref());
    let source=fs::read_to_string(&path)
        .map_err(|error|CollisionParseError::new(format!("{}: {error}",path.display())))?;

    let mut vertex_arrays=HashMap::<String,Vec<RenderVertex>>::new();
    let mut display_lists=HashMap::<String,Vec<GfxCommand>>::new();
    for (name,body) in extract_arrays(&source,"Vtx") {
        vertex_arrays.insert(name,parse_vertex_array(&body)?);
    }
    for (name,body) in extract_arrays(&source,"Gfx") {
        display_lists.insert(name,parse_display_list(&body)?);
    }

    let mut output=ParsedRenderGeometry::default();
    for (layer,name) in roots {
        let mut state=ExecState::new((*layer).to_owned());
        let mut recursion=HashSet::new();
        execute_display_list(
            name,
            &display_lists,
            &vertex_arrays,
            &mut state,
            &mut output,
            &mut recursion,
        )?;
    }
    Ok(output)
}

fn collect_model_files(dir: &Path, output: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_model_files(&path, output)?;
        } else if path.file_name().is_some_and(|name| name == "model.inc.c") {
            output.push(path);
        }
    }
    output.sort();
    Ok(())
}

fn parse_geo_display_lists(source: &str) -> Vec<GeoDisplayList> {
    source
        .lines()
        .filter_map(|raw| {
            let line = raw.trim();
            let args = macro_args(line, "GEO_DISPLAY_LIST")?;
            let parts = split_args(args);
            (parts.len() >= 2).then(|| GeoDisplayList {
                layer: parts[0].trim().to_owned(),
                name: parts[1].trim().to_owned(),
            })
        })
        .collect()
}

fn parse_vertex_array(body: &str) -> Result<Vec<RenderVertex>, CollisionParseError> {
    let number = Regex::new(r"-?0x[0-9A-Fa-f]+|-?\d+")
        .map_err(|error| CollisionParseError::new(error.to_string()))?;
    let mut vertices = Vec::new();

    for raw in body.lines() {
        if !raw.contains("{{{") {
            continue;
        }
        let cleaned = strip_line_comment(raw);
        let values = number
            .find_iter(cleaned)
            .filter_map(|m| parse_i32(m.as_str()).ok())
            .collect::<Vec<_>>();
        if values.len() < 10 {
            continue;
        }

        vertices.push(RenderVertex {
            position: [values[0] as f32, values[1] as f32, values[2] as f32],
            texcoord: [values[4] as i16, values[5] as i16],
            attributes: [
                values[6] as u8,
                values[7] as u8,
                values[8] as u8,
                values[9] as u8,
            ],
        });
    }
    Ok(vertices)
}

fn parse_display_list(body: &str) -> Result<Vec<GfxCommand>, CollisionParseError> {
    let mut commands = Vec::new();

    for raw in body.lines() {
        let line = raw.trim();
        if let Some(args) = macro_args(line, "gsDPSetTextureImage") {
            let parts = split_args(args);
            if let Some(symbol) = parts.last() {
                commands.push(GfxCommand::SetTexture(symbol.trim().to_owned()));
            }
        } else if let Some(args) = macro_args(line, "gsDPLoadTextureBlock") {
            let parts = split_args(args);
            if let Some(symbol) = parts.first() {
                commands.push(GfxCommand::SetTexture(symbol.trim().to_owned()));
            }
            if parts.len() >= 5 {
                let width=parse_i32(parts[3]).ok().filter(|v|*v>0).unwrap_or(32) as u32;
                let height=parse_i32(parts[4]).ok().filter(|v|*v>0).unwrap_or(32) as u32;
                commands.push(GfxCommand::SetTileSize([width,height]));
            }
        } else if let Some(args) = macro_args(line, "gsSPTexture") {
            if args.split(',').last().is_some_and(|value|value.trim()=="G_OFF") {
                commands.push(GfxCommand::ClearTexture);
            }
        } else if let Some(args) = macro_args(line, "gsSPLight") {
            let parts=split_args(args);
            if parts.len()>=2 && parts[1].trim()=="1" {
                let symbol=parts[0]
                    .trim()
                    .trim_start_matches('&')
                    .split('.')
                    .next()
                    .unwrap_or(parts[0].trim())
                    .to_owned();
                commands.push(GfxCommand::SetLight(symbol));
            }
        } else if let Some(args) = macro_args(line, "gsDPSetTileSize") {
            if let Some(size) = parse_tile_size(args) {
                commands.push(GfxCommand::SetTileSize(size));
            }
        } else if let Some(args) = macro_args(line, "gsSPVertex") {
            let parts = split_args(args);
            if parts.len() >= 3 {
                commands.push(GfxCommand::Vertex {
                    array: parts[0].trim().to_owned(),
                    count: parse_i32(parts[1]).unwrap_or(0).max(0) as usize,
                    first_slot: parse_i32(parts[2]).unwrap_or(0).max(0) as usize,
                });
            }
        } else if let Some(args) = macro_args(line, "gsSP1Triangle") {
            let parts = split_args(args);
            if parts.len() >= 3 {
                commands.push(GfxCommand::Triangle([
                    parse_i32(parts[0]).unwrap_or(0) as usize,
                    parse_i32(parts[1]).unwrap_or(0) as usize,
                    parse_i32(parts[2]).unwrap_or(0) as usize,
                ]));
            }
        } else if let Some(args) = macro_args(line, "gsSP2Triangles") {
            let parts = split_args(args);
            if parts.len() >= 7 {
                commands.push(GfxCommand::Triangle([
                    parse_i32(parts[0]).unwrap_or(0) as usize,
                    parse_i32(parts[1]).unwrap_or(0) as usize,
                    parse_i32(parts[2]).unwrap_or(0) as usize,
                ]));
                commands.push(GfxCommand::Triangle([
                    parse_i32(parts[4]).unwrap_or(0) as usize,
                    parse_i32(parts[5]).unwrap_or(0) as usize,
                    parse_i32(parts[6]).unwrap_or(0) as usize,
                ]));
            }
        } else if let Some(args) = macro_args(line, "gsSPDisplayList") {
            commands.push(GfxCommand::DisplayList(args.trim().to_owned()));
        } else if line.starts_with("gsSPEndDisplayList") {
            commands.push(GfxCommand::End);
        }
    }
    Ok(commands)
}

fn execute_display_list(
    name: &str,
    display_lists: &HashMap<String, Vec<GfxCommand>>,
    vertex_arrays: &HashMap<String, Vec<RenderVertex>>,
    state: &mut ExecState,
    output: &mut ParsedRenderGeometry,
    recursion: &mut HashSet<String>,
) -> Result<(), CollisionParseError> {
    if !recursion.insert(name.to_owned()) {
        return Err(CollisionParseError::new(format!(
            "recursive SM64 display list: {name}"
        )));
    }

    let commands = display_lists.get(name).ok_or_else(|| {
        CollisionParseError::new(format!("missing SM64 display list {name}"))
    })?;
    output.display_lists_executed += 1;

    for command in commands {
        match command {
            GfxCommand::SetTexture(symbol) => {
                state.texture_symbol = Some(symbol.clone());
            }
            GfxCommand::ClearTexture => {
                state.texture_symbol = None;
            }
            GfxCommand::SetLight(symbol) => {
                state.light_symbol = Some(symbol.clone());
            }
            GfxCommand::SetTileSize(size) => {
                state.texture_size = Some(*size);
            }
            GfxCommand::Vertex {
                array,
                count,
                first_slot,
            } => {
                let vertices = vertex_arrays.get(array).ok_or_else(|| {
                    CollisionParseError::new(format!("missing SM64 Vtx array {array}"))
                })?;
                for offset in 0..*count {
                    let Some(vertex) = vertices.get(offset).cloned() else {
                        break;
                    };
                    let slot = first_slot + offset;
                    if slot < state.cache.len() {
                        state.cache[slot] = Some(vertex);
                    }
                }
            }
            GfxCommand::Triangle(indices) => {
                let Some(a) = state.cache.get(indices[0]).and_then(Clone::clone) else {
                    continue;
                };
                let Some(b) = state.cache.get(indices[1]).and_then(Clone::clone) else {
                    continue;
                };
                let Some(c) = state.cache.get(indices[2]).and_then(Clone::clone) else {
                    continue;
                };
                push_triangle(output, state, [a, b, c]);
            }
            GfxCommand::DisplayList(child) => {
                execute_display_list(
                    child,
                    display_lists,
                    vertex_arrays,
                    state,
                    output,
                    recursion,
                )?;
            }
            GfxCommand::End => break,
        }
    }

    recursion.remove(name);
    Ok(())
}

fn push_triangle(
    output: &mut ParsedRenderGeometry,
    state: &ExecState,
    triangle: [RenderVertex; 3],
) {
    let same_batch = output.batches.last().is_some_and(|batch| {
        batch.layer == state.layer
            && batch.texture_symbol == state.texture_symbol
            && batch.texture_size == state.texture_size
            && batch.light_symbol == state.light_symbol
    });

    if !same_batch {
        output.batches.push(RenderBatch {
            layer: state.layer.clone(),
            texture_symbol: state.texture_symbol.clone(),
            texture_size: state.texture_size,
            light_symbol: state.light_symbol.clone(),
            vertices: Vec::new(),
        });
    }

    if let Some(batch) = output.batches.last_mut() {
        batch.vertices.extend(triangle);
        output.triangle_count += 1;
    }
}

fn parse_tile_size(args: &str) -> Option<[u32; 2]> {
    let re = Regex::new(r"\((\d+)\s*-\s*1\)").ok()?;
    let values = re
        .captures_iter(args)
        .filter_map(|capture| capture.get(1)?.as_str().parse::<u32>().ok())
        .collect::<Vec<_>>();
    (values.len() >= 2).then(|| [values[values.len() - 2], values[values.len() - 1]])
}

fn extract_arrays(source: &str, ty: &str) -> Vec<(String, String)> {
    let needle = format!("{ty} ");
    let mut arrays = Vec::new();
    let mut cursor = 0usize;

    while let Some(relative) = source[cursor..].find(&needle) {
        let type_start = cursor + relative;
        let name_start = type_start + needle.len();
        let Some(bracket_relative) = source[name_start..].find('[') else {
            break;
        };
        let bracket = name_start + bracket_relative;
        let name = source[name_start..bracket].trim();
        if name.is_empty() || name.contains(char::is_whitespace) {
            cursor = bracket + 1;
            continue;
        }

        let Some(eq_relative) = source[bracket..].find('=') else {
            cursor = bracket + 1;
            continue;
        };
        let eq = bracket + eq_relative;
        let Some(open_relative) = source[eq..].find('{') else {
            cursor = eq + 1;
            continue;
        };
        let open = eq + open_relative;

        let bytes = source.as_bytes();
        let mut depth = 0i32;
        let mut end = None;
        for (index, byte) in bytes.iter().enumerate().skip(open) {
            match *byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(index);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            break;
        };

        arrays.push((
            name.to_owned(),
            source[open + 1..end].to_owned(),
        ));
        cursor = end + 1;
    }

    arrays
}

fn macro_args<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(name)?.trim_start();
    let rest = rest.strip_prefix('(')?;
    let mut depth = 1i32;
    for (index, ch) in rest.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&rest[..index]);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_args(args: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    for (index, ch) in args.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(args[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(args[start..].trim());
    parts
}

fn parse_i32(text: &str) -> Result<i32, String> {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix("-0x") {
        i32::from_str_radix(rest, 16)
            .map(|value| -value)
            .map_err(|error| error.to_string())
    } else if let Some(rest) = text.strip_prefix("0x") {
        i32::from_str_radix(rest, 16).map_err(|error| error.to_string())
    } else {
        text.parse::<i32>().map_err(|error| error.to_string())
    }
}

fn strip_line_comment(line: &str) -> &str {
    line.split("//").next().unwrap_or(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_size_reads_original_dimensions() {
        assert_eq!(
            parse_tile_size("0, 0, 0, (32 - 1) << G_TEXTURE_IMAGE_FRAC, (64 - 1) << G_TEXTURE_IMAGE_FRAC"),
            Some([32, 64])
        );
    }

    #[test]
    fn argument_split_ignores_nested_parentheses() {
        assert_eq!(
            split_args("0, CALC_DXT(32, G_IM_SIZ_16b_BYTES), 4"),
            vec!["0", "CALC_DXT(32, G_IM_SIZ_16b_BYTES)", "4"]
        );
    }
}

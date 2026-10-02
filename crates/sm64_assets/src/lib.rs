mod discover;
mod collision_source;
mod level_script;
mod object_spawns;
mod render_geometry;
mod surface_names;
mod texture_sources;

pub use collision_source::{CollisionParseError, ParsedCollision, SpecialObject, parse_collision_source};
pub use level_script::{AreaSettings, MarioSpawn, load_area_settings, load_mario_spawn, parse_area_settings, parse_mario_spawn};
pub use render_geometry::{ParsedRenderGeometry, RenderBatch, RenderVertex, load_level_render_geometry, load_model_display_lists};
pub use surface_names::{surface_type_by_name, terrain_type_by_name};

use std::path::{Path, PathBuf};

pub fn load_collision_file(path: impl AsRef<Path>) -> Result<ParsedCollision, CollisionParseError> {
    let path=path.as_ref();
    let text=std::fs::read_to_string(path)
        .map_err(|e|CollisionParseError::new(format!("{}: {e}",path.display())))?;
    parse_collision_source(&text)
}

pub fn collision_path(
    decomp_root: impl AsRef<Path>,
    level: &str,
    area: u8,
) -> PathBuf {
    decomp_root
        .as_ref()
        .join("levels")
        .join(level)
        .join("areas")
        .join(area.to_string())
        .join("collision.inc.c")
}

pub fn load_level_collision(
    decomp_root: impl AsRef<Path>,
    level: &str,
    area: u8,
) -> Result<ParsedCollision, CollisionParseError> {
    load_collision_file(collision_path(decomp_root, level, area))
}

pub use texture_sources::{load_actor_texture_sources, load_texture_sources};

pub use discover::discover_levels;

pub use object_spawns::{LevelObjectSpawn, MacroObjectSpawn, MacroPresetDefinition, ParsedObjectSpawns, load_level_object_spawns, parse_level_script_objects, parse_macro_objects, parse_macro_preset_definitions};

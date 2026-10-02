mod collision_source;
mod surface_names;

pub use collision_source::{CollisionParseError, ParsedCollision, SpecialObject, parse_collision_source};
pub use surface_names::surface_type_by_name;

use std::path::Path;

pub fn load_collision_file(path: impl AsRef<Path>) -> Result<ParsedCollision, CollisionParseError> {
    let path=path.as_ref();
    let text=std::fs::read_to_string(path)
        .map_err(|e|CollisionParseError::new(format!("{}: {e}",path.display())))?;
    parse_collision_source(&text)
}

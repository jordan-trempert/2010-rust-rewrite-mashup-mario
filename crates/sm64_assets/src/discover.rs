use std::path::Path;

use crate::CollisionParseError;

pub fn discover_levels(
    decomp_root: impl AsRef<Path>,
) -> Result<Vec<String>, CollisionParseError> {
    let levels_dir=decomp_root.as_ref().join("levels");
    let entries=std::fs::read_dir(&levels_dir)
        .map_err(|e|CollisionParseError::new(format!("{}: {e}",levels_dir.display())))?;
    let mut levels=Vec::new();

    for entry in entries {
        let entry=entry.map_err(|e|CollisionParseError::new(e.to_string()))?;
        let path=entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name)=path.file_name().and_then(|name|name.to_str()) else {continue;};
        if path.join("script.c").is_file()
            && path.join("areas").join("1").join("collision.inc.c").is_file()
        {
            levels.push(name.to_owned());
        }
    }

    levels.sort_by(|a,b| {
        let (a_group,_) = level_sort_key(a);
        let (b_group,_) = level_sort_key(b);
        a_group.cmp(&b_group).then_with(||a.cmp(b))
    });
    Ok(levels)
}

fn level_sort_key(name:&str)->(u8,&str) {
    let main=matches!(
        name,
        "bob"|"wf"|"jrb"|"ccm"|"bbh"|"hmc"|"lll"|"ssl"|"ddd"|"sl"|"wdw"|"ttm"|"thi"|"ttc"|"rr"
    );
    let hub=matches!(name,"castle_grounds"|"castle_inside"|"castle_courtyard");
    (if main {0}else if hub {1}else{2},name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_courses_sort_before_hubs_and_secrets() {
        assert!(level_sort_key("bob")<level_sort_key("castle_inside"));
        assert!(level_sort_key("castle_inside")<level_sort_key("bits"));
    }
}

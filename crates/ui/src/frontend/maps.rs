use crate::MenuMapList;

pub fn map_label(map: &str) -> String {
    if let Some(("sm64", stem)) = map.split_once(':') {
        return match stem {
            "bob" => "BOB-OMB BATTLEFIELD".into(),
            _ => stem.replace('_', " ").to_uppercase(),
        };
    }
    map.split_once(':')
        .map_or(map, |(_, name)| name)
        .trim_start_matches("mp_")
        .replace('_', " ")
        .to_uppercase()
}

pub fn map_preview(map: &str) -> String {
    match map.split_once(':').unwrap_or(("iw4", map)) {
        (_, "") => String::new(),
        ("t5", stem) => format!(
            "t5:material/menu_mp_map_select_{}_big",
            stem.trim_start_matches("mp_")
        ),
        ("sm64", _) => String::new(),
        (namespace, stem) => format!("{namespace}:material/preview_{stem}"),
    }
}

pub fn pack_maps(maps: &MenuMapList, pack: usize) -> &[String] {
    maps.0.get(pack).map_or(&[], |pack| pack.maps.as_slice())
}

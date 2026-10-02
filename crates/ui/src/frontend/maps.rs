use crate::MenuMapList;

pub fn map_label(map: &str) -> String {
    if let Some(("sm64", stem)) = map.split_once(':') {
        return match stem {
            "bob" => "BOB-OMB BATTLEFIELD".into(),
            "wf" => "WHOMP'S FORTRESS".into(),
            "jrb" => "JOLLY ROGER BAY".into(),
            "ccm" => "COOL, COOL MOUNTAIN".into(),
            "bbh" => "BIG BOO'S HAUNT".into(),
            "hmc" => "HAZY MAZE CAVE".into(),
            "lll" => "LETHAL LAVA LAND".into(),
            "ssl" => "SHIFTING SAND LAND".into(),
            "ddd" => "DIRE, DIRE DOCKS".into(),
            "sl" => "SNOWMAN'S LAND".into(),
            "wdw" => "WET-DRY WORLD".into(),
            "ttm" => "TALL, TALL MOUNTAIN".into(),
            "thi" => "TINY-HUGE ISLAND".into(),
            "ttc" => "TICK TOCK CLOCK".into(),
            "rr" => "RAINBOW RIDE".into(),
            "castle_grounds" => "CASTLE GROUNDS".into(),
            "castle_inside" => "PEACH'S CASTLE".into(),
            "castle_courtyard" => "CASTLE COURTYARD".into(),
            "bitdw" => "BOWSER IN THE DARK WORLD".into(),
            "bitfs" => "BOWSER IN THE FIRE SEA".into(),
            "bits" => "BOWSER IN THE SKY".into(),
            "pss" => "THE PRINCESS'S SECRET SLIDE".into(),
            "cotmc" => "CAVERN OF THE METAL CAP".into(),
            "totwc" => "TOWER OF THE WING CAP".into(),
            "vcutm" => "VANISH CAP UNDER THE MOAT".into(),
            "wmotr" => "WING MARIO OVER THE RAINBOW".into(),
            "sa" => "THE SECRET AQUARIUM".into(),
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

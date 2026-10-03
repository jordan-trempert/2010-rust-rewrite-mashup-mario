use frame::UiMenuDvars;

use crate::MenuMapList;

pub fn selected_game(
    dvars: &UiMenuDvars,
    maps: &MenuMapList,
) -> Result<(String, sim::HostGameModeSelection), String> {
    let selected = dvars.get("ui_mapname").ok_or("No map selected")?;
    if !maps.contains(selected) {
        return Err(format!("Map `{selected}` is not installed"));
    }
    let mode = dvars
        .get("ui_gametype")
        .and_then(sim::HostGameModeSelection::from_token)
        .ok_or("Unsupported game mode")?;

    // The selector keeps the public-facing sm64:<level> key, but matches are
    // launched through the normal COD session loader.  The sm64cod namespace
    // deliberately does not match Sm64LaunchRequest::from_map_key, so the
    // frontend takes the same SessionSwapRequest/loading/HUD/player path as an
    // IW4 map instead of entering the old standalone Mario debug scene.
    let map = selected
        .strip_prefix("sm64:")
        .map_or_else(|| selected.to_owned(), |level| format!("sm64cod:{level}"));
    Ok((map, mode))
}

pub const MATCH_CONFIG: &str = "default_xboxlive.cfg";

fn is_rule(name: &str) -> bool {
    name.starts_with("scr_") || name == "g_hardcore"
}

pub fn seed_rules(dvars: &mut UiMenuDvars, config: &str) {
    for line in config.lines() {
        let line = line.split("//").next().unwrap_or("").trim();
        let Some(rest) = line.strip_prefix("set ") else {
            continue;
        };
        let mut words = rest.split_whitespace();
        let (Some(name), value) = (words.next(), words.next().unwrap_or("")) else {
            continue;
        };
        let name = name.to_ascii_lowercase();
        if is_rule(&name) && dvars.get(&name).is_none() {
            dvars.set(&name, value.trim_matches('"'));
        }
    }
}

pub fn host_rules(dvars: &UiMenuDvars) -> frame::HostMatchRules {
    frame::HostMatchRules(
        dvars
            .iter()
            .filter(|(name, _)| is_rule(name))
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect(),
    )
}

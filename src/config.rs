//! server.properties, same `key=value` format as PNX/vanilla. Written with defaults on
//! first run, unknown keys are left alone.

use std::collections::HashMap;
use std::sync::OnceLock;

pub struct Config {
    pub port: u16,
    pub motd: String,
    pub sub_motd: String,
    pub max_players: u32,
    pub level_name: String,
    /// normal, flat, void, or a plugin generator
    pub level_type: String,
    /// empty = random seed
    pub level_seed: String,
    /// bottom-up, e.g. `bedrock,2*dirt,grass_block`
    pub flat_layers: String,
    pub gamemode: String,
    pub difficulty: String,
    pub view_distance: i32,
    pub white_list: bool,
}

const DEFAULTS: &str = "\
# Rusted server settings. Restart the server after editing.
server-port=19132
motd=Rusted
sub-motd=Rusted
max-players=35
# Each level gets its own folder under worlds/.
level-name=world
# normal (vanilla-style), legacy (Rusted's first simple terrain), flat, void, or a plugin
# generator. Only used when the level is created.
level-type=normal
# Number or text; empty = random. Only used when the level is created.
level-seed=
flat-layers=bedrock,2*dirt,grass_block
gamemode=survival
difficulty=normal
# Max chunks sent around each player (2-96). Players get what their game asks for, up to
# this. 16 suits most servers; raise it on a strong machine.
view-distance=16
white-list=false
";

static CONFIG: OnceLock<Config> = OnceLock::new();

pub fn get() -> &'static Config {
    CONFIG.get_or_init(|| load(std::path::Path::new("server.properties")))
}

fn parse(text: &str) -> HashMap<String, String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

pub fn load(path: &std::path::Path) -> Config {
    let text = std::fs::read_to_string(path).unwrap_or_else(|_| {
        let _ = std::fs::write(path, DEFAULTS);
        DEFAULTS.to_string()
    });
    from_map(parse(DEFAULTS), parse(&text))
}

fn from_map(defaults: HashMap<String, String>, mut m: HashMap<String, String>) -> Config {
    for (k, v) in defaults {
        m.entry(k).or_insert(v);
    }
    let s = |k: &str| m[k].clone();
    let port = std::env::var("RUSTED_PORT").ok().unwrap_or_else(|| s("server-port"));
    Config {
        port: port.parse().unwrap_or(19132),
        motd: s("motd"),
        sub_motd: s("sub-motd"),
        max_players: s("max-players").parse().unwrap_or(35),
        level_name: s("level-name"),
        level_type: s("level-type").to_lowercase(),
        level_seed: s("level-seed"),
        flat_layers: s("flat-layers"),
        gamemode: s("gamemode").to_lowercase(),
        difficulty: s("difficulty").to_lowercase(),
        view_distance: s("view-distance").parse().unwrap_or(16).clamp(2, 96),
        white_list: s("white-list") == "true",
    }
}

/// Numeric seeds are used as-is, text gets Java's String.hashCode like vanilla does.
pub fn seed_from(text: &str) -> Option<u64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(n) = t.parse::<i64>() {
        return Some(n as u64);
    }
    let h = t.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32));
    Some(h as i64 as u64)
}

/// Parses flat layers into block ids, None on an unknown block name.
pub fn flat_layers(spec: &str) -> Option<Vec<i32>> {
    let mut out = Vec::new();
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (n, name) = match part.split_once('*') {
            Some((n, name)) => (n.trim().parse::<usize>().ok()?, name.trim()),
            None => (1, part),
        };
        let name = if name.contains(':') { name.to_string() } else { format!("minecraft:{name}") };
        let name = if name == "minecraft:grass" { "minecraft:grass_block".to_string() } else { name };
        out.extend(std::iter::repeat_n(crate::blocks::any_state(&name)?, n));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_and_layers() {
        assert_eq!(seed_from("12345"), Some(12345));
        assert_eq!(seed_from("-1"), Some(u64::MAX));
        assert_eq!(seed_from(""), None);
        // Java: "hello".hashCode() == 99162322
        assert_eq!(seed_from("hello"), Some(99_162_322));
        let l = flat_layers("bedrock,2*dirt,grass").unwrap();
        assert_eq!(l, vec![crate::world::BEDROCK, crate::world::DIRT, crate::world::DIRT, crate::world::GRASS]);
        assert!(flat_layers("bedrock,nonsense_block").is_none());
        let c = from_map(parse(DEFAULTS), parse("max-players=10\nlevel-type=FLAT"));
        assert_eq!((c.max_players, c.level_type.as_str(), c.view_distance), (10, "flat", 16));
    }
}

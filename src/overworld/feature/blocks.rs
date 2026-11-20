//! Block lookups and the block properties PNX features test (`canBeReplaced`, `isSolid`, ...),
//! approximated from block names since Rusted has no per-block classes.

use std::collections::HashMap;
use std::sync::OnceLock;
use parking_lot::Mutex;

use crate::blocks::State;

/// Default state of `minecraft:<name>` (or a full id), cached. Panics on unknown names so a
/// typo in a port shows up in tests.
pub fn id(name: &str) -> i32 {
    static C: OnceLock<Mutex<HashMap<String, i32>>> = OnceLock::new();
    let c = C.get_or_init(Default::default);
    if let Some(v) = c.lock().get(name) {
        return *v;
    }
    let full = if name.contains(':') { name.to_string() } else { format!("minecraft:{name}") };
    let v = crate::blocks::any_state(&full).unwrap_or_else(|| panic!("unknown block {full}"));
    let v = default_state(v);
    c.lock().insert(name.to_string(), v);
    v
}

/// The state PNX calls "default": first palette entry with common properties normalised.
fn default_state(v: i32) -> i32 {
    let mut out = v;
    for (k, s) in [
        ("pillar_axis", State::Str("y".into())),
        ("persistent_bit", State::Bool(false)),
        ("update_bit", State::Bool(false)),
        ("liquid_depth", State::Int(0)),
        ("age_bit", State::Bool(false)),
        ("growth", State::Int(0)),
    ] {
        if crate::blocks::has_state(out, k) {
            out = crate::blocks::with(out, &[(k, s)]);
        }
    }
    out
}

/// `id` with some states set, e.g. `with(id("oak_log"), &[("pillar_axis", s("x"))])`.
pub fn with(base: i32, states: &[(&str, State)]) -> i32 {
    crate::blocks::with(base, states)
}

pub fn s(v: &str) -> State {
    State::Str(v.into())
}
pub fn i(v: i32) -> State {
    State::Int(v)
}
pub fn b(v: bool) -> State {
    State::Bool(v)
}

pub fn air() -> i32 {
    crate::world::AIR
}

pub fn name(b: i32) -> &'static str {
    crate::blocks::entry(b).map_or("minecraft:air", |e| e.name.as_str())
}

/// Name without the `minecraft:` prefix.
pub fn short(b: i32) -> &'static str {
    name(b).trim_start_matches("minecraft:")
}

pub fn is(b: i32, n: &str) -> bool {
    short(b) == n
}

pub fn is_air(b: i32) -> bool {
    b == air() || matches!(short(b), "air" | "cave_air" | "void_air")
}

pub fn is_water(b: i32) -> bool {
    matches!(short(b), "water" | "flowing_water")
}

pub fn is_lava(b: i32) -> bool {
    matches!(short(b), "lava" | "flowing_lava")
}

pub fn is_liquid(b: i32) -> bool {
    is_water(b) || is_lava(b)
}

#[cfg(test)]
pub fn is_log(b: i32) -> bool {
    let n = short(b);
    n.ends_with("_log") || n.ends_with("_wood") || n.ends_with("_stem") || n.ends_with("_hyphae")
}

pub fn is_leaves(b: i32) -> bool {
    short(b).ends_with("leaves")
}

/// Dirt-like blocks plants and trees grow on (PNX `isSupportDirt` style checks).
pub fn is_dirt(b: i32) -> bool {
    matches!(short(b), "grass_block" | "dirt" | "coarse_dirt" | "podzol" | "mycelium" | "rooted_dirt" | "moss_block" | "mud" | "muddy_mangrove_roots" | "farmland" | "dirt_with_roots" | "pale_moss_block")
}

/// Small plants and other blocks that `canBeReplaced` in PNX (placing over them is allowed).
pub fn is_plant(b: i32) -> bool {
    let n = short(b);
    matches!(n, "short_grass" | "tall_grass" | "fern" | "large_fern" | "deadbush" | "dead_bush" | "vine" | "snow_layer" | "short_dry_grass" | "tall_dry_grass" | "bush" | "leaf_litter" | "seagrass" | "glow_lichen" | "firefly_bush")
        || n.ends_with("_flower") || n.ends_with("_tulip") || n.ends_with("_mushroom") && !n.contains("block")
        || matches!(n, "dandelion" | "poppy" | "blue_orchid" | "allium" | "azure_bluet" | "oxeye_daisy" | "cornflower" | "lily_of_the_valley" | "sunflower" | "lilac" | "rose_bush" | "peony" | "pink_petals" | "wildflowers" | "torchflower" | "closed_eyeblossom" | "open_eyeblossom")
}

/// PNX `canBeReplaced`.
pub fn replaceable(b: i32) -> bool {
    is_air(b) || is_liquid(b) || is_plant(b) || matches!(short(b), "fire" | "soul_fire" | "structure_void" | "light_block")
}

/// Roughly PNX `isSolid`: not air, liquid, plant or other pass-through block.
pub fn is_solid(b: i32) -> bool {
    !replaceable(b) && !is_leaves(b) && {
        let n = short(b);
        !(n.contains("sapling") || n.contains("torch") || n.contains("sign") || n.contains("button") || n.contains("rail") || n.contains("carpet") || n.contains("pressure_plate") || n == "ladder" || n == "cactus" || n == "reeds" || n == "sugar_cane" || n == "kelp" || n == "bamboo" || n == "sweet_berry_bush" || n == "cocoa" || n.ends_with("coral") || n.ends_with("coral_fan") || n == "sea_pickle" || n == "waterlily" || n == "lily_pad"
            || n == "redstone_wire" || n == "lever" || n.contains("repeater") || n.contains("comparator") || n == "tripwire" || n == "trip_wire" || n == "end_rod" || n == "lightning_rod" || n == "flower_pot")
    }
}

/// Roughly PNX `isFullBlock`.
pub fn is_full(b: i32) -> bool {
    is_solid(b) && {
        let n = short(b);
        !(n.ends_with("_slab") || n.ends_with("_stairs") || n.ends_with("_fence") || n.ends_with("_wall") || n.ends_with("_door") || n.ends_with("_trapdoor") || n.contains("pane") || n.ends_with("_bed") || n == "snow_layer" || n.contains("dripstone") || n.contains("chain") || n.contains("lantern"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_properties() {
        assert!(is_air(air()));
        assert!(is_log(id("oak_log")) && is_leaves(id("oak_leaves")));
        assert!(is_dirt(id("grass_block")) && !is_dirt(id("stone")));
        assert!(replaceable(id("short_grass")) && !replaceable(id("stone")));
        assert!(is_full(id("stone")) && !is_full(id("oak_slab")));
        assert_eq!(short(id("deepslate")), "deepslate");
    }
}

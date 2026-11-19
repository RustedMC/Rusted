//! Block drops (vanilla basics) and the tool tier each block needs.

use bedrock::protocol::v2168::types::NetworkItemStackDescriptorV2;

/// 0 none, 1 wood/gold, 2 stone, 3 iron, 4 diamond/netherite
pub fn pickaxe_tier(held: Option<&str>) -> u8 {
    match held.unwrap_or("").strip_prefix("minecraft:").unwrap_or("") {
        "wooden_pickaxe" | "golden_pickaxe" => 1,
        "stone_pickaxe" => 2,
        "iron_pickaxe" => 3,
        "diamond_pickaxe" | "netherite_pickaxe" => 4,
        _ => 0,
    }
}

/// Tier needed to get any drop (0 = hand is fine).
fn needs(name: &str) -> u8 {
    match name {
        "obsidian" | "crying_obsidian" | "ancient_debris" | "respawn_anchor" | "netherite_block" => 4,
        n if n.contains("diamond") || n.contains("emerald") || n.contains("gold") || n.contains("redstone_ore") => 3,
        n if n.contains("iron") || n.contains("copper") || n.contains("lapis") => 2,
        n if n.ends_with("_ore")
            || n.contains("stone") && !n.contains("glowstone") && !n.contains("redstone")
            || n.contains("deepslate")
            || n.contains("cobble")
            || n.contains("brick")
            || n.contains("andesite")
            || n.contains("diorite")
            || n.contains("granite")
            || n.contains("terracotta")
            || n.contains("prismarine")
            || n.contains("blackstone")
            || n.contains("basalt")
            || n.contains("netherrack")
            || n.contains("quartz")
            || n.contains("purpur")
            || n.contains("tuff")
            || n.contains("calcite")
            || n.contains("dripstone")
            || n == "furnace"
            || n == "smoker"
            || n == "blast_furnace"
            || n == "anvil"
            || n == "hopper"
            || n == "cauldron"
            || n == "coal_block"
            || n.ends_with("concrete") =>
        {
            1
        }
        _ => 0,
    }
}

/// Cheap deterministic "random" from a position for chance drops.
fn roll(pos: (i32, i32, i32), salt: u32) -> u32 {
    let mut h = (pos.0 as u32).wrapping_mul(0x9E37_79B1) ^ (pos.1 as u32).wrapping_mul(0x85EB_CA77) ^ (pos.2 as u32).wrapping_mul(0xC2B2_AE3D) ^ salt;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^ (h >> 12)
}

/// Survival drops for breaking `block` with `held`.
pub fn drops(block: i32, pos: (i32, i32, i32), held: Option<&str>) -> Vec<(NetworkItemStackDescriptorV2, i8)> {
    let Some(entry) = crate::blocks::entry(block) else { return vec![] };
    let name = entry.name.strip_prefix("minecraft:").unwrap_or(&entry.name);
    if pickaxe_tier(held) < needs(name) {
        return vec![];
    }
    let item = |n: &str, c: i8| crate::items::item_by_name(n).map(|i| (i, c));
    let chance = |one_in: u32, salt: u32| roll(pos, salt).is_multiple_of(one_in);
    let out = match name {
        "air" | "bedrock" | "barrier" | "water" | "flowing_water" | "lava" | "flowing_lava" | "fire" | "portal" | "end_portal" => None,
        "stone" => item("cobblestone", 1),
        "deepslate" => item("cobbled_deepslate", 1),
        "grass_block" | "mycelium" | "podzol" | "farmland" | "grass_path" | "dirt_path" => item("dirt", 1),
        "coal_ore" | "deepslate_coal_ore" => item("coal", 1),
        "iron_ore" | "deepslate_iron_ore" => item("raw_iron", 1),
        "gold_ore" | "deepslate_gold_ore" => item("raw_gold", 1),
        "copper_ore" | "deepslate_copper_ore" => item("raw_copper", 2 + (roll(pos, 1) % 4) as i8),
        "diamond_ore" | "deepslate_diamond_ore" => item("diamond", 1),
        "emerald_ore" | "deepslate_emerald_ore" => item("emerald", 1),
        "lapis_ore" | "deepslate_lapis_ore" => item("lapis_lazuli", 4 + (roll(pos, 2) % 5) as i8),
        "redstone_ore" | "deepslate_redstone_ore" | "lit_redstone_ore" | "lit_deepslate_redstone_ore" => item("redstone", 4 + (roll(pos, 3) % 2) as i8),
        "nether_gold_ore" => item("gold_nugget", 2 + (roll(pos, 4) % 5) as i8),
        "quartz_ore" => item("quartz", 1),
        "glowstone" => item("glowstone_dust", 2 + (roll(pos, 5) % 3) as i8),
        "clay" => item("clay_ball", 4),
        "bookshelf" => item("book", 3),
        "melon_block" => item("melon_slice", 3 + (roll(pos, 6) % 5) as i8),
        "snow" => item("snowball", 4),
        "snow_layer" => item("snowball", 1),
        "gravel" if chance(10, 7) => item("flint", 1),
        "short_grass" | "tallgrass" | "tall_grass" | "fern" | "large_fern" => {
            if chance(8, 8) { item("wheat_seeds", 1) } else { None }
        }
        n if n.ends_with("leaves") => {
            let sapling = n.replace("leaves", "sapling");
            if chance(20, 9) { item(&sapling, 1) } else if n == "oak_leaves" && chance(200, 10) { item("apple", 1) } else { None }
        }
        n if n.contains("glass") || n == "ice" || n == "budding_amethyst" => None,
        n if n.ends_with("_door") && entry_bit(block, "upper_block_bit") => None, // only the lower half drops the door
        "wheat" => {
            if entry_int(block, "growth") >= 7 { item("wheat", 1) } else { item("wheat_seeds", 1) }
        }
        "carrots" => item("carrot", if entry_int(block, "growth") >= 7 { 2 + (roll(pos, 11) % 3) as i8 } else { 1 }),
        "potatoes" => item("potato", if entry_int(block, "growth") >= 7 { 2 + (roll(pos, 12) % 3) as i8 } else { 1 }),
        "beetroot" => {
            if entry_int(block, "growth") >= 7 { item("beetroot", 1) } else { item("beetroot_seeds", 1) }
        }
        _ => crate::items::item_for_block(block).map(|i| (i, 1)),
    };
    out.into_iter().collect()
}

fn entry_bit(block: i32, key: &str) -> bool {
    entry_int(block, key) != 0
}

fn entry_int(block: i32, key: &str) -> i32 {
    match crate::blocks::state(block, key) {
        Some(crate::blocks::State::Int(i)) => *i,
        Some(crate::blocks::State::Bool(b)) => *b as i32,
        _ => 0,
    }
}

/// Roughly how many game ticks mining `block` takes with `tool` (vanilla: hardness x 1.5 s
/// with a usable tool, x 5 without, divided by the tool's speed). Only used for the crack
/// animation other players see; the miner's own client times the break itself.
pub fn break_ticks(block: i32, tool: Option<&str>) -> i32 {
    let n = crate::blocks::entry(block).map_or("", |e| e.name.as_str()).trim_start_matches("minecraft:");
    let hardness: f32 = match n {
        "obsidian" | "crying_obsidian" | "respawn_anchor" => 50.0,
        "ancient_debris" => 30.0,
        "bedrock" | "barrier" | "end_portal_frame" => return i32::MAX / 2,
        "ender_chest" => 22.5,
        n if n.ends_with("_ore") => 3.0,
        n if n.contains("deepslate") => 3.0,
        n if n.contains("planks") || n.ends_with("_log") || n.ends_with("_wood") || n.contains("chest") || n.contains("fence") || n.ends_with("_door") => 2.0,
        "stone" | "cobblestone" | "stone_bricks" | "bricks" | "andesite" | "diorite" | "granite" | "blackstone" => 1.5,
        n if n.contains("stone") || n.contains("brick") => 1.5,
        n if n.contains("dirt") || n == "grass_block" || n.contains("sand") || n == "gravel" || n == "clay" || n == "farmland" || n.contains("mud") => 0.6,
        n if n.contains("leaves") => 0.2,
        n if n.contains("glass") => 0.3,
        n if n.contains("wool") || n.contains("carpet") => 0.8,
        n if crate::overworld::feature::blocks::is_plant(block) || n.contains("torch") || n.contains("sapling") || n == "redstone_wire" => 0.0,
        _ => 1.0,
    };
    let t = tool.unwrap_or("").trim_start_matches("minecraft:");
    let speed = match t.split('_').next() {
        Some("wooden") => 2.0,
        Some("stone") => 4.0,
        Some("iron") => 6.0,
        Some("diamond") => 8.0,
        Some("netherite") => 9.0,
        Some("golden") => 12.0,
        _ => 1.0,
    };
    let is_tool = t.ends_with("_pickaxe") || t.ends_with("_axe") || t.ends_with("_shovel") || t.ends_with("_hoe");
    let secs = hardness * if is_tool { 1.5 / speed } else { 1.5 };
    (secs * 20.0).ceil() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name_of(d: &[(NetworkItemStackDescriptorV2, i8)]) -> Vec<String> {
        d.iter().map(|(i, _)| crate::items::name_of(i.id).unwrap_or("?").to_string()).collect()
    }

    #[test]
    fn stone_needs_a_pickaxe_and_drops_cobblestone() {
        let stone = crate::blocks::any_state("minecraft:stone").unwrap();
        assert!(drops(stone, (0, 0, 0), None).is_empty(), "by hand: nothing");
        assert_eq!(name_of(&drops(stone, (0, 0, 0), Some("minecraft:wooden_pickaxe"))), ["minecraft:cobblestone"]);
        let diamond = crate::blocks::any_state("minecraft:diamond_ore").unwrap();
        assert!(drops(diamond, (0, 0, 0), Some("minecraft:stone_pickaxe")).is_empty());
        assert_eq!(name_of(&drops(diamond, (0, 0, 0), Some("minecraft:iron_pickaxe"))), ["minecraft:diamond"]);
        assert_eq!(name_of(&drops(crate::world::GRASS, (0, 0, 0), None)), ["minecraft:dirt"]);
        assert!(drops(crate::world::BEDROCK, (0, 0, 0), Some("minecraft:diamond_pickaxe")).is_empty());
    }
}

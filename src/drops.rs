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

/// Game ticks mining `block` takes (vanilla/PNX Block.calculateBreakTime): hardness x 1.5 s,
/// x 5 when the block needs a pickaxe and this isn't one; the right tool divides by its speed
/// (+ efficiency^2 + 1), haste speeds it up 20% a level. Sent as the crack animation's speed,
/// so it has to match the client's own timer or the cracks finish before the block breaks.
pub fn break_ticks(block: i32, tool: Option<&str>, efficiency: i16, haste: Option<i32>) -> i32 {
    let n = crate::blocks::entry(block).map_or("", |e| e.name.as_str()).trim_start_matches("minecraft:");
    let hardness: f32 = match n {
        "obsidian" | "crying_obsidian" | "respawn_anchor" => 50.0,
        "ancient_debris" => 30.0,
        "bedrock" | "barrier" | "end_portal_frame" => return i32::MAX / 2,
        "ender_chest" => 22.5,
        "iron_block" | "diamond_block" | "emerald_block" | "netherite_block" | "anvil" | "chipped_anvil" | "damaged_anvil" | "enchanting_table" => 5.0,
        "gold_block" | "redstone_block" | "lapis_block" | "copper_block" | "coal_block" | "hopper" | "iron_bars" | "iron_door" | "iron_trapdoor" | "mob_spawner" | "observer" | "end_stone" => 3.0,
        "furnace" | "lit_furnace" | "blast_furnace" | "lit_blast_furnace" | "smoker" | "lit_smoker" | "dispenser" | "dropper" => 3.5,
        n if n.starts_with("deepslate_") && n.ends_with("_ore") => 4.5,
        n if n.ends_with("_ore") => 3.0,
        "deepslate" | "cobbled_deepslate" | "polished_deepslate" => 3.0,
        n if n.contains("deepslate") => 3.5,
        n if n.contains("glazed_terracotta") => 1.4,
        n if n.contains("terracotta") => 1.25,
        n if n.ends_with("_concrete_powder") => 0.5,
        n if n.ends_with("_concrete") => 1.8,
        "netherrack" | "crimson_nylium" | "warped_nylium" => 0.4,
        "glowstone" | "sea_lantern" => 0.3,
        "ice" | "packed_ice" => 0.5,
        "blue_ice" => 2.8,
        n if n.contains("chest") || n == "crafting_table" || n == "barrel" || n == "lectern" => 2.5,
        "bookshelf" => 1.5,
        n if n.contains("planks") || n.ends_with("_log") || n.ends_with("_wood") || n.ends_with("_stem") || n.ends_with("_hyphae") || n.contains("fence") || n.ends_with("_door") => 2.0,
        "cobblestone" | "mossy_cobblestone" | "stone_bricks" | "bricks" | "blackstone" | "nether_brick" | "red_nether_brick" => 2.0,
        n if n.contains("sandstone") => 0.8,
        n if n.contains("stone") || n.contains("brick") || matches!(n, "andesite" | "diorite" | "granite" | "polished_andesite" | "polished_diorite" | "polished_granite" | "tuff" | "calcite") => 1.5,
        "grass_block" | "mycelium" | "podzol" | "farmland" | "grass_path" | "dirt_path" | "clay" | "gravel" => 0.6,
        n if n.contains("dirt") || n.contains("sand") || n.contains("mud") || n.starts_with("soul_") => 0.5,
        n if n.contains("leaves") => 0.2,
        n if n.contains("glass") => 0.3,
        n if n.contains("carpet") => 0.1,
        n if n.contains("wool") => 0.8,
        "pumpkin" | "carved_pumpkin" | "lit_pumpkin" | "melon_block" => 1.0,
        "snow" => 0.2,
        "snow_layer" => 0.1,
        n if crate::overworld::feature::blocks::is_plant(block) || n.contains("torch") || n.contains("sapling") || n == "redstone_wire" => 0.0,
        _ => 1.0,
    };
    if hardness == 0.0 {
        return 0;
    }
    let t = tool.unwrap_or("").trim_start_matches("minecraft:");
    let metal = matches!(n, "iron_block" | "diamond_block" | "emerald_block" | "netherite_block" | "gold_block" | "redstone_block" | "lapis_block" | "copper_block" | "coal_block");
    let pick = metal
        || (!n.contains("sand") || n.contains("sandstone")) && !n.contains("redstone_wire") && !n.contains("torch")
            && (n.ends_with("_ore") || n.contains("stone") || n.contains("brick") || n.contains("deepslate") || n.contains("obsidian") || n.contains("terracotta") || n.ends_with("_concrete"))
        || matches!(n, "netherrack" | "furnace" | "lit_furnace" | "blast_furnace" | "lit_blast_furnace" | "smoker" | "lit_smoker" | "dispenser" | "dropper" | "hopper"
            | "anvil" | "chipped_anvil" | "damaged_anvil" | "iron_bars" | "iron_door" | "iron_trapdoor" | "ender_chest" | "enchanting_table" | "mob_spawner"
            | "ancient_debris" | "respawn_anchor" | "observer" | "andesite" | "diorite" | "granite" | "polished_andesite" | "polished_diorite" | "polished_granite"
            | "tuff" | "calcite" | "basalt" | "polished_basalt" | "crimson_nylium" | "warped_nylium" | "ice" | "packed_ice" | "blue_ice");
    let axe = !pick
        && (n.contains("planks") || n.ends_with("_log") || n.ends_with("_wood") || n.ends_with("_stem") || n.ends_with("_hyphae") || n.contains("fence") || n.ends_with("_door")
            || n.contains("chest") || matches!(n, "crafting_table" | "barrel" | "bookshelf" | "lectern" | "pumpkin" | "carved_pumpkin" | "lit_pumpkin" | "melon_block"));
    let shovel = !pick
        && (n.contains("dirt") || n.contains("sand") || n.contains("mud") || n.starts_with("soul_")
            || matches!(n, "grass_block" | "mycelium" | "podzol" | "farmland" | "grass_path" | "dirt_path" | "clay" | "gravel" | "snow" | "snow_layer"));
    let hoe = n.contains("leaves") || matches!(n, "hay_block" | "sponge" | "wet_sponge" | "target" | "sculk" | "nether_wart_block" | "warped_wart_block");
    let shears = t == "shears" && (n.contains("leaves") || n.contains("wool") || n == "web");
    let right = (pick && t.ends_with("_pickaxe")) || (axe && t.ends_with("_axe")) || (shovel && t.ends_with("_shovel")) || (hoe && t.ends_with("_hoe")) || shears;
    // Blocks that drop nothing by hand take 5x as long without a pickaxe.
    let needs_tool = pick && !matches!(n, "ice" | "packed_ice" | "blue_ice");
    let mut secs = hardness * if needs_tool && !right { 5.0 } else { 1.5 };
    if right {
        let speed = if shears {
            if n.contains("wool") { 5.0 } else { 15.0 }
        } else {
            match t.split('_').next() {
                Some("wooden") => 2.0,
                Some("stone") => 4.0,
                Some("iron") => 6.0,
                Some("diamond") => 8.0,
                Some("netherite") => 9.0,
                Some("golden") => 12.0,
                _ => 1.0,
            }
        };
        let eff = if efficiency > 0 { (efficiency as f32).powi(2) + 1.0 } else { 0.0 };
        secs /= speed + eff;
    }
    if let Some(amp) = haste {
        secs /= 1.0 + 0.2 * (amp + 1) as f32;
    }
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

    #[test]
    fn break_times_match_vanilla() {
        let b = |n: &str| crate::blocks::any_state(n).unwrap();
        assert_eq!(break_ticks(b("minecraft:stone"), None, 0, None), 150, "stone by hand 7.5 s");
        assert_eq!(break_ticks(b("minecraft:stone"), Some("minecraft:wooden_pickaxe"), 0, None), 23, "1.15 s");
        assert_eq!(break_ticks(b("minecraft:dirt"), None, 0, None), 15, "dirt by hand 0.75 s");
        assert_eq!(break_ticks(b("minecraft:oak_planks"), None, 0, None), 60, "planks by hand 3 s");
        assert_eq!(break_ticks(b("minecraft:oak_planks"), Some("minecraft:stone_pickaxe"), 0, None), 60, "wrong tool is like a hand");
        assert_eq!(break_ticks(b("minecraft:obsidian"), Some("minecraft:diamond_pickaxe"), 0, None), 188, "9.4 s");
    }
}

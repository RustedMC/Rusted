//! Chest loot for generated structures. PNX leaves these chests empty, so the tables follow
//! vanilla's structure loot (items, weights and counts from the Minecraft Wiki). A structure's
//! chests are tagged while it generates and filled the first time someone opens them.

use crate::inventory::Stack;
use parking_lot::Mutex;
use std::collections::HashMap;

/// One draw from a pool: (item, weight, min count, max count).
type Entry = (&'static str, u32, i8, i8);

/// Draw `rolls` times (an inclusive range) from `entries`.
struct Pool {
    rolls: (u32, u32),
    entries: &'static [Entry],
}

// shortcut: enchanted books come out as plain books, upgrade once item NBT enchanting is
// available for loot.
const DESERT_PYRAMID: &[Pool] = &[
    Pool { rolls: (2, 4), entries: &[
        ("diamond", 5, 1, 3), ("iron_ingot", 15, 1, 5), ("gold_ingot", 15, 2, 7), ("emerald", 15, 1, 3),
        ("bone", 25, 4, 6), ("spider_eye", 25, 1, 3), ("rotten_flesh", 25, 3, 7), ("saddle", 20, 1, 1),
        ("iron_horse_armor", 15, 1, 1), ("golden_horse_armor", 10, 1, 1), ("diamond_horse_armor", 5, 1, 1),
        ("book", 20, 1, 1), ("golden_apple", 20, 1, 1), ("enchanted_golden_apple", 2, 1, 1),
    ] },
    Pool { rolls: (4, 4), entries: &[
        ("bone", 10, 1, 8), ("gunpowder", 10, 1, 8), ("rotten_flesh", 10, 1, 8), ("string", 10, 1, 8), ("sand", 10, 1, 8),
    ] },
];

const JUNGLE_TEMPLE: &[Pool] = &[Pool { rolls: (2, 6), entries: &[
    ("diamond", 3, 1, 3), ("iron_ingot", 10, 1, 5), ("gold_ingot", 15, 2, 7), ("emerald", 2, 1, 3),
    ("bone", 20, 4, 6), ("rotten_flesh", 16, 3, 7), ("saddle", 3, 1, 1), ("iron_horse_armor", 1, 1, 1),
    ("golden_horse_armor", 1, 1, 1), ("diamond_horse_armor", 1, 1, 1), ("book", 1, 1, 1),
] }];

const MINESHAFT: &[Pool] = &[
    Pool { rolls: (1, 1), entries: &[
        ("golden_apple", 20, 1, 1), ("enchanted_golden_apple", 1, 1, 1), ("name_tag", 30, 1, 1),
        ("book", 10, 1, 1), ("iron_pickaxe", 5, 1, 1), ("bread", 15, 1, 3),
    ] },
    Pool { rolls: (2, 4), entries: &[
        ("iron_ingot", 10, 1, 5), ("gold_ingot", 5, 1, 3), ("redstone", 5, 4, 9), ("lapis_lazuli", 5, 4, 9),
        ("diamond", 3, 1, 2), ("coal", 10, 3, 8), ("bread", 15, 1, 3), ("beetroot_seeds", 10, 2, 4),
        ("melon_seeds", 10, 2, 4), ("pumpkin_seeds", 10, 2, 4),
    ] },
    Pool { rolls: (3, 3), entries: &[
        ("rail", 20, 4, 8), ("golden_rail", 5, 1, 4), ("detector_rail", 5, 1, 4), ("activator_rail", 5, 1, 4), ("torch", 15, 1, 16),
    ] },
];

const STRONGHOLD: &[Pool] = &[Pool { rolls: (2, 3), entries: &[
    ("ender_pearl", 10, 1, 1), ("diamond", 3, 1, 3), ("iron_ingot", 10, 1, 5), ("gold_ingot", 5, 1, 3),
    ("redstone", 5, 4, 9), ("bread", 15, 1, 3), ("apple", 15, 1, 3), ("iron_pickaxe", 5, 1, 1),
    ("iron_sword", 5, 1, 1), ("iron_chestplate", 5, 1, 1), ("iron_helmet", 5, 1, 1), ("iron_leggings", 5, 1, 1),
    ("iron_boots", 5, 1, 1), ("golden_apple", 1, 1, 1), ("saddle", 1, 1, 1), ("iron_horse_armor", 1, 1, 1),
    ("golden_horse_armor", 1, 1, 1), ("diamond_horse_armor", 1, 1, 1), ("book", 1, 1, 1),
] }];

const VILLAGE: &[Pool] = &[Pool { rolls: (3, 8), entries: &[
    ("bread", 15, 1, 4), ("apple", 15, 1, 5), ("wheat", 10, 1, 7), ("iron_ingot", 5, 1, 3), ("gold_ingot", 3, 1, 3),
    ("emerald", 2, 1, 2), ("potato", 10, 1, 7), ("carrot", 10, 1, 7), ("wheat_seeds", 10, 1, 5), ("book", 3, 1, 1),
    ("oak_sapling", 5, 1, 2), ("torch", 5, 1, 4),
] }];

const IGLOO: &[Pool] = &[
    Pool { rolls: (2, 8), entries: &[
        ("apple", 15, 1, 3), ("coal", 15, 1, 4), ("gold_nugget", 10, 1, 3), ("stone_axe", 2, 1, 1),
        ("rotten_flesh", 10, 1, 1), ("emerald", 1, 1, 1), ("wheat", 10, 2, 3),
    ] },
    Pool { rolls: (1, 1), entries: &[("golden_apple", 1, 1, 1)] },
];

const SHIPWRECK: &[Pool] = &[
    Pool { rolls: (3, 10), entries: &[
        ("paper", 8, 1, 12), ("potato", 7, 2, 6), ("poisonous_potato", 7, 2, 6), ("carrot", 7, 4, 8),
        ("wheat", 7, 8, 21), ("coal", 6, 2, 8), ("rotten_flesh", 5, 5, 24), ("gunpowder", 3, 1, 5),
        ("leather_helmet", 3, 1, 1), ("pumpkin", 2, 1, 3), ("tnt", 1, 1, 2),
    ] },
    Pool { rolls: (3, 6), entries: &[
        ("iron_ingot", 90, 1, 5), ("gold_ingot", 10, 1, 5), ("emerald", 40, 1, 5), ("diamond", 5, 1, 1), ("experience_bottle", 5, 1, 1),
    ] },
];

const OCEAN_RUIN: &[Pool] = &[Pool { rolls: (2, 8), entries: &[
    ("coal", 10, 1, 4), ("stone_axe", 2, 1, 1), ("rotten_flesh", 5, 1, 1), ("emerald", 1, 1, 1),
    ("wheat", 10, 2, 3), ("gold_nugget", 10, 1, 3),
] }];

const PILLAGER_OUTPOST: &[Pool] = &[
    Pool { rolls: (0, 1), entries: &[("crossbow", 1, 1, 1)] },
    Pool { rolls: (2, 3), entries: &[("wheat", 7, 3, 5), ("potato", 5, 2, 5), ("carrot", 5, 3, 5)] },
    Pool { rolls: (1, 3), entries: &[("dark_oak_log", 1, 2, 3)] },
    Pool { rolls: (2, 3), entries: &[
        ("experience_bottle", 7, 1, 1), ("string", 4, 1, 6), ("arrow", 4, 2, 7), ("tripwire_hook", 3, 1, 3),
        ("iron_ingot", 3, 1, 3), ("book", 1, 1, 1),
    ] },
];

const WOODLAND_MANSION: &[Pool] = &[
    Pool { rolls: (1, 3), entries: &[
        ("lead", 20, 1, 1), ("golden_apple", 15, 1, 1), ("enchanted_golden_apple", 2, 1, 1), ("music_disc_13", 15, 1, 1),
        ("music_disc_cat", 15, 1, 1), ("name_tag", 20, 1, 1), ("chainmail_chestplate", 10, 1, 1),
        ("diamond_hoe", 15, 1, 1), ("diamond_chestplate", 5, 1, 1), ("book", 10, 1, 1),
    ] },
    Pool { rolls: (1, 4), entries: &[
        ("iron_ingot", 10, 1, 4), ("gold_ingot", 5, 1, 4), ("bread", 20, 1, 1), ("wheat", 20, 1, 4), ("bucket", 10, 1, 1),
        ("redstone", 15, 1, 4), ("coal", 15, 1, 4), ("melon_seeds", 10, 2, 4), ("pumpkin_seeds", 10, 2, 4), ("beetroot_seeds", 10, 2, 4),
    ] },
    Pool { rolls: (3, 3), entries: &[("bone", 10, 1, 8), ("gunpowder", 10, 1, 8), ("rotten_flesh", 10, 1, 8), ("string", 10, 1, 8)] },
];

const ANCIENT_CITY: &[Pool] = &[Pool { rolls: (5, 10), entries: &[
    ("enchanted_golden_apple", 1, 1, 2), ("music_disc_otherside", 1, 1, 1), ("compass", 2, 1, 1), ("sculk_catalyst", 2, 1, 2),
    ("name_tag", 2, 1, 1), ("diamond_hoe", 2, 1, 1), ("lead", 2, 1, 1), ("diamond_horse_armor", 2, 1, 1), ("saddle", 2, 1, 1),
    ("music_disc_13", 2, 1, 1), ("music_disc_cat", 2, 1, 1), ("diamond_leggings", 2, 1, 1), ("book", 3, 1, 1),
    ("sculk", 3, 4, 10), ("sculk_sensor", 3, 1, 3), ("candle", 3, 1, 4), ("amethyst_shard", 3, 1, 15),
    ("experience_bottle", 3, 1, 3), ("glow_berries", 3, 1, 15), ("iron_leggings", 3, 1, 1), ("echo_shard", 4, 1, 3),
    ("disc_fragment_5", 4, 1, 3), ("bone", 5, 1, 15), ("soul_torch", 5, 1, 15), ("coal", 7, 6, 15),
] }];

const TRIAL_CHAMBERS: &[Pool] = &[Pool { rolls: (3, 5), entries: &[
    ("arrow", 2, 4, 14), ("baked_potato", 2, 2, 4), ("glow_berries", 2, 2, 10), ("moss_block", 1, 2, 5),
    ("bone_meal", 1, 2, 5), ("tuff", 1, 5, 10), ("torch", 1, 3, 6), ("milk_bucket", 1, 1, 1),
] }];

const RUINED_PORTAL: &[Pool] = &[Pool { rolls: (4, 8), entries: &[
    ("obsidian", 40, 1, 2), ("flint", 40, 1, 4), ("iron_nugget", 40, 9, 18), ("flint_and_steel", 40, 1, 1),
    ("fire_charge", 40, 1, 1), ("golden_apple", 15, 1, 1), ("gold_nugget", 15, 4, 24), ("golden_sword", 15, 1, 1),
    ("golden_axe", 15, 1, 1), ("golden_hoe", 15, 1, 1), ("golden_shovel", 15, 1, 1), ("golden_pickaxe", 15, 1, 1),
    ("golden_boots", 15, 1, 1), ("golden_chestplate", 15, 1, 1), ("golden_helmet", 15, 1, 1), ("golden_leggings", 15, 1, 1),
    ("glistering_melon_slice", 5, 4, 12), ("golden_horse_armor", 5, 1, 1), ("light_weighted_pressure_plate", 5, 1, 1),
    ("golden_carrot", 5, 4, 12), ("clock", 5, 1, 1), ("gold_ingot", 5, 2, 8), ("bell", 1, 1, 1),
    ("enchanted_golden_apple", 1, 1, 1), ("gold_block", 1, 1, 2),
] }];

const NETHER_FORTRESS: &[Pool] = &[Pool { rolls: (2, 4), entries: &[
    ("diamond", 5, 1, 3), ("iron_ingot", 5, 1, 5), ("gold_ingot", 15, 1, 3), ("golden_sword", 5, 1, 1),
    ("golden_chestplate", 5, 1, 1), ("flint_and_steel", 5, 1, 1), ("nether_wart", 5, 3, 7), ("saddle", 10, 1, 1),
    ("golden_horse_armor", 8, 1, 1), ("iron_horse_armor", 5, 1, 1), ("diamond_horse_armor", 3, 1, 1), ("obsidian", 2, 2, 4),
] }];

// Vanilla end_city_treasure (gear comes unenchanted, see the shortcut above).
const END_CITY: &[Pool] = &[Pool { rolls: (2, 6), entries: &[
    ("diamond", 5, 2, 7), ("iron_ingot", 10, 4, 8), ("gold_ingot", 15, 2, 7), ("emerald", 2, 2, 6),
    ("beetroot_seeds", 5, 1, 10), ("saddle", 3, 1, 1), ("iron_horse_armor", 1, 1, 1), ("golden_horse_armor", 1, 1, 1),
    ("diamond_horse_armor", 1, 1, 1), ("diamond_sword", 3, 1, 1), ("diamond_boots", 3, 1, 1), ("diamond_chestplate", 3, 1, 1),
    ("diamond_leggings", 3, 1, 1), ("diamond_helmet", 3, 1, 1), ("diamond_pickaxe", 3, 1, 1), ("diamond_shovel", 3, 1, 1),
    ("iron_sword", 3, 1, 1), ("iron_boots", 3, 1, 1), ("iron_chestplate", 3, 1, 1), ("iron_leggings", 3, 1, 1),
    ("iron_helmet", 3, 1, 1), ("iron_pickaxe", 3, 1, 1), ("iron_shovel", 3, 1, 1),
] }];

const BASTION: &[Pool] = &[
    Pool { rolls: (1, 2), entries: &[
        ("diamond_pickaxe", 6, 1, 1), ("diamond_shovel", 6, 1, 1), ("crossbow", 6, 1, 1), ("ancient_debris", 12, 1, 1),
        ("netherite_scrap", 4, 1, 1), ("arrow", 10, 10, 22), ("piglin_banner_pattern", 9, 1, 1),
        ("music_disc_pigstep", 5, 1, 1), ("golden_carrot", 12, 6, 17), ("golden_apple", 9, 1, 1), ("book", 10, 1, 1),
    ] },
    Pool { rolls: (2, 2), entries: &[
        ("iron_sword", 2, 1, 1), ("iron_block", 2, 1, 1), ("golden_boots", 1, 1, 1), ("golden_axe", 1, 1, 1),
        ("gold_block", 2, 1, 1), ("crossbow", 1, 1, 1), ("gold_ingot", 2, 1, 6), ("iron_ingot", 2, 1, 6),
        ("golden_sword", 1, 1, 1), ("golden_chestplate", 1, 1, 1), ("golden_helmet", 1, 1, 1), ("golden_leggings", 1, 1, 1),
    ] },
    Pool { rolls: (3, 4), entries: &[
        ("crying_obsidian", 3, 1, 5), ("gilded_blackstone", 2, 1, 1), ("iron_chain", 2, 2, 10), ("magma_cream", 2, 2, 6),
        ("bone_block", 2, 3, 6), ("iron_nugget", 2, 2, 8), ("obsidian", 2, 4, 6), ("gold_nugget", 2, 2, 8),
        ("string", 2, 4, 6), ("arrow", 2, 5, 17), ("cooked_porkchop", 1, 1, 1),
    ] },
];

/// The loot for chests in a structure (`StructureType::name`), if it has any.
fn table(structure: &str) -> Option<&'static [Pool]> {
    Some(match structure.trim_start_matches("normal_").trim_start_matches("generic_") {
        "desert_pyramid" => DESERT_PYRAMID,
        "jungle_temple" => JUNGLE_TEMPLE,
        "mineshaft" => MINESHAFT,
        "stronghold" => STRONGHOLD,
        "village" => VILLAGE,
        "igloo" => IGLOO,
        "shipwreck" => SHIPWRECK,
        "ocean_ruin" => OCEAN_RUIN,
        "pillager_outpost" => PILLAGER_OUTPOST,
        "woodland_mansion" => WOODLAND_MANSION,
        "ancient_city" => ANCIENT_CITY,
        "trial_chambers" => TRIAL_CHAMBERS,
        "ruined_portal" => RUINED_PORTAL,
        "nether_fortress" => NETHER_FORTRESS,
        "bastion" => BASTION,
        "end_city" => END_CITY,
        _ => return None,
    })
}

/// Mob spawners placed by structures: position -> the mob they spawn.
pub fn spawners() -> &'static Mutex<HashMap<(i32, i32, i32), &'static str>> {
    static S: std::sync::OnceLock<Mutex<HashMap<(i32, i32, i32), &'static str>>> = std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}

/// A generated chest or spawner block: tags it for loot or spawning.
pub fn generated(pos: (i32, i32, i32), block: i32, structure: &'static str) {
    let Some(e) = crate::blocks::entry(block) else { return };
    match e.name.as_str() {
        "minecraft:chest" | "minecraft:barrel" => mark(pos, structure),
        "minecraft:mob_spawner" => {
            let mob = match structure.trim_start_matches("normal_") {
                "mineshaft" => "cave_spider",
                "stronghold" => "silverfish",
                "nether_fortress" => "blaze",
                _ => return,
            };
            spawners().lock().insert(pos, mob);
        }
        _ => {}
    }
}

/// Tagged chests waiting to be opened: position -> structure name.
fn marks() -> &'static Mutex<HashMap<(i32, i32, i32), &'static str>> {
    static M: std::sync::OnceLock<Mutex<HashMap<(i32, i32, i32), &'static str>>> = std::sync::OnceLock::new();
    M.get_or_init(Default::default)
}

/// Chests already looted. Chunks regenerate from the seed, so without this an emptied
/// structure chest would fill up again after a restart. Saved in worlds/<level>/looted.txt.
fn looted() -> &'static Mutex<std::collections::HashSet<(i32, i32, i32)>> {
    static L: std::sync::OnceLock<Mutex<std::collections::HashSet<(i32, i32, i32)>>> = std::sync::OnceLock::new();
    L.get_or_init(|| {
        let text = looted_file().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        let set = text
            .lines()
            .filter_map(|l| {
                let v: Vec<i32> = l.split_whitespace().filter_map(|n| n.parse().ok()).collect();
                (v.len() == 3).then(|| (v[0], v[1], v[2]))
            })
            .collect();
        Mutex::new(set)
    })
}

fn looted_file() -> Option<std::path::PathBuf> {
    if cfg!(test) {
        return None; // tests mustn't touch the real world folder
    }
    Some(std::path::Path::new("worlds").join(&crate::config::get().level_name).join("looted.txt"))
}

/// The world generator placed a chest at `pos` as part of `structure`.
pub fn mark(pos: (i32, i32, i32), structure: &'static str) {
    if table(structure).is_some() && !looted().lock().contains(&pos) {
        marks().lock().insert(pos, structure);
    }
}

/// Fill for the generated chest at `pos`, the first time it's opened. `None` for player
/// chests and chests already looted.
pub fn take(pos: (i32, i32, i32), slots: usize) -> Option<Vec<Option<Stack>>> {
    let structure = marks().lock().remove(&pos)?;
    if !looted().lock().insert(pos) {
        return None;
    }
    if let Some(path) = looted_file() {
        use std::io::Write;
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&path);
        if let Err(e) = file.and_then(|mut f| writeln!(f, "{} {} {}", pos.0, pos.1, pos.2)) {
            println!("couldn't save looted chest to {}: {e}", path.display());
        }
    }
    let seed = (pos.0 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (pos.1 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ (pos.2 as u64) ^ now_nanos();
    Some(roll(table(structure)?, slots, seed))
}

fn now_nanos() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64)
}

/// Rolls every pool and scatters the items over random free slots, like vanilla.
fn roll(pools: &[Pool], slots: usize, seed: u64) -> Vec<Option<Stack>> {
    let mut state = seed | 1;
    let mut next = move |n: u32| -> u32 {
        // xorshift64*
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        ((state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) % n.max(1) as u64) as u32
    };
    let mut out = vec![None; slots];
    for pool in pools {
        let rolls = pool.rolls.0 + next(pool.rolls.1 - pool.rolls.0 + 1);
        let total: u32 = pool.entries.iter().map(|e| e.1).sum();
        for _ in 0..rolls {
            let mut pick = next(total);
            let Some(&(name, _, lo, hi)) = pool.entries.iter().find(|e| {
                let hit = pick < e.1;
                pick = pick.saturating_sub(e.1);
                hit
            }) else { continue };
            let count = lo + next((hi - lo + 1) as u32) as i8;
            let Some(item) = crate::items::item_by_name(name) else { continue };
            let free: Vec<usize> = (0..slots).filter(|&i| out[i].is_none()).collect();
            if free.is_empty() {
                break;
            }
            out[free[next(free.len() as u32) as usize]] = Some(Stack { item, count, net_id: 0 });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagged_chests_fill_once_with_real_items() {
        crate::blocks::init();
        for (i, name) in ["normal_desert_pyramid", "normal_mineshaft", "normal_stronghold", "normal_village", "normal_shipwreck", "generic_ruined_portal"].into_iter().enumerate() {
            let pos = (i as i32, 64, 7);
            mark(pos, name);
            let slots = take(pos, 27).expect(name);
            let items = slots.iter().flatten().count();
            assert!(items > 0 && slots.len() == 27, "{name}: {items} items");
            assert!(slots.iter().flatten().all(|s| s.count > 0), "{name}");
            assert!(take(pos, 27).is_none(), "{name} only fills once");
            mark(pos, name); // the chunk regenerated after a restart
            assert!(take(pos, 27).is_none(), "{name} stays looted");
        }
        // Every item named in every table exists in Bedrock's item list.
        let all = [DESERT_PYRAMID, JUNGLE_TEMPLE, MINESHAFT, STRONGHOLD, VILLAGE, IGLOO, SHIPWRECK, OCEAN_RUIN, PILLAGER_OUTPOST, WOODLAND_MANSION, ANCIENT_CITY, TRIAL_CHAMBERS, RUINED_PORTAL, NETHER_FORTRESS, BASTION];
        let missing: Vec<&str> = all.iter().flat_map(|t| t.iter()).flat_map(|p| p.entries).map(|e| e.0).filter(|n| crate::items::item_by_name(n).is_none()).collect();
        assert!(missing.is_empty(), "unknown items: {missing:?}");
        // Structures without chest loot, and player chests, stay empty.
        mark((1, 2, 3), "normal_swamp_hut");
        assert!(take((1, 2, 3), 27).is_none());
    }
}

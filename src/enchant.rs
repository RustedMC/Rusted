//! Enchantments (from PNX Enchantment and subclasses). Stored on items in the NBT
//! `ench` list, see itemdata.rs.

use crate::itemdata::ItemData;

pub const PROTECTION: i16 = 0;
pub const FIRE_PROTECTION: i16 = 1;
pub const FEATHER_FALLING: i16 = 2;
pub const BLAST_PROTECTION: i16 = 3;
pub const PROJECTILE_PROTECTION: i16 = 4;
pub const SHARPNESS: i16 = 9;
pub const SMITE: i16 = 10;
pub const BANE_OF_ARTHROPODS: i16 = 11;
pub const KNOCKBACK: i16 = 12;
pub const FIRE_ASPECT: i16 = 13;
pub const LOOTING: i16 = 14;
pub const SILK_TOUCH: i16 = 16;
pub const UNBREAKING: i16 = 17;
pub const FORTUNE: i16 = 18;

/// (name, id, max level, applies to)
const ALL: [(&str, i16, i16, Slot); 41] = [
    ("protection", 0, 4, Slot::Armor),
    ("fire_protection", 1, 4, Slot::Armor),
    ("feather_falling", 2, 4, Slot::Boots),
    ("blast_protection", 3, 4, Slot::Armor),
    ("projectile_protection", 4, 4, Slot::Armor),
    ("thorns", 5, 3, Slot::Armor),
    ("respiration", 6, 3, Slot::Helmet),
    ("depth_strider", 7, 3, Slot::Boots),
    ("aqua_affinity", 8, 1, Slot::Helmet),
    ("sharpness", 9, 5, Slot::Melee),
    ("smite", 10, 5, Slot::MeleeOrMace),
    ("bane_of_arthropods", 11, 5, Slot::MeleeOrMace),
    ("knockback", 12, 2, Slot::Sword),
    ("fire_aspect", 13, 2, Slot::Sword),
    ("looting", 14, 3, Slot::Sword),
    ("efficiency", 15, 5, Slot::Digger),
    ("silk_touch", 16, 1, Slot::Digger),
    ("unbreaking", 17, 3, Slot::Durable),
    ("fortune", 18, 3, Slot::Digger),
    ("power", 19, 5, Slot::Bow),
    ("punch", 20, 2, Slot::Bow),
    ("flame", 21, 1, Slot::Bow),
    ("infinity", 22, 1, Slot::Bow),
    ("luck_of_the_sea", 23, 3, Slot::Rod),
    ("lure", 24, 3, Slot::Rod),
    ("frost_walker", 25, 2, Slot::Boots),
    ("mending", 26, 1, Slot::Durable),
    ("binding", 27, 1, Slot::Armor),
    ("vanishing", 28, 1, Slot::Durable),
    ("impaling", 29, 5, Slot::Trident),
    ("riptide", 30, 3, Slot::Trident),
    ("loyalty", 31, 3, Slot::Trident),
    ("channeling", 32, 1, Slot::Trident),
    ("multishot", 33, 1, Slot::Crossbow),
    ("piercing", 34, 4, Slot::Crossbow),
    ("quick_charge", 35, 3, Slot::Crossbow),
    ("soul_speed", 36, 3, Slot::Boots),
    ("swift_sneak", 37, 3, Slot::Leggings),
    ("wind_burst", 38, 3, Slot::Mace),
    ("density", 39, 5, Slot::Mace),
    ("breach", 40, 4, Slot::Mace),
];

#[derive(Clone, Copy)]
enum Slot {
    Armor,
    Helmet,
    Leggings,
    Boots,
    Melee,
    MeleeOrMace,
    Sword,
    Digger,
    Durable,
    Bow,
    Rod,
    Trident,
    Crossbow,
    Mace,
}

fn fits(slot: Slot, item: &str) -> bool {
    let n = item.trim_start_matches("minecraft:");
    let armor = ["_helmet", "_chestplate", "_leggings", "_boots"].iter().any(|s| n.ends_with(s));
    let digger = ["_pickaxe", "_axe", "_shovel", "_hoe"].iter().any(|s| n.ends_with(s));
    match slot {
        Slot::Armor => armor,
        Slot::Helmet => n.ends_with("_helmet"),
        Slot::Leggings => n.ends_with("_leggings"),
        Slot::Boots => n.ends_with("_boots"),
        Slot::Melee => n.ends_with("_sword") || n.ends_with("_axe"),
        Slot::MeleeOrMace => n.ends_with("_sword") || n.ends_with("_axe") || n == "mace",
        Slot::Sword => n.ends_with("_sword"),
        Slot::Digger => digger || n == "shears",
        Slot::Durable => crate::items::max_durability(item).is_some(),
        Slot::Bow => n == "bow",
        Slot::Rod => n == "fishing_rod",
        Slot::Trident => n == "trident",
        Slot::Crossbow => n == "crossbow",
        Slot::Mace => n == "mace",
    }
}

pub fn max_level(id: i16) -> i16 {
    ALL.iter().find(|e| e.1 == id).map_or(1, |e| e.2)
}

/// What an enchanting table can roll. No treasure ones (frost walker, mending, curses,
/// soul speed, swift sneak, wind burst).
pub fn table_pool(item: &str) -> Vec<(i16, i16)> {
    let book = item == "minecraft:book";
    ALL.iter()
        .filter(|e| !matches!(e.1, 25..=28 | 36..=38))
        .filter(|e| book || fits(e.3, item))
        .filter(|_e| book || item != "minecraft:enchanted_book")
        .map(|e| (e.1, e.2))
        .collect()
}

/// Look up by name or id, gives (id, max level).
pub fn by_name(s: &str) -> Option<(i16, i16)> {
    let s = s.trim_start_matches("minecraft:");
    ALL.iter().find(|e| e.0 == s || s.parse::<i16>().ok() == Some(e.1)).map(|e| (e.1, e.2))
}

/// Books take anything.
pub fn applies(id: i16, item: &str) -> bool {
    item == "minecraft:book" || item == "minecraft:enchanted_book" || ALL.iter().any(|e| e.1 == id && fits(e.3, item))
}

/// 0 if not there.
pub fn level(data: &[u8], id: i16) -> i16 {
    ItemData::read(data).enchants().iter().find(|e| e.0 == id).map_or(0, |e| e.1)
}

/// Bedrock numbers: sharpness 1.25 per level, smite/bane 2.5 vs their mobs.
pub fn melee_bonus(data: &[u8], undead: bool, arthropod: bool) -> f32 {
    let e = ItemData::read(data).enchants();
    let lvl = |id| e.iter().find(|x| x.0 == id).map_or(0.0, |x| x.1 as f32);
    1.25 * lvl(SHARPNESS) + if undead { 2.5 * lvl(SMITE) } else { 0.0 } + if arthropod { 2.5 * lvl(BANE_OF_ARTHROPODS) } else { 0.0 }
}

/// Unbreaking roll (PNX). Tools wear 1/(lvl+1) of the time, armor 60% + 40%/(lvl+1).
/// `roll` is 0..1.
pub fn wears(lvl: i16, armor: bool, roll: f32) -> bool {
    if lvl <= 0 {
        return true;
    }
    let chance = if armor { 0.6 + 0.4 / (lvl as f32 + 1.0) } else { 1.0 / (lvl as f32 + 1.0) };
    roll < chance
}

#[derive(Clone, Copy, PartialEq)]
pub enum Hit {
    Other,
    #[allow(dead_code)] // TODO: lava and fire damage don't go through protection yet
    Fire,
    Explosion,
    Projectile,
    Fall,
}

/// Protection points from armor (PNX getProtectionFactor). Protection gives 1 per level,
/// the specific ones 2 (feather falling 3). 4% each, capped at 20 points.
pub fn protection(armor: &[&[u8]], hit: Hit) -> f32 {
    let mut epf = 0i32;
    for data in armor {
        for (id, lvl) in ItemData::read(data).enchants() {
            let lvl = lvl as i32;
            epf += match (id, hit) {
                (PROTECTION, _) => lvl,
                (FIRE_PROTECTION, Hit::Fire) | (BLAST_PROTECTION, Hit::Explosion) | (PROJECTILE_PROTECTION, Hit::Projectile) => 2 * lvl,
                (FEATHER_FALLING, Hit::Fall) => 3 * lvl,
                _ => 0,
            };
        }
    }
    1.0 - epf.clamp(0, 20) as f32 * 0.04
}

/// PNX fortune: count * (max(0, rand(lvl + 2) - 1) + 1)
pub fn fortune(lvl: i16, roll: u32) -> i8 {
    if lvl <= 0 {
        return 1;
    }
    (roll % (lvl as u32 + 2)).saturating_sub(1) as i8 + 1
}

/// Queued /enchant: (player id, enchantment, level).
pub static QUEUE: std::sync::Mutex<Vec<(i64, i16, i16)>> = std::sync::Mutex::new(Vec::new());

pub fn add(data: &[u8], id: i16, lvl: i16) -> Vec<u8> {
    let mut d = ItemData::read(data);
    let mut e = d.enchants();
    match e.iter_mut().find(|x| x.0 == id) {
        Some(x) => x.1 = lvl,
        None => e.push((id, lvl)),
    }
    d.set_enchants(&e);
    d.write()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_and_numbers() {
        assert_eq!(by_name("sharpness"), Some((9, 5)));
        assert!(applies(SHARPNESS, "minecraft:diamond_sword") && !applies(SHARPNESS, "minecraft:diamond_boots"));
        assert!(applies(FEATHER_FALLING, "minecraft:iron_boots") && !applies(FEATHER_FALLING, "minecraft:iron_helmet"));
        let d = add(&[0; 10], SHARPNESS, 5);
        assert_eq!(level(&d, SHARPNESS), 5);
        assert_eq!(melee_bonus(&d, false, false), 6.25);
        let boots = add(&[0; 10], FEATHER_FALLING, 4);
        assert!((protection(&[&boots], Hit::Fall) - 0.52).abs() < 1e-6, "12 points: 48% off");
        assert_eq!(protection(&[&boots], Hit::Fire), 1.0);
        assert!(wears(0, false, 0.99) && !wears(3, false, 0.5));
        assert_eq!(fortune(3, 4), 4);
    }
}

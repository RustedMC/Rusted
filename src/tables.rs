//! Anvil, grindstone and enchanting table (from PNX AnvilInventory, GrindstoneInventory,
//! EnchantInventory). Items in, result and cost out.

use bedrock::protocol::v2168::types::NetworkItemStackDescriptorV2 as Item;

use crate::itemdata::{ItemData, Tag};
use crate::items::{max_durability, name_of};

fn name(i: &Item) -> &'static str {
    name_of(i.id).unwrap_or("")
}

/// Repair material for an anvil.
fn repair_material(item: &str) -> Option<&'static [&'static str]> {
    let n = item.trim_start_matches("minecraft:");
    Some(match n {
        n if n.starts_with("wooden_") => &["minecraft:oak_planks", "minecraft:spruce_planks", "minecraft:birch_planks", "minecraft:jungle_planks", "minecraft:acacia_planks", "minecraft:dark_oak_planks", "minecraft:mangrove_planks", "minecraft:cherry_planks", "minecraft:bamboo_planks", "minecraft:crimson_planks", "minecraft:warped_planks", "minecraft:pale_oak_planks"],
        n if n.starts_with("stone_") => &["minecraft:cobblestone", "minecraft:cobbled_deepslate", "minecraft:blackstone"],
        n if n.starts_with("iron_") || n.starts_with("chainmail_") => &["minecraft:iron_ingot"],
        n if n.starts_with("golden_") => &["minecraft:gold_ingot"],
        n if n.starts_with("diamond_") => &["minecraft:diamond"],
        n if n.starts_with("netherite_") => &["minecraft:netherite_ingot"],
        n if n.starts_with("leather_") => &["minecraft:leather"],
        "turtle_helmet" => &["minecraft:turtle_scute", "minecraft:scute"],
        "elytra" => &["minecraft:phantom_membrane"],
        "mace" => &["minecraft:breeze_rod"],
        "shield" => &["minecraft:oak_planks", "minecraft:spruce_planks", "minecraft:birch_planks"],
        _ => return None,
    })
}

fn damage(d: &ItemData) -> i32 {
    d.get("Damage").and_then(Tag::int).unwrap_or(0)
}

fn repair_cost(d: &ItemData) -> i32 {
    d.get("RepairCost").and_then(Tag::int).unwrap_or(0)
}

fn custom_name(d: &ItemData) -> Option<String> {
    match d.get("display") {
        Some(Tag::Compound(c)) => c.iter().find(|(k, _)| k == "Name").and_then(|(_, v)| match v {
            Tag::String(s) => Some(s.clone()),
            _ => None,
        }),
        _ => None,
    }
}

fn set_name(d: &mut ItemData, n: Option<&str>) {
    match n {
        Some(n) if !n.is_empty() => d.set("display", Tag::Compound(vec![("Name".into(), Tag::String(n.into()))])),
        _ => d.nbt.retain(|(k, _)| k != "display"),
    }
}

/// (item, material used, level cost)
pub fn anvil(target: &Item, material: Option<(&Item, i8)>, rename: Option<&str>) -> Option<(Item, i8, i32)> {
    let tname = name(target);
    let mut out = target.clone();
    let mut data = ItemData::read(&target.user_data_buffer);
    let mut cost = 0;
    let mut used = 0;
    let max = max_durability(tname);
    if let Some((m, count)) = material {
        let mname = name(m);
        let mdata = ItemData::read(&m.user_data_buffer);
        let is_book = mname == "minecraft:enchanted_book";
        if let (Some(max), Some(mats)) = (max, repair_material(tname))
            && mats.contains(&mname)
        {
            // each material repairs a quarter
            let mut d = damage(&data);
            if d == 0 {
                return None;
            }
            while d > 0 && used < count {
                d = (d - max / 4).max(0);
                used += 1;
                cost += 1;
            }
            data.set("Damage", Tag::Int(d));
        } else if mname == tname || is_book {
            if let (Some(max), false) = (max, is_book) {
                // two of the same: durability adds up plus 12%
                let left = (max - damage(&data)) + (max - damage(&mdata)) + max * 12 / 100;
                data.set("Damage", Tag::Int((max - left).max(0)));
                cost += 2;
            }
            let mut ench = data.enchants();
            let mut changed = false;
            for (id, lvl) in mdata.enchants() {
                if !crate::enchant::applies(id, tname) {
                    continue;
                }
                let max_lvl = crate::enchant::max_level(id);
                let new = match ench.iter_mut().find(|e| e.0 == id) {
                    Some(e) => {
                        let v = if e.1 == lvl { (lvl + 1).min(max_lvl) } else { e.1.max(lvl) };
                        let up = v != e.1;
                        e.1 = v;
                        up.then_some(v)
                    }
                    None => {
                        ench.push((id, lvl));
                        Some(lvl)
                    }
                };
                if let Some(v) = new {
                    changed = true;
                    cost += v as i32 * if is_book { 1 } else { 2 };
                }
            }
            if !changed && is_book {
                return None;
            }
            data.set_enchants(&ench);
            used = 1;
        } else {
            return None;
        }
        cost += repair_cost(&mdata);
    }
    if let Some(n) = rename
        && custom_name(&data).as_deref() != Some(n)
    {
        set_name(&mut data, Some(n));
        cost += 1;
    }
    if cost == 0 {
        return None;
    }
    let prior = repair_cost(&data);
    cost += prior;
    data.set("RepairCost", Tag::Int(prior * 2 + 1));
    out.user_data_buffer = data.write();
    Some((out, used, cost))
}

/// (item with enchants stripped, XP back). Two of the same also merge durability +5%.
pub fn grindstone(a: Option<&Item>, b: Option<&Item>) -> Option<(Item, i32)> {
    let (base, other) = match (a, b) {
        (Some(a), b) => (a, b),
        (None, Some(b)) => (b, None),
        (None, None) => return None,
    };
    if let Some(o) = other
        && o.id != base.id
    {
        return None;
    }
    let mut data = ItemData::read(&base.user_data_buffer);
    let mut xp = 0;
    let mut keep = Vec::new();
    for item in [Some(base), other].into_iter().flatten() {
        for (id, lvl) in ItemData::read(&item.user_data_buffer).enchants() {
            if matches!(id, 27 | 28) {
                keep.push((id, lvl)); // curses stay
            } else {
                xp += lvl as i32 * 2;
            }
        }
    }
    if let (Some(o), Some(max)) = (other, max_durability(name(base))) {
        let od = ItemData::read(&o.user_data_buffer);
        let left = (max - damage(&data)) + (max - damage(&od)) + max * 5 / 100;
        data.set("Damage", Tag::Int((max - left).max(0)));
    } else if other.is_none() && xp == 0 {
        return None;
    }
    keep.dedup();
    if keep.is_empty() {
        data.nbt.retain(|(k, _)| k != "ench");
    } else {
        data.set_enchants(&keep);
    }
    data.nbt.retain(|(k, _)| k != "RepairCost");
    let mut out = base.clone();
    if name(base) == "minecraft:enchanted_book"
        && let Some(book) = crate::items::item_by_name("minecraft:book")
    {
        out = book;
    }
    out.user_data_buffer = data.write();
    Some((out, xp))
}

/// Vanilla enchantability.
fn enchantability(item: &str) -> i32 {
    let n = item.trim_start_matches("minecraft:");
    let armor = ["_helmet", "_chestplate", "_leggings", "_boots"].iter().any(|s| n.ends_with(s));
    match n.split('_').next().unwrap_or("") {
        "leather" => 15,
        "chainmail" => 12,
        "iron" if armor => 9,
        "iron" => 14,
        "golden" if armor => 25,
        "golden" => 22,
        "diamond" => 10,
        "netherite" => 15,
        "wooden" => 15,
        "stone" => 5,
        "turtle" => 9,
        _ => 1,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    /// level required
    pub level: i32,
    /// levels and lapis taken, 1 to 3
    pub cost: i32,
    pub enchants: Vec<(i16, i16)>,
}

fn next(seed: &mut u64) -> u32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    (*seed >> 33) as u32
}

/// The three offers for `shelves` bookshelves (vanilla cost formula). Empty if it
/// can't be enchanted.
pub fn offers(item: &Item, shelves: i32, mut seed: u64) -> Vec<Offer> {
    let n = name(item);
    if !ItemData::read(&item.user_data_buffer).enchants().is_empty() {
        return vec![];
    }
    let pool: Vec<(i16, i16)> = crate::enchant::table_pool(n);
    if pool.is_empty() {
        return vec![];
    }
    let shelves = shelves.clamp(0, 15);
    let base = 1 + (next(&mut seed) % 8) as i32 + shelves / 2 + (next(&mut seed) % (shelves as u32 + 1)) as i32;
    let levels = [(base / 3).max(1), base * 2 / 3 + 1, base.max(shelves * 2)];
    let ability = enchantability(n);
    levels
        .iter()
        .enumerate()
        .map(|(i, &level)| {
            let q = (ability / 4 + 1) as u32;
            let power = level + 1 + (next(&mut seed) % q) as i32 + (next(&mut seed) % q) as i32;
            let mut list = Vec::new();
            let picks = 1 + (power / 15).min(2);
            for _ in 0..picks {
                let (id, max) = pool[next(&mut seed) as usize % pool.len()];
                if list.iter().any(|(e, _)| *e == id) || conflicts(id, &list) {
                    continue;
                }
                let lvl = ((power as f32 / 30.0 * max as f32).round() as i16).clamp(1, max);
                list.push((id, lvl));
            }
            Offer { level, cost: i as i32 + 1, enchants: list }
        })
        .collect()
}

/// PNX Enchantment.isCompatibleWith
fn conflicts(id: i16, with: &[(i16, i16)]) -> bool {
    let group = |e: i16| match e {
        0..=4 => 1, // protections
        9..=11 => 2, // damage
        16 | 18 => 3, // silk touch / fortune
        22 | 26 => 4, // infinity / mending
        _ => 0,
    };
    let g = group(id);
    g != 0 && with.iter().any(|(e, _)| group(*e) == g && !(g == 1 && (*e == 2 || id == 2)))
}

/// Books become enchanted books.
pub fn enchanted(item: &Item, offer: &Offer) -> Item {
    let mut out = item.clone();
    if name(item) == "minecraft:book"
        && let Some(b) = crate::items::item_by_name("minecraft:enchanted_book")
    {
        out = b;
    }
    let mut d = ItemData::read(&item.user_data_buffer);
    d.set_enchants(&offer.enchants);
    out.user_data_buffer = d.write();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(n: &str) -> Item {
        crate::items::item_by_name(n).unwrap()
    }

    #[test]
    fn anvil_repairs_combines_and_renames() {
        let mut pick = item("minecraft:iron_pickaxe");
        crate::items::set_damage(&mut pick, 200);
        let ingot = item("minecraft:iron_ingot");
        let (out, used, cost) = anvil(&pick, Some((&ingot, 5)), None).unwrap();
        assert_eq!((crate::items::damage(&out), used, cost), (0, 4, 4), "4 ingots fix 200 of 250");

        let mut book = item("minecraft:enchanted_book");
        book.user_data_buffer = crate::enchant::add(&book.user_data_buffer, crate::enchant::SHARPNESS, 3);
        let sword = item("minecraft:diamond_sword");
        let (out, _, cost) = anvil(&sword, Some((&book, 1)), Some("Edge")).unwrap();
        assert_eq!(crate::enchant::level(&out.user_data_buffer, crate::enchant::SHARPNESS), 3);
        assert_eq!(cost, 3 + 1);
        assert!(anvil(&sword, Some((&ingot, 1)), None).is_none(), "wrong material");

        let (clean, xp) = grindstone(Some(&out), None).unwrap();
        assert!(ItemData::read(&clean.user_data_buffer).enchants().is_empty() && xp > 0);
    }

    #[test]
    fn enchanting_offers() {
        let o = offers(&item("minecraft:iron_sword"), 15, 42);
        assert_eq!(o.len(), 3);
        assert!(o[2].level >= 30 && o.iter().all(|x| !x.enchants.is_empty()));
        assert!(offers(&item("minecraft:dirt"), 15, 1).is_empty());
    }
}

/// Bedrock dye colours in banner order (0 black ... 15 white).
const DYES: [&str; 16] = [
    "black", "red", "green", "brown", "blue", "purple", "cyan", "light_gray",
    "gray", "pink", "lime", "yellow", "light_blue", "magenta", "orange", "white",
];

/// Patterns that need a banner pattern item in the loom's third slot.
const NEEDS_ITEM: [&str; 8] = ["cre", "sku", "flo", "moj", "glb", "pig", "flw", "gus"];

/// Loom result: `banner` with `pattern` added in the colour of `dye`. At most 6 patterns.
pub fn loom(banner: &Item, dye: &str, pattern: &str, has_pattern_item: bool) -> Option<Item> {
    use crate::itemdata::{ItemData, Tag};
    if crate::items::name_of(banner.id)? != "minecraft:banner" || pattern.is_empty() || pattern.len() > 4 {
        return None;
    }
    if NEEDS_ITEM.contains(&pattern) && !has_pattern_item {
        return None;
    }
    let colour = DYES.iter().position(|d| dye == format!("minecraft:{d}_dye"))? as i32;
    let mut data = ItemData::read(&banner.user_data_buffer);
    let mut patterns = match data.get("Patterns") {
        Some(Tag::List(_, p)) => p.clone(),
        _ => vec![],
    };
    if patterns.len() >= 6 {
        return None;
    }
    patterns.push(Tag::Compound(vec![("Color".into(), Tag::Int(colour)), ("Pattern".into(), Tag::String(pattern.into()))]));
    data.set("Patterns", Tag::List(10, patterns));
    let mut out = banner.clone();
    out.stack_size = 1;
    out.user_data_buffer = data.write();
    Some(out)
}

#[cfg(test)]
mod loom_tests {
    use super::*;
    use crate::itemdata::{ItemData, Tag};

    #[test]
    fn loom_adds_patterns_up_to_six() {
        crate::blocks::init();
        let mut banner = crate::items::item_by_name("minecraft:banner").unwrap();
        for n in 1..=6 {
            banner = loom(&banner, "minecraft:red_dye", "bo", false).expect("pattern added");
            let Some(Tag::List(_, p)) = ItemData::read(&banner.user_data_buffer).get("Patterns").cloned() else { panic!() };
            assert_eq!(p.len(), n);
        }
        assert!(loom(&banner, "minecraft:red_dye", "bo", false).is_none(), "six is the most");
        let plain = crate::items::item_by_name("minecraft:banner").unwrap();
        assert!(loom(&plain, "minecraft:red_dye", "cre", false).is_none(), "creeper needs its pattern item");
        assert!(loom(&plain, "minecraft:red_dye", "cre", true).is_some());
        assert!(loom(&plain, "minecraft:stick", "bo", false).is_none(), "needs a dye");
    }
}

/// The map id a filled map shows.
fn map_id(map: &Item) -> Option<i64> {
    match ItemData::read(&map.user_data_buffer).get("map_uuid") {
        Some(Tag::Long(id)) => Some(*id),
        _ => None,
    }
}

/// Cartography table result and how many (from PNX CraftRecipeOptionalProcessor
/// updateCartographyTableResult). Zooming and locking make new maps, so call it only when the
/// player takes the result.
pub fn cartography(input: &Item, extra: Option<&Item>) -> Option<(Item, i8)> {
    let get = |n: &str| crate::items::item_by_name(n);
    match (name(input), extra.map(name)) {
        ("minecraft:paper", None) => Some((get("minecraft:empty_map")?, 1)),
        ("minecraft:empty_map" | "minecraft:filled_map", None) => Some((input.clone(), 1)),
        // A compass makes a locator map.
        ("minecraft:paper" | "minecraft:empty_map", Some("minecraft:compass")) => {
            let mut map = get("minecraft:empty_map")?;
            map.aux_value = 2;
            Some((map, 1))
        }
        ("minecraft:filled_map", Some("minecraft:empty_map")) => Some((input.clone(), 2)),
        ("minecraft:filled_map", Some("minecraft:paper")) => Some((crate::maps::filled_map(crate::maps::zoom_out(map_id(input)?)?)?, 1)),
        ("minecraft:filled_map", Some("minecraft:glass_pane")) => Some((crate::maps::filled_map(crate::maps::lock(map_id(input)?)?)?, 1)),
        _ => None,
    }
}

#[cfg(test)]
mod cartography_tests {
    use super::*;

    #[test]
    fn paper_and_copies() {
        let item = |n: &str| crate::items::item_by_name(n).unwrap();
        let (m, n) = cartography(&item("minecraft:paper"), None).expect("paper makes an empty map");
        assert_eq!((name(&m), n), ("minecraft:empty_map", 1));
        let filled = item("minecraft:filled_map");
        let (copy, n) = cartography(&filled, Some(&item("minecraft:empty_map"))).expect("copy");
        assert_eq!((name(&copy), n), ("minecraft:filled_map", 2));
        assert!(cartography(&item("minecraft:stick"), None).is_none());
        assert!(cartography(&filled, Some(&item("minecraft:stick"))).is_none());
        // A map with no id can't be zoomed or locked.
        assert!(cartography(&filled, Some(&item("minecraft:paper"))).is_none());
    }
}

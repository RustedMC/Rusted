//! Item registry and creative inventory from kaooot/bedrock-network-data (1.26.60, via PNX).
//! Same for every player, so we build the packets once.

use std::sync::OnceLock;

use base64::Engine;
use bedrock::protocol::V2225;
use bedrock::protocol::v2168::packets::{
    CreativeContentPacket, CreativeItemCategory, CreativeItemData, CreativeItemGroup,
};
use bedrock::protocol::v2168::types::{NetworkItemInstanceDescriptor, NetworkItemStackDescriptorV2};
use bedrock::protocol::v776::enums::ItemVersion;
use bedrock::protocol::v776::packets::{ItemComponentPacket, ItemsEntry};
use serde_json::Value as Json;

/// Empty extra data (no NBT, can-place-on or can-destroy).
const EMPTY_USER_DATA: [u8; 10] = [0; 10];

fn compound_get<'a>(v: &'a nbtx::Value, key: &str) -> Option<&'a nbtx::Value> {
    match v {
        nbtx::Value::Compound(map) => map.iter().find(|(k, _)| k.as_slice() == key.as_bytes()).map(|(_, v)| v),
        _ => None,
    }
}

fn empty_compound() -> nbtx::Value {
    nbtx::Value::Compound(Default::default())
}

/// Items in the 1.26.60 data that the game doesn't have finished yet.
fn unreleased(name: &str) -> bool {
    let n = name.trim_start_matches("minecraft:");
    ["_wool_slab", "_wool_double_slab", "_wool_stairs", "_concrete_slab", "_concrete_double_slab", "_concrete_stairs"].iter().any(|s| n.ends_with(s))
}

/// Pulls the block id out of a creative entry's base64 block-state NBT.
fn block_network_id(b64: Option<&str>) -> i32 {
    let Some(b64) = b64 else { return 0 };
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) else { return 0 };
    let Ok(state) = nbtx::from_bytes::<nbtx::LittleEndian, nbtx::Value>(&mut &bytes[..]) else { return 0 };
    match compound_get(&state, "network_id") {
        Some(nbtx::Value::Int(id)) => *id,
        _ => 0,
    }
}

struct Data {
    registry: ItemComponentPacket<V2225>,
    creative: CreativeContentPacket<V2225>,
}

fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(|| {
        let palette: Json = serde_json::from_str(include_str!("../data/item_palette.json")).unwrap();
        let palette = palette["items"].as_array().unwrap();

        let components = {
            let gz = include_bytes!("../data/item_components.nbt");
            let mut raw = flate2::read::GzDecoder::new(&gz[..]);
            nbtx::from_bytes::<nbtx::BigEndian, nbtx::Value>(&mut raw).expect("item_components.nbt")
        };

        let mut ids = std::collections::HashMap::new();
        let items = palette
            .iter()
            .map(|i| {
                let name = i["name"].as_str().unwrap().to_string();
                let id = i["id"].as_i64().unwrap() as i16;
                ids.insert(name.clone(), id);
                let component_based = i["component_based"].as_bool().unwrap_or(false);
                ItemsEntry {
                    component_data: if component_based {
                        compound_get(&components, &name).cloned().unwrap_or_else(empty_compound)
                    } else {
                        empty_compound()
                    },
                    component_item_name: name,
                    runtime_id: id,
                    is_component_based: component_based,
                    version: match i["version"].as_i64() {
                        Some(0) => ItemVersion::Legacy,
                        Some(1) => ItemVersion::DataDriven,
                        _ => ItemVersion::None,
                    },
                }
            })
            .collect();

        let creative: Json = serde_json::from_str(include_str!("../data/creative_items.json")).unwrap();
        let instance = |item: &Json| -> Option<NetworkItemInstanceDescriptor> {
            Some(NetworkItemInstanceDescriptor {
                id: *ids.get(item["id"].as_str()?)? as i32,
                stack_size: 1,
                aux_value: item["damage"].as_u64().unwrap_or(0) as u32,
                block_runtime_id: block_network_id(item["block_state_b64"].as_str()),
                // Banners, fireworks, enchanted books: the entry's little-endian NBT behind the
                // 0xffff "has NBT" marker and version 1, then empty can-place/can-break lists.
                user_data_buffer: match item["nbt_b64"].as_str().and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok()) {
                    Some(nbt) => [&[0xff, 0xff, 1][..], &nbt, &[0; 8]].concat(),
                    None => EMPTY_USER_DATA.to_vec(),
                },
            })
        };
        let groups = creative["groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| CreativeItemGroup {
                category: match g["creative_category"].as_u64() {
                    Some(1) => CreativeItemCategory::Construction,
                    Some(2) => CreativeItemCategory::Nature,
                    Some(3) => CreativeItemCategory::Equipment,
                    Some(4) => CreativeItemCategory::Items,
                    _ => CreativeItemCategory::Undefined,
                },
                name: g["name"].as_str().unwrap_or_default().to_string(),
                icon: instance(&g["icon"]).unwrap_or(NetworkItemInstanceDescriptor {
                    id: 0,
                    stack_size: 0,
                    aux_value: 0,
                    block_runtime_id: 0,
                    user_data_buffer: EMPTY_USER_DATA.to_vec(),
                }),
            })
            .collect();
        let contents = creative["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|i| Some((instance(i)?, i["group_index"].as_u64()? as u32)))
            // Wool and concrete slabs/stairs are in the data but not released in the game yet:
            // the client shows them untextured with raw names, so they stay out of the menu.
            .filter(|(inst, _)| !ids.iter().any(|(n, &id)| id as i32 == inst.id && unreleased(n)))
            .enumerate()
            .map(|(n, (item_instance, group_id))| CreativeItemData {
                creative_net_id: n as u32 + 1,
                item_instance,
                group_id,
            })
            .collect();

        Data {
            registry: ItemComponentPacket { items },
            creative: CreativeContentPacket { groups, contents },
        }
    })
}

/// Our item data has no stack sizes, so guess from the name like vanilla
/// (tools/armor/potions 1, pearls/signs/buckets 16).
fn stack_size_for(name: &str) -> i8 {
    let n = name.trim_start_matches("minecraft:");
    const ONE_SUFFIX: [&str; 16] = [
        "_sword", "_pickaxe", "_axe", "_shovel", "_hoe", "_helmet", "_chestplate", "_leggings",
        "_boots", "_boat", "_chest_boat", "_minecart", "_bucket", "_bed", "_horse_armor", "_spear",
    ];
    const ONE: [&str; 24] = [
        "potion", "splash_potion", "lingering_potion", "bow", "crossbow", "trident", "shield",
        "elytra", "totem_of_undying", "saddle", "enchanted_book", "written_book", "writable_book",
        "fishing_rod", "carrot_on_a_stick", "warped_fungus_on_a_stick", "flint_and_steel", "shears",
        "mace", "brush", "spyglass", "minecart", "cake", "music_disc_13",
    ];
    const SIXTEEN: [&str; 9] = [
        "ender_pearl", "snowball", "egg", "bucket", "armor_stand", "honey_bottle", "blue_egg",
        "brown_egg", "wind_charge",
    ];
    if ONE.contains(&n) || n.starts_with("music_disc_") || n.ends_with("_shulker_box") || n == "shulker_box"
        || ONE_SUFFIX.iter().any(|s| n.ends_with(s))
    {
        1
    } else if SIXTEEN.contains(&n) || n.ends_with("_sign") || n.ends_with("_hanging_sign") || n.ends_with("_banner") || n == "banner" {
        16
    } else {
        64
    }
}

/// Bedrock durability values (PNX Item.getMaxDurability).
pub fn max_durability(name: &str) -> Option<i32> {
    let n = name.trim_start_matches("minecraft:");
    let tier = |t: &str| match t {
        "wooden" => Some(59),
        "stone" => Some(131),
        "iron" => Some(250),
        "golden" => Some(32),
        "diamond" => Some(1561),
        "netherite" => Some(2031),
        _ => None,
    };
    for tool in ["_sword", "_pickaxe", "_axe", "_shovel", "_hoe"] {
        if let Some(t) = n.strip_suffix(tool) {
            return tier(t);
        }
    }
    // armor is (helmet, chestplate, leggings, boots)
    let armor = |t: &str| match t {
        "leather" => Some([55, 80, 75, 65]),
        "chainmail" | "iron" => Some([165, 240, 225, 195]),
        "golden" => Some([77, 112, 105, 91]),
        "diamond" => Some([363, 528, 495, 429]),
        "netherite" => Some([407, 592, 555, 481]),
        _ => None,
    };
    for (i, piece) in ["_helmet", "_chestplate", "_leggings", "_boots"].iter().enumerate() {
        if let Some(t) = n.strip_suffix(piece) {
            return if t == "turtle" && i == 0 { Some(275) } else { armor(t).map(|a| a[i]) };
        }
    }
    Some(match n {
        "bow" | "fishing_rod" => 384,
        "crossbow" => 464,
        "shears" => 238,
        "flint_and_steel" | "brush" => 64,
        "trident" => 250,
        "shield" => 336,
        "elytra" => 432,
        "carrot_on_a_stick" => 25,
        "warped_fungus_on_a_stick" => 100,
        "mace" => 500,
        _ => return None,
    })
}

/// The NBT `Damage` value.
pub fn damage(item: &NetworkItemStackDescriptorV2) -> i32 {
    crate::itemdata::ItemData::read(&item.user_data_buffer).get("Damage").and_then(crate::itemdata::Tag::int).unwrap_or(0)
}

/// Keeps the rest of the NBT intact.
pub fn set_damage(item: &mut NetworkItemStackDescriptorV2, d: i32) {
    let mut data = crate::itemdata::ItemData::read(&item.user_data_buffer);
    data.set("Damage", crate::itemdata::Tag::Int(d));
    item.user_data_buffer = data.write();
}

/// Shulker box item with `slots` inside (NBT `Items` with Name, Count, Damage, Slot).
pub fn with_contents(mut item: NetworkItemStackDescriptorV2, slots: &[Option<crate::inventory::Stack>]) -> NetworkItemStackDescriptorV2 {
    use crate::itemdata::{ItemData, Tag};
    let list: Vec<Tag> = slots
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.as_ref().map(|s| (i, s)))
        .map(|(i, s)| {
            let mut c = vec![
                ("Count".to_string(), Tag::Byte(s.count)),
                ("Damage".to_string(), Tag::Short(s.item.aux_value as i16)),
                ("Name".to_string(), Tag::String(name_of(s.item.id).unwrap_or("minecraft:air").to_string())),
                ("Slot".to_string(), Tag::Byte(i as i8)),
                ("WasPickedUp".to_string(), Tag::Byte(0)),
            ];
            let inner = ItemData::read(&s.item.user_data_buffer);
            if !inner.nbt.is_empty() {
                c.push(("tag".to_string(), Tag::Compound(inner.nbt.clone())));
            }
            Tag::Compound(c)
        })
        .collect();
    if list.is_empty() {
        return item;
    }
    let mut d = ItemData::read(&item.user_data_buffer);
    d.set("Items", Tag::List(10, list));
    item.user_data_buffer = d.write();
    item
}

pub fn contents(item: &NetworkItemStackDescriptorV2) -> Vec<Option<crate::inventory::Stack>> {
    use crate::itemdata::{ItemData, Tag};
    let mut out = vec![None; crate::inventory::CHEST_SLOTS];
    let d = ItemData::read(&item.user_data_buffer);
    let Some(Tag::List(_, list)) = d.get("Items") else { return out };
    for t in list {
        let Tag::Compound(c) = t else { continue };
        let f = |k: &str| c.iter().find(|(key, _)| key == k).map(|(_, v)| v);
        let (Some(Tag::String(n)), Some(count), Some(slot)) = (f("Name"), f("Count").and_then(Tag::int), f("Slot").and_then(Tag::int)) else { continue };
        let Some(mut it) = item_by_name(n) else { continue };
        it.aux_value = f("Damage").and_then(Tag::int).unwrap_or(0).max(0) as u32;
        if let Some(Tag::Compound(tag)) = f("tag") {
            let mut inner = ItemData::read(&it.user_data_buffer);
            inner.nbt = tag.clone();
            it.user_data_buffer = inner.write();
        }
        if let Some(s) = out.get_mut(slot as usize) {
            *s = Some(crate::inventory::Stack { item: it, count: count as i8, net_id: 0 });
        }
    }
    out
}

/// 64 if we don't know the item.
pub fn max_stack(id: i16) -> i8 {
    static SIZES: OnceLock<std::collections::HashMap<i16, i8>> = OnceLock::new();
    SIZES
        .get_or_init(|| data().registry.items.iter().map(|i| (i.runtime_id, stack_size_for(&i.component_item_name))).collect())
        .get(&id)
        .copied()
        .unwrap_or(64)
}

/// Lookup for /give, with or without "minecraft:". Block items get their block from the
/// creative entry.
pub fn item_by_name(name: &str) -> Option<NetworkItemStackDescriptorV2> {
    let full = if name.contains(':') { name.to_string() } else { format!("minecraft:{name}") };
    let id = data().registry.items.iter().find(|i| i.component_item_name == full)?.runtime_id;
    let block = data()
        .creative
        .contents
        .iter()
        .find(|c| c.item_instance.id == id as i32 && c.item_instance.aux_value == 0)
        .map_or(0, |c| c.item_instance.block_runtime_id);
    Some(NetworkItemStackDescriptorV2 {
        id,
        stack_size: 1,
        aux_value: 0,
        net_id: None,
        block_runtime_id: block as u32,
        user_data_buffer: EMPTY_USER_DATA.to_vec(),
    })
}

pub fn name_of(id: i16) -> Option<&'static str> {
    data().registry.items.iter().find(|i| i.runtime_id == id).map(|i| i.component_item_name.as_str())
}

/// Names without "minecraft:", for autocomplete.
pub fn names() -> Vec<String> {
    data().registry.items.iter().map(|i| i.component_item_name.trim_start_matches("minecraft:").to_string()).collect()
}

/// What you get when taking a creative menu entry.
pub fn creative_item(creative_net_id: u32) -> Option<NetworkItemStackDescriptorV2> {
    let c = data().creative.contents.iter().find(|c| c.creative_net_id == creative_net_id)?;
    let i = &c.item_instance;
    Some(NetworkItemStackDescriptorV2 {
        id: i.id as i16,
        stack_size: 1,
        aux_value: i.aux_value,
        net_id: None,
        block_runtime_id: i.block_runtime_id as u32,
        user_data_buffer: i.user_data_buffer.clone(),
    })
}

/// Pick block result as an inventory stack.
pub fn item_for_block(block: i32) -> Option<NetworkItemStackDescriptorV2> {
    let name = crate::blocks::entry(block).map(|e| e.name.as_str()).unwrap_or("");
    // Exact state first, then any state of the same block (powered dust, rotated repeaters,
    // connected fences...), then blocks whose item has a different name.
    let c = data().creative.contents.iter().find(|c| c.item_instance.block_runtime_id == block).or_else(|| {
        data().creative.contents.iter().find(|c| c.item_instance.block_runtime_id != 0 && crate::blocks::entry(c.item_instance.block_runtime_id).is_some_and(|e| e.name == name))
    });
    let Some(c) = c else {
        let short = name.trim_start_matches("minecraft:");
        let item = match short {
            "redstone_wire" => "redstone",
            "powered_repeater" | "unpowered_repeater" => "repeater",
            "powered_comparator" | "unpowered_comparator" => "comparator",
            "unlit_redstone_torch" => "redstone_torch",
            "lit_redstone_lamp" => "redstone_lamp",
            "lit_furnace" => "furnace",
            "lit_smoker" => "smoker",
            "lit_blast_furnace" => "blast_furnace",
            "lit_redstone_ore" => "redstone_ore",
            "lit_deepslate_redstone_ore" => "deepslate_redstone_ore",
            "daylight_detector_inverted" => "daylight_detector",
            "piston_arm_collision" => "piston",
            "sticky_piston_arm_collision" => "sticky_piston",
            "tripwire" | "trip_wire" => "string",
            "wheat" => "wheat_seeds",
            "carrots" => "carrot",
            "potatoes" => "potato",
            "beetroot" => "beetroot_seeds",
            "reeds" => "sugar_cane",
            "cocoa" => "cocoa_beans",
            "standing_sign" | "wall_sign" => "oak_sign",
            "standing_banner" | "wall_banner" => "banner",
            "powered_rail" | "golden_rail" => "golden_rail",
            "wooden_door" => "oak_door",
            "unlit_torch" => "torch",
            s if s.ends_with("_wall_sign") => return item_by_name(&format!("minecraft:{}_sign", s.trim_end_matches("_wall_sign"))),
            s if s.ends_with("_standing_sign") => return item_by_name(&format!("minecraft:{}_sign", s.trim_end_matches("_standing_sign"))),
            s if s.ends_with("_double_slab") => return item_by_name(&format!("minecraft:{}_slab", s.trim_end_matches("_double_slab"))),
            s if s.ends_with("_door") || s.ends_with("_bed") || s.ends_with("_cauldron") || s == "cauldron" || s == "cake" => short,
            _ => return None,
        };
        return item_by_name(item);
    };
    let i = &c.item_instance;
    Some(NetworkItemStackDescriptorV2 {
        id: i.id as i16,
        stack_size: 64,
        aux_value: i.aux_value,
        net_id: None,
        block_runtime_id: i.block_runtime_id as u32,
        user_data_buffer: EMPTY_USER_DATA.to_vec(),
    })
}

/// Vanilla entity definitions. The client gets player physics and inventory from these,
/// without them you just float.
pub fn actor_identifiers() -> std::collections::HashMap<String, nbtx::Value> {
    let gz = include_bytes!("../data/entity_identifiers.nbt");
    let mut raw = flate2::read::GzDecoder::new(&gz[..]);
    match nbtx::from_bytes::<nbtx::BigEndian, nbtx::Value>(&mut raw).expect("entity_identifiers.nbt") {
        nbtx::Value::Compound(map) => map
            .into_iter()
            .map(|(k, v)| (String::from_utf8_lossy(&k).into_owned(), v))
            .collect(),
        _ => panic!("entity_identifiers.nbt: root is not a compound"),
    }
}

pub fn registry() -> ItemComponentPacket<V2225> {
    data().registry.clone()
}

pub fn creative() -> CreativeContentPacket<V2225> {
    data().creative.clone()
}

#[cfg(test)]
mod tests {

    #[test]
    fn shulker_box_keeps_items() {
        let mut slots = vec![None; crate::inventory::CHEST_SLOTS];
        let mut sword = item_by_name("minecraft:diamond_sword").unwrap();
        sword.user_data_buffer = crate::enchant::add(&sword.user_data_buffer, crate::enchant::SHARPNESS, 3);
        slots[0] = Some(crate::inventory::Stack { item: item_by_name("minecraft:diamond").unwrap(), count: 12, net_id: 0 });
        slots[5] = Some(crate::inventory::Stack { item: sword, count: 1, net_id: 0 });
        let boxed = with_contents(item_by_name("minecraft:undyed_shulker_box").or_else(|| item_by_name("minecraft:shulker_box")).unwrap(), &slots);
        let back = contents(&boxed);
        assert_eq!(back[0].as_ref().map(|s| (name_of(s.item.id), s.count)), Some((Some("minecraft:diamond"), 12)));
        assert_eq!(back[5].as_ref().map(|s| crate::enchant::level(&s.item.user_data_buffer, crate::enchant::SHARPNESS)), Some(3), "enchants kept");
        assert!(back[1].is_none());
    }

    use super::*;

    #[test]
    fn loads_vanilla_items() {
        let reg = registry();
        assert!(reg.items.len() > 2000);
        let grass = reg.items.iter().find(|i| i.component_item_name == "minecraft:grass_block").unwrap();
        let creative = creative();
        assert!(creative.groups.len() > 100);
        assert!(creative.contents.len() > 1500);
        // creative grass should place the same block our world uses
        let entry = creative.contents.iter().find(|c| c.item_instance.id == grass.runtime_id as i32).unwrap();
        assert_eq!(entry.item_instance.block_runtime_id, crate::world::GRASS);
        assert_eq!(item_by_name("grass_block").unwrap().block_runtime_id as i32, crate::world::GRASS);
        assert!(item_by_name("diamond").is_some());
        assert!(item_by_name("not_an_item").is_none());
        let size = |n: &str| max_stack(item_by_name(n).unwrap().id);
        assert_eq!(size("diamond_sword"), 1);
        assert_eq!(size("water_bucket"), 1);
        assert_eq!(size("bucket"), 16);
        assert_eq!(size("ender_pearl"), 16);
        assert_eq!(size("oak_sign"), 16);
        assert_eq!(size("dirt"), 64);
        assert_eq!(item_for_block(crate::world::GRASS).unwrap().id, grass.runtime_id);
        assert!(item_for_block(crate::world::AIR).is_none());
        let ids = actor_identifiers();
        let list = format!("{:?}", ids.get("idlist").unwrap());
        assert!(list.contains("minecraft:player"));
        // every group a creative item points to has to exist
        assert!(creative.contents.iter().all(|c| (c.group_id as usize) < creative.groups.len()));
    }
}



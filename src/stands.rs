//! Armor stands (PNX EntityArmorStand): a mob with no AI that holds armor and an item. Click
//! with armor or an item to put it on, with an empty hand to take something back, hit twice to
//! break it (it drops itself and everything it wore).

use crate::Protocol;
use crate::inventory::Stack;
use bedrock::protocol::v662::types::ActorRuntimeID;
use parking_lot::Mutex;
use std::collections::HashMap;

/// Hand, head, chest, legs, feet.
pub type Gear = [Option<Stack>; 5];

static GEAR: Mutex<Option<HashMap<i64, Gear>>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut HashMap<i64, Gear>) -> T) -> T {
    f(GEAR.lock().get_or_insert_with(HashMap::new))
}

/// Which slot `item` goes in: armor by type, everything else in the hand.
pub fn slot_for(item: &str) -> usize {
    let n = item.trim_start_matches("minecraft:");
    if n.ends_with("_helmet") || n.ends_with("_head") || n.ends_with("_skull") || n == "carved_pumpkin" {
        1
    } else if n.ends_with("_chestplate") || n == "elytra" {
        2
    } else if n.ends_with("_leggings") {
        3
    } else if n.ends_with("_boots") {
        4
    } else {
        0
    }
}

pub fn gear(id: i64) -> Gear {
    with(|m| m.get(&id).cloned()).unwrap_or_default()
}

/// Puts one of `held` on the stand, or (`held` None) takes the hand item, else the highest
/// armor piece. Returns what comes back to the player.
/// shortcut: vanilla takes the piece you click on; we take hand, then head to feet. Needs the click position.
pub fn swap(id: i64, held: Option<Stack>) -> Option<Stack> {
    with(|m| {
        let g = m.entry(id).or_default();
        match held {
            Some(mut h) => {
                h.count = 1;
                let slot = slot_for(crate::items::name_of(h.item.id).unwrap_or(""));
                g[slot].replace(h)
            }
            None => g.iter_mut().find(|s| s.is_some()).and_then(Option::take),
        }
    })
}

/// Everything it wore, for when it breaks.
pub fn take_all(id: i64) -> Vec<Stack> {
    with(|m| m.remove(&id)).map(|g| g.into_iter().flatten().collect()).unwrap_or_default()
}

/// What the stand wears and holds, for everyone who sees it.
pub fn packets(id: i64) -> Vec<Protocol> {
    let g = gear(id);
    if g.iter().all(Option::is_none) {
        return vec![];
    }
    let net = |s: &Option<Stack>| s.as_ref().map_or_else(crate::spawn::air, Stack::to_network);
    vec![
        Protocol::MobArmorEquipmentPacket(Box::new(bedrock::protocol::v1001::packets::MobArmorEquipmentPacket {
            target_runtime_id: ActorRuntimeID(id as u64),
            head: net(&g[1]),
            torso: net(&g[2]),
            legs: net(&g[3]),
            feet: net(&g[4]),
            body: crate::spawn::air(),
        })),
        Protocol::MobEquipmentPacket(Box::new(bedrock::protocol::v975::packets::MobEquipmentPacket {
            target_runtime_id: ActorRuntimeID(id as u64),
            item: net(&g[0]),
            slot: 0,
            selected_slot: 0,
            container_id: bedrock::protocol::v662::enums::ContainerID(0),
        })),
    ]
}

/// The gear as a save token ("gear=<hex>"), or None when it wears nothing.
pub fn save_token(id: i64) -> Option<String> {
    let g = gear(id);
    if g.iter().all(Option::is_none) {
        return None;
    }
    Some(format!("gear={}", crate::inventory::encode_slots(&g).iter().map(|b| format!("{b:02x}")).collect::<String>()))
}

/// Reads a save token back onto stand `id`.
pub fn load_token(id: i64, token: &str) {
    let Some(h) = token.strip_prefix("gear=") else { return };
    let bytes: Option<Vec<u8>> = (0..h.len()).step_by(2).map(|i| h.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok())).collect();
    if let Some((slots, _)) = bytes.and_then(|b| crate::inventory::decode_slots(&b, 5, 5)) {
        let mut g: Gear = Default::default();
        for (i, s) in slots.into_iter().take(5).enumerate() {
            g[i] = s;
        }
        with(|m| m.insert(id, g));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dress_undress_and_save() {
        crate::blocks::init();
        let st = |n: &str| Stack { item: crate::items::item_by_name(n).unwrap(), count: 3, net_id: 0 };
        let id = 987_654;
        assert!(swap(id, Some(st("minecraft:diamond_helmet"))).is_none());
        assert!(swap(id, Some(st("minecraft:iron_sword"))).is_none());
        let back = swap(id, Some(st("minecraft:golden_helmet"))).unwrap();
        assert_eq!(crate::items::name_of(back.item.id), Some("minecraft:diamond_helmet"), "the old helmet comes back");
        assert_eq!(packets(id).len(), 2);
        let token = save_token(id).unwrap();
        take_all(id);
        load_token(id, &token);
        let g = gear(id);
        assert_eq!(g[1].as_ref().and_then(|s| crate::items::name_of(s.item.id)), Some("minecraft:golden_helmet"));
        assert_eq!(g[0].as_ref().map(|s| s.count), Some(1), "one item, not the stack");
        assert_eq!(crate::items::name_of(swap(id, None).unwrap().item.id), Some("minecraft:iron_sword"), "empty hand takes the hand item first");
        assert_eq!(take_all(id).len(), 1);
        assert_eq!(slot_for("minecraft:leather_boots"), 4);
    }
}

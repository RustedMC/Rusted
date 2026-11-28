//! Item frames and glow item frames (PNX BlockItemFrame / BlockEntityItemFrame): click to put
//! the held item in, click again to turn it (8 steps of 45 degrees), hit to take it out.
//! Saved to worlds/<level>/frames.dat, one "x y z rotation <item hex>" line each.

use crate::Protocol;
use crate::inventory::Stack;
use bedrock::protocol::v944::types::NetworkBlockPosition;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

type Pos = (i32, i32, i32);

static FRAMES: Mutex<Option<HashMap<Pos, (Stack, u8)>>> = Mutex::new(None);
static DIRTY: AtomicBool = AtomicBool::new(false);

fn with<T>(f: impl FnOnce(&mut HashMap<Pos, (Stack, u8)>) -> T) -> T {
    f(FRAMES.lock().get_or_insert_with(HashMap::new))
}

pub fn is_frame(block: i32) -> bool {
    crate::blocks::entry(block).is_some_and(|e| e.name == "minecraft:frame" || e.name == "minecraft:glow_frame")
}

/// What's in the frame at `pos`, and its rotation.
pub fn get(pos: Pos) -> Option<(Stack, u8)> {
    with(|m| m.get(&pos).cloned())
}

/// Puts one of `item` in the empty frame at `pos`.
pub fn put(pos: Pos, mut item: Stack) {
    item.count = 1;
    with(|m| m.insert(pos, (item, 0)));
    DIRTY.store(true, Ordering::Relaxed);
}

/// Turns the item one step. Returns the new rotation (0-7).
pub fn rotate(pos: Pos) -> Option<u8> {
    let r = with(|m| m.get_mut(&pos).map(|e| {
        e.1 = (e.1 + 1) % 8;
        e.1
    }));
    DIRTY.store(true, Ordering::Relaxed);
    r
}

/// Takes the item out (hit, or the frame broke).
pub fn take(pos: Pos) -> Option<Stack> {
    let t = with(|m| m.remove(&pos)).map(|e| e.0);
    if t.is_some() {
        DIRTY.store(true, Ordering::Relaxed);
    }
    t
}

/// Block-entity data for the frame at `pos` (`glow` for a glow item frame).
pub fn data(pos: Pos, glow: bool) -> Protocol {
    use nbtx::Value as V;
    let mut tags: HashMap<String, V> = HashMap::new();
    tags.insert("id".into(), V::String(if glow { "GlowItemFrame" } else { "ItemFrame" }.into()));
    tags.insert("x".into(), V::Int(pos.0));
    tags.insert("y".into(), V::Int(pos.1));
    tags.insert("z".into(), V::Int(pos.2));
    tags.insert("isMovable".into(), V::Byte(1));
    if let Some((item, rot)) = get(pos) {
        tags.insert("Item".into(), crate::lectern::item_nbt(&item));
        tags.insert("ItemRotation".into(), V::Float(rot as f32 * 45.0));
        tags.insert("ItemDropChance".into(), V::Float(1.0));
    }
    Protocol::BlockActorDataPacket(Box::new(bedrock::protocol::v662::packets::BlockActorDataPacket {
        block_position: NetworkBlockPosition { x: pos.0, y: pos.1, z: pos.2 },
        actor_data_tags: tags,
    }))
}

/// Frame data for the chunks being sent (`glow_at` tells glow frames apart).
pub fn in_chunks(chunks: &[(i32, i32)], glow_at: impl Fn(Pos) -> bool) -> Vec<Protocol> {
    let at: Vec<Pos> = with(|m| m.keys().filter(|p| chunks.contains(&(p.0 >> 4, p.2 >> 4))).copied().collect());
    at.into_iter().map(|p| data(p, glow_at(p))).collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    (0..s.len()).step_by(2).map(|i| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok())).collect()
}

pub fn save_if_dirty(path: &std::path::Path) -> std::io::Result<()> {
    if !DIRTY.swap(false, Ordering::Relaxed) {
        return Ok(());
    }
    let mut out = String::new();
    for (p, (item, rot)) in with(|m| m.clone()) {
        out += &format!("{} {} {} {rot} {}\n", p.0, p.1, p.2, hex(&crate::inventory::encode_slots(&[Some(item)])));
    }
    let tmp = path.with_extension("tmp");
    let r = std::fs::write(&tmp, out).and_then(|()| std::fs::rename(&tmp, path));
    if r.is_err() {
        DIRTY.store(true, Ordering::Relaxed);
    }
    r
}

/// Missing file = no frames yet; a bad line is skipped (that frame shows empty).
pub fn load(path: &std::path::Path) {
    let Ok(text) = std::fs::read_to_string(path) else { return };
    with(|m| {
        for line in text.lines() {
            let f: Vec<&str> = line.split(' ').collect();
            let (Some(x), Some(y), Some(z), Some(r), Some(item)) = (
                f.first().and_then(|v| v.parse().ok()),
                f.get(1).and_then(|v| v.parse().ok()),
                f.get(2).and_then(|v| v.parse().ok()),
                f.get(3).and_then(|v| v.parse::<u8>().ok()),
                f.get(4).and_then(|h| unhex(h)).and_then(|b| crate::inventory::decode_slots(&b, 1, 1)).and_then(|(s, _)| s.into_iter().next().flatten()),
            ) else {
                continue;
            };
            m.insert((x, y, z), (item, r % 8));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_turn_take_and_save() {
        crate::blocks::init();
        let item = crate::items::item_by_name("minecraft:diamond").unwrap();
        let pos = (900, 64, 900);
        put(pos, Stack { item, count: 5, net_id: 0 });
        assert_eq!(get(pos).map(|(s, r)| (s.count, r)), Some((1, 0)), "one item, facing up");
        for _ in 0..9 {
            rotate(pos);
        }
        assert_eq!(get(pos).unwrap().1, 1, "8 steps go round");
        let path = std::env::temp_dir().join(format!("rusted_frames_{}.dat", std::process::id()));
        save_if_dirty(&path).unwrap();
        assert!(take(pos).is_some());
        assert!(get(pos).is_none());
        load(&path);
        assert_eq!(get(pos).map(|(s, r)| (crate::items::name_of(s.item.id), r)), Some((Some("minecraft:diamond"), 1)), "came back from the file");
        take(pos);
        let _ = std::fs::remove_file(path);
    }
}

//! Fishing rods (PNX `EntityFishingHook`, catch odds from the Minecraft Wiki). Cast, wait for a
//! bite on water, reel in while it's biting.

use crate::Protocol;
use crate::players::{Event, Server};
use bedrock::protocol::v662::types::{ActorRuntimeID, ActorUniqueID, DataItem, PropertySyncData};
use bedrock::protocol::v924::enums::ActorDataIDs;
use bedrock::protocol::v2168::enums::DataItemType;
use parking_lot::Mutex;
use std::collections::HashMap;

/// Hook ticks run at the mob tick rate (10 a second).
const TPS: u32 = 10;

struct Hook {
    id: i64,
    pos: (f32, f32, f32),
    vel: (f32, f32, f32),
    in_water: bool,
    /// Ticks until a fish bites (counts down once the hook is in water).
    wait: u32,
    /// Ticks left to reel in a biting fish.
    biting: u32,
    luck: i16,
    lure: i16,
}

/// One hook per player.
fn hooks() -> &'static Mutex<HashMap<i64, Hook>> {
    static H: std::sync::OnceLock<Mutex<HashMap<i64, Hook>>> = std::sync::OnceLock::new();
    H.get_or_init(Default::default)
}

fn relay(server: &Server, p: Protocol) {
    let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(p) });
}

fn remove(server: &Server, id: i64) {
    use bedrock::protocol::v662::packets::RemoveActorPacket;
    relay(server, Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) })));
}

fn rng(seed: u64) -> impl FnMut(u32) -> u32 {
    let mut s = seed | 1;
    move |n| {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s % n.max(1) as u64) as u32
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64)
}

/// 5-30 s, 5 s shorter per level of Lure (vanilla).
fn wait_ticks(lure: i16, roll: &mut impl FnMut(u32) -> u32) -> u32 {
    (5 + roll(26)).saturating_sub(5 * lure as u32).max(1) * TPS
}

pub fn has_hook(player: i64) -> bool {
    hooks().lock().contains_key(&player)
}

/// Casts a hook from the player's eye, the way they look. `rod` is the rod's item data.
pub fn cast(server: &Server, player: i64, eye: (f32, f32, f32), rot: (f32, f32), rod: &[u8]) {
    let (pitch, yaw) = (rot.0.to_radians(), rot.1.to_radians());
    let dir = (-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos());
    let pos = (eye.0 + dir.0 * 0.3, eye.1 - 0.1, eye.2 + dir.2 * 0.3);
    let vel = (dir.0 * 0.8, dir.1 * 0.8 + 0.2, dir.2 * 0.8);
    let id = crate::players::next_id();
    let add = Protocol::AddActorPacket(Box::new(bedrock::protocol::v2225::packets::AddActorPacket {
        target_actor_id: ActorUniqueID(id),
        target_runtime_id: ActorRuntimeID(id as u64),
        actor_type: "minecraft:fishing_hook".into(),
        position: pos,
        velocity: vel,
        rotation: (0.0, 0.0),
        y_head_rotation: 0.0,
        y_body_rotation: 0.0,
        attributes: vec![],
        // The line is drawn from the owner to the hook.
        actor_data: vec![DataItem { data_item_id: ActorDataIDs::Owner, data_item_type: DataItemType::Int64(player) }],
        synced_properties: PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
        actor_links: vec![],
        passenger_of_block_data: None,
    }));
    relay(server, add);
    let (luck, lure) = (crate::enchant::level(rod, crate::enchant::LUCK_OF_THE_SEA), crate::enchant::level(rod, crate::enchant::LURE));
    let old = hooks().lock().insert(player, Hook { id, pos, vel, in_water: false, wait: 0, biting: 0, luck, lure });
    if let Some(old) = old {
        remove(server, old.id);
    }
}

/// Reels the hook in. Returns the catch (item, count) and XP if a fish was biting.
pub fn reel(server: &Server, player: i64) -> Option<(&'static str, i8, i32)> {
    let hook = hooks().lock().remove(&player)?;
    remove(server, hook.id);
    if hook.biting == 0 {
        return None;
    }
    let mut roll = rng(now() ^ hook.id as u64);
    let item = catch(hook.luck, &mut roll);
    Some((item, 1, 1 + roll(6) as i32))
}

/// What comes out of the water: fish, junk or treasure. Luck of the Sea moves the odds from
/// junk (10%, -2.5% a level) to treasure (5%, +2.1% a level).
fn catch(luck: i16, roll: &mut impl FnMut(u32) -> u32) -> &'static str {
    let pick = |entries: &[(&'static str, u32)], roll: &mut dyn FnMut(u32) -> u32| {
        let mut n = roll(entries.iter().map(|e| e.1).sum());
        entries.iter().find(|e| {
            let hit = n < e.1;
            n = n.saturating_sub(e.1);
            hit
        }).map_or("minecraft:cod", |e| e.0)
    };
    let l = luck.clamp(0, 3) as u32;
    let (junk, treasure) = (100 - 25 * l, 50 + 21 * l); // out of 1000
    let r = roll(1000);
    if r < treasure {
        // shortcut: the enchanted book is a plain book and bows/rods come unenchanted; add enchanting once loot can roll enchantments.
        pick(&[("minecraft:bow", 1), ("minecraft:book", 1), ("minecraft:fishing_rod", 1), ("minecraft:name_tag", 1), ("minecraft:nautilus_shell", 1), ("minecraft:saddle", 1)], roll)
    } else if r < treasure + junk {
        pick(&[
            ("minecraft:waterlily", 17), ("minecraft:bowl", 10), ("minecraft:leather", 10), ("minecraft:leather_boots", 10),
            ("minecraft:rotten_flesh", 10), ("minecraft:stick", 5), ("minecraft:string", 5), ("minecraft:glass_bottle", 10),
            ("minecraft:bone", 10), ("minecraft:ink_sac", 1), ("minecraft:tripwire_hook", 10),
        ], roll)
    } else {
        pick(&[("minecraft:cod", 60), ("minecraft:salmon", 25), ("minecraft:tropical_fish", 2), ("minecraft:pufferfish", 13)], roll)
    }
}

/// Moves hooks, bobs them on water and makes fish bite. Run at 10 Hz.
pub fn tick(server: &Server) {
    let w = &server.world;
    let mut out = Vec::new();
    let mut gone = Vec::new();
    let mut roll = rng(now());
    for (&player, h) in hooks().lock().iter_mut() {
        let Some(owner) = server.get(player) else {
            gone.push(player);
            continue;
        };
        let d2 = (owner.pos.0 - h.pos.0).powi(2) + (owner.pos.1 - h.pos.1).powi(2) + (owner.pos.2 - h.pos.2).powi(2);
        if d2 > 32.0 * 32.0 {
            gone.push(player); // line snapped
            continue;
        }
        let at = (h.pos.0.floor() as i32, h.pos.1.floor() as i32, h.pos.2.floor() as i32);
        let block = w.get(at.0, at.1, at.2);
        let water = crate::overworld::feature::blocks::is_water(block);
        if water {
            if !h.in_water {
                h.in_water = true;
                h.wait = wait_ticks(h.lure, &mut roll);
            }
            // Float at the surface.
            h.vel = (h.vel.0 * 0.5, (at.1 as f32 + 0.9 - h.pos.1) * 0.3, h.vel.2 * 0.5);
        } else if h.in_water || crate::overworld::feature::blocks::is_air(block) {
            h.in_water = false;
            h.vel.1 -= 0.08; // two 0.04 game ticks
            h.vel = (h.vel.0 * 0.92, h.vel.1 * 0.92, h.vel.2 * 0.92);
        } else {
            h.vel = (0.0, 0.0, 0.0); // stuck on a block
        }
        h.pos = (h.pos.0 + h.vel.0 * 2.0, h.pos.1 + h.vel.1 * 2.0, h.pos.2 + h.vel.2 * 2.0);
        if h.in_water {
            use bedrock::protocol::v975::enums::ActorEvent;
            if h.biting > 0 {
                h.biting -= 1;
                if h.biting == 0 {
                    h.wait = wait_ticks(h.lure, &mut roll); // got away
                }
            } else if h.wait > 0 {
                h.wait -= 1;
                if h.wait == 0 {
                    h.biting = TPS; // about a second to reel in
                    out.push(event(h.id, ActorEvent::FishhookHooktime));
                    out.push(crate::packets::play_sound("random.splash", h.pos));
                }
            }
        }
        out.push(move_packet(h.id, h.pos));
    }
    let removed: Vec<i64> = {
        let mut hk = hooks().lock();
        gone.iter().filter_map(|p| hk.remove(p)).map(|h| h.id).collect()
    };
    for id in removed {
        remove(server, id);
    }
    for p in out {
        relay(server, p);
    }
}

fn event(id: i64, e: bedrock::protocol::v975::enums::ActorEvent) -> Protocol {
    Protocol::ActorEventPacket(Box::new(bedrock::protocol::v975::packets::ActorEventPacket {
        target_runtime_id: ActorRuntimeID(id as u64),
        event_id: e,
        data: 0,
        fire_at_position: None,
    }))
}

fn move_packet(id: i64, pos: (f32, f32, f32)) -> Protocol {
    use bedrock::protocol::v662::packets::MoveActorAbsolutePacket;
    use bedrock::protocol::v662::types::MoveActorAbsoluteData;
    Protocol::MoveActorAbsolutePacket(Box::new(MoveActorAbsolutePacket {
        move_data: MoveActorAbsoluteData { actor_runtime_id: ActorRuntimeID(id as u64), header: 0, position: pos, rotation_x: 0, rotation_y: 0, rotation_y_head: 0 },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catches_are_real_items_and_luck_helps() {
        crate::blocks::init();
        let mut roll = rng(12345);
        let mut treasure = [0; 2];
        for (i, luck) in [0i16, 3].into_iter().enumerate() {
            for _ in 0..4000 {
                let item = catch(luck, &mut roll);
                assert!(crate::items::item_by_name(item).is_some(), "{item}");
                if matches!(item, "minecraft:bow" | "minecraft:book" | "minecraft:fishing_rod" | "minecraft:name_tag" | "minecraft:nautilus_shell" | "minecraft:saddle") {
                    treasure[i] += 1;
                }
            }
        }
        assert!(treasure[1] > treasure[0] * 2, "luck of the sea finds more treasure: {treasure:?}");
        assert!(wait_ticks(3, &mut roll) <= 15 * TPS, "lure III bites within 15 s");
    }

    #[test]
    fn hook_floats_bites_and_reels_in_a_catch() {
        let server = Server::new();
        server.join(crate::players::tests::sample(9));
        let water = crate::blocks::any_state("minecraft:water").unwrap();
        for x in -3..=3 {
            for z in 0..=6 {
                server.world.set(x, -61, z, water);
            }
        }
        server.moved(9, (0.5, -58.0, 0.5), (0.0, 0.0), 0.0);
        cast(&server, 9, (0.5, -58.0, 0.5), (30.0, 0.0), &[]);
        assert!(has_hook(9));
        let mut bit = false;
        for _ in 0..(40 * TPS) {
            tick(&server);
            if hooks().lock()[&9].biting > 0 {
                bit = true;
                break;
            }
        }
        assert!(bit, "a fish bit within 30 s");
        let (item, count, xp) = reel(&server, 9).expect("reeled in a catch");
        assert!(count == 1 && (1..=6).contains(&xp) && crate::items::item_by_name(item).is_some());
        assert!(!has_hook(9));
        assert!(reel(&server, 9).is_none(), "nothing left to reel");
    }
}

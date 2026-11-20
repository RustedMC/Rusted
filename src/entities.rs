//! Items on the ground. They merge with nearby stacks, get picked up by walking over them,
//! and despawn after 5 minutes.

use std::collections::HashMap;
use parking_lot::Mutex;
use std::time::{Duration, Instant};

use bedrock::protocol::v662::packets::{RemoveActorPacket, TakeItemActorPacket};
use bedrock::protocol::v662::types::{ActorRuntimeID, ActorUniqueID};
use bedrock::protocol::v2168::types::NetworkItemStackDescriptorV2;

use crate::Protocol;
use crate::players::{Event, Server};

pub const DESPAWN: Duration = Duration::from_secs(300);
/// Vanilla waits ~2s before you can grab a thrown item.
#[cfg(test)]
pub const PICKUP_DELAY: Duration = Duration::from_millis(1500);
const PICKUP_RANGE: f32 = 1.5;

#[derive(Clone)]
pub struct ItemEntity {
    pub id: i64,
    pub item: NetworkItemStackDescriptorV2,
    pub count: i8,
    pub pos: (f32, f32, f32),
    pub born: Instant,
    /// blocks per game tick, same units the client predicts with
    pub vel: (f32, f32, f32),
    pub resting: bool,
    /// PNX: 40 ticks if thrown, 10 if dropped
    pub delay: Duration,
}

/// PNX EntityItem numbers: gravity 0.04/tick, drag 0.02, ground friction 0.6.
const GRAVITY: f32 = 0.04;
const DRAG: f32 = 0.02;
const GROUND_FRICTION: f32 = 0.6;

#[derive(Default)]
pub struct Entities {
    items: Mutex<HashMap<i64, ItemEntity>>,
    orbs: Mutex<HashMap<i64, Orb>>,
}

/// An experience orb (PNX EntityXpOrb): falls, drifts to the nearest player within 8 blocks.
#[derive(Clone)]
pub struct Orb {
    pub id: i64,
    pub pos: (f32, f32, f32),
    pub vel: (f32, f32, f32),
    pub value: i32,
    pub born: Instant,
}

/// Vanilla orb sizes, biggest first (EntityXpOrb.splitIntoOrbSizes).
const ORB_SIZES: [i32; 11] = [2477, 1237, 617, 307, 149, 73, 37, 17, 7, 3, 1];

/// Splits `points` into orbs the vanilla way.
pub fn orb_values(mut points: i32) -> Vec<i32> {
    let mut out = vec![];
    while points > 0 {
        let v = *ORB_SIZES.iter().find(|&&s| s <= points).unwrap_or(&1);
        out.push(v);
        points -= v;
    }
    out
}

fn orb_packet(o: &Orb) -> Protocol {
    use bedrock::protocol::v662::types::PropertySyncData;
    Protocol::AddActorPacket(Box::new(bedrock::protocol::v2225::packets::AddActorPacket {
        target_actor_id: ActorUniqueID(o.id),
        target_runtime_id: ActorRuntimeID(o.id as u64),
        actor_type: "minecraft:xp_orb".into(),
        position: o.pos,
        velocity: o.vel,
        rotation: (0.0, 0.0),
        y_head_rotation: 0.0,
        y_body_rotation: 0.0,
        attributes: vec![],
        actor_data: vec![],
        synced_properties: PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
        actor_links: vec![],
        passenger_of_block_data: None,
    }))
}

fn same(a: &NetworkItemStackDescriptorV2, b: &NetworkItemStackDescriptorV2) -> bool {
    a.id == b.id && a.aux_value == b.aux_value && a.block_runtime_id == b.block_runtime_id
}

pub fn add_packet(e: &ItemEntity) -> Protocol {
    let mut item = e.item.clone();
    item.stack_size = e.count as u16;
    item.net_id = None;
    Protocol::AddItemActorPacket(Box::new(bedrock::protocol::v662::packets::AddItemActorPacket {
        target_actor_id: ActorUniqueID(e.id),
        target_runtime_id: ActorRuntimeID(e.id as u64),
        item: to_wire(item),
        position: e.pos,
        velocity: e.vel,
        entity_data: vec![],
        from_fishing: false,
    }))
}

fn to_wire(i: NetworkItemStackDescriptorV2) -> bedrock::protocol::v2168::types::NetworkItemStackDescriptor {
    bedrock::protocol::v2168::types::NetworkItemStackDescriptor {
        id: i.id,
        stack_size: i.stack_size,
        aux_value: i.aux_value,
        net_id: i.net_id,
        block_runtime_id: i.block_runtime_id,
        user_data_buffer: i.user_data_buffer,
    }
}

fn remove_packet(id: i64) -> Protocol {
    Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) }))
}

fn passable(block: i32) -> bool {
    block == crate::world::AIR
        || crate::blocks::entry(block).is_some_and(|e| e.name.ends_with("water") || e.name.ends_with("lava"))
}

/// Rest position on top of the first solid block below.
fn resting(server: &Server, pos: (f32, f32, f32)) -> (f32, f32, f32) {
    let (x, z) = (pos.0.floor() as i32, pos.2.floor() as i32);
    let mut y = pos.1.floor() as i32;
    if !passable(server.world.get(x, y, z)) {
        return (pos.0, y as f32 + 1.0, pos.2); // inside a block: pop on top
    }
    while y > -64 && passable(server.world.get(x, y - 1, z)) {
        y -= 1;
    }
    (pos.0, y as f32 + 0.125, pos.2)
}

fn broadcast(server: &Server, p: Protocol) {
    let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(p) });
}

impl Entities {
    /// Items popping out of broken blocks, chests, mob loot. Motion from PNX Level.dropItem
    /// (random +-0.1 sideways, 0.2 up), pickup after 10 ticks.
    /// Where each dropped item stack is and how many items it holds (for pressure plates).
    pub fn stacks(&self) -> Vec<((f32, f32, f32), i8)> {
        self.items.lock().values().map(|e| (e.pos, e.count)).collect()
    }

    pub fn drop_item(&self, server: &Server, item: NetworkItemStackDescriptorV2, count: i8, pos: (f32, f32, f32)) {
        let h = (pos.0 * 73.1 + pos.1 * 19.7 + pos.2 * 37.3 + self.items.lock().len() as f32 * 11.0).sin() * 43_758.547;
        let (a, b) = (h.fract().abs(), (h * 1.618).fract().abs());
        self.spawn(server, item, count, pos, (a * 0.2 - 0.1, 0.2, b * 0.2 - 0.1), Duration::from_millis(500));
    }

    /// Thrown by a player, pickup after 40 ticks.
    pub fn drop_item_vel(&self, server: &Server, item: NetworkItemStackDescriptorV2, count: i8, pos: (f32, f32, f32), vel: (f32, f32, f32)) {
        self.spawn(server, item, count, pos, vel, Duration::from_secs(2));
    }

    fn spawn(&self, server: &Server, item: NetworkItemStackDescriptorV2, count: i8, pos: (f32, f32, f32), vel: (f32, f32, f32), delay: Duration) {
        if count <= 0 || item.id == 0 {
            return;
        }
        let _ = resting;
        let max = crate::items::max_stack(item.id);
        let mut items = self.items.lock();
        let near = items
            .values()
            .find(|e| same(&e.item, &item) && e.count + count <= max && dist2(e.pos, pos) < 1.0)
            .map(|e| e.id);
        let (count, born) = match near.and_then(|id| items.remove(&id)) {
            Some(old) => {
                broadcast(server, remove_packet(old.id));
                (old.count + count, old.born.min(Instant::now()))
            }
            None => (count, Instant::now()),
        };
        let e = ItemEntity { id: crate::players::next_id(), item, count, pos, born, vel, resting: false, delay };
        broadcast(server, add_packet(&e));
        items.insert(e.id, e);
    }

    /// Gravity, drag, landing, floating in water.
    pub fn tick(&self, server: &Server) {
        self.tick_orbs(server);
        let w = &server.world;
        let mut moved = Vec::new();
        {
            let mut items = self.items.lock();
            for e in items.values_mut() {
                if e.resting {
                    // block below got removed, fall again
                    let below = w.get(e.pos.0.floor() as i32, (e.pos.1 - 0.2).floor() as i32, e.pos.2.floor() as i32);
                    if passable(below) {
                        e.resting = false;
                    } else {
                        continue;
                    }
                }
                let mut p = e.pos;
                // two game ticks per 10 Hz update
                for _ in 0..2 {
                    let here = w.get(p.0.floor() as i32, p.1.floor() as i32, p.2.floor() as i32);
                    let in_water = crate::blocks::entry(here).is_some_and(|b| b.name.ends_with("water"));
                    if in_water {
                        e.vel.1 = (e.vel.1 + GRAVITY * 0.5).min(0.06); // bobs up
                    } else {
                        e.vel.1 -= GRAVITY;
                    }
                    let nx = (p.0 + e.vel.0, p.2 + e.vel.2);
                    if passable(w.get(nx.0.floor() as i32, p.1.floor() as i32, nx.1.floor() as i32)) {
                        p.0 = nx.0;
                        p.2 = nx.1;
                    } else {
                        e.vel.0 = 0.0;
                        e.vel.2 = 0.0;
                    }
                    let ny = p.1 + e.vel.1;
                    let (bx, bz) = (p.0.floor() as i32, p.2.floor() as i32);
                    let mut on_ground = false;
                    if e.vel.1 < 0.0 && !passable(w.get(bx, ny.floor() as i32, bz)) {
                        p.1 = ny.floor() + 1.0;
                        on_ground = true;
                    } else if e.vel.1 > 0.0 && !passable(w.get(bx, (ny + 0.25).floor() as i32, bz)) {
                        e.vel.1 = 0.0;
                    } else {
                        p.1 = ny;
                    }
                    let mut friction = 1.0 - DRAG;
                    if on_ground {
                        friction *= GROUND_FRICTION;
                    }
                    e.vel.0 *= friction;
                    e.vel.2 *= friction;
                    e.vel.1 *= 1.0 - DRAG;
                    if on_ground {
                        e.vel.1 *= -0.5; // small bounce
                        if e.vel.1.abs() < 0.02 && e.vel.0.abs() < 0.005 && e.vel.2.abs() < 0.005 {
                            e.vel = (0.0, 0.0, 0.0);
                            e.resting = true;
                            break;
                        }
                    }
                }
                if p.1 < -80.0 {
                    e.born -= DESPAWN; // fell out of the world: despawn on the next sweep
                }
                e.pos = p;
                moved.push((e.id, p, e.resting));
            }
        }
        for (id, pos, rest) in moved {
            broadcast(server, Protocol::MoveActorAbsolutePacket(Box::new(bedrock::protocol::v662::packets::MoveActorAbsolutePacket {
                move_data: bedrock::protocol::v662::types::MoveActorAbsoluteData {
                    actor_runtime_id: ActorRuntimeID(id as u64),
                    header: rest as i8,
                    position: pos,
                    rotation_x: 0,
                    rotation_y: 0,
                    rotation_y_head: 0,
                },
            })));
        }
    }

    /// For a player who just joined.
    pub fn spawn_packets(&self) -> Vec<Protocol> {
        let mut out: Vec<Protocol> = self.items.lock().values().map(add_packet).collect();
        out.extend(self.orbs.lock().values().map(orb_packet));
        out
    }

    /// Drops `points` of experience as orbs at `pos`, scattered a little.
    pub fn drop_xp(&self, server: &Server, pos: (f32, f32, f32), points: i32) {
        for (i, value) in orb_values(points).into_iter().enumerate() {
            let a = i as f32 * 2.4;
            let o = Orb {
                id: crate::players::next_id(),
                pos: (pos.0, pos.1 + 0.5, pos.2),
                vel: (a.cos() * 0.1, 0.2, a.sin() * 0.1),
                value,
                born: Instant::now(),
            };
            broadcast(server, orb_packet(&o));
            self.orbs.lock().insert(o.id, o);
        }
    }

    /// Orbs: gravity, then pulled toward the nearest player within 8 blocks (vanilla
    /// EntityXpOrb.onUpdate), picked up on touch.
    fn tick_orbs(&self, server: &Server) {
        let w = &server.world;
        let players = server.all();
        let mut picked: Vec<(i64, i64, i32)> = vec![];
        let mut moved = vec![];
        {
            let mut orbs = self.orbs.lock();
            for o in orbs.values_mut() {
                for _ in 0..2 {
                    o.vel.1 -= 0.03;
                    // Nearest player's middle, within 8 blocks.
                    let near = players
                        .iter()
                        .map(|p| (p, (p.pos.0, p.pos.1 - 0.8, p.pos.2)))
                        .map(|(p, c)| (p, c, dist2(o.pos, c).sqrt()))
                        .filter(|(_, _, d)| *d < 8.0)
                        .min_by(|a, b| a.2.total_cmp(&b.2));
                    if let Some((p, c, d)) = near {
                        if d < 1.0 {
                            picked.push((o.id, p.id, o.value));
                            break;
                        }
                        let f = 1.0 - d / 8.0;
                        let pull = f * f * 0.1;
                        o.vel.0 += (c.0 - o.pos.0) / d * pull;
                        o.vel.1 += (c.1 - o.pos.1) / d * pull;
                        o.vel.2 += (c.2 - o.pos.2) / d * pull;
                    }
                    let n = (o.pos.0 + o.vel.0, o.pos.1 + o.vel.1, o.pos.2 + o.vel.2);
                    if o.vel.1 < 0.0 && !passable(w.get(n.0.floor() as i32, n.1.floor() as i32, n.2.floor() as i32)) {
                        o.pos = (n.0, n.1.floor() + 1.0, n.2);
                        o.vel.1 = 0.0;
                        o.vel.0 *= 0.6;
                        o.vel.2 *= 0.6;
                    } else {
                        o.pos = n;
                    }
                    o.vel.0 *= 0.98;
                    o.vel.1 *= 0.98;
                    o.vel.2 *= 0.98;
                }
                moved.push((o.id, o.pos));
            }
            for (id, _, _) in &picked {
                orbs.remove(id);
            }
            let old: Vec<i64> = orbs.values().filter(|o| o.born.elapsed() >= DESPAWN || o.pos.1 < -80.0).map(|o| o.id).collect();
            for id in old {
                orbs.remove(&id);
                broadcast(server, remove_packet(id));
            }
        }
        for (id, pos) in moved {
            if picked.iter().any(|(o, _, _)| *o == id) {
                continue;
            }
            broadcast(server, Protocol::MoveActorAbsolutePacket(Box::new(bedrock::protocol::v662::packets::MoveActorAbsolutePacket {
                move_data: bedrock::protocol::v662::types::MoveActorAbsoluteData { actor_runtime_id: ActorRuntimeID(id as u64), header: 0, position: pos, rotation_x: 0, rotation_y: 0, rotation_y_head: 0 },
            })));
        }
        for (orb, player, value) in picked {
            broadcast(server, remove_packet(orb));
            let _ = server.events.send(Event::Xp { to: player, points: value });
            if let Some(p) = server.get(player) {
                broadcast(server, crate::packets::play_sound("random.orb", p.pos));
            }
        }
    }

    /// True if the inventory changed.
    pub fn pickup(&self, server: &Server, player: i64, feet: (f32, f32, f32), inv: &mut crate::inventory::Inventory) -> bool {
        let mut items = self.items.lock();
        let now = Instant::now();
        let ready: Vec<i64> = items
            .values()
            .filter(|e| now.duration_since(e.born) >= e.delay)
            .filter(|e| {
                let (dx, dz) = (e.pos.0 - feet.0, e.pos.2 - feet.2);
                dx * dx + dz * dz <= PICKUP_RANGE * PICKUP_RANGE && (e.pos.1 - feet.1).abs() <= 1.75
            })
            .map(|e| e.id)
            .collect();
        let mut changed = false;
        let me = server.get(player);
        for id in ready {
            let e = items.get_mut(&id).unwrap();
            let name = crate::items::name_of(e.item.id).unwrap_or("");
            if let Some(me) = &me
                && !server.plugins.item_pickup(server, me, name, e.count)
            {
                continue;
            }
            let left = inv.add(e.item.clone(), e.count);
            if left == e.count {
                continue; // inventory full
            }
            changed = true;
            broadcast(server, Protocol::TakeItemActorPacket(Box::new(TakeItemActorPacket {
                item_runtime_id: ActorRuntimeID(id as u64),
                actor_runtime_id: ActorRuntimeID(player as u64),
            })));
            if left == 0 {
                items.remove(&id);
                broadcast(server, remove_packet(id));
            } else {
                e.count = left;
                let e = e.clone();
                broadcast(server, remove_packet(id));
                broadcast(server, add_packet(&e));
            }
        }
        changed
    }

    /// Offers items sitting in the block above a hopper to `put`, which returns the leftover.
    pub fn suck(&self, server: &Server, h: (i32, i32, i32), mut put: impl FnMut(&crate::inventory::Stack) -> i8) {
        let mut items = self.items.lock();
        let now = Instant::now();
        let ids: Vec<i64> = items
            .values()
            .filter(|e| now.duration_since(e.born) >= e.delay)
            .filter(|e| e.pos.0.floor() as i32 == h.0 && e.pos.2.floor() as i32 == h.2 && (e.pos.1 - (h.1 + 1) as f32).abs() < 0.8)
            .map(|e| e.id)
            .collect();
        for id in ids {
            let e = items.get_mut(&id).unwrap();
            let stack = crate::inventory::Stack { item: e.item.clone(), count: e.count, net_id: 0 };
            let left = put(&stack);
            if left == e.count {
                continue;
            }
            broadcast(server, remove_packet(id));
            if left == 0 {
                items.remove(&id);
            } else {
                e.count = left;
                let e = e.clone();
                broadcast(server, add_packet(&e));
            }
        }
    }

    /// Despawn after 5 minutes.
    pub fn despawn_old(&self, server: &Server) {
        let mut items = self.items.lock();
        let old: Vec<i64> = items.values().filter(|e| e.born.elapsed() >= DESPAWN).map(|e| e.id).collect();
        for id in old {
            items.remove(&id);
            broadcast(server, remove_packet(id));
        }
    }

    #[cfg(test)]
    pub fn count(&self) -> usize {
        self.items.lock().len()
    }
}

#[cfg(test)]
#[test]
fn orbs_split_like_vanilla() {
    assert_eq!(orb_values(5), vec![3, 1, 1]);
    assert_eq!(orb_values(20), vec![17, 3]);
    assert_eq!(orb_values(0), Vec::<i32>::new());
}

fn dist2(a: (f32, f32, f32), b: (f32, f32, f32)) -> f32 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_merge_fall_and_pickup() {
        let server = Server::new();
        let dirt = crate::items::item_by_name("minecraft:dirt").unwrap();
        // flat world grass is at -61, so items rest at -60
        server.entities.drop_item(&server, dirt.clone(), 3, (0.5, -40.0, 0.5));
        server.entities.drop_item(&server, dirt.clone(), 2, (0.7, -40.0, 0.5));
        assert_eq!(server.entities.count(), 1, "merged");
        for _ in 0..40 {
            server.entities.tick(&server);
        }
        let e = server.entities.items.lock().values().next().unwrap().clone();
        assert_eq!(e.count, 5);
        assert!(e.resting && (e.pos.1 - -60.0).abs() < 0.01, "fell and rests on the grass: {:?}", e.pos);
        let mut inv = crate::inventory::Inventory::default();
        // Debug builds can take longer than the delay to get here, so restart its clock.
        server.entities.items.lock().values_mut().for_each(|e| e.born = Instant::now());
        assert!(!server.entities.pickup(&server, 1, (e.pos.0, -60.0, e.pos.2), &mut inv), "pickup delay");
        server.entities.items.lock().values_mut().for_each(|e| e.born -= PICKUP_DELAY);
        assert!(server.entities.pickup(&server, 1, (e.pos.0, -60.0, e.pos.2), &mut inv));
        assert_eq!(server.entities.count(), 0);
        assert_eq!(inv.slots[0].as_ref().unwrap().count, 5);
    }
}

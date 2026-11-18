//! Firework rockets launched from the ground (PNX `EntityFireworksRocket`): they climb,
//! speeding up, then burst after about two seconds.

use crate::Protocol;
use crate::itemdata::{ItemData, Tag};
use crate::players::{Event, Server};
use bedrock::protocol::v662::types::{ActorRuntimeID, ActorUniqueID, DataItem, PropertySyncData};
use bedrock::protocol::v924::enums::ActorDataIDs;
use bedrock::protocol::v2168::enums::DataItemType;
use parking_lot::Mutex;

struct Rocket {
    id: i64,
    pos: (f32, f32, f32),
    /// Per game tick, as PNX keeps it; we step two game ticks at a time.
    vel: (f32, f32, f32),
    age: u32,
    lifetime: u32,
}

static ROCKETS: Mutex<Vec<Rocket>> = Mutex::new(Vec::new());

fn relay(server: &Server, p: Protocol) {
    let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(p) });
}

/// Launches the rocket `item_data` (the held rocket's item data) from `at`.
pub fn launch(server: &Server, at: (f32, f32, f32), item_data: &[u8]) {
    let id = crate::players::next_id();
    let fireworks = ItemData::read(item_data).get("Fireworks").cloned().unwrap_or(Tag::Compound(vec![]));
    let flight = match &fireworks {
        Tag::Compound(f) => f.iter().find(|(k, _)| k == "Flight").and_then(|(_, v)| if let Tag::Byte(b) = v { Some(*b as u32) } else { None }).unwrap_or(1),
        _ => 1,
    };
    // The client draws the burst from the rocket's item NBT.
    let display = Tag::Compound(vec![
        ("Name".into(), Tag::String("minecraft:firework_rocket".into())),
        ("Count".into(), Tag::Byte(1)),
        ("Damage".into(), Tag::Short(0)),
        ("tag".into(), Tag::Compound(vec![("Fireworks".into(), fireworks)])),
    ]);
    let seed = (at.0 * 31.0 + at.2 * 17.0 + id as f32) as u32;
    // Vanilla: 10 game ticks per point of flight plus 0-11 more.
    let lifetime = 10 * (flight + 1) + seed % 12;
    let add = Protocol::AddActorPacket(Box::new(bedrock::protocol::v2225::packets::AddActorPacket {
        target_actor_id: ActorUniqueID(id),
        target_runtime_id: ActorRuntimeID(id as u64),
        actor_type: "minecraft:fireworks_rocket".into(),
        position: at,
        velocity: (0.0, 0.05, 0.0),
        rotation: (0.0, 0.0),
        y_head_rotation: 0.0,
        y_body_rotation: 0.0,
        attributes: vec![],
        actor_data: vec![DataItem { data_item_id: ActorDataIDs::DisplayTileRuntimeId, data_item_type: DataItemType::NBT(crate::lectern::value(&display)) }],
        synced_properties: PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
        actor_links: vec![],
        passenger_of_block_data: None,
    }));
    relay(server, add);
    relay(server, crate::packets::play_sound("firework.launch", at));
    ROCKETS.lock().push(Rocket { id, pos: at, vel: (0.0, 0.05, 0.0), age: 0, lifetime });
}

/// Moves rockets and bursts the ones that are done. Run at 10 Hz.
pub fn tick(server: &Server) {
    let mut out = Vec::new();
    ROCKETS.lock().retain_mut(|r| {
        for _ in 0..2 {
            r.vel = (r.vel.0 * 1.15, r.vel.1 + 0.04, r.vel.2 * 1.15);
            r.pos = (r.pos.0 + r.vel.0, r.pos.1 + r.vel.1, r.pos.2 + r.vel.2);
            r.age += 1;
        }
        if r.age >= r.lifetime {
            use bedrock::protocol::v975::enums::ActorEvent;
            out.push(Protocol::ActorEventPacket(Box::new(bedrock::protocol::v975::packets::ActorEventPacket {
                target_runtime_id: ActorRuntimeID(r.id as u64),
                event_id: ActorEvent::FireworksExplode,
                data: 0,
                fire_at_position: None,
            })));
            out.push(crate::packets::play_sound("firework.large_blast", r.pos));
            out.push(Protocol::RemoveActorPacket(Box::new(bedrock::protocol::v662::packets::RemoveActorPacket { target_actor_id: ActorUniqueID(r.id) })));
            return false;
        }
        out.push(Protocol::MoveActorAbsolutePacket(Box::new(bedrock::protocol::v662::packets::MoveActorAbsolutePacket {
            move_data: bedrock::protocol::v662::types::MoveActorAbsoluteData { actor_runtime_id: ActorRuntimeID(r.id as u64), header: 0, position: r.pos, rotation_x: 0, rotation_y: 0, rotation_y_head: 0 },
        })));
        true
    });
    for p in out {
        relay(server, p);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rockets_climb_then_burst() {
        crate::blocks::init();
        let server = crate::players::Server::new();
        super::launch(&server, (0.5, 70.0, 0.5), &[]);
        let start = super::ROCKETS.lock().last().map(|r| (r.id, r.lifetime)).unwrap();
        for _ in 0..(start.1 / 2 + 1) {
            super::tick(&server);
        }
        assert!(super::ROCKETS.lock().iter().all(|r| r.id != start.0), "burst after its lifetime");
    }
}

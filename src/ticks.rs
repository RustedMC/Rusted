//! World ticks that run outside player sessions: plants, redstone, hoppers, fire, weather.

use super::*;

/// Random ticks near players every 250 ms: 3 blocks per subchunk, 3 chunks out, 32 blocks tall.
/// Real game does that every tick; 5x slower keeps forests cheap.
pub(crate) fn random_ticks(server: &players::Server, rng: &mut u64) {
    let mut next = || {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 7;
        *rng ^= *rng << 17;
        *rng as u32
    };
    let raining = *commands::WEATHER.lock() != commands::Weather::Clear;
    let speed = {
        use bedrock::protocol::v844::types::Type;
        server.game_rules.lock().iter().find(|r| r.rule_name == "randomTickSpeed").map_or(1, |r| match r.rule_type {
            Type::Int(n) => n.clamp(0, 4096),
            _ => 1,
        })
    };
    let mut seen = std::collections::HashSet::new();
    for p in server.all() {
        let (cx, cz, sy) = ((p.pos.0.floor() as i32) >> 4, (p.pos.2.floor() as i32) >> 4, (p.pos.1.floor() as i32) >> 4);
        for x in cx - 3..=cx + 3 {
            for z in cz - 3..=cz + 3 {
                for s in sy - 2..=sy + 2 {
                    if !(-4..20).contains(&s) || !seen.insert((x, s, z)) {
                        continue;
                    }
                    // Bedrock: randomTickSpeed blocks per sub-chunk per game tick; this runs every 5 ticks.
                    for _ in 0..speed * 5 {
                        let r = next();
                        let pos = (x * 16 + (r & 15) as i32, s * 16 + ((r >> 4) & 15) as i32, z * 16 + ((r >> 8) & 15) as i32);
                        let snowy = raining && server.world.is_snowy(pos.0, pos.2);
                        let before = server.world.get(pos.0, pos.1, pos.2);
                        for ((bx, by, bz), b) in growth::random_tick(&server.world, pos, next(), raining, snowy) {
                            // Decaying leaves drop saplings/apples.
                            if (bx, by, bz) == pos && b == world::AIR && game_rule_on(server, "doTileDrops", true) {
                                for (item, count) in drops::drops(before, pos, None) {
                                    server.entities.drop_item(server, item, count, (bx as f32 + 0.5, by as f32 + 0.5, bz as f32 + 0.5));
                                }
                            }
                            server.set_block_quiet(0, bx, by, bz, b);
                            if blocks::entry(b).is_some_and(|e| e.name == "minecraft:fire") {
                                growth::FIRES.lock().push((bx, by, bz));
                            }
                        }
                    }
                }
            }
        }
    }
}

pub(crate) fn redstone_tick(server: &players::Server, plates: &mut HashMap<(i32, i32, i32), Instant>) {
    // Plates stay pressed while someone stands on them, then 1 s more (vanilla 20 ticks).
    for p in server.all() {
        let feet = (p.pos.0.floor() as i32, (p.pos.1 - 1.62 + 0.05).floor() as i32, p.pos.2.floor() as i32);
        let b = server.world.get(feet.0, feet.1, feet.2);
        if blocks::entry(b).is_some_and(|e| e.name.ends_with("pressure_plate"))
            && (plates.insert(feet, Instant::now()).is_none() || !matches!(blocks::state(b, "redstone_signal"), Some(blocks::State::Int(15)))) {
                server.set_block_quiet(0, feet.0, feet.1, feet.2, blocks::with(b, &[("redstone_signal", blocks::State::Int(15))]));
                let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound("random.click", (feet.0 as f32 + 0.5, feet.1 as f32, feet.2 as f32 + 0.5))) });
            }
    }
    plates.retain(|&q, t| {
        if t.elapsed() < std::time::Duration::from_secs(1) {
            return true;
        }
        let b = server.world.get(q.0, q.1, q.2);
        if blocks::entry(b).is_some_and(|e| e.name.ends_with("pressure_plate")) {
            server.set_block_quiet(0, q.0, q.1, q.2, blocks::with(b, &[("redstone_signal", blocks::State::Int(0))]));
        }
        false
    });
    server.scheduler.tick(server);
    let step = redstone::tick(&server.world, &|p| server.containers.comparator(&server.world, p));
    for (p, b) in step.blocks {
        server.set_block_quiet(0, p.0, p.1, p.2, b);
    }
    for p in step.dispense {
        dispense(server, p);
    }
    for p in step.ignite {
        mobs::ignite_tnt(server, p, 40);
    }
    for p in step.notes {
        apply_use(server, 0, p, &useblock::play_note(&server.world, p));
    }
    for (p, s) in step.sounds {
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound(s, (p.0 as f32 + 0.5, p.1 as f32 + 0.5, p.2 as f32 + 0.5))) });
    }
    for (p, ext, sticky) in step.pistons {
        let data = Protocol::BlockActorDataPacket(Box::new(bedrock::protocol::v662::packets::BlockActorDataPacket {
            block_position: NetworkBlockPosition { x: p.0, y: p.1, z: p.2 },
            actor_data_tags: redstone::piston_data(p, ext, sticky),
        }));
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(data) });
    }
}

/// Dispenser/dropper firing (PNX BlockDispenser). Droppers pass items on; dispensers
/// shoot, place liquids, light TNT/fire, bone meal, or drop the item.
pub(crate) fn dispense(server: &players::Server, p: (i32, i32, i32)) {
    let b = server.world.get(p.0, p.1, p.2);
    let Some(kind) = containers::kind(b) else { return };
    let face = match blocks::state(b, "facing_direction") {
        Some(blocks::State::Int(f)) => *f as u8,
        _ => 0,
    };
    let (dx, dy, dz) = placement::face_offset(face);
    let front = (p.0 + dx, p.1 + dy, p.2 + dz);
    let roll = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
    let Some(st) = server.containers.take_one(p, roll) else {
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(level_event(1001, p)) }); // empty click
        return;
    };
    let at = (front.0 as f32 + 0.5, front.1 as f32 + 0.3, front.2 as f32 + 0.5);
    let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(level_event(1000, p)) });
    let name = items::name_of(st.item.id).unwrap_or("");
    let fire_dir = |dx: i32, dy: i32, dz: i32| {
        let yaw = (-(dx as f32)).atan2(dz as f32).to_degrees();
        let pitch = -(dy as f32) * 90.0;
        (pitch, yaw)
    };
    if kind == containers::Kind::Dropper {
        if !server.containers.put_one(&server.world, front, &st) {
            server.entities.drop_item_vel(server, st.item, 1, at, (dx as f32 * 0.2, dy as f32 * 0.2 + 0.1, dz as f32 * 0.2));
        }
        return;
    }
    let eye = (p.0 as f32 + 0.5 + dx as f32 * 0.7, p.1 as f32 + 0.5 + dy as f32 * 0.7 + 1.62 - 0.3, p.2 as f32 + 0.5 + dz as f32 * 0.7);
    let back = |item: &str| {
        if let Some(it) = items::item_by_name(item) {
            server.containers.put_one(&server.world, p, &inventory::Stack { item: it, count: 1, net_id: 0 });
        }
    };
    match name {
        "minecraft:arrow" => server.mobs.player_shoot(server, 0, eye, fire_dir(dx, dy, dz), 1.0, false, 0, 0, false, items::item_by_name("minecraft:arrow")),
        "minecraft:snowball" | "minecraft:egg" => mobs::throw(server, mobs::Throw::from_item(name).unwrap(), 0, eye, fire_dir(dx, dy, dz)),
        "minecraft:splash_potion" | "minecraft:lingering_potion" => mobs::throw(server, mobs::Throw::Potion(st.item.aux_value), 0, eye, fire_dir(dx, dy, dz)),
        "minecraft:water_bucket" | "minecraft:lava_bucket" if server.world.get(front.0, front.1, front.2) == world::AIR => {
            let k = if name == "minecraft:water_bucket" { fluids::Kind::Water } else { fluids::Kind::Lava };
            server.set_block(0, front.0, front.1, front.2, k.source());
            back("minecraft:bucket");
        }
        "minecraft:bucket" if server.world.get(front.0, front.1, front.2) == fluids::Kind::Water.source() => {
            server.set_block(0, front.0, front.1, front.2, world::AIR);
            back("minecraft:water_bucket");
        }
        "minecraft:tnt" => {
            if let Some(t) = blocks::any_state("minecraft:tnt") {
                server.set_block_quiet(0, front.0, front.1, front.2, t);
                mobs::ignite_tnt(server, front, 40);
            }
        }
        "minecraft:flint_and_steel" | "minecraft:fire_charge" if server.world.get(front.0, front.1, front.2) == world::AIR => {
            if let Some(f) = blocks::any_state("minecraft:fire") {
                server.set_block(0, front.0, front.1, front.2, f);
                growth::FIRES.lock().push(front);
            }
            if name == "minecraft:flint_and_steel" {
                back(name); // TODO: dispensers should wear out flint and steel
            }
        }
        "minecraft:bone_meal" => {
            match growth::bone_meal(&server.world, front, roll) {
                Some(ch) => {
                    for ((x, y, z), b) in ch {
                        server.set_block(0, x, y, z, b);
                    }
                }
                None => back(name),
            }
        }
        _ => server.entities.drop_item_vel(server, st.item, 1, at, (dx as f32 * 0.2, dy as f32 * 0.2 + 0.1, dz as f32 * 0.2)),
    }
}

/// Hoppers move items, then each pulls in items lying on top of it.
pub(crate) fn hopper_ticks(server: &players::Server) {
    server.containers.tick_hoppers(&server.world);
    let hoppers: Vec<_> = server.containers.hoppers.lock().iter().copied().collect();
    for h in hoppers {
        server.entities.suck(server, h, |stack| server.containers.hopper_insert(h, stack));
    }
}

/// Lightning near players, ~1 strike per 100000 ticks per chunk (PNX Level.updateWeather).
/// Hurts anyone within 3 blocks and starts a fire.
pub(crate) fn lightning(server: &players::Server, rng: &mut u64) {
    let mut next = || {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 7;
        *rng ^= *rng << 17;
        *rng
    };
    for p in server.all() {
        if dims::of_f(p.pos.1) != dims::OVERWORLD || next() % 70 != 0 {
            continue; // ~289 chunks around a player, 5 ticks per call
        }
        let x = p.pos.0.floor() as i32 + (next() % 97) as i32 - 48;
        let z = p.pos.2.floor() as i32 + (next() % 97) as i32 - 48;
        let mut y = 318;
        while y > -63 && server.world.get(x, y, z) == world::AIR {
            y -= 1;
        }
        let at = (x as f32 + 0.5, (y + 1) as f32, z as f32 + 0.5);
        let bolt = players::next_id();
        let add = Protocol::AddActorPacket(Box::new(bedrock::protocol::v2225::packets::AddActorPacket {
            target_actor_id: ActorUniqueID(bolt),
            target_runtime_id: ActorRuntimeID(bolt as u64),
            actor_type: "minecraft:lightning_bolt".into(),
            position: at,
            velocity: (0.0, 0.0, 0.0),
            rotation: (0.0, 0.0),
            y_head_rotation: 0.0,
            y_body_rotation: 0.0,
            attributes: vec![],
            actor_data: vec![],
            synced_properties: bedrock::protocol::v662::types::PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
            actor_links: vec![],
            passenger_of_block_data: None,
        }));
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(add) });
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound("ambient.weather.thunder", at)) });
        server.scheduler.run_later(20, move |s| {
            let _ = s.events.send(players::Event::Relay { from: 0, packet: Box::new(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(bolt) }))) });
        });
        for q in server.all() {
            let d = ((q.pos.0 - at.0).powi(2) + (q.pos.1 - 1.62 - at.1).powi(2) + (q.pos.2 - at.2).powi(2)).sqrt();
            if d < 3.0 {
                let _ = server.events.send(players::Event::Damage { to: q.id, damage: health::Damage::Explosion(5.0, "Lightning"), knock: (0.0, 0.1, 0.0) });
            }
        }
        if game_rule_on(server, "doFireTick", true)
            && server.world.get(x, y + 1, z) == world::AIR
            && let Some(fire) = blocks::any_state("minecraft:fire")
        {
            server.set_block_quiet(0, x, y + 1, z, fire);
            growth::FIRES.lock().push((x, y + 1, z));
        }
    }
}

/// Fire spread, roughly every 30 game ticks like PNX.
pub(crate) fn fire_ticks(server: &players::Server, rng: &mut u64) {
    let fires = std::mem::take(&mut *growth::FIRES.lock());
    let raining = *commands::WEATHER.lock() != commands::Weather::Clear;
    let tick = game_rule_on(server, "doFireTick", true);
    let mut keep = Vec::new();
    for p in fires {
        let changes = if tick {
            growth::fire_tick(&server.world, p, raining, |n| {
                *rng ^= *rng << 13;
                *rng ^= *rng >> 7;
                *rng ^= *rng << 17;
                (*rng % n.max(1) as u64) as u32
            })
        } else {
            vec![]
        };
        if !changes.iter().any(|(q, b)| *q == p && *b == world::AIR) && blocks::entry(server.world.get(p.0, p.1, p.2)).is_some_and(|e| e.name == "minecraft:fire") {
            keep.push(p);
        }
        for (q, b) in changes {
            let old = server.world.get(q.0, q.1, q.2);
            if q != p && blocks::entry(old).is_some_and(|e| e.name == "minecraft:tnt") {
                mobs::ignite_tnt(server, q, 40);
                continue;
            }
            server.set_block_quiet(0, q.0, q.1, q.2, b);
            if q != p && blocks::entry(b).is_some_and(|e| e.name == "minecraft:fire") {
                keep.push(q);
            }
            if q != p && b == world::AIR && old != world::AIR {
                pop_unsupported(server, 0, q);
            }
        }
    }
    keep.sort_unstable();
    keep.dedup();
    growth::FIRES.lock().extend(keep);
}

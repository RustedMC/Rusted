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
                            let name = |b: i32| blocks::entry(b).map_or("", |e| e.name.as_str());
                            if !server.plugins.block_grow(server, (bx, by, bz), name(server.world.get(bx, by, bz)), name(b)) {
                                continue;
                            }
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
    // Plates stay pressed while something is on them, then 1 s more (vanilla 20 ticks).
    // Stone plates feel players and mobs; wooden ones also items; weighted ones count
    // (gold: 1 a thing, iron: 1 per 10), as in PNX BlockPressurePlateBase.
    let mut on: HashMap<(i32, i32, i32), (u32, u32)> = HashMap::new(); // (players + mobs, items)
    let block_of = |x: f32, y: f32, z: f32| (x.floor() as i32, (y + 0.05).floor() as i32, z.floor() as i32);
    for p in server.all() {
        on.entry(block_of(p.pos.0, p.pos.1 - 1.62, p.pos.2)).or_default().0 += 1;
    }
    for m in server.mobs.feet() {
        on.entry(block_of(m.0, m.1, m.2)).or_default().0 += 1;
    }
    for (pos, n) in server.entities.stacks() {
        on.entry(block_of(pos.0, pos.1, pos.2)).or_default().1 += n.max(0) as u32;
    }
    for (feet, (beings, items)) in on {
        let b = server.world.get(feet.0, feet.1, feet.2);
        let Some(name) = blocks::entry(b).map(|e| e.name.trim_start_matches("minecraft:")).filter(|n| n.ends_with("pressure_plate")) else { continue };
        let signal = match name {
            "light_weighted_pressure_plate" => (beings + items).min(15) as i32,
            "heavy_weighted_pressure_plate" => (beings + items).div_ceil(10).min(15) as i32,
            "stone_pressure_plate" | "polished_blackstone_pressure_plate" => if beings > 0 { 15 } else { 0 },
            _ => if beings + items > 0 { 15 } else { 0 },
        };
        if signal == 0 {
            continue;
        }
        let was = matches!(blocks::state(b, "redstone_signal"), Some(blocks::State::Int(s)) if *s == signal);
        plates.insert(feet, Instant::now());
        if !was {
            server.set_block_quiet(0, feet.0, feet.1, feet.2, blocks::with(b, &[("redstone_signal", blocks::State::Int(signal))]));
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

/// A crafter got a redstone pulse: craft its grid's recipe once and push the result out the
/// front, into a container there or onto the ground (vanilla crafter).
fn craft(server: &players::Server, p: (i32, i32, i32), b: i32) {
    let slots = server.containers.get(p);
    let mut grid: [Option<(&str, i16)>; 9] = [None; 9];
    for (i, s) in slots.iter().take(9).enumerate() {
        grid[i] = s.as_ref().and_then(|s| items::name_of(s.item.id).map(|n| (n, s.item.aux_value as i16)));
    }
    let Some((item, count)) = crate::recipes::get().craft_grid(&grid) else {
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(level_event(1001, p)) }); // nothing to craft
        return;
    };
    // Use up one of each input.
    let mut left = slots;
    for s in left.iter_mut().take(9) {
        if let Some(st) = s {
            st.count -= 1;
            if st.count <= 0 {
                *s = None;
            }
        }
    }
    server.containers.put(p, left);
    let front = match blocks::state(b, "orientation") {
        Some(blocks::State::Str(o)) => match o.split('_').next() {
            Some("down") => (0, -1, 0),
            Some("up") => (0, 1, 0),
            Some("north") => (0, 0, -1),
            Some("south") => (0, 0, 1),
            Some("west") => (-1, 0, 0),
            _ => (1, 0, 0),
        },
        _ => (0, 0, 1),
    };
    let to = (p.0 + front.0, p.1 + front.1, p.2 + front.2);
    let mut out = crate::inventory::Stack { item: item.clone(), count: 1, net_id: 0 };
    for _ in 0..count {
        if !server.containers.put_one(&server.world, to, &out) {
            let at = (to.0 as f32 + 0.5, to.1 as f32 + 0.3, to.2 as f32 + 0.5);
            server.entities.drop_item_vel(server, out.item.clone(), 1, at, (front.0 as f32 * 0.2, front.1 as f32 * 0.2 + 0.1, front.2 as f32 * 0.2));
        }
        out.count = 1;
    }
    let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(level_event(1000, p)) });
}

/// Dispenser/dropper firing (PNX BlockDispenser). Droppers pass items on; dispensers
/// shoot, place liquids, light TNT/fire, bone meal, or drop the item.
pub(crate) fn dispense(server: &players::Server, p: (i32, i32, i32)) {
    let b = server.world.get(p.0, p.1, p.2);
    let Some(kind) = containers::kind(b) else { return };
    if kind == containers::Kind::Crafter {
        return craft(server, p, b);
    }
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
    // A chest minecart over a hopper (or in it) gets emptied into it, like a chest would.
    let carts = server.mobs.chest_carts();
    for &h in &hoppers {
        for &(at, store) in &carts {
            if at == (h.0, h.1 + 1, h.2) || at == h {
                if let Some(loot) = crate::loot::take(store, crate::inventory::CHEST_SLOTS) {
                    server.containers.put(store, loot);
                }
                server.containers.move_one(store, 0..crate::inventory::CHEST_SLOTS, h, 0..5);
            }
        }
    }
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
            let spread = blocks::entry(b).is_some_and(|e| e.name == "minecraft:fire");
            let burned = b == world::AIR && old != world::AIR;
            if q != p && (spread || burned) && !server.plugins.block_burn(server, q, blocks::entry(old).map_or("", |e| e.name.as_str()), spread) {
                continue;
            }
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

#[cfg(test)]
mod crafter_tests {
    use super::*;

    #[test]
    fn pulsed_crafter_crafts_into_the_chest_in_front() {
        let server = players::Server::new();
        let crafter = blocks::with(blocks::any_state("minecraft:crafter").unwrap(), &[("orientation", blocks::State::Str("south_up".into()))]);
        let at = (5, -60, 5);
        server.world.set(at.0, at.1, at.2, crafter);
        server.world.set(5, -60, 6, blocks::any_state("minecraft:chest").unwrap());
        let plank = items::item_by_name("minecraft:oak_planks").unwrap();
        let mut slots = vec![None; crate::inventory::CHEST_SLOTS];
        slots[4] = Some(crate::inventory::Stack { item: plank.clone(), count: 3, net_id: 0 });
        slots[7] = Some(crate::inventory::Stack { item: plank, count: 1, net_id: 0 });
        server.containers.put(at, slots);
        dispense(&server, at);
        let left = server.containers.get(at);
        assert_eq!(left[4].as_ref().map(|s| s.count), Some(2), "one plank used from each slot");
        assert!(left[7].is_none());
        let chest = server.containers.get((5, -60, 6));
        let sticks: i32 = chest.iter().flatten().filter(|s| items::name_of(s.item.id) == Some("minecraft:stick")).map(|s| s.count as i32).sum();
        assert_eq!(sticks, 4, "sticks went into the chest");
        // Pulse again: a lone plank is a recipe too (a button).
        dispense(&server, at);
        assert_eq!(server.containers.get(at)[4].as_ref().map(|s| s.count), Some(1));
        let chest = server.containers.get((5, -60, 6));
        assert!(chest.iter().flatten().any(|s| items::name_of(s.item.id).is_some_and(|n| n.ends_with("button"))));
    }
}

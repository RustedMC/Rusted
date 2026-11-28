//! Breaking blocks, containers opening and closing, block-use effects.

use super::*;

/// Bookshelves in the ring two blocks out, on the table level and one up.
/// TODO: check for blocks in between.
pub(crate) fn bookshelves(w: &world::World, (x, y, z): (i32, i32, i32)) -> i32 {
    let mut n = 0;
    for dy in 0..=1 {
        for dx in -2..=2i32 {
            for dz in -2..=2i32 {
                if (dx.abs() == 2 || dz.abs() == 2) && blocks::entry(w.get(x + dx, y + dy, z + dz)).is_some_and(|e| e.name == "minecraft:bookshelf") {
                    n += 1;
                }
            }
        }
    }
    n.min(15)
}

/// Players who used a bed at night recently (all of them = skip the night).
pub(crate) fn sleepers() -> &'static parking_lot::Mutex<HashMap<i64, Instant>> {
    static S: std::sync::OnceLock<parking_lot::Mutex<HashMap<i64, Instant>>> = std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}

/// A player lying in a bed (or getting up): the sleeping player flag and the bed position,
/// so everyone sees them lie down (PNX Player.sleepOn / stopSleep).
pub(crate) fn sleep_packet(id: i64, bed: Option<(i32, i32, i32)>) -> Protocol {
    use bedrock::protocol::v2168::enums::DataItemType;
    use bedrock::protocol::v662::types::{ActorRuntimeID, BlockPos, DataItem, PropertySyncData};
    use bedrock::protocol::v924::enums::ActorDataIDs;
    // Player flag bit 1 = sleeping. Waking leaves the bed position out: sending 0,0,0 made the
    // client put the player there.
    let mut actor_data = vec![DataItem { data_item_id: ActorDataIDs::PlayerFlags, data_item_type: DataItemType::Byte(if bed.is_some() { 2 } else { 0 }) }];
    if let Some((x, y, z)) = bed {
        actor_data.push(DataItem { data_item_id: ActorDataIDs::BedPosition, data_item_type: DataItemType::Pos(BlockPos { x, y, z }) });
    }
    Protocol::SetActorDataPacket(Box::new(bedrock::protocol::v662::packets::SetActorDataPacket {
        target_runtime_id: ActorRuntimeID(id as u64),
        actor_data,
        synced_properties: PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
        tick: 0,
    }))
}

/// Gets a player out of bed.
pub(crate) fn wake(server: &players::Server, id: i64) {
    if sleepers().lock().remove(&id).is_some() {
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(sleep_packet(id, None)) });
    }
}

/// Once enough players have slept 5 s (100 ticks, vanilla), skip to morning
/// (playersSleepingPercentage, default 100).
pub(crate) fn check_sleep(server: &players::Server) {
    use bedrock::protocol::v844::types::Type;
    let online = server.all();
    let mut sl = sleepers().lock();
    sl.retain(|id, _| online.iter().any(|p| p.id == *id));
    let rested = sl.values().filter(|t| t.elapsed() >= std::time::Duration::from_secs(5)).count();
    let pct = server.game_rules.lock().iter().find(|r| r.rule_name == "playersSleepingPercentage").and_then(|r| match r.rule_type {
        Type::Int(v) => Some(v),
        _ => None,
    });
    let pct = pct.unwrap_or(100).clamp(0, 100) as usize;
    let needed = (online.len() * pct).div_ceil(100).max(1);
    if rested < needed || !server.plugins.night_skip(server) {
        return;
    }
    let sleeping: Vec<i64> = sl.drain().map(|(id, _)| id).collect();
    drop(sl);
    let t = server.time().rem_euclid(24_000);
    let morning = server.time() - t + 24_000;
    server.set_time(morning);
    let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(crate::packets::clock_packet(morning)) });
    let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(crate::packets::time_packet(morning)) });
    *commands::WEATHER.lock() = commands::Weather::Clear;
    for w in commands::weather_packets(commands::Weather::Clear) {
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(w) });
    }
    for id in sleeping {
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(sleep_packet(id, None)) });
    }
}

/// Lid/door animation and sound when the first player opens a chest or barrel.
pub(crate) fn container_opened(server: &players::Server, pos: (i32, i32, i32)) {
    if server.containers.viewers(pos, 1) == 1 {
        container_anim(server, pos, true);
    }
}

pub(crate) fn container_closed(server: &players::Server, pos: (i32, i32, i32)) {
    if server.containers.viewers(pos, -1) == 0 {
        container_anim(server, pos, false);
    }
}

pub(crate) fn container_anim(server: &players::Server, (x, y, z): (i32, i32, i32), open: bool) {
    let block = server.world.get(x, y, z);
    let name = blocks::entry(block).map_or("", |e| e.name.as_str());
    let center = (x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
    match name {
        "minecraft:chest" | "minecraft:trapped_chest" | "minecraft:ender_chest" => {
            // Block event 1 = chest lid (value 1 open, 0 closed); PNX's sounds.
            let lid = Protocol::BlockEventPacket(Box::new(bedrock::protocol::v662::packets::BlockEventPacket {
                block_position: NetworkBlockPosition { x, y, z },
                event_type: 1,
                event_value: open as i32,
            }));
            let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(lid) });
            let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound(if open { "random.chestopen" } else { "random.chestclosed" }, center)) });
        }
        n if n.ends_with("shulker_box") => {
            let lid = Protocol::BlockEventPacket(Box::new(bedrock::protocol::v662::packets::BlockEventPacket {
                block_position: NetworkBlockPosition { x, y, z },
                event_type: 1,
                event_value: open as i32,
            }));
            let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(lid) });
            let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound(if open { "random.shulkerboxopen" } else { "random.shulkerboxclosed" }, center)) });
        }
        "minecraft:barrel" => {
            server.set_block_quiet(0, x, y, z, blocks::with(block, &[("open_bit", blocks::State::Bool(open))]));
            let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound(if open { "block.barrel.open" } else { "block.barrel.close" }, center)) });
        }
        _ => {}
    }
}

pub(crate) fn apply_use(server: &players::Server, id: i64, p: (i32, i32, i32), o: &useblock::Outcome) {
    let by = (id != 0).then_some(id);
    match o.sound.as_ref().map(|(s, _)| s.as_str()) {
        Some("block.bell.hit") => server.plugins.bell_ring(server, p, by),
        Some("item.bone_meal.use") => server.plugins.bonemeal(server, p, by),
        Some("respawn_anchor.charge") => server.plugins.anchor_charge(server, p, by),
        Some("block.end_portal_frame.fill") => server.plugins.end_portal_fill(server, p, by),
        Some(s) if s.starts_with("block.composter") => server.plugins.composter_use(server, p, by),
        Some("use.wood") => server.plugins.log_strip(server, p, by),
        Some("copper.wax.on") => server.plugins.copper_wax(server, p, by, true),
        Some("copper.wax.off") => server.plugins.copper_wax(server, p, by, false),
        Some("scrape") => server.plugins.copper_scrape(server, p, by),
        Some("use.grass") => server.plugins.path_create(server, p, by),
        Some("use.gravel") => server.plugins.till(server, p, by),
        Some("random.fizz") => server.plugins.campfire_extinguish(server, p, by),
        Some("block.beehive.shear") => server.plugins.honey_harvest(server, p, by),
        Some("block.pumpkin.carve") => server.plugins.pumpkin_carve(server, p, by),
        Some("block.sweet_berry_bush.pick" | "pick_berries.cave_vines") => server.plugins.berry_pick(server, p, by),
        Some(s) if s.starts_with("record.") => server.plugins.jukebox_play(server, p, s, by),
        Some(s) if s.starts_with("note.") && by.is_some() => server.plugins.note_tune(server, p, by),
        _ => {}
    }
    if o.eat.is_some() {
        server.plugins.cake_eat(server, p, by);
    }
    let here = server.world.get(p.0, p.1, p.2);
    if o.changes.len() == 2
        && o.changes[0] == (p, crate::world::AIR)
        && crate::blocks::entry(here).is_some_and(|e| e.name == "minecraft:dragon_egg")
        && !server.plugins.dragon_egg_teleport(server, p, o.changes[1].0)
    {
        return;
    }
    if o.explode.is_some() && !server.plugins.block_explode(server, p, crate::blocks::entry(here).map_or("", |e| e.name.as_str())) {
        return;
    }
    let center = (p.0 as f32 + 0.5, p.1 as f32 + 0.5, p.2 as f32 + 0.5);
    for &((x, y, z), b) in &o.changes {
        server.set_block(id, x, y, z, b);
    }
    if let Some(s) = &o.stop_sound {
        let stop = Protocol::StopSoundPacket(Box::new(bedrock::protocol::v712::packets::StopSoundPacket { sound_name: s.clone(), stop_all_sounds: false, stop_music_legacy: false }));
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(stop) });
    }
    if let Some((s, pitch)) = &o.sound {
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound_pitch(s, center, *pitch)) });
    }
    if let Some((event_type, event_value)) = o.block_event {
        let ev = Protocol::BlockEventPacket(Box::new(bedrock::protocol::v662::packets::BlockEventPacket {
            block_position: NetworkBlockPosition { x: p.0, y: p.1, z: p.2 },
            event_type,
            event_value,
        }));
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(ev) });
    }
    for &(item, n) in &o.drops {
        if let Some(it) = items::item_by_name(item) {
            server.entities.drop_item(server, it, n, (center.0, p.1 as f32 + 1.0, center.2));
        }
    }
    if let Some(power) = o.explode {
        mobs::explode(server, center, power, "Respawn Anchor");
    }
}

/// Closes whatever container a player had open; a double chest goes back as two halves.
pub(crate) fn close_open(server: &players::Server, pos: (i32, i32, i32), slots: Vec<Option<inventory::Stack>>, pair: Option<(i32, i32, i32)>) {
    store_open(server, pos, slots, pair);
    // A chest minecart's or ender chest's store isn't a block.
    if pos.1 < -50_000 {
        return;
    }
    if let Some(other) = pair {
        container_closed(server, other);
    }
    container_closed(server, pos);
}

/// Writes an open screen's slots back to the store (both halves of a double chest).
pub(crate) fn store_open(server: &players::Server, pos: (i32, i32, i32), mut slots: Vec<Option<inventory::Stack>>, pair: Option<(i32, i32, i32)>) {
    if pos.1 < -50_000 {
        server.containers.put(pos, slots);
        return;
    }
    if let Some(other) = pair
        && slots.len() > inventory::CHEST_SLOTS
    {
        let rest = slots.split_off(inventory::CHEST_SLOTS);
        server.containers.close(&server.world, other, rest);
    }
    server.containers.close(&server.world, pos, slots);
}

/// Pair data for a chest and its partner, to everyone (after placing or breaking one).
pub(crate) fn announce_chest(server: &players::Server, pos: (i32, i32, i32)) {
    let pair = containers::chest_pair(&server.world, pos);
    let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(chest_data(pos, pair)) });
    if let Some(p) = pair {
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(chest_data(p, Some(pos))) });
    }
}

/// Chest pair data for player-placed chests in these chunks (sent with the chunks).
pub(crate) fn chest_pairs_in(world: &world::World, chunks: &[(i32, i32)]) -> Vec<Protocol> {
    let mut out = Vec::new();
    for &(cx, cz) in chunks {
        for (pos, id) in world.edits_in(cx, cz) {
            if containers::kind(id) == Some(containers::Kind::Chest)
                && let Some(pair) = containers::chest_pair(world, pos)
            {
                out.push(chest_data(pos, Some(pair)));
            }
        }
    }
    out
}

/// Furnaces, smokers and blast furnaces switch to their lit block while burning.
pub(crate) fn set_lit(server: &players::Server, (x, y, z): (i32, i32, i32), lit: bool) {
    let block = server.world.get(x, y, z);
    let Some(e) = blocks::entry(block) else { return };
    let base = e.name.trim_start_matches("minecraft:").trim_start_matches("lit_");
    if !matches!(base, "furnace" | "smoker" | "blast_furnace") {
        return;
    }
    // PNX BlockEntityFurnace: a burning furnace crackles every 1-5 seconds.
    if lit && std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos()) % 3 == 0 {
        let sound = match base {
            "smoker" => "block.smoker.smoke",
            "blast_furnace" => "block.blastfurnace.fire_crackle",
            _ => "block.furnace.lit",
        };
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound(sound, (x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5))) });
    }
    let want = if lit { format!("minecraft:lit_{base}") } else { format!("minecraft:{base}") };
    if e.name == want {
        return;
    }
    let Some(mut new) = blocks::any_state(&want) else { return };
    // Keep the facing.
    for (k, v) in &e.states {
        new = blocks::with(new, &[(k.as_str(), v.clone())]);
    }
    server.set_block_quiet(0, x, y, z, new);
}

/// Where a thrown item starts and how fast it goes (PNX values).
pub(crate) fn throw_from(me: &players::PlayerInfo) -> ((f32, f32, f32), (f32, f32, f32)) {
    // PNX Player.dropItem: from feet + 1.3, motion = look direction * 0.4 (per game tick).
    let (pitch, yaw) = (me.rot.0.to_radians(), me.rot.1.to_radians());
    let dir = (-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos());
    let at = (me.pos.0, me.pos.1 - 1.62 + 1.3, me.pos.2);
    (at, (dir.0 * 0.4, dir.1 * 0.4, dir.2 * 0.4))
}

pub(crate) fn oven(kind: containers::Kind) -> Option<recipes::Oven> {
    match kind {
        containers::Kind::Furnace => Some(recipes::Oven::Furnace),
        containers::Kind::Smoker => Some(recipes::Oven::Smoker),
        containers::Kind::BlastFurnace => Some(recipes::Oven::Blast),
        _ => None,
    }
}

/// Chests spill, survival drops items (`drops` = held item), gravity blocks above fall.
pub(crate) fn break_block(server: &players::Server, id: i64, p: (i32, i32, i32), drops: Option<Option<&str>>, tool: &[u8]) {
    let old = server.world.get(p.0, p.1, p.2);
    let center = (p.0 as f32 + 0.5, p.1 as f32 + 0.25, p.2 as f32 + 0.5);
    let is_shulker = containers::kind(old) == Some(containers::Kind::Shulker);
    let contents = server.containers.remove(p);
    if is_shulker {
        // Keeps its items (PNX `BlockShulkerBox.toItem`), even in creative.
        if let Some(it) = items::item_for_block(old) {
            server.entities.drop_item(server, items::with_contents(it, &contents), 1, center);
        }
    } else {
        for s in contents.into_iter().flatten() {
            server.entities.drop_item(server, s.item, s.count, center);
        }
    }
    let drops = if is_shulker { None } else { drops };
    if let Some(held) = drops && game_rule_on(server, "doTileDrops", true) {
        let normal = drops::drops(old, p, held);
        let block_item = items::item_for_block(old);
        // Silk touch drops the block itself (when it drops anything at all with this tool).
        let silk = enchant::level(tool, enchant::SILK_TOUCH) > 0 && !normal.is_empty() && containers::kind(old).is_none();
        let fortune = enchant::level(tool, enchant::FORTUNE);
        if silk && let Some(it) = block_item.clone() {
            server.entities.drop_item(server, it, 1, center);
        } else {
            let roll = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() / 7;
            let points = xp::from_block(blocks::entry(old).map_or("", |e| e.name.as_str()), roll);
            if points > 0 {
                let _ = server.events.send(players::Event::Xp { to: id, points });
            }
            for (item, count) in normal {
                // Fortune multiplies ore drops (anything that isn't the block itself).
                let roll = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() / 13;
                let ore = items::name_of(item.id).is_some_and(|n| {
                    matches!(n, "minecraft:diamond" | "minecraft:emerald" | "minecraft:coal" | "minecraft:lapis_lazuli" | "minecraft:redstone" | "minecraft:quartz"
                        | "minecraft:raw_iron" | "minecraft:raw_gold" | "minecraft:raw_copper" | "minecraft:gold_nugget" | "minecraft:amethyst_shard")
                });
                let count = if ore { count.saturating_mul(enchant::fortune(fortune, roll)) } else { count };
                server.entities.drop_item(server, item, count, center);
            }
        }
    }
    signs::removed(p);
    if let Some((disc, rec)) = useblock::removed(p) {
        if let Some(it) = items::item_by_name(disc) {
            server.entities.drop_item(server, it, 1, center);
        }
        let stop = Protocol::StopSoundPacket(Box::new(bedrock::protocol::v712::packets::StopSoundPacket { sound_name: rec, stop_all_sounds: false, stop_music_legacy: false }));
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(stop) });
    }
    let partner = containers::chest_pair(&server.world, p);
    // Tall grass, large ferns, sunflowers and the like: the other half goes too (no extra drop).
    if blocks::has_state(old, "upper_block_bit") && !blocks::entry(old).is_some_and(|e| e.name.ends_with("_door")) {
        let top = matches!(blocks::state(old, "upper_block_bit"), Some(blocks::State::Bool(true)));
        let other = if top { (p.0, p.1 - 1, p.2) } else { (p.0, p.1 + 1, p.2) };
        let ob = server.world.get(other.0, other.1, other.2);
        if blocks::entry(ob).map(|e| &e.name) == blocks::entry(old).map(|e| &e.name) {
            server.set_block_quiet(id, other.0, other.1, other.2, world::AIR);
        }
    }
    let wet = server.world.waterlogged(p.0, p.1, p.2);
    server.set_block(id, p.0, p.1, p.2, world::AIR);
    if wet {
        // Its water stays behind as a source (and the client drops the old water layer).
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(crate::packets::update_layer(p.0, p.1, p.2, world::AIR, 1)) });
        server.set_block_quiet(id, p.0, p.1, p.2, crate::fluids::Kind::Water.source());
    }
    if let Some(o) = partner {
        announce_chest(server, o);
    }
    for ((fx, fy, fz), b) in physics::settle(&server.world, p) {
        server.set_block_quiet(id, fx, fy, fz, b);
    }
    pop_unsupported(server, id, p);
    // Infested stone hides a silverfish.
    if drops.is_some() && blocks::entry(old).is_some_and(|e| e.name.starts_with("minecraft:infested_")) {
        server.mobs.spawn(server, mobs::Kind::Silverfish, (p.0 as f32 + 0.5, p.1 as f32, p.2 as f32 + 0.5));
    }
}

/// Crops, flowers, torches, ladders... that hung on the removed block at `p` pop off with drops.
pub(crate) fn pop_unsupported(server: &players::Server, id: i64, p: (i32, i32, i32)) {
    // A nether portal falls apart when its frame (or any portal block) breaks.
    let is_portal = |q: (i32, i32, i32)| blocks::entry(server.world.get(q.0, q.1, q.2)).is_some_and(|e| e.name == "minecraft:portal");
    let mut todo: Vec<(i32, i32, i32)> = redstone::DIRS.iter().map(|d| (p.0 + d.0, p.1 + d.1, p.2 + d.2)).filter(|&q| is_portal(q)).collect();
    let mut n = 0;
    while let Some(q) = todo.pop() {
        if !is_portal(q) || n > 441 {
            continue;
        }
        n += 1;
        server.set_block(id, q.0, q.1, q.2, world::AIR);
        todo.extend(redstone::DIRS.iter().map(|d| (q.0 + d.0, q.1 + d.1, q.2 + d.2)));
    }
    for (q, b) in physics::unsupported(&server.world, p) {
        if game_rule_on(server, "doTileDrops", true) {
            for (item, count) in drops::drops(b, q, None) {
                server.entities.drop_item(server, item, count, (q.0 as f32 + 0.5, q.1 as f32 + 0.25, q.2 as f32 + 0.5));
            }
        }
        server.set_block(id, q.0, q.1, q.2, world::AIR);
    }
}

//! Helpers the player session awaits: damage, effects, respawn, dimension changes.

use super::*;

mod cmd;
mod combat;
mod events;
mod held;
mod input;
mod interact;
mod state;
mod vanilla_cmds;
mod camera_cmds;
mod windows;

pub(crate) use state::*;

pub(crate) async fn set_game_mode(conn: &mut dims::DimConn, id: i64, mode: &GameType) {
    use bedrock::protocol::v662::packets::SetPlayerGameTypePacket;
    let _ = conn
        .send(&[
            Protocol::SetPlayerGameTypePacket(Box::new(SetPlayerGameTypePacket { player_game_type: mode.clone() })),
            spawn::abilities(id, mode),
        ])
        .await;
}

/// Instant effects act right away (PNX: heal 4 << amp, harm 6 << amp), others get sent.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn give_effect(conn: &mut dims::DimConn, server: &players::Server, id: i64, name: &str, hp: &mut health::Health, food: &mut hunger::Hunger, fx: &mut effects::Effects, inv: &mut inventory::Inventory, e: i32, x: effects::Effect, mode: &GameType) {
    match e {
        effects::INSTANT_HEALTH => {
            hp.health = (hp.health + (4 << x.amplifier.min(5)) as f32).min(hp.max);
            let _ = conn.send(&[spawn::health_packet(id, hp.health)]).await;
        }
        effects::INSTANT_DAMAGE => hurt(conn, server, id, name, hp, health::Damage::Magic((6 << x.amplifier.min(5)) as f32), inv, fx, mode).await,
        effects::SATURATION => {
            food.eat((x.amplifier + 1) as f32, (x.amplifier + 1) as f32 * 2.0);
            let _ = conn.send(&[food.packet(id)]).await;
        }
        _ => {
            if fx.add(e, x) {
                let mut out = vec![effect_packet(id, e, Some(x)), movement_packet(id, fx.speed())];
                if e == effects::ABSORPTION {
                    fx.set_absorb(4.0 * (x.amplifier + 1) as f32);
                    out.push(absorption_packet(id, fx.absorb()));
                }
                let _ = conn.send(&out).await;
            }
        }
    }
}

/// Cross-dimension teleport to `eye`: resend chunks, show/hide us (PNX Player.teleport).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn change_dim(conn: &mut dims::DimConn, server: &players::Server, chunks: &mut ChunkTracker, id: i64, to: u8, eye: (f32, f32, f32), rot: (f32, f32), head: f32) -> Result<(), ConnectionError> {
    if let Some(me) = server.get(id) {
        server.plugins.dimension_change(server, &me, to);
    }
    // Others stop seeing us where we were.
    let gone = Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) }));
    let _ = server.events.send(players::Event::Relay { from: id, packet: Box::new(gone) });
    // Between the overworld and an extra world the client stays in "the overworld": no
    // ChangeDimension (it ignores one to the dimension it's in), just new chunks and a teleport.
    let same = dims::network_id(to) == dims::network_id(conn.dim);
    conn.dim = to;
    if !same {
        conn.send(&[Protocol::ChangeDimensionPacket(Box::new(bedrock::protocol::v712::packets::ChangeDimensionPacket {
            dimension_id: dims::network_id(to),
            position: eye,
            respawn: false,
            loading_screen_id: None,
        }))]).await?;
    }
    chunks.sent.clear();
    chunks.pending.clear();
    chunks.center = None;
    server.moved(id, eye, rot, head);
    let (cx, cz) = ((eye.0.floor() as i32) >> 4, (eye.2.floor() as i32) >> 4);
    // Only the nearest chunks now (the rest stream in as usual), so the loading screen
    // doesn't wait on a whole view distance of terrain.
    chunks.update(conn, &server.world, cx, cz, 25).await?;
    let mut tp = move_player(id, eye, rot, head);
    if let Protocol::MovePlayerPacket(m) = &mut tp {
        m.position_mode = PlayerPositionMode::Teleport;
        m.teleport_data = Some(MovePlayerTeleportData { teleportation_cause: 0, source_actor_type: 0 });
    }
    // Chunks are there: tell the client it can drop the loading screen (PNX CHANGE_DIMENSION_ACK).
    let at = NetworkBlockPosition { x: eye.0.floor() as i32, y: eye.1.floor() as i32, z: eye.2.floor() as i32 };
    let ack = Protocol::PlayerActionPacket(Box::new(bedrock::protocol::v662::packets::PlayerActionPacket {
        player_runtime_id: ActorRuntimeID(id as u64),
        action: bedrock::protocol::v662::enums::PlayerActionType::ChangeDimensionAck,
        block_position: at.clone(),
        result_pos: at,
        face: 0,
    }));
    if same {
        conn.send(&[tp]).await?;
    } else {
        conn.send(&[tp, ack]).await?;
    }
    // Everyone in the new dimension sees us arrive.
    let _ = server.events.send(players::Event::Join(id));
    if to == dims::END {
        endfight::entered(server);
    } else {
        ui::remove_boss_bar(server, id);
    }
    Ok(())
}

/// How much breaking a block wears the held item (PNX: tools 1, swords 2).
pub(crate) fn mining_wear(held: Option<&str>) -> i32 {
    match held {
        Some(h) if h.ends_with("_sword") || h == "minecraft:trident" => 2,
        Some(h) if h.ends_with("_pickaxe") || h.ends_with("_axe") || h.ends_with("_shovel") || h.ends_with("_hoe") || h == "minecraft:shears" => 1,
        _ => 0,
    }
}

/// How much hitting a mob wears the held item (PNX: swords 1, other tools 2).
pub(crate) fn attack_wear(held: Option<&str>) -> i32 {
    match held {
        Some(h) if h.ends_with("_sword") || h == "minecraft:trident" || h == "minecraft:mace" => 1,
        Some(h) if h.ends_with("_pickaxe") || h.ends_with("_axe") || h.ends_with("_shovel") || h.ends_with("_hoe") => 2,
        _ => 0,
    }
}

/// Wears the item in `slot` in survival; tells the client, and plays the break sound if it broke.
pub(crate) async fn wear(conn: &mut dims::DimConn, server: &players::Server, id: i64, inv: &mut inventory::Inventory, slot: usize, uses: i32, mode: &GameType) {
    if !survival_like(mode) || uses <= 0 {
        return;
    }
    let name = inv.held(slot).and_then(|s| items::name_of(s.item.id)).unwrap_or("");
    let Some(broke) = inv.wear(slot, uses) else { return };
    let _ = conn.send(&[inv.content_packet()]).await;
    if broke && let Some(me) = server.get(id) {
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound("random.break", me.pos)) });
        server.plugins.item_break(server, &me, name);
    }
}

/// Survival bucket use: the held bucket becomes `to` (creative keeps its bucket).
pub(crate) async fn swap_bucket(conn: &mut dims::DimConn, inv: &mut inventory::Inventory, slot: u32, mode: &GameType, to: &str) {
    if !survival_like(mode) {
        return;
    }
    if let Some(item) = items::item_by_name(to)
        && let Some(stack) = inv.set_slot(slot as usize, item, 1)
    {
        let item = stack.to_network();
        let _ = conn.send(&[Protocol::InventorySlotPacket(Box::new(InventorySlotPacket {
            container_id: 0, slot, container_name_data: None, storage_item: None, item,
        }))]).await;
    }
}

/// Health, red flash for everyone watching, and death if it kills.
pub(crate) async fn hurt(conn: &mut dims::DimConn, server: &players::Server, id: i64, name: &str, hp: &mut health::Health, damage: health::Damage, inv: &mut inventory::Inventory, fx: &effects::Effects, mode: &GameType) {
    // Creative and spectator players only die to /kill and the void, as in vanilla.
    if !survival_like(mode) && !matches!(damage, health::Damage::Kill | health::Damage::Void) {
        return;
    }
    use health::Damage as D;
    if fx.level(effects::FIRE_RESISTANCE).is_some() && matches!(damage, D::Lava | D::Fire) || fx.level(effects::WATER_BREATHING).is_some() && damage == D::Drown {
        return;
    }
    if let Some(me) = server.get(id) {
        let (cause, amount) = damage_cause(damage);
        if !server.plugins.player_damage(server, &me, cause, amount) {
            return;
        }
    }
    // A raised shield stops mob hits and projectiles, and wears instead (1 + damage).
    // shortcut: blocks from every side, vanilla only blocks hits from the front; needs the attacker's position in Damage.
    if let D::Mob(d, _) | D::Shot(d, _) = damage
        && let Some(slot) = inv.shield()
    {
        let broke = inv.wear(slot, 1 + d as i32) == Some(true);
        if let Some(me) = server.get(id) {
            let sound = if broke { "random.break" } else { "item.shield.block" };
            let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound(sound, me.pos)) });
        }
        let _ = conn.send(&[inv.content_packet()]).await;
        return;
    }
    let damage = match damage {
        D::Mob(d, by) => D::Mob(fx.resist(d), by),
        D::Shot(d, by) => D::Shot(fx.resist(d), by),
        D::Explosion(d, by) => D::Explosion(fx.resist(d), by),
        D::Fall(d) => D::Fall(fx.resist(d)),
        other => other,
    };
    if let health::Damage::Mob(d, _) | health::Damage::Shot(d, _) | health::Damage::Explosion(d, _) = damage {
        let mut broke = false;
        for slot in inventory::ARMOR..inventory::ARMOR + 4 {
            broke |= inv.wear(slot, ((d / 4.0) as i32).max(1)) == Some(true);
        }
        if broke && let Some(me) = server.get(id) {
            let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound("random.break", me.pos)) });
        }
        let _ = conn.send(&inv.all_packets()[1..2]).await;
    }
    let damage = after_armor(damage, inv);
    let armor: Vec<Vec<u8>> = (inventory::ARMOR..inventory::ARMOR + 4).filter_map(|i| inv.held(i).map(|s| s.item.user_data_buffer.clone())).collect();
    let armor: Vec<&[u8]> = armor.iter().map(Vec::as_slice).collect();
    let damage = match damage {
        D::Mob(d, by) => D::Mob(d * enchant::protection(&armor, enchant::Hit::Other), by),
        D::Shot(d, by) => D::Shot(d * enchant::protection(&armor, enchant::Hit::Projectile), by),
        D::Explosion(d, by) => D::Explosion(d * enchant::protection(&armor, enchant::Hit::Explosion), by),
        D::Fall(d) => D::Fall((d * enchant::protection(&armor, enchant::Hit::Fall)).round()),
        other => other,
    };
    // Absorption hearts go first (PNX `EntityHuman.applyDamage`).
    let soak = |d: f32| {
        let a = fx.absorb();
        let take = a.min(d);
        fx.set_absorb(a - take);
        d - take
    };
    let had = fx.absorb();
    let damage = if had > 0.0 {
        match damage {
            D::Mob(d, by) => D::Mob(soak(d), by),
            D::Shot(d, by) => D::Shot(soak(d), by),
            D::Explosion(d, by) => D::Explosion(soak(d), by),
            D::Magic(d) => D::Magic(soak(d)),
            D::Fall(d) => D::Fall(soak(d)),
            D::Wither(d) => D::Wither(soak(d)),
            other => other,
        }
    } else {
        damage
    };
    if had != fx.absorb() {
        let _ = conn.send(&[absorption_packet(id, fx.absorb())]).await;
    }
    // A totem of undying cheats death, except from the void and /kill (vanilla).
    let lethal = hp.health - damage_cause(damage).1 <= 0.0 && !matches!(damage, D::Void | D::Kill);
    if lethal && !hp.dead && inv.use_totem() {
        use bedrock::protocol::v975::enums::ActorEvent as E;
        hp.health = 1.0;
        for (effect, amplifier, ticks) in [(effects::REGENERATION, 1, 45 * 20), (effects::ABSORPTION, 1, 5 * 20), (effects::FIRE_RESISTANCE, 0, 40 * 20)] {
            let _ = server.events.send(players::Event::Effect { to: id, effect, amplifier, ticks });
        }
        let totem = Protocol::ActorEventPacket(Box::new(ActorEventPacket { target_runtime_id: ActorRuntimeID(id as u64), event_id: E::TalismanActivate, data: 0, fire_at_position: None }));
        // The pop animation and sound go to this player first, before the totem leaves their hand.
        let pos = server.get(id).map_or((0.0, 0.0, 0.0), |p| p.pos);
        let sound = play_sound("random.totem", pos);
        let _ = conn.send(&[totem.clone(), sound.clone()]).await;
        let _ = server.events.send(players::Event::Relay { from: id, packet: Box::new(totem) });
        let _ = server.events.send(players::Event::Relay { from: id, packet: Box::new(sound) });
        let _ = conn.send(&[spawn::health_packet(id, hp.health), inv.content_packet()]).await;
        return;
    }
    let died = hp.hurt(damage);
    let event = |e: ActorEvent| {
        Protocol::ActorEventPacket(Box::new(ActorEventPacket { target_runtime_id: ActorRuntimeID(id as u64), event_id: e, data: 0, fire_at_position: None }))
    };
    let mut out = vec![spawn::health_packet(id, hp.health), event(ActorEvent::Hurt)];
    let _ = server.events.send(players::Event::Relay { from: id, packet: Box::new(event(if died { ActorEvent::Death } else { ActorEvent::Hurt })) });
    if died {
        out.push(event(ActorEvent::Death));
        // Death screen: tell the client where it will respawn.
        out.push(Protocol::RespawnPacket(Box::new(RespawnPacket {
            position: commands::spawn_of(name, server),
            state: PlayerRespawnState::SearchingForSpawn,
            player_runtime_id: ActorRuntimeID(id as u64),
        })));
        let mut msg = health::death_message(name, damage);
        if let Some(me) = server.get(id) {
            server.plugins.player_death(server, &me, &mut msg);
        }
        if !msg.is_empty() {
            server.broadcast(&msg);
        }
        if !game_rule_on(server, "keepInventory", false) {
            let at = server.get(id).map_or(server.world.spawn(), |p| (p.pos.0, p.pos.1 - 1.0, p.pos.2));
            for s in inv.take_all() {
                // Curse of Vanishing: gone instead of dropped.
                if enchant::level(&s.item.user_data_buffer, enchant::VANISHING) > 0 {
                    continue;
                }
                server.entities.drop_item(server, s.item, s.count, at);
            }
            out.extend(inv.all_packets());
        }
    }
    let _ = conn.send(&out).await;
}

/// A damage's cause name and amount, for plugins.
pub(crate) fn damage_cause(d: health::Damage) -> (&'static str, f32) {
    use health::Damage as D;
    match d {
        D::Fall(n) => ("fall", n),
        D::Void => ("void", 1000.0),
        D::Kill => ("kill", 1000.0),
        D::Starve => ("starve", 1.0),
        D::Drown => ("drown", 2.0),
        D::Lava => ("lava", 4.0),
        D::Fire => ("fire", 1.0),
        D::Mob(n, by) | D::Shot(n, by) | D::Explosion(n, by) => (by, n),
        D::Magic(n) => ("magic", n),
        D::Wither(n) => ("wither", n),
    }
}

/// Respawn at world spawn, in PNX order: spawn pos, attributes, data, abilities, inventory, teleport.
pub(crate) async fn respawn(
    conn: &mut dims::DimConn,
    server: &players::Server,
    id: i64,
    name: &str,
    hp: &mut health::Health,
    mode: &GameType,
    inv: &inventory::Inventory,
) {
    hp.respawn();
    let at = commands::spawn_of(name, server);
    let mut out = vec![
        Protocol::RespawnPacket(Box::new(RespawnPacket { position: at, state: PlayerRespawnState::ReadyToSpawn, player_runtime_id: ActorRuntimeID(id as u64) })),
        spawn::attributes(id),
        spawn::set_data(id, name),
        spawn::abilities(id, mode),
    ];
    out.extend(inv.all_packets());
    let (rot, head) = server.get(id).map_or(((0.0, 0.0), 0.0), |p| (p.rot, p.head_yaw));
    let mut tp = move_player(id, at, rot, head);
    if let Protocol::MovePlayerPacket(m) = &mut tp {
        m.position_mode = PlayerPositionMode::Respawn;
    }
    out.push(tp);
    let _ = conn.send(&out).await;
    server.moved(id, at, rot, head);
}

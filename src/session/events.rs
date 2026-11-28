//! The once-a-second tick and server events meant for this player.

use super::*;
use players::Event;

impl Session {
    /// Spawns other players in view this client doesn't have, and forgets the ones out of view
    /// (the client drops them with their chunks), so they reappear after far teleports.
    pub async fn show_players(&mut self) {
        let Some(me) = self.server.get(self.id) else { return };
        let range = (self.chunks.radius as f32 * 16.0 - 8.0).max(32.0);
        let mut out = vec![];
        let gone = |id: i64| Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) }));
        // A teleport: the client threw away what was in the old chunks, so send it all again.
        let jumped = self.last_view.replace(me.pos).is_some_and(|l| (l.0 - me.pos.0).powi(2) + (l.2 - me.pos.2).powi(2) > 64.0 * 64.0 || (l.1 - me.pos.1).abs() > 256.0);
        if jumped {
            out.extend(self.seen.drain().map(gone));
            for p in self.server.entities.spawn_packets().into_iter().chain(self.server.mobs.spawn_packets()) {
                let id = match &p {
                    Protocol::AddItemActorPacket(a) => Some(a.target_actor_id.0),
                    Protocol::AddActorPacket(a) => Some(a.target_actor_id.0),
                    _ => None,
                };
                out.extend(id.map(gone));
                out.push(p);
            }
        }
        for p in self.server.all() {
            if p.id == self.id {
                continue;
            }
            let (dx, dy, dz) = (p.pos.0 - me.pos.0, p.pos.1 - me.pos.1, p.pos.2 - me.pos.2);
            let near = dx * dx + dz * dz < range * range && dy.abs() < 512.0;
            if near && self.seen.insert(p.id) {
                out.push(add_player(&p));
            } else if !near && self.seen.remove(&p.id) {
                out.push(gone(p.id));
            }
        }
        if !out.is_empty() {
            let _ = self.conn.send(&out).await;
        }
    }

    pub async fn every_second(&mut self) {
        self.regen_ticks += 1;
        {
            // XP level changes come from many places; check once a second.
            static LEVELS: parking_lot::Mutex<Option<std::collections::HashMap<i64, i32>>> = parking_lot::Mutex::new(None);
            let old = LEVELS.lock().get_or_insert_with(Default::default).insert(self.id, self.xp.level);
            if let Some(old) = old
                && old != self.xp.level
                && let Some(me) = self.server.get(self.id)
            {
                self.server.plugins.xp_level_change(&self.server, &me, old, self.xp.level);
            }
        }
        self.show_players().await;
        {
            let armor = std::array::from_fn(|i| self.inv.held(inventory::ARMOR + i).cloned());
            let max = health::max_of(self.id);
            if max != self.hp.max {
                self.hp.max = max;
                self.hp.health = self.hp.health.min(max);
                let _ = self.conn.send(&[spawn::health_packet(self.id, self.hp.health)]).await;
            }
            let inventory = (0..inventory::MAIN).map(|i| self.inv.held(i).cloned()).collect();
            let eq = players::Equipment { held: self.inv.held(self.selected_slot as usize).cloned(), armor, inventory };
            let name = |s: &Option<inventory::Stack>| s.as_ref().and_then(|s| items::name_of(s.item.id)).unwrap_or("").to_string();
            let old = players::EQUIPMENT.lock().as_ref().and_then(|e| e.get(&self.id)).map(|e| e.armor.each_ref().map(name));
            let now = eq.armor.each_ref().map(name);
            players::EQUIPMENT.lock().get_or_insert_with(Default::default).insert(self.id, eq);
            if let Some(old) = old
                && old != now
                && let Some(me) = self.server.get(self.id)
            {
                for (i, (a, b)) in old.iter().zip(now.iter()).enumerate() {
                    if a != b {
                        self.server.plugins.armor_change(&self.server, &me, i as i32, b);
                    }
                }
            }
        }
        self.idle += 1;
        let limit = config::get().player_idle_timeout * 60;
        if limit > 0 && self.idle >= limit && !commands::is_op(&self.name) && self.server.get(self.id).is_none_or(|me| self.server.plugins.idle_kick(&self.server, &me)) {
            self.server.kick(self.id, "You were idle for too long");
        }
        // An elytra wears 1 durability per second of gliding (vanilla).
        if self.gliding && survival_like(&self.mode) {
            wear(&mut self.conn, &self.server, self.id, &mut self.inv, inventory::ARMOR + 1, 1, &self.mode).await;
        }
        let rule_on = !self.server.game_rules.lock().iter().any(|r| {
            r.rule_name == "naturalRegeneration" && matches!(r.rule_type, bedrock::protocol::v844::types::Type::Bool(false))
        });
        let _ = self.regen_ticks;
        // Furnaces smelt live while open: progress arrow and flame update every second.
        if let (Some(pos), Some((kind, cname)), Some(slots)) = (self.open_container, self.open_kind.clone(), self.inv.open.as_mut())
            && let Some(o) = oven(kind)
        {
            let (cook, lit, total) = self.server.containers.tick_open(o, pos, slots, 1.0);
            set_lit(&self.server, pos, lit > 0);
            let set = |id: i32, value: i32| Protocol::ContainerSetDataPacket(Box::new(bedrock::protocol::v662::packets::ContainerSetDataPacket {
                container_id: ContainerID(self.window_id),
                id,
                value,
            }));
            let mut out: Vec<Protocol> = self.inv.open_packet(self.window_id as u32, cname).into_iter().collect();
            out.extend([set(0, cook), set(1, lit), set(2, total)]);
            let _ = self.conn.send(&out).await;
        }
        // Brewing stands brew live while open: bubbles and fuel bar.
        if let (Some(pos), Some((containers::Kind::Brewing, cname)), Some(slots)) = (self.open_container, self.open_kind.clone(), self.inv.open.as_mut()) {
            let ((ticks, fuel, total), done) = self.server.containers.tick_open_brew(pos, slots);
            let set = |id: i32, value: i32| Protocol::ContainerSetDataPacket(Box::new(bedrock::protocol::v662::packets::ContainerSetDataPacket {
                container_id: ContainerID(self.window_id),
                id,
                value,
            }));
            let mut out: Vec<Protocol> = self.inv.open_packet(self.window_id as u32, cname).into_iter().collect();
            out.extend([set(0, ticks), set(1, fuel), set(2, total)]);
            if done {
                self.server.plugins.brew(&self.server, pos);
                out.push(play_sound("random.potion.brewed", (pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5)));
            }
            let _ = self.conn.send(&out).await;
        }
        // /effect for us, then a second of effects.
        let queued: Vec<effects::Queued> = {
            let mut q = effects::QUEUE.lock();
            let (mine, rest): (Vec<_>, Vec<_>) = q.drain(..).partition(|(who, _)| *who == self.id);
            *q = rest;
            mine.into_iter().map(|(_, c)| c).collect()
        };
        let enchants: Vec<(i16, i16)> = {
            let mut q = enchant::QUEUE.lock();
            let (mine, rest): (Vec<_>, Vec<_>) = q.drain(..).partition(|(who, ..)| *who == self.id);
            *q = rest;
            mine.into_iter().map(|(_, e, l)| (e, l)).collect()
        };
        for (e, l) in enchants {
            let slot = self.selected_slot as usize;
            let msg = match self.inv.slots.get_mut(slot).and_then(Option::as_mut) {
                Some(st) if items::name_of(st.item.id).is_some_and(|n| enchant::applies(e, n)) => {
                    st.item.user_data_buffer = enchant::add(&st.item.user_data_buffer, e, l);
                    "Your held item was enchanted".to_string()
                }
                Some(_) => "§cThat enchantment can't go on this item".to_string(),
                None => "§cYou aren't holding anything to enchant".to_string(),
            };
            let _ = self.conn.send(&[self.inv.content_packet(), chat_packet(String::new(), msg)]).await;
        }
        for c in queued {
            match c {
                effects::Queued::Give(e, x) => give_effect(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, &mut self.food, &mut self.effects, &mut self.inv, e, x, &self.mode).await,
                effects::Queued::Clear => {
                    let all: Vec<i32> = self.effects.active.keys().copied().collect();
                    self.effects.active.clear();
                    let mut out: Vec<Protocol> = all.into_iter().map(|e| effect_packet(self.id, e, None)).collect();
                    out.push(movement_packet(self.id, self.effects.speed()));
                    let _ = self.conn.send(&out).await;
                }
            }
        }
        if self.effects.level(effects::BAD_OMEN).is_some()
            && let Some(me) = self.server.get(self.id)
            && dims::of_f(me.pos.1) == dims::OVERWORLD
            && let Some(center) = raids::village_near(&self.server, (me.pos.0, me.pos.1 - 1.62, me.pos.2))
            && raids::start(&self.server, center)
        {
            self.effects.active.remove(&effects::BAD_OMEN);
            let _ = self.conn.send(&[effect_packet(self.id, effects::BAD_OMEN, None)]).await;
        }
        if !self.hp.dead && !self.effects.active.is_empty() {
            let t = self.effects.second();
            if t.heal > 0.0 && self.hp.health < self.hp.max {
                self.hp.health = (self.hp.health + t.heal).min(self.hp.max);
                let _ = self.conn.send(&[spawn::health_packet(self.id, self.hp.health)]).await;
            }
            if t.poison > 0.0 && self.hp.health > 1.0 && survival_like(&self.mode) {
                let d = t.poison.min(self.hp.health - 1.0); // poison never kills
                hurt(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, health::Damage::Magic(d), &mut self.inv, &self.effects, &self.mode).await;
            }
            if t.wither > 0.0 && survival_like(&self.mode) {
                hurt(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, health::Damage::Wither(t.wither), &mut self.inv, &self.effects, &self.mode).await;
            }
            if t.exhaustion > 0.0 && survival_like(&self.mode) && self.food.exhaust(t.exhaustion) {
                let _ = self.conn.send(&[self.food.packet(self.id)]).await;
            }
            if t.expired.contains(&effects::ABSORPTION) {
                self.effects.set_absorb(0.0);
                let _ = self.conn.send(&[absorption_packet(self.id, 0.0)]).await;
            }
            if !t.expired.is_empty() {
                if let Some(me) = self.server.get(self.id) {
                    for &e in &t.expired {
                        self.server.plugins.effect_remove(&self.server, &me, e);
                    }
                }
                let mut out: Vec<Protocol> = t.expired.iter().map(|&e| effect_packet(self.id, e, None)).collect();
                out.push(movement_packet(self.id, self.effects.speed()));
                let _ = self.conn.send(&out).await;
            }
        }
        // Drowning: 15s of air, then 2 damage a second. Lava 4/s, fire 1/s.
        if survival_like(&self.mode) && !self.hp.dead {
            let was = self.air;
            // Respiration: each level makes air last one more time as long.
            let resp = self.inv.held(inventory::ARMOR).map_or(0, |s| enchant::level(&s.item.user_data_buffer, enchant::RESPIRATION));
            self.air = if self.head_in_water { (self.air - 20 / (resp as i16 + 1)).max(-20) } else { 300 };
            if self.air != was {
                let _ = self.conn.send(&[spawn::air_packet(self.id, self.air.max(0))]).await;
            }
            if self.air <= -20 {
                self.air = 0;
                hurt(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, health::Damage::Drown, &mut self.inv, &self.effects, &self.mode).await;
            }
            if self.in_lava && !self.hp.dead {
                hurt(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, health::Damage::Lava(4.0), &mut self.inv, &self.effects, &self.mode).await;
            } else if self.in_fire && !self.hp.dead {
                hurt(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, health::Damage::Fire(1.0), &mut self.inv, &self.effects, &self.mode).await;
            }
            // Burning: lava and fire leave you alight a while (vanilla 15 s / 8 s); water puts it out.
            let was = self.burning > 0;
            if self.in_lava {
                self.burning = self.burning.max(15);
            } else if self.in_fire {
                self.burning = self.burning.max(8);
            }
            if !was && self.burning > 0
                && let Some(me) = self.server.get(self.id)
            {
                self.server.plugins.player_combust(&self.server, &me);
            }
            if self.head_in_water || self.hp.dead || self.effects.level(effects::FIRE_RESISTANCE).is_some() {
                self.burning = 0;
            } else if self.burning > 0 {
                self.burning -= 1;
                if !self.in_lava && !self.in_fire {
                    hurt(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, health::Damage::Fire(1.0), &mut self.inv, &self.effects, &self.mode).await;
                }
            }
            if was != (self.burning > 0) {
                self.update_pose().await;
            }
        }
        if survival_like(&self.mode) && !self.hp.dead {
            let before = (self.food.food, self.food.saturation, self.food.exhaustion);
            let difficulty = *commands::DIFFICULTY.lock();
            match self.food.tick(difficulty, self.hp.health, rule_on) {
                hunger::Step::Heal
                    if self.server.get(self.id).is_none_or(|me| self.server.plugins.health_regain(&self.server, &me, self.hp.health)) && self.hp.regenerate() =>
                {
                    let _ = self.conn.send(&[spawn::health_packet(self.id, self.hp.health)]).await;
                }
                hunger::Step::Starve => hurt(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, health::Damage::Starve, &mut self.inv, &self.effects, &self.mode).await,
                _ => {}
            }
            if self.food.food as i32 != before.0 as i32
                && let Some(me) = self.server.get(self.id)
            {
                self.server.plugins.food_change(&self.server, &me, self.food.food as i32);
            }
            if (self.food.food, self.food.saturation, self.food.exhaustion) != before {
                let _ = self.conn.send(&[self.food.packet(self.id)]).await;
            }
        }
    }

    pub async fn on_event(&mut self, event: Result<Event, tokio::sync::broadcast::error::RecvError>) -> Flow {
        let packets = match event {
            Ok(Event::Chat { from, message }) => vec![chat_packet(from, message)],
            Ok(Event::Tell { to, message }) if to == self.id => vec![chat_packet(String::new(), message)],
            Ok(Event::Relay { from, packet }) if from != self.id => vec![*packet],
            Ok(Event::SaveAll) => {
                if let Some(me) = self.server.get(self.id) {
                    save_player(&self.name, me.pos, &self.inv);
                    save_state(&self.name, &self.hp, &self.food, self.air, self.burning, self.selected_slot, &self.effects);
                }
                return GO;
            }
            Ok(Event::Damage { to, damage, knock }) if to == self.id => {
                if survival_like(&self.mode) && !self.hp.dead {
                    hurt(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, damage, &mut self.inv, &self.effects, &self.mode).await;
                    // Each netherite armour piece resists 10% of knockback (PNX knockBack).
                    let resist = (0..4).filter(|i| self.inv.held(inventory::ARMOR + i).and_then(|s| items::name_of(s.item.id)).is_some_and(|n| n.contains("netherite"))).count() as f32 * 0.1;
                    let knock = (knock.0 * (1.0 - resist), knock.1 * (1.0 - resist), knock.2 * (1.0 - resist));
                    let motion = Protocol::SetActorMotionPacket(Box::new(bedrock::protocol::v662::packets::SetActorMotionPacket {
                        target_runtime_id: ActorRuntimeID(self.id as u64),
                        motion: knock,
                        server_tick: 0,
                    }));
                    let _ = self.conn.send(&[motion]).await;
                }
                return GO;
            }
            Ok(Event::Effect { to, effect, amplifier, ticks }) if to == self.id => {
                if survival_like(&self.mode) && !self.hp.dead {
                    let x = effects::Effect { amplifier, ticks, particles: true };
                    give_effect(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, &mut self.food, &mut self.effects, &mut self.inv, effect, x, &self.mode).await;
                }
                return GO;
            }
            Ok(Event::Packet { to, packet }) if to == self.id => vec![*packet],
            Ok(Event::Give { to, item }) if to == self.id => {
                let left = self.inv.add(item.item.clone(), item.count);
                if left > 0 && let Some(me) = self.server.get(self.id) {
                    self.server.entities.drop_item(&self.server, item.item, left, (me.pos.0, me.pos.1 - 1.0, me.pos.2));
                }
                vec![self.inv.content_packet()]
            }
            Ok(Event::Take { to, what, then }) if to == self.id => {
                let got = match what {
                    players::TakeWhat::Held(n) => {
                        let slot = self.selected_slot as usize;
                        self.inv.held(slot).cloned().map(|st| {
                            let n = n.clamp(1, st.count);
                            for _ in 0..n {
                                self.inv.use_one(slot);
                            }
                            (st, n as u32)
                        })
                    }
                    players::TakeWhat::Item(name, n) => self.inv.take_count(&name, n),
                };
                if let Some(f) = then.0.lock().take() {
                    f(&self.server, got);
                }
                vec![self.inv.content_packet()]
            }
            Ok(Event::Ignite { to, secs }) if to == self.id => {
                if survival_like(&self.mode) && self.effects.level(effects::FIRE_RESISTANCE).is_none() {
                    let was = self.burning > 0;
                    self.burning = self.burning.max(secs);
                    if !was {
                        self.update_pose().await;
                    }
                }
                return GO;
            }
            Ok(Event::SignInput { to }) if to == self.id => {
                let Some(me) = self.server.get(self.id) else { return GO };
                let at = (me.pos.0.floor() as i32, (me.pos.1 - 1.62).floor() as i32 - 2, me.pos.2.floor() as i32);
                let real = self.server.world.get(at.0, at.1, at.2);
                let (Some(text), Some(sign)) = (ui::sign_input_open(self.id, at, real), blocks::any_state("minecraft:standing_sign")) else { return GO };
                let _ = self.conn.send(&[update_block(at.0, at.1, at.2, sign), signs::data_packet(at, &text, "")]).await;
                // The client needs the sign in place before it opens the editor.
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                let _ = self.conn.send(&[signs::open_packet(at)]).await;
                return GO;
            }
            Ok(Event::Menu { to }) if to == self.id => {
                self.open_menu().await;
                return GO;
            }
            Ok(Event::RunAs { to, line, reply_to }) if to == self.id => {
                let _ = self.cmd_tx.send((line, Some(reply_to)));
                return GO;
            }
            Ok(Event::Xp { to, points }) if to == self.id => {
                let mut points = points;
                if let Some(me) = self.server.get(self.id) {
                    self.server.plugins.xp_change(&self.server, &me, &mut points);
                }
                if points > 0 && survival_like(&self.mode) {
                    let left = self.inv.mend(points, self.selected_slot as usize);
                    if left != points {
                        if let Some(me) = self.server.get(self.id) {
                            self.server.plugins.mending_repair(&self.server, &me, points - left);
                        }
                        let _ = self.conn.send(&[self.inv.content_packet()]).await;
                    }
                    points = left;
                }
                if points == 0 {
                    return GO;
                }
                self.xp.add_points(points);
                save_xp(&self.name, self.xp);
                let mut out = vec![self.xp.packet(self.id)];
                if points > 0 && let Some(me) = self.server.get(self.id) {
                    out.push(play_sound("random.orb", me.pos));
                }
                let _ = self.conn.send(&out).await;
                return GO;
            }
            Ok(Event::Teleport { id: who, feet }) if who == self.id => {
                if let Some(me) = self.server.get(self.id) {
                    self.server.plugins.teleport(&self.server, &me, feet);
                }
                let eye = (feet.0, feet.1 + 1.62, feet.2);
                let (rot, head) = self.server.get(self.id).map_or(((0.0, 0.0), 0.0), |p| (p.rot, p.head_yaw));
                // Into another dimension or world: the full switch (chunks, others seeing us).
                let to = dims::of_f(feet.1);
                if to != self.conn.dim {
                    let from = self.conn.dim;
                    if change_dim(&mut self.conn, &self.server, &mut self.chunks, self.id, to, eye, rot, head).await.is_err() {
                        return END;
                    }
                    // Each extra world keeps its own inventory.
                    if crate::multiworld::swap_inventory(&self.name, &mut self.inv, from, to) {
                        let _ = self.conn.send(&self.inv.all_packets()).await;
                    }
                    return GO;
                }
                let mut tp = move_player(self.id, eye, rot, head);
                if let Protocol::MovePlayerPacket(m) = &mut tp {
                    m.position_mode = PlayerPositionMode::Teleport;
                    m.teleport_data = Some(MovePlayerTeleportData { teleportation_cause: 2, source_actor_type: 0 });
                }
                let _ = self.conn.send(&[tp]).await;
                self.server.moved(self.id, eye, rot, head);
                return GO;
            }
            Ok(Event::Kick { id: who, message, reason }) if who == self.id || who == 0 => {
                if let Some(me) = self.server.get(self.id) {
                    self.server.plugins.kick(&self.server, &me, &message);
                }
                let _ = self.conn.send(&[disconnect_packet(reason, &message)]).await;
                println!("{}: kicked ({reason:?}): {message}", self.name);
                return END;
            }
            Ok(Event::Block { x, y, z, id: block, old, by: _, effects }) => {
                let center = (x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                let mut packets = vec![update_block(x, y, z, block)];
                // The acting client shows nothing itself, so everyone gets the effects:
                // breaking = event 2001 (particles + sound), placing = "place" sound.
                if effects && block == world::AIR {
                    packets.push(Protocol::LevelEventPacket(Box::new(LevelEventPacket { event_id: 2001, position: center, data: old })));
                } else if effects {
                    packets.push(Protocol::LevelSoundEventPacket(Box::new(LevelSoundEventPacket {
                        event_name: "place".into(),
                        position: center,
                        data: block,
                        actor_identifier: String::new(),
                        is_baby_mob: false,
                        is_global: false,
                        entity_unique_id: u64::MAX, // -1: no entity
                        fire_at_position: None,
                    })));
                }
                packets
            }
            Ok(Event::Container { pos, by }) if by != self.id => {
                self.reload_container(pos).await;
                return GO;
            }
            Ok(Event::Join(other)) if other != self.id => match self.server.get(other) {
                Some(p) => {
                    self.seen.insert(other);
                    vec![player_list_add(&p), add_player(&p)]
                }
                None => return GO,
            },
            Ok(Event::Move { id: other, pos, rot, head_yaw }) if other != self.id => {
                vec![move_player(other, pos, rot, head_yaw)]
            }
            Ok(Event::Leave { id: other, uuid }) if other != self.id => {
                self.seen.remove(&other);
                vec![
                Protocol::RemoveActorPacket(Box::new(RemoveActorPacket {
                    target_actor_id: ActorUniqueID(other),
                })),
                Protocol::PlayerListPacket(Box::new(PlayerListPacket {
                    entries: vec![PlayerListEntry::Remove { uuid }],
                })),
            ]
            }
            // Fell behind the event stream; positions catch up on the next move.
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => return GO,
            Err(_) => return END,
            _ => return GO,
        };
        if let Err(e) = self.conn.send(&packets).await {
            println!("{}: send failed: {e}", self.name);
            return END;
        }
        GO
    }
}

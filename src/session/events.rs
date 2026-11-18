//! The once-a-second tick and server events meant for this player.

use super::*;
use players::Event;

impl Session {
    pub async fn every_second(&mut self) {
        self.regen_ticks += 1;
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
                effects::Queued::Give(e, x) => give_effect(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, &mut self.food, &mut self.effects, &mut self.inv, e, x).await,
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
            if t.heal > 0.0 && self.hp.health < health::MAX_HEALTH {
                self.hp.health = (self.hp.health + t.heal).min(health::MAX_HEALTH);
                let _ = self.conn.send(&[spawn::health_packet(self.id, self.hp.health)]).await;
            }
            if t.poison > 0.0 && self.hp.health > 1.0 && survival_like(&self.mode) {
                let d = t.poison.min(self.hp.health - 1.0); // poison never kills
                hurt(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, health::Damage::Magic(d), &mut self.inv, &self.effects).await;
            }
            if t.wither > 0.0 && survival_like(&self.mode) {
                hurt(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, health::Damage::Wither(t.wither), &mut self.inv, &self.effects).await;
            }
            if t.exhaustion > 0.0 && survival_like(&self.mode) && self.food.exhaust(t.exhaustion) {
                let _ = self.conn.send(&[self.food.packet(self.id)]).await;
            }
            if t.expired.contains(&effects::ABSORPTION) {
                self.effects.set_absorb(0.0);
                let _ = self.conn.send(&[absorption_packet(self.id, 0.0)]).await;
            }
            if !t.expired.is_empty() {
                let mut out: Vec<Protocol> = t.expired.iter().map(|&e| effect_packet(self.id, e, None)).collect();
                out.push(movement_packet(self.id, self.effects.speed()));
                let _ = self.conn.send(&out).await;
            }
        }
        // Drowning: 15s of air, then 2 damage a second. Lava 4/s, fire 1/s.
        if survival_like(&self.mode) && !self.hp.dead {
            let was = self.air;
            self.air = if self.head_in_water { (self.air - 20).max(-20) } else { 300 };
            if self.air != was {
                let _ = self.conn.send(&[spawn::air_packet(self.id, self.air.max(0))]).await;
            }
            if self.air <= -20 {
                self.air = 0;
                hurt(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, health::Damage::Drown, &mut self.inv, &self.effects).await;
            }
            if self.in_lava && !self.hp.dead {
                hurt(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, health::Damage::Lava, &mut self.inv, &self.effects).await;
            } else if self.in_fire && !self.hp.dead {
                hurt(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, health::Damage::Fire, &mut self.inv, &self.effects).await;
            }
        }
        if survival_like(&self.mode) && !self.hp.dead {
            let before = (self.food.food, self.food.saturation, self.food.exhaustion);
            let difficulty = *commands::DIFFICULTY.lock();
            match self.food.tick(difficulty, self.hp.health, rule_on) {
                hunger::Step::Heal if self.hp.regenerate() => {
                    let _ = self.conn.send(&[spawn::health_packet(self.id, self.hp.health)]).await;
                }
                hunger::Step::Starve => hurt(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, health::Damage::Starve, &mut self.inv, &self.effects).await,
                _ => {}
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
                }
                return GO;
            }
            Ok(Event::Damage { to, damage, knock }) if to == self.id => {
                if survival_like(&self.mode) && !self.hp.dead {
                    hurt(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, damage, &mut self.inv, &self.effects).await;
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
                    give_effect(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, &mut self.food, &mut self.effects, &mut self.inv, effect, x).await;
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
            Ok(Event::Join(other)) if other != self.id => match self.server.get(other) {
                Some(p) => vec![player_list_add(&p), add_player(&p)],
                None => return GO,
            },
            Ok(Event::Move { id: other, pos, rot, head_yaw }) if other != self.id => {
                vec![move_player(other, pos, rot, head_yaw)]
            }
            Ok(Event::Leave { id: other, uuid }) if other != self.id => vec![
                Protocol::RemoveActorPacket(Box::new(RemoveActorPacket {
                    target_actor_id: ActorUniqueID(other),
                })),
                Protocol::PlayerListPacket(Box::new(PlayerListPacket {
                    entries: vec![PlayerListEntry::Remove { uuid }],
                })),
            ],
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

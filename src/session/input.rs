//! Movement, view distance and respawning.

use super::*;
use bedrock::protocol::ProtoVersionPackets;

type ChunkRadius = <Protocol as ProtoVersionPackets>::RequestChunkRadiusPacket;
type AuthInput = <Protocol as ProtoVersionPackets>::PlayerAuthInputPacket;

impl Session {
    pub(super) async fn on_chunk_radius(&mut self, r: &ChunkRadius) -> Flow {
        self.chunks.radius = r.chunk_radius.clamp(2, config::get().view_distance);
        let _ = self.conn.send(&[Protocol::ChunkRadiusUpdatedPacket(Box::new(
            ChunkRadiusUpdatedPacket { chunk_radius: self.chunks.radius },
        ))]).await;
        // Send the newly visible chunks now.
        if let Some((cx, cz)) = self.chunks.center.take()
            && self.chunks.update(&mut self.conn, &self.server.world, cx, cz, CHUNK_BATCH).await.is_err()
        {
            return END;
        }
        GO
    }

    pub(super) async fn on_input(&mut self, input: &AuthInput) -> Flow {
        if input.move_vector.0.abs() + input.move_vector.1.abs() > 0.01 || self.server.get(self.id).is_some_and(|me| me.rot != input.player_rotation) {
            self.idle = 0;
        }
        if let Some(actions) = &input.player_block_actions {
            self.on_block_actions(actions).await;
        }
        self.mining_tick().await;
        use bedrock::protocol::v2168::enums::PlayerAuthInputData as Input;
        // The held Sneaking flag, not Start/StopSneaking: those can arrive after the click
        // they belong to, and then sneak-placing on a chest opened it.
        let sneak = input.input_data.iter().any(|f| matches!(f, Input::Sneaking));
        if sneak != self.sneaking {
            self.sneaking = sneak;
            if let Some(me) = self.server.get(self.id) {
                self.server.plugins.sneak(&self.server, &me, sneak);
            }
            self.inv.sneaking = sneak;
            self.raise_shield();
            if sneak {
                self.get_off();
            }
        }
        let (swim, crawl) = (self.swimming, self.crawling);
        let glide = self.gliding;
        for flag in &input.input_data {
            match flag {
                Input::StartSwimming => self.swimming = true,
                Input::StopSwimming => self.swimming = false,
                Input::StartCrawling => self.crawling = true,
                Input::StopCrawling => self.crawling = false,
                Input::StartFlying | Input::StopFlying => {
                    self.flying = matches!(flag, Input::StartFlying);
                    if let Some(me) = self.server.get(self.id) {
                        self.server.plugins.toggle_flight(&self.server, &me, self.flying);
                    }
                }
                Input::StartGliding => self.gliding = true,
                Input::StopGliding => self.gliding = false,
                Input::StartSprinting | Input::StopSprinting => {
                    self.sprinting = matches!(flag, Input::StartSprinting);
                    if let Some(me) = self.server.get(self.id) {
                        self.server.plugins.sprint(&self.server, &me, self.sprinting);
                    }
                }
                Input::StartJumping => {
                    if let Some(me) = self.server.get(self.id) {
                        self.server.plugins.jump(&self.server, &me);
                    }
                    if survival_like(&self.mode) && self.food.exhaust(if self.sprinting { 0.2 } else { 0.05 }) {
                        let _ = self.conn.send(&[self.food.packet(self.id)]).await;
                    }
                }
                _ => {}
            }
        }
        if (swim, crawl) != (self.swimming, self.crawling) {
            self.update_pose().await;
        }
        if let Some(me) = self.server.get(self.id) {
            if swim != self.swimming {
                self.server.plugins.swim(&self.server, &me, self.swimming);
            }
            if glide != self.gliding {
                self.server.plugins.glide(&self.server, &me, self.gliding);
            }
        }
        let pos = input.player_position;
        // Plugins can stop a move (regions, frozen players): send them back to where they were.
        if let (Some(last), Some(me)) = (self.last_pos, self.server.get(self.id))
            && last != pos
            && !self.server.plugins.moved(&self.server, &me, pos)
        {
            let _ = self.server.events.send(players::Event::Teleport { id: self.id, feet: (last.0, last.1 - 1.62, last.2) });
            return GO;
        }
        let feet = (pos.0.floor() as i32, (pos.1 - 1.62).floor() as i32, pos.2.floor() as i32);
        // Gliding resets the fall, so landing from a glide doesn't hurt.
        let takes_damage = matches!(self.mode, GameType::Survival | GameType::Adventure) && !self.flying && !self.gliding;
        let in_water = self.server.world.get(feet.0, feet.1, feet.2) == terrain::ids().water;
        let head = self.server.world.get(pos.0.floor() as i32, pos.1.floor() as i32, pos.2.floor() as i32);
        self.head_in_water = blocks::entry(head).is_some_and(|e| e.name.ends_with("water"));
        let body = blocks::entry(self.server.world.get(feet.0, feet.1, feet.2)).map_or("", |e| e.name.as_str());
        self.in_lava = body.ends_with("lava");
        self.in_fire = body == "minecraft:fire";
        if let Some(damage) = self.hp.on_move(pos.1 - 1.62, takes_damage, in_water) {
            hurt(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, damage, &mut self.inv, &self.effects, &self.mode).await;
        }
        // PNX Entity.fall: landing on farmland from more than 0.75 blocks tramples it to dirt.
        if std::mem::take(&mut self.hp.landed) > 0.75 {
            let under = (feet.0, (pos.1 - 1.62 - 0.1).floor() as i32, feet.2);
            if blocks::entry(self.server.world.get(under.0, under.1, under.2)).is_some_and(|e| e.name == "minecraft:farmland")
                && let Some(dirt) = blocks::any_state("minecraft:dirt")
            {
                self.server.set_block(self.id, under.0, under.1, under.2, dirt);
                crate::pop_unsupported(&self.server, self.id, under);
            }
        }
        // Frost Walker: still water under the player's feet freezes for a while.
        let fw = self.inv.held(inventory::ARMOR + 3).map_or(0, |s| enchant::level(&s.item.user_data_buffer, enchant::FROST_WALKER));
        if fw > 0 && !self.flying && self.last_pos.is_some_and(|l| (pos.1 - l.1).abs() < 0.01) {
            let r = 2 + fw as i32;
            let below = feet.1 - 1;
            if let Some(ice) = blocks::any_state("minecraft:frosted_ice") {
                for dx in -r..=r {
                    for dz in -r..=r {
                        let (x, z) = (feet.0 + dx, feet.2 + dz);
                        if dx * dx + dz * dz > r * r || self.server.world.get(x, below, z) != fluids::Kind::Water.source() || self.server.world.get(x, below + 1, z) != world::AIR {
                            continue;
                        }
                        self.server.set_block_quiet(self.id, x, below, z, ice);
                        // Melts back after 3-6 seconds (vanilla frosted ice ages away).
                        let delay = 60 + ((x * 31 + z * 17).rem_euclid(60)) as u64;
                        self.server.scheduler.run_later(delay, move |s| {
                            if s.world.get(x, below, z) == ice {
                                s.set_block_quiet(0, x, below, z, fluids::Kind::Water.source());
                            }
                        });
                    }
                }
            }
        }
        self.falling = self.last_pos.is_some_and(|l| pos.1 < l.1 - 0.01) && !in_water && !self.flying && !self.gliding && self.riding.is_none();
        if let Some(last) = self.last_pos.replace(pos) && survival_like(&self.mode) && !self.flying {
            let walked = ((pos.0 - last.0).powi(2) + (pos.2 - last.2).powi(2)).sqrt().min(10.0);
            if self.food.exhaust(walked * if self.sprinting { 0.1 } else { 0.01 }) {
                let _ = self.conn.send(&[self.food.packet(self.id)]).await;
            }
        }
        if !self.hp.dead && !matches!(self.mode, GameType::Spectator)
            && self.server.entities.pickup(&self.server, self.id, (pos.0, pos.1 - 1.62, pos.2), &mut self.inv)
        {
            let _ = self.conn.send(&[self.inv.content_packet()]).await;
        }
        let rot = input.player_rotation;
        let head = input.player_head_rotation;
        self.server.moved(self.id, pos, rot, head);
        // Walking (not flying, not standing still) makes vibrations sculk sensors hear.
        if !self.flying && !matches!(self.mode, GameType::Spectator) && input.move_vector.0.abs() + input.move_vector.1.abs() > 0.1 {
            crate::sculk::footstep(&self.server, self.id, (pos.0, pos.1 - 1.62, pos.2), self.sneaking);
        }
        if let Some(mount) = self.riding {
            if self.hp.dead || !self.server.mobs.carries(mount, self.id) {
                // bucked off, the mount died, or we died
                self.riding = None;
                self.server.mobs.dismount(&self.server, self.id);
            } else {
                self.server.mobs.ride(mount, self.id, pos, rot.1, input.move_vector.1 > 0.1);
            }
        }
        // Portals: nether portals after 4 s standing in them (instant in
        // creative), end portals at once (PNX `Player.checkNearPortal`).
        let feet = (pos.0.floor() as i32, (pos.1 - 1.62 + 0.1).floor() as i32, pos.2.floor() as i32);
        let inside = |n: &str| (0..2).any(|dy| blocks::entry(self.server.world.get(feet.0, feet.1 + dy, feet.2)).is_some_and(|e| e.name == n));
        if self.portal_done.elapsed() > std::time::Duration::from_secs(3) {
            if inside("minecraft:portal") {
                self.portal_ticks += 1;
                if self.portal_ticks >= if matches!(self.mode, GameType::Creative) { 2 } else { 80 } {
                    self.portal_ticks = 0;
                    self.portal_done = Instant::now();
                    let (to, x, z) = portal::mirror(pos);
                    let inner = match portal::find_portal(&self.server.world, to, x, z) {
                        Some(p) => p,
                        None => {
                            let (changes, inner) = portal::build_portal(&self.server.world, to, x, z);
                            for ((bx, by, bz), b) in changes {
                                self.server.set_block_quiet(0, bx, by, bz, b);
                            }
                            inner
                        }
                    };
                    let eye = (inner.0 as f32 + 0.5, inner.1 as f32 + 1.62, inner.2 as f32 + 0.5);
                    if change_dim(&mut self.conn, &self.server, &mut self.chunks, self.id, to, eye, rot, head).await.is_err() {
                        return END;
                    }
                    return GO;
                }
            } else {
                self.portal_ticks = 0;
            }
            if inside("minecraft:end_gateway") && let Some(dest) = endfight::gateway_exit(&self.server, pos) {
                self.portal_done = Instant::now();
                let mut tp = move_player(self.id, dest, rot, head);
                if let Protocol::MovePlayerPacket(m) = &mut tp {
                    m.position_mode = PlayerPositionMode::Teleport;
                    m.teleport_data = Some(MovePlayerTeleportData { teleportation_cause: 0, source_actor_type: 0 });
                }
                let _ = self.conn.send(&[tp]).await;
                self.server.moved(self.id, dest, rot, head);
                return GO;
            }
            if inside("minecraft:end_portal") {
                self.portal_done = Instant::now();
                let (to, eye) = if dims::of_f(pos.1) == dims::END {
                    (dims::OVERWORLD, commands::spawn_of(&self.name, &self.server))
                } else {
                    let (changes, eye) = portal::end_platform();
                    for ((bx, by, bz), b) in changes {
                        self.server.set_block_quiet(0, bx, by, bz, b);
                    }
                    (dims::END, eye)
                };
                if change_dim(&mut self.conn, &self.server, &mut self.chunks, self.id, to, eye, rot, head).await.is_err() {
                    return END;
                }
                return GO;
            }
        }
        let (cx, cz) = ((pos.0.floor() as i32) >> 4, (pos.2.floor() as i32) >> 4);
        if self.chunks.update(&mut self.conn, &self.server.world, cx, cz, CHUNK_BATCH).await.is_err() {
            return END;
        }
        let loaded = std::mem::take(&mut self.chunks.just_sent);
        let dropped = std::mem::take(&mut self.chunks.just_dropped);
        if (!loaded.is_empty() || !dropped.is_empty()) && let Some(me) = self.server.get(self.id) {
            for c in loaded {
                self.server.plugins.chunk_load(&self.server, &me, c);
            }
            for c in dropped {
                self.server.plugins.chunk_unload(&self.server, &me, c);
            }
        }
        GO
    }

    pub(super) async fn on_respawn(&mut self) -> Flow {
        // Dying in the Nether or End sends you back to the overworld spawn.
        if self.conn.dim != dims::OVERWORLD {
            let (rot, head) = self.server.get(self.id).map_or(((0.0, 0.0), 0.0), |p| (p.rot, p.head_yaw));
            if change_dim(&mut self.conn, &self.server, &mut self.chunks, self.id, dims::OVERWORLD, commands::spawn_of(&self.name, &self.server), rot, head).await.is_err() {
                return END;
            }
        }
        respawn(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, &self.mode, &self.inv).await;
        // Others still have our dead body: replace it. Ours get re-sent within a second.
        let gone = Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(self.id) }));
        let _ = self.server.events.send(players::Event::Relay { from: self.id, packet: Box::new(gone) });
        let _ = self.server.events.send(players::Event::Join(self.id));
        let mut out = vec![];
        for id in self.seen.drain() {
            out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) })));
        }
        let _ = self.conn.send(&out).await;
        if let Some(me) = self.server.get(self.id) {
            self.server.plugins.player_respawn(&self.server, &me);
        }
        self.food = hunger::Hunger::default();
        if !game_rule_on(&self.server, "keepInventory", false) {
            self.xp = xp::Xp::default();
            save_xp(&self.name, self.xp);
            let _ = self.conn.send(&[self.xp.packet(self.id)]).await;
        }
        self.last_pos = None;
        let _ = self.conn.send(&[self.food.packet(self.id)]).await;
        GO
    }

    /// Also the burning flames. Swimming or crawling: the SWIMMING (56) / CRAWLING (114) flags and a 0.6-high hitbox, which is what lets
    /// the client fit through 1x1 gaps (PNX Player.setSwimming / setCrawling).
    pub(super) async fn update_pose(&mut self) {
        use bedrock::protocol::v662::types::{ActorRuntimeID, DataItem, PropertySyncData};
        use bedrock::protocol::v2168::enums::DataItemType as T;
        use bedrock::protocol::v924::enums::ActorDataIDs as D;
        let low = self.swimming || self.crawling;
        let packet = Protocol::SetActorDataPacket(Box::new(bedrock::protocol::v662::packets::SetActorDataPacket {
            target_runtime_id: ActorRuntimeID(self.id as u64),
            actor_data: vec![
                // ON_FIRE (0) shows the flames.
                DataItem { data_item_id: D::Reserved0, data_item_type: T::Int64(spawn::FLAGS | (self.swimming as i64) << 56 | (self.burning > 0) as i64) },
                DataItem { data_item_id: D::Reserved054, data_item_type: T::Float(if low { 0.6 } else { 1.8 }) },
                // CRAWLING (114) sits in the extended flags with BLOCKING (72, a raised shield).
                DataItem { data_item_id: D::Reserved092, data_item_type: T::Int64(spawn::FLAGS_EXTENDED | (self.inv.shield().is_some() as i64) << (72 - 64) | (self.crawling as i64) << (114 - 64)) },
            ],
            synced_properties: PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
            tick: 0,
        }));
        let _ = self.conn.send(&[packet.clone()]).await;
        let _ = self.server.events.send(crate::players::Event::Relay { from: self.id, packet: Box::new(packet) });
    }

    /// Shows the shield raised while sneaking with one (the BLOCKING flag, 72).
    pub(super) fn raise_shield(&self) {
        use bedrock::protocol::v662::types::{ActorRuntimeID, DataItem, PropertySyncData};
        let up = self.inv.shield().is_some();
        let packet = Protocol::SetActorDataPacket(Box::new(bedrock::protocol::v662::packets::SetActorDataPacket {
            target_runtime_id: ActorRuntimeID(self.id as u64),
            actor_data: vec![DataItem {
                data_item_id: bedrock::protocol::v924::enums::ActorDataIDs::Reserved092,
                data_item_type: bedrock::protocol::v2168::enums::DataItemType::Int64(spawn::FLAGS_EXTENDED | (up as i64) << (72 - 64) | (self.crawling as i64) << (114 - 64)),
            }],
            synced_properties: PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
            tick: 0,
        }));
        let _ = self.server.events.send(crate::players::Event::Relay { from: 0, packet: Box::new(packet) });
    }
}

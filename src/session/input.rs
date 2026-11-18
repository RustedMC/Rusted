//! Movement, view distance and respawning.

use super::*;
use bedrock::protocol::ProtoVersionPackets;

type ChunkRadius = <Protocol as ProtoVersionPackets>::RequestChunkRadiusPacket;
type AuthInput = <Protocol as ProtoVersionPackets>::PlayerAuthInputPacket;

impl Session {
    pub(super) async fn on_chunk_radius(&mut self, r: &ChunkRadius) -> Flow {
        self.chunks.radius = r.chunk_radius.clamp(2, config::get().view_distance);
        println!("{}: view distance {} chunks", self.name, self.chunks.radius);
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
        use bedrock::protocol::v2168::enums::PlayerAuthInputData as Input;
        for flag in &input.input_data {
            match flag {
                Input::StartFlying => self.flying = true,
                Input::StartGliding => self.gliding = true,
                Input::StopGliding => self.gliding = false,
                Input::StopFlying => self.flying = false,
                Input::StartSneaking => {
                    self.sneaking = true;
                    self.inv.sneaking = true;
                    self.get_off();
                }
                Input::StopSneaking => {
                    self.sneaking = false;
                    self.inv.sneaking = false;
                }
                Input::StartSprinting => self.sprinting = true,
                Input::StopSprinting => self.sprinting = false,
                Input::StartJumping if survival_like(&self.mode) && self.food.exhaust(if self.sprinting { 0.2 } else { 0.05 }) => {
                    let _ = self.conn.send(&[self.food.packet(self.id)]).await;
                }
                _ => {}
            }
        }
        let pos = input.player_position;
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
            hurt(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, damage, &mut self.inv, &self.effects).await;
        }
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
        if let Some(mount) = self.riding {
            if self.hp.dead || self.server.mobs.rider_of(mount) != Some(self.id) {
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
}

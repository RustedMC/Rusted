//! Per-player state for one connection, and the packet dispatch.

use super::*;
use bedrock::protocol::ProtoVersionPackets;
use players::Event;
use std::ops::ControlFlow;

/// What a handler tells the session loop: keep going or end the session.
pub(crate) type Flow = ControlFlow<()>;
pub(crate) const GO: Flow = ControlFlow::Continue(());
pub(crate) const END: Flow = ControlFlow::Break(());

pub(crate) type Transaction = <Protocol as ProtoVersionPackets>::InventoryTransactionPacket;
pub(crate) type ItemUse = <Protocol as bedrock::protocol::ProtoVersionTypes>::ItemUseTransactionData;

pub(crate) struct Session {
    pub conn: dims::DimConn,
    pub chunks: ChunkTracker,
    pub server: players::Server,
    pub id: i64,
    pub name: String,
    pub inv: inventory::Inventory,
    pub mode: GameType,
    pub events: tokio::sync::broadcast::Receiver<Event>,
    // Commands to run here: (line, who to answer, None = us).
    pub cmd_tx: tokio::sync::mpsc::UnboundedSender<(String, Option<i64>)>,
    pub cmd_rx: tokio::sync::mpsc::UnboundedReceiver<(String, Option<i64>)>,
    pub regen: tokio::time::Interval,
    pub regen_ticks: u32,
    pub selected_slot: u32, // hotbar slot in hand, for pick block
    pub window_id: i8, // container windows; PNX starts at 2
    pub open_container: Option<(i32, i32, i32)>, // chest/barrel this player has open
    pub open_kind: Option<(containers::Kind, ContainerEnumName)>,
    pub open_pair: Option<(i32, i32, i32)>, // other half of an open double chest
    pub hp: health::Health,
    pub flying: bool,
    pub sneaking: bool,
    pub sprinting: bool,
    pub head_in_water: bool,
    pub in_lava: bool,
    pub in_fire: bool,
    pub air: i16, // ticks of air left (vanilla 15s)
    pub food: hunger::Hunger,
    pub effects: effects::Effects,
    pub table_changed: bool,
    pub enchant_changed: bool,
    pub next_offer_id: u32,
    // A trade just happened: refresh the screen (uses, level) afterwards.
    pub traded_job: Option<(i64, villagers::Job)>,
    pub xp: xp::Xp,
    pub last_pearl: Instant,
    // When the player started drawing a bow or raising a trident.
    pub drawing: Option<Instant>,
    pub last_pos: Option<(f32, f32, f32)>,
    // Input ticks spent standing in a nether portal, and when the last trip ended.
    pub portal_ticks: u32,
    pub portal_done: Instant,
    /// Mob this player is riding.
    pub riding: Option<i64>,
    /// Gliding with an elytra.
    pub gliding: bool,
}

impl Session {
    /// Sends the intro, joins the server and loads saved state. None if the client went away.
    pub async fn join(
        mut conn: dims::DimConn,
        mut chunks: ChunkTracker,
        me: players::PlayerInfo,
        server: players::Server,
        inv: inventory::Inventory,
        mode: GameType,
    ) -> Option<Session> {
        let (id, name) = (me.id, me.name.clone());
        let events = server.events.subscribe();
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<(String, Option<i64>)>();
        // Show everyone already online (and ourselves in the player list), then announce us.
        let mut intro = vec![available_commands(&server.plugins), player_list_add(&me), commands::difficulty_packet(*commands::DIFFICULTY.lock())];
        intro.extend(commands::weather_packets(*commands::WEATHER.lock()));
        intro.extend(server.entities.spawn_packets());
        intro.extend(server.mobs.spawn_packets());
        intro.extend(ui::join_packets());
        for other in server.all() {
            intro.push(player_list_add(&other));
            intro.push(add_player(&other));
        }
        if let Err(e) = conn.send(&intro).await {
            println!("{name}: failed to send player list: {e}");
            return None;
        }
        let me_info = me.clone();
        let start_dim = dims::of_f(me.pos.1);
        let (start_pos, start_rot, start_head) = (me.pos, me.rot, me.head_yaw);
        server.join(me);
        if start_dim != dims::OVERWORLD && change_dim(&mut conn, &server, &mut chunks, id, start_dim, start_pos, start_rot, start_head).await.is_err() {
            return None;
        }
        if commands::is_op(&name) {
            commands::op_ids().lock().insert(id);
        }
        server.mobs.set_targetable(id, survival_like(&mode));
        server.mobs.owner_joined(mobs::owner_key(&name), id);
        server.plugins.join(&server, &me_info);
        println!("{name} joined");
        let _ = server.events.send(Event::Chat {
            from: String::new(),
            message: format!("§e{name} joined the game"),
        });
        let xp = xp::Xp::from_text(&std::fs::read_to_string(player_path(&name).with_extension("xp")).unwrap_or_default());
        let _ = conn.send(&[xp.packet(id)]).await;
        Some(Session {
            conn,
            chunks,
            server,
            id,
            name,
            inv,
            mode,
            events,
            cmd_tx,
            cmd_rx,
            regen: tokio::time::interval(std::time::Duration::from_secs(1)),
            regen_ticks: 0,
            selected_slot: 0,
            window_id: 1,
            open_container: None,
            open_kind: None,
            open_pair: None,
            hp: health::Health::default(),
            flying: false,
            sneaking: false,
            sprinting: false,
            head_in_water: false,
            in_lava: false,
            in_fire: false,
            air: 300,
            food: hunger::Hunger::default(),
            effects: effects::Effects::default(),
            table_changed: false,
            enchant_changed: false,
            next_offer_id: 0x5000,
            traded_job: None,
            xp,
            last_pearl: Instant::now() - std::time::Duration::from_secs(5),
            drawing: None,
            last_pos: None,
            portal_ticks: 0,
            portal_done: Instant::now(),
            riding: None,
            gliding: false,
        })
    }

    pub async fn on_batch(&mut self, batch: Vec<Protocol>) -> Flow {
        for packet in batch {
            self.on_packet(packet).await?;
        }
        GO
    }

    async fn on_packet(&mut self, packet: Protocol) -> Flow {
        match packet {
            Protocol::RequestChunkRadiusPacket(r) => return self.on_chunk_radius(&r).await,
            Protocol::PlayerAuthInputPacket(input) => return self.on_input(&input).await,
            Protocol::ModalFormResponsePacket(f) => {
                ui::answered(&self.server, self.id, f.form_id, f.json_response.as_deref());
            }
            Protocol::CommandRequestPacket(cmd) => self.on_command_request(&cmd).await,
            Protocol::TextPacket(text) => self.on_chat(*text),
            Protocol::InventoryTransactionPacket(t) if matches!(&t.data, InventoryTransactionData::ItemRelease(r) if r.action_type == 0) => {
                self.on_release().await;
            }
            Protocol::InventoryTransactionPacket(t) if matches!(&t.data, InventoryTransactionData::ItemRelease(r) if r.action_type == 1) => {
                self.on_consume().await;
            }
            Protocol::InventoryTransactionPacket(t) if matches!(t.data, InventoryTransactionData::Normal) => self.on_drop(&t).await,
            Protocol::InventoryTransactionPacket(t) if matches!(&t.data, InventoryTransactionData::ItemUseOnEntity(e) if e.action_type == 0) => {
                self.on_mob_use(&t).await;
            }
            Protocol::InventoryTransactionPacket(t) if matches!(&t.data, InventoryTransactionData::ItemUseOnEntity(e) if e.action_type == 1) => {
                self.on_attack(&t).await;
            }
            Protocol::InventoryTransactionPacket(t) => self.on_item_use(t).await,
            Protocol::BlockActorDataPacket(b) => {
                let p = (b.block_position.x, b.block_position.y, b.block_position.z);
                if spawn::can_build(&self.mode) && let Some(packet) = signs::edited(&self.server.world, p, &b.actor_data_tags) {
                    let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(packet) });
                }
            }
            Protocol::SetLocalPlayerAsInitializedPacket(_) => {
                // Physics flags (gravity, collision), attributes, abilities, inventory.
                let r = self.conn.send(&spawn::after_load(self.id, &self.name, self.inv.all_packets())).await;
                println!("{}: client loaded; sent player state: {r:?}", self.name);
            }
            Protocol::InteractPacket(i) if matches!(i.action, InteractAction::OpenInventory) => self.on_open_inventory().await,
            Protocol::InteractPacket(i) if matches!(i.action, InteractAction::StopRiding) => self.get_off(),
            Protocol::LecternUpdatePacket(l) => {
                // Page turned: everyone sees the lectern open at it.
                let p = &l.position_of_lectern_to_update;
                let pos = (p.x, p.y, p.z);
                if let Some(book) = self.server.containers.get(pos).into_iter().next().flatten() {
                    let page = (l.new_page_to_show as i32).clamp(0, crate::lectern::pages(&book).max(1) - 1);
                    crate::lectern::pages_open().lock().insert(pos, page);
                    let _ = self.server.events.send(Event::Relay { from: self.id, packet: Box::new(crate::lectern::data(pos, Some(&book), page)) });
                }
            }
            Protocol::ContainerClosePacket(c) => self.on_container_close(c).await,
            Protocol::ItemStackRequestPacket(r) => self.on_stack_request(&r).await,
            Protocol::MobEquipmentPacket(m) => {
                self.selected_slot = m.selected_slot.max(0) as u32;
                self.inv.selected = self.selected_slot as usize;
                self.server.mobs.set_held(self.id, items::name_of(m.item.id));
                // Others see what we're holding.
                let _ = self.server.events.send(Event::Relay { from: self.id, packet: Box::new(Protocol::MobEquipmentPacket(m)) });
            }
            Protocol::AnimatePacket(a) => {
                let _ = self.server.events.send(Event::Relay { from: self.id, packet: Box::new(Protocol::AnimatePacket(a)) });
            }
            Protocol::BlockPickRequestPacket(r) => self.on_block_pick(&r).await,
            Protocol::RespawnPacket(r) if self.hp.dead && matches!(r.state, PlayerRespawnState::ClientReadyToSpawn) => return self.on_respawn().await,
            Protocol::PlayerActionPacket(a) if self.hp.dead && matches!(a.action, PlayerActionType::Respawn) => return self.on_respawn().await,
            Protocol::PlayerActionPacket(a) => self.on_player_action(&a).await,
            other => {
                // debug: log packets we don't handle yet
                let d = format!("{other:?}");
                if !d.starts_with("ServerBoundDiagnostics") && !d.starts_with("Animate") {
                    println!("PACKET {}", &d[..d.len().min(300)]);
                }
            }
        }
        GO
    }

    /// Saves and tells everyone we left.
    pub fn leave(mut self) {
        self.server.mobs.dismount(&self.server, self.id);
        let name = &self.name;
        if let (Some(pos), Some(slots)) = (self.open_container.take(), self.inv.open.take()) {
            close_open(&self.server, pos, slots, self.open_pair.take());
        }
        if let Some(me) = self.server.get(self.id) {
            save_player(&self.name, me.pos, &self.inv);
            self.server.plugins.leave(&self.server, &me);
            ui::left(self.id);
        }
        self.server.leave(self.id);
        commands::op_ids().lock().remove(&self.id);
        self.server.mobs.set_held(self.id, None);
        self.server.mobs.set_targetable(self.id, false);
        self.server.mobs.owner_left(self.id, mobs::owner_key(&self.name));
        println!("{name} left");
        let _ = self.server.events.send(Event::Chat {
            from: String::new(),
            message: format!("§e{name} left the game"),
        });
    }
}

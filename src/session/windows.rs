//! Container screens: chests, furnaces, crafting, enchanting, trading.

use super::*;
use bedrock::protocol::ProtoVersionPackets;
use players::Event;

type ContainerClose = <Protocol as ProtoVersionPackets>::ContainerClosePacket;
type StackRequest = <Protocol as ProtoVersionPackets>::ItemStackRequestPacket;

impl Session {
    pub(super) async fn open_container_at(&mut self, u: &ItemUse, kind: containers::Kind) {
        let p = &u.block_position;
        if let (Some(pos), Some(slots)) = (self.open_container.take(), self.inv.open.take()) {
            close_open(&self.server, pos, slots, self.open_pair.take());
        }
        let pos = (p.x, p.y, p.z);
        // A generated structure's chest gets its loot the first time it's opened.
        for at in std::iter::once(pos).chain(containers::chest_pair(&self.server.world, pos)) {
            if let Some(loot) = crate::loot::take(at, crate::inventory::CHEST_SLOTS) {
                self.server.containers.put(at, loot);
            }
        }
        self.window_id = if self.window_id >= 99 { 2 } else { self.window_id + 1 };
        let (container_type, name) = match kind {
            containers::Kind::Chest => (ContainerType::Container, ContainerEnumName::LevelEntityContainer),
            containers::Kind::Barrel => (ContainerType::Container, ContainerEnumName::BarrelContainer),
            containers::Kind::Furnace => (ContainerType::Furnace, ContainerEnumName::FurnaceIngredientContainer),
            containers::Kind::Smoker => (ContainerType::Smoker, ContainerEnumName::SmokerIngredientContainer),
            containers::Kind::BlastFurnace => (ContainerType::BlastFurnace, ContainerEnumName::BlastFurnaceIngredientContainer),
            containers::Kind::Workbench => (ContainerType::Workbench, ContainerEnumName::CraftingInputContainer),
            containers::Kind::Stonecutter => (ContainerType::StoneCutter, ContainerEnumName::StonecutterInputContainer),
            containers::Kind::Hopper => (ContainerType::Hopper, ContainerEnumName::LevelEntityContainer),
            containers::Kind::Brewing => (ContainerType::BrewingStand, ContainerEnumName::BrewingStandInputContainer),
            containers::Kind::Anvil => (ContainerType::Anvil, ContainerEnumName::AnvilInputContainer),
            containers::Kind::Grindstone => (ContainerType::Grindstone, ContainerEnumName::GrindstoneInputContainer),
            containers::Kind::Shulker => (ContainerType::Container, ContainerEnumName::ShulkerBoxContainer),
            containers::Kind::Dispenser => (ContainerType::Dispenser, ContainerEnumName::LevelEntityContainer),
            containers::Kind::Dropper => (ContainerType::Dropper, ContainerEnumName::LevelEntityContainer),
            containers::Kind::Smithing => (ContainerType::SmithingTable, ContainerEnumName::SmithingTableInputContainer),
            containers::Kind::Enchanting => (ContainerType::Enchantment, ContainerEnumName::EnchantingInputContainer),
            containers::Kind::Beacon => (ContainerType::Beacon, ContainerEnumName::BeaconPaymentContainer),
            containers::Kind::Loom => (ContainerType::Loom, ContainerEnumName::LoomInputContainer),
        };
        self.open_kind = Some((kind, name.clone()));
        let mut out = vec![Protocol::ContainerOpenPacket(Box::new(ContainerOpenPacket {
            container_id: ContainerID(self.window_id),
            container_type,
            position: NetworkBlockPosition { x: p.x, y: p.y, z: p.z },
            target_actor_id: ActorUniqueID(-1),
        }))];
        // Double chest: open from the lead half with both halves' 54 slots.
        let pair = (kind == containers::Kind::Chest).then(|| containers::chest_pair(&self.server.world, pos)).flatten();
        let pos = match pair {
            Some(o) => pos.min(o),
            None => pos,
        };
        if let (Some(o), Protocol::ContainerOpenPacket(c)) = (pair, &mut out[0]) {
            let lead = pos.min(o);
            c.position = NetworkBlockPosition { x: lead.0, y: lead.1, z: lead.2 };
        }
        if !containers::ui_only(kind) {
            let mut slots = self.server.containers.get(pos);
            slots.truncate(containers::open_slots(kind));
            self.open_pair = None;
            if let Some(o) = pair {
                let other = if o == pos { (p.x, p.y, p.z) } else { o };
                slots.extend(self.server.containers.get(other));
                self.open_pair = Some(other);
                container_opened(&self.server, other);
            }
            self.inv.open_container(slots);
            self.open_container = Some(pos);
            container_opened(&self.server, pos);
            out.extend(self.inv.open_packet(self.window_id as u32, name));
        }
        let _ = self.conn.send(&out).await;
    }

    pub(super) async fn on_open_inventory(&mut self) {
        // Same reply PNX gives: open the player's own inventory (type -1).
        self.window_id = if self.window_id >= 99 { 2 } else { self.window_id + 1 };
        let pos = self.server.get(self.id).map(|p| p.pos).unwrap_or_default();
        let _ = self.conn.send(&[Protocol::ContainerOpenPacket(Box::new(ContainerOpenPacket {
            container_id: ContainerID(self.window_id),
            container_type: ContainerType::Inventory,
            position: NetworkBlockPosition {
                x: pos.0.floor() as i32,
                y: (pos.1 - 1.62).floor() as i32,
                z: pos.2.floor() as i32,
            },
            target_actor_id: ActorUniqueID(-1),
        }))]).await;
    }

    pub(super) async fn on_container_close(&mut self, c: Box<ContainerClose>) {
        // A closing chest keeps its contents on the server.
        if let (Some(pos), Some(slots)) = (self.open_container.take(), self.inv.open.take()) {
            close_open(&self.server, pos, slots, self.open_pair.take());
        }
        // Items left in the crafting grid and on the cursor go back into the inventory.
        let spilled = self.inv.return_craft();
        if let Some(me) = self.server.get(self.id) {
            for st in spilled {
                self.server.entities.drop_item(&self.server, st.item, st.count, me.pos);
            }
        }
        self.inv.return_cursor();
        let returned = true;
        // PNX echoes the client's close back to confirm it.
        let mut out = vec![Protocol::ContainerClosePacket(c)];
        if returned {
            out.push(self.inv.content_packet());
        }
        let _ = self.conn.send(&out).await;
    }

    pub(super) async fn on_stack_request(&mut self, r: &StackRequest) {
        let mut thrown = Vec::new();
        let responses = r
            .requests
            .iter()
            .map(|req| {
                self.inv.levels = self.xp.level;
                self.inv.creative = matches!(self.mode, GameType::Creative);
                let before = self.inv.enchant_input().map(|s| (s.item.id, s.item.user_data_buffer.clone()));
                let r = self.inv.handle(req, items::creative_item);
                thrown.append(&mut self.inv.dropped);
                if let (Some((primary, secondary)), Some(pos)) = (self.inv.beacon.take(), self.open_container)
                    && !crate::beacon::configure(&self.server.world, pos, primary, secondary)
                {
                    let _ = self.server.events.send(Event::Tell { to: self.id, message: "§cThis beacon's pyramid isn't big enough for that effect".into() });
                }
                if self.inv.spent_levels > 0 {
                    self.xp.add_levels(-self.inv.spent_levels);
                    self.table_changed = true;
                }
                if self.inv.gained_xp > 0 {
                    self.xp.add_points(self.inv.gained_xp);
                    self.table_changed = true;
                }
                if !self.inv.traded.is_empty() && let Some((vid, _)) = self.inv.trading.clone() {
                    let mut job = None;
                    for (index, n, texp) in std::mem::take(&mut self.inv.traded) {
                        job = self.server.mobs.traded(&self.server, vid, index, n, texp).or(job);
                    }
                    self.traded_job = job.map(|j| (vid, j));
                }
                if self.inv.enchant_input().map(|s| (s.item.id, s.item.user_data_buffer.clone())) != before {
                    self.enchant_changed = true;
                }
                r
            })
            .collect();
        let _ = self.conn.send(&[Protocol::ItemStackResponsePacket(Box::new(ItemStackResponsePacket { responses }))]).await;
        if std::mem::take(&mut self.table_changed) {
            save_xp(&self.name, self.xp);
            let _ = self.conn.send(&[self.xp.packet(self.id)]).await;
            if let Some(pos) = self.open_container {
                let s = match self.open_kind.as_ref().map(|k| k.0) {
                    Some(containers::Kind::Anvil) => "random.anvil_use",
                    Some(containers::Kind::Grindstone) => "grindstone.use",
                    _ => "random.enchant",
                };
                let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(play_sound(s, (pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5))) });
            }
        }
        if let Some((vid, job)) = self.traded_job.take() {
            let (tags, offers) = villagers::screen(vid, &job);
            self.inv.trading = Some((vid, offers));
            let _ = self.conn.send(&[self.xp.packet(self.id), trade_packet(self.window_id, vid, self.id, &job, tags)]).await;
            save_xp(&self.name, self.xp);
        }
        // A new item in the enchanting table: show its three offers.
        if std::mem::take(&mut self.enchant_changed) && self.open_kind.as_ref().is_some_and(|k| k.0 == containers::Kind::Enchanting) {
            let shelves = self.open_container.map_or(0, |p| bookshelves(&self.server.world, p));
            let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64 | 1;
            self.inv.offers.clear();
            let offers = self.inv.enchant_input().map(|s| tables::offers(&s.item, shelves, seed)).unwrap_or_default();
            for o in offers {
                self.next_offer_id += 1;
                self.inv.offers.push((self.next_offer_id, o));
            }
            let _ = self.conn.send(&[enchant_options(&self.inv.offers)]).await;
        }
        if let Some(me) = self.server.get(self.id) {
            for st in thrown.drain(..) {
                self.server.plugins.item_drop(&self.server, &me, items::name_of(st.item.id).unwrap_or(""), st.count);
                let (at, vel) = throw_from(&me);
                self.server.entities.drop_item_vel(&self.server, st.item, st.count, at, vel);
            }
        }
    }
}

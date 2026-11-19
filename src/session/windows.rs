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
        let block = blocks::entry(self.server.world.get(p.x, p.y, p.z)).map_or("", |e| e.name.as_str());
        if let Some(me) = self.server.get(self.id)
            && !self.server.plugins.container_open(&self.server, &me, pos, block)
        {
            return;
        }
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
            containers::Kind::Crafter => (ContainerType::Crafter, ContainerEnumName::CrafterLevelEntityContainer),
        };
        self.open_kind = Some((kind, name.clone()));
        self.open_table = Some(pos);
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

    /// Opens a chest minecart's 27 slots, kept in the container store at `store`.
    pub(super) async fn open_cart(&mut self, cart: i64, store: (i32, i32, i32)) {
        if let Some(me) = self.server.get(self.id)
            && !self.server.plugins.container_open(&self.server, &me, store, "minecraft:chest_minecart")
        {
            return;
        }
        if let (Some(pos), Some(slots)) = (self.open_container.take(), self.inv.open.take()) {
            close_open(&self.server, pos, slots, self.open_pair.take());
        }
        if let Some(loot) = crate::loot::take(store, crate::inventory::CHEST_SLOTS) {
            self.server.containers.put(store, loot);
        }
        self.window_id = if self.window_id >= 99 { 2 } else { self.window_id + 1 };
        self.open_kind = Some((containers::Kind::Chest, ContainerEnumName::LevelEntityContainer));
        self.open_table = None;
        let mut slots = self.server.containers.get(store);
        slots.resize(crate::inventory::CHEST_SLOTS, None);
        let mut out = vec![Protocol::ContainerOpenPacket(Box::new(ContainerOpenPacket {
            container_id: ContainerID(self.window_id),
            container_type: ContainerType::MinecartChest,
            position: NetworkBlockPosition { x: 0, y: 0, z: 0 },
            target_actor_id: ActorUniqueID(cart),
        }))];
        self.open_pair = None;
        self.inv.open_container(slots);
        self.open_container = Some(store);
        out.extend(self.inv.open_packet(self.window_id as u32, ContainerEnumName::LevelEntityContainer));
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

    /// Shows the plugin menu from `ui::menu`: a chest only this player sees, under their feet,
    /// opened with the menu's items. Called again when the menu changes.
    pub(super) async fn open_menu(&mut self) {
        let Some((title, slots)) = ui::menu_view(self.id) else { return };
        if self.menu_at.is_some() {
            // Already open: just swap the items.
            self.inv.open_container(slots);
            let packets = self.inv.open_packet(self.window_id as u32, ContainerEnumName::LevelEntityContainer);
            let _ = self.conn.send(packets.as_slice()).await;
            return;
        }
        if let (Some(pos), Some(slots)) = (self.open_container.take(), self.inv.open.take()) {
            close_open(&self.server, pos, slots, self.open_pair.take());
        }
        let Some(me) = self.server.get(self.id) else { return };
        let pos = (me.pos.0.floor() as i32, (me.pos.1 - 1.62).floor() as i32 - 2, me.pos.2.floor() as i32);
        let real = self.server.world.get(pos.0, pos.1, pos.2);
        let Some(chest) = blocks::any_state("minecraft:chest") else { return };
        let _ = self.conn.send(&[update_block(pos.0, pos.1, pos.2, chest), named_chest(pos, &title)]).await;
        // The client needs the chest in place before it can open it.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        self.window_id = if self.window_id >= 99 { 2 } else { self.window_id + 1 };
        self.inv.open_container(slots);
        self.open_kind = Some((containers::Kind::Chest, ContainerEnumName::LevelEntityContainer));
        self.menu_at = Some((pos, real));
        let mut out = vec![Protocol::ContainerOpenPacket(Box::new(ContainerOpenPacket {
            container_id: ContainerID(self.window_id),
            container_type: ContainerType::Container,
            position: NetworkBlockPosition { x: pos.0, y: pos.1, z: pos.2 },
            target_actor_id: ActorUniqueID(-1),
        }))];
        out.extend(self.inv.open_packet(self.window_id as u32, ContainerEnumName::LevelEntityContainer));
        let _ = self.conn.send(&out).await;
    }

    /// The fake chest goes away and the menu is forgotten.
    async fn end_menu(&mut self) {
        if let Some((pos, real)) = self.menu_at.take() {
            ui::menu_closed(self.id);
            self.inv.open = None;
            self.open_kind = None;
            let _ = self.conn.send(&[update_block(pos.0, pos.1, pos.2, real), self.inv.content_packet()]).await;
        }
    }

    pub(super) async fn on_container_close(&mut self, c: Box<ContainerClose>) {
        if self.menu_at.is_some() {
            self.end_menu().await;
            let _ = self.conn.send(&[Protocol::ContainerClosePacket(c)]).await;
            return;
        }
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
        // In a plugin menu nothing moves: refuse every request and pass the clicks on.
        if self.menu_at.is_some() {
            let (responses, clicks): (Vec<_>, Vec<_>) = r.requests.iter().map(inventory::reject).unzip();
            let _ = self.conn.send(&[Protocol::ItemStackResponsePacket(Box::new(ItemStackResponsePacket { responses }))]).await;
            let mut keep = true;
            if let Some(me) = self.server.get(self.id) {
                for slot in clicks.into_iter().flatten() {
                    keep &= ui::menu_click(&self.server, &me, slot);
                }
            }
            if keep {
                // Undo what the client already moved on its side.
                self.open_menu().await;
                let _ = self.conn.send(&[self.inv.content_packet()]).await;
            } else {
                let close = Protocol::ContainerClosePacket(Box::new(ContainerClose {
                    container_id: ContainerID(self.window_id),
                    container_type: ContainerType::Container,
                    server_initiated_close: true,
                }));
                let _ = self.conn.send(&[close]).await;
                self.end_menu().await;
            }
            return;
        }
        let mut thrown = Vec::new();
        let responses = r
            .requests
            .iter()
            .map(|req| {
                self.inv.levels = self.xp.level;
                self.inv.creative = matches!(self.mode, GameType::Creative);
                let before = self.inv.enchant_input().map(|s| (s.item.id, s.item.user_data_buffer.clone()));
                // Plugins can refuse a crafting-table or inventory craft.
                let crafting = req.actions.iter().find_map(|a| match a {
                    bedrock::protocol::v2225::enums::ItemStackRequestActionType::CraftRecipe { recipe_network_id, number_of_requested_crafts }
                    | bedrock::protocol::v2225::enums::ItemStackRequestActionType::CraftRecipeAuto { recipe_network_id, number_of_requested_crafts, .. } => {
                        crate::recipes::get().crafted(*recipe_network_id).map(|(item, n)| (item, n as i32 * (*number_of_requested_crafts).max(1) as i32))
                    }
                    _ => None,
                });
                if let (Some((item, n)), Some(me)) = (crafting, self.server.get(self.id))
                    && !self.server.plugins.craft(&self.server, &me, items::name_of(item.id).unwrap_or(""), n)
                {
                    return inventory::reject(req).0;
                }
                // Plugins can undo any move in a screen.
                if let Some(me) = self.server.get(self.id) {
                    use bedrock::protocol::v2225::enums::ItemStackRequestActionType as A;
                    for a in &req.actions {
                        let (action, slot) = match a {
                            A::Take { source, .. } => ("take", Some(source)),
                            A::Place { destination, .. } => ("place", Some(destination)),
                            A::Swap { source, .. } => ("swap", Some(source)),
                            A::Drop { source, .. } => ("drop", Some(source)),
                            A::Destroy { source, .. } => ("destroy", Some(source)),
                            A::CraftRecipe { .. } | A::CraftRecipeAuto { .. } | A::CraftCreative { .. } => ("craft", None),
                            _ => ("other", None),
                        };
                        let container = slot.map(|s| format!("{:?}", s.container_name.container)).unwrap_or_default();
                        if !self.server.plugins.inventory_action(&self.server, &me, action, &container) {
                            return inventory::reject(req).0;
                        }
                    }
                }
                let r = self.inv.handle(req, items::creative_item);
                thrown.append(&mut self.inv.dropped);
                if let (Some((primary, secondary)), Some(pos)) = (self.inv.beacon.take(), self.open_table)
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
                let _ = self.server.plugins.item_drop(&self.server, &me, items::name_of(st.item.id).unwrap_or(""), st.count);
                let (at, vel) = throw_from(&me);
                self.server.entities.drop_item_vel(&self.server, st.item, st.count, at, vel);
            }
        }
    }
}

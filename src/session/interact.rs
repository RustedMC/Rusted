//! Breaking, using and placing blocks, and throwing things.

use super::*;
use bedrock::protocol::ProtoVersionPackets;
use players::Event;

type PlayerAction = <Protocol as ProtoVersionPackets>::PlayerActionPacket;

impl Session {
    /// Asks plugins about an item use, once per click. One right-click on a block arrives as
    /// several "use on block" packets (and more while held), so repeats of the same item within
    /// a quarter second reuse the first answer instead of firing the event again.
    fn item_use_allowed(&mut self, me: &players::PlayerInfo, st: &inventory::Stack) -> bool {
        let now = Instant::now();
        if let Some((at, item, allowed)) = &mut self.last_item_use
            && now.duration_since(*at) < std::time::Duration::from_millis(250)
            && *item == st.item.id
        {
            *at = now; // still holding the button: the window keeps going
            return *allowed;
        }
        let allowed = self.server.plugins.item_use(&self.server, me, st);
        self.last_item_use = Some((now, st.item.id, allowed));
        allowed
    }

    pub(super) async fn on_item_use(&mut self, t: Box<Transaction>) {
        // Creative placing: the client tells us the block it's holding.
        if let InventoryTransactionData::ItemUse(u) = &t.data
            && u.action_type == 2 // break block (survival)
        {
            let p = &u.block_position;
            if spawn::can_build(&self.mode)
                && self.server.get(self.id).is_some_and(|me| self.server.plugins.block_break(&self.server, &me, (p.x, p.y, p.z), self.server.world.get(p.x, p.y, p.z)))
            {
                let held = self.inv.held(self.selected_slot as usize).and_then(|s| items::name_of(s.item.id));
                let tool = self.inv.held(self.selected_slot as usize).map(|s| s.item.user_data_buffer.clone()).unwrap_or_default();
                break_block(&self.server, self.id, (p.x, p.y, p.z), survival_like(&self.mode).then_some(held), &tool);
                let uses = mining_wear(held);
                wear(&mut self.conn, &self.server, self.id, &mut self.inv, self.selected_slot as usize, uses, &self.mode).await;
            } else {
                let _ = self.conn.send(&[update_block(p.x, p.y, p.z, self.server.world.get(p.x, p.y, p.z))]).await;
            }
        }
        // Plugins see every use of a held item first, and can cancel it.
        if let InventoryTransactionData::ItemUse(u) = &t.data
            && matches!(u.action_type, 0 | 1)
            && let Some(st) = self.inv.held(self.selected_slot as usize).cloned()
            && let Some(me) = self.server.get(self.id)
            && !self.item_use_allowed(&me, &st)
        {
            if u.action_type == 0 {
                let p = &u.block_position;
                let _ = self.conn.send(&[update_block(p.x, p.y, p.z, self.server.world.get(p.x, p.y, p.z))]).await;
            }
            let _ = self.conn.send(&[self.inv.content_packet()]).await;
            return;
        }
        // Use in the air with armor (or an elytra, a carved pumpkin, a head): put it on, swapping
        // with what's worn, with the equip sound (Nukkit ItemArmor.onClickAir).
        if let InventoryTransactionData::ItemUse(u) = &t.data
            && u.action_type == 1
            && !matches!(self.mode, GameType::Spectator)
            && let Some(name) = self.inv.held(self.selected_slot as usize).and_then(|s| items::name_of(s.item.id))
            && let Some(piece) = armor_slot(name)
        {
            let sound = armor_sound(name);
            let (held, worn) = (self.selected_slot as usize, inventory::ARMOR + piece);
            // Curse of Binding keeps the worn piece on.
            let bound = !matches!(self.mode, GameType::Creative) && self.inv.slots[worn].as_ref().is_some_and(|s| enchant::level(&s.item.user_data_buffer, enchant::BINDING) > 0);
            if !bound {
                self.inv.slots.swap(held, worn);
                let _ = self.conn.send(&[self.inv.content_packet()]).await;
                if let Some(me) = self.server.get(self.id) {
                    let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(level_sound(sound, me.pos, -1)) });
                    save_player(&self.name, me.pos, &self.inv);
                }
            }
            return;
        }
        // Use in the air (action 1) with a bow or trident: start drawing it.
        if let InventoryTransactionData::ItemUse(u) = &t.data
            && u.action_type == 1
            && self.inv.held(self.selected_slot as usize).and_then(|s| items::name_of(s.item.id)).is_some_and(|n| matches!(n, "minecraft:bow" | "minecraft:trident" | "minecraft:crossbow"))
        {
            // A loaded crossbow fires; anything else starts drawing.
            if !self.fire_crossbow().await {
                self.drawing = Some(Instant::now());
            }
            return;
        }
        // A firework rocket mid-glide boosts you the way you're looking.
        if let InventoryTransactionData::ItemUse(u) = &t.data
            && u.action_type == 1
            && self.gliding
            && self.inv.held(self.selected_slot as usize).and_then(|s| items::name_of(s.item.id)) == Some("minecraft:firework_rocket")
            && let Some(me) = self.server.get(self.id)
        {
            let (pitch, yaw) = (me.rot.0.to_radians(), me.rot.1.to_radians());
            let look = (-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos());
            let boost = Protocol::SetActorMotionPacket(Box::new(bedrock::protocol::v662::packets::SetActorMotionPacket {
                target_runtime_id: ActorRuntimeID(self.id as u64),
                motion: (look.0 * 1.6, look.1 * 1.6, look.2 * 1.6),
                server_tick: 0,
            }));
            let _ = self.conn.send(&[boost]).await;
            let data = self.inv.held(self.selected_slot as usize).map(|s| s.item.user_data_buffer.clone()).unwrap_or_default();
            crate::rockets::boost(&self.server, self.id, me.pos, &data);
            if survival_like(&self.mode) {
                self.inv.use_one(self.selected_slot as usize);
                let _ = self.conn.send(&[self.inv.content_packet()]).await;
            }
            return;
        }
        // Fishing rods cast, or reel in what's on the line.
        if let InventoryTransactionData::ItemUse(u) = &t.data
            && u.action_type == 1
            && !matches!(self.mode, GameType::Spectator)
            && let Some(rod) = self.inv.held(self.selected_slot as usize).filter(|s| items::name_of(s.item.id) == Some("minecraft:fishing_rod")).cloned()
            && let Some(me) = self.server.get(self.id)
        {
            if !crate::fishing::has_hook(self.id) {
                crate::fishing::cast(&self.server, self.id, me.pos, me.rot, &rod.item.user_data_buffer);
                let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(play_sound("random.bow", me.pos)) });
                return;
            }
            if let Some((item, from, xp)) = crate::fishing::reel(&self.server, self.id)
                && let Some(it) = items::item_by_name(item)
                && self.server.plugins.fish(&self.server, &me, item)
            {
                // The catch flies out of the water at you (PNX reelLine).
                let vel = crate::fishing::fling(from, (me.pos.0, me.pos.1 - 1.0, me.pos.2));
                self.server.entities.drop_item_vel(&self.server, it, 1, from, vel);
                let _ = self.server.events.send(Event::Xp { to: self.id, points: xp });
                wear(&mut self.conn, &self.server, self.id, &mut self.inv, self.selected_slot as usize, 1, &self.mode).await;
                let _ = self.conn.send(&[self.inv.content_packet()]).await;
            }
            return;
        }
        // An empty map becomes a map of the area you're standing in.
        if let InventoryTransactionData::ItemUse(u) = &t.data
            && u.action_type == 1
            && self.inv.held(self.selected_slot as usize).and_then(|s| items::name_of(s.item.id)) == Some("minecraft:empty_map")
            && let Some(me) = self.server.get(self.id)
            && let Some(map) = crate::maps::filled_map(crate::maps::create(me.pos.0, me.pos.2, self.conn.dim))
        {
            let slot = self.selected_slot as usize;
            if survival_like(&self.mode) && self.inv.held(slot).is_some_and(|s| s.count > 1) {
                self.inv.use_one(slot);
                self.inv.add(map, 1);
            } else {
                self.inv.set_slot(slot, map, 1);
            }
            let _ = self.conn.send(&[self.inv.content_packet()]).await;
            let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(play_sound("ui.cartography_table.take_result", me.pos)) });
            return;
        }
        // Splash and lingering potions are thrown.
        if let InventoryTransactionData::ItemUse(u) = &t.data
            && u.action_type == 1
            && let Some(st) = self.inv.held(self.selected_slot as usize).cloned()
            && items::name_of(st.item.id).is_some_and(|n| matches!(n, "minecraft:splash_potion" | "minecraft:lingering_potion"))
            && let Some(me) = self.server.get(self.id)
        {
            mobs::throw(&self.server, mobs::Throw::Potion(st.item.aux_value), self.id, me.pos, me.rot);
            if survival_like(&self.mode) {
                self.inv.use_one(self.selected_slot as usize);
                let _ = self.conn.send(&[self.inv.content_packet()]).await;
            }
            return;
        }
        // Use in the air (action 1): throw snowballs, eggs, ender pearls.
        if let InventoryTransactionData::ItemUse(u) = &t.data
            && u.action_type == 1
            && !matches!(self.mode, GameType::Spectator)
            && let Some(kind) = self.inv.held(self.selected_slot as usize).and_then(|s| items::name_of(s.item.id)).and_then(mobs::Throw::from_item)
            && let Some(me) = self.server.get(self.id)
            && (kind != mobs::Throw::Pearl || self.last_pearl.elapsed() >= std::time::Duration::from_secs(1))
        {
            if kind == mobs::Throw::Pearl {
                self.last_pearl = Instant::now();
            }
            mobs::throw(&self.server, kind, self.id, me.pos, me.rot);
            if survival_like(&self.mode) {
                self.inv.use_one(self.selected_slot as usize);
                let _ = self.conn.send(&[self.inv.content_packet()]).await;
            }
            return;
        }
        if let InventoryTransactionData::ItemUse(u) = t.data
            && u.action_type == 0 // click block
        {
            self.click_block(&u).await;
        }
    }

    async fn click_block(&mut self, u: &ItemUse) {
        let p = &u.block_position;
        // Right-click a chest/barrel/furnace/crafting table: open it (sneak to place against it).
        let clicked = self.server.world.get(p.x, p.y, p.z);
        if !self.sneaking && !matches!(self.mode, GameType::Spectator) && let Some(kind) = containers::kind(clicked) {
            self.open_container_at(u, kind).await;
            return;
        }
        let held_name = items::name_of(u.held_item.id).unwrap_or("");
        let clicked_name = blocks::entry(clicked).map_or("", |e| e.name.as_str());
        // Minecarts go on rails.
        if matches!(held_name, "minecraft:minecart" | "minecraft:tnt_minecart")
            && !matches!(self.mode, GameType::Spectator)
            && crate::minecart::rail_at(&self.server.world, p.x, p.y, p.z).is_some()
        {
            let kind = if held_name == "minecraft:tnt_minecart" { mobs::Kind::TntMinecart } else { mobs::Kind::Minecart };
            if let Some(me) = self.server.get(self.id)
                && !self.server.plugins.vehicle_place(&self.server, &me, kind.identifier(), (p.x as f32 + 0.5, p.y as f32, p.z as f32 + 0.5))
            {
                return;
            }
            self.server.mobs.spawn_cart(&self.server, kind, (p.x, p.y, p.z));
            if survival_like(&self.mode) {
                self.inv.use_one(self.selected_slot as usize);
                let _ = self.conn.send(&[self.inv.content_packet()]).await;
            }
            return;
        }
        // Boats go on the block clicked, or float on water.
        if let Some((wood, chest)) = mobs::boat_wood(held_name)
            && !matches!(self.mode, GameType::Spectator)
            && let Some(me) = self.server.get(self.id)
        {
            let water = crate::overworld::feature::blocks::is_water(clicked);
            let y = if water { p.y as f32 + 0.9 } else { p.y as f32 + 1.0 };
            if !self.server.plugins.vehicle_place(&self.server, &me, if chest { "minecraft:chest_boat" } else { "minecraft:boat" }, (p.x as f32 + 0.5, y, p.z as f32 + 0.5)) {
                return;
            }
            self.server.mobs.spawn_boat(&self.server, wood, (p.x as f32 + 0.5, y, p.z as f32 + 0.5), me.rot.1, chest);
            if survival_like(&self.mode) {
                self.inv.use_one(self.selected_slot as usize);
                let _ = self.conn.send(&[self.inv.content_packet()]).await;
            }
            return;
        }
        // Vaults open with a trial key, once per player.
        if crate::trial::is_vault(clicked) && !matches!(self.mode, GameType::Spectator) {
            let held = self.inv.held(self.selected_slot as usize).and_then(|s| items::name_of(s.item.id)).unwrap_or("");
            match crate::trial::open_vault(&self.server, (p.x, p.y, p.z), &self.name, held) {
                Some(true) => {
                    if survival_like(&self.mode) {
                        self.inv.use_one(self.selected_slot as usize);
                        let _ = self.conn.send(&[self.inv.content_packet()]).await;
                    }
                }
                Some(false) => {
                    let _ = self.conn.send(&[chat_packet(String::new(), "§7You've already opened this vault".into())]).await;
                }
                None => {}
            }
            return;
        }
        // Lecterns: put a book on, sneak-click to take it back, click to read.
        if crate::lectern::is_lectern(clicked) && !matches!(self.mode, GameType::Spectator) {
            let pos = (p.x, p.y, p.z);
            let book = self.server.containers.get(pos).into_iter().next().flatten();
            let page = crate::lectern::pages_open().lock().get(&pos).copied().unwrap_or(0);
            let slot = self.selected_slot as usize;
            match book {
                None if crate::lectern::is_book(held_name) => {
                    let Some(mut b) = self.inv.held(slot).cloned() else { return };
                    b.count = 1;
                    if survival_like(&self.mode) {
                        self.inv.use_one(slot);
                        let _ = self.conn.send(&[self.inv.content_packet()]).await;
                    }
                    self.server.containers.put(pos, vec![Some(b.clone())]);
                    crate::lectern::pages_open().lock().insert(pos, 0);
                    let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(crate::lectern::data(pos, Some(&b), 0)) });
                    let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(play_sound("item.book.put", (p.x as f32 + 0.5, p.y as f32 + 1.0, p.z as f32 + 0.5))) });
                    return;
                }
                Some(b) if self.sneaking => {
                    let left = self.inv.add(b.item.clone(), 1);
                    if left > 0 && let Some(me) = self.server.get(self.id) {
                        self.server.entities.drop_item(&self.server, b.item, left, (me.pos.0, me.pos.1 - 1.0, me.pos.2));
                    }
                    self.server.containers.put(pos, vec![]);
                    crate::lectern::pages_open().lock().remove(&pos);
                    let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(crate::lectern::data(pos, None, 0)) });
                    let _ = self.conn.send(&[self.inv.content_packet()]).await;
                    return;
                }
                Some(b) => {
                    // The client opens the book screen itself; make sure it has the pages.
                    let _ = self.conn.send(&[crate::lectern::data(pos, Some(&b), page)]).await;
                    return;
                }
                None => {}
            }
        }
        // Beds set your spawn point (sleeping isn't simulated yet).
        if !self.sneaking && (clicked_name.ends_with("_bed") || clicked_name == "minecraft:bed") {
            self.use_bed(u).await;
            return;
        }
        // Blocks that react to a click live in useblock.rs.
        if self.server.get(self.id).is_some_and(|me| !self.server.plugins.player_interact(&self.server, &me, (p.x, p.y, p.z), held_name)) {
            let _ = self.conn.send(&[update_block(p.x, p.y, p.z, self.server.world.get(p.x, p.y, p.z))]).await;
            return;
        }
        if spawn::can_build(&self.mode) {
            let ctx = useblock::Ctx {
                held: held_name,
                sneaking: self.sneaking,
                survival: survival_like(&self.mode),
                hungry: self.food.food < hunger::MAX_FOOD,
                face: u.block_face,
                roll: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() ^ (p.x as u32).wrapping_mul(2_654_435_761),
                overworld: true,
            };
            if let Some(o) = useblock::use_block(&self.server.world, (p.x, p.y, p.z), &ctx) {
                apply_use(&self.server, self.id, (p.x, p.y, p.z), &o);
                if clicked_name == "minecraft:end_portal_frame"
                    && let Some(portal) = portal::light_end(&self.server.world, (p.x, p.y, p.z))
                    && self.server.get(self.id).is_none_or(|me| self.server.plugins.portal_create(&self.server, &me, "end"))
                {
                    for ((bx, by, bz), b) in portal {
                        self.server.set_block_quiet(self.id, bx, by, bz, b);
                    }
                    let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(play_sound("block.end_portal.spawn", (p.x as f32 + 0.5, p.y as f32, p.z as f32 + 0.5))) });
                }
                if let Some((f, sat)) = o.eat && (ctx.hungry || !ctx.survival) {
                    self.food.eat(f, sat);
                    let _ = self.conn.send(&[self.food.packet(self.id)]).await;
                }
                if ctx.survival {
                    let slot = self.selected_slot as usize;
                    match o.held {
                        useblock::Held::Keep if !o.changes.is_empty() || !o.drops.is_empty() => {
                            wear(&mut self.conn, &self.server, self.id, &mut self.inv, slot, 1, &self.mode).await;
                        }
                        useblock::Held::Keep => {}
                        useblock::Held::UseOne => {
                            self.inv.use_one(slot);
                            let _ = self.conn.send(&[self.inv.content_packet()]).await;
                        }
                        useblock::Held::Become(item) => swap_bucket(&mut self.conn, &mut self.inv, self.selected_slot, &self.mode, item).await,
                        useblock::Held::Trade(item) => {
                            self.inv.use_one(slot);
                            if let Some(it) = items::item_by_name(item) {
                                let left = self.inv.add(it.clone(), 1);
                                if left > 0 && let Some(me) = self.server.get(self.id) {
                                    self.server.entities.drop_item(&self.server, it, left, (me.pos.0, me.pos.1 - 1.0, me.pos.2));
                                }
                            }
                            let _ = self.conn.send(&[self.inv.content_packet()]).await;
                        }
                    }
                }
                return;
            }
        }
        // Flint and steel lights a fire on the clicked face.
        if held_name == "minecraft:flint_and_steel" && spawn::can_build(&self.mode) && clicked_name == "minecraft:tnt" {
            mobs::ignite_tnt(&self.server, (p.x, p.y, p.z), 40);
            wear(&mut self.conn, &self.server, self.id, &mut self.inv, self.selected_slot as usize, 1, &self.mode).await;
            return;
        }
        // A rocket used on a block launches from that face (vanilla ItemFirework.onActivate).
        if held_name == "minecraft:firework_rocket" && !matches!(self.mode, GameType::Spectator) {
            let (ox, oy, oz) = placement::face_offset(u.block_face);
            let at = ((p.x + ox) as f32 + 0.5, (p.y + oy) as f32 + 0.1, (p.z + oz) as f32 + 0.5);
            let data = self.inv.held(self.selected_slot as usize).map(|s| s.item.user_data_buffer.clone()).unwrap_or_default();
            crate::rockets::launch(&self.server, at, &data);
            if survival_like(&self.mode) {
                self.inv.use_one(self.selected_slot as usize);
                let _ = self.conn.send(&[self.inv.content_packet()]).await;
            }
            return;
        }
        if held_name == "minecraft:flint_and_steel" && spawn::can_build(&self.mode) {
            let (ox, oy, oz) = placement::face_offset(u.block_face);
            let at = (p.x + ox, p.y + oy, p.z + oz);
            if let Some(portal) = portal::light_nether(&self.server.world, at)
                && self.server.get(self.id).is_none_or(|me| self.server.plugins.portal_create(&self.server, &me, "nether"))
            {
                for ((bx, by, bz), b) in portal {
                    self.server.set_block_quiet(self.id, bx, by, bz, b);
                }
                let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(play_sound("fire.ignite", (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5))) });
                wear(&mut self.conn, &self.server, self.id, &mut self.inv, self.selected_slot as usize, 1, &self.mode).await;
                return;
            }
            let here = self.server.world.get(at.0, at.1, at.2);
            let open = here == world::AIR || (crate::overworld::feature::blocks::is_plant(here));
            if open && let Some(fire) = blocks::any_state("minecraft:fire") {
                // Quiet: the only sound is flint and steel's own ignite, not a block place.
                self.server.set_block_quiet(self.id, at.0, at.1, at.2, fire);
                growth::FIRES.lock().push(at);
                wear(&mut self.conn, &self.server, self.id, &mut self.inv, self.selected_slot as usize, 1, &self.mode).await;
                let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(play_sound("fire.ignite", (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5))) });
            }
            return;
        }
        // Seeds plant crops on farmland.
        let crop = match held_name {
            "minecraft:wheat_seeds" => Some("minecraft:wheat"),
            "minecraft:carrot" => Some("minecraft:carrots"),
            "minecraft:potato" => Some("minecraft:potatoes"),
            "minecraft:beetroot_seeds" => Some("minecraft:beetroot"),
            _ => None,
        };
        if let Some(crop) = crop && spawn::can_build(&self.mode) {
            let above = (p.x, p.y + 1, p.z);
            if clicked_name == "minecraft:farmland" && self.server.world.get(above.0, above.1, above.2) == world::AIR
                && let Some(b) = blocks::any_state(crop)
            {
                self.server.set_block(self.id, above.0, above.1, above.2, blocks::with(b, &[("growth", blocks::State::Int(0))]));
                if survival_like(&self.mode) {
                    self.inv.use_one(self.selected_slot as usize);
                }
            }
            return;
        }
        // Doors/levers/buttons get used instead of placed (sneak to place).
        if !self.sneaking && !matches!(self.mode, GameType::Spectator)
            && let Some(action) = physics::interact(&self.server.world, (p.x, p.y, p.z))
        {
            match action {
                physics::Interaction::Toggle(changes) => {
                    let block = self.server.world.get(p.x, p.y, p.z);
                    let on = changes.iter().any(|(_, b)| matches!(blocks::state(*b, "open_bit"), Some(blocks::State::Bool(true))));
                    if let Some(me) = self.server.get(self.id)
                        && !self.server.plugins.block_toggle(&self.server, &me, (p.x, p.y, p.z), blocks::entry(block).map_or("", |e| e.name.as_str()), on)
                    {
                        // put the client's guess back
                        let undo: Vec<Protocol> = changes.iter().map(|((x, y, z), _)| crate::packets::update_block(*x, *y, *z, self.server.world.get(*x, *y, *z))).collect();
                        let _ = self.conn.send(&undo).await;
                        return;
                    }
                    // Levers: power.on/off; repeaters and comparators: silent; doors,
                    // trapdoors and gates: 1003.
                    let clicked = self.server.world.get(p.x, p.y, p.z);
                    let n = blocks::entry(clicked).map_or("", |e| e.name.as_str());
                    let at = (p.x as f32 + 0.5, p.y as f32 + 0.5, p.z as f32 + 0.5);
                    if n == "minecraft:lever" {
                        let on = changes.iter().any(|(_, b)| matches!(blocks::state(*b, "open_bit"), Some(blocks::State::Bool(true))));
                        let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(level_sound(if on { "power.on" } else { "power.off" }, at, clicked)) });
                    } else if !n.contains("repeater") && !n.contains("comparator") {
                        let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(level_event(1003, (p.x, p.y, p.z))) });
                    }
                    // PNX BlockFenceGate.setOpen: every toggle re-faces the gate from the player's yaw,
                    // so it swings away from them.
                    let yaw = self.server.get(self.id).map_or(0.0, |me| me.rot.1);
                    let rotation = (yaw - 90.0).rem_euclid(360.0);
                    for ((bx, by, bz), mut b) in changes {
                        if blocks::entry(b).is_some_and(|e| e.name.ends_with("fence_gate"))
                            && let Some(blocks::State::Str(d)) = blocks::state(b, "minecraft:cardinal_direction")
                        {
                            let z_axis = matches!(d.as_str(), "north" | "south");
                            let face = if z_axis {
                                if rotation < 180.0 { "north" } else { "south" }
                            } else if (90.0..270.0).contains(&rotation) {
                                "east"
                            } else {
                                "west"
                            };
                            b = blocks::with(b, &[("minecraft:cardinal_direction", blocks::State::Str(face.into()))]);
                        }
                        self.server.set_block_quiet(self.id, bx, by, bz, b);
                    }
                }
                physics::Interaction::Press { pressed, released } => {
                    let clicked = self.server.world.get(p.x, p.y, p.z);
                    let at = (p.x as f32 + 0.5, p.y as f32 + 0.5, p.z as f32 + 0.5);
                    let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(level_sound("power.on", at, clicked)) });
                    for ((bx, by, bz), b) in pressed {
                        self.server.set_block_quiet(self.id, bx, by, bz, b);
                    }
                    let (server, id) = (self.server.clone(), self.id);
                    // Vanilla: stone and polished blackstone buttons 20 game ticks, wooden 30.
                    let stone = blocks::entry(self.server.world.get(p.x, p.y, p.z)).is_some_and(|e| e.name.contains("stone_button"));
                    let hold = std::time::Duration::from_millis(if stone { 1000 } else { 1500 });
                    tokio::spawn(async move {
                        tokio::time::sleep(hold).await;
                        for ((bx, by, bz), b) in released {
                            server.set_block_quiet(id, bx, by, bz, b);
                        }
                        let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(level_sound("power.off", at, clicked)) });
                    });
                }
            }
            return;
        }
        self.place_held(u).await;
    }

    async fn use_bed(&mut self, u: &ItemUse) {
        let p = &u.block_position;
        if let Some(me) = self.server.get(self.id)
            && !self.server.plugins.bed_enter(&self.server, &me, (p.x, p.y, p.z))
        {
            return;
        }
        // Beds blow up outside the overworld (vanilla: power 5, sets fires).
        if dims::of(p.y) != dims::OVERWORLD {
            self.server.set_block(self.id, p.x, p.y, p.z, world::AIR);
            mobs::explode(&self.server, (p.x as f32 + 0.5, p.y as f32 + 0.5, p.z as f32 + 0.5), 5.0, "[Intentional Game Design]");
            return;
        }
        commands::set_spawn(Some(&self.name), (p.x as f32 + 0.5, p.y as f32 + 1.0 + 1.62, p.z as f32 + 0.5));
        let t = self.server.time().rem_euclid(24_000);
        let storm = matches!(*commands::WEATHER.lock(), commands::Weather::Thunder);
        if !(12_542..23_460).contains(&t) && !storm {
            let _ = self.conn.send(&[chat_packet(String::new(), "You can only sleep at night or during thunderstorms".into())]).await;
            return;
        }
        // Lie down: sleeping flag and bed position for everyone, feet on the bed.
        let bed = (p.x, p.y, p.z);
        blockops::sleepers().lock().insert(self.id, Instant::now());
        let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(blockops::sleep_packet(self.id, Some(bed))) });
        self.server.teleport(self.id, (p.x as f32 + 0.5, p.y as f32 + 0.5625, p.z as f32 + 0.5));
        let (online, asleep) = (self.server.all().len(), blockops::sleepers().lock().len());
        self.server.actionbar(self.id, &format!("{asleep}/{online} players sleeping"));
        // Vanilla waits 100 ticks (5 s) of sleep before skipping the night.
        let server = self.server.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(5100)).await;
            blockops::check_sleep(&server);
        });
    }

    async fn place_held(&mut self, u: &ItemUse) {
        let p = &u.block_position;
        let (dx, dy, dz) = placement::face_offset(u.block_face);
        let held = items::name_of(u.held_item.id).unwrap_or("");
        // Spawn eggs: the mob appears on the clicked face (vanilla ItemSpawnEgg).
        if let Some(kind) = held.strip_suffix("_spawn_egg").and_then(mobs::Kind::from_name) {
            if !matches!(self.mode, GameType::Spectator) && !commands::spawn_protected(&self.server, &self.name, (p.x, p.y, p.z)) {
                let at = (p.x as f32 + dx as f32 + 0.5, (p.y + dy) as f32, p.z as f32 + dz as f32 + 0.5);
                self.server.mobs.spawn(&self.server, kind, at);
                let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(mobs::mob_sound("ambient", kind, at, false)) });
                if survival_like(&self.mode) {
                    self.inv.use_one(self.selected_slot as usize);
                    let _ = self.conn.send(&[self.inv.content_packet()]).await;
                }
            }
            return;
        }
        // Buckets: empty one picks up a source, full one places it.
        if spawn::can_build(&self.mode) && matches!(held, "minecraft:water_bucket" | "minecraft:lava_bucket" | "minecraft:bucket") {
            let target = (p.x + dx, p.y + dy, p.z + dz);
            let clicked = self.server.world.get(p.x, p.y, p.z);
            let at = (p.x as f32 + 0.5, p.y as f32 + 0.5, p.z as f32 + 0.5);
            // Plugins may stop it: put the blocks and the bucket back on the player's screen.
            let allowed = self.server.get(self.id).is_none_or(|me| {
                if held == "minecraft:bucket" {
                    let filled = if clicked == fluids::Kind::Lava.source() { "minecraft:lava_bucket" } else { "minecraft:water_bucket" };
                    self.server.plugins.bucket_fill(&self.server, &me, (p.x, p.y, p.z), filled)
                } else {
                    self.server.plugins.bucket_empty(&self.server, &me, target, held)
                }
            });
            if !allowed {
                let _ = self.conn.send(&[
                    update_block(p.x, p.y, p.z, clicked),
                    update_block(target.0, target.1, target.2, self.server.world.get(target.0, target.1, target.2)),
                    self.inv.content_packet(),
                ]).await;
                return;
            }
            // Waterlogging: water goes into a slab, stair, fence... instead of on top of it.
            if held == "minecraft:water_bucket" && world::waterloggable(clicked) && !self.server.world.waterlogged(p.x, p.y, p.z) && dims::of(p.y) != dims::NETHER {
                self.server.set_water(p.x, p.y, p.z, true);
                let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(play_sound("bucket.empty_water", at)) });
                swap_bucket(&mut self.conn, &mut self.inv, self.selected_slot, &self.mode, "minecraft:bucket").await;
                return;
            }
            if held == "minecraft:bucket" && self.server.world.waterlogged(p.x, p.y, p.z) {
                self.server.set_water(p.x, p.y, p.z, false);
                let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(play_sound("bucket.fill_water", at)) });
                swap_bucket(&mut self.conn, &mut self.inv, self.selected_slot, &self.mode, "minecraft:water_bucket").await;
                return;
            }
            if held == "minecraft:bucket" {
                let clicked = self.server.world.get(p.x, p.y, p.z);
                if clicked == fluids::Kind::Water.source() || clicked == fluids::Kind::Lava.source() {
                    // Only the bucket sound, no break particles or block sound.
                    self.server.set_block_quiet(self.id, p.x, p.y, p.z, world::AIR);
                    let full = if clicked == fluids::Kind::Water.source() { "minecraft:water_bucket" } else { "minecraft:lava_bucket" };
                    let s = if full == "minecraft:water_bucket" { "bucket.fill_water" } else { "bucket.fill_lava" };
                    let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(play_sound(s, (p.x as f32 + 0.5, p.y as f32 + 0.5, p.z as f32 + 0.5))) });
                    swap_bucket(&mut self.conn, &mut self.inv, self.selected_slot, &self.mode, full).await;
                }
            } else if self.server.world.get(target.0, target.1, target.2) == world::AIR {
                if held == "minecraft:water_bucket" && dims::of(target.1) == dims::NETHER {
                    let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(play_sound("random.fizz", (target.0 as f32 + 0.5, target.1 as f32 + 0.5, target.2 as f32 + 0.5))) });
                    swap_bucket(&mut self.conn, &mut self.inv, self.selected_slot, &self.mode, "minecraft:bucket").await;
                    return;
                }
                let kind = if held == "minecraft:water_bucket" { fluids::Kind::Water } else { fluids::Kind::Lava };
                self.server.set_block_quiet(self.id, target.0, target.1, target.2, kind.source());
                let s = if held == "minecraft:water_bucket" { "bucket.empty_water" } else { "bucket.empty_lava" };
                let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(play_sound(s, (target.0 as f32 + 0.5, target.1 as f32 + 0.5, target.2 as f32 + 0.5))) });
                swap_bucket(&mut self.conn, &mut self.inv, self.selected_slot, &self.mode, "minecraft:bucket").await;
            }
            return;
        }
        // Items whose block has another name carry no block id from the client.
        let block_id = match (u.held_item.block_runtime_id, held) {
            (0, "minecraft:redstone") => blocks::any_state("minecraft:redstone_wire"),
            (0, "minecraft:repeater") => blocks::any_state("minecraft:unpowered_repeater"),
            (0, "minecraft:comparator") => blocks::any_state("minecraft:unpowered_comparator"),
            (0, "minecraft:string") => blocks::any_state("minecraft:trip_wire"),
            (0, "minecraft:sugar_cane") => blocks::any_state("minecraft:reeds"),
            (0, n) => blocks::any_state(n),
            (id, _) => Some(id as i32),
        };
        let Some(block_id) = block_id else {
            return; // nothing placeable in hand
        };
        use crate::overworld::feature::blocks::{is_solid, replaceable};
        // Clicking grass, snow layers, water and the like places into that block (vanilla canBeReplaced).
        let (x, y, z) = if replaceable(self.server.world.get(p.x, p.y, p.z)) { (p.x, p.y, p.z) } else { (p.x + dx, p.y + dy, p.z + dz) };
        let (pitch, yaw) = self.server.get(self.id).map(|me| me.rot).unwrap_or_default();
        let click = placement::Click { face: u.block_face, click_y: u.click_position.1, yaw, pitch };
        let mut parts: Vec<_> = placement::orient(block_id, &click)
            .into_iter()
            .map(|(o, b)| (o, if blocks::has_state(b, "persistent_bit") { blocks::with(b, &[("persistent_bit", blocks::State::Bool(true))]) } else { b }))
            .collect();
        // PNX BlockDoor.place: hinge on the right when a door is already on the left, or the
        // left is open and the right is solid (so double doors pair up).
        if blocks::has_state(block_id, "door_hinge_bit") {
            let look = (((yaw.rem_euclid(360.0) + 45.0) / 90.0) as i32) % 4; // 0 S, 1 W, 2 N, 3 E
            let (lx, lz) = [(1, 0), (0, 1), (-1, 0), (0, -1)][look as usize]; // rotateYCCW of the facing
            let left = self.server.world.get(x + lx, y, z + lz);
            let right = self.server.world.get(x - lx, y, z - lz);
            let same = blocks::entry(left).map(|e| &e.name) == blocks::entry(block_id).map(|e| &e.name);
            let open = |b: i32| !crate::overworld::feature::blocks::is_solid(b);
            if same || (!open(right) && open(left)) {
                for (_, b) in parts.iter_mut() {
                    *b = blocks::with(*b, &[("door_hinge_bit", blocks::State::Bool(true))]);
                }
            }
        }
        // All parts (both door halves, both bed halves) need free space.
        let fits = spawn::can_build(&self.mode) && !commands::spawn_protected(&self.server, &self.name, (x, y, z)) && !parts.is_empty() && parts.iter().all(|((ox, oy, oz), b)| {
            let (bx, by, bz) = (x + ox, y + oy, z + oz);
            // Torches, flowers, rails and other pass-through blocks can go where a player stands.
            replaceable(self.server.world.get(bx, by, bz)) && !(is_solid(*b) && self.server.occupied(bx, by, bz))
        }) && parts.iter().all(|((ox, oy, oz), b)| {
            // Blocks that hang on something (dust, torches, plants, rails, carpets, buttons...)
            // need a full block there, and can't go into water or lava. Lily pads need water.
            let (bx, by, bz) = (x + ox, y + oy, z + oz);
            let Some((sx, sy, sz)) = physics::support_of(*b) else { return true };
            let short = blocks::entry(*b).map_or("", |e| e.name.trim_start_matches("minecraft:"));
            let under = self.server.world.get(bx + sx, by + sy, bz + sz);
            let liquid = |q: i32| blocks::entry(q).is_some_and(|e| e.name.ends_with("water") || e.name.ends_with("lava"));
            if short == "waterlily" || short == "lily_pad" {
                return liquid(under);
            }
            // Another part of this block (the lower half of a door) holds it up.
            let own_part = parts.iter().any(|((px, py, pz), _)| (x + px, y + py, z + pz) == (bx + sx, by + sy, bz + sz));
            !liquid(self.server.world.get(bx, by, bz)) && (own_part || crate::overworld::feature::blocks::is_full(under))
        }) && self.server.get(self.id).is_some_and(|me| {
            parts.iter().all(|((ox, oy, oz), b)| self.server.plugins.block_place(&self.server, &me, (x + ox, y + oy, z + oz), *b))
        });
        if fits {
            if survival_like(&self.mode) {
                self.inv.use_one(self.selected_slot as usize);
            }
            if let Some(((ox, oy, oz), _)) = parts.iter().find(|(_, b)| signs::is_sign(*b)) {
                let _ = self.conn.send(&[signs::open_packet((x + ox, y + oy, z + oz))]).await;
            }
            let placed_chest = parts.iter().find(|(_, b)| containers::kind(*b) == Some(containers::Kind::Chest)).map(|((ox, oy, oz), _)| (x + ox, y + oy, z + oz));
            let held_contents = self.inv.held(self.selected_slot as usize).map(|s| items::contents(&s.item));
            for ((ox, oy, oz), block) in parts {
                // Placed into a water source: it holds the water (waterlogged).
                let wet = self.server.world.get(x + ox, y + oy, z + oz) == fluids::Kind::Water.source() && world::waterloggable(block);
                if wet {
                    let (bx, by, bz) = (x + ox, y + oy, z + oz);
                    let server = self.server.clone();
                    // after the block is set below
                    tokio::spawn(async move { server.set_water(bx, by, bz, true) });
                }
                if containers::kind(block) == Some(containers::Kind::Shulker)
                    && let Some(c) = held_contents.clone()
                    && c.iter().any(Option::is_some)
                {
                    self.server.containers.put((x + ox, y + oy, z + oz), c);
                }
                if containers::kind(block) == Some(containers::Kind::Hopper) {
                    self.server.containers.hoppers.lock().insert((x + ox, y + oy, z + oz));
                }
                self.server.set_block(self.id, x + ox, y + oy, z + oz, block);
                for ((fx, fy, fz), b) in physics::settle(&self.server.world, (x + ox, y + oy, z + oz)) {
                    self.server.set_block_quiet(self.id, fx, fy, fz, b);
                }
                // Rails turn to join the rails around them.
                for ((rx, ry, rz), b) in crate::minecart::placed(&self.server.world, (x + ox, y + oy, z + oz)) {
                    self.server.set_block(self.id, rx, ry, rz, b);
                }
                // A head on a finished body builds a golem or the wither.
                mobs::summon_from_blocks(&self.server, (x + ox, y + oy, z + oz));
            }
            if let Some(c) = placed_chest {
                announce_chest(&self.server, c);
            }
        } else {
            // Undo the client's predicted placement.
            let _ = self.conn.send(&[update_block(x, y, z, self.server.world.get(x, y, z))]).await;
        }
    }

    pub(super) async fn on_player_action(&mut self, a: &PlayerAction) {
        if let PlayerActionType::StopSleeping = a.action {
            blockops::wake(&self.server, self.id);
            if let Some(me) = self.server.get(self.id) {
                self.server.plugins.bed_leave(&self.server, &me);
            }
        }
        // Creative breaks instantly; survival reports the finished break as a prediction.
        if let PlayerActionType::CreativeDestroyBlock | PlayerActionType::PredictDestroyBlock = a.action {
            let p = &a.block_position;
            self.destroy_block((p.x, p.y, p.z)).await;
        }
    }

    /// Breaks the block the player finished mining (or hit in creative), with drops, wear and
    /// plugin checks; puts it back on their screen if it may not break.
    pub(super) async fn destroy_block(&mut self, p: (i32, i32, i32)) {
        if spawn::can_build(&self.mode)
            && !commands::spawn_protected(&self.server, &self.name, p)
            && self.server.world.get(p.0, p.1, p.2) != world::AIR
            && self.server.get(self.id).is_some_and(|me| self.server.plugins.block_break(&self.server, &me, p, self.server.world.get(p.0, p.1, p.2)))
        {
            let held = self.inv.held(self.selected_slot as usize).and_then(|s| items::name_of(s.item.id));
            let tool = self.inv.held(self.selected_slot as usize).map(|s| s.item.user_data_buffer.clone()).unwrap_or_default();
            break_block(&self.server, self.id, p, survival_like(&self.mode).then_some(held), &tool);
            let uses = mining_wear(held);
            wear(&mut self.conn, &self.server, self.id, &mut self.inv, self.selected_slot as usize, uses, &self.mode).await;
        } else {
            if self.server.world.get(p.0, p.1, p.2) != world::AIR {
                println!("{}: break at {p:?} refused ({:?}, spawn protected: {})", self.name, self.mode, commands::spawn_protected(&self.server, &self.name, p));
            }
            let _ = self.conn.send(&[update_block(p.0, p.1, p.2, self.server.world.get(p.0, p.1, p.2))]).await;
        }
    }

    /// Ticks this player needs to break the block at `p` with what they hold (haste counted).
    fn break_ticks_at(&self, p: (i32, i32, i32)) -> i32 {
        let held = self.inv.held(self.selected_slot as usize);
        let eff = held.map_or(0, |s| enchant::level(&s.item.user_data_buffer, enchant::EFFICIENCY));
        crate::drops::break_ticks(self.server.world.get(p.0, p.1, p.2), held.and_then(|s| items::name_of(s.item.id)), eff, self.effects.level(3)).max(1) // 3 = haste
    }

    /// Server-side breaking: the client keeps mining until the server breaks the block once its
    /// break time has passed (PNX Player.onBlockBreakContinue, run every tick).
    pub(super) async fn mining_tick(&mut self) {
        let Some(p) = self.mining else { return };
        let needed = std::time::Duration::from_millis(self.break_ticks_at(p) as u64 * 50);
        if survival_like(&self.mode) && self.mining_since.0.elapsed() >= needed {
            self.mining = None;
            let center = (p.0 as f32 + 0.5, p.1 as f32 + 0.5, p.2 as f32 + 0.5);
            let stop = Protocol::LevelEventPacket(Box::new(LevelEventPacket { event_id: 3601, position: center, data: 0 }));
            let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(stop) });
            self.destroy_block(p).await;
        }
    }

    /// Mining progress the client reports inside its movement packets (server-side block
    /// breaking), ported from PNX PlayerAuthInputHandler: everyone, the miner too, sees the
    /// cracks, and the finished break goes through.
    pub(super) async fn on_block_actions(&mut self, actions: &[bedrock::protocol::v2168::types::PlayerBlockActionData<Protocol>]) {
        let event = |id: i32, p: (i32, i32, i32), data: i32| {
            let center = (p.0 as f32 + 0.5, p.1 as f32 + 0.5, p.2 as f32 + 0.5);
            Box::new(Protocol::LevelEventPacket(Box::new(LevelEventPacket { event_id: id, position: center, data })))
        };
        for a in actions {
            let pos = &a.position;
            let p = (pos.x, pos.y, pos.z);
            let block = self.server.world.get(p.0, p.1, p.2);
            // Stop (always at 0,0,0) is the client saying it finished mining: break the tracked
            // block if its time is about up (the client's clock runs a little ahead of ours).
            if let PlayerActionType::StopDestroyBlock = a.action_type {
                if let Some(last) = self.mining
                    && survival_like(&self.mode)
                    && self.mining_since.0.elapsed() + std::time::Duration::from_millis(BREAK_SLACK_MS) >= std::time::Duration::from_millis(self.break_ticks_at(last) as u64 * 50)
                {
                    self.mining = None;
                    let _ = self.server.events.send(Event::Relay { from: 0, packet: event(3601, last, 0) });
                    self.destroy_block(last).await;
                }
                continue;
            }
            // Moved onto another block: an instant break often sends only its start, so finish
            // the last one if it had time to break, else stop its cracks.
            if let Some(last) = self.mining
                && last != p
            {
                let (since, only_started) = self.mining_since;
                let needed = std::time::Duration::from_millis((self.break_ticks_at(last) as u64 * 50).saturating_sub(50));
                self.mining = None;
                if only_started && since.elapsed() >= needed && survival_like(&self.mode) {
                    self.destroy_block(last).await;
                } else {
                    let _ = self.server.events.send(Event::Relay { from: 0, packet: event(3601, last, 0) });
                }
            }
            let is_start = matches!(a.action_type, PlayerActionType::StartDestroyBlock);
            // Cracks start on a new block: the first hit, or sliding onto the next one.
            let start = is_start || (matches!(a.action_type, PlayerActionType::CrackBlock | PlayerActionType::ContinueDestroyBlock) && self.mining != Some(p));
            if start && survival_like(&self.mode) && block != world::AIR {
                self.mining = Some(p);
                self.mining_since = (Instant::now(), is_start);
                // LevelEvent 3600 starts the crack animation; data is 65535 / break ticks. With
                // server-side breaking the miner's own client waits for it too (PNX onBlockBreakStart).
                let ticks = self.break_ticks_at(p);
                let _ = self.server.events.send(Event::Relay { from: 0, packet: event(3600, p, 65535 / ticks) });
            } else if self.mining == Some(p) && !is_start {
                self.mining_since.1 = false;
            }
            match a.action_type {
                PlayerActionType::CrackBlock | PlayerActionType::ContinueDestroyBlock => {
                    // 2014: the little punch particles, on the face being mined.
                    let face = a.facing;
                    let _ = self.server.events.send(Event::Relay { from: self.id, packet: event(2014, p, block | (face << 24)) });
                    // The mining "hit" sound, every 4 ticks like vanilla; the miner's client waits for it too.
                    if self.mining_hit.elapsed() >= std::time::Duration::from_millis(200) {
                        self.mining_hit = Instant::now();
                        let center = (p.0 as f32 + 0.5, p.1 as f32 + 0.5, p.2 as f32 + 0.5);
                        let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(level_sound("hit", center, block)) });
                    }
                }
                PlayerActionType::AbortDestroyBlock => {
                    self.mining = None;
                    let _ = self.server.events.send(Event::Relay { from: 0, packet: event(3601, p, 0) });
                }
                PlayerActionType::PredictDestroyBlock | PlayerActionType::CreativeDestroyBlock => {
                    self.mining = None;
                    let _ = self.server.events.send(Event::Relay { from: 0, packet: event(3601, p, 0) });
                    self.destroy_block(p).await;
                }
                _ => {}
            }
        }
    }
}

/// How far ahead of the server's timer a client may finish a break (network lag, tick rounding).
const BREAK_SLACK_MS: u64 = 150;

/// Which armor slot (0 helmet .. 3 boots) an item goes in when used.
fn armor_slot(name: &str) -> Option<usize> {
    if let Some(c) = crate::custom::item(name) {
        return c.armor.map(|a| a.0 as usize);
    }
    let n = name.trim_start_matches("minecraft:");
    if n.ends_with("_helmet") || n == "carved_pumpkin" || n.ends_with("_head") || n.ends_with("_skull") {
        Some(0)
    } else if n.ends_with("_chestplate") || n == "elytra" {
        Some(1)
    } else if n.ends_with("_leggings") {
        Some(2)
    } else if n.ends_with("_boots") {
        Some(3)
    } else {
        None
    }
}

/// The vanilla equip sound for this armor's material.
fn armor_sound(name: &str) -> &'static str {
    let n = name.trim_start_matches("minecraft:");
    match n.split('_').next() {
        Some("leather") => "armor.equip_leather",
        Some("chainmail") => "armor.equip_chain",
        Some("iron") => "armor.equip_iron",
        Some("golden") => "armor.equip_gold",
        Some("diamond") => "armor.equip_diamond",
        Some("netherite") => "armor.equip_netherite",
        Some("turtle") => "armor.equip_generic",
        _ if n == "elytra" => "armor.equip_elytra",
        _ => "armor.equip_generic",
    }
}

//! Using what's in hand: bows, food, dropping items, pick block.

use super::*;
use bedrock::protocol::ProtoVersionPackets;

type BlockPick = <Protocol as ProtoVersionPackets>::BlockPickRequestPacket;

impl Session {
    pub(super) async fn on_release(&mut self) {
        // Let go of a drawn bow or raised trident (PNX ItemBow/ItemTrident.onRelease).
        let slot = self.selected_slot as usize;
        let Some(started) = self.drawing.take() else { return };
        let Some(st) = self.inv.held(slot).cloned() else { return };
        let held = items::name_of(st.item.id).unwrap_or("");
        let Some(me) = self.server.get(self.id) else { return };
        let ticks = started.elapsed().as_secs_f32() * 20.0;
        let data = st.item.user_data_buffer.clone();
        if held == "minecraft:trident" && ticks >= 10.0 {
            let mut item = st.item.clone();
            item.stack_size = 1;
            self.server.mobs.player_shoot(&self.server, self.id, me.pos, me.rot, 1.0, true, 0, 0, false, Some(item), &data);
            if survival_like(&self.mode) {
                self.inv.use_one(slot);
                let _ = self.conn.send(&[self.inv.content_packet()]).await;
            }
        } else if held == "minecraft:crossbow" {
            // Charged long enough: load an arrow into it (1.25 s, 0.25 s less per Quick Charge).
            let needed = 25.0 - 5.0 * enchant::level(&data, enchant::QUICK_CHARGE) as f32;
            if ticks < needed || crate::itemdata::ItemData::read(&data).get("chargedItem").is_some() {
                return;
            }
            // Ammo: the offhand first, then the inventory; arrows or firework rockets (PNX onUse).
            let ammo = |n: Option<&str>| matches!(n, Some("minecraft:arrow" | "minecraft:firework_rocket"));
            let ammo_slot = std::iter::once(inventory::OFFHAND).chain(0..36).find(|&i| ammo(self.inv.held(i).and_then(|s| items::name_of(s.item.id))));
            let loaded = ammo_slot.and_then(|i| self.inv.held(i)).and_then(|s| items::name_of(s.item.id)).unwrap_or("minecraft:arrow");
            // A rocket keeps its stars inside the crossbow (chargedItem's tag).
            let ammo_tag = ammo_slot.and_then(|i| self.inv.held(i)).map(|s| crate::itemdata::ItemData::read(&s.item.user_data_buffer).nbt).unwrap_or_default();
            if survival_like(&self.mode) {
                let Some(a) = ammo_slot else { return };
                self.inv.use_one(a);
            }
            let mut item = st.item.clone();
            item.user_data_buffer = crossbow_loaded(&data, Some(loaded));
            if !ammo_tag.is_empty() {
                use crate::itemdata::{ItemData, Tag};
                let mut d = ItemData::read(&item.user_data_buffer);
                if let Some(Tag::Compound(mut c)) = d.get("chargedItem").cloned() {
                    c.push(("tag".into(), Tag::Compound(ammo_tag)));
                    d.set("chargedItem", Tag::Compound(c));
                    item.user_data_buffer = d.write();
                }
            }
            self.inv.set_slot(slot, item, st.count);
            let _ = self.server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound("crossbow.loading.end", me.pos)) });
            let _ = self.conn.send(&[self.inv.content_packet()]).await;
        } else if held == "minecraft:bow" {
            let f = ticks / 20.0;
            let force = ((f * f + f * 2.0) / 3.0).min(1.0);
            if force < 0.1 {
                return;
            }
            let infinity = enchant::level(&data, 22) > 0;
            let arrow_slot = (0..36).find(|&i| self.inv.held(i).and_then(|s| items::name_of(s.item.id)) == Some("minecraft:arrow"));
            if survival_like(&self.mode) && arrow_slot.is_none() {
                return; // nothing to shoot
            }
            let keep = !survival_like(&self.mode) || infinity;
            if !keep && let Some(a) = arrow_slot {
                self.inv.use_one(a);
            }
            let drop = (!keep).then(|| items::item_by_name("minecraft:arrow")).flatten();
            self.server.mobs.player_shoot(&self.server, self.id, me.pos, me.rot, force, false, enchant::level(&data, 19), enchant::level(&data, 20), enchant::level(&data, 21) > 0, drop, &data);
            wear(&mut self.conn, &self.server, self.id, &mut self.inv, slot, 1, &self.mode).await;
            let _ = self.conn.send(&[self.inv.content_packet()]).await;
        }
    }

    /// Fires a loaded crossbow. True if it was loaded (Multishot fires three, 10 degrees apart).
    pub(super) async fn fire_crossbow(&mut self) -> bool {
        let slot = self.selected_slot as usize;
        let Some(st) = self.inv.held(slot).cloned() else { return false };
        let data = st.item.user_data_buffer.clone();
        if items::name_of(st.item.id) != Some("minecraft:crossbow") || crate::itemdata::ItemData::read(&data).get("chargedItem").is_none() {
            return false;
        }
        let Some(me) = self.server.get(self.id) else { return false };
        let spread: &[f32] = if enchant::level(&data, enchant::MULTISHOT) > 0 { &[0.0, -10.0, 10.0] } else { &[0.0] };
        // The rocket's own data (its stars), rebuilt from the crossbow's chargedItem tag.
        let rocket_data = match crate::itemdata::ItemData::read(&data).get("chargedItem") {
            Some(crate::itemdata::Tag::Compound(c)) => match c.iter().find(|(k, _)| k == "tag") {
                Some((_, crate::itemdata::Tag::Compound(t))) => {
                    let mut d = crate::itemdata::ItemData::read(&[]);
                    for (k, v) in t {
                        d.set(k, v.clone());
                    }
                    d.write()
                }
                _ => vec![],
            },
            _ => vec![],
        };
        let rocket = matches!(crate::itemdata::ItemData::read(&data).get("chargedItem"), Some(crate::itemdata::Tag::Compound(c)) if c.iter().any(|(k, v)| k == "Name" && matches!(v, crate::itemdata::Tag::String(n) if n == "minecraft:firework_rocket")));
        for (i, d) in spread.iter().enumerate() {
            if rocket {
                // A loaded rocket flies straight where you aim and bursts (vanilla crossbow fireworks).
                let (pitch, yaw) = (me.rot.0.to_radians(), (me.rot.1 + d).to_radians());
                let dir = (-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos());
                crate::rockets::shoot(&self.server, (me.pos.0, me.pos.1 - 0.1, me.pos.2), (dir.0 * 1.6, dir.1 * 1.6, dir.2 * 1.6), &rocket_data, self.id);
                continue;
            }
            // Only the middle arrow can be picked back up, like vanilla.
            let drop = (i == 0 && survival_like(&self.mode)).then(|| items::item_by_name("minecraft:arrow")).flatten();
            self.server.mobs.player_shoot(&self.server, self.id, me.pos, (me.rot.0, me.rot.1 + d), 1.0, false, 0, 0, false, drop, &data);
        }
        let mut item = st.item.clone();
        item.user_data_buffer = crossbow_loaded(&data, None);
        self.inv.set_slot(slot, item, st.count);
        let _ = self.server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound("crossbow.shoot", me.pos)) });
        let uses = if spread.len() > 1 { 3 } else { 1 };
        wear(&mut self.conn, &self.server, self.id, &mut self.inv, slot, uses, &self.mode).await;
        let _ = self.conn.send(&[self.inv.content_packet()]).await;
        true
    }

    pub(super) async fn on_consume(&mut self) {
        // Finished eating (release action 1 = consume).
        let slot = self.selected_slot as usize;
        let held = self.inv.held(slot).and_then(|s| items::name_of(s.item.id)).unwrap_or("");
        if let Some(me) = self.server.get(self.id)
            && !self.server.plugins.consume(&self.server, &me, held)
        {
            let _ = self.conn.send(&[self.inv.content_packet()]).await;
            return;
        }
        let eaten = hunger::food(held);
        // Drinks and bowls give their container back (PNX ItemFood/ItemPotion/ItemBucket).
        let leftover = match held {
            "minecraft:milk_bucket" => Some("minecraft:bucket"),
            "minecraft:honey_bottle" | "minecraft:potion" => Some("minecraft:glass_bottle"),
            "minecraft:mushroom_stew" | "minecraft:rabbit_stew" | "minecraft:beetroot_soup" | "minecraft:suspicious_stew" => Some("minecraft:bowl"),
            _ => None,
        };
        // Ominous bottle: Bad Omen at the bottle's level for 100 minutes, the bottle is used up.
        let ominous = held == "minecraft:ominous_bottle";
        if (eaten.is_some() || leftover.is_some() || ominous) && survival_like(&self.mode) {
            if let Some((points, sat)) = eaten {
                self.food.eat(points, sat);
            }
            let roll = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() % 100;
            let mut gained: Vec<(i32, effects::Effect)> = effects::food(held).into_iter()
                .filter(|e| roll < e.3)
                .map(|(e, amplifier, ticks, _)| (e, effects::Effect { amplifier, ticks, particles: true }))
                .collect();
            if ominous {
                let amplifier = self.inv.held(slot).map_or(0, |s| s.item.aux_value.min(4) as i32);
                self.effects.active.remove(&effects::BAD_OMEN); // a new bottle replaces the old omen
                gained.push((effects::BAD_OMEN, effects::Effect { amplifier, ticks: 120_000, particles: true }));
            }
            if held == "minecraft:potion" {
                let aux = self.inv.held(slot).map_or(0, |s| s.item.aux_value);
                gained.extend(effects::potion(aux).into_iter().map(|(e, amplifier, ticks)| (e, effects::Effect { amplifier, ticks, particles: true })));
            }
            // Milk clears everything; honey cures poison.
            let cured: Vec<i32> = match held {
                "minecraft:milk_bucket" => self.effects.active.keys().copied().collect(),
                "minecraft:honey_bottle" => vec![effects::POISON],
                _ => vec![],
            };
            for e in cured {
                self.effects.active.remove(&e);
                let _ = self.conn.send(&[effect_packet(self.id, e, None)]).await;
            }
            let _ = self.conn.send(&[movement_packet(self.id, self.effects.speed())]).await;
            for (e, x) in gained {
                give_effect(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, &mut self.food, &mut self.effects, &mut self.inv, e, x, &self.mode).await;
            }
            self.inv.use_one(slot);
            if let Some(it) = leftover.and_then(items::item_by_name) {
                if self.inv.held(slot).is_none() {
                    self.inv.set_slot(slot, it, 1);
                } else {
                    let left = self.inv.add(it.clone(), 1);
                    if left > 0 && let Some(me) = self.server.get(self.id) {
                        self.server.entities.drop_item(&self.server, it, left, (me.pos.0, me.pos.1 - 1.0, me.pos.2));
                    }
                }
            }
            let _ = self.conn.send(&[self.food.packet(self.id), self.inv.content_packet()]).await;
        }
    }

    pub(super) async fn on_drop(&mut self, t: &Transaction) {
        // Q outside the inventory screen: a world-interaction action (the thrown
        // stack) plus a container action (the hotbar slot it came from).
        let thrown = t.actions.iter().find(|a| format!("{:?}", a.source.source_type) == "WorldInteraction").map_or(0, |a| a.to_item_descriptor.stack_size as i8);
        let slot = t.actions.iter().find(|a| format!("{:?}", a.source.source_type) == "Container").map(|a| a.slot as usize);
        if thrown > 0
            && let Some(slot) = slot
            && let Some(st) = self.inv.held(slot).cloned()
            && let Some(me) = self.server.get(self.id)
        {
            let n = thrown.min(st.count);
            // Cancelled: the slot packet below puts the item back in the client's hand.
            if self.server.plugins.item_drop(&self.server, &me, items::name_of(st.item.id).unwrap_or(""), n) {
                for _ in 0..n {
                    self.inv.use_one(slot);
                }
                let (at, vel) = throw_from(&me);
                self.server.entities.drop_item_vel(&self.server, st.item, n, at, vel);
            }
            let item = self.inv.held(slot).map_or_else(spawn::air, inventory::Stack::to_network);
            let _ = self.conn.send(&[Protocol::InventorySlotPacket(Box::new(InventorySlotPacket {
                container_id: 0, slot: slot as u32, container_name_data: None, storage_item: None, item,
            }))]).await;
        }
    }

    pub(super) async fn on_block_pick(&mut self, r: &BlockPick) {
        let p = &r.position;
        if let Some(me) = self.server.get(self.id)
            && !self.server.plugins.block_pick(&self.server, &me, (p.x, p.y, p.z), blocks::entry(self.server.world.get(p.x, p.y, p.z)).map_or("", |e| e.name.as_str()))
        {
            return;
        }
        if let Some(item) = items::item_for_block(self.server.world.get(p.x, p.y, p.z))
            && let Some(stack) = self.inv.set_slot(self.selected_slot as usize, item.clone(), items::max_stack(item.id))
        {
            let item = stack.to_network();
            let _ = self.conn.send(&[Protocol::InventorySlotPacket(Box::new(InventorySlotPacket {
                container_id: 0,
                slot: self.selected_slot,
                container_name_data: None,
                storage_item: None,
                item,
            }))]).await;
        }
    }
}

/// The crossbow's item data with an arrow loaded (`chargedItem`, which Bedrock shows) or not.
fn crossbow_loaded(data: &[u8], loaded: Option<&str>) -> Vec<u8> {
    use crate::itemdata::{ItemData, Tag};
    let mut d = ItemData::read(data);
    if let Some(name) = loaded {
        d.set("chargedItem", Tag::Compound(vec![
            ("Name".into(), Tag::String(name.into())),
            ("Count".into(), Tag::Byte(1)),
            ("Damage".into(), Tag::Short(0)),
        ]));
    } else {
        d.remove("chargedItem");
    }
    d.write()
}

#[cfg(test)]
mod crossbow_tests {
    #[test]
    fn loading_sets_and_clears_the_charged_arrow() {
        let loaded = super::crossbow_loaded(&[], Some("minecraft:arrow"));
        assert!(crate::itemdata::ItemData::read(&loaded).get("chargedItem").is_some());
        let empty = super::crossbow_loaded(&loaded, None);
        assert!(crate::itemdata::ItemData::read(&empty).get("chargedItem").is_none());
    }
}

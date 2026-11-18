//! Right-clicking and hitting mobs.

use super::*;

impl Session {
    /// Off the mount, if riding one.
    pub(super) fn get_off(&mut self) {
        if self.riding.take().is_some() {
            self.server.mobs.dismount(&self.server, self.id);
        }
    }

    pub(super) async fn on_mob_use(&mut self, t: &Transaction) {
        // Right-click on a mob: feed it, or milk a cow.
        let InventoryTransactionData::ItemUseOnEntity(e) = &t.data else { return };
        let slot = self.selected_slot as usize;
        let held = self.inv.held(slot).and_then(|s| items::name_of(s.item.id));
        let changed = match self.server.mobs.interact(&self.server, e.actor_runtime_id as i64, held, self.id) {
            mobs::Use::Trade => {
                let vid = e.actor_runtime_id as i64;
                if let Some(job) = self.server.mobs.job_of(vid) {
                    self.window_id = if self.window_id >= 99 { 2 } else { self.window_id + 1 };
                    let (tags, offers) = villagers::screen(vid, &job);
                    self.inv.trading = Some((vid, offers));
                    let _ = self.conn.send(&[trade_packet(self.window_id, vid, self.id, &job, tags)]).await;
                }
                false
            }
            mobs::Use::Consume if survival_like(&self.mode) => {
                self.inv.use_one(slot);
                true
            }
            mobs::Use::Milk if survival_like(&self.mode) => {
                self.inv.use_one(slot);
                if let Some(milk) = items::item_by_name("minecraft:milk_bucket") {
                    if self.inv.held(slot).is_none() {
                        self.inv.set_slot(slot, milk, 1);
                    } else {
                        self.inv.add(milk, 1);
                    }
                }
                true
            }
            mobs::Use::Mount => {
                self.riding = Some(e.actor_runtime_id as i64);
                false
            }
            _ => false,
        };
        if changed {
            let _ = self.conn.send(&[self.inv.content_packet()]).await;
        }
    }

    pub(super) async fn on_attack(&mut self, t: &Transaction) {
        // Attack (action 1) on a mob.
        let InventoryTransactionData::ItemUseOnEntity(e) = &t.data else { return };
        let target = e.actor_runtime_id as i64;
        if !matches!(self.mode, GameType::Spectator) && self.server.mobs.contains(target)
            && let Some(me) = self.server.get(self.id)
        {
            let held = self.inv.held(self.selected_slot as usize).and_then(|s| items::name_of(s.item.id));
            let data = self.inv.held(self.selected_slot as usize).map(|s| s.item.user_data_buffer.clone()).unwrap_or_default();
            let kind = self.server.mobs.kind_of(target);
            let bonus = enchant::melee_bonus(&data, kind.is_some_and(|k| k.undead()), kind.is_some_and(|k| k.arthropod()));
            let hit = mobs::Hit {
                knockback: enchant::level(&data, enchant::KNOCKBACK),
                fire: 40 * enchant::level(&data, enchant::FIRE_ASPECT) as u32, // 4 s per level
                looting: enchant::level(&data, enchant::LOOTING),
            };
            let dmg = self.effects.melee(mobs::attack_damage(held) + bonus);
            if kind.is_none_or(|k| self.server.plugins.entity_damage(&self.server, &me, target, k.identifier(), dmg)) {
                self.server.mobs.hurt_with(&self.server, target, dmg, me.pos, Some(self.id), hit);
            }
            let uses = attack_wear(held);
            wear(&mut self.conn, &self.server, self.id, &mut self.inv, self.selected_slot as usize, uses, &self.mode).await;
            if survival_like(&self.mode) {
                self.food.exhaust(0.1);
            }
        }
    }
}

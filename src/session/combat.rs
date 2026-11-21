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
        if !self.server.mobs.contains(e.actor_runtime_id as i64) {
            if let Some(me) = self.server.get(self.id) {
                self.server.plugins.entity_click(&self.server, &me, e.actor_runtime_id as i64, false);
            }
            return;
        }
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
            mobs::Use::OpenCart(store) => {
                self.open_cart(e.actor_runtime_id as i64, store).await;
                false
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
        // Attack (action 1) on a mob or a player.
        let InventoryTransactionData::ItemUseOnEntity(e) = &t.data else { return };
        let target = e.actor_runtime_id as i64;
        if target != self.id && self.server.get(target).is_some() {
            return self.hit_player(target).await;
        }
        if !self.server.mobs.contains(target) && self.server.get(target).is_none() {
            if let Some(me) = self.server.get(self.id) {
                self.server.plugins.entity_click(&self.server, &me, target, true);
            }
            return;
        }
        if !matches!(self.mode, GameType::Spectator) && self.server.mobs.contains(target)
            && let Some(me) = self.server.get(self.id)
        {
            let held = self.inv.held(self.selected_slot as usize).and_then(|s| items::name_of(s.item.id));
            let data = self.inv.held(self.selected_slot as usize).map(|s| s.item.user_data_buffer.clone()).unwrap_or_default();
            let kind = self.server.mobs.kind_of(target);
            let bonus = enchant::melee_bonus(&data, kind.is_some_and(|k| k.undead()), kind.is_some_and(|k| k.arthropod()));
            let hit = mobs::Hit {
                knockback: enchant::level(&data, enchant::KNOCKBACK) + self.sprinting as i16,
                fire: 40 * enchant::level(&data, enchant::FIRE_ASPECT) as u32, // 4 s per level
                looting: enchant::level(&data, enchant::LOOTING),
            };
            let dmg = self.crit(target, self.effects.melee(mobs::attack_damage(held) + bonus));
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

    /// A hit while falling is critical: 1.5x damage and the star particles on the target.
    fn crit(&self, target: i64, dmg: f32) -> f32 {
        if !self.falling {
            return dmg;
        }
        use bedrock::protocol::v898::packets::AnimatePacketAction;
        let stars = Protocol::AnimatePacket(Box::new(bedrock::protocol::v2225::packets::AnimatePacket {
            action: AnimatePacketAction::CriticalHit,
            target_runtime_id: bedrock::protocol::v662::types::ActorRuntimeID(target as u64),
            data: 0.0,
            swing_source: None,
            hand: 0,
        }));
        let _ = self.server.events.send(crate::players::Event::Relay { from: 0, packet: Box::new(stars) });
        dmg * 1.5
    }

    /// PvP: the hit goes to the victim's session as mob-style damage named after us, so armor,
    /// shields, totems, death messages and plugins (`on_player_damage` sees our name) all apply.
    async fn hit_player(&mut self, target: i64) {
        if !crate::config::get().pvp || matches!(self.mode, GameType::Spectator) {
            return;
        }
        let (Some(me), Some(them)) = (self.server.get(self.id), self.server.get(target)) else { return };
        // Vanilla gives a victim half a second between hits.
        {
            let mut last = PVP_HITS.lock();
            let last = last.get_or_insert_with(std::collections::HashMap::new);
            if last.get(&target).is_some_and(|t| t.elapsed().as_millis() < 500) {
                return;
            }
            last.insert(target, std::time::Instant::now());
        }
        let held = self.inv.held(self.selected_slot as usize).and_then(|s| items::name_of(s.item.id));
        let data = self.inv.held(self.selected_slot as usize).map(|s| s.item.user_data_buffer.clone()).unwrap_or_default();
        let dmg = self.crit(target, self.effects.melee(mobs::attack_damage(held) + enchant::melee_bonus(&data, false, false)));
        let (dx, dz) = (them.pos.0 - me.pos.0, them.pos.2 - me.pos.2);
        let len = (dx * dx + dz * dz).sqrt().max(0.01);
        // PNX: knockback 0.3, +0.5 a Knockback level; up is the same, capped at 0.4.
        // Sprinting counts as one more Knockback level.
        let push = 0.3 + 0.5 * (enchant::level(&data, enchant::KNOCKBACK) + self.sprinting as i16) as f32;
        let knock = (dx / len * push, push.min(0.4), dz / len * push);
        let _ = self.server.events.send(crate::players::Event::Damage { to: target, damage: crate::health::Damage::Mob(dmg, intern(&me.name)), knock });
        // shortcut: fire aspect doesn't set players alight yet; add it with player burning.
        let uses = attack_wear(held);
        wear(&mut self.conn, &self.server, self.id, &mut self.inv, self.selected_slot as usize, uses, &self.mode).await;
        if survival_like(&self.mode) {
            self.food.exhaust(0.1);
        }
    }
}

/// Last time each player was hit by another player.
static PVP_HITS: parking_lot::Mutex<Option<std::collections::HashMap<i64, std::time::Instant>>> = parking_lot::Mutex::new(None);

/// Damage causes are `&'static str`; player names are kept once each (one per name ever seen
/// attacking, so it stays small).
fn intern(name: &str) -> &'static str {
    static NAMES: parking_lot::Mutex<Vec<&'static str>> = parking_lot::Mutex::new(Vec::new());
    let mut names = NAMES.lock();
    if let Some(n) = names.iter().find(|n| **n == name) {
        return n;
    }
    let n: &'static str = Box::leak(name.to_string().into_boxed_str());
    names.push(n);
    n
}

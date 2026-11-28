// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Villager jobs and trading (from PNX EntityVillagerV2, TradeInventory, TradeRecipeBuildUtils
//! and CraftRecipeActionProcessor). Levels at 0/10/70/150/250 trade XP.

use std::collections::HashMap;
use parking_lot::Mutex;

use crate::trades::{self, Trade};

/// PNX tierExpRequirement
pub const TIER_XP: [i32; 5] = [0, 10, 70, 150, 250];

/// Profession (0 = none), level 1-5, trade XP, offer seed, biome outfit and per-offer
/// uses since the last restock.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub profession: i32,
    pub tier: u8,
    pub exp: i32,
    pub seed: i32,
    pub clothing: trades::Clothing,
    pub uses: Vec<i32>,
    /// job block
    pub site: Option<(i32, i32, i32)>,
    /// What this villager has heard about each player (lowercase name), by `Gossip` kind.
    pub gossip: Vec<(String, [i32; 5])>,
}

/// PNX EntityVillagerV2.Gossip: (gain, decay a day, penalty when passed on, max, multiplier).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gossip {
    MajorPositive,
    MinorPositive,
    MinorNegative,
    MajorNegative,
    Trading,
}

impl Gossip {
    const ALL: [Gossip; 5] = [Gossip::MajorPositive, Gossip::MinorPositive, Gossip::MinorNegative, Gossip::MajorNegative, Gossip::Trading];
    /// (decay, penalty, max, multiplier)
    fn rules(self) -> (i32, i32, i32, i32) {
        match self {
            Gossip::MajorPositive => (0, 100, 20, 5),
            Gossip::MinorPositive => (1, 5, 200, 1),
            Gossip::MinorNegative => (20, 20, 200, -1),
            Gossip::MajorNegative => (10, 10, 100, -5),
            Gossip::Trading => (2, 20, 25, 1),
        }
    }
}

/// Names go in the save line, which splits on spaces, commas and colons.
fn escape(name: &str) -> String {
    name.replace('%', "%25").replace(' ', "%20").replace(',', "%2C").replace(':', "%3A").replace('/', "%2F")
}

fn unescape(name: &str) -> String {
    name.replace("%2F", "/").replace("%3A", ":").replace("%2C", ",").replace("%20", " ").replace("%25", "%")
}

impl Job {
    pub fn offers(&self) -> Vec<Trade> {
        trades::trades(self.profession, self.seed, self.clothing)
    }

    /// True on level up (PNX addExperience).
    pub fn add_exp(&mut self, n: i32) -> bool {
        self.exp += n;
        let next = self.tier as usize; // index of the next level's requirement
        if next < TIER_XP.len() && self.exp >= TIER_XP[next] {
            self.tier += 1;
            return true;
        }
        false
    }

    pub fn to_text(&self) -> String {
        let uses: Vec<String> = self.uses.iter().map(|u| u.to_string()).collect();
        let site = self.site.map_or("-".to_string(), |s| format!("{},{},{}", s.0, s.1, s.2));
        let gossip: Vec<String> = self.gossip.iter().map(|(n, v)| format!("{}/{}/{}/{}/{}/{}", escape(n), v[0], v[1], v[2], v[3], v[4])).collect();
        format!("{},{},{},{},{},{},{},{}", self.profession, self.tier, self.exp, self.seed, self.clothing as i32, site.replace(',', ":"), uses.join(":"), gossip.join(":"))
    }

    /// PNX addGossip: capped at the kind's max.
    pub fn add_gossip(&mut self, player: &str, kind: Gossip, value: i32) {
        let name = player.to_lowercase();
        let i = Gossip::ALL.iter().position(|g| *g == kind).unwrap_or(0);
        let max = kind.rules().2;
        match self.gossip.iter_mut().find(|(n, _)| *n == name) {
            Some((_, v)) => v[i] = (v[i] + value).min(max),
            None => {
                let mut v = [0; 5];
                v[i] = value.min(max);
                self.gossip.push((name, v));
            }
        }
    }

    /// PNX getReputation: each kind times its multiplier.
    pub fn reputation(&self, player: &str) -> i32 {
        let name = player.to_lowercase();
        self.gossip.iter().find(|(n, _)| *n == name).map_or(0, |(_, v)| Gossip::ALL.iter().enumerate().map(|(i, g)| v[i] * g.rules().3).sum())
    }

    /// PNX spreadGossip into `self` from `other`: what's passed on loses its penalty.
    pub fn hear(&mut self, other: &Job) {
        for (name, theirs) in &other.gossip {
            let mut mine = self.gossip.iter().find(|(n, _)| n == name).map_or([0; 5], |(_, v)| *v);
            for (i, g) in Gossip::ALL.iter().enumerate() {
                mine[i] = mine[i].max(theirs[i] - g.rules().1);
            }
            match self.gossip.iter_mut().find(|(n, _)| n == name) {
                Some((_, v)) => *v = mine,
                None => self.gossip.push((name.clone(), mine)),
            }
        }
    }

    /// A day passes: every kind fades by its decay; players nobody talks about are forgotten.
    pub fn decay_gossip(&mut self) {
        for (_, v) in &mut self.gossip {
            for (i, g) in Gossip::ALL.iter().enumerate() {
                v[i] = (v[i] - g.rules().0).max(0);
            }
        }
        self.gossip.retain(|(_, v)| v.iter().any(|x| *x > 0));
    }

    pub fn from_text(t: &str) -> Option<Job> {
        let f: Vec<&str> = t.split(',').collect();
        let n = |i: usize| f.get(i).and_then(|v| v.parse::<i32>().ok());
        let site = f.get(5).filter(|s| **s != "-").and_then(|s| {
            let p: Vec<i32> = s.split(':').filter_map(|v| v.parse().ok()).collect();
            (p.len() == 3).then(|| (p[0], p[1], p[2]))
        });
        Some(Job {
            profession: n(0)?,
            tier: n(1)?.clamp(1, 5) as u8,
            exp: n(2)?,
            seed: n(3)?,
            clothing: trades::Clothing::from_index(n(4).unwrap_or(0)),
            site,
            uses: f.get(6).map(|u| u.split(':').filter_map(|v| v.parse().ok()).collect()).unwrap_or_default(),
            gossip: f
                .get(7)
                .map(|g| {
                    g.split(':')
                        .filter_map(|e| {
                            let p: Vec<&str> = e.split('/').collect();
                            let n = |i: usize| p.get(i).and_then(|v| v.parse::<i32>().ok());
                            Some((unescape(p.first()?), [n(1)?, n(2)?, n(3)?, n(4)?, n(5)?]))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

/// One villager per job block.
static CLAIMED: Mutex<Option<HashMap<(i32, i32, i32), i64>>> = Mutex::new(None);

/// Grabs a free job block within 16 blocks (PNX WorkExecutor / setProfessionBlock).
pub fn find_job(world: &crate::world::World, villager: i64, at: (i32, i32, i32)) -> Option<((i32, i32, i32), i32)> {
    let claimed = CLAIMED.lock().clone().unwrap_or_default();
    for r in 1..=16i32 {
        for dx in -r..=r {
            for dz in -r..=r {
                if dx.abs() != r && dz.abs() != r {
                    continue; // ring by ring: nearest first
                }
                for dy in -3..=3 {
                    let p = (at.0 + dx, at.1 + dy, at.2 + dz);
                    let name = crate::blocks::entry(world.get(p.0, p.1, p.2)).map_or("", |e| e.name.as_str());
                    if let Some(prof) = trades::profession_for_block(name)
                        && claimed.get(&p).is_none_or(|v| *v == villager)
                    {
                        CLAIMED.lock().get_or_insert_with(HashMap::new).insert(p, villager);
                        return Some((p, prof.index));
                    }
                }
            }
        }
    }
    None
}

/// Villager died or lost its job.
pub fn release(villager: i64) {
    if let Some(c) = CLAIMED.lock().as_mut() {
        c.retain(|_, v| *v != villager);
    }
}

pub fn claim(p: (i32, i32, i32), villager: i64) {
    CLAIMED.lock().get_or_insert_with(HashMap::new).insert(p, villager);
}

/// net id -> (villager, offer index). PNX starts at 50000.
static NET: Mutex<Option<(u32, HashMap<u32, (i64, usize)>)>> = Mutex::new(None);

#[derive(Clone, Debug)]
pub struct OpenOffer {
    pub net_id: u32,
    pub index: usize,
    pub trade: Trade,
    pub uses: i32,
}

/// Stable for as long as the server runs.
fn net_ids(villager: i64, n: usize) -> Vec<u32> {
    let mut g = NET.lock();
    let (next, map) = g.get_or_insert_with(|| (50_000, HashMap::new()));
    (0..n)
        .map(|i| {
            if let Some((&id, _)) = map.iter().find(|(_, v)| **v == (villager, i)) {
                return id;
            }
            let id = *next;
            *next += 1;
            map.insert(id, (villager, i));
            id
        })
        .collect()
}

fn item_nbt(s: &trades::Stack) -> nbtx::Value {
    let mut c: Vec<(String, nbtx::Value)> = vec![
        ("Count".into(), nbtx::Value::Byte(s.count)),
        ("Damage".into(), nbtx::Value::Short(s.damage)),
        ("Name".into(), nbtx::Value::String(s.name.clone().into())),
        ("WasPickedUp".into(), nbtx::Value::Byte(0)),
    ];
    let mut tag: Vec<(String, nbtx::Value)> = Vec::new();
    if !s.enchants.is_empty() {
        let list: Vec<nbtx::Value> = s
            .enchants
            .iter()
            .map(|&(id, lvl)| nbtx::Value::Compound([("id".to_string(), nbtx::Value::Short(id)), ("lvl".to_string(), nbtx::Value::Short(lvl))].into_iter().map(|(k, v)| (k.into(), v)).collect()))
            .collect();
        if let Ok(l) = nbtx::ValueList::try_from(list) {
            tag.push(("ench".into(), nbtx::Value::List(l)));
        }
    }
    for (k, v) in &s.extra {
        tag.push((k.clone(), nbtx::Value::Int(*v)));
    }
    if !tag.is_empty() {
        c.push(("tag".into(), nbtx::Value::Compound(tag.into_iter().map(|(k, v)| (k.into(), v)).collect())));
    }
    nbtx::Value::Compound(c.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

/// Trade screen NBT (PNX updateTrades) plus the offers it lists.
pub fn screen(villager: i64, job: &Job, player: &str) -> (HashMap<String, nbtx::Value>, Vec<OpenOffer>) {
    // PNX updateTrades: good reputation knocks the price down, bad pushes it up, never below 1.
    let rep = job.reputation(player);
    let adjust = |count: &mut i8, mult: f32| *count = (*count as i32 - (rep as f32 * mult) as i32).clamp(1, 64) as i8;
    let trades: Vec<Trade> = job
        .offers()
        .into_iter()
        .map(|mut t| {
            adjust(&mut t.buy_a.count, t.price_multiplier_a);
            if let Some(b) = t.buy_b.as_mut() {
                adjust(&mut b.count, t.price_multiplier_b);
            }
            t
        })
        .collect();
    let ids = net_ids(villager, trades.len());
    let mut open = Vec::new();
    let recipes: Vec<nbtx::Value> = trades
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let uses = job.uses.get(i).copied().unwrap_or(0);
            open.push(OpenOffer { net_id: ids[i], index: i, trade: t.clone(), uses });
            let mut c: Vec<(String, nbtx::Value)> = vec![
                ("buyA".into(), item_nbt(&t.buy_a)),
                ("buyCountA".into(), nbtx::Value::Int(t.buy_a.count as i32)),
                ("buyCountB".into(), nbtx::Value::Int(t.buy_b.as_ref().map_or(0, |b| b.count as i32))),
                ("demand".into(), nbtx::Value::Int(0)),
                ("netId".into(), nbtx::Value::Int(ids[i] as i32)),
                ("priceMultiplierA".into(), nbtx::Value::Float(t.price_multiplier_a)),
                ("priceMultiplierB".into(), nbtx::Value::Float(t.price_multiplier_b)),
                ("sell".into(), item_nbt(&t.sell)),
                ("uses".into(), nbtx::Value::Int(uses)),
                ("maxUses".into(), nbtx::Value::Int(t.max_uses)),
                ("rewardExp".into(), nbtx::Value::Byte(t.reward_exp as i8)),
                ("tier".into(), nbtx::Value::Int(t.tier as i32 - 1)),
                ("traderExp".into(), nbtx::Value::Int(t.trader_exp)),
            ];
            if let Some(b) = &t.buy_b {
                c.push(("buyB".into(), item_nbt(b)));
            }
            nbtx::Value::Compound(c.into_iter().map(|(k, v)| (k.into(), v)).collect())
        })
        .collect();
    let tiers: Vec<nbtx::Value> = TIER_XP
        .iter()
        .enumerate()
        .map(|(i, x)| nbtx::Value::Compound([(i.to_string(), nbtx::Value::Int(*x))].into_iter().map(|(k, v)| (k.into(), v)).collect()))
        .collect();
    let mut tags = HashMap::new();
    if let Ok(l) = nbtx::ValueList::try_from(recipes) {
        tags.insert("Recipes".to_string(), nbtx::Value::List(l));
    }
    if let Ok(l) = nbtx::ValueList::try_from(tiers) {
        tags.insert("TierExpRequirements".to_string(), nbtx::Value::List(l));
    }
    (tags, open)
}

/// (name, data value, enchants, extra NBT ints)
pub fn to_item(s: &trades::Stack) -> Option<bedrock::protocol::v2168::types::NetworkItemStackDescriptorV2> {
    // only one bed item on Bedrock, the colour is the data value (white 0 to black 15)
    const COLORS: [&str; 16] = ["white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black"];
    let bed = s.name.strip_prefix("minecraft:").and_then(|n| n.strip_suffix("_bed")).and_then(|c| COLORS.iter().position(|x| *x == c));
    let mut it = crate::items::item_by_name(if bed.is_some() { "minecraft:bed" } else { &s.name })?;
    it.aux_value = bed.map_or(s.damage.max(0) as u32, |c| c as u32);
    for &(id, lvl) in &s.enchants {
        it.user_data_buffer = crate::enchant::add(&it.user_data_buffer, id, lvl);
    }
    if !s.extra.is_empty() {
        let mut d = crate::itemdata::ItemData::read(&it.user_data_buffer);
        for (k, v) in &s.extra {
            d.set(k, crate::itemdata::Tag::Int(*v));
        }
        it.user_data_buffer = d.write();
    }
    Some(it)
}

/// PNX getProfessionName
pub fn title(profession: i32) -> &'static str {
    trades::professions().iter().find(|p| p.index == profession).map_or("entity.villager.name", |p| p.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_and_saving() {
        let mut j = Job { profession: 1, tier: 1, exp: 0, seed: 7, clothing: trades::Clothing::from_index(0), uses: vec![2, 0, 5], site: Some((1, 64, -3)), gossip: vec![] };
        assert!(!j.add_exp(9));
        assert!(j.add_exp(1), "10 XP: apprentice");
        assert_eq!(j.tier, 2);
        assert_eq!(Job::from_text(&j.to_text()), Some(j.clone()));
        let (tags, open) = screen(99, &j, "test");
        assert!(tags.contains_key("Recipes") && !open.is_empty());
        assert_eq!(screen(99, &j, "test").1[0].net_id, open[0].net_id, "stable net ids");
    }
}

#[cfg(test)]
mod item_tests {
    #[test]
    fn every_trade_item_exists() {
        let mut bad = std::collections::BTreeSet::new();
        for p in crate::trades::professions() {
            for seed in 0..16 {
                for c in 0..7 {
                    for t in crate::trades::trades(p.index, seed, crate::trades::Clothing::from_index(c)) {
                        for s in [Some(&t.buy_a), t.buy_b.as_ref(), Some(&t.sell)].into_iter().flatten() {
                            if super::to_item(s).is_none() {
                                bad.insert(s.name.clone());
                            }
                        }
                    }
                }
            }
        }
        assert!(bad.is_empty(), "unknown items: {bad:?}");
    }
}

#[cfg(test)]
mod gossip_tests {
    use super::*;

    fn job() -> Job {
        Job { profession: 1, tier: 1, exp: 0, seed: 7, clothing: trades::Clothing::from_index(0), uses: vec![], site: None, gossip: vec![] }
    }

    #[test]
    fn reputation_caps_spreads_and_fades() {
        let mut a = job();
        a.add_gossip("Steve Jr", Gossip::MinorNegative, 25);
        a.add_gossip("Steve Jr", Gossip::Trading, 30); // capped at 25
        assert_eq!(a.reputation("steve jr"), -25 + 25);
        // Passed on, each kind loses its penalty: minor negative 25-20=5, trading 25-20=5.
        let mut b = job();
        b.hear(&a);
        assert_eq!(b.reputation("STEVE JR"), -5 + 5);
        // A day later: minor negative fades 20, trading 2.
        a.decay_gossip();
        assert_eq!(a.reputation("steve jr"), -5 + 23);
        // Saved and loaded, odd names included.
        let back = Job::from_text(&a.to_text()).unwrap();
        assert_eq!(back.gossip, a.gossip);
        assert_eq!(back.reputation("Steve Jr"), a.reputation("Steve Jr"));
    }

    #[test]
    fn good_reputation_lowers_prices() {
        let mut j = job();
        let (_, plain) = screen(1, &j, "alex");
        j.add_gossip("alex", Gossip::MajorPositive, 20); // reputation 100
        let (_, cheap) = screen(1, &j, "alex");
        let lowered = plain.iter().zip(&cheap).any(|(p, c)| c.trade.buy_a.count < p.trade.buy_a.count);
        assert!(lowered, "a hero gets a discount");
        assert!(cheap.iter().all(|o| o.trade.buy_a.count >= 1));
    }
}

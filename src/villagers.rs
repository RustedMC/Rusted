// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Villager jobs and trading (from PNX EntityVillagerV2, TradeInventory, TradeRecipeBuildUtils
//! and CraftRecipeActionProcessor). Levels at 0/10/70/150/250 trade XP.

use std::collections::HashMap;
use std::sync::Mutex;

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
        format!("{},{},{},{},{},{},{}", self.profession, self.tier, self.exp, self.seed, self.clothing as i32, site.replace(',', ":"), uses.join(":"))
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
        })
    }
}

/// One villager per job block.
static CLAIMED: Mutex<Option<HashMap<(i32, i32, i32), i64>>> = Mutex::new(None);

/// Grabs a free job block within 16 blocks (PNX WorkExecutor / setProfessionBlock).
pub fn find_job(world: &crate::world::World, villager: i64, at: (i32, i32, i32)) -> Option<((i32, i32, i32), i32)> {
    let claimed = CLAIMED.lock().unwrap().clone().unwrap_or_default();
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
                        CLAIMED.lock().unwrap().get_or_insert_with(HashMap::new).insert(p, villager);
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
    if let Some(c) = CLAIMED.lock().unwrap().as_mut() {
        c.retain(|_, v| *v != villager);
    }
}

pub fn claim(p: (i32, i32, i32), villager: i64) {
    CLAIMED.lock().unwrap().get_or_insert_with(HashMap::new).insert(p, villager);
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
    let mut g = NET.lock().unwrap();
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
pub fn screen(villager: i64, job: &Job) -> (HashMap<String, nbtx::Value>, Vec<OpenOffer>) {
    let trades = job.offers();
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
        let mut j = Job { profession: 1, tier: 1, exp: 0, seed: 7, clothing: trades::Clothing::from_index(0), uses: vec![2, 0, 5], site: Some((1, 64, -3)) };
        assert!(!j.add_exp(9));
        assert!(j.add_exp(1), "10 XP: apprentice");
        assert_eq!(j.tier, 2);
        assert_eq!(Job::from_text(&j.to_text()), Some(j.clone()));
        let (tags, open) = screen(99, &j);
        assert!(tags.contains_key("Recipes") && !open.is_empty());
        assert_eq!(screen(99, &j).1[0].net_id, open[0].net_id, "stable net ids");
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

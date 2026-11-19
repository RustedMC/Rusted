//! Quests: daily and weekly quests picked from a pool, with progress from what players do and
//! rewards (coins, XP, a command) claimed from `/quests` (styled menu).
//!
//! plugins/quests/quests.txt, one quest per id:
//!   miner.name = Miner
//!   miner.type = break        break | mob | kill | craft | eat | play (minutes)
//!   miner.target = stone      optional: block/mob/item name must contain this
//!   miner.amount = 200
//!   miner.coins = 150
//!   miner.xp = 50
//!   miner.command =           optional, runs for the player on claim ({player} works)
//!   miner.weekly = false
//! config.txt: `daily-count` (3), `weekly-count` (3). Everyone gets the same quests each
//! day/week. Progress is kept in memory and saved every 30 seconds. Edits to the pool apply
//! at the next daily reset or restart.

use crate::players::{PlayerInfo, Server};
use crate::plugin::{Config, Plugin};
use crate::ui::{self, Form, Response};
use parking_lot::Mutex;
use std::collections::HashMap;

const NAME: &str = "quests";

#[derive(Clone)]
struct Quest {
    id: String,
    name: String,
    kind: String,
    target: String,
    amount: u32,
    coins: i64,
    xp: i32,
    command: String,
    weekly: bool,
}

fn pool() -> Vec<Quest> {
    let mut c = Config::data(NAME, "quests");
    if c.entries().next().is_none() {
        let seed: &[(&str, &str, &str, &str, u32, i64, i32, bool)] = &[
            ("miner", "Miner", "break", "stone", 200, 150, 40, false),
            ("lumberjack", "Lumberjack", "break", "log", 64, 120, 30, false),
            ("digger", "Digger", "break", "dirt", 128, 80, 20, false),
            ("zombies", "Zombie Slayer", "mob", "zombie", 20, 150, 50, false),
            ("hunter", "Monster Hunter", "mob", "", 40, 200, 60, false),
            ("chef", "Hungry", "eat", "", 10, 60, 15, false),
            ("crafter", "Crafter", "craft", "", 32, 100, 25, false),
            ("online", "Regular", "play", "", 30, 100, 20, false),
            ("excavator", "Excavator", "break", "", 2000, 1000, 250, true),
            ("warrior", "Warrior", "kill", "", 10, 1500, 300, true),
            ("exterminator", "Exterminator", "mob", "", 300, 1200, 300, true),
            ("dedicated", "Dedicated", "play", "", 300, 800, 150, true),
        ];
        for (id, name, kind, target, amount, coins, xp, weekly) in seed {
            for (k, v) in [
                ("name", name.to_string()),
                ("type", kind.to_string()),
                ("target", target.to_string()),
                ("amount", amount.to_string()),
                ("coins", coins.to_string()),
                ("xp", xp.to_string()),
                ("command", String::new()),
                ("weekly", weekly.to_string()),
            ] {
                c.set(&format!("{id}.{k}"), &v);
            }
        }
        let _ = c.save();
    }
    let ids: Vec<String> = c.entries().filter_map(|(k, _)| k.strip_suffix(".type").map(str::to_string)).collect();
    ids.into_iter()
        .map(|id| {
            let g = |f: &str| c.get(&format!("{id}.{f}")).unwrap_or("").to_string();
            Quest {
                name: if g("name").is_empty() { id.clone() } else { g("name") },
                kind: g("type"),
                target: g("target").to_lowercase(),
                amount: g("amount").parse().unwrap_or(1).max(1),
                coins: g("coins").parse().unwrap_or(0),
                xp: g("xp").parse().unwrap_or(0),
                command: g("command"),
                weekly: g("weekly") == "true",
                id,
            }
        })
        .collect()
}

fn day() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86400)
}

/// Today's and this week's quests, the same for everyone, each with its period key. Worked out
/// once a day: every block break asks.
fn active() -> Vec<(String, Quest)> {
    static CACHE: Mutex<Option<(u64, Vec<(String, Quest)>)>> = Mutex::new(None);
    let today = day();
    let mut cache = CACHE.lock();
    if let Some((d, list)) = cache.as_ref()
        && *d == today
    {
        return list.clone();
    }
    let list = pick(today);
    *cache = Some((today, list.clone()));
    list
}

fn pick(today: u64) -> Vec<(String, Quest)> {
    let (d, w) = (today, today / 7);
    let mut c = Config::load(NAME);
    let daily: usize = c.get_or("daily-count", "3").parse().unwrap_or(3);
    let weekly: usize = c.get_or("weekly-count", "3").parse().unwrap_or(3);
    let _ = c.save();
    let hash = |period: u64, id: &str| id.bytes().fold(period.wrapping_mul(0x9E37_79B9_7F4A_7C15), |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01B3));
    let mut out = Vec::new();
    for (is_weekly, period, count, tag) in [(false, d, daily, "d"), (true, w, weekly, "w")] {
        let mut qs: Vec<Quest> = pool().into_iter().filter(|q| q.weekly == is_weekly).collect();
        qs.sort_by_key(|q| hash(period, &q.id));
        out.extend(qs.into_iter().take(count).map(|q| (format!("{tag}{period}.{}", q.id), q)));
    }
    out
}

/// (lowercase player, period key) -> progress
static PROGRESS: Mutex<Option<HashMap<(String, String), u32>>> = Mutex::new(None);
static DIRTY: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn progress_of(p: &PlayerInfo, key: &str) -> u32 {
    let who = p.name.to_lowercase();
    let mut map = PROGRESS.lock();
    let map = map.get_or_insert_with(HashMap::new);
    *map.entry((who, key.to_string())).or_insert_with(|| Config::player(NAME, p).get(key).and_then(|v| v.parse().ok()).unwrap_or(0))
}

fn claimed(p: &PlayerInfo, key: &str) -> bool {
    Config::player(NAME, p).get(&format!("{key}.claimed")) == Some("true")
}

/// Adds to every active quest of `kind` whose target matches `what`.
fn bump(server: &Server, p: &PlayerInfo, kind: &str, what: &str, n: u32) {
    let what = what.trim_start_matches("minecraft:").to_lowercase();
    for (key, q) in active().into_iter().filter(|(_, q)| q.kind == kind && (q.target.is_empty() || what.contains(&q.target))) {
        let before = progress_of(p, &key);
        if before >= q.amount {
            continue;
        }
        let now = (before + n).min(q.amount);
        PROGRESS.lock().get_or_insert_with(HashMap::new).insert((p.name.to_lowercase(), key.clone()), now);
        DIRTY.lock().push(p.name.to_lowercase());
        if now == q.amount {
            server.sound(p.id, "random.levelup");
            server.title(p.id, "§a§lQuest complete", &format!("§f{} §7- claim it in /quests", q.name));
        }
    }
}

fn save(server: &Server) {
    let who: Vec<String> = std::mem::take(&mut *DIRTY.lock());
    let map = PROGRESS.lock().clone().unwrap_or_default();
    for name in who.into_iter().collect::<std::collections::HashSet<_>>() {
        let Some(p) = server.all().into_iter().find(|p| p.name.to_lowercase() == name) else { continue };
        let mut c = Config::player(NAME, &p);
        for ((n, key), v) in &map {
            if *n == name {
                c.set(key, &v.to_string());
            }
        }
        let _ = c.save();
    }
}

fn bar(done: u32, total: u32) -> String {
    let filled = (done * 10 / total.max(1)) as usize;
    format!("§a{}§8{}", "■".repeat(filled), "■".repeat(10 - filled))
}

fn open(server: &Server, p: &PlayerInfo) {
    let quests = active();
    let buttons = quests
        .iter()
        .map(|(key, q)| {
            let done = progress_of(p, key);
            let state = if claimed(p, key) {
                "§8Claimed".to_string()
            } else if done >= q.amount {
                "§a§lClick to claim!".to_string()
            } else {
                format!("{} §7{done}/{}", bar(done, q.amount), q.amount)
            };
            let tag = if q.weekly { "§d[Weekly] " } else { "§b[Daily] " };
            (format!("{tag}§f{}\n{state}", q.name), Some(icon(&q.kind)))
        })
        .collect();
    let form = Form::Simple {
        title: "rusted:Quests".into(),
        content: "§7Daily quests reset at midnight (UTC), weekly ones every 7 days.".into(),
        buttons,
    };
    ui::show(server, p.id, form, move |server, p, answer| {
        let Response::Button(i) = answer else { return };
        let Some((key, q)) = quests.get(i) else { return };
        let msg = claim(server, p, key, q);
        server.tell(p.id, &msg);
    });
}

fn icon(kind: &str) -> String {
    match kind {
        "break" => "textures/items/iron_pickaxe",
        "mob" => "textures/items/iron_sword",
        "kill" => "textures/items/diamond_sword",
        "craft" => "textures/blocks/crafting_table_front",
        "eat" => "textures/items/apple",
        _ => "textures/items/clock_item",
    }
    .into()
}

fn claim(server: &Server, p: &PlayerInfo, key: &str, q: &Quest) -> String {
    if claimed(p, key) {
        return "§7Already claimed.".into();
    }
    let done = progress_of(p, key);
    if done < q.amount {
        return format!("§7{} - {done}/{} {}", q.name, q.amount, describe(q));
    }
    let mut c = Config::player(NAME, p);
    c.set(&format!("{key}.claimed"), "true");
    let _ = c.save();
    if q.coins > 0 {
        let _ = crate::economy::deposit(&p.name, q.coins);
    }
    if q.xp > 0 {
        let _ = server.events.send(crate::players::Event::Xp { to: p.id, points: q.xp });
    }
    if !q.command.is_empty() {
        server.run_as(p.id, &crate::placeholders::fill(&q.command, server, p));
    }
    server.sound(p.id, "random.orb");
    format!("§aClaimed §f{}§a: §6{} coins§a, §a{} XP.", q.name, crate::economy::format(q.coins), q.xp)
}

fn describe(q: &Quest) -> String {
    let what = if q.target.is_empty() { String::new() } else { format!("{} ", q.target) };
    match q.kind.as_str() {
        "break" => format!("(break {what}blocks)"),
        "mob" => format!("(kill {what}mobs)"),
        "kill" => "(kill players)".into(),
        "craft" => format!("(craft {what}items)"),
        "eat" => format!("(eat {what}food)"),
        _ => "(minutes online)".into(),
    }
}

pub struct Quests;

impl Plugin for Quests {
    fn name(&self) -> &str {
        NAME
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![("quests", "Daily and weekly quests")]
    }

    fn on_server_start(&self, server: &Server) {
        server.scheduler.run_repeating(1200, 1200, |server| {
            for p in server.all() {
                bump(server, &p, "play", "", 1);
            }
            true
        });
        server.scheduler.run_repeating(600, 600, |server| {
            save(server);
            true
        });
    }

    fn on_leave(&self, server: &Server, player: &PlayerInfo) {
        save(server);
        if let Some(m) = PROGRESS.lock().as_mut() {
            let me = player.name.to_lowercase();
            m.retain(|(n, _), _| *n != me);
        }
    }

    fn on_block_break(&self, server: &Server, player: &PlayerInfo, _pos: (i32, i32, i32), block: i32) -> bool {
        let name = crate::blocks::entry(block).map(|e| e.name.clone()).unwrap_or_default();
        bump(server, player, "break", &name, 1);
        true
    }

    fn on_entity_death(&self, server: &Server, kind: &str, _pos: (f32, f32, f32), killer: Option<i64>) {
        if let Some(p) = killer.and_then(|id| server.get(id)) {
            bump(server, &p, "mob", kind, 1);
        }
    }

    fn on_player_death(&self, server: &Server, player: &PlayerInfo, message: &mut String) {
        if let Some(k) = server.all().into_iter().find(|o| o.id != player.id && message.contains(&o.name)) {
            bump(server, &k, "kill", "player", 1);
        }
    }

    fn on_craft(&self, server: &Server, player: &PlayerInfo, item: &str, count: i32) -> bool {
        bump(server, player, "craft", item, count.max(1) as u32);
        true
    }

    fn on_consume(&self, server: &Server, player: &PlayerInfo, item: &str) -> bool {
        bump(server, player, "eat", item, 1);
        true
    }

    fn on_command(&self, server: &Server, player: &PlayerInfo, _name: &str, _args: &[&str]) -> Option<String> {
        open(server, player);
        Some(String::new())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rotation_and_bar() {
        let a = super::active();
        let keys = |v: &[(String, super::Quest)]| v.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>();
        assert_eq!(keys(&a), keys(&super::pick(super::day())), "same picks within a day");
        assert!(a.iter().filter(|(_, q)| !q.weekly).count() <= 3);
        assert_eq!(super::bar(5, 10).matches('■').count(), 10);
    }
}

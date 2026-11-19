//! Player stats and leaderboards: kills, deaths, mob kills, blocks broken, playtime (minutes)
//! and balance.
//!
//! - `/stats [player]`, `/top <stat>` (styled menu)
//! - Placeholders: `{kills}` `{deaths}` `{mob_kills}` `{blocks}` `{playtime}`, and
//!   `{top:<stat>:<n>}` / `{topvalue:<stat>:<n>}` for leaderboard holograms, e.g.
//!   `/holo create topkills §6#1 §f{top:kills:1} §7- §e{topvalue:kills:1}`
//!
//! Kept in memory, saved to plugins/stats/stats.txt every 30 seconds and when players leave.

use crate::players::{PlayerInfo, Server};
use crate::plugin::{Config, Plugin};
use crate::ui::{self, Form};
use parking_lot::Mutex;
use std::collections::HashMap;

const NAME: &str = "stats";
pub const STATS: &[(&str, &str)] = &[
    ("kills", "Kills"),
    ("deaths", "Deaths"),
    ("mob_kills", "Mob kills"),
    ("blocks", "Blocks broken"),
    ("playtime", "Playtime"),
    ("balance", "Balance"),
];

/// lowercase name -> stat -> value
static DATA: Mutex<Option<HashMap<String, HashMap<String, i64>>>> = Mutex::new(None);
static DIRTY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn with<T>(f: impl FnOnce(&mut HashMap<String, HashMap<String, i64>>) -> T) -> T {
    let mut d = DATA.lock();
    let d = d.get_or_insert_with(|| {
        let mut m: HashMap<String, HashMap<String, i64>> = HashMap::new();
        for (k, v) in Config::data(NAME, "stats").entries() {
            if let (Some((who, stat)), Ok(v)) = (k.rsplit_once('.'), v.parse()) {
                m.entry(who.to_string()).or_default().insert(stat.to_string(), v);
            }
        }
        m
    });
    f(d)
}

pub fn add(player: &str, stat: &str, n: i64) {
    with(|d| *d.entry(player.to_lowercase()).or_default().entry(stat.to_string()).or_default() += n);
    DIRTY.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn get(player: &str, stat: &str) -> i64 {
    if stat == "balance" {
        return crate::economy::balance(player);
    }
    with(|d| d.get(&player.to_lowercase()).and_then(|s| s.get(stat)).copied().unwrap_or(0))
}

fn save() {
    if !DIRTY.swap(false, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let mut book = Config::data(NAME, "stats");
    with(|d| {
        for (who, stats) in d.iter() {
            for (stat, v) in stats {
                book.set(&format!("{who}.{stat}"), &v.to_string());
            }
        }
    });
    let _ = book.save();
}

/// Top `n` (name, value) for a stat, highest first.
pub fn top(stat: &str, n: usize) -> Vec<(String, i64)> {
    let mut all: Vec<(String, i64)> = if stat == "balance" {
        Config::data("economy", "balances").entries().filter_map(|(k, v)| Some((k.to_string(), v.parse().ok()?))).collect()
    } else {
        with(|d| d.iter().filter_map(|(who, s)| Some((who.clone(), *s.get(stat)?))).collect())
    };
    all.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    all.truncate(n);
    all
}

/// "125" minutes -> "2h 5m"; other stats with thousands separators.
pub fn show(stat: &str, v: i64) -> String {
    if stat == "playtime" {
        return if v >= 60 { format!("{}h {}m", v / 60, v % 60) } else { format!("{v}m") };
    }
    crate::economy::format(v)
}

/// `top:kills:1` -> the name in 1st place, `topvalue:kills:1` -> its value. "-" if empty.
pub fn top_placeholder(name: &str) -> Option<String> {
    let mut it = name.split(':');
    let kind = it.next()?;
    let stat = it.next()?;
    let n: usize = it.next()?.parse().ok()?;
    if !STATS.iter().any(|(s, _)| *s == stat) || n == 0 {
        return None;
    }
    let row = top(stat, n).into_iter().nth(n - 1);
    Some(match (kind, row) {
        ("top", Some((who, _))) => who,
        ("topvalue", Some((_, v))) => show(stat, v),
        ("top" | "topvalue", None) => "-".into(),
        _ => return None,
    })
}

fn leaderboard(server: &Server, p: &PlayerInfo, stat: &str, label: &str) {
    let rows = top(stat, 10);
    let medal = |i: usize| match i {
        0 => "§6#1",
        1 => "§f#2",
        2 => "§c#3",
        _ => "§7",
    };
    let form = Form::Simple {
        title: format!("rusted:Top {label}"),
        content: format!("§7You: §f{}", show(stat, get(&p.name, stat))),
        buttons: rows
            .iter()
            .enumerate()
            .map(|(i, (who, v))| {
                let rank = if i < 3 { medal(i).to_string() } else { format!("§7#{}", i + 1) };
                (format!("{rank} §f{who}\n§e{}", show(stat, *v)), None)
            })
            .collect(),
    };
    ui::show(server, p.id, form, |_, _, _| {});
}

pub struct Stats;

impl Plugin for Stats {
    fn name(&self) -> &str {
        NAME
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![("stats", "Shows a player's stats"), ("top", "Leaderboards: kills, deaths, mob_kills, blocks, playtime, balance")]
    }

    fn on_server_start(&self, server: &Server) {
        crate::placeholders::register("kills", |_, p| show("kills", get(&p.name, "kills")));
        crate::placeholders::register("deaths", |_, p| show("deaths", get(&p.name, "deaths")));
        crate::placeholders::register("mob_kills", |_, p| show("mob_kills", get(&p.name, "mob_kills")));
        crate::placeholders::register("blocks", |_, p| show("blocks", get(&p.name, "blocks")));
        crate::placeholders::register("playtime", |_, p| show("playtime", get(&p.name, "playtime")));
        server.scheduler.run_repeating(1200, 1200, |server| {
            for p in server.all() {
                add(&p.name, "playtime", 1);
            }
            true
        });
        server.scheduler.run_repeating(600, 600, |_| {
            save();
            true
        });
    }

    fn on_server_stop(&self, _server: &Server) {
        save();
    }

    fn on_leave(&self, _server: &Server, _player: &PlayerInfo) {
        save();
    }

    fn on_player_death(&self, server: &Server, player: &PlayerInfo, message: &mut String) {
        add(&player.name, "deaths", 1);
        // Killed by another player: their name is in the death message.
        if let Some(killer) = server.all().into_iter().find(|o| o.id != player.id && message.contains(&o.name)) {
            add(&killer.name, "kills", 1);
        }
    }

    fn on_entity_death(&self, server: &Server, _kind: &str, _pos: (f32, f32, f32), killer: Option<i64>) {
        if let Some(p) = killer.and_then(|id| server.get(id)) {
            add(&p.name, "mob_kills", 1);
        }
    }

    fn on_block_break(&self, _server: &Server, player: &PlayerInfo, _pos: (i32, i32, i32), _block: i32) -> bool {
        add(&player.name, "blocks", 1);
        true
    }

    fn usage(&self, command: &str) -> &'static [&'static str] {
        match command {
            "stats" => &["[player]"],
            "top" => &["[kills|deaths|mob_kills|blocks|playtime|balance]"],
            _ => &[],
        }
    }

    fn on_command(&self, server: &Server, player: &PlayerInfo, name: &str, args: &[&str]) -> Option<String> {
        Some(match (name, args.first()) {
            ("stats", who) => {
                let who = who.copied().unwrap_or(&player.name);
                let mut out = format!("§6§l{who}");
                for (stat, label) in STATS {
                    out.push_str(&format!("\n§7{label}: §f{}", show(stat, get(who, stat))));
                }
                out
            }
            ("top", Some(stat)) => match STATS.iter().find(|(s, _)| s == stat) {
                Some((s, label)) => {
                    leaderboard(server, player, s, label);
                    String::new()
                }
                None => format!("§cStats: {}", STATS.iter().map(|(s, _)| *s).collect::<Vec<_>>().join(", ")),
            },
            ("top", None) => {
                // Pick a board from a menu.
                let form = Form::Simple {
                    title: "rusted:Leaderboards".into(),
                    content: String::new(),
                    buttons: STATS.iter().map(|(_, l)| (format!("§f{l}\n§7Top 10"), None)).collect(),
                };
                ui::show(server, player.id, form, |server, p, answer| {
                    if let crate::ui::Response::Button(i) = answer
                        && let Some((s, l)) = STATS.get(i)
                    {
                        leaderboard(server, p, s, l);
                    }
                });
                String::new()
            }
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn leaderboard_and_placeholders() {
        super::add("zz_a", "mob_kills", 5);
        super::add("zz_b", "mob_kills", 9);
        let top = super::top("mob_kills", 50);
        let a = top.iter().position(|(w, _)| w == "zz_a").unwrap();
        let b = top.iter().position(|(w, _)| w == "zz_b").unwrap();
        assert!(b < a, "higher first");
        assert_eq!(super::show("playtime", 125), "2h 5m");
        assert!(super::top_placeholder("top:nope:1").is_none());
        assert!(super::top_placeholder("topvalue:mob_kills:1").is_some());
    }
}

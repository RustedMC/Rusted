//! Ranks, like LuckPerms + a chat formatter: groups with a prefix, suffix, chat colour,
//! parent group and permission nodes; every player is in one group.
//!
//! - `/rank` shows your rank; `/rank info <player>`, `/rank list`
//! - Ops: `/rank set <player> <group>`, `/rank create <group>`, `/rank delete <group>`,
//!   `/rank prefix <group> <text>`, `/rank suffix <group> <text>`, `/rank parent <group> <parent>`,
//!   `/rank perm <group> <node> [true|false|unset]`, `/rank weight <group> <n>`
//!
//! Data: plugins/ranks/groups.txt (`vip.prefix = §a[VIP] `, `vip.perm.homes.many = true`) and
//! plugins/ranks/players.txt (`steve = vip`). Group permissions are checked after
//! permissions.txt, so a player line there still wins.
//! Chat: `chat.format` in plugins/ranks/config.txt, with any placeholder plus `{message}`.

use crate::players::{PlayerInfo, Server};
use crate::plugin::{Config, Plugin};

const NAME: &str = "ranks";
const DEFAULT: &str = "default";

fn groups() -> Config {
    let mut g = Config::data(NAME, "groups");
    if g.entries().next().is_none() {
        // First run: a ladder most servers start from.
        for (name, prefix, weight, parent) in [
            ("default", "§7", "0", ""),
            ("vip", "§a§lVIP §r§a", "10", "default"),
            ("mvp", "§b§lMVP §r§b", "20", "vip"),
            ("mod", "§9§lMOD §r§9", "50", "mvp"),
            ("admin", "§c§lADMIN §r§c", "90", "mod"),
            ("owner", "§4§lOWNER §r§4", "100", "admin"),
        ] {
            g.set(&format!("{name}.prefix"), prefix);
            g.set(&format!("{name}.suffix"), "");
            g.set(&format!("{name}.weight"), weight);
            g.set(&format!("{name}.parent"), parent);
        }
        g.set("vip.perm.essentials.homes.5", "true");
        g.set("admin.perm.*", "true");
        let _ = g.save();
    }
    g
}

fn exists(g: &Config, group: &str) -> bool {
    g.get(&format!("{group}.weight")).is_some()
}

/// The player's group ("default" if they have none or it was deleted).
pub fn group_of(player: &str) -> String {
    let g = groups();
    Config::data(NAME, "players")
        .get(&player.to_lowercase())
        .filter(|grp| exists(&g, grp))
        .map_or(DEFAULT.into(), str::to_string)
}

/// The group and its parents, nearest first (loops and deep chains are cut off).
fn chain(g: &Config, group: &str) -> Vec<String> {
    let mut out = vec![group.to_string()];
    while out.len() < 16 {
        let parent = g.get(&format!("{}.parent", out.last().unwrap())).unwrap_or("").to_string();
        if parent.is_empty() || out.contains(&parent) || !exists(g, &parent) {
            break;
        }
        out.push(parent);
    }
    out
}

fn field(player: &str, key: &str) -> String {
    let g = groups();
    g.get(&format!("{}.{key}", group_of(player))).unwrap_or("").to_string()
}

pub fn prefix(player: &str) -> String {
    field(player, "prefix")
}

pub fn suffix(player: &str) -> String {
    field(player, "suffix")
}

/// Group name as shown to players: "VIP" for vip.
pub fn display_name(player: &str) -> String {
    group_of(player).to_uppercase()
}

/// What the player's groups say about `node`: the nearest group with a rule wins, and inside a
/// group the most specific rule (`homes.set` beats `homes.*` beats `*`).
pub fn allowed(player: &str, node: &str) -> Option<bool> {
    let g = groups();
    for group in chain(&g, &group_of(player)) {
        let head = format!("{group}.perm.");
        let best = g
            .entries()
            .filter_map(|(k, v)| Some((k.strip_prefix(&head)?, v == "true")))
            .filter(|(rule, _)| {
                *rule == node || *rule == "*" || rule.strip_suffix(".*").is_some_and(|base| node.starts_with(base) && node[base.len()..].starts_with('.'))
            })
            .max_by_key(|(rule, _)| rule.len());
        if let Some((_, grant)) = best {
            return Some(grant);
        }
    }
    None
}

/// Group names, highest weight first.
fn list(g: &Config) -> Vec<String> {
    let mut names: Vec<(String, i32)> = g
        .entries()
        .filter_map(|(k, v)| Some((k.strip_suffix(".weight")?.to_string(), v.parse().unwrap_or(0))))
        .collect();
    names.sort_by(|a, b| b.1.cmp(&a.1));
    names.into_iter().map(|(n, _)| n).collect()
}

fn clean(name: &str) -> Option<String> {
    let n = name.to_lowercase();
    (!n.is_empty() && n.len() <= 16 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')).then_some(n)
}

fn admin(args: &[&str]) -> String {
    let mut g = groups();
    let usage = "§6/rank set §f<player> <group>\n§6/rank create|delete §f<group>\n§6/rank prefix|suffix §f<group> <text>\n§6/rank parent §f<group> <parent>\n§6/rank perm §f<group> <node> [true|false|unset]\n§6/rank weight §f<group> <n>";
    let Some(group) = args.get(1).and_then(|a| clean(a)) else { return usage.into() };
    let known = exists(&g, &group);
    let msg = match (args[0], &args[2..]) {
        ("set", [grp]) => {
            let Some(grp) = clean(grp).filter(|x| exists(&g, x)) else { return format!("§cNo group called {grp}") };
            let mut players = Config::data(NAME, "players");
            players.set(&group, &grp); // `group` holds the player name here
            let _ = players.save();
            return format!("§a{} is now §f{grp}§a.", args[1]);
        }
        ("create", []) if !known => {
            g.set(&format!("{group}.prefix"), "§7");
            g.set(&format!("{group}.suffix"), "");
            g.set(&format!("{group}.weight"), "1");
            g.set(&format!("{group}.parent"), DEFAULT);
            format!("§aCreated {group}.")
        }
        ("delete", []) if known && group != DEFAULT => {
            let head = format!("{group}.");
            let keys: Vec<String> = g.entries().filter(|(k, _)| k.starts_with(&head)).map(|(k, _)| k.to_string()).collect();
            for k in keys {
                g.remove(&k);
            }
            format!("§aDeleted {group}. Its players are back in default.")
        }
        ("prefix" | "suffix", text) if known => {
            // A trailing space matters in prefixes; `_` stands in for it at the end.
            let mut t = text.join(" ");
            if t.ends_with('_') {
                t.pop();
                t.push(' ');
            }
            g.set(&format!("{group}.{}", args[0]), &t);
            format!("§a{group} {} set to §r{t}§a.", args[0])
        }
        ("parent", [parent]) if known => {
            let parent = clean(parent).unwrap_or_default();
            if !parent.is_empty() && !exists(&g, &parent) {
                return format!("§cNo group called {parent}");
            }
            g.set(&format!("{group}.parent"), &parent);
            format!("§a{group} now inherits from {}.", if parent.is_empty() { "nothing" } else { &parent })
        }
        ("perm", [node, rest @ ..]) if known => {
            let key = format!("{group}.perm.{node}");
            match rest.first().copied().unwrap_or("true") {
                "unset" => g.remove(&key),
                v @ ("true" | "false") => g.set(&key, v),
                _ => return usage.into(),
            }
            format!("§a{group}: {node} updated.")
        }
        ("weight", [n]) if known => match n.parse::<i32>() {
            Ok(n) => {
                g.set(&format!("{group}.weight"), &n.to_string());
                format!("§a{group} weight is {n}.")
            }
            Err(_) => return usage.into(),
        },
        (_, _) if !known && args[0] != "create" => return format!("§cNo group called {group}"),
        _ => return usage.into(),
    };
    let _ = g.save();
    msg
}

pub struct Ranks;

impl Plugin for Ranks {
    fn name(&self) -> &str {
        NAME
    }

    fn version(&self) -> &str {
        "1.4.2"
    }

    fn description(&self) -> &str {
        "Rank ladder with prefixes, inheritance, permissions and chat format"
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![("rank", "Shows ranks; ops manage groups and permissions")]
    }

    fn on_chat(&self, server: &Server, player: &PlayerInfo, message: &mut String) -> bool {
        let mut c = Config::load(NAME);
        let format = c.get_or("chat.format", "{prefix}{player}{suffix}§8 » §f{message}");
        let _ = c.save();
        if format.is_empty() {
            return true;
        }
        // Placeholders first, so players can't type {balance} into their own message.
        let line = crate::placeholders::fill(&format, server, player).replace("{message}", message);
        println!("<{}> {message}", player.name);
        server.broadcast(&line);
        false
    }

    fn usage(&self, command: &str) -> &'static [&'static str] {
        match command {
            "rank" => &["list", "info <player>", "set <player> <group>", "create|delete <group>", "prefix|suffix <group> <text...>", "parent <group> <parent>", "perm <group> <node> [true|false|unset]", "weight <group> <number>"],
            _ => &[],
        }
    }

    fn on_command(&self, _server: &Server, player: &PlayerInfo, _name: &str, args: &[&str]) -> Option<String> {
        let g = groups();
        Some(match args {
            [] => format!("§7Your rank: §r{}{}", prefix(&player.name), group_of(&player.name)),
            ["info", who] => {
                let grp = group_of(who);
                format!("§7{who}: §r{}{grp} §7(inherits {})", prefix(who), chain(&g, &grp)[1..].join(", "))
            }
            ["list"] => {
                let mut out = String::from("§6§lRanks");
                for grp in list(&g) {
                    out.push_str(&format!("\n§r{}{grp} §8(weight {})", g.get(&format!("{grp}.prefix")).unwrap_or(""), g.get(&format!("{grp}.weight")).unwrap_or("0")));
                }
                out
            }
            _ if !crate::commands::is_op(&player.name) && !matches!(allowed(&player.name, "ranks.admin"), Some(true)) => {
                "§cYou do not have permission to use this command".into()
            }
            _ => admin(args),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::plugin::Config;

    #[test]
    fn inheritance_and_specific_rules() {
        let mut g = Config::data("ranks-test", "groups");
        for (k, v) in [("a.weight", "1"), ("a.parent", "b"), ("b.weight", "2"), ("b.parent", "a"), ("a.perm.x.*", "true"), ("a.perm.x.y", "false"), ("b.perm.z", "true")] {
            g.set(k, v);
        }
        assert_eq!(super::chain(&g, "a"), vec!["a".to_string(), "b".to_string()], "loops stop");
        assert_eq!(super::clean("VIP"), Some("vip".into()));
        assert_eq!(super::clean("a b"), None);
    }
}

//! WorldGuard: protected regions with flags. Select an area with the WorldEdit wand, then
//! `/rg define <name>`.
//!
//! - `/rg define|redefine|delete <name>`, `/rg list`, `/rg info [name]` (or where you stand)
//! - `/rg flag <name> <flag> <value>`: `build`, `pvp`, `use`, `entry`, `mob-spawning`,
//!   `explosions` take `allow`/`deny`; `greeting`/`farewell` take text (`none` clears)
//! - `/rg addmember|removemember <name> <player>`: members (and ops) can always build,
//!   use and enter
//! - `/rg priority <name> <n>`: where regions overlap, the highest priority decides
//!
//! New regions deny build and use for non-members and allow everything else.
//! Stored in plugins/worldguard/regions.txt.

use crate::players::{PlayerInfo, Server};
use crate::plugin::{Config, Plugin};
use parking_lot::Mutex;
use std::collections::HashMap;

const NAME: &str = "worldguard";
type Pos = (i32, i32, i32);
const FLAGS: &[&str] = &["build", "pvp", "use", "entry", "mob-spawning", "explosions", "greeting", "farewell"];

#[derive(Clone)]
struct Region {
    name: String,
    min: Pos,
    max: Pos,
    priority: i32,
    members: Vec<String>,
    flags: HashMap<String, String>,
}

/// Regions are checked on every move, so they're kept in memory and re-read after a change.
static CACHE: Mutex<Option<Vec<Region>>> = Mutex::new(None);

fn load() -> Vec<Region> {
    CACHE.lock().get_or_insert_with(read).clone()
}

fn read() -> Vec<Region> {
    let b = Config::data(NAME, "regions");
    let names: Vec<String> = b.entries().filter_map(|(k, _)| k.strip_suffix(".min").map(str::to_string)).collect();
    names
        .into_iter()
        .filter_map(|name| {
            let pos = |f: &str| -> Option<Pos> {
                let n: Vec<i32> = b.get(&format!("{name}.{f}"))?.split(',').filter_map(|x| x.parse().ok()).collect();
                (n.len() == 3).then(|| (n[0], n[1], n[2]))
            };
            let head = format!("{name}.flag.");
            Some(Region {
                min: pos("min")?,
                max: pos("max")?,
                priority: b.get(&format!("{name}.priority")).and_then(|v| v.parse().ok()).unwrap_or(0),
                members: b.get(&format!("{name}.members")).unwrap_or("").split(',').filter(|m| !m.is_empty()).map(str::to_lowercase).collect(),
                flags: b.entries().filter_map(|(k, v)| Some((k.strip_prefix(&head)?.to_string(), v.to_string()))).collect(),
                name,
            })
        })
        .collect()
}

fn save(r: &Region) {
    let mut b = Config::data(NAME, "regions");
    let n = &r.name;
    b.set(&format!("{n}.min"), &format!("{},{},{}", r.min.0, r.min.1, r.min.2));
    b.set(&format!("{n}.max"), &format!("{},{},{}", r.max.0, r.max.1, r.max.2));
    b.set(&format!("{n}.priority"), &r.priority.to_string());
    b.set(&format!("{n}.members"), &r.members.join(","));
    for f in FLAGS {
        match r.flags.get(*f) {
            Some(v) => b.set(&format!("{n}.flag.{f}"), v),
            None => b.remove(&format!("{n}.flag.{f}")),
        }
    }
    let _ = b.save();
    *CACHE.lock() = None;
}

fn delete(name: &str) {
    let mut b = Config::data(NAME, "regions");
    let head = format!("{name}.");
    let keys: Vec<String> = b.entries().filter(|(k, _)| k.starts_with(&head)).map(|(k, _)| k.to_string()).collect();
    for k in keys {
        b.remove(&k);
    }
    let _ = b.save();
    *CACHE.lock() = None;
}

fn inside(r: &Region, p: Pos) -> bool {
    (r.min.0..=r.max.0).contains(&p.0) && (r.min.1..=r.max.1).contains(&p.1) && (r.min.2..=r.max.2).contains(&p.2)
}

/// Regions at a point, highest priority first.
fn at(p: Pos) -> Vec<Region> {
    let mut rs: Vec<Region> = load().into_iter().filter(|r| inside(r, p)).collect();
    rs.sort_by(|a, b| b.priority.cmp(&a.priority));
    rs
}

/// What the top region with an opinion says about `flag` at `p` for `player`. Members and ops
/// bypass build/use/entry. True = allowed.
fn allowed(p: Pos, flag: &str, player: Option<&str>) -> bool {
    let bypass = |r: &Region| {
        player.is_some_and(|n| crate::commands::is_op(n) || r.members.contains(&n.to_lowercase())) && matches!(flag, "build" | "use" | "entry")
    };
    for r in at(p) {
        if bypass(&r) {
            return true;
        }
        let default = if matches!(flag, "build" | "use") { "deny" } else { "allow" };
        return r.flags.get(flag).map_or(default, String::as_str) != "deny";
    }
    true
}

fn block(p: (f32, f32, f32)) -> Pos {
    (p.0.floor() as i32, p.1.floor() as i32, p.2.floor() as i32)
}

fn feet(p: &PlayerInfo) -> Pos {
    block((p.pos.0, p.pos.1 - 1.62, p.pos.2))
}

/// The top region each player was last in, for greetings.
static LAST: Mutex<Option<HashMap<i64, String>>> = Mutex::new(None);

fn clean(name: &str) -> Option<String> {
    let n = name.to_lowercase();
    (!n.is_empty() && n.len() <= 32 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')).then_some(n)
}

fn command(p: &PlayerInfo, args: &[&str]) -> String {
    let find = |n: &str| load().into_iter().find(|r| r.name == n.to_lowercase());
    match args {
        ["define" | "redefine", name] => {
            let Some(name) = clean(name) else { return "§cRegion names are letters, numbers, _ and -".into() };
            let Some((min, max)) = crate::worldedit::selection(&p.name) else { return "§cSelect an area with the wand first (/wand).".into() };
            let existing = find(&name);
            if args[0] == "define" && existing.is_some() {
                return format!("§c{name} exists. Use /rg redefine {name}.");
            }
            let mut r = existing.unwrap_or(Region { name: name.clone(), min, max, priority: 0, members: vec![p.name.to_lowercase()], flags: HashMap::new() });
            r.min = min;
            r.max = max;
            save(&r);
            format!("§aRegion §f{name} §asaved. Non-members can't build or use things in it.")
        }
        ["delete", name] => {
            delete(&name.to_lowercase());
            format!("§aRegion {name} deleted.")
        }
        ["list"] => {
            let names: Vec<String> = load().into_iter().map(|r| r.name).collect();
            if names.is_empty() { "§7No regions.".into() } else { format!("§7Regions: §f{}", names.join("§7, §f")) }
        }
        ["info", rest @ ..] => {
            let r = match rest.first() {
                Some(n) => find(n),
                None => at(feet(p)).into_iter().next(),
            };
            let Some(r) = r else { return "§7No region here.".into() };
            let flags: Vec<String> = r.flags.iter().map(|(k, v)| format!("{k}={v}")).collect();
            format!(
                "§6§l{} §7(priority {})\n§7From §f{:?} §7to §f{:?}\n§7Members: §f{}\n§7Flags: §f{}",
                r.name, r.priority, r.min, r.max,
                if r.members.is_empty() { "none".into() } else { r.members.join(", ") },
                if flags.is_empty() { "defaults".into() } else { flags.join(", ") }
            )
        }
        ["flag", name, flag, value @ ..] if !value.is_empty() => {
            let Some(mut r) = find(name) else { return format!("§cNo region {name}.") };
            if !FLAGS.contains(flag) {
                return format!("§cFlags: {}", FLAGS.join(", "));
            }
            let v = value.join(" ");
            if matches!(*flag, "greeting" | "farewell") {
                if v == "none" { r.flags.remove(*flag) } else { r.flags.insert(flag.to_string(), v.clone()) };
            } else if v == "allow" || v == "deny" {
                r.flags.insert(flag.to_string(), v.clone());
            } else {
                return "§cUse allow or deny.".into();
            }
            save(&r);
            format!("§a{name}: {flag} = {v}")
        }
        ["addmember" | "removemember", name, who] => {
            let Some(mut r) = find(name) else { return format!("§cNo region {name}.") };
            let who = who.to_lowercase();
            r.members.retain(|m| *m != who);
            if args[0] == "addmember" {
                r.members.push(who.clone());
            }
            save(&r);
            format!("§a{name} members: {}", r.members.join(", "))
        }
        ["priority", name, n] => match (find(name), n.parse()) {
            (Some(mut r), Ok(n)) => {
                r.priority = n;
                save(&r);
                format!("§a{name} priority {n}.")
            }
            _ => "§cUsage: /rg priority <name> <number>".into(),
        },
        _ => "§6/rg §fdefine|redefine|delete|list|info|flag|addmember|removemember|priority".into(),
    }
}

pub struct WorldGuard;

impl Plugin for WorldGuard {
    fn name(&self) -> &str {
        NAME
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![("rg", "Region protection (ops)"), ("region", "Region protection (ops)")]
    }

    fn on_block_break(&self, server: &Server, player: &PlayerInfo, pos: Pos, _block: i32) -> bool {
        let ok = allowed(pos, "build", Some(&player.name));
        if !ok {
            server.actionbar(player.id, "§cYou can't build here.");
        }
        ok
    }

    fn on_block_place(&self, server: &Server, player: &PlayerInfo, pos: Pos, _block: i32) -> bool {
        let ok = allowed(pos, "build", Some(&player.name));
        if !ok {
            server.actionbar(player.id, "§cYou can't build here.");
        }
        ok
    }

    fn on_container_open(&self, server: &Server, player: &PlayerInfo, pos: Pos, _block: &str) -> bool {
        let ok = allowed(pos, "use", Some(&player.name));
        if !ok {
            server.actionbar(player.id, "§cYou can't use that here.");
        }
        ok
    }

    fn on_player_damage(&self, server: &Server, player: &PlayerInfo, cause: &str, _amount: f32) -> bool {
        // A player hit (the cause is their name): both must be somewhere PvP is allowed.
        let Some(attacker) = server.find_by_name(cause).and_then(|id| server.get(id)) else { return true };
        allowed(feet(player), "pvp", None) && allowed(feet(&attacker), "pvp", None)
    }

    fn on_entity_spawn(&self, _server: &Server, _kind: &str, pos: (f32, f32, f32)) -> bool {
        allowed(block(pos), "mob-spawning", None)
    }

    fn on_explosion(&self, _server: &Server, pos: (f32, f32, f32), _power: f32) -> bool {
        allowed(block(pos), "explosions", None)
    }

    fn on_move(&self, server: &Server, player: &PlayerInfo, to: (f32, f32, f32)) -> bool {
        let here = block((to.0, to.1 - 1.62, to.2));
        if !allowed(here, "entry", Some(&player.name)) {
            server.actionbar(player.id, "§cYou can't go in there.");
            return false;
        }
        let top = at(here).into_iter().next();
        let mut last = LAST.lock();
        let last = last.get_or_insert_with(HashMap::new);
        let now = top.as_ref().map(|r| r.name.clone());
        if last.get(&player.id) != now.as_ref() {
            if let Some(old) = last.get(&player.id).and_then(|n| load().into_iter().find(|r| &r.name == n))
                && let Some(bye) = old.flags.get("farewell")
            {
                server.actionbar(player.id, bye);
            }
            if let Some(hi) = top.as_ref().and_then(|r| r.flags.get("greeting")) {
                server.actionbar(player.id, hi);
            }
            match now {
                Some(n) => last.insert(player.id, n),
                None => last.remove(&player.id),
            };
        }
        true
    }

    fn on_leave(&self, _server: &Server, player: &PlayerInfo) {
        if let Some(l) = LAST.lock().as_mut() {
            l.remove(&player.id);
        }
    }

    fn usage(&self, command: &str) -> &'static [&'static str] {
        match command {
            "rg" => &["define|redefine|delete <name>", "list", "info [name]", "flag <name> build|pvp|use|entry|mob-spawning|explosions|greeting|farewell <value...>", "addmember|removemember <name> <player>", "priority <name> <number>"],
            _ => &[],
        }
    }

    fn on_command(&self, _server: &Server, player: &PlayerInfo, _name: &str, args: &[&str]) -> Option<String> {
        if !crate::commands::is_op(&player.name) {
            return Some("§cYou do not have permission to use this command".into());
        }
        Some(command(player, args))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds() {
        let r = Region { name: "x".into(), min: (0, 0, 0), max: (10, 5, 10), priority: 0, members: vec![], flags: HashMap::new() };
        assert!(inside(&r, (0, 0, 0)) && inside(&r, (10, 5, 10)));
        assert!(!inside(&r, (11, 0, 0)) && !inside(&r, (0, -1, 0)));
        assert_eq!(clean("Spawn"), Some("spawn".into()));
    }
}

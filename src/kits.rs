//! Kits: `/kit` opens a menu of kits (styled by the lobby pack), `/kit <name>` claims one.
//! Each kit has items, a cooldown and an optional permission (`kits.<name>`, from ranks or
//! permissions.txt). Also drops a player's head when they're killed (`heads.enabled`).
//!
//! plugins/kits/kits.txt:
//!   starter.items = minecraft:stone_sword 1, minecraft:bread 16
//!   starter.cooldown = 3600        (seconds, 0 = once ever is `-1`)
//!   starter.icon = textures/items/stone_sword
//!   starter.public = true          (false = needs the kits.starter permission)

use crate::players::{PlayerInfo, Server};
use crate::plugin::{self, Config, Plugin};
use crate::ui::{self, Form, Response};

const NAME: &str = "kits";

struct Kit {
    name: String,
    items: Vec<(String, i8)>,
    cooldown: i64,
    icon: String,
    public: bool,
}

fn kits() -> Vec<Kit> {
    let mut c = Config::data(NAME, "kits");
    if c.entries().next().is_none() {
        for (k, v) in [
            ("starter.items", "minecraft:stone_sword 1, minecraft:stone_pickaxe 1, minecraft:bread 16"),
            ("starter.cooldown", "3600"),
            ("starter.icon", "textures/items/stone_sword"),
            ("starter.public", "true"),
            ("vip.items", "minecraft:iron_sword 1, minecraft:iron_pickaxe 1, minecraft:cooked_beef 32, minecraft:golden_apple 2"),
            ("vip.cooldown", "86400"),
            ("vip.icon", "textures/items/iron_sword"),
            ("vip.public", "false"),
        ] {
            c.set(k, v);
        }
        let _ = c.save();
    }
    let names: Vec<String> = c.entries().filter_map(|(k, _)| k.strip_suffix(".items").map(str::to_string)).collect();
    names
        .into_iter()
        .map(|name| Kit {
            items: parse_items(c.get(&format!("{name}.items")).unwrap_or("")),
            cooldown: c.get(&format!("{name}.cooldown")).and_then(|v| v.parse().ok()).unwrap_or(0),
            icon: c.get(&format!("{name}.icon")).unwrap_or("textures/items/chest_minecart").to_string(),
            public: c.get(&format!("{name}.public")) != Some("false"),
            name,
        })
        .collect()
}

/// "minecraft:bread 16, minecraft:apple" -> [(bread, 16), (apple, 1)]
fn parse_items(text: &str) -> Vec<(String, i8)> {
    text.split(',')
        .filter_map(|part| {
            let mut it = part.split_whitespace();
            let id = it.next()?.to_string();
            let n = it.next().and_then(|n| n.parse().ok()).unwrap_or(1i8).clamp(1, 64);
            Some((id, n))
        })
        .collect()
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn can_use(p: &PlayerInfo, kit: &Kit) -> bool {
    kit.public || crate::commands::is_op(&p.name) || crate::ranks::allowed(&p.name, &format!("kits.{}", kit.name)) == Some(true)
}

/// Seconds until `kit` can be claimed again (0 = now), or None if it was a one-time kit.
fn wait(p: &PlayerInfo, kit: &Kit) -> Option<i64> {
    let last: i64 = Config::player(NAME, p).get(&kit.name).and_then(|v| v.parse().ok()).unwrap_or(0);
    if last == 0 {
        return Some(0);
    }
    if kit.cooldown < 0 {
        return None;
    }
    Some((last + kit.cooldown - now()).max(0))
}

fn time(secs: i64) -> String {
    match secs {
        s if s >= 86400 => format!("{}d {}h", s / 86400, s % 86400 / 3600),
        s if s >= 3600 => format!("{}h {}m", s / 3600, s % 3600 / 60),
        s if s >= 60 => format!("{}m {}s", s / 60, s % 60),
        s => format!("{s}s"),
    }
}

fn status(p: &PlayerInfo, kit: &Kit) -> String {
    if !can_use(p, kit) {
        return "§cLocked".into();
    }
    match wait(p, kit) {
        Some(0) => "§aReady".into(),
        Some(s) => format!("§7{}", time(s)),
        None => "§8Claimed".into(),
    }
}

fn claim(server: &Server, p: &PlayerInfo, kit: &Kit) -> String {
    if !can_use(p, kit) {
        return format!("§cYou need a higher rank for the {} kit.", kit.name);
    }
    match wait(p, kit) {
        Some(0) => {}
        Some(s) => return format!("§cYou can claim {} again in {}.", kit.name, time(s)),
        None => return format!("§cYou already claimed {}.", kit.name),
    }
    for (id, n) in &kit.items {
        if let Some(item) = plugin::item(id) {
            server.give(p.id, item.count(*n).build());
        }
    }
    let mut d = Config::player(NAME, p);
    d.set(&kit.name, &now().to_string());
    let _ = d.save();
    server.sound(p.id, "random.levelup");
    format!("§aClaimed the §f{} §akit.", kit.name)
}

fn open_menu(server: &Server, p: &PlayerInfo) {
    let list = kits();
    let form = Form::Simple {
        title: "rusted_grid:Kits".into(),
        content: String::new(),
        buttons: list.iter().map(|k| (format!("§f{}\n{}", capital(&k.name), status(p, k)), Some(k.icon.clone()))).collect(),
    };
    ui::show(server, p.id, form, move |server, player, answer| {
        if let Response::Button(i) = answer
            && let Some(kit) = list.get(i)
        {
            let msg = claim(server, player, kit);
            server.tell(player.id, &msg);
        }
    });
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
}

pub struct Kits;

impl Plugin for Kits {
    fn name(&self) -> &str {
        NAME
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![("kit", "Opens the kits menu, or claims a kit"), ("kits", "Opens the kits menu")]
    }

    fn on_player_death(&self, server: &Server, player: &PlayerInfo, message: &mut String) {
        let mut c = Config::load(NAME);
        let heads = c.get_or("heads.enabled", "true") == "true";
        let _ = c.save();
        // Only player kills drop a head: "X was slain by Y".
        let killed = server.all().iter().any(|o| o.id != player.id && message.contains(&o.name));
        if heads
            && killed
            && let Some(head) = plugin::item("minecraft:player_head").or_else(|| plugin::item("minecraft:skull"))
        {
            let stack = head.name(&format!("§e{}'s Head", player.name)).build();
            server.entities.drop_item(server, stack.item, 1, (player.pos.0, player.pos.1 - 1.0, player.pos.2));
        }
    }

    fn usage(&self, command: &str) -> &'static [&'static str] {
        match command {
            "kit" => &["[name]"],
            _ => &[],
        }
    }

    fn on_command(&self, server: &Server, player: &PlayerInfo, _name: &str, args: &[&str]) -> Option<String> {
        match args.first() {
            None => {
                open_menu(server, player);
                Some(String::new())
            }
            Some(want) => Some(match kits().into_iter().find(|k| k.name.eq_ignore_ascii_case(want)) {
                Some(kit) => claim(server, player, &kit),
                None => format!("§cNo kit called {want}."),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn items_and_times() {
        assert_eq!(super::parse_items("minecraft:bread 16, minecraft:apple"), vec![("minecraft:bread".into(), 16), ("minecraft:apple".into(), 1)]);
        assert_eq!(super::time(90), "1m 30s");
        assert_eq!(super::time(90000), "1d 1h");
    }
}

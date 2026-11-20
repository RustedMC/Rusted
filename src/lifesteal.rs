//! Lifesteal: kill a player to take one of their hearts. Hit 0 hearts and you're eliminated
//! until someone revives you. Off by default (`enabled = true` in plugins/lifesteal/config.txt).
//!
//! - `/withdraw [n]` turns hearts into Heart items (right-click one to use it)
//! - `/revive <player>` spends `revive-cost` of your hearts to bring an eliminated player back
//! - `/hearts [player]`, and for ops `/lifesteal set <player> <hearts>`
//! - Placeholder `{hearts}`
//!
//! Settings: `start-hearts` (10), `max-hearts` (20), `revive-cost` (5), `revive-hearts` (3).

use crate::inventory::Stack;
use crate::players::{PlayerInfo, Server};
use crate::plugin::{self, Config, Plugin};

const NAME: &str = "lifesteal";

fn setting(key: &str, default: &str) -> i32 {
    let mut c = Config::load(NAME);
    let v = c.get_or(key, default).parse().unwrap_or(0);
    let _ = c.save();
    v
}

fn on() -> bool {
    let mut c = Config::load(NAME);
    let v = c.get_or("enabled", "false") == "true";
    let _ = c.save();
    v
}

pub fn hearts(player: &str) -> i32 {
    Config::data(NAME, "hearts").get(&player.to_lowercase()).and_then(|v| v.parse().ok()).unwrap_or_else(|| setting("start-hearts", "10"))
}

/// Saves `n` hearts (0..=max) and applies them if the player is online.
fn set_hearts(server: &Server, player: &str, n: i32) -> i32 {
    let n = n.clamp(0, setting("max-hearts", "20").max(1));
    let mut book = Config::data(NAME, "hearts");
    book.set(&player.to_lowercase(), &n.to_string());
    let _ = book.save();
    if n > 0
        && let Some(id) = server.find_by_name(player)
    {
        crate::health::set_max(id, n as f32 * 2.0);
    }
    n
}

fn heart_item(count: i8) -> Option<Stack> {
    let mut st = plugin::item("minecraft:nether_star")?.name("§c§l❤ Heart").lore(&["§7Right-click to gain a heart"]).tag(NAME, "heart").build();
    st.count = count;
    Some(st)
}

fn eliminate(server: &Server, p: &PlayerInfo) {
    server.broadcast(&format!("§4☠ §c{} §7lost their last heart and was eliminated.", p.name));
    let id = p.id;
    // After the death screen, so the kill itself finishes normally.
    server.scheduler.run_later(20, move |server| server.kick(id, "§cYou ran out of hearts.\n§7Ask a friend to /revive you."));
}

pub struct Lifesteal;

impl Plugin for Lifesteal {
    fn name(&self) -> &str {
        NAME
    }

    fn version(&self) -> &str {
        "1.0.1"
    }

    fn description(&self) -> &str {
        "Lifesteal: steal hearts on kills, withdraw, revive"
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("withdraw", "Turns hearts into Heart items"),
            ("revive", "Spends hearts to bring back an eliminated player"),
            ("hearts", "Shows how many hearts someone has"),
            ("lifesteal", "Lifesteal admin (ops)"),
        ]
    }

    fn on_server_start(&self, _server: &Server) {
        crate::placeholders::register("hearts", |_, p| hearts(&p.name).to_string());
    }

    fn on_pre_login(&self, _server: &Server, name: &str, _xuid: &str) -> Option<String> {
        (on() && hearts(name) <= 0).then(|| "§cYou're out of hearts.\n§7Ask a friend to /revive you.".into())
    }

    fn on_join(&self, _server: &Server, player: &PlayerInfo) {
        if on() {
            crate::health::set_max(player.id, hearts(&player.name) as f32 * 2.0);
        }
    }

    fn on_player_death(&self, server: &Server, player: &PlayerInfo, message: &mut String) {
        if !on() {
            return;
        }
        // Only player kills move hearts; the killer's name is in the death message.
        let Some(killer) = server.all().into_iter().find(|o| o.id != player.id && message.contains(&o.name)) else { return };
        let left = set_hearts(server, &player.name, hearts(&player.name) - 1);
        let max = setting("max-hearts", "20");
        let mine = hearts(&killer.name);
        if mine < max {
            set_hearts(server, &killer.name, mine + 1);
            server.tell(killer.id, &format!("§a+1 ❤ §7You now have §c{} hearts§7.", mine + 1));
        } else if let Some(h) = heart_item(1) {
            // At the cap, the heart drops as an item instead.
            server.give(killer.id, h);
            server.tell(killer.id, "§7You're at max hearts, so you got a Heart item.");
        }
        message.push_str(&format!(" §c(-1 ❤, {left} left)"));
        if left == 0 {
            eliminate(server, player);
        }
    }

    fn on_item_use(&self, server: &Server, player: &PlayerInfo, item: &Stack) -> bool {
        if !on() || plugin::tag(item, NAME).as_deref() != Some("heart") {
            return true;
        }
        let max = setting("max-hearts", "20");
        if hearts(&player.name) >= max {
            server.tell(player.id, &format!("§cYou're at the max of {max} hearts."));
            return false;
        }
        let name = player.name.clone();
        server.take_held(player.id, 1, move |server, got| {
            if got.is_some() {
                let n = set_hearts(server, &name, hearts(&name) + 1);
                if let Some(id) = server.find_by_name(&name) {
                    server.sound(id, "random.levelup");
                    server.tell(id, &format!("§a+1 ❤ §7You now have §c{n} hearts§7."));
                }
            }
        });
        false
    }

    fn usage(&self, command: &str) -> &'static [&'static str] {
        match command {
            "withdraw" => &["[amount]"],
            "revive" => &["<player>"],
            "hearts" => &["[player]"],
            "lifesteal" => &["set <player> <hearts>"],
            _ => &[],
        }
    }

    fn on_command(&self, server: &Server, player: &PlayerInfo, name: &str, args: &[&str]) -> Option<String> {
        if !on() {
            return Some("§cLifesteal is off on this server.".into());
        }
        Some(match (name, args) {
            ("hearts", []) => format!("§7You have §c{} hearts§7.", hearts(&player.name)),
            ("hearts", [who]) => format!("§7{who} has §c{} hearts§7.", hearts(who)),
            ("withdraw", rest) => {
                let n: i32 = rest.first().and_then(|n| n.parse().ok()).unwrap_or(1);
                let have = hearts(&player.name);
                if n < 1 || n >= have {
                    return Some(format!("§cYou can withdraw 1 to {} (you keep at least one).", have - 1));
                }
                set_hearts(server, &player.name, have - n);
                if let Some(h) = heart_item(n.min(64) as i8) {
                    server.give(player.id, h);
                }
                format!("§7Withdrew §c{n} ❤§7. You have §c{} §7left.", have - n)
            }
            ("revive", [who]) => {
                if hearts(who) > 0 {
                    return Some(format!("§c{who} isn't eliminated."));
                }
                let cost = setting("revive-cost", "5");
                let have = hearts(&player.name);
                if have <= cost {
                    return Some(format!("§cReviving costs {cost} hearts and you'd need one left."));
                }
                set_hearts(server, &player.name, have - cost);
                let back = setting("revive-hearts", "3");
                set_hearts(server, who, back);
                server.broadcast(&format!("§a✚ §f{} §7revived §f{who}§7!", player.name));
                String::new()
            }
            ("lifesteal", ["set", who, n]) if crate::commands::is_op(&player.name) => match n.parse::<i32>() {
                Ok(n) => format!("§a{who} now has {} hearts.", set_hearts(server, who, n)),
                Err(_) => "§cUsage: /lifesteal set <player> <hearts>".into(),
            },
            ("lifesteal", _) => "§6/lifesteal set §f<player> <hearts> §7(ops)".into(),
            _ => format!("§cUsage: /{name} <player>"),
        })
    }
}

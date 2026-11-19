//! Custom menus, like DeluxeMenus: make plugins/menus/<name>.txt and open it with
//! `/menu <name>` (or from another menu's button). Styled by the lobby pack.
//!
//! ```text
//! title = Server menu
//! style = grid                           (cards; leave it out for full-width rows)
//! text = §7Pick something
//! button.1.text = §fKits\n§7Free stuff
//! button.1.icon = textures/items/chest_minecart
//! button.1.command = kit                 (runs for the player, skipping the op check; {player} works)
//! button.2.text = §fShop
//! button.2.menu = shop                   (opens menus/shop.txt)
//! button.2.permission = menus.shop       (optional: hidden without it)
//! ```

use crate::players::{PlayerInfo, Server};
use crate::plugin::{Config, Plugin};
use crate::ui::{self, Form, Response};

const NAME: &str = "menus";

struct Button {
    text: String,
    icon: Option<String>,
    command: Option<String>,
    menu: Option<String>,
}

fn clean(name: &str) -> Option<String> {
    let n = name.to_lowercase();
    (!n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')).then_some(n)
}

fn seed() {
    let mut c = Config::data(NAME, "main");
    if c.entries().next().is_some() {
        return;
    }
    for (k, v) in [
        ("title", "Server menu"),
        ("style", "grid"),
        ("text", "§7Everything in one place."),
        ("button.1.text", "§fKits\\n§7Claim your kits"),
        ("button.1.icon", "textures/items/iron_sword"),
        ("button.1.command", "kit"),
        ("button.2.text", "§fAuction House\\n§7Buy and sell"),
        ("button.2.icon", "textures/items/gold_ingot"),
        ("button.2.command", "ah"),
        ("button.3.text", "§fLeaderboards\\n§7Who's on top"),
        ("button.3.icon", "textures/items/diamond"),
        ("button.3.command", "top"),
        ("button.4.text", "§fYour team\\n§7Members and home"),
        ("button.4.icon", "textures/items/banner_pattern"),
        ("button.4.command", "team"),
    ] {
        c.set(k, v);
    }
    let _ = c.save();
}

fn open(server: &Server, p: &PlayerInfo, name: &str) -> Result<(), String> {
    let Some(name) = clean(name) else { return Err("§cNo such menu.".into()) };
    let c = Config::data(NAME, &name);
    let Some(title) = c.get("title").map(str::to_string) else { return Err(format!("§cNo menu called {name}.")) };
    let allowed = |n: usize| {
        c.get(&format!("button.{n}.permission")).is_none_or(|node| server.plugins.allowed(&p.name, node))
    };
    let buttons: Vec<Button> = (1..=50)
        .filter(|n| c.get(&format!("button.{n}.text")).is_some() && allowed(*n))
        .map(|n| {
            let g = |f: &str| c.get(&format!("button.{n}.{f}")).map(str::to_string);
            Button {
                text: crate::placeholders::fill(&g("text").unwrap_or_default().replace("\\n", "\n"), server, p),
                icon: g("icon"),
                command: g("command"),
                menu: g("menu"),
            }
        })
        .collect();
    // `style = grid` shows cards; anything else, full-width rows.
    let flag = if c.get("style") == Some("grid") { "rusted_grid" } else { "rusted" };
    let form = Form::Simple {
        title: format!("{flag}:{}", crate::placeholders::fill(&title, server, p)),
        content: crate::placeholders::fill(&c.get("text").unwrap_or("").replace("\\n", "\n"), server, p),
        buttons: buttons.iter().map(|b| (b.text.clone(), b.icon.clone())).collect(),
    };
    ui::show(server, p.id, form, move |server, p, answer| {
        let Response::Button(i) = answer else { return };
        let Some(b) = buttons.get(i) else { return };
        if let Some(cmd) = &b.command {
            // Menu files are written by admins: the command runs for the player without the op
            // check, like a console command aimed at them.
            server.run_as(p.id, &crate::placeholders::fill(cmd, server, p));
        }
        if let Some(m) = &b.menu
            && let Err(e) = open(server, p, m)
        {
            server.tell(p.id, &e);
        }
    });
    Ok(())
}

pub struct Menus;

impl Plugin for Menus {
    fn name(&self) -> &str {
        NAME
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![("menu", "Opens a custom menu: /menu [name]")]
    }

    fn on_server_start(&self, _server: &Server) {
        seed();
    }

    fn usage(&self, command: &str) -> &'static [&'static str] {
        match command {
            "menu" => &["[name]"],
            _ => &[],
        }
    }

    fn on_command(&self, server: &Server, player: &PlayerInfo, _name: &str, args: &[&str]) -> Option<String> {
        Some(match open(server, player, args.first().copied().unwrap_or("main")) {
            Ok(()) => String::new(),
            Err(e) => e,
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn menu_names_stay_in_their_folder() {
        assert_eq!(super::clean("Shop"), Some("shop".into()));
        assert_eq!(super::clean("../ops"), None);
    }
}

//! A small demo plugin showing custom items, item use and chest menus. Type `/lobby` for a
//! compass; right-click it for a menu.

use crate::inventory::Stack;
use crate::players::{PlayerInfo, Server};
use crate::plugin::{self, Plugin};
use crate::ui;

pub struct Demo;

fn icon(name: &str, label: &str, lore: &str) -> Option<Stack> {
    plugin::item(name).map(|i| i.name(label).lore(&[lore]).build())
}

fn open_menu(server: &Server, to: i64) {
    let mut items = vec![None; 27];
    items[10] = icon("minecraft:golden_apple", "§6Heal me", "§7Regeneration for 10 s");
    items[12] = icon("minecraft:feather", "§bSpeed", "§7Speed II for 30 s");
    items[14] = icon("minecraft:paper", "§eSay hi", "§7Shows a title");
    items[16] = icon("minecraft:ender_pearl", "§dTo spawn", "§7Teleports you to 0, 100, 0");
    items[22] = icon("minecraft:barrier", "§cClose", "§7Closes this menu");
    ui::menu(server, to, "§lRusted demo", items, |server, player, slot| {
        match slot {
            10 => server.effect(player.id, crate::effects::REGENERATION, 1, 10),
            12 => server.effect(player.id, crate::effects::SPEED, 1, 30),
            14 => server.title(player.id, "§aHello!", &format!("§7Welcome, {}", player.name)),
            16 => server.teleport(player.id, (0.5, 100.0, 0.5)),
            22 => return false,
            _ => return true,
        }
        server.sound(player.id, "random.orb");
        true
    });
}

impl Plugin for Demo {
    fn name(&self) -> &str {
        "demo"
    }

    fn description(&self) -> &str {
        "Custom item and chest menu example: /lobby"
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![("lobby", "Get the demo menu compass")]
    }

    fn on_command(&self, server: &Server, player: &PlayerInfo, _name: &str, _args: &[&str]) -> Option<String> {
        let compass = plugin::item("minecraft:compass")?.name("§aDemo menu").lore(&["§7Right-click me"]).tag("menu", "demo").build();
        server.give(player.id, compass);
        Some("§aHere's your menu compass. Right-click it!".into())
    }

    fn on_item_use(&self, server: &Server, player: &PlayerInfo, item: &Stack) -> bool {
        if plugin::tag(item, "menu").as_deref() == Some("demo") {
            open_menu(server, player.id);
            return false;
        }
        true
    }
}

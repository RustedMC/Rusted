//! A small runtime plugin: greets players, adds /hello and /kit, counts joins in its saved data,
//! and keeps a word out of chat.

use rusted_plugin::{Event, plugin};

fn init() {
    rusted_plugin::register_command("hello", "Says hello back");
    rusted_plugin::register_command("kit", "Gives you a starter kit, once");
    rusted_plugin::log("hello plugin loaded");
}

fn event(e: &Event) -> bool {
    match e.kind() {
        "join" => {
            // Saved data survives restarts: count how many times anyone has joined.
            let joins = rusted_plugin::get_data("joins").parse::<u32>().unwrap_or(0) + 1;
            rusted_plugin::set_data("joins", &joins.to_string());
            rusted_plugin::broadcast(&format!("§eWelcome to the server, {}! (join #{joins})", e.player_name()));
            rusted_plugin::title(e.player_id(), "§6Welcome!");
        }
        "command" if e.command() == "hello" => {
            let who = e.args().first().copied().unwrap_or(e.player_name());
            rusted_plugin::reply(&format!("§aHello, {who}!"));
        }
        "command" if e.command() == "kit" => {
            let key = format!("kit_{}", e.player_name().to_lowercase());
            if rusted_plugin::get_data(&key) == "taken" {
                rusted_plugin::reply("§cYou already took your kit.");
            } else {
                let id = e.player_id();
                rusted_plugin::give(id, "stone_sword", 1);
                rusted_plugin::give(id, "bread", 16);
                rusted_plugin::play_sound("random.levelup", 0.0, 0.0, 0.0);
                rusted_plugin::set_data(&key, "taken");
                rusted_plugin::reply("§aEnjoy your kit!");
            }
        }
        // Chat: block one word.
        "chat" if e.message().to_lowercase().contains("griefing") => {
            rusted_plugin::tell(e.player_id(), "§cPlease don't talk about griefing here.");
            return false;
        }
        // Nobody breaks bedrock, even in creative.
        "block_break" if e.block() == "minecraft:bedrock" => return false,
        _ => {}
    }
    true
}

plugin!(init, event);

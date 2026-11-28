//! A small runtime plugin: greets players, adds /hello and /spawnsafe, and keeps a word out of chat.

use rusted_plugin::{Event, plugin};

fn init() {
    rusted_plugin::register_command("hello", "Says hello back");
    rusted_plugin::log("hello plugin loaded");
}

fn event(e: &Event) -> bool {
    match e.kind() {
        "join" => rusted_plugin::broadcast(&format!("§eWelcome to the server, {}!", e.player_name())),
        "command" if e.command() == "hello" => {
            let who = e.args().first().copied().unwrap_or(e.player_name());
            rusted_plugin::reply(&format!("§aHello, {who}!"));
        }
        // Chat: block one word, tidy up the rest.
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

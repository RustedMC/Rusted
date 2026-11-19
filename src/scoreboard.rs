//! Scoreboard: a sidebar for every player on servers that aren't running the lobby core,
//! refreshed every 2 seconds. Lines take any placeholder (`{balance}`, `{rank}`, `{kills}`,
//! `{team}`, `{playtime}`...). Edit plugins/scoreboard/config.txt; `enabled = false` turns it off.

use crate::players::Server;
use crate::plugin::{Config, Plugin};

const NAME: &str = "scoreboard";

const DEFAULT: &[&str] = &[
    " ",
    "§6§lYOU",
    "§7Rank §f{rank}",
    "§7Coins §e{balance}",
    "§7Kills §f{kills}",
    " ",
    "§6§lSERVER",
    "§7Online §f{online}",
    "§7Playtime §f{playtime}",
    " ",
    "§eplay.example.com",
];

fn refresh(server: &Server) {
    let mut c = Config::load(NAME);
    let on = c.get_or("enabled", "true") == "true";
    let title = c.get_or("title", "§6§lRUSTED");
    let lines: Vec<String> = (1..=15).map(|n| c.get_or(&format!("line.{n}"), DEFAULT.get(n - 1).copied().unwrap_or(""))).collect();
    let _ = c.save();
    // The lobby core has its own sidebar.
    let lobby = Config::load("lobby").get("enabled") == Some("true");
    for p in server.all() {
        if !on || lobby {
            continue;
        }
        let filled: Vec<String> = lines.iter().filter(|l| !l.is_empty()).map(|l| crate::placeholders::fill(l, server, &p)).collect();
        crate::ui::sidebar(server, p.id, "board", &crate::placeholders::fill(&title, server, &p), filled);
    }
}

pub struct Scoreboard;

impl Plugin for Scoreboard {
    fn name(&self) -> &str {
        NAME
    }

    fn on_server_start(&self, server: &Server) {
        server.scheduler.run_repeating(40, 40, |server| {
            refresh(server);
            true
        });
    }
}

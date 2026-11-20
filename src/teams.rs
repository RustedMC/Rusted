//! Teams: `/team` opens your team's menu (or how to start one).
//!
//! - `/team create <name>`, `/team invite <player>`, `/team join <name>`, `/team leave`,
//!   `/team kick <player>` and `/team disband` (owner), `/team sethome`, `/team home`,
//!   `/team info [name]`, `/team list`, `/tc <message>` team chat
//! - Placeholder `{team}` (empty if none), for chat formats and sidebars.
//!
//! Stored in plugins/teams/teams.txt. `max-members` in config.txt.
//! Teammates can't hurt each other.

use crate::players::{PlayerInfo, Server};
use crate::plugin::{Config, Plugin};
use crate::ui::{self, Form, Response};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::Instant;

const NAME: &str = "teams";

/// invited player (lowercase) -> (team, when)
static INVITES: Mutex<Option<HashMap<String, (String, Instant)>>> = Mutex::new(None);

fn book() -> Config {
    Config::data(NAME, "teams")
}

fn members(b: &Config, team: &str) -> Vec<String> {
    b.get(&format!("{team}.members")).unwrap_or("").split(',').filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// The team `player` is in.
pub fn team_of(player: &str) -> Option<String> {
    let b = book();
    let me = player.to_lowercase();
    let teams: Vec<String> = b.entries().filter_map(|(k, _)| k.strip_suffix(".owner").map(str::to_string)).collect();
    teams.into_iter().find(|t| members(&b, t).iter().any(|m| m.to_lowercase() == me))
}

fn display(b: &Config, team: &str) -> String {
    b.get(&format!("{team}.display")).unwrap_or(team).to_string()
}

fn clean(name: &str) -> Option<String> {
    let n = name.to_lowercase();
    (n.len() >= 2 && n.len() <= 12 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).then_some(n)
}

fn tell_team(server: &Server, team: &str, msg: &str) {
    let b = book();
    for m in members(&b, team) {
        if let Some(id) = server.find_by_name(&m) {
            server.tell(id, msg);
        }
    }
}

fn menu(server: &Server, p: &PlayerInfo) {
    let Some(team) = team_of(&p.name) else {
        server.tell(p.id, "§7You're not in a team. §f/team create <name> §7or accept an invite with §f/team join <name>§7.");
        return;
    };
    let b = book();
    let list = members(&b, &team);
    let owner = b.get(&format!("{team}.owner")).unwrap_or("").to_string();
    let online = list.iter().filter(|m| server.find_by_name(m).is_some()).count();
    let mut buttons: Vec<(String, Option<String>)> = vec![
        ("§fTeam home\n§7Teleport there".into(), Some("textures/items/bed_red".into())),
        ("§fLeave team\n§7You can rejoin if invited".into(), Some("textures/ui/cancel".into())),
    ];
    for m in &list {
        let state = if server.find_by_name(m).is_some() { "§aOnline" } else { "§8Offline" };
        let role = if *m == owner { " §6(owner)" } else { "" };
        let icon = if *m == owner { "gold_helmet" } else if server.find_by_name(m).is_some() { "emerald" } else { "coal" };
        buttons.push((format!("§f{m}{role}\n{state}"), Some(format!("textures/items/{icon}"))));
    }
    let form = Form::Simple {
        title: format!("{}", display(&b, &team)),
        content: format!("§7{} members, {online} online. §f/tc <message> §7talks to your team.", list.len()),
        buttons,
    };
    ui::show(server, p.id, form, |server, p, answer| {
        let msg = match answer {
            Response::Button(0) => command(server, p, &["home"]),
            Response::Button(1) => command(server, p, &["leave"]),
            _ => return,
        };
        server.tell(p.id, &msg);
    });
}

fn command(server: &Server, p: &PlayerInfo, args: &[&str]) -> String {
    let mut b = book();
    let mine = team_of(&p.name);
    let owner_of = |b: &Config, t: &str| b.get(&format!("{t}.owner")) == Some(p.name.as_str());
    match args {
        ["create", name] => {
            if mine.is_some() {
                return "§cLeave your team first.".into();
            }
            let Some(t) = clean(name) else { return "§cTeam names are 2-12 letters, numbers or _.".into() };
            if b.get(&format!("{t}.owner")).is_some() {
                return "§cThat name is taken.".into();
            }
            b.set(&format!("{t}.owner"), &p.name);
            b.set(&format!("{t}.members"), &p.name);
            b.set(&format!("{t}.display"), name);
            let _ = b.save();
            format!("§aTeam §f{name} §acreated. Invite people with /team invite <player>.")
        }
        ["invite", who] => {
            let Some(t) = mine else { return "§cYou're not in a team.".into() };
            let Some(target) = server.all().into_iter().find(|o| o.name.eq_ignore_ascii_case(who)) else { return format!("§c{who} isn't online.") };
            let max: usize = Config::load(NAME).get_or("max-members", "8").parse().unwrap_or(8);
            if members(&b, &t).len() >= max {
                return format!("§cTeams can have {max} members.");
            }
            INVITES.lock().get_or_insert_with(HashMap::new).insert(target.name.to_lowercase(), (t.clone(), Instant::now()));
            server.tell(target.id, &format!("§e{} §7invited you to §f{}§7. §a/team join {t}", p.name, display(&b, &t)));
            format!("§7Invited §e{}§7.", target.name)
        }
        ["join", name] => {
            if mine.is_some() {
                return "§cLeave your team first.".into();
            }
            let t = name.to_lowercase();
            let invited = INVITES.lock().get_or_insert_with(HashMap::new).remove(&p.name.to_lowercase()).filter(|(it, at)| *it == t && at.elapsed().as_secs() < 300);
            if invited.is_none() {
                return "§cYou don't have an invite to that team.".into();
            }
            let mut list = members(&b, &t);
            if list.is_empty() {
                return "§cThat team is gone.".into();
            }
            list.push(p.name.clone());
            b.set(&format!("{t}.members"), &list.join(","));
            let _ = b.save();
            tell_team(server, &t, &format!("§a{} joined the team.", p.name));
            String::new()
        }
        ["leave"] => {
            let Some(t) = mine else { return "§cYou're not in a team.".into() };
            if owner_of(&b, &t) {
                return "§cYou own this team. /team disband, or give it away by disbanding and re-creating.".into();
            }
            let list: Vec<String> = members(&b, &t).into_iter().filter(|m| *m != p.name).collect();
            b.set(&format!("{t}.members"), &list.join(","));
            let _ = b.save();
            tell_team(server, &t, &format!("§c{} left the team.", p.name));
            "§7You left the team.".into()
        }
        ["kick", who] => {
            let Some(t) = mine.filter(|t| owner_of(&b, t)) else { return "§cOnly the team owner can kick.".into() };
            let list: Vec<String> = members(&b, &t).into_iter().filter(|m| !m.eq_ignore_ascii_case(who) || *m == p.name).collect();
            b.set(&format!("{t}.members"), &list.join(","));
            let _ = b.save();
            format!("§7Kicked {who}.")
        }
        ["disband"] => {
            let Some(t) = mine.filter(|t| owner_of(&b, t)) else { return "§cOnly the team owner can disband.".into() };
            tell_team(server, &t, "§cYour team was disbanded.");
            for f in ["owner", "members", "display", "home"] {
                b.remove(&format!("{t}.{f}"));
            }
            let _ = b.save();
            String::new()
        }
        ["sethome"] => {
            let Some(t) = mine.filter(|t| owner_of(&b, t)) else { return "§cOnly the team owner can set the home.".into() };
            b.set(&format!("{t}.home"), &format!("{:.2},{:.2},{:.2}", p.pos.0, p.pos.1 - 1.62, p.pos.2));
            let _ = b.save();
            "§aTeam home set.".into()
        }
        ["home"] => {
            let Some(t) = mine else { return "§cYou're not in a team.".into() };
            let at: Vec<f32> = b.get(&format!("{t}.home")).unwrap_or("").split(',').filter_map(|x| x.parse().ok()).collect();
            if at.len() != 3 {
                return "§cYour team has no home yet.".into();
            }
            server.teleport(p.id, (at[0], at[1], at[2]));
            "§aTeleported to your team home.".into()
        }
        ["info", rest @ ..] => {
            let Some(t) = rest.first().map(|n| n.to_lowercase()).or(mine) else { return "§cYou're not in a team.".into() };
            let list = members(&b, &t);
            if list.is_empty() {
                return "§cNo such team.".into();
            }
            format!("§6§l{} §7({} members)\n§f{}", display(&b, &t), list.len(), list.join("§7, §f"))
        }
        ["list"] => {
            let mut teams: Vec<(String, usize)> = b.entries().filter_map(|(k, _)| k.strip_suffix(".owner")).map(|t| (display(&b, t), members(&b, t).len())).collect();
            teams.sort_by(|a, b| b.1.cmp(&a.1));
            if teams.is_empty() {
                return "§7No teams yet.".into();
            }
            teams.iter().map(|(t, n)| format!("§f{t} §7({n})")).collect::<Vec<_>>().join("§8, ")
        }
        _ => "§6/team §fcreate|invite|join|leave|kick|disband|sethome|home|info|list\n§6/tc §f<message> §7team chat".into(),
    }
}

pub struct Teams;

impl Plugin for Teams {
    fn name(&self) -> &str {
        NAME
    }

    fn version(&self) -> &str {
        "1.1.1"
    }

    fn description(&self) -> &str {
        "Teams with a menu, invites, team chat, home and no friendly fire"
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![("team", "Team menu and commands"), ("tc", "Talks to your team")]
    }

    /// No friendly fire: a hit's cause is the attacker's name when it's a player.
    fn on_player_damage(&self, server: &Server, player: &PlayerInfo, cause: &str, _amount: f32) -> bool {
        if server.find_by_name(cause).is_none() {
            return true;
        }
        team_of(cause).is_none_or(|t| team_of(&player.name).as_deref() != Some(t.as_str()))
    }

    fn on_server_start(&self, _server: &Server) {
        crate::placeholders::register("team", |_, p| team_of(&p.name).map(|t| display(&book(), &t)).unwrap_or_default());
    }

    fn usage(&self, command: &str) -> &'static [&'static str] {
        match command {
            "team" => &["create <name>", "invite <player>", "join <name>", "leave", "kick <player>", "disband", "sethome", "home", "info [name]", "list"],
            "tc" => &["<message...>"],
            _ => &[],
        }
    }

    fn on_command(&self, server: &Server, player: &PlayerInfo, name: &str, args: &[&str]) -> Option<String> {
        Some(match (name, args) {
            ("team", []) => {
                menu(server, player);
                String::new()
            }
            ("team", args) => command(server, player, args),
            ("tc", []) => "§cUsage: /tc <message>".into(),
            ("tc", words) => match team_of(&player.name) {
                Some(t) => {
                    tell_team(server, &t, &format!("§b[Team] §f{}§7: {}", player.name, words.join(" ")));
                    String::new()
                }
                None => "§cYou're not in a team.".into(),
            },
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn team_names() {
        assert_eq!(super::clean("Wolves_1"), Some("wolves_1".into()));
        assert_eq!(super::clean("a"), None);
        assert_eq!(super::clean("no spaces"), None);
    }
}

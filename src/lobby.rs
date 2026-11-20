//! Lobby core: everything a hub server needs, all set in plugins/lobby/config.txt.
//!
//! - A server selector (chest menu) with each server's live player count
//! - NPCs live in the npcs plugin (`/npc`); they can open this selector or send players on
//! - A per-player sidebar and boss bar with placeholders
//! - Lobby items (selector, play, settings) given on join
//! - A settings form: show players, sidebar, boss bar, who can send friend requests
//! - World protection, no damage, no hunger, a void catch and launch pads (slime by default)
//! - Lobby items locked in place (no dropping or moving them)
//!
//! It's off until `enabled = true` is set, so normal servers aren't affected.

use crate::inventory::Stack;
use crate::players::{PlayerInfo, Server};
use crate::plugin::{self, Config, Plugin};
use crate::ui::{self, Element, Form, Response};
use crate::Protocol;
use parking_lot::Mutex;
use std::collections::HashMap;

pub struct Lobby;

const NAME: &str = "lobby";

fn config() -> Config {
    Config::load(NAME)
}

fn on() -> bool {
    config().get_or("enabled", "false") == "true"
}

fn get(c: &mut Config, key: &str, default: &str) -> String {
    c.get_or(key, default)
}

// ---------------------------------------------------------------------------------------------
// Servers and live player counts

#[derive(Clone)]
struct Target {
    name: String,
    ip: String,
    port: u16,
    icon: String,
    slot: usize,
    lore: String,
    /// Button icon in the form selector: a texture path ("textures/items/diamond") or an https URL.
    image: String,
}

/// Servers from the config: server.1.name, server.1.ip, ... up to server.27.
fn servers() -> Vec<Target> {
    let mut c = config();
    (1..=27)
        .filter_map(|n| {
            let name = c.get(&format!("server.{n}.name"))?.to_string();
            Some(Target {
                ip: c.get(&format!("server.{n}.ip")).unwrap_or("127.0.0.1").to_string(),
                port: c.get(&format!("server.{n}.port")).and_then(|p| p.parse().ok()).unwrap_or(19132),
                icon: c.get(&format!("server.{n}.icon")).unwrap_or("minecraft:grass_block").to_string(),
                slot: c.get(&format!("server.{n}.slot")).and_then(|s| s.parse().ok()).unwrap_or(9 + n).min(26),
                lore: get(&mut c, &format!("server.{n}.lore"), "§7Click to join"),
                image: get(&mut c, &format!("server.{n}.image"), "textures/blocks/grass_side_carried"),
                name,
            })
        })
        .collect()
}

/// Last known (online, max) per "ip:port", refreshed in the background.
static COUNTS: Mutex<Option<HashMap<String, (u32, u32)>>> = Mutex::new(None);

fn count(ip: &str, port: u16) -> Option<(u32, u32)> {
    COUNTS.lock().as_ref()?.get(&format!("{ip}:{port}")).copied()
}

/// Asks a Bedrock server how many players it has (a RakNet unconnected ping, like the game's
/// server list does). The reply's MOTD is "MCPE;name;protocol;version;online;max;...".
fn ping(ip: &str, port: u16) -> Option<(u32, u32)> {
    use std::net::{ToSocketAddrs, UdpSocket};
    let addr = (ip, port).to_socket_addrs().ok()?.next()?;
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(2))).ok()?;
    let mut p = vec![0x01];
    p.extend_from_slice(&0i64.to_be_bytes());
    p.extend_from_slice(&[0x00, 0xff, 0xff, 0x00, 0xfe, 0xfe, 0xfe, 0xfe, 0xfd, 0xfd, 0xfd, 0xfd, 0x12, 0x34, 0x56, 0x78]);
    p.extend_from_slice(&0i64.to_be_bytes());
    s.send_to(&p, addr).ok()?;
    let mut buf = [0u8; 1500];
    let n = s.recv(&mut buf).ok()?;
    let motd = String::from_utf8_lossy(buf.get(35..n)?).into_owned();
    let f: Vec<&str> = motd.split(';').collect();
    Some((f.get(4)?.parse().ok()?, f.get(5)?.parse().ok()?))
}

fn start_pinging() {
    std::thread::spawn(|| loop {
        for t in servers() {
            let c = ping(&t.ip, t.port);
            let mut m = COUNTS.lock();
            let m = m.get_or_insert_with(Default::default);
            match c {
                Some(c) => m.insert(format!("{}:{}", t.ip, t.port), c),
                None => m.remove(&format!("{}:{}", t.ip, t.port)),
            };
        }
        std::thread::sleep(std::time::Duration::from_secs(10));
    });
}

/// "§a3 §7playing" for a server's last ping, for NPC name tags.
pub fn players_text(ip: &str, port: u16) -> String {
    count_text(&Target { name: String::new(), ip: ip.into(), port, icon: String::new(), slot: 0, lore: String::new(), image: String::new() })
}

pub fn open_servers(server: &Server, to: i64) {
    open_selector(server, to);
}

fn count_text(t: &Target) -> String {
    match count(&t.ip, t.port) {
        Some((on, _)) => format!("§7{on} playing"),
        None => "§cOffline".into(),
    }
}

fn open_selector(server: &Server, to: i64) {
    let mut c = config();
    // "form" is a button list; "chest" is a chest menu.
    // "chest" works without any pack.
    if get(&mut c, "selector.style", "chest") == "form" {
        let title = get(&mut c, "selector.title", "Servers");
        let body = get(&mut c, "selector.text", "");
        let _ = c.save();
        let list = servers();
        let form = Form::Simple {
            title: title.clone(),
            content: body,
            buttons: list.iter().map(|t| (format!("§f{}
{}", t.name, count_text(t)), Some(t.image.clone()))).collect(),
        };
        ui::show(server, to, form, move |server, player, answer| {
            if let Response::Button(i) = answer
                && let Some(t) = list.get(i)
            {
                server.tell(player.id, &format!("§7Sending you to §b{}§7...", t.name));
                server.transfer(player.id, &t.ip, t.port);
            }
        });
        return;
    }
    let title = get(&mut c, "selector.title", "§lPick a server");
    let _ = c.save();
    let mut items = vec![None; 27];
    let list = servers();
    for t in &list {
        items[t.slot] = plugin::item(&t.icon).map(|i| i.name(&format!("§l§b{}", t.name)).lore(&[&t.lore, &count_text(t)]).build());
    }
    ui::menu(server, to, &title, items, move |server, player, slot| {
        if let Some(t) = list.iter().find(|t| t.slot == slot) {
            server.tell(player.id, &format!("§7Sending you to §b{}§7...", t.name));
            server.transfer(player.id, &t.ip, t.port);
            return false;
        }
        true
    });
}

// ---------------------------------------------------------------------------------------------
// Per-player settings

struct Settings {
    show_players: bool,
    sidebar: bool,
    boss_bar: bool,
    /// 0 anyone, 1 friends of friends, 2 no one
    friend_requests: usize,
}

fn settings(p: &PlayerInfo) -> Settings {
    let mut d = Config::player(NAME, p);
    Settings {
        show_players: d.get_or("show-players", "true") == "true",
        sidebar: d.get_or("sidebar", "true") == "true",
        boss_bar: d.get_or("boss-bar", "true") == "true",
        friend_requests: d.get_or("friend-requests", "0").parse().unwrap_or(0).min(2),
    }
}

/// Other plugins (a friends system) read a player's choice: "anyone", "friends-of-friends" or "no-one".
#[allow(dead_code)] // nothing in Rusted itself asks yet
pub fn friend_requests(p: &PlayerInfo) -> &'static str {
    ["anyone", "friends-of-friends", "no-one"][settings(p).friend_requests]
}

fn open_settings(server: &Server, p: &PlayerInfo) {
    let s = settings(p);
    let mut c = config();
    let title = get(&mut c, "settings.title", "Settings");
    let _ = c.save();
    let form = Form::Custom {
        title,
        elements: vec![
            Element::Toggle { text: "Show other players".into(), default: s.show_players },
            Element::Toggle { text: "Show the sidebar".into(), default: s.sidebar },
            Element::Toggle { text: "Show the boss bar".into(), default: s.boss_bar },
            Element::StepSlider { text: "Friend requests from".into(), steps: vec!["Anyone".into(), "Friends of friends".into(), "No one".into()], default: s.friend_requests },
        ],
    };
    ui::show(server, p.id, form, |server, player, answer| {
        let Response::Values(v) = answer else { return };
        let b = |i: usize| v.get(i).and_then(|x| x.as_bool()).unwrap_or(true);
        let mut d = Config::player(NAME, player);
        d.set("show-players", &b(0).to_string());
        d.set("sidebar", &b(1).to_string());
        d.set("boss-bar", &b(2).to_string());
        d.set("friend-requests", &v.get(3).and_then(|x| x.as_u64()).unwrap_or(0).min(2).to_string());
        let _ = d.save();
        apply_visibility(server, player);
        refresh_hud(server, player);
        server.sound(player.id, "random.orb");
        server.actionbar(player.id, "§aSettings saved");
    });
}

/// Hides or shows everyone else for this player, as their settings say.
fn apply_visibility(server: &Server, p: &PlayerInfo) {
    let show = settings(p).show_players;
    for other in server.all().into_iter().filter(|o| o.id != p.id) {
        let packet = if show {
            crate::packets::add_player(&other)
        } else {
            Protocol::RemoveActorPacket(Box::new(bedrock::protocol::v662::packets::RemoveActorPacket {
                target_actor_id: bedrock::protocol::v662::types::ActorUniqueID(other.id),
            }))
        };
        let _ = server.events.send(crate::players::Event::Packet { to: p.id, packet: Box::new(packet) });
    }
}

// ---------------------------------------------------------------------------------------------
// Sidebar and boss bar

fn fill(text: &str, server: &Server, p: &PlayerInfo) -> String {
    let mut out = crate::placeholders::fill(text, server, p);
    for t in servers() {
        let n = count(&t.ip, t.port).map_or("-".to_string(), |c| c.0.to_string());
        out = out.replace(&format!("{{count:{}}}", t.name), &n);
        let dot = if count(&t.ip, t.port).is_some() { "§a●" } else { "§c●" };
        out = out.replace(&format!("{{status:{}}}", t.name), dot);
    }
    out
}

fn send(server: &Server, to: i64, packet: Protocol) {
    let _ = server.events.send(crate::players::Event::Packet { to, packet: Box::new(packet) });
}

/// Rebuilds this player's sidebar and boss bar from the config.
fn refresh_hud(server: &Server, p: &PlayerInfo) {
    let s = settings(p);
    let mut c = config();
    ui::remove_sidebar(server, p.id, "lobby");
    if s.sidebar {
        let title = fill(&get(&mut c, "sidebar.title", "§6§lRUSTED"), server, p);
        let mut lines = Vec::new();
        for n in 1..=15 {
            let default = match n {
                1 => " ",
                2 => "§6§lYOU",
                3 => "§7Name §f{player}",
                4 => " ",
                5 => "§6§lNETWORK",
                6 => "§7Online §f{online}",
                7 => "§7Survival §a{count:Survival}",
                8 => " ",
                9 => "§eplay.example.com",
                _ => "",
            };
            let line = get(&mut c, &format!("sidebar.line.{n}"), default);
            if !line.is_empty() {
                lines.push(fill(&line, server, p));
            }
        }
        ui::sidebar(server, p.id, "lobby", &title, lines);
    }
    let bar = get(&mut c, "boss-bar.text", "§bWelcome to the lobby, §f{player}");
    let _ = c.save();
    if s.boss_bar && !bar.is_empty() {
        ui::boss_bar(server, p.id, &fill(&bar, server, p), 1.0, 1);
    } else {
        ui::remove_boss_bar(server, p.id);
    }
}


/// Puts a player back at the lobby spawn with fresh lobby items.
fn send_to_lobby(server: &Server, p: &PlayerInfo) {
    let spawn = config().get_or("spawn", "");
    let v: Vec<f32> = spawn.split(',').filter_map(|n| n.trim().parse().ok()).collect();
    let at = if v.len() == 3 { (v[0], v[1], v[2]) } else { crate::commands::world_spawn(server) };
    server.teleport(p.id, at);
    lobby_items(server, p);
    refresh_hud(server, p);
}

/// /lobbyadmin: tools for running the lobby without a restart.
fn admin(server: &Server, args: &[&str]) -> String {
    match args {
        ["reload"] => {
            // Settings are read live; this re-applies them to everyone online right now.
            for p in server.all() {
                lobby_items(server, &p);
                refresh_hud(server, &p);
            }
            "§aLobby config re-applied to everyone online.".into()
        }
        ["status"] => {
            let mut out = format!("§6§lLobby status
§7Players here: §f{}", server.all().len());
            for t in servers() {
                out.push_str(&format!("
§7{} §8({}:{}) §f{}", t.name, t.ip, t.port, count_text(&t)));
            }
            out
        }
        ["npcs"] => crate::npcs::list(),
        ["lobbyall"] => {
            for p in server.all() {
                send_to_lobby(server, &p);
            }
            "§aSent everyone to the lobby spawn.".into()
        }
        _ => "§6/lobbyadmin §freload §7- re-apply the config to everyone
§6/lobbyadmin §fstatus §7- servers and player counts
§6/lobbyadmin §fnpcs §7- list NPCs
§6/lobbyadmin §flobbyall §7- send everyone to spawn".into(),
    }
}

/// Last launch per player, so one pad doesn't fire every move packet.
static LAUNCHED: Mutex<Option<HashMap<i64, std::time::Instant>>> = Mutex::new(None);

/// Standing on the launch-pad block throws the player forward and up.
fn launch_pad(server: &Server, p: &PlayerInfo, to: (f32, f32, f32)) {
    let mut c = config();
    let block = get(&mut c, "launch-pad.block", "minecraft:slime");
    if block.is_empty() {
        return;
    }
    let below = server.world.get(to.0.floor() as i32, (to.1 - 1.62 - 0.5).floor() as i32, to.2.floor() as i32);
    if crate::blocks::entry(below).is_none_or(|e| e.name != block) {
        return;
    }
    let mut last = LAUNCHED.lock();
    let last = last.get_or_insert_with(HashMap::new);
    if last.get(&p.id).is_some_and(|t| t.elapsed().as_millis() < 1000) {
        return;
    }
    last.insert(p.id, std::time::Instant::now());
    let power: f32 = get(&mut c, "launch-pad.power", "2.5").parse().unwrap_or(2.5);
    let up: f32 = get(&mut c, "launch-pad.up", "1.0").parse().unwrap_or(1.0);
    let _ = c.save();
    let yaw = p.rot.1.to_radians();
    send(server, p.id, Protocol::SetActorMotionPacket(Box::new(bedrock::protocol::v662::packets::SetActorMotionPacket {
        target_runtime_id: bedrock::protocol::v662::types::ActorRuntimeID(p.id as u64),
        motion: (-yaw.sin() * power, up, yaw.cos() * power),
        server_tick: 0,
    })));
    server.sound(p.id, "mob.enderdragon.flap");
}

// ---------------------------------------------------------------------------------------------
// Lobby items

fn lobby_items(server: &Server, p: &PlayerInfo) {
    let mut c = config();
    if get(&mut c, "items.clear-on-join", "true") == "true" {
        server.run_as(p.id, "clear");
    }
    let make = |c: &mut Config, key: &str, item: &str, name: &str, lore: &str| {
        let item = get(c, &format!("items.{key}.item"), item);
        let name = get(c, &format!("items.{key}.name"), name);
        let lore = get(c, &format!("items.{key}.lore"), lore);
        plugin::item(&item).map(|i| i.name(&name).lore(&[&lore]).tag("lobby", key).build())
    };
    for (key, item, name, lore) in [
        ("selector", "minecraft:compass", "§b§lServer Selector", "§7Right-click to pick a server"),
        ("play", "minecraft:nether_star", "§a§lPlay", "§7Right-click to jump into a game"),
        ("settings", "minecraft:comparator", "§e§lSettings", "§7Right-click to change your settings"),
    ] {
        if get(&mut c, &format!("items.{key}.enabled"), "true") == "true"
            && let Some(stack) = make(&mut c, key, item, name, lore)
        {
            server.give(p.id, stack);
        }
    }
    let _ = c.save();
}

// ---------------------------------------------------------------------------------------------
// NPCs

// ---------------------------------------------------------------------------------------------

impl Plugin for Lobby {
    fn name(&self) -> &str {
        NAME
    }

    fn version(&self) -> &str {
        "2.4.0"
    }

    fn description(&self) -> &str {
        "Hub server core: server selector, NPCs, sidebar, settings"
    }

    fn on_server_start(&self, server: &Server) {
        let mut c = config();
        get(&mut c, "enabled", "false");
        get(&mut c, "spawn", "");
        get(&mut c, "protect-world", "true");
        get(&mut c, "no-damage", "true");
        get(&mut c, "void-y", "-70");
        get(&mut c, "play.server", "1");
        if c.get("server.1.name").is_none() {
            c.set("server.1.name", "Survival");
            c.set("server.1.ip", "127.0.0.1");
            c.set("server.1.port", "19133");
            c.set("server.1.icon", "minecraft:grass_block");
            c.set("server.1.slot", "13");
            c.set("server.1.lore", "§7Classic survival");
        }
        let _ = c.save();
        if !on() {
            return;
        }
        start_pinging();
        server.scheduler.run_repeating(40, 40, |server| {
            for p in server.all() {
                refresh_hud(server, &p);
            }
            true
        });
        println!("Lobby core on: {} servers", servers().len());
    }

    fn on_join(&self, server: &Server, player: &PlayerInfo) {
        if !on() {
            return;
        }
        let mut c = config();
        let spawn = get(&mut c, "spawn", "");
        let v: Vec<f32> = spawn.split(',').filter_map(|n| n.trim().parse().ok()).collect();
        if v.len() == 3 {
            server.teleport(player.id, (v[0], v[1], v[2]));
        }
        let title = get(&mut c, "join.title", "§6§lRUSTED");
        let subtitle = get(&mut c, "join.subtitle", "§7Welcome, §f{player}");
        let no_hunger = get(&mut c, "no-hunger", "true") == "true";
        let _ = c.save();
        if !title.is_empty() {
            server.title(player.id, &fill(&title, server, player), &fill(&subtitle, server, player));
        }
        if no_hunger {
            server.effect(player.id, 23, 255, 1_000_000); // saturation
        }
        let id = player.id;
        // A moment later, once the player is in the world and their saved inventory has loaded.
        server.scheduler.run_later(20, move |server| {
            let Some(p) = server.get(id) else { return };
            lobby_items(server, &p);
            refresh_hud(server, &p);
            apply_visibility(server, &p);
            // Hide this player from everyone who chose not to see others.
            for other in server.all().into_iter().filter(|o| o.id != id && !settings(o).show_players) {
                apply_visibility(server, &other);
            }
        });
    }


    fn on_item_use(&self, server: &Server, player: &PlayerInfo, item: &Stack) -> bool {
        if !on() {
            return true;
        }
        match plugin::tag(item, "lobby").as_deref() {
            Some("selector") => open_selector(server, player.id),
            Some("settings") => open_settings(server, player),
            Some("play") => {
                let n = config().get_or("play.server", "1");
                match servers().into_iter().find(|t| config().get(&format!("server.{n}.name")) == Some(t.name.as_str())) {
                    Some(t) => server.transfer(player.id, &t.ip, t.port),
                    None => open_selector(server, player.id),
                }
            }
            _ => return true,
        }
        false
    }

    fn on_block_break(&self, _server: &Server, player: &PlayerInfo, _pos: (i32, i32, i32), _block: i32) -> bool {
        !(on() && config().get_or("protect-world", "true") == "true" && !crate::commands::is_op(&player.name))
    }

    fn on_block_place(&self, _server: &Server, player: &PlayerInfo, _pos: (i32, i32, i32), _block: i32) -> bool {
        !(on() && config().get_or("protect-world", "true") == "true" && !crate::commands::is_op(&player.name))
    }

    fn on_player_damage(&self, _server: &Server, _player: &PlayerInfo, _cause: &str, _amount: f32) -> bool {
        !(on() && config().get_or("no-damage", "true") == "true")
    }

    fn on_item_drop(&self, _server: &Server, _player: &PlayerInfo, _item: &str, _count: i8) -> bool {
        !(on() && config().get_or("items.lock", "true") == "true")
    }

    fn on_inventory_action(&self, _server: &Server, _player: &PlayerInfo, _action: &str, _container: &str) -> bool {
        !(on() && config().get_or("items.lock", "true") == "true")
    }

    fn on_move(&self, server: &Server, player: &PlayerInfo, to: (f32, f32, f32)) -> bool {
        if on() {
            launch_pad(server, player, to);
            let void: f32 = config().get_or("void-y", "-70").parse().unwrap_or(-70.0);
            if to.1 - 1.62 < void {
                let spawn = config().get_or("spawn", "");
                let v: Vec<f32> = spawn.split(',').filter_map(|n| n.trim().parse().ok()).collect();
                let at = if v.len() == 3 { (v[0], v[1], v[2]) } else { crate::commands::world_spawn(server) };
                server.teleport(player.id, at);
            }
        }
        true
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("servers", "Opens the server selector"),
            ("settings", "Opens your lobby settings"),
            ("setlobby", "Sets the lobby spawn to where you stand"),
            ("lobby", "Teleports you back to the lobby"),
            ("hub", "Teleports you back to the lobby"),
            ("lobbyadmin", "Lobby tools for ops: reload, status, npcs"),
        ]
    }

    fn usage(&self, command: &str) -> &'static [&'static str] {
        match command {
            "lobbyadmin" => &["reload|status|npcs|lobbyall"],
            _ => &[],
        }
    }

    fn on_command(&self, server: &Server, player: &PlayerInfo, name: &str, args: &[&str]) -> Option<String> {
        if !on() {
            return None;
        }
        match name {
            "servers" => {
                open_selector(server, player.id);
                Some(String::new())
            }
            "settings" => {
                open_settings(server, player);
                Some(String::new())
            }
            "lobby" | "hub" => {
                send_to_lobby(server, player);
                Some("§aBack at the lobby.".into())
            }
            "lobbyadmin" if crate::commands::is_op(&player.name) => Some(admin(server, args)),
            "lobbyadmin" => Some("§cYou do not have permission to use this command".into()),
            "setlobby" if crate::commands::is_op(&player.name) => {
                let mut c = config();
                c.set("spawn", &format!("{:.1},{:.1},{:.1}", player.pos.0, player.pos.1 - 1.62, player.pos.2));
                let _ = c.save();
                Some("§aLobby spawn set here.".into())
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn placeholders() {
        crate::blocks::init();
        let server = crate::players::Server::new();
        let p = crate::players::tests::sample(1);
        let text = super::fill("hi {player}, {online} on", &server, &p);
        assert!(text.starts_with("hi ") && text.ends_with(" on") && !text.contains('{'));
    }
}

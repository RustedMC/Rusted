//! Lobby core: everything a hub server needs, all set in plugins/lobby/config.txt.
//!
//! - A server selector (chest menu) with each server's live player count
//! - NPCs that look like you, wear your gear and send players to a server when clicked:
//!   `/npc <name> <ip> <port> [scale] [player|fixed]`
//! - A per-player sidebar and boss bar with placeholders
//! - Lobby items (selector, play, settings) given on join
//! - A settings form: show players, sidebar, boss bar, who can send friend requests
//! - World protection, no damage, and a void catch
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

fn count_text(t: &Target) -> String {
    match count(&t.ip, t.port) {
        Some((on, max)) => format!("§a{on}§7/§a{max} §7playing"),
        None => "§cOffline".into(),
    }
}

fn open_selector(server: &Server, to: i64) {
    let mut c = config();
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
    let form = Form::Custom {
        title: "Settings".into(),
        elements: vec![
            Element::Toggle { text: "Show other players".into(), default: s.show_players },
            Element::Toggle { text: "Show the sidebar".into(), default: s.sidebar },
            Element::Toggle { text: "Show the boss bar".into(), default: s.boss_bar },
            Element::StepSlider { text: "Who can send you friend requests".into(), steps: vec!["Anyone".into(), "Friends of friends".into(), "No one".into()], default: s.friend_requests },
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
        server.tell(player.id, "§aSettings saved.");
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
    let mut out = text
        .replace("{player}", &p.name)
        .replace("{online}", &server.all().len().to_string())
        .replace("{max}", &crate::config::get().max_players.to_string());
    for t in servers() {
        let n = count(&t.ip, t.port).map_or("-".to_string(), |c| c.0.to_string());
        out = out.replace(&format!("{{count:{}}}", t.name), &n);
    }
    out
}

fn send(server: &Server, to: i64, packet: Protocol) {
    let _ = server.events.send(crate::players::Event::Packet { to, packet: Box::new(packet) });
}

/// Rebuilds this player's sidebar and boss bar from the config.
fn refresh_hud(server: &Server, p: &PlayerInfo) {
    use bedrock::protocol::v2168::packets::{ScorePacketEntry, SetScorePacket};
    use bedrock::protocol::v662::enums::ObjectiveSortOrder;
    use bedrock::protocol::v662::types::ScoreboardId;
    let s = settings(p);
    let mut c = config();
    send(server, p.id, Protocol::RemoveObjectivePacket(Box::new(bedrock::protocol::v662::packets::RemoveObjectivePacket { objective_name: "lobby".into() })));
    if s.sidebar {
        let title = fill(&get(&mut c, "sidebar.title", "§l§6LOBBY"), server, p);
        let mut lines = Vec::new();
        for n in 1..=15 {
            let default = match n {
                1 => "§7Name: §f{player}",
                2 => "§7Online: §a{online}§7/§a{max}",
                3 => " ",
                4 => "§eplay.example.com",
                _ => "",
            };
            let line = get(&mut c, &format!("sidebar.line.{n}"), default);
            if !line.is_empty() {
                lines.push(fill(&line, server, p));
            }
        }
        send(server, p.id, Protocol::SetDisplayObjectivePacket(Box::new(bedrock::protocol::v662::packets::SetDisplayObjectivePacket {
            display_slot_name: "sidebar".into(),
            objective_name: "lobby".into(),
            objective_display_name: title,
            criteria_name: "dummy".into(),
            sort_order: ObjectiveSortOrder::Descending,
        })));
        // Highest score shows first; each line needs a different text, so blanks get § padding.
        let count = lines.len() as i32;
        let entries = lines.into_iter().enumerate().map(|(i, mut text)| {
            if text.trim().is_empty() {
                text = "§r".repeat(i + 1);
            }
            ScorePacketEntry::ChangeFakePlayer { scoreboard_id: ScoreboardId { id: 9000 + i as i64 }, objective_name: "lobby".into(), score_value: count - i as i32, fake_player_name: text }
        }).collect();
        send(server, p.id, Protocol::SetScorePacket(Box::new(SetScorePacket { score_info: entries })));
    }
    let bar = get(&mut c, "boss-bar.text", "§bWelcome to the lobby, §f{player}");
    let _ = c.save();
    if s.boss_bar && !bar.is_empty() {
        ui::boss_bar(server, p.id, &fill(&bar, server, p), 1.0, 1);
    } else {
        ui::remove_boss_bar(server, p.id);
    }
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

#[derive(Clone)]
struct Npc {
    id: i64,
    name: String,
    ip: String,
    port: u16,
    scale: f32,
    look_at_player: bool,
    pos: (f32, f32, f32),
    rot: (f32, f32),
    skin: std::sync::Arc<bedrock::protocol::v2225::types::SerializedSkin<Protocol>>,
    equipment: crate::players::Equipment,
}

static NPCS: Mutex<Vec<Npc>> = Mutex::new(Vec::new());

fn npc_file() -> std::path::PathBuf {
    std::path::Path::new("plugins").join(NAME).join("npcs.txt")
}

fn skin_file(n: &Npc) -> std::path::PathBuf {
    std::path::Path::new("plugins").join(NAME).join("npc-skins").join(format!("{}.skin", n.name.replace(|c: char| !c.is_ascii_alphanumeric(), "_")))
}

fn save_npcs() {
    use bedrock::protocol::ProtoCodec;
    let npcs = NPCS.lock().clone();
    let mut text = String::from("# name|ip|port|scale|look|x|y|z|yaw|pitch (skins in npc-skins/)\n");
    for n in &npcs {
        text.push_str(&format!("{}|{}|{}|{}|{}|{}|{}|{}|{}|{}\n", n.name, n.ip, n.port, n.scale, if n.look_at_player { "player" } else { "fixed" }, n.pos.0, n.pos.1, n.pos.2, n.rot.1, n.rot.0));
        let mut bytes = Vec::new();
        if ProtoCodec::serialize(&*n.skin, &mut bytes).is_ok() {
            let path = skin_file(n);
            let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
            let _ = std::fs::write(path, bytes);
        }
    }
    let _ = std::fs::create_dir_all(npc_file().parent().unwrap_or(std::path::Path::new(".")));
    let _ = std::fs::write(npc_file(), text);
}

fn load_npcs() {
    use bedrock::protocol::ProtoCodec;
    let Ok(text) = std::fs::read_to_string(npc_file()) else { return };
    let mut out = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let f: Vec<&str> = line.split('|').collect();
        if f.len() < 10 {
            continue;
        }
        let num = |i: usize| f[i].parse::<f32>().unwrap_or(0.0);
        let id = crate::players::next_id();
        let skin_path = std::path::Path::new("plugins").join(NAME).join("npc-skins").join(format!("{}.skin", f[0].replace(|c: char| !c.is_ascii_alphanumeric(), "_")));
        let Ok(bytes) = std::fs::read(skin_path) else { continue };
        let Ok(skin) = ProtoCodec::deserialize(&mut &bytes[..]) else { continue };
        let n = Npc {
            id,
            name: f[0].into(),
            ip: f[1].into(),
            port: f[2].parse().unwrap_or(19132),
            scale: num(3).max(0.1),
            look_at_player: f[4] == "player",
            pos: (num(5), num(6), num(7)),
            rot: (num(9), num(8)),
            skin: std::sync::Arc::new(skin),
            equipment: Default::default(),
        };
        out.push(n);
    }
    *NPCS.lock() = out;
}

/// The packets that put an NPC in front of a player: a player-list entry (for the skin), the
/// player itself, its gear and size, then off the player list again so it isn't in the tab list.
fn npc_packets(n: &Npc) -> Vec<Protocol> {
    use bedrock::protocol::v662::types::{ActorRuntimeID, DataItem, PropertySyncData};
    use bedrock::protocol::v924::enums::ActorDataIDs;
    use bedrock::protocol::v2168::enums::DataItemType;
    let target = Target { name: n.name.clone(), ip: n.ip.clone(), port: n.port, icon: String::new(), slot: 0, lore: String::new() };
    let info = PlayerInfo {
        id: n.id,
        uuid: uuid::Uuid::from_u128(n.id as u128 ^ 0x5275_7374_6564_4e50_4300_0000_0000_0000),
        name: format!("§l§b{}\n{}", n.name, if n.ip.is_empty() { String::new() } else { count_text(&target) }),
        xuid: String::new(),
        skin: n.skin.clone(),
        pos: (n.pos.0, n.pos.1 + 1.62, n.pos.2),
        rot: n.rot,
        head_yaw: n.rot.1,
    };
    let air = crate::spawn::air;
    let stack = |s: &Option<Stack>| s.as_ref().map_or_else(air, Stack::to_network);
    vec![
        crate::packets::player_list_add(&info),
        crate::packets::add_player(&info),
        Protocol::MobEquipmentPacket(Box::new(bedrock::protocol::v975::packets::MobEquipmentPacket {
            target_runtime_id: ActorRuntimeID(n.id as u64),
            item: stack(&n.equipment.held),
            slot: 0,
            selected_slot: 0,
            container_id: bedrock::protocol::v662::enums::ContainerID(0),
        })),
        Protocol::MobArmorEquipmentPacket(Box::new(bedrock::protocol::v1001::packets::MobArmorEquipmentPacket {
            target_runtime_id: ActorRuntimeID(n.id as u64),
            head: stack(&n.equipment.armor[0]),
            torso: stack(&n.equipment.armor[1]),
            legs: stack(&n.equipment.armor[2]),
            feet: stack(&n.equipment.armor[3]),
            body: air(),
        })),
        Protocol::SetActorDataPacket(Box::new(bedrock::protocol::v662::packets::SetActorDataPacket {
            target_runtime_id: ActorRuntimeID(n.id as u64),
            actor_data: vec![DataItem { data_item_id: ActorDataIDs::Reserved038, data_item_type: DataItemType::Float(n.scale) }],
            synced_properties: PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
            tick: 0,
        })),
    ]
}

fn npc_unlist(n: &Npc) -> Protocol {
    use bedrock::protocol::v2225::packets::{PlayerListEntry, PlayerListPacket};
    let uuid = uuid::Uuid::from_u128(n.id as u128 ^ 0x5275_7374_6564_4e50_4300_0000_0000_0000);
    Protocol::PlayerListPacket(Box::new(PlayerListPacket { entries: vec![PlayerListEntry::Remove { uuid }] }))
}

fn show_npcs(server: &Server, to: i64) {
    let npcs = NPCS.lock().clone();
    for n in &npcs {
        for p in npc_packets(n) {
            send(server, to, p);
        }
    }
    // Off the tab list a moment later, once the client has the skin.
    server.scheduler.run_later(40, move |server| {
        for n in &npcs {
            send(server, to, npc_unlist(n));
        }
    });
}

/// NPCs that look at players turn their head toward each viewer separately.
fn turn_npcs(server: &Server) {
    let npcs = NPCS.lock().clone();
    for p in server.all() {
        for n in npcs.iter().filter(|n| n.look_at_player) {
            let eye = (n.pos.0, n.pos.1 + 1.62 * n.scale, n.pos.2);
            let (dx, dy, dz) = (p.pos.0 - eye.0, p.pos.1 - eye.1, p.pos.2 - eye.2);
            if dx * dx + dz * dz > 32.0 * 32.0 {
                continue;
            }
            let yaw = (-dx).atan2(dz).to_degrees();
            let pitch = (-dy).atan2((dx * dx + dz * dz).sqrt()).to_degrees();
            send(server, p.id, crate::packets::move_player(n.id, (n.pos.0, n.pos.1 + 1.62, n.pos.2), (pitch, yaw), yaw));
        }
    }
}

fn npc_command(server: &Server, player: &PlayerInfo, args: &[&str]) -> String {
    if !crate::commands::is_op(&player.name) {
        return "§cOnly operators can manage NPCs.".into();
    }
    match args {
        ["remove", name] => {
            let gone: Vec<Npc> = {
                let mut list = NPCS.lock();
                let gone = list.iter().filter(|n| n.name.eq_ignore_ascii_case(name)).cloned().collect();
                list.retain(|n| !n.name.eq_ignore_ascii_case(name));
                gone
            };
            if gone.is_empty() {
                return format!("§cNo NPC called {name}.");
            }
            for n in &gone {
                let _ = std::fs::remove_file(skin_file(n));
                let packet = Protocol::RemoveActorPacket(Box::new(bedrock::protocol::v662::packets::RemoveActorPacket {
                    target_actor_id: bedrock::protocol::v662::types::ActorUniqueID(n.id),
                }));
                let _ = server.events.send(crate::players::Event::Relay { from: 0, packet: Box::new(packet) });
            }
            save_npcs();
            format!("§aRemoved {name}.")
        }
        ["list"] => {
            let list = NPCS.lock();
            if list.is_empty() {
                return "§7No NPCs yet. Make one with /npc <name> <ip> <port> [scale] [player|fixed]".into();
            }
            list.iter().map(|n| format!("§b{} §7-> {}:{} (scale {}, {})", n.name, n.ip, n.port, n.scale, if n.look_at_player { "looks at players" } else { "fixed" })).collect::<Vec<_>>().join("\n")
        }
        [name, ip, port, rest @ ..] => {
            let Ok(port) = port.parse::<u16>() else { return "§cThe port must be a number, like 19132.".into() };
            let scale = rest.first().and_then(|s| s.parse::<f32>().ok()).unwrap_or(1.0).clamp(0.1, 10.0);
            let look_at_player = rest.get(1).is_none_or(|m| !m.eq_ignore_ascii_case("fixed"));
            let n = Npc {
                id: crate::players::next_id(),
                name: (*name).into(),
                ip: (*ip).into(),
                port,
                scale,
                look_at_player,
                pos: (player.pos.0, player.pos.1 - 1.62, player.pos.2),
                rot: player.rot,
                skin: player.skin.clone(),
                equipment: crate::players::equipment(player.id),
            };
            {
                let mut list = NPCS.lock();
                list.retain(|o| !o.name.eq_ignore_ascii_case(name));
                list.push(n.clone());
            }
            save_npcs();
            for p in server.all() {
                for packet in npc_packets(&n) {
                    send(server, p.id, packet);
                }
            }
            let unlist = n.clone();
            server.scheduler.run_later(40, move |server| {
                let _ = server.events.send(crate::players::Event::Relay { from: 0, packet: Box::new(npc_unlist(&unlist)) });
            });
            format!("§aMade NPC §b{name}§a. Clicking it sends players to {ip}:{port}.")
        }
        _ => "§cUse /npc <name> <ip> <port> [scale] [player|fixed], /npc remove <name> or /npc list".into(),
    }
}

// ---------------------------------------------------------------------------------------------

impl Plugin for Lobby {
    fn name(&self) -> &str {
        NAME
    }

    fn version(&self) -> &str {
        "1.0.0"
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
        load_npcs();
        start_pinging();
        server.scheduler.run_repeating(40, 40, |server| {
            for p in server.all() {
                refresh_hud(server, &p);
            }
            true
        });
        server.scheduler.run_repeating(5, 5, |server| {
            turn_npcs(server);
            true
        });
        println!("Lobby core on: {} servers, {} NPCs", servers().len(), NPCS.lock().len());
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
        lobby_items(server, player);
        let id = player.id;
        // A moment later, once the player is in the world.
        server.scheduler.run_later(20, move |server| {
            let Some(p) = server.get(id) else { return };
            show_npcs(server, id);
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

    fn on_entity_click(&self, server: &Server, player: &PlayerInfo, entity: i64, _attack: bool) {
        let target = NPCS.lock().iter().find(|n| n.id == entity).cloned();
        if let Some(n) = target {
            if n.ip.eq_ignore_ascii_case("menu") {
                open_selector(server, player.id);
            } else {
                server.tell(player.id, &format!("§7Sending you to §b{}§7...", n.name));
                server.transfer(player.id, &n.ip, n.port);
            }
        }
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

    fn on_move(&self, server: &Server, player: &PlayerInfo, to: (f32, f32, f32)) -> bool {
        if on() {
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
            ("npc", "Makes an NPC that sends players to a server"),
            ("setlobby", "Sets the lobby spawn to where you stand"),
        ]
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
            "npc" => Some(npc_command(server, player, args)),
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

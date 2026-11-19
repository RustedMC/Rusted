//! NPCs, like FancyNPCs: player-shaped NPCs with your skin and gear that do something when
//! clicked. Works on any server (the lobby core doesn't need to be on). Ops only.
//!
//! - `/npc create <name> [scale] [player|fixed]` where you stand, wearing your skin and gear
//!   (`player` = turns its head to look at each viewer)
//! - `/npc action <name> transfer <ip> <port>` | `command <command...>` | `message <text...>` |
//!   `menu <menu name>` | `servers` (the lobby's server selector)
//! - `/npc movehere <name>`, `/npc skin <name> <online player>`, `/npc scale <name> <n>`,
//!   `/npc remove <name>`, `/npc list`
//! - The old form still works: `/npc <name> <ip> <port> [scale] [player|fixed]`
//!
//! Saved in plugins/lobby/npcs.txt (skins in plugins/lobby/npc-skins/), where they've always been.

use crate::inventory::Stack;
use crate::players::{PlayerInfo, Server};
use crate::plugin::Plugin;
use crate::Protocol;
use parking_lot::Mutex;

const DIR: &str = "lobby";

#[derive(Clone)]
struct Npc {
    id: i64,
    name: String,
    /// What a click does: "cmd:<command>", "msg:<text>", "ui:<menu>", "menu" (server
    /// selector), or a server address to send players to (with `port`).
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

fn send(server: &Server, to: i64, packet: Protocol) {
    let _ = server.events.send(crate::players::Event::Packet { to, packet: Box::new(packet) });
}

fn npc_file() -> std::path::PathBuf {
    std::path::Path::new("plugins").join(DIR).join("npcs.txt")
}

fn skin_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new("plugins").join(DIR).join("npc-skins").join(format!("{}.skin", name.replace(|c: char| !c.is_ascii_alphanumeric(), "_")))
}

fn save_npcs() {
    use bedrock::protocol::ProtoCodec;
    let npcs = NPCS.lock().clone();
    let mut text = String::from("# name|action or ip|port|scale|look|x|y|z|yaw|pitch (skins in npc-skins/)\n");
    for n in &npcs {
        let action = n.ip.replace(['|', '\n'], " ");
        text.push_str(&format!("{}|{}|{}|{}|{}|{}|{}|{}|{}|{}\n", n.name, action, n.port, n.scale, if n.look_at_player { "player" } else { "fixed" }, n.pos.0, n.pos.1, n.pos.2, n.rot.1, n.rot.0));
        let mut bytes = Vec::new();
        if ProtoCodec::serialize(&*n.skin, &mut bytes).is_ok() {
            let path = skin_path(&n.name);
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
        let Ok(bytes) = std::fs::read(skin_path(f[0])) else { continue };
        let Ok(skin) = ProtoCodec::deserialize(&mut &bytes[..]) else { continue };
        out.push(Npc {
            id: crate::players::next_id(),
            name: f[0].into(),
            ip: f[1].into(),
            port: f[2].parse().unwrap_or(19132),
            scale: num(3).max(0.1),
            look_at_player: f[4] == "player",
            pos: (num(5), num(6), num(7)),
            rot: (num(9), num(8)),
            skin: std::sync::Arc::new(skin),
            equipment: Default::default(),
        });
    }
    *NPCS.lock() = out;
}

/// Second line of the name tag.
fn subtitle(n: &Npc) -> String {
    if n.ip.is_empty() {
        String::new()
    } else if n.ip.contains(':') || n.ip == "menu" {
        "§7Click me".into()
    } else {
        crate::lobby::players_text(&n.ip, n.port)
    }
}

/// A player-list entry (for the skin), the player itself, its gear and size. It comes off the
/// player list again (`npc_unlist`) so it isn't in the tab list.
fn npc_packets(n: &Npc) -> Vec<Protocol> {
    use bedrock::protocol::v2168::enums::DataItemType;
    use bedrock::protocol::v662::types::{ActorRuntimeID, DataItem, PropertySyncData};
    use bedrock::protocol::v924::enums::ActorDataIDs;
    let info = PlayerInfo {
        id: n.id,
        uuid: uuid::Uuid::from_u128(n.id as u128 ^ 0x5275_7374_6564_4e50_4300_0000_0000_0000),
        name: format!("§l§b{}\n{}", n.name, subtitle(n)),
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

fn remove_packet(n: &Npc) -> Protocol {
    Protocol::RemoveActorPacket(Box::new(bedrock::protocol::v662::packets::RemoveActorPacket {
        target_actor_id: bedrock::protocol::v662::types::ActorUniqueID(n.id),
    }))
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

/// Shows (or re-shows after a change) one NPC to everyone.
fn show_everyone(server: &Server, n: &Npc) {
    for p in server.all() {
        send(server, p.id, remove_packet(n));
        for packet in npc_packets(n) {
            send(server, p.id, packet);
        }
    }
    let unlist = n.clone();
    server.scheduler.run_later(40, move |server| {
        let _ = server.events.send(crate::players::Event::Relay { from: 0, packet: Box::new(npc_unlist(&unlist)) });
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

pub fn list() -> String {
    let list = NPCS.lock();
    if list.is_empty() {
        return "§7No NPCs yet. Make one with /npc create <name>".into();
    }
    list.iter()
        .map(|n| format!("§b{} §7-> {} (scale {}, {})", n.name, describe(n), n.scale, if n.look_at_player { "looks at players" } else { "fixed" }))
        .collect::<Vec<_>>()
        .join("\n")
}

fn describe(n: &Npc) -> String {
    match n.ip.split_once(':') {
        Some(("cmd", c)) => format!("runs /{c}"),
        Some(("msg", m)) => format!("says \"{m}\""),
        Some(("ui", m)) => format!("opens menu {m}"),
        _ if n.ip == "menu" => "opens the server selector".into(),
        _ => format!("sends to {}:{}", n.ip, n.port),
    }
}

/// Changes one NPC and shows the change to everyone.
fn edit(server: &Server, name: &str, f: impl FnOnce(&mut Npc)) -> bool {
    let changed = {
        let mut list = NPCS.lock();
        let Some(n) = list.iter_mut().find(|n| n.name.eq_ignore_ascii_case(name)) else { return false };
        f(n);
        n.clone()
    };
    save_npcs();
    show_everyone(server, &changed);
    true
}

fn create(server: &Server, player: &PlayerInfo, name: &str, action: String, port: u16, rest: &[&str]) -> String {
    let scale = rest.first().and_then(|s| s.parse::<f32>().ok()).unwrap_or(1.0).clamp(0.1, 10.0);
    let look_at_player = rest.get(1).is_none_or(|m| !m.eq_ignore_ascii_case("fixed"));
    let n = Npc {
        id: crate::players::next_id(),
        name: name.into(),
        ip: action,
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
        for old in list.iter().filter(|o| o.name.eq_ignore_ascii_case(name)) {
            let _ = server.events.send(crate::players::Event::Relay { from: 0, packet: Box::new(remove_packet(old)) });
        }
        list.retain(|o| !o.name.eq_ignore_ascii_case(name));
        list.push(n.clone());
    }
    save_npcs();
    show_everyone(server, &n);
    format!("§aMade NPC §b{name}§a: it {}. Change that with /npc action {name} ...", describe(&n))
}

fn npc_command(server: &Server, player: &PlayerInfo, args: &[&str]) -> String {
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
                let _ = std::fs::remove_file(skin_path(&n.name));
                let _ = server.events.send(crate::players::Event::Relay { from: 0, packet: Box::new(remove_packet(n)) });
            }
            save_npcs();
            format!("§aRemoved {name}.")
        }
        ["list"] => list(),
        ["create", name, rest @ ..] => create(server, player, name, format!("msg:§7Hi, I'm {name}!"), 0, rest),
        ["action", name, kind, value @ ..] => {
            let text = value.join(" ");
            let (action, port) = match (*kind, value) {
                ("transfer", [ip, port]) => match port.parse::<u16>() {
                    Ok(p) => (ip.to_string(), p),
                    Err(_) => return "§cThe port must be a number, like 19132.".into(),
                },
                ("command", [_, ..]) => (format!("cmd:{}", text.trim_start_matches('/')), 0),
                ("message", [_, ..]) => (format!("msg:{text}"), 0),
                ("menu", [m]) => (format!("ui:{m}"), 0),
                ("servers", []) => ("menu".into(), 0),
                _ => return "§cUse /npc action <name> transfer <ip> <port> | command <cmd> | message <text> | menu <name> | servers".into(),
            };
            if edit(server, name, |n| {
                n.ip = action;
                n.port = port;
            }) {
                format!("§aUpdated {name}.")
            } else {
                format!("§cNo NPC called {name}.")
            }
        }
        ["movehere", name] => {
            let (pos, rot) = ((player.pos.0, player.pos.1 - 1.62, player.pos.2), player.rot);
            if edit(server, name, |n| {
                n.pos = pos;
                n.rot = rot;
            }) { format!("§aMoved {name} here.") } else { format!("§cNo NPC called {name}.") }
        }
        ["skin", name, who] => {
            let Some(from) = server.all().into_iter().find(|p| p.name.eq_ignore_ascii_case(who)) else { return format!("§c{who} isn't online.") };
            let eq = crate::players::equipment(from.id);
            if edit(server, name, |n| {
                n.skin = from.skin.clone();
                n.equipment = eq;
            }) { format!("§a{name} now looks like {}.", from.name) } else { format!("§cNo NPC called {name}.") }
        }
        ["scale", name, s] => match s.parse::<f32>() {
            Ok(s) if edit(server, name, |n| n.scale = s.clamp(0.1, 10.0)) => format!("§a{name} resized."),
            Ok(_) => format!("§cNo NPC called {name}."),
            Err(_) => "§cUsage: /npc scale <name> <size>".into(),
        },
        // The original lobby form: /npc <name> <ip> <port> [scale] [player|fixed]
        [name, ip, port, rest @ ..] => match port.parse::<u16>() {
            Ok(port) => create(server, player, name, ip.to_string(), port, rest),
            Err(_) => "§cThe port must be a number, like 19132.".into(),
        },
        _ => "§6/npc §fcreate|action|movehere|skin|scale|remove|list §7(see /npc create <name> to start)".into(),
    }
}

pub struct Npcs;

impl Plugin for Npcs {
    fn name(&self) -> &str {
        "npcs"
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![("npc", "Makes and edits NPCs (ops)")]
    }

    fn on_server_start(&self, server: &Server) {
        load_npcs();
        server.scheduler.run_repeating(5, 5, |server| {
            turn_npcs(server);
            true
        });
    }

    fn on_join(&self, server: &Server, player: &PlayerInfo) {
        let id = player.id;
        // Once they're in the world.
        server.scheduler.run_later(20, move |server| show_npcs(server, id));
    }

    fn on_entity_click(&self, server: &Server, player: &PlayerInfo, entity: i64, _attack: bool) {
        let Some(n) = NPCS.lock().iter().find(|n| n.id == entity).cloned() else { return };
        match n.ip.split_once(':') {
            Some(("cmd", c)) => server.run_as(player.id, &crate::placeholders::fill(c, server, player)),
            Some(("msg", m)) => server.tell(player.id, &format!("§b{}§7: §f{}", n.name, crate::placeholders::fill(m, server, player))),
            Some(("ui", m)) => server.run_as(player.id, &format!("menu {m}")),
            _ if n.ip == "menu" => crate::lobby::open_servers(server, player.id),
            _ => {
                server.tell(player.id, &format!("§7Sending you to §b{}§7...", n.name));
                server.transfer(player.id, &n.ip, n.port);
            }
        }
    }

    fn usage(&self, command: &str) -> &'static [&'static str] {
        match command {
            "npc" => &["create <name> [scale] [player|fixed]", "action <name> transfer|command|message|menu|servers [value...]", "movehere <name>", "skin <name> <player>", "scale <name> <size>", "remove <name>", "list"],
            _ => &[],
        }
    }

    fn on_command(&self, server: &Server, player: &PlayerInfo, _name: &str, args: &[&str]) -> Option<String> {
        if !crate::commands::is_op(&player.name) {
            return Some("§cOnly operators can manage NPCs.".into());
        }
        Some(npc_command(server, player, args))
    }
}

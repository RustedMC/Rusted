//! Holograms: floating text in the world, with placeholders filled per player and refreshed
//! every 2 seconds (`{player}`, `{balance}`, `{online}`, `{rank}`...).
//!
//! - `/holo create <name> <text>` at your feet, `/holo addline <name> <text>`,
//!   `/holo setline <name> <n> <text>`, `/holo move <name>`, `/holo delete <name>`, `/holo list`
//!
//! Ops only. Stored in plugins/holograms/holograms.txt as `name = x,y,z|line 1|line 2`.
//! shortcut: holograms show in every dimension at their coordinates; store a dimension when
//! multiworld lands.

use crate::players::{PlayerInfo, Server};
use crate::plugin::{Config, Plugin};
use crate::Protocol;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};

const NAME: &str = "holograms";

#[derive(Clone)]
struct Holo {
    name: String,
    at: (f32, f32, f32),
    lines: Vec<String>,
}

fn all() -> Vec<Holo> {
    let book = Config::data(NAME, "holograms");
    book.entries()
        .filter_map(|(name, v)| {
            let mut parts = v.split('|');
            let n: Vec<f32> = parts.next()?.split(',').filter_map(|x| x.parse().ok()).collect();
            (n.len() == 3).then(|| Holo { name: name.to_string(), at: (n[0], n[1], n[2]), lines: parts.map(str::to_string).collect() })
        })
        .collect()
}

fn save(h: &Holo) {
    let mut book = Config::data(NAME, "holograms");
    let mut v = format!("{:.2},{:.2},{:.2}", h.at.0, h.at.1, h.at.2);
    for l in &h.lines {
        v.push('|');
        v.push_str(&l.replace('|', "/"));
    }
    book.set(&h.name, &v);
    let _ = book.save();
}

/// Entity id per hologram name, stable while the server runs.
fn entity_id(name: &str) -> i64 {
    let h = name.bytes().fold(1469598103934665603u64, |h, b| (h ^ b as u64).wrapping_mul(1099511628211));
    0x4000_0000_0000 | (h & 0xFFFF_FFFF) as i64
}

/// Which holograms each player has been sent.
static SHOWN: Mutex<Option<HashMap<i64, HashSet<String>>>> = Mutex::new(None);

fn send(server: &Server, to: i64, packet: Protocol) {
    let _ = server.events.send(crate::players::Event::Packet { to, packet: Box::new(packet) });
}

fn refresh(server: &Server) {
    use bedrock::protocol::v2168::enums::DataItemType;
    use bedrock::protocol::v662::types::{ActorRuntimeID, ActorUniqueID, DataItem, PropertySyncData};
    use bedrock::protocol::v924::enums::ActorDataIDs;
    let holos = all();
    let mut shown = SHOWN.lock();
    let shown = shown.get_or_insert_with(HashMap::new);
    for p in server.all() {
        let seen = shown.entry(p.id).or_default();
        // Deleted ones go away.
        let names: HashSet<String> = holos.iter().map(|h| h.name.clone()).collect();
        for gone in seen.iter().filter(|n| !names.contains(*n)).cloned().collect::<Vec<_>>() {
            send(server, p.id, Protocol::RemoveActorPacket(Box::new(bedrock::protocol::v662::packets::RemoveActorPacket { target_actor_id: ActorUniqueID(entity_id(&gone)) })));
            seen.remove(&gone);
        }
        for h in &holos {
            let id = entity_id(&h.name);
            let text = h.lines.iter().map(|l| crate::placeholders::fill(l, server, &p)).collect::<Vec<_>>().join("\n");
            if seen.insert(h.name.clone()) {
                // Invisible, no AI, name always shown.
                let flags = crate::spawn::FLAGS | 1 << 5 | 1 << 14 | 1 << 15 | 1 << 16;
                send(server, p.id, Protocol::AddActorPacket(Box::new(bedrock::protocol::v2225::packets::AddActorPacket {
                    target_actor_id: ActorUniqueID(id),
                    target_runtime_id: ActorRuntimeID(id as u64),
                    actor_type: "minecraft:armor_stand".into(),
                    position: h.at,
                    velocity: (0.0, 0.0, 0.0),
                    rotation: (0.0, 0.0),
                    y_head_rotation: 0.0,
                    y_body_rotation: 0.0,
                    attributes: vec![],
                    actor_data: vec![
                        DataItem { data_item_id: ActorDataIDs::Reserved0, data_item_type: DataItemType::Int64(flags) },
                        DataItem { data_item_id: ActorDataIDs::Name, data_item_type: DataItemType::String(text) },
                    ],
                    synced_properties: PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
                    actor_links: vec![],
                    passenger_of_block_data: None,
                })));
            } else {
                send(server, p.id, Protocol::SetActorDataPacket(Box::new(bedrock::protocol::v662::packets::SetActorDataPacket {
                    target_runtime_id: ActorRuntimeID(id as u64),
                    actor_data: vec![DataItem { data_item_id: ActorDataIDs::Name, data_item_type: DataItemType::String(text) }],
                    synced_properties: PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
                    tick: 0,
                })));
            }
        }
    }
}

/// Re-sends one hologram to everyone (after a move).
fn respawn(server: &Server, name: &str) {
    use bedrock::protocol::v662::types::ActorUniqueID;
    if let Some(shown) = SHOWN.lock().as_mut() {
        for (pid, seen) in shown.iter_mut() {
            if seen.remove(name) {
                send(server, *pid, Protocol::RemoveActorPacket(Box::new(bedrock::protocol::v662::packets::RemoveActorPacket { target_actor_id: ActorUniqueID(entity_id(name)) })));
            }
        }
    }
}

fn clean(name: &str) -> Option<String> {
    let n = name.to_lowercase();
    (!n.is_empty() && n.len() <= 24 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')).then_some(n)
}

fn command(server: &Server, p: &PlayerInfo, args: &[&str]) -> String {
    let usage = "§6/holo create §f<name> <text>\n§6/holo addline §f<name> <text>\n§6/holo setline §f<name> <line> <text>\n§6/holo move §f<name>\n§6/holo delete §f<name>\n§6/holo list";
    let here = (p.pos.0, p.pos.1 - 1.62 + 0.3, p.pos.2);
    let find = |n: &str| all().into_iter().find(|h| h.name == n);
    match args {
        ["list"] => {
            let names: Vec<String> = all().into_iter().map(|h| h.name).collect();
            if names.is_empty() { "§7No holograms.".into() } else { format!("§7Holograms: §f{}", names.join("§7, §f")) }
        }
        ["create", name, text @ ..] if !text.is_empty() => {
            let Some(name) = clean(name) else { return "§cNames are letters, numbers, _ and -".into() };
            if find(&name).is_some() {
                return format!("§c{name} already exists.");
            }
            save(&Holo { name: name.clone(), at: here, lines: vec![text.join(" ")] });
            refresh(server);
            format!("§aHologram {name} created.")
        }
        ["addline", name, text @ ..] if !text.is_empty() => match find(&name.to_lowercase()) {
            Some(mut h) => {
                h.lines.push(text.join(" "));
                save(&h);
                respawn(server, &h.name);
                refresh(server);
                "§aLine added.".into()
            }
            None => format!("§cNo hologram {name}."),
        },
        ["setline", name, n, text @ ..] if !text.is_empty() => match (find(&name.to_lowercase()), n.parse::<usize>()) {
            (Some(mut h), Ok(n)) if n >= 1 && n <= h.lines.len() => {
                h.lines[n - 1] = text.join(" ");
                save(&h);
                refresh(server);
                "§aLine updated.".into()
            }
            _ => "§cNo such hologram or line.".into(),
        },
        ["move", name] => match find(&name.to_lowercase()) {
            Some(mut h) => {
                h.at = here;
                save(&h);
                respawn(server, &h.name);
                refresh(server);
                "§aMoved here.".into()
            }
            None => format!("§cNo hologram {name}."),
        },
        ["delete", name] => {
            let mut book = Config::data(NAME, "holograms");
            book.remove(&name.to_lowercase());
            let _ = book.save();
            refresh(server);
            "§aDeleted.".into()
        }
        _ => usage.into(),
    }
}

pub struct Holograms;

impl Plugin for Holograms {
    fn name(&self) -> &str {
        NAME
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![("holo", "Floating text holograms (ops)")]
    }

    fn on_server_start(&self, server: &Server) {
        server.scheduler.run_repeating(40, 40, |server| {
            refresh(server);
            true
        });
    }

    fn on_leave(&self, _server: &Server, player: &PlayerInfo) {
        if let Some(s) = SHOWN.lock().as_mut() {
            s.remove(&player.id);
        }
    }

    fn usage(&self, command: &str) -> &'static [&'static str] {
        match command {
            "holo" => &["create <name> <text...>", "addline <name> <text...>", "setline <name> <line> <text...>", "move <name>", "delete <name>", "list"],
            _ => &[],
        }
    }

    fn on_command(&self, server: &Server, player: &PlayerInfo, _name: &str, args: &[&str]) -> Option<String> {
        if !crate::commands::is_op(&player.name) {
            return Some("§cYou do not have permission to use this command".into());
        }
        Some(command(server, player, args))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids_are_stable_and_distinct() {
        assert_eq!(super::entity_id("a"), super::entity_id("a"));
        assert_ne!(super::entity_id("a"), super::entity_id("b"));
        assert_eq!(super::clean("Top_Kills"), Some("top_kills".into()));
    }
}

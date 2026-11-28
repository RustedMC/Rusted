//! More vanilla commands that act on this player or around them: /clearspawnpoint, /hud, /fog,
//! /playanimation, /inputpermission, /replaceitem, /loot, /particle, /spreadplayers,
//! /testforblocks and /schedule. Commands with a player argument already arrive here as `@s`
//! in each target's session (commands::expand).

use super::*;
use bedrock::protocol::ProtoVersionPackets;
use players::Event;

type HudPacket = <Protocol as ProtoVersionPackets>::SetHudPacket;
type FogPacket = <Protocol as ProtoVersionPackets>::PlayerFogPacket;
type AnimatePacket = <Protocol as ProtoVersionPackets>::AnimateEntityPacket;
type ParticlePacket = <Protocol as ProtoVersionPackets>::SpawnParticleEffectPacket;
type InputLocksPacket = <Protocol as ProtoVersionPackets>::UpdateClientInputLocksPacket;

/// Bedrock ClientInputLocks bits, by /inputpermission name.
const INPUT_LOCKS: &[(&str, u32)] = &[
    ("camera", 1 << 1),
    ("movement", 1 << 2),
    ("lateral_movement", 1 << 4),
    ("sneak", 1 << 5),
    ("jump", 1 << 6),
    ("mount", 1 << 7),
    ("dismount", 1 << 8),
    ("move_forward", 1 << 9),
    ("move_backward", 1 << 10),
    ("move_left", 1 << 11),
    ("move_right", 1 << 12),
];

/// "20", "20t", "5s", "1d" (game days) -> ticks.
fn parse_ticks(s: &str) -> Option<u64> {
    let (n, mul) = match s.as_bytes().last()? {
        b't' => (&s[..s.len() - 1], 1),
        b's' => (&s[..s.len() - 1], 20),
        b'd' => (&s[..s.len() - 1], 24_000),
        _ => (s, 1),
    };
    n.parse::<f64>().ok().filter(|v| *v >= 0.0).map(|v| (v * mul as f64).round() as u64)
}

/// Highest block you can stand on at (x, z) in the overworld, as the feet position above it.
fn surface(world: &world::World, x: i32, z: i32) -> Option<i32> {
    (-64..320).rev().find(|&y| {
        let b = world.get(x, y, z);
        b != world::AIR && crate::overworld::feature::blocks::is_solid(b)
    }).map(|y| y + 1)
}

impl Session {
    /// None if `args` isn't one of these commands.
    pub(super) async fn vanilla_command(&mut self, args: &[&str], here: (f32, f32, f32)) -> Option<String> {
        Some(match args {
            ["clearspawnpoint", ..] => {
                commands::clear_spawn(&self.name);
                format!("Removed the spawn point of {}", self.name)
            }
            ["hud", _, action, rest @ ..] => {
                use bedrock::protocol::v786::enums::{HudElement as H, HudVisibility};
                let visibility = match *action {
                    "hide" => HudVisibility::Hide,
                    "reset" => HudVisibility::Reset,
                    _ => return Some("§cUse /hud <player> hide|reset [element]".into()),
                };
                let all = [H::PaperDoll, H::Armor, H::ToolTips, H::TouchControls, H::Crosshair, H::HotBar, H::Health, H::ProgressBar, H::Hunger, H::AirBubbles, H::HorseHealth, H::StatusEffects, H::ItemText];
                let elements = match rest.first().copied().unwrap_or("all") {
                    "all" => all.to_vec(),
                    "paperdoll" => vec![H::PaperDoll],
                    "armor" => vec![H::Armor],
                    "tooltips" => vec![H::ToolTips],
                    "touch_controls" => vec![H::TouchControls],
                    "crosshair" => vec![H::Crosshair],
                    "hotbar" => vec![H::HotBar],
                    "health" => vec![H::Health],
                    "progress_bar" => vec![H::ProgressBar],
                    "hunger" => vec![H::Hunger],
                    "air_bubbles" => vec![H::AirBubbles],
                    "horse_health" => vec![H::HorseHealth],
                    "status_effects" => vec![H::StatusEffects],
                    "item_text" => vec![H::ItemText],
                    other => return Some(format!("§cUnknown HUD element: {other}")),
                };
                let _ = self.conn.send(&[Protocol::SetHudPacket(Box::new(HudPacket { hud_elements_list: elements, hud_visibility: visibility }))]).await;
                String::new()
            }
            // /fog <player> push <fog id> <name> | pop <name> | remove <name>
            ["fog", _, action, rest @ ..] => {
                match (*action, rest) {
                    ("push", [fog, name, ..]) => self.fog.push((name.to_string(), fog.to_string())),
                    ("pop", [name, ..]) => {
                        if let Some(i) = self.fog.iter().rposition(|(n, _)| n == name) {
                            self.fog.remove(i);
                        }
                    }
                    ("remove", [name, ..]) => self.fog.retain(|(n, _)| n != name),
                    _ => return Some("§cUse /fog <player> push <fog id> <name> | pop <name> | remove <name>".into()),
                }
                let stack = self.fog.iter().map(|(_, f)| f.clone()).collect();
                let _ = self.conn.send(&[Protocol::PlayerFogPacket(Box::new(FogPacket { fog_stack: stack }))]).await;
                String::new()
            }
            // /playanimation <player> <animation> [next state] [blend out] [stop expression] [controller]
            ["playanimation", _, animation, rest @ ..] => {
                use bedrock::protocol::v662::enums::MolangVersion;
                let pkt = Protocol::AnimateEntityPacket(Box::new(AnimatePacket {
                    animation: animation.to_string(),
                    next_state: rest.first().copied().unwrap_or("default").to_string(),
                    stop_expression: rest.get(2).copied().unwrap_or("query.any_animation_finished").to_string(),
                    stop_expression_molang_version: MolangVersion::Initial,
                    controller: rest.get(3).copied().unwrap_or("__runtime_controller").to_string(),
                    blend_out_time: rest.get(1).and_then(|v| v.parse().ok()).unwrap_or(0.0),
                    runtime_ids: vec![ActorRuntimeID(self.id as u64)],
                }));
                let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(pkt) });
                String::new()
            }
            // /inputpermission set <player> <permission> enabled|disabled, /inputpermission query <player> <permission>
            ["inputpermission", action, _, perm, rest @ ..] => {
                let Some(&(_, bit)) = INPUT_LOCKS.iter().find(|(n, _)| n == perm) else {
                    return Some(format!("§cUnknown permission: {perm}. Known: {}", INPUT_LOCKS.iter().map(|p| p.0).collect::<Vec<_>>().join(", ")));
                };
                match (*action, rest.first().copied()) {
                    ("set", Some(state @ ("enabled" | "disabled"))) => {
                        if state == "disabled" { self.input_locks |= bit } else { self.input_locks &= !bit }
                        let _ = self.conn.send(&[Protocol::UpdateClientInputLocksPacket(Box::new(InputLocksPacket { input_lock_component_data: self.input_locks }))]).await;
                        format!("Set {perm} to {state} for {}", self.name)
                    }
                    ("query", _) => format!("{}: {perm} is {}", self.name, if self.input_locks & bit != 0 { "disabled" } else { "enabled" }),
                    _ => "§cUse /inputpermission set <player> <permission> enabled|disabled".into(),
                }
            }
            // /replaceitem entity <player> <slot type> <slot> <item> [amount]
            ["replaceitem", "entity", _, slot_type, slot, item, rest @ ..] => {
                let n: usize = slot.parse().unwrap_or(0);
                let target = match *slot_type {
                    "slot.hotbar" if n < 9 => Some(n),
                    "slot.inventory" if n < 27 => Some(9 + n),
                    "slot.armor.head" => Some(inventory::ARMOR),
                    "slot.armor.chest" => Some(inventory::ARMOR + 1),
                    "slot.armor.legs" => Some(inventory::ARMOR + 2),
                    "slot.armor.feet" => Some(inventory::ARMOR + 3),
                    "slot.weapon.mainhand" => Some(self.selected_slot as usize),
                    "slot.weapon.offhand" => Some(inventory::OFFHAND),
                    _ => None,
                };
                let count: i8 = rest.first().and_then(|c| c.parse().ok()).unwrap_or(1);
                match (target, items::item_by_name(item)) {
                    (None, _) => format!("§cBad slot: {slot_type} {slot}"),
                    (_, None) => format!("§cUnknown item: {item}"),
                    (Some(s), Some(it)) => {
                        let count = count.clamp(1, items::max_stack(it.id));
                        self.inv.set_slot(s, it, count);
                        let _ = self.conn.send(&[self.inv.content_packet()]).await;
                        save_player(&self.name, self.server.get(self.id).map_or(here, |p| p.pos), &self.inv);
                        format!("Replaced {slot_type} {slot} with {item} * {count}")
                    }
                }
            }
            ["replaceitem", "block", ..] => "§cUse /replaceitem entity; putting items in a block's slots isn't supported yet".into(),
            // /loot give <player> loot <table>, /loot spawn <x y z> loot <table>
            ["loot", "give", _, "loot", table, ..] => match crate::loot::roll_named(table) {
                Some(stacks) => {
                    let n = stacks.len();
                    for s in stacks {
                        let left = self.inv.add(s.item.clone(), s.count);
                        if left > 0 {
                            self.server.entities.drop_item(&self.server, s.item, left, self.server.get(self.id).map_or(here, |p| p.pos));
                        }
                    }
                    let _ = self.conn.send(&[self.inv.content_packet()]).await;
                    format!("Gave {n} stacks from {table} to {}", self.name)
                }
                None => format!("§cUnknown loot table: {table}. Known: {}", crate::loot::NAMES.join(", ")),
            },
            ["loot", "spawn", x, y, z, "loot", table, ..] => match (coords(&[x, y, z], here), crate::loot::roll_named(table)) {
                (None, _) => "§cBad position".into(),
                (_, None) => format!("§cUnknown loot table: {table}. Known: {}", crate::loot::NAMES.join(", ")),
                (Some(p), Some(stacks)) => {
                    let n = stacks.len();
                    for s in stacks {
                        self.server.entities.drop_item(&self.server, s.item, s.count, p);
                    }
                    format!("Dropped {n} stacks from {table}")
                }
            },
            ["loot", ..] => "§cUse /loot give <player> loot <table> or /loot spawn <x y z> loot <table>".into(),
            // /particle <effect> [x y z]
            ["particle", effect, rest @ ..] => {
                let at = if rest.len() >= 3 { coords(&rest[..3], here) } else { Some(here) };
                match at {
                    Some(at) => {
                        let pkt = Protocol::SpawnParticleEffectPacket(Box::new(ParticlePacket {
                            dimension_id: crate::dims::network_id(self.conn.dim) as i8,
                            actor_id: ActorUniqueID(-1),
                            position: at,
                            effect_name: effect.to_string(),
                            molang_variables: None,
                        }));
                        let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(pkt) });
                        format!("Playing effect {effect}")
                    }
                    None => "§cBad position".into(),
                }
            }
            // /spreadplayers <x> <z> <spread distance> <max range> <players...>
            ["spreadplayers", x, z, spread, range, targets @ ..] if !targets.is_empty() => {
                let parse = |s: &str, base: f32| match s.strip_prefix('~') {
                    Some(r) => r.parse::<f32>().ok().or(r.is_empty().then_some(0.0)).map(|d| base + d),
                    None => s.parse::<f32>().ok(),
                };
                let (Some(cx), Some(cz), Ok(spread), Ok(range)) = (parse(x, here.0), parse(z, here.2), spread.parse::<f32>(), range.parse::<f32>()) else {
                    return Some("§cUse /spreadplayers <x> <z> <spread distance> <max range> <players>".into());
                };
                if range < 1.0 || spread < 0.0 || spread > range * 2.0 {
                    return Some("§cThe max range must be at least 1 and wide enough for the spread distance".into());
                }
                let mut ids = Vec::new();
                for t in targets {
                    match commands::select(&self.server, self.id, t) {
                        Ok(v) => ids.extend(v),
                        Err(e) => return Some(e),
                    }
                }
                ids.sort();
                ids.dedup();
                let mut seed = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64)) | 1;
                let mut rand = move || {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    (seed >> 11) as f32 / (1u64 << 53) as f32
                };
                let mut placed: Vec<(f32, f32, i32)> = Vec::new();
                for _ in &ids {
                    // A few hundred tries for a spot with ground, far enough from the others.
                    let spot = (0..300).find_map(|_| {
                        let (px, pz) = (cx + (rand() * 2.0 - 1.0) * range, cz + (rand() * 2.0 - 1.0) * range);
                        let far = placed.iter().all(|(qx, qz, _)| (qx - px).hypot(qz - pz) >= spread);
                        let y = surface(&self.server.world, px.floor() as i32, pz.floor() as i32)?;
                        far.then_some((px.floor() + 0.5, pz.floor() + 0.5, y))
                    });
                    match spot {
                        Some(s) => placed.push(s),
                        None => return Some(format!("§cCould not spread {} players around {cx} {cz} (too many for the area?)", ids.len())),
                    }
                }
                for (id, (px, pz, y)) in ids.iter().zip(&placed) {
                    let line = format!("tp @s {px} {y} {pz}");
                    if *id == self.id {
                        let _ = self.cmd_tx.send((line, None));
                    } else {
                        let _ = self.server.events.send(Event::RunAs { to: *id, line, reply_to: self.id });
                    }
                }
                format!("Spread {} players around {cx} {cz}", ids.len())
            }
            // /testforblocks <begin> <end> <destination> [all|masked]
            ["testforblocks", x1, y1, z1, x2, y2, z2, dx, dy, dz, rest @ ..] => {
                match (coords(&[x1, y1, z1], here), coords(&[x2, y2, z2], here), coords(&[dx, dy, dz], here)) {
                    (Some(a), Some(b), Some(d)) => {
                        let f = |p: (f32, f32, f32)| (p.0.floor() as i32, p.1.floor() as i32, p.2.floor() as i32);
                        let (a, b, d) = (f(a), f(b), f(d));
                        let lo = (a.0.min(b.0), a.1.min(b.1), a.2.min(b.2));
                        let hi = (a.0.max(b.0), a.1.max(b.1), a.2.max(b.2));
                        let n = (hi.0 - lo.0 + 1) as i64 * (hi.1 - lo.1 + 1) as i64 * (hi.2 - lo.2 + 1) as i64;
                        if n > 524_288 {
                            return Some(format!("§cToo many blocks in the specified area ({n} > 524288)"));
                        }
                        let masked = rest.first() == Some(&"masked");
                        let w = &self.server.world;
                        let mut checked = 0;
                        for x in lo.0..=hi.0 {
                            for y in lo.1..=hi.1 {
                                for z in lo.2..=hi.2 {
                                    let src = w.get(x, y, z);
                                    if masked && src == world::AIR {
                                        continue;
                                    }
                                    if src != w.get(d.0 + x - lo.0, d.1 + y - lo.1, d.2 + z - lo.2) {
                                        return Some("§cSource and destination are not the same".into());
                                    }
                                    checked += 1;
                                }
                            }
                        }
                        format!("{checked} blocks compared")
                    }
                    _ => "§cBad position".into(),
                }
            }
            // /schedule delay add <function> <time>, /schedule on_area_loaded add ... <function>
            ["schedule", "delay", "add", function, time, ..] => match parse_ticks(time) {
                Some(ticks) => {
                    let tx = self.cmd_tx.clone();
                    let line = format!("function {function}");
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(ticks * 50)).await;
                        let _ = tx.send((line, None));
                    });
                    format!("Scheduled {function} in {ticks} ticks")
                }
                None => "§cUse a time like 20, 20t, 5s or 1d".into(),
            },
            // The whole world is always loaded here, so an area is loaded right away.
            ["schedule", "on_area_loaded", "add", rest @ ..] if rest.len() >= 2 => {
                let function = rest.last().unwrap();
                let _ = self.cmd_tx.send((format!("function {function}"), None));
                format!("Ran {function} (the area is already loaded)")
            }
            ["schedule", ..] => "§cUse /schedule delay add <function> <time> or /schedule on_area_loaded add <area> <function>".into(),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn schedule_times() {
        assert_eq!(super::parse_ticks("20"), Some(20));
        assert_eq!(super::parse_ticks("20t"), Some(20));
        assert_eq!(super::parse_ticks("5s"), Some(100));
        assert_eq!(super::parse_ticks("1d"), Some(24_000));
        assert_eq!(super::parse_ticks("x"), None);
        assert_eq!(super::parse_ticks("-1"), None);
    }
}

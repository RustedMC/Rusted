// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Dragon fight (from PNX EndDragonFight). Dragon and crystals show up when someone
//! enters the End, killing it opens the exit portal and a gateway.

use std::sync::{Mutex, OnceLock};

use crate::dims;
use crate::mobs::Kind;
use crate::players::{Event, Server};
use crate::world::AIR;

pub static END: OnceLock<std::sync::Arc<crate::end::End>> = OnceLock::new();

/// (dragon killed before, gateways opened). Saved in worlds/<level>/end_fight.txt.
static STATE: Mutex<Option<(bool, usize)>> = Mutex::new(None);

fn path() -> std::path::PathBuf {
    std::path::Path::new("worlds").join(&crate::config::get().level_name).join("end_fight.txt")
}

fn state() -> (bool, usize) {
    let mut s = STATE.lock().unwrap();
    *s.get_or_insert_with(|| {
        let t = std::fs::read_to_string(path()).unwrap_or_default();
        let mut it = t.split_whitespace().map(|v| v.parse::<usize>().unwrap_or(0));
        (it.next().unwrap_or(0) == 1, it.next().unwrap_or(0))
    })
}

fn save(v: (bool, usize)) {
    *STATE.lock().unwrap() = Some(v);
    let _ = std::fs::write(path(), format!("{} {}", v.0 as u8, v.1));
}

/// Starts the fight once when someone enters the End.
pub fn entered(server: &Server) {
    let (killed, _) = state();
    if killed || !server.mobs.of_kind(Kind::EnderDragon).is_empty() {
        return;
    }
    let Some(end) = END.get() else { return };
    let base = dims::base(dims::END) as f32;
    server.mobs.spawn(server, Kind::EnderDragon, (0.5, base + 128.0, 0.5));
    let crystals = server.mobs.of_kind(Kind::EndCrystal);
    for (x, z, _, h) in end.pillars() {
        let at = (x as f32 + 0.5, base + (h + 1) as f32, z as f32 + 0.5);
        if !crystals.iter().any(|(_, p, _)| (p.0 - at.0).abs() < 1.0 && (p.2 - at.2).abs() < 1.0) {
            server.mobs.spawn(server, Kind::EndCrystal, at);
        }
    }
}

/// Runs every second. Crystals within 32 blocks heal 1/s, boss bar tracks health,
/// shulkers get spawned into nearby end cities.
pub fn tick(server: &Server) {
    let in_end: Vec<i64> = server.all().into_iter().filter(|p| dims::of_f(p.pos.1) == dims::END).map(|p| p.id).collect();
    if in_end.is_empty() {
        return;
    }
    if let Some(&(id, pos, hp)) = server.mobs.of_kind(Kind::EnderDragon).first() {
        let near = server.mobs.of_kind(Kind::EndCrystal).iter().any(|(_, c, _)| (c.0 - pos.0).powi(2) + (c.1 - pos.1).powi(2) + (c.2 - pos.2).powi(2) < 32.0 * 32.0);
        if near && hp < 200.0 {
            server.mobs.heal(id, 1.0);
        }
        for &p in &in_end {
            crate::ui::boss_bar(server, p, "Ender Dragon", hp / 200.0, 5);
        }
    }
    spawn_shulkers(server);
}

/// Spawns end city shulkers once per chunk around players.
fn spawn_shulkers(server: &Server) {
    static DONE: Mutex<Option<std::collections::HashSet<(i32, i32)>>> = Mutex::new(None);
    let Some(end) = END.get() else { return };
    let base = dims::base(dims::END) as f32;
    for p in server.all().into_iter().filter(|p| dims::of_f(p.pos.1) == dims::END) {
        let (pcx, pcz) = ((p.pos.0.floor() as i32) >> 4, (p.pos.2.floor() as i32) >> 4);
        for cx in pcx - 3..=pcx + 3 {
            for cz in pcz - 3..=pcz + 3 {
                // TODO: remember which chunks already spawned, right now a restart spawns them again
                if !DONE.lock().unwrap().get_or_insert_with(Default::default).insert((cx, cz)) {
                    continue;
                }
                for (id, at) in end.entities(cx, cz) {
                    if id == "minecraft:shulker" {
                        server.mobs.spawn(server, Kind::Shulker, (at.0, base + at.1, at.2));
                    }
                }
            }
        }
    }
}

/// Dragon died. Opens the exit portal, egg on the first kill, next gateway,
/// and XP for the killer (12000 first time, 500 after).
pub fn dragon_died(server: &Server, killer: Option<i64>) {
    let (killed, gateways) = state();
    let Some(end) = END.get() else { return };
    let base = dims::base(dims::END);
    let y = base + end.exit_portal_y();
    if let Some(portal) = crate::blocks::any_state("minecraft:end_portal") {
        for x in -3i32..=3 {
            for z in -3i32..=3 {
                if x * x + z * z < 7 && (x, z) != (0, 0) && server.world.get(x, y, z) == AIR {
                    server.set_block_quiet(0, x, y, z, portal);
                }
            }
        }
    }
    if !killed && let Some(egg) = crate::blocks::any_state("minecraft:dragon_egg") {
        server.set_block_quiet(0, 0, y + 4, 0, egg);
    }
    let gates = end.gateways();
    if let Some(&(gx, gy, gz)) = gates.get(gateways)
        && let (Some(gate), Some(bedrock)) = (crate::blocks::any_state("minecraft:end_gateway"), crate::blocks::any_state("minecraft:bedrock"))
    {
        server.set_block_quiet(0, gx, base + gy, gz, gate);
        server.set_block_quiet(0, gx, base + gy + 1, gz, bedrock);
        server.set_block_quiet(0, gx, base + gy - 1, gz, bedrock);
    }
    if let Some(p) = killer {
        let _ = server.events.send(Event::Xp { to: p, points: if killed { 500 } else { 12_000 } });
    }
    for p in server.all() {
        crate::ui::remove_boss_bar(server, p.id);
    }
    server.broadcast("The Ender Dragon has been slain!");
    save((true, (gateways + 1).min(gates.len())));
}

/// End gateway destination. Main island gateways throw you ~1000 blocks out (onto a small
/// platform if there's no land), outer ones bring you back.
pub fn gateway_exit(server: &Server, from: (f32, f32, f32)) -> Option<(f32, f32, f32)> {
    if dims::of_f(from.1) != dims::END {
        return None;
    }
    let base = dims::base(dims::END);
    let (x, z) = (from.0, from.2);
    let d = (x * x + z * z).sqrt().max(1.0);
    let (tx, tz) = if d < 200.0 { ((x / d * 1024.0) as i32, (z / d * 1024.0) as i32) } else { ((x / d * 60.0) as i32, (z / d * 60.0) as i32) };
    let top = (base..base + 250).rev().find(|&y| server.world.get(tx, y, tz) != AIR);
    let y = match top {
        Some(y) => y + 1,
        None => {
            let stone = crate::blocks::any_state("minecraft:end_stone")?;
            for dx in -2..=2 {
                for dz in -2..=2 {
                    server.set_block_quiet(0, tx + dx, base + 69, tz + dz, stone);
                }
            }
            base + 70
        }
    };
    Some((tx as f32 + 0.5, y as f32 + 1.62, tz as f32 + 0.5))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateways_throw_you_out_and_back() {
        crate::blocks::init();
        let server = Server::new();
        let base = dims::base(dims::END) as f32;
        // gateway 96 east should land ~1024 out, on a platform over the void
        let out = gateway_exit(&server, (96.5, base + 76.0, 0.5)).unwrap();
        assert!((out.0 - 1024.0).abs() < 2.0 && dims::of_f(out.1) == dims::END, "{out:?}");
        assert_ne!(server.world.get(out.0.floor() as i32, base as i32 + 69, out.2.floor() as i32), AIR, "platform built");
        let back = gateway_exit(&server, out).unwrap();
        assert!((back.0 - 60.0).abs() < 2.0, "{back:?}");
        assert!(gateway_exit(&server, (0.0, 64.0, 0.0)).is_none(), "overworld");
    }
}

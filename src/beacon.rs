//! Beacons (PNX `BlockEntityBeacon`, numbers from the Minecraft Wiki). A beacon on a pyramid of
//! iron, gold, emerald, diamond or netherite blocks gives nearby players the effects chosen in
//! its screen. Choices are saved in worlds/<level>/beacons.txt.

use crate::players::{Event, Server};
use crate::world::World;
use parking_lot::Mutex;
use std::collections::HashMap;

pub const SPEED: i32 = 1;
pub const HASTE: i32 = 3;
pub const STRENGTH: i32 = 5;
pub const JUMP_BOOST: i32 = 8;
pub const REGENERATION: i32 = 10;
pub const RESISTANCE: i32 = 11;

/// Items the beacon takes as payment.
pub const PAYMENT: [&str; 5] = ["minecraft:iron_ingot", "minecraft:gold_ingot", "minecraft:emerald", "minecraft:diamond", "minecraft:netherite_ingot"];

/// Beacon position -> (primary, secondary) effect ids, 0 for none.
fn beacons() -> &'static Mutex<HashMap<(i32, i32, i32), (i32, i32)>> {
    static B: std::sync::OnceLock<Mutex<HashMap<(i32, i32, i32), (i32, i32)>>> = std::sync::OnceLock::new();
    B.get_or_init(|| {
        let text = file().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        let map = text
            .lines()
            .filter_map(|l| {
                let v: Vec<i32> = l.split_whitespace().filter_map(|n| n.parse().ok()).collect();
                (v.len() == 5).then(|| ((v[0], v[1], v[2]), (v[3], v[4])))
            })
            .collect();
        Mutex::new(map)
    })
}

fn file() -> Option<std::path::PathBuf> {
    if cfg!(test) {
        return None;
    }
    Some(std::path::Path::new("worlds").join(&crate::config::get().level_name).join("beacons.txt"))
}

fn save(map: &HashMap<(i32, i32, i32), (i32, i32)>) {
    let Some(path) = file() else { return };
    let text: String = map.iter().map(|((x, y, z), (p, s))| format!("{x} {y} {z} {p} {s}\n")).collect();
    let tmp = path.with_extension("tmp");
    if let Err(e) = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &path)) {
        println!("couldn't save beacons to {}: {e}", path.display());
    }
}

/// How many full pyramid layers are under the beacon (0-4).
pub fn level(w: &World, (x, y, z): (i32, i32, i32)) -> i32 {
    let base = |b: i32| {
        crate::blocks::entry(b).is_some_and(|e| {
            matches!(e.name.as_str(), "minecraft:iron_block" | "minecraft:gold_block" | "minecraft:emerald_block" | "minecraft:diamond_block" | "minecraft:netherite_block")
        })
    };
    let mut layers = 0;
    for l in 1..=4 {
        let full = (-l..=l).all(|dx| (-l..=l).all(|dz| base(w.get(x + dx, y - l, z + dz))));
        if !full {
            break;
        }
        layers = l;
    }
    layers
}

/// Whether `effect` may be picked as primary at this pyramid level.
fn primary_ok(effect: i32, level: i32) -> bool {
    match effect {
        SPEED | HASTE => level >= 1,
        RESISTANCE | JUMP_BOOST => level >= 2,
        STRENGTH => level >= 3,
        _ => false,
    }
}

/// The player picked effects in the beacon screen and paid. False if the choice isn't allowed
/// for this pyramid.
pub fn configure(w: &World, at: (i32, i32, i32), primary: i32, secondary: i32) -> bool {
    let lvl = level(w, at);
    // At level 4 the secondary is regeneration or the primary again (level II).
    let secondary_ok = secondary == 0 || lvl >= 4 && (secondary == REGENERATION || secondary == primary);
    if !primary_ok(primary, lvl) || !secondary_ok {
        return false;
    }
    let mut map = beacons().lock();
    map.insert(at, (primary, secondary));
    save(&map);
    true
}

/// The effects each beacon gives right now: (player, effect, amplifier, ticks).
fn effects(server: &Server) -> Vec<(i64, i32, i32, i32)> {
    let w = &server.world;
    let mut out = Vec::new();
    let mut gone = Vec::new();
    let list: Vec<_> = beacons().lock().iter().map(|(p, e)| (*p, *e)).collect();
    for (at, (primary, secondary)) in list {
        if crate::blocks::entry(w.get(at.0, at.1, at.2)).is_none_or(|e| e.name != "minecraft:beacon") {
            gone.push(at);
            continue;
        }
        let lvl = level(w, at);
        if lvl == 0 {
            continue;
        }
        // Range 10 blocks per layer plus 10; lasts 9 s plus 2 s per layer (vanilla).
        let range = (10 + 10 * lvl) as f32;
        let ticks = (9 + 2 * lvl) * 20;
        let (cx, cz) = (at.0 as f32 + 0.5, at.2 as f32 + 0.5);
        for p in server.all() {
            if (p.pos.0 - cx).abs() > range || (p.pos.2 - cz).abs() > range || crate::dims::of(p.pos.1 as i32) != crate::dims::of(at.1) {
                continue;
            }
            if primary_ok(primary, lvl) {
                let amp = (lvl >= 4 && secondary == primary) as i32;
                out.push((p.id, primary, amp, ticks));
            }
            if lvl >= 4 && secondary == REGENERATION {
                out.push((p.id, REGENERATION, 0, ticks));
            }
        }
    }
    if !gone.is_empty() {
        let mut map = beacons().lock();
        gone.iter().for_each(|p| {
            map.remove(p);
        });
        save(&map);
    }
    out
}

/// Every 4 s (vanilla), refresh nearby players' beacon effects.
pub fn tick(server: &Server) {
    static TICKS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    // Called at 10 Hz.
    if !TICKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed).is_multiple_of(40) {
        return;
    }
    for (to, effect, amplifier, ticks) in effects(server) {
        let _ = server.events.send(Event::Effect { to, effect, amplifier, ticks });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pyramid_levels_choices_and_range() {
        let server = Server::new();
        server.join(crate::players::tests::sample(3));
        let iron = crate::blocks::any_state("minecraft:iron_block").unwrap();
        let beacon = crate::blocks::any_state("minecraft:beacon").unwrap();
        let at = (300, -50, 300);
        server.world.set(at.0, at.1, at.2, beacon);
        assert_eq!(level(&server.world, at), 0);
        // A 3x3 layer: level 1, so speed but not strength.
        for l in 1..=1 {
            for dx in -l..=l {
                for dz in -l..=l {
                    server.world.set(at.0 + dx, at.1 - l, at.2 + dz, iron);
                }
            }
        }
        assert_eq!(level(&server.world, at), 1);
        assert!(!configure(&server.world, at, STRENGTH, 0), "strength needs 3 layers");
        assert!(!configure(&server.world, at, SPEED, REGENERATION), "regeneration needs 4 layers");
        // Full 4 layers: speed II.
        for l in 2..=4 {
            for dx in -l..=l {
                for dz in -l..=l {
                    server.world.set(at.0 + dx, at.1 - l, at.2 + dz, iron);
                }
            }
        }
        assert_eq!(level(&server.world, at), 4);
        assert!(configure(&server.world, at, SPEED, SPEED));
        // A player 40 blocks away (inside the 50-block range) gets Speed II for 17 s.
        server.moved(3, (340.5, -40.0, 300.5), (0.0, 0.0), 0.0);
        let got: Vec<_> = effects(&server).into_iter().filter(|e| e.0 == 3).collect();
        assert!(got.contains(&(3, SPEED, 1, 17 * 20)), "{got:?}");
        // 60 blocks away: nothing.
        server.moved(3, (360.5, -40.0, 300.5), (0.0, 0.0), 0.0);
        assert!(!effects(&server).iter().any(|e| e.0 == 3), "out of range");
        // Breaking the beacon forgets it.
        server.world.set(at.0, at.1, at.2, crate::world::AIR);
        effects(&server);
        assert!(!beacons().lock().contains_key(&at));
    }
}

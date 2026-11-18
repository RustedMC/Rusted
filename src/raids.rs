// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Pillager patrols and raids (from PNX VillageRaid). Patrols start on day 5, killing the
//! captain gives Bad Omen, and bringing that to a village starts a raid.

use std::sync::Mutex;

use crate::mobs::Kind;
use crate::players::{Event, Server};

struct Raid {
    center: (f32, f32, f32),
    wave: usize,
    waves: usize,
    raiders: Vec<i64>,
    wait: u32,
}

static RAIDS: Mutex<Vec<Raid>> = Mutex::new(Vec::new());

/// Raiders per wave, vanilla normal difficulty. Columns are pillagers, vindicators,
/// ravagers, witches, evokers.
const WAVES: [[u8; 5]; 7] = [[4, 0, 0, 0, 0], [3, 2, 0, 0, 0], [3, 0, 0, 1, 0], [4, 1, 1, 3, 0], [4, 4, 0, 0, 1], [4, 2, 1, 0, 1], [2, 5, 0, 1, 2]];

fn surface(server: &Server, x: i32, z: i32, near_y: i32) -> f32 {
    let w = &server.world;
    let mut y = (near_y + 30).min(318);
    while y > -60 && !crate::overworld::feature::blocks::is_solid(w.get(x, y, z)) {
        y -= 1;
    }
    (y + 1) as f32
}

/// Average villager position near `at`, used as the village centre.
pub fn village_near(server: &Server, at: (f32, f32, f32)) -> Option<(f32, f32, f32)> {
    let near: Vec<_> = server.mobs.of_kind(Kind::Villager).into_iter().map(|v| v.1).filter(|p| (p.0 - at.0).powi(2) + (p.2 - at.2).powi(2) < 48.0 * 48.0).collect();
    if near.is_empty() {
        return None;
    }
    let n = near.len() as f32;
    Some((near.iter().map(|p| p.0).sum::<f32>() / n, near.iter().map(|p| p.1).sum::<f32>() / n, near.iter().map(|p| p.2).sum::<f32>() / n))
}

/// Starts a raid when a Bad Omen player reaches a village (unless one's already going).
pub fn start(server: &Server, center: (f32, f32, f32)) -> bool {
    let mut raids = RAIDS.lock().unwrap();
    if raids.iter().any(|r| (r.center.0 - center.0).powi(2) + (r.center.2 - center.2).powi(2) < 96.0 * 96.0) {
        return false;
    }
    let waves = match *crate::commands::DIFFICULTY.lock().unwrap() {
        0 => return false,
        1 => 3,
        2 => 5,
        _ => 7,
    };
    raids.push(Raid { center, wave: 0, waves, raiders: vec![], wait: 5 });
    drop(raids);
    let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(crate::play_sound("raid.horn", center)) });
    true
}

/// Every second. Patrols, waves, boss bars, win/lose.
pub fn tick(server: &Server, roll: u32) {
    patrols(server, roll);
    let mut raids = RAIDS.lock().unwrap();
    let alive: std::collections::HashSet<i64> = Kind::ALL
        .iter()
        .filter(|k| matches!(k, Kind::Pillager | Kind::Vindicator | Kind::Ravager | Kind::Witch | Kind::Evoker))
        .flat_map(|k| server.mobs.of_kind(*k))
        .map(|m| m.0)
        .collect();
    let mut finished = Vec::new();
    for (i, r) in raids.iter_mut().enumerate() {
        r.raiders.retain(|id| alive.contains(id));
        let villagers = village_near(server, r.center).is_some();
        let watchers: Vec<i64> = server.all().into_iter().filter(|p| (p.pos.0 - r.center.0).powi(2) + (p.pos.2 - r.center.2).powi(2) < 96.0 * 96.0).map(|p| p.id).collect();
        if !villagers {
            for &p in &watchers {
                crate::ui::remove_boss_bar(server, p);
                server.tell(p, "§cRaid - Defeat");
            }
            finished.push(i);
            continue;
        }
        if r.raiders.is_empty() {
            if r.wave >= r.waves {
                for &p in &watchers {
                    crate::ui::remove_boss_bar(server, p);
                    server.tell(p, "§aRaid - Victory");
                    let _ = server.events.send(Event::Effect { to: p, effect: crate::effects::HERO_OF_THE_VILLAGE, amplifier: 0, ticks: 48_000 });
                }
                finished.push(i);
                continue;
            }
            if r.wait > 0 {
                r.wait -= 1;
            } else {
                // next wave comes in from one side
                let wave = WAVES[r.wave.min(WAVES.len() - 1)];
                let a = (roll % 628) as f32 / 100.0;
                let (sx, sz) = (r.center.0 + a.cos() * 32.0, r.center.2 + a.sin() * 32.0);
                for (kind, n) in [Kind::Pillager, Kind::Vindicator, Kind::Ravager, Kind::Witch, Kind::Evoker].into_iter().zip(wave) {
                    for j in 0..n {
                        let (x, z) = (sx + (j as f32 * 1.3) % 4.0, sz + (j as f32 * 0.7) % 4.0);
                        let y = surface(server, x.floor() as i32, z.floor() as i32, r.center.1 as i32);
                        let id = server.mobs.spawn(server, kind, (x, y, z));
                        if id != 0 {
                            r.raiders.push(id);
                        }
                    }
                }
                r.wave += 1;
                r.wait = 15;
                let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(crate::play_sound("raid.horn", r.center)) });
            }
        }
        let progress = if r.raiders.is_empty() { 0.0 } else { r.raiders.len() as f32 / 10.0 };
        for &p in &watchers {
            crate::ui::boss_bar(server, p, &format!("Raid - Wave {}/{}", r.wave, r.waves), progress, 2);
        }
    }
    for i in finished.into_iter().rev() {
        raids.remove(i);
    }
}

/// Vanilla patrols: from day 5, every so often, 24-48 blocks from a player.
fn patrols(server: &Server, roll: u32) {
    if server.time() / 24_000 < 5 || !roll.is_multiple_of(1200) || *crate::commands::DIFFICULTY.lock().unwrap() == 0 {
        return;
    }
    let Some(p) = server.all().into_iter().find(|p| crate::dims::of_f(p.pos.1) == crate::dims::OVERWORLD) else { return };
    let a = (roll / 1200 % 628) as f32 / 100.0;
    let (x, z) = (p.pos.0 + a.cos() * 36.0, p.pos.2 + a.sin() * 36.0);
    for i in 0..2 + roll % 4 {
        let (px, pz) = (x + i as f32, z + (i % 2) as f32);
        let y = surface(server, px.floor() as i32, pz.floor() as i32, p.pos.1 as i32);
        let id = server.mobs.spawn(server, Kind::Pillager, (px, y, pz));
        if i == 0 && id != 0 {
            server.mobs.set_captain(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raid_needs_villagers_and_runs_waves() {
        crate::blocks::init();
        let server = Server::new();
        assert!(village_near(&server, (0.0, -60.0, 0.0)).is_none());
        server.mobs.spawn(&server, Kind::Villager, (2.5, -60.0, 2.5));
        let c = village_near(&server, (0.0, -60.0, 0.0)).unwrap();
        assert!(start(&server, c));
        assert!(!start(&server, c), "one raid per village");
        for _ in 0..7 {
            tick(&server, 1);
        }
        assert!(!server.mobs.of_kind(Kind::Pillager).is_empty(), "first wave of pillagers");
        RAIDS.lock().unwrap().clear();
    }
}

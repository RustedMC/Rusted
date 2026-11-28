//! Sculk sensors and shriekers (vanilla game events). Footsteps, block changes and other
//! vibrations within 8 blocks wake a sensor: it gives a redstone signal for 2 seconds that's
//! stronger the closer the vibration was, then rests a moment. A sensor going off near a
//! shrieker makes it shriek; natural shriekers warn nearby players, and the fourth warning
//! calls a warden.

use crate::blocks::{self, State};
use crate::players::{Event, Server};
use parking_lot::Mutex;
use std::collections::HashMap;

type Pos = (i32, i32, i32);

/// Ticks (10 a second) a sensor stays active (vanilla 40 game ticks), then rests.
const ACTIVE: u32 = 20;
const COOLDOWN: u32 = 1;
/// Ticks a shrieker shrieks for (vanilla 90 game ticks).
const SHRIEK: u32 = 45;
const RANGE: i32 = 8;

#[derive(Default)]
struct Sculk {
    /// Active sensors: signal strength and ticks left.
    active: HashMap<Pos, (u8, u32)>,
    /// Resting sensors: ticks left.
    resting: HashMap<Pos, u32>,
    /// Shrieking shriekers: ticks left.
    shrieking: HashMap<Pos, u32>,
    /// Per player: warning level (0-4) and ticks until it drops by one.
    warnings: HashMap<i64, (u8, u32)>,
    /// Per player: ticks until their footsteps count again (one every half second).
    steps: HashMap<i64, u32>,
}

static S: Mutex<Option<Sculk>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Sculk) -> R) -> R {
    f(S.lock().get_or_insert_with(Default::default))
}

fn name(b: i32) -> &'static str {
    blocks::entry(b).map_or("", |e| e.name.as_str()).trim_start_matches("minecraft:")
}

fn is_sensor(b: i32) -> bool {
    matches!(name(b), "sculk_sensor" | "calibrated_sculk_sensor")
}

fn is_sculk(b: i32) -> bool {
    name(b).starts_with("sculk") || name(b) == "calibrated_sculk_sensor"
}

/// Redstone signal an active sensor gives out (0 if it isn't active).
pub fn power(p: Pos) -> u8 {
    with(|s| s.active.get(&p).map_or(0, |a| a.0))
}

fn relay(server: &Server, p: crate::Protocol) {
    let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(p) });
}

fn set_phase(server: &Server, p: Pos, phase: i32) {
    let b = server.world.get(p.0, p.1, p.2);
    if blocks::has_state(b, "sculk_sensor_phase") {
        server.set_block_quiet(0, p.0, p.1, p.2, blocks::with(b, &[("sculk_sensor_phase", State::Int(phase))]));
    }
    crate::redstone::mark(p);
}

/// Something made a vibration at `at` (`by` is the player, if one did it).
pub fn vibrate(server: &Server, at: (f32, f32, f32), by: Option<i64>) {
    let c = (at.0.floor() as i32, at.1.floor() as i32, at.2.floor() as i32);
    let mut woken = Vec::new();
    for dx in -RANGE..=RANGE {
        for dy in -RANGE..=RANGE {
            for dz in -RANGE..=RANGE {
                let p = (c.0 + dx, c.1 + dy, c.2 + dz);
                if !is_sensor(server.world.get(p.0, p.1, p.2)) {
                    continue;
                }
                let d = ((dx * dx + dy * dy + dz * dz) as f32).sqrt();
                if d > RANGE as f32 {
                    continue;
                }
                // Closer is stronger: 15 right next to it, down to 1 at 8 blocks.
                let strength = (15.0 - (d / RANGE as f32 * 14.0)).round().clamp(1.0, 15.0) as u8;
                woken.push((p, strength));
            }
        }
    }
    let woken: Vec<_> = with(|s| {
        let fresh: Vec<_> = woken.into_iter().filter(|(p, _)| !s.active.contains_key(p) && !s.resting.contains_key(p)).collect();
        for &(p, st) in &fresh {
            s.active.insert(p, (st, ACTIVE));
        }
        fresh
    });
    for (p, _) in &woken {
        set_phase(server, *p, 1);
        relay(server, crate::packets::play_sound("block.sculk_sensor.clicking", (p.0 as f32 + 0.5, p.1 as f32 + 0.5, p.2 as f32 + 0.5)));
        // A sensor going off sets off shriekers near it.
        for dx in -RANGE..=RANGE {
            for dy in -RANGE..=RANGE {
                for dz in -RANGE..=RANGE {
                    let q = (p.0 + dx, p.1 + dy, p.2 + dz);
                    if name(server.world.get(q.0, q.1, q.2)) == "sculk_shrieker" {
                        shriek(server, q, by);
                    }
                }
            }
        }
    }
}

/// A player walked: every half second of walking (not sneaking) is a vibration.
pub fn footstep(server: &Server, player: i64, feet: (f32, f32, f32), sneaking: bool) {
    if sneaking || !with(|s| {
        let left = s.steps.entry(player).or_insert(0);
        if *left > 0 {
            return false;
        }
        *left = 5;
        true
    }) {
        return;
    }
    // Stepping onto a shrieker sets it off directly.
    let below = (feet.0.floor() as i32, (feet.1 - 0.5).floor() as i32, feet.2.floor() as i32);
    if name(server.world.get(below.0, below.1, below.2)) == "sculk_shrieker" {
        shriek(server, below, Some(player));
    }
    vibrate(server, feet, Some(player));
}

/// A block changed at `p`: a vibration, unless it's the sculk itself changing state.
pub fn block_changed(server: &Server, p: Pos, old: i32, new: i32) {
    if is_sculk(old) && is_sculk(new) {
        return;
    }
    vibrate(server, (p.0 as f32 + 0.5, p.1 as f32 + 0.5, p.2 as f32 + 0.5), None);
}

fn shriek(server: &Server, p: Pos, by: Option<i64>) {
    let b = server.world.get(p.0, p.1, p.2);
    if with(|s| s.shrieking.contains_key(&p)) {
        return;
    }
    with(|s| s.shrieking.insert(p, SHRIEK));
    server.plugins.shriek(server, p, by);
    if blocks::has_state(b, "active") {
        server.set_block_quiet(0, p.0, p.1, p.2, blocks::with(b, &[("active", State::Bool(true))]));
    }
    let at = (p.0 as f32 + 0.5, p.1 as f32 + 0.5, p.2 as f32 + 0.5);
    relay(server, crate::packets::play_sound("block.sculk_shrieker.shriek", at));
    // Only natural shriekers (can_summon) warn players and call wardens.
    if !matches!(blocks::state(b, "can_summon"), Some(State::Bool(true))) {
        return;
    }
    let mut summon = false;
    for pl in server.all() {
        let d2 = (pl.pos.0 - at.0).powi(2) + (pl.pos.1 - at.1).powi(2) + (pl.pos.2 - at.2).powi(2);
        if d2 > 40.0 * 40.0 {
            continue;
        }
        // Darkness for everyone near; the warning level only rises for whoever set it off.
        let _ = server.events.send(Event::Effect { to: pl.id, effect: crate::effects::DARKNESS, amplifier: 0, ticks: 12 * 20 });
        if by == Some(pl.id) {
            let level = with(|s| {
                let w = s.warnings.entry(pl.id).or_insert((0, 0));
                w.0 = (w.0 + 1).min(4);
                w.1 = 10 * 60 * 10; // drops one level every 10 minutes
                w.0
            });
            if level >= 4 {
                summon = true;
                with(|s| s.warnings.insert(pl.id, (0, 0)));
            }
        }
    }
    if summon && server.plugins.warden_summon(server, (at.0 + 2.0, at.1, at.2)) {
        server.mobs.spawn(server, crate::mobs::Kind::Warden, (at.0 + 2.0, at.1, at.2));
        relay(server, crate::packets::play_sound("mob.warden.emerge", at));
    }
}

/// Run at 10 Hz: sensors go quiet and rest, shriekers stop, warnings fade.
pub fn tick(server: &Server) {
    let (ended, rested, quiet) = with(|s| {
        let mut ended = Vec::new();
        s.active.retain(|&p, (_, t)| {
            *t -= 1;
            if *t == 0 {
                ended.push(p);
            }
            *t > 0
        });
        for &p in &ended {
            s.resting.insert(p, COOLDOWN);
        }
        let mut rested = Vec::new();
        s.resting.retain(|&p, t| {
            if *t == 0 {
                rested.push(p);
                return false;
            }
            *t -= 1;
            true
        });
        let mut quiet = Vec::new();
        s.shrieking.retain(|&p, t| {
            *t -= 1;
            if *t == 0 {
                quiet.push(p);
            }
            *t > 0
        });
        for w in s.warnings.values_mut() {
            if w.1 > 0 {
                w.1 -= 1;
                if w.1 == 0 && w.0 > 0 {
                    w.0 -= 1;
                    w.1 = 10 * 60 * 10;
                }
            }
        }
        for t in s.steps.values_mut() {
            *t = t.saturating_sub(1);
        }
        (ended, rested, quiet)
    });
    for p in ended {
        set_phase(server, p, 2);
    }
    for p in rested {
        set_phase(server, p, 0);
    }
    for p in quiet {
        let b = server.world.get(p.0, p.1, p.2);
        if blocks::has_state(b, "active") {
            server.set_block_quiet(0, p.0, p.1, p.2, blocks::with(b, &[("active", State::Bool(false))]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensors_hear_close_vibrations_and_power_redstone() {
        crate::blocks::init();
        let server = Server::new();
        let sensor = blocks::any_state("minecraft:sculk_sensor").expect("sculk sensor block");
        assert!(blocks::has_state(sensor, "sculk_sensor_phase"), "Bedrock names the state sculk_sensor_phase");
        let p = (500, -60, 500);
        server.world.set(p.0, p.1, p.2, sensor);
        vibrate(&server, (503.5, -60.0, 500.5), None);
        let near = power(p);
        assert!(near > 0, "woke up");
        assert!(matches!(blocks::state(server.world.get(p.0, p.1, p.2), "sculk_sensor_phase"), Some(State::Int(1))));
        for _ in 0..ACTIVE + COOLDOWN + 1 {
            tick(&server);
        }
        assert_eq!(power(p), 0, "quiet again");
        // Further away gives a weaker signal.
        vibrate(&server, (507.5, -60.0, 500.5), None);
        assert!(power(p) < near && power(p) > 0);
        // Out of range: nothing.
        let q = (600, -60, 600);
        server.world.set(q.0, q.1, q.2, sensor);
        vibrate(&server, (620.5, -60.0, 600.5), None);
        assert_eq!(power(q), 0);
    }
}

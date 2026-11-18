//! Status effects (from PNX Effect and subclasses). Pure logic, the session applies
//! the results.

use std::collections::BTreeMap;
use parking_lot::Mutex;

pub const SPEED: i32 = 1;
pub const SLOWNESS: i32 = 2;
pub const STRENGTH: i32 = 5;
pub const INSTANT_HEALTH: i32 = 6;
pub const INSTANT_DAMAGE: i32 = 7;
pub const JUMP_BOOST: i32 = 8;
pub const NAUSEA: i32 = 9;
pub const REGENERATION: i32 = 10;
pub const RESISTANCE: i32 = 11;
pub const FIRE_RESISTANCE: i32 = 12;
pub const WATER_BREATHING: i32 = 13;
pub const INVISIBILITY: i32 = 14;
pub const NIGHT_VISION: i32 = 16;
pub const HUNGER: i32 = 17;
pub const WEAKNESS: i32 = 18;
pub const POISON: i32 = 19;
pub const WITHER: i32 = 20;
pub const ABSORPTION: i32 = 22;
pub const SATURATION: i32 = 23;
pub const FATAL_POISON: i32 = 25;
pub const SLOW_FALLING: i32 = 27;
pub const BAD_OMEN: i32 = 28;
pub const HERO_OF_THE_VILLAGE: i32 = 29;

/// Bedrock identifiers for /effect.
pub const NAMES: [(&str, i32); 30] = [
    ("speed", 1), ("slowness", 2), ("haste", 3), ("mining_fatigue", 4), ("strength", 5), ("instant_health", 6),
    ("instant_damage", 7), ("jump_boost", 8), ("nausea", 9), ("regeneration", 10), ("resistance", 11),
    ("fire_resistance", 12), ("water_breathing", 13), ("invisibility", 14), ("blindness", 15), ("night_vision", 16),
    ("hunger", 17), ("weakness", 18), ("poison", 19), ("wither", 20), ("health_boost", 21), ("absorption", 22),
    ("saturation", 23), ("levitation", 24), ("fatal_poison", 25), ("conduit_power", 26), ("slow_falling", 27),
    ("bad_omen", 28), ("village_hero", 29), ("darkness", 30),
];

pub fn by_name(n: &str) -> Option<i32> {
    let n = n.trim_start_matches("minecraft:");
    NAMES.iter().find(|(k, _)| *k == n).map(|&(_, id)| id).or_else(|| n.parse().ok().filter(|i| (1..=30).contains(i)))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Effect {
    pub amplifier: i32,
    pub ticks: i32,
    pub particles: bool,
}

/// Result of one second of effects.
#[derive(Debug, Default, PartialEq)]
pub struct Tick {
    pub heal: f32,
    /// poison (never kills, stops at 1) and wither
    pub poison: f32,
    pub wither: f32,
    pub exhaustion: f32,
    pub food: f32,
    /// expired, so the client can drop them
    pub expired: Vec<i32>,
}

#[derive(Debug, Default)]
pub struct Effects {
    /// golden apple hearts, eaten before health
    absorb: std::sync::atomic::AtomicU32,
    pub active: BTreeMap<i32, Effect>,
    pulse: BTreeMap<i32, i32>,
}

/// /effect commands waiting for the player's session, by entity id.
pub enum Queued {
    Give(i32, Effect),
    Clear,
}
pub static QUEUE: Mutex<Vec<(i64, Queued)>> = Mutex::new(Vec::new());

/// Instant effects apply once and never show up as active.
pub fn instant(id: i32) -> bool {
    matches!(id, INSTANT_HEALTH | INSTANT_DAMAGE | SATURATION)
}

impl Effects {
    pub fn absorb(&self) -> f32 {
        f32::from_bits(self.absorb.load(std::sync::atomic::Ordering::Relaxed))
    }

    pub fn set_absorb(&self, v: f32) {
        self.absorb.store(v.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }

    /// Stronger or longer replaces weaker (PNX Entity.addEffect). False if the old one stayed.
    pub fn add(&mut self, id: i32, e: Effect) -> bool {
        if let Some(old) = self.active.get(&id)
            && (old.amplifier > e.amplifier || old.amplifier == e.amplifier && old.ticks > e.ticks)
        {
            return false;
        }
        self.active.insert(id, e);
        true
    }

    pub fn level(&self, id: i32) -> Option<i32> {
        self.active.get(&id).map(|e| e.amplifier)
    }

    /// 20 ticks worth. Pulse rates from PNX Effect.canTick: regen 50 >> amp, poison 25 >> amp,
    /// wither 40 >> amp, hunger adds 0.005 * (amp + 1) exhaustion per tick.
    pub fn second(&mut self) -> Tick {
        let mut t = Tick::default();
        for (&id, e) in self.active.iter_mut() {
            let every = match id {
                REGENERATION => Some(50),
                POISON | FATAL_POISON => Some(25),
                WITHER => Some(40),
                _ => None,
            };
            if let Some(every) = every {
                let every = (every >> e.amplifier.clamp(0, 5)).max(1);
                let n = self.pulse.entry(id).or_default();
                *n += 20;
                let pulses = *n / every;
                *n %= every;
                match id {
                    REGENERATION => t.heal += pulses as f32,
                    POISON | FATAL_POISON => t.poison += pulses as f32,
                    _ => t.wither += pulses as f32,
                }
            }
            if id == HUNGER {
                t.exhaustion += 0.1 * (e.amplifier + 1) as f32;
            }
            if id == SATURATION {
                t.food += (e.amplifier + 1) as f32;
            }
            e.ticks -= 20;
            if e.ticks <= 0 {
                t.expired.push(id);
            }
        }
        for id in &t.expired {
            self.active.remove(id);
            self.pulse.remove(id);
        }
        t
    }

    /// strength +3 per level, weakness -4
    pub fn melee(&self, base: f32) -> f32 {
        let s = self.level(STRENGTH).map_or(0.0, |a| 3.0 * (a + 1) as f32);
        let w = self.level(WEAKNESS).map_or(0.0, |a| 4.0 * (a + 1) as f32);
        (base + s - w).max(0.0)
    }

    /// 20% less per resistance level
    pub fn resist(&self, d: f32) -> f32 {
        self.level(RESISTANCE).map_or(d, |a| d * (1.0 - 0.2 * (a + 1) as f32).max(0.0))
    }

    /// base 0.1, speed +20% and slowness -15% per level
    pub fn speed(&self) -> f32 {
        let up = self.level(SPEED).map_or(0.0, |a| 0.2 * (a + 1) as f32);
        let down = self.level(SLOWNESS).map_or(0.0, |a| 0.15 * (a + 1) as f32);
        (0.1 * (1.0 + up - down)).max(0.0)
    }
}

fn s(secs: i32) -> i32 {
    secs * 20
}

/// Potion effects by aux value (Bedrock potion ids).
pub fn potion(aux: u32) -> Vec<(i32, i32, i32)> {
    // (effect, amplifier, ticks), instants just get 1 tick
    match aux {
        5 => vec![(NIGHT_VISION, 0, s(180))],
        6 => vec![(NIGHT_VISION, 0, s(480))],
        7 => vec![(INVISIBILITY, 0, s(180))],
        8 => vec![(INVISIBILITY, 0, s(480))],
        9 => vec![(JUMP_BOOST, 0, s(180))],
        10 => vec![(JUMP_BOOST, 0, s(480))],
        11 => vec![(JUMP_BOOST, 1, s(90))],
        12 => vec![(FIRE_RESISTANCE, 0, s(180))],
        13 => vec![(FIRE_RESISTANCE, 0, s(480))],
        14 => vec![(SPEED, 0, s(180))],
        15 => vec![(SPEED, 0, s(480))],
        16 => vec![(SPEED, 1, s(90))],
        17 => vec![(SLOWNESS, 0, s(90))],
        18 => vec![(SLOWNESS, 0, s(240))],
        19 => vec![(WATER_BREATHING, 0, s(180))],
        20 => vec![(WATER_BREATHING, 0, s(480))],
        21 => vec![(INSTANT_HEALTH, 0, 1)],
        22 => vec![(INSTANT_HEALTH, 1, 1)],
        23 => vec![(INSTANT_DAMAGE, 0, 1)],
        24 => vec![(INSTANT_DAMAGE, 1, 1)],
        25 => vec![(POISON, 0, s(45))],
        26 => vec![(POISON, 0, s(120))],
        27 => vec![(POISON, 1, s(22))],
        28 => vec![(REGENERATION, 0, s(45))],
        29 => vec![(REGENERATION, 0, s(120))],
        30 => vec![(REGENERATION, 1, s(22))],
        31 => vec![(STRENGTH, 0, s(180))],
        32 => vec![(STRENGTH, 0, s(480))],
        33 => vec![(STRENGTH, 1, s(90))],
        34 => vec![(WEAKNESS, 0, s(90))],
        35 => vec![(WEAKNESS, 0, s(240))],
        36 => vec![(WITHER, 0, s(40))],
        37 => vec![(SLOWNESS, 3, s(20)), (RESISTANCE, 2, s(20))],
        38 => vec![(SLOWNESS, 3, s(40)), (RESISTANCE, 2, s(40))],
        39 => vec![(SLOWNESS, 5, s(20)), (RESISTANCE, 3, s(20))],
        40 => vec![(SLOW_FALLING, 0, s(90))],
        41 => vec![(SLOW_FALLING, 0, s(240))],
        42 => vec![(SLOWNESS, 3, s(20))],
        _ => vec![],
    }
}

/// Food effects and their chance in percent (PNX ItemFood subclasses).
pub fn food(name: &str) -> Vec<(i32, i32, i32, u32)> {
    match name.trim_start_matches("minecraft:") {
        "golden_apple" => vec![(REGENERATION, 1, s(5), 100), (ABSORPTION, 0, s(120), 100)],
        "enchanted_golden_apple" => vec![(REGENERATION, 1, s(30), 100), (ABSORPTION, 3, s(120), 100), (RESISTANCE, 0, s(300), 100), (FIRE_RESISTANCE, 0, s(300), 100)],
        "rotten_flesh" => vec![(HUNGER, 0, s(30), 80)],
        "chicken" => vec![(HUNGER, 0, s(30), 30)],
        "spider_eye" => vec![(POISON, 0, s(5), 100)],
        "poisonous_potato" => vec![(POISON, 0, s(5), 60)],
        "pufferfish" => vec![(HUNGER, 2, s(15), 100), (POISON, 3, s(60), 100), (NAUSEA, 0, s(15), 100)],
        _ => vec![],
    }
}

/// Args after the target: <effect> [seconds] [amplifier] [hideParticles].
pub fn parse(args: &[&str]) -> Option<(i32, Effect)> {
    let id = by_name(args.first()?)?;
    let secs: i32 = args.get(1).map_or(Some(30), |a| a.parse().ok())?;
    let amplifier: i32 = args.get(2).map_or(Some(0), |a| a.parse().ok())?;
    let hide = args.get(3).is_some_and(|a| *a == "true");
    Some((id, Effect { amplifier: amplifier.clamp(0, 255), ticks: if instant(id) { 1 } else { s(secs.clamp(0, 1_000_000)) }, particles: !hide }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regen_poison_expire_and_replace() {
        let mut e = Effects::default();
        e.add(REGENERATION, Effect { amplifier: 1, ticks: s(5), particles: true });
        let t = e.second();
        assert_eq!(t.heal, 0.0, "regen II pulses every 25 ticks");
        let t = e.second();
        assert_eq!(t.heal, 1.0);
        assert!(!e.add(REGENERATION, Effect { amplifier: 0, ticks: s(60), particles: true }), "weaker one doesn't replace");
        let mut gone = false;
        for _ in 0..5 {
            gone |= e.second().expired.contains(&REGENERATION);
        }
        assert!(gone && e.level(REGENERATION).is_none());

        e.add(STRENGTH, Effect { amplifier: 0, ticks: s(10), particles: true });
        assert_eq!(e.melee(1.0), 4.0);
        assert_eq!(potion(16), vec![(SPEED, 1, s(90))]);
        assert_eq!(parse(&["speed", "10", "1"]).map(|(i, x)| (i, x.amplifier, x.ticks)), Some((SPEED, 1, 200)));
        assert!(parse(&["nope"]).is_none());
    }
}

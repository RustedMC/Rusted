//! Trial spawners and vaults in trial chambers.
//!
//! A trial spawner wakes when a player comes within 14 blocks, spawns its mob a few at a time
//! (6 in all, +2 for each extra player; 2 at once, +1 each extra player), and once they're all
//! dead throws out a reward and a trial key for each player who fought, then rests for 30
//! minutes. A vault opens with a trial key, once per player, and throws out its loot.
//! (Vanilla TrialSpawner / VaultBlockEntity behaviour; PNX doesn't have these yet.)

use crate::mobs::Kind;
use crate::players::Server;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

const RANGE: f32 = 14.0;
const COOLDOWN: Duration = Duration::from_secs(30 * 60);
/// Seconds between spawns while active (vanilla 40 ticks).
const SPAWN_EVERY: Duration = Duration::from_secs(2);

/// trial_spawner_state block values.
const STATE_WAITING: i32 = 1;
const STATE_ACTIVE: i32 = 2;
const STATE_COOLDOWN: i32 = 5;

#[derive(Default)]
struct Trial {
    mob: &'static str,
    baby: bool,
    spawned: u32,
    total: u32,
    alive: Vec<i64>,
    fighters: HashSet<i64>,
    last_spawn: Option<Instant>,
    cooldown_until: Option<Instant>,
}

fn spawners() -> &'static Mutex<HashMap<(i32, i32, i32), Trial>> {
    static S: std::sync::OnceLock<Mutex<HashMap<(i32, i32, i32), Trial>>> = std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}

/// The mob a trial chamber spawner template spawns, from its template name, as (mob, baby).
/// "trial_chambers/spawner/ranged/poison_skeleton" -> ("bogged", false).
pub fn mob_for_template(name: &str) -> Option<(&'static str, bool)> {
    let rest = name.strip_prefix("trial_chambers/spawner/")?;
    let last = rest.rsplit('/').next()?;
    Some(match last {
        "skeleton" => ("skeleton", false),
        "stray" => ("stray", false),
        "poison_skeleton" => ("bogged", false),
        "zombie" => ("zombie", false),
        "husk" => ("husk", false),
        "spider" => ("spider", false),
        "slime" => ("slime", false),
        "cave_spider" => ("cave_spider", false),
        "baby_zombie" => ("zombie", true),
        "silverfish" => ("silverfish", false),
        "breeze" => ("breeze", false),
        _ => return None,
    })
}

/// How many trial spawners the world generator has placed so far.
#[cfg(test)]
pub fn registered() -> usize {
    spawners().lock().len()
}

/// The world generator placed a trial spawner for `mob` at `pos`.
pub fn generated(pos: (i32, i32, i32), mob: &'static str, baby: bool) {
    spawners().lock().entry(pos).or_insert_with(|| Trial { mob, baby, ..Default::default() });
}

fn set_state(server: &Server, pos: (i32, i32, i32), state: i32) {
    let b = server.world.get(pos.0, pos.1, pos.2);
    if crate::blocks::has_state(b, "trial_spawner_state") {
        let nb = crate::blocks::with(b, &[("trial_spawner_state", crate::blocks::State::Int(state))]);
        if nb != b {
            server.set_block_quiet(0, pos.0, pos.1, pos.2, nb);
        }
    }
}

/// Called with the mob tick: wakes, spawns and rewards trial spawners near players.
pub fn tick(server: &Server, mobs: &crate::mobs::Mobs, rand: impl Fn() -> u32) {
    let players: Vec<(i64, (f32, f32, f32))> = server.all().iter().map(|p| (p.id, (p.pos.0, p.pos.1 - 1.62, p.pos.2))).collect();
    if players.is_empty() {
        return;
    }
    let now = Instant::now();
    let mut changes = Vec::new();
    let mut to_spawn = Vec::new();
    let mut rewards = Vec::new();
    {
        let mut all = spawners().lock();
        all.retain(|pos, _| crate::overworld::feature::blocks::short(server.world.get(pos.0, pos.1, pos.2)) == "trial_spawner");
        for (&pos, t) in all.iter_mut() {
            let centre = (pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5);
            let near: Vec<i64> = players.iter().filter(|(_, p)| crate::mobs::dist2(*p, centre) < RANGE * RANGE).map(|(id, _)| *id).collect();
            if let Some(until) = t.cooldown_until {
                if now < until {
                    continue;
                }
                t.cooldown_until = None;
                changes.push((pos, STATE_WAITING));
            }
            t.alive.retain(|id| mobs.contains(*id));
            if t.total == 0 {
                // Waiting for players.
                if near.is_empty() {
                    continue;
                }
                let extra = near.len() as u32 - 1;
                t.total = 6 + 2 * extra;
                t.spawned = 0;
                t.fighters.clear();
                changes.push((pos, STATE_ACTIVE));
            }
            t.fighters.extend(near.iter().copied());
            let at_once = 2 + t.fighters.len() as u32 - 1;
            if t.spawned < t.total {
                let ready = t.last_spawn.is_none_or(|l| now.duration_since(l) >= SPAWN_EVERY);
                if ready && (t.alive.len() as u32) < at_once && !near.is_empty() {
                    t.last_spawn = Some(now);
                    to_spawn.push((pos, t.mob, t.baby));
                    t.spawned += 1;
                }
            } else if t.alive.is_empty() {
                // All beaten: a reward for everyone who fought, then rest.
                rewards.push((pos, t.fighters.len()));
                t.total = 0;
                t.cooldown_until = Some(now + COOLDOWN);
                changes.push((pos, STATE_COOLDOWN));
            }
        }
    }
    for (pos, state) in changes {
        set_state(server, pos, state);
    }
    for (pos, mob, baby) in to_spawn {
        let Some(kind) = Kind::from_name(mob) else { continue };
        // Somewhere free within 4 blocks, at the spawner's height or a little off.
        let spot = (0..12).find_map(|_| {
            let r = rand();
            let p = (pos.0 + (r % 9) as i32 - 4, pos.1 + ((r >> 4) % 3) as i32 - 1, pos.2 + ((r >> 8) % 9) as i32 - 4);
            crate::mobs::standable(&server.world, p).then_some((p.0 as f32 + 0.5, p.1 as f32, p.2 as f32 + 0.5))
        });
        if let Some(at) = spot {
            let id = mobs.spawn_baby(server, kind, at, baby);
            if id != 0 {
                crate::plugin::later(crate::plugin::Later::TrialSpawn(pos, kind.identifier(), id));
            }
            if id != 0
                && let Some(t) = spawners().lock().get_mut(&pos)
            {
                t.alive.push(id);
            }
            let _ = server.events.send(crate::players::Event::Relay { from: 0, packet: Box::new(crate::packets::play_sound("trial_spawner.spawn_mob", at)) });
        }
    }
    for (pos, fighters) in rewards {
        let at = (pos.0 as f32 + 0.5, pos.1 as f32 + 1.2, pos.2 as f32 + 0.5);
        for _ in 0..fighters.max(1) {
            for s in crate::loot::roll_named("trial_spawner").unwrap_or_default() {
                server.entities.drop_item(server, s.item, s.count, at);
            }
            if let Some(key) = crate::items::item_by_name("minecraft:trial_key") {
                server.entities.drop_item(server, key, 1, at);
            }
        }
        let _ = server.events.send(crate::players::Event::Relay { from: 0, packet: Box::new(crate::packets::play_sound("trial_spawner.eject_item", at)) });
    }
}

// ---------------------------------------------------------------------------------------------
// Vaults

fn vault_file() -> std::path::PathBuf {
    std::path::Path::new("worlds").join(&crate::config::get().level_name).join("vaults.txt")
}

/// Who has opened which vault: (x, y, z, lowercase name).
fn opened() -> &'static Mutex<HashSet<(i32, i32, i32, String)>> {
    static O: std::sync::OnceLock<Mutex<HashSet<(i32, i32, i32, String)>>> = std::sync::OnceLock::new();
    O.get_or_init(|| {
        let text = std::fs::read_to_string(vault_file()).unwrap_or_default();
        Mutex::new(
            text.lines()
                .filter_map(|l| {
                    let p: Vec<&str> = l.split_whitespace().collect();
                    Some((p.first()?.parse().ok()?, p.get(1)?.parse().ok()?, p.get(2)?.parse().ok()?, p.get(3)?.to_string()))
                })
                .collect(),
        )
    })
}

pub fn is_vault(block: i32) -> bool {
    crate::overworld::feature::blocks::short(block) == "vault"
}

/// A player used `held` on the vault at `pos`. Some(true) if it opened (the key is used up),
/// Some(false) if this player already opened it, None if they aren't holding a trial key.
pub fn open_vault(server: &Server, pos: (i32, i32, i32), player: &str, held: &str) -> Option<bool> {
    let ominous = match held {
        "minecraft:trial_key" => false,
        "minecraft:ominous_trial_key" => true,
        _ => return None,
    };
    let key = (pos.0, pos.1, pos.2, player.to_lowercase());
    {
        let mut o = opened().lock();
        if !o.insert(key) {
            return Some(false);
        }
        let text: String = o.iter().map(|(x, y, z, n)| format!("{x} {y} {z} {n}\n")).collect();
        if let Err(e) = std::fs::write(vault_file(), text) {
            println!("failed to save vaults: {e}");
        }
    }
    let at = (pos.0 as f32 + 0.5, pos.1 as f32 + 1.2, pos.2 as f32 + 0.5);
    for s in crate::loot::roll_named(if ominous { "ominous_vault" } else { "vault" }).unwrap_or_default() {
        server.entities.drop_item(server, s.item, s.count, at);
    }
    let _ = server.events.send(crate::players::Event::Relay { from: 0, packet: Box::new(crate::packets::play_sound("vault.open_shutter", at)) });
    Some(true)
}

#[cfg(test)]
mod tests {
    #[test]
    fn template_names_map_to_mobs() {
        assert_eq!(super::mob_for_template("trial_chambers/spawner/ranged/poison_skeleton"), Some(("bogged", false)));
        assert_eq!(super::mob_for_template("trial_chambers/spawner/small_melee/baby_zombie"), Some(("zombie", true)));
        assert_eq!(super::mob_for_template("trial_chambers/spawner/breeze/breeze"), Some(("breeze", false)));
        assert_eq!(super::mob_for_template("trial_chambers/chamber/end"), None);
        for (mob, _) in ["skeleton", "stray", "poison_skeleton", "zombie", "husk", "spider", "slime", "cave_spider", "baby_zombie", "silverfish", "breeze"]
            .iter()
            .filter_map(|n| super::mob_for_template(&format!("trial_chambers/spawner/x/{n}")))
        {
            assert!(crate::mobs::Kind::from_name(mob).is_some(), "{mob} is a known mob");
        }
    }
}


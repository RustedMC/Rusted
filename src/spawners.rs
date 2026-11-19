//! Smart spawners (Donut SMP style) and mineable spawners.
//!
//! - Spawner items (`/spawner give <player> <mob> [n]` for ops) place as smart spawners: no
//!   mobs, they fill up with that mob's loot and XP every minute while a player is within 16
//!   blocks. Right-click opens a menu: collect loot, collect XP, sell everything.
//! - Click a spawner holding another of the same mob to stack it (more stacks, more loot).
//! - Breaking one drops its spawner items. Breaking a dungeon spawner with silk touch drops a
//!   spawner item of its mob (`mine.silk-touch-only = false` lets any pickaxe do it).
//!
//! Stored in plugins/spawners/spawners.txt. Sell prices: `sell.<item> = <coins each>`.

use crate::players::{PlayerInfo, Server};
use crate::plugin::{self, Config, Plugin};
use crate::ui::{self, Form, Response};
use parking_lot::Mutex;

const NAME: &str = "spawners";
type Pos = (i32, i32, i32);

static LOCK: Mutex<()> = Mutex::new(());

/// (item, min, max) dropped per stack per minute.
fn loot(mob: &str) -> &'static [(&'static str, u32, u32)] {
    match mob {
        "zombie" | "husk" | "drowned" => &[("minecraft:rotten_flesh", 1, 3)],
        "skeleton" | "stray" => &[("minecraft:bone", 1, 3), ("minecraft:arrow", 0, 2)],
        "spider" | "cave_spider" => &[("minecraft:string", 1, 3), ("minecraft:spider_eye", 0, 1)],
        "creeper" => &[("minecraft:gunpowder", 1, 3)],
        "blaze" => &[("minecraft:blaze_rod", 0, 2)],
        "cow" | "mooshroom" => &[("minecraft:beef", 1, 3), ("minecraft:leather", 0, 2)],
        "pig" => &[("minecraft:porkchop", 1, 3)],
        "chicken" => &[("minecraft:chicken", 1, 1), ("minecraft:feather", 0, 2)],
        "sheep" => &[("minecraft:mutton", 1, 2), ("minecraft:white_wool", 1, 1)],
        "iron_golem" => &[("minecraft:iron_ingot", 3, 5), ("minecraft:poppy", 0, 2)],
        "enderman" => &[("minecraft:ender_pearl", 0, 1)],
        "witch" => &[("minecraft:redstone", 0, 2), ("minecraft:glowstone_dust", 0, 2), ("minecraft:sugar", 0, 2)],
        "slime" => &[("minecraft:slime_ball", 0, 2)],
        "magma_cube" => &[("minecraft:magma_cream", 0, 1)],
        "rabbit" => &[("minecraft:rabbit", 0, 1), ("minecraft:rabbit_hide", 0, 1)],
        _ => &[],
    }
}

fn xp_per_minute(mob: &str) -> u32 {
    match mob {
        "blaze" | "iron_golem" => 10,
        "cow" | "pig" | "chicken" | "sheep" | "rabbit" | "mooshroom" => 3,
        _ => 5,
    }
}

fn default_price(item: &str) -> i64 {
    match item {
        "minecraft:iron_ingot" => 15,
        "minecraft:blaze_rod" | "minecraft:ender_pearl" => 12,
        "minecraft:gunpowder" | "minecraft:slime_ball" | "minecraft:magma_cream" | "minecraft:leather" => 6,
        _ => 3,
    }
}

#[derive(Clone, Default)]
struct Spawner {
    mob: String,
    stack: u32,
    xp: u32,
    /// item -> count
    loot: Vec<(String, u32)>,
}

fn key(p: Pos) -> String {
    format!("{},{},{}", p.0, p.1, p.2)
}

fn parse_pos(s: &str) -> Option<Pos> {
    let n: Vec<i32> = s.split(',').filter_map(|x| x.parse().ok()).collect();
    (n.len() == 3).then(|| (n[0], n[1], n[2]))
}

/// "zombie|3|120|minecraft:rotten_flesh=40;minecraft:..."
fn decode(v: &str) -> Option<Spawner> {
    let f: Vec<&str> = v.split('|').collect();
    Some(Spawner {
        mob: f.first()?.to_string(),
        stack: f.get(1)?.parse().ok()?,
        xp: f.get(2)?.parse().ok()?,
        loot: f.get(3).unwrap_or(&"").split(';').filter_map(|e| e.split_once('=').and_then(|(i, n)| Some((i.to_string(), n.parse().ok()?)))).collect(),
    })
}

fn encode(s: &Spawner) -> String {
    let loot: Vec<String> = s.loot.iter().filter(|(_, n)| *n > 0).map(|(i, n)| format!("{i}={n}")).collect();
    format!("{}|{}|{}|{}", s.mob, s.stack, s.xp, loot.join(";"))
}

fn book() -> Config {
    Config::data(NAME, "spawners")
}

fn get(pos: Pos) -> Option<Spawner> {
    book().get(&key(pos)).and_then(decode)
}

fn put(pos: Pos, s: Option<&Spawner>) {
    let mut b = book();
    match s {
        Some(s) => b.set(&key(pos), &encode(s)),
        None => b.remove(&key(pos)),
    }
    let _ = b.save();
}

fn pretty(mob: &str) -> String {
    crate::market::pretty(mob)
}

pub fn spawner_item(mob: &str, n: i8) -> Option<crate::inventory::Stack> {
    let mut st = plugin::item("minecraft:mob_spawner")?.name(&format!("§e{} Spawner", pretty(mob))).lore(&["§7Place it, then right-click it", "§7for loot and XP."]).tag(NAME, mob).build();
    st.count = n.max(1);
    Some(st)
}

/// One minute of output for `stack` spawners.
fn produce(s: &mut Spawner, seed: &mut u64) {
    let cap = 2304 * s.stack;
    for (item, lo, hi) in loot(&s.mob) {
        let mut add = 0;
        for _ in 0..s.stack {
            *seed ^= *seed << 13;
            *seed ^= *seed >> 7;
            *seed ^= *seed << 17;
            add += lo + (*seed % (*hi - *lo + 1) as u64) as u32;
        }
        match s.loot.iter_mut().find(|(i, _)| i == item) {
            Some(e) => e.1 = (e.1 + add).min(cap),
            None => s.loot.push((item.to_string(), add.min(cap))),
        }
    }
    s.xp = (s.xp + xp_per_minute(&s.mob) * s.stack).min(100_000);
}

fn tick(server: &Server) {
    let _g = LOCK.lock();
    let mut b = book();
    let players: Vec<(f32, f32, f32)> = server.all().iter().map(|p| p.pos).collect();
    let mut seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64) | 1;
    let all: Vec<(Pos, Spawner)> = b.entries().filter_map(|(k, v)| Some((parse_pos(k)?, decode(v)?))).collect();
    for (pos, mut s) in all {
        let near = players.iter().any(|p| {
            let (dx, dy, dz) = (p.0 - pos.0 as f32, p.1 - pos.1 as f32, p.2 - pos.2 as f32);
            dx * dx + dy * dy + dz * dz < 16.0 * 16.0
        });
        if near {
            produce(&mut s, &mut seed);
            b.set(&key(pos), &encode(&s));
        }
    }
    let _ = b.save();
}

fn price(item: &str) -> i64 {
    let mut c = Config::load(NAME);
    let v = c.get_or(&format!("sell.{}", item.trim_start_matches("minecraft:")), &default_price(item).to_string()).parse().unwrap_or(0);
    let _ = c.save();
    v
}

fn open_menu(server: &Server, p: &PlayerInfo, pos: Pos) {
    let Some(s) = get(pos) else { return };
    let worth: i64 = s.loot.iter().map(|(i, n)| price(i) * *n as i64).sum();
    let mut text = format!("§7Stack: §f{}x\n§7XP stored: §a{}\n§7Loot worth: §6{} coins\n", s.stack, s.xp, crate::economy::format(worth));
    for (i, n) in s.loot.iter().filter(|(_, n)| *n > 0) {
        text.push_str(&format!("\n§8- §f{n}x {}", crate::market::pretty(i)));
    }
    let form = Form::Simple {
        title: format!("rusted:{} Spawner", pretty(&s.mob)),
        content: text,
        buttons: vec![
            ("§fCollect loot\n§7Into your inventory".into(), Some("textures/blocks/chest_front".into())),
            ("§fCollect XP\n§7Level up".into(), Some("textures/items/experience_bottle".into())),
            ("§fSell all loot\n§6+{worth} coins".replace("{worth}", &crate::economy::format(worth)), Some("textures/items/gold_ingot".into())),
        ],
    };
    ui::show(server, p.id, form, move |server, p, answer| {
        let Response::Button(i) = answer else { return };
        let _g = LOCK.lock();
        let Some(mut s) = get(pos) else { return };
        match i {
            0 => {
                for (item, n) in s.loot.drain(..) {
                    if let Some(b) = plugin::item(&item) {
                        let st = b.build();
                        let mut left = n;
                        while left > 0 {
                            let mut c = st.clone();
                            c.count = left.min(64) as i8;
                            left -= c.count as u32;
                            server.give(p.id, c);
                        }
                    }
                }
                server.tell(p.id, "§aCollected the loot.");
            }
            1 => {
                let _ = server.events.send(crate::players::Event::Xp { to: p.id, points: s.xp.min(i32::MAX as u32) as i32 });
                server.tell(p.id, &format!("§a+{} XP", s.xp));
                s.xp = 0;
            }
            2 => {
                let worth: i64 = s.loot.iter().map(|(i, n)| price(i) * *n as i64).sum();
                s.loot.clear();
                if worth > 0 {
                    let _ = crate::economy::deposit(&p.name, worth);
                }
                server.sound(p.id, "random.orb");
                server.tell(p.id, &format!("§aSold the loot for §6{} coins§a.", crate::economy::format(worth)));
            }
            _ => return,
        }
        put(pos, Some(&s));
    });
}

fn held_tag(p: &PlayerInfo) -> Option<String> {
    crate::players::equipment(p.id).held.as_ref().and_then(|h| plugin::tag(h, NAME))
}

pub struct Spawners;

impl Plugin for Spawners {
    fn name(&self) -> &str {
        NAME
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![("spawner", "Gives spawner items (ops)")]
    }

    fn on_server_start(&self, server: &Server) {
        server.scheduler.run_repeating(1200, 1200, |server| {
            tick(server);
            true
        });
    }

    fn on_block_place(&self, server: &Server, player: &PlayerInfo, pos: Pos, block: i32) -> bool {
        if crate::blocks::entry(block).is_none_or(|e| e.name != "minecraft:mob_spawner") {
            return true;
        }
        if let Some(mob) = held_tag(player) {
            let _g = LOCK.lock();
            put(pos, Some(&Spawner { mob: mob.clone(), stack: 1, ..Default::default() }));
            server.tell(player.id, &format!("§aPlaced a {} spawner. Right-click it for loot.", pretty(&mob)));
        }
        true
    }

    fn on_player_interact(&self, server: &Server, player: &PlayerInfo, pos: Pos, _item: &str) -> bool {
        let Some(s) = get(pos) else { return true };
        // Holding the same spawner: stack it.
        if held_tag(player).as_deref() == Some(s.mob.as_str()) {
            let name = player.name.clone();
            server.take_held(player.id, 1, move |server, got| {
                if got.is_none() {
                    return;
                }
                let _g = LOCK.lock();
                if let Some(mut s) = get(pos) {
                    s.stack += 1;
                    put(pos, Some(&s));
                    if let Some(id) = server.find_by_name(&name) {
                        server.tell(id, &format!("§aStacked: §f{}x §a{} spawner.", s.stack, pretty(&s.mob)));
                    }
                }
            });
            return false;
        }
        open_menu(server, player, pos);
        false
    }

    fn on_block_break(&self, server: &Server, player: &PlayerInfo, pos: Pos, block: i32) -> bool {
        if crate::blocks::entry(block).is_none_or(|e| e.name != "minecraft:mob_spawner") {
            return true;
        }
        let drop_at = (pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5);
        if let Some(s) = get(pos) {
            let _g = LOCK.lock();
            put(pos, None);
            // Loot and XP inside go to the breaker, then the spawners drop.
            if let Some(st) = spawner_item(&s.mob, s.stack.min(64) as i8) {
                server.entities.drop_item(server, st.item, st.count, drop_at);
            }
            server.tell(player.id, "§7Spawner picked up. Collect its loot first next time!");
            return true;
        }
        // A dungeon spawner: mineable with silk touch.
        let mut c = Config::load(NAME);
        let silk_only = c.get_or("mine.silk-touch-only", "true") == "true";
        let _ = c.save();
        let held = crate::players::equipment(player.id).held;
        let silk = held.as_ref().is_some_and(|h| crate::enchant::level(&h.item.user_data_buffer, crate::enchant::SILK_TOUCH) > 0);
        let mob = crate::loot::spawners().lock().get(&pos).copied();
        if let Some(mob) = mob
            && (silk || !silk_only)
            && let Some(st) = spawner_item(mob, 1)
        {
            crate::loot::spawners().lock().remove(&pos);
            server.entities.drop_item(server, st.item, 1, drop_at);
        }
        true
    }

    fn usage(&self, command: &str) -> &'static [&'static str] {
        match command {
            "spawner" => &["give <player> zombie|skeleton|spider|cave_spider|creeper|blaze|cow|pig|chicken|sheep|iron_golem|enderman|witch|slime|magma_cube|rabbit [amount]"],
            _ => &[],
        }
    }

    fn on_command(&self, server: &Server, player: &PlayerInfo, _name: &str, args: &[&str]) -> Option<String> {
        if !crate::commands::is_op(&player.name) {
            return Some("§cYou do not have permission to use this command".into());
        }
        Some(match args {
            ["give", who, mob, rest @ ..] => {
                let Some(target) = server.all().into_iter().find(|o| o.name.eq_ignore_ascii_case(who)) else { return Some(format!("§c{who} isn't online.")) };
                let mob = mob.trim_start_matches("minecraft:").to_lowercase();
                if loot(&mob).is_empty() {
                    return Some("§cSpawners: zombie, skeleton, spider, cave_spider, creeper, blaze, cow, pig, chicken, sheep, iron_golem, enderman, witch, slime, magma_cube, rabbit".into());
                }
                let n: i8 = rest.first().and_then(|n| n.parse().ok()).unwrap_or(1).clamp(1, 64);
                match spawner_item(&mob, n) {
                    Some(st) => {
                        server.give(target.id, st);
                        format!("§aGave {} {n} {} spawner(s).", target.name, pretty(&mob))
                    }
                    None => "§cCouldn't make that item.".into(),
                }
            }
            _ => "§6/spawner give §f<player> <mob> [amount]".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_round_trip_and_production() {
        let mut s = Spawner { mob: "zombie".into(), stack: 3, xp: 0, loot: vec![] };
        let mut seed = 12345u64;
        produce(&mut s, &mut seed);
        let flesh = s.loot.iter().find(|(i, _)| i == "minecraft:rotten_flesh").unwrap().1;
        assert!((3..=9).contains(&flesh), "1-3 per stack, 3 stacks: {flesh}");
        assert_eq!(s.xp, 15);
        let back = decode(&encode(&s)).unwrap();
        assert_eq!((back.mob.as_str(), back.stack, back.xp, back.loot.len()), ("zombie", 3, 15, 1));
    }
}

//! Using blocks that don't open a menu (sounds and animations included). Ported from the
//! onActivate/onInteract methods in PNX's block classes. Returns an [`Outcome`] for the session.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::blocks::{State, any_state, entry, state, with};
use crate::physics::Change;
use crate::world::{AIR, World};

pub struct Ctx<'a> {
    pub held: &'a str,
    pub sneaking: bool,
    pub survival: bool,
    pub hungry: bool,
    /// 0 down, 1 up, 2 north, 3 south, 4 west, 5 east
    pub face: u8,
    pub roll: u32,
    pub overworld: bool,
}

#[derive(Debug, PartialEq)]
pub enum Held {
    Keep,
    /// one used up (survival only)
    UseOne,
    /// stack turns into this, e.g. buckets (survival only)
    Become(&'static str),
    /// one used up and this given back, e.g. bottles (survival only)
    Trade(&'static str),
}

#[derive(Debug, PartialEq)]
pub struct Outcome {
    pub changes: Vec<Change>,
    /// PlaySoundPacket name and pitch
    pub sound: Option<(String, f32)>,
    /// stop this sound for everyone (jukebox record)
    pub stop_sound: Option<String>,
    pub drops: Vec<(&'static str, i8)>,
    pub held: Held,
    pub eat: Option<(f32, f32)>,
    /// block event (type, value), e.g. note particle or bell swing
    pub block_event: Option<(i32, i32)>,
    /// explodes with this power (respawn anchor outside the Nether)
    pub explode: Option<f32>,
}

impl Outcome {
    fn new(changes: Vec<Change>) -> Self {
        Outcome { changes, sound: None, stop_sound: None, drops: vec![], held: Held::Keep, eat: None, block_event: None, explode: None }
    }
    fn sound(mut self, s: &str) -> Self {
        self.sound = Some((s.into(), 1.0));
        self
    }
    fn held(mut self, h: Held) -> Self {
        self.held = h;
        self
    }
    fn drop(mut self, item: &'static str, n: i8) -> Self {
        self.drops.push((item, n));
        self
    }
}

/// Note block pitches (0-24) and jukebox discs by position.
/// TODO: save these once block entities are in.
static NOTES: Mutex<Option<HashMap<(i32, i32, i32), i32>>> = Mutex::new(None);
static DISCS: Mutex<Option<HashMap<(i32, i32, i32), &'static str>>> = Mutex::new(None);

/// Registry's own copy of the name so we can hold onto it.
fn static_name(item: &str) -> Option<&'static str> {
    crate::items::item_by_name(item).and_then(|i| crate::items::name_of(i.id))
}

fn name(b: i32) -> &'static str {
    entry(b).map_or("", |e| e.name.as_str())
}

fn int(b: i32, key: &str) -> i32 {
    match state(b, key) {
        Some(State::Int(i)) => *i,
        Some(State::Bool(v)) => *v as i32,
        _ => 0,
    }
}

fn set(b: i32, key: &str, v: State) -> i32 {
    with(b, &[(key, v)])
}

/// Block `new` with whatever states it shares with `b`.
fn rename(b: i32, new: &str) -> Option<i32> {
    let base = any_state(new)?;
    let st: Vec<(&str, State)> = entry(b).map(|e| e.states.iter().map(|(k, v)| (k.as_str(), v.clone())).collect()).unwrap_or_default();
    Some(with(base, &st))
}

/// Instrument from the block below (PNX BlockNoteblock.getInstrument).
fn instrument(below: &str) -> (&'static str, i32) {
    let n = below.trim_start_matches("minecraft:");
    match n {
        n if n.ends_with("_planks") || n.ends_with("_log") || n.ends_with("_wood") || n.contains("stem") || n == "bookshelf" || n == "noteblock" || n == "jukebox" || n.ends_with("_fence") => ("note.bass", 4),
        "sand" | "red_sand" | "gravel" | "concrete_powder" => ("note.snare", 2),
        n if n.ends_with("concrete_powder") => ("note.snare", 2),
        n if n.ends_with("glass") || n.ends_with("glass_pane") || n == "sea_lantern" || n == "beacon" => ("note.hat", 3),
        n if n.contains("stone") || n.contains("deepslate") || n.contains("brick") || n == "netherrack" || n.ends_with("_ore") || n == "obsidian" || n.ends_with("concrete") || n.ends_with("terracotta") => ("note.bd", 1),
        "gold_block" => ("note.bell", 6),
        "clay" => ("note.flute", 5),
        "packed_ice" => ("note.chime", 8),
        n if n.ends_with("_wool") => ("note.guitar", 7),
        "bone_block" => ("note.xylophone", 9),
        "iron_block" => ("note.iron_xylophone", 10),
        "soul_sand" => ("note.cow_bell", 11),
        "pumpkin" => ("note.didgeridoo", 12),
        "emerald_block" => ("note.bit", 13),
        "hay_block" => ("note.banjo", 14),
        "glowstone" => ("note.pling", 15),
        _ => ("note.harp", 0),
    }
}

fn record(item: &str) -> Option<String> {
    item.strip_prefix("minecraft:music_disc_").map(|d| format!("record.{d}"))
}

/// Undo one copper oxidation stage (axe scrape). None for plain copper.
fn scraped(n: &str) -> Option<String> {
    let n = n.trim_start_matches("minecraft:");
    let back = if let Some(r) = n.strip_prefix("oxidized_") {
        format!("weathered_{r}")
    } else if let Some(r) = n.strip_prefix("weathered_") {
        format!("exposed_{r}")
    } else {
        let r = n.strip_prefix("exposed_")?;
        if r == "copper" { "copper_block".into() } else { r.to_string() }
    };
    Some(format!("minecraft:{back}"))
}

fn is_copper(n: &str) -> bool {
    n.contains("copper") && !n.ends_with("_ore") && n != "minecraft:raw_copper_block"
}

pub fn use_block(world: &World, p: (i32, i32, i32), c: &Ctx) -> Option<Outcome> {
    let b = world.get(p.0, p.1, p.2);
    let n = name(b);
    let short = n.trim_start_matches("minecraft:");
    let held = c.held;
    let tool = |suffix: &str| held.ends_with(suffix);

    // item-on-block uses come first, they work while sneaking like vanilla
    if held == "minecraft:bone_meal" {
        if short == "sweet_berry_bush" && int(b, "growth") >= 3 {
            // fully grown, so fall through to picking
        } else if let Some(ch) = crate::growth::bone_meal(world, p, c.roll) {
            return Some(Outcome::new(ch).sound("item.bone_meal.use").held(Held::UseOne));
        }
    }
    if tool("_axe") {
        let stripped = if short.ends_with("_log") || short.ends_with("_wood") || short.ends_with("_stem") || short.ends_with("_hyphae") || short == "bamboo_block" {
            (!short.starts_with("stripped_")).then(|| format!("minecraft:stripped_{short}"))
        } else {
            None
        };
        if let Some(s) = stripped.and_then(|s| rename(b, &s)) {
            return Some(Outcome::new(vec![(p, s)]).sound("use.wood"));
        }
        if is_copper(n) {
            if let Some(r) = short.strip_prefix("waxed_") {
                let r = if r == "copper" { "copper_block" } else { r };
                if let Some(nb) = rename(b, &format!("minecraft:{r}")) {
                    return Some(Outcome::new(vec![(p, nb)]).sound("copper.wax.off"));
                }
            }
            if let Some(nb) = scraped(n).and_then(|s| rename(b, &s)) {
                return Some(Outcome::new(vec![(p, nb)]).sound("scrape"));
            }
        }
    }
    if held == "minecraft:honeycomb" && is_copper(n) && !short.starts_with("waxed_") {
        let base = if short == "copper_block" { "copper" } else { short };
        if let Some(nb) = rename(b, &format!("minecraft:waxed_{base}")) {
            return Some(Outcome::new(vec![(p, nb)]).sound("copper.wax.on").held(Held::UseOne));
        }
    }
    let air_above = world.get(p.0, p.1 + 1, p.2) == AIR;
    if tool("_shovel") {
        if matches!(short, "grass_block" | "dirt" | "podzol" | "mycelium" | "rooted_dirt") && air_above && c.face != 0
            && let Some(path) = any_state("minecraft:grass_path").or_else(|| any_state("minecraft:dirt_path")) {
                return Some(Outcome::new(vec![(p, path)]).sound("use.grass"));
            }
        if short.ends_with("campfire") && int(b, "extinguished") == 0 {
            return Some(Outcome::new(vec![(p, set(b, "extinguished", State::Bool(true)))]).sound("random.fizz"));
        }
    }
    if tool("_hoe") && air_above && c.face != 0 {
        let to = match short {
            "grass_block" | "dirt" | "dirt_path" | "grass_path" if int(b, "dirt_type") == 0 || short != "dirt" => Some("minecraft:farmland"),
            "dirt" => Some("minecraft:dirt"), // coarse dirt becomes dirt
            "rooted_dirt" => Some("minecraft:dirt"),
            _ => None,
        };
        if let Some(to) = to.and_then(any_state) {
            let to = if name(to) == "minecraft:dirt" { set(to, "dirt_type", State::Str("normal".into())) } else { to };
            let mut o = Outcome::new(vec![(p, to)]).sound("use.gravel");
            if short == "rooted_dirt" {
                o = o.drop("minecraft:hanging_roots", 1);
            }
            return Some(o);
        }
    }
    if held == "minecraft:shears" {
        if short == "pumpkin" {
            let dir = match c.face {
                2 => "north",
                3 => "south",
                4 => "west",
                5 => "east",
                _ => "south",
            };
            if let Some(cp) = any_state("minecraft:carved_pumpkin") {
                let cp = set(cp, "minecraft:cardinal_direction", State::Str(dir.into()));
                return Some(Outcome::new(vec![(p, cp)]).sound("block.pumpkin.carve").drop("minecraft:pumpkin_seeds", 4));
            }
        }
        if matches!(short, "beehive" | "bee_nest") && int(b, "honey_level") >= 5 {
            return Some(Outcome::new(vec![(p, set(b, "honey_level", State::Int(0)))]).sound("block.beehive.shear").drop("minecraft:honeycomb", 3));
        }
    }
    if held == "minecraft:glass_bottle" {
        if matches!(short, "beehive" | "bee_nest") && int(b, "honey_level") >= 5 {
            return Some(Outcome::new(vec![(p, set(b, "honey_level", State::Int(0)))]).sound("bottle.fill").held(Held::Trade("minecraft:honey_bottle")));
        }
        if short == "cauldron" && int(b, "fill_level") >= 2 && matches!(state(b, "cauldron_liquid"), Some(State::Str(s)) if s == "water") {
            let lvl = int(b, "fill_level") - 2;
            return Some(Outcome::new(vec![(p, set(b, "fill_level", State::Int(lvl)))]).sound("bottle.fill").held(Held::Trade("minecraft:potion")));
        }
    }
    if held == "minecraft:flint_and_steel" || held == "minecraft:fire_charge" {
        let lights = held == "minecraft:flint_and_steel";
        if (short.ends_with("candle") || short.ends_with("candle_cake")) && int(b, "lit") == 0 {
            return Some(Outcome::new(vec![(p, set(b, "lit", State::Bool(true)))]).sound("fire.ignite").held(if lights { Held::Keep } else { Held::UseOne }));
        }
        if short.ends_with("campfire") && int(b, "extinguished") == 1 {
            return Some(Outcome::new(vec![(p, set(b, "extinguished", State::Bool(false)))]).sound("fire.ignite").held(if lights { Held::Keep } else { Held::UseOne }));
        }
    }
    if let Some((next, bucket)) = crate::physics::cauldron(b, held) {
        let s = match (held, bucket) {
            ("minecraft:lava_bucket", _) => "bucket.empty_lava",
            (_, "minecraft:lava_bucket") => "bucket.fill_lava",
            (_, "minecraft:bucket") => "bucket.empty_water",
            _ => "bucket.fill_water",
        };
        return Some(Outcome::new(vec![(p, next)]).sound(s).held(Held::Become(bucket)));
    }
    if short == "end_portal_frame" && held == "minecraft:ender_eye" && int(b, "end_portal_eye_bit") == 0 {
        return Some(Outcome::new(vec![(p, set(b, "end_portal_eye_bit", State::Bool(true)))]).sound("block.end_portal_frame.fill").held(Held::UseOne));
    }
    if short == "respawn_anchor" {
        let charge = int(b, "respawn_anchor_charge");
        if held == "minecraft:glowstone" && charge < 4 {
            return Some(Outcome::new(vec![(p, set(b, "respawn_anchor_charge", State::Int(charge + 1)))]).sound("respawn_anchor.charge").held(Held::UseOne));
        }
        if charge > 0 && c.overworld && !c.sneaking {
            // charged anchor outside the Nether blows up (PNX power 5)
            let mut o = Outcome::new(vec![(p, AIR)]);
            o.explode = Some(5.0);
            return Some(o);
        }
    }
    if let Some((next, used, out)) = crate::physics::compost(b, held, c.roll % 100) {
        if c.sneaking && !out && used {
            return None;
        }
        let level = int(b, "composter_fill_level");
        let s = if out {
            "block.composter.empty"
        } else if int(next, "composter_fill_level") > level {
            if int(next, "composter_fill_level") >= 7 { "block.composter.ready" } else { "block.composter.fill_success" }
        } else {
            "block.composter.fill"
        };
        let mut o = Outcome::new(vec![(p, next)]).sound(s).held(if used { Held::UseOne } else { Held::Keep });
        if out {
            o = o.drop("minecraft:bone_meal", 1);
        }
        return Some(o);
    }
    if short == "jukebox" {
        let mut discs = DISCS.lock().unwrap();
        let discs = discs.get_or_insert_with(HashMap::new);
        if let Some(disc) = discs.remove(&p) {
            let mut o = Outcome::new(vec![]).drop(disc, 1);
            o.stop_sound = record(disc);
            return Some(o);
        }
        if let Some(rec) = record(held) {
            let item = static_name(held)?;
            discs.insert(p, item);
            return Some(Outcome { sound: Some((rec, 1.0)), ..Outcome::new(vec![]) }.held(Held::UseOne));
        }
        return None;
    }

    // sneaking places against the block instead
    if c.sneaking {
        return None;
    }
    match short {
        "noteblock" => {
            let mut notes = NOTES.lock().unwrap();
            let pitch = notes.get_or_insert_with(HashMap::new).entry(p).or_insert(-1);
            *pitch = (*pitch + 1) % 25;
            let (sound, inst) = instrument(name(world.get(p.0, p.1 - 1, p.2)));
            let mut o = Outcome::new(vec![]);
            // PNX pitch = 2^((note - 12) / 12)
            o.sound = Some((sound.into(), 2f32.powf((*pitch - 12) as f32 / 12.0)));
            o.block_event = Some((inst, *pitch));
            Some(o)
        }
        "bell" => {
            let mut o = Outcome::new(vec![]).sound("block.bell.hit");
            o.block_event = Some((1, c.face as i32));
            Some(o)
        }
        "cake" => {
            if c.hungry || !c.survival {
                let next = crate::physics::cake_bite(b)?;
                Some(Outcome { eat: Some((2.0, 0.4)), ..Outcome::new(vec![(p, next)]) }.sound("random.burp"))
            } else {
                Some(Outcome::new(vec![]))
            }
        }
        n if (n.ends_with("candle") || n.ends_with("candle_cake")) && int(b, "lit") == 1 => {
            Some(Outcome::new(vec![(p, set(b, "lit", State::Bool(false)))]).sound("random.fizz"))
        }
        n if n.ends_with("candle_cake") => {
            // candle cake: drop the candle and take a bite
            if !(c.hungry || !c.survival) {
                return Some(Outcome::new(vec![]));
            }
            let cake = any_state("minecraft:cake").map(|k| set(k, "bite_counter", State::Int(1)))?;
            let candle: &'static str = static_name(&format!("minecraft:{}", n.trim_end_matches("_cake")))?;
            Some(Outcome { eat: Some((2.0, 0.4)), ..Outcome::new(vec![(p, cake)]) }.sound("random.burp").drop(candle, 1))
        }
        "daylight_detector" | "daylight_detector_inverted" => {
            let other = if short == "daylight_detector" { "minecraft:daylight_detector_inverted" } else { "minecraft:daylight_detector" };
            Some(Outcome::new(vec![(p, rename(b, other)?)]).sound("random.click"))
        }
        "unpowered_repeater" | "powered_repeater" => {
            let d = (int(b, "repeater_delay") + 1) % 4;
            Some(Outcome::new(vec![(p, set(b, "repeater_delay", State::Int(d)))]).sound("random.click"))
        }
        "unpowered_comparator" | "powered_comparator" => {
            let sub = int(b, "output_subtract_bit") == 0;
            let mut o = Outcome::new(vec![(p, set(b, "output_subtract_bit", State::Bool(sub)))]);
            o.sound = Some(("random.click".into(), if sub { 0.55 } else { 0.5 }));
            Some(o)
        }
        "redstone_ore" | "deepslate_redstone_ore" => Some(Outcome::new(vec![(p, rename(b, &format!("minecraft:lit_{short}"))?)])),
        "sweet_berry_bush" if int(b, "growth") >= 2 => {
            let g = int(b, "growth");
            let count = if g == 3 { 2 + (c.roll % 2) as i8 } else { 1 + (c.roll % 2) as i8 };
            Some(Outcome::new(vec![(p, set(b, "growth", State::Int(1)))]).sound("block.sweet_berry_bush.pick").drop("minecraft:sweet_berries", count))
        }
        "cave_vines_body_with_berries" | "cave_vines_head_with_berries" => {
            Some(Outcome::new(vec![(p, rename(b, "minecraft:cave_vines")?)]).sound("pick_berries.cave_vines").drop("minecraft:glow_berries", 1))
        }
        "dragon_egg" => {
            // teleport up to 15 blocks to a free spot (PNX BlockDragonEgg.teleport)
            for i in 0..1000u32 {
                let r = c.roll.wrapping_mul(2_654_435_761).wrapping_add(i.wrapping_mul(40_503));
                let q = (p.0 + (r % 31) as i32 - 15, p.1 + ((r / 31) % 15) as i32 - 7, p.2 + ((r / 465) % 31) as i32 - 15);
                if world.get(q.0, q.1, q.2) == AIR && q != p {
                    return Some(Outcome::new(vec![(p, AIR), (q, b)]));
                }
            }
            None
        }
        _ => None,
    }
}

/// Redstone hit, play the current note.
pub fn play_note(world: &World, p: (i32, i32, i32)) -> Outcome {
    let pitch = NOTES.lock().unwrap().as_ref().and_then(|n| n.get(&p).copied()).unwrap_or(0);
    let (sound, inst) = instrument(name(world.get(p.0, p.1 - 1, p.2)));
    let mut o = Outcome::new(vec![]);
    o.sound = Some((sound.into(), 2f32.powf((pitch - 12) as f32 / 12.0)));
    o.block_event = Some((inst, pitch));
    o
}

/// Random tick turns lit redstone ore back off.
pub fn unlit_ore(b: i32) -> Option<i32> {
    name(b).strip_prefix("minecraft:lit_").filter(|r| r.ends_with("redstone_ore")).and_then(|r| rename(b, &format!("minecraft:{r}")))
}

/// Clears per-block data on break. Returns a disc to drop and its sound.
pub fn removed(p: (i32, i32, i32)) -> Option<(&'static str, String)> {
    if let Some(n) = NOTES.lock().unwrap().as_mut() {
        n.remove(&p);
    }
    let disc = DISCS.lock().unwrap().as_mut()?.remove(&p)?;
    Some((disc, record(disc)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(n: &str) -> i32 {
        any_state(&format!("minecraft:{n}")).unwrap()
    }

    fn ctx(held: &str) -> Ctx<'_> {
        Ctx { held, sneaking: false, survival: true, hungry: true, face: 1, roll: 0, overworld: true }
    }

    #[test]
    fn tools_change_blocks() {
        let w = World::default();
        w.set(0, -60, 0, block("oak_log"));
        let o = use_block(&w, (0, -60, 0), &ctx("minecraft:iron_axe")).unwrap();
        assert_eq!(name(o.changes[0].1), "minecraft:stripped_oak_log");
        assert_eq!(o.sound.unwrap().0, "use.wood");

        let o = use_block(&w, (3, -61, 3), &ctx("minecraft:wooden_shovel")).unwrap();
        assert!(name(o.changes[0].1).ends_with("path"), "grass to path");

        w.set(5, -60, 5, block("pumpkin"));
        let o = use_block(&w, (5, -60, 5), &ctx("minecraft:shears")).unwrap();
        assert_eq!((name(o.changes[0].1), o.drops.clone()), ("minecraft:carved_pumpkin", vec![("minecraft:pumpkin_seeds", 4)]));

        w.set(7, -60, 7, block("oxidized_copper"));
        let o = use_block(&w, (7, -60, 7), &ctx("minecraft:iron_axe")).unwrap();
        assert_eq!(name(o.changes[0].1), "minecraft:weathered_copper");
        w.set(7, -60, 7, block("copper_block"));
        let o = use_block(&w, (7, -60, 7), &ctx("minecraft:honeycomb")).unwrap();
        assert_eq!((name(o.changes[0].1), o.held), ("minecraft:waxed_copper", Held::UseOne));
    }

    #[test]
    fn noteblock_bell_repeater_anchor() {
        let w = World::default();
        w.set(0, -60, 0, block("noteblock"));
        let o = use_block(&w, (0, -60, 0), &ctx("")).unwrap();
        assert_eq!(o.block_event, Some((0, 0)), "harp on grass, first note");
        let o = use_block(&w, (0, -60, 0), &ctx("")).unwrap();
        assert_eq!(o.block_event, Some((0, 1)));

        w.set(2, -60, 2, block("bell"));
        assert_eq!(use_block(&w, (2, -60, 2), &ctx("")).unwrap().sound.unwrap().0, "block.bell.hit");

        let rep = set(block("unpowered_repeater"), "repeater_delay", State::Int(3));
        w.set(4, -60, 4, rep);
        assert_eq!(int(use_block(&w, (4, -60, 4), &ctx("")).unwrap().changes[0].1, "repeater_delay"), 0);

        w.set(6, -60, 6, block("respawn_anchor"));
        let o = use_block(&w, (6, -60, 6), &ctx("minecraft:glowstone")).unwrap();
        w.set(6, -60, 6, o.changes[0].1);
        assert_eq!(use_block(&w, (6, -60, 6), &ctx("")).unwrap().explode, Some(5.0), "charged anchor in overworld blows up");
    }

    #[test]
    fn harvest_and_jukebox() {
        let w = World::default();
        w.set(0, -60, 0, set(block("sweet_berry_bush"), "growth", State::Int(3)));
        let o = use_block(&w, (0, -60, 0), &ctx("")).unwrap();
        assert_eq!(o.drops, vec![("minecraft:sweet_berries", 2)]);

        w.set(2, -60, 2, block("jukebox"));
        let o = use_block(&w, (2, -60, 2), &ctx("minecraft:music_disc_cat")).unwrap();
        assert_eq!((o.sound.unwrap().0, o.held), ("record.cat".to_string(), Held::UseOne));
        let o = use_block(&w, (2, -60, 2), &ctx("")).unwrap();
        assert_eq!((o.drops, o.stop_sound), (vec![("minecraft:music_disc_cat", 1)], Some("record.cat".to_string())));

        w.set(4, -60, 4, block("cake"));
        let mut full = ctx("");
        full.hungry = false;
        assert!(use_block(&w, (4, -60, 4), &full).unwrap().changes.is_empty(), "not hungry: no bite");
        assert!(use_block(&w, (4, -60, 4), &ctx("")).unwrap().eat.is_some());
    }
}

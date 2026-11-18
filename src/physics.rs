//! Right-click stuff (doors, trapdoors, gates, levers, buttons) and falling blocks.
//! Reads the world and returns changes, so it's testable without a client.

use crate::blocks::{State, entry, state, with};
use crate::world::{AIR, World};

pub type Change = ((i32, i32, i32), i32);

/// Right-click result. `Press` changes revert after a moment (buttons).
pub enum Interaction {
    Toggle(Vec<Change>),
    Press { pressed: Vec<Change>, released: Vec<Change> },
}

fn name(id: i32) -> &'static str {
    entry(id).map_or("", |e| e.name.as_str())
}

pub fn interact(world: &World, (x, y, z): (i32, i32, i32)) -> Option<Interaction> {
    let id = world.get(x, y, z);
    let n = name(id);
    if n.contains("iron_door") || n.contains("iron_trapdoor") {
        return None; // vanilla: iron ones need redstone
    }
    if let Some(State::Bool(pressed)) = state(id, "button_pressed_bit") {
        if *pressed {
            return None;
        }
        return Some(Interaction::Press {
            pressed: vec![((x, y, z), with(id, &[("button_pressed_bit", State::Bool(true))]))],
            released: vec![((x, y, z), id)],
        });
    }
    let Some(State::Bool(open)) = state(id, "open_bit") else { return None };
    let flip = |b: i32| with(b, &[("open_bit", State::Bool(!open))]);
    let mut changes = vec![((x, y, z), flip(id))];
    // other half of the door opens too
    if let Some(State::Bool(upper)) = state(id, "upper_block_bit") {
        let other_y = if *upper { y - 1 } else { y + 1 };
        let other = world.get(x, other_y, z);
        if name(other) == n {
            changes.push(((x, other_y, z), flip(other)));
        }
    }
    Some(Interaction::Toggle(changes))
}

/// Blocks that break when the block under them goes (from PNX canPlaceOn/onUpdate).
/// TODO: only checks floor support, wall torches, signs and ladders don't pop yet.
fn needs_floor(n: &str) -> bool {
    let n = n.trim_start_matches("minecraft:");
    matches!(n, "wheat" | "carrots" | "potatoes" | "beetroot" | "pumpkin_stem" | "melon_stem" | "reeds" | "cactus" | "sweet_berry_bush"
        | "nether_wart" | "short_grass" | "tall_grass" | "fern" | "large_fern" | "deadbush" | "dandelion" | "poppy" | "blue_orchid" | "allium"
        | "azure_bluet" | "oxeye_daisy" | "cornflower" | "lily_of_the_valley" | "torchflower" | "wither_rose" | "sunflower" | "lilac"
        | "rose_bush" | "peony" | "brown_mushroom" | "red_mushroom" | "redstone_wire" | "rail" | "golden_rail" | "detector_rail"
        | "activator_rail" | "snow_layer" | "standing_sign" | "standing_banner" | "flower_pot" | "pink_petals")
        || n.ends_with("_sapling") || n.ends_with("_carpet") || n.ends_with("pressure_plate") || n.ends_with("_tulip")
        || n.ends_with("_standing_sign") || n.ends_with("_door") || n == "wooden_door"
}

/// Offset to the block holding `b` up (PNX canPlaceOn/onUpdate).
fn attached_to(b: i32) -> Option<(i32, i32, i32)> {
    let n = name(b);
    let short = n.trim_start_matches("minecraft:");
    let wall = |f: i32| match f {
        2 => Some((0, 0, 1)),
        3 => Some((0, 0, -1)),
        4 => Some((1, 0, 0)),
        5 => Some((-1, 0, 0)),
        _ => None,
    };
    let int = |key: &str| match state(b, key) {
        Some(State::Int(i)) => *i,
        _ => -1,
    };
    if short.ends_with("torch") && !short.contains("redstone_torch_") {
        // torch_facing_direction is where it leans, so the wall is behind
        return match state(b, "torch_facing_direction") {
            Some(State::Str(s)) => match s.as_str() {
                "west" => Some((1, 0, 0)),
                "east" => Some((-1, 0, 0)),
                "north" => Some((0, 0, 1)),
                "south" => Some((0, 0, -1)),
                _ => Some((0, -1, 0)),
            },
            _ => Some((0, -1, 0)),
        };
    }
    if short == "lever" {
        return match state(b, "lever_direction") {
            Some(State::Str(d)) => match d.as_str() {
                "east" => Some((-1, 0, 0)),
                "west" => Some((1, 0, 0)),
                "south" => Some((0, 0, -1)),
                "north" => Some((0, 0, 1)),
                d if d.starts_with("up") => Some((0, -1, 0)),
                _ => Some((0, 1, 0)),
            },
            _ => Some((0, -1, 0)),
        };
    }
    if short == "ladder" || short.ends_with("wall_sign") || short == "wall_banner" {
        return wall(int("facing_direction"));
    }
    if short.ends_with("_button") {
        return match int("facing_direction") {
            0 => Some((0, 1, 0)),
            1 => Some((0, -1, 0)),
            f => wall(f),
        };
    }
    needs_floor(n).then_some((0, -1, 0))
}

/// Same, but for redstone. Levers and buttons strongly power this block.
pub fn support_of(b: i32) -> Option<(i32, i32, i32)> {
    attached_to(b)
}

/// Breaks everything that depended on the removed block, and whatever depended on those.
/// Returns (pos, old block) in break order.
pub fn unsupported(world: &World, pos: (i32, i32, i32)) -> Vec<Change> {
    let mut out: Vec<Change> = Vec::new();
    let mut todo = vec![pos];
    while let Some((x, y, z)) = todo.pop() {
        for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
            let q = (x + dx, y + dy, z + dz);
            if out.iter().any(|(p, _)| *p == q) || q == pos {
                continue;
            }
            let b = world.get(q.0, q.1, q.2);
            if attached_to(b) == Some((-dx, -dy, -dz)) {
                out.push((q, b));
                todo.push(q);
            }
        }
    }
    out
}

fn int_state(b: i32, key: &str) -> i32 {
    match state(b, key) {
        Some(State::Int(i)) => *i,
        _ => 0,
    }
}

/// from PNX BlockCake.onActivate: 2 food and 0.4 saturation a bite, 7 bites total
pub fn cake_bite(b: i32) -> Option<i32> {
    if name(b) != "minecraft:cake" {
        return None;
    }
    let bites = int_state(b, "bite_counter");
    Some(if bites >= 6 { AIR } else { with(b, &[("bite_counter", State::Int(bites + 1))]) })
}

/// Composter chances in percent (PNX BlockComposter).
fn compost_chance(item: &str) -> u32 {
    let n = item.trim_start_matches("minecraft:");
    match n {
        "cake" | "pumpkin_pie" => 100,
        "bread" | "baked_potato" | "cookie" | "hay_block" | "brown_mushroom_block" | "red_mushroom_block" => 85,
        "apple" | "carrot" | "potato" | "beetroot" | "wheat" | "pumpkin" | "carved_pumpkin" | "melon_block" | "lily_pad" | "waterlily"
        | "brown_mushroom" | "red_mushroom" | "nether_wart" | "fern" | "large_fern" | "cocoa_beans" => 65,
        "melon_slice" | "cactus" | "sugar_cane" | "vine" | "tall_grass" | "dried_kelp_block" | "glow_lichen" => 50,
        "wheat_seeds" | "beetroot_seeds" | "melon_seeds" | "pumpkin_seeds" | "short_grass" | "kelp" | "sweet_berries" | "glow_berries"
        | "seagrass" | "moss_carpet" => 30,
        n if n.ends_with("_sapling") || n.ends_with("leaves") => 30,
        n if n.ends_with("_tulip") || matches!(n, "dandelion" | "poppy" | "blue_orchid" | "allium" | "azure_bluet" | "oxeye_daisy" | "cornflower" | "lily_of_the_valley") => 65,
        _ => 0,
    }
}

/// Some((new block, item used, bone meal out)). Level 8 pops bone meal and resets.
/// `roll` is 0..100.
pub fn compost(b: i32, held: &str, roll: u32) -> Option<(i32, bool, bool)> {
    if name(b) != "minecraft:composter" {
        return None;
    }
    let level = int_state(b, "composter_fill_level");
    if level >= 8 {
        return Some((with(b, &[("composter_fill_level", State::Int(0))]), false, true));
    }
    let chance = compost_chance(held);
    if chance == 0 || level >= 7 {
        return None;
    }
    // TODO: vanilla waits 20 ticks before level 7 turns ready, we do it instantly
    let next = if roll < chance { (level + 1).min(7) + i32::from(level + 1 == 7) } else { level };
    Some((with(b, &[("composter_fill_level", State::Int(next))]), true, false))
}

/// Cauldron + bucket (PNX BlockCauldron.onActivate). Full bucket fills it, empty bucket
/// empties a full one. Some((new block, new bucket)).
pub fn cauldron(b: i32, held: &str) -> Option<(i32, &'static str)> {
    if name(b) != "minecraft:cauldron" {
        return None;
    }
    let level = int_state(b, "fill_level");
    let liquid = match state(b, "cauldron_liquid") {
        Some(State::Str(s)) => s.as_str(),
        _ => "water",
    };
    let set = |lvl: i32, liq: &str| with(b, &[("fill_level", State::Int(lvl)), ("cauldron_liquid", State::Str(liq.into()))]);
    match held {
        "minecraft:water_bucket" if level == 0 || liquid == "water" => Some((set(6, "water"), "minecraft:bucket")),
        "minecraft:lava_bucket" if level == 0 => Some((set(6, "lava"), "minecraft:bucket")),
        "minecraft:bucket" if level == 6 => Some((set(0, "water"), if liquid == "lava" { "minecraft:lava_bucket" } else { "minecraft:water_bucket" })),
        _ => None,
    }
}

pub fn falls(id: i32) -> bool {
    let n = name(id);
    matches!(n, "minecraft:sand" | "minecraft:red_sand" | "minecraft:gravel") || n.ends_with("_concrete_powder")
}

fn passable(world: &World, x: i32, y: i32, z: i32) -> bool {
    let b = world.get(x, y, z);
    b == AIR || name(b) == "minecraft:water" || name(b) == "minecraft:lava"
}

/// Drops unsupported gravity blocks in the column above `pos` down to the first solid
/// block. Returns the changes.
pub fn settle(world: &World, (x, y, z): (i32, i32, i32)) -> Vec<Change> {
    const MIN_Y: i32 = -64;
    let mut changes = Vec::new();
    // use a small overlay so a falling stack lands in the right order
    let mut overlay: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();
    let get = |overlay: &std::collections::HashMap<i32, i32>, yy: i32| overlay.get(&yy).copied().unwrap_or_else(|| world.get(x, yy, z));
    let open = |overlay: &std::collections::HashMap<i32, i32>, yy: i32| match overlay.get(&yy) {
        Some(&b) => b == AIR,
        None => passable(world, x, yy, z),
    };
    for yy in y.max(MIN_Y + 1)..y + 64 {
        let b = get(&overlay, yy);
        if !falls(b) {
            if b == AIR && yy > y {
                break; // gap above the changed block: nothing further up rests on it
            }
            continue;
        }
        let mut land = yy;
        while land - 1 > MIN_Y && open(&overlay, land - 1) {
            land -= 1;
        }
        if land != yy {
            overlay.insert(yy, AIR);
            overlay.insert(land, b);
        }
    }
    let mut ys: Vec<_> = overlay.into_iter().collect();
    ys.sort();
    for (yy, b) in ys {
        if world.get(x, yy, z) != b {
            changes.push(((x, yy, z), b));
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::any_state;
    use crate::world::GRASS;

    fn block(name: &str) -> i32 {
        any_state(&format!("minecraft:{name}")).unwrap()
    }

    fn apply(w: &World, changes: &[Change]) {
        for &((x, y, z), b) in changes {
            w.set(x, y, z, b);
        }
    }

    #[test]
    fn cane_and_flowers_pop_when_their_floor_goes() {
        let w = World::default();
        w.set(2, -60, 2, block("reeds"));
        w.set(2, -59, 2, block("reeds"));
        w.set(2, -58, 2, block("stone"));
        assert_eq!(unsupported(&w, (2, -61, 2)).len(), 2, "both cane blocks, not the stone");
        w.set(4, -60, 4, block("poppy"));
        assert_eq!(unsupported(&w, (4, -61, 4)).len(), 1);
        assert!(unsupported(&w, (6, -61, 6)).is_empty());
        // torch on the east side leans east and should pop with the stone
        let torch = with(block("torch"), &[("torch_facing_direction", State::Str("east".into()))]);
        w.set(8, -60, 8, block("stone"));
        w.set(9, -60, 8, torch);
        assert_eq!(unsupported(&w, (8, -60, 8)), vec![((9, -60, 8), torch)]);
    }

    #[test]
    fn cake_composter_cauldron() {
        let mut cake = block("cake");
        for _ in 0..6 {
            cake = cake_bite(cake).unwrap();
            assert_ne!(cake, AIR);
        }
        assert_eq!(cake_bite(cake), Some(AIR), "7th bite finishes it");

        let mut c = block("composter");
        for _ in 0..7 {
            let (n, used, out) = compost(c, "minecraft:cake", 0).unwrap();
            assert!(used && !out);
            c = n;
        }
        assert_eq!(int_state(c, "composter_fill_level"), 8, "ready");
        assert!(compost(c, "minecraft:cake", 0).unwrap().2, "gives bone meal");
        assert!(compost(block("composter"), "minecraft:stone", 0).is_none());

        let (full, bucket) = cauldron(block("cauldron"), "minecraft:water_bucket").unwrap();
        assert_eq!((int_state(full, "fill_level"), bucket), (6, "minecraft:bucket"));
        assert_eq!(cauldron(full, "minecraft:bucket").map(|x| x.1), Some("minecraft:water_bucket"));
    }

    #[test]
    fn doors_open_both_halves_and_iron_stays_shut() {
        let w = World::default();
        let door = block("wooden_door");
        let lower = with(door, &[("upper_block_bit", State::Bool(false))]);
        let upper = with(door, &[("upper_block_bit", State::Bool(true))]);
        w.set(0, -60, 0, lower);
        w.set(0, -59, 0, upper);
        let Some(Interaction::Toggle(c)) = interact(&w, (0, -59, 0)) else { panic!("door should toggle") };
        assert_eq!(c.len(), 2);
        apply(&w, &c);
        assert_eq!(state(w.get(0, -60, 0), "open_bit"), Some(&State::Bool(true)));
        assert_eq!(state(w.get(0, -59, 0), "open_bit"), Some(&State::Bool(true)));
        w.set(2, -60, 0, block("iron_door"));
        assert!(interact(&w, (2, -60, 0)).is_none());
        assert!(interact(&w, (5, -61, 5)).is_none(), "grass isn't interactive");
    }

    #[test]
    fn buttons_press_and_release() {
        let w = World::default();
        w.set(1, -60, 1, block("stone_button"));
        let Some(Interaction::Press { pressed, released }) = interact(&w, (1, -60, 1)) else { panic!() };
        apply(&w, &pressed);
        assert_eq!(state(w.get(1, -60, 1), "button_pressed_bit"), Some(&State::Bool(true)));
        assert!(interact(&w, (1, -60, 1)).is_none(), "already pressed");
        apply(&w, &released);
        assert_eq!(state(w.get(1, -60, 1), "button_pressed_bit"), Some(&State::Bool(false)));
    }

    #[test]
    fn sand_stack_falls_when_support_breaks() {
        let w = World::default();
        let (sand, gravel) = (block("sand"), block("gravel"));
        w.set(3, -55, 3, sand);
        w.set(3, -54, 3, gravel);
        // placed in the air, lands on the grass (top at -61) and keeps stack order
        apply(&w, &settle(&w, (3, -55, 3)));
        assert_eq!(w.get(3, -60, 3), sand);
        assert_eq!(w.get(3, -59, 3), gravel);
        assert_eq!(w.get(3, -55, 3), AIR);
        // dig out the grass and the stack drops one
        w.set(3, -61, 3, AIR);
        apply(&w, &settle(&w, (3, -61, 3)));
        assert_eq!(w.get(3, -61, 3), sand);
        assert_eq!(w.get(3, -60, 3), gravel);
        assert_eq!(w.get(3, -59, 3), AIR);
        assert!(settle(&w, (9, -61, 9)).is_empty());
        assert_eq!(w.get(9, -61, 9), GRASS);
    }
}

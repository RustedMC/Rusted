//! Random ticks, like vanilla's randomTickSpeed. Leaves decay, crops and saplings grow,
//! grass spreads, ice melts near light, snow piles up when it rains in snowy biomes.

use crate::blocks::{self, State};
use crate::world::{AIR, World};

fn name(b: i32) -> &'static str {
    blocks::entry(b).map_or("", |e| e.name.as_str())
}

fn is_log(b: i32) -> bool {
    let n = name(b);
    n.ends_with("_log") || n.ends_with("_wood") || n.ends_with("_stem")
}

fn int(b: i32, key: &str) -> i32 {
    match blocks::state(b, key) {
        Some(State::Int(i)) => *i,
        Some(State::Bool(v)) => *v as i32,
        _ => 0,
    }
}

fn emits_light(b: i32) -> bool {
    matches!(name(b), "minecraft:torch" | "minecraft:lantern" | "minecraft:glowstone" | "minecraft:lava" | "minecraft:flowing_lava"
        | "minecraft:sea_lantern" | "minecraft:jack_o_lantern" | "minecraft:lit_furnace" | "minecraft:campfire" | "minecraft:shroomlight" | "minecraft:fire")
}

/// Random tick at `p`, returns (pos, new block). `raining`/`snowy` are the weather and
/// biome there.
pub fn random_tick(world: &World, p: (i32, i32, i32), roll: u32, raining: bool, snowy: bool) -> Vec<((i32, i32, i32), i32)> {
    let b = world.get(p.0, p.1, p.2);
    let n = name(b);
    let (x, y, z) = p;
    let above = world.get(x, y + 1, z);
    if let Some(off) = crate::useblock::unlit_ore(b) {
        return vec![(p, off)];
    }
    match n {
        n if n.ends_with("leaves") && int(b, "persistent_bit") == 0 => {
            // decays with no log within 4 blocks
            for dx in -4..=4 {
                for dy in -4..=4 {
                    for dz in -4..=4 {
                        if is_log(world.get(x + dx, y + dy, z + dz)) {
                            return vec![];
                        }
                    }
                }
            }
            vec![(p, AIR)]
        }
        n if n.ends_with("_sapling") || n == "minecraft:sapling" => {
            if !roll.is_multiple_of(7) {
                return vec![];
            }
            if int(b, "age_bit") == 0 {
                return vec![(p, blocks::with(b, &[("age_bit", State::Bool(true))]))];
            }
            grow_tree(world, p, &n.replace("_sapling", ""))
        }
        "minecraft:wheat" | "minecraft:carrots" | "minecraft:potatoes" | "minecraft:beetroot" => {
            let g = int(b, "growth");
            let on_farmland = name(world.get(x, y - 1, z)) == "minecraft:farmland";
            if g < 7 && on_farmland && roll.is_multiple_of(3) {
                vec![(p, blocks::with(b, &[("growth", State::Int(g + 1))]))]
            } else {
                vec![]
            }
        }
        // from PNX BlockLava: hop up to 2 blocks looking for air next to something flammable,
        // otherwise try the tops of up to 3 blocks nearby
        "minecraft:lava" | "minecraft:flowing_lava" => {
            let Some(fire) = blocks::any_state("minecraft:fire") else { return vec![] };
            let near_fuel = |q: (i32, i32, i32)| {
                [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)].iter().any(|(a, c, e)| flammability(world.get(q.0 + a, q.1 + c, q.2 + e)).0 > 0)
            };
            let mut r = roll;
            let mut next = |n: u32| {
                let v = r % n;
                r /= n;
                v as i32
            };
            let hops = next(3);
            let mut q = p;
            if hops > 0 {
                for _ in 0..hops {
                    q = (q.0 + next(3) - 1, q.1 + 1, q.2 + next(3) - 1);
                    let here = world.get(q.0, q.1, q.2);
                    if here == AIR {
                        if near_fuel(q) {
                            return vec![(q, fire)];
                        }
                    } else if crate::overworld::feature::blocks::is_solid(here) {
                        break;
                    }
                }
            } else {
                for _ in 0..3 {
                    let t = (x + next(3) - 1, y, z + next(3) - 1);
                    if world.get(t.0, t.1 + 1, t.2) == AIR && flammability(world.get(t.0, t.1, t.2)).0 > 0 {
                        return vec![((t.0, t.1 + 1, t.2), fire)];
                    }
                }
            }
            vec![]
        }
        // from PNX BlockSugarcane/BlockCactus: age goes to 15, then it grows a block (max 3 tall)
        "minecraft:reeds" | "minecraft:cactus" => {
            if world.get(x, y + 1, z) != AIR || (1..3).all(|d| name(world.get(x, y - d, z)) == n) {
                return vec![];
            }
            let age = int(b, "age");
            if age < 15 {
                return vec![(p, blocks::with(b, &[("age", State::Int(age + 1))]))];
            }
            vec![(p, blocks::with(b, &[("age", State::Int(0))])), ((x, y + 1, z), blocks::with(b, &[("age", State::Int(0))]))]
        }
        // PNX BlockStem: 1 in 2, puts a fruit on a free side once fully grown
        "minecraft:pumpkin_stem" | "minecraft:melon_stem" => {
            if !roll.is_multiple_of(2) {
                return vec![];
            }
            let g = int(b, "growth");
            if g < 7 {
                return vec![(p, blocks::with(b, &[("growth", State::Int(g + 1))]))];
            }
            let fruit = if n == "minecraft:pumpkin_stem" { "minecraft:pumpkin" } else { "minecraft:melon_block" };
            let sides = [(1, 0), (-1, 0), (0, 1), (0, -1)];
            let (dx, dz) = sides[(roll / 2 % 4) as usize];
            let ground = name(world.get(x + dx, y - 1, z + dz));
            if world.get(x + dx, y, z + dz) == AIR
                && matches!(ground, "minecraft:dirt" | "minecraft:grass_block" | "minecraft:farmland")
                && !sides.iter().any(|(a, c)| name(world.get(x + a, y, z + c)) == fruit)
                && let Some(f) = blocks::any_state(fruit)
            {
                return vec![((x + dx, y, z + dz), f)];
            }
            vec![]
        }
        // PNX BlockSweetBerryBush: 1 in 5, ages 0..3.
        "minecraft:sweet_berry_bush" => grow_by(b, "growth", 3, roll.is_multiple_of(5), p),
        // PNX BlockNetherWart: 1 in 10, ages 0..3.
        "minecraft:nether_wart" => grow_by(b, "age", 3, roll.is_multiple_of(10), p),
        // PNX BlockCocoa: 1 in 5, ages 0..2.
        "minecraft:cocoa" => grow_by(b, "age", 2, roll.is_multiple_of(5), p),
        // PNX BlockFarmland: stays wet with water within 4 blocks, otherwise dries and turns
        // back to dirt if nothing's planted
        "minecraft:farmland" => {
            let wet = (-4..=4).any(|dx| (0..=1).any(|dy| (-4..=4).any(|dz| name(world.get(x + dx, y + dy, z + dz)).ends_with("water"))));
            let m = int(b, "moisturized_amount");
            if wet {
                return if m < 7 { vec![(p, blocks::with(b, &[("moisturized_amount", State::Int(7))]))] } else { vec![] };
            }
            if m > 0 {
                return vec![(p, blocks::with(b, &[("moisturized_amount", State::Int(m - 1))]))];
            }
            let planted = matches!(name(above), "minecraft:wheat" | "minecraft:carrots" | "minecraft:potatoes" | "minecraft:beetroot" | "minecraft:pumpkin_stem" | "minecraft:melon_stem");
            match blocks::any_state("minecraft:dirt") {
                Some(d) if !planted => vec![(p, d)],
                _ => vec![],
            }
        }
        "minecraft:grass_block" => {
            // spread to nearby dirt with air above
            // TODO: grass should die under solid blocks, needs light/opacity data
            if raining && snowy && above == AIR && let Some(s) = blocks::any_state("minecraft:snow_layer") {
                return vec![((x, y + 1, z), s)];
            }
            let (dx, dy, dz) = ((roll % 3) as i32 - 1, ((roll / 3) % 5) as i32 - 3, ((roll / 15) % 3) as i32 - 1);
            let t = (x + dx, y + dy, z + dz);
            let tb = world.get(t.0, t.1, t.2);
            if name(tb) == "minecraft:dirt" && int(tb, "dirt_type") == 0 && world.get(t.0, t.1 + 1, t.2) == AIR {
                return vec![(t, b)];
            }
            vec![]
        }
        "minecraft:ice" => {
            let lit = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1), (2, 0, 0), (-2, 0, 0), (0, 0, 2), (0, 0, -2)]
                .iter()
                .any(|(dx, dy, dz)| emits_light(world.get(x + dx, y + dy, z + dz)));
            if lit { vec![(p, crate::fluids::Kind::Water.source())] } else { vec![] }
        }
        "minecraft:snow_layer" => {
            let lit = [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, 1, 0)].iter().any(|(dx, dy, dz)| emits_light(world.get(x + dx, y + dy, z + dz)));
            if lit { vec![(p, AIR)] } else { vec![] }
        }
        n if raining && snowy && above == AIR && blocks::entry(b).is_some() && b != AIR && !n.contains("water") && !n.contains("lava") && !n.contains("snow") && !n.contains("leaves") && !n.contains("flower") && !n.contains("grass") => {
            // snow settles on other solid ground in snowy biomes too
            match blocks::any_state("minecraft:snow_layer") {
                Some(s) if roll.is_multiple_of(4) => vec![((x, y + 1, z), s)],
                _ => vec![],
            }
        }
        _ => vec![],
    }
}

/// Fires being ticked. PNX schedules each one every 30-40 ticks.
pub static FIRES: parking_lot::Mutex<Vec<(i32, i32, i32)>> = parking_lot::Mutex::new(Vec::new());

/// (catch chance, burn-away chance) from PNX BlockFire. 0 = fireproof.
fn flammability(b: i32) -> (u32, u32) {
    let n = name(b).trim_start_matches("minecraft:");
    match n {
        n if n.ends_with("_planks") || n.ends_with("_fence") || n.ends_with("_fence_gate") || n.ends_with("_stairs") && !n.contains("stone") && !n.contains("brick") => (5, 20),
        n if (n.ends_with("_log") || n.ends_with("_wood")) && !n.contains("crimson") && !n.contains("warped") => (5, 5),
        n if n.ends_with("leaves") => (30, 60),
        n if n.ends_with("_wool") || n == "bookshelf" => (30, 60),
        n if n.ends_with("_carpet") || n == "hay_block" => (60, 20),
        "short_grass" | "tall_grass" | "fern" | "large_fern" | "deadbush" | "vine" | "tnt" => (60, 100),
        n if n.ends_with("_tulip") || matches!(n, "dandelion" | "poppy" | "cornflower" | "allium" | "oxeye_daisy" | "azure_bluet") => (60, 100),
        _ => (0, 0),
    }
}

/// One fire update (from PNX BlockFire.onUpdate). Ages, goes out in rain or without fuel,
/// burns neighbours and spreads to air next to flammable stuff. `rand(n)` is 0..n.
pub fn fire_tick(world: &World, p: (i32, i32, i32), raining: bool, mut rand: impl FnMut(u32) -> u32) -> Vec<((i32, i32, i32), i32)> {
    let b = world.get(p.0, p.1, p.2);
    if name(b) != "minecraft:fire" {
        return vec![];
    }
    let (x, y, z) = p;
    let below = name(world.get(x, y - 1, z));
    let forever = matches!(below, "minecraft:netherrack" | "minecraft:magma" | "minecraft:soul_soil" | "minecraft:soul_sand");
    let sides = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)];
    let fueled = sides.iter().any(|(dx, dy, dz)| flammability(world.get(x + dx, y + dy, z + dz)).0 > 0);
    let age = int(b, "age");
    if raining && !forever && rand(5) < 2 {
        return vec![(p, AIR)];
    }
    let mut out = Vec::new();
    if !forever && !fueled {
        if age > 3 || !crate::overworld::feature::blocks::is_solid(world.get(x, y - 1, z)) {
            return vec![(p, AIR)];
        }
    } else if !forever && age == 15 && rand(4) == 0 && flammability(world.get(x, y - 1, z)).0 == 0 {
        return vec![(p, AIR)];
    }
    if age < 15 {
        out.push((p, blocks::with(b, &[("age", State::Int((age + rand(3) as i32 / 2).min(15)))])));
    }
    // some neighbours catch, the rest just burn away
    for (dx, dy, dz) in sides {
        let q = (x + dx, y + dy, z + dz);
        let (_, burn) = flammability(world.get(q.0, q.1, q.2));
        let bound = if dy != 0 { 250 } else { 300 };
        if burn > 0 && rand(bound) < burn {
            out.push((q, if rand(age as u32 + 10) < 5 { blocks::with(b, &[("age", State::Int(0))]) } else { AIR }));
        }
    }
    // spread to nearby air that touches something flammable
    let (dx, dy, dz) = (rand(3) as i32 - 1, rand(6) as i32 - 1, rand(3) as i32 - 1);
    let q = (x + dx, y + dy, z + dz);
    if q != p && world.get(q.0, q.1, q.2) == AIR {
        let catch = sides.iter().map(|(a, c, e)| flammability(world.get(q.0 + a, q.1 + c, q.2 + e)).0).max().unwrap_or(0);
        if catch > 0 && rand(100) < catch {
            out.push((q, blocks::with(b, &[("age", State::Int(0))])));
        }
    }
    out
}

fn grow_by(b: i32, key: &str, max: i32, chance: bool, p: (i32, i32, i32)) -> Vec<((i32, i32, i32), i32)> {
    let g = int(b, key);
    if chance && g < max { vec![(p, blocks::with(b, &[(key, State::Int(g + 1))]))] } else { vec![] }
}

/// Bone meal (PNX). Crops/stems jump 2-5 stages, berries and cocoa 1, saplings grow 45%
/// of the time. None means it did nothing and isn't used up.
pub fn bone_meal(world: &World, p: (i32, i32, i32), roll: u32) -> Option<Vec<((i32, i32, i32), i32)>> {
    let b = world.get(p.0, p.1, p.2);
    let n = name(b);
    let bump = |key: &str, max: i32, by: i32| {
        let g = int(b, key);
        (g < max).then(|| vec![(p, blocks::with(b, &[(key, State::Int((g + by).min(max)))]))])
    };
    match n {
        "minecraft:wheat" | "minecraft:carrots" | "minecraft:potatoes" | "minecraft:beetroot" | "minecraft:pumpkin_stem" | "minecraft:melon_stem" => {
            bump("growth", 7, 2 + (roll % 4) as i32)
        }
        "minecraft:sweet_berry_bush" => bump("growth", 3, 1),
        "minecraft:cocoa" => bump("age", 2, 1),
        n if n.ends_with("_sapling") || n == "minecraft:sapling" => {
            if roll % 100 >= 45 {
                return Some(vec![]); // used up, no growth this time
            }
            Some(grow_tree(world, p, &n.replace("_sapling", "")))
        }
        _ => None,
    }
}

/// Small tree at a sapling if there's room.
fn grow_tree(world: &World, p: (i32, i32, i32), kind: &str) -> Vec<((i32, i32, i32), i32)> {
    let kind = kind.strip_prefix("minecraft:").unwrap_or(kind);
    let kind = if kind.is_empty() || kind == "sapling" { "oak" } else { kind };
    let (Some(log), Some(leaves)) = (blocks::any_state(&format!("minecraft:{kind}_log")), blocks::any_state(&format!("minecraft:{kind}_leaves"))) else {
        return vec![];
    };
    let (x, y, z) = p;
    let trunk = 5;
    for dy in 1..=trunk + 1 {
        if world.get(x, y + dy, z) != AIR {
            return vec![]; // no room
        }
    }
    let mut out = Vec::new();
    let top = y + trunk - 1;
    for ly in top - 2..=top + 1 {
        let r: i32 = if ly >= top { 1 } else { 2 };
        for dx in -r..=r {
            for dz in -r..=r {
                if dx.abs() == r && dz.abs() == r && ly == top + 1 {
                    continue;
                }
                if world.get(x + dx, ly, z + dz) == AIR {
                    out.push(((x + dx, ly, z + dz), leaves));
                }
            }
        }
    }
    for dy in 0..trunk {
        out.retain(|(q, _)| *q != (x, y + dy, z));
        out.push(((x, y + dy, z), log));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_grow_leaves_decay_saplings_grow() {
        let w = World::default(); // flat: grass at y -61
        let farmland = blocks::any_state("minecraft:farmland").unwrap();
        let wheat = blocks::with(blocks::any_state("minecraft:wheat").unwrap(), &[("growth", State::Int(0))]);
        w.set(0, -61, 0, farmland);
        w.set(0, -60, 0, wheat);
        let ch = random_tick(&w, (0, -60, 0), 3, false, false);
        assert_eq!(ch.len(), 1);
        assert_eq!(int(ch[0].1, "growth"), 1);

        let leaves = blocks::with(blocks::any_state("minecraft:oak_leaves").unwrap(), &[("persistent_bit", State::Bool(false))]);
        w.set(5, -55, 5, leaves);
        assert_eq!(random_tick(&w, (5, -55, 5), 0, false, false), vec![((5, -55, 5), AIR)], "no log nearby: decays");
        w.set(5, -57, 5, blocks::any_state("minecraft:oak_log").unwrap());
        assert!(random_tick(&w, (5, -55, 5), 0, false, false).is_empty(), "log nearby: stays");

        let sapling = blocks::any_state("minecraft:oak_sapling").unwrap();
        let grown = blocks::with(sapling, &[("age_bit", State::Bool(true))]);
        w.set(10, -60, 10, grown);
        let tree = random_tick(&w, (10, -60, 10), 0, false, false);
        assert!(tree.iter().any(|(q, b)| *q == (10, -60, 10) && is_log(*b)), "sapling becomes a trunk");
    }

    #[test]
    fn lava_lights_nearby_wood() {
        let w = World::default();
        let lava = crate::fluids::Kind::Lava.source();
        w.set(0, -60, 0, lava);
        w.set(1, -59, 0, blocks::any_state("minecraft:oak_planks").unwrap());
        // roll 0 means no hops, so it checks block tops around it. planks under air there?
        // roll for one hop straight up: hops=1 (r%3), dx=1 (=> +0), dz=1 (=> +0)
        let roll = 1 + 3 * (1 + 3);
        let ch = random_tick(&w, (0, -60, 0), roll, false, false);
        assert_eq!(ch.first().map(|(q, b)| (*q, name(*b))), Some(((0, -59, 0), "minecraft:fire")));
    }

    #[test]
    fn fire_burns_out_alone_and_spreads_on_wood() {
        let w = World::default();
        let fire = blocks::with(blocks::any_state("minecraft:fire").unwrap(), &[("age", State::Int(5))]);
        w.set(0, -60, 0, fire);
        assert_eq!(fire_tick(&w, (0, -60, 0), false, |_| 0), vec![((0, -60, 0), AIR)], "no fuel: goes out");
        let planks = blocks::any_state("minecraft:oak_planks").unwrap();
        w.set(4, -60, 4, fire);
        w.set(5, -60, 4, planks);
        let ch = fire_tick(&w, (4, -60, 4), false, |_| 0);
        assert!(ch.iter().any(|(q, b)| *q == (5, -60, 4) && name(*b) == "minecraft:fire"), "planks caught fire: {ch:?}");
    }

    #[test]
    fn cane_stems_farmland_and_bone_meal() {
        let w = World::default();
        let cane = blocks::with(blocks::any_state("minecraft:reeds").unwrap(), &[("age", State::Int(15))]);
        w.set(0, -60, 0, cane);
        let ch = random_tick(&w, (0, -60, 0), 0, false, false);
        assert!(ch.iter().any(|(q, b)| *q == (0, -59, 0) && name(*b) == "minecraft:reeds"), "cane grew a block");

        let stem = blocks::with(blocks::any_state("minecraft:pumpkin_stem").unwrap(), &[("growth", State::Int(7))]);
        w.set(4, -60, 4, stem);
        let ch = random_tick(&w, (4, -60, 4), 0, false, false);
        assert!(ch.iter().any(|(_, b)| name(*b) == "minecraft:pumpkin"), "pumpkin appeared: {ch:?}");

        let dry = blocks::with(blocks::any_state("minecraft:farmland").unwrap(), &[("moisturized_amount", State::Int(0))]);
        w.set(8, -61, 8, dry);
        assert_eq!(random_tick(&w, (8, -61, 8), 0, false, false).first().map(|(_, b)| name(*b)), Some("minecraft:dirt"));

        let wheat = blocks::with(blocks::any_state("minecraft:wheat").unwrap(), &[("growth", State::Int(0))]);
        w.set(12, -60, 12, wheat);
        let g = bone_meal(&w, (12, -60, 12), 1).unwrap();
        assert_eq!(int(g[0].1, "growth"), 3);
        assert!(bone_meal(&w, (20, -61, 20), 0).is_none(), "nothing on grass");
    }
}

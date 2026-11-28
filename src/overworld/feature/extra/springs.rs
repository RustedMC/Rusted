// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Water and lava springs in stone walls, from PNX `OverworldSurfaceSpringsFeature`.

use super::super::blocks::{self, is_air};
use super::super::decoration::placement::{reseed, MIN_Y};
use super::super::{Ctx, FeatureFn, Region};
use crate::overworld::biome::id as biome;
use std::collections::HashMap;

const NAME: &str = "minecraft:overworld_surface_springs_feature";
const VALID: [&str; 7] = ["stone", "deepslate", "tuff", "calcite", "snow", "powder_snow", "packed_ice"];
/// DOWN, NORTH, SOUTH, WEST, EAST.
const NEIGHBORS: [(i32, i32, i32); 5] = [(0, -1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)];

pub fn register(m: &mut HashMap<&'static str, FeatureFn>) {
    m.insert(NAME, apply);
    m.insert("minecraft:legacy:springs_feature", apply);
}

// from PNX OverworldSurfaceSpringsFeature
fn apply(r: &mut Region, ctx: &mut Ctx) {
    reseed(ctx, NAME);
    place(r, ctx, 25, false);
    place(r, ctx, 20, true);
}

fn place(r: &mut Region, ctx: &mut Ctx, count: i32, lava: bool) {
    let fluid = blocks::id(if lava { "lava" } else { "water" });
    let rand = &mut ctx.random;
    for _ in 0..count {
        let x = (ctx.cx << 4) + rand.next_int_below(16);
        let z = (ctx.cz << 4) + rand.next_int_below(16);
        let y = if lava {
            // range 174 - minY is always positive for MIN_Y -64
            let bias = rand.next_double() * rand.next_double() * rand.next_double();
            MIN_Y + 8 + (bias * (174 - MIN_Y) as f64) as i32
        } else {
            MIN_Y + rand.next_int_below(192 - MIN_Y)
        };
        if lava && !lava_biome_allowed(r.biome(x, y, z)) {
            continue;
        }
        if can_place(r, x, y, z) {
            r.set(x, y, z, fluid);
            // Flows once the chunk is in the world (PNX schedules a block update).
            // shortcut: assumes the main overworld (y base 0); springs in /mv worlds stay still until touched.
            crate::fluids::spring((x, y, z));
        }
    }
}

fn valid(b: i32) -> bool {
    VALID.contains(&blocks::short(b))
}

/// Biome tags: frozen+ocean are the three frozen oceans, ice_plains+mutated is ice spikes.
fn lava_biome_allowed(b: u16) -> bool {
    !matches!(b, biome::DEEP_DARK | biome::LEGACY_FROZEN_OCEAN | biome::FROZEN_OCEAN | biome::DEEP_FROZEN_OCEAN | biome::ICE_PLAINS_SPIKES)
}

fn can_place(r: &Region, x: i32, y: i32, z: i32) -> bool {
    if !valid(r.get(x, y + 1, z)) || !valid(r.get(x, y - 1, z)) {
        return false;
    }
    let cur = r.get(x, y, z);
    if !is_air(cur) && !valid(cur) {
        return false;
    }
    let (mut rock, mut hole) = (0, 0);
    for (dx, dy, dz) in NEIGHBORS {
        let b = r.get(x + dx, y + dy, z + dz);
        rock += valid(b) as i32;
        hole += is_air(b) as i32;
    }
    rock == 4 && hole == 1
}

#[cfg(test)]
mod tests {
    use super::super::super::{blocks, Ctx, Region, Xoroshiro};
    use crate::overworld::Chunk;

    #[test]
    fn springs_run() {
        let stone = blocks::id("stone");
        let chunks = (0..9).map(|_| {
            let mut c = Chunk::new();
            for (x, z, y) in (0..16).flat_map(|x| (0..16).flat_map(move |z| (-64..100).map(move |y| (x, z, y)))) {
                if y % 7 != 0 {
                    c.set(x, y, z, stone);
                }
            }
            c
        }).collect();
        let mut r = Region::new(0, 0, 1, chunks);
        super::apply(&mut r, &mut Ctx { cx: 0, cz: 0, seed: 1, random: Xoroshiro::new(0) });
    }
}

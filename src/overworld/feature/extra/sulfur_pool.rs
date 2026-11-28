// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Small water pools rimmed with sulfur and a wet potent sulfur at the bottom, from PNX
//! `SulfurPoolWithPotentSulfurSnapToSurfaceFeature` (a `LakeFeature`).

use std::collections::HashMap;

use super::super::blocks::{self, s};
use super::super::decoration::placement::{reseed, MIN_Y};
use super::super::{Ctx, FeatureFn, Region, Xoroshiro};
use crate::overworld::biome::id::SULFUR_CAVES;

const NAME: &str = "minecraft:sulfur_pool_with_potent_sulfur_snap_to_surface_feature";
const PLACEMENT_ATTEMPTS: i32 = 256;
const MAX_ENVIRONMENT_SCAN_STEPS: i32 = 32;

pub fn register(m: &mut HashMap<&'static str, FeatureFn>) {
    m.insert("minecraft:sulfur_caves_surface_sulfur_pool_with_potent_sulfur_feature_rules", apply);
    m.insert(NAME, apply);
}

// from PNX SulfurPoolWithPotentSulfurSnapToSurfaceFeature
fn apply(r: &mut Region, ctx: &mut Ctx) {
    reseed(ctx, NAME);
    let max_y = 256; // min(256, getMaxHeight())
    for _ in 0..PLACEMENT_ATTEMPTS {
        let x = (ctx.cx << 4) + ctx.random.next_int_below(16);
        let y = MIN_Y + ctx.random.next_int_below(max_y - MIN_Y + 1);
        let z = (ctx.cz << 4) + ctx.random.next_int_below(16);
        if !blocks::is_solid(r.get(x, y, z)) {
            continue;
        }
        let Some(surface) = scan_up_to_surface(r, x, y, z, max_y) else { continue };
        if r.biome(x, surface, z) != SULFUR_CAVES {
            continue;
        }
        let lake_y = surface + 1;
        if place_lake(r, &mut ctx.random, x, lake_y, z) {
            place_potent_sulfur(r, x, lake_y, z);
            break;
        }
    }
}

fn scan_up_to_surface(r: &Region, x: i32, y: i32, z: i32, max_y: i32) -> Option<i32> {
    let mut step = 0;
    while step <= MAX_ENVIRONMENT_SCAN_STEPS && y + step <= max_y {
        let v = r.get(x, y + step, z);
        if blocks::is_air(v) || blocks::is(v, "sulfur_spike") {
            return Some(y + step - 1);
        }
        step += 1;
    }
    None
}

fn place_potent_sulfur(r: &mut Region, x: i32, y: i32, z: i32) {
    for step in 0..=4 {
        let sy = y - step;
        if blocks::is_solid(r.get(x, sy, z)) && blocks::is_water(r.get(x, sy + 1, z)) {
            r.set(x, sy, z, blocks::with(blocks::id("potent_sulfur"), &[("potent_sulfur_state", s("wet"))]));
            return;
        }
    }
}

/// Approximation of the vanilla `minecraft:not_feature_replaceable` tag.
// TODO: Rusted has no block tags; list taken from the vanilla tag, check it against PNX's data.
fn not_feature_replaceable(v: i32) -> bool {
    matches!(blocks::short(v), "bedrock" | "mob_spawner" | "chest" | "end_portal_frame" | "end_portal" | "end_gateway" | "reinforced_deepslate" | "trial_spawner" | "vault")
}

fn idx(x: usize, y: usize, z: usize) -> usize {
    (x * 16 + z) * 8 + y
}

fn boundary(g: &[bool], x: usize, y: usize, z: usize) -> bool {
    !g[idx(x, y, z)]
        && (x < 15 && g[idx(x + 1, y, z)]
            || x > 0 && g[idx(x - 1, y, z)]
            || z < 15 && g[idx(x, y, z + 1)]
            || z > 0 && g[idx(x, y, z - 1)]
            || y < 7 && g[idx(x, y + 1, z)]
            || y > 0 && g[idx(x, y - 1, z)])
}

/// PNX `LakeFeature.place` with this feature's fluid (water), barrier (sulfur) and
/// `canPlaceFeature` (not on a sulfur spike).
fn place_lake(r: &mut Region, rand: &mut Xoroshiro, ox: i32, oy: i32, oz: i32) -> bool {
    if oy <= MIN_Y + 4 {
        return false;
    }
    let (ox, oy, oz) = (ox - 8, oy - 4, oz - 8);
    let mut g = vec![false; 16 * 16 * 8];
    let spots = rand.next_int_below(4) + 4;
    for _ in 0..spots {
        let xr = rand.next_double() * 6.0 + 3.0;
        let yr = rand.next_double() * 4.0 + 2.0;
        let zr = rand.next_double() * 6.0 + 3.0;
        let xp = rand.next_double() * (16.0 - xr - 2.0) + 1.0 + xr / 2.0;
        let yp = rand.next_double() * (8.0 - yr - 4.0) + 2.0 + yr / 2.0;
        let zp = rand.next_double() * (16.0 - zr - 2.0) + 1.0 + zr / 2.0;
        for xx in 1..15 {
            for zz in 1..15 {
                for yy in 1..7 {
                    let xd = (xx as f64 - xp) / (xr / 2.0);
                    let yd = (yy as f64 - yp) / (yr / 2.0);
                    let zd = (zz as f64 - zp) / (zr / 2.0);
                    if xd * xd + yd * yd + zd * zd < 1.0 {
                        g[idx(xx, yy, zz)] = true;
                    }
                }
            }
        }
    }
    let (water, sulfur, air) = (blocks::id("water"), blocks::id("sulfur"), blocks::air());
    let at = |xx: usize, yy: usize, zz: usize| (ox + xx as i32, oy + yy as i32, oz + zz as i32);

    // validateBoundary
    for xx in 0..16 {
        for zz in 0..16 {
            for yy in 0..8 {
                if !boundary(&g, xx, yy, zz) {
                    continue;
                }
                let (x, y, z) = at(xx, yy, zz);
                let v = r.get(x, y, z);
                if yy >= 4 && blocks::is_liquid(v) || yy < 4 && !blocks::is_solid(v) && v != water || blocks::is(v, "sulfur_spike") {
                    return false;
                }
            }
        }
    }
    // carveLake
    for xx in 0..16 {
        for zz in 0..16 {
            for yy in 0..8 {
                let (x, y, z) = at(xx, yy, zz);
                if g[idx(xx, yy, zz)] && !not_feature_replaceable(r.get(x, y, z)) {
                    r.set(x, y, z, if yy >= 4 { air } else { water });
                }
            }
        }
    }
    // placeBarrier
    for xx in 0..16 {
        for zz in 0..16 {
            for yy in 0..8 {
                if !boundary(&g, xx, yy, zz) || yy >= 4 && rand.next_int_below(2) == 0 {
                    continue;
                }
                let (x, y, z) = at(xx, yy, zz);
                let v = r.get(x, y, z);
                if blocks::is_solid(v) && !not_feature_replaceable(v) {
                    r.set(x, y, z, sulfur);
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::super::super::{blocks, Region, Xoroshiro};
    use crate::overworld::Chunk;

    #[test]
    fn sulfur_pool_lake_runs() {
        let stone = blocks::id("stone");
        let chunks = (0..9)
            .map(|_| {
                let mut c = Chunk::new();
                for x in 0..16 {
                    for z in 0..16 {
                        for y in -64..20 {
                            c.set(x, y, z, stone);
                        }
                    }
                }
                c
            })
            .collect();
        let mut r = Region::new(0, 0, 1, chunks);
        assert!(super::place_lake(&mut r, &mut Xoroshiro::new(3), 8, 20, 8));
        super::place_potent_sulfur(&mut r, 8, 20, 8);
        let mut ctx = super::super::super::Ctx { cx: 0, cz: 0, seed: 1, random: Xoroshiro::new(0) };
        super::apply(&mut r, &mut ctx);
    }
}

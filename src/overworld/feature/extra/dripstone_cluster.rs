// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Dripstone blocks and pointed dripstone on dripstone cave floors and ceilings,
//! from PNX `DripstoneClusterFeature`.

use super::super::blocks::{self, b, is_air, s};
use super::super::decoration::noise::SimplexF;
use super::super::decoration::placement::MIN_Y;
use super::super::decoration::water;
use super::super::{chunk_hash, Ctx, FeatureFn, Region, Xoroshiro};
use crate::overworld::biome::id::DRIPSTONE_CAVES;
use std::collections::HashMap;

pub fn register(m: &mut HashMap<&'static str, FeatureFn>) {
    m.insert("minecraft:dripstone_cluster_feature", apply);
}

/// PNX `FeatureHolder.dripstoneCluster`: `SimplexF(identical(), 30, 1/99, 1/15)`.
// shortcut: rebuilt every call (~770 draws) instead of living in decoration's noise Holder, which this file may not edit.
fn noise(seed: i64) -> SimplexF {
    SimplexF::new(&mut Xoroshiro::new(seed), 30.0, 1.0 / 99.0, 1.0 / 15.0)
}

// from PNX DripstoneClusterFeature
fn apply(r: &mut Region, ctx: &mut Ctx) {
    ctx.random.set_seed(ctx.seed ^ chunk_hash(ctx.cx, ctx.cz));
    let noise = noise(ctx.seed);
    let block = blocks::id("dripstone_block");
    let rand = &mut ctx.random;
    for x in 0..16 {
        let bx = (ctx.cx << 4) + x;
        for z in 0..16 {
            let bz = (ctx.cz << 4) + z;
            if noise.noise_2d(bx as f32 * 1.25, bz as f32 * 1.25, true) as f64 <= 0.1 {
                continue;
            }
            if !(MIN_Y + 1..=r.height(bx, bz)).rev().any(|y| r.biome(bx, y, bz) == DRIPSTONE_CAVES) {
                continue;
            }
            for y in workable(r, bx, bz, 1) {
                let depth = depth(&noise, bx, y, bz);
                let mut placed_water = false;
                for i in 0..depth {
                    if i == 0 && rand.next_float() < 0.002 {
                        let open = [(0, -1), (1, 0), (0, 1), (-1, 0)].iter().any(|&(dx, dz)| is_air(r.get(bx + dx, y, bz + dz)));
                        if !open {
                            r.set(bx, y, bz, water());
                            placed_water = true;
                        }
                    } else {
                        r.set(bx, y - i, bz, block);
                    }
                }
                if !placed_water && (rand.next_float() as f64) < 0.1 {
                    let len = rand.next_int_range(1, 3);
                    pointed(r, bx, y + 1, bz, false, len);
                }
            }
            for y in workable(r, bx, bz, -1) {
                for i in 0..depth(&noise, bx, y, bz) {
                    r.set(bx, y + i, bz, block);
                }
                if (rand.next_float() as f64) < 0.3 {
                    let len = rand.next_int_range(1, 5);
                    pointed(r, bx, y - 1, bz, true, len);
                }
            }
        }
    }
}

/// `(int) clamp(remapFromNormalized(noise3D(x, y, z, true), 1, 3), 1, 2)`.
fn depth(n: &SimplexF, x: i32, y: i32, z: i32) -> i32 {
    let v = n.noise_3d(x as f32, y as f32, z as f32, true);
    let v = 1.0 + (v - -1.0) / (1.0 - -1.0) * (3.0 - 1.0);
    (if v < 1.0 { 1.0 } else { v.min(2.0) }) as i32
}

/// `getHighestWorkableBlocks` (`dy` 1: air above) / `getLowestWorkableBlocks` (`dy` -1: air below).
/// Collected up front, as PNX reads the chunk before the manager's writes land.
fn workable(r: &Region, x: i32, z: i32, dy: i32) -> Vec<i32> {
    let air = blocks::air();
    (MIN_Y + 1..=r.height(x, z))
        .rev()
        .filter(|&y| r.biome(x, y, z) == DRIPSTONE_CAVES && matches!(blocks::short(r.get(x, y, z)), "stone" | "deepslate") && r.get(x, y + dy, z) == air)
        .collect()
}

fn pointed(r: &mut Region, x: i32, y: i32, z: i32, hanging: bool, max_len: i32) {
    let step = if hanging { -1 } else { 1 };
    let mut planned = Vec::new();
    let mut merge = None;
    for i in 0..max_len {
        let cy = y + i * step;
        let cur = r.get(x, cy, z);
        if blocks::short(cur) == "pointed_dripstone" {
            let cur_hanging = matches!(crate::blocks::state(cur, "hanging"), Some(crate::blocks::State::Bool(true)));
            if cur_hanging != hanging && !planned.is_empty() {
                merge = Some(cy);
            }
            break;
        }
        if !is_air(cur) {
            break;
        }
        planned.push(cy);
    }
    if planned.is_empty() {
        return;
    }
    let last = planned.len() - 1;
    for (i, &py) in planned.iter().enumerate() {
        let t = if i == last {
            if merge.is_some() { "merge" } else { "tip" }
        } else if i == 0 && last >= 2 {
            "base"
        } else if i == last - 1 {
            "frustum"
        } else {
            "middle"
        };
        r.set(x, py, z, state(hanging, t));
    }
    if let Some(my) = merge {
        r.set(x, my, z, state(!hanging, "merge"));
    }
}

fn state(hanging: bool, thickness: &str) -> i32 {
    blocks::with(blocks::id("pointed_dripstone"), &[("hanging", b(hanging)), ("dripstone_thickness", s(thickness))])
}

#[cfg(test)]
mod tests {
    use super::super::super::{blocks, Ctx, Region, Xoroshiro};
    use crate::overworld::Chunk;

    #[test]
    fn pointed_dripstone_shapes() {
        let mut r = Region::new(0, 0, 1, (0..9).map(|_| Chunk::new()).collect());
        super::pointed(&mut r, 0, 10, 0, true, 4);
        let tip = super::state(true, "tip");
        assert_eq!(r.get(0, 7, 0), tip);
        assert_eq!(r.get(0, 10, 0), super::state(true, "base"));
        assert!(blocks::is_air(r.get(0, 6, 0)));
        // and the whole feature runs
        super::apply(&mut r, &mut Ctx { cx: 0, cz: 0, seed: 1, random: Xoroshiro::new(0) });
    }
}

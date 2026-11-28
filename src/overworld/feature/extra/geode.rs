// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Amethyst geodes, from PNX `AmethystGeodeFeature`.

use super::super::blocks::{self, s};
use super::super::decoration::placement::{MAX_Y, MIN_Y};
use super::super::{Ctx, FeatureFn, Region};
use std::collections::HashMap;

pub fn register(m: &mut HashMap<&'static str, FeatureFn>) {
    m.insert("minecraft:overworld_amethyst_geode_feature", apply);
    m.insert("minecraft:amethyst_geode_feature", apply);
}

// from PNX AmethystGeodeFeature
fn apply(r: &mut Region, ctx: &mut Ctx) {
    ctx.random.set_seed(ctx.seed ^ (((ctx.cx as i64) << 8) ^ ctx.cz as i64));
    let rand = &mut ctx.random;
    if rand.next_bounded_int(24) != 0 {
        return;
    }
    let [basalt, calcite, amethyst, budding, cluster] = ["smooth_basalt", "calcite", "amethyst_block", "budding_amethyst", "amethyst_cluster"].map(blocks::id);
    let air = blocks::air();
    let center_x = rand.next_bounded_int(16);
    let center_z = rand.next_bounded_int(16);
    let outer = 4 + rand.next_bounded_int(5);
    let outer_sq = (outer * outer) as f64;
    let has_crack = rand.next_bounded_int(100) < 95;
    // TODO: Java Math.cos/sin may differ from Rust's by an ulp; not observable at these thresholds in practice.
    let angle = rand.next_bounded_int(360) as f64 * std::f64::consts::PI / 180.0;
    let (dir_x, dir_z) = (angle.cos(), angle.sin());

    let min_center = MIN_Y.max(-58) + outer;
    let max_center = (MAX_Y - 1).min(30) - outer;
    if min_center > max_center {
        return;
    }
    let center_y = min_center + rand.next_bounded_int(max_center - min_center + 1);
    let shell = |r: &Region, x: i32, y: i32, z: i32| (MIN_Y..MAX_Y).contains(&y) && matches!(r.get(x, y, z), b if b == amethyst || b == budding);

    for lx in center_x - outer..=center_x + outer {
        for lz in center_z - outer..=center_z + outer {
            for y in center_y - outer..=center_y + outer {
                let wx = (ctx.cx << 4) + lx;
                let wz = (ctx.cz << 4) + lz;
                let dx = (lx - center_x) as f64 + 0.5;
                let dz = (lz - center_z) as f64 + 0.5;
                let dy = (y - center_y) as f64 + 0.5;
                let dist_sq = dx * dx + dy * dy + dz * dz;
                if dist_sq > outer_sq {
                    continue;
                }
                if has_crack {
                    let along = dx * dir_x + dz * dir_z;
                    let side = (dx * -dir_z + dz * dir_x).abs();
                    if along > 0.0 && along <= outer as f64 + 1.0 && side <= 1.5 && dy.abs() <= 2.0 {
                        r.set(wx, y, wz, air);
                        continue;
                    }
                }
                let from_outer = outer as f64 - dist_sq.sqrt();
                if from_outer <= 0.5 {
                    r.set(wx, y, wz, basalt);
                } else if from_outer <= 1.5 {
                    r.set(wx, y, wz, calcite);
                } else if from_outer <= 2.5 {
                    r.set(wx, y, wz, amethyst);
                    if rand.next_bounded_int(1000) < 83 {
                        r.set(wx, y, wz, budding);
                    }
                } else {
                    r.set(wx, y, wz, air);
                    // later checks win, as in PNX
                    let mut face = None;
                    for (ox, oy, oz, f) in [(1, 0, 0, "west"), (-1, 0, 0, "east"), (0, 1, 0, "down"), (0, -1, 0, "up"), (0, 0, 1, "north"), (0, 0, -1, "south")] {
                        if shell(r, wx + ox, y + oy, wz + oz) {
                            face = Some(f);
                        }
                    }
                    if let Some(f) = face {
                        if rand.next_bounded_int(100) < 18 {
                            r.set(wx, y, wz, blocks::with(cluster, &[("minecraft:block_face", s(f))]));
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::{blocks, Ctx, Region, Xoroshiro};
    use crate::overworld::Chunk;

    #[test]
    fn geode_places_amethyst() {
        let amethyst = blocks::id("amethyst_block");
        let mut found = false;
        for cx in 0..200 {
            let chunks = (0..9).map(|_| Chunk::new()).collect();
            let mut r = Region::new(cx, 0, 7, chunks);
            super::apply(&mut r, &mut Ctx { cx, cz: 0, seed: 7, random: Xoroshiro::new(0) });
            found |= (-64..40).any(|y| (0..16).any(|x| (0..16).any(|z| r.get((cx << 4) + x, y, z) == amethyst)));
        }
        assert!(found);
    }
}

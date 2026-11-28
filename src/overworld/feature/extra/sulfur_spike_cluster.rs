// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Discs of sulfur spikes on cave floors and ceilings, from PNX `SulfurSpikeClusterFeature`.

use std::collections::HashMap;

use super::super::blocks;
use super::super::decoration::placement::{reseed, MAX_Y, MIN_Y};
use super::super::{Ctx, FeatureFn, Region};
use super::sulfur_spike::{air_or_water, is_base, place_spike};
use crate::overworld::biome::id::SULFUR_CAVES;

const NAME: &str = "minecraft:sulfur_spike_cluster_feature";

pub fn register(m: &mut HashMap<&'static str, FeatureFn>) {
    m.insert("minecraft:sulfur_caves_after_surface_spike_cluster_feature_rules", apply);
    m.insert(NAME, apply);
}

// from PNX SulfurSpikeClusterFeature
fn apply(r: &mut Region, ctx: &mut Ctx) {
    reseed(ctx, NAME);
    let (min_y, max_y) = (MIN_Y, 256.min(MAX_Y - 1));
    let rand = &mut ctx.random;
    let attempts = rand.next_int_range(48, 96);
    for _ in 0..attempts {
        let cx = (ctx.cx << 4) + rand.next_bounded_int(15);
        let cz = (ctx.cz << 4) + rand.next_bounded_int(15);
        let cy = rand.next_int_range(min_y, max_y);
        if r.biome(cx, cy, cz) != SULFUR_CAVES {
            continue;
        }
        let radius = rand.next_int_range(2, 8);
        let density = 0.3 + rand.next_float() * 0.4;
        let max_h = rand.next_int_range(1, 4);
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                if dx * dx + dz * dz > radius * radius || rand.next_float() > density {
                    continue;
                }
                let (x, z) = (cx + dx, cz + dz);
                if x >> 4 != ctx.cx || z >> 4 != ctx.cz {
                    continue;
                }
                let hanging = !rand.next_boolean();
                let step = if hanging { 1 } else { -1 };
                for i in 0..=12 {
                    let sy = cy + i * step;
                    let support = r.get(x, sy, z);
                    if is_base(support) {
                        // nextRangeSafe(1, max): no draw when max <= 1
                        let len = if max_h <= 1 { 1 } else { rand.next_int_range(1, max_h) };
                        place_spike(r, x, sy - step, z, hanging, len);
                        break;
                    }
                    // the cluster's isAirOrWater also passes through spikes
                    if !(air_or_water(support) || blocks::is(support, "sulfur_spike")) {
                        break;
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
    fn sulfur_spike_cluster_runs() {
        let stone = blocks::id("stone");
        let chunks = (0..9)
            .map(|_| {
                let mut c = Chunk::new();
                for x in 0..16 {
                    for z in 0..16 {
                        for y in -64..60 {
                            if y % 9 != 0 {
                                c.set(x, y, z, stone);
                            }
                        }
                    }
                }
                c
            })
            .collect();
        let mut r = Region::new(0, 0, 1, chunks);
        super::apply(&mut r, &mut Ctx { cx: 0, cz: 0, seed: 1, random: Xoroshiro::new(0) });
    }
}

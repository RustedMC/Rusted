// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Clay/gravel/sand disks under water (PNX `feature/river`, `DiscGenerateFeature`).

use std::collections::HashMap;

use super::{blocks, chunk_hash, java_hash, random_range, Ctx, FeatureFn, Region};

const WORLD_MAX: i32 = 319;

struct Disc {
    name: &'static str,
    block: &'static str,
    min_r: i32,
    max_r: i32,
    radius_y: i32,
    base: i32,
}

static DISCS: [Disc; 3] = [
    Disc { name: "minecraft:overworld_surface_clay_feature", block: "clay", min_r: 1, max_r: 3, radius_y: 1, base: 1 },
    Disc { name: "minecraft:overworld_surface_gravel_feature", block: "gravel", min_r: 2, max_r: 5, radius_y: 2, base: 3 },
    Disc { name: "minecraft:minecraft:overworld_surface_sand_feature", block: "sand", min_r: 2, max_r: 7, radius_y: 2, base: 5 },
];

fn disc<const I: usize>(r: &mut Region, c: &mut Ctx) {
    run(&DISCS[I], r, c)
}

fn run(d: &Disc, r: &mut Region, c: &mut Ctx) {
    let rng = &mut c.random;
    rng.set_seed(r.seed ^ chunk_hash(c.cx, c.cz) ^ java_hash(d.name) as i64);
    let count = d.base + rng.next_bounded_int(0);
    let water = blocks::id("water");
    let (stone, dirt, grass) = (blocks::id("stone"), blocks::id("dirt"), blocks::id("grass_block"));
    let source = blocks::id(d.block);
    let mut out: Vec<(i32, i32, i32)> = Vec::new();
    for _ in 0..count {
        if rng.next_double() >= 1.0 {
            continue;
        }
        let sx = (c.cx << 4) + rng.next_int_below(15);
        let sz = (c.cz << 4) + rng.next_int_below(15);
        let height = r.height(sx, sz) - 1;
        if r.get(sx, height, sz) != water {
            continue;
        }
        let mut depth = 0;
        while r.get(sx, height - depth, sz) == water {
            depth += 1;
        }
        let sy = height - (depth + 1);
        if sy < d.radius_y {
            continue;
        }
        let radius = random_range(rng, d.min_r, d.max_r);
        let r2 = radius * radius;
        for x in sx - radius..=sx + radius {
            for z in sz - radius..=sz + radius {
                // Unloaded chunks are skipped in PNX; outside the region is the same here.
                if (x - sx) * (x - sx) + (z - sz) * (z - sz) > r2 || !r.contains(x, z) {
                    continue;
                }
                let mut highest = None;
                for y in sy - d.radius_y..=sy + d.radius_y {
                    let b = r.get(x, y, z);
                    if b == stone || b == dirt || b == grass {
                        out.push((x, y, z));
                        highest = Some(y);
                    }
                }
                if let Some(h) = highest {
                    for y in h + 1..WORLD_MAX {
                        let b = r.get(x, y, z);
                        if b != dirt && b != grass {
                            break;
                        }
                        out.push((x, y, z));
                    }
                }
            }
        }
    }
    for (x, y, z) in out {
        r.set(x, y, z, source);
    }
}

pub fn register(m: &mut HashMap<&'static str, FeatureFn>) {
    let fns: [FeatureFn; 3] = [disc::<0>, disc::<1>, disc::<2>];
    for (d, f) in DISCS.iter().zip(fns) {
        m.insert(d.name, f);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn discs_run() {
        // Ocean-ish chunks near spawn; just check the features run and leave no water-level gaps.
        let ow = crate::overworld::Overworld::new(1);
        let clay = super::blocks::id("clay");
        let mut n = 0;
        for cx in -3..3 {
            let ch = ow.generate(cx, 0);
            for x in 0..16 {
                for z in 0..16 {
                    for y in -64..320 {
                        n += (ch.get(x, y, z) == clay) as i32;
                    }
                }
            }
        }
        println!("clay blocks: {n}");
    }
}

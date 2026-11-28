// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! from PNX AzaleaRootSystemSnapToCeilingFeature: an azalea tree on the surface above a lush
//! cave ceiling, a rooted dirt column down to the cave and hanging roots under it.

use super::super::blocks::{self, is_lava, is_solid};
use super::super::decoration::{biome, noise};
use super::super::objects::Azalea;
use super::super::{chunk_hash, java_hash, Ctx, FeatureFn, Region};
use std::collections::HashMap;

const NAME: &str = "minecraft:azalea_root_system_snap_to_ceiling_feature";
const ROOT_COLUMN_MAX_HEIGHT: i32 = 96;
const MIN_TREE_XZ_DISTANCE: i32 = 5;

pub fn register(m: &mut HashMap<&'static str, FeatureFn>) {
    m.insert("minecraft:lush_caves_after_surface_azalea_root_system_feature", apply);
    m.insert(NAME, apply);
}

fn apply(r: &mut Region, ctx: &mut Ctx) {
    ctx.random.set_seed(ctx.seed ^ chunk_hash(ctx.cx, ctx.cz) ^ java_hash(NAME) as i64);
    let h = noise::holder(ctx.seed);
    let air = blocks::air();
    for x in 0..16 {
        let wx = (ctx.cx << 4) + x;
        for z in 0..16 {
            let wz = (ctx.cz << 4) + z;
            if h.moss_ceiling.noise_2d(wx as f32 * 0.25, wz as f32 * 0.25, true) <= 0.6 {
                continue;
            }
            let surface = r.height(wx, wz) - 1;
            let mut y = surface;
            while y > -64 + 8 {
                if r.biome(wx, y, wz) == biome::LUSH_CAVES && r.get(wx, y, wz) == air && is_solid(r.get(wx, y + 1, wz)) && place_dirt_and_tree(r, ctx, wx, y, wz, surface) {
                    hanging_roots(r, ctx, wx, y, wz);
                    break;
                }
                y -= 1;
            }
        }
    }
}

fn place_dirt_and_tree(r: &mut Region, ctx: &mut Ctx, ox: i32, oy: i32, oz: i32, surface: i32) -> bool {
    let air = blocks::air();
    let mut tree_y = (oy + ROOT_COLUMN_MAX_HEIGHT).min(surface);
    loop {
        if tree_y <= oy {
            return false;
        }
        if r.get(ox, tree_y, oz) == air && (1..=4).all(|i| r.get(ox, tree_y + i, oz) == air) && !nearby_tree(r, ox, tree_y, oz) {
            break;
        }
        tree_y -= 1;
    }
    let below = r.get(ox, tree_y - 1, oz);
    if is_lava(below) || !is_solid(below) {
        return false;
    }
    if !Azalea.generate(r, &mut ctx.random, ox, tree_y, oz) {
        return false;
    }
    let rooted = blocks::id("dirt_with_roots");
    for y in (oy..=tree_y - 1).rev() {
        for _ in 0..18 {
            let x = ox + ctx.random.next_int_below(3) - ctx.random.next_int_below(3);
            let z = oz + ctx.random.next_int_below(3) - ctx.random.next_int_below(3);
            if matches!(blocks::short(r.get(x, y, z)), "stone" | "deepslate" | "tuff" | "dirt" | "clay" | "moss_block" | "dirt_with_roots") {
                r.set(x, y, z, rooted);
            }
        }
    }
    true
}

fn hanging_roots(r: &mut Region, ctx: &mut Ctx, ox: i32, oy: i32, oz: i32) {
    let roots = blocks::id("hanging_roots");
    let rand = &mut ctx.random;
    for _ in 0..32 {
        let x = ox + rand.next_int_below(5) - rand.next_int_below(5);
        let y = oy + rand.next_int_below(4) - rand.next_int_below(4);
        let z = oz + rand.next_int_below(5) - rand.next_int_below(5);
        if r.get(x, y, z) == blocks::air() && is_solid(r.get(x, y + 1, z)) {
            r.set(x, y, z, roots);
        }
    }
}

fn nearby_tree(r: &Region, cx: i32, cy: i32, cz: i32) -> bool {
    let d = MIN_TREE_XZ_DISTANCE;
    for dx in -d..=d {
        for dz in -d..=d {
            if dx == 0 && dz == 0 {
                continue;
            }
            for dy in -2..=10 {
                let n = blocks::name(r.get(cx + dx, cy + dy, cz + dz));
                if ["log", "wood", "leaves", "azalea"].iter().any(|t| n.contains(t)) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn registers_both_names() {
        let mut m = std::collections::HashMap::new();
        super::register(&mut m);
        assert_eq!(m.len(), 2);
    }
}

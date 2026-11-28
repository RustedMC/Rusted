// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Sulfur root columns from a cave ceiling down to a level floor, with a tuff cover, from PNX
//! `SulfurSpringTrailToSurfaceSnapToCeilingFeature`.

use std::collections::HashMap;

use super::super::blocks;
use super::super::decoration::placement::{reseed, MAX_Y, MIN_Y};
use super::super::{Ctx, FeatureFn, Region, Xoroshiro};

const NAME: &str = "minecraft:sulfur_spring_trail_to_surface_snap_to_ceiling_feature";

const REQUIRED_VERTICAL_SPACE_FOR_TREE: i32 = 5;
const LEVEL_TEST_DISTANCE: i32 = 8;
const MAX_LEVEL_DEVIATION: i32 = 2;
const ROOT_RADIUS: i32 = 3;
const ROOT_PLACEMENT_ATTEMPTS: i32 = 20;
const ROOT_COLUMN_MAX_HEIGHT: i32 = 184;
const HANGING_ROOT_PLACEMENT_ATTEMPTS: i32 = 1;
const HANGING_ROOT_RADIUS: i32 = 1;
const HANGING_ROOT_VERTICAL_SPAN: i32 = 1;
const ALLOWED_VERTICAL_WATER_FOR_TREE: i32 = 1;
const MAX_CEILING_SCAN_STEPS: i32 = 12;
const TEMPLATE_Y_OFFSET: i32 = -7;

/// (weight, tuff count, tuff spread, template count).
const VARIANTS: [(i32, i32, i32, i32); 4] = [(200, 64, 7, 4), (90, 80, 8, 3), (20, 96, 9, 2), (5, 128, 10, 1)];

pub fn register(m: &mut HashMap<&'static str, FeatureFn>) {
    m.insert("minecraft:sulfur_caves_before_surface_sulfur_spring_trail_to_surface_feature_rules", apply);
    m.insert(NAME, apply);
}

// from PNX SulfurSpringTrailToSurfaceSnapToCeilingFeature
fn apply(r: &mut Region, ctx: &mut Ctx) {
    reseed(ctx, NAME);
    let (min_y, max_y) = (MIN_Y, 256.min(MAX_Y - 1));
    let rand = &mut ctx.random;
    let count = rand.next_int_range(1, 2);
    for _ in 0..count {
        let x = (ctx.cx << 4) + rand.next_bounded_int(15);
        let z = (ctx.cz << 4) + rand.next_bounded_int(15);
        let y = rand.next_int_range(min_y, max_y);
        let Some(origin_y) = find_ceiling_anchor(r, x, y, z, min_y, max_y) else { continue };
        if origin_y < min_y || origin_y > max_y {
            continue;
        }
        place_rooted_spring(r, rand, x, origin_y, z, min_y, max_y);
    }
}

fn find_ceiling_anchor(r: &Region, x: i32, y: i32, z: i32, min_y: i32, max_y: i32) -> Option<i32> {
    let mut step = 0;
    while step <= MAX_CEILING_SCAN_STEPS && y + step <= max_y {
        let v = r.get(x, y + step, z);
        if blocks::is_solid(v) {
            let anchor = y + step - 1;
            return (anchor >= min_y && blocks::is_air(r.get(x, anchor, z))).then_some(anchor);
        }
        if !blocks::is_air(v) {
            return None;
        }
        step += 1;
    }
    None
}

fn place_rooted_spring(r: &mut Region, rand: &mut Xoroshiro, ox: i32, oy: i32, oz: i32, min_y: i32, max_y: i32) -> bool {
    if !blocks::is_air(r.get(ox, oy, oz)) {
        return false;
    }
    let Some(chosen) = find_allowed_tree_position(r, ox, oy, oz, min_y, max_y) else { return false };
    place_sulfur_spring(r, rand, ox, chosen, oz, min_y);
    place_root_column(r, rand, ox, oz, oy, chosen);
    place_hanging_roots(r, rand, ox, oy, oz);
    true
}

fn find_allowed_tree_position(r: &Region, x: i32, oy: i32, z: i32, min_y: i32, max_y: i32) -> Option<i32> {
    let end = (oy + ROOT_COLUMN_MAX_HEIGHT).min(max_y);
    (oy.max(min_y + 1)..=end).find(|&y| blocks::is_air(r.get(x, y, z)) && vertical_space(r, x, y, z) && level_test(r, x, y, z) && solid_non_lava_below(r, x, y, z))
}

fn place_root_column(r: &mut Region, rand: &mut Xoroshiro, ox: i32, oz: i32, start: i32, end: i32) {
    let sulfur = blocks::id("sulfur");
    for y in start.min(end)..=start.max(end) {
        for _ in 0..ROOT_PLACEMENT_ATTEMPTS {
            let x = ox + triangle(rand, ROOT_RADIUS);
            let z = oz + triangle(rand, ROOT_RADIUS);
            if matches!(blocks::short(r.get(x, y, z)), "stone" | "deepslate" | "tuff" | "dirt" | "clay" | "moss_block" | "dirt_with_roots" | "sulfur" | "cinnabar") {
                r.set(x, y, z, sulfur);
            }
        }
    }
}

fn place_hanging_roots(r: &mut Region, rand: &mut Xoroshiro, ox: i32, oy: i32, oz: i32) {
    for _ in 0..HANGING_ROOT_PLACEMENT_ATTEMPTS {
        let x = ox + triangle(rand, HANGING_ROOT_RADIUS);
        let y = oy + triangle(rand, HANGING_ROOT_VERTICAL_SPAN);
        let z = oz + triangle(rand, HANGING_ROOT_RADIUS);
        if blocks::is_air(r.get(x, y, z)) && blocks::is_solid(r.get(x, y + 1, z)) {
            r.set(x, y, z, blocks::id("sulfur"));
        }
    }
}

fn place_sulfur_spring(r: &mut Region, rand: &mut Xoroshiro, ox: i32, oy: i32, oz: i32, min_y: i32) {
    // pickVariant: PNX draws nextInt(totalWeight - 1)
    let total: i32 = VARIANTS.iter().map(|v| v.0).sum();
    let mut value = rand.next_int_below(total - 1);
    let mut variant = VARIANTS[0];
    for v in VARIANTS {
        value -= v.0;
        if value < 0 {
            variant = v;
            break;
        }
    }
    let (_, tuff_count, spread, templates) = variant;
    let tuff = blocks::id("tuff");
    for _ in 0..tuff_count {
        let x = ox + triangle(rand, spread);
        let y = oy + triangle(rand, 3);
        let z = oz + triangle(rand, spread);
        // scanDownToSolid(.., 4); the hit is solid already, PNX's second isSolid check is a no-op
        if let Some(ty) = (0..=4).map(|s| y - s).take_while(|&sy| sy >= min_y).find(|&sy| blocks::is_solid(r.get(x, sy, z))) {
            r.set(x, ty, z, tuff);
        }
    }
    if oy + TEMPLATE_Y_OFFSET >= min_y {
        // Template index draw (PNX: nextInt(templates.length - 1)).
        rand.next_int_below(templates - 1);
        // TODO: the "spring/sulfur_spring_*" templates are not in PNX's repo nor in
        // data/structures.nbt, so PNX's Registries.STRUCTURE.get returns null and places nothing
        // (no rotation draw either). Place them here if the templates ever show up.
    }
}

/// `nextInt(spread) - nextInt(spread)`.
fn triangle(rand: &mut Xoroshiro, spread: i32) -> i32 {
    let a = rand.next_int_below(spread);
    a - rand.next_int_below(spread)
}

fn vertical_space(r: &Region, x: i32, y: i32, z: i32) -> bool {
    let mut water = 0;
    for i in 1..=REQUIRED_VERTICAL_SPACE_FOR_TREE {
        let v = r.get(x, y + i, z);
        if blocks::is_air(v) {
            continue;
        }
        if blocks::is_water(v) {
            water += 1;
            if water <= ALLOWED_VERTICAL_WATER_FOR_TREE {
                continue;
            }
        }
        return false;
    }
    true
}

fn level_test(r: &Region, x: i32, y: i32, z: i32) -> bool {
    let base = y - 1;
    let d = LEVEL_TEST_DISTANCE;
    [(d, 0), (-d, 0), (0, d), (0, -d)].iter().all(|&(dx, dz)| (top_solid_y(r, x + dx, base, z + dz) - base).abs() <= MAX_LEVEL_DEVIATION)
}

fn top_solid_y(r: &Region, x: i32, start: i32, z: i32) -> i32 {
    for off in 0..=MAX_LEVEL_DEVIATION {
        if blocks::is_solid(r.get(x, start + off, z)) {
            return start + off;
        }
        if blocks::is_solid(r.get(x, start - off, z)) {
            return start - off;
        }
    }
    i32::MIN / 2
}

fn solid_non_lava_below(r: &Region, x: i32, y: i32, z: i32) -> bool {
    let v = r.get(x, y - 1, z);
    blocks::is_solid(v) && !blocks::is_lava(v)
}

#[cfg(test)]
mod tests {
    use super::super::super::{blocks, Ctx, Region, Xoroshiro};
    use crate::overworld::Chunk;

    #[test]
    fn sulfur_spring_trail_runs() {
        let stone = blocks::id("stone");
        // floor up to y 0, open cave 1..=10, ceiling above
        let chunks = (0..9)
            .map(|_| {
                let mut c = Chunk::new();
                for x in 0..16 {
                    for z in 0..16 {
                        for y in (-64..=0).chain(11..40) {
                            c.set(x, y, z, stone);
                        }
                    }
                }
                c
            })
            .collect();
        let mut r = Region::new(0, 0, 1, chunks);
        let mut rand = Xoroshiro::new(5);
        assert_eq!(super::find_ceiling_anchor(&r, 8, 3, 8, -64, 256), Some(10));
        assert!(super::place_rooted_spring(&mut r, &mut rand, 8, 10, 8, -64, 256));
        super::apply(&mut r, &mut Ctx { cx: 0, cz: 0, seed: 1, random: Xoroshiro::new(0) });
    }
}

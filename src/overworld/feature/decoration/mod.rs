// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Plant/decoration features (PNX `feature/decoration`, `feature/multi`) under their PNX names.
//! Region only has one view, so a feature can see blocks PNX wouldn't. No waterlogging either.

mod ice;
mod lush;
pub(crate) mod noise;
pub(crate) mod placement;
mod water;

use std::collections::{HashMap, HashSet};

use super::blocks::{self, b, i, s};
use super::{chunk_hash, random_range, Ctx, FeatureFn, NukkitRandom, Region, Xoroshiro};
use placement::{reseed, Coord, Order, PlaceCtx, Scatter, Vegetation, MAX_Y, MIN_Y};

pub(crate) const SEA_LEVEL: i32 = 63;

/// Bedrock biome ids PNX features compare against.
pub(crate) mod biome {
    pub const LEGACY_FROZEN_OCEAN: u16 = 10;
    pub const MUSHROOM_ISLAND: u16 = 14;
    pub const FROZEN_OCEAN: u16 = 46;
    pub const DEEP_FROZEN_OCEAN: u16 = 47;
    pub const ICE_PLAINS_SPIKES: u16 = 140;
    pub const LUSH_CAVES: u16 = 187;
    pub const CHERRY_GROVE: u16 = 192;
    pub const PALE_GARDEN: u16 = 193;
    pub const DAPPLED_FOREST: u16 = 195;

    /// Vanilla biome tags PNX `BiomeTags` checks, by id.
    pub fn taiga(b: u16) -> bool {
        matches!(b, 5 | 19 | 30 | 31 | 32 | 33 | 133 | 158 | 160 | 161)
    }
    pub fn birch_forest_not_hills(b: u16) -> bool {
        matches!(b, 27 | 155)
    }
    pub fn bamboo(b: u16) -> bool {
        matches!(b, 48 | 49)
    }
    pub fn mooshroom(b: u16) -> bool {
        matches!(b, 14 | 15)
    }
}

pub(crate) fn water() -> i32 {
    crate::overworld::ids().water
}

/// Lower or upper half of a double plant.
fn double(name: &str, upper: bool) -> i32 {
    blocks::with(blocks::id(name), &[("upper_block_bit", b(upper))])
}

/// `BlockTags.DIRT`.
pub(crate) fn dirt(bl: i32) -> bool {
    blocks::is_dirt(bl)
}

/// `BlockTags.SAND`.
pub(crate) fn sand(bl: i32) -> bool {
    matches!(blocks::short(bl), "sand" | "red_sand" | "suspicious_sand")
}

/// `BlockTags.GRASS` / `Supportable.isSupportGrass`.
pub(crate) fn grass(bl: i32) -> bool {
    blocks::is(bl, "grass_block")
}

/// `BlockReeds.isSupportValid`: sugar cane below, or dirt/sand next to water.
fn reeds_support(r: &Region, x: i32, y: i32, z: i32) -> bool {
    let bl = r.get(x, y, z);
    if matches!(blocks::short(bl), "reeds" | "sugar_cane") {
        return true;
    }
    (dirt(bl) || sand(bl)) && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dz)| blocks::is_water(r.get(x + dx, y, z + dz)) || blocks::is(r.get(x + dx, y, z + dz), "frosted_ice"))
}

/// `MinecraftCardinalDirection.values()` order.
const DIRS: [&str; 4] = ["south", "west", "north", "east"];

/// `CountGenerateFeature`: reseed with the name and roll the iteration count.
fn count(ctx: &mut Ctx, name: &str, base: i32, rnd: i32) -> i32 {
    reseed(ctx, name);
    base + ctx.random.next_bounded_int(rnd)
}

type Valid = fn(&Region, i32, i32, i32) -> bool;
type Place = fn(&mut Region, &mut Xoroshiro, i32, i32, i32);

/// A `SurfaceGenerateFeature` subclass: count, support test, placement.
struct Surface {
    name: &'static str,
    base: i32,
    rnd: i32,
    valid: Valid,
    place: Place,
}

fn default_valid(r: &Region, x: i32, y: i32, z: i32) -> bool {
    let bl = r.get(x, y, z);
    dirt(bl) && !blocks::is(bl, "coarse_dirt")
}

fn set(r: &mut Region, x: i32, y: i32, z: i32, name: &str) {
    r.set(x, y, z, blocks::id(name));
}

const JUNGLE_GRASS: &str = "minecraft:jungle_tall_grass_feature";
const GRASS_DOUBLE: &str = "minecraft:grass_double_plant_patch_feature";
const REEDS: &str = "minecraft:reeds_feature";

const SURFACE: &[Surface] = &[
    Surface { name: "minecraft:scatter_bush_feature", base: 8, rnd: 0, valid: default_valid, place: |r, _, x, y, z| set(r, x, y, z, "bush") },
    Surface { name: "minecraft:jungle_after_surface_melon_feature", base: -1500, rnd: 1520, valid: default_valid, place: |r, _, x, y, z| set(r, x, y, z, "melon_block") },
    Surface { name: "minecraft:pumpkin_feature", base: -2000, rnd: 2015, valid: default_valid, place: |r, _, x, y, z| set(r, x, y, z, "pumpkin") },
    Surface {
        name: JUNGLE_GRASS,
        base: 100,
        rnd: 10,
        valid: default_valid,
        place: |r, rand, x, y, z| set(r, x, y, z, if rand.next_int_below(7) == 0 { "fern" } else { "short_grass" }),
    },
    Surface {
        name: "minecraft:scatter_sweet_berry_bush_feature",
        base: -66,
        rnd: 70,
        valid: default_valid,
        place: |r, _, x, y, z| r.set(x, y, z, blocks::with(blocks::id("sweet_berry_bush"), &[("growth", i(7))])),
    },
    Surface { name: GRASS_DOUBLE, base: 5, rnd: 0, valid: default_valid, place: |r, _, x, y, z| place_double(r, x, y, z, "tall_grass") },
    // PNX also checks the root view; with one view that is the same test.
    Surface { name: "minecraft:fern_double_plant_patch_feature", base: 5, rnd: 0, valid: default_valid, place: |r, _, x, y, z| place_double(r, x, y, z, "large_fern") },
    Surface {
        name: "minecraft:scatter_eyeblossom_feature",
        base: 10,
        rnd: 0,
        valid: |r, x, y, z| dirt(r.get(x, y, z)) && r.biome(x, y, z) == biome::PALE_GARDEN,
        place: |r, _, x, y, z| set(r, x, y, z, "closed_eyeblossom"),
    },
    Surface {
        name: "minecraft:scatter_red_shrub_bush_feature",
        base: 8,
        rnd: 0,
        valid: |r, x, y, z| default_valid(r, x, y, z) && r.biome(x, y, z) == biome::DAPPLED_FOREST,
        place: |r, _, x, y, z| set(r, x, y, z, "red_shrub"),
    },
    Surface { name: "minecraft:legacy:forest_rock_feature", base: -1, rnd: 2, valid: |r, x, y, z| dirt(r.get(x, y, z)) && biome::taiga(r.biome(x, y, z)), place: forest_rock },
    Surface { name: "minecraft:bamboo_then_podzol_feature", base: 240, rnd: 80, valid: default_valid, place: bamboo },
];

fn place_double(r: &mut Region, x: i32, y: i32, z: i32, name: &str) {
    if blocks::is_air(r.get(x, y + 1, z)) {
        r.set(x, y, z, double(name, false));
        r.set(x, y + 1, z, double(name, true));
    }
}

/// `SurfaceGenerateFeature.populate`, `count` times.
fn surface(sf: &Surface, r: &mut Region, ctx: &mut Ctx) {
    let n = count(ctx, sf.name, sf.base, sf.rnd);
    for _ in 0..n {
        let x = ctx.random.next_bounded_int(15);
        let z = ctx.random.next_bounded_int(15);
        let (wx, wz) = ((ctx.cx << 4) + x, (ctx.cz << 4) + z);
        let mut y = r.height(wx, wz);
        while !(sf.valid)(r, wx, y, wz) && y >= SEA_LEVEL - 1 {
            y -= 1;
        }
        if y >= SEA_LEVEL - 1 && (sf.valid)(r, wx, y, wz) {
            if !blocks::is_air(r.get(wx, y + 1, wz)) {
                continue;
            }
            (sf.place)(r, &mut ctx.random, wx, y + 1, wz);
        }
    }
}

/// `ForestRockFeature.place` (the `_z < sizeX` typo kept).
fn forest_rock(r: &mut Region, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) {
    rand.set_seed(x.wrapping_add(y).wrapping_add(z) as i64);
    let size_x = 3 + rand.next_int_below(1);
    let size_z = 3 + rand.next_int_below(1);
    let stone = blocks::id("mossy_cobblestone");
    for dx in 0..size_x {
        for dy in -1..=1 {
            for dz in 0..size_x {
                let corner = (dx == 0 || dx == size_x - 1) && (dz == 0 || dz == size_z - 1) && dy != 0;
                if !corner {
                    r.set(x + dx, y + dy, z + dz, stone);
                }
            }
        }
    }
}

/// `BambooForestBambooFeature.place`.
fn bamboo(r: &mut Region, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) {
    if !biome::bamboo(r.biome(x, y, z)) {
        return;
    }
    rand.set_seed(x.wrapping_add(y).wrapping_add(z) as i64);
    for n in 0..4 {
        let (mut px, mut py, mut pz) = (x, y, z);
        if n > 0 {
            px += rand.next_int_range(-3, 3);
            pz += rand.next_int_range(-3, 3);
            py = r.height(px, pz);
        }
        bamboo_at(r, rand, px, py, pz);
    }
}

fn bamboo_at(r: &mut Region, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) {
    if !blocks::is_air(r.get(x, y, z)) || !dirt(r.get(x, y - 1, z)) {
        return;
    }
    let (mid_x, mid_z) = (x + 1, z + 1);
    let rad = rand.next_int_range(0, 2);
    let podzol = blocks::id("podzol");
    for dx in -rad - 1..=rad {
        for dz in -rad - 1..=rad {
            let (cx, cz, cr) = (dx as f32 + 0.5, dz as f32 + 0.5, rad as f32 + 0.8);
            let (px, pz) = (mid_x + dx, mid_z + dz);
            if cx * cx + cz * cz < cr * cr {
                let py = r.height(px, pz) - 1;
                if dirt(r.get(px, py, pz)) {
                    r.set(px, py, pz, podzol);
                }
            }
        }
    }
    let height = 5 + rand.next_int_below(11);
    let leaf = |l: &str| blocks::with(blocks::id("bamboo"), &[("bamboo_leaf_size", s(l))]);
    for n in 0..=height {
        let diff = height - n;
        r.set(x, y + n, z, leaf(if diff < 3 { "large_leaves" } else if diff < 4 { "small_leaves" } else { "no_leaves" }));
    }
}

/// `TallGrassPatchFeature` (its own populate: mooshroom check, stricter sea-level bounds).
fn tall_grass_patch(r: &mut Region, ctx: &mut Ctx) {
    const NAME: &str = "minecraft:scatter_tall_grass_feature";
    let n = count(ctx, NAME, 10, 0);
    let (ox, oz) = (ctx.cx << 4, ctx.cz << 4);
    let grass_id = blocks::id("short_grass");
    for _ in 0..n {
        if biome::mooshroom(r.biome(ox + 7, r.height(ox + 7, oz + 7), oz + 7)) {
            return;
        }
        let x = ctx.random.next_bounded_int(15);
        let z = ctx.random.next_bounded_int(15);
        let (wx, wz) = (ox + x, oz + z);
        let mut y = r.height(wx, wz);
        while !default_valid(r, wx, y, wz) && y > SEA_LEVEL {
            y -= 1;
        }
        if y < SEA_LEVEL || !default_valid(r, wx, y, wz) || !blocks::is_air(r.get(wx, y + 1, wz)) {
            continue;
        }
        r.set(wx, y + 1, wz, grass_id);
    }
}

/// A `GroupedDiscFeature` subclass.
struct Disc {
    name: &'static str,
    base: i32,
    rnd: i32,
    min_r: i32,
    max_r: i32,
    /// `getProbability()`, a float constant widened to double in PNX.
    p: f32,
    source: fn(&Ctx, i32) -> i32,
    valid: Valid,
    /// `ScatterRedMushroomFeature.getY`: search down for a valid support under air.
    mushroom_y: bool,
}

fn disc_valid(r: &Region, x: i32, y: i32, z: i32) -> bool {
    let bl = r.get(x, y, z);
    dirt(bl) || sand(bl)
}

fn mushroom_valid(r: &Region, x: i32, y: i32, z: i32) -> bool {
    let bm = r.biome(x, y, z);
    blocks::is_solid(r.get(x, y, z)) && (r.height(x, z) - 1 != y || bm == biome::MUSHROOM_ISLAND || bm == biome::DAPPLED_FOREST)
}

fn water_dirt_valid(r: &Region, x: i32, y: i32, z: i32) -> bool {
    dirt(r.get(x, y, z)) && reeds_support(r, x, y, z)
}

const DISCS: &[Disc] = &[
    Disc { name: "minecraft:dead_bush_feature", base: -10, rnd: 12, min_r: 3, max_r: 4, p: 0.2, source: |_, _| blocks::id("deadbush"), valid: disc_valid, mushroom_y: false },
    Disc { name: "minecraft:scatter_red_mushroom_feature", base: -7, rnd: 8, min_r: 1, max_r: 2, p: 0.1, source: |_, _| blocks::id("red_mushroom"), valid: mushroom_valid, mushroom_y: true },
    Disc { name: "minecraft:scatter_brown_mushroom_feature", base: -3, rnd: 4, min_r: 1, max_r: 2, p: 0.1, source: |_, _| blocks::id("brown_mushroom"), valid: mushroom_valid, mushroom_y: true },
    Disc {
        name: "minecraft:scatter_dry_grass_feature",
        base: -10,
        rnd: 12,
        min_r: 3,
        max_r: 4,
        p: 0.4,
        // TODO: PNX uses an unseeded NukkitRandom here, we seed from the chunk so it's reproducible.
        source: |c, n| blocks::id(if NukkitRandom::new(c.seed ^ chunk_hash(c.cx, c.cz) ^ n as i64).next_bounded_int(3) == 0 { "tall_dry_grass" } else { "short_dry_grass" }),
        valid: disc_valid,
        mushroom_y: false,
    },
    Disc { name: "minecraft:firefly_bush_cluster_feature", base: -10, rnd: 12, min_r: 1, max_r: 1, p: 0.8, source: |_, _| blocks::id("firefly_bush"), valid: water_dirt_valid, mushroom_y: false },
    Disc { name: "minecraft:firefly_bush_water_cluster_feature", base: -10, rnd: 12, min_r: 3, max_r: 4, p: 0.5, source: |_, _| blocks::id("firefly_bush"), valid: water_dirt_valid, mushroom_y: false },
    Disc { name: "minecraft:scatter_swamp_flower_feature", base: -10, rnd: 12, min_r: 1, max_r: 2, p: 0.7, source: |_, _| blocks::id("blue_orchid"), valid: water_dirt_valid, mushroom_y: false },
];

fn disc_y(d: &Disc, r: &Region, x: i32, z: i32) -> i32 {
    let start = r.height(x, z) - 1;
    if d.mushroom_y {
        let mut y = start;
        while y > MIN_Y {
            if blocks::is_air(r.get(x, y + 1, z)) && (d.valid)(r, x, y, z) {
                return y;
            }
            y -= 1;
        }
    }
    start
}

/// `GroupedDiscFeature.populate`, `count` times.
fn disc(d: &Disc, r: &mut Region, ctx: &mut Ctx) {
    let n = count(ctx, d.name, d.base, d.rnd);
    let air = blocks::air();
    let p = d.p as f64;
    for it in 0..n {
        let rx = ctx.random.next_int_below(15);
        let rz = ctx.random.next_int_below(15);
        let (sx, sz) = ((ctx.cx << 4) + rx, (ctx.cz << 4) + rz);
        let height = disc_y(d, r, sx, sz);
        if height < MIN_Y || r.get(sx, height + 1, sz) != air {
            continue;
        }
        let source = (d.source)(ctx, it);
        let radius = random_range(&mut ctx.random, d.min_r, d.max_r);
        let (min_x, max_x, min_z, max_z) = (sx - radius, sx + radius, sz - radius, sz + radius);
        // Chunk by chunk like PNX, which changes the random order near borders.
        for ccx in (min_x >> 4)..=(max_x >> 4) {
            let (x0, x1) = (min_x.max(ccx << 4), max_x.min((ccx << 4) + 15));
            for ccz in (min_z >> 4)..=(max_z >> 4) {
                let (z0, z1) = (min_z.max(ccz << 4), max_z.min((ccz << 4) + 15));
                if !r.contains(ccx << 4, ccz << 4) {
                    continue;
                }
                for x in x0..=x1 {
                    for z in z0..=z1 {
                        let (dx, dz) = (x - sx, z - sz);
                        if dx * dx + dz * dz > radius * radius {
                            continue;
                        }
                        if p < 1.0 && ctx.random.next_double() >= p {
                            continue;
                        }
                        let sy = disc_y(d, r, x, z);
                        if sy < MIN_Y || r.get(x, sy + 1, z) != air {
                            continue;
                        }
                        if (d.valid)(r, x, sy, z) {
                            r.set(x, sy + 1, z, source);
                        }
                    }
                }
            }
        }
    }
}

/// `ReedsFeature`: an occasional row of sugar cane along sea-level water.
fn reeds(r: &mut Region, ctx: &mut Ctx) {
    ctx.random.set_seed(ctx.seed ^ chunk_hash(ctx.cx, ctx.cz));
    if ctx.random.next_int_below(20) != 0 {
        return;
    }
    let mut max = ctx.random.next_int_below(3);
    let cane = blocks::with(blocks::id("reeds"), &[("age", i(15))]);
    for x in 0..16 {
        for z in 0..16 {
            let (wx, wz) = ((ctx.cx << 4) + x, (ctx.cz << 4) + z);
            if r.height(wx, wz) == SEA_LEVEL + 1 && reeds_support(r, wx, SEA_LEVEL, wz) {
                for n in 1..4 {
                    r.set(wx, SEA_LEVEL + n, wz, cane);
                }
                max -= 1;
                if max <= 0 {
                    return;
                }
            }
        }
    }
}

/// `PinkPetalsFeature`.
fn pink_petals(r: &mut Region, ctx: &mut Ctx) {
    ctx.random.set_seed(ctx.seed ^ chunk_hash(ctx.cx, ctx.cz));
    let base = blocks::id("pink_petals");
    for x in 0..16 {
        for z in 0..16 {
            if !ctx.random.next_boolean() {
                continue;
            }
            let (wx, wz) = ((ctx.cx << 4) + x, (ctx.cz << 4) + z);
            let y = r.height(wx, wz);
            if r.biome(wx, y, wz) == biome::CHERRY_GROVE && grass(r.get(wx, y - 1, wz)) && r.get(wx, y, wz) == blocks::air() {
                let dir = DIRS[ctx.random.next_int_below(3) as usize];
                let growth = ctx.random.next_int_below(3);
                r.set(wx, y, wz, blocks::with(base, &[("minecraft:cardinal_direction", s(dir)), ("growth", i(growth))]));
            }
        }
    }
}

/// `PaleMossPatchFeature`.
fn pale_moss(r: &mut Region, ctx: &mut Ctx) {
    ctx.random.set_seed(ctx.seed ^ chunk_hash(ctx.cx, ctx.cz));
    let (grass_block, moss, carpet) = (blocks::id("grass_block"), blocks::id("pale_moss_block"), blocks::id("pale_moss_carpet"));
    for x in 0..16 {
        for z in 0..16 {
            let (wx, wz) = ((ctx.cx << 4) + x, (ctx.cz << 4) + z);
            if r.biome(wx, SEA_LEVEL, wz) != biome::PALE_GARDEN || ctx.random.next_int_below(9) >= 7 {
                continue;
            }
            let mut y = r.height(wx, wz);
            while r.get(wx, y, wz) != grass_block && y > SEA_LEVEL {
                y -= 1;
            }
            if y > SEA_LEVEL {
                r.set(wx, y, wz, moss);
                if ctx.random.next_int_below(9) < 2 {
                    r.set(wx, y + 1, wz, carpet);
                }
            }
        }
    }
}

/// `DesertCactusFeature`.
fn cactus(r: &mut Region, ctx: &mut Ctx) {
    let n = count(ctx, "minecraft:desert_after_surface_cactus_feature_rules", 2, 0);
    let c = blocks::id("cactus");
    for _ in 0..n {
        let x = ctx.random.next_bounded_int(13) + 1;
        let z = ctx.random.next_bounded_int(13) + 1;
        let (wx, wz) = ((ctx.cx << 4) + x, (ctx.cz << 4) + z);
        let y = r.height(wx, wz);
        let range = ctx.random.next_bounded_int(18);
        let height = if range >= 16 { 3 } else if range >= 11 { 2 } else { 1 };
        if sand(r.get(wx, y - 1, wz)) && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().all(|(dx, dz)| blocks::replaceable(r.get(wx + dx, y, wz + dz))) && y > 0 {
            for n in 0..=height {
                r.set(wx, y + n, wz, c);
            }
            if ctx.random.next_boolean() {
                set(r, wx, y + height + 1, wz, "cactus_flower");
            }
        }
    }
}

/// `SunflowerDouplePlantPatchFeature` and `ScatterPlainsFlowerFeature`. `pick` places the plant.
fn flower_patch(r: &mut Region, ctx: &mut Ctx, name: &str, base: i32, rnd: i32, chance: f32, pick: fn(&mut Region, &mut Xoroshiro, i32, i32, i32)) {
    let n = count(ctx, name, base, rnd);
    for _ in 0..n {
        let sx = (ctx.cx << 4) + ctx.random.next_int_below(15);
        let sz = (ctx.cz << 4) + ctx.random.next_int_below(15);
        let radius = random_range(&mut ctx.random, 2, 3);
        for x in sx - radius..=sx + radius {
            for z in sz - radius..=sz + radius {
                if (x - sx) * (x - sx) + (z - sz) * (z - sz) <= radius * radius && ctx.random.next_float() < chance {
                    let h = r.height(x, z) - 1;
                    if grass(r.get(x, h, z)) {
                        pick(r, &mut ctx.random, x, h + 1, z);
                    }
                }
            }
        }
    }
}

const PLAINS_FLOWERS: [&str; 9] = ["azure_bluet", "cornflower", "dandelion", "oxeye_daisy", "poppy", "orange_tulip", "pink_tulip", "red_tulip", "white_tulip"];

/// `ScatterOverworldFlowerFeature`.
fn overworld_flowers(r: &mut Region, ctx: &mut Ctx) {
    let n = count(ctx, "minecraft:scatter_overworld_flower_feature", -7, 8);
    let air = blocks::air();
    for _ in 0..n {
        let sx = (ctx.cx << 4) + ctx.random.next_int_below(15);
        let sz = (ctx.cz << 4) + ctx.random.next_int_below(15);
        let radius = random_range(&mut ctx.random, 2, 3);
        let state = blocks::id(if ctx.random.next_boolean() { "dandelion" } else { "poppy" });
        for x in sx - radius..=sx + radius {
            for z in sz - radius..=sz + radius {
                if !r.contains(x, z) {
                    continue;
                }
                let (dx, dz) = (x - sx, z - sz);
                if dx * dx + dz * dz > radius * radius || ctx.random.next_float() >= 0.2 {
                    continue;
                }
                let y = r.height(x, z) - 1;
                if r.get(x, y + 1, z) == air && dirt(r.get(x, y, z)) {
                    r.set(x, y + 1, z, state);
                }
            }
        }
    }
}

/// `FlowerForestFoliageFeature`.
fn flower_forest(r: &mut Region, ctx: &mut Ctx) {
    const SINGLE: [&str; 12] = ["", "allium", "azure_bluet", "cornflower", "dandelion", "lily_of_the_valley", "oxeye_daisy", "poppy", "orange_tulip", "pink_tulip", "red_tulip", "white_tulip"];
    let n = count(ctx, "minecraft:flower_forest_first_foliage_feature", -1, 8);
    let air = blocks::air();
    for _ in 0..n {
        let sx = (ctx.cx << 4) + ctx.random.next_int_below(15);
        let sz = (ctx.cz << 4) + ctx.random.next_int_below(15);
        let flower = ctx.random.next_int_below(14) as usize;
        let radius = random_range(&mut ctx.random, 4, 5);
        'patch: for x in sx - radius..=sx + radius {
            for z in sz - radius..=sz + radius {
                if !r.contains(x, z) {
                    break 'patch;
                }
                if (x - sx) * (x - sx) + (z - sz) * (z - sz) > radius * radius || ctx.random.next_float() >= 0.3 {
                    continue;
                }
                let height = r.height(x, z);
                let (mut depth, mut top) = (0, Some(r.get(x, height, z)));
                while let Some(t) = top {
                    if !(blocks::is_leaves(t) || t == air) {
                        break;
                    }
                    if height - depth <= MIN_Y {
                        top = None;
                        break;
                    }
                    depth += 1;
                    top = Some(r.get(x, height - depth, z));
                }
                let Some(t) = top else { continue };
                if !grass(t) {
                    continue;
                }
                let y = height - depth + 1;
                match flower {
                    1..=11 => set(r, x, y, z, SINGLE[flower]),
                    12 | 13
                        if ctx.random.next_boolean() => {
                            let name = if flower == 12 { "lilac" } else { "peony" };
                            r.set(x, y, z, double(name, false));
                            r.set(x, y + 1, z, double(name, true));
                        }
                    _ => {}
                }
            }
        }
    }
}

/// `ForestFlowerFoliageFeature` (also `ForestFoliageFeature`, which only changes the name/seed).
fn forest_double_flowers(r: &mut Region, ctx: &mut Ctx, name: &str) {
    reseed(ctx, name);
    let n = ctx.random.next_int_below(5) - 3;
    if n <= 0 {
        return;
    }
    let air = blocks::air();
    let (ox, oz) = (ctx.cx << 4, ctx.cz << 4);
    let mut occupied: HashSet<(i32, i32, i32)> = HashSet::new();
    for _ in 0..n {
        let plant = ["lilac", "rose_bush", "peony"][ctx.random.next_int_below(3) as usize];
        let (lower, upper) = (double(plant, false), double(plant, true));
        for _ in 0..5 {
            let lx = ctx.random.next_int_below(16);
            let lz = ctx.random.next_int_below(16);
            let bound = r.height(ox + lx, oz + lz) + 31;
            if bound <= 0 {
                continue;
            }
            let (cx, cy, cz) = (ox + lx, ctx.random.next_int_below(bound), oz + lz);
            let rnd = &mut ctx.random;
            for _ in 0..64 {
                let z = cz + ((rnd.next_int() & 7) - (rnd.next_int() & 7));
                let y = cy + ((rnd.next_int() & 3) - (rnd.next_int() & 3));
                let x = cx + ((rnd.next_int() & 7) - (rnd.next_int() & 7));
                if y <= MIN_Y || y + 1 >= MAX_Y {
                    continue;
                }
                if occupied.contains(&(x, y, z)) || occupied.contains(&(x, y + 1, z)) || occupied.contains(&(x, y - 1, z)) || !r.contains(x, z) {
                    continue;
                }
                if r.get(x, y, z) != air || r.get(x, y + 1, z) != air || !dirt(r.get(x, y - 1, z)) {
                    continue;
                }
                r.set(x, y, z, lower);
                r.set(x, y + 1, z, upper);
                occupied.insert((x, y, z));
                occupied.insert((x, y + 1, z));
            }
        }
    }
}

/// `BirchForestWildflowersFeature`: 3 surface candidates of a 64-try wildflower scatter.
fn birch_wildflowers(r: &mut Region, ctx: &mut Ctx) {
    let flowers = Scatter {
        iterations: 64,
        order: Order::Zyx,
        x: Coord::triangle(-6, 6),
        y: Coord::triangle(-2, 2),
        z: Coord::triangle(-6, 6),
        child: Box::new(Vegetation {
            state: |c| {
                let growth = c.random.next_exclusive_int(4);
                let dir = c.random.next_exclusive_int(4);
                blocks::with(blocks::id("wildflowers"), &[("minecraft:cardinal_direction", s(DIRS[dir as usize])), ("growth", i(growth))])
            },
            target: |c| c.get(0, 0, 0) == blocks::air(),
            survival: |c| dirt(c.get(0, -1, 0)),
        }),
    };
    placement::surface_candidates(r, ctx, "minecraft:scatter_birch_forest_wildflowers_feature", 3, |r| r.next_exclusive_int(2) == 0, &flowers, |c: &PlaceCtx| biome::birch_forest_not_hills(c.biome()));
}

/// `MultiFeatureWrapper`: runs other registered features in order (missing ones are skipped).
fn multi(names: &[&str], r: &mut Region, ctx: &mut Ctx) {
    for n in names {
        if let Some(f) = super::registry().get(n) {
            f(r, ctx);
        }
    }
}

macro_rules! table {
    ($m:ident, $run:ident, $tab:ident, [$($i:literal),*]) => {
        $( $m.insert($tab[$i].name, (|r: &mut Region, c: &mut Ctx| $run(&$tab[$i], r, c)) as FeatureFn); )*
    };
}

pub fn register(m: &mut HashMap<&'static str, FeatureFn>) {
    table!(m, surface, SURFACE, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    table!(m, disc, DISCS, [0, 1, 2, 3, 4, 5, 6]);
    m.insert("minecraft:scatter_tall_grass_feature", tall_grass_patch);
    m.insert(REEDS, reeds);
    m.insert("minecraft:cherry_grove_surface_pink_petals_feature_rules", pink_petals);
    m.insert("minecraft:pale_moss_patch_feature", pale_moss);
    m.insert("minecraft:desert_after_surface_cactus_feature_rules", cactus);
    m.insert("minecraft:sunflower_double_plant_patch_feature", |r, c| {
        flower_patch(r, c, "minecraft:sunflower_double_plant_patch_feature", -1, 3, 0.3, |r, _, x, y, z| {
            r.set(x, y, z, double("sunflower", false));
            r.set(x, y + 1, z, double("sunflower", true));
        })
    });
    m.insert("minecraft:scatter_plains_flower_feature", |r, c| {
        flower_patch(r, c, "minecraft:scatter_plains_flower_feature", -7, 8, 0.1, |r, rand, x, y, z| set(r, x, y, z, PLAINS_FLOWERS[rand.next_bounded_int(8) as usize]))
    });
    m.insert("minecraft:scatter_overworld_flower_feature", overworld_flowers);
    m.insert("minecraft:flower_forest_first_foliage_feature", flower_forest);
    m.insert("minecraft:forest_first_foliage_feature", |r, c| forest_double_flowers(r, c, "minecraft:forest_first_foliage_feature"));
    m.insert("minecraft:legacy:forest_foliage_feature", |r, c| forest_double_flowers(r, c, "minecraft:legacy:forest_foliage_feature"));
    m.insert("minecraft:scatter_birch_forest_wildflowers_feature", birch_wildflowers);
    m.insert("minecraft:scatter_jungle_plant_feature", |r, c| multi(&["minecraft:jungle_bush", JUNGLE_GRASS, GRASS_DOUBLE], r, c));
    m.insert("minecraft:legacy:swamp_foliage_feature", |r, c| multi(&[REEDS, "minecraft:huge_mushroom_feature", "minecraft:swamp_oak_tree_feature"], r, c));
    water::register(m);
    ice::register(m);
    lush::register(m);
}

#[cfg(test)]
mod tests;

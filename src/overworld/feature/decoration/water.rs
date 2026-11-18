// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Underwater and water-surface plants: seagrass, kelp, coral, sea pickles, lily pads.

use std::collections::HashMap;

use super::super::blocks::{self, i, s};
use super::super::{Ctx, FeatureFn, Region, Xoroshiro};
use super::placement::{MAX_Y, MIN_Y};
use super::{count, noise, water, SEA_LEVEL};

/// `WaterFoliageFeature.populate`, `n` times on the floor under still water.
fn water_foliage(r: &mut Region, ctx: &mut Ctx, n: i32, can_stay: fn(&Region, i32, i32, i32) -> bool, place: &mut dyn FnMut(&mut Region, &mut Xoroshiro, i32, i32, i32)) {
    let w = water();
    for _ in 0..n {
        let x = (ctx.cx << 4) + ctx.random.next_int_below(15);
        let z = (ctx.cz << 4) + ctx.random.next_int_below(15);
        let height = r.height(x, z) - 1;
        if r.get(x, height, z) != w {
            continue;
        }
        let mut depth = 0;
        while r.get(x, height - depth, z) == w {
            depth += 1;
        }
        let y = height - depth + 1;
        if y > 0 && can_stay(r, x, y, z) {
            place(r, &mut ctx.random, x, y, z);
        }
    }
}

const SEAGRASS_SEED_NAME: &str = "minecraft:river_after_surface_seagrass_feature_rules";

fn seagrass_stay(r: &Region, x: i32, y: i32, z: i32) -> bool {
    r.get(x, y, z) == water() && blocks::is_solid(r.get(x, y - 1, z))
}

/// `SeagrassRiverGenerateFeature` and subclasses (all seeded with the river feature's name).
fn seagrass(r: &mut Region, ctx: &mut Ctx, base: i32, tall: f32) {
    let n = count(ctx, SEAGRASS_SEED_NAME, base, 0);
    let grass = |t: &str| blocks::with(blocks::id("seagrass"), &[("sea_grass_type", s(t))]);
    water_foliage(r, ctx, n, seagrass_stay, &mut |r, rand, x, y, z| {
        if rand.next_double() < tall as f64 {
            if r.get(x, y + 1, z) == water() {
                r.set(x, y, z, grass("double_bot"));
                r.set(x, y + 1, z, grass("double_top"));
            }
        } else {
            r.set(x, y, z, blocks::id("seagrass"));
        }
    });
}

/// `KelpFeature`.
fn kelp(r: &mut Region, ctx: &mut Ctx) {
    let n = count(ctx, "minecraft:kelp_feature", 30, 15);
    let h = noise::holder(ctx.seed);
    let kelp_age = |a: i32| blocks::with(blocks::id("kelp"), &[("kelp_age", i(a))]);
    water_foliage(r, ctx, n, |r, x, y, z| r.get(x, y, z) == water(), &mut |r, rand, x, y, z| {
        let w = water();
        if h.kelp.value(x as f64, 0.0, z as f64) < 0.0 || r.get(x, y + 1, z) != w {
            return;
        }
        let below = r.get(x, y - 1, z);
        if !blocks::is_solid(below) && !blocks::is(below, "kelp") {
            return;
        }
        let height = rand.next_bounded_int(10) + 1;
        for hh in 0..=height {
            if r.get(x, y + hh, z) != w {
                return;
            }
            if hh == height || r.get(x, y + hh + 2, z) != w {
                let a = 20 + rand.next_bounded_int(4);
                r.set(x, y + hh, z, kelp_age(a));
                return;
            }
            r.set(x, y + hh, z, kelp_age(25));
        }
    });
}

pub(super) const CORAL: [&str; 5] = ["tube", "brain", "bubble", "fire", "horn"];

/// `BlockFace` horizontal plane order (N, E, S, W) as (dx, dz).
const FACES: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

/// `AbstractCoralFeature` state: bounds are the feature's own chunk (PNX works in chunk coords).
struct Coral {
    ox: i32,
    oz: i32,
}

fn is_water(b: i32) -> bool {
    blocks::is_water(b)
}

impl Coral {
    fn in_bounds(&self, x: i32, y: i32, z: i32) -> bool {
        (self.ox..self.ox + 16).contains(&x) && (self.oz..self.oz + 16).contains(&z) && (MIN_Y..MAX_Y).contains(&y)
    }

    /// `placeCoralBlock`.
    fn block(&self, r: &mut Region, rand: &mut Xoroshiro, x: i32, y: i32, z: i32, coral: i32) -> bool {
        if !self.in_bounds(x, y, z) || !self.in_bounds(x, y + 1, z) {
            return false;
        }
        let t = r.get(x, y, z);
        if (!is_water(t) && !blocks::short(t).contains("coral")) || !is_water(r.get(x, y + 1, z)) {
            return false;
        }
        r.set(x, y, z, coral);
        if rand.next_float() < 0.25 {
            let fan = blocks::id(&format!("{}_coral_fan", CORAL[rand.next_int_below(5) as usize]));
            r.set(x, y + 1, z, fan);
        } else if rand.next_float() < 0.05 {
            r.set(x, y + 1, z, pickle(rand.next_int_below(4)));
        }
        for (fi, (dx, dz)) in FACES.into_iter().enumerate() {
            if rand.next_float() >= 0.2 {
                continue;
            }
            let (nx, nz) = (x + dx, z + dz);
            if !self.in_bounds(nx, y, nz) || !is_water(r.get(nx, y, nz)) {
                continue;
            }
            let kind = CORAL[rand.next_int_below(5) as usize];
            // coralDirection: WEST 0, EAST 1, NORTH 2, SOUTH 3.
            let dir = [2, 1, 3, 0][fi];
            r.set(nx, y, nz, blocks::with(blocks::id(&format!("{kind}_coral_wall_fan")), &[("coral_direction", i(dir))]));
        }
        true
    }

    /// `CoralClawFeature.placeFeature`.
    fn claw(&self, r: &mut Region, rand: &mut Xoroshiro, x: i32, y: i32, z: i32, coral: i32) {
        if !self.block(r, rand, x, y, z, coral) {
            return;
        }
        let claw = rand.next_int_below(4) as usize;
        let branches = rand.next_int_below(2) + 2;
        // claw, rotateY (clockwise), rotateYCCW
        let mut possible = [claw, (claw + 1) % 4, (claw + 3) % 4];
        for n in (1..possible.len()).rev() {
            let j = rand.next_int_below(n as i32 + 1) as usize;
            possible.swap(n, j);
        }
        let (cdx, cdz) = FACES[claw];
        for &branch in possible.iter().take(branches as usize) {
            let (bdx, bdz) = FACES[branch];
            let (mut px, mut py, mut pz) = (x + bdx, y, z + bdz);
            let sideway = rand.next_int_below(2) + 1;
            let (seg, inway): ((i32, i32, i32), i32) = if branch == claw {
                ((cdx, 0, cdz), rand.next_int_below(3) + 2)
            } else {
                py += 1;
                let seg = if rand.next_boolean() { (bdx, 0, bdz) } else { (0, 1, 0) };
                (seg, rand.next_int_below(3) + 3)
            };
            let mut n = 0;
            while n < sideway && self.block(r, rand, px, py, pz, coral) {
                px += seg.0;
                py += seg.1;
                pz += seg.2;
                n += 1;
            }
            px -= seg.0;
            py -= seg.1;
            pz -= seg.2;
            py += 1;
            for _ in 0..inway {
                px += cdx;
                pz += cdz;
                if !self.block(r, rand, px, py, pz, coral) {
                    break;
                }
                if rand.next_float() < 0.25 {
                    py += 1;
                }
            }
        }
    }

    /// `CoralMushroomFeature.placeFeature` (the "crust").
    fn crust(&self, r: &mut Region, rand: &mut Xoroshiro, x: i32, y: i32, z: i32, coral: i32) {
        let height = rand.next_int_below(3) + 3;
        let width = rand.next_int_below(3) + 3;
        let length = rand.next_int_below(3) + 3;
        let sink = rand.next_int_below(3) + 1;
        for ix in 0..=width {
            for iy in 0..=height {
                for iz in 0..=length {
                    let ex = ix == 0 || ix == width;
                    let ey = iy == 0 || iy == height;
                    let ez = iz == 0 || iz == length;
                    if !(ex && ey) && !(ez && ey) && !(ex && ez) && (ex || ey || ez) && rand.next_float() >= 0.1 {
                        self.block(r, rand, x + ix, y + iy - sink, z + iz, coral);
                    }
                }
            }
        }
    }
}

fn pickle(n: i32) -> i32 {
    blocks::with(blocks::id("sea_pickle"), &[("cluster_count", i(n))])
}

#[derive(Clone, Copy)]
enum CoralKind {
    Anemone,
    Claw,
    Crust,
}

/// `AbstractCoralFeature` subclasses (each: count 1).
fn coral(r: &mut Region, ctx: &mut Ctx, name: &str, kind: CoralKind) {
    let n = count(ctx, name, 1, 0);
    let c = Coral { ox: ctx.cx << 4, oz: ctx.cz << 4 };
    // `canStay`: the column is always inside the chunk, so only the y bound remains.
    let stay = |r: &Region, x: i32, y: i32, z: i32| is_water(r.get(x, y, z)) && matches!(blocks::short(r.get(x, y - 1, z)), "sand" | "red_sand") && (MIN_Y..MAX_Y).contains(&(y - 1));
    water_foliage(r, ctx, n, stay, &mut |r, rand, x, y, z| {
        let block = blocks::id(&format!("{}_coral_block", CORAL[rand.next_int_below(5) as usize]));
        match kind {
            CoralKind::Anemone => {
                c.block(r, rand, x, y, z, block);
            }
            CoralKind::Claw => c.claw(r, rand, x, y, z, block),
            CoralKind::Crust => c.crust(r, rand, x, y, z, block),
        }
    });
}

/// `SeaPickleFeature`.
fn sea_pickles(r: &mut Region, ctx: &mut Ctx) {
    let n = count(ctx, "minecraft:pickle_feature", 1, 0);
    let (ox, oz) = (ctx.cx << 4, ctx.cz << 4);
    let floor = |r: &Region, x: i32, z: i32| {
        let mut y = r.height(x, z) - 1;
        if y <= MIN_Y {
            return y;
        }
        while y > MIN_Y && is_water(r.get(x, y, z)) {
            y -= 1;
        }
        y + 1
    };
    let ok = |r: &Region, x: i32, y: i32, z: i32| y > MIN_Y && y < MAX_Y && is_water(r.get(x, y, z)) && blocks::is_solid(r.get(x, y - 1, z));
    for _ in 0..n {
        let (lx0, lz0) = (ctx.random.next_int_below(15), ctx.random.next_int_below(15));
        let mut placed = 0;
        for _ in 0..20 {
            let lx = lx0 + ctx.random.next_int_below(8) - ctx.random.next_int_below(8);
            let lz = lz0 + ctx.random.next_int_below(8) - ctx.random.next_int_below(8);
            if !(0..16).contains(&lx) || !(0..16).contains(&lz) {
                continue;
            }
            let (x, z) = (ox + lx, oz + lz);
            let y = floor(r, x, z);
            if !ok(r, x, y, z) {
                continue;
            }
            r.set(x, y, z, pickle(ctx.random.next_int_below(4)));
            placed += 1;
        }
        if placed == 0 {
            let (x, z) = (ox + lx0, oz + lz0);
            let y = floor(r, x, z);
            if ok(r, x, y, z) {
                r.set(x, y, z, pickle(ctx.random.next_int_below(4)));
            }
        }
    }
}

/// `WaterlilyFeature`.
fn waterlilies(r: &mut Region, ctx: &mut Ctx) {
    let n = count(ctx, "minecraft:fixup_waterlily_position_feature", 4, 2);
    for _ in 0..n {
        let x = (ctx.cx << 4) + ctx.random.next_int_below(15);
        let z = (ctx.cz << 4) + ctx.random.next_int_below(15);
        let y = r.height(x, z) - 1;
        if y == SEA_LEVEL && blocks::is(r.get(x, y, z), "water") {
            r.set(x, y + 1, z, blocks::id("waterlily"));
        }
    }
}

pub(super) fn register(m: &mut HashMap<&'static str, FeatureFn>) {
    m.insert(SEAGRASS_SEED_NAME, |r, c| seagrass(r, c, 24, 0.3));
    m.insert("minecraft:scatter_ocean_seagrass_feature", |r, c| seagrass(r, c, 24, 0.3));
    m.insert("minecraft:scatter_warm_ocean_seagrass_feature", |r, c| seagrass(r, c, 45, 0.3));
    m.insert("minecraft:scatter_swamp_seagrass_feature", |r, c| seagrass(r, c, 48, 0.6));
    m.insert("minecraft:kelp_feature", kelp);
    m.insert("minecraft:sea_anemone_feature", |r, c| coral(r, c, "minecraft:sea_anemone_feature", CoralKind::Anemone));
    m.insert("minecraft:coral_hang_feature", |r, c| coral(r, c, "minecraft:coral_hang_feature", CoralKind::Claw));
    m.insert("minecraft:coral_crust_feature", |r, c| coral(r, c, "minecraft:coral_crust_feature", CoralKind::Crust));
    m.insert("minecraft:pickle_feature", sea_pickles);
    m.insert("minecraft:fixup_waterlily_position_feature", waterlilies);
}

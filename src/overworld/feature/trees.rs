// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Tree and huge-mushroom features (PNX `feature/tree`, `HugeMushroomFeature`,
//! `RandomRoofedForestFeatureWithDecorationFeature`), registered under their PNX `NAME`.
//! Legacy per-biome tree rules are one runner fed by a table, and PNX's TreePlacementFeatures
//! graph is written out as plain functions. Features PNX doesn't have aren't registered either.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::blocks::{self, i, s};
use super::decoration::placement::{reseed, surface_candidates, Coord, Order, PlaceCtx, Placement, Scatter, Vegetation, MAX_Y, MIN_Y};
use super::objects::{self, mh_cos, mh_sin, vine, BigMushroom, Cherry, Face, JungleBush, Mangrove, MegaConifer, MegaJungle, PaleOak, Pine, RoofedWorldgen, SavannaWorldgen, SmallSpruce, SwampOak};
use super::{chunk_hash, java_hash, random_range, Ctx, FeatureFn, Region, Xoroshiro};

const SEA_LEVEL: i32 = 63;

type P = fn(&mut PlaceCtx) -> bool;

// ---------------------------------------------------------------------------------------------
// Biome tags (from the vanilla biome definitions, like PNX `Registries.BIOME.containsTag`).

fn tags() -> &'static HashMap<u16, Vec<String>> {
    static T: OnceLock<HashMap<u16, Vec<String>>> = OnceLock::new();
    T.get_or_init(|| {
        let batch = crate::vanilla::batches().nth(2).expect("biome definitions frame");
        let decoded = bedrock::network::codec::decode_packets_each::<crate::Protocol>(batch, None, None, usize::MAX, bedrock::network::codec::UnreadBytes::Warn);
        let Some(crate::Protocol::BiomeDefinitionListPacket(p)) = decoded.ok().and_then(|d| d.into_iter().next()).and_then(|(_, p)| p.ok()) else {
            return HashMap::new();
        };
        let s = |i: u16| p.strings.get(i as usize).cloned().unwrap_or_default();
        p.biomes
            .iter()
            .filter_map(|e| {
                let id = super::super::biome_id(s(e.name_index).trim_start_matches("minecraft:"))?;
                Some((id, e.definition.tags.iter().flat_map(|t| t.tags.iter()).map(|&t| s(t)).collect()))
            })
            .collect()
    })
}

fn tagged(biome: u16, tag: &str) -> bool {
    tags().get(&biome).is_some_and(|v| v.iter().any(|t| t == tag))
}

/// `TreePlacementFeatures.hasTag`: biome one below the position.
fn has(c: &PlaceCtx, tag: &str) -> bool {
    tagged(c.region.biome(c.x, MIN_Y.max(c.y - 1), c.z), tag)
}

fn has_at_y(c: &PlaceCtx, tag: &str) -> bool {
    tagged(c.region.biome(c.x, c.y, c.z), tag)
}

// ---------------------------------------------------------------------------------------------
// Placement combinators (PNX AggregatePlacementFeature & co).

#[derive(Clone, Copy, PartialEq)]
enum Early {
    None,
    FirstFailure,
    FirstSuccess,
}

fn agg(c: &mut PlaceCtx, early: Early, fs: &[&dyn Fn(&mut PlaceCtx) -> bool]) -> bool {
    let input = c.pos();
    let (mut any, mut out) = (false, input);
    for f in fs {
        c.set_pos(input.0, input.1, input.2);
        if f(c) {
            any = true;
            out = c.pos();
            if early == Early::FirstSuccess {
                return true;
            }
        } else if early == Early::FirstFailure {
            break;
        }
    }
    let p = if any { out } else { input };
    c.set_pos(p.0, p.1, p.2);
    any
}

/// `TreePlacementFeatures.optional`.
fn optional(c: &mut PlaceCtx, denominator: i32, f: P) -> bool {
    c.random.next_exclusive_int(denominator) == 0 && f(c)
}

// ---------------------------------------------------------------------------------------------
// Shared block predicates of the tree placement features.

fn nm(b: i32) -> &'static str {
    blocks::short(b)
}

fn vanilla_leaves(n: &str) -> bool {
    matches!(n, "oak_leaves" | "spruce_leaves" | "birch_leaves" | "jungle_leaves" | "acacia_leaves" | "dark_oak_leaves")
}

/// `mayReplace` (air or vanilla leaves).
fn may_replace(b: i32) -> bool {
    blocks::is_air(b) || vanilla_leaves(nm(b))
}

fn may_grow_through(b: i32) -> bool {
    matches!(nm(b), "dirt" | "grass_block" | "coarse_dirt") || may_replace(b)
}

fn may_grow_on(b: i32, podzol: bool, farmland: bool) -> bool {
    match nm(b) {
        "dirt" | "grass_block" | "coarse_dirt" => true,
        "podzol" => podzol,
        "farmland" => farmland,
        _ => false,
    }
}

fn canopy_replaceable(b: i32) -> bool {
    blocks::is_air(b) || nm(b) == "vine" || blocks::is_leaves(b)
}

fn fix_base(r: &mut Region, x: i32, y: i32, z: i32) {
    if !matches!(nm(r.get(x, y, z)), "dirt" | "coarse_dirt") {
        r.set(x, y, z, blocks::id("dirt"));
    }
}

fn sample_range(r: &mut Xoroshiro, min: i32, max: i32) -> i32 {
    if min >= max - 1 { min } else { min + r.next_exclusive_int(max - min) }
}

/// The `prepare`/`hasClearance` column check shared by classic, fallen and jungle trees.
fn clearance(r: &Region, x: i32, y: i32, z: i32, h: i32, through: fn(i32) -> bool) -> bool {
    for oy in 0..=h + 1 {
        let rad = if oy == 0 { 0 } else if oy >= h - 1 { 2 } else { 1 };
        for ox in -rad..=rad {
            for oz in -rad..=rad {
                if !through(r.get(x + ox, y + oy, z + oz)) {
                    return false;
                }
            }
        }
    }
    true
}

fn log(name: &str, axis: &str) -> i32 {
    objects::axis_log(name, axis)
}

// Vines by the face they hang on (VINE_WEST = vine(EAST) etc.), for -x, +x, -z, +z.
const SIDE_VINES: [(i32, i32, i32); 4] = [(-1, 0, 8), (1, 0, 2), (0, -1, 1), (0, 1, 4)];

// ---------------------------------------------------------------------------------------------
// ClassicTreePlacementFeature.

struct Classic {
    log: &'static str,
    leaves: &'static str,
    min: i32,
    max: i32,
    min_mod: i32,
    max_mod: i32,
    spruce: bool,
}

const OAK: Classic = Classic { log: "oak_log", leaves: "oak_leaves", min: 4, max: 7, min_mod: 0, max_mod: 0, spruce: false };
const BIRCH: Classic = Classic { log: "birch_log", leaves: "birch_leaves", min: 5, max: 8, min_mod: 0, max_mod: 0, spruce: false };
const SUPER_BIRCH: Classic = Classic { log: "birch_log", leaves: "birch_leaves", min: 5, max: 8, min_mod: 0, max_mod: 7, spruce: false };
const SPRUCE: Classic = Classic { log: "spruce_log", leaves: "spruce_leaves", min: 6, max: 10, min_mod: -2, max_mod: 1, spruce: true };

fn classic(c: &mut PlaceCtx, t: &Classic, vines: bool) -> bool {
    let (x, y, z) = c.pos();
    let base = sample_range(c.random, t.min, t.max);
    if y <= MIN_Y || y + base >= MAX_Y || !may_grow_on(c.region.get(x, y - 1, z), true, true) || !clearance(c.region, x, y, z, base, may_grow_through) {
        return false;
    }
    let height = base + sample_range(c.random, t.min_mod, t.max_mod);
    let (logb, leaves) = (log(t.log, "y"), blocks::id(t.leaves));
    let mut last = None;
    for oy in 0..height {
        let ty = y + oy;
        if !may_replace(c.region.get(x, ty, z)) {
            continue;
        }
        c.region.set(x, ty, z, logb);
        if vines {
            for (dx, dz, m) in SIDE_VINES {
                if blocks::is_air(c.region.get(x + dx, ty, z + dz)) {
                    c.region.set(x + dx, ty, z + dz, vine(m));
                }
            }
        }
        last = Some(ty);
    }
    let Some(last) = last else { return false };
    let cy = last + 1;
    let mut leaf = |r: &mut Region, x, y, z| {
        if canopy_replaceable(r.get(x, y, z)) {
            r.set(x, y, z, leaves);
        }
    };
    if t.spruce {
        let lower = 1 + c.random.next_exclusive_int(2);
        let upper = c.random.next_exclusive_int(3);
        let max_radius = 2 + c.random.next_exclusive_int(2);
        let mut radius = c.random.next_exclusive_int(2);
        let (mut ceiling, mut reset) = (1, 0);
        let mut ly = cy + upper;
        while ly >= y + lower {
            for ox in -radius..=radius {
                for oz in -radius..=radius {
                    if radius > 0 && ox.abs() == radius && oz.abs() == radius {
                        continue;
                    }
                    leaf(c.region, x + ox, ly, z + oz);
                }
            }
            if radius >= ceiling {
                radius = reset;
                reset = 1;
                ceiling = (ceiling + 1).min(max_radius);
            } else {
                radius += 1;
            }
            ly -= 1;
        }
    } else {
        simple_canopy(c, x, cy, z, &mut leaf);
    }
    fix_base(c.region, x, y - 1, z);
    c.set_pos(x, cy, z);
    true
}

fn simple_canopy(c: &mut PlaceCtx, x: i32, y: i32, z: i32, leaf: &mut dyn FnMut(&mut Region, i32, i32, i32)) {
    for oy in -3..=0 {
        let rad: i32 = if oy <= -2 { 2 } else { 1 };
        for ox in -rad..=rad {
            for oz in -rad..=rad {
                let corner = ox.abs() == rad && oz.abs() == rad;
                if corner && (oy == 0 || c.random.next_exclusive_int(2) == 0) {
                    continue;
                }
                leaf(c.region, x + ox, y + oy, z + oz);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// FallenTreePlacementFeature.

struct Fallen {
    log: &'static str,
    min: i32,
    max: i32,
    min_mod: i32,
    max_mod: i32,
    farmland: bool,
    stump_vines: bool,
}

const FALLEN_OAK: Fallen = Fallen { log: "oak_log", min: 4, max: 7, min_mod: 0, max_mod: 0, farmland: true, stump_vines: true };
const FALLEN_BIRCH: Fallen = Fallen { log: "birch_log", min: 5, max: 8, min_mod: 0, max_mod: 0, farmland: true, stump_vines: false };
const FALLEN_SUPER_BIRCH: Fallen = Fallen { log: "birch_log", min: 5, max: 8, min_mod: 0, max_mod: 7, farmland: true, stump_vines: false };
const FALLEN_SPRUCE: Fallen = Fallen { log: "spruce_log", min: 6, max: 10, min_mod: 0, max_mod: 0, farmland: false, stump_vines: false };
const FALLEN_JUNGLE: Fallen = Fallen { log: "jungle_log", min: 4, max: 11, min_mod: 0, max_mod: 0, farmland: true, stump_vines: true };
const FALLEN_POPLAR: Fallen = Fallen { log: "poplar_log", min: 4, max: 7, min_mod: 0, max_mod: 0, farmland: true, stump_vines: false };

const DIR_X: [i32; 4] = [0, 0, -1, 1];
const DIR_Z: [i32; 4] = [-1, 1, 0, 0];
const SHADE: [(i32, i32); 13] = [(0, 0), (0, -1), (1, 0), (0, 1), (-1, 0), (1, -1), (-1, -1), (1, 1), (-1, 1), (0, -2), (2, 0), (0, 2), (-2, 0)];

fn fallen_may_replace(b: i32) -> bool {
    may_replace(b) || matches!(nm(b), "red_poplar_leaves" | "orange_poplar_leaves" | "yellow_poplar_leaves")
}

fn fallen_may_grow_through(b: i32) -> bool {
    matches!(nm(b), "dirt" | "grass_block" | "coarse_dirt") || fallen_may_replace(b)
}

fn built_over(b: i32) -> bool {
    if nm(b) == "snow_layer" {
        return !matches!(crate::blocks::state(b, "height"), Some(crate::blocks::State::Int(7)));
    }
    blocks::replaceable(b)
}

/// `BlockShelfMushroom` facing a direction with a growth stage.
fn shelf_mushroom(face: Face, growth: i32) -> i32 {
    let dir = match face {
        Face::South => "south",
        Face::West => "west",
        Face::North => "north",
        Face::East => "east",
    };
    blocks::with(blocks::id("shelf_mushroom"), &[("minecraft:cardinal_direction", s(dir)), ("growth", i(growth))])
}

fn fallen(c: &mut PlaceCtx, t: &Fallen) -> bool {
    let (x, y, z) = c.pos();
    let base = sample_range(c.random, t.min, t.max);
    if y <= MIN_Y || y + base >= MAX_Y || !may_grow_on(c.region.get(x, y - 1, z), true, t.farmland) || !clearance(c.region, x, y, z, base, fallen_may_grow_through) {
        return false;
    }
    let modifier = sample_range(c.random, t.min_mod, t.max_mod);
    let dir = c.random.next_exclusive_int(4) as usize;
    let sep = 2 + c.random.next_exclusive_int(2);
    let len = base + modifier - 2;
    let (sx, sz) = (x + DIR_X[dir] * sep, z + DIR_Z[dir] * sep);
    let mut sy = y + 1;
    while sy > MIN_Y {
        let below = c.region.get(sx, sy - 1, sz);
        if !blocks::is_air(below) && !built_over(below) {
            break;
        }
        sy -= 1;
    }
    if can_place_run(c.region, sx, sy, sz, dir, len) {
        let logb = log(t.log, if dir <= 1 { "z" } else { "x" });
        for k in 0..len {
            let (bx, bz) = (sx + DIR_X[dir] * k, sz + DIR_Z[dir] * k);
            c.region.set(bx, sy, bz, logb);
            log_mushroom(c, bx, sy, bz);
        }
        if t.log == "poplar_log" && c.random.next_float() < 0.25 {
            let attempts = 1 + c.random.next_exclusive_int(2);
            for _ in 0..attempts {
                let k = c.random.next_exclusive_int(len);
                let side = if c.random.next_boolean() { 1 } else { -1 };
                let (side_x, side_z) = (DIR_Z[dir] * side, -DIR_X[dir] * side);
                let (mx, mz) = (sx + DIR_X[dir] * k + side_x, sz + DIR_Z[dir] * k + side_z);
                if !blocks::is_air(c.region.get(mx, sy, mz)) {
                    continue;
                }
                let face = if side_x > 0 { Face::East } else if side_x < 0 { Face::West } else if side_z > 0 { Face::South } else { Face::North };
                let g = c.random.next_exclusive_int(2);
                c.region.set(mx, sy, mz, shelf_mushroom(face, g));
            }
        }
    }
    let mut stump = false;
    if fallen_may_replace(c.region.get(x, y, z)) {
        c.region.set(x, y, z, log(t.log, "y"));
        stump = true;
        if t.stump_vines {
            for (dx, dz, m) in SIDE_VINES {
                if c.random.next_float() * 100.0 >= 75.0 {
                    continue;
                }
                if blocks::is_air(c.region.get(x + dx, y, z + dz)) {
                    c.region.set(x + dx, y, z + dz, vine(m));
                }
            }
        }
    }
    fix_base(c.region, x, y - 1, z);
    if !stump {
        return false;
    }
    c.set_pos(x, y + 1, z);
    true
}

fn can_place_run(r: &Region, x: i32, y: i32, z: i32, dir: usize, len: i32) -> bool {
    let mut unsupported = 0;
    for k in 0..len {
        let (bx, bz) = (x + DIR_X[dir] * k, z + DIR_Z[dir] * k);
        if !(MIN_Y..MAX_Y).contains(&y) {
            return false;
        }
        let target = r.get(bx, y, bz);
        if !blocks::is_air(target) && !built_over(target) {
            return false;
        }
        // isSolid && isFullBlock && !leaves (is_full already excludes leaves)
        if blocks::is_full(r.get(bx, y - 1, bz)) {
            unsupported = 0;
        } else {
            unsupported += 1;
            if unsupported > 2 {
                return false;
            }
        }
    }
    true
}

fn log_mushroom(c: &mut PlaceCtx, x: i32, y: i32, z: i32) {
    if c.random.next_exclusive_int(10) != 0 {
        return;
    }
    let brown = c.random.next_float() * 100.0 < 50.0;
    let my = y + 1;
    let shaded = |r: &Region, x: i32, z: i32| y + 1 < r.height(x, z) || (my + 1..=(my + 10).min(MAX_Y - 1)).any(|by| blocks::is_solid(r.get(x, by, z)));
    if blocks::is_air(c.region.get(x, my, z)) && SHADE.iter().all(|&(dx, dz)| shaded(c.region, x + dx, z + dz)) {
        c.region.set(x, my, z, blocks::id(if brown { "brown_mushroom" } else { "red_mushroom" }));
    }
    for _ in 0..6 {
        c.random.next_int();
    }
}

// ---------------------------------------------------------------------------------------------
// FancyOakPlacementFeature.

const FANCY_VARIANCE: i32 = 12;

fn fancy_through(r: &Region, x: i32, y: i32, z: i32) -> bool {
    (MIN_Y..MAX_Y).contains(&y) && may_grow_through(r.get(x, y, z))
}

fn check_line(r: &Region, a: (i32, i32, i32), b: (i32, i32, i32)) -> i32 {
    let (dx, dy, dz) = (b.0 - a.0, b.1 - a.1, b.2 - a.2);
    let steps = dx.abs().max(dy.abs()).max(dz.abs());
    if steps == 0 {
        return if fancy_through(r, a.0, a.1, a.2) { -1 } else { 0 };
    }
    let (sx, sy, sz) = (dx as f32 / steps as f32, dy as f32 / steps as f32, dz as f32 / steps as f32);
    for k in 0..=steps {
        let kf = k as f32;
        if !fancy_through(r, a.0 + (0.5 + kf * sx) as i32, a.1 + (0.5 + kf * sy) as i32, a.2 + (0.5 + kf * sz) as i32) {
            return k;
        }
    }
    -1
}

fn place_limb(r: &mut Region, a: (i32, i32, i32), b: (i32, i32, i32)) {
    let (dx, dy, dz) = (b.0 - a.0, b.1 - a.1, b.2 - a.2);
    let steps = dx.abs().max(dy.abs()).max(dz.abs());
    if steps == 0 {
        r.set(a.0, a.1, a.2, log("oak_log", "y"));
        return;
    }
    let (sx, sy, sz) = (dx as f32 / steps as f32, dy as f32 / steps as f32, dz as f32 / steps as f32);
    for k in 0..=steps {
        let kf = k as f32;
        let (ox, oy, oz) = ((0.5 + kf * sx) as i32, (0.5 + kf * sy) as i32, (0.5 + kf * sz) as i32);
        let axis = if (ox.abs() | oz.abs()) == 0 { "y" } else if ox.abs() >= oz.abs() { "x" } else { "z" };
        r.set(a.0 + ox, a.1 + oy, a.2 + oz, log("oak_log", axis));
    }
}

fn fancy(c: &mut PlaceCtx) -> bool {
    let (ox, oy, oz) = c.pos();
    let sampled = 5 + c.random.next_exclusive_int(FANCY_VARIANCE);
    if !may_grow_on(c.region.get(ox, oy - 1, oz), false, true) {
        return false;
    }
    let height = match check_line(c.region, (ox, oy, oz), (ox, oy + sampled - 1, oz)) {
        -1 => sampled,
        o if o <= 5 => return false,
        o => o,
    };
    let trunk_top = (height - 1).min((height as f32 * 0.618) as i32);
    let clusters = {
        let v = height as f32 / (FANCY_VARIANCE + 1) as f32;
        1.max((v * v + 1.382) as i32)
    };
    let mut rel = height - FANCY_VARIANCE / 3;
    let mut foliage = vec![(ox, oy + rel, oz, oy + trunk_top)];
    while rel >= 0 {
        let shape = tree_shape(height, rel);
        if shape >= 0.0 {
            for _ in 0..clusters {
                let rd = c.random.next_float();
                let ra = c.random.next_float();
                let dist = (rd + 0.328) * shape;
                let angle = (ra + ra) * std::f32::consts::PI;
                let cx = ox + (mh_sin(angle) * dist + 0.5) as i32;
                let cz = oz + (mh_cos(angle) * dist + 0.5) as i32;
                let cy = oy + rel - 1;
                if check_line(c.region, (cx, cy, cz), (cx, cy + FANCY_VARIANCE / 3, cz)) != -1 {
                    continue;
                }
                let (dx, dz) = (ox - cx, oz - cz);
                let hd = ((dx as f64 * dx as f64) + (dz as f64 * dz as f64)).sqrt() as i32;
                let branch_base = (oy + trunk_top).min((cy as f32 - hd as f32 * 0.381) as i32);
                if check_line(c.region, (ox, branch_base, oz), (cx, cy, cz)) != -1 {
                    continue;
                }
                foliage.push((cx, cy, cz, branch_base));
            }
        }
        rel -= 1;
    }
    let leaves = blocks::id("oak_leaves");
    for &(x, y, z, _) in &foliage {
        for layer in 0..4 {
            let rad: i32 = if layer == 0 || layer == 3 { 2 } else { 3 };
            let r2 = (rad * rad) as f32;
            for lx in -rad..=rad {
                let fx = lx.abs() as f32 + 0.5;
                for lz in -rad..=rad {
                    let fz = lz.abs() as f32 + 0.5;
                    let ly = y + layer;
                    if fx * fx + fz * fz > r2 || !(MIN_Y..MAX_Y).contains(&ly) || !may_replace(c.region.get(x + lx, ly, z + lz)) {
                        continue;
                    }
                    c.region.set(x + lx, ly, z + lz, leaves);
                }
            }
        }
    }
    place_limb(c.region, (ox, oy, oz), (ox, oy + trunk_top, oz));
    for &(x, y, z, base) in &foliage {
        if ((base - oy) as f32) < height as f32 * 0.2 {
            continue;
        }
        place_limb(c.region, (ox, base, oz), (x, y, z));
    }
    true
}

fn tree_shape(height: i32, rel: i32) -> f32 {
    if (rel as f32) < height as f32 * 0.3 {
        return -1.0;
    }
    let half = height as f32 * 0.5;
    let adj = half - rel as f32;
    if adj == 0.0 {
        half * 0.5
    } else if adj.abs() >= half {
        0.0
    } else {
        (half * half - adj * adj).sqrt() * 0.5
    }
}

// ---------------------------------------------------------------------------------------------
// JungleTreePlacementFeature + JungleCocoaPlacementFeature.

fn jungle_tree(c: &mut PlaceCtx) -> bool {
    let (x, y, z) = c.pos();
    let h = 4 + c.random.next_exclusive_int(7);
    if y <= MIN_Y || y + h >= MAX_Y || !may_grow_on(c.region.get(x, y - 1, z), true, true) || !clearance(c.region, x, y, z, h, may_grow_through) {
        return false;
    }
    let logb = log("jungle_log", "y");
    let mut last = None;
    for o in 0..h {
        let ty = y + o;
        if !may_replace(c.region.get(x, ty, z)) {
            continue;
        }
        c.region.set(x, ty, z, logb);
        for (dx, dz, m) in SIDE_VINES {
            if c.random.next_float() * 100.0 >= 33.33 {
                continue;
            }
            if blocks::is_air(c.region.get(x + dx, ty, z + dz)) {
                c.region.set(x + dx, ty, z + dz, vine(m));
            }
        }
        last = Some(ty);
    }
    let Some(last) = last else { return false };
    let cy = last + 1;
    let leaves = blocks::id("jungle_leaves");
    simple_canopy(c, x, cy, z, &mut |r: &mut Region, x, y, z| {
        if canopy_replaceable(r.get(x, y, z)) {
            r.set(x, y, z, leaves);
        }
    });
    for oy in -3..=0 {
        let rad: i32 = if oy <= -2 { 2 } else { 1 };
        for ox in -rad..=rad {
            for oz in -rad..=rad {
                let (lx, ly, lz) = (x + ox, cy + oy, z + oz);
                if nm(c.region.get(lx, ly, lz)) != "jungle_leaves" {
                    continue;
                }
                for (dx, dz, m) in SIDE_VINES {
                    if c.random.next_exclusive_int(4) != 0 {
                        continue;
                    }
                    let mut vy = ly;
                    for _ in 0..5 {
                        if !blocks::is_air(c.region.get(lx + dx, vy, lz + dz)) {
                            break;
                        }
                        c.region.set(lx + dx, vy, lz + dz, vine(m));
                        vy -= 1;
                    }
                }
            }
        }
    }
    fix_base(c.region, x, y - 1, z);
    c.set_pos(x, cy, z);
    true
}

fn cocoa(c: &mut PlaceCtx) -> bool {
    if c.random.next_exclusive_int(5) != 0 {
        return false;
    }
    let (x, y, z) = c.pos();
    let a = cocoa_layer(c, x, y - 5, z, 4);
    cocoa_layer(c, x, y - 4, z, 3) || a
}

fn cocoa_layer(c: &mut PlaceCtx, x: i32, y: i32, z: i32, d: i32) -> bool {
    let mut placed = false;
    for (sx, sz) in [(0, 1), (1, 0), (2, 0), (0, -1)] {
        if c.random.next_exclusive_int(d) != 0 {
            continue;
        }
        let (bx, bz) = (x + sx, z + sz);
        let age = c.random.next_exclusive_int(3);
        c.random.next_exclusive_int(1); // SingleBlockFeature's one-entry weighted pick
        let mut support = None;
        for (f, dx, dz) in [(Face::North, 0, -1), (Face::East, 1, 0), (Face::South, 0, 1), (Face::West, -1, 0)] {
            if nm(c.region.get(bx + dx, y, bz + dz)) == "jungle_log" {
                support = Some(f);
            }
        }
        let Some(f) = support else { continue };
        c.region.set(bx, y, bz, blocks::with(blocks::id("cocoa"), &[("age", i(age)), ("direction", i(f.horizontal_index()))]));
        placed = true;
    }
    placed
}

// ---------------------------------------------------------------------------------------------
// ObjectPoplarTree (not in the object library; only the dappled forest uses it).

fn poplar_replace(b: i32) -> bool {
    blocks::is_air(b) || matches!(nm(b), "poplar_sapling" | "vine") || blocks::is_leaves(b)
}

fn poplar(c: &mut PlaceCtx) -> bool {
    let (x, y, z) = c.pos();
    let trunk = 7 + c.random.next_exclusive_int(5);
    if y <= MIN_Y || y + trunk + 1 >= MAX_Y || !may_grow_on(c.region.get(x, y - 1, z), true, true) || !(0..trunk).all(|o| poplar_replace(c.region.get(x, y + o, z))) {
        return false;
    }
    let leaves = blocks::id(["red_poplar_leaves", "orange_poplar_leaves", "yellow_poplar_leaves"][c.random.next_exclusive_int(3) as usize]);
    let branch_y = y + trunk - 4 - 1;
    fix_base(c.region, x, y - 1, z);
    // trunk
    let vertical = log("poplar_log", "y");
    for o in 0..trunk {
        c.region.set(x, y + o, z, vertical);
    }
    let mut dirs = Face::HORIZONTALS;
    for rem in (2..=4).rev() {
        let sel = c.random.next_exclusive_int(rem as i32) as usize;
        dirs.swap(sel, rem - 1);
    }
    let branches = 1 + c.random.next_exclusive_int(4);
    for &d in &dirs[..branches as usize] {
        let (bx, bz) = (x + d.dx(), z + d.dz());
        if poplar_replace(c.region.get(bx, branch_y, bz)) {
            c.region.set(bx, branch_y, bz, log("poplar_log", if d.is_x() { "x" } else { "z" }));
        }
    }
    // shelf mushrooms
    if c.random.next_float() < 0.25 {
        let attempts = 1 + c.random.next_exclusive_int(2);
        for _ in 0..attempts {
            let face = Face::HORIZONTALS[c.random.next_exclusive_int(4) as usize];
            let my = y + 1 + c.random.next_exclusive_int(1.max(branch_y - y - 1));
            let (mx, mz) = (x + face.dx(), z + face.dz());
            if !blocks::is_air(c.region.get(mx, my, mz)) {
                continue;
            }
            let g = c.random.next_exclusive_int(2);
            c.region.set(mx, my, mz, shelf_mushroom(face, g));
        }
    }
    // canopy
    let cy = branch_y + 1;
    let fh = 5 + c.random.next_exclusive_int(2);
    let radius = match c.random.next_exclusive_int(12) {
        s if s < 5 => 5,
        s if s < 10 => 6,
        10 => 7,
        _ => 8,
    } - 1;
    let alt = c.random.next_boolean();
    let layer = |c: &mut PlaceCtx, rad: i32, oy: i32| poplar_layer(c, leaves, x, cy, z, rad, oy, fh, alt);
    layer(c, radius - 2, fh - 1);
    layer(c, radius - 1, fh - 2);
    layer(c, radius - 1, fh - 3);
    for oy in (1..=fh - 4).rev() {
        layer(c, radius, oy);
    }
    // cross branches
    let oy = fh - 4;
    let partial = poplar_partial(fh, oy);
    for ox in -radius..=radius {
        for oz in -radius..=radius {
            let (ax, az) = (ox.abs(), oz.abs());
            let inside = ax + az <= radius * 2 - poplar_corner_cut(ox, oz, radius, partial, alt) - 2;
            let cross = az == 0 && radius - ax >= 4 || ax == 0 && radius - az >= 4;
            let (lx, ly, lz) = (x + ox, cy + oy, z + oz);
            if inside && cross && c.region.get(lx, ly, lz) == leaves {
                c.region.set(lx, ly, lz, log("poplar_log", if az == 0 { "x" } else { "z" }));
            }
        }
    }
    layer(c, radius - 1, 0);
    layer(c, 1.max(radius - 2), -1);
    true
}

fn poplar_partial(fh: i32, oy: i32) -> bool {
    oy == fh - 1 || oy == fh - 2
}

fn poplar_corner_cut(ox: i32, oz: i32, rad: i32, partial: bool, alt: bool) -> i32 {
    let short = if alt { ox > 0 && oz > 0 || ox < 0 && oz < 0 } else { ox > 0 && oz < 0 || ox < 0 && oz > 0 };
    if short {
        rad - 1
    } else if partial {
        rad + 1
    } else {
        rad
    }
}

#[allow(clippy::too_many_arguments)]
fn poplar_layer(c: &mut PlaceCtx, leaves: i32, x: i32, y: i32, z: i32, rad: i32, oy: i32, fh: i32, alt: bool) {
    let partial = poplar_partial(fh, oy);
    for ox in -rad..=rad {
        for oz in -rad..=rad {
            let (ax, az) = (ox.abs(), oz.abs());
            if partial && (ax == rad || az == rad) {
                continue;
            }
            let cut = poplar_corner_cut(ox, oz, rad, partial, alt);
            let gap = if c.random.next_float() <= 0.15 { 1 } else { 0 };
            if ax + az > rad * 2 - cut - gap {
                continue;
            }
            let (lx, ly, lz) = (x + ox, y + oy, z + oz);
            if poplar_replace(c.region.get(lx, ly, lz)) {
                c.region.set(lx, ly, lz, leaves);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// TreePlacementFeatures graph.

fn plain_oak(c: &mut PlaceCtx) -> bool {
    classic(c, &OAK, false)
}
fn oak_vines(c: &mut PlaceCtx) -> bool {
    classic(c, &OAK, true)
}
fn plain_birch(c: &mut PlaceCtx) -> bool {
    classic(c, &BIRCH, false)
}
fn super_birch_tree(c: &mut PlaceCtx) -> bool {
    classic(c, &SUPER_BIRCH, false)
}
fn plain_spruce(c: &mut PlaceCtx) -> bool {
    classic(c, &SPRUCE, false)
}
fn spruce_vines(c: &mut PlaceCtx) -> bool {
    classic(c, &SPRUCE, true)
}
fn fallen_oak(c: &mut PlaceCtx) -> bool {
    fallen(c, &FALLEN_OAK)
}
fn fallen_birch(c: &mut PlaceCtx) -> bool {
    fallen(c, &FALLEN_BIRCH)
}
fn fallen_super_birch(c: &mut PlaceCtx) -> bool {
    fallen(c, &FALLEN_SUPER_BIRCH)
}
fn fallen_spruce(c: &mut PlaceCtx) -> bool {
    fallen(c, &FALLEN_SPRUCE)
}
fn fallen_jungle(c: &mut PlaceCtx) -> bool {
    fallen(c, &FALLEN_JUNGLE)
}
fn fallen_poplar(c: &mut PlaceCtx) -> bool {
    fallen(c, &FALLEN_POPLAR)
}

macro_rules! object {
    ($name:ident, $obj:expr) => {
        fn $name(c: &mut PlaceCtx) -> bool {
            $obj.generate(c.region, c.random, c.x, c.y, c.z)
        }
    };
}
object!(pine, Pine);
object!(mega_spruce, MegaConifer { canopy_height_min: 13, canopy_height_max: 18 });
object!(mega_pine, MegaConifer { canopy_height_min: 3, canopy_height_max: 8 });
object!(jungle_bush, JungleBush);
object!(mega_jungle, MegaJungle);
object!(savanna, SavannaWorldgen);
object!(plain_roofed, RoofedWorldgen { vines: false });
object!(roofed_vines, RoofedWorldgen { vines: true });
object!(huge_mushroom, BigMushroom { kind: None });

fn standing_oak(c: &mut PlaceCtx) -> bool {
    agg(c, Early::FirstSuccess, &[&|c| optional(c, 12, oak_vines), &plain_oak])
}
fn oak(c: &mut PlaceCtx) -> bool {
    agg(c, Early::FirstSuccess, &[&|c| optional(c, 80, fallen_oak), &standing_oak])
}
fn birch(c: &mut PlaceCtx) -> bool {
    agg(c, Early::FirstSuccess, &[&|c| optional(c, 80, fallen_birch), &plain_birch])
}
fn standing_spruce(c: &mut PlaceCtx) -> bool {
    agg(c, Early::FirstSuccess, &[&|c| optional(c, 12, spruce_vines), &plain_spruce])
}
fn spruce(c: &mut PlaceCtx) -> bool {
    agg(c, Early::FirstSuccess, &[&|c| optional(c, 80, fallen_spruce), &standing_spruce])
}
fn jungle(c: &mut PlaceCtx) -> bool {
    agg(c, Early::FirstSuccess, &[&|c| optional(c, 80, fallen_jungle), &|c| jungle_tree(c) && cocoa(c)])
}
fn roofed(c: &mut PlaceCtx) -> bool {
    agg(c, Early::FirstSuccess, &[&|c| optional(c, 6, roofed_vines), &plain_roofed])
}

fn bee_chance(c: &PlaceCtx) -> f32 {
    if !has(c, "bee_habitat") {
        0.0
    } else if has(c, "plains") || has(c, "sunflower_plains") {
        0.05
    } else if has(c, "flower_forest") {
        0.03
    } else if has(c, "birch") || has(c, "forest") {
        0.00035
    } else {
        0.0
    }
}
fn optional_beehive(c: &mut PlaceCtx) -> bool {
    let chance = bee_chance(c);
    if chance <= 0.0 || c.random.next_float() >= chance {
        return false;
    }
    beehive(c)
}
fn beehive(c: &mut PlaceCtx) -> bool {
    objects::bee_nest_place(c.region, c.random, c.x, c.y, c.z)
}
fn super_birch(c: &mut PlaceCtx) -> bool {
    agg(c, Early::FirstSuccess, &[&|c| optional(c, 80, fallen_super_birch), &|c| agg(c, Early::FirstFailure, &[&super_birch_tree, &optional_beehive])])
}
fn oak_opt_bee(c: &mut PlaceCtx) -> bool {
    agg(c, Early::FirstFailure, &[&plain_oak, &optional_beehive])
}
fn fancy_opt_bee(c: &mut PlaceCtx) -> bool {
    agg(c, Early::FirstFailure, &[&fancy, &optional_beehive])
}
fn fancy_bee(c: &mut PlaceCtx) -> bool {
    agg(c, Early::FirstFailure, &[&fancy, &beehive])
}
fn super_birch_bee(c: &mut PlaceCtx) -> bool {
    agg(c, Early::FirstFailure, &[&super_birch_tree, &beehive])
}
fn oak_ll(c: &mut PlaceCtx) -> bool {
    agg(c, Early::FirstFailure, &[&oak, &leaf_litter_scatter])
}
fn birch_ll(c: &mut PlaceCtx) -> bool {
    agg(c, Early::FirstFailure, &[&birch, &leaf_litter_scatter])
}
fn fancy_ll_opt_bee(c: &mut PlaceCtx) -> bool {
    agg(c, Early::FirstFailure, &[&fancy_opt_bee, &leaf_litter_scatter])
}
fn poplar_with_fallen(c: &mut PlaceCtx) -> bool {
    agg(c, Early::FirstSuccess, &[&|c| optional(c, 80, fallen_poplar), &|c| agg(c, Early::FirstFailure, &[&poplar, &leaf_litter_scatter])])
}

// Vegetation children (TallGrassAroundTreePlacementFeature, LeafLitterPlacementFeature).

fn air_here(c: &PlaceCtx) -> bool {
    c.get(0, 0, 0) == blocks::air()
}

fn tall_grass(c: &mut PlaceCtx) -> bool {
    Vegetation { state: |_| blocks::id("short_grass"), target: air_here, survival: |c| blocks::is_dirt(c.get(0, -1, 0)) }.place(c)
}

fn leaf_litter(c: &mut PlaceCtx) -> bool {
    Vegetation {
        state: |c| {
            let growth = c.random.next_exclusive_int(4);
            let dir = ["south", "west", "north", "east"][c.random.next_exclusive_int(4) as usize];
            blocks::with(blocks::id("leaf_litter"), &[("minecraft:cardinal_direction", s(dir)), ("growth", i(growth))])
        },
        target: air_here,
        // TODO: leaf litter support is just a full block below, no waterlogging.
        survival: |c| blocks::is_full(c.get(0, -1, 0)),
    }
    .place(c)
}

fn scatter(c: &mut PlaceCtx, iterations: i32, order: Order, (x, y, z): (i32, i32, i32), child: P) -> bool {
    Scatter { iterations, order, x: Coord::gaussian(-x, x), y: Coord::gaussian(-y, y), z: Coord::gaussian(-z, z), child: Box::new(child) }.place(c)
}

fn tall_grass_scatter(c: &mut PlaceCtx) -> bool {
    scatter(c, 23, Order::Zyx, (3, 4, 3), tall_grass)
}

fn leaf_litter_scatter(c: &mut PlaceCtx) -> bool {
    scatter(c, 96, Order::Zxy, (4, 2, 4), leaf_litter)
}

// ---------------------------------------------------------------------------------------------
// TreeRuleFeature with TreeFeaturePlan.legacy.

/// `TreeCountPolicy.legacy`.
fn legacy_count(r: &mut Xoroshiro, amount: f32) -> i32 {
    if r.next_exclusive_int(10) == 0 {
        return (amount * r.next_float()) as i32;
    }
    if amount < 1.0 {
        return (amount > r.next_float()) as i32;
    }
    amount as i32 + r.next_exclusive_int(10).max(7) - 7
}

fn run_legacy(region: &mut Region, ctx: &mut Ctx, name: &str, amount: f32, pred: fn(&PlaceCtx) -> bool, select: fn(&mut Xoroshiro) -> P) {
    reseed(ctx, name);
    let count = legacy_count(&mut ctx.random, amount);
    let (ox, oz) = (ctx.cx << 4, ctx.cz << 4);
    let mut c = PlaceCtx { region, random: &mut ctx.random, x: ox, y: MIN_Y, z: oz };
    for _ in 0..count {
        let feature = select(c.random);
        let z = oz + c.random.next_exclusive_int(14) + 1;
        let x = ox + c.random.next_exclusive_int(14) + 1;
        let y = c.region.height(x, z);
        if !(MIN_Y..MAX_Y).contains(&y) {
            continue;
        }
        c.set_pos(x, y, z);
        if !pred(&c) {
            continue;
        }
        feature(&mut c);
        c.set_pos(x, y, z);
        tall_grass_scatter(&mut c);
    }
}

macro_rules! legacy {
    ($m:ident; $($name:literal, $amount:expr, $pred:expr, $sel:expr;)*) => {
        $( $m.insert($name, |r, c| run_legacy(r, c, $name, $amount, $pred, $sel)); )*
    };
}

fn normal_forest(c: &PlaceCtx) -> bool {
    (has(c, "forest") || has(c, "forest_generation")) && !has(c, "birch") && !has(c, "roofed") && !has(c, "extreme_hills") && !has(c, "taiga")
}

// ---------------------------------------------------------------------------------------------
// ObjectGeneratorFeature / GriddedFeature.

type Gen = fn(&mut Region, &mut Xoroshiro, i32, i32, i32);

#[allow(clippy::too_many_arguments)]
fn run_object(region: &mut Region, ctx: &mut Ctx, name: &str, min: i32, max: i32, can_spawn: fn(u16) -> bool, swamp: bool, generate: Gen) {
    // Java: seed ^ chunkHash + hash (the addition binds first).
    ctx.random.set_seed(ctx.seed ^ chunk_hash(ctx.cx, ctx.cz).wrapping_add(java_hash(name) as i64));
    let r = &mut ctx.random;
    let amount = random_range(r, min, max);
    for _ in 0..amount {
        let x = (ctx.cx << 4) + r.next_int_below(15);
        let z = (ctx.cz << 4) + r.next_int_below(15);
        let mut y = region.height(x, z) - 1;
        if y < MIN_Y || !can_spawn(region.biome(x, y, z)) {
            continue;
        }
        loop {
            let b = region.get(x, y, z);
            let sink = if swamp { blocks::replaceable(b) } else { (blocks::replaceable(b) || !blocks::is_full(b)) && !blocks::is_liquid(b) && y > SEA_LEVEL };
            if !sink || y <= MIN_Y {
                break;
            }
            y -= 1;
        }
        if blocks::is_dirt(region.get(x, y, z)) {
            generate(region, r, x, y + 1, z);
        }
    }
}

fn grove_spruce(region: &mut Region, ctx: &mut Ctx) {
    const NAME: &str = "minecraft:grove_spruce_tree_feature";
    let hash = java_hash(NAME) as i64;
    for gx in 0..2 {
        for gz in 0..2 {
            ctx.random.set_seed(ctx.seed ^ chunk_hash(ctx.cx, ctx.cz) ^ (gx + gz) as i64 ^ hash);
            let x = 2 + ctx.random.next_int_below(6) + gx * 8 + (ctx.cx << 4);
            let z = 2 + ctx.random.next_int_below(6) + gz * 8 + (ctx.cz << 4);
            let y = region.height(x, z) - 1;
            if !tagged(region.biome(x, y, z), "grove") {
                continue;
            }
            if blocks::is_dirt(region.get(x, y, z)) {
                SmallSpruce.generate(region, &mut ctx.random, x, y + 1, z);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// RandomRoofedForestFeatureWithDecorationFeature.

fn roofed_weighted(c: &mut PlaceCtx) -> bool {
    let mut sel = c.random.next_exclusive_int(1000);
    for (f, w) in [(huge_mushroom as P, 75), (roofed, 630), (birch, 70), (fancy, 30), (oak, 195)] {
        sel -= w;
        if sel < 0 {
            return f(c);
        }
    }
    false
}

fn roofed_decoration(c: &mut PlaceCtx) -> bool {
    agg(c, Early::None, &[&|c| agg(c, Early::FirstFailure, &[&roofed_weighted, &leaf_litter_scatter]), &|c| scatter(c, 20, Order::Zyx, (3, 4, 3), tall_grass)])
}

// ---------------------------------------------------------------------------------------------

pub fn register(m: &mut HashMap<&'static str, FeatureFn>) {
    legacy! { m;
        "minecraft:bamboo_jungle_surface_trees_feature", 30.0, |c| has(c, "bamboo") && has(c, "jungle"), |r| {
            if r.next_exclusive_int(20) == 0 { fancy } else if r.next_exclusive_int(20) <= 2 { jungle_bush } else if r.next_exclusive_int(10) <= 6 { mega_jungle } else { tall_grass_scatter }
        };
        "minecraft:birch_forest_mutated_surface_trees_feature", 10.0, |c| has(c, "forest") && has(c, "birch") && has(c, "mutated"), |r| {
            if r.next_exclusive_int(5) == 0 { birch } else if r.next_boolean() { super_birch } else { birch }
        };
        "minecraft:birch_forest_surface_trees_feature", 10.0, |c| has(c, "forest") && has(c, "birch") && !has(c, "mutated"), |_| birch;
        "minecraft:select_tree_dappled_forest_feature", 7.0, |c| has(c, "dappled_forest"), |r| if r.next_exclusive_int(20) == 0 { spruce } else { poplar_with_fallen };
        "minecraft:extreme_hills_plus_trees_surface_trees_feature", 3.0, |c| has(c, "extreme_hills"), |r| {
            if r.next_exclusive_int(3) != 0 { spruce } else if r.next_exclusive_int(10) == 0 { fancy } else { oak }
        };
        "minecraft:flower_forest_surface_trees_feature", 6.0, |c| has(c, "flower_forest"), |r| {
            if r.next_exclusive_int(5) == 0 { birch } else if r.next_exclusive_int(10) == 0 { fancy_opt_bee } else { oak_opt_bee }
        };
        "minecraft:forest_surface_trees_feature", 10.0, normal_forest, |r| {
            if r.next_exclusive_int(5) == 0 { birch_ll } else if r.next_exclusive_int(10) == 0 { fancy_ll_opt_bee } else { oak_ll }
        };
        "minecraft:grove_pine_tree_feature", 4.0, |c| has(c, "grove"), |_| pine;
        "minecraft:ice_surface_trees_feature", 0.1, |c| has(c, "ice"), |_| spruce;
        "minecraft:jungle_edge_surface_trees_feature", 2.0, |c| has(c, "jungle") && !has(c, "bamboo") && has(c, "edge"), |r| {
            if r.next_exclusive_int(10) == 0 { fancy_opt_bee } else if r.next_exclusive_int(2) == 0 { jungle_bush } else { jungle }
        };
        "minecraft:jungle_surface_trees_feature", 50.0, |c| has(c, "jungle") && !has(c, "bamboo") && !has(c, "edge"), |r| {
            if r.next_exclusive_int(10) == 0 { fancy } else if r.next_exclusive_int(2) == 0 { jungle_bush } else if r.next_exclusive_int(3) == 0 { mega_jungle } else { jungle }
        };
        "minecraft:meadow_surface_trees_feature", 0.01, |c| has(c, "meadow"), |r| if r.next_boolean() { fancy_bee } else { super_birch_bee };
        "minecraft:mega_taiga_surface_trees_feature", 10.0, |c| has(c, "taiga") && has(c, "mega") && !has(c, "mutated"), |r| {
            if r.next_exclusive_int(3) == 0 {
                if r.next_exclusive_int(13) == 0 { mega_spruce } else { mega_pine }
            } else if r.next_exclusive_int(3) == 0 { pine } else { spruce }
        };
        "minecraft:mesa_plateau_stone_surface_trees_feature", 5.0, |c| has(c, "mesa") && has(c, "stone"), |_| oak_ll;
        "minecraft:plains_surface_trees_feature", 0.05, |c| has(c, "plains"), |r| if r.next_exclusive_int(3) == 0 { fancy_opt_bee } else { oak };
        "minecraft:redwood_taiga_mutated_surface_trees_feature", 10.0, |c| has(c, "taiga") && has(c, "mega") && has(c, "mutated") && !has(c, "hills"), |r| {
            if r.next_exclusive_int(3) == 0 { mega_spruce } else if r.next_exclusive_int(3) == 0 { pine } else { spruce }
        };
        "minecraft:savanna_mutated_surface_trees_feature", 2.0, |c| has(c, "savanna") && has(c, "mutated"), |r| if r.next_exclusive_int(5) > 0 { savanna } else { oak };
        "minecraft:savanna_surface_trees_feature", 1.0, |c| has(c, "savanna") && !has(c, "mutated"), |r| if r.next_exclusive_int(5) > 0 { savanna } else { oak };
        "minecraft:taiga_surface_trees_feature", 10.0, |c| has(c, "taiga") && !has(c, "mega"), |r| if r.next_exclusive_int(3) == 0 { pine } else { spruce };
    }
    m.insert("minecraft:grove_spruce_tree_feature", grove_spruce);
    m.insert("minecraft:random_roofed_forest_feature_with_decoration_feature", |r, c| {
        surface_candidates(r, c, "minecraft:random_roofed_forest_feature_with_decoration_feature", 16, |_| true, &roofed_decoration, |c| has_at_y(c, "roofed") && has_at_y(c, "forest"))
    });
    m.insert("minecraft:cherry_grove_after_surface_cherry_tree_feature_rules", |r, c| {
        run_object(r, c, "minecraft:cherry_grove_after_surface_cherry_tree_feature_rules", 2, 4, |b| tagged(b, "cherry_grove"), false, |w, r, x, y, z| {
            Cherry.generate(w, r, x, y, z);
        })
    });
    m.insert("minecraft:mangrove_swamp_mangrove_tree_feature", |r, c| {
        run_object(r, c, "minecraft:mangrove_swamp_mangrove_tree_feature", 12, 15, |b| tagged(b, "mangrove_swamp"), false, |w, r, x, y, z| {
            let mut t = Mangrove::new(r.next_float() > 0.15);
            t.with_bee_nest = r.next_float() < 0.04;
            t.generate(w, r, x, y, z);
        })
    });
    m.insert("minecraft:random_pale_oak_tree_feature", |r, c| {
        run_object(r, c, "minecraft:random_pale_oak_tree_feature", 8, 10, |b| tagged(b, "pale_garden"), false, |w, r, x, y, z| {
            PaleOak { try_creaking_heart: r.next_int_below(4) == 0 }.generate(w, r, x, y, z);
        })
    });
    m.insert("minecraft:swamp_oak_tree_feature", |r, c| {
        run_object(r, c, "minecraft:swamp_oak_tree_feature", 3, 5, |b| tagged(b, "swamp"), true, |w, r, x, y, z| {
            SwampOak { min_tree_height: 7, max_tree_height: 8 }.generate(w, r, x, y, z);
        })
    });
    m.insert("minecraft:jungle_bush", |r, c| {
        run_object(r, c, "minecraft:jungle_bush", 5, 6, |b| tagged(b, "jungle"), false, |w, r, x, y, z| {
            JungleBush.generate(w, r, x, y, z);
        })
    });
    m.insert("minecraft:mushroom_island_surface_huge_mushroom_feature", |r, c| {
        run_object(r, c, "minecraft:mushroom_island_surface_huge_mushroom_feature", 1, 2, |b| tagged(b, "mooshroom_island"), false, |w, r, x, y, z| {
            BigMushroom { kind: None }.generate(w, r, x, y, z);
        })
    });
    m.insert("minecraft:huge_mushroom_feature", |r, c| {
        run_object(r, c, "minecraft:huge_mushroom_feature", -4, 1, |b| tagged(b, "roofed") || tagged(b, "swamp") || tagged(b, "mooshroom_island"), false, |w, r, x, y, z| {
            BigMushroom { kind: None }.generate(w, r, x, y, z);
        })
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overworld::{Chunk, Overworld};

    #[test]
    fn block_states_exist() {
        for n in ["poplar_log", "red_poplar_leaves", "orange_poplar_leaves", "yellow_poplar_leaves", "shelf_mushroom", "leaf_litter", "cocoa", "short_grass", "brown_mushroom", "red_mushroom"] {
            blocks::id(n);
        }
        assert_ne!(shelf_mushroom(Face::East, 1), blocks::id("shelf_mushroom"));
        assert!(tagged(1, "plains"), "biome tags loaded");
    }

    #[test]
    fn legacy_count_policy() {
        let mut r = Xoroshiro::new(7);
        for _ in 0..1000 {
            assert!((0..=12).contains(&legacy_count(&mut r, 10.0)));
            assert!((0..=1).contains(&legacy_count(&mut r, 0.5)));
        }
    }

    fn count(c: &Chunk, pred: fn(i32) -> bool) -> usize {
        let mut n = 0;
        for x in 0..16 {
            for z in 0..16 {
                for y in -64..320 {
                    n += pred(c.get(x, y, z)) as usize;
                }
            }
        }
        n
    }

    #[test]
    fn forests_grow_trees() {
        let w = Overworld::new(12345);
        let (mut logs, mut leaves, mut tried) = (0, 0, 0);
        'outer: for r in 0..12i32 {
            for cx in -r..=r {
                for cz in -r..=r {
                    if cx.abs().max(cz.abs()) != r {
                        continue;
                    }
                    let (cx, cz) = (cx * 5, cz * 5);
                    let b = w.generate_terrain(cx, cz).surface_biome(8, 8);
                    if !(tagged(b, "forest") || tagged(b, "jungle") || tagged(b, "taiga")) {
                        continue;
                    }
                    let ch = w.generate(cx, cz);
                    logs += count(&ch, blocks::is_log);
                    leaves += count(&ch, blocks::is_leaves);
                    tried += 1;
                    if tried >= 3 {
                        break 'outer;
                    }
                }
            }
        }
        assert!(tried > 0, "no forest chunk found");
        assert!(logs > 0 && leaves > 0, "forest chunks have no trees: {logs} logs, {leaves} leaves");
    }
}

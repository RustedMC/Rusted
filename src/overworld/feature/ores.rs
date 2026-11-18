// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Ores and stone blobs (PNX `feature/ore`), plus underwater magma and glow lichen.
//!
//! PNX buffers writes in a BlockManager but reads the world, so we buffer them too.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::{blocks, chunk_hash, java_hash, random_range_triangle, Ctx, FeatureFn, Region, Xoroshiro};

const WORLD_MIN: i32 = -64;
const WORLD_MAX: i32 = 319;

/// What an ore turns its host into.
#[derive(Clone, Copy)]
enum Kind {
    /// Stone -> first, deepslate -> second (PNX `getState` switch), others unchanged.
    Ore(&'static str, &'static str),
    /// Always this block (stone blobs).
    Blob(&'static str),
}

struct Ore {
    name: &'static str,
    kind: Kind,
    count: i32,
    size: i32,
    min: i32,
    max: i32,
    triangle: bool,
    skip_air: f32,
    rare: bool,
    /// `AbstractOreUpperGeneratorFeature`: only plain stone is replaced.
    stone_only: bool,
}

#[allow(clippy::too_many_arguments)]
const fn o(name: &'static str, kind: Kind, count: i32, size: i32, min: i32, max: i32, triangle: bool, skip_air: f32, rare: bool, stone_only: bool) -> Ore {
    Ore { name, kind, count, size, min, max, triangle, skip_air, rare, stone_only }
}

const COAL: Kind = Kind::Ore("coal_ore", "deepslate_coal_ore");
const COPPER: Kind = Kind::Ore("copper_ore", "deepslate_copper_ore");
const DIAMOND: Kind = Kind::Ore("diamond_ore", "deepslate_diamond_ore");
const EMERALD: Kind = Kind::Ore("emerald_ore", "deepslate_emerald_ore");
const GOLD: Kind = Kind::Ore("gold_ore", "deepslate_gold_ore");
const IRON: Kind = Kind::Ore("iron_ore", "deepslate_iron_ore");
const LAPIS: Kind = Kind::Ore("lapis_ore", "deepslate_lapis_ore");
const REDSTONE: Kind = Kind::Ore("redstone_ore", "deepslate_redstone_ore");
const INFESTED: Kind = Kind::Ore("infested_stone", "infested_deepslate");
const T: bool = true;
const U: bool = false;

/// Values resolved through the Java class hierarchy (subclasses override some getters).
static ORES: [Ore; 30] = [
    o("minecraft:overworld_underground_coal_ore_upper_feature", COAL, 30, 17, 136, 320, U, 0.0, false, false),
    o("minecraft:overworld_underground_coal_ore_lower_feature", COAL, 20, 17, 0, 192, T, 0.5, false, false),
    o("minecraft:mountains_underground_coal_ore_feature", COAL, 20, 17, 128, 156, U, 0.0, false, false),
    o("minecraft:overworld_underground_copper_ore_feature", COPPER, 16, 10, -16, 112, T, 0.0, false, false),
    o("minecraft:dripstone_caves_copper_ore_feature", COPPER, 16, 20, -16, 112, T, 0.0, false, false),
    o("minecraft:overworld_underground_diamond_ore_feature_square", DIAMOND, 8, 2, -64, -4, U, 0.5, false, false),
    o("minecraft:overworld_underground_diamond_ore_feature", DIAMOND, 4, 7, -64, 16, T, 0.5, false, false),
    o("minecraft:overworld_underground_diamond_ore_buried_feature", DIAMOND, 8, 4, -64, 16, T, 1.0, false, false),
    o("minecraft:overworld_underground_diamond_ore_large_feature", DIAMOND, 9, 12, -64, 16, T, 0.7, true, false),
    o("minecraft:overworld_underground_emerald_ore_feature", EMERALD, 100, 3, -16, 420, U, 0.0, false, false),
    o("minecraft:extreme_hills_after_surface_silverfish_feature", INFESTED, 100, 3, -16, 420, U, 0.0, false, false),
    o("minecraft:mesa_underground_gold_ore_feature", GOLD, 50, 9, 32, 256, U, 0.0, false, false),
    o("minecraft:overworld_underground_gold_ore_feature", GOLD, 4, 9, -64, 32, T, 0.5, false, false),
    o("minecraft:overworld_underground_gold_ore_lower_feature", GOLD, 2, 9, -64, -48, U, 0.5, true, false),
    o("minecraft:overworld_underground_iron_ore_upper_feature", IRON, 90, 10, 80, 384, T, 0.0, false, false),
    o("minecraft:overworld_underground_iron_ore_middle_feature", IRON, 10, 10, -24, 56, T, 0.0, false, false),
    o("minecraft:overworld_underground_iron_ore_small_feature", IRON, 10, 4, -64, 72, U, 0.0, false, false),
    o("minecraft:overworld_underground_lapis_ore_buried_feature", LAPIS, 4, 7, -64, 64, U, 1.0, false, false),
    o("minecraft:overworld_underground_lapis_ore_feature", LAPIS, 2, 7, -32, 32, T, 1.0, false, false),
    o("minecraft:overworld_underground_redstone_ore_feature", REDSTONE, 4, 8, -64, 15, U, 0.0, false, false),
    o("minecraft:overworld_underground_redstone_ore_lower_feature", REDSTONE, 8, 8, -64, -32, T, 0.0, false, false),
    o("minecraft:overworld_underground_granite_lower_feature", Kind::Blob("granite"), 2, 64, 0, 60, U, 0.0, false, true),
    o("minecraft:overworld_underground_granite_upper_feature", Kind::Blob("granite"), 6, 64, 64, 128, U, 0.0, true, true),
    o("minecraft:overworld_underground_diorite_lower_feature", Kind::Blob("diorite"), 2, 64, 0, 60, U, 0.0, false, true),
    o("minecraft:overworld_underground_diorite_upper_feature", Kind::Blob("diorite"), 6, 64, 64, 128, U, 0.0, true, true),
    o("minecraft:overworld_underground_andesite_lower_feature", Kind::Blob("andesite"), 2, 64, 0, 60, U, 0.0, false, true),
    o("minecraft:overworld_underground_andesite_upper_feature", Kind::Blob("andesite"), 6, 64, 64, 128, U, 0.0, true, true),
    o("minecraft:overworld_underground_gravel_ore_feature", Kind::Blob("gravel"), 14, 33, -64, 320, U, 0.0, false, true),
    o("minecraft:overworld_underground_dirt_feature", Kind::Blob("dirt"), 7, 33, 0, 160, U, 0.0, false, false),
    o("minecraft:overworld_underground_tuff_feature", Kind::Blob("tuff"), 2, 64, -64, 0, U, 0.0, false, false),
];

fn ore<const I: usize>(r: &mut Region, c: &mut Ctx) {
    run_ore(&ORES[I], r, c)
}

macro_rules! ore_fns { ($($i:literal)*) => { [$(ore::<$i> as FeatureFn),*] } }
static ORE_FNS: [FeatureFn; 30] = ore_fns!(0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29);

/// PNX `MathHelper.sin/cos` lookup table.
fn sin_table() -> &'static [f32] {
    static T: OnceLock<Vec<f32>> = OnceLock::new();
    T.get_or_init(|| (0..65536).map(|i| (i as f64 * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32).collect())
}
fn msin(f: f32) -> f32 {
    sin_table()[((f * 10430.378f32) as i32 & 0xFFFF) as usize]
}
fn mcos(f: f32) -> f32 {
    sin_table()[((f * 10430.378f32 + 16384.0f32) as i32 & 0xFFFF) as usize]
}
fn floor(d: f64) -> i32 {
    let i = d as i32;
    if d < i as f64 { i - 1 } else { i }
}

struct Ids {
    stone: i32,
    deepslate: i32,
    netherrack: i32,
}

fn ids() -> &'static Ids {
    static I: OnceLock<Ids> = OnceLock::new();
    I.get_or_init(|| Ids { stone: blocks::id("stone"), deepslate: blocks::id("deepslate"), netherrack: blocks::id("netherrack") })
}

fn run_ore(ore: &Ore, r: &mut Region, c: &mut Ctx) {
    let ids = ids();
    let (on_stone, on_deep) = match ore.kind {
        Kind::Ore(a, b) => (blocks::id(a), blocks::id(b)),
        Kind::Blob(a) => (blocks::id(a), blocks::id(a)),
    };
    let blob = matches!(ore.kind, Kind::Blob(_));
    let replaceable = |b: i32| b == ids.stone || (!ore.stone_only && (b == ids.deepslate || b == ids.netherrack));
    // Host is already a default stone/deepslate/netherrack state (checked by `replaceable`).
    let state = |b: i32| if blob || b == ids.stone { on_stone } else if b == ids.deepslate { on_deep } else { b };

    let ch = chunk_hash(c.cx, c.cz);
    let rng = &mut c.random;
    rng.set_seed(ch ^ r.seed.wrapping_add(java_hash(ore.name) as i64));
    let (sx, sz) = (c.cx << 4, c.cz << 4);
    let mut manager: Vec<(i32, i32, i32, i32)> = Vec::new();
    let mut object: Vec<(i32, i32, i32, i32)> = Vec::new();
    let mut i = 0;
    loop {
        // The Java loop bound is re-evaluated (and re-rolled for rare ores) every iteration.
        let n = if ore.rare { (rng.next_int_below(ore.count) == 0) as i32 } else { ore.count };
        if i >= n {
            break;
        }
        i += 1;
        object.clear();
        let max_y = ore.max.min(WORLD_MAX);
        let min_y = ore.min.max(WORLD_MIN);
        let x = sx + rng.next_int_below(15);
        let z = sz + rng.next_int_below(15);
        let y = if ore.triangle { random_range_triangle(rng, min_y, max_y) } else { min_y + rng.next_bounded_int(max_y - min_y) };
        let original = r.get(x, y, z);
        if !replaceable(original) {
            continue;
        }
        if ore.size == 1 {
            object.push((x, y, z, state(original)));
        } else {
            rng.set_seed(r.seed ^ ch ^ x.wrapping_add(y).wrapping_add(z) as i64);
            spawn(r, rng, ore.size, x, y, z, &replaceable, &state, &mut object);
            rng.set_seed(r.seed ^ ch);
        }
        let mut skip = false;
        if ore.skip_air != 0.0 && object.iter().any(|&(x, y, z, _)| blocks::is_air(r.get(x, y, z))) {
            skip = rng.next_float() < ore.skip_air;
        }
        if !skip {
            manager.extend_from_slice(&object);
        }
    }
    for (x, y, z, b) in manager {
        r.set(x, y, z, b);
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn(r: &Region, rand: &mut Xoroshiro, size: i32, x: i32, y: i32, z: i32, replaceable: &dyn Fn(i32) -> bool, state: &dyn Fn(i32) -> i32, out: &mut Vec<(i32, i32, i32, i32)>) {
    let pi = std::f32::consts::PI;
    let sizef = size as f32;
    let pi_scaled = rand.next_float() * pi;
    let scale_max_x = ((x + 8) as f32 + msin(pi_scaled) * sizef / 8.0) as f64;
    let scale_min_x = ((x + 8) as f32 - msin(pi_scaled) * sizef / 8.0) as f64;
    let scale_max_z = ((z + 8) as f32 + mcos(pi_scaled) * sizef / 8.0) as f64;
    let scale_min_z = ((z + 8) as f32 - mcos(pi_scaled) * sizef / 8.0) as f64;
    let scale_max_y = (y + rand.next_bounded_int(3) - 2) as f64;
    let scale_min_y = (y + rand.next_bounded_int(3) - 2) as f64;
    for i in 0..size {
        let incr = i as f32 / sizef;
        let sxv = scale_max_x + (scale_min_x - scale_max_x) * incr as f64;
        let syv = scale_max_y + (scale_min_y - scale_max_y) * incr as f64;
        let szv = scale_max_z + (scale_min_z - scale_max_z) * incr as f64;
        let off = rand.next_double() * size as f64 / 16.0;
        let v1 = (msin(pi * incr) + 1.0) as f64 * off + 1.0;
        let v2 = (msin(pi * incr) + 1.0) as f64 * off + 1.0;
        let (h1, h2) = (v1 / 2.0, v2 / 2.0);
        for xs in floor(sxv - h1)..=floor(sxv + h1) {
            let xv = (xs as f64 + 0.5 - sxv) / h1;
            if xv * xv >= 1.0 {
                continue;
            }
            for ys in floor(syv - h2)..=floor(syv + h2) {
                if ys < -64 {
                    continue;
                }
                let yv = (ys as f64 + 0.5 - syv) / h2;
                if xv * xv + yv * yv >= 1.0 {
                    continue;
                }
                for zs in floor(szv - h1)..=floor(szv + h1) {
                    let zv = (zs as f64 + 0.5 - szv) / h1;
                    if xv * xv + yv * yv + zv * zv < 1.0 {
                        // Own earlier writes only turn hosts into non-hosts, so reading the
                        // world instead of the object cache gives the same blocks.
                        let orig = r.get(xs, ys, zs);
                        if replaceable(orig) {
                            out.push((xs, ys, zs, state(orig)));
                        }
                    }
                }
            }
        }
    }
}

/// PNX `CountGenerateFeature` seeding: returns the count.
fn count_seed(r: &Region, c: &mut Ctx, name: &str, base: i32, rand: i32) -> i32 {
    c.random.set_seed(r.seed ^ chunk_hash(c.cx, c.cz) ^ java_hash(name) as i64);
    base + c.random.next_bounded_int(rand)
}

const EMERALD_SURFACE: &str = "minecraft:extreme_hills_after_surface_emerald_ore_feature";

fn emerald_surface(r: &mut Region, c: &mut Ctx) {
    let n = count_seed(r, c, EMERALD_SURFACE, -2000, 2015);
    for _ in 0..n {
        let x = (c.cx << 4) + c.random.next_int_below(15);
        let z = (c.cz << 4) + c.random.next_int_below(15);
        let y = r.height(x, z) - 1;
        if blocks::is(r.get(x, y, z), "stone") {
            r.set(x, y, z, blocks::id("emerald_ore"));
        }
    }
}

const MAGMA: &str = "minecraft:overworld_underwater_magma_feature";

fn magma(r: &mut Region, c: &mut Ctx) {
    let rng = &mut c.random;
    rng.set_seed(r.seed ^ chunk_hash(c.cx, c.cz) ^ java_hash(MAGMA) as i64);
    let (bx, bz) = (c.cx << 4, c.cz << 4);
    let at = |x: i32, y: i32, z: i32| r.get(bx + x, y, bz + z);
    let solid = |x: i32, y: i32, z: i32| blocks::is_solid(at(x, y, z));
    let magma = blocks::id("magma");
    let mut out: Vec<(i32, i32, i32)> = Vec::new();
    let count = 44 + rng.next_int_below(9);
    let (min_y, max_y) = (WORLD_MIN, 256.min(WORLD_MAX));
    for _ in 0..count {
        let x = rng.next_int_below(16);
        let z = rng.next_int_below(16);
        let sy = min_y + rng.next_int_below(max_y - min_y + 1);
        let floor_y = 'f: {
            if !(WORLD_MIN..=WORLD_MAX).contains(&sy) || !blocks::is_water(at(x, sy, z)) {
                break 'f i32::MIN;
            }
            let lo = WORLD_MIN.max(sy - 5);
            (lo..sy).rev().find(|&y| !blocks::is_water(at(x, y, z))).unwrap_or(i32::MIN)
        };
        if floor_y <= min_y {
            continue;
        }
        let surface = r.height(bx + x, bz + z) - 1;
        let connected = sy >= surface || (sy + 1..=surface).all(|y| blocks::is_water(at(x, y, z)));
        if connected {
            continue;
        }
        for px in x - 1..=x + 1 {
            if !(0..=15).contains(&px) {
                continue;
            }
            for py in WORLD_MIN.max(floor_y - 1)..=WORLD_MAX.min(floor_y + 1) {
                for pz in z - 1..=z + 1 {
                    if !(0..=15).contains(&pz) || rng.next_float() >= 0.5 {
                        continue;
                    }
                    let t = at(px, py, pz);
                    let ok = !(blocks::is_water(t) || blocks::is_air(t))
                        && !(py - 1 < WORLD_MIN || px - 1 < 0 || px + 1 > 15 || pz - 1 < 0 || pz + 1 > 15)
                        && solid(px, py - 1, pz) && solid(px + 1, py, pz) && solid(px - 1, py, pz) && solid(px, py, pz + 1) && solid(px, py, pz - 1);
                    if ok {
                        out.push((bx + px, py, bz + pz));
                    }
                }
            }
        }
    }
    for (x, y, z) in out {
        r.set(x, y, z, magma);
    }
}

const LICHEN: &str = "minecraft:underground_glow_lichen_feature";
/// PNX BlockFace order: offsets and DUSWNE bit index.
const FACES: [((i32, i32, i32), u32); 6] = [((0, -1, 0), 0), ((0, 1, 0), 1), ((0, 0, -1), 4), ((0, 0, 1), 2), ((-1, 0, 0), 3), ((1, 0, 0), 5)];
/// `BlockFace.getEdges()` as an EnumSet (ordinal order), by face index.
const EDGES: [[usize; 4]; 6] = [[2, 3, 4, 5], [2, 3, 4, 5], [0, 1, 4, 5], [0, 1, 4, 5], [0, 1, 2, 3], [0, 1, 2, 3]];

fn lichen(r: &mut Region, c: &mut Ctx) {
    let n = count_seed(r, c, LICHEN, 52, 26);
    let lichen = blocks::id("glow_lichen");
    let (min_y, max_y) = (WORLD_MIN, 256.min(WORLD_MAX - 1));
    let mut all: Vec<(i32, i32, i32, i32)> = Vec::new();
    let mut obj: Vec<(i32, i32, i32, i32)> = Vec::new();
    for _ in 0..n {
        let rng = &mut c.random;
        obj.clear();
        let x = (c.cx << 4) + rng.next_int_below(14) + 1;
        let z = (c.cz << 4) + rng.next_int_below(14) + 1;
        let y = rng.next_int_range(min_y, max_y);
        let floor = (min_y..=max_y).rev().find(|&y| {
            let b = r.get(x, y, z);
            !blocks::is_air(b) && !blocks::is_liquid(b)
        });
        if y > floor.unwrap_or(min_y) - 13 {
            continue;
        }
        // The populate's own BlockManager: its writes first, then the world.
        let get = |obj: &Vec<(i32, i32, i32, i32)>, x, y, z| obj.iter().rev().find(|e| (e.0, e.1, e.2) == (x, y, z)).map_or_else(|| r.get(x, y, z), |e| e.3);
        let place = |obj: &mut Vec<_>, rng: &mut Xoroshiro, x: i32, y: i32, z: i32, pref: Option<usize>| -> Option<usize> {
            let cur = get(obj, x, y, z);
            let is_lichen = blocks::is(cur, "glow_lichen");
            if !blocks::is_air(cur) && !is_lichen {
                return None;
            }
            let bits = if is_lichen { match crate::blocks::state(cur, "multi_face_direction_bits") { Some(crate::blocks::State::Int(v)) => *v, _ => 0 } } else { 0 };
            let start = pref.unwrap_or_else(|| rng.next_int_below(5) as usize);
            for k in 0..6 {
                let f = (start + k) % 6;
                let ((dx, dy, dz), bi) = FACES[f];
                if !blocks::is_solid(get(obj, x + dx, y + dy, z + dz)) {
                    continue;
                }
                let bit = 1 << bi;
                if bits & bit != 0 {
                    continue;
                }
                obj.push((x, y, z, blocks::with(lichen, &[("multi_face_direction_bits", blocks::i(bits | bit))])));
                return Some(f);
            }
            None
        };
        let Some(face) = place(&mut obj, rng, x, y, z, None) else { continue };
        let target = 1 + rng.next_int_below(3);
        let (mut placed, mut cur, mut f) = (1, (x, y, z), face);
        let mut attempts = 0;
        while attempts < 8 && placed < target {
            attempts += 1;
            let e = EDGES[f][rng.next_int_below(3) as usize];
            let (dx, dy, dz) = FACES[e].0;
            let next = (cur.0 + dx, cur.1 + dy, cur.2 + dz);
            if let Some(nf) = place(&mut obj, rng, next.0, next.1, next.2, Some(f)) {
                placed += 1;
                cur = next;
                f = nf;
            }
        }
        all.extend_from_slice(&obj);
    }
    for (x, y, z, b) in all {
        r.set(x, y, z, b);
    }
}

pub fn register(m: &mut HashMap<&'static str, FeatureFn>) {
    for (ore, f) in ORES.iter().zip(ORE_FNS) {
        m.insert(ore.name, f);
    }
    // The biome data asks for these under slightly different identifiers than PNX's NAME.
    m.insert("minecraft:dripstone_caves_underground_copper_ore_feature", ORE_FNS[4]);
    m.insert(EMERALD_SURFACE, emerald_surface);
    m.insert(MAGMA, magma);
    m.insert(LICHEN, lichen);
    m.insert("minecraft:overworld_underground_glow_lichen_feature", lichen);
}

#[cfg(test)]
mod tests {
    #[test]
    fn ores_appear() {
        let ch = crate::overworld::Overworld::new(1).generate(0, 0);
        let (coal, iron) = (super::blocks::id("coal_ore"), super::blocks::id("iron_ore"));
        let (dc, di) = (super::blocks::id("deepslate_coal_ore"), super::blocks::id("deepslate_iron_ore"));
        let mut n = [0; 2];
        for x in 0..16 {
            for z in 0..16 {
                for y in -64..320 {
                    let b = ch.get(x, y, z);
                    n[0] += (b == coal || b == dc) as i32;
                    n[1] += (b == iron || b == di) as i32;
                }
            }
        }
        assert!(n[0] > 0 && n[1] > 0, "coal {} iron {}", n[0], n[1]);
    }
}

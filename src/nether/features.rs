// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
// from PNX NetherPopulatorStage, OreGeneratorFeature, the nether *Populator classes and
// LegacyNetherTree / LegacyCrimsonTree / LegacyWarpedTree.
//! Nether populators. Each chunk is decorated inside a 3x3 region so neighbours can spill in.
//! TODO: ruined portals and lava spring updates aren't ported yet.

use super::rng::NukkitRandom;
use super::{ids, Nether, NetherChunk, BASALT_DELTAS, CRIMSON_FOREST, WARPED_FOREST};
use crate::overworld::feature::blocks as bl;
use crate::overworld::feature::{chunk_hash, java_hash, random_range, random_range_triangle, Xoroshiro};

const W: i32 = 48;
const NONE: i32 = i32::MIN;

/// The chunk being decorated plus its 8 neighbours.
pub struct Region {
    cx: i32,
    cz: i32,
    chunks: Vec<NetherChunk>,
    /// Current populator object (PNX `BlockManager`), NONE if unset.
    obj: Vec<i32>,
    touched: Vec<u32>,
    /// Queued objects (PNX root `BlockManager`), written by `flush`.
    pending: Vec<(u32, i32)>,
}

impl Region {
    /// `chunks` in order dz -1..=1 (outer), dx -1..=1 (inner).
    pub fn new(cx: i32, cz: i32, chunks: Vec<NetherChunk>) -> Self {
        assert_eq!(chunks.len(), 9);
        Self { cx, cz, chunks, obj: vec![NONE; (W * W * 128) as usize], touched: Vec::new(), pending: Vec::new() }
    }

    fn index(&self, x: i32, y: i32, z: i32) -> Option<u32> {
        let (rx, rz) = (x - ((self.cx - 1) << 4), z - ((self.cz - 1) << 4));
        ((0..W).contains(&rx) && (0..W).contains(&rz) && (0..128).contains(&y)).then(|| ((rx * W + rz) * 128 + y) as u32)
    }

    fn chunk_of(&self, idx: u32) -> (usize, i32, i32, i32) {
        let (rxz, y) = (idx as i32 / 128, idx as i32 % 128);
        let (rx, rz) = (rxz / W, rxz % W);
        (((rz >> 4) * 3 + (rx >> 4)) as usize, rx & 15, y, rz & 15)
    }

    /// The world as written so far (PNX `level`).
    pub fn world(&self, x: i32, y: i32, z: i32) -> i32 {
        match self.index(x, y, z) {
            Some(i) => {
                let (c, lx, y, lz) = self.chunk_of(i);
                self.chunks[c].get(lx, y, lz)
            }
            None => ids().air,
        }
    }

    /// Reads through the current object.
    pub fn get(&self, x: i32, y: i32, z: i32) -> i32 {
        match self.index(x, y, z) {
            Some(i) if self.obj[i as usize] != NONE => self.obj[i as usize],
            _ => self.world(x, y, z),
        }
    }

    /// Stage a block in the current object (ignored outside the region).
    pub fn set(&mut self, x: i32, y: i32, z: i32, id: i32) {
        if let Some(i) = self.index(x, y, z) {
            if self.obj[i as usize] == NONE {
                self.touched.push(i);
            }
            self.obj[i as usize] = id;
        }
    }

    /// Writes straight to the world (PNX `level.setBlock`).
    pub fn set_now(&mut self, x: i32, y: i32, z: i32, id: i32) {
        if let Some(i) = self.index(x, y, z) {
            let (c, lx, y, lz) = self.chunk_of(i);
            self.chunks[c].set(lx, y, lz, id);
        }
    }

    /// PNX `queueObject`.
    pub fn commit(&mut self) {
        for &i in &self.touched {
            self.pending.push((i, self.obj[i as usize]));
            self.obj[i as usize] = NONE;
        }
        self.touched.clear();
    }

    /// PNX `root.applySubChunkUpdate`.
    fn flush(&mut self) {
        for (i, id) in std::mem::take(&mut self.pending) {
            let (c, lx, y, lz) = self.chunk_of(i);
            self.chunks[c].set(lx, y, lz, id);
        }
    }

    pub fn biome(&self, x: i32, z: i32) -> u16 {
        match self.index(x, 0, z) {
            Some(i) => {
                let (c, lx, _, lz) = self.chunk_of(i);
                self.chunks[c].biome(lx, 0, lz)
            }
            None => 0,
        }
    }

    fn into_centre(mut self) -> NetherChunk {
        self.chunks.swap_remove(4)
    }
}

pub struct Ctx<'a> {
    pub terrain: &'a super::terrain::Terrain,
    pub cx: i32,
    pub cz: i32,
    pub seed: i64,
    /// Shared Xoroshiro128 like PNX's thread-local one; most populators reseed it.
    pub random: Xoroshiro,
}

type PopFn = fn(&mut Region, &mut Ctx<'_>);

/// `NetherPopulatorStage.POPULATORS` up to the structures, in order.
const POPULATORS: [PopFn; 21] = [
    glowstone,
    |r, c| ore(&SOULSAND, r, c),
    |r, c| ore(&MAGMA, r, c),
    |r, c| ore(&LAVA_ORE, r, c),
    fire,
    lava,
    |r, c| ore(&GOLD, r, c),
    |r, c| ore(&DEBRIS_SMALL, r, c),
    |r, c| ore(&DEBRIS_LARGE, r, c),
    |r, c| ore(&QUARTZ, r, c),
    basalt_delta_lava,
    basalt_delta_pillar,
    basalt_delta_magma,
    crimson_fungi_tree,
    |r, c| grasses(r, c, CRIMSON_FOREST, true),
    crimson_weeping_vines,
    warped_fungi_tree,
    |r, c| grasses(r, c, WARPED_FOREST, false),
    warped_twisting_vines,
    |r, c| ore(&BLACKSTONE, r, c),
    |r, c| ore(&GRAVEL, r, c),
];

pub fn decorate(n: &Nether, mut region: Region) -> NetherChunk {
    let mut ctx = Ctx { terrain: &n.terrain, cx: 0, cz: 0, seed: n.seed, random: Xoroshiro::new(0) };
    for dz in -1..=1 {
        for dx in -1..=1 {
            ctx.cx = region.cx + dx;
            ctx.cz = region.cz + dz;
            for p in POPULATORS {
                p(&mut region, &mut ctx);
                region.commit();
            }
            region.flush();
        }
    }
    region.into_centre()
}

fn rx(c: &mut Ctx) -> (i32, i32) {
    let x = random_range(&mut c.random, c.cx << 4, (c.cx << 4) + 15);
    let z = random_range(&mut c.random, c.cz << 4, (c.cz << 4) + 15);
    (x, z)
}

fn reseed(c: &mut Ctx) {
    c.random.set_seed(c.seed ^ chunk_hash(c.cx, c.cz));
}

const HORIZONTAL: [(i32, i32); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];

// ---------------------------------------------------------------------------------------------
// Ores (PNX `OreGeneratorFeature`)

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    SoulSand,
    Magma,
    Lava,
    Gold,
    Debris,
    Quartz,
    Blackstone,
    Gravel,
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
}

const fn o(name: &'static str, kind: Kind, count: i32, size: i32, min: i32, max: i32, triangle: bool, skip_air: f32) -> Ore {
    Ore { name, kind, count, size, min, max, triangle, skip_air }
}

const SOULSAND: Ore = o("nether_soulsand", Kind::SoulSand, 12, 12, 0, 32, false, 0.0);
const MAGMA: Ore = o("nether_magma", Kind::Magma, 9, 28, 23, 36, false, 0.0);
const LAVA_ORE: Ore = o("nether_lava_ore", Kind::Lava, 32, 1, 0, 32, false, 0.0);
const GOLD: Ore = o("nether_nether_gold_ore", Kind::Gold, 10, 10, 10, 117, true, 0.0);
const DEBRIS_SMALL: Ore = o("nether_ancientdebris_small", Kind::Debris, 3, 2, 8, 119, false, 1.0);
const DEBRIS_LARGE: Ore = o("nether_ancientdebris_large", Kind::Debris, 2, 3, 8, 23, false, 1.0);
const QUARTZ: Ore = o("nether_quartz", Kind::Quartz, 20, 14, 10, 117, false, 0.0);
const BLACKSTONE: Ore = o("nether_blackstone", Kind::Blackstone, 2, 33, 5, 36, false, 0.0);
const GRAVEL: Ore = o("nether_gravel", Kind::Gravel, 2, 33, 5, 36, false, 0.0);

/// PNX `MathHelper.sin/cos` lookup table.
fn sin_table() -> &'static [f32] {
    static T: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
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

fn ore(ore: &Ore, r: &mut Region, c: &mut Ctx) {
    let i = ids();
    let state = match ore.kind {
        Kind::SoulSand => i.soul_sand,
        Kind::Magma => i.magma,
        Kind::Lava => i.lava,
        Kind::Gold => i.nether_gold_ore,
        Kind::Debris => i.ancient_debris,
        Kind::Quartz => i.quartz_ore,
        Kind::Blackstone => i.blackstone,
        Kind::Gravel => i.gravel,
    };
    let replaceable = |b: i32| b == i.stone || b == i.deepslate || b == i.netherrack;
    let ch = chunk_hash(c.cx, c.cz);
    let seed = c.seed;
    let rng = &mut c.random;
    rng.set_seed(ch ^ seed.wrapping_add(java_hash(ore.name) as i64));
    let (sx, sz) = (c.cx << 4, c.cz << 4);
    let mut object: Vec<(i32, i32, i32)> = Vec::new();
    for _ in 0..ore.count {
        object.clear();
        let (max_y, min_y) = (ore.max.min(super::MAX_Y), ore.min.max(super::MIN_Y));
        let x = sx + rng.next_int_below(15);
        let z = sz + rng.next_int_below(15);
        let y = if ore.triangle { random_range_triangle(rng, min_y, max_y) } else { min_y + rng.next_bounded_int(max_y - min_y) };
        if !replaceable(r.world(x, y, z)) {
            continue;
        }
        if ore.size == 1 {
            object.push((x, y, z));
        } else {
            rng.set_seed(seed ^ ch ^ x.wrapping_add(y).wrapping_add(z) as i64);
            // gravel and blackstone skip basalt deltas
            let skip = matches!(ore.kind, Kind::Gravel | Kind::Blackstone) && r.biome(x, z) == BASALT_DELTAS;
            if !skip {
                spawn(r, rng, ore.size, x, y, z, &replaceable, &mut object);
            }
            rng.set_seed(seed ^ ch);
        }
        let mut skip = false;
        if ore.skip_air != 0.0 && object.iter().any(|&(x, y, z)| r.world(x, y, z) == i.air) {
            skip = rng.next_float() < ore.skip_air;
        }
        if !skip {
            for &(x, y, z) in &object {
                r.set(x, y, z, state);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn(r: &Region, rand: &mut Xoroshiro, size: i32, x: i32, y: i32, z: i32, replaceable: &dyn Fn(i32) -> bool, out: &mut Vec<(i32, i32, i32)>) {
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
                    // we only ever turn hosts into ore, so reading via the populator object
                    // matches PNX's per-cluster object
                    if xv * xv + yv * yv + zv * zv < 1.0 && replaceable(r.get(xs, ys, zs)) {
                        out.push((xs, ys, zs));
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Glowstone, fire, lava

fn glowstone(r: &mut Region, c: &mut Ctx) {
    let i = ids();
    reseed(c);
    if c.random.next_int_below(11) != 0 {
        return;
    }
    let (x, z) = rx(c);
    // first air from y 125 down
    let mut y = 125;
    while y >= 0 && r.world(x, y, z) != i.air {
        y -= 1;
    }
    let y = if y == 0 { -1 } else { y };
    if y == -1 || r.world(x, y, z) == i.netherrack {
        return;
    }
    let rnd = &mut c.random;
    let mut count = random_range(rnd, 40, 60);
    r.set(x, y, z, i.glowstone);
    let mut cycles = 0;
    while count != 0 {
        if cycles == 1500 {
            break;
        }
        let sx = x + rnd.next_int_below(9) - rnd.next_int_below(9);
        let sy = y - rnd.next_int_below(9);
        let sz = z + rnd.next_int_below(9) - rnd.next_int_below(9);
        if cycles % 128 == 0 && cycles != 0 {
            let gx = x + rnd.next_int_range(-3, 3);
            let gy = y - rnd.next_int_below(5);
            let gz = z + rnd.next_int_range(-3, 3);
            r.set(gx, gy, gz, i.glowstone);
            count -= 1;
        }
        let around = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)].iter().any(|&(dx, dy, dz)| r.get(sx + dx, sy + dy, sz + dz) == i.glowstone);
        if around {
            r.set(sx, sy, sz, i.glowstone);
            count -= 1;
        }
        cycles += 1;
    }
}

fn fire(r: &mut Region, c: &mut Ctx) {
    let i = ids();
    reseed(c);
    let amount = c.random.next_int_below(2) + 1;
    let mut ys = Vec::new();
    for _ in 0..amount {
        let (x, z) = rx(c);
        ys.clear();
        for y in (1..=128).rev() {
            let b = r.get(x, y, z);
            if (b == i.netherrack || b == i.soul_sand || b == i.soul_soil) && r.get(x, y + 1, z) == i.air {
                ys.push(y + 1);
            }
        }
        for &y in &ys {
            if y <= 1 || c.random.next_int_below(4) == 1 {
                continue;
            }
            let f = if r.get(x, y - 1, z) == i.netherrack { i.fire } else { i.soul_fire };
            r.set(x, y, z, f);
        }
    }
}

fn lava(r: &mut Region, c: &mut Ctx) {
    let i = ids();
    reseed(c);
    let amount = c.random.next_bounded_int(30) - 29;
    for _ in 0..amount {
        let (x, z) = rx(c);
        let mut y = 127;
        while y >= 0 && r.get(x, y, z) != i.air {
            y -= 1;
        }
        let y = if y == 0 { -1 } else { y };
        if y <= 1 || c.random.next_int_below(4) == 1 {
            continue;
        }
        r.set_now(x, y + 1, z, i.lava);
    }
}

// ---------------------------------------------------------------------------------------------
// Basalt deltas

fn basalt_delta_lava(r: &mut Region, c: &mut Ctx) {
    let i = ids();
    // rolled before reseeding, so it continues the previous populator's random
    let amount = c.random.next_int_below(64) + 64;
    reseed(c);
    let mut ys = Vec::new();
    for _ in 0..amount {
        let (x, z) = rx(c);
        if r.biome(x, z) != BASALT_DELTAS {
            continue;
        }
        ys.clear();
        for y in (1..=128).rev() {
            let b = r.get(x, y, z);
            if (b == i.basalt || b == i.blackstone) && r.get(x, y + 1, z) == i.air && HORIZONTAL.iter().all(|&(dx, dz)| r.get(x + dx, y, z + dz) != i.air) {
                ys.push(y);
            }
        }
        for &y in &ys {
            if y > 1 {
                r.set(x, y, z, i.flowing_lava);
            }
        }
    }
    let (bx, bz) = (c.cx << 4, c.cz << 4);
    let t = c.terrain;
    for x in bx..bx + 16 {
        for z in bz..bz + 16 {
            if r.biome(x, z) != BASALT_DELTAS {
                continue;
            }
            for y in 1..127 {
                if r.get(x, y, z) != i.gravel {
                    continue;
                }
                let (fx, fy, fz) = (x as f64, y as f64, z as f64);
                let sec = t.delta_surface_sec.value(fx, fy, fz);
                let state = if sec < -0.9 { i.blackstone } else if sec < 0.8 { i.basalt } else { i.magma };
                if t.delta_surface.value(fx, fy, fz) > 0.0 || HORIZONTAL.iter().any(|&(dx, dz)| r.get(x + dx, y, z + dz) == i.air) {
                    r.set(x, y, z, state);
                } else {
                    r.set(x, y, z, i.lava);
                }
            }
        }
    }
}

fn basalt_delta_pillar(r: &mut Region, c: &mut Ctx) {
    let i = ids();
    reseed(c);
    let amount = c.random.next_bounded_int(128) + 128;
    let mut ys = Vec::new();
    for _ in 0..amount {
        let (x, z) = rx(c);
        if r.biome(x, z) != BASALT_DELTAS {
            continue;
        }
        ys.clear();
        for y in (2..=126).rev() {
            if r.get(x, y, z) == i.blackstone && r.get(x, y + 1, z) == i.air {
                ys.push(y + 1);
            }
        }
        for &y in &ys {
            if y <= 1 || c.random.next_bounded_int(5) == 0 {
                continue;
            }
            let mut h = 0;
            while h < c.random.next_bounded_int(5) + 1 {
                let p = y + h;
                if p >= 128 {
                    break;
                }
                r.set(x, p, z, i.basalt);
                h += 1;
            }
        }
    }
}

fn basalt_delta_magma(r: &mut Region, c: &mut Ctx) {
    let i = ids();
    reseed(c);
    let amount = c.random.next_bounded_int(4) + 20;
    let mut ys = Vec::new();
    for _ in 0..amount {
        let (x, z) = rx(c);
        if r.biome(x, z) != BASALT_DELTAS {
            continue;
        }
        ys.clear();
        for y in (1..=128).rev() {
            let b = r.get(x, y, z);
            if (b == i.basalt || b == i.blackstone)
                && r.get(x, y + 1, z) == i.air
                && HORIZONTAL.iter().any(|&(dx, dz)| {
                    let n = r.get(x + dx, y, z + dz);
                    n == i.lava || n == i.flowing_lava
                })
            {
                ys.push(y);
            }
        }
        for &y in &ys {
            if y > 1 {
                r.set(x, y, z, i.magma);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Crimson and warped forests

fn can_be_replaced(b: i32) -> bool {
    bl::replaceable(b)
}

fn fungi_tree(r: &mut Region, c: &mut Ctx, biome: u16, crimson: bool) {
    let i = ids();
    reseed(c);
    let amount = c.random.next_int_below(6) + 4;
    let nylium = if crimson { i.crimson_nylium } else { i.warped_nylium };
    let mut ys = Vec::new();
    for _ in 0..amount {
        let (x, z) = rx(c);
        if r.biome(x, z) != biome {
            continue;
        }
        ys.clear();
        for y in (1..=128).rev() {
            if r.get(x, y, z) == nylium && can_be_replaced(r.get(x, y + 1, z)) {
                ys.push(y + 1);
            }
        }
        for &y in &ys {
            if y <= 1 || c.random.next_int_below(4) == 1 {
                continue;
            }
            nether_tree(r, c, x, y, z, crimson);
        }
    }
}

fn crimson_fungi_tree(r: &mut Region, c: &mut Ctx) {
    fungi_tree(r, c, CRIMSON_FOREST, true)
}

fn warped_fungi_tree(r: &mut Region, c: &mut Ctx) {
    fungi_tree(r, c, WARPED_FOREST, false)
}

fn grasses(r: &mut Region, c: &mut Ctx, biome: u16, crimson: bool) {
    let i = ids();
    reseed(c);
    let amount = c.random.next_int_below(128) + 192;
    let (nylium, fungus, roots) = if crimson { (i.crimson_nylium, i.crimson_fungus, i.crimson_roots) } else { (i.warped_nylium, i.warped_fungus, i.warped_roots) };
    let mut ys = Vec::new();
    for _ in 0..amount {
        let (x, z) = rx(c);
        if r.biome(x, z) != biome {
            continue;
        }
        ys.clear();
        for y in (1..=128).rev() {
            if r.get(x, y, z) == nylium && r.get(x, y + 1, z) == i.air {
                ys.push(y + 1);
            }
        }
        for &y in &ys {
            if y <= 1 {
                continue;
            }
            let b = if c.random.next_int_below(6) == 0 { fungus } else { roots };
            r.set(x, y, z, b);
        }
    }
}

fn crimson_weeping_vines(r: &mut Region, c: &mut Ctx) {
    let i = ids();
    reseed(c);
    let amount = c.random.next_int_below(5) + 1;
    let ends = [i.netherrack, i.crimson_nylium, i.nether_wart_block, i.lava, i.flowing_lava, i.crimson_fungus, i.crimson_roots, i.quartz_ore, i.nether_gold_ore, i.ancient_debris];
    let mut ys = Vec::new();
    for _ in 0..amount {
        let (x, z) = rx(c);
        if r.biome(x, z) != CRIMSON_FOREST {
            continue;
        }
        ys.clear();
        for y in (1..=128).rev() {
            let b = r.get(x, y, z);
            if (b == i.crimson_nylium || b == i.netherrack) && r.get(x, y - 1, z) == i.air {
                ys.push(y - 1);
            }
        }
        for &y in &ys {
            if y <= 1 {
                continue;
            }
            // getHighestEndingBlock
            let mut e = y;
            while e > 0 {
                if r.get(x, e + 1, z) == i.air && ends.contains(&r.get(x, e, z)) {
                    break;
                }
                e -= 1;
            }
            let end_y = e + 1;
            let n = c.random.next_int_below(y - end_y);
            let mut p = y;
            while p > y - n {
                r.set(x, p, z, i.weeping_vines);
                p -= 1;
            }
        }
    }
}

fn warped_twisting_vines(r: &mut Region, c: &mut Ctx) {
    let i = ids();
    reseed(c);
    let amount = c.random.next_int_below(6) + 2;
    let mut ys = Vec::new();
    for _ in 0..amount {
        let (x, z) = rx(c);
        if r.biome(x, z) != WARPED_FOREST {
            continue;
        }
        ys.clear();
        for y in (1..=128).rev() {
            let b = r.get(x, y, z);
            if (b == i.warped_nylium || b == i.warped_wart) && can_be_replaced(r.get(x, y + 1, z)) {
                ys.push(y + 1);
            }
        }
        for &y in &ys {
            if y <= 1 || c.random.next_int_below(5) == 0 {
                continue;
            }
            let mut e = y;
            while e < 128 {
                if r.get(x, e - 1, z) == i.air && bl::is_solid(r.get(x, e, z)) {
                    break;
                }
                e += 1;
            }
            let end_y = e - 1;
            let n = c.random.next_int_below(end_y - y + 1);
            for p in y..y + n / 2 {
                r.set(x, p, z, i.twisting_vines);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// LegacyNetherTree (crimson / warped fungi)

fn overridable(b: i32) -> bool {
    b == ids().air || bl::is_leaves(b) || bl::short(b).ends_with("sapling") || bl::is(b, "snow_layer")
}

fn nether_tree(r: &mut Region, c: &mut Ctx, x: i32, y: i32, z: i32, crimson: bool) {
    let i = ids();
    // TODO: PNX rolls the height from a clock-seeded random (not reproducible),
    // we use seed + position instead.
    let height = NukkitRandom::new(c.seed ^ chunk_hash(x, z) ^ y as i64).next_int_max(9) + 4;
    let (trunk, leaf) = if crimson { (i.crimson_stem, i.nether_wart_block) } else { (i.warped_stem, i.warped_wart) };
    let check_y = |y: i32| y > 126;
    if check_y(y) {
        return;
    }
    let rnd = &mut c.random;
    // placeTrunk
    r.set(x, y, z, trunk);
    for yy in 0..height {
        if check_y(y + yy) {
            continue;
        }
        if overridable(r.get(x, y + yy, z)) {
            r.set(x, y + yy, z, trunk);
        }
    }
    let mid = 2; // (int) (1 - -3 / 2.0)
    for yy in y - 3 + height..=y + height - 1 {
        if check_y(yy) {
            continue;
        }
        for xx in x - mid..=x + mid {
            let x_off = (xx - x).abs();
            let mut zz = z - mid;
            while zz <= z + mid {
                let z_off = (zz - z).abs();
                if !(x_off == mid && z_off == mid && rnd.next_int_below(2) == 0) && !bl::is_solid(r.get(xx, yy, zz)) {
                    let b = if rnd.next_int_below(20) == 0 { i.shroomlight } else { leaf };
                    r.set(xx, yy, zz, b);
                }
                zz += mid * 2;
            }
        }
        for zz in z - mid..=z + mid {
            let z_off = (zz - z).abs();
            let mut xx = x - mid;
            while xx <= x + mid {
                let x_off = (xx - x).abs();
                if !(x_off == mid && z_off == mid && rnd.next_int_below(2) == 0) && !bl::is_solid(r.get(xx, yy, zz)) {
                    let b = if rnd.next_int_below(20) == 0 { i.shroomlight } else { leaf };
                    r.set(xx, yy, zz, b);
                }
                xx += mid * 2;
            }
        }
    }
    for yy in y - 4 + height..=y + height - 3 {
        if check_y(yy) {
            continue;
        }
        for xx in x - mid..=x + mid {
            let mut zz = z - mid;
            while zz <= z + mid {
                hang(r, rnd, xx, yy, zz, 5, leaf);
                zz += mid * 2;
            }
        }
        for zz in z - mid..=z + mid {
            let mut xx = x - mid;
            while xx <= x + mid {
                hang(r, rnd, xx, yy, zz, 4, leaf);
                xx += mid * 2;
            }
        }
    }
    for xc in x - mid + 1..=x + mid - 1 {
        for zc in z - mid + 1..=z + mid - 1 {
            if !bl::is_solid(r.get(xc, y + height, zc)) {
                r.set(xc, y + height, zc, leaf);
            }
        }
    }
}

/// Hanging wart under a canopy edge. Loop bound re-rolls each iteration like PNX.
fn hang(r: &mut Region, rnd: &mut Xoroshiro, x: i32, y: i32, z: i32, n: i32, leaf: i32) {
    if bl::is_solid(r.get(x, y, z)) || rnd.next_int_below(3) != 0 {
        return;
    }
    let mut k = 0;
    while k < rnd.next_int_below(n) {
        if !bl::is_solid(r.get(x, y - k, z)) {
            r.set(x, y - k, z, leaf);
        }
        k += 1;
    }
}

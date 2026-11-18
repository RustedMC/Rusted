// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
// Ported from PNX TheEnd, TheEndTerrainStage, TheEndPopulatorStage, TheEndBiomePicker,
// TheEndObjectHolder, the_end/* populators, ObjectEndGateway, ObjectEndIsland,
// ObjectObsidianPillar, ObjectExitPortal, LegacyChorusTree, EndCityPopulator,
// EndDragonFight.initializeGateways/spawnGateway (positions only).
//! The End: main island, outer islands, pillars, exit portal, chorus plants, end cities.
//! Cross-chunk objects are rebuilt from their source chunk or cached, so chunk order doesn't matter.
//! TODO: populators only see bare terrain and overlaps apply in a fixed order, unlike PNX.

#![allow(dead_code)] // public API is wired into the server separately

mod city;
mod noise;
mod templates;

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::Mutex;

use crate::blocks::State;
use crate::overworld::feature::blocks as bl;
use crate::overworld::feature::{chunk_hash, NukkitRandom, Xoroshiro};
use noise::{Octaves, Simplex};

pub const MIN_Y: i32 = 0;
pub const MAX_Y: i32 = 255;
/// Bedrock `the_end`.
const BIOME: u16 = 9;
const COORD_SCALE: f64 = 684.412;
const CITY_SALT: i32 = 0x009E_7F71;
const CITY_SPACING: i32 = 20;
const CITY_SEPARATION: i32 = 11;
/// Chunks around a city start its pieces may reach (bridges + ship stay well inside).
const CITY_REACH: i32 = 12;

type Block = (i32, i32, i32, i32);

/// PNX `ObjectObsidianPillar.BedrockMersenneTwister` (= `BedrockRandom`'s raw output).
struct Mt {
    s: [u32; 624],
    i: usize,
}

impl Mt {
    fn new(seed: i32) -> Self {
        let mut s = [0u32; 624];
        s[0] = seed as u32;
        for i in 1..624 {
            let p = s[i - 1];
            s[i] = 0x6c07_8965u32.wrapping_mul(p ^ (p >> 30)).wrapping_add(i as u32);
        }
        Self { s, i: 624 }
    }

    fn next(&mut self) -> u32 {
        if self.i >= 624 {
            for i in 0..624 {
                let v = (self.s[i] & 0x8000_0000) | (self.s[(i + 1) % 624] & 0x7fff_ffff);
                self.s[i] = self.s[(i + 397) % 624] ^ (v >> 1);
                if v & 1 != 0 {
                    self.s[i] ^= 0x9908_b0df;
                }
            }
            self.i = 0;
        }
        let mut v = self.s[self.i];
        self.i += 1;
        v ^= v >> 11;
        v ^= (v << 7) & 0x9d2c_5680;
        v ^= (v << 15) & 0xefc6_0000;
        v ^= v >> 18;
        v
    }

    /// The pillar / gateway shuffle: `remainderUnsigned(nextInt(), i + 1)`.
    fn shuffled(seed: i64, n: usize) -> Vec<usize> {
        let mut r = Mt::new(seed as i32);
        let mut v: Vec<usize> = (0..n).collect();
        for i in 1..n {
            let j = (r.next() % (i as u32 + 1)) as usize;
            v.swap(i, j);
        }
        v
    }
}

/// End terrain of one chunk: a solid bit per y (0..128) per column.
struct Terrain {
    cols: [u128; 256],
}

impl Terrain {
    fn solid(&self, lx: i32, y: i32, lz: i32) -> bool {
        (0..128).contains(&y) && self.cols[((lz << 4) | lx) as usize] >> y & 1 != 0
    }
    /// PNX height map: highest block + 1, 0 for an empty column.
    fn height(&self, lx: i32, lz: i32) -> i32 {
        128 - self.cols[((lz << 4) | lx) as usize].leading_zeros() as i32
    }
}

struct Ids {
    air: i32,
    end_stone: i32,
    obsidian: i32,
    bedrock: i32,
    bedrock_burn: i32,
    iron_bars: i32,
    fire: i32,
    gateway: i32,
    plant: i32,
    flower: i32,
}

#[derive(Default)]
struct City {
    /// World blocks per chunk hash.
    chunks: HashMap<i64, Vec<Block>>,
    shulkers: Vec<(i32, i32, i32)>,
}

pub struct End {
    seed: i64,
    rough: Octaves,
    rough2: Octaves,
    detail: Octaves,
    /// Terrain stage island noise (Xoroshiro holder).
    island_terrain: Simplex,
    /// Populators' island noise (`new NukkitRandom(seed)`).
    island_pop: Simplex,
    ids: Ids,
    pillars: Vec<(i32, i32, i32, i32, bool)>,
    /// Pillars + exit portal, per chunk hash.
    main: HashMap<i64, Vec<Block>>,
    terrain: Mutex<HashMap<i64, Arc<Terrain>>>,
    cities: Mutex<HashMap<i64, Arc<City>>>,
}

pub struct EndChunk {
    blocks: Vec<i32>,
}

impl EndChunk {
    pub fn get(&self, lx: i32, y: i32, lz: i32) -> i32 {
        if !(0..16).contains(&lx) || !(0..16).contains(&lz) || !(MIN_Y..=MAX_Y).contains(&y) {
            return bl::air();
        }
        self.blocks[((y << 8) | (lz << 4) | lx) as usize]
    }

    pub fn biome(&self, _lx: i32, _y: i32, _lz: i32) -> u16 {
        BIOME
    }

    fn set(&mut self, cx: i32, cz: i32, b: &Block) {
        let (lx, lz) = (b.0 - (cx << 4), b.2 - (cz << 4));
        if (0..16).contains(&lx) && (0..16).contains(&lz) && (MIN_Y..=MAX_Y).contains(&b.1) {
            self.blocks[((b.1 << 8) | (lz << 4) | lx) as usize] = b.3;
        }
    }
}

fn pack(x: i32, y: i32, z: i32) -> i64 {
    ((x as i64 & 0x3ff_ffff) << 38) | ((z as i64 & 0x3ff_ffff) << 12) | (y as i64 & 0xfff)
}

/// PNX `TheEndTerrainStage.getIslandHeight`.
fn island_height(cx: i32, cz: i32, x: i32, z: i32, n: &Simplex) -> f32 {
    let x1 = (cx * 2 + x) as f32;
    let z1 = (cz * 2 + z) as f32;
    let mut h = (100.0 - (x1 * x1 + z1 * z1).sqrt() * 8.0).clamp(-100.0, 80.0);
    for i in -12..=12 {
        for j in -12..=12 {
            let x2 = cx as i64 + i;
            let z2 = cz as i64 + j;
            if x2 * x2 + z2 * z2 > 4096 && n.value(x2 as f64, z2 as f64) < (-0.9f32) as f64 {
                let x1 = (x as i64 - i * 2) as f32;
                let z1 = (z as i64 - j * 2) as f32;
                let f = ((x2 as f32).abs() * 3439.0 + (z2 as f32).abs() * 147.0) % 13.0 + 9.0;
                let h2 = (100.0 - (x1 * x1 + z1 * z1).sqrt() * f).clamp(-100.0, 80.0);
                h = h.max(h2);
            }
        }
    }
    h
}

fn far(cx: i32, cz: i32) -> bool {
    (cx as i64) * (cx as i64) + (cz as i64) * (cz as i64) > 4096
}

/// Local block view for object generators: own staged blocks over the terrain.
struct Stage<'a> {
    end: &'a End,
    map: HashMap<i64, i32>,
    out: &'a mut Vec<Block>,
}

impl Stage<'_> {
    fn set(&mut self, x: i32, y: i32, z: i32, v: i32) {
        self.map.insert(pack(x, y, z), v);
        self.out.push((x, y, z, v));
    }
    fn get(&self, x: i32, y: i32, z: i32) -> i32 {
        self.map.get(&pack(x, y, z)).copied().unwrap_or_else(|| self.end.terrain_block(x, y, z))
    }
}

const HORIZONTAL: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)]; // N, E, S, W
const FACES: [(i32, i32, i32); 6] = [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)]; // D U N S W E

impl End {
    pub fn new(seed: u64) -> Self {
        let seed = seed as i64;
        let holder = Xoroshiro::new(seed);
        let ids = Ids {
            air: bl::air(),
            end_stone: bl::id("end_stone"),
            obsidian: bl::id("obsidian"),
            bedrock: bl::id("bedrock"),
            bedrock_burn: bl::with(bl::id("bedrock"), &[("infiniburn_bit", bl::b(true))]),
            iron_bars: bl::id("iron_bars"),
            fire: bl::id("fire"),
            gateway: bl::id("end_gateway"),
            plant: bl::id("chorus_plant"),
            flower: bl::with(bl::id("chorus_flower"), &[("age", bl::i(5))]),
        };
        let order = Mt::shuffled(seed, 10);
        let pillars = (0..10)
            .map(|i| {
                let a = 2.0 * (i as f64 * std::f64::consts::PI / 10.0 - std::f64::consts::PI);
                let p = order[i] as i32;
                ((42.0 * a.cos()) as i32, (42.0 * a.sin()) as i32, 2 + p / 3, 76 + p * 3, p == 1 || p == 2)
            })
            .collect();
        let mut end = Self {
            seed,
            rough: Octaves::new(&mut holder.identical(), 16),
            rough2: Octaves::new(&mut holder.identical(), 16),
            detail: Octaves::new(&mut holder.identical(), 8),
            island_terrain: Simplex::new(&mut holder.identical()),
            island_pop: Simplex::new(&mut NukkitRandom::new(seed)),
            ids,
            pillars,
            main: HashMap::new(),
            terrain: Mutex::default(),
            cities: Mutex::default(),
        };
        let mut blocks = vec![];
        end.place_pillars(&mut blocks);
        end.place_exit_portal(&mut blocks);
        for b in blocks {
            end.main.entry(chunk_hash(b.0 >> 4, b.2 >> 4)).or_default().push(b);
        }
        end
    }

    /// The exit portal's floor y (its open centre), PNX's `height(16, 0) - 1`.
    pub fn exit_portal_y(&self) -> i32 {
        self.height(16, 0) - 1
    }

    /// The 10 pillars as (x, z, radius, height), PNX `ObjectObsidianPillar.fromSeed` order.
    pub fn pillars(&self) -> Vec<(i32, i32, i32, i32)> {
        self.pillars.iter().map(|p| (p.0, p.1, p.2, p.3)).collect()
    }

    /// The 20 gateway spots (x, 75, z), in the order PNX opens them after each dragon kill.
    pub fn gateways(&self) -> Vec<(i32, i32, i32)> {
        Mt::shuffled(self.seed, 20)
            .into_iter()
            .rev()
            .map(|g| {
                let a = 2.0 * (g as f64 * std::f64::consts::PI / 20.0 - std::f64::consts::PI);
                ((a.cos() * 96.0) as i32, 75, (a.sin() * 96.0) as i32)
            })
            .collect()
    }

    pub fn generate(&self, cx: i32, cz: i32) -> EndChunk {
        let mut c = EndChunk { blocks: vec![self.ids.air; 16 * 16 * 256] };
        let t = self.terrain_of(cx, cz);
        for lz in 0..16 {
            for lx in 0..16 {
                let mut col = t.cols[((lz << 4) | lx) as usize];
                while col != 0 {
                    let y = col.trailing_zeros() as i32;
                    col &= col - 1;
                    c.blocks[((y << 8) | (lz << 4) | lx) as usize] = self.ids.end_stone;
                }
            }
        }
        for b in self.main.get(&chunk_hash(cx, cz)).into_iter().flatten() {
            c.set(cx, cz, b);
        }
        let mut buf = vec![];
        for sx in cx - 1..=cx + 1 {
            for sz in cz - 1..=cz + 1 {
                buf.clear();
                self.populate(sx, sz, &mut buf);
                for b in &buf {
                    c.set(cx, cz, b);
                }
            }
        }
        for city in self.cities_near(cx, cz) {
            for b in city.chunks.get(&chunk_hash(cx, cz)).into_iter().flatten() {
                c.set(cx, cz, b);
            }
        }
        c
    }

    /// Entities PNX spawns here: pillar crystals, the dragon (chunk 1, 0), city shulkers.
    pub fn entities(&self, cx: i32, cz: i32) -> Vec<(&'static str, (f32, f32, f32))> {
        let mut v = vec![];
        for p in &self.pillars {
            if p.0 >> 4 == cx && p.1 >> 4 == cz {
                v.push(("minecraft:ender_crystal", (p.0 as f32 + 0.5, (p.3 + 1) as f32, p.1 as f32 + 0.5)));
            }
        }
        if (cx, cz) == (1, 0) {
            v.push(("minecraft:ender_dragon", (0.5, 128.0, 0.5)));
        }
        for city in self.cities_near(cx, cz) {
            for s in &city.shulkers {
                if s.0 >> 4 == cx && s.2 >> 4 == cz {
                    v.push(("minecraft:shulker", (s.0 as f32 + 0.5, s.1 as f32, s.2 as f32 + 0.5)));
                }
            }
        }
        v
    }

    // -----------------------------------------------------------------------------------------
    // Terrain

    fn terrain_of(&self, cx: i32, cz: i32) -> Arc<Terrain> {
        let k = chunk_hash(cx, cz);
        if let Some(t) = self.terrain.lock().get(&k) {
            return t.clone();
        }
        let t = Arc::new(self.build_terrain(cx, cz));
        let mut m = self.terrain.lock();
        if m.len() >= 4096 {
            m.clear();
        }
        m.entry(k).or_insert(t).clone()
    }

    fn terrain_block(&self, x: i32, y: i32, z: i32) -> i32 {
        if self.terrain_of(x >> 4, z >> 4).solid(x & 15, y, z & 15) { self.ids.end_stone } else { self.ids.air }
    }

    fn height(&self, x: i32, z: i32) -> i32 {
        self.terrain_of(x >> 4, z >> 4).height(x & 15, z & 15)
    }

    /// PNX `TheEndTerrainStage.apply`.
    fn build_terrain(&self, cx: i32, cz: i32) -> Terrain {
        let off = [cx << 1, 0, cz << 1];
        let size = [3, 33, 3];
        let mut detail = [0.0; 297];
        let mut r1 = [0.0; 297];
        let mut r2 = [0.0; 297];
        self.detail.generate(&mut detail, off, size, [COORD_SCALE * 2.0 / 80.0, 4.277575000000001, COORD_SCALE * 2.0 / 80.0]);
        self.rough.generate(&mut r1, off, size, [COORD_SCALE * 2.0, COORD_SCALE, COORD_SCALE * 2.0]);
        self.rough2.generate(&mut r2, off, size, [COORD_SCALE * 2.0, COORD_SCALE, COORD_SCALE * 2.0]);
        let mut density = [[[0.0f64; 33]; 3]; 3];
        let mut index = 0;
        for (i, di) in density.iter_mut().enumerate() {
            for (j, dj) in di.iter_mut().enumerate() {
                let h = island_height(cx, cz, i as i32, j as i32, &self.island_terrain);
                for (k, d) in dj.iter_mut().enumerate() {
                    let nr = r1[index] / 512.0;
                    let nr2 = r2[index] / 512.0;
                    let nd = (detail[index] / 10.0 + 1.0) / 2.0;
                    let mut dens = if nd < 0.0 { nr } else if nd > 1.0 { nr2 } else { nr + (nr2 - nr) * nd };
                    dens = (dens - 8.0) + h as f64;
                    index += 1;
                    if k < 8 {
                        let low = ((8 - k) as f32 / 7.0) as f64;
                        dens = dens * (1.0 - low) + low * -30.0;
                    } else if k > 33 / 2 - 2 {
                        let low = (((k - (33 / 2 - 2)) as f32) as f64 / 64.0).clamp(0.0, 1.0);
                        dens = dens * (1.0 - low) + low * -3000.0;
                    }
                    *d = dens;
                }
            }
        }
        let mut t = Terrain { cols: [0; 256] };
        for i in 0..2 {
            for j in 0..2 {
                for k in 0..32 {
                    let mut d1 = density[i][j][k];
                    let mut d2 = density[i + 1][j][k];
                    let mut d3 = density[i][j + 1][k];
                    let mut d4 = density[i + 1][j + 1][k];
                    let d5 = (density[i][j][k + 1] - d1) / 4.0;
                    let d6 = (density[i + 1][j][k + 1] - d2) / 4.0;
                    let d7 = (density[i][j + 1][k + 1] - d3) / 4.0;
                    let d8 = (density[i + 1][j + 1][k + 1] - d4) / 4.0;
                    for l in 0..4 {
                        let mut d9 = d1;
                        let mut d10 = d3;
                        for m in 0..8 {
                            let mut dens = d9;
                            for n in 0..8 {
                                if dens > 0.0 {
                                    let (x, y, z) = (m + (i << 3), l + (k << 2), n + (j << 3));
                                    t.cols[(z << 4) | x] |= 1u128 << y;
                                }
                                dens += (d10 - d9) / 8.0;
                            }
                            d9 += (d2 - d1) / 8.0;
                            d10 += (d4 - d3) / 8.0;
                        }
                        d1 += d5;
                        d3 += d7;
                        d2 += d6;
                        d4 += d8;
                    }
                }
            }
        }
        t
    }

    // -----------------------------------------------------------------------------------------
    // Main island objects

    /// PNX `ObjectObsidianPillar.generate` for all 10 (crystals are in `entities`).
    fn place_pillars(&self, out: &mut Vec<Block>) {
        let ids = &self.ids;
        for &(x, z, radius, height, guarded) in &self.pillars {
            for i in 0..height {
                for j in -radius..=radius {
                    for k in -radius..=radius {
                        if j * j + k * k <= radius * radius + 1 {
                            out.push((x + j, i, z + k, ids.obsidian));
                        }
                    }
                }
            }
            if guarded {
                for i in -2..=2i32 {
                    for j in -2..=2i32 {
                        if i.abs() == 2 || j.abs() == 2 {
                            for k in 0..3 {
                                out.push((x + i, height + k, z + j, ids.iron_bars));
                            }
                        }
                        out.push((x + i, height + 3, z + j, ids.iron_bars));
                    }
                }
            }
            out.push((x, height, z, ids.bedrock_burn));
            out.push((x, height + 1, z, ids.fire));
        }
    }

    /// PNX `ExitPortalPopulator`, unlit. PNX reads the height at world (16, 0), so we do too.
    fn place_exit_portal(&self, out: &mut Vec<Block>) {
        let ids = &self.ids;
        let oy = self.height(16, 0) - 1;
        for x in -4..=4 {
            for y in -1..=32 {
                for z in -4..=4 {
                    let d = x * x + y * y + z * z;
                    if d >= 13 {
                        continue;
                    }
                    let v = if y < 0 {
                        if d < 7 { ids.bedrock_burn } else { ids.end_stone }
                    } else if y > 0 || d < 7 {
                        ids.air
                    } else {
                        ids.bedrock_burn
                    };
                    out.push((x, oy + y, z, v));
                }
            }
        }
        for y in 0..4 {
            out.push((0, oy + y, 0, ids.bedrock_burn));
        }
        let torch = bl::id("torch");
        for ((dx, dz), dir) in HORIZONTAL.into_iter().zip(["south", "west", "north", "east"]) {
            out.push((dx, oy + 2, dz, bl::with(torch, &[("torch_facing_direction", bl::s(dir))])));
        }
    }

    // -----------------------------------------------------------------------------------------
    // Outer populators

    /// Chorus, gateway and island populators of source chunk (sx, sz), in PNX order.
    fn populate(&self, sx: i32, sz: i32, out: &mut Vec<Block>) {
        if !far(sx, sz) {
            return;
        }
        let seed = self.seed ^ chunk_hash(sx, sz);
        let outer = island_height(sx, sz, 1, 1, &self.island_pop);
        // ChorusFlowerPopulator (Xoroshiro128 random)
        if outer > 40.0 {
            let mut r = Xoroshiro::new(seed);
            let mut i = 0;
            while i < r.next_bounded_int(5) {
                let x = (sx << 4) + r.next_bounded_int(16);
                let z = (sz << 4) + r.next_bounded_int(16);
                let y = self.height(x, z) - 1;
                if y > 0 && self.terrain_block(x, y + 1, z) == self.ids.air && self.terrain_block(x, y, z) == self.ids.end_stone {
                    let mut st = Stage { end: self, map: HashMap::new(), out };
                    self.chorus_tree(&mut st, &mut r, (x, y + 1, z), 8);
                }
                i += 1;
            }
        }
        // EndGatewayPopulator.
        let mut r = NukkitRandom::new(seed);
        if outer > 40.0 && r.next_bounded_int(700) == 0 {
            let x = (sx << 4) + r.next_bounded_int(16);
            let z = (sz << 4) + r.next_bounded_int(16);
            let y = self.height(x, z) + r.next_bounded_int(7) + 2;
            if y > 1 && y < 254 {
                for (dy, b) in [(1, self.ids.bedrock), (-1, self.ids.bedrock)] {
                    for f in FACES {
                        out.push((x + f.0, y + dy + f.1, z + f.2, b));
                    }
                }
                out.push((x, y, z, self.ids.gateway));
            }
        }
        // EndIslandPopulator.
        let mut r = NukkitRandom::new(seed);
        if r.next_bounded_int(14) == 0 && outer < -20.0 {
            for first in [true, false] {
                if !first && r.next_bounded_int(4) != 0 {
                    break;
                }
                let x = (sx << 4) + 8 + r.next_bounded_int(16);
                let y = 55 + r.next_bounded_int(16);
                let z = (sz << 4) + 8 + r.next_bounded_int(16);
                self.end_island(&mut r, (x, y, z), out);
            }
        }
    }

    /// PNX `ObjectEndIsland`.
    fn end_island(&self, r: &mut NukkitRandom, p: (i32, i32, i32), out: &mut Vec<Block>) {
        let mut n = (r.next_bounded_int(2) + 4) as f32;
        let mut y = 0;
        while n > 0.5 {
            for x in (-n).floor() as i32..=n.ceil() as i32 {
                for z in (-n).floor() as i32..=n.ceil() as i32 {
                    if ((x * x + z * z) as f32) <= (n + 1.0) * (n + 1.0) {
                        out.push((p.0 + x, p.1 + y, p.2 + z, self.ids.end_stone));
                    }
                }
            }
            n -= (r.next_bounded_int(1) as f64 + 0.5) as f32;
            y -= 1;
        }
    }

    /// PNX `LegacyChorusTree.generate`.
    fn chorus_tree(&self, st: &mut Stage, r: &mut Xoroshiro, p: (i32, i32, i32), max: i32) {
        let mut plants = vec![p];
        st.set(p.0, p.1, p.2, self.ids.plant);
        self.chorus_grow(st, r, p, max, 0, &mut plants);
        let (pl, fl, es) = (self.ids.plant, self.ids.flower, self.ids.end_stone);
        const KEYS: [&str; 6] = [
            "minecraft:connection_down",
            "minecraft:connection_up",
            "minecraft:connection_north",
            "minecraft:connection_south",
            "minecraft:connection_west",
            "minecraft:connection_east",
        ];
        for q in plants {
            let mut ch: Vec<(&str, State)> = Vec::with_capacity(6);
            for (fi, f) in FACES.iter().enumerate() {
                let n = st.get(q.0 + f.0, q.1 + f.1, q.2 + f.2);
                let n_is = |v: i32| bl::name(n) == bl::name(v);
                ch.push((KEYS[fi], bl::b(n_is(pl) || n_is(fl) || (fi == 0 && n == es))));
            }
            st.set(q.0, q.1, q.2, bl::with(pl, &ch));
        }
    }

    fn chorus_grow(&self, st: &mut Stage, r: &mut Xoroshiro, p: (i32, i32, i32), max: i32, age: i32, plants: &mut Vec<(i32, i32, i32)>) {
        let air = self.ids.air;
        let horizontal_air = |st: &Stage, q: (i32, i32, i32), except: Option<usize>| {
            HORIZONTAL.iter().enumerate().all(|(i, d)| Some(i) == except || st.get(q.0 + d.0, q.1, q.2 + d.1) == air)
        };
        let mut height = 1 + r.next_int_below(4);
        if age == 0 {
            height += 1;
        }
        for y in 1..=height {
            if !horizontal_air(st, (p.0, p.1 + y, p.2), None) {
                return;
            }
            st.set(p.0, p.1 + y, p.2, self.ids.plant);
            plants.push((p.0, p.1 + y, p.2));
        }
        if age < 4 {
            let mut attempt = r.next_int_below(4);
            if age == 0 {
                attempt += 1;
            }
            for _ in 0..attempt {
                let fi = r.next_int_below(4) as usize;
                let d = HORIZONTAL[fi];
                let c = (p.0 + d.0, p.1 + height, p.2 + d.1);
                if st.get(c.0, c.1, c.2) == air
                    && st.get(c.0, c.1 - 1, c.2) == air
                    && (c.0 - p.0).abs() < max
                    && (c.2 - p.2).abs() < max
                    && horizontal_air(st, c, Some((fi + 2) % 4))
                {
                    st.set(c.0, c.1, c.2, self.ids.plant);
                    plants.push(c);
                    self.chorus_grow(st, r, c, max, age + 1, plants);
                }
            }
        } else {
            st.set(p.0, p.1 + height, p.2, self.ids.flower);
        }
    }

    // -----------------------------------------------------------------------------------------
    // End cities

    /// PNX `StructureRandomSpreadPlacement` (TRIANGULAR) candidate chunk of a region.
    fn city_candidate(&self, rx: i32, rz: i32) -> (i32, i32) {
        let rs = rx.wrapping_mul(0x9939_F508u32 as i32).wrapping_add(rz.wrapping_mul(0xF156_5BD5u32 as i32)).wrapping_add(self.seed as i32).wrapping_add(CITY_SALT);
        let mut r = Mt::new(rs);
        let range = (CITY_SPACING - CITY_SEPARATION) as u32;
        let mut off = || ((r.next() % range + r.next() % range) / 2) as i32;
        let ox = off();
        let oz = off();
        (rx * CITY_SPACING + ox, rz * CITY_SPACING + oz)
    }

    fn cities_near(&self, cx: i32, cz: i32) -> Vec<Arc<City>> {
        let mut v = vec![];
        for rx in (cx - CITY_REACH).div_euclid(CITY_SPACING)..=(cx + CITY_REACH).div_euclid(CITY_SPACING) {
            for rz in (cz - CITY_REACH).div_euclid(CITY_SPACING)..=(cz + CITY_REACH).div_euclid(CITY_SPACING) {
                let (sx, sz) = self.city_candidate(rx, rz);
                if (sx - cx).abs() <= CITY_REACH && (sz - cz).abs() <= CITY_REACH && far(sx, sz) {
                    v.push(self.city(sx, sz));
                }
            }
        }
        v
    }

    fn city(&self, sx: i32, sz: i32) -> Arc<City> {
        let k = chunk_hash(sx, sz);
        if let Some(c) = self.cities.lock().get(&k) {
            return c.clone();
        }
        let c = Arc::new(self.build_city(sx, sz));
        self.cities.lock().entry(k).or_insert(c).clone()
    }

    /// PNX `EndCityPopulator.apply` + `EndCityPieces.place` (blocks only).
    fn build_city(&self, sx: i32, sz: i32) -> City {
        let rotation = (Mt::new(sz.wrapping_mul(CITY_SALT).wrapping_add(sx)).next() % 4) as i32;
        let t = self.terrain_of(sx, sz);
        let mut min_y = i32::MAX;
        for dx in 0..5 {
            for dz in 0..5 {
                let (x, z) = match rotation {
                    1 => (4 - dz, dx),
                    2 => (4 - dx, 4 - dz),
                    3 => (dz, 4 - dx),
                    _ => (dx, dz),
                };
                min_y = min_y.min(t.height(x + 7, z + 7) - 1);
            }
        }
        if min_y < 60 {
            return City::default();
        }
        let mut r = Xoroshiro::new(self.seed);
        let a = r.next_int() as i64;
        let b = r.next_int() as i64;
        r.set_seed((sx as i64).wrapping_mul(a) ^ (sz as i64).wrapping_mul(b) ^ self.seed);
        let Some(pieces) = city::pieces([(sx << 4) + 8, min_y, (sz << 4) + 8], rotation, &mut r) else { return City::default() };
        let frame = bl::with(bl::id("frame"), &[("facing_direction", bl::i(3))]);
        let mut order: Vec<i64> = vec![];
        let mut map: HashMap<i64, Block> = HashMap::new();
        for p in &pieces {
            let Some(tpl) = templates::get(p.name) else { continue };
            let ship = p.name == "ship";
            let patch = |x, y, z| (ship && (x, y, z) == (6, 5, 7)).then_some(frame);
            tpl.for_each(city::template_rot(p.rotation), patch, |x, y, z, v| {
                if !p.overwrite && v == self.ids.air {
                    return;
                }
                let b = (p.pos[0] + x, p.pos[1] + y, p.pos[2] + z, v);
                if map.insert(pack(b.0, b.1, b.2), b).is_none() {
                    order.push(pack(b.0, b.1, b.2));
                }
            });
        }
        let mut city = City::default();
        let mut queued = false;
        for k in &order {
            let b = map[k];
            let n = bl::short(b.3);
            if n == "structure_block" {
                let below = map.get(&pack(b.0, b.1 - 1, b.2)).is_some_and(|u| bl::short(u.3) == "chest");
                if !below {
                    city.shulkers.push((b.0, b.1, b.2));
                }
                map.insert(*k, (b.0, b.1, b.2, self.ids.air));
                queued = true;
            } else if matches!(n, "chest" | "standing_banner" | "frame" | "brewing_stand") {
                queued = true;
            }
        }
        // PNX queues the city only when it has loot, banners, frames, stands or shulkers.
        if !queued {
            return City::default();
        }
        for k in order {
            let b = map[&k];
            crate::loot::generated((b.0, b.1 + crate::dims::base(crate::dims::END), b.2), b.3, "end_city");
            city.chunks.entry(chunk_hash(b.0 >> 4, b.2 >> 4)).or_default().push(b);
        }
        city
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: u64 = 12345;

    fn column_has(c: &EndChunk, lx: i32, lz: i32, f: impl Fn(i32) -> bool) -> bool {
        (MIN_Y..=MAX_Y).any(|y| f(c.get(lx, y, lz)))
    }

    #[test]
    fn main_island_void_and_portal() {
        let e = End::new(SEED);
        let c = e.generate(2, 2);
        assert!(column_has(&c, 0, 0, |b| b == e.ids.end_stone), "main island");
        assert_eq!(c.biome(0, 64, 0), 9);
        let v = e.generate(37, 3);
        assert!((0..16).all(|x| (0..16).all(|z| !column_has(&v, x, z, |b| b != e.ids.air))), "void at 600");
        let o = e.generate(0, 0);
        assert!(column_has(&o, 0, 0, |b| bl::is(b, "bedrock")), "exit portal");
        assert!(column_has(&o, 1, 0, |b| bl::is(b, "torch")));
        assert_eq!(e.gateways().len(), 20);
        assert!(e.entities(1, 0).iter().any(|e| e.0 == "minecraft:ender_dragon"));
    }

    #[test]
    fn pillars() {
        let e = End::new(SEED);
        let p = e.pillars();
        assert_eq!(p.len(), 10);
        let mut heights: Vec<i32> = p.iter().map(|p| p.3).collect();
        heights.sort();
        assert_eq!(heights, (0..10).map(|i| 76 + i * 3).collect::<Vec<_>>());
        for (x, z, _, h) in p {
            let c = e.generate(x >> 4, z >> 4);
            assert!(bl::is(c.get(x & 15, 10, z & 15), "obsidian"));
            assert!(bl::is(c.get(x & 15, h, z & 15), "bedrock"));
            assert!(e.entities(x >> 4, z >> 4).iter().any(|e| e.0 == "minecraft:ender_crystal"));
        }
        assert_eq!(e.pillars.iter().filter(|p| p.4).count(), 2);
    }

    #[test]
    fn outer_islands_and_chorus() {
        let e = End::new(SEED);
        let mut land = false;
        let mut chorus = false;
        'o: for cx in 69..94 {
            for cz in -12..12 {
                land |= (0..16).any(|x| (0..16).any(|z| e.height((cx << 4) + x, (cz << 4) + z) > 0));
                if land {
                    let mut buf = vec![];
                    e.populate(cx, cz, &mut buf);
                    if buf.iter().any(|b| b.3 == e.ids.flower) {
                        let c = e.generate(cx, cz);
                        chorus = (0..16).any(|x| (0..16).any(|z| column_has(&c, x, z, |b| bl::is(b, "chorus_plant"))));
                        if chorus {
                            break 'o;
                        }
                    }
                }
            }
        }
        assert!(land, "outer island land 1100-1500 out");
        assert!(chorus, "chorus plant on outer islands");
    }

    #[test]
    fn end_city() {
        let e = End::new(SEED);
        let mut found = 0;
        for rx in -6..6 {
            for rz in -6..6 {
                let (sx, sz) = e.city_candidate(rx, rz);
                if !far(sx, sz) {
                    continue;
                }
                let pl = crate::overworld::structures::Placement::new(CITY_SALT as i64, CITY_SEPARATION, CITY_SPACING, crate::overworld::structures::Spread::Triangular);
                assert!(pl.is_candidate(e.seed, sx, sz));
                let c = e.city(sx, sz);
                if !c.chunks.is_empty() {
                    let ch = e.generate(sx, sz);
                    if (0..16).any(|x| (0..16).any(|z| column_has(&ch, x, z, |b| bl::short(b).starts_with("purpur")))) {
                        found += 1;
                    }
                }
            }
        }
        assert!(found > 0, "an end city piece");
    }

    #[test]
    fn deterministic_and_order_free() {
        let (a, b) = (End::new(SEED), End::new(SEED));
        let a1 = a.generate(80, 5).blocks;
        let a2 = a.generate(81, 5).blocks;
        let b2 = b.generate(81, 5).blocks;
        let b1 = b.generate(80, 5).blocks;
        assert!(a1 == b1 && a2 == b2);
    }
}

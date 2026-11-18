// SPDX-License-Identifier: LGPL-3.0 -- this directory is ported from PowerNukkitX; see NOTICE.md.
//! Vanilla-style overworld gen ported from PowerNukkitX (LGPL, see NOTICE).
//! Same seed gives the same terrain as PNX, just faster.

mod aquifer;
pub mod biome;
mod density;
pub mod feature;
pub mod structures;
mod noise;
mod rng;
mod router;

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::OnceLock;

use aquifer::{Aquifer, Fluid, SEA_LEVEL, Substance, Vein};
use biome::id as b;
use density::Ctx;
use rng::NukkitRandom;
use router::Router;


const MIN_Y: i32 = -64;
const MAX_Y: i32 = 319;
const HEIGHT: usize = 384;

/// A generated chunk column: blocks (a small palette plus one byte per block) and biomes.
#[derive(Clone)]
pub struct Chunk {
    palette: Vec<i32>,
    blocks: Vec<u8>,
    pub heights: [i32; 256],
    surface: [u16; 256],
    cave: [u16; 256],
    deep: [u16; 256],
}

fn bi(lx: i32, y: i32, lz: i32) -> usize {
    (((lx * 16 + lz) as usize) * HEIGHT) + (y - MIN_Y) as usize
}

impl Chunk {
    fn new() -> Self {
        Self { palette: vec![ids().air], blocks: vec![0; 256 * HEIGHT], heights: [MIN_Y; 256], surface: [0; 256], cave: [0; 256], deep: [0; 256] }
    }

    /// Block at local x/z and world y.
    pub fn get(&self, lx: i32, y: i32, lz: i32) -> i32 {
        if !(MIN_Y..=MAX_Y).contains(&y) {
            return ids().air;
        }
        self.palette[self.blocks[bi(lx, y, lz)] as usize]
    }

    pub(crate) fn set(&mut self, lx: i32, y: i32, lz: i32, id: i32) {
        if !(MIN_Y..=MAX_Y).contains(&y) {
            return;
        }
        let p = match self.palette.iter().position(|&v| v == id) {
            Some(p) => p,
            None => {
                self.palette.push(id);
                self.palette.len() - 1
            }
        };
        self.blocks[bi(lx, y, lz)] = p as u8;
    }

    /// Biome at local x/z and world y: surface biome near the top, cave biomes deeper down.
    pub fn biome(&self, lx: i32, y: i32, lz: i32) -> u16 {
        let i = (lx * 16 + lz) as usize;
        let h = self.heights[i];
        if y >= h - 26 {
            self.surface[i]
        } else if y >= h - 127 {
            self.cave[i]
        } else {
            self.deep[i]
        }
    }

    /// Serialises the chunk for the on-disk chunk cache (deflate-compressed).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(self.blocks.len() + 4096);
        b.extend(CHUNK_MAGIC);
        b.extend(GEN_VERSION.to_le_bytes());
        b.extend((self.palette.len() as u16).to_le_bytes());
        for p in &self.palette {
            b.extend(p.to_le_bytes());
        }
        b.extend(&self.blocks);
        for h in self.heights {
            b.extend((h as i16).to_le_bytes());
        }
        for arr in [&self.surface, &self.cave, &self.deep] {
            for v in arr.iter() {
                b.extend(v.to_le_bytes());
            }
        }
        use std::io::Write;
        let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        let _ = e.write_all(&b);
        e.finish().unwrap_or_default()
    }

    /// Reads `to_bytes` output; None if damaged or from another generator version.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        use std::io::Read;
        let mut b = Vec::new();
        flate2::read::DeflateDecoder::new(data).read_to_end(&mut b).ok()?;
        let mut i = 0;
        let mut take = |n: usize| -> Option<&[u8]> {
            let s = b.get(i..i + n)?;
            i += n;
            Some(s)
        };
        if take(4)? != CHUNK_MAGIC || u32::from_le_bytes(take(4)?.try_into().ok()?) != GEN_VERSION {
            return None;
        }
        let n = u16::from_le_bytes(take(2)?.try_into().ok()?) as usize;
        let palette = (0..n).map(|_| take(4).and_then(|s| s.try_into().ok()).map(i32::from_le_bytes)).collect::<Option<Vec<_>>>()?;
        let blocks = take(256 * HEIGHT)?.to_vec();
        if blocks.iter().any(|&p| p as usize >= palette.len()) {
            return None;
        }
        let mut heights = [0i32; 256];
        for h in &mut heights {
            *h = i16::from_le_bytes(take(2)?.try_into().ok()?) as i32;
        }
        let mut biomes = [[0u16; 256]; 3];
        for arr in &mut biomes {
            for v in arr.iter_mut() {
                *v = u16::from_le_bytes(take(2)?.try_into().ok()?);
            }
        }
        let [surface, cave, deep] = biomes;
        Some(Self { palette, blocks, heights, surface, cave, deep })
    }

    pub fn surface_biome(&self, lx: i32, lz: i32) -> u16 {
        self.surface[(lx * 16 + lz) as usize]
    }
}

const CHUNK_MAGIC: &[u8; 4] = b"RSTK";
/// Bump when generation changes, so cached chunks from the old generator are rebuilt.
pub const GEN_VERSION: u32 = 1;

pub struct Ids {
    pub air: i32,
    stone: i32,
    deepslate: i32,
    bedrock: i32,
    pub water: i32,
    lava: i32,
    veins: [i32; 6],
    coarse_dirt: i32,
    grass: i32,
    hardened_clay: i32,
    orange: i32,
    white: i32,
    yellow: i32,
    brown: i32,
    red: i32,
    light_gray: i32,
    snow: i32,
    packed_ice: i32,
}

pub fn ids() -> &'static Ids {
    static I: OnceLock<Ids> = OnceLock::new();
    I.get_or_init(|| {
        let id = |n: &str| crate::blocks::any_state(&format!("minecraft:{n}")).unwrap_or_else(|| panic!("block {n} missing"));
        Ids {
            air: crate::world::AIR,
            stone: id("stone"),
            deepslate: crate::blocks::with(id("deepslate"), &[("pillar_axis", crate::blocks::State::Str("y".into()))]),
            bedrock: crate::world::BEDROCK,
            water: crate::fluids::Kind::Water.source(),
            lava: crate::fluids::Kind::Lava.source(),
            veins: [id("copper_ore"), id("raw_copper_block"), id("granite"), id("deepslate_iron_ore"), id("raw_iron_block"), id("tuff")],
            coarse_dirt: id("coarse_dirt"),
            grass: crate::world::GRASS,
            hardened_clay: id("hardened_clay"),
            orange: id("orange_terracotta"),
            white: id("white_terracotta"),
            yellow: id("yellow_terracotta"),
            brown: id("brown_terracotta"),
            red: id("red_terracotta"),
            light_gray: id("light_gray_terracotta"),
            snow: id("snow"),
            packed_ice: id("packed_ice"),
        }
    })
}

/// A biome's surface blocks, read from the vanilla biome definitions we replay on join.
#[derive(Clone)]
struct Surface {
    top: i32,
    mid: i32,
    floor: i32,
    floor_depth: i32,
    /// (noise lower, noise upper, top, mid, floor) overrides.
    adjust: Vec<(f32, f32, Option<i32>, Option<i32>, Option<i32>)>,
}

const BIOME_IDS: &[(&str, u16)] = &[
    ("ocean", 0), ("plains", 1), ("desert", 2), ("extreme_hills", 3), ("forest", 4), ("taiga", 5), ("swampland", 6), ("river", 7), ("hell", 8), ("the_end", 9), ("legacy_frozen_ocean", 10), ("frozen_river", 11), ("ice_plains", 12), ("ice_mountains", 13), ("mushroom_island", 14), ("mushroom_island_shore", 15), ("beach", 16), ("desert_hills", 17), ("forest_hills", 18), ("taiga_hills", 19), ("extreme_hills_edge", 20), ("jungle", 21), ("jungle_hills", 22), ("jungle_edge", 23), ("deep_ocean", 24), ("stone_beach", 25), ("cold_beach", 26), ("birch_forest", 27), ("birch_forest_hills", 28), ("roofed_forest", 29), ("cold_taiga", 30), ("cold_taiga_hills", 31), ("mega_taiga", 32), ("mega_taiga_hills", 33), ("extreme_hills_plus_trees", 34), ("savanna", 35), ("savanna_plateau", 36), ("mesa", 37), ("mesa_plateau_stone", 38), ("mesa_plateau", 39), ("warm_ocean", 40), ("deep_warm_ocean", 41), ("lukewarm_ocean", 42), ("deep_lukewarm_ocean", 43), ("cold_ocean", 44), ("deep_cold_ocean", 45), ("frozen_ocean", 46), ("deep_frozen_ocean", 47), ("bamboo_jungle", 48), ("bamboo_jungle_hills", 49), ("sunflower_plains", 129), ("desert_mutated", 130), ("extreme_hills_mutated", 131), ("flower_forest", 132), ("taiga_mutated", 133), ("swampland_mutated", 134), ("ice_plains_spikes", 140), ("jungle_mutated", 149), ("jungle_edge_mutated", 151), ("birch_forest_mutated", 155), ("birch_forest_hills_mutated", 156), ("roofed_forest_mutated", 157), ("cold_taiga_mutated", 158), ("redwood_taiga_mutated", 160), ("redwood_taiga_hills_mutated", 161), ("extreme_hills_plus_trees_mutated", 162), ("savanna_mutated", 163), ("savanna_plateau_mutated", 164), ("mesa_bryce", 165), ("mesa_plateau_stone_mutated", 166), ("mesa_plateau_mutated", 167), ("soulsand_valley", 178), ("crimson_forest", 179), ("warped_forest", 180), ("basalt_deltas", 181), ("jagged_peaks", 182), ("frozen_peaks", 183), ("snowy_slopes", 184), ("grove", 185), ("meadow", 186), ("lush_caves", 187), ("dripstone_caves", 188), ("stony_peaks", 189), ("deep_dark", 190), ("mangrove_swamp", 191), ("cherry_grove", 192), ("pale_garden", 193), ("sulfur_caves", 194), ("dappled_forest", 195),
];

/// Bedrock biome id for a biome name (without `minecraft:`).
pub fn biome_id(name: &str) -> Option<u16> {
    BIOME_IDS.iter().find(|(n, _)| *n == name).map(|(_, id)| *id)
}

fn surfaces() -> &'static HashMap<u16, Surface> {
    static S: OnceLock<HashMap<u16, Surface>> = OnceLock::new();
    S.get_or_init(|| {
        let mut out = HashMap::new();
        let batch = crate::vanilla::batches().nth(2).expect("biome definitions frame");
        let decoded = bedrock::network::codec::decode_packets_each::<crate::Protocol>(batch, None, None, usize::MAX, bedrock::network::codec::UnreadBytes::Warn);
        let Some(crate::Protocol::BiomeDefinitionListPacket(p)) = decoded.ok().and_then(|d| d.into_iter().next()).and_then(|(_, p)| p.ok()) else {
            println!("overworld: could not read biome surfaces; using grass everywhere");
            return out;
        };
        let known = |v: i32| crate::blocks::entry(v).is_some().then_some(v);
        for e in &p.biomes {
            let name = p.strings[e.name_index as usize].trim_start_matches("minecraft:");
            let Some(&(_, id)) = BIOME_IDS.iter().find(|(n, _)| *n == name) else { continue };
            let Some(g) = &e.definition.chunk_gen_data else { continue };
            let Some(m) = g.surface_builder_data.as_ref().and_then(|s| s.surface_material.as_ref()) else { continue };
            let adjust = g.surface_material_adjustment.as_ref().map_or(vec![], |a| {
                a.biome_elements
                    .iter()
                    .map(|el| {
                        let am = &el.adjusted_materials;
                        (el.noise_lower_bound, el.noise_upper_bound, known(am.top_block_runtime_id), known(am.mid_block_runtime_id), known(am.sea_floor_block_runtime_id))
                    })
                    .collect()
            });
            out.insert(id, Surface { top: m.top_block_runtime_id, mid: m.mid_block_runtime_id, floor: m.sea_floor_block_runtime_id, floor_depth: m.sea_floor_depth, adjust });
        }
        out
    })
}

thread_local! {
    static CTX: RefCell<Ctx> = RefCell::new(Ctx::default());
}

pub struct Overworld {
    seed: i64,
    router: Router,
    clay_bands: [i32; 192],
    /// Undecorated terrain, shared by neighbouring chunks' decoration.
    terrain: parking_lot::Mutex<HashMap<(i32, i32), std::sync::Arc<Chunk>>>,
    /// Built structures, shared by every chunk they reach.
    structures: structures::Cache,
}

const TERRAIN_CACHE_LIMIT: usize = 2048;

fn java_hash(s: &str) -> i32 {
    s.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

fn chunk_hash(x: i32, z: i32) -> i64 {
    ((x as i64) << 32) | (z as u32 as i64)
}

impl Overworld {
    pub fn new(seed: u64) -> Self {
        let seed = seed as i64;
        let router = Router::new(seed);
        let clay_bands = clay_bands(seed);
        Self { seed, router, clay_bands, terrain: Default::default(), structures: Default::default() }
    }

    /// A finished chunk: terrain plus features (trees, ores, plants) from it and its neighbours.
    pub fn generate(&self, cx: i32, cz: i32) -> Chunk {
        let mut chunks = Vec::with_capacity(9);
        for dz in -1..=1 {
            for dx in -1..=1 {
                chunks.push((*self.terrain_chunk(cx + dx, cz + dz)).clone());
            }
        }
        let mut ch = feature::decorate(feature::Region::new(cx, cz, self.seed, chunks));
        structures::apply(self, &mut ch, cx, cz);
        ch
    }

    fn terrain_chunk(&self, cx: i32, cz: i32) -> std::sync::Arc<Chunk> {
        if let Some(c) = self.terrain.lock().get(&(cx, cz)) {
            return c.clone();
        }
        let c = std::sync::Arc::new(self.generate_terrain(cx, cz));
        let mut m = self.terrain.lock();
        if m.len() >= TERRAIN_CACHE_LIMIT {
            // TODO: drops half at the cap, switch to LRU if regen shows up in profiles.
            let keys: Vec<_> = m.keys().copied().step_by(2).collect();
            for k in keys {
                m.remove(&k);
            }
        }
        m.entry((cx, cz)).or_insert(c).clone()
    }

    /// Terrain only (no features).
    pub fn generate_terrain(&self, cx: i32, cz: i32) -> Chunk {
        CTX.with(|c| {
            let mut c = c.borrow_mut();
            c.clear();
            let mut ch = Chunk::new();
            self.terrain(&mut c, &mut ch, cx, cz);
            self.biomes(&mut c, &mut ch, cx, cz);
            self.surface(&mut ch, cx, cz);
            self.overwrite(&mut ch, cx, cz);
            c.clear();
            ch
        })
    }

    fn material(&self, c: &mut Ctx, aq: &mut Aquifer, vr: &mut NukkitRandom, x: i32, y: i32, z: i32) -> i32 {
        let r = &self.router;
        let ids = ids();
        let d = r.terrain.eval(c, x, y, z);
        match aq.substance(r, c, x, y, z, d) {
            Some(Substance::Fluid(Fluid::Air)) => ids.air,
            Some(Substance::Fluid(Fluid::Water)) => ids.water,
            Some(Substance::Fluid(Fluid::Lava)) => ids.lava,
            Some(Substance::Stone) => ids.stone,
            None => match aquifer::ore_vein(r, c, vr, x, y, z) {
                Some(v) => ids.veins[match v {
                    Vein::CopperOre => 0,
                    Vein::RawCopperBlock => 1,
                    Vein::Granite => 2,
                    Vein::DeepslateIronOre => 3,
                    Vein::RawIronBlock => 4,
                    Vein::Tuff => 5,
                }],
                None => ids.stone,
            },
        }
    }

    /// PNX `NormalTerrainStage`: fills 4x8x4 cells, flooding out from solid ones to skip open sky.
    fn terrain(&self, c: &mut Ctx, ch: &mut Chunk, cx: i32, cz: i32) {
        let r = &self.router;
        let ids = ids();
        let (bx, bz) = (cx << 4, cz << 4);
        let cell_min_y = MIN_Y.div_euclid(8) * 8;
        let cell_max_y = MAX_Y.div_euclid(8) * 8;
        let ycount = ((cell_max_y - cell_min_y) / 8 + 1) as usize;
        let mut rand = NukkitRandom::new(self.seed ^ chunk_hash(cx, cz));
        let mut vein_rand = NukkitRandom::new(r.ore_vein_seed);
        let mut mandatory = [0i32; 256];
        for x in 0..16 {
            for z in 0..16 {
                let upper = r.prelim_upper.eval(c, bx + x, 0, bz + z).ceil() as i32;
                mandatory[(x * 16 + z) as usize] = MAX_Y.min(SEA_LEVEL.max(upper));
            }
        }
        let mut aq = Aquifer::new(self.seed, r, c, cx, cz);
        let cells = 16 * ycount;
        let idx = |x: usize, y: usize, z: usize| (y * 4 + z) * 4 + x;
        let mut queued = vec![false; cells];
        let mut solid = vec![false; cells];
        let gen_cell = |c: &mut Ctx, ch: &mut Chunk, aq: &mut Aquifer, rand: &mut NukkitRandom, vr: &mut NukkitRandom, cxi: usize, cyi: usize, czi: usize| {
            let (x0, y0, z0) = (cxi as i32 * 4, cell_min_y + cyi as i32 * 8, czi as i32 * 4);
            let mut any = false;
            for lx in 0..4 {
                for lz in 0..4 {
                    let (x, z) = (x0 + lx, z0 + lz);
                    let hi = &mut ch.heights[(x * 16 + z) as usize];
                    let mut height = *hi;
                    for ly in (0..8).rev() {
                        let y = y0 + ly;
                        if !(MIN_Y..=MAX_Y).contains(&y) {
                            continue;
                        }
                        let mut s = self.material(c, aq, vr, bx + x, y, bz + z);
                        if s == ids.stone && (y < 0 || (y <= 8 && rand.next_bounded_int(9) >= y)) {
                            s = ids.deepslate;
                        }
                        if s != ids.air {
                            any = true;
                            height = height.max(y + 1);
                        }
                        ch.set(x, y, z, s);
                    }
                    ch.heights[(x * 16 + z) as usize] = height;
                }
            }
            any
        };
        for cyi in 0..ycount {
            let cy = cell_min_y + cyi as i32 * 8;
            for cxi in 0..4 {
                for czi in 0..4 {
                    let mandatory_cell = cy + 7 <= SEA_LEVEL || (0..4).any(|lx| (0..4).any(|lz| cy <= mandatory[(cxi * 4 + lx) * 16 + czi * 4 + lz]));
                    if !mandatory_cell && !corner_seed(cxi, cyi, cy, czi) {
                        continue;
                    }
                    let i = idx(cxi, cyi, czi);
                    queued[i] = true;
                    solid[i] = gen_cell(c, ch, &mut aq, &mut rand, &mut vein_rand, cxi, cyi, czi);
                }
            }
        }
        let mut queue = VecDeque::new();
        let enqueue = |queue: &mut VecDeque<usize>, queued: &mut [bool], x: i32, y: i32, z: i32| {
            if !(0..4).contains(&x) || !(0..ycount as i32).contains(&y) || !(0..4).contains(&z) {
                return;
            }
            let i = idx(x as usize, y as usize, z as usize);
            if !queued[i] {
                queued[i] = true;
                queue.push_back(i);
            }
        };
        let neighbours = |i: usize| {
            let (x, z, y) = ((i % 4) as i32, ((i / 4) % 4) as i32, (i / 16) as i32);
            [(x + 1, y, z), (x - 1, y, z), (x, y + 1, z), (x, y - 1, z), (x, y, z + 1), (x, y, z - 1)]
        };
        for i in (0..cells).filter(|&i| solid[i]) {
            {
                for (x, y, z) in neighbours(i) {
                    enqueue(&mut queue, &mut queued, x, y, z);
                }
            }
        }
        while let Some(i) = queue.pop_front() {
            let (x, z, y) = (i % 4, (i / 4) % 4, i / 16);
            if gen_cell(c, ch, &mut aq, &mut rand, &mut vein_rand, x, y, z) {
                for (x, y, z) in neighbours(i) {
                    enqueue(&mut queue, &mut queued, x, y, z);
                }
            }
        }
        for x in 0..16 {
            for z in 0..16 {
                ch.set(x, MIN_Y, z, ids.bedrock);
                let depth = rand.next_bounded_int(6);
                for i in 0..depth {
                    if ch.get(x, MIN_Y + i, z) != ids.air {
                        ch.set(x, MIN_Y + i, z, ids.bedrock);
                    }
                }
            }
        }
    }

    fn biomes(&self, c: &mut Ctx, ch: &mut Chunk, cx: i32, cz: i32) {
        let r = &self.router;
        for x in 0..16 {
            for z in 0..16 {
                let (wx, wz) = ((cx << 4) + x, (cz << 4) + z);
                let v = |f: &density::F, c: &mut Ctx| f.eval(c, wx, SEA_LEVEL, wz) as f32;
                let p = biome::pick(v(&r.continents, c), v(&r.temperature, c), v(&r.humidity, c), v(&r.erosion, c), v(&r.ridges, c));
                let i = (x * 16 + z) as usize;
                ch.surface[i] = p.biome;
                ch.cave[i] = p.at_depth(26);
                ch.deep[i] = p.at_depth(127);
            }
        }
    }

    /// PNX `NormalSurfaceDataStage`: top/mid/sea-floor blocks from the biome definitions.
    fn surface(&self, ch: &mut Chunk, cx: i32, cz: i32) {
        let ids = ids();
        let noise = &self.router.surface;
        let table = surfaces();
        for x in 0..16 {
            for z in 0..16 {
                let y = ch.heights[(x * 16 + z) as usize] - 1;
                let mut top_state = ch.get(x, y, z);
                let Some(s) = table.get(&ch.biome(x, y, z)) else { continue };
                let (mut top, mut mid, mut floor) = (s.top, s.mid, s.floor);
                if !s.adjust.is_empty() {
                    let rv = noise.value(((cx << 4) + x) as f64, 0.0, ((cz << 4) + z) as f64) as f32;
                    for &(lo, hi, t, m, f) in &s.adjust {
                        if rv < hi && rv > lo {
                            top = t.unwrap_or(top);
                            mid = m.unwrap_or(mid);
                            floor = f.unwrap_or(floor);
                        }
                    }
                }
                if top_state != ids.water {
                    ch.set(x, y, z, top);
                    let v = noise.value(x as f64, 0.0, z as f64) as f32;
                    let mid_depth = 1.0 + (v + 1.0) / 2.0 * 3.0;
                    let mut i = 1;
                    while (i as f32) < mid_depth {
                        ch.set(x, y - i, z, mid);
                        i += 1;
                    }
                } else {
                    let mut depth = 0;
                    while top_state == ids.water {
                        depth += 1;
                        top_state = ch.get(x, y - depth, z);
                    }
                    if depth > s.floor_depth {
                        ch.set(x, y - depth, z, floor);
                    } else {
                        for i in 0..3 {
                            ch.set(x, y - depth - i, z, mid);
                        }
                    }
                }
            }
        }
    }

    /// PNX `NormalSurfaceOverwriteStage`: badlands bands, swamp puddles, icebergs and so on.
    fn overwrite(&self, ch: &mut Chunk, cx: i32, cz: i32) {
        let ids = ids();
        let o = &self.router.overwrite;
        for x in 0..16 {
            for z in 0..16 {
                let (wx, wz) = ((cx << 4) + x, (cz << 4) + z);
                let y = ch.heights[(x * 16 + z) as usize] - 1;
                let biome = ch.biome(x, y, z);
                if matches!(biome, b::MESA | b::MESA_BRYCE | b::MESA_PLATEAU_STONE) {
                    self.clay_bands_depth(ch, x, z, wx, wz, y);
                }
                match biome {
                    b::SWAMPLAND if o.swamp.value(wx as f64, y as f64, wz as f64) > 0.0 && y == SEA_LEVEL - 1 => ch.set(x, y, z, ids.water),
                    b::MANGROVE_SWAMP if o.swamp.value(wx as f64, y as f64, wz as f64) > 0.0 && (SEA_LEVEL - 2..SEA_LEVEL).contains(&y) => ch.set(x, y, z, ids.water),
                    b::MESA_PLATEAU_STONE if y > 97 => {
                        let n = o.surface.value((wx as f32 * 0.25) as f64, y as f64, (wz as f32 * 0.25) as f64) as f32;
                        let coarse = (-0.909..=-0.5454).contains(&n) || (-0.1818..=0.1818).contains(&n) || (0.5454..=0.909).contains(&n);
                        ch.set(x, y, z, if coarse { ids.coarse_dirt } else { ids.grass });
                    }
                    b::FROZEN_OCEAN | b::DEEP_FROZEN_OCEAN | b::LEGACY_FROZEN_OCEAN => self.iceberg(ch, x, z, wx, wz, y, cx, cz),
                    b::MESA_BRYCE => self.eroded_badlands(ch, x, z, wx, wz, y),
                    _ => {}
                }
            }
        }
    }

    fn clay_band(&self, wx: i32, y: i32, wz: i32) -> i32 {
        let n = self.router.overwrite.clay_offset.value(wx as f64, 0.0, wz as f64) as f32;
        let offset = (n * 4.0 + 0.5).floor() as i32;
        let len = self.clay_bands.len() as i32;
        self.clay_bands[(y + offset + len).rem_euclid(len) as usize]
    }

    #[allow(clippy::too_many_arguments)]
    fn clay_bands_depth(&self, ch: &mut Chunk, x: i32, z: i32, wx: i32, wz: i32, surface_y: i32) {
        let ids = ids();
        if surface_y >= 256 {
            ch.set(x, surface_y, z, ids.orange);
        } else if surface_y >= 74 {
            ch.set(x, surface_y, z, self.clay_band(wx, surface_y, wz));
        }
        for y in (64..=74).rev() {
            if ch.get(x, y, z) == ids.air {
                continue;
            }
            let b = if surface_y > 63 && surface_y < 74 { ids.orange } else { self.clay_band(wx, y, wz) };
            ch.set(x, y, z, b);
        }
        let n = self.router.overwrite.surface.value((wx as f32 * 0.25) as f64, 0.0, (wz as f32 * 0.25) as f64);
        let depth = 4 + (n.abs() * 3.0).floor() as i32;
        let min_y = MIN_Y.max(surface_y - depth);
        let mut y = surface_y - 1;
        while y >= min_y {
            let s = ch.get(x, y, z);
            if !solid(s) {
                break;
            }
            ch.set(x, y, z, self.clay_band(wx, y, wz));
            y -= 1;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn iceberg(&self, ch: &mut Chunk, x: i32, z: i32, wx: i32, wz: i32, height: i32, cx: i32, cz: i32) {
        let ids = ids();
        let o = &self.router.overwrite;
        let (fx, fz) = (wx as f64, wz as f64);
        let berg = (o.iceberg_surface.value(fx, 0.0, fz) * 8.25).abs().min(o.iceberg_pillar.value(fx * 1.28, 0.0, fz * 1.28) * 15.0);
        if berg <= 1.8 {
            return;
        }
        let roof = (o.iceberg_roof.value(fx * 1.17, 0.0, fz * 1.17) * 1.5).abs();
        let top = (berg * berg * 1.2).min((roof * 40.0).ceil() + 14.0);
        if top <= 2.0 {
            return;
        }
        let ext_bottom = SEA_LEVEL as f64 - top - 7.0;
        let ext_top = top + SEA_LEVEL as f64;
        let mut rand = NukkitRandom::new(self.seed ^ chunk_hash(cx, cz) ^ wx as i64 ^ wz as i64);
        let max_snow = 2 + rand.next_bounded_int(4);
        let min_snow_h = SEA_LEVEL + 18 + rand.next_bounded_int(10);
        let mut snow = 0;
        let mut y = height.max(ext_top as i32 + 1);
        while y > 0 {
            let s = ch.get(x, y, z);
            if s == ids.stone {
                break;
            }
            let place = (s == ids.air && y < ext_top as i32 && rand.next_double() > 0.01)
                || (is_water(s) && y > ext_bottom as i32 && y <= SEA_LEVEL && rand.next_double() > 0.15);
            if place {
                if snow <= max_snow && y > min_snow_h {
                    ch.set(x, y, z, ids.snow);
                    snow += 1;
                } else {
                    ch.set(x, y, z, ids.packed_ice);
                }
            }
            y -= 1;
        }
    }

    fn eroded_badlands(&self, ch: &mut Chunk, x: i32, z: i32, wx: i32, wz: i32, height: i32) {
        let o = &self.router.overwrite;
        let (fx, fz) = (wx as f64, wz as f64);
        let buffer = (o.badlands_surface.value(fx, 0.0, fz) * 8.25).abs().min(o.badlands_pillar.value(fx * 0.2, 0.0, fz * 0.2) * 15.0);
        if buffer <= 0.0 {
            return;
        }
        let floor = (o.badlands_roof.value(fx * 0.75, 0.0, fz * 0.75) * 1.5).abs();
        let ext_top = 64.0 + (buffer * buffer * 2.5).min((floor * 50.0).ceil() + 24.0);
        let start = ext_top.floor() as i32;
        if height > start {
            return;
        }
        for y in (MIN_Y..=start).rev() {
            let s = ch.get(x, y, z);
            if is_water(s) {
                return;
            }
            if solid(s) {
                break;
            }
        }
        for y in (MIN_Y..=start).rev() {
            if ch.get(x, y, z) != ids().air {
                break;
            }
            ch.set(x, y, z, self.clay_band(wx, y, wz));
        }
    }

    /// A dry spot near the origin, as an eye position.
    pub fn spawn(&self, chunk: impl Fn(i32, i32) -> std::sync::Arc<Chunk>) -> (f32, f32, f32) {
        for r in 0i32..48 {
            for (x, z) in [(r * 16, 0), (0, r * 16), (-r * 16, 0), (0, -r * 16), (r * 16, r * 16), (-r * 16, -r * 16), (r * 16, -r * 16), (-r * 16, r * 16)] {
                let ch = chunk(x >> 4, z >> 4);
                let (lx, lz) = (x & 15, z & 15);
                let h = ch.heights[(lx * 16 + lz) as usize];
                let top = ch.get(lx, h - 1, lz);
                if h > SEA_LEVEL && !is_water(top) && top != ids().lava && solid(top) {
                    return (x as f32 + 0.5, h as f32 + 1.62, z as f32 + 0.5);
                }
            }
        }
        (0.5, 100.0, 0.5)
    }
}

fn is_water(b: i32) -> bool {
    crate::blocks::entry(b).is_some_and(|e| e.name.ends_with("water"))
}

fn solid(b: i32) -> bool {
    b != ids().air && !crate::blocks::entry(b).is_some_and(|e| e.name.ends_with("water") || e.name.ends_with("lava"))
}

fn corner_seed(x: usize, yi: usize, y: i32, z: usize) -> bool {
    if y > 192 || !yi.is_multiple_of(3) {
        return false;
    }
    let (x, z) = (x as i32, z as i32);
    let nw = (x == 0 && z == 0) || (x == 1 && z == 0) || (x == 0 && z == 1);
    let ne = (x == 3 && z == 0) || (x == 2 && z == 0) || (x == 3 && z == 1);
    let sw = (x == 0 && z == 3) || (x == 1 && z == 3) || (x == 0 && z == 2);
    let se = (x == 3 && z == 3) || (x == 2 && z == 3) || (x == 3 && z == 2);
    nw || ne || sw || se
}

fn clay_bands(seed: i64) -> [i32; 192] {
    let ids = ids();
    let mut bands = [ids.hardened_clay; 192];
    let mut r = NukkitRandom::new(seed ^ java_hash("clay_bands") as i64);
    let len = bands.len() as i32;
    let mut i = 0;
    while i < len {
        i += r.next_bounded_int(5) + 1;
        if i < len {
            bands[i as usize] = ids.orange;
        }
        i += 1;
    }
    let mut make = |r: &mut NukkitRandom, base: i32, state: i32| {
        let count = r.next_range(6, 15);
        for _ in 0..count {
            let width = base + r.next_bounded_int(3);
            let start = r.next_bounded_int(len - 1);
            let mut p = 0;
            while start + p < len && p < width {
                bands[(start + p) as usize] = state;
                p += 1;
            }
        }
    };
    make(&mut r, 1, ids.yellow);
    make(&mut r, 2, ids.brown);
    make(&mut r, 1, ids.red);
    let white = r.next_range(9, 15);
    let (mut count, mut start) = (0, 0);
    while count < white && start < len {
        bands[start as usize] = ids.white;
        if start - 1 > 0 && r.next_boolean() {
            bands[(start - 1) as usize] = ids.light_gray;
        }
        if start + 1 < len && r.next_boolean() {
            bands[(start + 1) as usize] = ids.light_gray;
        }
        count += 1;
        start += r.next_bounded_int(16) + 4;
    }
    bands
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_vanilla_like_terrain() {
        let w = Overworld::new(12345);
        let t = std::time::Instant::now();
        let mut biomes = std::collections::HashSet::new();
        let (mut water, mut stone, mut heights) = (0, 0, vec![]);
        for cx in -2..2 {
            for cz in -2..2 {
                let ch = w.generate(cx * 20, cz * 20);
                for i in 0..256 {
                    heights.push(ch.heights[i]);
                    biomes.insert(ch.surface[i]);
                }
                for y in -64..320 {
                    let b = ch.get(5, y, 5);
                    water += (b == ids().water) as u32;
                    stone += (b == ids().stone || b == ids().deepslate) as u32;
                }
                assert_eq!(ch.get(0, -64, 0), ids().bedrock);
            }
        }
        let per = t.elapsed() / 16;
        println!("16 chunks, {per:?} per chunk, biomes {biomes:?}, heights {}..{}", heights.iter().min().unwrap(), heights.iter().max().unwrap());
        assert!(stone > 1000, "solid ground");
        assert!(biomes.len() >= 2, "several biomes: {biomes:?}");
        let again = Overworld::new(12345).generate(0, 0);
        let first = w.generate(0, 0);
        assert!((-64..320).all(|y| again.get(3, y, 7) == first.get(3, y, 7)), "same seed, same chunk");
        let _ = water;
    }
}

#[cfg(test)]
mod chunk_bytes {
    #[test]
    fn chunks_round_trip_through_bytes() {
        let c = super::Overworld::new(9).generate_terrain(0, 0);
        let back = super::Chunk::from_bytes(&c.to_bytes()).expect("decodes");
        assert!((-64..320).all(|y| back.get(3, y, 5) == c.get(3, y, 5)));
        assert_eq!(back.heights, c.heights);
        assert!(super::Chunk::from_bytes(b"junk").is_none());
    }
}

//! Generated terrain (flat or seeded) plus whatever players changed. Only the changes
//! get saved, terrain comes back from the seed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

// Block network ids are FNV hashes of the state (StartGame sets block_network_ids_are_hashes).
// Values from kaooot/bedrock-network-data block_palette.nbt (1.26.60).
pub const AIR: i32 = -604_749_536;
pub const BEDROCK: i32 = -173_245_189; // infiniburn_bit = 0
pub const DIRT: i32 = -2_108_756_090;
pub const GRASS: i32 = -567_203_660;

const PLAINS: i32 = 1;
const MIN_Y: i32 = -64;
#[cfg(test)]
const MAX_Y: i32 = 319;
/// y -64..320 is 24 sub-chunks.
const SUBCHUNKS: usize = 24;


/// Classic flat: bedrock, two dirt, grass from y -64 up.
pub const DEFAULT_LAYERS: [i32; 4] = [BEDROCK, DIRT, DIRT, GRASS];

/// Plugin world generator. Register it with `register_generator`, then set
/// `level-type=<name>` in server.properties.
pub trait WorldGenerator: Send + Sync {
    fn block(&self, x: i32, y: i32, z: i32) -> i32;
    /// spawn point (eye position)
    fn spawn(&self) -> (f32, f32, f32);
    /// biome id of a column (1 = plains)
    fn biome(&self, _x: i32, _z: i32) -> i32 {
        PLAINS
    }
}

type Factory = fn(u64) -> Arc<dyn WorldGenerator>;
static GENERATORS: Mutex<Vec<(String, Factory)>> = Mutex::new(Vec::new());

/// `make` gets the world seed.
#[allow(dead_code)] // plugin API
pub fn register_generator(name: &str, make: Factory) {
    GENERATORS.lock().unwrap().push((name.to_lowercase(), make));
}

fn find_generator(name: &str) -> Option<Factory> {
    GENERATORS.lock().unwrap().iter().find(|(n, _)| n == name).map(|(_, f)| *f)
}

pub enum Generator {
    /// bottom-up from y -64, empty = void
    Flat { layers: Vec<i32>, chunk: OnceLock<(Vec<u8>, u32)> },
    Normal { seed: u64, cache: crate::terrain::Cache },
    /// vanilla-style terrain, ported from PNX's generator
    Vanilla { seed: u64, ow: Arc<crate::overworld::Overworld>, cache: VanillaCache, spawn: OnceLock<(f32, f32, f32)> },
    Custom { name: String, seed: u64, generator: Arc<dyn WorldGenerator> },
}

/// Chunk column from another dimension (local x/z, the dimension's own y).
pub trait Column: Send + Sync {
    fn get(&self, lx: i32, y: i32, lz: i32) -> i32;
    fn biome(&self, lx: i32, y: i32, lz: i32) -> u16;
}

pub trait DimGen: Send + Sync {
    fn column(&self, cx: i32, cz: i32) -> Arc<dyn Column>;
}

/// Nether and End generators (set at startup) and their chunk cache.
#[derive(Default)]
pub struct Dims {
    gens: [OnceLock<Box<dyn DimGen>>; 2],
    cache: Mutex<HashMap<(u8, i32, i32), Arc<dyn Column>>>,
}

impl Dims {
    fn column(&self, dim: u8, cx: i32, cz: i32) -> Option<Arc<dyn Column>> {
        let g = self.gens.get(dim as usize - 1)?.get()?;
        if let Some(c) = self.cache.lock().unwrap().get(&(dim, cx, cz)) {
            return Some(c.clone());
        }
        let c = g.column(cx, cz); // outside the lock, in parallel
        let mut m = self.cache.lock().unwrap();
        if m.len() >= VANILLA_CACHE_LIMIT {
            let keys: Vec<_> = m.keys().copied().step_by(2).collect();
            for k in keys {
                m.remove(&k);
            }
        }
        Some(m.entry((dim, cx, cz)).or_insert(c).clone())
    }
}

/// Player edits per chunk column on top of generated terrain, shared by all sessions.
/// `World::default()` is flat. Nether and End sit in height bands above, see dims.rs.
#[derive(Clone)]
pub struct World {
    changes: Arc<Mutex<HashMap<(i32, i32), HashMap<(i32, i32, i32), i32>>>>,
    /// set on edit, cleared after a save
    dirty: Arc<std::sync::atomic::AtomicBool>,
    generator: Arc<Generator>,
    dims: Arc<Dims>,
}

impl Default for World {
    fn default() -> Self {
        Self::flat(DEFAULT_LAYERS.to_vec())
    }
}

/// Bounded so exploring doesn't eat memory forever.
#[derive(Default)]
pub struct VanillaCache(Mutex<HashMap<(i32, i32), Arc<crate::overworld::Chunk>>>);

const VANILLA_CACHE_LIMIT: usize = 1024;

/// worlds/<level>/chunks, set once at startup.
pub static CHUNK_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();

/// Load a finished chunk from disk, or generate and write it.
fn load_or_generate(ow: &crate::overworld::Overworld, cx: i32, cz: i32) -> crate::overworld::Chunk {
    let file = CHUNK_DIR.get().map(|d| d.join(format!("{cx}.{cz}.bin")));
    if let Some(f) = &file
        && let Ok(data) = std::fs::read(f)
        && let Some(c) = crate::overworld::Chunk::from_bytes(&data)
    {
        return c;
    }
    let c = ow.generate(cx, cz);
    if let Some(f) = file {
        let tmp = f.with_extension("tmp");
        if std::fs::write(&tmp, c.to_bytes()).and_then(|()| std::fs::rename(&tmp, &f)).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
    c
}

impl VanillaCache {
    fn get(&self, ow: &crate::overworld::Overworld, cx: i32, cz: i32) -> Arc<crate::overworld::Chunk> {
        if let Some(c) = self.0.lock().unwrap().get(&(cx, cz)) {
            return c.clone();
        }
        let c = Arc::new(load_or_generate(ow, cx, cz)); // outside the lock, in parallel
        let mut m = self.0.lock().unwrap();
        if m.len() >= VANILLA_CACHE_LIMIT {
            // TODO: dropping a random half at the cap is crude, switch to LRU if regen shows up in profiles
            let keys: Vec<_> = m.keys().copied().step_by(2).collect();
            for k in keys {
                m.remove(&k);
            }
        }
        m.entry((cx, cz)).or_insert(c).clone()
    }
}

/// Save v3: magic, version, generator (0 flat, 1 normal, 2 custom), seed u64, flat layer
/// count u32 + ids, custom name len u32 + bytes, entry count, then (x, y, z, block id) LE i32s.
/// v2 has no layers/name, v1 (no generator) is always flat.
const SAVE_MAGIC: &[u8; 4] = b"RSTW";
const SAVE_VERSION: u32 = 3;

impl World {
    pub fn with_generator(generator: Generator) -> Self {
        Self { changes: Arc::default(), dirty: Arc::default(), generator: Arc::new(generator), dims: Arc::default() }
    }

    /// No layers means a void world (with one bedrock block to stand on).
    pub fn flat(layers: Vec<i32>) -> Self {
        let void = layers.is_empty();
        let w = Self::with_generator(Generator::Flat { layers, chunk: OnceLock::new() });
        if void {
            w.set(0, 63, 0, BEDROCK);
        }
        w
    }

    pub fn vanilla(seed: u64) -> Self {
        Self::with_generator(Generator::Vanilla { seed, ow: Arc::new(crate::overworld::Overworld::new(seed)), cache: VanillaCache::default(), spawn: OnceLock::new() })
    }

    /// None if no plugin registered that generator.
    pub fn custom(name: &str, seed: u64) -> Option<Self> {
        let generator = find_generator(name)?(seed);
        Some(Self::with_generator(Generator::Custom { name: name.into(), seed, generator }))
    }

    /// Only knows biomes for the vanilla generator.
    pub fn biome_at(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        let dim = crate::dims::of(y);
        if dim != crate::dims::OVERWORLD {
            let lo = crate::dims::range(dim).0;
            return self.dims.column(dim, x >> 4, z >> 4).map(|c| c.biome(x & 15, y - lo, z & 15));
        }
        match &*self.generator {
            Generator::Vanilla { ow, cache, .. } => Some(cache.get(ow, x >> 4, z >> 4).biome(x & 15, y, z & 15)),
            _ => None,
        }
    }

    /// Generated terrain only.
    pub fn is_snowy(&self, x: i32, z: i32) -> bool {
        match &*self.generator {
            Generator::Normal { seed, cache } => {
                cache.get(*seed, x >> 4, z >> 4).biome(x & 15, z & 15) == crate::terrain::Biome::Snowy
            }
            Generator::Vanilla { ow, cache, .. } => {
                // Bedrock ids of the snowy overworld biomes
                matches!(cache.get(ow, x >> 4, z >> 4).surface_biome(x & 15, z & 15), 10..=13 | 26 | 30 | 31 | 46 | 47 | 140 | 158 | 182..=185)
            }
            _ => false,
        }
    }

    /// Rough block light from light blocks players placed (torches, lanterns, glowstone...),
    /// brightest source minus taxicab distance.
    // TODO: walls don't block it and generated light (lava, glow lichen) is ignored.
    // Needs a real flood-fill light engine.
    pub fn block_light(&self, x: i32, y: i32, z: i32) -> u8 {
        // from PNX light propagation: -1 per block, opaque blocks stop it. We search outward
        // from the target instead of storing light since it's only used for spawn checks.
        // TODO: water/leaves should dim by more than 1, needs a stored light map
        use std::collections::{HashSet, VecDeque};
        let mut seen = HashSet::from([(x, y, z)]);
        let mut queue = VecDeque::from([((x, y, z), 0i32)]);
        let mut best = 0;
        while let Some(((px, py, pz), d)) = queue.pop_front() {
            if 15 - d <= best {
                break; // breadth-first: nothing farther can beat this
            }
            let id = self.get(px, py, pz);
            best = best.max(emission(id) - d);
            if d > 0 && crate::overworld::feature::blocks::is_solid(id) {
                continue; // emits, but light doesn't pass through it
            }
            for n in [(px + 1, py, pz), (px - 1, py, pz), (px, py + 1, pz), (px, py - 1, pz), (px, py, pz + 1), (px, py, pz - 1)] {
                if seen.insert(n) {
                    queue.push_back((n, d + 1));
                }
            }
        }
        best.clamp(0, 15) as u8
    }

    /// Player-placed blocks in chunk (cx, cz), as world positions.
    pub fn edits_in(&self, cx: i32, cz: i32) -> Vec<((i32, i32, i32), i32)> {
        self.changes
            .lock()
            .unwrap()
            .get(&(cx, cz))
            .map(|c| c.iter().map(|(&(lx, y, lz), &id)| ((cx * 16 + lx, y, cz * 16 + lz), id)).collect())
            .unwrap_or_default()
    }

    pub fn placed(&self, f: impl Fn(i32) -> bool) -> Vec<(i32, i32, i32)> {
        let all = self.changes.lock().unwrap();
        all.iter()
            .flat_map(|(&(cx, cz), col)| col.iter().filter(|(_, id)| f(**id)).map(move |(&(lx, y, lz), _)| (cx * 16 + lx, y, cz * 16 + lz)))
            .collect()
    }

    pub fn edited_chunks(&self) -> usize {
        self.changes.lock().unwrap().len()
    }

    /// 0 for flat worlds
    pub fn seed(&self) -> u64 {
        match &*self.generator {
            Generator::Flat { .. } => 0,
            Generator::Normal { seed, .. } | Generator::Custom { seed, .. } | Generator::Vanilla { seed, .. } => *seed,
        }
    }

    pub fn normal(seed: u64) -> Self {
        Self::with_generator(Generator::Normal { seed, cache: crate::terrain::Cache::default() })
    }

    /// `dim` 1 is the Nether, 2 the End.
    pub fn set_dim(&self, dim: u8, g: Box<dyn DimGen>) {
        if let Some(slot) = self.dims.gens.get(dim as usize - 1) {
            let _ = slot.set(g);
        }
    }

    fn generated_at(&self, x: i32, y: i32, z: i32) -> i32 {
        let dim = crate::dims::of(y);
        if dim != crate::dims::OVERWORLD {
            let (lo, hi) = crate::dims::range(dim);
            if !(lo..=hi).contains(&y) {
                return AIR;
            }
            return self.dims.column(dim, x >> 4, z >> 4).map_or(AIR, |c| c.get(x & 15, y - lo, z & 15));
        }
        match &*self.generator {
            Generator::Flat { layers, .. } => layer(layers, y),
            Generator::Normal { seed, cache } => {
                let (cx, cz) = (x >> 4, z >> 4);
                crate::terrain::block(*seed, &cache.get(*seed, cx, cz), cx, cz, x & 15, y, z & 15)
            }
            Generator::Custom { generator, .. } => generator.block(x, y, z),
            Generator::Vanilla { ow, cache, .. } => cache.get(ow, x >> 4, z >> 4).get(x & 15, y, z & 15),
        }
    }

    pub fn spawn(&self) -> (f32, f32, f32) {
        match &*self.generator {
            Generator::Flat { layers, .. } if layers.is_empty() => (0.5, 64.0 + 1.62, 0.5),
            Generator::Flat { layers, .. } => (0.5, (MIN_Y + layers.len() as i32) as f32 + 1.62, 0.5),
            Generator::Normal { seed, .. } => crate::terrain::spawn_point(*seed),
            Generator::Custom { generator, .. } => generator.spawn(),
            Generator::Vanilla { ow, cache, spawn, .. } => *spawn.get_or_init(|| ow.spawn(|cx, cz| cache.get(ow, cx, cz))),
        }
    }

    /// False if y is outside the world.
    pub fn set(&self, x: i32, y: i32, z: i32, id: i32) -> bool {
        let (lo, hi) = crate::dims::range(crate::dims::of(y));
        if !(lo..=hi).contains(&y) {
            return false;
        }
        let mut all = self.changes.lock().unwrap();
        let col = all.entry((x >> 4, z >> 4)).or_default();
        let local = (x & 15, y, z & 15);
        if self.generated_at(x, y, z) == id {
            col.remove(&local); // back to how the world was generated: nothing to remember
        } else {
            col.insert(local, id);
        }
        self.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
        true
    }

    /// Saves if anything changed. Writes a temp file and renames it so a crash mid-save
    /// can't leave a half-written world.
    pub fn save_if_dirty(&self, path: &std::path::Path) -> std::io::Result<bool> {
        if !self.dirty.swap(false, std::sync::atomic::Ordering::Relaxed) {
            return Ok(false);
        }
        let mut buf = Vec::new();
        {
            let all = self.changes.lock().unwrap();
            let count: usize = all.values().map(HashMap::len).sum();
            buf.extend(SAVE_MAGIC);
            buf.extend(SAVE_VERSION.to_le_bytes());
            let (kind, seed, layers, name): (u8, u64, &[i32], &str) = match &*self.generator {
                Generator::Flat { layers, .. } => (0, 0, layers, ""),
                Generator::Normal { seed, .. } => (1, *seed, &[], ""),
                Generator::Custom { name, seed, .. } => (2, *seed, &[], name),
                Generator::Vanilla { seed, .. } => (3, *seed, &[], ""),
            };
            buf.push(kind);
            buf.extend(seed.to_le_bytes());
            buf.extend((layers.len() as u32).to_le_bytes());
            for l in layers {
                buf.extend(l.to_le_bytes());
            }
            buf.extend((name.len() as u32).to_le_bytes());
            buf.extend(name.as_bytes());
            buf.extend((count as u32).to_le_bytes());
            for (&(cx, cz), col) in all.iter() {
                for (&(lx, y, lz), &id) in col {
                    for v in [cx * 16 + lx, y, cz * 16 + lz, id] {
                        buf.extend(v.to_le_bytes());
                    }
                }
            }
        }
        let tmp = path.with_extension("tmp");
        let result = std::fs::write(&tmp, &buf).and_then(|()| std::fs::rename(&tmp, path));
        if result.is_err() {
            self.dirty.store(true, std::sync::atomic::Ordering::Relaxed); // retry next time
        }
        result.map(|()| true)
    }

    /// A missing file starts a new world from `create`, saved on the first autosave so the
    /// seed and generator aren't lost.
    pub fn load(path: &std::path::Path, create: impl FnOnce() -> std::io::Result<Self>) -> std::io::Result<Self> {
        let buf = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let world = create()?;
                world.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
                return Ok(world);
            }
            Err(e) => return Err(e),
        };
        let bad = |why: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{}: {why}", path.display()));
        if buf.len() < 12 || &buf[..4] != SAVE_MAGIC {
            return Err(bad("not a Rusted world file"));
        }
        let int = |i: usize| i32::from_le_bytes(buf[i..i + 4].try_into().unwrap());
        // v1 (flat only) has count at 8. v2 has generator at 8, seed 9..17, count at 17.
        let (world, start) = match int(4) as u32 {
            1 => (Self::default(), 8),
            2 if buf.len() >= 21 => {
                let seed = u64::from_le_bytes(buf[9..17].try_into().unwrap());
                let world = match buf[8] {
                    0 => Self::default(),
                    1 => Self::normal(seed),
                    _ => return Err(bad("unknown world generator")),
                };
                (world, 17)
            }
            3 if buf.len() >= 25 => {
                let seed = u64::from_le_bytes(buf[9..17].try_into().unwrap());
                let n = int(17) as u32 as usize;
                let layers_end = 21 + n * 4;
                if buf.len() < layers_end + 4 {
                    return Err(bad("truncated or corrupt"));
                }
                let layers = (0..n).map(|i| int(21 + i * 4)).collect();
                let name_len = int(layers_end) as u32 as usize;
                let name = buf.get(layers_end + 4..layers_end + 4 + name_len).ok_or_else(|| bad("truncated or corrupt"))?;
                let name = String::from_utf8_lossy(name).to_string();
                let world = match buf[8] {
                    0 => Self::with_generator(Generator::Flat { layers, chunk: OnceLock::new() }),
                    1 => Self::normal(seed),
                    2 => Self::custom(&name, seed).ok_or_else(|| bad(&format!("world generator '{name}' is not installed")))?,
                    3 => Self::vanilla(seed),
                    _ => return Err(bad("unknown world generator")),
                };
                (world, layers_end + 4 + name_len)
            }
            _ => return Err(bad("unsupported save version")),
        };
        let count = int(start) as u32 as usize;
        if buf.len() != start + 4 + count * 16 {
            return Err(bad("truncated or corrupt"));
        }
        for e in 0..count {
            let o = start + 4 + e * 16;
            world.set(int(o), int(o + 4), int(o + 8), int(o + 12));
        }
        world.dirty.store(false, std::sync::atomic::Ordering::Relaxed);
        Ok(world)
    }

    pub fn get(&self, x: i32, y: i32, z: i32) -> i32 {
        self.changes
            .lock()
            .unwrap()
            .get(&(x >> 4, z >> 4))
            .and_then(|c| c.get(&(x & 15, y, z & 15)).copied())
            .unwrap_or_else(|| self.generated_at(x, y, z))
    }

    /// LevelChunk payload for a Nether/End column (own y from 0) and its sub-chunk count
    /// (8 Nether, 16 End).
    pub fn chunk_payload_dim(&self, cx: i32, cz: i32, dim: u8) -> (Vec<u8>, u32) {
        if dim == crate::dims::OVERWORLD {
            return self.chunk_payload(cx, cz);
        }
        let (lo, hi) = crate::dims::range(dim);
        let edits = self.changes.lock().unwrap().get(&(cx, cz)).cloned().unwrap_or_default();
        let col = self.dims.column(dim, cx, cz);
        // edits are stored `lo` blocks up
        let local: HashMap<(i32, i32, i32), i32> = edits.into_iter().filter(|((_, y, _), _)| (lo..=hi).contains(y)).map(|((x, y, z), b)| ((x, y - lo, z), b)).collect();
        let default_biome = if dim == crate::dims::NETHER { 8 } else { 9 };
        encode_range(
            &local,
            |lx, y, lz| col.as_ref().map_or(AIR, |c| c.get(lx, y, lz)),
            |lx, y, lz| col.as_ref().map_or(default_biome, |c| c.biome(lx, y, lz) as i32),
            0,
            ((hi - lo + 1) / 16) as usize,
        )
    }

    pub fn chunk_payload(&self, cx: i32, cz: i32) -> (Vec<u8>, u32) {
        let edits = self.changes.lock().unwrap().get(&(cx, cz)).cloned().unwrap_or_default();
        match &*self.generator {
            Generator::Flat { layers, chunk } => {
                let flat = |_: i32, y: i32, _: i32| layer(layers, y);
                if edits.is_empty() {
                    return chunk.get_or_init(|| encode_column(&HashMap::new(), flat, |_, _, _| PLAINS)).clone();
                }
                encode_column(&edits, flat, |_, _, _| PLAINS)
            }
            Generator::Custom { generator, .. } => encode_column(
                &edits,
                |lx, y, lz| generator.block(cx * 16 + lx, y, cz * 16 + lz),
                |lx, _, lz| generator.biome(cx * 16 + lx, cz * 16 + lz),
            ),
            Generator::Vanilla { ow, cache, .. } => {
                let ch = cache.get(ow, cx, cz);
                encode_column(&edits, |lx, y, lz| ch.get(lx, y, lz), |lx, y, lz| ch.biome(lx, y, lz) as i32)
            }
            Generator::Normal { seed, cache } => {
                let g = cache.get(*seed, cx, cz);
                encode_column(
                    &edits,
                    |lx, y, lz| crate::terrain::block(*seed, &g, cx, cz, lx, y, lz),
                    |lx, _, lz| g.biome(lx, lz).id(),
                )
            }
        }
    }
}

fn layer(layers: &[i32], y: i32) -> i32 {
    layers.get((y - MIN_Y) as usize).copied().unwrap_or(AIR)
}

/// Vanilla light levels, 0 = none.
pub fn emission(id: i32) -> i32 {
    let Some(e) = crate::blocks::entry(id) else { return 0 };
    match e.name.trim_start_matches("minecraft:") {
        "glowstone" | "sea_lantern" | "lantern" | "jack_o_lantern" | "lava" | "flowing_lava" | "beacon" | "shroomlight" | "campfire" | "lit_pumpkin"
        | "ochre_froglight" | "verdant_froglight" | "pearlescent_froglight" | "end_gateway" | "conduit" | "respawn_anchor" | "fire" => 15,
        "torch" | "end_rod" | "lit_furnace" | "lit_smoker" | "lit_blast_furnace" | "wall_torch" => 14,
        "soul_lantern" | "soul_torch" | "soul_campfire" | "soul_fire" | "crying_obsidian" => 10,
        "redstone_torch" | "glow_lichen" | "enchanting_table" | "ender_chest" => 7,
        "magma" | "brewing_stand" => 3,
        n if n.ends_with("candle") || n.ends_with("candle_cake") => 3,
        _ => 0,
    }
}

/// Paletted storage: header (bits << 1 | 1), packed words, palette length, palette.
/// One-entry palettes use 0 bits and skip the length.
fn write_paletted(buf: &mut Vec<u8>, indices: &[u16; 4096], palette: &[i32]) {
    if palette.len() == 1 {
        buf.push(1);
        write_varint(buf, palette[0]);
        return;
    }
    let bits = [1u32, 2, 3, 4, 5, 6, 8, 16].into_iter().find(|b| palette.len() <= 1 << b).unwrap();
    let per_word = 32 / bits;
    let mut words = vec![0u32; 4096_usize.div_ceil(per_word as usize)];
    for (i, &p) in indices.iter().enumerate() {
        let i = i as u32;
        words[(i / per_word) as usize] |= (p as u32) << ((i % per_word) * bits);
    }
    buf.push(((bits as u8) << 1) | 1);
    for w in words {
        buf.extend(w.to_le_bytes());
    }
    write_varint(buf, palette.len() as i32);
    for &id in palette {
        write_varint(buf, id);
    }
}

fn write_varint(buf: &mut Vec<u8>, v: i32) {
    let mut u = ((v << 1) ^ (v >> 31)) as u32; // zigzag
    loop {
        let byte = (u & 0x7f) as u8;
        u >>= 7;
        if u == 0 {
            buf.push(byte);
            return;
        }
        buf.push(byte | 0x80);
    }
}

/// Encodes a column: generated blocks from `base` (local x, world y, local z) with `edits`
/// on top, plus `biome` per column.
fn encode_column(
    edits: &HashMap<(i32, i32, i32), i32>,
    base: impl Fn(i32, i32, i32) -> i32,
    biome: impl Fn(i32, i32, i32) -> i32,
) -> (Vec<u8>, u32) {
    // Always send the full height. The client treats missing sub-chunks as solid and players
    // got stuck in the air above the ground.
    encode_range(edits, base, biome, MIN_Y, SUBCHUNKS)
}

fn encode_range(
    edits: &HashMap<(i32, i32, i32), i32>,
    base: impl Fn(i32, i32, i32) -> i32,
    biome: impl Fn(i32, i32, i32) -> i32,
    min_y: i32,
    count: usize,
) -> (Vec<u8>, u32) {
    let block = |x: i32, y: i32, z: i32| edits.get(&(x, y, z)).copied().unwrap_or_else(|| base(x, y, z));
    // Always send the full height. The client treats missing sub-chunks as solid and players
    // got stuck in the air above the ground.

    let mut buf = Vec::with_capacity(count * 1100 + 64);
    for s in 0..count {
        let base_y = min_y + s as i32 * 16;
        let mut palette: Vec<i32> = Vec::new();
        let mut indices = [0u16; 4096];
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..16 {
                    let id = block(x, base_y + y, z);
                    let p = palette.iter().position(|&b| b == id).unwrap_or_else(|| {
                        palette.push(id);
                        palette.len() - 1
                    });
                    indices[((x << 8) | (z << 4) | y) as usize] = p as u16; // Bedrock order: x, z, y
                }
            }
        }

        buf.extend([9, 1, (base_y >> 4) as i8 as u8]); // format 9, 1 layer, absolute index
        write_paletted(&mut buf, &indices, &palette);
    }

    // one biome storage per sub-chunk, same format (cave biomes change with height)
    for s in 0..count {
        let base_y = min_y + s as i32 * 16;
        let mut biome_palette: Vec<i32> = Vec::new();
        let mut biome_indices = [0u16; 4096];
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..16 {
                    let b = biome(x, base_y + y, z);
                    let p = biome_palette.iter().position(|&v| v == b).unwrap_or_else(|| {
                        biome_palette.push(b);
                        biome_palette.len() - 1
                    });
                    biome_indices[((x << 8) | (z << 4) | y) as usize] = p as u16;
                }
            }
        }
        write_paletted(&mut buf, &biome_indices, &biome_palette);
    }
    buf.push(0); // border blocks (education edition)
    (buf, count as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Checker;
    impl WorldGenerator for Checker {
        fn block(&self, x: i32, y: i32, z: i32) -> i32 {
            if y == 0 && (x + z) % 2 == 0 { BEDROCK } else { AIR }
        }
        fn spawn(&self) -> (f32, f32, f32) {
            (0.5, 2.62, 0.5)
        }
    }

    #[test]
    fn flat_layers_void_and_plugin_generators_save_and_load() {
        let path = std::env::temp_dir().join(format!("rusted_gen_{}.dat", std::process::id()));
        let layers = vec![BEDROCK, DIRT, GRASS, GRASS];
        let w = World::flat(layers.clone());
        assert_eq!(w.get(3, -62, 3), GRASS);
        assert_eq!(w.get(3, -61, 3), GRASS);
        assert_eq!(w.get(3, -60, 3), AIR);
        assert_eq!(w.spawn().1, -60.0 + 1.62);
        w.set(1, -59, 1, DIRT);
        w.save_if_dirty(&path).unwrap();
        let back = World::load(&path, || Ok(World::normal(1))).unwrap();
        assert!(matches!(&*back.generator, Generator::Flat { layers: l, .. } if *l == layers), "custom layers survive a save");
        assert_eq!(back.get(1, -59, 1), DIRT);

        let void = World::flat(vec![]);
        assert_eq!(void.get(0, 63, 0), BEDROCK, "void spawn platform");
        assert_eq!(void.get(5, -64, 5), AIR);

        register_generator("checker", |_| std::sync::Arc::new(Checker));
        let c = World::custom("checker", 9).unwrap();
        assert_eq!((c.get(2, 0, 0), c.get(1, 0, 0)), (BEDROCK, AIR));
        c.set(1, 5, 1, DIRT);
        c.save_if_dirty(&path).unwrap();
        let back = World::load(&path, || Ok(World::normal(1))).unwrap();
        assert_eq!((back.seed(), back.get(2, 0, 0), back.get(1, 5, 1)), (9, BEDROCK, DIRT));
        assert!(World::custom("missing", 1).is_none());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn block_light_spreads_and_walls_block_it() {
        crate::blocks::init();
        let w = World::flat(vec![BEDROCK, DIRT, GRASS]);
        let glow = crate::blocks::any_state("minecraft:glowstone").unwrap();
        w.set(0, -61, 0, glow);
        assert_eq!(w.block_light(3, -61, 0), 12, "15 minus 3 blocks");
        // seal (5,-61,0) in stone, no light inside
        let stone = crate::blocks::any_state("minecraft:stone").unwrap();
        for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, 0, 1), (0, 0, -1)] {
            w.set(5 + dx, -61 + dy, dz, stone);
        }
        assert_eq!(w.block_light(5, -61, 0), 0);
    }

    #[test]
    fn varint_zigzag() {
        let mut b = Vec::new();
        write_varint(&mut b, 1);
        write_varint(&mut b, -1);
        write_varint(&mut b, 300);
        assert_eq!(b, [2, 1, 0xd8, 0x04]);
    }

    #[test]
    fn flat_chunk_layout() {
        let (p, count) = World::default().chunk_payload(0, 0);
        assert_eq!(count, SUBCHUNKS as u32, "full height, like vanilla");
        assert_eq!(&p[..4], &[9, 1, 0xfc, 0b101]); // 4 block types -> 2 bits
        let word0 = u32::from_le_bytes(p[4..8].try_into().unwrap());
        let index = |y: u32| (word0 >> (y * 2)) & 3;
        // palette goes in order of first use: bedrock, dirt, grass, air
        assert_eq!([index(0), index(1), index(2), index(3), index(4)], [0, 1, 1, 2, 3]);
        let palette_at = 4 + 256 * 4;
        assert_eq!(p[palette_at], 8); // zigzag(4 entries)
        assert_eq!(*p.last().unwrap(), 0);
    }

    #[test]
    fn edits_change_and_revert() {
        let w = World::default();
        let flat = w.chunk_payload(0, 0);
        assert!(w.set(3, -61, 4, AIR));
        assert_eq!(w.get(3, -61, 4), AIR);
        assert_ne!(w.chunk_payload(0, 0), flat);
        // putting the original block back drops the edit
        assert!(w.set(3, -61, 4, GRASS));
        assert_eq!(w.chunk_payload(0, 0), flat);
        // negative coords should land in the right chunk
        w.set(-1, -61, -1, AIR);
        assert_eq!(w.get(-1, -61, -1), AIR);
        assert_eq!(w.get(15, -61, 15), GRASS);
    }

    #[test]
    fn save_and_load_round_trip() {
        let path = std::env::temp_dir().join(format!("rusted_world_test_{}.dat", std::process::id()));
        let w = World::default();
        assert!(!w.save_if_dirty(&path).unwrap(), "nothing to save yet");
        w.set(5, -61, -20, AIR);
        w.set(-3, 70, 9, DIRT);
        assert!(w.save_if_dirty(&path).unwrap());
        assert!(!w.save_if_dirty(&path).unwrap(), "clean after saving");
        let loaded = World::load(&path, || Ok(World::normal(1))).unwrap();
        assert_eq!(loaded.get(5, -61, -20), AIR);
        assert_eq!(loaded.get(-3, 70, 9), DIRT);
        assert_eq!(loaded.get(0, -61, 0), GRASS);
        assert_eq!(loaded.chunk_payload(0, -2), w.chunk_payload(0, -2));
        // corrupt files get refused, not half loaded
        std::fs::write(&path, b"RSTW      ").unwrap();
        assert!(World::load(&path, || Ok(World::normal(1))).is_err());
        std::fs::remove_file(&path).ok();
        // missing file = new world, saved right away so the seed sticks
        let fresh = World::load(&path, || Ok(World::normal(1))).unwrap();
        assert!(matches!(&*fresh.generator, Generator::Normal { .. }));
        assert!(fresh.save_if_dirty(&path).unwrap());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn terrain_chunks_are_fast_enough() {
        // crossing into a new chunk needs ~17 chunks, a fresh join ~200
        let w = World::normal(5);
        let start = std::time::Instant::now();
        for i in 0..200 {
            w.chunk_payload(i % 20, i / 20);
        }
        let per_chunk = start.elapsed() / 200;
        println!("terrain chunk: {per_chunk:?} each");
        assert!(per_chunk < std::time::Duration::from_millis(25), "{per_chunk:?} per chunk");
    }

    #[test]
    fn old_v1_saves_stay_flat() {
        let path = std::env::temp_dir().join(format!("rusted_world_v1_{}.dat", std::process::id()));
        let mut v1 = b"RSTW".to_vec();
        v1.extend(1u32.to_le_bytes());
        v1.extend(1u32.to_le_bytes());
        for v in [5i32, -61, 5, AIR] {
            v1.extend(v.to_le_bytes());
        }
        std::fs::write(&path, &v1).unwrap();
        let w = World::load(&path, || Ok(World::normal(1))).unwrap();
        assert!(matches!(&*w.generator, Generator::Flat { .. }));
        assert_eq!(w.get(5, -61, 5), AIR);
        assert_eq!(w.get(6, -61, 5), GRASS);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn terrain_world_edits_and_saves() {
        let path = std::env::temp_dir().join(format!("rusted_world_v2_{}.dat", std::process::id()));
        let w = World::normal(77);
        let (x, z) = (3, 4);
        let top = (MIN_Y..=MAX_Y).rev().find(|&y| w.get(x, y, z) != AIR).unwrap();
        assert!(w.set(x, top, z, AIR));
        assert!(w.save_if_dirty(&path).unwrap());
        let back = World::load(&path, || Ok(World::normal(1))).unwrap();
        assert_eq!(back.get(x, top, z), AIR);
        assert_eq!(back.get(x + 1, top - 5, z), w.get(x + 1, top - 5, z), "same seed, same terrain");
        let (payload, count) = back.chunk_payload(0, 0);
        assert_eq!(count, SUBCHUNKS as u32);
        assert!(payload.len() > 2000);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn tall_build_changes_its_subchunk() {
        let w = World::default();
        let flat = w.chunk_payload(0, 0);
        w.set(0, 100, 0, DIRT);
        let (built, count) = w.chunk_payload(0, 0);
        assert_eq!(count, SUBCHUNKS as u32);
        assert!(built.len() > flat.0.len());
        assert!(!w.set(0, 400, 0, DIRT));
    }
}

//! Overworld terrain. Deterministic from the seed so only player edits get saved.
//! Heights, biomes and features are cached per chunk, single blocks are cheap rules after that.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub const SEA_LEVEL: i32 = 62;
const MIN_Y: i32 = -64;

/// Network ids of the blocks terrain uses, looked up once.
pub struct Ids {
    pub air: i32,
    pub bedrock: i32,
    pub stone: i32,
    pub deepslate: i32,
    pub dirt: i32,
    pub grass: i32,
    pub sand: i32,
    pub sandstone: i32,
    pub gravel: i32,
    pub water: i32,
    pub ice: i32,
    pub snow_layer: i32,
    pub oak_log: i32,
    pub oak_leaves: i32,
    pub birch_log: i32,
    pub birch_leaves: i32,
    pub spruce_log: i32,
    pub spruce_leaves: i32,
    pub short_grass: i32,
    pub dandelion: i32,
    pub poppy: i32,
    pub cactus: i32,
    pub ores: [(i32, i32, i32, i32, u32); 5], // (stone ore, deepslate ore, min y, max y, 1-in-N)
}

pub fn ids() -> &'static Ids {
    static IDS: OnceLock<Ids> = OnceLock::new();
    IDS.get_or_init(|| {
        let b = |name: &str| crate::blocks::any_state(&format!("minecraft:{name}")).unwrap_or_else(|| panic!("no block {name}"));
        Ids {
            air: crate::world::AIR,
            bedrock: crate::world::BEDROCK,
            stone: b("stone"),
            deepslate: b("deepslate"),
            dirt: crate::world::DIRT,
            grass: crate::world::GRASS,
            sand: b("sand"),
            sandstone: b("sandstone"),
            gravel: b("gravel"),
            water: b("water"),
            ice: b("ice"),
            snow_layer: b("snow_layer"),
            oak_log: b("oak_log"),
            oak_leaves: b("oak_leaves"),
            birch_log: b("birch_log"),
            birch_leaves: b("birch_leaves"),
            spruce_log: b("spruce_log"),
            spruce_leaves: b("spruce_leaves"),
            short_grass: b("short_grass"),
            dandelion: b("dandelion"),
            poppy: b("poppy"),
            cactus: b("cactus"),
            ores: [
                (b("coal_ore"), b("deepslate_coal_ore"), 0, 128, 90),
                (b("copper_ore"), b("deepslate_copper_ore"), -16, 96, 140),
                (b("iron_ore"), b("deepslate_iron_ore"), -48, 72, 130),
                (b("gold_ore"), b("deepslate_gold_ore"), -64, 32, 420),
                (b("diamond_ore"), b("deepslate_diamond_ore"), -64, 14, 650),
            ],
        }
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Biome {
    Ocean,
    Beach,
    Plains,
    Forest,
    Desert,
    Snowy,
}

impl Biome {
    pub fn id(self) -> i32 {
        match self {
            Biome::Ocean => 0,
            Biome::Plains => 1,
            Biome::Desert => 2,
            Biome::Forest => 4,
            Biome::Snowy => 12,
            Biome::Beach => 16,
        }
    }
}

/// Cheap int hash, all the randomness comes from this so it's stable everywhere.
fn hash(seed: u64, x: i32, y: i32, z: i32) -> u64 {
    let mut h = seed ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (z as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^ (h >> 33)
}

fn chance(seed: u64, x: i32, y: i32, z: i32, one_in: u32) -> bool {
    hash(seed, x, y, z).is_multiple_of(u64::from(one_in))
}

/// Perlin-style 2D noise, roughly -1..1.
fn noise(seed: u64, x: f64, z: f64) -> f64 {
    let (x0, z0) = (x.floor(), z.floor());
    let (fx, fz) = (x - x0, z - z0);
    let (ix, iz) = (x0 as i32, z0 as i32);
    let grad = |gx: i32, gz: i32, dx: f64, dz: f64| {
        let a = (hash(seed, gx, 0, gz) % 360) as f64 * std::f64::consts::PI / 180.0;
        a.cos() * dx + a.sin() * dz
    };
    let fade = |t: f64| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (u, v) = (fade(fx), fade(fz));
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let top = lerp(grad(ix, iz, fx, fz), grad(ix + 1, iz, fx - 1.0, fz), u);
    let bottom = lerp(grad(ix, iz + 1, fx, fz - 1.0), grad(ix + 1, iz + 1, fx - 1.0, fz - 1.0), u);
    lerp(top, bottom, v) * 1.414
}

/// Tunnels where two noise fields both cross zero. They shift with height so tunnels
/// go up and down.
fn is_cave(seed: u64, x: i32, y: i32, z: i32) -> bool {
    let (x, y, z) = (f64::from(x), f64::from(y), f64::from(z));
    let a = noise(seed ^ 0xC0FE, x / 48.0 + y / 31.0, z / 48.0);
    if a.abs() > 0.07 {
        return false; // cheap early out for most blocks
    }
    let b = noise(seed ^ 0xCA7E, x / 48.0, z / 48.0 + y / 27.0);
    b.abs() < 0.07
}

/// Octave noise, `scale` blocks per feature.
fn fbm(seed: u64, x: i32, z: i32, scale: f64, octaves: u32) -> f64 {
    let (mut sum, mut amp, mut freq, mut total) = (0.0, 1.0, 1.0 / scale, 0.0);
    for o in 0..octaves {
        sum += noise(seed.wrapping_add(u64::from(o) * 7919), f64::from(x) * freq, f64::from(z) * freq) * amp;
        total += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / total
}

pub fn column(seed: u64, x: i32, z: i32) -> (i32, Biome) {
    let continent = fbm(seed, x, z, 700.0, 3);
    let hills = fbm(seed ^ 0x51, x, z, 180.0, 3);
    let detail = fbm(seed ^ 0xA7, x, z, 40.0, 2);
    let land = ((continent + 0.15) * 2.5).clamp(-1.0, 1.0); // > 0 is land
    let height = if land > 0.0 {
        64.0 + land * 10.0 + hills.max(0.0) * 34.0 * land + detail * 3.0
    } else {
        SEA_LEVEL as f64 + land * 22.0 + detail * 2.0
    };
    let h = height.round() as i32;
    let temperature = fbm(seed ^ 0x7E, x, z, 420.0, 2);
    let moisture = fbm(seed ^ 0x3C, x, z, 300.0, 2);
    let biome = if h < SEA_LEVEL - 1 {
        Biome::Ocean
    } else if h <= SEA_LEVEL + 1 {
        if temperature < -0.35 { Biome::Snowy } else { Biome::Beach }
    } else if temperature > 0.3 {
        Biome::Desert
    } else if temperature < -0.35 {
        Biome::Snowy
    } else if moisture > 0.1 {
        Biome::Forest
    } else {
        Biome::Plains
    };
    (h, biome)
}

/// Heights, biomes and features (trees, plants) for one chunk.
pub struct ChunkGen {
    heights: [i32; 256],
    biomes: [Biome; 256],
    features: HashMap<(i32, i32, i32), i32>, // local x, world y, local z
}

impl ChunkGen {
    pub fn biome(&self, lx: i32, lz: i32) -> Biome {
        self.biomes[(lx * 16 + lz) as usize]
    }

    pub fn height(&self, lx: i32, lz: i32) -> i32 {
        self.heights[(lx * 16 + lz) as usize]
    }
}

fn tree(ids: &Ids, biome: Biome, seed: u64, x: i32, z: i32) -> Option<(i32, i32, i32)> {
    // (log, leaves, trunk height)
    let h = 4 + (hash(seed, x, 7, z) % 3) as i32;
    match biome {
        Biome::Forest if hash(seed, x, 1, z).is_multiple_of(4) => Some((ids.birch_log, ids.birch_leaves, h + 1)),
        Biome::Forest => Some((ids.oak_log, ids.oak_leaves, h)),
        Biome::Plains => Some((ids.oak_log, ids.oak_leaves, h)),
        Biome::Snowy => Some((ids.spruce_log, ids.spruce_leaves, h + 2)),
        _ => None,
    }
}

fn tree_chance(biome: Biome) -> u32 {
    match biome {
        Biome::Forest => 22,
        Biome::Plains => 260,
        Biome::Snowy => 90,
        _ => 0,
    }
}

pub fn generate_chunk(seed: u64, cx: i32, cz: i32) -> ChunkGen {
    let ids = ids();
    let mut heights = [0; 256];
    let mut biomes = [Biome::Plains; 256];
    for lx in 0..16 {
        for lz in 0..16 {
            let (h, b) = column(seed, cx * 16 + lx, cz * 16 + lz);
            heights[(lx * 16 + lz) as usize] = h;
            biomes[(lx * 16 + lz) as usize] = b;
        }
    }
    let mut features = HashMap::new();
    let mut put = |x: i32, y: i32, z: i32, id: i32, replace_leaves: bool| {
        let (lx, lz) = (x - cx * 16, z - cz * 16);
        if (0..16).contains(&lx) && (0..16).contains(&lz) {
            let e = features.entry((lx, y, lz));
            if replace_leaves {
                e.insert_entry(id);
            } else {
                e.or_insert(id);
            }
        }
    };
    // trees from neighbouring chunks whose canopy (radius 2) reaches in here
    for x in cx * 16 - 2..cx * 16 + 18 {
        for z in cz * 16 - 2..cz * 16 + 18 {
            let (h, b) = column(seed, x, z);
            let every = tree_chance(b);
            if every == 0 || h <= SEA_LEVEL || !chance(seed, x, 3, z, every) {
                continue;
            }
            let Some((log, leaves, trunk)) = tree(ids, b, seed, x, z) else { continue };
            let top = h + trunk;
            for y in top - 2..=top + 1 {
                let r: i32 = if y >= top { 1 } else { 2 };
                for dx in -r..=r {
                    for dz in -r..=r {
                        let corner = dx.abs() == r && dz.abs() == r;
                        if corner && (y == top + 1 || chance(seed, x + dx, y, z + dz, 2)) {
                            continue;
                        }
                        put(x + dx, y, z + dz, leaves, false);
                    }
                }
            }
            for y in h + 1..=top {
                put(x, y, z, log, true); // trunk wins over its own leaves
            }
        }
    }
    // plants on this chunk's own columns
    for lx in 0..16 {
        for lz in 0..16 {
            let (x, z) = (cx * 16 + lx, cz * 16 + lz);
            let h = heights[(lx * 16 + lz) as usize];
            if h <= SEA_LEVEL || features.contains_key(&(lx, h + 1, lz)) {
                continue;
            }
            let plant = match biomes[(lx * 16 + lz) as usize] {
                Biome::Plains | Biome::Forest => {
                    let r = hash(seed, x, 11, z) % 100;
                    if r < 14 { Some(ids.short_grass) } else if r < 15 { Some(ids.dandelion) } else if r < 16 { Some(ids.poppy) } else { None }
                }
                Biome::Desert if chance(seed, x, 13, z, 160) => {
                    let tall = 1 + (hash(seed, x, 14, z) % 3) as i32;
                    for y in h + 2..=h + tall {
                        features.insert((lx, y, lz), ids.cactus);
                    }
                    Some(ids.cactus)
                }
                Biome::Snowy => Some(ids.snow_layer),
                _ => None,
            };
            if let Some(p) = plant {
                features.insert((lx, h + 1, lz), p);
            }
        }
    }
    ChunkGen { heights, biomes, features }
}

pub fn block(seed: u64, g: &ChunkGen, cx: i32, cz: i32, lx: i32, y: i32, lz: i32) -> i32 {
    let ids = ids();
    if let Some(&f) = g.features.get(&(lx, y, lz)) {
        return f;
    }
    let h = g.height(lx, lz);
    let biome = g.biome(lx, lz);
    let (x, z) = (cx * 16 + lx, cz * 16 + lz);
    if y == MIN_Y || (y < MIN_Y + 4 && chance(seed, x, y, z, (y - MIN_Y + 1) as u32 + 1)) {
        return ids.bedrock;
    }
    if y > h {
        return if y <= SEA_LEVEL {
            if y == SEA_LEVEL && biome == Biome::Snowy { ids.ice } else { ids.water }
        } else {
            ids.air
        };
    }
    if y < h - 4 && y > MIN_Y + 4 && is_cave(seed, x, y, z) {
        // deep caves fill with lava, vanilla does this below -54
        return if y < -54 { crate::fluids::Kind::Lava.source() } else { ids.air };
    }
    let depth = h - y;
    let underwater = h < SEA_LEVEL;
    if depth < 4 {
        return match (biome, depth) {
            (Biome::Desert | Biome::Beach, 0..=2) => ids.sand,
            (Biome::Desert | Biome::Beach, _) => ids.sandstone,
            (Biome::Ocean, _) if underwater => if hash(seed, x, 5, z).is_multiple_of(3) { ids.sand } else { ids.gravel },
            (_, 0) if underwater => ids.dirt,
            (_, 0) => ids.grass,
            _ => ids.dirt,
        };
    }
    let deep = y < 0;
    for &(ore, deep_ore, lo, hi, every) in &ids.ores {
        if (lo..=hi).contains(&y) && chance(seed, x, y, z, every) {
            return if deep { deep_ore } else { ore };
        }
    }
    if deep { ids.deepslate } else { ids.stone }
}

/// Bounded so long trips don't eat memory.
#[derive(Default)]
pub struct Cache {
    chunks: Mutex<HashMap<(i32, i32), std::sync::Arc<ChunkGen>>>,
}

const CACHE_LIMIT: usize = 4096;

impl Cache {
    pub fn get(&self, seed: u64, cx: i32, cz: i32) -> std::sync::Arc<ChunkGen> {
        if let Some(g) = self.chunks.lock().unwrap().get(&(cx, cz)) {
            return g.clone();
        }
        let g = std::sync::Arc::new(generate_chunk(seed, cx, cz)); // generated outside the lock
        let mut chunks = self.chunks.lock().unwrap();
        if chunks.len() >= CACHE_LIMIT {
            chunks.clear(); // TODO: LRU instead of clearing everything, if it shows up in profiles
        }
        chunks.entry((cx, cz)).or_insert(g).clone()
    }
}

/// Dry open spot near 0,0 to spawn on (eye height).
pub fn spawn_point(seed: u64) -> (f32, f32, f32) {
    for r in 0i32..64 {
        for (x, z) in [(r * 8, 0), (0, r * 8), (-r * 8, 0), (0, -r * 8), (r * 8, r * 8), (-r * 8, -r * 8)] {
            let (h, b) = column(seed, x, z);
            if h > SEA_LEVEL && !matches!(b, Biome::Ocean) {
                let g = generate_chunk(seed, x >> 4, z >> 4);
                // stand on top of plants/snow too
                let top = (h + 1..h + 12).take_while(|&y| g.features.contains_key(&(x & 15, y, z & 15))).last().unwrap_or(h);
                return (x as f32 + 0.5, top as f32 + 1.0 + 1.62, z as f32 + 0.5);
            }
        }
    }
    (0.5, SEA_LEVEL as f32 + 2.0 + 1.62, 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_varied() {
        assert_eq!(column(42, 100, -200), column(42, 100, -200));
        let heights: Vec<i32> = (0..200).map(|i| column(42, i * 37, i * 53).0).collect();
        let (lo, hi) = (*heights.iter().min().unwrap(), *heights.iter().max().unwrap());
        assert!(hi - lo > 15, "terrain should have hills and valleys ({lo}..{hi})");
        assert!(heights.iter().all(|h| (-30..200).contains(h)), "{lo}..{hi}");
        assert_ne!(column(42, 5000, 5000), column(43, 5000, 5000));
    }

    #[test]
    fn layers_make_sense() {
        let ids = ids();
        let g = generate_chunk(7, 0, 0);
        for lx in 0..16 {
            for lz in 0..16 {
                let h = g.height(lx, lz);
                assert_eq!(block(7, &g, 0, 0, lx, MIN_Y, lz), ids.bedrock);
                let deep = block(7, &g, 0, 0, lx, -30, lz);
                assert!(deep == ids.deepslate || ids.ores.iter().any(|o| o.1 == deep) || deep == ids.bedrock);
                let above = block(7, &g, 0, 0, lx, h + 30, lz);
                assert!(above == ids.air || above == ids.water || g.features.contains_key(&(lx, h + 30, lz)));
                if h < SEA_LEVEL - 1 {
                    assert_eq!(block(7, &g, 0, 0, lx, h + 1, lz), ids.water, "ocean columns fill with water");
                }
            }
        }
    }

    #[test]
    fn caves_exist_underground() {
        let seed = 42;
        let mut air = 0;
        for x in 0..64 {
            for z in 0..64 {
                for y in -40..30 {
                    air += is_cave(seed, x, y, z) as u32;
                }
            }
        }
        let total = 64 * 64 * 70;
        assert!(air > total / 1000 && air < total / 10, "cave share {air}/{total}");
    }

    #[test]
    fn biomes_trees_and_ores_appear() {
        let mut biomes = std::collections::HashSet::new();
        let mut logs = 0;
        let mut ores = 0;
        let ids = ids();
        for cx in -12..12 {
            for cz in -12..12 {
                let g = generate_chunk(99, cx * 8, cz * 8);
                biomes.extend(g.biomes.iter().copied());
                logs += g.features.values().filter(|&&b| b == ids.oak_log || b == ids.birch_log || b == ids.spruce_log).count();
                ores += (0..16).filter(|&y| ids.ores.iter().any(|o| block(99, &g, cx * 8, cz * 8, 3, y * 4, 3) == o.0 || block(99, &g, cx * 8, cz * 8, 3, y * 4, 3) == o.1)).count();
            }
        }
        assert!(biomes.len() >= 4, "{biomes:?}");
        assert!(logs > 0, "some trees");
        assert!(ores > 0, "some ores");
    }

    #[test]
    fn spawn_is_on_dry_land() {
        let (x, y, z) = spawn_point(1234);
        let (h, b) = column(1234, x.floor() as i32, z.floor() as i32);
        assert!(h > SEA_LEVEL && b != Biome::Ocean);
        assert!(y > h as f32 + 1.0);
    }
}

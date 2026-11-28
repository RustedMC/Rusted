// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Features (ores, trees, plants) placed after terrain, from each biome's vanilla feature list.
//! A chunk is decorated in a 3x3 Region running all nine chunks' features, then the centre is kept.

pub mod blocks;
mod decoration;
mod dungeon;
mod extra;
pub mod objects;
mod ores;
mod river;
mod trees;

use std::collections::HashMap;
use std::sync::OnceLock;

use super::Chunk;
pub use super::rng::{NukkitRandom, Xoroshiro};

/// The chunks a feature may touch: the chunk being decorated and its 8 neighbours.
pub struct Region {
    /// Centre chunk coordinates.
    pub cx: i32,
    pub cz: i32,
    pub seed: i64,
    chunks: Vec<Chunk>,
}

impl Region {
    /// `chunks` in order dz -1..=1 (outer), dx -1..=1 (inner).
    pub(super) fn new(cx: i32, cz: i32, seed: i64, chunks: Vec<Chunk>) -> Self {
        assert_eq!(chunks.len(), 9);
        Self { cx, cz, seed, chunks }
    }

    pub(super) fn into_centre(mut self) -> Chunk {
        self.chunks.swap_remove(4)
    }

    fn slot(&self, x: i32, z: i32) -> Option<usize> {
        let (dx, dz) = ((x >> 4) - self.cx, (z >> 4) - self.cz);
        ((-1..=1).contains(&dx) && (-1..=1).contains(&dz)).then(|| ((dz + 1) * 3 + dx + 1) as usize)
    }

    /// Block at world coordinates; air outside the region or the world height.
    pub fn get(&self, x: i32, y: i32, z: i32) -> i32 {
        match self.slot(x, z) {
            Some(i) => self.chunks[i].get(x & 15, y, z & 15),
            None => blocks::air(),
        }
    }

    /// Sets a block at world coordinates; ignored outside the region.
    pub fn set(&mut self, x: i32, y: i32, z: i32, id: i32) {
        if let Some(i) = self.slot(x, z) {
            self.chunks[i].set(x & 15, y, z & 15, id);
        }
    }

    /// Whether (x, z) is inside the region (writes there are kept by some chunk).
    pub fn contains(&self, x: i32, z: i32) -> bool {
        self.slot(x, z).is_some()
    }

    /// PNX `getHeightMap`: one above the highest non-air terrain block (features don't move it).
    pub fn height(&self, x: i32, z: i32) -> i32 {
        match self.slot(x, z) {
            Some(i) => self.chunks[i].heights[((x & 15) * 16 + (z & 15)) as usize],
            None => -64,
        }
    }

    /// Bedrock biome id at a position.
    pub fn biome(&self, x: i32, y: i32, z: i32) -> u16 {
        match self.slot(x, z) {
            Some(i) => self.chunks[i].biome(x & 15, y, z & 15),
            None => 0,
        }
    }

    fn chunk_at(&self, cx: i32, cz: i32) -> &Chunk {
        &self.chunks[((cz - self.cz + 1) * 3 + cx - self.cx + 1) as usize]
    }
}

/// What a feature run knows: the chunk it belongs to and the world seed.
pub struct Ctx {
    pub cx: i32,
    pub cz: i32,
    pub seed: i64,
    /// PNX's shared thread-local `Xoroshiro128` (each feature reseeds it).
    pub random: Xoroshiro,
}

pub type FeatureFn = fn(&mut Region, &mut Ctx);

/// Java `String.hashCode`.
pub fn java_hash(s: &str) -> i32 {
    s.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

/// PNX `Level.chunkHash`.
pub fn chunk_hash(x: i32, z: i32) -> i64 {
    ((x as i64) << 32) | (z as u32 as i64)
}

/// PNX `NukkitMath.randomRange(random, start, end)`.
pub fn random_range(r: &mut Xoroshiro, start: i32, end: i32) -> i32 {
    start + (r.next_int() % (end + 1 - start))
}

/// PNX `NukkitMath.randomRangeTriangle`.
pub fn random_range_triangle(r: &mut Xoroshiro, start: i32, end: i32) -> i32 {
    let diff = (end - start).abs();
    let half = diff / 2;
    let half2 = diff - half;
    start.min(end) + random_range(r, 0, half2) + random_range(r, 0, half)
}

/// Feature implementations by PNX feature identifier.
fn registry() -> &'static HashMap<&'static str, FeatureFn> {
    static R: OnceLock<HashMap<&'static str, FeatureFn>> = OnceLock::new();
    R.get_or_init(|| {
        let mut m: HashMap<&'static str, FeatureFn> = HashMap::new();
        ores::register(&mut m);
        river::register(&mut m);
        decoration::register(&mut m);
        trees::register(&mut m);
        extra::register(&mut m);
        m
    })
}

const PASSES: [&str; 11] = [
    "first_pass", "before_underground_pass", "underground_pass", "after_underground_pass", "before_surface_pass",
    "surface_pass", "after_surface_pass", "before_sky_pass", "sky_pass", "after_sky_pass", "final_pass",
];

/// Per biome: (pass index, identifier, feature name) in definition order.
fn biome_features() -> &'static HashMap<u16, Vec<(usize, String, String)>> {
    static F: OnceLock<HashMap<u16, Vec<(usize, String, String)>>> = OnceLock::new();
    F.get_or_init(|| {
        let mut out = HashMap::new();
        let batch = crate::vanilla::batches().nth(2).expect("biome definitions frame");
        let decoded = bedrock::network::codec::decode_packets_each::<crate::Protocol>(batch, None, None, usize::MAX, bedrock::network::codec::UnreadBytes::Warn);
        let Some(crate::Protocol::BiomeDefinitionListPacket(p)) = decoded.ok().and_then(|d| d.into_iter().next()).and_then(|(_, p)| p.ok()) else {
            return out;
        };
        let s = |i: u16| p.strings.get(i as usize).cloned().unwrap_or_default();
        for e in &p.biomes {
            let name = s(e.name_index);
            let Some(id) = super::biome_id(name.trim_start_matches("minecraft:")) else { continue };
            let Some(list) = e.definition.chunk_gen_data.as_ref().and_then(|g| g.consolidated_features.as_ref()) else { continue };
            let feats = list
                .entries
                .iter()
                .filter_map(|f| PASSES.iter().position(|p| *p == s(f.pass)).map(|pass| (pass, s(f.identifier), s(f.feature))))
                .collect();
            out.insert(id, feats);
        }
        out
    })
}

/// Runs every feature of chunk (fcx, fcz), pass by pass (PNX `NormalChunkFeatureStage`).
fn decorate_chunk(region: &mut Region, fcx: i32, fcz: i32) {
    let mut biomes: Vec<u16> = {
        let ch = region.chunk_at(fcx, fcz);
        let mut b = std::collections::BTreeSet::new();
        for x in 0..16 {
            for z in 0..16 {
                let h = ch.heights[(x * 16 + z) as usize];
                b.insert(ch.biome(x, h - 1, z));
                if h - 27 > -64 {
                    b.insert(ch.biome(x, h - 27, z));
                }
                if h - 128 > -64 {
                    b.insert(ch.biome(x, h - 128, z));
                }
            }
        }
        b.into_iter().collect()
    };
    biomes.dedup();
    let table = biome_features();
    let reg = registry();
    let mut ctx = Ctx { cx: fcx, cz: fcz, seed: region.seed, random: Xoroshiro::new(0) };
    // Dungeons close PNX's pregeneration carver (after the caves, which the terrain already
    // has), before every other feature.
    if biomes.iter().any(|&b| crate::overworld::carver::carver_biome(b)) {
        dungeon::monster_rooms(region, &mut ctx);
    }
    for pass in 0..PASSES.len() {
        let mut seen: Vec<&str> = Vec::new();
        for b in &biomes {
            for (p, ident, feature) in table.get(b).into_iter().flatten() {
                if *p != pass || seen.contains(&ident.as_str()) {
                    continue;
                }
                seen.push(ident);
                let f = reg.get(ident.as_str()).or_else(|| reg.get(feature.as_str()));
                if let Some(f) = f {
                    f(region, &mut ctx);
                }
            }
        }
    }
}

/// Decorates the centre chunk of `region` with the features of all nine chunks.
pub(super) fn decorate(mut region: Region) -> Chunk {
    for dz in -1..=1 {
        for dx in -1..=1 {
            let (fx, fz) = (region.cx + dx, region.cz + dz);
            decorate_chunk(&mut region, fx, fz);
        }
    }
    snow(&mut region);
    region.into_centre()
}

/// Vanilla biome temperatures, from the biome definitions.
fn temperatures() -> &'static HashMap<u16, f32> {
    static T: OnceLock<HashMap<u16, f32>> = OnceLock::new();
    T.get_or_init(|| {
        let batch = crate::vanilla::batches().nth(2).expect("biome definitions frame");
        let decoded = bedrock::network::codec::decode_packets_each::<crate::Protocol>(batch, None, None, usize::MAX, bedrock::network::codec::UnreadBytes::Warn);
        let Some(crate::Protocol::BiomeDefinitionListPacket(p)) = decoded.ok().and_then(|d| d.into_iter().next()).and_then(|(_, p)| p.ok()) else {
            return HashMap::new();
        };
        p.biomes
            .iter()
            .filter_map(|e| super::biome_id(p.strings[e.name_index as usize].trim_start_matches("minecraft:")).map(|id| (id, e.definition.temperature)))
            .collect()
    })
}

/// Snow on cold ground and ice on cold water (PNX's snow reconciliation, simplified).
// TODO: single snow layer, no BiomeSnowNoise or block light. Port it for exact snow depth.
fn snow(region: &mut Region) {
    let temps = temperatures();
    let (layer, ice) = (blocks::id("snow_layer"), blocks::id("ice"));
    let (bx, bz) = (region.cx << 4, region.cz << 4);
    for x in bx..bx + 16 {
        for z in bz..bz + 16 {
            // Highest non-air block after decoration (trees included).
            let mut y = 319;
            while y > -64 && blocks::is_air(region.get(x, y, z)) {
                y -= 1;
            }
            let Some(&t) = temps.get(&region.biome(x, y, z)) else { continue };
            let t = if y > 64 { t - (y - 64) as f32 * 0.05 / 40.0 } else { t };
            if t > 0.15 {
                continue;
            }
            let top = region.get(x, y, z);
            if blocks::is(top, "water") {
                region.set(x, y, z, ice);
            } else if (blocks::is_solid(top) || blocks::is_leaves(top)) && !blocks::is(top, "ice") && !blocks::is(top, "packed_ice") {
                region.set(x, y + 1, z, layer);
            }
        }
    }
}

/// Feature identifiers the biome definitions ask for that nothing implements yet.
#[cfg(test)]
pub fn missing() -> Vec<String> {
    let reg = registry();
    let mut m: Vec<String> = biome_features()
        .values()
        .flatten()
        .filter(|(_, i, f)| !reg.contains_key(i.as_str()) && !reg.contains_key(f.as_str()))
        .map(|(_, i, _)| i.clone())
        .collect();
    m.sort();
    m.dedup();
    m
}

#[cfg(test)]
mod list {
    #[test]
    #[ignore]
    fn print_missing() {
        for m in super::missing() {
            println!("MISSING {m}");
        }
    }
}



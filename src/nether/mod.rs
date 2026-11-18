// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
// Ported from PNX Nether (generator stages: BiomeMapStage, NetherTerrainStage, NetherPopulatorStage).
//! Nether generator. Terrain, then populators in a 3x3 region, then cached structures,
//! so the output doesn't depend on chunk order.

mod features;
mod fortress;
mod jigsaw;
mod noise;
mod rng;
mod structures;
mod templates;
mod terrain;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use parking_lot::Mutex;

use crate::overworld::feature::blocks as bl;

pub const MIN_Y: i32 = 0;
pub const MAX_Y: i32 = 127;
const HEIGHT: usize = 128;

pub const HELL: u16 = 8;
pub const SOULSAND_VALLEY: u16 = 178;
pub const CRIMSON_FOREST: u16 = 179;
pub const WARPED_FOREST: u16 = 180;
pub const BASALT_DELTAS: u16 = 181;

/// A generated nether chunk column. One biome per column since PNX's nether biomes ignore y.
#[derive(Clone)]
pub struct NetherChunk {
    blocks: Box<[i32]>,
    biomes: [u16; 256],
}

fn bi(lx: i32, y: i32, lz: i32) -> usize {
    ((lx * 16 + lz) as usize) * HEIGHT + y as usize
}

impl NetherChunk {
    fn new() -> Self {
        Self { blocks: vec![ids().air; 256 * HEIGHT].into_boxed_slice(), biomes: [HELL; 256] }
    }

    /// Block network id at local x/z and world y; air outside the height range.
    pub fn get(&self, lx: i32, y: i32, lz: i32) -> i32 {
        if !(MIN_Y..=MAX_Y).contains(&y) {
            return ids().air;
        }
        self.blocks[bi(lx, y, lz)]
    }

    /// Bedrock biome id.
    pub fn biome(&self, lx: i32, _y: i32, lz: i32) -> u16 {
        self.biomes[(lx * 16 + lz) as usize]
    }

    fn set(&mut self, lx: i32, y: i32, lz: i32, id: i32) {
        if (MIN_Y..=MAX_Y).contains(&y) {
            self.blocks[bi(lx, y, lz)] = id;
        }
    }
}

/// Block ids the generator uses, looked up once.
pub(crate) struct Ids {
    pub air: i32,
    pub bedrock: i32,
    pub netherrack: i32,
    pub lava: i32,
    pub flowing_lava: i32,
    pub basalt: i32,
    pub blackstone: i32,
    pub gravel: i32,
    pub soul_sand: i32,
    pub soul_soil: i32,
    pub warped_wart: i32,
    pub warped_nylium: i32,
    pub nether_wart_block: i32,
    pub crimson_nylium: i32,
    pub glowstone: i32,
    pub magma: i32,
    pub quartz_ore: i32,
    pub nether_gold_ore: i32,
    pub ancient_debris: i32,
    pub stone: i32,
    pub deepslate: i32,
    pub fire: i32,
    pub soul_fire: i32,
    pub crimson_fungus: i32,
    pub crimson_roots: i32,
    pub warped_fungus: i32,
    pub warped_roots: i32,
    pub weeping_vines: i32,
    pub twisting_vines: i32,
    pub crimson_stem: i32,
    pub warped_stem: i32,
    pub shroomlight: i32,
}

pub(crate) fn ids() -> &'static Ids {
    static I: OnceLock<Ids> = OnceLock::new();
    I.get_or_init(|| {
        let id = bl::id;
        Ids {
            air: bl::air(),
            bedrock: id("bedrock"),
            netherrack: id("netherrack"),
            lava: id("lava"),
            flowing_lava: id("flowing_lava"),
            basalt: id("basalt"),
            blackstone: id("blackstone"),
            gravel: id("gravel"),
            soul_sand: id("soul_sand"),
            soul_soil: id("soul_soil"),
            warped_wart: id("warped_wart_block"),
            warped_nylium: id("warped_nylium"),
            nether_wart_block: id("nether_wart_block"),
            crimson_nylium: id("crimson_nylium"),
            glowstone: id("glowstone"),
            magma: id("magma"),
            quartz_ore: id("quartz_ore"),
            nether_gold_ore: id("nether_gold_ore"),
            ancient_debris: id("ancient_debris"),
            stone: id("stone"),
            deepslate: id("deepslate"),
            fire: id("fire"),
            soul_fire: id("soul_fire"),
            crimson_fungus: id("crimson_fungus"),
            crimson_roots: id("crimson_roots"),
            warped_fungus: id("warped_fungus"),
            warped_roots: id("warped_roots"),
            weeping_vines: id("weeping_vines"),
            twisting_vines: id("twisting_vines"),
            crimson_stem: id("crimson_stem"),
            warped_stem: id("warped_stem"),
            shroomlight: id("shroomlight"),
        }
    })
}

const TERRAIN_CACHE_LIMIT: usize = 2048;

/// Nether generator for one seed. Caches sit behind mutexes so it's Send + Sync.
pub struct Nether {
    seed: i64,
    terrain: terrain::Terrain,
    /// Undecorated terrain, shared by neighbouring chunks' decoration and structure builds.
    terrain_cache: Mutex<HashMap<(i32, i32), Arc<NetherChunk>>>,
    /// Built structures, shared by every chunk they reach.
    structures: structures::Cache,
}

impl Nether {
    pub fn new(seed: u64) -> Self {
        let seed = seed as i64;
        Self { seed, terrain: terrain::Terrain::new(seed), terrain_cache: Default::default(), structures: Default::default() }
    }

    pub fn generate(&self, cx: i32, cz: i32) -> NetherChunk {
        let mut chunks = Vec::with_capacity(9);
        for dz in -1..=1 {
            for dx in -1..=1 {
                chunks.push((*self.terrain_chunk(cx + dx, cz + dz)).clone());
            }
        }
        let mut ch = features::decorate(self, features::Region::new(cx, cz, chunks));
        structures::apply(self, &mut ch, cx, cz);
        ch
    }

    fn terrain_chunk(&self, cx: i32, cz: i32) -> Arc<NetherChunk> {
        if let Some(c) = self.terrain_cache.lock().get(&(cx, cz)) {
            return c.clone();
        }
        let c = Arc::new(self.terrain.generate(self.seed, cx, cz));
        let mut m = self.terrain_cache.lock();
        if m.len() >= TERRAIN_CACHE_LIMIT {
            // TODO: just drops half at the cap, maybe LRU later
            let keys: Vec<_> = m.keys().copied().step_by(2).collect();
            for k in keys {
                m.remove(&k);
            }
        }
        m.entry((cx, cz)).or_insert(c).clone()
    }
}

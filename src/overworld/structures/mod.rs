// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Structures from PNX's populators. Each type has a cheap seed-only `hint`, gets built once
//! into a cached `StructWorld`, and every chunk copies in its own blocks.

mod big;
mod jigsaw;
mod placement;
mod small;
mod templates;
mod underground;

pub use placement::{Placement, Spread};

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use parking_lot::Mutex;

use super::Overworld;
use super::density::FxMap;
use super::feature::objects::World;
pub use super::rng::{NukkitRandom, Xoroshiro};

/// Where a structure is being built: undecorated terrain plus the structure's own writes.
pub struct StructWorld<'a> {
    ow: &'a Overworld,
    writes: FxMap<i64, i32>,
    pub seed: i64,
}

fn key(x: i32, y: i32, z: i32) -> i64 {
    (((x as i64) & 0x3FF_FFFF) << 38) | (((z as i64) & 0x3FF_FFFF) << 12) | ((y as i64 + 64) & 0xFFF)
}

fn unkey(k: i64) -> (i32, i32, i32) {
    let x = ((k >> 38) << 38 >> 38) as i32;
    let z = ((k << 26) >> 38) as i32;
    let y = (k & 0xFFF) as i32 - 64;
    (x, y, z)
}

impl StructWorld<'_> {
    /// Bedrock biome id at a position (from the terrain chunk's biome map).
    pub fn biome(&self, x: i32, y: i32, z: i32) -> u16 {
        self.ow.terrain_chunk(x >> 4, z >> 4).biome(x & 15, y, z & 15)
    }

    /// PNX `chunk.getBiomeId(7, SEA_LEVEL, 7)` for chunk (cx, cz).
    pub fn chunk_biome(&self, cx: i32, cz: i32) -> u16 {
        self.biome((cx << 4) + 7, super::aquifer::SEA_LEVEL, (cz << 4) + 7)
    }
}

impl World for StructWorld<'_> {
    fn get(&self, x: i32, y: i32, z: i32) -> i32 {
        if let Some(v) = self.writes.get(&key(x, y, z)) {
            return *v;
        }
        self.ow.terrain_chunk(x >> 4, z >> 4).get(x & 15, y, z & 15)
    }

    fn set(&mut self, x: i32, y: i32, z: i32, id: i32) {
        if (-64..=319).contains(&y) {
            self.writes.insert(key(x, y, z), id);
        }
    }

    fn height(&self, x: i32, z: i32) -> i32 {
        self.ow.terrain_chunk(x >> 4, z >> 4).heights[((x & 15) * 16 + (z & 15)) as usize]
    }

    fn seed(&self) -> i64 {
        self.seed
    }
}

pub type BuildFn = fn(&mut StructWorld, &mut Xoroshiro, i32, i32);

pub struct StructureType {
    #[allow(dead_code)] // handy when debugging placement
    pub name: &'static str,
    /// Furthest a structure reaches from its start chunk, in chunks.
    pub radius: i32,
    /// Cheap seed-only check. Must be true wherever `build` might place something.
    pub hint: fn(i64, i32, i32) -> bool,
    pub build: BuildFn,
}

fn types() -> &'static Vec<StructureType> {
    static T: OnceLock<Vec<StructureType>> = OnceLock::new();
    T.get_or_init(|| {
        let mut v = Vec::new();
        small::register(&mut v);
        underground::register(&mut v);
        big::register(&mut v);
        templates::register(&mut v);
        jigsaw::register(&mut v);
        v
    })
}

/// A built structure's blocks, grouped by chunk: (local x, y, local z, id).
type Built = HashMap<(i32, i32), Vec<(u8, i16, u8, i32)>>;

#[derive(Default)]
pub struct Cache(Mutex<HashMap<(usize, i32, i32), Arc<Built>>>);

const CACHE_LIMIT: usize = 4096;

fn build(ow: &Overworld, t: &StructureType, cx: i32, cz: i32) -> Built {
    let mut w = StructWorld { ow, writes: FxMap::default(), seed: ow.seed };
    let mut r = Xoroshiro::new(0);
    (t.build)(&mut w, &mut r, cx, cz);
    let mut out: Built = HashMap::new();
    for (k, id) in w.writes {
        let (x, y, z) = unkey(k);
        crate::loot::generated((x, y, z), id, t.name);
        out.entry((x >> 4, z >> 4)).or_default().push(((x & 15) as u8, y as i16, (z & 15) as u8, id));
    }
    out
}

/// Copies every structure block that falls in chunk (cx, cz) into it.
pub(super) fn apply(ow: &Overworld, chunk: &mut super::Chunk, cx: i32, cz: i32) {
    for (ti, t) in types().iter().enumerate() {
        for sx in cx - t.radius..=cx + t.radius {
            for sz in cz - t.radius..=cz + t.radius {
                if !(t.hint)(ow.seed, sx, sz) {
                    continue;
                }
                let cached = ow.structures.0.lock().get(&(ti, sx, sz)).cloned();
                let built = match cached {
                    Some(b) => b,
                    None => {
                        let b = Arc::new(build(ow, t, sx, sz)); // outside the lock
                        let mut m = ow.structures.0.lock();
                        if m.len() >= CACHE_LIMIT {
                            m.clear(); // TODO: LRU if rebuilding shows up in profiles
                        }
                        m.entry((ti, sx, sz)).or_insert(b).clone()
                    }
                };
                if let Some(blocks) = built.get(&(cx, cz)) {
                    for &(x, y, z, id) in blocks {
                        chunk.set(x as i32, y as i32, z as i32, id);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip() {
        for p in [(0, 0, 0), (-1, -64, -1), (123_456, 319, -987_654), (-30_000_000, 70, 29_999_999)] {
            assert_eq!(unkey(key(p.0, p.1, p.2)), p);
        }
    }
}

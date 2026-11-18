// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Where structures may start (PNX `StructurePlacement` and `StructureRandomSpreadPlacement`).

use super::super::rng::{BedrockRandom, Xoroshiro};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Spread {
    /// PNX `StructurePlacement` (Xoroshiro, region = maxDistance chunks).
    Legacy,
    /// `StructureRandomSpreadPlacement` LINEAR (Bedrock random).
    Linear,
    /// `StructureRandomSpreadPlacement` TRIANGULAR.
    Triangular,
}

/// Placement settings: `salt`, `min_distance` (separation), `max_distance` (spacing).
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub salt: i64,
    pub min_distance: i32,
    pub max_distance: i32,
    pub spread: Spread,
}

fn chunk_hash(x: i32, z: i32) -> i64 {
    ((x as i64) << 32) | (z as u32 as i64)
}

/// PNX `StructurePlacement.regionCoord` (rounds toward negative infinity, off by one as in PNX).
fn region_coord(chunk: i32, spacing: i32) -> i32 {
    if chunk < 0 { (chunk - spacing - 1) / spacing } else { chunk / spacing }
}

impl Placement {
    pub const fn new(salt: i64, min_distance: i32, max_distance: i32, spread: Spread) -> Self {
        Self { salt, min_distance, max_distance, spread }
    }

    /// Whether chunk (cx, cz) is this placement's candidate chunk (biome not checked; PNX
    /// checks the biome separately with `isValidBiome`).
    pub fn is_candidate(&self, seed: i64, cx: i32, cz: i32) -> bool {
        let spacing = self.max_distance;
        match self.spread {
            Spread::Legacy => {
                let (rx, rz) = (region_coord(cx, spacing), region_coord(cz, spacing));
                let mut r = Xoroshiro::new((seed ^ self.salt).wrapping_add(chunk_hash(rx, rz)));
                rx * spacing + r.next_bounded_int(spacing - self.min_distance) == cx
                    && rz * spacing + r.next_bounded_int(spacing - self.min_distance) == cz
            }
            Spread::Linear | Spread::Triangular => {
                let (rx, rz) = (cx.div_euclid(spacing), cz.div_euclid(spacing));
                let range = spacing - self.min_distance;
                let mut r = BedrockRandom::new(self.region_seed(seed, rx, rz) as i64);
                let ox = self.offset(&mut r, range);
                let oz = self.offset(&mut r, range);
                rx * spacing + ox == cx && rz * spacing + oz == cz
            }
        }
    }

    fn region_seed(&self, seed: i64, rx: i32, rz: i32) -> i32 {
        rx.wrapping_mul(0x9939_F508u32 as i32)
            .wrapping_add(rz.wrapping_mul(0xF156_5BD5u32 as i32))
            .wrapping_add(seed as i32)
            .wrapping_add(self.salt as i32)
    }

    fn offset(&self, r: &mut BedrockRandom, range: i32) -> i32 {
        match self.spread {
            Spread::Triangular => (r.next_int_below(range) + r.next_int_below(range)) / 2,
            _ => r.next_int_below(range),
        }
    }

    /// PNX `createPostSpreadRandom`: the region random after the two placement draws.
    pub fn post_spread_random(&self, seed: i64, cx: i32, cz: i32) -> BedrockRandom {
        let spacing = self.max_distance;
        let (rx, rz) = (cx.div_euclid(spacing), cz.div_euclid(spacing));
        let mut r = BedrockRandom::new(self.region_seed(seed, rx, rz) as i64);
        let range = spacing - self.min_distance;
        self.offset(&mut r, range);
        self.offset(&mut r, range);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_candidate_per_region() {
        for spread in [Spread::Legacy, Spread::Linear, Spread::Triangular] {
            let p = Placement::new(14_357_617, 8, 32, spread);
            let n = (0..64).flat_map(|x| (0..64).map(move |z| (x, z))).filter(|&(x, z)| p.is_candidate(42, x, z)).count();
            assert!((1..=9).contains(&n), "{spread:?}: {n} candidates in 4 regions");
        }
    }
}

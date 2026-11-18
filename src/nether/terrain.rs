// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
// Ported from PNX NetherTerrainStage, DensityNether, DensityBase3dNoise.nether, NetherBiomePicker,
// NetherObjectHolder, BiomeMapStage (non-overworld branch).
//! Nether terrain: 3D noise between floor and roof gradients, lava sea up to y 31,
//! biome surfaces and bedrock.

use super::noise::{NormalNoise, OctaveList};
use super::rng::NukkitRandom;
use super::{ids, NetherChunk, BASALT_DELTAS, CRIMSON_FOREST, HELL, SOULSAND_VALLEY, WARPED_FOREST};
use crate::overworld::feature::chunk_hash;

pub const LAVA_LEVEL: i32 = 31;
/// PNX `NormalTerrainStage.SEA_LEVEL`: the height the biome picker samples.
const SEA_LEVEL: i32 = 63;

/// PNX `OldBlendedNoise` with the nether parameters (0.25, 0.375, 80, 60, 8).
struct Blended {
    min_limit: OctaveList,
    max_limit: OctaveList,
    main: OctaveList,
}

impl Blended {
    fn eval(&self, x: i32, y: i32, z: i32) -> f64 {
        const BASE: f64 = 684.412;
        let sxz = BASE * 0.25;
        let sy = BASE * 0.375;
        let (mxz, my) = (sxz / 80.0, sy / 60.0);
        let smear = sy * 8.0;
        let (x, y, z) = (x as f64, y as f64, z as f64);
        let mut main = 0.0;
        let mut f = 1.0;
        for s in &self.main.samplers {
            main += s.sample(x * mxz * f, y * my * f, z * mxz * f, smear * f, y * my * f) / f;
            f /= 2.0;
        }
        let blend = (main / 10.0 + 1.0).clamp(0.0, 2.0) * 0.5;
        let (only_max, only_min) = (blend >= 1.0, blend <= 0.0);
        let (mut lo, mut hi) = (0.0, 0.0);
        f = 1.0;
        let n = self.min_limit.samplers.len().min(self.max_limit.samplers.len());
        for i in 0..n {
            let (px, py, pz, ps) = (x * sxz * f, y * sy * f, z * sxz * f, smear * f);
            if !only_max {
                lo += self.min_limit.samplers[i].sample(px, py, pz, ps, py) / f;
            }
            if !only_min {
                hi += self.max_limit.samplers[i].sample(px, py, pz, ps, py) / f;
            }
            f /= 2.0;
        }
        let (lo, hi) = (lo / 512.0, hi / 512.0);
        (lo + blend * (hi - lo)) / 128.0
    }
}

fn clamped_map(v: f64, from: f64, to: f64, fv: f64, tv: f64) -> f64 {
    let t = ((v - from) / (to - from)).clamp(0.0, 1.0);
    fv + t * (tv - fv)
}

/// The nether biome picker (PNX `NetherBiomePicker`).
pub struct Picker {
    temperature: NormalNoise,
    humidity: NormalNoise,
}

impl Picker {
    pub fn new(seed: i64) -> Self {
        let mut r = NukkitRandom::new(seed);
        let temperature = NormalNoise::new(r.fork(), -10, &[1.5, 0.0, 1.0, 0.0, 0.0, 0.0]);
        let humidity = NormalNoise::new(r.fork(), -8, &[1.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
        Self { temperature, humidity }
    }

    pub fn pick(&self, x: i32, y: i32, z: i32) -> u16 {
        // (biome, temperature, humidity, z) in PNX `POINTS`; z is always compared against 0.
        const POINTS: [(u16, f32, f32, f32); 5] = [
            (BASALT_DELTAS, -0.5, 0.0, 0.175),
            (CRIMSON_FOREST, 0.4, 0.0, 0.0),
            (HELL, 0.0, 0.0, 0.0),
            (SOULSAND_VALLEY, 0.0, -0.5, 0.0),
            (WARPED_FOREST, 0.0, 0.5, 0.375),
        ];
        let t = self.temperature.value(x as f64, y as f64, z as f64) as f64;
        let h = self.humidity.value(x as f64, y as f64, z as f64) as f64;
        // TODO: PNX iterates a JVM-salted Map.of and keeps the first strict min; only exact ties differ.
        let mut best = (f32::MAX as f64, HELL);
        for (b, pt, ph, pz) in POINTS {
            let (dx, dy, dz) = (t - pt as f64, h - ph as f64, -(pz as f64));
            let d = dx * dx + dy * dy + dz * dz;
            if d < best.0 {
                best = (d, b);
            }
        }
        best.1
    }

    /// The biome BiomeMapStage stores for a column (sampled at sea level).
    pub fn column(&self, x: i32, z: i32) -> u16 {
        self.pick(x, SEA_LEVEL, z)
    }
}

/// PNX `NetherObjectHolder` (terrain and basalt-delta noises) plus the biome picker.
pub struct Terrain {
    pub picker: Picker,
    patch: NormalNoise,
    soulsand: NormalNoise,
    state: NormalNoise,
    netherwart: NormalNoise,
    base3d: Blended,
    /// BasaltDeltaHolder.
    pub delta_surface: NormalNoise,
    pub delta_surface_sec: NormalNoise,
}

impl Terrain {
    pub fn new(seed: i64) -> Self {
        // Every holder noise is built from `identical()`: a fresh generator with the level seed.
        let id = || NukkitRandom::new(seed);
        let patch = NormalNoise::new(id(), -5, &[1.0, 0.0, 0.0, 0.0, 0.0, 0.013_333_334]);
        let soulsand = NormalNoise::new(id(), -8, &[1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.013_333_334]);
        let state = NormalNoise::new(id(), -4, &[1.0]);
        let netherwart = NormalNoise::new(id(), -3, &[1.0, 0.0, 0.0, 0.9]);
        let mut r = id();
        let base3d = Blended { min_limit: OctaveList::new(&mut r.fork(), -15), max_limit: OctaveList::new(&mut r.fork(), -15), main: OctaveList::new(&mut r.fork(), -7) };
        let delta_surface = NormalNoise::new(id(), -6, &[1.0, 1.0, 1.0]);
        let delta_surface_sec = NormalNoise::new(id(), -6, &[1.0, 0.0, 1.0, 1.0]);
        Self { picker: Picker::new(seed), patch, soulsand, state, netherwart, base3d, delta_surface, delta_surface_sec }
    }

    /// `DensityNether.finalDensity` before interpolation. Only the sign matters here.
    fn inner(&self, x: i32, y: i32, z: i32) -> f64 {
        let yf = y as f64;
        let d = -0.9375 + self.base3d.eval(x, y, z);
        let d = clamped_map(yf, 104.0, 128.0, 1.0, 0.0) * d;
        let d = 0.9375 + d;
        let d = -2.5 + d;
        let d = clamped_map(yf, -8.0, 24.0, 0.0, 1.0) * d;
        2.5 + d
    }

    /// Terrain stage plus biome map for chunk (cx, cz).
    pub fn generate(&self, seed: i64, cx: i32, cz: i32) -> NetherChunk {
        let ids = ids();
        let mut ch = NetherChunk::new();
        let (bx, bz) = (cx << 4, cz << 4);
        for x in 0..16 {
            for z in 0..16 {
                ch.biomes[(x * 16 + z) as usize] = self.picker.column(bx + x, bz + z);
            }
        }
        // Corner densities: 5 x 17 x 5 (x, y, z), cells of 4 x 8 x 4.
        let mut corners = [0f64; 5 * 17 * 5];
        let ci = |i: usize, j: usize, k: usize| (i * 17 + j) * 5 + k;
        for i in 0..5 {
            for j in 0..17 {
                for k in 0..5 {
                    corners[ci(i, j, k)] = self.inner(bx + 4 * i as i32, 8 * j as i32, bz + 4 * k as i32);
                }
            }
        }
        for cxi in 0..4 {
            for czi in 0..4 {
                for cyi in 0..16 {
                    let d000 = corners[ci(cxi, cyi, czi)];
                    let d100 = corners[ci(cxi + 1, cyi, czi)];
                    let d010 = corners[ci(cxi, cyi + 1, czi)];
                    let d110 = corners[ci(cxi + 1, cyi + 1, czi)];
                    let d001 = corners[ci(cxi, cyi, czi + 1)];
                    let d101 = corners[ci(cxi + 1, cyi, czi + 1)];
                    let d011 = corners[ci(cxi, cyi + 1, czi + 1)];
                    let d111 = corners[ci(cxi + 1, cyi + 1, czi + 1)];
                    let c000 = d000;
                    let c100 = d100 - d000;
                    let c010 = d010 - d000;
                    let c001 = d001 - d000;
                    let c110 = d110 - d100 - d010 + d000;
                    let c101 = d101 - d100 - d001 + d000;
                    let c011 = d011 - d010 - d001 + d000;
                    let c111 = d111 - d110 - d101 - d011 + d100 + d010 + d001 - d000;
                    for ly in 0..8 {
                        let y = cyi as i32 * 8 + ly;
                        if !(1..=126).contains(&y) {
                            continue;
                        }
                        let ya = ly as f64 * (1.0 / 8.0);
                        for lz in 0..4 {
                            let za = lz as f64 * 0.25;
                            let yz = ya * za;
                            let zt = za * (c001 + ya * c011);
                            for lx in 0..4 {
                                let xa = lx as f64 * 0.25;
                                let v = c000 + xa * (c100 + ya * c110 + za * c101 + yz * c111) + ya * c010 + zt;
                                let (x, z) = (cxi as i32 * 4 + lx, czi as i32 * 4 + lz);
                                if v > 0.0 {
                                    ch.set(x, y, z, ids.netherrack);
                                } else if y <= LAVA_LEVEL {
                                    ch.set(x, y, z, ids.lava);
                                }
                            }
                        }
                    }
                }
            }
        }
        self.surface(&mut ch, seed, cx, cz);
        ch
    }

    fn surface(&self, ch: &mut NetherChunk, seed: i64, cx: i32, cz: i32) {
        let i = ids();
        let mut random = NukkitRandom::new(seed ^ chunk_hash(cx, cz));
        let (bx, bz) = (cx << 4, cz << 4);
        let air = i.air;
        let is_top = |ch: &NetherChunk, x: i32, y: i32, z: i32| (0..5).map(|k| y + k).any(|yy| (1..=127).contains(&yy) && ch.get(x, yy, z) == air);
        let is_ceil = |ch: &NetherChunk, x: i32, y: i32, z: i32| (0..5).map(|k| y - k).any(|yy| (1..=127).contains(&yy) && ch.get(x, yy, z) == air);
        for x in 0..16 {
            for z in 0..16 {
                let (nx, nz) = ((x + bx) as f64, (z + bz) as f64);
                let biome = ch.biomes[(x * 16 + z) as usize];
                ch.set(x, 0, z, i.bedrock);
                ch.set(x, 126, z, i.netherrack);
                ch.set(x, 127, z, i.bedrock);
                for y in 1..127 {
                    let yf = y as f64;
                    if ch.get(x, y, z) != i.netherrack {
                        continue;
                    }
                    match biome {
                        BASALT_DELTAS => {
                            if is_ceil(ch, x, y, z) {
                                ch.set(x, y, z, i.basalt);
                                continue;
                            } else if ch.get(x, y + 1, z) == air
                                && (self.state.value(nx, yf, nz) >= 0.0 || (30..=35).contains(&y) && self.patch.value(nx, yf, nz) >= -0.012)
                            {
                                ch.set(x, y, z, i.gravel);
                                continue;
                            }
                            if is_top(ch, x, y, z) || is_ceil(ch, x, y, z) {
                                ch.set(x, y, z, i.blackstone);
                            }
                        }
                        SOULSAND_VALLEY => {
                            if is_ceil(ch, x, y, z) {
                                ch.set(x, y, z, if self.state.value(nx, yf, nz) >= 0.0 { i.soul_sand } else { i.soul_soil });
                            } else if is_top(ch, x, y, z) {
                                if self.state.value(nx, yf, nz) >= 0.0 || ((30..=35).contains(&y) && self.patch.value(nx, yf, nz) >= -0.012) {
                                    ch.set(x, y, z, i.soul_sand);
                                } else {
                                    ch.set(x, y, z, i.soul_soil);
                                }
                            }
                        }
                        WARPED_FOREST => {
                            if ch.get(x, y + 1, z) == air && y > 31 && self.state.value(nx, yf, nz) <= 0.0 {
                                ch.set(x, y, z, if self.netherwart.value(nx, yf, nz) >= 1.17 { i.warped_wart } else { i.warped_nylium });
                            }
                        }
                        CRIMSON_FOREST => {
                            if ch.get(x, y + 1, z) == air && y > 31 && self.state.value(nx, yf, nz) <= 0.54 {
                                ch.set(x, y, z, if self.netherwart.value(nx, yf, nz) >= 1.17 { i.nether_wart_block } else { i.crimson_nylium });
                            }
                        }
                        HELL
                            if is_top(ch, x, y, z) => {
                                if y > 31 && y < 35 && self.soulsand.value(nx, yf, nz) >= -0.012 {
                                    ch.set(x, y, z, i.gravel);
                                    continue;
                                }
                                if (30..=35).contains(&y) && self.soulsand.value(nx, yf, nz) >= -0.012 {
                                    ch.set(x, y, z, i.soul_sand);
                                }
                            }
                        _ => {}
                    }
                }
                // bound re-rolls every iteration, same as PNX
                let mut k = 0;
                while k < random.next_bounded_int(6) {
                    if ch.get(x, 126 - k, z) == i.netherrack && ch.get(x, 125 - k, z) != air {
                        ch.set(x, 126 - k, z, i.bedrock);
                    }
                    k += 1;
                }
            }
        }
    }
}

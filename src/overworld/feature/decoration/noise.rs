// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! The feature noises PNX keeps in `NormalObjectHolder.FeatureHolder`: `SimplexF` (Nukkit float
//! simplex, `noise/f`) and the vanilla-style `SimplexNoise` (`noise/minecraft/simplex`).

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use parking_lot::Mutex;

use super::super::{java_hash, NukkitRandom, Xoroshiro};

/// PNX `SimplexF` (with its `PerlinF`/`NoiseF` bases), all `f32` like the Java.
pub(crate) struct SimplexF {
    perm: [i32; 512],
    off: [f32; 3],
    octaves: f32,
    persistence: f32,
    expansion: f32,
}

const GRAD3: [[i32; 3]; 12] = [[1, 1, 0], [-1, 1, 0], [1, -1, 0], [-1, -1, 0], [1, 0, 1], [-1, 0, 1], [1, 0, -1], [-1, 0, -1], [0, 1, 1], [0, -1, 1], [0, 1, -1], [0, -1, -1]];

impl SimplexF {
    pub(crate) fn new(r: &mut Xoroshiro, octaves: f32, persistence: f32, expansion: f32) -> Self {
        let off = [r.next_float() * 256.0, r.next_float() * 256.0, r.next_float() * 256.0];
        let mut perm = [0i32; 512];
        for p in perm.iter_mut().take(256) {
            *p = r.next_bounded_int(255);
        }
        for i in 0..256 {
            let pos = (r.next_bounded_int(255 - i as i32) + i as i32) as usize;
            perm.swap(i, pos);
            perm[i + 256] = perm[i];
        }
        let _offset_w = r.next_float() * 256.0; // SimplexF's own field, consumed for the RNG order
        Self { perm, off, octaves, persistence, expansion }
    }

    fn p(&self, i: i32) -> i32 {
        self.perm[i as usize]
    }

    fn noise3(&self, x: f32, y: f32, z: f32) -> f32 {
        const F3: f32 = 1.0 / 3.0;
        const G3: f32 = 1.0 / 6.0;
        let (x, y, z) = (x + self.off[0], y + self.off[1], z + self.off[2]);
        let s = (x + y + z) * F3;
        let (i, j, k) = ((x + s) as i32, (y + s) as i32, (z + s) as i32);
        let t = i.wrapping_add(j).wrapping_add(k) as f32 * G3;
        let (x0, y0, z0) = (x - (i as f32 - t), y - (j as f32 - t), z - (k as f32 - t));
        let (i1, j1, k1, i2, j2, k2) = if x0 >= y0 {
            if y0 >= z0 {
                (1, 0, 0, 1, 1, 0)
            } else if x0 >= z0 {
                (1, 0, 0, 1, 0, 1)
            } else {
                (0, 0, 1, 1, 0, 1)
            }
        } else if y0 < z0 {
            (0, 0, 1, 0, 1, 1)
        } else if x0 < z0 {
            (0, 1, 0, 0, 1, 1)
        } else {
            (0, 1, 0, 1, 1, 0)
        };
        let c = [
            (x0, y0, z0, 0, 0, 0),
            (x0 - i1 as f32 + G3, y0 - j1 as f32 + G3, z0 - k1 as f32 + G3, i1, j1, k1),
            (x0 - i2 as f32 + 2.0 * G3, y0 - j2 as f32 + 2.0 * G3, z0 - k2 as f32 + 2.0 * G3, i2, j2, k2),
            (x0 - 1.0 + 3.0 * G3, y0 - 1.0 + 3.0 * G3, z0 - 1.0 + 3.0 * G3, 1, 1, 1),
        ];
        let (ii, jj, kk) = (i & 255, j & 255, k & 255);
        let mut n = 0f32;
        for (cx, cy, cz, a, b, d) in c {
            let t = 0.6 - cx * cx - cy * cy - cz * cz;
            if t > 0.0 {
                let g = GRAD3[(self.p(ii + a + self.p(jj + b + self.p(kk + d))) % 12) as usize];
                n += t * t * t * t * (g[0] as f32 * cx + g[1] as f32 * cy + g[2] as f32 * cz);
            }
        }
        32.0 * n
    }

    fn noise2(&self, x: f32, y: f32) -> f32 {
        let sqrt3 = 3f64.sqrt() as f32;
        let f2 = 0.5 * (sqrt3 - 1.0);
        let g2 = (3.0 - sqrt3) / 6.0;
        let g22 = g2 * 2.0 - 1.0;
        let (x, y) = (x + self.off[0], y + self.off[1]);
        let s = (x + y) * f2;
        let (i, j) = ((x + s) as i32, (y + s) as i32);
        let t = i.wrapping_add(j) as f32 * g2; // Java int add wraps
        let (x0, y0) = (x - (i as f32 - t), y - (j as f32 - t));
        let (i1, j1) = if x0 > y0 { (1, 0) } else { (0, 1) };
        let c = [(x0, y0, 0, 0), (x0 - i1 as f32 + g2, y0 - j1 as f32 + g2, i1, j1), (x0 + g22, y0 + g22, 1, 1)];
        let (ii, jj) = (i & 255, j & 255);
        let mut n = 0f32;
        for (cx, cy, a, b) in c {
            let t = 0.5 - cx * cx - cy * cy;
            if t > 0.0 {
                let g = GRAD3[(self.p(ii + a + self.p(jj + b)) % 12) as usize];
                n += t * t * t * t * (g[0] as f32 * cx + g[1] as f32 * cy);
            }
        }
        70.0 * n
    }

    /// `NoiseF.noise2D(x, z, normalized)`.
    pub(crate) fn noise_2d(&self, x: f32, z: f32, normalized: bool) -> f32 {
        let (mut result, mut amp, mut freq, mut max) = (0f32, 1f32, 1f32, 0f32);
        let (x, z) = (x * self.expansion, z * self.expansion);
        let mut i = 0;
        while (i as f32) < self.octaves {
            result += self.noise2(x * freq, z * freq) * amp;
            max += amp;
            freq *= 2.0;
            amp *= self.persistence;
            i += 1;
        }
        if normalized { result / max } else { result }
    }

    /// `NoiseF.noise3D(x, y, z, normalized)`.
    pub(crate) fn noise_3d(&self, x: f32, y: f32, z: f32, normalized: bool) -> f32 {
        let (mut result, mut amp, mut freq, mut max) = (0f32, 1f32, 1f32, 0f32);
        let (x, y, z) = (x * self.expansion, y * self.expansion, z * self.expansion);
        let mut i = 0;
        while (i as f32) < self.octaves {
            result += self.noise3(x * freq, y * freq, z * freq) * amp;
            max += amp;
            freq *= 2.0;
            amp *= self.persistence;
            i += 1;
        }
        if normalized { result / max } else { result }
    }
}

/// PNX `SimplexNoiseSampler` (3D only; features don't sample 2D).
struct Sampler {
    perm: [u8; 256],
}

fn grad(hash: i32, x: f64, y: f64, z: f64) -> f64 {
    match hash & 15 {
        0 => x + y,
        1 => -x + y,
        2 => x - y,
        3 => -x - y,
        4 => x + z,
        5 => -x + z,
        6 => x - z,
        7 => -x - z,
        8 => y + z,
        9 | 13 => -y + z,
        10 => y - z,
        11 | 15 => -y - z,
        12 => y + x,
        _ => y - x,
    }
}

fn floor(d: f64) -> i32 {
    let i = d as i32;
    if d < i as f64 { i - 1 } else { i }
}

impl Sampler {
    fn new(r: &mut NukkitRandom) -> Self {
        for _ in 0..3 {
            r.next_double(); // origin x/y/z (unused by 3D sampling)
        }
        let mut perm = [0u8; 256];
        for (j, p) in perm.iter_mut().enumerate() {
            *p = j as u8;
        }
        for index in 0..256 {
            let ri = (r.next_bounded_int(255 - index as i32) + index as i32) as usize;
            perm.swap(index, ri);
        }
        Self { perm }
    }

    fn lookup(&self, h: i32) -> i32 {
        self.perm[(h & 0xff) as usize] as i32
    }

    fn corner(hash: i32, x: f64, y: f64, z: f64, max: f64) -> f64 {
        let c = max - x * x - y * y - z * z;
        if c < 0.0 { 0.0 } else { c * c * c * c * grad(hash, x, y, z) }
    }

    fn sample_3d(&self, x: f64, y: f64, z: f64) -> f64 {
        const F3: f64 = 0.3333333333333333;
        const G3: f64 = 0.16666666666666666;
        let s = (x + y + z) * F3;
        let (i, j, k) = (floor(x + s), floor(y + s), floor(z + s));
        let t = i.wrapping_add(j).wrapping_add(k) as f64 * G3;
        let (x0, y0, z0) = (x - (i as f64 - t), y - (j as f64 - t), z - (k as f64 - t));
        let (i1, j1, k1, i2, j2, k2) = if x0 >= y0 {
            if y0 >= z0 {
                (1, 0, 0, 1, 1, 0)
            } else if x0 >= z0 {
                (1, 0, 0, 1, 0, 1)
            } else {
                (0, 0, 1, 1, 0, 1)
            }
        } else if y0 < z0 {
            (0, 0, 1, 0, 1, 1)
        } else if x0 < z0 {
            (0, 1, 0, 0, 1, 1)
        } else {
            (0, 1, 0, 1, 1, 0)
        };
        let (ii, jj, kk) = (i & 255, j & 255, k & 255);
        let h = |a: i32, b: i32, c: i32| self.lookup(ii + a + self.lookup(jj + b + self.lookup(kk + c))) % 12;
        let t0 = Self::corner(h(0, 0, 0), x0, y0, z0, 0.6);
        let t1 = Self::corner(h(i1, j1, k1), x0 - i1 as f64 + G3, y0 - j1 as f64 + G3, z0 - k1 as f64 + G3, 0.6);
        let t2 = Self::corner(h(i2, j2, k2), x0 - i2 as f64 + F3, y0 - j2 as f64 + F3, z0 - k2 as f64 + F3, 0.6);
        let t3 = Self::corner(h(1, 1, 1), x0 - 0.5, y0 - 0.5, z0 - 0.5, 0.6);
        32.0 * (t0 + t1 + t2 + t3)
    }
}

/// PNX `noise.minecraft.simplex.SimplexNoise`.
pub(crate) struct SimplexNoise {
    amplitudes: Vec<f32>,
    levels: Vec<Option<Sampler>>,
    input_factor: f32,
    value_factor: f32,
}

impl SimplexNoise {
    pub(crate) fn new(r: &mut Xoroshiro, first_octave: i32, amplitudes: &[f32]) -> Self {
        let n = amplitudes.len() as i32;
        let levels = (0..n)
            .map(|i| {
                (amplitudes[i as usize] != 0.0).then(|| {
                    let s = r.next_long().wrapping_add(java_hash(&format!("octave_{}", first_octave + i)) as i64);
                    Sampler::new(&mut NukkitRandom::new(s))
                })
            })
            .collect();
        Self {
            amplitudes: amplitudes.to_vec(),
            levels,
            input_factor: 2f64.powi(first_octave) as f32,
            value_factor: (2f64.powi(n - 1) / (2f64.powi(n) - 1.0)) as f32,
        }
    }

    pub(crate) fn value(&self, x: f64, y: f64, z: f64) -> f32 {
        let wrap = |v: f64| (v - (v / 3.3554432E7 + 0.5).floor() * 3.3554432E7) as f32;
        let (mut d0, mut d1, mut d2) = (0f32, self.input_factor, self.value_factor);
        for (i, l) in self.levels.iter().enumerate() {
            if let Some(s) = l {
                let d3 = s.sample_3d(wrap(x * d1 as f64) as f64, wrap(y * d1 as f64) as f64, wrap(z * d1 as f64) as f64) as f32;
                d0 += self.amplitudes[i] * d3 * d2;
            }
            d1 *= 2.0;
            d2 /= 2.0;
        }
        d0
    }
}

/// The FeatureHolder noises decoration uses, built once per world seed.
pub(crate) struct Holder {
    pub kelp: SimplexNoise,
    pub clay_dripleaves: SimplexF,
    pub moss_ceiling: SimplexF,
}

pub(crate) fn holder(seed: i64) -> Arc<Holder> {
    static C: OnceLock<Mutex<HashMap<i64, Arc<Holder>>>> = OnceLock::new();
    let c = C.get_or_init(Default::default);
    if let Some(h) = c.lock().get(&seed) {
        return h.clone();
    }
    // Every holder noise takes `identical()` of the root provider, i.e. a fresh Xoroshiro(seed).
    let id = || Xoroshiro::new(seed);
    let clay_dripleaves = SimplexF::new(&mut id(), 1.0, 2.0 / 4.0, 1.0 / 15.0);
    let moss_ceiling = SimplexF::new(&mut id(), 2.0, 2.0 / 4.0, 1.0 / 30.0);
    let kelp = SimplexNoise::new(&mut id(), -7, &[1.0]);
    let h = Arc::new(Holder { kelp, clay_dripleaves, moss_ceiling });
    c.lock().insert(seed, h.clone());
    h
}

// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
// Ported from PNX NoiseGeneratorImprovedD, NoiseGeneratorOctavesD, NoiseGeneratorSimplexD
//! The legacy double-precision noises PNX's End terrain uses.

use crate::overworld::feature::{NukkitRandom, Xoroshiro};

/// The two calls PNX's noise constructors make on a `RandomSourceProvider`.
pub trait Source {
    fn double(&mut self) -> f64;
    /// PNX `nextBoundedInt(max)`: 0..=max for both generators.
    fn bounded(&mut self, max: i32) -> i32;
}

impl Source for Xoroshiro {
    fn double(&mut self) -> f64 {
        self.next_double()
    }
    fn bounded(&mut self, max: i32) -> i32 {
        self.next_bounded_int(max)
    }
}

impl Source for NukkitRandom {
    fn double(&mut self) -> f64 {
        self.next_double()
    }
    fn bounded(&mut self, max: i32) -> i32 {
        self.next_bounded_int(max)
    }
}

/// Offsets + permutation table (same construction in Improved and Simplex).
fn perm(r: &mut impl Source) -> ([f64; 3], [i32; 512]) {
    let o = [r.double() * 256.0, r.double() * 256.0, r.double() * 256.0];
    let mut p = [0i32; 512];
    for (i, v) in p.iter_mut().enumerate().take(256) {
        *v = i as i32;
    }
    for l in 0..256 {
        // bounded is inclusive, so j can be 256 (PNX quirk, kept).
        let j = (r.bounded(256 - l as i32) + l as i32) as usize;
        p.swap(l, j);
        p[l + 256] = p[l];
    }
    (o, p)
}

const GX: [f64; 16] = [1., -1., 1., -1., 1., -1., 1., -1., 0., 0., 0., 0., 1., 0., -1., 0.];
const GY: [f64; 16] = [1., 1., -1., -1., 0., 0., 0., 0., 1., -1., 1., -1., 1., -1., 1., -1.];
const GZ: [f64; 16] = [0., 0., 0., 0., 1., 1., -1., -1., 1., 1., -1., -1., 0., 1., 0., -1.];

fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}
fn grad(h: i32, x: f64, y: f64, z: f64) -> f64 {
    let i = (h & 15) as usize;
    GX[i] * x + GY[i] * y + GZ[i] * z
}
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}
fn split(v: f64) -> (usize, f64) {
    let mut i = v as i32;
    if v < i as f64 {
        i -= 1;
    }
    ((i & 255) as usize, v - i as f64)
}

pub struct Improved {
    o: [f64; 3],
    p: [i32; 512],
}

impl Improved {
    pub fn new(r: &mut impl Source) -> Self {
        let (o, p) = perm(r);
        Self { o, p }
    }

    /// `populateNoiseArray`, 3D branch only (the End always samples ySize 33).
    #[allow(clippy::too_many_arguments)]
    fn populate(&self, out: &mut [f64], off: [f64; 3], size: [usize; 3], scale: [f64; 3], noise_scale: f64) {
        let p = &self.p;
        let pi = |i: usize| p[i] as usize;
        let d0 = 1.0 / noise_scale;
        let mut idx = 0;
        let mut k = usize::MAX;
        let (mut d1, mut d2, mut d3, mut d4) = (0.0, 0.0, 0.0, 0.0);
        for l2 in 0..size[0] {
            let (j3, d5) = split(off[0] + l2 as f64 * scale[0] + self.o[0]);
            let d6 = fade(d5);
            for k3 in 0..size[2] {
                let (i4, d7) = split(off[2] + k3 as f64 * scale[2] + self.o[2]);
                let d8 = fade(d7);
                for j4 in 0..size[1] {
                    let (l4, d9) = split(off[1] + j4 as f64 * scale[1] + self.o[1]);
                    let d10 = fade(d9);
                    if j4 == 0 || l4 != k {
                        k = l4;
                        let l = pi(j3) + l4;
                        let i1 = pi(l) + i4;
                        let j1 = pi(l + 1) + i4;
                        let k1 = pi(j3 + 1) + l4;
                        let l1 = pi(k1) + i4;
                        let i2 = pi(k1 + 1) + i4;
                        d1 = lerp(d6, grad(p[i1], d5, d9, d7), grad(p[l1], d5 - 1.0, d9, d7));
                        d2 = lerp(d6, grad(p[j1], d5, d9 - 1.0, d7), grad(p[i2], d5 - 1.0, d9 - 1.0, d7));
                        d3 = lerp(d6, grad(p[i1 + 1], d5, d9, d7 - 1.0), grad(p[l1 + 1], d5 - 1.0, d9, d7 - 1.0));
                        d4 = lerp(d6, grad(p[j1 + 1], d5, d9 - 1.0, d7 - 1.0), grad(p[i2 + 1], d5 - 1.0, d9 - 1.0, d7 - 1.0));
                    }
                    let v = lerp(d8, lerp(d10, d1, d2), lerp(d10, d3, d4));
                    out[idx] += v * d0;
                    idx += 1;
                }
            }
        }
    }
}

pub struct Octaves(Vec<Improved>);

impl Octaves {
    pub fn new(r: &mut impl Source, n: usize) -> Self {
        Self((0..n).map(|_| Improved::new(r)).collect())
    }

    /// `generateNoiseOctaves` into `out` (len xs*ys*zs, order x, z, y).
    #[allow(clippy::too_many_arguments)]
    pub fn generate(&self, out: &mut [f64], off: [i32; 3], size: [usize; 3], scale: [f64; 3]) {
        out.iter_mut().for_each(|v| *v = 0.0);
        let mut d3 = 1.0;
        for g in &self.0 {
            let mut d0 = off[0] as f64 * d3 * scale[0];
            let d1 = off[1] as f64 * d3 * scale[1];
            let mut d2 = off[2] as f64 * d3 * scale[2];
            let k = d0.floor() as i64;
            let l = d2.floor() as i64;
            d0 -= k as f64;
            d2 -= l as f64;
            d0 += (k % 16_777_216) as f64;
            d2 += (l % 16_777_216) as f64;
            g.populate(out, [d0, d1, d2], size, [scale[0] * d3, scale[1] * d3, scale[2] * d3], d3);
            d3 /= 2.0;
        }
    }
}

const GRAD3: [[i32; 2]; 12] = [[1, 1], [-1, 1], [1, -1], [-1, -1], [1, 0], [-1, 0], [1, 0], [-1, 0], [0, 1], [0, -1], [0, 1], [0, -1]];

pub struct Simplex {
    p: [i32; 512],
}

impl Simplex {
    pub fn new(r: &mut impl Source) -> Self {
        Self { p: perm(r).1 }
    }

    pub fn value(&self, x: f64, z: f64) -> f64 {
        let s3 = 3f64.sqrt();
        let fl = |v: f64| if v > 0.0 { v as i32 } else { v as i32 - 1 };
        let d4 = (x + z) * (0.5 * (s3 - 1.0));
        let i = fl(x + d4);
        let j = fl(z + d4);
        let g = (3.0 - s3) / 6.0;
        let d6 = (i + j) as f64 * g;
        let d9 = x - (i as f64 - d6);
        let d10 = z - (j as f64 - d6);
        let (k, l) = if d9 > d10 { (1, 0) } else { (0, 1) };
        let d11 = d9 - k as f64 + g;
        let d12 = d10 - l as f64 + g;
        let d13 = d9 - 1.0 + 2.0 * g;
        let d14 = d10 - 1.0 + 2.0 * g;
        let (i1, j1) = ((i & 255) as usize, (j & 255) as usize);
        let p = &self.p;
        let k1 = p[i1 + p[j1] as usize] % 12;
        let l1 = p[i1 + k + p[j1 + l] as usize] % 12;
        let i2 = p[i1 + 1 + p[j1 + 1] as usize] % 12;
        let corner = |gi: i32, a: f64, b: f64| {
            let t = 0.5 - a * a - b * b;
            if t < 0.0 {
                0.0
            } else {
                let t = t * t;
                let gr = GRAD3[gi as usize];
                t * t * (gr[0] as f64 * a + gr[1] as f64 * b)
            }
        };
        70.0 * (corner(k1, d9, d10) + corner(l1, d11, d12) + corner(i2, d13, d14))
    }
}

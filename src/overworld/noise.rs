//! Improved Perlin noise as PNX (and vanilla Java) use it: single samplers, octave stacks and
//! the "normal noise" pair. Ported with the same seeding order so seeds match PNX.

use super::rng::{Xoroshiro, skip_262};

const GRAD: [[f64; 3]; 16] = [
    [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0], [1.0, -1.0, 0.0], [-1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0], [-1.0, 0.0, 1.0], [1.0, 0.0, -1.0], [-1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0], [0.0, -1.0, 1.0], [0.0, 1.0, -1.0], [0.0, -1.0, -1.0],
    [1.0, 1.0, 0.0], [0.0, -1.0, 1.0], [-1.0, 1.0, 0.0], [0.0, -1.0, -1.0],
];

pub struct Perlin {
    origin: [f64; 3],
    p: [u8; 256],
}

impl Perlin {
    pub fn new(r: &mut Xoroshiro) -> Self {
        let origin = [r.next_double() * 256.0, r.next_double() * 256.0, r.next_double() * 256.0];
        let mut p = [0u8; 256];
        for (i, v) in p.iter_mut().enumerate() {
            *v = i as u8;
        }
        for i in 0..256 {
            // nextBoundedInt(255 - i) = nextInt(256 - i)
            let j = r.next_int_below(256 - i as i32) as usize + i;
            p.swap(i, j);
        }
        Self { origin, p }
    }

    /// Noise at a point; `y_amp`/`min_y` quantise y for the blended terrain noise (0 = off).
    pub fn sample(&self, x: f64, y: f64, z: f64, y_amp: f64, min_y: f64) -> f64 {
        let (ox, oy, oz) = (x + self.origin[0], y + self.origin[1], z + self.origin[2]);
        let (fx, fy, fz) = (ox.floor(), oy.floor(), oz.floor());
        let (lx, ly, lz) = (ox - fx, oy - fy, oz - fz);
        let mut y_off = 0.0;
        if y_amp != 0.0 {
            let clamp = if min_y >= 0.0 && min_y < ly { min_y } else { ly };
            y_off = (clamp / y_amp + 1.0e-7).floor() * y_amp;
        }
        self.sample_at(fx as i32, fy as i32, fz as i32, lx, ly - y_off, lz, ly)
    }

    #[allow(clippy::too_many_arguments)]
    fn sample_at(&self, sx: i32, sy: i32, sz: i32, lx: f64, ly: f64, lz: f64, fade_ly: f64) -> f64 {
        let p = |i: i32| self.p[(i & 0xff) as usize] as i32;
        let (a, b) = (p(sx), p(sx + 1));
        let (aa, ba, ab, bb) = (p(a + sy), p(b + sy), p(a + sy + 1), p(b + sy + 1));
        let g = |h: i32, x: f64, y: f64, z: f64| {
            let g = &GRAD[(p(h) & 15) as usize];
            g[0] * x + g[1] * y + g[2] * z
        };
        let (x1, y1, z1) = (lx - 1.0, ly - 1.0, lz - 1.0);
        let g000 = g(aa + sz, lx, ly, lz);
        let g100 = g(ba + sz, x1, ly, lz);
        let g010 = g(ab + sz, lx, y1, lz);
        let g110 = g(bb + sz, x1, y1, lz);
        let g001 = g(aa + sz + 1, lx, ly, z1);
        let g101 = g(ba + sz + 1, x1, ly, z1);
        let g011 = g(ab + sz + 1, lx, y1, z1);
        let g111 = g(bb + sz + 1, x1, y1, z1);
        let fade = |t: f64| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
        let (u, v, w) = (fade(lx), fade(fade_ly), fade(lz));
        let x00 = g000 + u * (g100 - g000);
        let x10 = g010 + u * (g110 - g010);
        let x01 = g001 + u * (g101 - g001);
        let x11 = g011 + u * (g111 - g011);
        let y0 = x00 + v * (x10 - x00);
        let y1 = x01 + v * (x11 - x01);
        y0 + w * (y1 - y0)
    }
}

fn maintain_precision(d: f64) -> f64 {
    const WRAP: f64 = 33_554_432.0; // 2^25
    if (-WRAP / 2.0..WRAP / 2.0).contains(&d) {
        return d;
    }
    d - ((d / WRAP + 0.5).floor()) * WRAP
}

/// PNX `PerlinNoise`: octaves from `first_octave` with per-octave amplitudes.
pub struct PerlinOctaves {
    levels: Vec<Option<Perlin>>,
    amps: Vec<f64>,
    input_factor: f64,
    value_factor: f64,
    pub max_value: f64,
}

impl PerlinOctaves {
    pub fn new(r: &mut Xoroshiro, first_octave: i32, amps: &[f64]) -> Self {
        let octaves = amps.len() as i32;
        let zero = -first_octave;
        let mut levels: Vec<Option<Perlin>> = (0..octaves).map(|_| None).collect();
        let zero_octave = Perlin::new(r);
        let mut zero_octave = Some(zero_octave);
        if zero >= 0 && zero < octaves && amps[zero as usize] != 0.0 {
            levels[zero as usize] = zero_octave.take();
        }
        let mut ix = zero - 1;
        while ix >= 0 {
            if ix < octaves && amps[ix as usize] != 0.0 {
                levels[ix as usize] = Some(Perlin::new(r));
            } else {
                r.set_seed(skip_262(r.seed));
            }
            ix -= 1;
        }
        let value_factor = 2f64.powi(octaves - 1) / (2f64.powi(octaves) - 1.0);
        let mut s = Self { levels, amps: amps.to_vec(), input_factor: 2f64.powi(-zero), value_factor, max_value: 0.0 };
        s.max_value = s.edge_value(2.0);
        s
    }

    pub fn value(&self, x: f64, y: f64, z: f64) -> f64 {
        let (mut v, mut f, mut vf) = (0.0, self.input_factor, self.value_factor);
        for (i, n) in self.levels.iter().enumerate() {
            if let Some(n) = n {
                let s = n.sample(maintain_precision(x * f), maintain_precision(y * f), maintain_precision(z * f), 0.0, 0.0);
                v += self.amps[i] * s * vf;
            }
            f *= 2.0;
            vf /= 2.0;
        }
        v
    }

    fn edge_value(&self, n: f64) -> f64 {
        let (mut v, mut vf) = (0.0, self.value_factor);
        for (i, l) in self.levels.iter().enumerate() {
            if l.is_some() {
                v += self.amps[i] * n * vf;
            }
            vf /= 2.0;
        }
        v
    }
}

/// PNX `NormalNoise`: two octave stacks summed, the second at a slightly scaled input.
pub struct NormalNoise {
    first: PerlinOctaves,
    second: PerlinOctaves,
    value_factor: f64,
    pub max_value: f64,
}

impl NormalNoise {
    pub fn new(mut r: Xoroshiro, first_octave: i32, amps: &[f32]) -> Self {
        let amps: Vec<f64> = amps.iter().map(|&a| a as f64).collect();
        let nz: Vec<usize> = (0..amps.len()).filter(|&i| amps[i] != 0.0).collect();
        let (min, max) = (nz.first().copied().unwrap_or(usize::MAX) as i64, nz.last().copied().map_or(i32::MIN as i64, |m| m as i64));
        let first = PerlinOctaves::new(&mut r, first_octave, &amps);
        let second = PerlinOctaves::new(&mut r, first_octave, &amps);
        let span = (max - min) as i32;
        let value_factor = 1.0 / 6.0 / (0.1 * (1.0 + 1.0 / (span as f64 + 1.0)));
        let max_value = (first.max_value + second.max_value) * value_factor;
        Self { first, second, value_factor, max_value }
    }

    /// PNX returns a float here; the precision loss is part of the terrain shape.
    pub fn value(&self, x: f64, y: f64, z: f64) -> f64 {
        const K: f64 = 1.018_126_888_217_522_7;
        ((self.first.value(x, y, z) + self.second.value(x * K, y * K, z * K)) * self.value_factor) as f32 as f64
    }
}

/// PNX `OctavePerlinNoiseSampler` from octaves `lo..=0` (blended terrain noise).
pub struct OctaveList {
    pub samplers: Vec<Perlin>,
}

impl OctaveList {
    /// Octaves `lo..=0` (lo < 0): sampler 0 is octave 0, then one per lower octave.
    pub fn new(r: &mut Xoroshiro, lo: i32) -> Self {
        let length = (-lo + 1) as usize;
        let samplers = (0..length).map(|_| Perlin::new(r)).collect();
        Self { samplers }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_bounded_and_deterministic() {
        let n = NormalNoise::new(Xoroshiro::new(1), -7, &[1.0, 1.0]);
        let m = NormalNoise::new(Xoroshiro::new(1), -7, &[1.0, 1.0]);
        let mut seen = 0.0f64;
        for i in 0..500 {
            let v = n.value(i as f64 * 3.1, 7.0, i as f64 * -1.7);
            assert_eq!(v, m.value(i as f64 * 3.1, 7.0, i as f64 * -1.7));
            assert!(v.abs() <= n.max_value + 1e-9, "{v} > {}", n.max_value);
            seen = seen.max(v.abs());
        }
        assert!(seen > 0.05, "noise varies");
    }
}

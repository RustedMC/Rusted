// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
// Ported from PNX NukkitRandom, ReseedableMersenneTwister, BedrockRandom, LCG (SKIP_262).
//! Random sources, copied from `overworld/rng.rs` plus what the nether needs
//! (`NukkitRandom` keeps its seed and has nextInt/nextLong/fork/identical).

#![allow(dead_code)] // copied from the overworld; not every method is used here


/// Java's `java.util.Random` LCG stepped 262 times, as PNX skips noise octaves.
pub fn skip_262(seed: i64) -> i64 {
    const M: u64 = 25_214_903_917;
    const A: u64 = 11;
    const MASK: u64 = (1 << 48) - 1;
    // LCG.combine(262): binary exponentiation of the affine map.
    let (mut mul, mut add) = (1u64, 0u64);
    let (mut im, mut ia) = (M, A);
    let mut k = 262u64;
    while k != 0 {
        if k & 1 != 0 {
            mul = mul.wrapping_mul(im);
            add = im.wrapping_mul(add).wrapping_add(ia);
        }
        ia = im.wrapping_add(1).wrapping_mul(ia);
        im = im.wrapping_mul(im);
        k >>= 1;
    }
    let (mul, add) = (mul & MASK, add & MASK);
    ((seed as u64).wrapping_mul(mul).wrapping_add(add) & MASK) as i64
}

/// PNX `NukkitRandom`.
pub struct NukkitRandom {
    state: Box<[i32; 624]>,
    index: usize,
    bool_source: i32,
    seed: i64,
}

const N: usize = 624;
const M_: usize = 397;
const GOLDEN: i64 = 0x9e37_79b9_7f4a_7c15u64 as i64;

fn initial_state() -> &'static [i32; N] {
    static S: std::sync::OnceLock<[i32; N]> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        let mut s = [0i32; N];
        let mut v: i64 = 19_650_218;
        s[0] = v as i32;
        for (i, slot) in s.iter_mut().enumerate().skip(1) {
            v = (1_812_433_253i64.wrapping_mul(v ^ ((v as u64) >> 30) as i64).wrapping_add(i as i64)) & 0xffff_ffff;
            *slot = v as i32;
        }
        s
    })
}

fn stafford13(v: i64) -> i64 {
    let mut v = v as u64;
    v = (v ^ (v >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    v = (v ^ (v >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    (v ^ (v >> 31)) as i64
}

impl NukkitRandom {
    pub fn new(seed: i64) -> Self {
        let mut r = Self { state: Box::new([0; N]), index: N, bool_source: 1, seed };
        r.set_seed(seed);
        r
    }

    pub fn set_seed(&mut self, seed: i64) {
        self.seed = seed;
        *self.state = *initial_state();
        let s = &mut self.state;
        let mix_seed = |s: &mut [i32; N], i: usize, value: i32, seed_index: i32| {
            let p = s[i - 1];
            s[i] = (s[i] ^ (p ^ ((p as u32) >> 30) as i32).wrapping_mul(1_664_525)).wrapping_add(value).wrapping_add(seed_index);
        };
        let mix_state = |s: &mut [i32; N], i: usize| {
            let p = s[i - 1];
            s[i] = (s[i] ^ (p ^ ((p as u32) >> 30) as i32).wrapping_mul(1_566_083_941)).wrapping_sub(i as i32);
        };
        let mut sm_state = if seed == GOLDEN.wrapping_neg() { !seed } else { seed };
        let mut sm_value = 0i64;
        for seed_index in 0..N - 1 {
            let value = if seed_index & 1 == 0 {
                sm_state = sm_state.wrapping_add(GOLDEN);
                sm_value = stafford13(sm_state);
                sm_value as i32
            } else {
                ((sm_value as u64) >> 32) as i32
            };
            mix_seed(s, seed_index + 1, value, seed_index as i32);
        }
        s[0] = s[N - 1];
        mix_seed(s, 1, ((sm_value as u64) >> 32) as i32, (N - 1) as i32);
        for i in 2..N {
            mix_state(s, i);
        }
        s[0] = s[N - 1];
        mix_state(s, 1);
        s[0] = i32::MIN; // UPPER_MASK
        self.index = N;
        self.bool_source = 1;
    }

    fn next(&mut self) -> i32 {
        const UPPER: i32 = i32::MIN;
        const LOWER: i32 = i32::MAX;
        const MATRIX_A: i32 = 0x9908_b0dfu32 as i32;
        let s = &mut self.state;
        if self.index >= N {
            let twist = |a: i32, b: i32, m: i32| {
                let v = (a & UPPER) | (b & LOWER);
                m ^ ((v as u32) >> 1) as i32 ^ (MATRIX_A & (v & 1).wrapping_neg())
            };
            for i in 0..N - M_ {
                s[i] = twist(s[i], s[i + 1], s[i + M_]);
            }
            for i in N - M_..N - 1 {
                s[i] = twist(s[i], s[i + 1], s[i + M_ - N]);
            }
            s[N - 1] = twist(s[N - 1], s[0], s[M_ - 1]);
            self.index = 0;
        }
        let mut v = s[self.index];
        self.index += 1;
        v ^= ((v as u32) >> 11) as i32;
        v ^= (v << 7) & 0x9d2c_5680u32 as i32;
        v ^= (v << 15) & 0xefc6_0000u32 as i32;
        v ^= ((v as u32) >> 18) as i32;
        v
    }

    pub fn next_float(&mut self) -> f32 {
        ((self.next() as u32) >> 8) as f32 * (1.0 / (1u32 << 24) as f32)
    }

    pub fn next_double(&mut self) -> f64 {
        let high = (((self.next() as u32) >> 6) as u64) << 27;
        let low = ((self.next() as u32) >> 5) as u64;
        (high | low) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Commons RNG `nextInt(n)` (Lemire's method), n > 0.
    fn next_int_n(&mut self, n: i32) -> i32 {
        let n = n as u64;
        let mut m = (self.next() as u32 as u64) * n;
        let mut l = m & 0xffff_ffff;
        if l < n {
            let t = (1u64 << 32) % n;
            while l < t {
                m = (self.next() as u32 as u64) * n;
                l = m & 0xffff_ffff;
            }
        }
        (m >> 32) as i32
    }

    /// PNX `nextBoundedInt(bound)`: 0..=bound.
    pub fn next_bounded_int(&mut self, bound: i32) -> i32 {
        self.next_int_n(bound + 1)
    }

    /// PNX `nextInt(min, max)`: min..=max.
    pub fn next_range(&mut self, min: i32, max: i32) -> i32 {
        min + self.next_int_n(max - min + 1)
    }

    pub fn next_boolean(&mut self) -> bool {
        let mut bits = self.bool_source;
        if bits == 1 {
            bits = self.next();
            self.bool_source = ((bits as u32) >> 1) as i32 | i32::MIN;
            return bits & 1 == 1;
        }
        self.bool_source = ((bits as u32) >> 1) as i32;
        bits & 1 == 1
    }
}


impl NukkitRandom {
    pub fn seed(&self) -> i64 {
        self.seed
    }

    /// Commons RNG `nextInt()`.
    pub fn next_int(&mut self) -> i32 {
        self.next()
    }

    /// PNX `nextInt(max)`: 0..=max.
    pub fn next_int_max(&mut self, max: i32) -> i32 {
        self.next_int_n(max + 1)
    }

    /// Commons RNG `nextLong()`: high word first.
    pub fn next_long(&mut self) -> i64 {
        let hi = self.next() as i64;
        let lo = self.next() as u32 as i64;
        (hi << 32) | lo
    }

    /// PNX `fork()`: a new generator seeded with `nextLong()`.
    pub fn fork(&mut self) -> Self {
        Self::new(self.next_long())
    }

    /// PNX `identical()`: a new generator with this one's seed.
    pub fn identical(&self) -> Self {
        Self::new(self.seed)
    }
}

#[derive(Clone)]
pub struct BedrockRandom {
    state: Box<[i32; 624]>,
    index: usize,
    seed: i64,
    gaussian: Option<f64>,
}

impl BedrockRandom {
    pub fn new(seed: i64) -> Self {
        let mut r = Self { state: Box::new([0; 624]), index: 624, seed: 0, gaussian: None };
        r.set_seed(seed);
        r
    }

    pub fn seed(&self) -> i64 {
        self.seed
    }

    pub fn set_seed(&mut self, seed: i64) {
        let v = seed as i32;
        self.seed = v as i64;
        let s = &mut self.state;
        s[0] = v;
        for i in 1..624 {
            let p = s[i - 1];
            s[i] = 1_812_433_253i32.wrapping_mul(p ^ ((p as u32) >> 30) as i32).wrapping_add(i as i32);
        }
        self.index = 624;
        self.gaussian = None;
    }

    fn raw(&mut self) -> u32 {
        if self.index >= 624 {
            let s = &mut self.state;
            let tw = |a: i32, b: i32, m: i32| {
                let y = (a & i32::MIN) | (b & i32::MAX);
                m ^ ((y as u32) >> 1) as i32 ^ ((y & 1).wrapping_neg() & 0x9908_b0dfu32 as i32)
            };
            for i in 0..624 - 397 {
                s[i] = tw(s[i], s[i + 1], s[i + 397]);
            }
            for i in 624 - 397..623 {
                s[i] = tw(s[i], s[i + 1], s[i + 397 - 624]);
            }
            s[623] = tw(s[623], s[0], s[396]);
            self.index = 0;
        }
        let mut v = self.state[self.index];
        self.index += 1;
        v ^= ((v as u32) >> 11) as i32;
        v ^= (v << 7) & 0x9d2c_5680u32 as i32;
        v ^= (v << 15) & 0xefc6_0000u32 as i32;
        v ^= ((v as u32) >> 18) as i32;
        v as u32
    }

    pub fn fork(&mut self) -> Self {
        Self::new(self.next_long())
    }

    pub fn next_int(&mut self) -> i32 {
        (self.raw() >> 1) as i32
    }

    /// `nextInt(max)` = `nextBoundedInt(max)` = `nextExclusiveInt(max)`: 0..max (unsigned remainder).
    pub fn next_int_below(&mut self, max: i32) -> i32 {
        if max <= 0 { 0 } else { (self.raw() % max as u32) as i32 }
    }

    /// `nextInt(min, max)`: min..=max.
    pub fn next_int_range(&mut self, min: i32, max: i32) -> i32 {
        if max == min {
            return min;
        }
        let range = max as i64 - min as i64 + 1;
        (min as i64 + (self.raw() as i64) % range) as i32
    }

    pub fn next_long(&mut self) -> i64 {
        let high = (self.raw() as i64) << 32;
        high.wrapping_add(self.raw() as i64)
    }

    pub fn next_boolean(&mut self) -> bool {
        (self.raw() >> 27) & 1 != 0
    }

    pub fn next_float(&mut self) -> f32 {
        (self.raw() as f64 * (1.0 / 4_294_967_296.0)) as f32
    }

    pub fn next_double(&mut self) -> f64 {
        self.raw() as f64 * (1.0 / 4_294_967_296.0)
    }

    pub fn next_gaussian(&mut self) -> f64 {
        if let Some(g) = self.gaussian.take() {
            return g;
        }
        loop {
            let x = 2.0 * self.next_double() - 1.0;
            let y = 2.0 * self.next_double() - 1.0;
            let sq = x * x + y * y;
            if sq < 1.0 && sq != 0.0 {
                let m = (-2.0 * sq.ln() / sq).sqrt();
                self.gaussian = Some(y * m);
                return x * m;
            }
        }
    }
}

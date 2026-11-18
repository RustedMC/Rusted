//! Density functions (from PNX `DensityCommon`), built once per world, evaluated per chunk.
//! Caching can differ from PNX, except `Interpolated` and `FlatCache` which change values.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::noise::{NormalNoise, OctaveList};

pub type F = Arc<Df>;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Op {
    Add,
    Mul,
    Min,
    Max,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Map {
    Abs,
    Square,
    Cube,
    HalfNegative,
    QuarterNegative,
    Invert,
    Squeeze,
}

impl Map {
    fn apply(self, v: f64) -> f64 {
        match self {
            Map::Abs => v.abs(),
            Map::Square => v * v,
            Map::Cube => v * v * v,
            Map::HalfNegative => if v > 0.0 { v } else { v * 0.5 },
            Map::QuarterNegative => if v > 0.0 { v } else { v * 0.25 },
            Map::Invert => if v == 0.0 { f64::INFINITY } else { 1.0 / v },
            Map::Squeeze => {
                let c = v.clamp(-1.0, 1.0);
                c / 2.0 - c * c * c / 24.0
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
#[allow(clippy::enum_variant_names)]
pub enum Cache {
    Flat,
    Cache2d,
    Once,
    AllInCell,
    Interpolated,
}

pub enum SplineValue {
    Const(f64),
    Func(F),
}

pub struct Spline {
    coord: F,
    locations: Vec<f64>,
    values: Vec<SplineValue>,
    derivatives: Vec<f64>,
}

pub struct Blended {
    min_limit: OctaveList,
    max_limit: OctaveList,
    main: OctaveList,
    xz_scale: f64,
    y_scale: f64,
    xz_factor: f64,
    y_factor: f64,
    smear: f64,
}

pub enum Kind {
    Const(f64),
    Y,
    Noise { n: Arc<NormalNoise>, xz: f64, y: f64 },
    /// x = noise(x/4, 0, z/4) * 4
    ShiftA(Arc<NormalNoise>),
    /// noise(z/4, x/4, 0) * 4
    ShiftB(Arc<NormalNoise>),
    Shifted { sx: F, sy: F, sz: F, xz: f64, y: f64, n: Arc<NormalNoise> },
    Ap2 { op: Op, a: F, b: F },
    MulOrAdd { mul: bool, input: F, arg: f64 },
    Mapped { map: Map, input: F },
    Clamp { input: F, min: f64, max: f64 },
    Range { input: F, lo: f64, hi: f64, a: F, b: F },
    /// Spaghetti caves: `rarity * |noise(pos / rarity)|`, rarity from the input value.
    Weird { input: F, n: Arc<NormalNoise>, type1: bool },
    YGrad { from: i32, to: i32, fv: f64, tv: f64 },
    Spline(Box<Spline>),
    Blended(Box<Blended>),
    Cached { cache: Cache, slot: usize, input: F },
}

pub struct Df {
    pub kind: Kind,
    pub min: f64,
    pub max: f64,
}

static SLOTS: AtomicUsize = AtomicUsize::new(0);

fn node(kind: Kind, min: f64, max: f64) -> F {
    Arc::new(Df { kind, min, max })
}

pub fn constant(v: f64) -> F {
    node(Kind::Const(v), v, v)
}

pub fn zero() -> F {
    constant(0.0)
}

pub fn y() -> F {
    node(Kind::Y, f64::NEG_INFINITY, f64::INFINITY)
}

pub fn noise(n: &Arc<NormalNoise>, xz: f64, y: f64) -> F {
    node(Kind::Noise { n: n.clone(), xz, y }, -n.max_value, n.max_value)
}

pub fn shift_a(n: &Arc<NormalNoise>) -> F {
    node(Kind::ShiftA(n.clone()), -n.max_value * 4.0, n.max_value * 4.0)
}

pub fn shift_b(n: &Arc<NormalNoise>) -> F {
    node(Kind::ShiftB(n.clone()), -n.max_value * 4.0, n.max_value * 4.0)
}

pub fn shifted(sx: &F, sy: &F, sz: &F, xz: f64, y: f64, n: &Arc<NormalNoise>) -> F {
    node(Kind::Shifted { sx: sx.clone(), sy: sy.clone(), sz: sz.clone(), xz, y, n: n.clone() }, -n.max_value, n.max_value)
}

fn ap2(op: Op, a: &F, b: &F) -> F {
    let (min1, max1, min2, max2) = (a.min, a.max, b.min, b.max);
    let min = match op {
        Op::Add => min1 + min2,
        Op::Mul => {
            if min1 > 0.0 && min2 > 0.0 { min1 * min2 } else if max1 < 0.0 && max2 < 0.0 { max1 * max2 } else { (min1 * max2).min(max1 * min2) }
        }
        Op::Min => min1.min(min2),
        Op::Max => min1.max(min2),
    };
    let max = match op {
        Op::Add => max1 + max2,
        Op::Mul => {
            if min1 > 0.0 && min2 > 0.0 { max1 * max2 } else if max1 < 0.0 && max2 < 0.0 { min1 * min2 } else { (min1 * min2).max(max1 * max2) }
        }
        Op::Min => max1.min(max2),
        Op::Max => max1.max(max2),
    };
    if matches!(op, Op::Add | Op::Mul) {
        if let Kind::Const(c) = a.kind {
            return node(Kind::MulOrAdd { mul: op == Op::Mul, input: b.clone(), arg: c }, min, max);
        }
        if let Kind::Const(c) = b.kind {
            return node(Kind::MulOrAdd { mul: op == Op::Mul, input: a.clone(), arg: c }, min, max);
        }
    }
    node(Kind::Ap2 { op, a: a.clone(), b: b.clone() }, min, max)
}

pub fn add(a: &F, b: &F) -> F {
    ap2(Op::Add, a, b)
}
pub fn mul(a: &F, b: &F) -> F {
    ap2(Op::Mul, a, b)
}
pub fn min(a: &F, b: &F) -> F {
    ap2(Op::Min, a, b)
}
pub fn max(a: &F, b: &F) -> F {
    ap2(Op::Max, a, b)
}
pub fn addc(c: f64, a: &F) -> F {
    add(&constant(c), a)
}
pub fn mulc(c: f64, a: &F) -> F {
    mul(&constant(c), a)
}

pub fn map(input: &F, m: Map) -> F {
    let (lo, hi) = (m.apply(input.min), m.apply(input.max));
    let (min, max) = match m {
        Map::Invert if input.min < 0.0 && input.max > 0.0 => (f64::NEG_INFINITY, f64::INFINITY),
        Map::Abs | Map::Square => (lo.min(hi).max(0.0), lo.max(hi)),
        _ => (lo.min(hi), lo.max(hi)),
    };
    node(Kind::Mapped { map: m, input: input.clone() }, min, max)
}

pub fn clamp(input: &F, min: f64, max: f64) -> F {
    node(Kind::Clamp { input: input.clone(), min, max }, min, max)
}

pub fn range(input: &F, lo: f64, hi: f64, a: &F, b: &F) -> F {
    node(Kind::Range { input: input.clone(), lo, hi, a: a.clone(), b: b.clone() }, a.min.min(b.min), a.max.max(b.max))
}

pub fn weird(input: &F, n: &Arc<NormalNoise>, type1: bool) -> F {
    let max_rarity = if type1 { 2.0 } else { 3.0 };
    node(Kind::Weird { input: input.clone(), n: n.clone(), type1 }, 0.0, max_rarity * n.max_value)
}

pub fn y_grad(from: i32, to: i32, fv: f64, tv: f64) -> F {
    node(Kind::YGrad { from, to, fv, tv }, fv.min(tv), fv.max(tv))
}

pub fn cached(cache: Cache, input: &F) -> F {
    let slot = SLOTS.fetch_add(1, Ordering::Relaxed);
    node(Kind::Cached { cache, slot, input: input.clone() }, input.min, input.max)
}
pub fn flat(input: &F) -> F {
    cached(Cache::Flat, input)
}
pub fn cache2d(input: &F) -> F {
    cached(Cache::Cache2d, input)
}
pub fn once(input: &F) -> F {
    cached(Cache::Once, input)
}
pub fn interpolated(input: &F) -> F {
    cached(Cache::Interpolated, input)
}
pub fn all_in_cell(input: &F) -> F {
    cached(Cache::AllInCell, input)
}

/// `mid + half_range * input`, mapping a [-1, 1] noise to [lo, hi].
pub fn mapped_noise(n: &Arc<NormalNoise>, xz: f64, y: f64, lo: f64, hi: f64) -> F {
    add(&constant((lo + hi) * 0.5), &mul(&constant((hi - lo) * 0.5), &noise(n, xz, y)))
}

/// A spline point: location, value (constant or another function), derivative.
pub enum P {
    C(f64),
    F(F),
}

pub fn spline(coord: &F, points: &[(f64, P, f64)]) -> F {
    let mut pts: Vec<&(f64, P, f64)> = points.iter().collect();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut s = Spline { coord: coord.clone(), locations: vec![], values: vec![], derivatives: vec![] };
    for (loc, v, d) in pts {
        let (vmin, vmax, v) = match v {
            P::C(c) => (*c, *c, SplineValue::Const(*c)),
            P::F(f) => (f.min, f.max, SplineValue::Func(f.clone())),
        };
        lo = lo.min(vmin);
        hi = hi.max(vmax);
        s.locations.push(*loc);
        s.values.push(v);
        s.derivatives.push(*d);
    }
    node(Kind::Spline(Box::new(s)), lo, hi)
}

pub fn blended(min_limit: OctaveList, max_limit: OctaveList, main: OctaveList, xz_scale: f64, y_scale: f64, xz_factor: f64, y_factor: f64, smear: f64) -> F {
    node(Kind::Blended(Box::new(Blended { min_limit, max_limit, main, xz_scale, y_scale, xz_factor, y_factor, smear })), -1.5, 1.5)
}

// ---------------------------------------------------------------------------------------------

/// Fast hash for the integer keys the caches use.
#[derive(Default, Clone, Copy)]
pub struct Fx(u64);
impl std::hash::Hasher for Fx {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ b as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }
    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    fn write_i64(&mut self, v: i64) {
        self.write_u64(v as u64);
    }
}
pub type FxMap<K, V> = HashMap<K, V, std::hash::BuildHasherDefault<Fx>>;

#[derive(Default)]
enum Slot {
    #[default]
    Empty,
    /// (x, z) -> value at y 0
    Flat(FxMap<i64, f64>),
    Last { key: (i32, i32, i32), value: f64 },
    /// Corner values by packed position, plus the last filled cell.
    Interp { corners: FxMap<i64, f64>, cell: (i32, i32, i32), values: Box<[f64; 128]> },
    Cell { cell: (i32, i32, i32), values: Box<[f64; 128]> },
}

/// Per-chunk eval state. Reuse one per thread, clear() between chunks.
#[derive(Default)]
pub struct Ctx {
    slots: Vec<Slot>,
}

const NO_CELL: (i32, i32, i32) = (i32::MIN, i32::MIN, i32::MIN);

impl Ctx {
    pub fn clear(&mut self) {
        for s in &mut self.slots {
            match s {
                Slot::Flat(m) => m.clear(),
                Slot::Interp { corners, cell, .. } => {
                    corners.clear();
                    *cell = NO_CELL;
                }
                Slot::Cell { cell, .. } => *cell = NO_CELL,
                Slot::Last { key, .. } => *key = NO_CELL,
                Slot::Empty => {}
            }
        }
    }

    fn slot(&mut self, i: usize) -> &mut Slot {
        if i >= self.slots.len() {
            self.slots.resize_with(i + 1, Slot::default);
        }
        &mut self.slots[i]
    }
}

fn pack2(x: i32, z: i32) -> i64 {
    ((x as i64) << 32) ^ (z as u32 as i64)
}

fn pack3(x: i32, y: i32, z: i32) -> i64 {
    (((x as i64) & 0x3FF_FFFF) << 38) | (((z as i64) & 0x3FF_FFFF) << 12) | ((y as i64) & 0xFFF)
}

fn clamped_map(v: f64, from: f64, to: f64, fv: f64, tv: f64) -> f64 {
    if from == to {
        return if v < from { fv } else { tv };
    }
    let t = ((v - from) / (to - from)).clamp(0.0, 1.0);
    fv + t * (tv - fv)
}

impl Df {
    pub fn eval(&self, c: &mut Ctx, x: i32, y: i32, z: i32) -> f64 {
        match &self.kind {
            Kind::Const(v) => *v,
            Kind::Y => y as f64,
            Kind::Noise { n, xz, y: ys } => n.value(x as f64 * xz, y as f64 * ys, z as f64 * xz),
            Kind::ShiftA(n) => n.value(x as f64 * 0.25, 0.0, z as f64 * 0.25) * 4.0,
            Kind::ShiftB(n) => n.value(z as f64 * 0.25, x as f64 * 0.25, 0.0) * 4.0,
            Kind::Shifted { sx, sy, sz, xz, y: ys, n } => {
                let px = x as f64 * xz + sx.eval(c, x, y, z);
                let py = y as f64 * ys + sy.eval(c, x, y, z);
                let pz = z as f64 * xz + sz.eval(c, x, y, z);
                n.value(px, py, pz)
            }
            Kind::Ap2 { op, a, b } => {
                let v1 = a.eval(c, x, y, z);
                match op {
                    Op::Add => v1 + b.eval(c, x, y, z),
                    Op::Mul => if v1 == 0.0 { 0.0 } else { v1 * b.eval(c, x, y, z) },
                    Op::Min => if v1 < b.min { v1 } else { v1.min(b.eval(c, x, y, z)) },
                    Op::Max => if v1 > b.max { v1 } else { v1.max(b.eval(c, x, y, z)) },
                }
            }
            Kind::MulOrAdd { mul, input, arg } => {
                let v = input.eval(c, x, y, z);
                if *mul { v * arg } else { v + arg }
            }
            Kind::Mapped { map, input } => map.apply(input.eval(c, x, y, z)),
            Kind::Clamp { input, min, max } => input.eval(c, x, y, z).clamp(*min, *max),
            Kind::Range { input, lo, hi, a, b } => {
                let v = input.eval(c, x, y, z);
                if v >= *lo && v < *hi { a.eval(c, x, y, z) } else { b.eval(c, x, y, z) }
            }
            Kind::Weird { input, n, type1 } => {
                let v = input.eval(c, x, y, z);
                let r = if *type1 {
                    if v < -0.5 { 0.75 } else if v < 0.0 { 1.0 } else if v < 0.5 { 1.5 } else { 2.0 }
                } else if v < -0.75 { 0.5 } else if v < -0.5 { 0.75 } else if v < 0.5 { 1.0 } else if v < 0.75 { 2.0 } else { 3.0 };
                r * n.value(x as f64 / r, y as f64 / r, z as f64 / r).abs()
            }
            Kind::YGrad { from, to, fv, tv } => clamped_map(y as f64, *from as f64, *to as f64, *fv, *tv),
            Kind::Spline(s) => s.eval(c, x, y, z),
            Kind::Blended(b) => b.eval(x, y, z),
            Kind::Cached { cache, slot, input } => match cache {
                Cache::Flat => {
                    let k = pack2(x, z);
                    if let Slot::Flat(m) = c.slot(*slot)
                        && let Some(v) = m.get(&k)
                    {
                        return *v;
                    }
                    let v = input.eval(c, x, 0, z);
                    match c.slot(*slot) {
                        Slot::Flat(m) => {
                            m.insert(k, v);
                        }
                        s => *s = Slot::Flat([(k, v)].into_iter().collect()),
                    }
                    v
                }
                Cache::Cache2d | Cache::Once => {
                    let key = if *cache == Cache::Cache2d { (x, i32::MIN, z) } else { (x, y, z) };
                    if let Slot::Last { key: k, value } = c.slot(*slot)
                        && *k == key
                    {
                        return *value;
                    }
                    let value = input.eval(c, x, y, z);
                    *c.slot(*slot) = Slot::Last { key, value };
                    value
                }
                Cache::AllInCell => {
                    let cell = (x >> 2, y >> 3, z >> 2);
                    let idx = (((y & 7) << 4) | ((z & 3) << 2) | (x & 3)) as usize;
                    if let Slot::Cell { cell: cc, values } = c.slot(*slot)
                        && *cc == cell
                    {
                        return values[idx];
                    }
                    let mut values = match std::mem::take(c.slot(*slot)) {
                        Slot::Cell { values, .. } => values,
                        _ => Box::new([0.0; 128]),
                    };
                    let (sx, sy, sz) = (cell.0 << 2, cell.1 << 3, cell.2 << 2);
                    let mut i = 0;
                    for ly in 0..8 {
                        for lz in 0..4 {
                            for lx in 0..4 {
                                values[i] = input.eval(c, sx + lx, sy + ly, sz + lz);
                                i += 1;
                            }
                        }
                    }
                    let v = values[idx];
                    *c.slot(*slot) = Slot::Cell { cell, values };
                    v
                }
                Cache::Interpolated => self.interpolated(c, *slot, input, x, y, z),
            },
        }
    }

    fn interpolated(&self, c: &mut Ctx, slot: usize, input: &F, x: i32, y: i32, z: i32) -> f64 {
        let cell = ((x >> 2) << 2, (y >> 3) << 3, (z >> 2) << 2);
        let idx = (((y & 7) << 4) | ((z & 3) << 2) | (x & 3)) as usize;
        if let Slot::Interp { cell: cc, values, .. } = c.slot(slot)
            && *cc == cell
        {
            return values[idx];
        }
        let (mut corners, mut values) = match std::mem::take(c.slot(slot)) {
            Slot::Interp { corners, values, .. } => (corners, values),
            _ => (FxMap::default(), Box::new([0.0; 128])),
        };
        let (cx, cy, cz) = cell;
        let mut corner = |c: &mut Ctx, x: i32, y: i32, z: i32| {
            let k = pack3(x, y, z);
            if let Some(v) = corners.get(&k) {
                return *v;
            }
            let v = input.eval(c, x, y, z);
            corners.insert(k, v);
            v
        };
        let d000 = corner(c, cx, cy, cz);
        let d100 = corner(c, cx + 4, cy, cz);
        let d010 = corner(c, cx, cy + 8, cz);
        let d110 = corner(c, cx + 4, cy + 8, cz);
        let d001 = corner(c, cx, cy, cz + 4);
        let d101 = corner(c, cx + 4, cy, cz + 4);
        let d011 = corner(c, cx, cy + 8, cz + 4);
        let d111 = corner(c, cx + 4, cy + 8, cz + 4);
        let c000 = d000;
        let c100 = d100 - d000;
        let c010 = d010 - d000;
        let c001 = d001 - d000;
        let c110 = d110 - d100 - d010 + d000;
        let c101 = d101 - d100 - d001 + d000;
        let c011 = d011 - d010 - d001 + d000;
        let c111 = d111 - d110 - d101 - d011 + d100 + d010 + d001 - d000;
        let mut i = 0;
        for ly in 0..8 {
            let ya = ly as f64 * (1.0 / 8.0);
            for lz in 0..4 {
                let za = lz as f64 * 0.25;
                let yz = ya * za;
                let zt = za * (c001 + ya * c011);
                for lx in 0..4 {
                    let xa = lx as f64 * 0.25;
                    values[i] = c000 + xa * (c100 + ya * c110 + za * c101 + yz * c111) + ya * c010 + zt;
                    i += 1;
                }
            }
        }
        let v = values[idx];
        *c.slot(slot) = Slot::Interp { corners, cell, values };
        v
    }
}

impl Spline {
    fn value(&self, i: usize, c: &mut Ctx, x: i32, y: i32, z: i32) -> f64 {
        match &self.values[i] {
            SplineValue::Const(v) => *v,
            SplineValue::Func(f) => f.eval(c, x, y, z),
        }
    }

    fn eval(&self, c: &mut Ctx, x: i32, y: i32, z: i32) -> f64 {
        let t = self.coord.eval(c, x, y, z);
        let locs = &self.locations;
        // Last index with locs[i] <= t, or -1.
        let range = locs.partition_point(|&l| l <= t) as isize - 1;
        let extend = |at: f64, value: f64, d: f64| if d == 0.0 { value } else { value + d * (t - at) };
        let last = locs.len() - 1;
        if range < 0 {
            let v = self.value(0, c, x, y, z);
            return extend(locs[0], v, self.derivatives[0]);
        }
        let r = range as usize;
        if r == last {
            let v = self.value(last, c, x, y, z);
            return extend(locs[last], v, self.derivatives[last]);
        }
        let (l0, l1) = (locs[r], locs[r + 1]);
        let dist = l1 - l0;
        let k = (t - l0) / dist;
        let y0 = self.value(r, c, x, y, z);
        let y1 = self.value(r + 1, c, x, y, z);
        let yd = y1 - y0;
        let p = self.derivatives[r] * dist - yd;
        let q = -self.derivatives[r + 1] * dist + yd;
        y0 + k * yd + k * (1.0 - k) * (p + k * (q - p))
    }
}

impl Blended {
    fn eval(&self, x: i32, y: i32, z: i32) -> f64 {
        const BASE: f64 = 684.412;
        let sxz = BASE * self.xz_scale;
        let sy = BASE * self.y_scale;
        let (mxz, my) = (sxz / self.xz_factor, sy / self.y_factor);
        let smear = sy * self.smear;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splines_ranges_and_interpolation() {
        let s = spline(&y(), &[(0.0, P::C(0.0), 0.0), (10.0, P::C(10.0), 0.0)]);
        let mut c = Ctx::default();
        assert_eq!(s.eval(&mut c, 0, 5, 0), 5.0, "cubic through equal derivatives is linear here");
        assert_eq!(s.eval(&mut c, 0, -5, 0), 0.0, "flat extension below");
        assert_eq!(s.eval(&mut c, 0, 20, 0), 10.0, "flat extension above");
        let i = interpolated(&y());
        assert_eq!(i.eval(&mut c, 1, 3, 2), 3.0, "interpolating a linear function is exact");
        let f = flat(&y());
        assert_eq!(f.eval(&mut c, 1, 50, 2), 0.0, "flat cache samples y = 0");
        let m = min(&constant(1.0), &y());
        assert_eq!(m.eval(&mut c, 0, -4, 0), -4.0);
        assert_eq!(map(&constant(-2.0), Map::QuarterNegative).eval(&mut c, 0, 0, 0), -0.5);
    }
}

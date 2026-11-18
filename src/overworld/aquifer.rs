//! Aquifers (from PNX `Aquifer`): water, lava or air where the density is empty.
//! Ore veins live here too (PNX `OreVeinifier`).

use super::density::{Ctx, FxMap};
use super::rng::{NukkitRandom, Xoroshiro, mix_seed};
use super::router::Router;

pub const SEA_LEVEL: i32 = 63;
const WAY_BELOW: i32 = -1_000_000;
const LAVA_LEVEL: i32 = -54;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Fluid {
    Air,
    Water,
    Lava,
}

#[derive(Clone, Copy, Debug)]
struct Status {
    level: i32,
    fluid: Fluid,
}

impl Status {
    fn at(self, y: i32) -> Fluid {
        if y < self.level { self.fluid } else { Fluid::Air }
    }
}

fn global(y: i32) -> Status {
    // Lava below -54, the sea above.
    if y < LAVA_LEVEL.min(SEA_LEVEL) { Status { level: LAVA_LEVEL, fluid: Fluid::Lava } } else { Status { level: SEA_LEVEL, fluid: Fluid::Water } }
}

/// What the aquifer decided for an empty (or barrier) position.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Substance {
    Fluid(Fluid),
    Stone,
}

const OFFSETS: [(i32, i32); 13] = [(0, 0), (-2, -1), (-1, -1), (0, -1), (1, -1), (-3, 0), (-2, 0), (-1, 0), (1, 0), (-2, 1), (-1, 1), (0, 1), (1, 1)];

pub struct Aquifer {
    seed: i64,
    min_grid: (i32, i32, i32),
    size_x: i32,
    size_z: i32,
    offsets: Vec<(u8, u8, u8)>,
    status: Vec<Option<Status>>,
    skip_above: i32,
    prelim_cache: FxMap<i64, i32>,
}

fn grid_x(b: i32) -> i32 {
    b >> 4
}
fn grid_y(b: i32) -> i32 {
    b.div_euclid(12)
}
fn from_xz(g: i32, o: i32) -> i32 {
    (g << 4) + o
}
fn from_y(g: i32, o: i32) -> i32 {
    g * 12 + o
}

fn similarity(d1: i32, d2: i32) -> f64 {
    1.0 - (d2 - d1) as f64 / 25.0
}

fn clamped_map(v: f64, a: f64, b: f64, oa: f64, ob: f64) -> f64 {
    if a == b {
        return if v < a { oa } else { ob };
    }
    oa + (ob - oa) * ((v - a) / (b - a)).clamp(0.0, 1.0)
}

fn lin_map(v: f64, a: f64, b: f64, oa: f64, ob: f64) -> f64 {
    if a == b { oa } else { oa + (ob - oa) * ((v - a) / (b - a)) }
}

impl Aquifer {
    pub fn new(world_seed: i64, r: &Router, c: &mut Ctx, cx: i32, cz: i32) -> Self {
        let (min_y, height) = (-64, 384);
        let (min_bx, min_bz) = (cx << 4, cz << 4);
        let min_gx = grid_x(min_bx - 5);
        let max_gx = grid_x(min_bx + 15 - 5) + 1;
        let min_gy = grid_y(min_y + 1) - 1;
        let max_gy = grid_y(min_y + height + 1) + 1;
        let min_gz = grid_z(min_bz - 5);
        let max_gz = grid_z(min_bz + 15 - 5) + 1;
        let (size_x, size_y, size_z) = (max_gx - min_gx + 1, max_gy - min_gy + 1, max_gz - min_gz + 1);
        let seed = world_seed ^ 0x4f_9939_f508;
        let mut rand = Xoroshiro::new(0);
        let mut offsets = Vec::with_capacity((size_x * size_y * size_z) as usize);
        for y in 0..size_y {
            for z in 0..size_z {
                for x in 0..size_x {
                    rand.set_seed(mix_seed(seed, min_gx + x, min_gy + y, min_gz + z));
                    let ox = rand.next_int_below(10);
                    let oy = rand.next_int_below(9);
                    let oz = rand.next_int_below(10);
                    offsets.push((ox as u8, oy as u8, oz as u8));
                }
            }
        }
        let mut a = Self {
            seed,
            min_grid: (min_gx, min_gy, min_gz),
            size_x,
            size_z,
            status: vec![None; offsets.len()],
            offsets,
            skip_above: 0,
            prelim_cache: FxMap::default(),
        };
        let max_surface = a.max_prelim(r, c, from_xz(min_gx, 0), from_xz(min_gz, 0), from_xz(max_gx, 9), from_xz(max_gz, 9)) + 8;
        a.skip_above = from_y(grid_y(max_surface + 12) + 1, 11) - 1;
        let _ = a.seed;
        a
    }

    fn index(&self, gx: i32, gy: i32, gz: i32) -> usize {
        (((gy - self.min_grid.1) * self.size_z + (gz - self.min_grid.2)) * self.size_x + (gx - self.min_grid.0)) as usize
    }

    fn location(&self, i: usize) -> (i32, i32, i32) {
        let x = i as i32 % self.size_x;
        let z = (i as i32 / self.size_x) % self.size_z;
        let y = i as i32 / (self.size_x * self.size_z);
        let (ox, oy, oz) = self.offsets[i];
        (from_xz(self.min_grid.0 + x, ox as i32), from_y(self.min_grid.1 + y, oy as i32), from_xz(self.min_grid.2 + z, oz as i32))
    }

    /// The substance for an empty spot (`density` <= 0); `None` when density says solid.
    pub fn substance(&mut self, r: &Router, c: &mut Ctx, x: i32, y: i32, z: i32, density: f64) -> Option<Substance> {
        if density > 0.0 {
            return None;
        }
        let g = global(y).at(y);
        if y > self.skip_above {
            return Some(Substance::Fluid(g));
        }
        if g == Fluid::Lava {
            return Some(Substance::Fluid(Fluid::Lava));
        }
        let (xa, ya, za) = (grid_x(x - 5), grid_y(y + 1), grid_z(z - 5));
        let mut d = [i32::MAX; 4];
        let mut idx = [0usize; 4];
        for x1 in 0..=1 {
            for y1 in -1..=1 {
                for z1 in 0..=1 {
                    let i = self.index(xa + x1, ya + y1, za + z1);
                    let (lx, ly, lz) = self.location(i);
                    let (dx, dy, dz) = (lx - x, ly - y, lz - z);
                    let nd = dx * dx + dy * dy + dz * dz;
                    if d[0] >= nd {
                        d = [nd, d[0], d[1], d[2]];
                        idx = [i, idx[0], idx[1], idx[2]];
                    } else if d[1] >= nd {
                        d = [d[0], nd, d[1], d[2]];
                        idx = [idx[0], i, idx[1], idx[2]];
                    } else if d[2] >= nd {
                        d[3] = d[2];
                        idx[3] = idx[2];
                        d[2] = nd;
                        idx[2] = i;
                    } else if d[3] >= nd {
                        d[3] = nd;
                        idx[3] = i;
                    }
                }
            }
        }
        let s1 = self.status(r, c, idx[0]);
        let sim12 = similarity(d[0], d[1]);
        let fluid = s1.at(y);
        if sim12 <= 0.0 {
            return Some(Substance::Fluid(fluid));
        }
        if fluid == Fluid::Water && global(y - 1).at(y - 1) == Fluid::Lava {
            return Some(Substance::Fluid(fluid));
        }
        let mut barrier = f64::NAN;
        let s2 = self.status(r, c, idx[1]);
        if density + sim12 * pressure(r, c, &mut barrier, x, y, z, s1, s2) > 0.0 {
            return Some(Substance::Stone);
        }
        let s3 = self.status(r, c, idx[2]);
        let sim13 = similarity(d[0], d[2]);
        if sim13 > 0.0 && density + sim12 * sim13 * pressure(r, c, &mut barrier, x, y, z, s1, s3) > 0.0 {
            return Some(Substance::Stone);
        }
        let sim23 = similarity(d[1], d[2]);
        if sim23 > 0.0 && density + sim12 * sim23 * pressure(r, c, &mut barrier, x, y, z, s2, s3) > 0.0 {
            return Some(Substance::Stone);
        }
        Some(Substance::Fluid(fluid))
    }

    fn status(&mut self, r: &Router, c: &mut Ctx, i: usize) -> Status {
        if let Some(s) = self.status[i] {
            return s;
        }
        let (x, y, z) = self.location(i);
        let s = self.compute(r, c, x, y, z);
        self.status[i] = Some(s);
        s
    }

    fn compute(&mut self, r: &Router, c: &mut Ctx, x: i32, y: i32, z: i32) -> Status {
        let g = global(y);
        let mut lowest = i32::MAX;
        let (top, bottom) = (y + 12, y - 12);
        let mut center_under = false;
        for (ox, oz) in OFFSETS {
            let (sx, sz) = (x + (ox << 4), z + (oz << 4));
            let prelim = self.prelim(r, c, sx, sz);
            let adjusted = prelim + 8;
            let start = ox == 0 && oz == 0;
            if start && bottom > adjusted {
                return g;
            }
            let pokes = top > adjusted;
            if pokes || start {
                let gs = global(adjusted);
                if gs.at(adjusted) != Fluid::Air {
                    if start {
                        center_under = true;
                    }
                    if pokes {
                        return gs;
                    }
                }
            }
            lowest = lowest.min(prelim);
        }
        let level = self.surface_level(r, c, x, y, z, g, lowest, center_under);
        let mut fluid = g.fluid;
        if level <= -10 && level != WAY_BELOW && g.fluid != Fluid::Lava {
            let v = r.lava.eval(c, x.div_euclid(64), y.div_euclid(40), z.div_euclid(64));
            if v.abs() > 0.3 {
                fluid = Fluid::Lava;
            }
        }
        Status { level, fluid }
    }

    #[allow(clippy::too_many_arguments)]
    fn surface_level(&mut self, r: &Router, c: &mut Ctx, x: i32, y: i32, z: i32, g: Status, lowest: i32, center_under: bool) -> i32 {
        let deep_dark = r.erosion.eval(c, x, y, z) < -0.225 && r.depth.eval(c, x, y, z) > 0.9;
        let (partial, full) = if deep_dark {
            (-1.0, -1.0)
        } else {
            let below = lowest + 8 - y;
            let factor = if center_under { clamped_map(below as f64, 0.0, 64.0, 1.0, 0.0) } else { 0.0 };
            let noise = r.floodedness.eval(c, x, y, z).clamp(-1.0, 1.0);
            let full_t = lin_map(factor, 1.0, 0.0, -0.3, 0.8);
            let part_t = lin_map(factor, 1.0, 0.0, -0.8, 0.4);
            (noise - part_t, noise - full_t)
        };
        if full > 0.0 {
            return g.level;
        }
        if partial > 0.0 {
            let (cx, cy, cz) = (x.div_euclid(16), y.div_euclid(40), z.div_euclid(16));
            let middle = cy * 40 + 20;
            let spread = r.spread.eval(c, cx, cy, cz);
            let quant = (spread / 3.0).floor() as i32 * 3;
            return lowest.min(middle + quant);
        }
        WAY_BELOW
    }

    /// The highest y (in steps of 8) where the rough surface density is solid.
    pub fn prelim(&mut self, r: &Router, c: &mut Ctx, x: i32, z: i32) -> i32 {
        let k = ((x as i64) << 32) ^ (z as u32 as i64);
        if let Some(v) = self.prelim_cache.get(&k) {
            return *v;
        }
        let v = prelim_surface(r, c, x, z);
        self.prelim_cache.insert(k, v);
        v
    }

    fn max_prelim(&mut self, r: &Router, c: &mut Ctx, min_x: i32, min_z: i32, max_x: i32, max_z: i32) -> i32 {
        let mut m = i32::MIN;
        let mut x = min_x;
        while x <= max_x {
            let mut z = min_z;
            while z <= max_z {
                m = m.max(self.prelim(r, c, x, z));
                z += 4;
            }
            x += 4;
        }
        if m == i32::MIN { 63 } else { m }
    }
}

fn grid_z(b: i32) -> i32 {
    b >> 4
}

pub fn prelim_surface(r: &Router, c: &mut Ctx, x: i32, z: i32) -> i32 {
    let lower = -64;
    let upper = r.prelim_upper.eval(c, x, 0, z).floor() as i32;
    let upper = 319.min(upper.div_euclid(8) * 8);
    if upper > lower {
        let mut y = upper;
        while y >= lower {
            if r.prelim_surface.eval(c, x, y, z) > 0.0 {
                return y;
            }
            y -= 8;
        }
    }
    lower
}

#[allow(clippy::too_many_arguments)]
fn pressure(r: &Router, c: &mut Ctx, barrier: &mut f64, x: i32, y: i32, z: i32, s1: Status, s2: Status) -> f64 {
    let (t1, t2) = (s1.at(y), s2.at(y));
    if (t1 == Fluid::Lava && t2 == Fluid::Water) || (t1 == Fluid::Water && t2 == Fluid::Lava) {
        return 2.0;
    }
    let diff = (s1.level - s2.level).abs();
    if diff == 0 {
        return 0.0;
    }
    let avg = 0.5 * (s1.level + s2.level) as f64;
    let above = y as f64 + 0.5 - avg;
    let base = diff as f64 / 2.0;
    let toward_middle = base - above.abs();
    let gradient = if above > 0.0 {
        let cp = toward_middle;
        if cp > 0.0 { cp / 1.5 } else { cp / 2.5 }
    } else {
        let cp = 3.0 + toward_middle;
        if cp > 0.0 { cp / 3.0 } else { cp / 10.0 }
    };
    let noise = if (-2.0..=2.0).contains(&gradient) {
        if barrier.is_nan() {
            *barrier = r.barrier.eval(c, x, y, z);
        }
        *barrier
    } else {
        0.0
    };
    2.0 * (noise + gradient)
}

/// Big copper/granite and iron/tuff ore veins.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Vein {
    CopperOre,
    RawCopperBlock,
    Granite,
    DeepslateIronOre,
    RawIronBlock,
    Tuff,
}

pub fn ore_vein(r: &Router, c: &mut Ctx, rand: &mut NukkitRandom, x: i32, y: i32, z: i32) -> Option<Vein> {
    if !(-60..=50).contains(&y) {
        return None;
    }
    let toggle = r.vein_toggle.eval(c, x, y, z);
    let copper = toggle > 0.0;
    let (lo, hi) = if copper { (0, 50) } else { (-60, -8) };
    let ridged = toggle.abs();
    let (from_top, from_bottom) = (hi - y, y - lo);
    if from_bottom < 0 || from_top < 0 {
        return None;
    }
    let edge = from_top.min(from_bottom) as f64;
    let roundoff = -0.2 + 0.2 * (edge / 20.0).clamp(0.0, 1.0);
    if ridged + roundoff < 0.4f32 as f64 {
        return None;
    }
    if r.vein_ridged.eval(c, x, y, z) >= 0.0 {
        return None;
    }
    rand.set_seed(mix_seed(r.ore_vein_seed, x, y, z));
    if rand.next_float() > 0.7 {
        return None;
    }
    let (a, b) = (0.4f32 as f64, 0.6f32 as f64);
    let (ra, rb) = (0.1f32 as f64, 0.3f32 as f64);
    let richness = ra + (rb - ra) * ((ridged - a) / (b - a)).clamp(0.0, 1.0);
    if (rand.next_float() as f64) < richness && r.vein_gap.eval(c, x, y, z) > -0.3f32 as f64 {
        let raw = rand.next_float() < 0.02;
        return Some(match (copper, raw) {
            (true, true) => Vein::RawCopperBlock,
            (true, false) => Vein::CopperOre,
            (false, true) => Vein::RawIronBlock,
            (false, false) => Vein::DeepslateIronOre,
        });
    }
    Some(if copper { Vein::Granite } else { Vein::Tuff })
}

// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Ice plains spikes (spikes, packed-ice patches) and frozen-ocean icebergs.

use std::collections::{HashMap, HashSet};
use std::f64::consts::PI;

use super::super::blocks;
use super::super::{chunk_hash, java_hash, Ctx, FeatureFn, NukkitRandom, Region};
use super::placement::{MAX_Y, MIN_Y};
use super::{biome, count, SEA_LEVEL};

fn name(r: &Region, x: i32, y: i32, z: i32) -> &'static str {
    blocks::short(r.get(x, y, z))
}

/// `IcePatchFeature`.
fn ice_patch(r: &mut Region, ctx: &mut Ctx) {
    let n = count(ctx, "minecraft:ice_patch_feature", 1, 2);
    let packed = blocks::id("packed_ice");
    for _ in 0..n {
        let lx = 3 + ctx.random.next_int_below(10);
        let lz = 3 + ctx.random.next_int_below(10);
        let (sx, sz) = ((ctx.cx << 4) + lx, (ctx.cz << 4) + lz);
        let mut sy = r.height(sx, sz) - 1;
        if r.biome(sx, sy, sz) != biome::ICE_PLAINS_SPIKES {
            continue;
        }
        while sy > MIN_Y + 1 && blocks::is_air(r.get(sx, sy, sz)) {
            sy -= 1;
        }
        let radius = 2 + ctx.random.next_int_below(3);
        for x in sx - radius..=sx + radius {
            for z in sz - radius..=sz + radius {
                if (x - sx) * (x - sx) + (z - sz) * (z - sz) > radius * radius {
                    continue;
                }
                for y in (sy - 2..=sy + 1).rev() {
                    let id = name(r, x, y, z);
                    if matches!(id, "snow" | "snow_layer" | "ice" | "packed_ice" | "dirt" | "grass_block") {
                        r.set(x, y, z, packed);
                    } else if id != "air" {
                        break;
                    }
                }
            }
        }
    }
}

fn spike_replace(id: &str) -> bool {
    matches!(id, "snow" | "snow_layer" | "ice" | "air" | "dirt" | "grass_block")
}

/// `IceSpikeFeature`.
fn ice_spike(r: &mut Region, ctx: &mut Ctx) {
    super::placement::reseed(ctx, "minecraft:ice_spike_feature");
    let rand = &mut ctx.random;
    let packed = blocks::id("packed_ice");
    let attempts = 3 + rand.next_int_below(3);
    for _ in 0..attempts {
        let ox = (ctx.cx << 4) + rand.next_int_below(16);
        let oz = (ctx.cz << 4) + rand.next_int_below(16);
        let mut oy = r.height(ox, oz);
        while blocks::is_air(r.get(ox, oy, oz)) && oy > MIN_Y + 2 {
            oy -= 1;
        }
        if r.biome(ox, oy, oz) != biome::ICE_PLAINS_SPIKES || !matches!(name(r, ox, oy, oz), "snow" | "snow_layer" | "dirt" | "grass_block" | "ice" | "packed_ice") {
            continue;
        }
        let height = rand.next_int_below(4) + 7;
        let width = height / 4 + rand.next_int_below(2);
        if width > 1 && rand.next_int_below(60) == 0 {
            oy += 10 + rand.next_int_below(30);
        }
        for yo in 0..height {
            let scale = (1.0f32 - yo as f32 / height as f32) * width as f32;
            let nw = scale.ceil() as i32;
            for xo in -nw..=nw {
                let dx = xo.abs() as f32 - 0.25;
                for zo in -nw..=nw {
                    let dz = zo.abs() as f32 - 0.25;
                    let inside = xo == 0 && zo == 0 || dx * dx + dz * dz <= scale * scale;
                    if inside && (xo != -nw && xo != nw && zo != -nw && zo != nw || rand.next_float() <= 0.75) {
                        let (px, py, pz) = (ox + xo, oy + yo, oz + zo);
                        if spike_replace(name(r, px, py, pz)) {
                            r.set(px, py, pz, packed);
                        }
                        if yo != 0 && nw > 1 {
                            let ny = oy - yo;
                            if spike_replace(name(r, px, ny, pz)) {
                                r.set(px, ny, pz, packed);
                            }
                        }
                    }
                }
            }
        }
        let pw = (width - 1).clamp(0, 1);
        for xo in -pw..=pw {
            for zo in -pw..=pw {
                let (cx, cz) = (ox + xo, oz + zo);
                let mut cy = oy - 1;
                let mut run = if xo.abs() == 1 && zo.abs() == 1 { rand.next_int_below(5) } else { 50 };
                while cy > 50 {
                    let id = name(r, cx, cy, cz);
                    if !spike_replace(id) && id != "packed_ice" {
                        break;
                    }
                    r.set(cx, cy, cz, packed);
                    cy -= 1;
                    run -= 1;
                    if run <= 0 {
                        cy -= rand.next_int_below(5) + 1;
                        run = rand.next_int_below(5);
                    }
                }
            }
        }
    }
}

const ICEBERG: &str = "minecraft:frozen_ocean_after_surface_ice_feature";

/// `IcebergFeature` (driven by its own NukkitRandom).
struct Iceberg<'a> {
    r: &'a mut Region,
    rand: NukkitRandom,
    mask: HashSet<(i32, i32, i32)>,
    packed: i32,
    snow: i32,
    blue: i32,
}

impl Iceberg<'_> {
    fn ri(&mut self, bound: i32) -> i32 {
        if bound <= 1 { 0 } else { self.rand.next_range(0, bound - 1) }
    }
    fn id(&self, x: i32, y: i32, z: i32) -> &'static str {
        name(self.r, x, y, z)
    }
    fn set(&mut self, x: i32, y: i32, z: i32, b: i32) {
        if (MIN_Y..=MAX_Y).contains(&y) {
            self.r.set(x, y, z, b);
        }
    }

    fn radius_round(&mut self, yo: i32, height: i32, width: i32) -> i32 {
        let k = 3.5f32 - self.rand.next_float();
        let mut scale = (1.0f32 - (yo as f64).powi(2) as f32 / (height as f32 * k)) * width as f32;
        if height > 15 + self.ri(5) {
            let t = if yo < 3 + self.ri(6) { yo / 2 } else { yo };
            scale = (1.0f32 - t as f32 / (height as f32 * k * 0.4)) * width as f32;
        }
        (scale / 2.0).ceil() as i32
    }
    fn radius_ellipse(yo: i32, height: i32, width: i32) -> i32 {
        let scale = (1.0f32 - (yo as f64).powi(2) as f32 / height as f32) * width as f32;
        (scale / 2.0).ceil() as i32
    }
    fn radius_steep(&mut self, yo: i32, height: i32, width: i32) -> i32 {
        let k = 1.0f32 + self.rand.next_float() / 2.0;
        let scale = (1.0f32 - yo as f32 / (height as f32 * k)) * width as f32;
        (scale / 2.0).ceil() as i32
    }
    fn dist_circle(&mut self, xo: i32, zo: i32, radius: i32) -> f64 {
        let off = 10.0f32 * self.rand.next_float().clamp(0.2, 0.8) / radius.max(1) as f32;
        off as f64 + (xo as f64).powi(2) + (zo as f64).powi(2) - (radius as f64).powi(2)
    }
    fn dist_ellipse(xo: i32, zo: i32, ox: i32, oz: i32, a: i32, c: i32, angle: f64) -> f64 {
        let (dx, dz) = ((xo - ox) as f64, (zo - oz) as f64);
        ((dx * angle.cos() - dz * angle.sin()) / a.max(1) as f64).powi(2) + ((dx * angle.sin() + dz * angle.cos()) / c.max(1) as f64).powi(2) - 1.0
    }

    #[allow(clippy::too_many_arguments)]
    fn block(&mut self, o: (i32, i32, i32), height: i32, xo: i32, yo: i32, zo: i32, radius: i32, a: i32, ellipse: bool, ec: i32, angle: f64, snow_top: bool) {
        let d = if ellipse {
            let c = if yo > 0 && height - yo <= 3 { ec - (4 - (height - yo)) } else { ec };
            Self::dist_ellipse(xo, zo, 0, 0, a, c, angle)
        } else {
            self.dist_circle(xo, zo, radius)
        };
        if d >= 0.0 {
            return;
        }
        let (x, y, z) = (o.0 + xo, o.1 + yo, o.2 + zo);
        let cmp = if ellipse { -0.5 } else { (-6 - self.ri(3)) as f64 };
        if d > cmp && self.rand.next_double() > 0.9 {
            return;
        }
        let h_diff = height - yo;
        let state = self.id(x, y, z);
        if matches!(state, "air" | "snow" | "ice" | "water" | "flowing_water") {
            let randomness = !ellipse || self.rand.next_double() > 0.05;
            let divisor = if ellipse { 3 } else { 2 };
            let water = matches!(state, "water" | "flowing_water");
            let b = if snow_top && !water && h_diff <= self.ri(1.max(height / divisor)) + (height as f32 * 0.6) as i32 && randomness { self.snow } else { self.packed };
            self.set(x, y, z, b);
            self.mask.insert((x, y, z));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn carve(&mut self, radius: i32, yo: i32, o: (i32, i32, i32), under: bool, angle: f64, lox: i32, loz: i32, ea: i32, ec: i32) {
        let a = radius + 1 + ea / 3;
        let c = (radius - 3).min(3) + ec / 2 - 1;
        if c <= 0 {
            return;
        }
        let (air, water) = (blocks::air(), super::water());
        for xo in -a..a {
            for zo in -a..a {
                if Self::dist_ellipse(xo, zo, lox, loz, a, c, angle) >= 0.0 {
                    continue;
                }
                let (x, y, z) = (o.0 + xo, o.1 + yo, o.2 + zo);
                if matches!(self.id(x, y, z), "packed_ice" | "snow" | "blue_ice") {
                    if under {
                        self.set(x, y, z, water);
                    } else {
                        self.set(x, y, z, air);
                        if self.id(x, y + 1, z) == "snow_layer" {
                            self.set(x, y + 1, z, air);
                        }
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn cut_out(&mut self, width: i32, height: i32, o: (i32, i32, i32), ellipse: bool, ea: i32, angle0: f64, ec: i32) {
        let sx = if self.rand.next_boolean() { -1 } else { 1 };
        let sz = if self.rand.next_boolean() { -1 } else { 1 };
        let mut xoff = self.ri((width / 2 - 2).max(1));
        if self.rand.next_boolean() {
            xoff = width / 2 + 1 - self.ri((width - width / 2 - 1).max(1));
        }
        let mut zoff = self.ri((width / 2 - 2).max(1));
        if self.rand.next_boolean() {
            zoff = width / 2 + 1 - self.ri((width - width / 2 - 1).max(1));
        }
        if ellipse {
            xoff = self.ri((ea - 5).max(1));
            zoff = xoff;
        }
        let (lox, loz) = (sx * xoff, sz * zoff);
        let angle = if ellipse { angle0 + PI / 2.0 } else { self.rand.next_double() * 2.0 * PI };
        for yo in 0..height - 3 {
            let radius = self.radius_round(yo, height, width);
            self.carve(radius, yo, o, false, angle, lox, loz, ea, ec);
        }
        let mut yo = -1;
        // Java re-evaluates the bound (and its random) every iteration.
        while yo > -height + self.ri(5) {
            let radius = self.radius_steep(-yo, height, width);
            self.carve(radius, yo, o, true, angle, lox, loz, ea, ec);
            yo -= 1;
        }
    }

    fn blue_ice(&mut self, ox: i32, oz: i32) {
        let x = ox + self.ri(8) - self.ri(8);
        let z = oz + self.ri(8) - self.ri(8);
        let y = SEA_LEVEL - 2;
        let w = |s: &str| matches!(s, "water" | "flowing_water");
        if !w(self.id(x, y, z)) && !w(self.id(x, y - 1, z)) {
            return;
        }
        if ![(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, 1, 0)].iter().any(|(dx, dy, dz)| self.id(x + dx, y + dy, z + dz) == "packed_ice") {
            return;
        }
        self.set(x, y, z, self.blue);
        for _ in 0..200 {
            let yo = self.ri(5) - self.ri(6);
            let mut xz = 3;
            if yo < 2 {
                xz += yo / 2;
            }
            if xz < 1 {
                continue;
            }
            let px = x + self.ri(xz) - self.ri(xz);
            let py = y + yo;
            let pz = z + self.ri(xz) - self.ri(xz);
            let st = self.id(px, py, pz);
            if (st == "air" || w(st) || st == "packed_ice" || st == "ice")
                && [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, 1, 0), (0, -1, 0)].iter().any(|(dx, dy, dz)| self.id(px + dx, py + dy, pz + dz) == "blue_ice")
            {
                self.set(px, py, pz, self.blue);
            }
        }
    }

    fn smooth(&mut self, o: (i32, i32, i32), width: i32, height: i32, ellipse: bool, ea: i32) {
        let a = if ellipse { ea } else { width / 2 };
        let air = blocks::air();
        for _ in 0..3 {
            let mut remove = HashSet::new();
            for xo in -a..=a {
                for zo in -a..=a {
                    for yo in 0..=height {
                        let (x, y, z) = (o.0 + xo, o.1 + yo, o.2 + zo);
                        if !self.mask.contains(&(x, y, z)) {
                            continue;
                        }
                        if !self.mask.contains(&(x, y - 1, z)) {
                            remove.insert((x, y, z));
                            if self.mask.contains(&(x, y + 1, z)) {
                                remove.insert((x, y + 1, z));
                            }
                        } else {
                            let open = [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().filter(|(dx, dz)| !self.mask.contains(&(x + dx, y, z + dz))).count();
                            if open >= 3 {
                                remove.insert((x, y, z));
                            }
                        }
                    }
                }
            }
            if remove.is_empty() {
                break;
            }
            for p in remove {
                self.set(p.0, p.1, p.2, air);
                self.mask.remove(&p);
            }
        }
    }
}

fn iceberg(r: &mut Region, ctx: &mut Ctx) {
    let mut g = Iceberg {
        rand: NukkitRandom::new(ctx.seed ^ chunk_hash(ctx.cx, ctx.cz) ^ java_hash(ICEBERG) as i64),
        r,
        mask: HashSet::new(),
        packed: blocks::id("packed_ice"),
        snow: blocks::id("snow"),
        blue: blocks::id("blue_ice"),
    };
    let ox = (ctx.cx << 4) + g.ri(16);
    let oz = (ctx.cz << 4) + g.ri(16);
    if !matches!(g.r.biome(ox, SEA_LEVEL, oz), biome::FROZEN_OCEAN | biome::DEEP_FROZEN_OCEAN | biome::LEGACY_FROZEN_OCEAN) || g.ri(8) != 0 {
        return;
    }
    let snow_top = g.rand.next_double() > 0.7;
    let angle = g.rand.next_double() * 2.0 * PI;
    let ea = 11 - g.ri(5);
    let ec = 3 + g.ri(3);
    let ellipse = g.rand.next_double() > 0.7;
    let mut over = if ellipse { g.ri(6) + 6 } else { g.ri(15) + 3 };
    if !ellipse && g.rand.next_double() > 0.9 {
        over += g.ri(19) + 7;
    }
    let under = (over + g.ri(11)).min(18);
    let width = (over + g.ri(7) - g.ri(5)).min(11);
    let a = if ellipse { ea } else { 11 };
    let o = (ox, SEA_LEVEL, oz);
    for xo in -a..a {
        for zo in -a..a {
            for yo in 0..over {
                let radius = if ellipse { Iceberg::radius_ellipse(yo, over, width) } else { g.radius_round(yo, over, width) };
                if ellipse || xo < radius {
                    g.block(o, over, xo, yo, zo, radius, a, ellipse, ec, angle, snow_top);
                }
            }
        }
    }
    for xo in -a..a {
        for zo in -a..a {
            for yo in (-under + 1..=-1).rev() {
                let na = if ellipse { (a as f32 * (1.0 - (yo as f64).powi(2) as f32 / (under as f32 * 8.0))).ceil() as i32 } else { a };
                let radius = g.radius_steep(-yo, under, width);
                if xo < radius {
                    g.block(o, under, xo, yo, zo, radius, na, ellipse, ec, angle, snow_top);
                }
            }
        }
    }
    g.smooth(o, width, over, ellipse, ea);
    let cut = if ellipse { g.rand.next_double() > 0.1 } else { g.rand.next_double() > 0.7 };
    if cut {
        g.cut_out(width, over, o, ellipse, ea, angle, ec);
    }
    g.blue_ice(ox, oz);
}

pub(super) fn register(m: &mut HashMap<&'static str, FeatureFn>) {
    m.insert("minecraft:ice_patch_feature", ice_patch);
    m.insert("minecraft:ice_spike_feature", ice_spike);
    m.insert(ICEBERG, iceberg);
}

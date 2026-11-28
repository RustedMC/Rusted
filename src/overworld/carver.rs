// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Carver caves: PNX `OverworldCaveCarverFeature` (pregeneration pass), which runs
//! `CaveGenerateFeature`, `CaveExtraUndergroundFeature` and `CanyonCarverFeature` on each chunk
//! right after its terrain, before any decoration. The fourth part, dungeons, needs the
//! neighbouring chunks and runs at the start of decoration (feature::dungeon).
//! Java float/double mixing is kept exactly as PNX has it.

use super::rng::{NukkitRandom, Xoroshiro};
use super::{ids, Chunk};
use crate::overworld::feature::objects::{floor, mh_cos, mh_sin};
use std::collections::HashSet;
use std::f64::consts::PI;
use std::sync::OnceLock;

const CHUNK: i32 = 16;
const MIN_Y: i32 = -64;
const MAX_Y: i32 = 320;
const LAVA_LEVEL_OFFSET: i32 = 8;
const RANGE: i32 = 8;

/// Biomes whose definition runs `minecraft:overworld_cave_carver_feature` in the
/// pregeneration pass.
fn carver_biomes() -> &'static HashSet<u16> {
    static B: OnceLock<HashSet<u16>> = OnceLock::new();
    B.get_or_init(|| {
        let mut out = HashSet::new();
        let Some(batch) = crate::vanilla::batches().nth(2) else { return out };
        let decoded = bedrock::network::codec::decode_packets_each::<crate::Protocol>(batch, None, None, usize::MAX, bedrock::network::codec::UnreadBytes::Warn);
        let Some(crate::Protocol::BiomeDefinitionListPacket(p)) = decoded.ok().and_then(|d| d.into_iter().next()).and_then(|(_, p)| p.ok()) else {
            return out;
        };
        let s = |i: u16| p.strings.get(i as usize).cloned().unwrap_or_default();
        for e in &p.biomes {
            let Some(id) = super::biome_id(s(e.name_index).trim_start_matches("minecraft:")) else { continue };
            let Some(list) = e.definition.chunk_gen_data.as_ref().and_then(|g| g.consolidated_features.as_ref()) else { continue };
            if list.entries.iter().any(|f| s(f.pass) == "pregeneration_pass" && s(f.identifier) == "minecraft:overworld_cave_carver_feature") {
                out.insert(id);
            }
        }
        out
    })
}

/// Whether a biome runs the carver (dungeons go with it).
pub(crate) fn carver_biome(b: u16) -> bool {
    carver_biomes().contains(&b)
}

/// Whether any biome in the chunk (sampled like the feature stage) runs the carver.
fn wants_carver(ch: &Chunk) -> bool {
    let set = carver_biomes();
    (0..16).any(|x| {
        (0..16).any(|z| {
            let h = ch.heights[(x * 16 + z) as usize];
            set.contains(&ch.biome(x, h - 1, z)) || (h - 27 > MIN_Y && set.contains(&ch.biome(x, h - 27, z))) || (h - 128 > MIN_Y && set.contains(&ch.biome(x, h - 128, z)))
        })
    })
}

/// The two RNGs PNX hands the carving code: the feature's Xoroshiro for the chunk-level
/// rolls, NukkitRandom (Mersenne Twister) for each tunnel.
trait Rand {
    fn float(&mut self) -> f32;
    /// PNX `nextInt(n)` of this generator.
    fn int(&mut self, n: i32) -> i32;
    fn long(&mut self) -> i64;
}

impl Rand for Xoroshiro {
    fn float(&mut self) -> f32 {
        self.next_float()
    }
    fn int(&mut self, n: i32) -> i32 {
        self.next_int_below(n)
    }
    fn long(&mut self) -> i64 {
        self.next_long()
    }
}

impl Rand for NukkitRandom {
    fn float(&mut self) -> f32 {
        self.next_float()
    }
    /// NukkitRandom.nextInt(max) is 0..=max.
    fn int(&mut self, n: i32) -> i32 {
        self.next_bounded_int(n)
    }
    fn long(&mut self) -> i64 {
        self.next_long()
    }
}

#[derive(Clone, Copy)]
struct Cave {
    probability: f32,
    bound: i32,
    max_y: i32,
}

const CAVES: Cave = Cave { probability: 0.15, bound: 15, max_y: 180 };
const EXTRA_UNDERGROUND: Cave = Cave { probability: 0.07, bound: 15, max_y: 47 };

/// Carves chunk (cx, cz), if its biomes ask for it.
pub fn carve(ch: &mut Chunk, cx: i32, cz: i32, seed: i64) {
    if !wants_carver(ch) {
        return;
    }
    let mut c = Carver { ch, cx, cz };
    let mut random = Xoroshiro::new(0);
    for cave in [Some(CAVES), Some(EXTRA_UNDERGROUND), None] {
        // CaveGenerateFeature.apply (the canyon carver shares it)
        random.set_seed(seed);
        let x_seed = random.next_long();
        let z_seed = random.next_long();
        for sx in cx - RANGE..=cx + RANGE {
            for sz in cz - RANGE..=cz + RANGE {
                random.set_seed((sx as i64).wrapping_mul(x_seed) ^ (sz as i64).wrapping_mul(z_seed) ^ seed);
                match cave {
                    Some(cave) => c.carve_chunk(&mut random, cave, sx, sz),
                    None => c.canyon_chunk(&mut random, sx, sz),
                }
            }
        }
    }
}

struct Carver<'a> {
    ch: &'a mut Chunk,
    cx: i32,
    cz: i32,
}

impl Carver<'_> {
    fn carve_chunk(&mut self, random: &mut Xoroshiro, cave: Cave, sx: i32, sz: i32) {
        if random.float() > cave.probability {
            return;
        }
        let cave_min_y = (MIN_Y + LAVA_LEVEL_OFFSET).clamp(MIN_Y + 1, MAX_Y - 1);
        let cave_max_y = cave.max_y.clamp(cave_min_y, MAX_Y - 1);
        let lava = MIN_Y + LAVA_LEVEL_OFFSET;
        let max_distance = RANGE * CHUNK - CHUNK;
        let inner = random.int(cave.bound) + 1;
        let inner = random.int(inner) + 1;
        let count = random.int(inner);
        for _ in 0..count {
            let x = (sx * CHUNK + random.int(CHUNK)) as f64;
            let y = random.next_int_range(cave_min_y, cave_max_y + 1) as f64;
            let z = (sz * CHUNK + random.int(CHUNK)) as f64;
            let h_mul = 0.7f64 + random.float() as f64 * 0.7f64;
            let v_mul = 0.8f64 + random.float() as f64 * 0.5f64;
            let floor_level = -1.0f64 + random.float() as f64 * 0.6f64;
            let mut tunnels = 1;
            if random.int(4) == 0 {
                let y_scale = 0.1f64 + random.float() as f64 * 0.8f64;
                let thickness = 1.0f32 + random.float() * 6.0f32;
                // createRoom
                let h = 1.5f64 + (mh_sin((PI / 2.0) as f32) * thickness) as f64;
                self.ellipsoid(x + 1.0, y, z, h, h * y_scale, floor_level, lava);
                tunnels += random.int(4);
            }
            for _ in 0..tunnels {
                let yaw = random.float() * (std::f32::consts::PI * 2.0f32);
                let pitch = (random.float() - 0.5f32) / 4.0f32;
                // getThickness
                let mut thickness = random.float() * 2.0f32 + random.float();
                if random.int(10) == 0 {
                    thickness *= random.float() * random.float() * 3.0f32 + 1.0f32;
                }
                let distance = max_distance - random.int(max_distance / 4);
                let tunnel_seed = random.long();
                let t = Tunnel { h_mul, v_mul, floor_level, lava };
                self.tunnel(tunnel_seed, (x, y, z), &t, thickness, yaw, pitch, 0, distance, 1.0);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn tunnel(&mut self, seed: i64, (mut x, mut y, mut z): (f64, f64, f64), t: &Tunnel, thickness: f32, mut yaw: f32, mut pitch: f32, step: i32, dist: i32, y_scale: f64) {
        let mut random = NukkitRandom::new(seed);
        let split = random.int(dist / 2) + dist / 4;
        let steep = random.int(6) == 0;
        let (mut y_rota, mut x_rota) = (0.0f32, 0.0f32);
        let (centre_x, centre_z) = ((self.cx * CHUNK + 8) as f64, (self.cz * CHUNK + 8) as f64);
        for current in step..dist {
            let h = 1.5f64 + (mh_sin(std::f32::consts::PI * current as f32 / dist as f32) * thickness) as f64;
            let v = h * y_scale;
            let cos_x = mh_cos(pitch);
            x += (mh_cos(yaw) * cos_x) as f64;
            y += mh_sin(pitch) as f64;
            z += (mh_sin(yaw) * cos_x) as f64;
            pitch *= if steep { 0.92f32 } else { 0.7f32 };
            pitch += x_rota * 0.1f32;
            yaw += y_rota * 0.1f32;
            x_rota *= 0.9f32;
            y_rota *= 0.75f32;
            x_rota += (random.float() - random.float()) * random.float() * 2.0f32;
            y_rota += (random.float() - random.float()) * random.float() * 4.0f32;
            if current == split && thickness > 1.0f32 {
                let half_pi = (PI / 2.0) as f32;
                let (s1, th1) = (random.long(), random.float() * 0.5f32 + 0.5f32);
                self.tunnel(s1, (x, y, z), t, th1, yaw - half_pi, pitch / 3.0f32, current, dist, 1.0);
                let (s2, th2) = (random.long(), random.float() * 0.5f32 + 0.5f32);
                self.tunnel(s2, (x, y, z), t, th2, yaw + half_pi, pitch / 3.0f32, current, dist, 1.0);
                return;
            }
            if random.int(4) != 0 {
                if !can_reach(centre_x, centre_z, x, z, current, dist, thickness) {
                    return;
                }
                self.ellipsoid(x, y, z, h * t.h_mul, v * t.v_mul, t.floor_level, t.lava);
            }
        }
    }

    /// Clamped local box for an ellipsoid at (x, y, z), or None if it misses this chunk.
    fn bounds(&self, x: f64, y: f64, z: f64, h: f64, v: f64) -> Option<(i32, i32, i32, i32, i32, i32)> {
        let (bx, bz) = (self.cx * CHUNK, self.cz * CHUNK);
        let x_from = (floor(x - h) - bx - 1).max(0);
        let x_to = (floor(x + h) - bx + 1).min(CHUNK);
        let y_from = (floor(y - v) - 1).max(MIN_Y + 1);
        let y_to = (floor(y + v) + 1).min(MAX_Y - 1);
        let z_from = (floor(z - h) - bz - 1).max(0);
        let z_to = (floor(z + h) - bz + 1).min(CHUNK);
        (x_from < x_to && z_from < z_to && y_from < y_to).then_some((x_from, x_to, y_from, y_to, z_from, z_to))
    }

    /// CaveGenerateFeature.carveEllipsoid.
    #[allow(clippy::too_many_arguments)]
    fn ellipsoid(&mut self, x: f64, y: f64, z: f64, h: f64, v: f64, floor_level: f64, lava: i32) {
        let Some((x_from, x_to, y_from, y_to, z_from, z_to)) = self.bounds(x, y, z, h, v) else { return };
        if self.has_liquid(x_from, x_to, y_from, y_to, z_from, z_to) {
            return;
        }
        let (bx, bz) = (self.cx * CHUNK, self.cz * CHUNK);
        let (inv_h, inv_v) = (1.0 / h, 1.0 / v);
        for xx in x_from..x_to {
            let xd = (xx as f64 + bx as f64 + 0.5 - x) * inv_h;
            for zz in z_from..z_to {
                let zd = (zz as f64 + bz as f64 + 0.5 - z) * inv_h;
                let hsq = xd * xd + zd * zd;
                if hsq >= 1.0 {
                    continue;
                }
                let mut grass = false;
                for yy in (y_from + 1..=y_to).rev() {
                    let yd = (yy as f64 - 0.5 - y) * inv_v;
                    if yd <= floor_level || hsq + yd * yd >= 1.0 {
                        continue;
                    }
                    self.clear(xx, yy, zz, lava, &mut grass);
                }
            }
        }
    }

    fn canyon_chunk(&mut self, random: &mut Xoroshiro, sx: i32, sz: i32) {
        if random.float() > 0.01f32 {
            return;
        }
        let lava = MIN_Y + LAVA_LEVEL_OFFSET;
        let x = (sx * CHUNK + random.int(CHUNK)) as f64;
        let y = (10 + random.int(67 - 10 + 1)).clamp(MIN_Y + 1, MAX_Y - 1) as f64;
        let z = (sz * CHUNK + random.int(CHUNK)) as f64;
        let yaw = random.float() * (std::f32::consts::PI * 2.0f32);
        let pitch = random.float() * 0.25f32 - 0.125f32;
        let thickness = 2.0f32 + random.float() * 4.0f32;
        let max_distance = RANGE * CHUNK - CHUNK;
        let distance = (max_distance as f32 * (0.75f32 + random.float() * 0.25f32)) as i32;
        self.canyon(random, (x, y, z), thickness, yaw, pitch, distance, lava);
    }

    #[allow(clippy::too_many_arguments)]
    fn canyon(&mut self, random: &mut Xoroshiro, (mut x, mut y, mut z): (f64, f64, f64), thickness: f32, mut yaw: f32, mut pitch: f32, distance: i32, lava: i32) {
        // initWidthFactors
        let depth = (MAX_Y - MIN_Y + 1) as usize;
        let mut widths = vec![0f32; depth];
        let mut w = 1.0f32;
        for (i, slot) in widths.iter_mut().enumerate() {
            if i == 0 || random.int(3) == 0 {
                w = 1.0f32 + random.float() * random.float();
            }
            *slot = w * w;
        }
        let (mut x_rota, mut y_rota) = (0.0f32, 0.0f32);
        let (centre_x, centre_z) = ((self.cx * CHUNK + 8) as f64, (self.cz * CHUNK + 8) as f64);
        for current in 0..distance {
            let mut h = 1.5f64 + (mh_sin(current as f32 * std::f32::consts::PI / distance as f32) * thickness) as f64;
            // updateVerticalRadius (its factor is always 1)
            let v = 1.0f32 as f64 * (h * 3.0f32 as f64) * (0.75f32 + random.float() * 0.25f32) as f64;
            h *= (0.75f32 + random.float() * 0.25f32) as f64;
            let xz_cos = mh_cos(pitch);
            let y_sin = mh_sin(pitch);
            x += (mh_cos(yaw) * xz_cos) as f64;
            y += y_sin as f64;
            z += (mh_sin(yaw) * xz_cos) as f64;
            pitch *= 0.7f32;
            pitch += x_rota * 0.05f32;
            yaw += y_rota * 0.05f32;
            x_rota *= 0.8f32;
            y_rota *= 0.5f32;
            x_rota += (random.float() - random.float()) * random.float() * 2.0f32;
            y_rota += (random.float() - random.float()) * random.float() * 4.0f32;
            if random.int(4) == 0 {
                continue;
            }
            if !can_reach(centre_x, centre_z, x, z, current, distance, thickness) {
                return;
            }
            let Some((x_from, x_to, y_from, y_to, z_from, z_to)) = self.bounds(x, y, z, h, v) else { continue };
            if self.has_liquid(x_from, x_to, y_from, y_to, z_from, z_to) {
                continue;
            }
            let (bx, bz) = (self.cx * CHUNK, self.cz * CHUNK);
            let (inv_h, inv_v) = (1.0 / h, 1.0 / v);
            for xx in x_from..x_to {
                let xd = (xx as f64 + bx as f64 + 0.5 - x) * inv_h;
                for zz in z_from..z_to {
                    let zd = (zz as f64 + bz as f64 + 0.5 - z) * inv_h;
                    let hsq = xd * xd + zd * zd;
                    if hsq >= 1.0 {
                        continue;
                    }
                    let mut grass = false;
                    for yy in (y_from + 1..=y_to).rev() {
                        let yi = (yy - MIN_Y) as usize;
                        if yi == 0 || yi >= widths.len() {
                            continue;
                        }
                        let yd = (yy as f64 - 0.5 - y) * inv_v;
                        if hsq * widths[yi - 1] as f64 + yd * yd / 6.0 >= 1.0 {
                            continue;
                        }
                        self.clear(xx, yy, zz, lava, &mut grass);
                    }
                }
            }
        }
    }

    /// One carved block: lava at the bottom, air above, grass put back on exposed dirt.
    fn clear(&mut self, xx: i32, yy: i32, zz: i32, lava: i32, grass: &mut bool) {
        let ids = ids();
        let current = self.ch.get(xx, yy, zz);
        if current == ids.grass {
            *grass = true;
        }
        if yy <= lava {
            if current != ids.lava {
                self.ch.set(xx, yy, zz, ids.lava);
            }
        } else {
            if current != ids.air {
                self.ch.set(xx, yy, zz, ids.air);
            }
            // restoreSurfaceIfNeeded
            if *grass {
                let below = self.ch.get(xx, yy - 1, zz);
                if crate::overworld::feature::blocks::is_dirt(below) && below != ids.grass
                    && let Some(s) = super::surfaces().get(&self.ch.biome(xx, yy - 1, zz))
                {
                    self.ch.set(xx, yy - 1, zz, s.top);
                }
            }
        }
    }

    /// CaveGenerateFeature.hasLiquid: the box's outer shell, plus just above and below it.
    fn has_liquid(&self, x_from: i32, x_to: i32, y_from: i32, y_to: i32, z_from: i32, z_to: i32) -> bool {
        let liquid = |b: i32| matches!(crate::overworld::feature::blocks::short(b), "water" | "flowing_water" | "lava" | "flowing_lava");
        for xx in x_from..x_to {
            for zz in z_from..z_to {
                let edge = xx == x_from || xx == x_to - 1 || zz == z_from || zz == z_to - 1;
                let mut yy = y_to + 1;
                while yy >= y_from - 1 {
                    if yy < MAX_Y && liquid(self.ch.get(xx, yy, zz)) {
                        return true;
                    }
                    if yy != y_from - 1 && !edge {
                        yy = y_from;
                    }
                    yy -= 1;
                }
            }
        }
        false
    }
}

struct Tunnel {
    h_mul: f64,
    v_mul: f64,
    floor_level: f64,
    lava: i32,
}

fn can_reach(centre_x: f64, centre_z: f64, x: f64, z: f64, current: i32, distance: i32, thickness: f32) -> bool {
    let (dx, dz) = (x - centre_x, z - centre_z);
    let remaining = (distance - current) as f64;
    let max_reach = (thickness + 2.0f32 + CHUNK as f32) as f64;
    dx * dx + dz * dz - remaining * remaining <= max_reach * max_reach
}

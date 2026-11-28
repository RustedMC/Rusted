// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Sulfur caves: sulfur/cinnabar gradient on exposed stone plus sulfur spikes, from PNX
//! `SulfurSpikeFeature`. The spike placement is shared with the spike cluster.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;

use super::super::blocks::{self, b, s};
use super::super::decoration::noise::SimplexNoise;
use super::super::decoration::placement::{reseed, MAX_Y, MIN_Y};
use super::super::{Ctx, FeatureFn, Region, Xoroshiro};
use crate::overworld::biome::id::SULFUR_CAVES;

const NAME: &str = "minecraft:sulfur_spike_feature";

pub fn register(m: &mut HashMap<&'static str, FeatureFn>) {
    m.insert("minecraft:sulfur_caves_after_surface_sulfur_spike_feature_rules", apply);
    m.insert(NAME, apply);
}

/// PNX `FeatureHolder.sulfurCaveGradient`: the 36th `fork()` of the world provider (BiomeHolder
/// forks 7, TerrainHolder 28, as in `router.rs`), `SimplexNoise(-5, {1, 0, 1})`.
fn gradient(seed: i64) -> Arc<SimplexNoise> {
    static C: OnceLock<Mutex<HashMap<i64, Arc<SimplexNoise>>>> = OnceLock::new();
    let c = C.get_or_init(Default::default);
    if let Some(g) = c.lock().get(&seed) {
        return g.clone();
    }
    let mut r = Xoroshiro::new(seed);
    for _ in 0..35 {
        r.fork();
    }
    let g = Arc::new(SimplexNoise::new(&mut r.fork(), -5, &[1.0, 0.0, 1.0]));
    c.lock().insert(seed, g.clone());
    g
}

// from PNX SulfurSpikeFeature
fn apply(r: &mut Region, ctx: &mut Ctx) {
    reseed(ctx, NAME);
    let g = gradient(ctx.seed);
    noise_gradient_surface(r, ctx, &g);

    let (min_y, max_y) = (MIN_Y, 256.min(MAX_Y - 1));
    let rand = &mut ctx.random;
    let attempts = rand.next_int_range(192, 256);
    for _ in 0..attempts {
        let base_x = (ctx.cx << 4) + rand.next_bounded_int(15);
        let base_z = (ctx.cz << 4) + rand.next_bounded_int(15);
        let base_y = rand.next_int_range(min_y, max_y);
        let inner = rand.next_int_range(1, 5);
        for _ in 0..inner {
            let x = base_x + clamped_normal(rand, 3.0, 10);
            let y = base_y + clamped_normal(rand, 0.6, 2);
            let z = base_z + clamped_normal(rand, 3.0, 10);
            if x >> 4 != ctx.cx || z >> 4 != ctx.cz || y < min_y || y > max_y || r.biome(x, y, z) != SULFUR_CAVES {
                continue;
            }
            let hanging = !rand.next_boolean();
            // floor scan goes down and grows up; ceiling scan goes up and hangs down
            let step = if hanging { 1 } else { -1 };
            for i in 0..=12 {
                let cy = y + i * step;
                let cur = r.get(x, cy, z);
                if is_base(cur) {
                    r.set(x, cy, z, spike_base_state(&g, x, cy, z));
                    let len = rand.next_int_range(1, 5);
                    place_spike(r, x, cy - step, z, hanging, len);
                    break;
                }
                if !air_or_water(cur) {
                    break;
                }
            }
        }
    }
}

fn noise_gradient_surface(r: &mut Region, ctx: &Ctx, g: &SimplexNoise) {
    for lx in 0..16 {
        let wx = (ctx.cx << 4) + lx;
        for lz in 0..16 {
            let wz = (ctx.cz << 4) + lz;
            let mut y = 256.min(r.height(wx, wz));
            while y > MIN_Y + 1 {
                if r.biome(wx, y, wz) == SULFUR_CAVES && is_natural_base(r.get(wx, y, wz)) && exposed(r, wx, y, wz) {
                    if let Some(st) = gradient_state(g.value(wx as f64, y as f64, wz as f64)) {
                        r.set(wx, y, wz, st);
                    }
                }
                y -= 1;
            }
        }
    }
}

fn exposed(r: &Region, x: i32, y: i32, z: i32) -> bool {
    [(0, 1, 0), (0, -1, 0), (1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1)].iter().any(|&(dx, dy, dz)| air_or_water(r.get(x + dx, y + dy, z + dz)))
}

fn is_natural_base(v: i32) -> bool {
    matches!(blocks::short(v), "stone" | "deepslate" | "tuff")
}

/// Stone, deepslate, tuff, sulfur or cinnabar (both features' `isBaseBlock`).
pub(super) fn is_base(v: i32) -> bool {
    matches!(blocks::short(v), "stone" | "deepslate" | "tuff" | "sulfur" | "cinnabar")
}

pub(super) fn air_or_water(v: i32) -> bool {
    blocks::is_air(v) || blocks::is_water(v)
}

fn gradient_state(n: f32) -> Option<i32> {
    if (-0.4..-0.1).contains(&n) || (0.4..=1.0).contains(&n) {
        Some(blocks::id("cinnabar"))
    } else if (0.0..0.4).contains(&n) {
        Some(blocks::id("sulfur"))
    } else {
        None
    }
}

fn spike_base_state(g: &SimplexNoise, x: i32, y: i32, z: i32) -> i32 {
    gradient_state(g.value(x as f64, y as f64, z as f64)).unwrap_or_else(|| blocks::id("sulfur"))
}

/// PNX `placeSulfurSpike` (identical in the spike and spike cluster features).
pub(super) fn place_spike(r: &mut Region, x: i32, y: i32, z: i32, hanging: bool, max_len: i32) {
    let step = if hanging { -1 } else { 1 };
    let mut planned = Vec::new();
    let mut merge_y = None;
    for i in 0..max_len {
        let cy = y + i * step;
        let cur = r.get(x, cy, z);
        if blocks::is(cur, "sulfur_spike") {
            if crate::blocks::state(cur, "hanging") != Some(&crate::blocks::State::Bool(hanging)) && !planned.is_empty() {
                merge_y = Some(cy);
            }
            break;
        }
        if !air_or_water(cur) {
            break;
        }
        planned.push(cy);
    }
    if planned.is_empty() {
        return;
    }
    let last = planned.len() - 1;
    for (i, &py) in planned.iter().enumerate() {
        let t = if i == last {
            if merge_y.is_some() { "merge" } else { "tip" }
        } else if i == 0 && last >= 2 {
            "base"
        } else if i == last - 1 {
            "frustum"
        } else {
            "middle"
        };
        // TODO: PNX also keeps water on layer 1 when the spot was water; Region has one layer.
        r.set(x, py, z, spike(hanging, t));
    }
    if let Some(my) = merge_y {
        r.set(x, my, z, spike(!hanging, "merge"));
    }
}

fn spike(hanging: bool, thickness: &str) -> i32 {
    blocks::with(blocks::id("sulfur_spike"), &[("hanging", b(hanging)), ("dripstone_thickness", s(thickness))])
}

fn clamped_normal(rand: &mut Xoroshiro, deviation: f32, max: i32) -> i32 {
    let u1 = rand.next_double().max(f64::from_bits(1)); // Double.MIN_VALUE
    let u2 = rand.next_double();
    let v = ((-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos() * deviation as f64) as f32;
    ((v + 0.5).floor() as i32).clamp(-max, max) // Java Math.round(float)
}

#[cfg(test)]
mod tests {
    use super::super::super::{blocks, Ctx, Region, Xoroshiro};
    use crate::overworld::Chunk;

    #[test]
    fn sulfur_spike_runs() {
        let stone = blocks::id("stone");
        let chunks = (0..9)
            .map(|_| {
                let mut c = Chunk::new();
                for x in 0..16 {
                    for z in 0..16 {
                        for y in -64..60 {
                            if y % 9 != 0 {
                                c.set(x, y, z, stone);
                            }
                        }
                    }
                }
                c
            })
            .collect();
        let mut r = Region::new(0, 0, 1, chunks);
        super::apply(&mut r, &mut Ctx { cx: 0, cz: 0, seed: 1, random: Xoroshiro::new(0) });
        super::place_spike(&mut r, 0, 60, 0, false, 4);
        assert!(blocks::is(r.get(0, 60, 0), "sulfur_spike"));
    }
}

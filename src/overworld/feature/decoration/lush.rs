// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Lush caves: moss ceilings with cave vines, moss floor patches, clay pools with dripleaves.

use std::collections::HashMap;

use super::super::blocks::{self, b, s};
use super::super::{chunk_hash, Ctx, FeatureFn, Region};
use super::placement::{self, Node, PlaceCtx, Placement, SnapToFloor, Weighted, MAX_Y, MIN_Y};
use super::{biome, dirt, noise, water, DIRS};

/// `MossSnapToCeilingFeature`.
fn moss_ceiling(r: &mut Region, ctx: &mut Ctx) {
    ctx.random.set_seed(ctx.seed ^ chunk_hash(ctx.cx, ctx.cz));
    let h = noise::holder(ctx.seed);
    let air = blocks::air();
    let [moss, vine, body, head, spore] = ["moss_block", "cave_vines", "cave_vines_body_with_berries", "cave_vines_head_with_berries", "spore_blossom"].map(blocks::id);
    let rand = &mut ctx.random;
    for x in 0..16 {
        // PNX's `x + chunkX << 4` is `(x + chunkX) << 4`; kept.
        let nx = (x + ctx.cx) << 4;
        let wx = (ctx.cx << 4) + x;
        for z in 0..16 {
            let nz = (z + ctx.cz) << 4;
            let wz = (ctx.cz << 4) + z;
            if h.moss_ceiling.noise_2d(nx as f32 * 0.25, nz as f32 * 0.25, true) <= 0.0 {
                continue;
            }
            let mut y = r.height(wx, wz);
            while y > MIN_Y {
                if r.biome(wx, y, wz) == biome::LUSH_CAVES && r.get(wx, y, wz) == air && matches!(blocks::short(r.get(wx, y + 2, wz)), "stone" | "deepslate") {
                    let yy = y + 2;
                    r.set(wx, yy, wz, moss);
                    if rand.next_float() < 0.1 {
                        if rand.next_float() < 0.003 {
                            if r.get(wx, yy - 1, wz) == air {
                                r.set(wx, yy - 1, wz, spore);
                            }
                        } else {
                            let len = rand.next_int_range(0, 4);
                            let (mut last, mut any) = (yy, false);
                            for seg in 0..len {
                                let by = yy - 1 - seg;
                                if r.get(wx, by, wz) != air {
                                    break;
                                }
                                r.set(wx, by, wz, if rand.next_float() as f64 > 0.3 { vine } else { body });
                                last = by;
                                any = true;
                            }
                            if r.get(wx, last - 1, wz) == air {
                                r.set(wx, last - 1, wz, if rand.next_float() as f64 > 0.3 { vine } else { head });
                            } else if any {
                                r.set(wx, last, wz, if rand.next_float() as f64 > 0.3 { vine } else { head });
                            }
                        }
                    }
                }
                y -= 1;
            }
        }
    }
}

fn air_at(c: &PlaceCtx) -> bool {
    c.get(0, 0, 0) == blocks::air()
}

/// `MossVegetationPlacementFeature`.
fn moss_vegetation() -> Weighted {
    fn dirt_supported(c: &mut PlaceCtx, name: &str) -> bool {
        if !air_at(c) || !dirt(c.get(0, -1, 0)) {
            return false;
        }
        c.region.set(c.x, c.y, c.z, blocks::id(name));
        true
    }
    let n = |f: fn(&mut PlaceCtx) -> bool| -> Node { Box::new(f) };
    Weighted(vec![
        (n(|c| dirt_supported(c, "flowering_azalea")), 4.0),
        (n(|c| dirt_supported(c, "azalea")), 7.0),
        (
            n(|c| {
                if !air_at(c) || c.get(0, -1, 0) == blocks::air() {
                    return false;
                }
                c.region.set(c.x, c.y, c.z, blocks::id("moss_carpet"));
                true
            }),
            25.0,
        ),
        (n(|c| dirt_supported(c, "short_grass")), 50.0),
        (
            n(|c| {
                if !air_at(c) || c.y + 1 >= MAX_Y || c.get(0, 1, 0) != blocks::air() || !dirt(c.get(0, -1, 0)) {
                    return false;
                }
                let tall = |u: bool| blocks::with(blocks::id("tall_grass"), &[("upper_block_bit", b(u))]);
                c.region.set(c.x, c.y, c.z, tall(false));
                c.region.set(c.x, c.y + 1, c.z, tall(true));
                true
            }),
            10.0,
        ),
    ])
}

/// `MossPatchPlacementFeature`.
struct MossPatch {
    vegetation: Weighted,
}

impl Placement for MossPatch {
    fn place(&self, c: &mut PlaceCtx) -> bool {
        let (ox, oy, oz) = c.pos();
        let rx = 5 + c.random.next_exclusive_int(4);
        let rz = 5 + c.random.next_exclusive_int(4);
        let air = blocks::air();
        let moss = blocks::id("moss_block");
        let mut ground = Vec::new();
        for dx in -rx..=rx {
            let xe = dx == -rx || dx == rx;
            for dz in -rz..=rz {
                let ze = dz == -rz || dz == rz;
                if xe && ze || (xe || ze) && c.random.next_float() > 0.3 {
                    continue;
                }
                let (x, z) = (ox + dx, oz + dz);
                let mut y = oy;
                let mut step = 0;
                while c.region.get(x, y, z) == air && step < 5 {
                    y -= 1;
                    step += 1;
                }
                step = 0;
                while c.region.get(x, y, z) != air && step < 5 {
                    y += 1;
                    step += 1;
                }
                if y <= MIN_Y || y >= MAX_Y || c.region.get(x, y, z) != air {
                    continue;
                }
                let gy = y - 1;
                let g = c.region.get(x, gy, z);
                if !blocks::is_full(g) {
                    continue;
                }
                let n = blocks::short(g);
                if n != "moss_block" {
                    let replaceable = matches!(
                        n,
                        "dirt" | "coarse_dirt" | "podzol" | "dirt_with_roots" | "grass_block" | "mycelium" | "stone" | "granite" | "polished_granite" | "diorite" | "polished_diorite" | "andesite" | "polished_andesite" | "deepslate" | "tuff" | "cave_vines" | "cave_vines_body_with_berries" | "cave_vines_head_with_berries"
                    );
                    if !replaceable {
                        continue;
                    }
                    c.region.set(x, gy, z, moss);
                }
                ground.push((x, gy, z));
            }
        }
        let Some(&last) = ground.last() else {
            c.set_pos(ox, oy, oz);
            return false;
        };
        for &(x, y, z) in &ground {
            if c.random.next_float() < 0.8 {
                c.set_pos(x, y + 1, z);
                self.vegetation.place(c);
            }
        }
        c.set_pos(last.0, last.1, last.2);
        true
    }
}

/// `MossPatchSnapToFloorFeature`: 125 volume candidates in lush caves.
fn moss_patch(r: &mut Region, ctx: &mut Ctx) {
    let f = SnapToFloor { range: 12, child: Box::new(MossPatch { vegetation: moss_vegetation() }) };
    placement::volume_candidates(r, ctx, "minecraft:moss_patch_snap_to_floor_feature", 125, -64, 256, &f, |c| c.biome() == biome::LUSH_CAVES);
}

/// PNX `BlockManager` writes this feature iterates over, in insertion order.
#[derive(Default)]
struct Writes {
    order: Vec<(i32, i32, i32)>,
    map: HashMap<(i32, i32, i32), i32>,
}

impl Writes {
    fn set(&mut self, p: (i32, i32, i32), b: i32) {
        if self.map.insert(p, b).is_none() {
            self.order.push(p);
        }
    }
    fn get(&self, r: &Region, p: (i32, i32, i32)) -> i32 {
        self.map.get(&p).copied().unwrap_or_else(|| r.get(p.0, p.1, p.2))
    }
}

/// `RandomClayWithDripleavesSnapToFloorFeature`.
fn clay_dripleaves(r: &mut Region, ctx: &mut Ctx) {
    ctx.random.set_seed(ctx.seed ^ chunk_hash(ctx.cx, ctx.cz));
    let h = noise::holder(ctx.seed);
    let noise = &h.clay_dripleaves;
    let (air, clay, w) = (blocks::air(), blocks::id("clay"), water());
    let mut m = Writes::default();
    for x in 0..16 {
        let bx = (ctx.cx << 4) + x;
        for z in 0..16 {
            let bz = (ctx.cz << 4) + z;
            if noise.noise_2d(bx as f32 * 0.25, bz as f32 * 0.25, true) as f64 <= 0.3 {
                continue;
            }
            let mut y = r.height(bx, bz);
            while y > MIN_Y {
                if r.biome(bx, y, bz) == biome::LUSH_CAVES && matches!(blocks::short(r.get(bx, y, bz)), "stone" | "deepslate" | "clay") && r.get(bx, y + 1, bz) == air {
                    let v = noise.noise_3d(bx as f32, y as f32, bz as f32, true);
                    let remap = (v + 1.0) / 2.0 * 2.0 + 3.0; // remapFromNormalized(v, 3, 5)
                    let depth = remap.clamp(3.0, 4.0) as i32;
                    for n in 0..depth {
                        if r.biome(bx, y - n, bz) == biome::LUSH_CAVES {
                            m.set((bx, y - n, bz), clay);
                        }
                    }
                }
                y -= 1;
            }
        }
    }
    // `block.up()` reads the level, not the pending writes.
    for idx in 0..m.order.len() {
        let p = m.order[idx];
        if !blocks::is_solid(m.map[&p]) || !blocks::is_air(r.get(p.0, p.1 + 1, p.2)) {
            continue;
        }
        if noise.noise_3d(p.0 as f32, p.1 as f32, p.2 as f32, true) <= 0.1 {
            continue;
        }
        if [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().all(|(dx, dz)| m.map.contains_key(&(p.0 + dx, p.1, p.2 + dz))) {
            m.set(p, w);
        }
    }
    let rand = &mut ctx.random;
    for idx in 0..m.order.len() {
        let p = m.order[idx];
        if !blocks::replaceable(r.get(p.0, p.1 + 1, p.2)) || rand.next_float() as f64 >= 0.01 {
            continue;
        }
        let dir = s(DIRS[rand.next_int_below(3) as usize]);
        let is_water = m.map[&p] == w;
        if rand.next_int_below(9) < 8 {
            let big = blocks::id("big_dripleaf");
            let stem = blocks::with(big, &[("minecraft:cardinal_direction", dir.clone())]);
            let head = blocks::with(big, &[("minecraft:cardinal_direction", dir), ("big_dripleaf_head", b(true)), ("big_dripleaf_tilt", s("none"))]);
            let size = rand.next_int_range(if is_water { 1 } else { 0 }, 4);
            for n in 0..size {
                let q = (p.0, p.1 + n, p.2);
                let cur = m.get(r, q);
                if blocks::is_water(cur) || blocks::is_air(cur) {
                    m.set(q, stem);
                }
            }
            let hp = (p.0, p.1 + size, p.2);
            if !m.map.contains_key(&hp) {
                m.set(hp, head);
            }
        } else {
            let small = blocks::id("small_dripleaf_block");
            let stem = blocks::with(small, &[("minecraft:cardinal_direction", dir.clone())]);
            let head = blocks::with(small, &[("minecraft:cardinal_direction", dir), ("upper_block_bit", b(true))]);
            let off = if is_water { 0 } else { 1 };
            m.set((p.0, p.1 + off, p.2), stem);
            m.set((p.0, p.1 + 1 + off, p.2), head);
        }
    }
    for p in m.order {
        r.set(p.0, p.1, p.2, m.map[&p]);
    }
}

pub(super) fn register(m: &mut HashMap<&'static str, FeatureFn>) {
    m.insert("minecraft:moss_ceiling_snap_to_ceiling_feature", moss_ceiling);
    m.insert("minecraft:moss_patch_snap_to_floor_feature", moss_patch);
    m.insert("minecraft:random_clay_with_dripleaves_snap_to_floor_feature", clay_dripleaves);
}

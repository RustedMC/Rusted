// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
// Ported from PNX NetherComplexPlacement, NetherFortressPopulator, BastionRemnantPopulator,
// NetherFossilPopulator, StructurePlacement.
//! Nether structures. Fortresses and bastions share PNX's nether complex placement
//! (one per 30x30 chunk region), fossils use `StructurePlacement`. Built once and cached.

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::Mutex;

use super::fortress::{self, BBox, Lvl};
use super::rng::{BedrockRandom, NukkitRandom};
use super::terrain::LAVA_LEVEL;
use super::{ids, jigsaw, templates, Nether, NetherChunk, BASALT_DELTAS, SOULSAND_VALLEY};
use crate::overworld::feature::blocks as bl;
use crate::overworld::feature::{chunk_hash, Xoroshiro};
use crate::overworld::structures::{Placement, Spread};

const REGION: i32 = 30;
const EDGE: i32 = 4;
const SALT: i32 = 0x01CB_0C88;
/// NetherFortressPopulator.START_SEARCH_RADIUS.
const FORTRESS_RADIUS: i32 = 8;
// TODO: assumes bastions reach at most 8 chunks; raise it if one gets cut at a border.
const BASTION_RADIUS: i32 = 8;
const FOSSIL: Placement = Placement::new(0x0766_9456_5C61_6765, 2, 32, Spread::Legacy);
const FOSSIL_RADIUS: i32 = 1;
/// PNX PopulatorRuinedPortal NETHER_PLACEMENT.
const PORTAL: Placement = Placement::new(40_552_231, 10, 25, Spread::Linear);
/// Giant portals are up to about 20 blocks across.
const PORTAL_RADIUS: i32 = 2;

fn region_seed(seed: i64, rx: i32, rz: i32) -> i32 {
    rx.wrapping_mul(0x9939_F508u32 as i32).wrapping_add(rz.wrapping_mul(0xF156_5BD5u32 as i32)).wrapping_add(seed as i32).wrapping_add(SALT)
}

/// The region's candidate chunk and whether it is a bastion (`isNetherComplexStart`,
/// `shouldGenerateBastion`).
fn complex(seed: i64, rx: i32, rz: i32) -> (i32, i32, bool) {
    let mut r = BedrockRandom::new(region_seed(seed, rx, rz) as i64);
    let x = rx * REGION + r.next_int_below(REGION - EDGE);
    let z = rz * REGION + r.next_int_below(REGION - EDGE);
    (x, z, r.next_int_below(6) >= 2)
}

/// `createPostSelectionRandom`.
fn post_selection_random(seed: i64, cx: i32, cz: i32) -> BedrockRandom {
    let mut r = BedrockRandom::new(region_seed(seed, cx.div_euclid(REGION), cz.div_euclid(REGION)) as i64);
    r.next_int_below(REGION - EDGE);
    r.next_int_below(REGION - EDGE);
    r.next_int_below(6);
    r
}

/// Complex starts (chunk x, z, bastion) whose chunk lies within `radius` of (cx, cz).
fn complex_starts(seed: i64, cx: i32, cz: i32, radius: i32) -> Vec<(i32, i32, bool)> {
    let mut out = Vec::new();
    for rx in (cx - radius).div_euclid(REGION)..=(cx + radius).div_euclid(REGION) {
        for rz in (cz - radius).div_euclid(REGION)..=(cz + radius).div_euclid(REGION) {
            let (x, z, b) = complex(seed, rx, rz);
            if (x - cx).abs() <= radius && (z - cz).abs() <= radius {
                out.push((x, z, b));
            }
        }
    }
    out
}

/// Blocks by chunk: (local x, y, local z, id).
type Blocks = HashMap<(i32, i32), Vec<(u8, u8, u8, i32)>>;

enum Built {
    Blocks(Blocks),
    Fortress(fortress::Start),
}

#[derive(Default)]
pub struct Cache(Mutex<HashMap<(u8, i32, i32), Arc<Built>>>);

const CACHE_LIMIT: usize = 1024;

fn cached(n: &Nether, kind: u8, sx: i32, sz: i32, build: impl FnOnce() -> Built) -> Arc<Built> {
    if let Some(b) = n.structures.0.lock().get(&(kind, sx, sz)) {
        return b.clone();
    }
    let b = Arc::new(build()); // outside the lock
    let mut m = n.structures.0.lock();
    if m.len() >= CACHE_LIMIT {
        m.clear(); // TODO: rebuilds on demand, maybe LRU
    }
    m.entry((kind, sx, sz)).or_insert(b).clone()
}

fn group(list: impl IntoIterator<Item = (i32, i32, i32, i32)>) -> Blocks {
    let mut out: Blocks = HashMap::new();
    for (x, y, z, v) in list {
        if (super::MIN_Y..=super::MAX_Y).contains(&y) {
            out.entry((x >> 4, z >> 4)).or_default().push(((x & 15) as u8, y as u8, (z & 15) as u8, v));
        }
    }
    out
}

fn copy(b: &Built, ch: &mut NetherChunk, cx: i32, cz: i32) {
    if let Built::Blocks(m) = b
        && let Some(list) = m.get(&(cx, cz))
    {
        for &(x, y, z, v) in list {
            ch.set(x as i32, y as i32, z as i32, v);
        }
    }
}

/// Structures reaching chunk (cx, cz), in PNX populator order: bastions, fortresses, fossils.
pub fn apply(n: &Nether, ch: &mut NetherChunk, cx: i32, cz: i32) {
    let seed = n.seed;
    // BastionRemnantPopulator
    for (sx, sz, bastion) in complex_starts(seed, cx, cz, BASTION_RADIUS) {
        if !bastion || n.terrain.picker.column((sx << 4) + 7, (sz << 4) + 7) == BASALT_DELTAS {
            continue;
        }
        let b = cached(n, 0, sx, sz, || {
            let blocks = jigsaw::bastion([sx << 4, 33, sz << 4], &mut post_selection_random(seed, sx, sz));
            for &(x, y, z, v) in &blocks {
                crate::loot::generated((x, y + crate::dims::base(crate::dims::NETHER), z), v, "bastion");
            }
            Built::Blocks(group(blocks))
        });
        copy(&b, ch, cx, cz);
    }
    // NetherFortressPopulator
    let cb = BBox::chunk(cx, cz);
    let mut x = Xoroshiro::new(seed);
    let (r1, r2) = (x.next_int(), x.next_int());
    for (sx, sz, bastion) in complex_starts(seed, cx, cz, FORTRESS_RADIUS) {
        if bastion {
            continue;
        }
        let b = cached(n, 1, sx, sz, || Built::Fortress(fortress::generate(&mut post_selection_random(seed, sx, sz), sx, sz)));
        let Built::Fortress(s) = &*b else { continue };
        if !s.pieces.is_empty() && s.bb.intersects(&cb) {
            let mut random = NukkitRandom::new((cx as i64).wrapping_mul(r1 as i64) ^ (cz as i64).wrapping_mul(r2 as i64) ^ seed);
            fortress::post_process(s, &mut Lvl { ch, bx: cx << 4, bz: cz << 4 }, &mut random, &cb);
        }
    }
    // NetherFossilPopulator
    for sx in cx - FOSSIL_RADIUS..=cx + FOSSIL_RADIUS {
        for sz in cz - FOSSIL_RADIUS..=cz + FOSSIL_RADIUS {
            if !FOSSIL.is_candidate(seed, sx, sz) {
                continue;
            }
            let b = cached(n, 2, sx, sz, || Built::Blocks(group(fossil(n, sx, sz))));
            copy(&b, ch, cx, cz);
        }
    }
    // PopulatorRuinedPortal
    for sx in cx - PORTAL_RADIUS..=cx + PORTAL_RADIUS {
        for sz in cz - PORTAL_RADIUS..=cz + PORTAL_RADIUS {
            if !PORTAL.is_candidate(seed, sx, sz) {
                continue;
            }
            let b = cached(n, 3, sx, sz, || Built::Blocks(group(ruined_portal(n, sx, sz))));
            copy(&b, ch, cx, cz);
        }
    }
}

/// PNX `PopulatorRuinedPortal` in the Nether: a portal in blackstone, sunk to where 3 of its
/// corners rest on solid ground, sometimes high up in an air pocket.
fn ruined_portal(n: &Nether, cx: i32, cz: i32) -> Vec<(i32, i32, i32, i32)> {
    use crate::overworld::feature::random_range;
    let mut r = Xoroshiro::new(0);
    r.set_seed(n.seed ^ chunk_hash(cx, cz));
    let (x, z) = ((cx << 4) + 7, (cz << 4) + 7);
    let big = r.next_bounded_int(20) == 0;
    let name = if big { format!("ruined_portal/giant_portal_{}", r.next_int_below(3) + 1) } else { format!("ruined_portal/portal_{}", r.next_int_below(10) + 1) };
    let Some(tpl) = templates::get(&name) else { return vec![] };
    let [sx, _, sz] = tpl.size;
    let air_pocket = r.next_float() < 0.5;
    let start = if air_pocket {
        random_range(&mut r, 32, 100)
    } else if r.next_boolean() {
        random_range(&mut r, 27, 29)
    } else {
        random_range(&mut r, 29, 100)
    };
    let get = |x: i32, y: i32, z: i32| n.terrain_chunk(x >> 4, z >> 4).get(x & 15, y, z & 15);
    let ground = |b: i32| bl::is_full(b);
    let min_y = super::MIN_Y + 15;
    let mut y = start;
    while y > min_y {
        let corners = [(x, z), (x + sx - 1, z), (x, z + sz - 1), (x + sx - 1, z + sz - 1)];
        if corners.iter().filter(|&&(a, b)| ground(get(a, y, b))).count() >= 3 {
            break;
        }
        y -= 1;
    }
    let mut staged = templates::Blocks::default();
    tpl.stage(&mut staged, (x, y, z));
    for (bx, by, bz, v) in staged.list() {
        let short = bl::short(v);
        let swap = match short {
            "jigsaw" => Some(ids().netherrack),
            "obsidian" if r.next_int_below(5) == 0 => crate::blocks::any_state("minecraft:crying_obsidian"),
            "chiseled_stone_bricks" => Some(crate::blocks::rename(v, "chiseled_polished_blackstone")),
            "cracked_stone_bricks" => Some(crate::blocks::rename(v, "cracked_polished_blackstone_bricks")),
            "stone_bricks" => Some(crate::blocks::rename(v, "polished_blackstone_bricks")),
            "stone_brick_slab" => Some(crate::blocks::rename(v, "polished_blackstone_brick_slab")),
            "stone_brick_stairs" => Some(crate::blocks::rename(v, "polished_blackstone_brick_stairs")),
            "stone_brick_wall" => Some(crate::blocks::rename(v, "polished_blackstone_wall")),
            "mossy_stone_bricks" => {
                staged.unset(bx, by, bz);
                None
            }
            _ => None,
        };
        if let Some(b) = swap {
            staged.set(bx, by, bz, b);
        }
    }
    let out = staged.list();
    for &(bx, by, bz, v) in &out {
        crate::loot::generated((bx, by + crate::dims::base(crate::dims::NETHER), bz), v, "ruined_portal");
    }
    out
}

/// `NetherFossilPopulator.apply` for its start chunk, reading the undecorated terrain.
// TODO: PNX reads the live world here, we only read raw terrain.
fn fossil(n: &Nether, cx: i32, cz: i32) -> Vec<(i32, i32, i32, i32)> {
    let i = ids();
    let t = n.terrain_chunk(cx, cz);
    if t.biome(3, LAVA_LEVEL, 3) != SOULSAND_VALLEY {
        return vec![];
    }
    let mut r = Xoroshiro::new(n.seed ^ chunk_hash(cx, cz));
    let (x, z) = ((cx << 4) + 3, (cz << 4) + 3);
    let name = format!("nether_fossils/fossil_{}", r.next_int_below(14) + 1);
    let Some(tpl) = templates::get(&name) else { return vec![] };
    let get = |x: i32, y: i32, z: i32| n.terrain_chunk(x >> 4, z >> 4).get(x & 15, y, z & 15);
    let mut height = i32::MAX;
    for bx in 0..tpl.size[0] {
        for bz in 0..tpl.size[2] {
            for y in (1..=128).rev() {
                let b = get(x + bx, y, z + bz);
                if (b == i.soul_sand || b == i.soul_soil) && bl::replaceable(get(x + bx, y + 1, z + bz)) {
                    height = height.min(y + 1);
                }
            }
        }
    }
    if height == i32::MAX {
        return vec![];
    }
    let mut staged = templates::Blocks::default();
    tpl.stage(&mut staged, (x, height, z));
    if r.next_int_below(3) == 0
        && let Some(g) = crate::blocks::any_state("minecraft:dried_ghast")
    {
        staged.set(x - 1, height, z - 1, g);
    }
    staged.list().into_iter().filter(|b| !bl::is_air(b.3)).collect()
}

#[cfg(test)]
pub fn complex_for_tests(seed: i64, rx: i32, rz: i32) -> (i32, i32, bool) {
    complex(seed, rx, rz)
}

#[cfg(test)]
mod portal_tests {
    use super::*;

    #[test]
    fn nether_ruined_portals_generate_in_blackstone() {
        if !std::path::Path::new("data/structures.nbt").exists() {
            return; // template data isn't in every checkout
        }
        crate::blocks::init();
        let n = Nether::new(7);
        let start = (0..200).flat_map(|x| (0..200).map(move |z| (x, z))).find(|&(x, z)| PORTAL.is_candidate(n.seed, x, z)).expect("a portal chunk");
        let blocks = ruined_portal(&n, start.0, start.1);
        assert!(!blocks.is_empty(), "the portal template loaded");
        assert!(blocks.iter().any(|b| bl::short(b.3).contains("obsidian")), "has its obsidian frame");
        assert!(!blocks.iter().any(|b| bl::short(b.3) == "stone_bricks"), "stone bricks became blackstone");
    }
}


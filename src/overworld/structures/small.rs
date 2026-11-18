// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Small structures: swamp hut, desert well, desert pyramid, jungle temple
//! (PNX `SwampHutPopulator`, `DesertWellPopulator`, `DesertPyramidPopulator`, `JungleTemplePopulator`).
//!
//! No loot or block entities. Buried treasure only adds a volume, so it's skipped.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::super::feature::blocks::{self as bl, b, i, id, s, with};
use super::super::feature::chunk_hash;
use super::super::feature::objects::World;
use super::{Placement, Spread, StructWorld, StructureType, Xoroshiro};

/// Shared by the hut, pyramid and temple (same salt and spacing in PNX; the biome picks one).
const PLACEMENT: Placement = Placement::new(14_357_617, 8, 32, Spread::Linear);

fn hint(seed: i64, cx: i32, cz: i32) -> bool {
    PLACEMENT.is_candidate(seed, cx, cz)
}

pub fn register(v: &mut Vec<StructureType>) {
    v.push(StructureType { name: "normal_swamp_hut", radius: 1, hint, build: swamp_hut });
    v.push(StructureType { name: "normal_desert_well", radius: 1, hint: well_hint, build: desert_well });
    v.push(StructureType { name: "normal_desert_pyramid", radius: 2, hint, build: desert_pyramid });
    v.push(StructureType { name: "normal_jungle_temple", radius: 1, hint, build: jungle_temple });
}

// Biome tags from the vanilla biome definitions (same approach as feature/trees.rs).
fn tags() -> &'static HashMap<u16, Vec<String>> {
    static T: OnceLock<HashMap<u16, Vec<String>>> = OnceLock::new();
    T.get_or_init(|| {
        let batch = crate::vanilla::batches().nth(2).expect("biome definitions frame");
        let decoded = bedrock::network::codec::decode_packets_each::<crate::Protocol>(batch, None, None, usize::MAX, bedrock::network::codec::UnreadBytes::Warn);
        let Some(crate::Protocol::BiomeDefinitionListPacket(p)) = decoded.ok().and_then(|d| d.into_iter().next()).and_then(|(_, p)| p.ok()) else {
            return HashMap::new();
        };
        let s = |i: u16| p.strings.get(i as usize).cloned().unwrap_or_default();
        p.biomes
            .iter()
            .filter_map(|e| {
                let id = super::super::biome_id(s(e.name_index).trim_start_matches("minecraft:"))?;
                Some((id, e.definition.tags.iter().flat_map(|t| t.tags.iter()).map(|&t| s(t)).collect()))
            })
            .collect()
    })
}

fn tagged(biome: u16, tag: &str) -> bool {
    tags().get(&biome).is_some_and(|v| v.iter().any(|t| t == tag))
}

fn is_biome(biome: u16, names: &[&str]) -> bool {
    names.iter().any(|n| super::super::biome_id(n) == Some(biome))
}

/// PNX: reseed with `seed ^ chunkHash`, pick x/z in the chunk, y = heightmap - 1.
fn start(w: &StructWorld, r: &mut Xoroshiro, cx: i32, cz: i32) -> (i32, i32, i32) {
    r.set_seed(w.seed ^ chunk_hash(cx, cz));
    let x = (cx << 4) + r.next_bounded_int(15);
    let z = (cz << 4) + r.next_bounded_int(15);
    (x, w.height(x, z) - 1, z)
}

/// PNX `chunk.getBiomeId(7, chunk.getHeightMap(7, 7) - 1, 7)`.
fn surface_biome(w: &StructWorld, cx: i32, cz: i32) -> u16 {
    let (x, z) = ((cx << 4) + 7, (cz << 4) + 7);
    w.biome(x, w.height(x, z) - 1, z)
}

/// PNX `StructureHelper`: writes relative to an origin.
struct H<'a, 'b> {
    w: &'a mut StructWorld<'b>,
    o: (i32, i32, i32),
}

impl H<'_, '_> {
    fn get(&self, x: i32, y: i32, z: i32) -> i32 {
        self.w.get(x + self.o.0, y + self.o.1, z + self.o.2)
    }
    fn set(&mut self, x: i32, y: i32, z: i32, v: i32) {
        self.w.set(x + self.o.0, y + self.o.1, z + self.o.2, v);
    }
    /// `fill(min, max, outer, inner)`.
    fn fill2(&mut self, a: (i32, i32, i32), m: (i32, i32, i32), outer: i32, inner: i32) {
        for y in a.1..=m.1 {
            for x in a.0..=m.0 {
                for z in a.2..=m.2 {
                    let edge = x == a.0 || x == m.0 || z == a.2 || z == m.2 || y == a.1 || y == m.1;
                    self.set(x, y, z, if edge { outer } else { inner });
                }
            }
        }
    }
    fn fill(&mut self, a: (i32, i32, i32), m: (i32, i32, i32), v: i32) {
        self.fill2(a, m, v, v);
    }
    /// `setBlockDownward`.
    fn downward(&mut self, x: i32, mut y: i32, z: i32, v: i32) {
        while bl::replaceable(self.get(x, y, z)) && y > 1 {
            self.set(x, y, z, v);
            y -= 1;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Swamp hut

fn swamp_hut(w: &mut StructWorld, r: &mut Xoroshiro, cx: i32, cz: i32) {
    if !tagged(w.chunk_biome(cx, cz), "swamp") {
        return;
    }
    let o = start(w, r, cx, cz);
    let planks = id("spruce_planks");
    let air = bl::air();
    let fence = id("oak_fence");
    let log = id("oak_log");
    let stairs = |d| with(id("spruce_stairs"), &[("weirdo_direction", i(d))]);
    let mut h = H { w, o };
    h.fill2((1, 1, 2), (5, 4, 7), planks, air);
    h.fill2((1, 1, 1), (5, 1, 1), planks, air);
    h.fill2((2, 1, 0), (4, 1, 0), planks, air);
    h.fill((4, 2, 2), (4, 3, 2), air);
    h.fill((5, 3, 4), (5, 3, 5), air);
    h.set(1, 3, 4, air);
    h.set(1, 3, 5, id("flower_pot")); // red mushroom content is block-entity data: skipped
    h.set(2, 3, 2, fence);
    h.set(3, 3, 7, fence);
    h.fill((0, 4, 1), (6, 4, 1), stairs(2));
    h.fill((6, 4, 2), (6, 4, 7), stairs(1));
    h.fill((0, 4, 8), (6, 4, 8), stairs(3));
    h.fill((0, 4, 2), (0, 4, 7), stairs(0));
    for (x, z) in [(1, 2), (5, 2), (1, 7), (5, 7)] {
        h.fill((x, 0, z), (x, 3, z), log);
    }
    h.set(1, 2, 1, fence);
    h.set(5, 2, 1, fence);
    h.set(4, 2, 6, id("cauldron"));
    h.set(3, 2, 6, id("crafting_table"));
    for (x, z) in [(1, 2), (5, 2), (1, 7), (5, 7)] {
        h.downward(x, -1, z, log);
    }
}

// ---------------------------------------------------------------------------------------------
// Desert well (no placement: every chunk rolls 1 in 501)

/// PNX seeds the well roll with `seed ^ (x + y + z)`. We don't know y, so try every y <= 128.
fn well_hint(seed: i64, cx: i32, cz: i32) -> bool {
    let mut r = Xoroshiro::new(seed ^ chunk_hash(cx, cz));
    let x = (cx << 4) + r.next_bounded_int(15);
    let z = (cz << 4) + r.next_bounded_int(15);
    (-65..=128).any(|y| Xoroshiro::new(seed ^ x.wrapping_add(y).wrapping_add(z) as i64).next_bounded_int(500) == 0)
}

fn desert_well(w: &mut StructWorld, r: &mut Xoroshiro, cx: i32, cz: i32) {
    let (x, y, z) = start(w, r, cx, cz);
    // canGenerateAt
    let mut rr = Xoroshiro::new(w.seed ^ x.wrapping_add(y).wrapping_add(z) as i64);
    if !tagged(w.biome(x, y, z), "desert") || rr.next_bounded_int(500) != 0 || y > 128 {
        return;
    }
    if !matches!(bl::short(w.get(x, y, z)), "sand" | "red_sand" | "suspicious_sand") {
        return;
    }
    for dx in -2..=2 {
        for dz in -2..=2 {
            if bl::is_air(w.get(x + dx, y - 1, z + dz)) && bl::is_air(w.get(x + dx, y - 2, z + dz)) {
                return;
            }
        }
    }
    let sandstone = id("sandstone");
    let water = id("water");
    let slab = id("sandstone_slab");
    let mut h = H { w, o: (x, y, z) };
    h.fill((-2, -1, -2), (2, 0, 2), sandstone);
    for (dx, dz) in [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)] {
        h.set(dx, 0, dz, water);
    }
    for dx in -2..=2 {
        for dz in -2..=2 {
            if dx == -2 || dx == 2 || dz == -2 || dz == 2 {
                h.set(dx, 1, dz, sandstone);
            }
        }
    }
    for (dx, dz) in [(2, 0), (-2, 0), (0, 2), (0, -2)] {
        h.set(dx, 1, dz, slab);
    }
    for dx in -1..=1 {
        for dz in -1..=1 {
            h.set(dx, 4, dz, if dx == 0 && dz == 0 { sandstone } else { slab });
        }
    }
    for dy in 1..=3 {
        for (dx, dz) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
            h.set(dx, dy, dz, sandstone);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Desert pyramid

fn desert_pyramid(w: &mut StructWorld, r: &mut Xoroshiro, cx: i32, cz: i32) {
    if !is_biome(surface_biome(w, cx, cz), &["desert", "desert_hills", "desert_mutated"]) {
        return;
    }
    let (x, y, z) = start(w, r, cx, cz);
    let ss = id("sandstone");
    let cut = id("cut_sandstone");
    let chis = id("chiseled_sandstone");
    let slab = id("sandstone_slab");
    let slab_up = with(slab, &[("minecraft:vertical_half", s("top"))]);
    let air = bl::air();
    let orange = id("orange_terracotta");
    let blue = id("blue_terracotta");
    let st = |d| with(id("sandstone_stairs"), &[("weirdo_direction", i(d))]);
    let (sn, se, ss_, sw) = (st(2), st(1), st(3), st(0));
    let chest = |d: &str| with(id("chest"), &[("minecraft:cardinal_direction", s(d))]);
    let mut h = H { w, o: (x, y - 20, z) };

    // Foundation: from bounding-box minY (-18) + 13 downwards until solid.
    for x2 in 0..21 {
        for z2 in 0..21 {
            let mut y2 = -5;
            while !bl::is_solid(h.get(x2, y2, z2)) && y2 > 1 {
                h.set(x2, y2, z2, ss);
                y2 -= 1;
            }
        }
    }
    h.fill((0, 14, 0), (20, 18, 20), ss);
    for i in 1..=9 {
        h.fill((i, i + 18, i), (20 - i, i + 18, 20 - i), ss);
        h.fill((i + 1, i + 18, i + 1), (19 - i, i + 18, 19 - i), air);
    }
    // east tower
    h.fill2((0, 18, 0), (4, 27, 4), ss, air);
    h.fill((1, 28, 1), (3, 28, 3), ss);
    h.set(2, 28, 0, sn);
    h.set(4, 28, 2, se);
    h.set(2, 28, 4, ss_);
    h.set(0, 28, 2, sw);
    h.fill((1, 19, 5), (3, 22, 11), ss);
    h.fill((2, 22, 4), (2, 24, 4), air);
    h.fill((1, 19, 3), (2, 20, 3), ss);
    h.set(1, 19, 2, ss);
    h.set(1, 20, 2, slab);
    h.set(2, 19, 2, se);
    for i in 0..2 {
        h.set(2, 21 + i, 4 + i, sn);
    }
    // west tower
    h.fill2((16, 18, 0), (20, 27, 4), ss, air);
    h.fill((17, 28, 1), (19, 28, 3), ss);
    h.set(18, 28, 0, sn);
    h.set(20, 28, 2, se);
    h.set(18, 28, 4, ss_);
    h.set(16, 28, 2, sw);
    h.fill((17, 19, 5), (19, 22, 11), ss);
    h.fill((18, 22, 4), (18, 24, 4), air);
    h.fill((18, 19, 3), (19, 20, 3), ss);
    h.set(19, 19, 2, ss);
    h.set(19, 20, 2, slab_up);
    h.set(18, 19, 2, sw);
    for i in 0..2 {
        h.set(18, 21 + i, 4 + i, sn);
    }
    // tower symbols
    for i in 0..2 {
        let f = i << 4;
        h.fill((1 + f, 20, 0), (1 + f, 21, 0), cut);
        h.fill((2 + f, 20, 0), (2 + f, 21, 0), orange);
        h.fill((3 + f, 20, 0), (3 + f, 21, 0), cut);
        for (dy, row) in [(22, [orange, chis, orange]), (23, [cut, orange, cut]), (24, [orange, chis, orange])] {
            for (k, v) in row.into_iter().enumerate() {
                h.set(1 + k as i32 + f, dy, 0, v);
            }
        }
        h.fill((1 + f, 25, 0), (3 + f, 25, 0), orange);
        h.fill((1 + f, 26, 0), (3 + f, 26, 0), cut);
        let sx = i * 20;
        h.fill((sx, 20, 1), (sx, 21, 1), cut);
        h.fill((sx, 20, 2), (sx, 21, 2), orange);
        h.fill((sx, 20, 3), (sx, 21, 3), cut);
        for (dy, row) in [(22, [orange, chis, orange]), (23, [cut, orange, cut]), (24, [orange, chis, orange])] {
            for (k, v) in row.into_iter().enumerate() {
                h.set(sx, dy, 1 + k as i32, v);
            }
        }
        h.fill((sx, 25, 1), (sx, 25, 3), orange);
        h.fill((sx, 26, 1), (sx, 26, 3), cut);
    }
    // front entrance
    h.fill2((8, 18, 1), (12, 22, 4), ss, air);
    h.fill((9, 19, 0), (11, 21, 4), air);
    h.fill((9, 19, 1), (9, 20, 1), cut);
    h.fill((11, 19, 1), (11, 20, 1), cut);
    h.fill((9, 21, 1), (11, 21, 1), cut);
    h.fill((8, 18, 0), (8, 21, 0), ss);
    h.fill((12, 18, 0), (12, 21, 0), ss);
    h.fill((8, 22, 0), (12, 22, 0), cut);
    for (k, v) in [cut, orange, chis, orange, cut].into_iter().enumerate() {
        h.set(8 + k as i32, 23, 0, v);
    }
    h.fill((9, 24, 0), (11, 24, 0), cut);
    // east / west entrances
    h.fill((5, 23, 9), (5, 25, 11), cut);
    h.fill((6, 25, 9), (6, 25, 11), ss);
    h.fill((5, 23, 10), (6, 24, 10), air);
    h.fill((15, 23, 9), (15, 25, 11), cut);
    h.fill((14, 25, 9), (14, 25, 11), ss);
    h.fill((14, 23, 10), (15, 24, 10), air);
    // corridors to towers
    h.fill2((4, 19, 1), (8, 21, 3), ss, air);
    h.fill((4, 19, 2), (8, 20, 2), air);
    h.fill2((12, 19, 1), (16, 21, 3), ss, air);
    h.fill((12, 19, 2), (16, 20, 2), air);
    // pillars
    for (px, pz) in [(8, 8), (12, 8), (12, 12), (8, 12)] {
        h.fill((px, 19, pz), (px, 21, pz), cut);
    }
    // 2nd floor
    h.fill((5, 22, 5), (15, 22, 15), ss);
    h.fill((9, 22, 9), (11, 22, 11), air);
    // east and west corridors
    h.fill((3, 19, 5), (3, 20, 11), air);
    h.fill((4, 21, 5), (4, 21, 16), ss);
    h.fill((17, 19, 5), (17, 20, 11), air);
    h.fill((16, 21, 5), (16, 21, 16), ss);
    h.fill((2, 19, 12), (2, 19, 18), ss);
    h.fill((18, 19, 12), (18, 19, 18), ss);
    h.fill((3, 19, 18), (18, 19, 18), ss);
    for i in 0..7 {
        h.set(4, 19, 5 + (i << 1), cut);
        h.set(4, 20, 5 + (i << 1), chis);
        h.set(16, 19, 5 + (i << 1), cut);
        h.set(16, 20, 5 + (i << 1), chis);
    }
    // floor symbols
    for (fx, fz) in [(9, 9), (11, 9), (11, 11), (9, 11)] {
        h.set(fx, 18, fz, orange);
    }
    h.set(10, 18, 10, blue);
    h.fill((10, 18, 7), (10, 18, 8), orange);
    h.fill((12, 18, 10), (13, 18, 10), orange);
    h.fill((10, 18, 12), (10, 18, 13), orange);
    h.fill((7, 18, 10), (8, 18, 10), orange);
    // trap chamber
    h.fill((8, 0, 8), (12, 3, 12), cut);
    h.fill((8, 4, 8), (12, 4, 12), chis);
    h.fill((8, 5, 8), (12, 5, 12), cut);
    h.fill((8, 6, 8), (12, 13, 12), ss);
    h.fill((9, 3, 9), (11, 17, 11), air);
    h.fill((9, 1, 9), (11, 1, 11), id("tnt"));
    h.fill((9, 2, 9), (11, 2, 11), cut);
    for (ax, az, bx, bz) in [(10, 8, 10, 7), (12, 10, 13, 10), (10, 12, 10, 13), (8, 10, 7, 10)] {
        h.fill((ax, 3, az), (ax, 4, az), air);
        h.set(bx, 3, bz, cut);
        h.set(bx, 4, bz, chis);
    }
    h.set(10, 3, 10, id("stone_pressure_plate"));
    // chests (loot tables skipped)
    h.set(10, 3, 12, chest("north"));
    h.set(8, 3, 10, chest("east"));
    h.set(10, 3, 8, chest("south"));
    h.set(12, 3, 10, chest("west"));
}

// ---------------------------------------------------------------------------------------------
// Jungle temple

fn jungle_temple(w: &mut StructWorld, r: &mut Xoroshiro, cx: i32, cz: i32) {
    if !is_biome(surface_biome(w, cx, cz), &["jungle"]) {
        return;
    }
    let (x, y, z) = start(w, r, cx, cz);
    // PNX uses ObjectJungleTemple's never-reseeded Xoroshiro (leaks between temples), we reseed.
    let cobble = id("cobblestone");
    let mossy = id("mossy_cobblestone");
    let air = bl::air();
    let st = |d| with(id("stone_stairs"), &[("weirdo_direction", i(d))]);
    let (sn, se, ss, sw) = (st(2), st(1), st(3), st(0));
    let wire = id("redstone_wire");
    let trip = id("trip_wire");
    let hook = |d| with(id("tripwire_hook"), &[("direction", i(d)), ("attached_bit", b(true))]);
    let mut h = H { w, o: (x, y - 3, z) };
    // getRandomBlock over {cobblestone: 4, mossy: 6} (insertion-ordered map)
    let stone = |r: &mut Xoroshiro| {
        let wt = r.next_bounded_int(10);
        if !(4..10).contains(&wt) { cobble } else { mossy }
    };
    let mut rf = |h: &mut H, a: (i32, i32, i32), m: (i32, i32, i32)| {
        for yy in a.1..=m.1 {
            for xx in a.0..=m.0 {
                for zz in a.2..=m.2 {
                    let v = stone(r);
                    h.set(xx, yy, zz, v);
                }
            }
        }
    };
    macro_rules! rs {
        ($x:expr, $y:expr, $z:expr) => {
            rf(&mut h, ($x, $y, $z), ($x, $y, $z))
        };
    }
    // 1st floor
    rf(&mut h, (0, 0, 0), (11, 0, 14));
    rf(&mut h, (0, 1, 0), (11, 3, 0));
    rf(&mut h, (11, 1, 1), (11, 3, 13));
    rf(&mut h, (0, 1, 1), (0, 3, 13));
    rf(&mut h, (0, 1, 14), (11, 3, 14));
    rf(&mut h, (0, 4, 0), (11, 4, 14));
    h.fill((4, 4, 0), (7, 4, 0), sn);
    h.fill((1, 1, 1), (10, 3, 13), air);
    h.fill((5, 4, 7), (6, 4, 9), air);
    // 2nd floor
    rf(&mut h, (2, 5, 2), (9, 6, 2));
    rf(&mut h, (9, 5, 3), (9, 6, 11));
    rf(&mut h, (2, 5, 12), (9, 6, 12));
    rf(&mut h, (2, 5, 3), (2, 6, 11));
    rf(&mut h, (1, 7, 1), (10, 7, 13));
    h.fill((3, 5, 3), (8, 6, 11), air);
    h.fill((4, 7, 6), (7, 7, 9), air);
    h.fill((5, 5, 2), (6, 6, 2), air);
    h.fill((5, 6, 12), (6, 6, 12), air);
    // 3rd floor
    rf(&mut h, (1, 8, 1), (10, 9, 1));
    rf(&mut h, (10, 8, 2), (10, 9, 12));
    rf(&mut h, (1, 8, 13), (10, 9, 13));
    rf(&mut h, (1, 8, 2), (1, 9, 12));
    h.fill((2, 8, 2), (9, 9, 12), air);
    h.fill((5, 9, 1), (6, 9, 1), air);
    h.fill((5, 9, 13), (6, 9, 13), air);
    for (ax, az) in [(10, 5), (10, 9), (1, 5), (1, 9)] {
        h.set(ax, 9, az, air);
    }
    // roof
    rf(&mut h, (1, 10, 1), (10, 10, 4));
    rf(&mut h, (8, 10, 5), (10, 10, 9));
    rf(&mut h, (1, 10, 5), (3, 10, 9));
    rf(&mut h, (1, 10, 10), (10, 10, 13));
    rf(&mut h, (3, 11, 3), (8, 11, 5));
    rf(&mut h, (7, 11, 6), (8, 11, 8));
    rf(&mut h, (3, 11, 6), (4, 11, 8));
    rf(&mut h, (3, 11, 9), (8, 11, 11));
    rf(&mut h, (4, 12, 4), (7, 12, 10));
    h.fill((4, 10, 5), (7, 10, 9), air);
    h.fill((5, 11, 6), (6, 11, 8), air);
    // outside walls decorations
    for dx in [2, 4, 7, 9] {
        rf(&mut h, (dx, 8, 0), (dx, 9, 0));
    }
    rf(&mut h, (5, 10, 0), (6, 10, 0));
    for i in 0..6 {
        rf(&mut h, (11, 8, 2 + (i << 1)), (11, 9, 2 + (i << 1)));
        rf(&mut h, (0, 8, 2 + (i << 1)), (0, 9, 2 + (i << 1)));
    }
    rs!(11, 10, 5);
    rs!(11, 10, 9);
    rs!(0, 10, 5);
    rs!(0, 10, 9);
    for dx in [2, 4, 7, 9] {
        rf(&mut h, (dx, 8, 14), (dx, 9, 14));
    }
    // roof decorations
    rf(&mut h, (2, 11, 2), (2, 13, 2));
    rf(&mut h, (9, 11, 2), (9, 13, 2));
    rf(&mut h, (9, 11, 12), (9, 13, 12));
    rf(&mut h, (2, 11, 12), (2, 13, 12));
    rs!(4, 13, 4);
    rs!(7, 13, 4);
    rs!(7, 13, 10);
    rs!(4, 13, 10);
    h.fill((5, 13, 6), (6, 13, 6), sn);
    rf(&mut h, (5, 13, 7), (6, 13, 7));
    h.fill((5, 13, 8), (6, 13, 8), ss);
    // 1st floor inside
    for i in 0..6 {
        rf(&mut h, (1, 3, 2 + (i << 1)), (3, 3, 2 + (i << 1)));
    }
    for i in 0..7 {
        rf(&mut h, (1, 1, 1 + (i << 1)), (1, 2, 1 + (i << 1)));
    }
    rs!(2, 2, 1);
    h.set(3, 1, 1, mossy);
    rf(&mut h, (4, 2, 1), (5, 2, 1));
    rs!(6, 1, 1);
    rs!(6, 3, 1);
    rf(&mut h, (7, 2, 1), (9, 2, 1));
    h.set(8, 1, 1, mossy);
    rf(&mut h, (10, 1, 1), (10, 3, 7));
    rf(&mut h, (9, 3, 1), (9, 3, 7));
    h.set(9, 1, 2, mossy);
    h.set(9, 1, 4, mossy);
    h.set(8, 1, 5, mossy);
    h.fill((7, 2, 5), (7, 3, 5), mossy);
    h.set(6, 1, 5, mossy);
    rs!(6, 2, 5);
    h.fill((5, 2, 5), (5, 3, 5), mossy);
    h.set(4, 1, 5, mossy);
    rf(&mut h, (7, 1, 6), (7, 3, 11));
    rf(&mut h, (4, 1, 6), (4, 3, 11));
    rf(&mut h, (5, 3, 11), (6, 3, 11));
    rf(&mut h, (8, 3, 11), (10, 3, 11));
    rf(&mut h, (8, 1, 11), (10, 1, 11));
    rf(&mut h, (5, 1, 8), (6, 1, 8));
    rf(&mut h, (6, 1, 7), (6, 2, 7));
    rs!(5, 2, 7);
    rf(&mut h, (6, 1, 6), (6, 3, 6));
    rf(&mut h, (5, 2, 6), (5, 3, 6));
    rf(&mut h, (8, 2, 6), (9, 2, 6));
    rs!(8, 3, 6);
    rf(&mut h, (9, 1, 7), (9, 2, 7));
    rf(&mut h, (8, 1, 7), (8, 3, 7));
    rf(&mut h, (10, 1, 8), (10, 1, 10));
    h.set(10, 2, 9, mossy);
    rf(&mut h, (8, 1, 8), (8, 1, 10));
    h.fill((8, 2, 11), (10, 2, 11), id("chiseled_stone_bricks"));
    h.fill((8, 2, 12), (10, 2, 12), with(id("lever"), &[("lever_direction", s("south"))]));
    let disp = |d| with(id("dispenser"), &[("facing_direction", i(d))]);
    h.set(3, 2, 1, disp(3)); // arrows (block entity) skipped
    h.set(9, 2, 3, disp(4));
    let vine = |d| with(id("vine"), &[("vine_direction_bits", i(d))]);
    h.set(3, 2, 2, vine(4));
    h.fill((8, 2, 3), (8, 3, 3), vine(8));
    h.fill((2, 1, 8), (3, 1, 8), trip);
    h.set(4, 1, 8, hook(1));
    h.set(1, 1, 8, hook(3));
    h.fill((5, 1, 1), (5, 1, 7), wire);
    h.set(4, 1, 1, wire);
    h.fill((7, 1, 2), (7, 1, 4), trip);
    h.set(7, 1, 1, hook(0));
    h.set(7, 1, 5, hook(2));
    h.fill((8, 1, 6), (9, 1, 6), wire);
    h.set(9, 1, 5, wire);
    h.set(9, 2, 4, wire);
    let piston = |d| with(id("sticky_piston"), &[("facing_direction", i(d))]);
    h.fill((10, 2, 8), (10, 3, 8), piston(5)); // piston arm block entities skipped
    h.set(9, 2, 8, piston(1));
    h.set(10, 3, 9, wire);
    h.fill((8, 2, 9), (8, 2, 10), wire);
    h.set(10, 2, 10, with(id("unpowered_repeater"), &[("minecraft:cardinal_direction", s("south")), ("repeater_delay", i(1))]));
    let chest = |d: &str| with(id("chest"), &[("minecraft:cardinal_direction", s(d))]);
    h.set(8, 1, 3, chest("west")); // loot skipped
    h.set(9, 1, 10, chest("north"));
    // 2nd floor inside
    for i in 0..4 {
        h.fill((5, 4 - i, 6 + i), (6, 4 - i, 6 + i), ss);
    }
    rf(&mut h, (4, 5, 10), (7, 6, 10));
    rs!(4, 5, 9);
    rs!(7, 5, 9);
    for i in 0..3 {
        h.set(7, 5 + i, 8 + i, sn);
        h.set(4, 5 + i, 8 + i, sn);
    }
    // 3rd floor inside
    rf(&mut h, (5, 8, 5), (6, 8, 5));
    h.set(7, 8, 5, se);
    h.set(4, 8, 5, sw);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overworld::Overworld;

    /// Finds a candidate chunk whose biome passes `ok`, generates the area and counts `block`s.
    fn find(seed: i64, ok: impl Fn(u16) -> bool, block: &str, h: fn(i64, i32, i32) -> bool) -> Option<bool> {
        let ow = Overworld::new(seed as u64);
        for rx in -6..6 {
            for rz in -6..6 {
                for cx in rx * 32..rx * 32 + 24 {
                    for cz in rz * 32..rz * 32 + 24 {
                        if !h(seed, cx, cz) {
                            continue;
                        }
                        let t = ow.generate_terrain(cx, cz);
                        let hy = t.heights[7 * 16 + 7];
                        if !ok(t.biome(7, hy - 1, 7)) {
                            continue;
                        }
                        for dx in 0..=1 {
                            for dz in 0..=1 {
                                let c = ow.generate(cx + dx, cz + dz);
                                let n = (0..16).flat_map(|x| (0..16).map(move |z| (x, z))).flat_map(|(x, z)| (-64..320).map(move |y| (x, y, z))).filter(|&(x, y, z)| bl::is(c.get(x, y, z), block)).count();
                                if n > 0 {
                                    return Some(true);
                                }
                            }
                        }
                        return Some(false);
                    }
                }
            }
        }
        None
    }

    #[test]
    fn desert_pyramid_places_terracotta() {
        assert_eq!(Some(true), find(1, |b| is_biome(b, &["desert", "desert_hills", "desert_mutated"]), "blue_terracotta", hint));
    }

    #[test]
    fn jungle_temple_places_mossy() {
        assert_eq!(Some(true), find(2, |b| is_biome(b, &["jungle"]), "sticky_piston", hint));
    }

    #[test]
    fn swamp_hut_places_cauldron() {
        assert_eq!(Some(true), (3..20).find_map(|seed| find(seed, |b| tagged(b, "swamp"), "cauldron", hint)));
    }
}

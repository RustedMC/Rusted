// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Jigsaw structures: villages, ancient cities, trail ruins, trial chambers (PNX `JigsawStructure` & co).
//! No mobs or NBT. PNX's per-block collision check never fires off-origin, so it's left out.

mod pools;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use super::super::feature::blocks::{self as bl, id};
use super::super::feature::chunk_hash;
use super::super::feature::objects::World;
use super::super::rng::BedrockRandom;
use super::templates::{self, Blocks, Jigsaw, Mirror, Rotation, Template};
use super::{NukkitRandom, Placement, Spread, StructWorld, StructureType, Xoroshiro};
use crate::blocks::State;
use pools::Pool;

const MIN_Y: i32 = -64;
const MAX_Y: i32 = 319;

const VILLAGE: Placement = Placement::new(10_387_312, 8, 34, Spread::Triangular);
const CITY: Placement = Placement::new(0x0132_7220, 8, 24, Spread::Triangular);
const TRAIL: Placement = Placement::new(83_469_867, 8, 34, Spread::Linear);
const TRIAL: Placement = Placement::new(94_251_327, 12, 34, Spread::Linear);

pub fn register(v: &mut Vec<StructureType>) {
    v.push(StructureType { name: "normal_village", radius: 9, hint: |s, x, z| candidate(&VILLAGE, s, x, z), build: village });
    v.push(StructureType { name: "normal_ancient_city", radius: 12, hint: |s, x, z| candidate(&CITY, s, x, z), build: ancient_city });
    v.push(StructureType { name: "normal_trail_ruins", radius: 6, hint: |s, x, z| candidate(&TRAIL, s, x, z), build: trail_ruins });
    v.push(StructureType { name: "normal_trial_chambers", radius: 9, hint: |s, x, z| candidate(&TRIAL, s, x, z), build: trial_chambers });
}

thread_local! {
    /// (seed, salt, region x, region z) -> candidate chunk; the big radii call the hint a lot.
    static CANDIDATES: RefCell<HashMap<(i64, i64, i32, i32), (i32, i32)>> = RefCell::new(HashMap::new());
}

/// `Placement::is_candidate`, memoised per region (each check seeds a Mersenne Twister).
fn candidate(p: &Placement, seed: i64, cx: i32, cz: i32) -> bool {
    let s = p.max_distance;
    let key = (seed, p.salt, cx.div_euclid(s), cz.div_euclid(s));
    let hit = CANDIDATES.with(|c| c.borrow().get(&key).copied());
    let (x, z) = match hit {
        Some(v) => v,
        None => {
            let (x0, z0) = (key.2 * s, key.3 * s);
            let v = (0..s).flat_map(|dx| (0..s).map(move |dz| (x0 + dx, z0 + dz))).find(|&(x, z)| p.is_candidate(seed, x, z)).unwrap_or((i32::MIN, i32::MIN));
            CANDIDATES.with(|c| {
                let mut c = c.borrow_mut();
                if c.len() > 65_536 {
                    c.clear();
                }
                c.insert(key, v);
            });
            v
        }
    };
    (x, z) == (cx, cz)
}

// ---------------------------------------------------------------------------------------------
// Structure definitions

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Village,
    City,
    Trail,
    Trial,
}

struct Def {
    kind: Kind,
    pools: &'static [Pool],
    entry: &'static str,
    max_depth: i32,
    start_jigsaw: &'static str,
    /// -1 = unlimited.
    max_dist: i32,
    /// Village lamp marker lifted one block (`getLampHeightOffset`).
    lamp: &'static str,
}

const fn village_def(pools: &'static [Pool], entry: &'static str, lamp: &'static str) -> Def {
    Def { kind: Kind::Village, pools, entry, max_depth: 7, start_jigsaw: "", max_dist: -1, lamp }
}

const PLAINS: Def = village_def(pools::PLAINS, "village/plains/town_centers", "");
const DESERT: Def = village_def(pools::DESERT, "village/desert/town_centers", "desert_lamp");
const SAVANNA: Def = village_def(pools::SAVANNA, "village/savanna/town_centers", "savanna_lamp_post");
const SNOWY: Def = village_def(pools::SNOWY, "village/snowy/town_centers", "");
const TAIGA: Def = village_def(pools::TAIGA, "village/taiga/town_centers", "taiga_lamp_post");
const ANCIENT_CITY: Def = Def { kind: Kind::City, pools: pools::ANCIENT_CITY, entry: "ancient_city/city_center", max_depth: 7, start_jigsaw: "city_anchor", max_dist: 128, lamp: "" };
const TRAIL_RUINS: Def = Def { kind: Kind::Trail, pools: pools::TRAIL_RUINS, entry: "trail_ruins/tower", max_depth: 7, start_jigsaw: "", max_dist: 80, lamp: "" };
const TRIAL_CHAMBERS: Def = Def { kind: Kind::Trial, pools: pools::TRIAL_CHAMBERS, entry: "trial_chambers/chamber/end", max_depth: 20, start_jigsaw: "", max_dist: 116, lamp: "" };

/// `VillagePopulator.getVillageForBiome`.
fn village_for(biome: u16) -> Option<&'static Def> {
    Some(match biome {
        2 => &DESERT,
        35 => &SAVANNA,
        5 => &TAIGA,
        12 | 30 => &SNOWY,
        1 | 129 | 186 => &PLAINS,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------------------------
// Populators

/// `findGenerationY`: the highest non-replaceable block in the chunk's (7, 7) column.
fn ground_y(w: &StructWorld, cx: i32, cz: i32) -> i32 {
    let (x, z) = ((cx << 4) + 7, (cz << 4) + 7);
    let mut y = w.height(x, z);
    while y > MIN_Y && bl::replaceable(w.get(x, y, z)) {
        y -= 1;
    }
    y
}

fn surface_biome(w: &StructWorld, cx: i32, cz: i32) -> u16 {
    let (x, z) = ((cx << 4) + 7, (cz << 4) + 7);
    w.biome(x, w.height(x, z) - 1, z)
}

fn village(w: &mut StructWorld, _: &mut Xoroshiro, cx: i32, cz: i32) {
    if !VILLAGE.is_candidate(w.seed, cx, cz) {
        return;
    }
    let Some(def) = village_for(surface_biome(w, cx, cz)) else { return };
    let o = [cx << 4, ground_y(w, cx, cz), cz << 4];
    let mut r = VILLAGE.post_spread_random(w.seed, cx, cz);
    assemble(w, def, o, &mut r);
}

fn ancient_city(w: &mut StructWorld, _: &mut Xoroshiro, cx: i32, cz: i32) {
    if !CITY.is_candidate(w.seed, cx, cz) {
        return;
    }
    // PNX predicts the biome at y -27 from the biome picker; ours comes from the terrain chunk.
    if w.biome((cx << 4) + 7, -27, (cz << 4) + 7) != 190 {
        return;
    }
    let mut r = CITY.post_spread_random(w.seed, cx, cz);
    assemble(w, &ANCIENT_CITY, [cx << 4, -51, cz << 4], &mut r);
}

fn trail_ruins(w: &mut StructWorld, r: &mut Xoroshiro, cx: i32, cz: i32) {
    if !TRAIL.is_candidate(w.seed, cx, cz) || !tagged(surface_biome(w, cx, cz), "has_structure_trail_ruins") {
        return;
    }
    r.set_seed(w.seed ^ chunk_hash(cx, cz));
    let o = [cx << 4, (MIN_Y + 1).max(ground_y(w, cx, cz) - 15), cz << 4];
    let mut f = r.fork();
    assemble(w, &TRAIL_RUINS, o, &mut f);
}

fn trial_chambers(w: &mut StructWorld, _: &mut Xoroshiro, cx: i32, cz: i32) {
    if !TRIAL.is_candidate(w.seed, cx, cz) {
        return;
    }
    let mut r = Xoroshiro::new(w.seed);
    let xm = r.next_long() | 1;
    let zm = r.next_long() | 1;
    r.set_seed((cx as i64).wrapping_mul(xm).wrapping_add((cz as i64).wrapping_mul(zm)) ^ w.seed);
    let y = -40 + r.next_bounded_int(20);
    assemble(w, &TRIAL_CHAMBERS, [cx << 4, y, cz << 4], &mut r);
}

// Biome tags from the vanilla biome definitions (same approach as small.rs).
fn tags() -> &'static HashMap<u16, Vec<String>> {
    static T: std::sync::OnceLock<HashMap<u16, Vec<String>>> = std::sync::OnceLock::new();
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

// ---------------------------------------------------------------------------------------------
// Jigsaw assembly (PNX `JigsawStructure.assemble`)

/// PNX `nextExclusiveInt` on the structure's random source.
trait Rand {
    fn below(&mut self, n: i32) -> i32;
}

impl Rand for BedrockRandom {
    fn below(&mut self, n: i32) -> i32 {
        self.next_int_below(n)
    }
}

impl Rand for Xoroshiro {
    fn below(&mut self, n: i32) -> i32 {
        self.next_exclusive_int(n)
    }
}

/// Inclusive box: x0, y0, z0, x1, y1, z1.
type Bb = [i32; 6];

const ROTATIONS: [Rotation; 4] = [Rotation::None, Rotation::R90, Rotation::R180, Rotation::R270];

fn norm(k: &str) -> &str {
    k.find(':').map_or(k, |i| &k[i + 1..])
}

fn is_empty_entry(k: &str) -> bool {
    let n = norm(k);
    n.is_empty() || n == "empty"
}

fn shuffle_from_end<T>(v: &mut [T], r: &mut impl Rand) {
    for i in (1..v.len()).rev() {
        let j = r.below(i as i32 + 1) as usize;
        v.swap(i, j);
    }
}

// Bedrock facing indices: 0 down, 1 up, 2 north, 3 south, 4 west, 5 east.
fn cw(f: i32) -> i32 {
    match f {
        2 => 5,
        5 => 3,
        3 => 4,
        4 => 2,
        f => f,
    }
}

fn rotate_face(f: i32, r: Rotation) -> i32 {
    match r {
        Rotation::None => f,
        Rotation::R90 => cw(cw(cw(f))),
        Rotation::R180 => cw(cw(f)),
        Rotation::R270 => cw(f),
    }
}

fn horizontal(f: i32) -> bool {
    (2..=5).contains(&f)
}

fn side(p: [i32; 3], f: i32) -> [i32; 3] {
    let d = match f {
        0 => [0, -1, 0],
        1 => [0, 1, 0],
        2 => [0, 0, -1],
        3 => [0, 0, 1],
        4 => [-1, 0, 0],
        _ => [1, 0, 0],
    };
    [p[0] + d[0], p[1] + d[1], p[2] + d[2]]
}

fn int_state(v: i32, k: &str) -> i32 {
    match crate::blocks::state(v, k) {
        Some(State::Int(i)) => *i,
        _ => 0,
    }
}

/// PNX `getJigsawOrientation`: (front, top) of an unrotated template jigsaw after `r`.
fn orientation(t: &Template, j: &Jigsaw, r: Rotation) -> Option<(i32, i32)> {
    let b = t.get(j.pos[0], j.pos[1], j.pos[2]).filter(|&b| bl::is(b, "jigsaw"))?;
    let front = rotate_face(int_state(b, "facing_direction"), r);
    if horizontal(front) {
        return Some((front, 1));
    }
    let rot = int_state(b, "rotation");
    let top = if front == 0 {
        [2, 4, 3, 5][(rot & 3) as usize]
    } else {
        [2, 5, 3, 4][(rot & 3) as usize]
    };
    Some((front, rotate_face(top, r)))
}

/// PNX `getOrderedJigsaws`: shuffled, then stably sorted by selection priority (high first).
fn ordered(src: &[Jigsaw], r: &mut impl Rand) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..src.len()).collect();
    shuffle_from_end(&mut idx, r);
    idx.sort_by_key(|&i| std::cmp::Reverse(src[i].selection_priority));
    idx
}

fn bbox(p: [i32; 3], t: &Template, r: Rotation) -> Bb {
    let (sx, sz) = t.rotated_size(r);
    [p[0], p[1], p[2], p[0] + sx - 1, p[1] + t.size[1] - 1, p[2] + sz - 1]
}

fn center(b: &Bb) -> [i32; 3] {
    [b[0] + (b[3] - b[0] + 1) / 2, b[1] + (b[4] - b[1] + 1) / 2, b[2] + (b[5] - b[2] + 1) / 2]
}

struct Piece {
    name: &'static str,
    t: &'static Template,
    pos: [i32; 3],
    rot: Rotation,
    /// Index of this piece's box in `occupied`.
    bb: usize,
    /// Rotated jigsaws relative to `pos`, after piece post-processing (same order as `t.jigsaws`).
    placed: Vec<Jigsaw>,
}

struct Asm<'a, 'w> {
    w: &'a StructWorld<'w>,
    def: &'static Def,
    staged: Blocks,
    occupied: Vec<Bb>,
    aliases: Vec<(&'static str, String)>,
}

impl Asm<'_, '_> {
    fn pool(&self, name: &str) -> Option<&'static Pool> {
        self.def.pools.iter().rev().find(|p| p.0 == name)
    }

    fn intersects(&self, a: &Bb, b: &Bb) -> bool {
        match self.def.kind {
            Kind::Village => a[3] >= b[0] && a[0] <= b[3] && a[5] >= b[2] && a[2] <= b[5],
            Kind::Trial => {
                let open = a[3] > b[0] && a[0] < b[3] && a[4] > b[1] && a[1] < b[4] && a[5] > b[2] && a[2] < b[5];
                let contains = |o: &Bb, i: &Bb| o[0] <= i[0] && o[3] >= i[3] && o[1] <= i[1] && o[4] >= i[4] && o[2] <= i[2] && o[5] >= i[5];
                open && !contains(a, b) && !contains(b, a)
            }
            _ => a[3] >= b[0] && a[0] <= b[3] && a[4] >= b[1] && a[1] <= b[4] && a[5] >= b[2] && a[2] <= b[5],
        }
    }

    fn within(&self, b: &Bb, c: [i32; 3]) -> bool {
        let m = self.def.max_dist;
        m < 0 || (b[0] >= c[0] - m && b[3] <= c[0] + m && b[1] >= c[1] - m && b[4] <= c[1] + m && b[2] >= c[2] - m && b[5] <= c[2] + m)
    }

    fn collides(&self, b: &Bb, ignored: usize) -> bool {
        self.occupied.iter().enumerate().any(|(i, o)| i != ignored && self.intersects(o, b))
    }

    /// PNX `placeStructurePiece` (bounds already checked): stage, post-process, merge.
    fn place(&mut self, pos: [i32; 3], rot: Rotation, t: &'static Template) -> Piece {
        let bb = bbox(pos, t, rot);
        let mut tmp = Blocks::default();
        t.for_each(rot, Mirror::None, |x, y, z, v| tmp.set(pos[0] + x, pos[1] + y, pos[2] + z, v));
        let mut js = t.jigsaws_rotated(rot, Mirror::None);
        for j in &mut js {
            j.pos = [j.pos[0] + pos[0], j.pos[1] + pos[1], j.pos[2] + pos[2]];
        }
        self.post_piece(&t.name, &mut tmp, &mut js);
        for j in &js {
            let fs = if bl::is(j.final_state, "jigsaw") { bl::air() } else { j.final_state };
            if !bl::is(fs, "structure_void") {
                tmp.set(j.pos[0], j.pos[1], j.pos[2], fs);
            }
        }
        self.staged.merge(&tmp);
        for j in &mut js {
            j.pos = [j.pos[0] - pos[0], j.pos[1] - pos[1], j.pos[2] - pos[2]];
        }
        self.occupied.push(bb);
        Piece { name: &t.name, t, pos, rot, bb: self.occupied.len() - 1, placed: js }
    }

    fn post_piece(&self, name: &str, tmp: &mut Blocks, js: &mut [Jigsaw]) {
        match self.def.kind {
            Kind::Village => self.village_piece(name, tmp, js),
            Kind::City => {
                let shrieker = bl::with(id("sculk_shrieker"), &[("can_summon", bl::b(true))]);
                for (x, y, z, v) in tmp.list() {
                    if bl::is(v, "jigsaw") {
                        tmp.set(x, y, z, shrieker);
                    }
                }
            }
            Kind::Trail => trail_gravel(self.w.seed, name, tmp),
            // Spawner rooms: remember which mob each trial spawner calls up.
            Kind::Trial => {
                if let Some((mob, baby)) = crate::trial::mob_for_template(name) {
                    for (x, y, z, v) in tmp.list() {
                        if bl::is(v, "trial_spawner") {
                            crate::trial::generated((x, y, z), mob, baby);
                        }
                    }
                }
            }
        }
    }

    // --- VillageStructure.postProcessStructurePiece

    fn village_piece(&self, name: &str, tmp: &mut Blocks, js: &mut [Jigsaw]) {
        if name.contains("lamp") || name.contains("_decoration_") {
            let blocks = tmp.list();
            let Some(&first) = blocks.first() else { return };
            let mut anchor = None::<(i32, i32, i32, i32)>;
            for &b in blocks.iter().filter(|b| !bl::is(b.3, "jigsaw")) {
                if anchor.is_none_or(|a| b.1 < a.1) {
                    anchor = Some(b);
                }
            }
            let a = anchor.unwrap_or(first);
            let dy = placement_y(self.w, a.0, a.2) - a.1;
            if dy != 0 {
                shift(tmp, js, dy);
            }
            if !self.def.lamp.is_empty() && name.contains(self.def.lamp) {
                shift(tmp, js, 1);
            }
            for (x, y, z, v) in tmp.list() {
                if bl::is_air(v) {
                    tmp.unset(x, y, z);
                }
            }
        } else if name.contains("/streets/") || name.contains("/terminators/") {
            let mut cols: HashMap<(i32, i32), i32> = HashMap::new();
            for (x, y, z, v) in tmp.list() {
                tmp.unset(x, y, z);
                if bl::is(v, "jigsaw") {
                    continue;
                }
                let h = *cols.entry((x, z)).or_insert_with(|| placement_y(self.w, x, z) - 1);
                if bl::is(v, "grass_path") || bl::is(v, "dirt_path") || bl::is(v, "grass_block") || bl::is(v, "grass") {
                    if !tmp.contains(x, h + 1, z) {
                        tmp.set(x, h + 1, z, bl::air());
                        tmp.set(x, h + 2, z, bl::air());
                    }
                    tmp.set(x, h, z, v);
                }
            }
            for j in js {
                let h = cols.get(&(j.pos[0], j.pos[2])).copied().unwrap_or_else(|| placement_y(self.w, j.pos[0], j.pos[2]) - 1);
                j.pos[1] = h + 1;
            }
        }
    }

    /// The whole of `assemble`; returns the terrain-adaptation pieces (box, ground y).
    fn run(&mut self, origin: [i32; 3], r: &mut impl Rand) -> Vec<(Bb, i32)> {
        let mut terrain = vec![];
        let Some(start) = self.pool(self.def.entry) else { return terrain };
        let rot = ROTATIONS[r.below(4) as usize];
        let Some(root) = self.place_root(origin, rot, start, r) else { return terrain };
        let c = center(&self.occupied[root.bb]);
        let adapts = |name: &str, kind: Kind| match kind {
            Kind::Village => !(name.contains("lamp") || name.contains("_decoration_") || name.contains("/streets/") || name.contains("/terminators/")),
            _ => true,
        };
        if adapts(root.name, self.def.kind) {
            let b = self.occupied[root.bb];
            terrain.push((b, b[1]));
        }
        let mut connected: HashSet<[i32; 3]> = HashSet::new();
        let mut pending: Vec<(Piece, i32, i32)> = vec![(root, 0, 0)];
        while !pending.is_empty() {
            let (piece, depth, _) = pending.remove(0);
            if depth >= self.def.max_depth {
                continue;
            }
            for si in ordered(&piece.t.jigsaws, r) {
                let src = &piece.t.jigsaws[si];
                let pj = &piece.placed[si];
                let parent_pos = [piece.pos[0] + pj.pos[0], piece.pos[1] + pj.pos[1], piece.pos[2] + pj.pos[2]];
                if connected.contains(&parent_pos) {
                    continue;
                }
                let key = norm(&src.pool);
                let next = self.aliases.iter().find(|a| a.0 == key).map_or(key, |a| a.1.as_str());
                let Some(pool) = self.pool(next) else { continue };
                let Some((t, crot, cpos, cjig)) = self.candidate(pool, &piece, src, parent_pos, &connected, c, r) else { continue };
                let child = self.place(cpos, crot, t);
                connected.insert(parent_pos);
                connected.insert(cjig);
                if adapts(child.name, self.def.kind) {
                    terrain.push((self.occupied[child.bb], cjig[1]));
                }
                if depth + 1 < self.def.max_depth {
                    let prio = src.placement_priority;
                    let at = pending.iter().position(|p| prio > p.2).unwrap_or(pending.len());
                    pending.insert(at, (child, depth + 1, prio));
                }
            }
        }
        terrain
    }

    fn place_root(&mut self, origin: [i32; 3], rot: Rotation, pool: &Pool, r: &mut impl Rand) -> Option<Piece> {
        let total: i32 = pool.2.iter().map(|e| e.1).sum();
        let mut target = r.below(total);
        let entry = pool.2.iter().find(|e| {
            let hit = target < e.1;
            target -= e.1;
            hit
        })?;
        if is_empty_entry(entry.0) {
            return None;
        }
        let t = templates::get(norm(entry.0))?;
        let mut pos = origin;
        if !self.def.start_jigsaw.is_empty() {
            let mut js = t.jigsaws_rotated(rot, Mirror::None);
            for i in 1..js.len() {
                let j = r.below(i as i32 + 1) as usize;
                js.swap(i, j);
            }
            let s = js.iter().find(|j| norm(&j.name) == self.def.start_jigsaw)?;
            pos = [pos[0] - s.pos[0], pos[1], pos[2] - s.pos[2]];
        }
        Some(self.place(pos, rot, t))
    }

    /// PNX `findCandidate` (+ fallback pool): (template, rotation, position, child jigsaw pos).
    #[allow(clippy::too_many_arguments)]
    fn candidate(&self, pool: &Pool, parent: &Piece, src: &Jigsaw, parent_pos: [i32; 3], connected: &HashSet<[i32; 3]>, c: [i32; 3], r: &mut impl Rand) -> Option<(&'static Template, Rotation, [i32; 3], [i32; 3])> {
        match self.candidate_in(pool, parent, src, parent_pos, connected, c, r) {
            Ok(v) => Some(v),
            Err(true) => None,
            Err(false) => {
                let fb = norm(pool.1);
                if fb.is_empty() {
                    return None;
                }
                let fb = self.pool(fb)?;
                self.candidate_in(fb, parent, src, parent_pos, connected, c, r).ok()
            }
        }
    }

    /// Err(true) = hit an empty entry (terminal).
    #[allow(clippy::too_many_arguments)]
    fn candidate_in(&self, pool: &Pool, parent: &Piece, src: &Jigsaw, parent_pos: [i32; 3], connected: &HashSet<[i32; 3]>, c: [i32; 3], r: &mut impl Rand) -> Result<(&'static Template, Rotation, [i32; 3], [i32; 3]), bool> {
        let mut entries: Vec<&str> = pool.2.iter().flat_map(|e| std::iter::repeat_n(e.0, e.1.max(0) as usize)).collect();
        shuffle_from_end(&mut entries, r);
        let Some((pf, ptop)) = orientation(parent.t, src, parent.rot) else { return Err(false) };
        let joint = if src.joint.trim().is_empty() { if horizontal(pf) { "aligned" } else { "rollable" } } else { src.joint.as_str() };
        let target = norm(&src.target);
        for e in entries {
            if is_empty_entry(e) {
                return Err(true);
            }
            let Some(t) = templates::get(norm(e)) else { continue };
            let mut rots = ROTATIONS;
            shuffle_from_end(&mut rots, r);
            for rot in rots {
                // resolveConnection
                let rotated = t.jigsaws_rotated(rot, Mirror::None);
                let mut conn = None;
                for ci in ordered(&t.jigsaws, r) {
                    let cj = &t.jigsaws[ci];
                    if norm(&cj.name) != target {
                        continue;
                    }
                    let Some((cf, ctop)) = orientation(t, cj, rot) else { continue };
                    // Faces must point at each other: the opposite face, floors and ceilings included
                    // (two clockwise turns only flip the walls, so up/down never matched).
                    if pf != (cf ^ 1) || !(joint == "rollable" || ptop == ctop) {
                        continue;
                    }
                    let wp = side(parent_pos, pf);
                    let p = rotated[ci].pos;
                    conn = Some(([wp[0] - p[0], wp[1] - p[1], wp[2] - p[2]], wp));
                    break;
                }
                let Some((cpos, cjig)) = conn else { continue };
                if connected.contains(&cjig) {
                    continue;
                }
                let b = bbox(cpos, t, rot);
                if !self.within(&b, c) || self.collides(&b, parent.bb) {
                    continue;
                }
                return Ok((t, rot, cpos, cjig));
            }
        }
        Err(false)
    }
}

/// `shiftWholePiece`: moves every non-jigsaw block and every jigsaw by `dy`.
fn shift(tmp: &mut Blocks, js: &mut [Jigsaw], dy: i32) {
    let mut out = Blocks::default();
    for (x, y, z, v) in tmp.list() {
        if !bl::is(v, "jigsaw") {
            out.set(x, y + dy, z, v);
        }
    }
    *tmp = out;
    for j in js {
        j.pos[1] += dy;
    }
}

/// `VillageStructure.getPlacementY`.
fn placement_y(w: &StructWorld, x: i32, z: i32) -> i32 {
    let h = w.height(x, z) - 1;
    if bl::is_water(w.get(x, h, z)) {
        return h + 1;
    }
    let mut y = w.height(x, z);
    while y > MIN_Y {
        let b = w.get(x, y, z);
        // snow cover, replaceable or transparent
        if !(bl::is(b, "snow_layer") || bl::is(b, "snow") || bl::replaceable(b) || !bl::is_full(b)) {
            break;
        }
        y -= 1;
    }
    y + 1
}

/// `TrailRuinsStructure.postProcessStructurePiece`: some gravel becomes dirt / suspicious gravel.
fn trail_gravel(seed: i64, name: &str, tmp: &mut Blocks) {
    let mut gravel: Vec<(i32, i32, i32)> = tmp.list().into_iter().filter(|b| bl::is(b.3, "gravel")).map(|b| (b.0, b.1, b.2)).collect();
    let Some(&(x, y, z)) = gravel.first() else { return };
    let s = seed ^ 0x9E37_79B9_7F4A_7C15u64.wrapping_mul(x as i64 as u64) as i64;
    let s = s ^ 0xC2B2_AE3D_27D4_EB4Fu64.wrapping_mul(y as i64 as u64) as i64;
    let s = s ^ 0x1656_67B1_9E37_79F9u64.wrapping_mul(z as i64 as u64) as i64;
    let mut r = NukkitRandom::new(s);
    let top = name.contains("/tower/tower_top_");
    let road = name.contains("/roads/");
    let mut replace = |n: usize, v: i32, g: &mut Vec<(i32, i32, i32)>| {
        for _ in 0..n {
            if g.is_empty() {
                break;
            }
            let i = r.next_bounded_int(g.len() as i32 - 1) as usize;
            let (x, y, z) = g.remove(i.min(g.len() - 1));
            tmp.set(x, y, z, v);
        }
    };
    if !top {
        replace(gravel.len() / 5, id("dirt"), &mut gravel);
        replace(gravel.len() / 10, bl::with(id("dirt"), &[("dirt_type", bl::s("coarse"))]), &mut gravel);
    }
    let sus = id("suspicious_gravel");
    replace(if top || road { 2 } else { 6 }, sus, &mut gravel);
    replace(if top || road { 0 } else { 3 }, sus, &mut gravel);
}

// ---------------------------------------------------------------------------------------------
// Beardifier with a ground y per piece (templates.rs only has the box-floor one).

const KR: i32 = 12;

fn kernel() -> &'static [f64] {
    static K: std::sync::OnceLock<Vec<f64>> = std::sync::OnceLock::new();
    K.get_or_init(|| {
        let s = (KR * 2) as usize;
        let mut k = vec![0.0; s * s * s];
        for zi in 0..s {
            for xi in 0..s {
                for yi in 0..s {
                    let (dx, dy, dz) = (xi as f64 - KR as f64, yi as f64 - KR as f64 + 0.5, zi as f64 - KR as f64);
                    k[zi * s * s + xi * s + yi] = (-(dx * dx + dy * dy + dz * dz) / 16.0).exp();
                }
            }
        }
        k
    })
}

fn contribution(dx: i32, dy: i32, dz: i32, y_to_ground: i32) -> f64 {
    let (xi, yi, zi) = (dx + KR, dy + KR, dz + KR);
    let r = 0..KR * 2;
    if !r.contains(&xi) || !r.contains(&yi) || !r.contains(&zi) {
        return 0.0;
    }
    let d = y_to_ground as f64 + 0.5;
    let dist = dx as f64 * dx as f64 + d * d + dz as f64 * dz as f64;
    let v = -d / (dist / 2.0).sqrt() / 2.0;
    let s = (KR * 2) as usize;
    v * kernel()[zi as usize * s * s + xi as usize * s + yi as usize]
}

fn dist_to_range(v: i32, lo: i32, hi: i32) -> i32 {
    0.max((lo - v).max(v - hi))
}

fn fillable(b: i32) -> bool {
    bl::replaceable(b) || !bl::is_solid(b)
}

/// Village: biome surface at 0.03. City: carve air / fill deepslate at +-0.03.
fn beard(w: &StructWorld, staged: &mut Blocks, pieces: &[(Bb, i32)], city: bool) {
    let deepslate = id("deepslate");
    let air = bl::air();
    for &(b, g) in pieces {
        let min_y = MIN_Y.max(g - KR);
        let max_y = MAX_Y.min(if city { b[4] } else { g } + KR);
        for x in b[0] - KR..=b[3] + KR {
            for z in b[2] - KR..=b[5] + KR {
                let (dx, dz) = (dist_to_range(x, b[0], b[3]), dist_to_range(z, b[2], b[5]));
                if dx >= KR || dz >= KR {
                    continue;
                }
                let mats = if city { None } else { surface_materials(w, x, b[1], z) };
                if !city && mats.is_none() {
                    continue;
                }
                let mut highest = None;
                for y in min_y..=max_y {
                    if staged.contains(x, y, z) {
                        continue;
                    }
                    let dy = if city { dist_to_range(y, g, b[4]) } else { (y - g).abs() };
                    if dy >= KR {
                        continue;
                    }
                    let c = contribution(dx, dy, dz, y - g) * 0.8;
                    if city {
                        if c < -0.03 {
                            staged.set(x, y, z, air);
                        } else if c > 0.03 && fillable(w.get(x, y, z)) {
                            staged.set(x, y, z, deepslate);
                        }
                    } else if c > 0.03 && fillable(w.get(x, y, z)) {
                        staged.set(x, y, z, mats.unwrap().1);
                        highest = Some(y);
                    }
                }
                if let (Some(y), Some((top, _))) = (highest, mats) {
                    staged.set(x, y, z, top);
                }
            }
        }
    }
}

/// `getBiomeSurfaceMaterials`: (top, mid) of the biome at (x, y, z), with the noise adjustment.
fn surface_materials(w: &StructWorld, x: i32, y: i32, z: i32) -> Option<(i32, i32)> {
    let s = super::super::surfaces().get(&w.biome(x, y, z))?;
    let (mut top, mut mid) = (s.top, s.mid);
    if !s.adjust.is_empty() {
        let v = w.ow.router.surface.value(x as f64, 0.0, z as f64) as f32;
        for &(lo, hi, t, m, _) in &s.adjust {
            if v < hi && v > lo {
                top = t.unwrap_or(top);
                mid = m.unwrap_or(mid);
            }
        }
    }
    Some((top, mid))
}

fn assemble(w: &mut StructWorld, def: &'static Def, origin: [i32; 3], r: &mut impl Rand) {
    let mut a = Asm { w, def, staged: Blocks::default(), occupied: vec![], aliases: vec![] };
    if def.kind == Kind::Trial {
        // createPoolAliases
        let ranged = ["skeleton", "stray", "poison_skeleton"][r.below(3) as usize];
        a.aliases.push(("trial_chambers/spawner/contents/ranged", format!("trial_chambers/spawner/ranged/{ranged}")));
        a.aliases.push(("trial_chambers/spawner/contents/slow_ranged", format!("trial_chambers/spawner/slow_ranged/{ranged}")));
        let melee = ["zombie", "husk", "spider"][r.below(3) as usize];
        a.aliases.push(("trial_chambers/spawner/contents/melee", format!("trial_chambers/spawner/melee/{melee}")));
        let small = ["slime", "cave_spider", "silverfish", "baby_zombie"][r.below(4) as usize];
        a.aliases.push(("trial_chambers/spawner/contents/small_melee", format!("trial_chambers/spawner/small_melee/{small}")));
    }
    let terrain = a.run(origin, r);
    let mut staged = std::mem::take(&mut a.staged);
    match def.kind {
        Kind::Village => {
            beard(w, &mut staged, &terrain, false);
            for (x, y, z, v) in staged.list() {
                if bl::is(v, "jigsaw") {
                    staged.set(x, y, z, bl::air());
                }
            }
        }
        Kind::City => beard(w, &mut staged, &terrain, true),
        _ => {}
    }
    staged.write(w);
}

#[cfg(test)]
mod tests;

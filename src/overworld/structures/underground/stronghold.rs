// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! PNX `StrongholdPopulator`, `StrongholdPlacement` and `StrongholdPieces`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use super::super::super::feature::blocks as bl;
use super::super::{Placement, Spread, StructWorld, Xoroshiro};
use super::piece::{BBox, Base, Face, Lvl, collides};
use crate::overworld::biome::id as b;
use crate::overworld::rng::BedrockRandom;

/// PNX `START_SEARCH_RADIUS`.
pub(super) const RADIUS: i32 = 8;

// ---- placement ----

const FAR_REGION: i32 = 200;
const VILLAGE: Placement = Placement::new(10_387_312, 8, 34, Spread::Triangular);

fn origin_excluded(cx: i32, cz: i32) -> bool {
    (cx.wrapping_mul(cx).wrapping_add(cz.wrapping_mul(cz)) as u32) < 100
}

fn far_region(c: i32) -> i32 {
    (c as f32 / FAR_REGION as f32).floor() as i32
}

fn far_field_start(seed: i64, cx: i32, cz: i32) -> bool {
    if origin_excluded(cx, cz) {
        return false;
    }
    let (rx, rz) = (far_region(cx), far_region(cz));
    let (mx, mz) = (rx * FAR_REGION + FAR_REGION / 2, rz * FAR_REGION + FAR_REGION / 2);
    if origin_excluded(mx, mz) {
        return false;
    }
    let s = mx
        .wrapping_mul(0x9BAB_EB51u32 as i32)
        .wrapping_add(mz.wrapping_mul(0xBE68_AEB9u32 as i32))
        .wrapping_add(seed as i32)
        .wrapping_add(0x05D5_34E7);
    let mut r = BedrockRandom::new(s as i64);
    let ox = r.next_int_below(100) + 50;
    let oz = r.next_int_below(100) + 50;
    r.next_float() < 0.25 && rx * FAR_REGION + ox == cx && rz * FAR_REGION + oz == cz
}

fn mc_sin_index(i: i32) -> f32 {
    ((i & 0xFFFF) as f64 * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32
}

fn village_biome(id: u16) -> bool {
    matches!(id, b::DESERT | b::SAVANNA | b::TAIGA | b::ICE_PLAINS | b::COLD_TAIGA | b::PLAINS | b::SUNFLOWER_PLAINS | b::MEADOW)
}

/// The three village-anchored strongholds (PNX `createInitialStrongholds`), cached per seed.
fn initial(seed: i64) -> Arc<Vec<(i32, i32)>> {
    static C: OnceLock<Mutex<HashMap<i64, Arc<Vec<(i32, i32)>>>>> = OnceLock::new();
    let mut m = C.get_or_init(Default::default).lock().unwrap();
    if let Some(v) = m.get(&seed) {
        return v.clone();
    }
    let router = crate::overworld::router::Router::new(seed);
    let mut r = BedrockRandom::new(seed);
    let mut angle = r.next_float() * 6.283_185_5f32;
    let mut radius = r.next_int_below(16) + 40;
    let mut out = Vec::new();
    let mut attempt = 0;
    while out.len() < 3 && attempt < 1000 {
        let cos = mc_sin_index((angle * 10430.378f32 + 16384.0f32) as i32);
        let sin = mc_sin_index((angle * 10430.378f32) as i32);
        let (tx, tz) = ((radius as f32 * cos).floor() as i32, (radius as f32 * sin).floor() as i32);
        let found = (tx - 8..tx + 8).flat_map(|x| (tz - 8..tz + 8).map(move |z| (x, z))).find(|&(x, z)| {
            VILLAGE.is_candidate(seed, x, z) && village_biome(super::raw_biome(&router, (x << 4) + 8, super::super::super::aquifer::SEA_LEVEL, (z << 4) + 8))
        });
        if let Some(v) = found {
            out.push(v);
            radius += 8;
            angle += 1.884_955_8;
        } else {
            radius += 4;
            angle += std::f32::consts::FRAC_PI_4;
        }
        attempt += 1;
    }
    let v = Arc::new(out);
    m.insert(seed, v.clone());
    v
}

/// PNX `StrongholdPlacement.canGenerate` (exact; seed only).
pub(super) fn is_start(seed: i64, cx: i32, cz: i32) -> bool {
    far_field_start(seed, cx, cz) || initial(seed).contains(&(cx, cz))
}

// ---- pieces ----

#[derive(Clone, Copy, PartialEq, Debug)]
enum Door {
    Opening,
    Wood,
    Grates,
    Iron,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum K {
    Straight,
    PrisonHall,
    LeftTurn,
    RightTurn,
    RoomCrossing,
    StraightStairsDown,
    StairsDown,
    FiveCrossing,
    ChestCorridor,
    Library,
    PortalRoom,
}

const WEIGHTS: [(K, i32, i32); 11] = [
    (K::Straight, 40, 0),
    (K::PrisonHall, 5, 5),
    (K::LeftTurn, 20, 0),
    (K::RightTurn, 20, 0),
    (K::RoomCrossing, 10, 6),
    (K::StraightStairsDown, 5, 5),
    (K::StairsDown, 5, 5),
    (K::FiveCrossing, 5, 4),
    (K::ChestCorridor, 5, 4),
    (K::Library, 10, 2),
    (K::PortalRoom, 20, 1),
];

enum Kind {
    Filler { steps: i32 },
    StairsDown { source: bool },
    Straight { left: bool, right: bool },
    ChestCorridor { placed: bool },
    StraightStairsDown,
    LeftTurn,
    RightTurn,
    RoomCrossing { ty: i32 },
    PrisonHall,
    Library { tall: bool },
    FiveCrossing { ll: bool, lh: bool, rl: bool, rh: bool },
    PortalRoom { spawner: bool },
}

struct Piece {
    b: Base,
    door: Door,
    kind: Kind,
}

struct Gen<'r> {
    pieces: Vec<Piece>,
    pending: Vec<usize>,
    counts: [i32; 11],
    current: Vec<usize>,
    total: i32,
    imposed: Option<K>,
    prev: Option<usize>,
    portal: bool,
    r: &'r mut Xoroshiro,
}

fn ok_box(b: &BBox) -> bool {
    b.y0 > 10
}

impl Gen<'_> {
    fn collides(&self, b: &BBox) -> bool {
        collides(self.pieces.iter().map(|p| &p.b.bb), b)
    }

    fn door(&mut self) -> Door {
        match self.r.next_int_below(5) {
            2 => Door::Wood,
            3 => Door::Grates,
            4 => Door::Iron,
            _ => Door::Opening,
        }
    }

    fn create(&mut self, k: K, x: i32, y: i32, z: i32, o: Face, depth: i32) -> Option<Piece> {
        let ob = |xo, yo, zo, xl, yl, zl| BBox::orient(x, y, z, xo, yo, zo, xl, yl, zl, o);
        let bb = match k {
            K::Straight | K::ChestCorridor => ob(-1, -1, 0, 5, 5, 7),
            K::PrisonHall => ob(-1, -1, 0, 9, 5, 11),
            K::LeftTurn | K::RightTurn => ob(-1, -1, 0, 5, 5, 5),
            K::RoomCrossing => ob(-4, -1, 0, 11, 7, 11),
            K::StraightStairsDown => ob(-1, -7, 0, 5, 11, 8),
            K::StairsDown => ob(-1, -7, 0, 5, 11, 5),
            K::FiveCrossing => ob(-4, -3, 0, 10, 9, 11),
            K::PortalRoom => ob(-4, -1, 0, 11, 8, 16),
            K::Library => {
                let t = ob(-4, -1, 0, 14, 11, 15);
                if ok_box(&t) && !self.collides(&t) {
                    t
                } else {
                    ob(-4, -1, 0, 14, 6, 15)
                }
            }
        };
        if !ok_box(&bb) || self.collides(&bb) {
            return None;
        }
        let base = Base::new(bb, depth, Some(o));
        if k == K::PortalRoom {
            return Some(Piece { b: base, door: Door::Opening, kind: Kind::PortalRoom { spawner: false } });
        }
        let door = self.door();
        let kind = match k {
            K::Straight => {
                let left = self.r.next_int_below(2) == 0;
                let right = self.r.next_int_below(2) == 0;
                Kind::Straight { left, right }
            }
            K::ChestCorridor => Kind::ChestCorridor { placed: false },
            K::PrisonHall => Kind::PrisonHall,
            K::LeftTurn => Kind::LeftTurn,
            K::RightTurn => Kind::RightTurn,
            K::RoomCrossing => Kind::RoomCrossing { ty: self.r.next_int_below(5) },
            K::StraightStairsDown => Kind::StraightStairsDown,
            K::StairsDown => Kind::StairsDown { source: false },
            K::FiveCrossing => {
                let ll = self.r.next_boolean();
                let lh = self.r.next_boolean();
                let rl = self.r.next_boolean();
                let rh = self.r.next_int_below(3) > 0;
                Kind::FiveCrossing { ll, lh, rl, rh }
            }
            K::Library => Kind::Library { tall: bb.y_span() > 6 },
            K::PortalRoom => unreachable!(),
        };
        Some(Piece { b: base, door, kind })
    }

    fn update_weights(&mut self) -> bool {
        self.total = self.current.iter().map(|&i| WEIGHTS[i].1).sum();
        self.current.iter().any(|&i| WEIGHTS[i].2 > 0 && self.counts[i] < WEIGHTS[i].2)
    }

    fn do_place(&self, i: usize, depth: i32) -> bool {
        let max = WEIGHTS[i].2;
        (max == 0 || self.counts[i] < max)
            && match WEIGHTS[i].0 {
                K::Library => depth > 4,
                K::PortalRoom => depth > 5,
                _ => true,
            }
    }

    /// PNX `generatePieceFromSmallDoor`.
    #[allow(clippy::wrong_self_convention)] // PNX name
    fn from_small_door(&mut self, x: i32, y: i32, z: i32, o: Face, depth: i32) -> Option<Piece> {
        if !self.update_weights() {
            return None;
        }
        if let Some(k) = self.imposed.take()
            && let Some(p) = self.create(k, x, y, z, o, depth) {
                return Some(p);
            }
        for _ in 0..5 {
            let mut target = self.r.next_int_below(self.total);
            let mut ci = 0;
            while ci < self.current.len() {
                let wi = self.current[ci];
                target -= WEIGHTS[wi].1;
                if target < 0 {
                    if !self.do_place(wi, depth) || self.prev == Some(wi) {
                        break;
                    }
                    if let Some(p) = self.create(WEIGHTS[wi].0, x, y, z, o, depth) {
                        self.counts[wi] += 1;
                        self.prev = Some(wi);
                        let max = WEIGHTS[wi].2;
                        if !(max == 0 || self.counts[wi] < max) {
                            self.current.remove(ci);
                        }
                        return Some(p);
                    }
                }
                ci += 1;
            }
        }
        // FillerCorridor.findPieceBox
        let bb = BBox::orient(x, y, z, -1, -1, 0, 5, 5, 4, o);
        let hit = self.pieces.iter().find(|p| p.b.bb.intersects(&bb)).map(|p| p.b.bb)?;
        if hit.y0 != bb.y0 {
            return None;
        }
        for m in (1..=3).rev() {
            if !hit.intersects(&BBox::orient(x, y, z, -1, -1, 0, 5, 5, m - 1, o)) {
                let bb = BBox::orient(x, y, z, -1, -1, 0, 5, 5, m, o);
                if bb.y0 <= 1 {
                    return None;
                }
                let steps = if o.axis_z() { bb.z_span() } else { bb.x_span() };
                return Some(Piece { b: Base::new(bb, depth, Some(o)), door: Door::Opening, kind: Kind::Filler { steps } });
            }
        }
        None
    }

    fn add(&mut self, x: i32, y: i32, z: i32, o: Face, depth: i32) {
        let s = self.pieces[0].b.bb;
        if depth > 50 || (x - s.x0).abs() > 112 || (z - s.z0).abs() > 112 {
            return;
        }
        if let Some(p) = self.from_small_door(x, y, z, o, depth + 1) {
            self.pieces.push(p);
            self.pending.push(self.pieces.len() - 1);
        }
    }

    fn forward(&mut self, i: usize, x: i32, y: i32) {
        let p = &self.pieces[i].b;
        let (bb, d) = (p.bb, p.depth);
        match p.orient {
            Some(o @ Face::North) => self.add(bb.x0 + x, bb.y0 + y, bb.z0 - 1, o, d),
            Some(o @ Face::South) => self.add(bb.x0 + x, bb.y0 + y, bb.z1 + 1, o, d),
            Some(o @ Face::West) => self.add(bb.x0 - 1, bb.y0 + y, bb.z0 + x, o, d),
            Some(o @ Face::East) => self.add(bb.x1 + 1, bb.y0 + y, bb.z0 + x, o, d),
            None => {}
        }
    }

    fn left(&mut self, i: usize, y: i32, z: i32) {
        let p = &self.pieces[i].b;
        let (bb, d) = (p.bb, p.depth);
        match p.orient {
            Some(Face::North | Face::South) => self.add(bb.x0 - 1, bb.y0 + y, bb.z0 + z, Face::West, d),
            Some(_) => self.add(bb.x0 + z, bb.y0 + y, bb.z0 - 1, Face::North, d),
            None => {}
        }
    }

    fn right(&mut self, i: usize, y: i32, z: i32) {
        let p = &self.pieces[i].b;
        let (bb, d) = (p.bb, p.depth);
        match p.orient {
            Some(Face::North | Face::South) => self.add(bb.x1 + 1, bb.y0 + y, bb.z0 + z, Face::East, d),
            Some(_) => self.add(bb.x0 + z, bb.y0 + y, bb.z1 + 1, Face::South, d),
            None => {}
        }
    }

    fn children(&mut self, i: usize) {
        let o = self.pieces[i].b.orient;
        let ne = matches!(o, Some(Face::North | Face::East));
        match self.pieces[i].kind {
            Kind::StairsDown { source } => {
                if source {
                    self.imposed = Some(K::FiveCrossing);
                }
                self.forward(i, 1, 1);
            }
            Kind::Straight { left, right } => {
                self.forward(i, 1, 1);
                if left {
                    self.left(i, 1, 2);
                }
                if right {
                    self.right(i, 1, 2);
                }
            }
            Kind::ChestCorridor { .. } | Kind::StraightStairsDown | Kind::PrisonHall => self.forward(i, 1, 1),
            Kind::LeftTurn => {
                if ne { self.left(i, 1, 1) } else { self.right(i, 1, 1) }
            }
            Kind::RightTurn => {
                if ne { self.right(i, 1, 1) } else { self.left(i, 1, 1) }
            }
            Kind::RoomCrossing { .. } => {
                self.forward(i, 4, 1);
                self.left(i, 1, 4);
                self.right(i, 1, 4);
            }
            Kind::FiveCrossing { ll, lh, rl, rh } => {
                let (mut low, mut high) = (3, 5);
                if matches!(o, Some(Face::West | Face::North)) {
                    low = 8 - low;
                    high = 8 - high;
                }
                self.forward(i, 5, 1);
                if ll {
                    self.left(i, low, 1);
                }
                if lh {
                    self.left(i, high, 7);
                }
                if rl {
                    self.right(i, low, 1);
                }
                if rh {
                    self.right(i, high, 7);
                }
            }
            Kind::PortalRoom { .. } => self.portal = true,
            Kind::Filler { .. } | Kind::Library { .. } => {}
        }
    }
}

struct Ids {
    air: i32,
    stone_bricks: i32,
    mossy: i32,
    cracked: i32,
    infested: i32,
    brick_slab: i32,
    smooth_slab: i32,
    smooth_double: i32,
    cobble: i32,
    water: i32,
    water1: i32,
    water9: i32,
    lava: i32,
    fence: i32,
    planks: i32,
    oak_door: i32,
    oak_door_u: i32,
    iron_door: i32,
    iron_door_u: i32,
    iron_door_w: i32,
    iron_door_w_u: i32,
    bars: i32,
    torch: i32,
    torch_n: i32,
    torch_e: i32,
    torch_s: i32,
    torch_w: i32,
    end_portal: i32,
    bookshelf: i32,
    web: i32,
    spawner: i32,
    ladder_w: i32,
    ladder_s: i32,
    brick_stairs_n: i32,
    stairs_s: i32,
    button_2: i32,
    button_3: i32,
}

fn ids() -> &'static Ids {
    static I: OnceLock<Ids> = OnceLock::new();
    I.get_or_init(|| {
        let w = |n: &str, st: &[(&str, crate::blocks::State)]| bl::with(bl::id(n), st);
        let card = |n: &str, d: &str, up: bool| w(n, &[("minecraft:cardinal_direction", bl::s(d)), ("upper_block_bit", bl::b(up))]);
        let torch = |d: &str| w("torch", &[("torch_facing_direction", bl::s(d))]);
        Ids {
            air: bl::air(),
            stone_bricks: bl::id("stone_bricks"),
            mossy: bl::id("mossy_stone_bricks"),
            cracked: bl::id("cracked_stone_bricks"),
            infested: bl::id("infested_stone_bricks"),
            brick_slab: bl::id("stone_brick_slab"),
            smooth_slab: bl::id("smooth_stone_slab"),
            smooth_double: bl::id("smooth_stone_double_slab"),
            cobble: bl::id("cobblestone"),
            water: bl::id("water"),
            water1: w("water", &[("liquid_depth", bl::i(1))]),
            water9: w("water", &[("liquid_depth", bl::i(9))]),
            lava: bl::id("lava"),
            fence: bl::id("oak_fence"),
            planks: bl::id("oak_planks"),
            oak_door: card("wooden_door", "east", false),
            oak_door_u: card("wooden_door", "east", true),
            iron_door: card("iron_door", "east", false),
            iron_door_u: card("iron_door", "east", true),
            iron_door_w: card("iron_door", "west", false),
            iron_door_w_u: card("iron_door", "west", true),
            bars: bl::id("iron_bars"),
            torch: torch("top"),
            torch_n: torch("south"),
            torch_e: torch("west"),
            torch_s: torch("north"),
            torch_w: torch("east"),
            end_portal: bl::id("end_portal"),
            bookshelf: bl::id("bookshelf"),
            web: bl::id("web"),
            spawner: bl::id("mob_spawner"),
            ladder_w: w("ladder", &[("facing_direction", bl::i(4))]),
            ladder_s: w("ladder", &[("facing_direction", bl::i(3))]),
            brick_stairs_n: w("stone_brick_stairs", &[("weirdo_direction", bl::i(3))]),
            stairs_s: w("stone_stairs", &[("weirdo_direction", bl::i(2))]),
            button_2: w("stone_button", &[("facing_direction", bl::i(2))]),
            button_3: w("stone_button", &[("facing_direction", bl::i(3))]),
        }
    })
}

/// PNX `SmoothStoneSelector`.
fn smooth(r: &mut Xoroshiro, edge: bool) -> i32 {
    let k = ids();
    if !edge {
        return k.air;
    }
    match r.next_int_below(100) {
        c if c < 20 => k.cracked,
        c if c < 50 => k.mossy,
        c if c < 55 => k.infested,
        _ => k.stone_bricks,
    }
}

#[allow(clippy::too_many_arguments)]
fn small_door(p: &Base, l: &mut Lvl, cb: &BBox, d: Door, x: i32, y: i32, z: i32) {
    let k = ids();
    let frame = |l: &mut Lvl, b: i32| {
        for (dx, dy) in [(0, 0), (0, 1), (0, 2), (1, 2), (2, 2), (2, 1), (2, 0)] {
            p.place(l, b, x + dx, y + dy, z, cb);
        }
    };
    match d {
        Door::Opening => p.fill(l, cb, x, y, z, x + 2, y + 2, z, k.air, k.air, false),
        Door::Wood => {
            frame(l, k.stone_bricks);
            p.place(l, k.oak_door, x + 1, y, z, cb);
            p.place(l, k.oak_door_u, x + 1, y + 1, z, cb);
        }
        Door::Grates => {
            p.place(l, k.air, x + 1, y, z, cb);
            p.place(l, k.air, x + 1, y + 1, z, cb);
            frame(l, k.bars);
        }
        Door::Iron => {
            frame(l, k.stone_bricks);
            p.place(l, k.iron_door, x + 1, y, z, cb);
            p.place(l, k.iron_door_u, x + 1, y + 1, z, cb);
            p.place(l, k.button_2, x + 2, y + 1, z + 1, cb);
            p.place(l, k.button_3, x + 2, y + 1, z - 1, cb);
        }
    }
}

impl Piece {
    fn post(&mut self, l: &mut Lvl, r: &mut Xoroshiro, cb: &BBox) {
        let k = ids();
        let p = &self.b;
        let air = k.air;
        let door = self.door;
        let left = matches!(self.kind, Kind::LeftTurn);
        match &mut self.kind {
            Kind::Filler { steps } => {
                for z in 0..*steps {
                    for x in 0..=4 {
                        p.place(l, k.stone_bricks, x, 0, z, cb);
                    }
                    for y in 1..=3 {
                        p.place(l, k.stone_bricks, 0, y, z, cb);
                        for x in 1..=3 {
                            p.place(l, air, x, y, z, cb);
                        }
                        p.place(l, k.stone_bricks, 4, y, z, cb);
                    }
                    for x in 0..=4 {
                        p.place(l, k.stone_bricks, x, 4, z, cb);
                    }
                }
            }
            Kind::StairsDown { .. } => {
                p.fill_sel(l, cb, 0, 0, 0, 4, 10, 4, false, r, smooth);
                small_door(p, l, cb, door, 1, 7, 0);
                small_door(p, l, cb, Door::Opening, 1, 1, 4);
                let (sb, sl) = (k.stone_bricks, k.brick_slab);
                for (b, x, y, z) in [
                    (sb, 2, 6, 1), (sb, 1, 5, 1), (sl, 1, 6, 1), (sb, 1, 5, 2), (sb, 1, 4, 3), (sl, 1, 5, 3), (sb, 2, 4, 3), (sb, 3, 3, 3),
                    (sl, 3, 4, 3), (sb, 3, 3, 2), (sb, 3, 2, 1), (sl, 3, 3, 1), (sb, 2, 2, 1), (sb, 1, 1, 1), (sl, 1, 2, 1), (sb, 1, 1, 2), (sl, 1, 1, 3),
                ] {
                    p.place(l, b, x, y, z, cb);
                }
            }
            Kind::Straight { left, right } => {
                p.fill_sel(l, cb, 0, 0, 0, 4, 4, 6, false, r, smooth);
                small_door(p, l, cb, door, 1, 1, 0);
                small_door(p, l, cb, Door::Opening, 1, 1, 6);
                p.maybe_place(l, cb, r, 10, 1, 2, 1, k.torch_e);
                p.maybe_place(l, cb, r, 10, 3, 2, 1, k.torch_w);
                p.maybe_place(l, cb, r, 10, 1, 2, 5, k.torch_e);
                p.maybe_place(l, cb, r, 10, 3, 2, 5, k.torch_w);
                if *left {
                    p.fill(l, cb, 0, 1, 2, 0, 3, 4, air, air, false);
                }
                if *right {
                    p.fill(l, cb, 4, 1, 2, 4, 3, 4, air, air, false);
                }
            }
            Kind::ChestCorridor { placed } => {
                p.fill_sel(l, cb, 0, 0, 0, 4, 4, 6, true, r, smooth);
                small_door(p, l, cb, door, 1, 1, 0);
                small_door(p, l, cb, Door::Opening, 1, 1, 6);
                p.fill(l, cb, 3, 1, 2, 3, 1, 4, k.stone_bricks, k.stone_bricks, false);
                for (x, y, z) in [(3, 1, 1), (3, 1, 5), (3, 2, 2), (3, 2, 4), (2, 1, 2), (2, 1, 3), (2, 1, 4)] {
                    p.place(l, k.brick_slab, x, y, z, cb);
                }
                if !*placed && cb.inside(p.wx(3, 3), p.wy(2), p.wz(3, 3)) {
                    *placed = true;
                    p.place(l, p.chest(), 3, 2, 3, cb);
                    r.next_int(); // loot table seed (TODO: loot)
                }
            }
            Kind::StraightStairsDown => {
                p.fill_sel(l, cb, 0, 0, 0, 4, 10, 7, false, r, smooth);
                small_door(p, l, cb, door, 1, 7, 0);
                small_door(p, l, cb, Door::Opening, 1, 1, 7);
                for i in 0..6 {
                    for x in 1..=3 {
                        p.place(l, k.stairs_s, x, 6 - i, 1 + i, cb);
                    }
                    if i < 5 {
                        for x in 1..=3 {
                            p.place(l, k.stone_bricks, x, 5 - i, 1 + i, cb);
                        }
                    }
                }
            }
            Kind::LeftTurn | Kind::RightTurn => {
                p.fill_sel(l, cb, 0, 0, 0, 4, 4, 4, !left, r, smooth);
                small_door(p, l, cb, door, 1, 1, 0);
                let ne = matches!(p.orient, Some(Face::North | Face::East));
                let x = if ne == left { 0 } else { 4 };
                p.fill(l, cb, x, 1, 1, x, 3, 3, air, air, false);
            }
            Kind::RoomCrossing { ty } => {
                p.fill_sel(l, cb, 0, 0, 0, 10, 6, 10, false, r, smooth);
                small_door(p, l, cb, door, 4, 1, 0);
                p.fill(l, cb, 4, 1, 10, 6, 3, 10, air, air, false);
                p.fill(l, cb, 0, 1, 4, 0, 3, 6, air, air, false);
                p.fill(l, cb, 10, 1, 4, 10, 3, 6, air, air, false);
                match *ty {
                    0 => {
                        for y in 1..=3 {
                            p.place(l, k.stone_bricks, 5, y, 5, cb);
                        }
                        p.place(l, k.torch_w, 4, 3, 5, cb);
                        p.place(l, k.torch_e, 6, 3, 5, cb);
                        p.place(l, k.torch_s, 5, 3, 4, cb);
                        p.place(l, k.torch_n, 5, 3, 6, cb);
                        for (x, z) in [(4, 4), (4, 5), (4, 6), (6, 4), (6, 5), (6, 6), (5, 4), (5, 6)] {
                            p.place(l, k.smooth_slab, x, 1, z, cb);
                        }
                    }
                    1 => {
                        for i in 0..5 {
                            p.place(l, k.stone_bricks, 3, 1, 3 + i, cb);
                            p.place(l, k.stone_bricks, 7, 1, 3 + i, cb);
                            p.place(l, k.stone_bricks, 3 + i, 1, 3, cb);
                            p.place(l, k.stone_bricks, 3 + i, 1, 7, cb);
                        }
                        for y in 1..=3 {
                            p.place(l, k.stone_bricks, 5, y, 5, cb);
                        }
                        p.place(l, k.water, 5, 4, 5, cb);
                        for (x, y, z) in [(6, 4, 5), (4, 4, 5), (5, 4, 6), (5, 4, 4), (6, 1, 4), (6, 1, 6), (4, 1, 4), (4, 1, 6)] {
                            p.place(l, k.water1, x, y, z, cb);
                        }
                        for y in 1..4 {
                            for (x, z) in [(6, 5), (4, 5), (5, 6), (5, 4)] {
                                p.place(l, k.water9, x, y, z, cb);
                            }
                        }
                    }
                    2 => {
                        for z in 1..=9 {
                            p.place(l, k.cobble, 1, 3, z, cb);
                            p.place(l, k.cobble, 9, 3, z, cb);
                        }
                        for x in 1..=9 {
                            p.place(l, k.cobble, x, 3, 1, cb);
                            p.place(l, k.cobble, x, 3, 9, cb);
                        }
                        for (x, y, z) in [(5, 1, 4), (5, 1, 6), (5, 3, 4), (5, 3, 6), (4, 1, 5), (6, 1, 5), (4, 3, 5), (6, 3, 5)] {
                            p.place(l, k.cobble, x, y, z, cb);
                        }
                        for y in 1..=3 {
                            for (x, z) in [(4, 4), (6, 4), (4, 6), (6, 6)] {
                                p.place(l, k.cobble, x, y, z, cb);
                            }
                        }
                        p.place(l, k.torch, 5, 3, 5, cb);
                        for z in 2..=8 {
                            p.place(l, k.planks, 2, 3, z, cb);
                            p.place(l, k.planks, 3, 3, z, cb);
                            if z <= 3 || z >= 7 {
                                for x in 4..=6 {
                                    p.place(l, k.planks, x, 3, z, cb);
                                }
                            }
                            p.place(l, k.planks, 7, 3, z, cb);
                            p.place(l, k.planks, 8, 3, z, cb);
                        }
                        for y in 1..=3 {
                            p.place(l, k.ladder_w, 9, y, 3, cb);
                        }
                        p.place(l, p.chest(), 3, 4, 8, cb);
                        if cb.inside(p.wx(3, 8), p.wy(4), p.wz(3, 8)) {
                            r.next_int(); // loot table seed
                        }
                    }
                    _ => {}
                }
            }
            Kind::PrisonHall => {
                p.fill_sel(l, cb, 0, 0, 0, 8, 4, 10, false, r, smooth);
                small_door(p, l, cb, door, 1, 1, 0);
                p.fill(l, cb, 1, 1, 10, 3, 3, 10, air, air, false);
                for z in [1, 3, 7, 9] {
                    p.fill_sel(l, cb, 4, 1, z, 4, 3, z, false, r, smooth);
                }
                for y in 1..=3 {
                    for (x, z) in [(4, 4), (4, 5), (4, 6), (5, 5), (6, 5), (7, 5)] {
                        p.place(l, k.bars, x, y, z, cb);
                    }
                }
                p.place(l, k.bars, 4, 3, 2, cb);
                p.place(l, k.bars, 4, 3, 8, cb);
                p.place(l, k.iron_door_w, 4, 1, 2, cb);
                p.place(l, k.iron_door_w_u, 4, 2, 2, cb);
                p.place(l, k.iron_door_w, 4, 1, 8, cb);
                p.place(l, k.iron_door_w_u, 4, 2, 8, cb);
            }
            Kind::Library { tall } => library(p, l, cb, r, door, *tall),
            Kind::FiveCrossing { ll, lh, rl, rh } => {
                p.fill_sel(l, cb, 0, 0, 0, 9, 8, 10, false, r, smooth);
                small_door(p, l, cb, door, 4, 3, 0);
                if *ll {
                    p.fill(l, cb, 0, 3, 1, 0, 5, 3, air, air, false);
                }
                if *rl {
                    p.fill(l, cb, 9, 3, 1, 9, 5, 3, air, air, false);
                }
                if *lh {
                    p.fill(l, cb, 0, 5, 7, 0, 7, 9, air, air, false);
                }
                if *rh {
                    p.fill(l, cb, 9, 5, 7, 9, 7, 9, air, air, false);
                }
                p.fill(l, cb, 5, 1, 10, 7, 3, 10, air, air, false);
                p.fill_sel(l, cb, 1, 2, 1, 8, 2, 6, false, r, smooth);
                p.fill_sel(l, cb, 4, 1, 5, 4, 4, 9, false, r, smooth);
                p.fill_sel(l, cb, 8, 1, 5, 8, 4, 9, false, r, smooth);
                p.fill_sel(l, cb, 1, 4, 7, 3, 4, 9, false, r, smooth);
                p.fill_sel(l, cb, 1, 3, 5, 3, 3, 6, false, r, smooth);
                p.fill(l, cb, 1, 3, 4, 3, 3, 4, k.brick_slab, k.brick_slab, false);
                p.fill(l, cb, 1, 4, 6, 3, 4, 6, k.brick_slab, k.brick_slab, false);
                p.fill_sel(l, cb, 5, 1, 7, 7, 1, 8, false, r, smooth);
                p.fill(l, cb, 5, 1, 9, 7, 1, 9, k.brick_slab, k.brick_slab, false);
                p.fill(l, cb, 5, 2, 7, 7, 2, 7, k.brick_slab, k.brick_slab, false);
                p.fill(l, cb, 4, 5, 7, 4, 5, 9, k.brick_slab, k.brick_slab, false);
                p.fill(l, cb, 8, 5, 7, 8, 5, 9, k.brick_slab, k.brick_slab, false);
                p.fill(l, cb, 5, 5, 7, 7, 5, 9, k.smooth_double, k.smooth_double, false);
                p.place(l, k.torch_s, 6, 5, 6, cb);
            }
            Kind::PortalRoom { spawner } => {
                p.fill_sel(l, cb, 0, 0, 0, 10, 7, 15, false, r, smooth);
                small_door(p, l, cb, Door::Grates, 4, 1, 0);
                let y = 6;
                p.fill_sel(l, cb, 1, y, 1, 1, y, 14, false, r, smooth);
                p.fill_sel(l, cb, 9, y, 1, 9, y, 14, false, r, smooth);
                p.fill_sel(l, cb, 2, y, 1, 8, y, 2, false, r, smooth);
                p.fill_sel(l, cb, 2, y, 14, 8, y, 14, false, r, smooth);
                p.fill_sel(l, cb, 1, 1, 1, 2, 1, 4, false, r, smooth);
                p.fill_sel(l, cb, 8, 1, 1, 9, 1, 4, false, r, smooth);
                p.fill(l, cb, 1, 1, 1, 1, 1, 3, k.lava, k.lava, false);
                p.fill(l, cb, 9, 1, 1, 9, 1, 3, k.lava, k.lava, false);
                p.fill_sel(l, cb, 3, 1, 8, 7, 1, 12, false, r, smooth);
                p.fill(l, cb, 4, 1, 9, 6, 1, 11, k.lava, k.lava, false);
                for z in (3..14).step_by(2) {
                    p.fill(l, cb, 0, 3, z, 0, 4, z, k.bars, k.bars, false);
                    p.fill(l, cb, 10, 3, z, 10, 4, z, k.bars, k.bars, false);
                }
                for x in (2..9).step_by(2) {
                    p.fill(l, cb, x, 3, 15, x, 4, 15, k.bars, k.bars, false);
                }
                p.fill_sel(l, cb, 4, 1, 5, 6, 1, 7, false, r, smooth);
                p.fill_sel(l, cb, 4, 2, 6, 6, 2, 7, false, r, smooth);
                p.fill_sel(l, cb, 4, 3, 7, 6, 3, 7, false, r, smooth);
                for x in 4..=6 {
                    p.place(l, k.brick_stairs_n, x, 1, 4, cb);
                    p.place(l, k.brick_stairs_n, x, 2, 5, cb);
                    p.place(l, k.brick_stairs_n, x, 3, 6, cb);
                }
                let mut eyes = [false; 12];
                for e in &mut eyes {
                    *e = r.next_int_below(100) > 90;
                }
                let frames = [(4, 8), (5, 8), (6, 8), (4, 12), (5, 12), (6, 12), (3, 9), (3, 10), (3, 11), (7, 9), (7, 10), (7, 11)];
                for (i, (x, z)) in frames.into_iter().enumerate() {
                    portal_frame(p, l, eyes[i], x, 3, z, cb);
                }
                if eyes.iter().all(|&e| e) {
                    for x in 4..=6 {
                        for z in 9..=11 {
                            p.place(l, k.end_portal, x, 3, z, cb);
                        }
                    }
                }
                if !*spawner {
                    let (vx, vy, vz) = (p.wx(5, 6), p.wy(3), p.wz(5, 6));
                    if cb.inside(vx, vy, vz) {
                        *spawner = true;
                        l.set(vx, vy, vz, k.spawner); // TODO: spawner block entity
                    }
                }
            }
        }
    }
}

fn library(p: &Base, l: &mut Lvl, cb: &BBox, r: &mut Xoroshiro, door: Door, tall: bool) {
    let k = ids();
    let height = if tall { 11 } else { 6 };
    p.fill_sel(l, cb, 0, 0, 0, 13, height - 1, 14, false, r, smooth);
    small_door(p, l, cb, door, 4, 1, 0);
    p.maybe_fill(l, cb, r, 7, 2, 1, 1, 11, 4, 13, k.web, k.web, false, false);
    for z in 1..=13 {
        let b = if (z - 1) % 4 == 0 { k.planks } else { k.bookshelf };
        p.fill(l, cb, 1, 1, z, 1, 4, z, b, b, false);
        p.fill(l, cb, 12, 1, z, 12, 4, z, b, b, false);
        if b == k.planks {
            p.place(l, k.torch_e, 2, 3, z, cb);
            p.place(l, k.torch_w, 11, 3, z, cb);
        }
        if tall {
            p.fill(l, cb, 1, 6, z, 1, 9, z, b, b, false);
            p.fill(l, cb, 12, 6, z, 12, 9, z, b, b, false);
        }
    }
    for z in (3..12).step_by(2) {
        for x in [3, 6, 9] {
            p.fill(l, cb, x, 1, z, x + 1, 3, z, k.bookshelf, k.bookshelf, false);
        }
    }
    if tall {
        let (pl, f) = (k.planks, k.fence);
        p.fill(l, cb, 1, 5, 1, 3, 5, 13, pl, pl, false);
        p.fill(l, cb, 10, 5, 1, 12, 5, 13, pl, pl, false);
        p.fill(l, cb, 4, 5, 1, 9, 5, 2, pl, pl, false);
        p.fill(l, cb, 4, 5, 12, 9, 5, 13, pl, pl, false);
        p.place(l, pl, 9, 5, 11, cb);
        p.place(l, pl, 8, 5, 11, cb);
        p.place(l, pl, 9, 5, 10, cb);
        p.fill(l, cb, 3, 6, 3, 3, 6, 11, f, f, false);
        p.fill(l, cb, 10, 6, 3, 10, 6, 9, f, f, false);
        p.fill(l, cb, 4, 6, 2, 9, 6, 2, f, f, false);
        p.fill(l, cb, 4, 6, 12, 7, 6, 12, f, f, false);
        p.place(l, f, 3, 6, 2, cb);
        p.place(l, f, 3, 6, 12, cb);
        p.place(l, f, 10, 6, 2, cb);
        for i in 0..=2 {
            p.place(l, f, 8 + i, 6, 12 - i, cb);
            if i != 2 {
                p.place(l, f, 8 + i, 6, 11 - i, cb);
            }
        }
        for y in 1..=7 {
            p.place(l, k.ladder_s, 10, y, 13, cb);
        }
        for (x, y, z) in [(6, 9, 7), (7, 9, 7), (6, 8, 7), (7, 8, 7), (6, 7, 7), (7, 7, 7), (5, 7, 7), (8, 7, 7), (6, 7, 6), (6, 7, 8), (7, 7, 6), (7, 7, 8)] {
            p.place(l, f, x, y, z, cb);
        }
        for (x, z) in [(5, 7), (8, 7), (6, 6), (6, 8), (7, 6), (7, 8)] {
            p.place(l, k.torch, x, 8, z, cb);
        }
    }
    let chest = p.chest();
    p.place(l, chest, 3, 3, 5, cb);
    if cb.inside(p.wx(3, 5), p.wy(3), p.wz(3, 5)) {
        r.next_int(); // loot table seed (TODO: loot)
    }
    if tall {
        p.place(l, k.air, 12, 9, 1, cb);
        p.place(l, chest, 12, 8, 1, cb);
        if cb.inside(p.wx(12, 1), p.wy(8), p.wz(12, 1)) {
            r.next_int();
        }
    }
}

fn portal_frame(p: &Base, l: &mut Lvl, eye: bool, x: i32, y: i32, z: i32, cb: &BBox) {
    let (px, py, pz) = (p.wx(x, z), p.wy(y), p.wz(x, z));
    if !cb.inside(px, py, pz) {
        return;
    }
    let (dx, dz) = (p.wx(5, 10) - px, p.wz(5, 10) - pz);
    let face = if dx.abs() > dz.abs() {
        if dx > 0 { "east" } else { "west" }
    } else if dz > 0 {
        "south"
    } else {
        "north"
    };
    let b = bl::with(bl::id("end_portal_frame"), &[("minecraft:cardinal_direction", bl::s(face)), ("end_portal_eye_bit", bl::b(eye))]);
    l.set(px, py, pz, b);
}

/// PNX `StrongholdPopulator.apply` for start chunk (cx, cz), over every chunk it reaches.
pub(super) fn build(w: &mut StructWorld, r: &mut Xoroshiro, cx: i32, cz: i32) {
    let seed = w.seed;
    if !is_start(seed, cx, cz) {
        return;
    }
    let mut sr = Xoroshiro::new(seed);
    let mut count = 0i64;
    let (pieces, bb) = loop {
        sr.set_seed(seed.wrapping_add(count));
        count += 1;
        let s = (cx as i64).wrapping_mul(sr.next_int() as i64) ^ (cz as i64).wrapping_mul(sr.next_int() as i64) ^ seed;
        sr.set_seed(s);
        let (x, z) = ((cx << 4) + 2, (cz << 4) + 2);
        let o = [Face::North, Face::East, Face::South, Face::West][sr.next_int_below(4) as usize];
        let start = Piece { b: Base::new(BBox::new(x, 64, z, x + 4, 74, z + 4), 0, Some(o)), door: Door::Opening, kind: Kind::StairsDown { source: true } };
        let mut g = Gen { pieces: vec![start], pending: Vec::new(), counts: [0; 11], current: (0..11).collect(), total: 0, imposed: None, prev: None, portal: false, r: &mut sr };
        g.children(0);
        while !g.pending.is_empty() {
            let n = g.pending.len() as i32;
            let i = g.pending.remove(g.r.next_int_below(n) as usize);
            g.children(i);
        }
        let (mut pieces, portal) = (g.pieces, g.portal);
        let mut bb = BBox::unknown();
        for p in &pieces {
            bb.expand(&p.b.bb);
        }
        let range = super::super::super::aquifer::SEA_LEVEL - 10;
        let mut y = bb.y_span() - 64 + 1;
        if y < range {
            y += sr.next_bounded_int(range - y);
        }
        let dy = y - bb.y1;
        bb.shift(0, dy, 0);
        pieces.iter_mut().for_each(|p| p.b.bb.shift(0, dy, 0));
        // TODO: PNX retries forever, we give up after 64 tries without a portal room.
        if portal || count >= 64 {
            break (pieces, bb);
        }
    };
    let mut pieces = pieces;
    let mut l = Lvl { w, water_fix: false };
    for ccx in (bb.x0 >> 4)..=(bb.x1 >> 4) {
        for ccz in (bb.z0 >> 4)..=(bb.z1 >> 4) {
            let cb = BBox::chunk(ccx, ccz);
            r.set_seed(seed);
            for p in &mut pieces {
                if p.b.bb.intersects(&cb) {
                    p.post(&mut l, r, &cb);
                }
            }
        }
    }
}

#[cfg(test)]
pub(super) fn positions(seed: i64) -> Vec<(i32, i32)> {
    initial(seed).to_vec()
}

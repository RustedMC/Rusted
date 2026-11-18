// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
// from PNX NetherBridgePieces, NetherFortressStart, StructureStart, StructurePiece, BoundingBox
// (piece helpers copied from overworld underground/piece.rs)
//! Nether fortresses. Pieces are generated once per start, then post-processed per chunk.

use super::rng::{BedrockRandom, NukkitRandom};
use super::NetherChunk;
use crate::blocks::State;
use crate::overworld::feature::blocks as bl;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Face {
    North,
    South,
    West,
    East,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BBox {
    pub x0: i32,
    pub y0: i32,
    pub z0: i32,
    pub x1: i32,
    pub y1: i32,
    pub z1: i32,
}

impl BBox {
    const fn new(x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) -> Self {
        Self { x0, y0, z0, x1, y1, z1 }
    }
    const fn unknown() -> Self {
        Self::new(i32::MAX, i32::MAX, i32::MAX, i32::MIN, i32::MIN, i32::MIN)
    }
    /// PNX `new BoundingBox(x0, z0, x1, z1)`: a chunk column.
    pub fn chunk(cx: i32, cz: i32) -> Self {
        Self::new(cx << 4, -63, cz << 4, (cx << 4) + 15, 512, (cz << 4) + 15)
    }
    #[allow(clippy::too_many_arguments)]
    fn orient(x: i32, y: i32, z: i32, xo: i32, yo: i32, zo: i32, xl: i32, yl: i32, zl: i32, f: Face) -> Self {
        match f {
            Face::North => Self::new(x + xo, y + yo, z - zl + 1 + zo, x + xl - 1 + xo, y + yl - 1 + yo, z + zo),
            Face::West => Self::new(x - zl + 1 + zo, y + yo, z + xo, x + zo, y + yl - 1 + yo, z + xl - 1 + xo),
            Face::East => Self::new(x + zo, y + yo, z + xo, x + zl - 1 + zo, y + yl - 1 + yo, z + xl - 1 + xo),
            Face::South => Self::new(x + xo, y + yo, z + zo, x + xl - 1 + xo, y + yl - 1 + yo, z + zl - 1 + zo),
        }
    }
    pub fn intersects(&self, o: &BBox) -> bool {
        self.x1 >= o.x0 && self.x0 <= o.x1 && self.z1 >= o.z0 && self.z0 <= o.z1 && self.y1 >= o.y0 && self.y0 <= o.y1
    }
    fn expand(&mut self, o: &BBox) {
        self.x0 = self.x0.min(o.x0);
        self.y0 = self.y0.min(o.y0);
        self.z0 = self.z0.min(o.z0);
        self.x1 = self.x1.max(o.x1);
        self.y1 = self.y1.max(o.y1);
        self.z1 = self.z1.max(o.z1);
    }
    fn inside(&self, x: i32, y: i32, z: i32) -> bool {
        x >= self.x0 && x <= self.x1 && z >= self.z0 && z <= self.z1 && y >= self.y0 && y <= self.y1
    }
}

// ---------------------------------------------------------------------------------------------
// Block rotation (PNX `StructurePiece.placeBlock` + `StructureRotationUtil`)

fn turns(o: Option<Face>) -> u8 {
    match o {
        Some(Face::South) => 2,
        Some(Face::West) => 3,
        Some(Face::East) => 1,
        _ => 0,
    }
}

fn st_str(b: i32, k: &str) -> Option<&'static str> {
    match crate::blocks::state(b, k) {
        Some(State::Str(s)) => Some(s.as_str()),
        _ => None,
    }
}

fn st_int(b: i32, k: &str) -> Option<i32> {
    match crate::blocks::state(b, k) {
        Some(State::Int(v)) => Some(*v),
        _ => None,
    }
}

/// PNX `StructureRotationUtil.clockwise90`, for the properties fortress blocks use.
fn cw90(b: i32) -> i32 {
    let mut ch: Vec<(&str, State)> = Vec::new();
    if let Some(v) = st_int(b, "weirdo_direction") {
        ch.push(("weirdo_direction", bl::i(match v {
            0 => 2,
            1 => 3,
            2 => 1,
            _ => 0,
        })));
    }
    if let Some(v) = st_str(b, "minecraft:cardinal_direction") {
        let r = match v {
            "north" => "east",
            "east" => "south",
            "south" => "west",
            _ => "north",
        };
        ch.push(("minecraft:cardinal_direction", bl::s(r)));
    }
    if ch.is_empty() { b } else { bl::with(b, &ch) }
}

fn rotate(mut b: i32, t: u8) -> i32 {
    // stairs and chests just take plain quarter turns
    for _ in 0..t {
        b = cw90(b);
    }
    b
}

// ---------------------------------------------------------------------------------------------
// Pieces

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    BridgeStraight,
    BridgeEndFiller,
    BridgeCrossing,
    RoomCrossing,
    StairsRoom,
    MonsterThrone,
    CastleEntrance,
    CastleStalkRoom,
    CastleSmallCorridorPiece,
    CastleSmallCorridorCrossingPiece,
    CastleSmallCorridorRightTurnPiece,
    CastleSmallCorridorLeftTurnPiece,
    CastleCorridorStairsPiece,
    CastleCorridorTBalconyPiece,
}

#[derive(Clone, Debug)]
pub struct Piece {
    kind: Kind,
    pub bb: BBox,
    depth: i32,
    orient: Option<Face>,
    /// `isNeedingChest` of the corridor turns.
    chest: bool,
}

/// Block states the pieces place.
struct K {
    air: i32,
    bricks: i32,
    fence: i32,
    soul_sand: i32,
    wart: i32,
    lava: i32,
    spawner: i32,
    stairs_s: i32,
    stairs_n: i32,
    stairs_w: i32,
    stairs_e: i32,
    chest: i32,
}

impl K {
    /// Chest facing: `MinecraftCardinalDirection.values()[opposite.getIndex() - 2]`.
    fn chest(&self, o: Option<Face>) -> i32 {
        let d = match o.unwrap_or(Face::North) {
            Face::North => "west",
            Face::South => "south",
            Face::West => "east",
            Face::East => "north",
        };
        bl::with(self.chest, &[("minecraft:cardinal_direction", bl::s(d))])
    }
}

fn k() -> &'static K {
    static K_: std::sync::OnceLock<K> = std::sync::OnceLock::new();
    K_.get_or_init(|| {
        let stairs = |d: i32| bl::with(bl::id("nether_brick_stairs"), &[("weirdo_direction", bl::i(d)), ("upside_down_bit", bl::b(false))]);
        K {
            air: bl::air(),
            bricks: bl::id("nether_brick"),
            fence: bl::id("nether_brick_fence"),
            soul_sand: bl::id("soul_sand"),
            wart: bl::with(bl::id("nether_wart"), &[("age", bl::i(3))]),
            lava: bl::id("lava"),
            spawner: bl::id("mob_spawner"),
            stairs_s: stairs(2),
            stairs_n: stairs(3),
            stairs_w: stairs(1),
            stairs_e: stairs(0),
            chest: bl::id("chest"),
        }
    })
}

/// The chunk being generated; every write is clipped to it.
pub struct Lvl<'a> {
    pub ch: &'a mut NetherChunk,
    pub bx: i32,
    pub bz: i32,
}

impl Lvl<'_> {
    fn get(&self, x: i32, y: i32, z: i32) -> i32 {
        self.ch.get(x - self.bx, y, z - self.bz)
    }
    fn set(&mut self, x: i32, y: i32, z: i32, id: i32) {
        crate::loot::generated((x, y + crate::dims::base(crate::dims::NETHER), z), id, "nether_fortress");
        self.ch.set(x - self.bx, y, z - self.bz, id);
    }
}

impl Piece {
    fn wx(&self, x: i32, z: i32) -> i32 {
        match self.orient {
            None => x,
            Some(Face::North | Face::South) => self.bb.x0 + x,
            Some(Face::West) => self.bb.x1 - z,
            Some(Face::East) => self.bb.x0 + z,
        }
    }
    fn wy(&self, y: i32) -> i32 {
        if self.orient.is_none() { y } else { y + self.bb.y0 }
    }
    fn wz(&self, x: i32, z: i32) -> i32 {
        match self.orient {
            None => z,
            Some(Face::North) => self.bb.z1 - z,
            Some(Face::South) => self.bb.z0 + z,
            Some(Face::West | Face::East) => self.bb.z0 + x,
        }
    }

    fn place(&self, l: &mut Lvl, b: i32, x: i32, y: i32, z: i32, cb: &BBox) {
        let (vx, vy, vz) = (self.wx(x, z), self.wy(y), self.wz(x, z));
        if cb.inside(vx, vy, vz) {
            l.set(vx, vy, vz, rotate(b, turns(self.orient)));
        }
    }

    /// `generateBox(..., skipAir = false)`.
    #[allow(clippy::too_many_arguments)]
    fn fill(&self, l: &mut Lvl, cb: &BBox, x1: i32, y1: i32, z1: i32, x2: i32, y2: i32, z2: i32, out: i32, ins: i32) {
        for y in y1..=y2 {
            for x in x1..=x2 {
                for z in z1..=z2 {
                    let edge = y == y1 || y == y2 || x == x1 || x == x2 || z == z1 || z == z2;
                    self.place(l, if edge { out } else { ins }, x, y, z, cb);
                }
            }
        }
    }

    /// `fillColumnDown`: through air and liquids, down to y 2.
    fn column_down(&self, l: &mut Lvl, b: i32, x: i32, y: i32, z: i32, cb: &BBox) {
        let (wx, mut wy, wz) = (self.wx(x, z), self.wy(y), self.wz(x, z));
        if !cb.inside(wx, wy, wz) {
            return;
        }
        let open = |v: i32| bl::is_air(v) || bl::is_liquid(v);
        let mut cur = l.get(wx, wy, wz);
        while open(cur) && wy > 1 {
            l.set(wx, wy, wz, b);
            wy -= 1;
            cur = l.get(wx, wy, wz);
        }
    }
}

/// Box (offsets and size) of each piece kind for `BoundingBox.orientBox`.
fn dims(k: Kind) -> [i32; 6] {
    match k {
        Kind::BridgeStraight => [-1, -3, 0, 5, 10, 19],
        Kind::BridgeEndFiller => [-1, -3, 0, 5, 10, 8],
        Kind::BridgeCrossing => [-8, -3, 0, 19, 10, 19],
        Kind::RoomCrossing => [-2, 0, 0, 7, 9, 7],
        Kind::StairsRoom => [-2, 0, 0, 7, 11, 7],
        Kind::MonsterThrone => [-2, 0, 0, 7, 8, 9],
        Kind::CastleEntrance | Kind::CastleStalkRoom => [-5, -3, 0, 13, 14, 13],
        Kind::CastleSmallCorridorPiece | Kind::CastleSmallCorridorCrossingPiece | Kind::CastleSmallCorridorRightTurnPiece | Kind::CastleSmallCorridorLeftTurnPiece => [-1, 0, 0, 5, 7, 5],
        Kind::CastleCorridorStairsPiece => [-1, -7, 0, 5, 14, 10],
        Kind::CastleCorridorTBalconyPiece => [-3, 0, 0, 9, 7, 9],
    }
}

/// PNX `PieceWeight`.
struct Weight {
    kind: Kind,
    weight: i32,
    max: i32,
    allow_in_row: bool,
    count: i32,
}

const fn w(kind: Kind, weight: i32, max: i32, allow_in_row: bool) -> Weight {
    Weight { kind, weight, max, allow_in_row, count: 0 }
}

impl Weight {
    fn valid(&self) -> bool {
        self.max == 0 || self.count < self.max
    }
}

/// `NetherFortressStart` while its pieces are generated.
struct Gen {
    pieces: Vec<Piece>,
    pending: Vec<usize>,
    bridge: Vec<Weight>,
    castle: Vec<Weight>,
    previous: Option<Kind>,
}

/// `createPiece`: box must be high enough and free, then the piece rolls.
#[allow(clippy::too_many_arguments)]
fn create(pieces: &[Piece], kind: Kind, r: &mut BedrockRandom, x: i32, y: i32, z: i32, f: Face, depth: i32) -> Option<Piece> {
    let d = dims(kind);
    let bb = BBox::orient(x, y, z, d[0], d[1], d[2], d[3], d[4], d[5], f);
    if bb.y0 <= 10 || pieces.iter().any(|p| p.bb.intersects(&bb)) {
        return None;
    }
    let chest = match kind {
        Kind::BridgeEndFiller => {
            let _self_seed = r.next_int(); // PNX keeps it but post-processes with `random.fork()`
            false
        }
        Kind::CastleSmallCorridorRightTurnPiece | Kind::CastleSmallCorridorLeftTurnPiece => r.next_int_below(3) == 0,
        _ => false,
    };
    Some(Piece { kind, bb, depth, orient: Some(f), chest })
}

impl Gen {
    #[allow(clippy::too_many_arguments)]
    fn generate_piece(&mut self, castle: bool, r: &mut BedrockRandom, x: i32, y: i32, z: i32, f: Face, depth: i32) -> Option<Piece> {
        let list = if castle { &mut self.castle } else { &mut self.bridge };
        let success = list.iter().any(|w| w.max > 0 && w.count < w.max);
        let total: i32 = list.iter().map(|w| w.weight).sum();
        let total = if success { total } else { -1 };
        if total > 0 && depth <= 30 {
            for _ in 0..5 {
                let mut target = r.next_int_below(total);
                let mut i = 0;
                while i < list.len() {
                    target -= list[i].weight;
                    if target < 0 {
                        let wt = &list[i];
                        if !wt.valid() || (Some(wt.kind) == self.previous && !wt.allow_in_row) {
                            break;
                        }
                        if let Some(p) = create(&self.pieces, wt.kind, r, x, y, z, f, depth) {
                            list[i].count += 1;
                            self.previous = Some(list[i].kind);
                            if !list[i].valid() {
                                list.remove(i);
                            }
                            return Some(p);
                        }
                    }
                    i += 1;
                }
            }
        }
        create(&self.pieces, Kind::BridgeEndFiller, r, x, y, z, f, depth)
    }

    #[allow(clippy::too_many_arguments)]
    fn generate_and_add(&mut self, r: &mut BedrockRandom, x: i32, y: i32, z: i32, f: Face, depth: i32, castle: bool) {
        let s = self.pieces[0].bb;
        if (x - s.x0).abs() > 112 || (z - s.z0).abs() > 112 {
            // the caller drops it, only the rolls matter
            let _ = create(&self.pieces, Kind::BridgeEndFiller, r, x, y, z, f, depth);
            return;
        }
        if let Some(p) = self.generate_piece(castle, r, x, y, z, f, depth + 1) {
            self.pieces.push(p);
            self.pending.push(self.pieces.len() - 1);
        }
    }

    fn forward(&mut self, i: usize, r: &mut BedrockRandom, h: i32, yo: i32, castle: bool) {
        let p = self.pieces[i].clone();
        let Some(f) = p.orient else { return };
        let b = p.bb;
        let (x, z) = match f {
            Face::North => (b.x0 + h, b.z0 - 1),
            Face::South => (b.x0 + h, b.z1 + 1),
            Face::West => (b.x0 - 1, b.z0 + h),
            Face::East => (b.x1 + 1, b.z0 + h),
        };
        self.generate_and_add(r, x, b.y0 + yo, z, f, p.depth, castle);
    }

    fn left(&mut self, i: usize, r: &mut BedrockRandom, yo: i32, h: i32, castle: bool) {
        let p = self.pieces[i].clone();
        let Some(f) = p.orient else { return };
        let b = p.bb;
        match f {
            Face::North | Face::South => self.generate_and_add(r, b.x0 - 1, b.y0 + yo, b.z0 + h, Face::West, p.depth, castle),
            Face::West | Face::East => self.generate_and_add(r, b.x0 + h, b.y0 + yo, b.z0 - 1, Face::North, p.depth, castle),
        }
    }

    fn right(&mut self, i: usize, r: &mut BedrockRandom, yo: i32, h: i32, castle: bool) {
        let p = self.pieces[i].clone();
        let Some(f) = p.orient else { return };
        let b = p.bb;
        match f {
            Face::North | Face::South => self.generate_and_add(r, b.x1 + 1, b.y0 + yo, b.z0 + h, Face::East, p.depth, castle),
            Face::West | Face::East => self.generate_and_add(r, b.x0 + h, b.y0 + yo, b.z1 + 1, Face::South, p.depth, castle),
        }
    }

    fn add_children(&mut self, i: usize, r: &mut BedrockRandom) {
        match self.pieces[i].kind {
            Kind::BridgeStraight => self.forward(i, r, 1, 3, false),
            Kind::BridgeCrossing => {
                self.forward(i, r, 8, 3, false);
                self.left(i, r, 3, 8, false);
                self.right(i, r, 3, 8, false);
            }
            Kind::RoomCrossing => {
                self.forward(i, r, 2, 0, false);
                self.left(i, r, 0, 2, false);
                self.right(i, r, 0, 2, false);
            }
            Kind::StairsRoom => self.right(i, r, 6, 2, false),
            Kind::CastleEntrance => self.forward(i, r, 5, 3, true),
            Kind::CastleStalkRoom => {
                self.forward(i, r, 5, 3, true);
                self.forward(i, r, 5, 11, true);
            }
            Kind::CastleSmallCorridorPiece | Kind::CastleCorridorStairsPiece => self.forward(i, r, 1, 0, true),
            Kind::CastleSmallCorridorCrossingPiece => {
                self.forward(i, r, 1, 0, true);
                self.left(i, r, 0, 1, true);
                self.right(i, r, 0, 1, true);
            }
            Kind::CastleSmallCorridorRightTurnPiece => self.right(i, r, 0, 1, true),
            Kind::CastleSmallCorridorLeftTurnPiece => self.left(i, r, 0, 1, true),
            Kind::CastleCorridorTBalconyPiece => {
                let h = if matches!(self.pieces[i].orient, Some(Face::West | Face::North)) { 5 } else { 1 };
                let c = r.next_int_below(8) > 0;
                self.left(i, r, 0, h, c);
                let c = r.next_int_below(8) > 0;
                self.right(i, r, 0, h, c);
            }
            Kind::BridgeEndFiller | Kind::MonsterThrone => {}
        }
    }
}

/// A generated fortress (PNX `NetherFortressStart` after `generatePieces`).
pub struct Start {
    pub pieces: Vec<Piece>,
    pub bb: BBox,
}

/// `NetherFortressStart.generatePieces` for start chunk (cx, cz).
pub fn generate(r: &mut BedrockRandom, cx: i32, cz: i32) -> Start {
    use Kind::*;
    let (x, z) = ((cx << 4) + 2, (cz << 4) + 2);
    // StartPiece: BridgeCrossing(random, x, z), orientation from Plane.HORIZONTAL (N, E, S, W).
    let f = [Face::North, Face::East, Face::South, Face::West][r.next_int_below(4) as usize];
    let start = Piece { kind: BridgeCrossing, bb: BBox::new(x, 64, z, x + 18, 73, z + 18), depth: 0, orient: Some(f), chest: false };
    let mut g = Gen {
        pieces: vec![start],
        pending: vec![],
        bridge: vec![w(BridgeStraight, 30, 0, true), w(BridgeCrossing, 10, 4, false), w(RoomCrossing, 10, 4, false), w(StairsRoom, 10, 3, false), w(MonsterThrone, 5, 2, false), w(CastleEntrance, 5, 1, false)],
        castle: vec![
            w(CastleSmallCorridorPiece, 25, 0, true),
            w(CastleSmallCorridorCrossingPiece, 15, 5, false),
            w(CastleSmallCorridorRightTurnPiece, 5, 10, false),
            w(CastleSmallCorridorLeftTurnPiece, 5, 10, false),
            w(CastleCorridorStairsPiece, 10, 3, true),
            w(CastleCorridorTBalconyPiece, 7, 2, false),
            w(CastleStalkRoom, 5, 2, false),
        ],
        previous: None,
    };
    g.add_children(0, r);
    while !g.pending.is_empty() {
        // exclusive bound, so the newest pending piece always waits (same as PNX)
        let i = r.next_int_below(g.pending.len() as i32 - 1) as usize;
        let p = g.pending.remove(i);
        g.add_children(p, r);
    }
    let mut bb = BBox::unknown();
    for p in &g.pieces {
        bb.expand(&p.bb);
    }
    // moveInsideHeights(random, 48, 70)
    let range = 70 - 48 + 1 - (bb.y1 - bb.y0 + 1);
    let y = if range > 1 { 48 + r.next_int_below(range) } else { 48 };
    let off = y - bb.y0;
    bb.y0 += off;
    bb.y1 += off;
    for p in &mut g.pieces {
        p.bb.y0 += off;
        p.bb.y1 += off;
    }
    Start { pieces: g.pieces, bb }
}

/// `StructureStart.postProcessPieces` for one chunk.
pub fn post_process(s: &Start, l: &mut Lvl, random: &mut NukkitRandom, cb: &BBox) {
    for p in &s.pieces {
        if p.bb.intersects(cb) {
            post(p, l, random, cb);
        }
    }
}

#[allow(clippy::identity_op)]
fn post(p: &Piece, l: &mut Lvl, random: &mut NukkitRandom, cb: &BBox) {
    let k = k();
    match p.kind {
        Kind::BridgeStraight => {
            p.fill(l, cb, 0, 3, 0, 4, 4, 18, k.bricks, k.bricks);
            p.fill(l, cb, 1, 5, 0, 3, 7, 18, k.air, k.air);
            p.fill(l, cb, 0, 5, 0, 0, 5, 18, k.bricks, k.bricks);
            p.fill(l, cb, 4, 5, 0, 4, 5, 18, k.bricks, k.bricks);
            p.fill(l, cb, 0, 2, 0, 4, 2, 5, k.bricks, k.bricks);
            p.fill(l, cb, 0, 2, 13, 4, 2, 18, k.bricks, k.bricks);
            p.fill(l, cb, 0, 0, 0, 4, 1, 3, k.bricks, k.bricks);
            p.fill(l, cb, 0, 0, 15, 4, 1, 18, k.bricks, k.bricks);
            for x in 0..=4 {
                for z in 0..=2 {
                    p.column_down(l, k.bricks, x, -1, z, cb);
                    p.column_down(l, k.bricks, x, -1, 18 - z, cb);
                }
            }
            p.fill(l, cb, 0, 1, 1, 0, 4, 1, k.fence, k.fence);
            p.fill(l, cb, 0, 3, 4, 0, 4, 4, k.fence, k.fence);
            p.fill(l, cb, 0, 3, 14, 0, 4, 14, k.fence, k.fence);
            p.fill(l, cb, 0, 1, 17, 0, 4, 17, k.fence, k.fence);
            p.fill(l, cb, 4, 1, 1, 4, 4, 1, k.fence, k.fence);
            p.fill(l, cb, 4, 3, 4, 4, 4, 4, k.fence, k.fence);
            p.fill(l, cb, 4, 3, 14, 4, 4, 14, k.fence, k.fence);
            p.fill(l, cb, 4, 1, 17, 4, 4, 17, k.fence, k.fence);
        }
        Kind::BridgeEndFiller => {
            let mut rand = random.fork();
            for x in 0..=4 {
                for y in 3..=4 {
                    p.fill(l, cb, x, y, 0, x, y, rand.next_bounded_int(8), k.bricks, k.bricks);
                }
            }
            p.fill(l, cb, 0, 5, 0, 0, 5, rand.next_bounded_int(8), k.bricks, k.bricks);
            p.fill(l, cb, 4, 5, 0, 4, 5, rand.next_bounded_int(8), k.bricks, k.bricks);
            for x in 0..=4 {
                p.fill(l, cb, x, 2, 0, x, 2, rand.next_bounded_int(5), k.bricks, k.bricks);
            }
            for x in 0..=4 {
                for y in 0..=1 {
                    p.fill(l, cb, x, y, 0, x, y, rand.next_bounded_int(3), k.bricks, k.bricks);
                }
            }
        }
        Kind::BridgeCrossing => {
            p.fill(l, cb, 7, 3, 0, 11, 4, 18, k.bricks, k.bricks);
            p.fill(l, cb, 0, 3, 7, 18, 4, 11, k.bricks, k.bricks);
            p.fill(l, cb, 8, 5, 0, 10, 7, 18, k.air, k.air);
            p.fill(l, cb, 0, 5, 8, 18, 7, 10, k.air, k.air);
            p.fill(l, cb, 7, 5, 0, 7, 5, 7, k.bricks, k.bricks);
            p.fill(l, cb, 7, 5, 11, 7, 5, 18, k.bricks, k.bricks);
            p.fill(l, cb, 11, 5, 0, 11, 5, 7, k.bricks, k.bricks);
            p.fill(l, cb, 11, 5, 11, 11, 5, 18, k.bricks, k.bricks);
            p.fill(l, cb, 0, 5, 7, 7, 5, 7, k.bricks, k.bricks);
            p.fill(l, cb, 11, 5, 7, 18, 5, 7, k.bricks, k.bricks);
            p.fill(l, cb, 0, 5, 11, 7, 5, 11, k.bricks, k.bricks);
            p.fill(l, cb, 11, 5, 11, 18, 5, 11, k.bricks, k.bricks);
            p.fill(l, cb, 7, 2, 0, 11, 2, 5, k.bricks, k.bricks);
            p.fill(l, cb, 7, 2, 13, 11, 2, 18, k.bricks, k.bricks);
            p.fill(l, cb, 7, 0, 0, 11, 1, 3, k.bricks, k.bricks);
            p.fill(l, cb, 7, 0, 15, 11, 1, 18, k.bricks, k.bricks);
            for x in 7..=11 {
                for z in 0..=2 {
                    p.column_down(l, k.bricks, x, -1, z, cb);
                    p.column_down(l, k.bricks, x, -1, 18 - z, cb);
                }
            }
            p.fill(l, cb, 0, 2, 7, 5, 2, 11, k.bricks, k.bricks);
            p.fill(l, cb, 13, 2, 7, 18, 2, 11, k.bricks, k.bricks);
            p.fill(l, cb, 0, 0, 7, 3, 1, 11, k.bricks, k.bricks);
            p.fill(l, cb, 15, 0, 7, 18, 1, 11, k.bricks, k.bricks);
            for x in 0..=2 {
                for z in 7..=11 {
                    p.column_down(l, k.bricks, x, -1, z, cb);
                    p.column_down(l, k.bricks, 18 - x, -1, z, cb);
                }
            }
        }
        Kind::RoomCrossing => {
            p.fill(l, cb, 0, 0, 0, 6, 1, 6, k.bricks, k.bricks);
            p.fill(l, cb, 0, 2, 0, 6, 7, 6, k.air, k.air);
            p.fill(l, cb, 0, 2, 0, 1, 6, 0, k.bricks, k.bricks);
            p.fill(l, cb, 0, 2, 6, 1, 6, 6, k.bricks, k.bricks);
            p.fill(l, cb, 5, 2, 0, 6, 6, 0, k.bricks, k.bricks);
            p.fill(l, cb, 5, 2, 6, 6, 6, 6, k.bricks, k.bricks);
            p.fill(l, cb, 0, 2, 0, 0, 6, 1, k.bricks, k.bricks);
            p.fill(l, cb, 0, 2, 5, 0, 6, 6, k.bricks, k.bricks);
            p.fill(l, cb, 6, 2, 0, 6, 6, 1, k.bricks, k.bricks);
            p.fill(l, cb, 6, 2, 5, 6, 6, 6, k.bricks, k.bricks);
            p.fill(l, cb, 2, 6, 0, 4, 6, 0, k.bricks, k.bricks);
            p.fill(l, cb, 2, 5, 0, 4, 5, 0, k.fence, k.fence);
            p.fill(l, cb, 2, 6, 6, 4, 6, 6, k.bricks, k.bricks);
            p.fill(l, cb, 2, 5, 6, 4, 5, 6, k.fence, k.fence);
            p.fill(l, cb, 0, 6, 2, 0, 6, 4, k.bricks, k.bricks);
            p.fill(l, cb, 0, 5, 2, 0, 5, 4, k.fence, k.fence);
            p.fill(l, cb, 6, 6, 2, 6, 6, 4, k.bricks, k.bricks);
            p.fill(l, cb, 6, 5, 2, 6, 5, 4, k.fence, k.fence);
            for x in 0..=6 {
                for z in 0..=6 {
                    p.column_down(l, k.bricks, x, -1, z, cb);
                }
            }
        }
        Kind::StairsRoom => {
            p.fill(l, cb, 0, 0, 0, 6, 1, 6, k.bricks, k.bricks);
            p.fill(l, cb, 0, 2, 0, 6, 10, 6, k.air, k.air);
            p.fill(l, cb, 0, 2, 0, 1, 8, 0, k.bricks, k.bricks);
            p.fill(l, cb, 5, 2, 0, 6, 8, 0, k.bricks, k.bricks);
            p.fill(l, cb, 0, 2, 1, 0, 8, 6, k.bricks, k.bricks);
            p.fill(l, cb, 6, 2, 1, 6, 8, 6, k.bricks, k.bricks);
            p.fill(l, cb, 1, 2, 6, 5, 8, 6, k.bricks, k.bricks);
            p.fill(l, cb, 0, 3, 2, 0, 5, 4, k.fence, k.fence);
            p.fill(l, cb, 6, 3, 2, 6, 5, 2, k.fence, k.fence);
            p.fill(l, cb, 6, 3, 4, 6, 5, 4, k.fence, k.fence);
            p.place(l, k.bricks, 5, 2, 5, cb);
            p.fill(l, cb, 4, 2, 5, 4, 3, 5, k.bricks, k.bricks);
            p.fill(l, cb, 3, 2, 5, 3, 4, 5, k.bricks, k.bricks);
            p.fill(l, cb, 2, 2, 5, 2, 5, 5, k.bricks, k.bricks);
            p.fill(l, cb, 1, 2, 5, 1, 6, 5, k.bricks, k.bricks);
            p.fill(l, cb, 1, 7, 1, 5, 7, 4, k.bricks, k.bricks);
            p.fill(l, cb, 6, 8, 2, 6, 8, 4, k.air, k.air);
            p.fill(l, cb, 2, 6, 0, 4, 8, 0, k.bricks, k.bricks);
            p.fill(l, cb, 2, 5, 0, 4, 5, 0, k.fence, k.fence);
            for x in 0..=6 {
                for z in 0..=6 {
                    p.column_down(l, k.bricks, x, -1, z, cb);
                }
            }
        }
        Kind::MonsterThrone => {
            p.fill(l, cb, 0, 2, 0, 6, 7, 7, k.air, k.air);
            p.fill(l, cb, 1, 0, 0, 5, 1, 7, k.bricks, k.bricks);
            p.fill(l, cb, 1, 2, 1, 5, 2, 7, k.bricks, k.bricks);
            p.fill(l, cb, 1, 3, 2, 5, 3, 7, k.bricks, k.bricks);
            p.fill(l, cb, 1, 4, 3, 5, 4, 7, k.bricks, k.bricks);
            p.fill(l, cb, 1, 2, 0, 1, 4, 2, k.bricks, k.bricks);
            p.fill(l, cb, 5, 2, 0, 5, 4, 2, k.bricks, k.bricks);
            p.fill(l, cb, 1, 5, 2, 1, 5, 3, k.bricks, k.bricks);
            p.fill(l, cb, 5, 5, 2, 5, 5, 3, k.bricks, k.bricks);
            p.fill(l, cb, 0, 5, 3, 0, 5, 8, k.bricks, k.bricks);
            p.fill(l, cb, 6, 5, 3, 6, 5, 8, k.bricks, k.bricks);
            p.fill(l, cb, 1, 5, 8, 5, 5, 8, k.bricks, k.bricks);
            p.place(l, k.fence, 1, 6, 3, cb);
            p.place(l, k.fence, 5, 6, 3, cb);
            p.place(l, k.fence, 0, 6, 3, cb);
            p.place(l, k.fence, 6, 6, 3, cb);
            p.fill(l, cb, 0, 6, 4, 0, 6, 7, k.fence, k.fence);
            p.fill(l, cb, 6, 6, 4, 6, 6, 7, k.fence, k.fence);
            p.place(l, k.fence, 0, 6, 8, cb);
            p.place(l, k.fence, 6, 6, 8, cb);
            p.fill(l, cb, 1, 6, 8, 5, 6, 8, k.fence, k.fence);
            p.place(l, k.fence, 1, 7, 8, cb);
            p.fill(l, cb, 2, 7, 8, 4, 7, 8, k.fence, k.fence);
            p.place(l, k.fence, 5, 7, 8, cb);
            p.place(l, k.fence, 2, 8, 8, cb);
            p.place(l, k.fence, 3, 8, 8, cb);
            p.place(l, k.fence, 4, 8, 8, cb);
            let (vx, vy, vz) = (p.wx(3, 5), p.wy(5), p.wz(3, 5));
            if cb.inside(vx, vy, vz) {
                // TODO: no blaze spawner block entity yet
                l.set(vx, vy, vz, k.spawner);
            }
            for x in 0..=6 {
                for z in 0..=6 {
                    p.column_down(l, k.bricks, x, -1, z, cb);
                }
            }
        }
        Kind::CastleEntrance => {
            p.fill(l, cb, 0, 3, 0, 12, 4, 12, k.bricks, k.bricks);
            p.fill(l, cb, 0, 5, 0, 12, 13, 12, k.air, k.air);
            p.fill(l, cb, 0, 5, 0, 1, 12, 12, k.bricks, k.bricks);
            p.fill(l, cb, 11, 5, 0, 12, 12, 12, k.bricks, k.bricks);
            p.fill(l, cb, 2, 5, 11, 4, 12, 12, k.bricks, k.bricks);
            p.fill(l, cb, 8, 5, 11, 10, 12, 12, k.bricks, k.bricks);
            p.fill(l, cb, 5, 9, 11, 7, 12, 12, k.bricks, k.bricks);
            p.fill(l, cb, 2, 5, 0, 4, 12, 1, k.bricks, k.bricks);
            p.fill(l, cb, 8, 5, 0, 10, 12, 1, k.bricks, k.bricks);
            p.fill(l, cb, 5, 9, 0, 7, 12, 1, k.bricks, k.bricks);
            p.fill(l, cb, 2, 11, 2, 10, 12, 10, k.bricks, k.bricks);
            p.fill(l, cb, 5, 8, 0, 7, 8, 0, k.fence, k.fence);
            for i in (1..=11).step_by(2) {
                p.fill(l, cb, i, 10, 0, i, 11, 0, k.fence, k.fence);
                p.fill(l, cb, i, 10, 12, i, 11, 12, k.fence, k.fence);
                p.fill(l, cb, 0, 10, i, 0, 11, i, k.fence, k.fence);
                p.fill(l, cb, 12, 10, i, 12, 11, i, k.fence, k.fence);
                p.place(l, k.bricks, i, 13, 0, cb);
                p.place(l, k.bricks, i, 13, 12, cb);
                p.place(l, k.bricks, 0, 13, i, cb);
                p.place(l, k.bricks, 12, 13, i, cb);
                if i != 11 {
                    p.place(l, k.fence, i + 1, 13, 0, cb);
                    p.place(l, k.fence, i + 1, 13, 12, cb);
                    p.place(l, k.fence, 0, 13, i + 1, cb);
                    p.place(l, k.fence, 12, 13, i + 1, cb);
                }
            }
            p.place(l, k.fence, 0, 13, 0, cb);
            p.place(l, k.fence, 0, 13, 12, cb);
            p.place(l, k.fence, 12, 13, 12, cb);
            p.place(l, k.fence, 12, 13, 0, cb);
            for z in (3..=9).step_by(2) {
                p.fill(l, cb, 1, 7, z, 1, 8, z, k.fence, k.fence);
                p.fill(l, cb, 11, 7, z, 11, 8, z, k.fence, k.fence);
            }
            p.fill(l, cb, 4, 2, 0, 8, 2, 12, k.bricks, k.bricks);
            p.fill(l, cb, 0, 2, 4, 12, 2, 8, k.bricks, k.bricks);
            p.fill(l, cb, 4, 0, 0, 8, 1, 3, k.bricks, k.bricks);
            p.fill(l, cb, 4, 0, 9, 8, 1, 12, k.bricks, k.bricks);
            p.fill(l, cb, 0, 0, 4, 3, 1, 8, k.bricks, k.bricks);
            p.fill(l, cb, 9, 0, 4, 12, 1, 8, k.bricks, k.bricks);
            for x in 4..=8 {
                for lz in 0..=2 {
                    p.column_down(l, k.bricks, x, -1, lz, cb);
                    p.column_down(l, k.bricks, x, -1, 12 - lz, cb);
                }
            }
            for x in 0..=2 {
                for n in 4..=8 {
                    p.column_down(l, k.bricks, x, -1, n, cb);
                    p.column_down(l, k.bricks, 12 - x, -1, n, cb);
                }
            }
            p.fill(l, cb, 5, 5, 5, 7, 5, 7, k.bricks, k.bricks);
            p.fill(l, cb, 6, 1, 6, 6, 4, 6, k.air, k.air);
            p.place(l, k.bricks, 6, 0, 6, cb);
            p.place(l, k.lava, 6, 5, 6, cb);
            // TODO: PNX spreads this lava, we just leave the source
        }
        Kind::CastleStalkRoom => {
            p.fill(l, cb, 0, 3, 0, 12, 4, 12, k.bricks, k.bricks);
            p.fill(l, cb, 0, 5, 0, 12, 13, 12, k.air, k.air);
            p.fill(l, cb, 0, 5, 0, 1, 12, 12, k.bricks, k.bricks);
            p.fill(l, cb, 11, 5, 0, 12, 12, 12, k.bricks, k.bricks);
            p.fill(l, cb, 2, 5, 11, 4, 12, 12, k.bricks, k.bricks);
            p.fill(l, cb, 8, 5, 11, 10, 12, 12, k.bricks, k.bricks);
            p.fill(l, cb, 5, 9, 11, 7, 12, 12, k.bricks, k.bricks);
            p.fill(l, cb, 2, 5, 0, 4, 12, 1, k.bricks, k.bricks);
            p.fill(l, cb, 8, 5, 0, 10, 12, 1, k.bricks, k.bricks);
            p.fill(l, cb, 5, 9, 0, 7, 12, 1, k.bricks, k.bricks);
            p.fill(l, cb, 2, 11, 2, 10, 12, 10, k.bricks, k.bricks);
            for i in (1..=11).step_by(2) {
                p.fill(l, cb, i, 10, 0, i, 11, 0, k.fence, k.fence);
                p.fill(l, cb, i, 10, 12, i, 11, 12, k.fence, k.fence);
                p.fill(l, cb, 0, 10, i, 0, 11, i, k.fence, k.fence);
                p.fill(l, cb, 12, 10, i, 12, 11, i, k.fence, k.fence);
                p.place(l, k.bricks, i, 13, 0, cb);
                p.place(l, k.bricks, i, 13, 12, cb);
                p.place(l, k.bricks, 0, 13, i, cb);
                p.place(l, k.bricks, 12, 13, i, cb);
                if i != 11 {
                    p.place(l, k.fence, i + 1, 13, 0, cb);
                    p.place(l, k.fence, i + 1, 13, 12, cb);
                    p.place(l, k.fence, 0, 13, i + 1, cb);
                    p.place(l, k.fence, 12, 13, i + 1, cb);
                }
            }
            p.place(l, k.fence, 0, 13, 0, cb);
            p.place(l, k.fence, 0, 13, 12, cb);
            p.place(l, k.fence, 12, 13, 12, cb);
            p.place(l, k.fence, 12, 13, 0, cb);
            for z in (3..=9).step_by(2) {
                p.fill(l, cb, 1, 7, z, 1, 8, z, k.fence, k.fence);
                p.fill(l, cb, 11, 7, z, 11, 8, z, k.fence, k.fence);
            }
            let statrs_n = k.stairs_n;
            for y in 0..=6 {
                let z = y + 4;
                for x in 5..=7 {
                    p.place(l, statrs_n, x, 5 + y, z, cb);
                }
                if (5..=8).contains(&z) {
                    p.fill(l, cb, 5, 5, z, 7, y + 4, z, k.bricks, k.bricks);
                } else if (9..=10).contains(&z) {
                    p.fill(l, cb, 5, 8, z, 7, y + 4, z, k.bricks, k.bricks);
                }
                if y >= 1 {
                    p.fill(l, cb, 5, 6 + y, z, 7, 9 + y, z, k.air, k.air);
                }
            }
            for x in 5..=7 {
                p.place(l, statrs_n, x, 12, 11, cb);
            }
            p.fill(l, cb, 5, 6, 7, 5, 7, 7, k.fence, k.fence);
            p.fill(l, cb, 7, 6, 7, 7, 7, 7, k.fence, k.fence);
            p.fill(l, cb, 5, 13, 12, 7, 13, 12, k.air, k.air);
            p.fill(l, cb, 2, 5, 2, 3, 5, 3, k.bricks, k.bricks);
            p.fill(l, cb, 2, 5, 9, 3, 5, 10, k.bricks, k.bricks);
            p.fill(l, cb, 2, 5, 4, 2, 5, 8, k.bricks, k.bricks);
            p.fill(l, cb, 9, 5, 2, 10, 5, 3, k.bricks, k.bricks);
            p.fill(l, cb, 9, 5, 9, 10, 5, 10, k.bricks, k.bricks);
            p.fill(l, cb, 10, 5, 4, 10, 5, 8, k.bricks, k.bricks);
            let statrs_w = k.stairs_w;
            p.place(l, statrs_w, 4, 5, 2, cb);
            p.place(l, statrs_w, 4, 5, 3, cb);
            p.place(l, statrs_w, 4, 5, 9, cb);
            p.place(l, statrs_w, 4, 5, 10, cb);
            let statrs_e = k.stairs_e;
            p.place(l, statrs_e, 8, 5, 2, cb);
            p.place(l, statrs_e, 8, 5, 3, cb);
            p.place(l, statrs_e, 8, 5, 9, cb);
            p.place(l, statrs_e, 8, 5, 10, cb);
            p.fill(l, cb, 3, 4, 4, 4, 4, 8, k.soul_sand, k.soul_sand);
            p.fill(l, cb, 8, 4, 4, 9, 4, 8, k.soul_sand, k.soul_sand);
            p.fill(l, cb, 3, 5, 4, 4, 5, 8, k.wart, k.wart);
            p.fill(l, cb, 8, 5, 4, 9, 5, 8, k.wart, k.wart);
            p.fill(l, cb, 4, 2, 0, 8, 2, 12, k.bricks, k.bricks);
            p.fill(l, cb, 0, 2, 4, 12, 2, 8, k.bricks, k.bricks);
            p.fill(l, cb, 4, 0, 0, 8, 1, 3, k.bricks, k.bricks);
            p.fill(l, cb, 4, 0, 9, 8, 1, 12, k.bricks, k.bricks);
            p.fill(l, cb, 0, 0, 4, 3, 1, 8, k.bricks, k.bricks);
            p.fill(l, cb, 9, 0, 4, 12, 1, 8, k.bricks, k.bricks);
            for x in 4..=8 {
                for z in 0..=2 {
                    p.column_down(l, k.bricks, x, -1, z, cb);
                    p.column_down(l, k.bricks, x, -1, 12 - z, cb);
                }
            }
            for x in 0..=2 {
                for z in 4..=8 {
                    p.column_down(l, k.bricks, x, -1, z, cb);
                    p.column_down(l, k.bricks, 12 - x, -1, z, cb);
                }
            }
        }
        Kind::CastleSmallCorridorPiece => {
            p.fill(l, cb, 0, 0, 0, 4, 1, 4, k.bricks, k.bricks);
            p.fill(l, cb, 0, 2, 0, 4, 5, 4, k.air, k.air);
            p.fill(l, cb, 0, 2, 0, 0, 5, 4, k.bricks, k.bricks);
            p.fill(l, cb, 4, 2, 0, 4, 5, 4, k.bricks, k.bricks);
            p.fill(l, cb, 0, 3, 1, 0, 4, 1, k.fence, k.fence);
            p.fill(l, cb, 0, 3, 3, 0, 4, 3, k.fence, k.fence);
            p.fill(l, cb, 4, 3, 1, 4, 4, 1, k.fence, k.fence);
            p.fill(l, cb, 4, 3, 3, 4, 4, 3, k.fence, k.fence);
            p.fill(l, cb, 0, 6, 0, 4, 6, 4, k.bricks, k.bricks);
            for x in 0..=4 {
                for z in 0..=4 {
                    p.column_down(l, k.bricks, x, -1, z, cb);
                }
            }
        }
        Kind::CastleSmallCorridorCrossingPiece => {
            p.fill(l, cb, 0, 0, 0, 4, 1, 4, k.bricks, k.bricks);
            p.fill(l, cb, 0, 2, 0, 4, 5, 4, k.air, k.air);
            p.fill(l, cb, 0, 2, 0, 0, 5, 0, k.bricks, k.bricks);
            p.fill(l, cb, 4, 2, 0, 4, 5, 0, k.bricks, k.bricks);
            p.fill(l, cb, 0, 2, 4, 0, 5, 4, k.bricks, k.bricks);
            p.fill(l, cb, 4, 2, 4, 4, 5, 4, k.bricks, k.bricks);
            p.fill(l, cb, 0, 6, 0, 4, 6, 4, k.bricks, k.bricks);
            for x in 0..=4 {
                for z in 0..=4 {
                    p.column_down(l, k.bricks, x, -1, z, cb);
                }
            }
        }
        Kind::CastleSmallCorridorRightTurnPiece => {
            p.fill(l, cb, 0, 0, 0, 4, 1, 4, k.bricks, k.bricks);
            p.fill(l, cb, 0, 2, 0, 4, 5, 4, k.air, k.air);
            p.fill(l, cb, 0, 2, 0, 0, 5, 4, k.bricks, k.bricks);
            p.fill(l, cb, 0, 3, 1, 0, 4, 1, k.fence, k.fence);
            p.fill(l, cb, 0, 3, 3, 0, 4, 3, k.fence, k.fence);
            p.fill(l, cb, 4, 2, 0, 4, 5, 0, k.bricks, k.bricks);
            p.fill(l, cb, 1, 2, 4, 4, 5, 4, k.bricks, k.bricks);
            p.fill(l, cb, 1, 3, 4, 1, 4, 4, k.fence, k.fence);
            p.fill(l, cb, 3, 3, 4, 3, 4, 4, k.fence, k.fence);
            if p.chest && cb.inside(p.wx(1, 3), p.wy(2), p.wz(1, 3)) {
                // TODO: chest loot (needs block entities)
                p.place(l, k.chest(p.orient), 1, 2, 3, cb);
            }
            p.fill(l, cb, 0, 6, 0, 4, 6, 4, k.bricks, k.bricks);
            for x in 0..=4 {
                for z in 0..=4 {
                    p.column_down(l, k.bricks, x, -1, z, cb);
                }
            }
        }
        Kind::CastleSmallCorridorLeftTurnPiece => {
            p.fill(l, cb, 0, 0, 0, 4, 1, 4, k.bricks, k.bricks);
            p.fill(l, cb, 0, 2, 0, 4, 5, 4, k.air, k.air);
            p.fill(l, cb, 4, 2, 0, 4, 5, 4, k.bricks, k.bricks);
            p.fill(l, cb, 4, 3, 1, 4, 4, 1, k.fence, k.fence);
            p.fill(l, cb, 4, 3, 3, 4, 4, 3, k.fence, k.fence);
            p.fill(l, cb, 0, 2, 0, 0, 5, 0, k.bricks, k.bricks);
            p.fill(l, cb, 0, 2, 4, 3, 5, 4, k.bricks, k.bricks);
            p.fill(l, cb, 1, 3, 4, 1, 4, 4, k.fence, k.fence);
            p.fill(l, cb, 3, 3, 4, 3, 4, 4, k.fence, k.fence);
            if p.chest && cb.inside(p.wx(3, 3), p.wy(2), p.wz(3, 3)) {
                // TODO: chest loot (needs block entities)
                p.place(l, k.chest(p.orient), 3, 2, 3, cb);
            }
            p.fill(l, cb, 0, 6, 0, 4, 6, 4, k.bricks, k.bricks);
            for x in 0..=4 {
                for z in 0..=4 {
                    p.column_down(l, k.bricks, x, -1, z, cb);
                }
            }
        }
        Kind::CastleCorridorStairsPiece => {
            for i in 0..=9 {
                let max_y = 1.max(7 - i);
                let min_y = (max_y + 5).max(14 - i).min(13);
                p.fill(l, cb, 0, 0, i, 4, max_y, i, k.bricks, k.bricks);
                p.fill(l, cb, 1, max_y + 1, i, 3, min_y - 1, i, k.air, k.air);
                if i <= 6 {
                    p.place(l, k.stairs_s, 1, max_y + 1, i, cb);
                    p.place(l, k.stairs_s, 2, max_y + 1, i, cb);
                    p.place(l, k.stairs_s, 3, max_y + 1, i, cb);
                }
                p.fill(l, cb, 0, min_y, i, 4, min_y, i, k.bricks, k.bricks);
                p.fill(l, cb, 0, max_y + 1, i, 0, min_y - 1, i, k.bricks, k.bricks);
                p.fill(l, cb, 4, max_y + 1, i, 4, min_y - 1, i, k.bricks, k.bricks);
                if i & 1 == 0 {
                    p.fill(l, cb, 0, max_y + 2, i, 0, max_y + 3, i, k.fence, k.fence);
                    p.fill(l, cb, 4, max_y + 2, i, 4, max_y + 3, i, k.fence, k.fence);
                }
                for x in 0..=4 {
                    p.column_down(l, k.bricks, x, -1, i, cb);
                }
            }
        }
        Kind::CastleCorridorTBalconyPiece => {
            p.fill(l, cb, 0, 0, 0, 8, 1, 8, k.bricks, k.bricks);
            p.fill(l, cb, 0, 2, 0, 8, 5, 8, k.air, k.air);
            p.fill(l, cb, 0, 6, 0, 8, 6, 5, k.bricks, k.bricks);
            p.fill(l, cb, 0, 2, 0, 2, 5, 0, k.bricks, k.bricks);
            p.fill(l, cb, 6, 2, 0, 8, 5, 0, k.bricks, k.bricks);
            p.fill(l, cb, 1, 3, 0, 1, 4, 0, k.fence, k.fence);
            p.fill(l, cb, 7, 3, 0, 7, 4, 0, k.fence, k.fence);
            p.fill(l, cb, 0, 2, 4, 8, 2, 8, k.bricks, k.bricks);
            p.fill(l, cb, 1, 1, 4, 2, 2, 4, k.air, k.air);
            p.fill(l, cb, 6, 1, 4, 7, 2, 4, k.air, k.air);
            p.fill(l, cb, 1, 3, 8, 7, 3, 8, k.fence, k.fence);
            p.place(l, k.fence, 0, 3, 8, cb);
            p.place(l, k.fence, 8, 3, 8, cb);
            p.fill(l, cb, 0, 3, 6, 0, 3, 7, k.fence, k.fence);
            p.fill(l, cb, 8, 3, 6, 8, 3, 7, k.fence, k.fence);
            p.fill(l, cb, 0, 3, 4, 0, 5, 5, k.bricks, k.bricks);
            p.fill(l, cb, 8, 3, 4, 8, 5, 5, k.bricks, k.bricks);
            p.fill(l, cb, 1, 3, 5, 2, 5, 5, k.bricks, k.bricks);
            p.fill(l, cb, 6, 3, 5, 7, 5, 5, k.bricks, k.bricks);
            p.fill(l, cb, 1, 4, 5, 1, 5, 5, k.fence, k.fence);
            p.fill(l, cb, 7, 4, 5, 7, 5, 5, k.fence, k.fence);
            for x in 0..=8 {
                for z in 0..=5 {
                    p.column_down(l, k.bricks, x, -1, z, cb);
                }
            }
        }
    }
}

// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! PNX `MineshaftPopulator` and `MineshaftPieces`.

use super::super::super::feature::blocks as bl;
use super::super::{StructWorld, Xoroshiro};
use super::piece::{BBox, Base, Face, Lvl, collides, sturdy};
use crate::overworld::rng::BedrockRandom;

const PROBABILITY: f32 = 0.004;
const DISTANCE_BOUND: i32 = 80;
/// PNX `START_SEARCH_RADIUS`.
pub(super) const RADIUS: i32 = 7;

/// PNX `MineshaftPlacementCache.isStart` (seed only).
pub(super) fn is_start(seed: i64, cx: i32, cz: i32) -> bool {
    let s = seed as i32;
    let mut r = BedrockRandom::new(s as i64);
    let (r1, r2) = (r.next_int(), r.next_int());
    r.set_seed((cx.wrapping_mul(r1) ^ cz.wrapping_mul(r2) ^ s) as i64);
    r.next_int();
    r.next_float() < PROBABILITY && r.next_int_below(DISTANCE_BOUND) < cx.unsigned_abs().max(cz.unsigned_abs()) as i32
}

#[derive(Clone, Copy, PartialEq)]
enum Ty {
    Normal,
    Mesa,
}

enum Kind {
    Room { entrances: Vec<BBox> },
    Corridor { rails: bool, spider: bool, sections: i32, placed_spider: bool },
    Crossing { dir: Face, two_floors: bool },
    Stairs,
}

struct Piece {
    b: Base,
    kind: Kind,
}

struct Gen<'r> {
    pieces: Vec<Piece>,
    r: &'r mut Xoroshiro,
}

impl Gen<'_> {
    fn collides(&self, b: &BBox) -> bool {
        collides(self.pieces.iter().map(|p| &p.b.bb), b)
    }

    fn random_piece(&mut self, x: i32, y: i32, z: i32, o: Face, depth: i32) -> Option<Piece> {
        let chance = self.r.next_bounded_int(100);
        if chance >= 80 {
            let mut b = BBox::new(x, y, z, x, y + 2, z);
            if self.r.next_bounded_int(4) == 0 {
                b.y1 += 4;
            }
            match o {
                Face::North => (b.x0, b.x1, b.z0) = (x - 1, x + 3, z - 4),
                Face::South => (b.x0, b.x1, b.z1) = (x - 1, x + 3, z + 4),
                Face::West => (b.x0, b.z0, b.z1) = (x - 4, z - 1, z + 3),
                Face::East => (b.x1, b.z0, b.z1) = (x + 4, z - 1, z + 3),
            }
            (!self.collides(&b)).then(|| Piece { b: Base::new(b, depth, None), kind: Kind::Crossing { dir: o, two_floors: b.y_span() > 3 } })
        } else if chance >= 70 {
            let mut b = BBox::new(x, y - 5, z, x, y + 2, z);
            match o {
                Face::North => (b.x1, b.z0) = (x + 2, z - 8),
                Face::South => (b.x1, b.z1) = (x + 2, z + 8),
                Face::West => (b.x0, b.z1) = (x - 8, z + 2),
                Face::East => (b.x1, b.z1) = (x + 8, z + 2),
            }
            (!self.collides(&b)).then(|| Piece { b: Base::new(b, depth, Some(o)), kind: Kind::Stairs })
        } else {
            let mut b = BBox::new(x, y, z, x, y + 2, z);
            let mut count = self.r.next_bounded_int(3) + 2;
            while count > 0 {
                let i = count * 5;
                match o {
                    Face::North => (b.x1, b.z0) = (x + 2, z - (i - 1)),
                    Face::South => (b.x1, b.z1) = (x + 2, z + i - 1),
                    Face::West => (b.x0, b.z1) = (x - (i - 1), z + 2),
                    Face::East => (b.x1, b.z1) = (x + i - 1, z + 2),
                }
                if !self.collides(&b) {
                    break;
                }
                count -= 1;
            }
            if count <= 0 {
                return None;
            }
            let rails = self.r.next_bounded_int(3) == 0;
            let spider = !rails && self.r.next_bounded_int(23) == 0;
            let sections = if o.axis_z() { b.z_span() / 5 } else { b.x_span() / 5 };
            Some(Piece { b: Base::new(b, depth, Some(o)), kind: Kind::Corridor { rails, spider, sections, placed_spider: false } })
        }
    }

    /// PNX `generateAndAddPiece`; returns the new piece's box.
    fn add(&mut self, x: i32, y: i32, z: i32, o: Face, depth: i32) -> Option<BBox> {
        let s = self.pieces[0].b.bb;
        if depth > 8 || (x - s.x0).abs() > 80 || (z - s.z0).abs() > 80 {
            return None;
        }
        let p = self.random_piece(x, y, z, o, depth + 1)?;
        let bb = p.b.bb;
        self.pieces.push(p);
        self.children(self.pieces.len() - 1);
        Some(bb)
    }

    fn children(&mut self, i: usize) {
        let (b, d) = (self.pieces[i].b.bb, self.pieces[i].b.depth);
        let o = self.pieces[i].b.orient;
        match self.pieces[i].kind {
            Kind::Room { .. } => {
                let mut ent = Vec::new();
                let yo = (b.y_span() - 4).max(1);
                let mut x = 0;
                while x < b.x_span() {
                    x += self.r.next_bounded_int(b.x_span());
                    if x + 3 > b.x_span() {
                        break;
                    }
                    let y = b.y0 + self.r.next_bounded_int(yo) + 1;
                    if let Some(n) = self.add(b.x0 + x, y, b.z0 - 1, Face::North, d) {
                        ent.push(BBox::new(n.x0, n.y0, b.z0, n.x1, n.y1, b.z0 + 1));
                    }
                    x += 4;
                }
                let mut x = 0;
                while x < b.x_span() {
                    x += self.r.next_bounded_int(b.x_span());
                    if x + 3 > b.x_span() {
                        break;
                    }
                    let y = b.y0 + self.r.next_bounded_int(yo) + 1;
                    if let Some(n) = self.add(b.x0 + x, y, b.z1 + 1, Face::South, d) {
                        ent.push(BBox::new(n.x0, n.y0, b.z1 - 1, n.x1, n.y1, b.z1));
                    }
                    x += 4;
                }
                let mut z = 0;
                while z < b.z_span() {
                    z += self.r.next_bounded_int(b.z_span());
                    if z + 3 > b.z_span() {
                        break;
                    }
                    let y = b.y0 + self.r.next_bounded_int(yo) + 1;
                    if let Some(n) = self.add(b.x0 - 1, y, b.z0 + z, Face::West, d) {
                        ent.push(BBox::new(b.x0, n.y0, n.z0, b.x0 + 1, n.y1, n.z1));
                    }
                    z += 4;
                }
                let mut z = 0;
                while z < b.z_span() {
                    z += self.r.next_bounded_int(b.z_span());
                    if z + 3 > b.z_span() {
                        break;
                    }
                    let y = b.y0 + self.r.next_bounded_int(yo) + 1;
                    if let Some(n) = self.add(b.x1 + 1, y, b.z0 + z, Face::East, d) {
                        ent.push(BBox::new(b.x1 - 1, n.y0, n.z0, b.x1, n.y1, n.z1));
                    }
                    z += 4;
                }
                if let Kind::Room { entrances } = &mut self.pieces[i].kind {
                    *entrances = ent;
                }
            }
            Kind::Corridor { .. } => {
                let target = self.r.next_bounded_int(4);
                if let Some(o) = o {
                    let y = |r: &mut Xoroshiro| b.y0 - 1 + r.next_bounded_int(3);
                    match o {
                        Face::North => {
                            let yy = y(self.r);
                            if target <= 1 {
                                self.add(b.x0, yy, b.z0 - 1, o, d);
                            } else if target == 2 {
                                self.add(b.x0 - 1, yy, b.z0, Face::West, d);
                            } else {
                                self.add(b.x1 + 1, yy, b.z0, Face::East, d);
                            }
                        }
                        Face::South => {
                            let yy = y(self.r);
                            if target <= 1 {
                                self.add(b.x0, yy, b.z1 + 1, o, d);
                            } else if target == 2 {
                                self.add(b.x0 - 1, yy, b.z1 - 3, Face::West, d);
                            } else {
                                self.add(b.x1 + 1, yy, b.z1 - 3, Face::East, d);
                            }
                        }
                        Face::West => {
                            let yy = y(self.r);
                            if target <= 1 {
                                self.add(b.x0 - 1, yy, b.z0, o, d);
                            } else if target == 2 {
                                self.add(b.x0, yy, b.z0 - 1, Face::North, d);
                            } else {
                                self.add(b.x0, yy, b.z1 + 1, Face::South, d);
                            }
                        }
                        Face::East => {
                            let yy = y(self.r);
                            if target <= 1 {
                                self.add(b.x1 + 1, yy, b.z0, o, d);
                            } else if target == 2 {
                                self.add(b.x1 - 3, yy, b.z0 - 1, Face::North, d);
                            } else {
                                self.add(b.x1 - 3, yy, b.z1 + 1, Face::South, d);
                            }
                        }
                    }
                }
                if d < 8 {
                    if !matches!(o, Some(Face::North | Face::South)) {
                        let mut x = b.x0 + 3;
                        while x + 3 <= b.x1 {
                            match self.r.next_bounded_int(5) {
                                0 => drop(self.add(x, b.y0, b.z0 - 1, Face::North, d + 1)),
                                1 => drop(self.add(x, b.y0, b.z1 + 1, Face::South, d + 1)),
                                _ => {}
                            }
                            x += 5;
                        }
                    } else {
                        let mut z = b.z0 + 3;
                        while z + 3 <= b.z1 {
                            match self.r.next_bounded_int(5) {
                                0 => drop(self.add(b.x0 - 1, b.y0, z, Face::West, d + 1)),
                                1 => drop(self.add(b.x1 + 1, b.y0, z, Face::East, d + 1)),
                                _ => {}
                            }
                            z += 5;
                        }
                    }
                }
            }
            Kind::Crossing { dir, two_floors } => {
                match dir {
                    Face::North => {
                        self.add(b.x0 + 1, b.y0, b.z0 - 1, Face::North, d);
                        self.add(b.x0 - 1, b.y0, b.z0 + 1, Face::West, d);
                        self.add(b.x1 + 1, b.y0, b.z0 + 1, Face::East, d);
                    }
                    Face::South => {
                        self.add(b.x0 + 1, b.y0, b.z1 + 1, Face::South, d);
                        self.add(b.x0 - 1, b.y0, b.z0 + 1, Face::West, d);
                        self.add(b.x1 + 1, b.y0, b.z0 + 1, Face::East, d);
                    }
                    Face::West => {
                        self.add(b.x0 + 1, b.y0, b.z0 - 1, Face::North, d);
                        self.add(b.x0 + 1, b.y0, b.z1 + 1, Face::South, d);
                        self.add(b.x0 - 1, b.y0, b.z0 + 1, Face::West, d);
                    }
                    Face::East => {
                        self.add(b.x0 + 1, b.y0, b.z0 - 1, Face::North, d);
                        self.add(b.x0 + 1, b.y0, b.z1 + 1, Face::South, d);
                        self.add(b.x1 + 1, b.y0, b.z0 + 1, Face::East, d);
                    }
                }
                if two_floors {
                    if self.r.next_boolean() {
                        self.add(b.x0 + 1, b.y0 + 4, b.z0 - 1, Face::North, d);
                    }
                    if self.r.next_boolean() {
                        self.add(b.x0 - 1, b.y0 + 4, b.z0 + 1, Face::West, d);
                    }
                    if self.r.next_boolean() {
                        self.add(b.x1 + 1, b.y0 + 4, b.z0 + 1, Face::East, d);
                    }
                    if self.r.next_boolean() {
                        self.add(b.x0 + 1, b.y0 + 4, b.z1 + 1, Face::South, d);
                    }
                }
            }
            Kind::Stairs => match o {
                Some(Face::North) => drop(self.add(b.x0, b.y0, b.z0 - 1, Face::North, d)),
                Some(Face::South) => drop(self.add(b.x0, b.y0, b.z1 + 1, Face::South, d)),
                Some(Face::West) => drop(self.add(b.x0 - 1, b.y0, b.z0, Face::West, d)),
                Some(Face::East) => drop(self.add(b.x1 + 1, b.y0, b.z0, Face::East, d)),
                None => {}
            },
        }
    }
}

struct Blocks {
    planks: i32,
    fence: i32,
    wood: i32,
    chain: i32,
    web: i32,
    spawner: i32,
    torch_n: i32,
    torch_s: i32,
    rail_ns: i32,
    rail_ew: i32,
    air: i32,
}

fn blocks(ty: Ty) -> Blocks {
    let m = ty == Ty::Mesa;
    let torch = |d: &str| bl::with(bl::id("torch"), &[("torch_facing_direction", bl::s(d))]);
    let rail = |d: i32| bl::with(bl::id("rail"), &[("rail_direction", bl::i(d))]);
    Blocks {
        planks: bl::id(if m { "dark_oak_planks" } else { "oak_planks" }),
        fence: bl::id(if m { "dark_oak_fence" } else { "oak_fence" }),
        wood: bl::id(if m { "dark_oak_log" } else { "oak_log" }),
        chain: bl::id("iron_chain"),
        web: bl::id("web"),
        spawner: bl::id("mob_spawner"),
        torch_n: torch("north"),
        torch_s: torch("south"),
        rail_ns: rail(0),
        rail_ew: rail(1),
        air: bl::air(),
    }
}

fn fallable(b: i32) -> bool {
    let n = bl::short(b);
    matches!(n, "sand" | "red_sand" | "gravel" | "suspicious_sand" | "suspicious_gravel" | "scaffolding") || n.ends_with("concrete_powder") || n.ends_with("anvil")
}

fn support_material(b: i32) -> bool {
    bl::is_air(b) || bl::is_liquid(b)
}

impl Piece {
    fn post(&mut self, l: &mut Lvl, r: &mut Xoroshiro, cb: &BBox, k: &Blocks) {
        let p = &self.b;
        if p.edges_liquid(l, cb) {
            return;
        }
        let bb = p.bb;
        let air = k.air;
        match &mut self.kind {
            Kind::Room { entrances } => {
                p.fill(l, cb, bb.x0, bb.y0 + 1, bb.z0, bb.x1, (bb.y0 + 3).min(bb.y1), bb.z1, air, air, false);
                for e in entrances.iter() {
                    p.fill(l, cb, e.x0, e.y1 - 2, e.z0, e.x1, e.y1, e.z1, air, air, false);
                }
                p.upper_half_sphere(l, cb, bb.x0, bb.y0 + 4, bb.z0, bb.x1, bb.y1, bb.z1, air);
            }
            Kind::Corridor { rails, spider, sections, placed_spider } => {
                let z1 = *sections * 5 - 1;
                p.fill(l, cb, 0, 0, 0, 2, 1, z1, air, air, false);
                p.maybe_fill(l, cb, r, 80, 0, 2, 0, 2, 2, z1, air, air, false, false);
                if *spider {
                    p.maybe_fill(l, cb, r, 60, 0, 0, 0, 2, 1, z1, k.web, air, false, true);
                }
                for i in 0..*sections {
                    let z = 2 + i * 5;
                    support(p, l, cb, z, r, k);
                    for (prob, x, dz) in [(10, 0, -1), (10, 2, -1), (10, 0, 1), (10, 2, 1), (5, 0, -2), (5, 2, -2), (5, 0, 2), (5, 2, 2)] {
                        cobweb(p, l, cb, r, prob, x, 2, z + dz, k);
                    }
                    if r.next_bounded_int(100) == 0 {
                        chest(p, l, cb, r, 2, 0, z - 1, k);
                    }
                    if r.next_bounded_int(100) == 0 {
                        chest(p, l, cb, r, 0, 0, z + 1, k);
                    }
                    if *spider && !*placed_spider {
                        let pz = z - 1 + r.next_bounded_int(3);
                        let (vx, vy, vz) = (p.wx(1, pz), p.wy(0), p.wz(1, pz));
                        if cb.inside(vx, vy, vz) && p.interior(l, 1, 0, pz, cb) {
                            *placed_spider = true;
                            l.set(vx, vy, vz, k.spawner); // TODO: spawner block entity
                        }
                    }
                }
                for x in 0..=2 {
                    for z in 0..=z1 {
                        set_planks(p, l, cb, k.planks, x, -1, z);
                    }
                }
                double_support(p, l, cb, 0, -1, 2, k);
                if *sections > 1 {
                    double_support(p, l, cb, 0, -1, z1 - 2, k);
                }
                if *rails {
                    for z in 0..=z1 {
                        let b = p.block(l, 1, -1, z, cb);
                        let id = l.get(p.wx(1, z), p.wy(-1), p.wz(1, z));
                        if !bl::is_air(b) && sturdy(id) {
                            let prob = if p.interior(l, 1, 0, z, cb) { 70 } else { 90 };
                            p.maybe_place(l, cb, r, prob, 1, 0, z, k.rail_ns);
                        }
                    }
                }
            }
            Kind::Crossing { two_floors, .. } => {
                if *two_floors {
                    p.fill(l, cb, bb.x0 + 1, bb.y0, bb.z0, bb.x1 - 1, bb.y0 + 2, bb.z1, air, air, false);
                    p.fill(l, cb, bb.x0, bb.y0, bb.z0 + 1, bb.x1, bb.y0 + 2, bb.z1 - 1, air, air, false);
                    p.fill(l, cb, bb.x0 + 1, bb.y1 - 2, bb.z0, bb.x1 - 1, bb.y1, bb.z1, air, air, false);
                    p.fill(l, cb, bb.x0, bb.y1 - 2, bb.z0 + 1, bb.x1, bb.y1, bb.z1 - 1, air, air, false);
                    p.fill(l, cb, bb.x0 + 1, bb.y0 + 3, bb.z0 + 1, bb.x1 - 1, bb.y0 + 3, bb.z1 - 1, air, air, false);
                } else {
                    p.fill(l, cb, bb.x0 + 1, bb.y0, bb.z0, bb.x1 - 1, bb.y1, bb.z1, air, air, false);
                    p.fill(l, cb, bb.x0, bb.y0, bb.z0 + 1, bb.x1, bb.y1, bb.z1 - 1, air, air, false);
                }
                for (x, z) in [(bb.x0 + 1, bb.z0 + 1), (bb.x0 + 1, bb.z1 - 1), (bb.x1 - 1, bb.z0 + 1), (bb.x1 - 1, bb.z1 - 1)] {
                    if !bl::is_air(p.block(l, x, bb.y1 + 1, z, cb)) {
                        p.fill(l, cb, x, bb.y0, z, x, bb.y1, z, k.planks, air, false);
                    }
                }
                // PNX quirk kept: crossings have no orientation, so these local coords are absolute.
                for x in 0..bb.x_span() {
                    for z in 0..bb.z_span() {
                        set_planks(p, l, cb, k.planks, x, -1, z);
                    }
                }
            }
            Kind::Stairs => {
                p.fill(l, cb, 0, 5, 0, 2, 7, 1, air, air, false);
                p.fill(l, cb, 0, 0, 7, 2, 2, 8, air, air, false);
                for i in 0..5 {
                    p.fill(l, cb, 0, 5 - i - (i < 4) as i32, 2 + i, 2, 7 - i, 2 + i, air, air, false);
                }
            }
        }
    }
}

fn set_planks(p: &Base, l: &mut Lvl, cb: &BBox, planks: i32, x: i32, y: i32, z: i32) {
    if !p.interior(l, x, y, z, cb) {
        return;
    }
    let (vx, vy, vz) = (p.wx(x, z), p.wy(y), p.wz(x, z));
    if cb.inside(vx, vy, vz) && !sturdy(l.get(vx, vy, vz)) {
        l.set(vx, vy, vz, planks);
    }
}

fn support(p: &Base, l: &mut Lvl, cb: &BBox, z: i32, r: &mut Xoroshiro, k: &Blocks) {
    let (x0, y0, y1, x1) = (0, 0, 2, 2);
    if (x0..=x1).any(|x| bl::is_air(p.block(l, x, y1 + 1, z, cb))) {
        return;
    }
    p.fill(l, cb, x0, y0, z, x0, y1 - 1, z, k.fence, k.air, false);
    p.fill(l, cb, x1, y0, z, x1, y1 - 1, z, k.fence, k.air, false);
    if r.next_bounded_int(4) == 0 {
        p.fill(l, cb, x0, y1, z, x0, y1, z, k.planks, k.air, false);
        p.fill(l, cb, x1, y1, z, x1, y1, z, k.planks, k.air, false);
    } else {
        p.fill(l, cb, x0, y1, z, x1, y1, z, k.planks, k.air, false);
        p.maybe_place(l, cb, r, 5, x0 + 1, y1, z - 1, k.torch_s);
        p.maybe_place(l, cb, r, 5, x0 + 1, y1, z + 1, k.torch_n);
    }
}

#[allow(clippy::too_many_arguments)]
fn cobweb(p: &Base, l: &mut Lvl, cb: &BBox, r: &mut Xoroshiro, prob: i32, x: i32, y: i32, z: i32, k: &Blocks) {
    if !p.interior(l, x, y, z, cb) {
        return;
    }
    let (wx, wy, wz) = (p.wx(x, z), p.wy(y), p.wz(x, z));
    let mut n = 0;
    for (dx, dy, dz) in [(0, 1, 0), (0, -1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)] {
        let (nx, ny, nz) = (wx + dx, wy + dy, wz + dz);
        if cb.inside(nx, ny, nz) && sturdy(l.get(nx, ny, nz)) {
            n += 1;
            if n >= 2 {
                p.maybe_place(l, cb, r, prob, x, y, z, k.web);
                return;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn chest(p: &Base, l: &mut Lvl, cb: &BBox, r: &mut Xoroshiro, x: i32, y: i32, z: i32, k: &Blocks) {
    let (vx, vy, vz) = (p.wx(x, z), p.wy(y), p.wz(x, z));
    if cb.inside(vx, vy, vz) && bl::is(l.get(vx, vy, vz), "air") && !bl::is(l.get(vx, vy - 1, vz), "air") {
        // TODO: chest minecart entity, only the rail for now.
        p.place(l, if r.next_boolean() { k.rail_ns } else { k.rail_ew }, x, y, z, cb);
    }
}

fn double_support(p: &Base, l: &mut Lvl, cb: &BBox, x: i32, y: i32, z: i32, k: &Blocks) {
    for x in [x, x + 2] {
        if bl::name(p.block(l, x, y, z, cb)) == bl::name(k.planks) {
            pillar_or_chain(p, l, x, y, z, cb, k);
        }
    }
}

fn pillar_or_chain(p: &Base, l: &mut Lvl, x: i32, y: i32, z: i32, cb: &BBox, k: &Blocks) {
    let (wx, wy, wz) = (p.wx(x, z), p.wy(y), p.wz(x, z));
    if !cb.inside(wx, wy, wz) {
        return;
    }
    let mut d = 1;
    while d <= 20 && wy - d > -64 + 1 {
        let cur = l.get(wx, wy - d, wz);
        if !support_material(cur) {
            if sturdy(cur) {
                for py in wy - d + 1..wy {
                    l.set(wx, py, wz, k.wood);
                }
            }
            return;
        }
        if bl::is_lava(cur) {
            return;
        }
        d += 1;
    }
    let mut d = 1;
    while d <= 50 && wy + d < 320 {
        let ny = wy + d;
        let cur = l.get(wx, ny, wz);
        if !support_material(cur) {
            if sturdy(cur) && !fallable(cur) {
                l.set(wx, wy + 1, wz, k.fence);
                for py in wy + 2..ny {
                    l.set(wx, py, wz, k.chain);
                }
            }
            return;
        }
        d += 1;
    }
}

fn is_badlands(b: u16) -> bool {
    use crate::overworld::biome::id as b_;
    (b_::MESA..=39).contains(&b) || (b_::MESA_BRYCE..=167).contains(&b)
}

/// PNX `MineshaftPopulator.apply` for start chunk (cx, cz), over every chunk it reaches.
pub(super) fn build(w: &mut StructWorld, r: &mut Xoroshiro, cx: i32, cz: i32) {
    let seed = w.seed;
    if !is_start(seed, cx, cz) {
        return;
    }
    let ty = if is_badlands(super::raw_biome(&w.ow.router, cx << 4, 0, cz << 4)) { Ty::Mesa } else { Ty::Normal };
    // StructureStart's random.
    let mut sr = Xoroshiro::new(seed);
    let s = (cx as i64).wrapping_mul(sr.next_int() as i64) ^ (cz as i64).wrapping_mul(sr.next_int() as i64) ^ seed;
    sr.set_seed(s);
    let (x, z) = ((cx << 4) + 2, (cz << 4) + 2);
    let x1 = x + 7 + sr.next_bounded_int(6);
    let y1 = 54 + sr.next_bounded_int(6);
    let z1 = z + 7 + sr.next_bounded_int(6);
    let room = Piece { b: Base::new(BBox::new(x, 50, z, x1, y1, z1), 0, None), kind: Kind::Room { entrances: Vec::new() } };
    let mut g = Gen { pieces: vec![room], r: &mut sr };
    g.children(0);
    let mut pieces = g.pieces;
    let mut bb = BBox::unknown();
    for p in &pieces {
        bb.expand(&p.b.bb);
    }
    let dy = if ty == Ty::Mesa {
        64 - bb.y1 + bb.y_span() / 2 + 5
    } else {
        let range = super::super::super::aquifer::SEA_LEVEL - 10;
        let mut y = bb.y_span() - 64 + 1;
        if y < range {
            y += sr.next_bounded_int(range - y);
        }
        y - bb.y1
    };
    bb.shift(0, dy, 0);
    for p in &mut pieces {
        p.b.bb.shift(0, dy, 0);
        if let Kind::Room { entrances } = &mut p.kind {
            entrances.iter_mut().for_each(|e| e.shift(0, dy, 0));
        }
    }
    let k = blocks(ty);
    let mut l = Lvl { w, water_fix: true };
    for ccx in (bb.x0 >> 4)..=(bb.x1 >> 4) {
        for ccz in (bb.z0 >> 4)..=(bb.z1 >> 4) {
            let cb = BBox::chunk(ccx, ccz);
            r.set_seed(seed);
            for p in &mut pieces {
                if p.b.bb.intersects(&cb) {
                    p.post(&mut l, r, &cb, &k);
                }
            }
        }
    }
}

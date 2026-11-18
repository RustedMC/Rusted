// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! PNX `BoundingBox` and the `StructurePiece` placement helpers shared by mineshafts and
//! strongholds.

use super::super::super::feature::blocks as bl;
use super::super::super::feature::objects::World;
use super::super::{StructWorld, Xoroshiro};
use crate::blocks::State;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Face {
    North,
    South,
    West,
    East,
}

impl Face {
    pub(crate) fn axis_z(self) -> bool {
        matches!(self, Face::North | Face::South)
    }
    fn opposite_name(self) -> &'static str {
        match self {
            Face::North => "south",
            Face::South => "north",
            Face::West => "east",
            Face::East => "west",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct BBox {
    pub x0: i32,
    pub y0: i32,
    pub z0: i32,
    pub x1: i32,
    pub y1: i32,
    pub z1: i32,
}

impl BBox {
    pub(crate) const fn new(x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) -> Self {
        Self { x0, y0, z0, x1, y1, z1 }
    }
    pub(crate) const fn unknown() -> Self {
        Self::new(i32::MAX, i32::MAX, i32::MAX, i32::MIN, i32::MIN, i32::MIN)
    }
    /// PNX `new BoundingBox(x0, z0, x1, z1)`: a chunk column (y -63..512).
    pub(crate) fn chunk(cx: i32, cz: i32) -> Self {
        Self::new(cx << 4, -63, cz << 4, (cx << 4) + 15, 512, (cz << 4) + 15)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn orient(x: i32, y: i32, z: i32, xo: i32, yo: i32, zo: i32, xl: i32, yl: i32, zl: i32, f: Face) -> Self {
        match f {
            Face::North => Self::new(x + xo, y + yo, z - zl + 1 + zo, x + xl - 1 + xo, y + yl - 1 + yo, z + zo),
            Face::West => Self::new(x - zl + 1 + zo, y + yo, z + xo, x + zo, y + yl - 1 + yo, z + xl - 1 + xo),
            Face::East => Self::new(x + zo, y + yo, z + xo, x + zl - 1 + zo, y + yl - 1 + yo, z + xl - 1 + xo),
            Face::South => Self::new(x + xo, y + yo, z + zo, x + xl - 1 + xo, y + yl - 1 + yo, z + zl - 1 + zo),
        }
    }
    pub(crate) fn intersects(&self, o: &BBox) -> bool {
        self.x1 >= o.x0 && self.x0 <= o.x1 && self.z1 >= o.z0 && self.z0 <= o.z1 && self.y1 >= o.y0 && self.y0 <= o.y1
    }
    pub(crate) fn expand(&mut self, o: &BBox) {
        self.x0 = self.x0.min(o.x0);
        self.y0 = self.y0.min(o.y0);
        self.z0 = self.z0.min(o.z0);
        self.x1 = self.x1.max(o.x1);
        self.y1 = self.y1.max(o.y1);
        self.z1 = self.z1.max(o.z1);
    }
    pub(crate) fn shift(&mut self, x: i32, y: i32, z: i32) {
        self.x0 += x;
        self.y0 += y;
        self.z0 += z;
        self.x1 += x;
        self.y1 += y;
        self.z1 += z;
    }
    pub(crate) fn inside(&self, x: i32, y: i32, z: i32) -> bool {
        x >= self.x0 && x <= self.x1 && z >= self.z0 && z <= self.z1 && y >= self.y0 && y <= self.y1
    }
    pub(crate) fn x_span(&self) -> i32 {
        self.x1 - self.x0 + 1
    }
    pub(crate) fn y_span(&self) -> i32 {
        self.y1 - self.y0 + 1
    }
    pub(crate) fn z_span(&self) -> i32 {
        self.z1 - self.z0 + 1
    }
}

/// PNX `StructurePiece.findCollisionPiece`: whether any box intersects `b`.
pub(crate) fn collides<'a>(mut boxes: impl Iterator<Item = &'a BBox>, b: &BBox) -> bool {
    boxes.any(|p| p.intersects(b))
}

/// BlockManager over a StructWorld. `water_fix` is the mineshaft pass that keeps water as water.
pub(crate) struct Lvl<'a, 'b> {
    pub w: &'a mut StructWorld<'b>,
    pub water_fix: bool,
}

impl Lvl<'_, '_> {
    pub(crate) fn get(&self, x: i32, y: i32, z: i32) -> i32 {
        self.w.get(x, y, z)
    }
    pub(crate) fn set(&mut self, x: i32, y: i32, z: i32, mut id: i32) {
        if self.water_fix && bl::is_air(id) {
            let terrain = |x: i32, y: i32, z: i32| self.w.ow.terrain_chunk(x >> 4, z >> 4).get(x & 15, y, z & 15);
            if bl::is_water(terrain(x, y, z)) {
                id = bl::id("water");
            } else if bl::is_water(terrain(x, y + 1, z)) {
                let biome = self.w.biome(x, y, z);
                if let Some(s) = super::super::super::surfaces().get(&biome) {
                    id = s.floor;
                }
            }
        }
        self.w.set(x, y, z, id);
    }
    pub(crate) fn height(&self, x: i32, z: i32) -> i32 {
        self.w.height(x, z)
    }
}

pub(crate) fn is_liquid(b: i32) -> bool {
    bl::is_liquid(b)
}

/// PNX `Block.isSolid(face)` for a full face.
pub(crate) fn sturdy(b: i32) -> bool {
    bl::is_full(b)
}

/// PNX `StructurePiece` state: box, generation depth and orientation (rotation follows it).
#[derive(Clone, Debug)]
pub(crate) struct Base {
    pub bb: BBox,
    pub depth: i32,
    pub orient: Option<Face>,
}

/// Quarter turns clockwise for an orientation (PNX `Rotation`).
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

/// PNX `StructureRotationUtil.clockwise90`, for the properties these structures use.
pub(crate) fn cw90(b: i32) -> i32 {
    let mut ch: Vec<(&str, State)> = Vec::new();
    if let Some(v) = st_str(b, "torch_facing_direction") {
        // PNX turns "top" into "unknown" here, we keep standing torches standing.
        let r = match v {
            "north" => "east",
            "east" => "south",
            "south" => "west",
            "west" => "north",
            o => o,
        };
        ch.push(("torch_facing_direction", bl::s(r)));
    }
    if let Some(v) = st_int(b, "rail_direction") {
        let r = match v {
            0 => 1,
            2 => 5,
            3 => 4,
            4 => 2,
            5 => 3,
            6 => 7,
            7 => 8,
            8 => 9,
            9 => 6,
            _ => 0,
        };
        ch.push(("rail_direction", bl::i(r)));
    }
    if let Some(v) = st_int(b, "weirdo_direction") {
        ch.push(("weirdo_direction", bl::i(match v {
            0 => 2,
            1 => 3,
            2 => 1,
            _ => 0,
        })));
    }
    if let Some(v) = st_int(b, "direction") {
        ch.push(("direction", bl::i((v + 1) % 4)));
    }
    if let Some(v) = st_int(b, "facing_direction") {
        let r = match v & 7 {
            2 => 5,
            5 => 3,
            3 => 4,
            4 => 2,
            o => o,
        };
        ch.push(("facing_direction", bl::i(r | (v & !7))));
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
    if let Some(v) = st_str(b, "pillar_axis") {
        let r = match v {
            "x" => "z",
            "z" => "x",
            _ => "y",
        };
        ch.push(("pillar_axis", bl::s(r)));
    }
    if ch.is_empty() { b } else { bl::with(b, &ch) }
}

fn cw(mut b: i32, n: u8) -> i32 {
    for _ in 0..n {
        b = cw90(b);
    }
    b
}

/// PNX `placeBlock`'s rotation, with its per-block special cases.
fn rotate(b: i32, t: u8) -> i32 {
    if t == 0 {
        return b;
    }
    let n = bl::short(b);
    match n {
        "torch" => {
            let ew = matches!(st_str(b, "torch_facing_direction"), Some("east" | "west"));
            let ns = matches!(st_str(b, "torch_facing_direction"), Some("north" | "south"));
            match t {
                1 => cw(b, 1),
                2 if ns => cw(b, 2),
                3 if ew => cw(b, 1),
                3 if ns => cw(b, 3),
                _ => b,
            }
        }
        "ladder" | "oak_stairs" | "spruce_stairs" | "acacia_stairs" | "stone_stairs" | "sandstone_stairs" => {
            let v = st_int(b, if n == "ladder" { "facing_direction" } else { "weirdo_direction" }).unwrap_or(0);
            match t {
                1 => cw(b, 1),
                2 if v == 2 || v == 3 => cw(b, 1),
                3 if v == 4 || v == 5 => cw(b, 1),
                3 if v == 2 || v == 3 => cw(b, 3),
                _ => b,
            }
        }
        _ => cw(b, t),
    }
}

impl Base {
    pub(crate) fn new(bb: BBox, depth: i32, orient: Option<Face>) -> Self {
        Self { bb, depth, orient }
    }

    pub(crate) fn wx(&self, x: i32, z: i32) -> i32 {
        match self.orient {
            None => x,
            Some(Face::North | Face::South) => self.bb.x0 + x,
            Some(Face::West) => self.bb.x1 - z,
            Some(Face::East) => self.bb.x0 + z,
        }
    }
    pub(crate) fn wy(&self, y: i32) -> i32 {
        if self.orient.is_none() { y } else { y + self.bb.y0 }
    }
    pub(crate) fn wz(&self, x: i32, z: i32) -> i32 {
        match self.orient {
            None => z,
            Some(Face::North) => self.bb.z1 - z,
            Some(Face::South) => self.bb.z0 + z,
            Some(Face::West | Face::East) => self.bb.z0 + x,
        }
    }

    /// Chest facing PNX uses: the orientation's opposite (north if none), as a cardinal state.
    pub(crate) fn chest(&self) -> i32 {
        let d = self.orient.map_or("north", Face::opposite_name);
        bl::with(bl::id("chest"), &[("minecraft:cardinal_direction", bl::s(d))])
    }

    pub(crate) fn place(&self, l: &mut Lvl, b: i32, x: i32, y: i32, z: i32, cb: &BBox) {
        let (vx, vy, vz) = (self.wx(x, z), self.wy(y), self.wz(x, z));
        if cb.inside(vx, vy, vz) {
            l.set(vx, vy, vz, rotate(b, turns(self.orient)));
        }
    }

    pub(crate) fn block(&self, l: &Lvl, x: i32, y: i32, z: i32, cb: &BBox) -> i32 {
        let (vx, vy, vz) = (self.wx(x, z), self.wy(y), self.wz(x, z));
        if cb.inside(vx, vy, vz) { l.get(vx, vy, vz) } else { bl::air() }
    }

    pub(crate) fn interior(&self, l: &Lvl, x: i32, y: i32, z: i32, cb: &BBox) -> bool {
        let (vx, vy, vz) = (self.wx(x, z), self.wy(y + 1), self.wz(x, z));
        cb.inside(vx, vy, vz) && vy < l.height(vx, vz)
    }

    pub(crate) fn edges_liquid(&self, l: &Lvl, cb: &BBox) -> bool {
        let b = &self.bb;
        let (x0, y0, z0) = ((b.x0 - 1).max(cb.x0), (b.y0 - 1).max(cb.y0), (b.z0 - 1).max(cb.z0));
        let (x1, y1, z1) = ((b.x1 + 1).min(cb.x1), (b.y1 + 1).min(cb.y1), (b.z1 + 1).min(cb.z1));
        for x in x0..=x1 {
            for z in z0..=z1 {
                if is_liquid(l.get(x, y0, z)) || is_liquid(l.get(x, y1, z)) {
                    return true;
                }
            }
        }
        for x in x0..=x1 {
            for y in y0..=y1 {
                if is_liquid(l.get(x, y, z0)) || is_liquid(l.get(x, y, z1)) {
                    return true;
                }
            }
        }
        for z in z0..=z1 {
            for y in y0..=y1 {
                if is_liquid(l.get(x0, y, z)) || is_liquid(l.get(x1, y, z)) {
                    return true;
                }
            }
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn fill(&self, l: &mut Lvl, cb: &BBox, x1: i32, y1: i32, z1: i32, x2: i32, y2: i32, z2: i32, out: i32, ins: i32, skip_air: bool) {
        for y in y1..=y2 {
            for x in x1..=x2 {
                for z in z1..=z2 {
                    if skip_air {
                        let b = self.block(l, x, y, z, cb);
                        if bl::is(b, "air") || bl::is(b, "water") {
                            continue;
                        }
                    }
                    let edge = y == y1 || y == y2 || x == x1 || x == x2 || z == z1 || z == z2;
                    self.place(l, if edge { out } else { ins }, x, y, z, cb);
                }
            }
        }
    }

    /// `generateBox` with a `BlockSelector` (`sel(random, edge)`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn fill_sel(&self, l: &mut Lvl, cb: &BBox, x1: i32, y1: i32, z1: i32, x2: i32, y2: i32, z2: i32, skip_air: bool, r: &mut Xoroshiro, sel: fn(&mut Xoroshiro, bool) -> i32) {
        for y in y1..=y2 {
            for x in x1..=x2 {
                for z in z1..=z2 {
                    if !skip_air || !bl::is_air(self.block(l, x, y, z, cb)) {
                        let b = sel(r, y == y1 || y == y2 || x == x1 || x == x2 || z == z1 || z == z2);
                        self.place(l, b, x, y, z, cb);
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn maybe_fill(&self, l: &mut Lvl, cb: &BBox, r: &mut Xoroshiro, prob: i32, x1: i32, y1: i32, z1: i32, x2: i32, y2: i32, z2: i32, out: i32, ins: i32, skip_air: bool, check_interior: bool) {
        for y in y1..=y2 {
            for x in x1..=x2 {
                for z in z1..=z2 {
                    if r.next_bounded_int(100) <= prob
                        && (!skip_air || !bl::is_air(self.block(l, x, y, z, cb)))
                        && (!check_interior || self.interior(l, x, y, z, cb))
                    {
                        let edge = y == y1 || y == y2 || x == x1 || x == x2 || z == z1 || z == z2;
                        self.place(l, if edge { out } else { ins }, x, y, z, cb);
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn maybe_place(&self, l: &mut Lvl, cb: &BBox, r: &mut Xoroshiro, prob: i32, x: i32, y: i32, z: i32, b: i32) {
        if r.next_bounded_int(100) < prob {
            self.place(l, b, x, y, z, cb);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn upper_half_sphere(&self, l: &mut Lvl, cb: &BBox, x1: i32, y1: i32, z1: i32, x2: i32, y2: i32, z2: i32, b: i32) {
        let (xl, yl, zl) = ((x2 - x1 + 1) as f32, (y2 - y1 + 1) as f32, (z2 - z1 + 1) as f32);
        let (xh, zh) = (x1 as f32 + xl / 2.0, z1 as f32 + zl / 2.0);
        for y in y1..=y2 {
            let dy = (y - y1) as f32 / yl;
            for x in x1..=x2 {
                let dx = (x as f32 - xh) / (xl * 0.5);
                for z in z1..=z2 {
                    let dz = (z as f32 - zh) / (zl * 0.5);
                    if dx * dx + dy * dy + dz * dz <= 1.05 {
                        self.place(l, b, x, y, z, cb);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_and_boxes() {
        let rail = bl::with(bl::id("rail"), &[("rail_direction", bl::i(0))]);
        assert_eq!(st_int(cw90(rail), "rail_direction"), Some(1));
        let door = bl::with(bl::id("iron_door"), &[("minecraft:cardinal_direction", bl::s("east"))]);
        assert_eq!(st_str(cw(door, 2), "minecraft:cardinal_direction"), Some("west"));
        let b = BBox::orient(0, 0, 0, -1, -1, 0, 5, 5, 7, Face::North);
        assert_eq!((b.x_span(), b.y_span(), b.z_span()), (5, 5, 7));
        assert!(b.inside(0, 0, 0) && b.z1 == 0);
    }
}

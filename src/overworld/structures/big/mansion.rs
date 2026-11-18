// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Woodland mansions (PNX `WoodlandMansionPopulator`/`Pieces`) from `data/structures.nbt`.
//! No mobs, loot or spawner types, and no wall connection refresh.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::{BB, start_random};
use crate::blocks::State;
use crate::overworld::density::FxMap;
use crate::overworld::feature::blocks::{air, id, is_air, is_liquid, short};
use crate::overworld::feature::objects::{Face, World};
use crate::overworld::rng::Xoroshiro;
use crate::overworld::structures::{Placement, Spread, StructWorld, StructureType};

const PLACEMENT: Placement = Placement::new(10_387_319, 20, 80, Spread::Triangular);

pub fn register(v: &mut Vec<StructureType>) {
    // 11x11 grid of 8-block cells around the start chunk plus overhangs, so 5 chunks is enough (tested).
    v.push(StructureType { name: "normal_woodland_mansion", radius: 5, hint: |s, x, z| PLACEMENT.is_candidate(s, x, z), build });
}

const ROOFED_FOREST: u16 = 29;
const ROOFED_FOREST_MUTATED: u16 = 157;

fn build(w: &mut StructWorld, _r: &mut Xoroshiro, cx: i32, cz: i32) {
    let seed = w.seed;
    if !PLACEMENT.is_candidate(seed, cx, cz) || templates().is_empty() {
        return;
    }
    let (bx, bz) = (cx << 4, cz << 4);
    let biome = w.biome(bx + 7, w.height(bx + 7, bz + 7) - 1, bz + 7);
    if biome != ROOFED_FOREST && biome != ROOFED_FOREST_MUTATED {
        return;
    }
    let mut min_y = i32::MAX;
    for dx in 0..5 {
        for dz in 0..5 {
            min_y = min_y.min((w.height(bx + 7 + dx, bz + 7 + dz) - 1).max(64));
        }
    }
    let mut r = start_random(seed, cx, cz);
    let pieces = generate((bx + 8, min_y, bz + 8), 0, &mut r);
    place(w, &pieces);
}

// ---- geometry -----------------------------------------------------------------------------

type P = (i32, i32, i32);

fn up(p: P, n: i32) -> P {
    (p.0, p.1 + n, p.2)
}

fn cw(f: Face) -> Face {
    match f {
        Face::North => Face::East,
        Face::East => Face::South,
        Face::South => Face::West,
        Face::West => Face::North,
    }
}

fn ccw(f: Face) -> Face {
    cw(cw(cw(f)))
}

/// `rotateFace` (rotation 0..3 = NONE, 90, 180, 270).
fn rotate_face(f: Face, rot: u8) -> Face {
    (0..rot).fold(f, |f, _| cw(f))
}

fn relative(p: P, rot: u8, f: Face, d: i32) -> P {
    let f = rotate_face(f, rot);
    (p.0 + f.dx() * d, p.1, p.2 + f.dz() * d)
}

/// `StructureRotationUtil.rotateBy`.
fn rot_by(a: u8, b: u8) -> u8 {
    (a + b) & 3
}

fn rotate_offset(p: P, rot: u8) -> P {
    match rot {
        1 => (-p.2, p.1, p.0),
        2 => (-p.0, p.1, -p.2),
        3 => (p.2, p.1, -p.0),
        _ => p,
    }
}

const NONE: u8 = 0;
const LEFT_RIGHT: u8 = 1;
const FRONT_BACK: u8 = 2;

fn zero_pos_with_transform(p: P, mirror: u8, rot: u8, sx: i32, sz: i32) -> P {
    let (mx, mz) = (sx - 1, sz - 1);
    let dx = if mirror == FRONT_BACK { mx } else { 0 };
    let dz = if mirror == LEFT_RIGHT { mz } else { 0 };
    match rot {
        3 => (p.0 + dz, p.1, p.2 + mx - dx),
        1 => (p.0 + mz - dz, p.1, p.2 + dx),
        2 => (p.0 + mx - dx, p.1, p.2 + mz - dz),
        _ => (p.0 + dx, p.1, p.2 + dz),
    }
}

/// `transformLocalVanilla`.
fn transform_local(x: i32, z: i32, mirror: u8, rot: u8) -> (i32, i32) {
    let (mut tx, mut tz) = (x, z);
    match mirror {
        LEFT_RIGHT => tz = -tz,
        FRONT_BACK => tx = -tx,
        _ => {}
    }
    match rot {
        3 => (tz, -tx),
        1 => (-tz, tx),
        2 => (-tx, -tz),
        _ => (tx, tz),
    }
}

// ---- templates ----------------------------------------------------------------------------

/// A `PNXStructure`: blocks indexed x + sx * (y + sy * z), 0 = structure void, else palette + 1.
struct Template {
    sx: i32,
    sy: i32,
    sz: i32,
    palette: Vec<i32>,
    blocks: Vec<u8>,
}

fn nbt_get<'a>(v: &'a nbtx::Value, key: &str) -> Option<&'a nbtx::Value> {
    match v {
        nbtx::Value::Compound(m) => m.iter().find(|(k, _)| k.as_slice() == key.as_bytes()).map(|(_, v)| v),
        _ => None,
    }
}

fn ints(v: Option<&nbtx::Value>) -> Vec<i32> {
    match v {
        Some(nbtx::Value::List(l)) => l.to_values().into_iter().filter_map(|v| if let nbtx::Value::Int(i) = v { Some(i) } else { None }).collect(),
        Some(nbtx::Value::IntArray(a)) => a.clone(),
        _ => vec![],
    }
}

fn templates() -> &'static HashMap<String, Template> {
    static T: OnceLock<HashMap<String, Template>> = OnceLock::new();
    T.get_or_init(|| {
        // Loaded at runtime: data/structures.nbt is local-only (see .gitignore); no file, no mansions.
        let Ok(gz) = std::fs::read("data/structures.nbt") else { return HashMap::new() };
        let mut raw = flate2::read::GzDecoder::new(&gz[..]);
        let Ok(root) = nbtx::from_bytes::<nbtx::BigEndian, nbtx::Value>(&mut raw) else { return HashMap::new() };
        let Some(nbtx::Value::Compound(m)) = nbt_get(&root, "woodland_mansion") else { return HashMap::new() };
        m.iter()
            .filter_map(|(k, v)| {
                let o = nbt_get(v, "object")?;
                let size = ints(nbt_get(o, "size"));
                let Some(nbtx::Value::ByteArray(blocks)) = nbt_get(o, "blocks") else { return None };
                let t = Template { sx: size[0], sy: size[1], sz: size[2], palette: ints(nbt_get(o, "palette")).into_iter().map(resolve).collect(), blocks: blocks.clone() };
                Some((String::from_utf8_lossy(k.as_slice()).into_owned(), t))
            })
            .collect()
    })
}

/// Bedrock block-state hash (FNV-1a 32 of the little-endian `{name, states}` NBT).
fn state_hash(name: &str, states: &[(String, State)]) -> i32 {
    let mut o = vec![10u8, 0, 0];
    let s = |o: &mut Vec<u8>, v: &str| {
        o.extend((v.len() as u16).to_le_bytes());
        o.extend(v.as_bytes());
    };
    o.push(8);
    s(&mut o, "name");
    s(&mut o, name);
    o.push(10);
    s(&mut o, "states");
    for (k, v) in states {
        match v {
            State::Int(i) => {
                o.push(3);
                s(&mut o, k);
                o.extend(i.to_le_bytes());
            }
            State::Bool(b) => {
                o.push(1);
                s(&mut o, k);
                o.push(*b as u8);
            }
            State::Str(v) => {
                o.push(8);
                s(&mut o, k);
                s(&mut o, v);
            }
        }
    }
    o.extend([0, 0]);
    o.iter().fold(0x811c_9dc5u32, |h, &b| (h ^ b as u32).wrapping_mul(0x0100_0193)) as i32
}

/// Palette hash to state. Old templates lack stairs corner / connection states, so unknown hashes
/// retry without them. Still unknown means `i32::MIN` and it gets skipped.
fn resolve(h: i32) -> i32 {
    static OLD: OnceLock<HashMap<i32, i32>> = OnceLock::new();
    if crate::blocks::entry(h).is_some() {
        return h;
    }
    let old = OLD.get_or_init(|| {
        const COLORS: [&str; 16] = ["white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black"];
        let mut names: Vec<String> = ["glass_pane", "iron_bars", "redstone_wire"].map(String::from).to_vec();
        for w in ["oak", "spruce", "birch", "dark_oak", "stone", "cobblestone", "stone_brick", "mossy_cobblestone"] {
            names.push(format!("{w}_stairs"));
            names.push(format!("{w}_fence"));
        }
        names.extend(COLORS.map(|c| format!("{c}_stained_glass_pane")));
        let mut m = HashMap::new();
        for n in names {
            let Some(base) = crate::blocks::any_state(&format!("minecraft:{n}")) else { continue };
            for (d, u, s) in (0..4).flat_map(|d| [false, true].into_iter().flat_map(move |u| (0..16).map(move |s| (d, u, s)))) {
                let b = crate::blocks::with(base, &[("weirdo_direction", State::Int(d)), ("upside_down_bit", State::Bool(u)), ("redstone_signal", State::Int(s))]);
                let e = crate::blocks::entry(b).unwrap();
                let st: Vec<_> = e.states.iter().filter(|(k, _)| k != "minecraft:corner" && !k.starts_with("minecraft:connection_")).cloned().collect();
                m.entry(state_hash(&e.name, &st)).or_insert(b);
            }
        }
        m
    });
    old.get(&h).copied().unwrap_or(i32::MIN)
}

fn template(name: &str) -> &'static Template {
    templates().get(name).unwrap_or_else(|| panic!("Missing Woodland Mansion structure part: {name}"))
}

struct Piece {
    name: String,
    pos: P,
    rot: u8,
    mirror: u8,
    bb: BB,
}

impl Piece {
    fn new(name: String, pos: P, rot: u8) -> Self {
        Self::mirrored(name, pos, rot, NONE)
    }

    /// `WoodlandMansionPiece` constructor (`createBoundingBox`).
    fn mirrored(name: String, pos: P, rot: u8, mirror: u8) -> Self {
        let t = template(&name);
        let cs = [(0, 0), (t.sx - 1, 0), (0, t.sz - 1), (t.sx - 1, t.sz - 1)].map(|(x, z)| transform_local(x, z, mirror, rot));
        let (x0, x1) = (cs.iter().map(|c| c.0).min().unwrap(), cs.iter().map(|c| c.0).max().unwrap());
        let (z0, z1) = (cs.iter().map(|c| c.1).min().unwrap(), cs.iter().map(|c| c.1).max().unwrap());
        let bb = BB::new(pos.0 + x0, pos.1, pos.2 + z0, pos.0 + x1, pos.1 + t.sy - 1, pos.2 + z1);
        Self { name, pos, rot, mirror, bb }
    }
}

// ---- block state rotation / mirroring (StructureRotationUtil, mirrorState) ----------------

fn horizontal_cw(s: &str) -> Option<&'static str> {
    Some(match s {
        "north" => "east",
        "east" => "south",
        "south" => "west",
        "west" => "north",
        _ => return None,
    })
}

const LEVER: [&str; 8] = ["down_east_west", "east", "west", "south", "north", "up_north_south", "up_east_west", "down_north_south"];

/// `StructureRotationUtil.clockwise90` for the states the mansion templates use.
fn rotate_cw(b: i32) -> i32 {
    let Some(e) = crate::blocks::entry(b) else { return b };
    let get = |k: &str| e.states.iter().find(|(n, _)| n == k).map(|(_, v)| v);
    let trapdoor = e.name.ends_with("trapdoor");
    let mut ch: Vec<(&str, State)> = vec![];
    for (k, v) in &e.states {
        let nv = match (k.as_str(), v) {
            ("torch_facing_direction" | "minecraft:cardinal_direction" | "minecraft:facing_direction", State::Str(s)) => horizontal_cw(s).map(|s| State::Str(s.into())),
            ("weirdo_direction", State::Int(d)) => Some(State::Int([2, 3, 1, 0][(*d & 3) as usize])),
            // Trapdoors use the E-W-S-N order (CommonPropertyMap.EWSN_DIRECTION).
            ("direction", State::Int(d)) => Some(State::Int(if trapdoor { [2, 3, 1, 0][(*d & 3) as usize] } else { (d + 1) % 4 })),
            ("ground_sign_direction", State::Int(d)) => Some(State::Int((d + 4) % 16)),
            ("facing_direction", State::Int(d)) => Some(State::Int(match d & 7 {
                2 => 5,
                5 => 3,
                3 => 4,
                4 => 2,
                f => f,
            } | (d & !7))),
            ("lever_direction", State::Str(s)) => LEVER.iter().position(|l| l == s).map(|m| State::Str(LEVER[[7, 3, 4, 2, 1, 6, 5, 0][m]].into())),
            ("pillar_axis", State::Str(s)) => Some(State::Str(match s.as_str() {
                "x" => "z",
                "z" => "x",
                _ => "y",
            }.into())),
            ("huge_mushroom_bits", State::Int(m)) => Some(State::Int(if *m <= 10 { m * 3 % 10 } else { *m })),
            ("vine_direction_bits", State::Int(m)) => Some(State::Int(((m << 1) | (m >> 3)) & 0xf)),
            _ => None,
        };
        if let Some(nv) = nv {
            ch.push((k.as_str(), nv));
        }
    }
    if get("wall_post_bit").is_some() {
        for (to, from) in [("east", "north"), ("north", "west"), ("south", "east"), ("west", "south")] {
            if let Some(v) = get(&format!("wall_connection_type_{from}")) {
                ch.push((["wall_connection_type_east", "wall_connection_type_north", "wall_connection_type_south", "wall_connection_type_west"][["east", "north", "south", "west"].iter().position(|d| *d == to).unwrap()], v.clone()));
            }
        }
    }
    crate::blocks::with(b, &ch)
}

/// `WoodlandMansionPiece.mirrorState`.
fn mirror_state(b: i32, mirror: u8) -> i32 {
    let Some(e) = crate::blocks::entry(b) else { return b };
    let fb = mirror == FRONT_BACK;
    let flip = |s: &str| -> Option<&'static str> {
        Some(match (s, fb) {
            ("east", true) => "west",
            ("west", true) => "east",
            ("north", false) => "south",
            ("south", false) => "north",
            _ => return None,
        })
    };
    let mut ch: Vec<(&str, State)> = vec![];
    for (k, v) in &e.states {
        let nv = match (k.as_str(), v) {
            ("facing_direction", State::Int(d)) => {
                let f = d & 7;
                let m = match (f, fb) {
                    (4, true) => 5,
                    (5, true) => 4,
                    (2, false) => 3,
                    (3, false) => 2,
                    _ => f,
                };
                (m != f).then_some(State::Int(m | (d & !7)))
            }
            ("minecraft:cardinal_direction" | "torch_facing_direction", State::Str(s)) => flip(s).map(|s| State::Str(s.into())),
            ("lever_direction", State::Str(s)) => LEVER.iter().position(|l| l == s).and_then(|m| {
                let n = match (m, fb) {
                    (1, true) => 2,
                    (2, true) => 1,
                    (3, false) => 4,
                    (4, false) => 3,
                    _ => return None,
                };
                Some(State::Str(LEVER[n].into()))
            }),
            ("weirdo_direction", State::Int(d)) => match (d, fb) {
                (0, true) => Some(1),
                (1, true) => Some(0),
                (2, false) => Some(3),
                (3, false) => Some(2),
                _ => None,
            }
            .map(State::Int),
            _ => None,
        };
        if let Some(nv) = nv {
            ch.push((k.as_str(), nv));
        }
    }
    crate::blocks::with(b, &ch)
}

// ---- placement ----------------------------------------------------------------------------

/// `WoodlandMansionPieces.place`: pieces, foundation fill, marker removal, then into the world.
fn place(w: &mut StructWorld, pieces: &[Piece]) {
    let mut cache: FxMap<P, i32> = FxMap::default();
    let mut states: HashMap<(i32, u8, u8), i32> = HashMap::new();
    for p in pieces {
        let t = template(&p.name);
        for (i, &v) in t.blocks.iter().enumerate() {
            let Some(&raw) = (v as usize).checked_sub(1).and_then(|j| t.palette.get(j)).filter(|&&b| b != i32::MIN) else { continue }; // void or unknown
            let st = *states.entry((raw, p.mirror, p.rot)).or_insert_with(|| {
                let m = if p.mirror == NONE { raw } else { mirror_state(raw, p.mirror) };
                (0..p.rot).fold(m, |b, _| rotate_cw(b))
            });
            if short(st) == "structure_void" {
                continue;
            }
            let i = i as i32;
            let (x, y, z) = (i % t.sx, i / t.sx % t.sy, i / (t.sx * t.sy));
            let (tx, tz) = transform_local(x, z, p.mirror, p.rot);
            let pos = (p.pos.0 + tx, p.pos.1 + y, p.pos.2 + tz);
            if is_air(st) && cache.get(&pos).is_some_and(|&c| !is_air(c)) {
                continue;
            }
            if (-64..=319).contains(&pos.1) {
                cache.insert(pos, st);
            }
        }
    }
    foundation(w, &mut cache, pieces);
    for (pos, b) in cache {
        // Structure blocks are mob markers; PNX replaces them with air.
        w.set(pos.0, pos.1, pos.2, if short(b) == "structure_block" { air() } else { b });
    }
}

/// `afterPlaceFoundationFill`: cobblestone under the floor down to solid ground.
fn foundation(w: &StructWorld, cache: &mut FxMap<P, i32>, pieces: &[Piece]) {
    let Some(first) = pieces.first() else { return };
    let b = pieces.iter().fold(first.bb, |a, p| BB::new(a.x0.min(p.bb.x0), a.y0.min(p.bb.y0), a.z0.min(p.bb.z0), a.x1.max(p.bb.x1), a.y1.max(p.bb.y1), a.z1.max(p.bb.z1)));
    let cobble = id("cobblestone");
    let y0 = b.y0;
    for x in b.x0..=b.x1 {
        for z in b.z0..=b.z1 {
            if !pieces.iter().any(|p| p.bb.inside(x, y0, z)) {
                continue;
            }
            let at = |cache: &FxMap<P, i32>, y: i32| cache.get(&(x, y, z)).copied().unwrap_or_else(|| w.get(x, y, z));
            if is_air(at(cache, y0)) {
                continue;
            }
            let mut y = y0 - 1;
            while y > -64 {
                let id = at(cache, y);
                if !(id == air() || is_liquid(id)) {
                    break;
                }
                cache.insert((x, y, z), cobble);
                y -= 1;
            }
        }
    }
}

// ---- grid (MansionGrid / SimpleGrid) ------------------------------------------------------

const ROOM_1X1: i32 = 65536;
const ROOM_1X2: i32 = 131072;
const ROOM_2X2: i32 = 262144;
const ROOM_ORIGIN_FLAG: i32 = 1048576;
const ROOM_DOOR_FLAG: i32 = 2097152;
const ROOM_STAIRS_FLAG: i32 = 4194304;
const ROOM_CORRIDOR_FLAG: i32 = 8388608;
const ROOM_TYPE_MASK: i32 = 983040;
const ROOM_ID_MASK: i32 = 65535;

#[derive(Clone)]
struct Grid {
    g: Vec<i32>,
    w: i32,
    h: i32,
    outside: i32,
}

impl Grid {
    fn new(w: i32, h: i32, outside: i32) -> Self {
        Self { g: vec![0; (w * h) as usize], w, h, outside }
    }
    fn set(&mut self, x: i32, y: i32, v: i32) {
        if x >= 0 && x < self.w && y >= 0 && y < self.h {
            self.g[(x * self.h + y) as usize] = v;
        }
    }
    fn set_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, v: i32) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.set(x, y, v);
            }
        }
    }
    fn get(&self, x: i32, y: i32) -> i32 {
        if x >= 0 && x < self.w && y >= 0 && y < self.h { self.g[(x * self.h + y) as usize] } else { self.outside }
    }
    fn set_if(&mut self, x: i32, y: i32, if_v: i32, v: i32) {
        if self.get(x, y) == if_v {
            self.set(x, y, v);
        }
    }
    fn edges_to(&self, x: i32, y: i32, v: i32) -> bool {
        self.get(x - 1, y) == v || self.get(x + 1, y) == v || self.get(x, y + 1) == v || self.get(x, y - 1) == v
    }
}

fn is_house(g: &Grid, x: i32, y: i32) -> bool {
    matches!(g.get(x, y), 1..=4)
}

struct Mansion {
    base: Grid,
    third: Grid,
    floors: [Grid; 3],
    ex: i32,
    ey: i32,
}

impl Mansion {
    fn new(r: &mut Xoroshiro) -> Self {
        let (ex, ey) = (7, 4);
        let mut base = Grid::new(11, 11, 5);
        base.set_rect(ex, ey, ex + 1, ey + 1, 3);
        base.set_rect(ex - 1, ey, ex - 1, ey + 1, 2);
        base.set_rect(ex + 2, ey - 2, ex + 3, ey + 3, 5);
        base.set_rect(ex + 1, ey - 2, ex + 1, ey - 1, 1);
        base.set_rect(ex + 1, ey + 2, ex + 1, ey + 3, 1);
        base.set(ex - 1, ey - 1, 1);
        base.set(ex - 1, ey + 2, 1);
        base.set_rect(0, 0, 11, 1, 5);
        base.set_rect(0, 9, 11, 11, 5);
        recursive_corridor(r, &mut base, ex, ey - 2, Face::West, 6);
        recursive_corridor(r, &mut base, ex, ey + 3, Face::West, 6);
        recursive_corridor(r, &mut base, ex - 2, ey - 1, Face::West, 3);
        recursive_corridor(r, &mut base, ex - 2, ey + 2, Face::West, 3);
        while clean_edges(&mut base) {}
        let empty = Grid::new(11, 11, 5);
        let mut m = Self { third: Grid::new(base.w, base.h, 5), base, floors: [empty.clone(), empty.clone(), empty], ex, ey };
        identify_rooms(r, &m.base, &mut m.floors[0]);
        identify_rooms(r, &m.base, &mut m.floors[1]);
        m.floors[0].set_rect(ex + 1, ey, ex + 1, ey + 1, ROOM_CORRIDOR_FLAG);
        m.floors[1].set_rect(ex + 1, ey, ex + 1, ey + 1, ROOM_CORRIDOR_FLAG);
        m.setup_third_floor(r);
        let third = m.third.clone();
        identify_rooms(r, &third, &mut m.floors[2]);
        m
    }

    fn is_room_id(&self, x: i32, y: i32, floor: usize, id: i32) -> bool {
        self.floors[floor].get(x, y) & ROOM_ID_MASK == id
    }

    fn room_1x2_direction(&self, x: i32, y: i32, floor: usize, id: i32) -> Option<Face> {
        Face::PLANE.into_iter().find(|f| self.is_room_id(x + f.dx(), y + f.dz(), floor, id))
    }

    fn setup_third_floor(&mut self, r: &mut Xoroshiro) {
        let (tw, th) = (self.third.w, self.third.h);
        let mut potential = vec![];
        for y in 0..th {
            for x in 0..tw {
                let d = self.floors[1].get(x, y);
                if d & ROOM_TYPE_MASK == ROOM_1X2 && d & ROOM_DOOR_FLAG == ROOM_DOOR_FLAG {
                    potential.push((x, y));
                }
            }
        }
        if potential.is_empty() {
            self.third.set_rect(0, 0, tw, th, 5);
            return;
        }
        let (rx, ry) = potential[r.next_int_below(potential.len() as i32) as usize];
        let data = self.floors[1].get(rx, ry);
        self.floors[1].set(rx, ry, data | ROOM_STAIRS_FLAG);
        let Some(dir) = self.room_1x2_direction(rx, ry, 1, data & ROOM_ID_MASK) else {
            self.third.set_rect(0, 0, tw, th, 5);
            return;
        };
        let (ex, ey) = (rx + dir.dx(), ry + dir.dz());
        for y in 0..th {
            for x in 0..tw {
                if !is_house(&self.base, x, y) {
                    self.third.set(x, y, 5);
                } else if (x, y) == (rx, ry) || (x, y) == (ex, ey) {
                    self.third.set(x, y, 3);
                    self.floors[2].set(x, y, ROOM_CORRIDOR_FLAG);
                }
            }
        }
        let corridors: Vec<Face> = Face::PLANE.into_iter().filter(|f| self.third.get(ex + f.dx(), ey + f.dz()) == 0).collect();
        if corridors.is_empty() {
            self.third.set_rect(0, 0, tw, th, 5);
            self.floors[1].set(rx, ry, data);
            return;
        }
        let cd = corridors[r.next_int_below(corridors.len() as i32) as usize];
        recursive_corridor(r, &mut self.third, ex + cd.dx(), ey + cd.dz(), cd, 4);
        while clean_edges(&mut self.third) {}
    }
}

fn recursive_corridor(r: &mut Xoroshiro, g: &mut Grid, x: i32, y: i32, heading: Face, depth: i32) {
    if depth <= 0 {
        return;
    }
    g.set(x, y, 1);
    g.set_if(x + heading.dx(), y + heading.dz(), 0, 1);
    for _ in 0..8 {
        let next = Face::HORIZONTALS[r.next_int_below(4) as usize];
        if next != heading.opposite() && (next != Face::East || !r.next_boolean()) {
            let (nx, ny) = (x + heading.dx(), y + heading.dz());
            if g.get(nx + next.dx(), ny + next.dz()) == 0 && g.get(nx + next.dx() * 2, ny + next.dz() * 2) == 0 {
                recursive_corridor(r, g, nx + next.dx(), ny + next.dz(), next, depth - 1);
                break;
            }
        }
    }
    let (c, cc, h) = (cw(heading), ccw(heading), heading);
    g.set_if(x + c.dx(), y + c.dz(), 0, 2);
    g.set_if(x + cc.dx(), y + cc.dz(), 0, 2);
    g.set_if(x + h.dx() + c.dx(), y + h.dz() + c.dz(), 0, 2);
    g.set_if(x + h.dx() + cc.dx(), y + h.dz() + cc.dz(), 0, 2);
    g.set_if(x + h.dx() * 2, y + h.dz() * 2, 0, 2);
    g.set_if(x + c.dx() * 2, y + c.dz() * 2, 0, 2);
    g.set_if(x + cc.dx() * 2, y + cc.dz() * 2, 0, 2);
}

fn clean_edges(g: &mut Grid) -> bool {
    let mut touched = false;
    for y in 0..g.h {
        for x in 0..g.w {
            if g.get(x, y) != 0 {
                continue;
            }
            let h = |dx: i32, dy: i32| is_house(g, x + dx, y + dy) as i32;
            let direct = h(1, 0) + h(-1, 0) + h(0, 1) + h(0, -1);
            if direct >= 3 || direct == 2 && h(1, 1) + h(-1, 1) + h(1, -1) + h(-1, -1) <= 1 {
                g.set(x, y, 2);
                touched = true;
            }
        }
    }
    touched
}

fn identify_rooms(r: &mut Xoroshiro, from: &Grid, rooms: &mut Grid) {
    let mut pos = vec![];
    for y in 0..from.h {
        for x in 0..from.w {
            if from.get(x, y) == 2 {
                pos.push((x, y));
            }
        }
    }
    for i in (1..pos.len()).rev() {
        pos.swap(i, r.next_int_below(i as i32 + 1) as usize);
    }
    let mut room_id = 10;
    for (x, y) in pos {
        if rooms.get(x, y) != 0 {
            continue;
        }
        let (mut x0, mut x1, mut y0, mut y1) = (x, x, y, y);
        let free = |dx: i32, dy: i32| rooms.get(x + dx, y + dy) == 0 && from.get(x + dx, y + dy) == 2;
        let ty = if free(1, 0) && free(0, 1) && free(1, 1) {
            x1 = x + 1;
            y1 = y + 1;
            ROOM_2X2
        } else if free(-1, 0) && free(0, 1) && free(-1, 1) {
            x0 = x - 1;
            y1 = y + 1;
            ROOM_2X2
        } else if free(-1, 0) && free(0, -1) && free(-1, -1) {
            x0 = x - 1;
            y0 = y - 1;
            ROOM_2X2
        } else if free(1, 0) {
            x1 = x + 1;
            ROOM_1X2
        } else if free(0, 1) {
            y1 = y + 1;
            ROOM_1X2
        } else if free(-1, 0) {
            x0 = x - 1;
            ROOM_1X2
        } else if free(0, -1) {
            y0 = y - 1;
            ROOM_1X2
        } else {
            ROOM_1X1
        };
        let mut dx = if r.next_boolean() { x0 } else { x1 };
        let mut dy = if r.next_boolean() { y0 } else { y1 };
        let mut door = ROOM_DOOR_FLAG;
        let flip = |v: i32, a: i32, b: i32| if v == a { b } else { a };
        if !from.edges_to(dx, dy, 1) {
            dx = flip(dx, x0, x1);
            dy = flip(dy, y0, y1);
            if !from.edges_to(dx, dy, 1) {
                dy = flip(dy, y0, y1);
                if !from.edges_to(dx, dy, 1) {
                    dx = flip(dx, x0, x1);
                    dy = flip(dy, y0, y1);
                    if !from.edges_to(dx, dy, 1) {
                        door = 0;
                        dx = x0;
                        dy = y0;
                    }
                }
            }
        }
        for ry in y0..=y1 {
            for rx in x0..=x1 {
                rooms.set(rx, ry, if (rx, ry) == (dx, dy) { ROOM_ORIGIN_FLAG | door | ty | room_id } else { ty | room_id });
            }
        }
        room_id += 1;
    }
}

// ---- room names (FloorRoomCollection) -----------------------------------------------------

fn n(r: &mut Xoroshiro, k: i32) -> i32 {
    r.next_int_below(k) + 1
}

fn get_1x1(r: &mut Xoroshiro, floor: usize) -> String {
    if floor == 0 { format!("1x1_a{}", n(r, 5)) } else { format!("1x1_b{}", n(r, 5)) }
}
fn get_1x1_secret(r: &mut Xoroshiro) -> String {
    format!("1x1_as{}", n(r, 4))
}
fn get_1x2_side(r: &mut Xoroshiro, floor: usize, stairs: bool) -> String {
    match (floor, stairs) {
        (0, _) => format!("1x2_a{}", n(r, 9)),
        (_, true) => "1x2_c_stairs".into(),
        _ => format!("1x2_c{}", n(r, 4)),
    }
}
fn get_1x2_front(r: &mut Xoroshiro, floor: usize, stairs: bool) -> String {
    match (floor, stairs) {
        (0, _) => format!("1x2_b{}", n(r, 5)),
        (_, true) => "1x2_d_stairs".into(),
        _ => format!("1x2_d{}", n(r, 5)),
    }
}
fn get_1x2_secret(r: &mut Xoroshiro, floor: usize) -> String {
    if floor == 0 { format!("1x2_s{}", n(r, 2)) } else { "1x2_se1".into() }
}
fn get_2x2(r: &mut Xoroshiro, floor: usize) -> String {
    if floor == 0 { format!("2x2_a{}", n(r, 4)) } else { format!("2x2_b{}", n(r, 5)) }
}

// ---- MansionPiecePlacer -------------------------------------------------------------------

struct Data {
    pos: P,
    rot: u8,
    wall: &'static str,
}

#[derive(Clone, Copy, PartialEq)]
enum Door {
    F(Face),
    Up,
}

struct Placer<'a> {
    r: &'a mut Xoroshiro,
    pieces: Vec<Piece>,
    sx: i32,
    sy: i32,
}

fn generate(origin: P, rot: u8, r: &mut Xoroshiro) -> Vec<Piece> {
    let m = Mansion::new(r);
    let mut p = Placer { r, pieces: vec![], sx: m.ex + 1, sy: m.ey + 1 };
    p.create(origin, rot, &m);
    p.pieces
}

impl Placer<'_> {
    fn add(&mut self, name: &str, pos: P, rot: u8) {
        self.pieces.push(Piece::new(name.into(), pos, rot));
    }

    fn cell(&self, o: P, rot: u8, x: i32, y: i32, east: i32) -> P {
        relative(relative(o, rot, Face::South, 8 + (y - self.sy) * 8), rot, Face::East, east + (x - self.sx) * 8)
    }

    fn create(&mut self, origin: P, rot: u8, m: &Mansion) {
        let mut data = Data { pos: origin, rot, wall: "wall_flat" };
        // entrance
        let west = rotate_face(Face::West, data.rot);
        self.add("entrance", (data.pos.0 + west.dx() * 9, data.pos.1, data.pos.2 + west.dz() * 9), data.rot);
        data.pos = relative(data.pos, data.rot, Face::South, 16);
        let mut second = Data { pos: up(data.pos, 8), rot: data.rot, wall: "wall_window" };
        let (sx, sy) = (self.sx, self.sy);
        let (end_x, end_y) = (m.ex + 1, m.ey);
        self.traverse_outer_walls(&mut data, &m.base, Face::South, sx, sy, end_x, end_y);
        self.traverse_outer_walls(&mut second, &m.base, Face::South, sx, sy, end_x, end_y);
        let mut third = Data { pos: up(data.pos, 19), rot: data.rot, wall: "wall_window" };
        'done: for y in 0..m.third.h {
            for x in (0..m.third.w).rev() {
                if is_house(&m.third, x, y) {
                    third.pos = relative(third.pos, rot, Face::South, 8 + (y - sy) * 8);
                    third.pos = relative(third.pos, rot, Face::East, (x - sx) * 8);
                    self.traverse_wall_piece(&mut third);
                    self.traverse_outer_walls(&mut third, &m.third, Face::South, x, y, x, y);
                    break 'done;
                }
            }
        }
        self.create_roof(up(origin, 16), rot, &m.base, Some(&m.third));
        self.create_roof(up(origin, 27), rot, &m.third, None);

        for floor in 0..3usize {
            let fo = up(origin, 8 * floor as i32 + if floor == 2 { 3 } else { 0 });
            let rooms = &m.floors[floor];
            let grid = if floor == 2 { &m.third } else { &m.base };
            let south_piece = if floor == 0 { "carpet_south_1" } else { "carpet_south_2" };
            let west_piece = if floor == 0 { "carpet_west_1" } else { "carpet_west_2" };
            let corr = |x: i32, y: i32| grid.get(x, y) == 1 || rooms.get(x, y) & ROOM_CORRIDOR_FLAG == ROOM_CORRIDOR_FLAG || floor == 2 && grid.get(x, y) == 3;
            for y in 0..grid.h {
                for x in 0..grid.w {
                    let third_start = floor == 2 && grid.get(x, y) == 3 && rooms.get(x, y) & ROOM_CORRIDOR_FLAG == ROOM_CORRIDOR_FLAG;
                    if grid.get(x, y) == 1 {
                        let pos = self.cell(fo, rot, x, y, 0);
                        self.add("corridor_floor", pos, rot);
                        if corr(x, y - 1) {
                            self.add("carpet_north", up(relative(pos, rot, Face::East, 1), 1), rot);
                        }
                        if corr(x + 1, y) {
                            self.add("carpet_east", up(relative(relative(pos, rot, Face::South, 1), rot, Face::East, 5), 1), rot);
                        }
                        if corr(x, y + 1) {
                            self.add(south_piece, relative(relative(pos, rot, Face::South, 5), rot, Face::West, 1), rot);
                        }
                        if corr(x - 1, y) {
                            self.add(west_piece, relative(relative(pos, rot, Face::West, 1), rot, Face::North, 1), rot);
                        }
                    } else if third_start {
                        let pos = self.cell(fo, rot, x, y, 0);
                        if grid.get(x, y + 1) == 1 {
                            let p = relative(relative(pos, rot, Face::South, 5), rot, Face::West, 1);
                            let p = template_rotation_offset(south_piece, p, rot);
                            let p = relative(relative(p, rot, Face::West, 2), rot, Face::South, 1);
                            self.add(south_piece, p, rot_by(rot, 2));
                        }
                        if grid.get(x - 1, y) == 1 {
                            let p = relative(relative(pos, rot, Face::West, 1), rot, Face::North, 1);
                            let p = template_rotation_offset(west_piece, p, rot);
                            let p = relative(relative(p, rot, Face::West, 2), rot, Face::South, 1);
                            self.add(west_piece, p, rot_by(rot, 2));
                        }
                    }
                }
            }

            let wall_piece = if floor == 0 { "indoors_wall_1" } else { "indoors_wall_2" };
            let door_piece = if floor == 0 { "indoors_door_1" } else { "indoors_door_2" };
            for y in 0..grid.h {
                for x in 0..grid.w {
                    let mut third_start = floor == 2 && grid.get(x, y) == 3;
                    if !(grid.get(x, y) == 2 || third_start) {
                        continue;
                    }
                    let data = rooms.get(x, y);
                    let ty = data & ROOM_TYPE_MASK;
                    let id = data & ROOM_ID_MASK;
                    third_start = third_start && data & ROOM_CORRIDOR_FLAG == ROOM_CORRIDOR_FLAG;
                    let doors: Vec<Face> = if data & ROOM_DOOR_FLAG == ROOM_DOOR_FLAG {
                        Face::PLANE.into_iter().filter(|f| grid.get(x + f.dx(), y + f.dz()) == 1).collect()
                    } else {
                        vec![]
                    };
                    let door = if !doors.is_empty() {
                        Some(Door::F(doors[self.r.next_int_below(doors.len() as i32) as usize]))
                    } else if data & ROOM_ORIGIN_FLAG == ROOM_ORIGIN_FLAG {
                        Some(Door::Up)
                    } else {
                        None
                    };
                    let pick = |f: Face| if door == Some(Door::F(f)) { door_piece } else { wall_piece };
                    let room_pos = self.cell(fo, rot, x, y, -1);
                    if is_house(grid, x - 1, y) && !m.is_room_id(x - 1, y, floor, id) {
                        self.add(pick(Face::West), room_pos, rot);
                    }
                    if grid.get(x + 1, y) == 1 && !third_start {
                        self.add(pick(Face::East), relative(room_pos, rot, Face::East, 8), rot);
                    }
                    if is_house(grid, x, y + 1) && !m.is_room_id(x, y + 1, floor, id) {
                        self.add(pick(Face::South), relative(relative(room_pos, rot, Face::South, 7), rot, Face::East, 7), rot_by(rot, 1));
                    }
                    if grid.get(x, y - 1) == 1 && !third_start {
                        self.add(pick(Face::North), relative(relative(room_pos, rot, Face::North, 1), rot, Face::East, 7), rot_by(rot, 1));
                    }
                    match (ty, door) {
                        (ROOM_1X1, _) => self.room_1x1(room_pos, rot, door, floor),
                        (ROOM_1X2, Some(d)) => {
                            let dir = m.room_1x2_direction(x, y, floor, id);
                            self.room_1x2(room_pos, rot, dir, d, floor, data & ROOM_STAIRS_FLAG == ROOM_STAIRS_FLAG);
                        }
                        (ROOM_2X2, Some(Door::F(d))) => {
                            let mut dir = cw(d);
                            if !m.is_room_id(x + dir.dx(), y + dir.dz(), floor, id) {
                                dir = dir.opposite();
                            }
                            self.room_2x2(room_pos, rot, dir, d, floor);
                        }
                        (ROOM_2X2, Some(Door::Up)) => {
                            let p = relative(room_pos, rot, Face::East, 1);
                            self.pieces.push(Piece::mirrored("2x2_s1".into(), p, rot, NONE));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn traverse_outer_walls(&mut self, data: &mut Data, g: &Grid, mut dir: Face, sx: i32, sy: i32, ex: i32, ey: i32) {
        let (mut gx, mut gy) = (sx, sy);
        let start = dir;
        loop {
            if !is_house(g, gx + dir.dx(), gy + dir.dz()) {
                self.traverse_turn(data);
                dir = cw(dir);
                if gx != ex || gy != ey || start != dir {
                    self.traverse_wall_piece(data);
                }
            } else if is_house(g, gx + dir.dx() + ccw(dir).dx(), gy + dir.dz() + ccw(dir).dz()) {
                self.traverse_inner_turn(data);
                gx += dir.dx();
                gy += dir.dz();
                dir = ccw(dir);
            } else {
                gx += dir.dx();
                gy += dir.dz();
                if gx != ex || gy != ey || start != dir {
                    self.traverse_wall_piece(data);
                }
            }
            if !(gx != ex || gy != ey || start != dir) {
                break;
            }
        }
    }

    fn traverse_wall_piece(&mut self, d: &mut Data) {
        self.add(d.wall, relative(d.pos, d.rot, Face::East, 7), d.rot);
        d.pos = relative(d.pos, d.rot, Face::South, 8);
    }

    fn traverse_turn(&mut self, d: &mut Data) {
        d.pos = relative(d.pos, d.rot, Face::South, -1);
        self.add("wall_corner", d.pos, d.rot);
        d.pos = relative(d.pos, d.rot, Face::South, -7);
        d.pos = relative(d.pos, d.rot, Face::West, -6);
        d.rot = rot_by(d.rot, 1);
    }

    fn traverse_inner_turn(&mut self, d: &mut Data) {
        d.pos = relative(d.pos, d.rot, Face::South, 6);
        d.pos = relative(d.pos, d.rot, Face::East, 8);
        d.rot = rot_by(d.rot, 3);
    }

    fn create_roof(&mut self, o: P, rot: u8, g: &Grid, above: Option<&Grid>) {
        let h = |x: i32, y: i32| is_house(g, x, y);
        let over = |x: i32, y: i32| above.is_some_and(|a| is_house(a, x, y));
        let rel = |p: P, f: Face, d: i32| relative(p, rot, f, d);
        for y in 0..g.h {
            for x in 0..g.w {
                let pos = self.cell(o, rot, x, y, 0);
                if h(x, y) && !over(x, y) {
                    self.add("roof", up(pos, 3), rot);
                    if !h(x + 1, y) {
                        self.add("roof_front", rel(pos, Face::East, 6), rot);
                    }
                    if !h(x - 1, y) {
                        self.add("roof_front", rel(rel(pos, Face::East, 0), Face::South, 7), rot_by(rot, 2));
                    }
                    if !h(x, y - 1) {
                        self.add("roof_front", rel(pos, Face::West, 1), rot_by(rot, 3));
                    }
                    if !h(x, y + 1) {
                        self.add("roof_front", rel(rel(pos, Face::East, 6), Face::South, 6), rot_by(rot, 1));
                    }
                }
            }
        }
        if above.is_some() {
            for y in 0..g.h {
                for x in 0..g.w {
                    let pos = self.cell(o, rot, x, y, 0);
                    if !(h(x, y) && over(x, y)) {
                        continue;
                    }
                    if !h(x + 1, y) {
                        self.add("small_wall", rel(pos, Face::East, 7), rot);
                    }
                    if !h(x - 1, y) {
                        self.add("small_wall", rel(rel(pos, Face::West, 1), Face::South, 6), rot_by(rot, 2));
                    }
                    if !h(x, y - 1) {
                        self.add("small_wall", rel(rel(pos, Face::West, 0), Face::North, 1), rot_by(rot, 3));
                    }
                    if !h(x, y + 1) {
                        self.add("small_wall", rel(rel(pos, Face::East, 6), Face::South, 7), rot_by(rot, 1));
                    }
                    if !h(x + 1, y) {
                        if !h(x, y - 1) {
                            self.add("small_wall_corner", rel(rel(pos, Face::East, 7), Face::North, 2), rot);
                        }
                        if !h(x, y + 1) {
                            self.add("small_wall_corner", rel(rel(pos, Face::East, 8), Face::South, 7), rot_by(rot, 1));
                        }
                    }
                    if !h(x - 1, y) {
                        if !h(x, y - 1) {
                            self.add("small_wall_corner", rel(rel(pos, Face::West, 2), Face::North, 1), rot_by(rot, 3));
                        }
                        if !h(x, y + 1) {
                            self.add("small_wall_corner", rel(rel(pos, Face::West, 1), Face::South, 8), rot_by(rot, 2));
                        }
                    }
                }
            }
        }
        for y in 0..g.h {
            for x in 0..g.w {
                let pos = self.cell(o, rot, x, y, 0);
                if !(h(x, y) && !over(x, y)) {
                    continue;
                }
                if !h(x + 1, y) {
                    let p2 = rel(pos, Face::East, 6);
                    if !h(x, y + 1) {
                        self.add("roof_corner", rel(p2, Face::South, 6), rot);
                    } else if h(x + 1, y + 1) {
                        self.add("roof_inner_corner", rel(p2, Face::South, 5), rot);
                    }
                    if !h(x, y - 1) {
                        self.add("roof_corner", p2, rot_by(rot, 3));
                    } else if h(x + 1, y - 1) {
                        self.add("roof_inner_corner", rel(rel(pos, Face::East, 9), Face::North, 2), rot_by(rot, 1));
                    }
                }
                if !h(x - 1, y) {
                    let p2 = rel(rel(pos, Face::East, 0), Face::South, 0);
                    if !h(x, y + 1) {
                        self.add("roof_corner", rel(p2, Face::South, 6), rot_by(rot, 1));
                    } else if h(x - 1, y + 1) {
                        self.add("roof_inner_corner", rel(rel(p2, Face::South, 8), Face::West, 3), rot_by(rot, 3));
                    }
                    if !h(x, y - 1) {
                        self.add("roof_corner", p2, rot_by(rot, 2));
                    } else if h(x - 1, y - 1) {
                        self.add("roof_inner_corner", rel(p2, Face::South, 1), rot_by(rot, 2));
                    }
                }
            }
        }
    }

    fn room_1x1(&mut self, room_pos: P, rot: u8, door: Option<Door>, floor: usize) {
        let mut piece_rot = 0;
        let mut name = get_1x1(self.r, floor);
        if door != Some(Door::F(Face::East)) {
            match door {
                Some(Door::F(Face::North)) => piece_rot = rot_by(piece_rot, 3),
                Some(Door::F(Face::West)) => piece_rot = rot_by(piece_rot, 2),
                Some(Door::F(Face::South)) => piece_rot = rot_by(piece_rot, 1),
                _ => name = get_1x1_secret(self.r),
            }
        }
        let o = zero_pos_with_transform((1, 0, 0), NONE, piece_rot, 7, 7);
        let piece_rot = rot_by(piece_rot, rot);
        let o = rotate_offset(o, rot);
        self.pieces.push(Piece::new(name, (room_pos.0 + o.0, room_pos.1, room_pos.2 + o.2), piece_rot));
    }

    fn room_1x2(&mut self, room_pos: P, rot: u8, dir: Option<Face>, door: Door, floor: usize, stairs: bool) {
        use Face::*;
        let Some(dir) = dir else { return };
        let rel = |p: P, f: Face, d: i32| relative(p, rot, f, d);
        let (side, front) = (0, 1);
        let (kind, pos, prot, mirror) = match (door, dir) {
            (Door::F(East), South) => (side, rel(room_pos, East, 1), rot, NONE),
            (Door::F(East), North) => (side, rel(rel(room_pos, East, 1), South, 6), rot, LEFT_RIGHT),
            (Door::F(West), North) => (side, rel(rel(room_pos, East, 7), South, 6), rot_by(rot, 2), NONE),
            (Door::F(West), South) => (side, rel(room_pos, East, 7), rot, FRONT_BACK),
            (Door::F(South), East) => (side, rel(room_pos, East, 1), rot_by(rot, 1), LEFT_RIGHT),
            (Door::F(South), West) => (side, rel(room_pos, East, 7), rot_by(rot, 1), NONE),
            (Door::F(North), West) => (side, rel(rel(room_pos, East, 7), South, 6), rot_by(rot, 1), FRONT_BACK),
            (Door::F(North), East) => (side, rel(rel(room_pos, East, 1), South, 6), rot_by(rot, 3), NONE),
            (Door::F(South), North) => (front, rel(rel(room_pos, East, 1), North, 8), rot, NONE),
            (Door::F(North), South) => (front, rel(rel(room_pos, East, 7), South, 14), rot_by(rot, 2), NONE),
            (Door::F(West), East) => (front, rel(room_pos, East, 15), rot_by(rot, 1), NONE),
            (Door::F(East), West) => (front, rel(rel(room_pos, West, 7), South, 6), rot_by(rot, 3), NONE),
            (Door::Up, East) => (2, rel(room_pos, East, 15), rot_by(rot, 1), NONE),
            (Door::Up, South) => (2, rel(rel(room_pos, East, 1), North, 0), rot, NONE),
            _ => return,
        };
        let name = match kind {
            0 => get_1x2_side(self.r, floor, stairs),
            1 => get_1x2_front(self.r, floor, stairs),
            _ => get_1x2_secret(self.r, floor),
        };
        self.pieces.push(Piece::mirrored(name, pos, prot, mirror));
    }

    fn room_2x2(&mut self, room_pos: P, rot: u8, dir: Face, door: Face, floor: usize) {
        use Face::*;
        let (east, south, r2, mirror) = match (door, dir) {
            (East, South) => (-7, 0, rot, NONE),
            (East, North) => (-7, 6, rot, LEFT_RIGHT),
            (North, East) => (1, 14, rot_by(rot, 3), NONE),
            (North, West) => (7, 14, rot_by(rot, 3), LEFT_RIGHT),
            (South, West) => (7, -8, rot_by(rot, 1), NONE),
            (South, East) => (1, -8, rot_by(rot, 1), LEFT_RIGHT),
            (West, North) => (15, 6, rot_by(rot, 2), NONE),
            (West, South) => (15, 0, rot, FRONT_BACK),
            _ => (0, 0, rot, NONE),
        };
        let pos = relative(relative(room_pos, rot, East, east), rot, South, south);
        let name = get_2x2(self.r, floor);
        self.pieces.push(Piece::mirrored(name, pos, r2, mirror));
    }
}

/// `applyTemplateRotationOffset`.
fn template_rotation_offset(name: &str, pos: P, rot: u8) -> P {
    let t = template(name);
    let o = zero_pos_with_transform((0, 0, 0), NONE, 2, t.sx, t.sz);
    relative(relative(pos, rot, Face::East, o.0), rot, Face::South, o.2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overworld::Overworld;

    #[test]
    fn templates_resolve() {
        let t = templates();
        assert!(t.len() > 70);
        let known = |h: i32| crate::blocks::entry(h).is_some();
        assert!(known(state_hash("minecraft:dark_oak_planks", &[])) && state_hash("minecraft:dark_oak_planks", &[]) == id("dark_oak_planks"));
        let unknown = t.values().flat_map(|t| t.palette.iter()).filter(|&&h| !known(h)).count();
        assert!(unknown <= 4, "{unknown} palette hashes missing from the block registry");
        assert!(t.values().any(|t| t.palette.iter().any(|&h| h != i32::MIN && short(h) == "dark_oak_planks")));
    }

    #[test]
    fn rotation_round_trips() {
        let stairs = crate::blocks::with(id("dark_oak_stairs"), &[("weirdo_direction", State::Int(1))]);
        assert_ne!(rotate_cw(stairs), stairs);
        assert_eq!((0..4).fold(stairs, |b, _| rotate_cw(b)), stairs);
        assert_eq!(mirror_state(mirror_state(stairs, FRONT_BACK), FRONT_BACK), stairs);
    }

    #[test]
    fn layout_stays_within_radius() {
        for s in 0..20 {
            let mut r = start_random(s, 0, 0);
            let pieces = generate((8, 70, 8), 0, &mut r);
            assert!(pieces.len() > 100);
            for p in &pieces {
                assert!(p.bb.x0 >> 4 >= -5 && p.bb.x1 >> 4 <= 5 && p.bb.z0 >> 4 >= -5 && p.bb.z1 >> 4 <= 5, "{} {:?}", p.name, p.bb);
            }
        }
    }

    #[test]
    fn mansion_generates() {
        let planks = id("dark_oak_planks");
        for seed in [1u64, 2, 3, 4, 5, 6, 7, 8] {
            let ow = Overworld::new(seed);
            for cx in -120..120 {
                for cz in -120..120 {
                    if !PLACEMENT.is_candidate(seed as i64, cx, cz) {
                        continue;
                    }
                    let t = ow.generate_terrain(cx, cz);
                    let b = t.biome(7, t.heights[7 * 16 + 7] - 1, 7);
                    if b != ROOFED_FOREST && b != ROOFED_FOREST_MUTATED {
                        continue;
                    }
                    let ch = ow.generate(cx, cz);
                    let n = (0..16).flat_map(|x| (0..16).flat_map(move |z| (60..140).map(move |y| (x, y, z)))).filter(|&(x, y, z)| ch.get(x, y, z) == planks).count();
                    assert!(n > 20, "mansion at chunk {cx},{cz} seed {seed}: {n} dark oak planks");
                    return;
                }
            }
        }
        panic!("no mansion start found");
    }
}

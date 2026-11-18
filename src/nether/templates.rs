// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
// from PNX StructureRegistry, PNXStructure, StructureRotationUtil, BlockManager
// (copied from overworld structures/templates.rs)
//! Structure templates for bastions and nether fossils, loaded once from data/structures.nbt
//! (only the `bastion/` and `nether_fossils/` entries are kept).

#![allow(dead_code)] // copied loader API, not every part is used here

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

use crate::blocks::State;
use crate::overworld::feature::blocks as bl;
use crate::overworld::feature::objects::World;

/// Bedrock block-state hash: FNV-1a 32 of the little-endian NBT `{name, states}`.
fn state_hash(name: &str, states: &[(String, State)], skip: Option<usize>) -> i32 {
    fn s(o: &mut Vec<u8>, v: &str) {
        o.extend_from_slice(&(v.len() as u16).to_le_bytes());
        o.extend_from_slice(v.as_bytes());
    }
    let mut o = vec![10, 0, 0, 8];
    s(&mut o, "name");
    s(&mut o, name);
    o.push(10);
    s(&mut o, "states");
    for (i, (k, v)) in states.iter().enumerate() {
        if Some(i) == skip {
            continue;
        }
        match v {
            State::Int(n) => {
                o.push(3);
                s(&mut o, k);
                o.extend_from_slice(&n.to_le_bytes());
            }
            State::Bool(b) => {
                o.push(1);
                s(&mut o, k);
                o.push(*b as u8);
            }
            State::Str(t) => {
                o.push(8);
                s(&mut o, k);
                s(&mut o, t);
            }
        }
    }
    o.extend_from_slice(&[0, 0]);
    o.iter().fold(0x811c_9dc5u32, |h, &c| (h ^ c as u32).wrapping_mul(0x0100_0193)) as i32
}

fn nbt_get<'a>(v: &'a nbtx::Value, key: &str) -> Option<&'a nbtx::Value> {
    match v {
        nbtx::Value::Compound(m) => m.iter().find(|(k, _)| k.as_slice() == key.as_bytes()).map(|(_, v)| v),
        _ => None,
    }
}

/// Maps old-version hashes in `want` to current ids by hashing each state with one property dropped.
fn legacy_map(want: &HashSet<i32>) -> HashMap<i32, i32> {
    let mut out: HashMap<i32, (i32, bool)> = HashMap::new();
    if want.is_empty() {
        return HashMap::new();
    }
    let gz = include_bytes!("../../data/block_palette.nbt");
    let Ok(root) = nbtx::from_bytes::<nbtx::BigEndian, nbtx::Value>(&mut flate2::read::GzDecoder::new(&gz[..])) else { return HashMap::new() };
    let Some(nbtx::Value::List(list)) = nbt_get(&root, "blocks") else { return HashMap::new() };
    for b in list.to_values() {
        let Some(nbtx::Value::Int(nid)) = nbt_get(&b, "network_id") else { continue };
        let Some(e) = crate::blocks::entry(*nid) else { continue };
        for i in 0..e.states.len() {
            let h = state_hash(&e.name, &e.states, Some(i));
            if !want.contains(&h) {
                continue;
            }
            let default = matches!(&e.states[i].1, State::Int(0) | State::Bool(false)) || e.states[i].1 == State::Str("none".into());
            match out.get(&h) {
                Some(&(_, true)) => {}
                Some(_) if !default => {}
                _ => {
                    out.insert(h, (*nid, default));
                }
            }
        }
    }
    out.into_iter().map(|(k, (v, _))| (k, v)).collect()
}

static UNKNOWN: AtomicUsize = AtomicUsize::new(0);

/// Palette states (summed over templates) that did not resolve to a Rusted block.
pub fn unknown_states() -> usize {
    UNKNOWN.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------------------------
// Templates

/// PNX `PNXStructure.Jigsaw`. `final_state` is air when its hash did not resolve.
#[derive(Clone, Debug)]
pub struct Jigsaw {
    pub pos: [i32; 3],
    pub final_state: i32,
    pub name: String,
    pub joint: String,
    pub pool: String,
    pub target: String,
    pub placement_priority: i32,
    pub selection_priority: i32,
}

/// Rotation of the template geometry (PNX `Rotation`): `R90` maps (x, z) to (z, sx-1-x).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rotation {
    None,
    R90,
    R180,
    R270,
}

/// PNX `Mirror` (geometry only, states unchanged, as in PNX).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mirror {
    None,
    X,
    Z,
    XZ,
}

impl Rotation {
    /// Clockwise quarter turns applied to block states (inverse of the geometry, as PNX).
    fn state_turns(self) -> u32 {
        match self {
            Rotation::None => 0,
            Rotation::R90 => 3,
            Rotation::R180 => 2,
            Rotation::R270 => 1,
        }
    }
}

pub struct Template {
    pub name: String,
    /// (x, y, z).
    pub size: [i32; 3],
    /// Rusted ids; `None` = structure void or unresolved state (never placed).
    pub palette: Vec<Option<i32>>,
    /// Palette index + 1 per block, x fastest then y then z; 0 = structure void.
    blocks: Vec<u8>,
    pub jigsaws: Vec<Jigsaw>,
}

impl Template {
    /// Block at template coordinates (None = void / unknown / out of range).
    pub fn get(&self, x: i32, y: i32, z: i32) -> Option<i32> {
        let [sx, sy, sz] = self.size;
        if !(0..sx).contains(&x) || !(0..sy).contains(&y) || !(0..sz).contains(&z) {
            return None;
        }
        let p = self.blocks[(x + y * sx + z * sx * sy) as usize];
        if p == 0 { None } else { self.palette.get(p as usize - 1).copied().flatten() }
    }

    /// (size x, size z) after rotation.
    pub fn rotated_size(&self, r: Rotation) -> (i32, i32) {
        match r {
            Rotation::R90 | Rotation::R270 => (self.size[2], self.size[0]),
            _ => (self.size[0], self.size[2]),
        }
    }

    /// PNX `getRotatedX/Z`.
    fn rotate_xz(&self, x: i32, z: i32, r: Rotation) -> (i32, i32) {
        let [sx, _, sz] = self.size;
        match r {
            Rotation::None => (x, z),
            Rotation::R90 => (z, sx - 1 - x),
            Rotation::R180 => (sx - 1 - x, sz - 1 - z),
            Rotation::R270 => (sz - 1 - z, x),
        }
    }

    fn mirror_xz(&self, x: i32, z: i32, m: Mirror) -> (i32, i32) {
        let [sx, _, sz] = self.size;
        match m {
            Mirror::None => (x, z),
            Mirror::X => (sx - 1 - x, z),
            Mirror::Z => (x, sz - 1 - z),
            Mirror::XZ => (sx - 1 - x, sz - 1 - z),
        }
    }

    /// PNX `getJigsaws(rotation)`, optionally mirrored. Positions relative to the rotated origin.
    pub fn jigsaws_rotated(&self, r: Rotation, m: Mirror) -> Vec<Jigsaw> {
        self.jigsaws
            .iter()
            .map(|j| {
                let (mx, mz) = self.mirror_xz(j.pos[0], j.pos[2], m);
                let (x, z) = self.rotate_xz(mx, mz, r);
                Jigsaw { pos: [x, j.pos[1], z], final_state: rotate_state(j.final_state, r.state_turns()), ..j.clone() }
            })
            .collect()
    }

    /// Placeable blocks relative to origin, in PNX `preparePlace` order (mirror, then rotate).
    /// Air included since PNX places it.
    pub fn for_each(&self, r: Rotation, m: Mirror, mut f: impl FnMut(i32, i32, i32, i32)) {
        let turns = r.state_turns();
        let pal: Vec<Option<i32>> = if turns == 0 { self.palette.clone() } else { self.palette.iter().map(|p| p.map(|v| rotate_state(v, turns))).collect() };
        let [sx, sy, sz] = self.size;
        let mut i = 0;
        for z in 0..sz {
            for y in 0..sy {
                for x in 0..sx {
                    let p = self.blocks.get(i).copied().unwrap_or(0);
                    i += 1;
                    let Some(Some(v)) = (p as usize).checked_sub(1).and_then(|k| pal.get(k)) else { continue };
                    let (mx, mz) = self.mirror_xz(x, z, m);
                    let (rx, rz) = self.rotate_xz(mx, mz, r);
                    f(rx, y, rz, *v);
                }
            }
        }
    }

    /// Stages the template at `o` (PNX `preparePlace(pos, manager)` without rotation).
    pub fn stage(&self, b: &mut Blocks, o: (i32, i32, i32)) {
        self.for_each(Rotation::None, Mirror::None, |x, y, z, v| b.set(o.0 + x, o.1 + y, o.2 + z, v));
    }

    /// Writes the template into a world at `o` (PNX `place`).
    pub fn place<W: World + ?Sized>(&self, w: &mut W, o: (i32, i32, i32), r: Rotation, m: Mirror) {
        self.for_each(r, m, |x, y, z, v| w.set(o.0 + x, o.1 + y, o.2 + z, v));
    }
}

fn resolve(h: i32, legacy: &HashMap<i32, i32>) -> Option<i32> {
    let v = if crate::blocks::entry(h).is_some() { Some(h) } else { legacy.get(&h).copied() };
    if v.is_none() {
        UNKNOWN.fetch_add(1, Ordering::Relaxed);
    }
    v.filter(|&v| !bl::is(v, "structure_void"))
}

fn str_of(v: &nbtx::Value, k: &str) -> String {
    match nbt_get(v, k) {
        Some(nbtx::Value::String(s)) => String::from_utf8_lossy(s).into_owned(),
        _ => String::new(),
    }
}

fn int_of(v: &nbtx::Value, k: &str) -> i32 {
    if let Some(nbtx::Value::Int(i)) = nbt_get(v, k) { *i } else { 0 }
}

fn load(path: &str) -> HashMap<String, Template> {
    let mut out = HashMap::new();
    let Ok(file) = std::fs::File::open(path) else {
        println!("structures: {path} missing; template structures disabled");
        return out;
    };
    let root = match nbtx::from_bytes::<nbtx::BigEndian, nbtx::Value>(&mut flate2::read::GzDecoder::new(std::io::BufReader::new(file))) {
        Ok(r) => r,
        Err(e) => {
            println!("structures: cannot read {path}: {e:?}");
            return out;
        }
    };
    // PNX `registerFromTag`: nested compounds, an `object` child is a template.
    let mut raw: Vec<(String, &nbtx::Value)> = vec![];
    fn walk<'a>(prefix: &str, v: &'a nbtx::Value, raw: &mut Vec<(String, &'a nbtx::Value)>) {
        let nbtx::Value::Compound(m) = v else { return };
        for (k, c) in m.iter() {
            let k = String::from_utf8_lossy(k);
            if k == "object" {
                if !prefix.is_empty() {
                    raw.push((prefix.to_string(), c));
                }
                continue;
            }
            let name = if prefix.is_empty() { k.into_owned() } else { format!("{prefix}/{k}") };
            walk(&name, c, raw);
        }
    }
    walk("", &root, &mut raw);
    raw.retain(|(n, _)| n.starts_with("bastion/") || n.starts_with("nether_fossils/") || n.starts_with("ruined_portal/"));
    let palette_of = |o: &nbtx::Value| -> Vec<i32> {
        match nbt_get(o, "palette") {
            Some(nbtx::Value::List(nbtx::ValueList::Int(v))) => v.clone(),
            _ => vec![],
        }
    };
    let mut want = HashSet::new();
    for (_, o) in &raw {
        want.extend(palette_of(o).into_iter().filter(|&h| crate::blocks::entry(h).is_none()));
        if let Some(nbtx::Value::List(l)) = nbt_get(o, "jigsaw") {
            want.extend(l.to_values().iter().map(|j| int_of(j, "final_state")).filter(|&h| crate::blocks::entry(h).is_none()));
        }
    }
    let legacy = legacy_map(&want);
    for (name, o) in raw {
        let size = match nbt_get(o, "size") {
            Some(nbtx::Value::IntArray(v)) => [0, 1, 2].map(|i| v.get(i).copied().unwrap_or(0)),
            _ => [0; 3],
        };
        let blocks = match nbt_get(o, "blocks") {
            Some(nbtx::Value::ByteArray(v)) => v.clone(),
            _ => vec![],
        };
        let palette = palette_of(o).into_iter().map(|h| resolve(h, &legacy)).collect();
        let jigsaws = match nbt_get(o, "jigsaw") {
            Some(nbtx::Value::List(l)) => l
                .to_values()
                .iter()
                .map(|j| {
                    let pos = match nbt_get(j, "pos") {
                        Some(nbtx::Value::IntArray(v)) => [0, 1, 2].map(|i| v.get(i).copied().unwrap_or(0)),
                        _ => [0; 3],
                    };
                    let fs = int_of(j, "final_state");
                    let final_state = if crate::blocks::entry(fs).is_some() { fs } else { legacy.get(&fs).copied().unwrap_or(bl::air()) };
                    Jigsaw {
                        pos,
                        final_state,
                        name: str_of(j, "name"),
                        joint: str_of(j, "joint"),
                        pool: str_of(j, "pool"),
                        target: str_of(j, "target"),
                        placement_priority: int_of(j, "placement_priority"),
                        selection_priority: int_of(j, "selection_priority"),
                    }
                })
                .collect(),
            _ => vec![],
        };
        out.insert(name.clone(), Template { name, size, palette, blocks, jigsaws });
    }
    out
}

fn registry() -> &'static HashMap<String, Template> {
    static R: OnceLock<HashMap<String, Template>> = OnceLock::new();
    R.get_or_init(|| load("data/structures.nbt"))
}

/// A template by PNX name (e.g. "shipwreck/with_mast"); None if absent or no data file.
pub fn get(name: &str) -> Option<&'static Template> {
    registry().get(name)
}

// ---------------------------------------------------------------------------------------------
// Block-state rotation (PNX `StructureRotationUtil`)

fn cw_face(f: &str) -> Option<&'static str> {
    Some(match f {
        "north" => "east",
        "east" => "south",
        "south" => "west",
        "west" => "north",
        _ => return None,
    })
}

/// Bedrock `facing_direction` index (0 down, 1 up, 2 n, 3 s, 4 w, 5 e) turned clockwise.
fn cw_facing(v: i32, ccw: bool) -> i32 {
    let cw = |f| match f {
        2 => 5,
        5 => 3,
        3 => 4,
        4 => 2,
        f => f,
    };
    let f = v & 7;
    let r = if ccw { cw(cw(cw(f))) } else { cw(f) };
    (v & !7) | r
}

/// PNX `clockwise90` applied `turns` times (lever directions are not rotated).
pub fn rotate_state(id: i32, turns: u32) -> i32 {
    let mut v = id;
    for _ in 0..turns % 4 {
        let Some(e) = crate::blocks::entry(v) else { return v };
        let n = e.name.trim_start_matches("minecraft:");
        let get = |k: &str| e.states.iter().find(|(a, _)| a == k).map(|(_, s)| s.clone());
        let mut ch: Vec<(&str, State)> = vec![];
        for (k, s) in &e.states {
            let new = match (k.as_str(), s) {
                ("torch_facing_direction" | "minecraft:cardinal_direction", State::Str(f)) => cw_face(f).map(|f| State::Str(f.into())),
                ("minecraft:facing_direction", State::Str(f)) => cw_face(f).map(|f| State::Str(f.into())),
                ("weirdo_direction", State::Int(d)) => Some(State::Int([2, 3, 1, 0][(*d & 3) as usize])),
                ("direction", State::Int(d)) => Some(State::Int(if n.ends_with("trapdoor") { [2, 3, 1, 0][(*d & 3) as usize] } else { (d + 1) % 4 })),
                ("ground_sign_direction", State::Int(d)) => Some(State::Int((d + 4) % 16)),
                ("facing_direction", State::Int(d)) => Some(State::Int(cw_facing(*d, n == "jigsaw"))),
                ("rotation", State::Int(d)) if n == "jigsaw" => Some(State::Int(((d & 3) + 1) % 4)),
                ("rail_direction", State::Int(d)) => {
                    let m10 = [1, 0, 5, 4, 2, 3, 7, 8, 9, 6];
                    let m6 = [1, 0, 5, 4, 2, 3];
                    let m: &[i32] = if n == "rail" { &m10 } else { &m6 };
                    Some(State::Int(m.get(*d as usize).copied().unwrap_or(0)))
                }
                ("pillar_axis", State::Str(a)) => Some(State::Str(match a.as_str() { "x" => "z", "z" => "x", _ => "y" }.into())),
                ("huge_mushroom_bits", State::Int(m)) => Some(State::Int(if *m <= 10 { (m * 3) % 10 } else { *m })),
                ("vine_direction_bits", State::Int(m)) => Some(State::Int(((m << 1) | (m >> 3)) & 0xf)),
                ("wall_connection_type_east", _) => get("wall_connection_type_north"),
                ("wall_connection_type_north", _) => get("wall_connection_type_west"),
                ("wall_connection_type_south", _) => get("wall_connection_type_east"),
                ("wall_connection_type_west", _) => get("wall_connection_type_south"),
                _ => None,
            };
            if let Some(new) = new {
                ch.push((k.as_str(), new));
            }
        }
        v = crate::blocks::with(v, &ch);
    }
    v
}

// ---------------------------------------------------------------------------------------------
// Staging (PNX `BlockManager`)

/// Ordered staged blocks: later `set`s override, `unset` removes, iteration is first-set order.
#[derive(Default)]
pub struct Blocks {
    order: Vec<(i32, i32, i32)>,
    map: HashMap<(i32, i32, i32), i32>,
}

impl Blocks {
    pub fn set(&mut self, x: i32, y: i32, z: i32, v: i32) {
        if self.map.insert((x, y, z), v).is_none() {
            self.order.push((x, y, z));
        }
    }
    pub fn unset(&mut self, x: i32, y: i32, z: i32) {
        self.map.remove(&(x, y, z));
    }
    pub fn get(&self, x: i32, y: i32, z: i32) -> Option<i32> {
        self.map.get(&(x, y, z)).copied()
    }
    pub fn contains(&self, x: i32, y: i32, z: i32) -> bool {
        self.map.contains_key(&(x, y, z))
    }
    /// Snapshot of the staged blocks in order.
    pub fn list(&self) -> Vec<(i32, i32, i32, i32)> {
        let mut seen = HashSet::new();
        self.order.iter().filter(|p| seen.insert(**p)).filter_map(|&(x, y, z)| self.get(x, y, z).map(|v| (x, y, z, v))).collect()
    }
    pub fn merge(&mut self, o: &Blocks) {
        for (x, y, z, v) in o.list() {
            self.set(x, y, z, v);
        }
    }
    /// Min and max corner of the staged blocks.
    pub fn bounds(&self) -> Option<((i32, i32, i32), (i32, i32, i32))> {
        let l = self.list();
        let first = l.first()?;
        let mut lo = (first.0, first.1, first.2);
        let mut hi = lo;
        for &(x, y, z, _) in &l {
            lo = (lo.0.min(x), lo.1.min(y), lo.2.min(z));
            hi = (hi.0.max(x), hi.1.max(y), hi.2.max(z));
        }
        Some((lo, hi))
    }
    pub fn write<W: World + ?Sized>(&self, w: &mut W) {
        for (x, y, z, v) in self.list() {
            w.set(x, y, z, v);
        }
    }
}


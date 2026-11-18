// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Template structures (shipwrecks, igloos, ocean ruins, outposts, fossils, ruined portals) and
//! the template loader (PNX `StructureRegistry` + `PNXStructure`) reading `data/structures.nbt`.
//! Decay is rolled in placement order, not PNX's hash order, so it differs a bit in detail.

#![allow(dead_code)] // loader API is used by the jigsaw port

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

use crate::blocks::State;

use super::super::aquifer::SEA_LEVEL;
use super::super::feature::blocks::{self as bl, id};
use super::super::feature::objects::World;
use super::super::feature::{chunk_hash, random_range};
use super::{NukkitRandom, Placement, Spread, StructWorld, StructureType, Xoroshiro};

const MIN_Y: i32 = -64;
const MAX_Y: i32 = 320;

// ---------------------------------------------------------------------------------------------
// Block-state hashes

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

/// Old-version hashes -> current ids, for the hashes in `want`: every current state hashed with
/// one property dropped (preferring a dropped default-looking value).
fn legacy_map(want: &HashSet<i32>) -> HashMap<i32, i32> {
    let mut out: HashMap<i32, (i32, bool)> = HashMap::new();
    if want.is_empty() {
        return HashMap::new();
    }
    let gz = include_bytes!("../../../data/block_palette.nbt");
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
pub(crate) fn unknown_states() -> usize {
    UNKNOWN.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------------------------
// Templates

/// PNX `PNXStructure.Jigsaw`. `final_state` is air when its hash did not resolve.
#[derive(Clone, Debug)]
pub(crate) struct Jigsaw {
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
pub(crate) enum Rotation {
    None,
    R90,
    R180,
    R270,
}

/// PNX `Mirror` (geometry only, states unchanged, as in PNX).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mirror {
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

pub(crate) struct Template {
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

    /// PNX `getJigsaws(rotation)` after an optional mirror (positions relative to the rotated
    /// template origin; final states rotated like blocks).
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

    /// Every placeable block as (x, y, z, id) relative to the origin, in PNX `preparePlace`
    /// order, mirrored first, then rotated. Air is included (PNX places it).
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
pub(crate) fn get(name: &str) -> Option<&'static Template> {
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
pub(crate) fn rotate_state(id: i32, turns: u32) -> i32 {
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
pub(crate) struct Blocks {
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

// ---------------------------------------------------------------------------------------------
// Beardifier (PNX `Beardifier`, surface processor only)

const KR: i32 = 12;

fn kernel() -> &'static [f64] {
    static K: OnceLock<Vec<f64>> = OnceLock::new();
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

/// `Beardifier.apply` with a box-floor surface. `staged` is the helper, terrain comes from `w`.
pub(crate) fn beard_surface(w: &StructWorld, staged: &mut Blocks, lo: (i32, i32, i32), hi: (i32, i32, i32), top: i32, mid: i32, threshold: f64) {
    let ground = lo.1;
    let (min_y, max_y) = (MIN_Y.max(ground - KR), (MAX_Y - 1).min(ground + KR));
    for x in lo.0 - KR..=hi.0 + KR {
        for z in lo.2 - KR..=hi.2 + KR {
            let (dx, dz) = (dist_to_range(x, lo.0, hi.0), dist_to_range(z, lo.2, hi.2));
            if dx >= KR || dz >= KR {
                continue;
            }
            let mut highest = None;
            for y in min_y..=max_y {
                if staged.contains(x, y, z) {
                    continue;
                }
                let dy = (y - ground).abs();
                if dy >= KR || contribution(dx, dy, dz, y - ground) * 0.8 <= threshold {
                    continue;
                }
                let cur = w.get(x, y, z);
                if bl::replaceable(cur) || !bl::is_solid(cur) {
                    staged.set(x, y, z, mid);
                    highest = Some(y);
                }
            }
            if let Some(y) = highest {
                staged.set(x, y, z, top);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Shared helpers

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

/// PNX `isReplaceableSurface` (snow layers below full height count as replaceable).
fn surface_replaceable(b: i32) -> bool {
    if bl::is(b, "snow_layer") {
        return !matches!(crate::blocks::state(b, "height"), Some(State::Int(7)));
    }
    bl::replaceable(b)
}

/// `chunk.getHeightMap(lx, lz)` for chunk (cx, cz).
fn hm(w: &StructWorld, cx: i32, cz: i32, lx: i32, lz: i32) -> i32 {
    w.height((cx << 4) + lx, (cz << 4) + lz)
}

/// `chunk.getBiomeId(lx, y, lz)` with y = heightmap + dy.
fn biome_at(w: &StructWorld, cx: i32, cz: i32, l: i32, dy: i32) -> u16 {
    let (x, z) = ((cx << 4) + l, (cz << 4) + l);
    w.biome(x, w.height(x, z) + dy, z)
}

fn tpl(name: &str) -> Option<&'static Template> {
    get(name)
}

pub fn register(v: &mut Vec<StructureType>) {
    v.push(StructureType { name: "normal_shipwreck", radius: 2, hint: |s, x, z| SHIPWRECK.is_candidate(s, x, z), build: shipwreck });
    v.push(StructureType { name: "normal_igloo", radius: 1, hint: |s, x, z| IGLOO.is_candidate(s, x, z), build: igloo });
    v.push(StructureType { name: "normal_ocean_ruin", radius: 1, hint: |s, x, z| OCEAN_RUIN.is_candidate(s, x, z), build: ocean_ruin });
    v.push(StructureType { name: "normal_pillager_outpost", radius: 1, hint: |s, x, z| OUTPOST.is_candidate(s, x, z), build: outpost });
    v.push(StructureType { name: "normal_fossil", radius: 1, hint: fossil_hint, build: fossil });
    v.push(StructureType { name: "generic_ruined_portal", radius: 1, hint: |s, x, z| PORTAL.is_candidate(s, x, z), build: ruined_portal });
}

// ---------------------------------------------------------------------------------------------
// Shipwreck (PNX `ShipwreckPopulator`)

const SHIPWRECK: Placement = Placement::new(165_745_295, 4, 24, Spread::Linear);

const BEACHED: [&str; 11] = [
    "with_mast", "sideways_full", "sideways_fronthalf", "sideways_backhalf", "rightsideup_full", "rightsideup_fronthalf", "rightsideup_backhalf",
    "with_mast_degraded", "rightsideup_full_degraded", "rightsideup_fronthalf_degraded", "rightsideup_backhalf_degraded",
];
const OCEAN: [&str; 20] = [
    "with_mast", "upsidedown_full", "upsidedown_fronthalf", "upsidedown_backhalf", "sideways_full", "sideways_fronthalf", "sideways_backhalf",
    "rightsideup_full", "rightsideup_fronthalf", "rightsideup_backhalf", "with_mast_degraded", "upsidedown_full_degraded",
    "upsidedown_fronthalf_degraded", "upsidedown_backhalf_degraded", "sideways_full_degraded", "sideways_fronthalf_degraded",
    "sideways_backhalf_degraded", "rightsideup_full_degraded", "rightsideup_fronthalf_degraded", "rightsideup_backhalf_degraded",
];

/// (stripped log, planks, stairs, slab, fence, trapdoor, door) per `ShipwreckWood`.
fn wood(w: &str) -> [String; 7] {
    let (trap, door) = match w {
        "oak" => ("trapdoor".to_string(), "wooden_door".to_string()),
        _ => (format!("{w}_trapdoor"), format!("{w}_door")),
    };
    [format!("stripped_{w}_log"), format!("{w}_planks"), format!("{w}_stairs"), format!("{w}_slab"), format!("{w}_fence"), trap, door]
}

const WOOD_PALETTES: [(&str, &str); 15] = [
    ("oak", "spruce"), ("oak", "birch"), ("oak", "dark_oak"), ("spruce", "oak"), ("spruce", "jungle"), ("spruce", "dark_oak"), ("birch", "oak"),
    ("birch", "spruce"), ("jungle", "oak"), ("jungle", "spruce"), ("jungle", "acacia"), ("spruce", "acacia"), ("dark_oak", "spruce"),
    ("dark_oak", "jungle"), ("dark_oak", "acacia"),
];

/// PNX `applyWood`: same states, mapped block name.
fn apply_wood(v: i32, map: &HashMap<&'static str, String>) -> i32 {
    let Some(target) = map.get(bl::short(v)) else { return v };
    let Some(base) = crate::blocks::any_state(&format!("minecraft:{target}")) else { return v };
    let e = crate::blocks::entry(v).unwrap();
    let st: Vec<(&str, State)> = e.states.iter().map(|(k, s)| (k.as_str(), s.clone())).collect();
    crate::blocks::with(base, &st)
}

fn shipwreck(w: &mut StructWorld, r: &mut Xoroshiro, cx: i32, cz: i32) {
    r.set_seed(w.seed ^ chunk_hash(cx, cz));
    let biome = biome_at(w, cx, cz, 5, 0);
    if !SHIPWRECK.is_candidate(w.seed, cx, cz) || !(tagged(biome, "ocean") || tagged(biome, "beach")) {
        return;
    }
    let name = if tagged(biome, "beach") { BEACHED[r.next_int_below(BEACHED.len() as i32) as usize] } else { OCEAN[r.next_int_below(OCEAN.len() as i32) as usize] };
    let (p, s) = WOOD_PALETTES[r.next_int_below(15) as usize];
    let Some(t) = tpl(&format!("shipwreck/{name}")) else { return };
    let (pw, sw) = (wood(p), wood(s));
    let mut map: HashMap<&'static str, String> = HashMap::new();
    for (k, v) in ["oak_log", "oak_planks", "oak_stairs", "oak_slab", "oak_fence", "trapdoor", "wooden_door"].into_iter().zip(pw) {
        map.insert(k, v);
    }
    for (k, v) in ["spruce_log", "spruce_planks", "spruce_stairs", "spruce_slab", "spruce_fence"].into_iter().zip(sw) {
        map.insert(k, v);
    }
    let [sx, _, sz] = t.size;
    let (mut sum, mut n) = (0, 0);
    for x in 0..sx.min(16) {
        for z in 0..sz.min(16) {
            let mut y = hm(w, cx, cz, x, z);
            let (wx, wz) = ((cx << 4) + x, (cz << 4) + z);
            while surface_replaceable(w.get(wx, y, wz)) && y > MIN_Y {
                y -= 1;
            }
            sum += y;
            n += 1;
        }
    }
    let y = sum / n.max(1);
    let o = (cx << 4, y, cz << 4);
    t.for_each(Rotation::None, Mirror::None, |x, dy, z, v| {
        let v = apply_wood(v, &map);
        if !bl::is_air(v) && !bl::is(v, "structure_block") {
            w.set(o.0 + x, o.1 + dy, o.2 + z, v);
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Igloo (PNX `IglooPopulator`)

const IGLOO: Placement = Placement::new(14_357_617, 8, 32, Spread::Linear);

fn igloo(w: &mut StructWorld, r: &mut Xoroshiro, cx: i32, cz: i32) {
    r.set_seed(w.seed ^ chunk_hash(cx, cz));
    let biome = biome_at(w, cx, cz, 3, 0);
    if !IGLOO.is_candidate(w.seed, cx, cz) || !matches!(biome, 12 | 30 | 184) {
        return;
    }
    let (Some(top), Some(middle), Some(bottom)) = (tpl("igloo/top"), tpl("igloo/middle"), tpl("igloo/bottom")) else { return };
    let lab = r.next_boolean();
    let (mut sum, mut n) = (0, 0);
    for x in 0..top.size[0] {
        for z in 2..top.size[2] + 2 {
            let mut y = hm(w, cx, cz, x, z);
            let (wx, wz) = ((cx << 4) + x, (cz << 4) + z);
            while surface_replaceable(w.get(wx, y, wz)) && y > 64 {
                y -= 1;
            }
            sum += y.max(64);
            n += 1;
        }
    }
    let sy = sum / n - 1;
    let mut b = Blocks::default();
    let mut v = ((cx << 4), sy + 1, (cz << 4) + 2);
    top.stage(&mut b, v);
    if lab {
        v.0 += 2;
        v.2 += 4;
        let mut i = 0;
        while i < r.next_bounded_int(8) + 3 {
            v.1 -= middle.size[1];
            middle.stage(&mut b, v);
            i += 1;
        }
        v.0 -= 2;
        v.2 -= 6;
        v.1 -= bottom.size[1];
        bottom.stage(&mut b, v);
    } else {
        b.set(v.0 + 3, v.1, v.2 + 5, id("snow"));
    }
    let packed = id("packed_ice");
    for (x, y, z, id) in b.list() {
        if bl::is(id, "structure_block") {
            b.set(x, y, z, bl::air());
        }
        if bl::is(id, "ice") {
            b.set(x, y, z, packed);
        }
    }
    b.write(w);
}

// ---------------------------------------------------------------------------------------------
// Ocean ruins (PNX `OceanRuinPopulator`)

const OCEAN_RUIN: Placement = Placement::new(14_357_621, 8, 20, Spread::Linear);
const ADJACENT: [(i32, i32); 8] = [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)];

fn ruin(kind: &str, i: i32) -> Option<&'static Template> {
    match kind {
        "warm" => tpl(&format!("underwater_ruin/warm_{}", i + 1)),
        "big_warm" => tpl(&format!("underwater_ruin/big_warm_{}", i + 4)),
        k if k.starts_with("big_") => tpl(&format!("underwater_ruin/{k}_{}", [1, 2, 3, 8][i as usize])),
        k => tpl(&format!("underwater_ruin/{k}_{}", i + 1)),
    }
}

fn place_ruin(w: &StructWorld, t: Option<&Template>, cx: i32, cz: i32, seed: i32, large: bool, index: i32, out: &mut Blocks) {
    let Some(t) = t else { return };
    let mut r = NukkitRandom::new(seed as i64);
    let [sx, _, sz] = t.size;
    let x = r.next_bounded_int(16 - sx);
    let z = r.next_bounded_int(16 - sz);
    let mut y = 256;
    for lx in x..x + sx {
        for lz in z..z + sz {
            let mut h = hm(w, cx, cz, lx, lz);
            while surface_replaceable(w.get((cx << 4) + lx, h, (cz << 4) + lz)) && h > 1 {
                h -= 1;
            }
            y = y.min(h);
        }
    }
    let o = ((cx << 4) + x, y, (cz << 4) + z);
    let mut m1 = Blocks::default();
    t.stage(&mut m1, o);
    for (x, y, z, _) in m1.list() {
        let ran = r.next_range(0, 9);
        if ran == 0 || (ran == 1 && !large) {
            m1.unset(x, y, z);
        }
    }
    out.merge(&m1);
    if index != -1 {
        let (mossy, cracked) = if large { (ruin("big_mossy", index), ruin("big_cracked", index)) } else { (ruin("mossy", index), ruin("cracked", index)) };
        let mut m2 = Blocks::default();
        if let Some(t) = mossy {
            t.stage(&mut m2, o);
        }
        for (x, y, z, _) in m2.list() {
            if r.next_range(0, 9) < 3 {
                m2.unset(x, y, z);
            }
        }
        out.merge(&m2);
        let mut m3 = Blocks::default();
        if let Some(t) = cracked {
            t.stage(&mut m3, o);
        }
        for (x, y, z, _) in m3.list() {
            if r.next_boolean() {
                m3.unset(x, y, z);
            }
        }
        out.merge(&m3);
    }
}

fn ocean_ruin(w: &mut StructWorld, r: &mut Xoroshiro, cx: i32, cz: i32) {
    let biome = biome_at(w, cx, cz, 7, 0);
    if !OCEAN_RUIN.is_candidate(w.seed, cx, cz) || !tagged(biome, "ocean") {
        return;
    }
    r.set_seed(w.seed ^ chunk_hash(cx, cz));
    let warm = tagged(biome, "warm");
    let large = r.next_bounded_int(100) <= 30;
    let (t, index) = if warm {
        (if large { ruin("big_warm", r.next_bounded_int(3)) } else { ruin("warm", r.next_bounded_int(7)) }, -1)
    } else if large {
        let i = r.next_bounded_int(3);
        (ruin("big_brick", i), i)
    } else {
        let i = r.next_bounded_int(7);
        (ruin("brick", i), i)
    };
    let mut b = Blocks::default();
    let seed = r.next_int();
    place_ruin(w, t, cx, cz, seed, large, index, &mut b);
    if large && r.next_bounded_int(100) <= 90 {
        let mut adj = ADJACENT.to_vec();
        let mut i = 0;
        while i < r.next_int_range(4, 8) {
            let (dx, dz) = adj.remove(r.next_bounded_int(adj.len() as i32 - 1) as usize);
            let (t, index) = if warm {
                (ruin("warm", r.next_bounded_int(7)), -1)
            } else {
                let index = r.next_bounded_int(7);
                (ruin("brick", r.next_bounded_int(7)), index)
            };
            let seed = r.next_int();
            place_ruin(w, t, cx + dx, cz + dz, seed, false, index, &mut b);
            i += 1;
        }
    }
    let chest = id("chest");
    for (x, y, z, v) in b.list() {
        if bl::is_air(v) {
            b.unset(x, y, z);
        } else if bl::is(v, "structure_block") {
            b.set(x, y, z, chest);
        }
    }
    b.write(w);
}

// ---------------------------------------------------------------------------------------------
// Fossils (PNX `FossilPopulator`)

fn fossil_hint(seed: i64, cx: i32, cz: i32) -> bool {
    Xoroshiro::new(seed ^ chunk_hash(cx, cz)).next_int_below(64) == 0
}

fn fossil(w: &mut StructWorld, r: &mut Xoroshiro, cx: i32, cz: i32) {
    r.set_seed(w.seed ^ chunk_hash(cx, cz));
    let biome = biome_at(w, cx, cz, 3, -1);
    if r.next_int_below(64) != 0 || !(tagged(biome, "desert") || tagged(biome, "swamp") || tagged(biome, "mangrove_swamp")) {
        return;
    }
    let (x0, z0) = (cx << 4, cz << 4);
    let mut y = 64.min(w.height(x0, z0) - 1);
    while bl::is(w.get(x0, y, z0), "water") && y > MIN_Y {
        y -= 1;
    }
    let base_y = 10.max(y - 15 - r.next_bounded_int(10));
    let kind = if r.next_boolean() { "skull" } else { "spine" };
    let Some(t) = tpl(&format!("fossil/{kind}_{}", r.next_int_below(4) + 1)) else { return };
    let mut b = Blocks::default();
    t.stage(&mut b, (x0, base_y, z0));
    let Some((lo, hi)) = b.bounds() else { return };
    let mut empty = 0;
    for x in [lo.0, hi.0] {
        for yy in [lo.1, hi.1] {
            for z in [lo.2, hi.2] {
                let v = w.get(x, yy, z);
                empty += (bl::is(v, "air") || bl::is(v, "water") || bl::is(v, "lava")) as i32;
            }
        }
    }
    if empty > 4 {
        return;
    }
    let ore = if y < 0 { id("deepslate_diamond_ore") } else { id("coal_ore") };
    for (x, yy, z, _) in b.list() {
        match r.next_int_below(10) {
            0 => b.unset(x, yy, z),
            1 => b.set(x, yy, z, ore),
            _ => {}
        }
    }
    b.write(w);
}

// ---------------------------------------------------------------------------------------------
// Pillager outposts (PNX `PillagerOutpostPopulator`)

const OUTPOST: Placement = Placement::new(165_745_296, 24, 80, Spread::Triangular);
const FEATURES: [&str; 7] = ["feature_cage1", "feature_cage2", "feature_logs", "feature_tent1", "feature_tent2", "feature_targets", "feature_cage_with_allays"];

/// PNX `fillBase`: reads the terrain at `base_y`, extends wooden/cobble bases downward.
fn fill_base(w: &mut StructWorld, base_y: i32, x0: i32, z0: i32, sx: i32, sz: i32) {
    for x in x0..x0 + sx {
        for z in z0..z0 + sz {
            let b = w.get(x, base_y, z);
            if !matches!(bl::short(b), "cobblestone" | "mossy_cobblestone" | "oak_log" | "dark_oak_log" | "oak_planks" | "dark_oak_planks" | "oak_fence" | "dark_oak_fence") {
                continue;
            }
            let fill = id(bl::short(b));
            let mut y = base_y - 1;
            while surface_replaceable(w.get(x, y, z)) && y > 1 {
                w.set(x, y, z, fill);
                y -= 1;
            }
        }
    }
}

fn outpost(w: &mut StructWorld, r: &mut Xoroshiro, cx: i32, cz: i32) {
    let biome = biome_at(w, cx, cz, 7, -1);
    if !OUTPOST.is_candidate(w.seed, cx, cz) || !matches!(biome, 1 | 2 | 35 | 5 | 12 | 30 | 129 | 186 | 185 | 184 | 182 | 183 | 189 | 192) {
        return;
    }
    let (Some(tower), Some(overgrown)) = (tpl("pillager_outpost/watchtower"), tpl("pillager_outpost/watchtower_overgrown")) else { return };
    r.set_seed(w.seed ^ chunk_hash(cx, cz));
    let (x0, z0) = (cx << 4, cz << 4);
    let mut y = w.height(x0, z0);
    while surface_replaceable(w.get(x0, y, z0)) && y > 1 {
        y -= 1;
    }
    let o = (x0, y + 1, z0);
    let mut helper = Blocks::default();
    tower.stage(&mut helper, o);
    let mut h2 = Blocks::default();
    overgrown.stage(&mut h2, o);
    for (x, yy, z, _) in h2.list() {
        if r.next_int_below(20) != 0 {
            h2.unset(x, yy, z);
        }
    }
    helper.merge(&h2);
    let [sx, sy, sz] = tower.size;
    beard_surface(w, &mut helper, o, (o.0 + sx - 1, o.1 + sy - 1, o.2 + sz - 1), id("grass_block"), id("dirt"), 0.03);
    fill_base(w, y, x0, z0, sx, sz);
    r.set_seed(w.seed ^ chunk_hash(cx, cz) ^ 0x5DEE_CE66D);
    for (dx, dz) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
        if !r.next_boolean() {
            continue;
        }
        let name = FEATURES[r.next_int_below(7) as usize];
        let seed = r.next_int();
        let Some(t) = tpl(&format!("pillager_outpost/{name}")) else { continue };
        let mut nr = NukkitRandom::new(seed as i64);
        let (fx, fz) = (nr.next_bounded_int(16 - t.size[0]), nr.next_bounded_int(16 - t.size[2]));
        let (bx, bz) = (((cx + dx) << 4) + fx, ((cz + dz) << 4) + fz);
        let by = w.height(bx, bz) - 1;
        t.stage(&mut helper, (bx, by, bz));
        fill_base(w, by, bx, bz, t.size[0], t.size[2]);
    }
    for (x, yy, z, v) in helper.list() {
        if bl::is_air(v) || bl::is(v, "jigsaw") {
            helper.unset(x, yy, z);
        }
    }
    helper.write(w);
}

// ---------------------------------------------------------------------------------------------
// Ruined portals (PNX `PopulatorRuinedPortal`, overworld)

const PORTAL: Placement = Placement::new(40_552_231, 15, 40, Spread::Linear);

#[derive(PartialEq, Clone, Copy)]
enum PortalHeight {
    Land,
    PartlyBuried,
    OceanFloor,
    Mountain,
    Underground,
}

fn opaque(b: i32) -> bool {
    bl::is_full(b) && !bl::is(b, "glass") && !bl::short(b).contains("glass")
}

fn ruined_portal(w: &mut StructWorld, r: &mut Xoroshiro, cx: i32, cz: i32) {
    if !PORTAL.is_candidate(w.seed, cx, cz) {
        return;
    }
    let biome = w.chunk_biome(cx, cz);
    r.set_seed(w.seed ^ chunk_hash(cx, cz));
    let (x, z) = ((cx << 4) + 7, (cz << 4) + 7);
    let height = if tagged(biome, "desert") {
        PortalHeight::PartlyBuried
    } else if tagged(biome, "jungle") || tagged(biome, "swamp") {
        PortalHeight::Land
    } else if tagged(biome, "mountains") {
        if r.next_boolean() { PortalHeight::Land } else { PortalHeight::Mountain }
    } else if tagged(biome, "ocean") {
        PortalHeight::OceanFloor
    } else if r.next_boolean() {
        PortalHeight::Land
    } else {
        PortalHeight::Underground
    };
    let big = r.next_bounded_int(20) == 0;
    let name = if big { format!("ruined_portal/giant_portal_{}", r.next_int_below(3) + 1) } else { format!("ruined_portal/portal_{}", r.next_int_below(10) + 1) };
    let Some(t) = tpl(&name) else { return };
    let [sx, sy, sz] = t.size;
    // findSuitableY
    let min_y = MIN_Y + 15;
    let (ccx, ccz) = (x + (sx >> 1), z + (sz >> 1));
    let mut surface = w.height(ccx, ccz);
    if height == PortalHeight::OceanFloor {
        while surface > MIN_Y && bl::is_water(w.get(ccx, surface, ccz)) {
            surface -= 1;
        }
    }
    let within = |r: &mut Xoroshiro, a: i32, b: i32| if a < b { random_range(r, a, b) } else { b };
    let start = match height {
        PortalHeight::Mountain => within(r, 70, surface - sy),
        PortalHeight::Underground => within(r, min_y, surface - sy),
        PortalHeight::PartlyBuried => surface - sy + random_range(r, 2, 8),
        _ => surface,
    };
    let mut y = start;
    while y > min_y {
        let n = [(x, z), (x + sx - 1, z), (x, z + sz - 1), (x + sx - 1, z + sz - 1)].iter().filter(|&&(a, b)| opaque(w.get(a, y, b))).count();
        if n >= 3 {
            break;
        }
        y -= 1;
    }
    let mut b = Blocks::default();
    t.stage(&mut b, (x, y, z));
    let (water, netherrack, obsidian, crying) = (super::super::ids().water, id("netherrack"), id("obsidian"), id("crying_obsidian"));
    for (bx, by, bz, v) in b.list() {
        if bl::is_air(v) && by < SEA_LEVEL {
            let near = [(0, 0, 0), (0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)].iter().any(|&(dx, dy, dz)| bl::is_water(w.get(bx + dx, by + dy, bz + dz)));
            if near {
                b.set(bx, by, bz, water);
                continue;
            }
        }
        if bl::is(v, "jigsaw") {
            b.set(bx, by, bz, netherrack);
        }
        if bl::is_lava(v) && bl::is_water(w.get(bx, by + 1, bz)) {
            b.set(bx, by, bz, obsidian);
        }
        if v == obsidian && r.next_int_below(5) == 0 {
            b.set(bx, by, bz, crying);
        }
    }
    b.write(w);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn have_data() -> bool {
        let ok = std::path::Path::new("data/structures.nbt").exists();
        if !ok {
            eprintln!("data/structures.nbt absent; skipping");
        }
        ok
    }

    #[test]
    fn hash_matches_network_ids() {
        let g = crate::world::GRASS;
        let e = crate::blocks::entry(g).unwrap();
        assert_eq!(state_hash(&e.name, &e.states, None), g);
    }

    #[test]
    fn loads_templates() {
        if !have_data() {
            return;
        }
        let t = get("shipwreck/with_mast").expect("with_mast");
        assert_eq!(t.size, [9, 21, 28]);
        let mut n = 0;
        t.for_each(Rotation::None, Mirror::None, |_, _, _, _| n += 1);
        assert!(n > 100, "{n} blocks");
        // Old stair hashes resolve via the legacy map.
        assert!(t.palette.iter().flatten().any(|&v| bl::short(v).ends_with("_stairs")));
        assert!(!get("pillager_outpost/base_plate").unwrap().jigsaws.is_empty());
        assert!(registry().len() > 1000);
        eprintln!("unresolved palette states: {}", unknown_states());
        // Rotation keeps the block count and swaps the footprint.
        let mut m = 0;
        t.for_each(Rotation::R90, Mirror::X, |x, _, z, _| {
            assert!((0..28).contains(&x) && (0..9).contains(&z));
            m += 1;
        });
        assert_eq!(n, m);
        let stairs = crate::blocks::any_state("minecraft:oak_stairs").unwrap();
        assert_eq!(rotate_state(stairs, 4), stairs);
    }

    fn find(t: usize, seed: u64) -> bool {
        let ow = crate::overworld::Overworld::new(seed);
        let ty = &super::super::types().iter().filter(|x| x.name.starts_with("normal_shipwreck") || x.name.starts_with("normal_igloo")).collect::<Vec<_>>()[t];
        let target = if t == 0 { "shipwreck" } else { "igloo" };
        let mut tried = 0;
        for cx in -200..200 {
            for cz in -200..200 {
                if !(ty.hint)(ow.seed, cx, cz) || tried > 12 {
                    continue;
                }
                let mut w = StructWorld { ow: &ow, writes: Default::default(), seed: ow.seed };
                let b = if t == 0 { biome_at(&w, cx, cz, 5, 0) } else { biome_at(&w, cx, cz, 3, 0) };
                let ok = if t == 0 { tagged(b, "ocean") || tagged(b, "beach") } else { matches!(b, 12 | 30 | 184) };
                if !ok {
                    continue;
                }
                tried += 1;
                (ty.build)(&mut w, &mut Xoroshiro::new(0), cx, cz);
                if w.writes.is_empty() {
                    continue;
                }
                let ch = ow.generate(cx, cz);
                let mine: Vec<_> = w.writes.iter().map(|(k, v)| (super::super::unkey(*k), *v)).filter(|((x, _, z), _)| x >> 4 == cx && z >> 4 == cz).collect();
                let hits = mine.iter().filter(|((x, y, z), v)| ch.get(x & 15, *y, z & 15) == *v).count();
                assert!(hits * 2 > mine.len(), "{target} at {cx},{cz}: {hits}/{} blocks present", mine.len());
                eprintln!("{target} at chunk {cx},{cz}: {hits} blocks");
                return true;
            }
        }
        eprintln!("no {target} candidate with a matching biome found");
        false
    }

    #[test]
    fn shipwreck_and_igloo_generate() {
        if !have_data() {
            return;
        }
        assert!(find(0, 12345), "no shipwreck");
        let _ = find(1, 12345); // igloo biomes are rare near spawn; reported, not required
    }
}

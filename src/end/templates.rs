// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
// from PNX StructureRegistry, PNXStructure, StructureRotationUtil
// (copied from overworld structures/templates.rs)
//! End city templates (`end_city/*` in data/structures.nbt), loaded once.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::blocks::State;
use crate::overworld::feature::blocks as bl;

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


/// Geometry rotation as PNX `PNXStructure.rotate`: `R90` maps (x, z) to (z, sx-1-x).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rot {
    None,
    R90,
    R180,
    R270,
}

impl Rot {
    /// Clockwise quarter turns applied to block states (inverse of the geometry, as PNX).
    pub fn state_turns(self) -> u32 {
        match self {
            Rot::None => 0,
            Rot::R90 => 3,
            Rot::R180 => 2,
            Rot::R270 => 1,
        }
    }
}

pub struct Template {
    pub size: [i32; 3],
    pub palette: Vec<Option<i32>>,
    blocks: Vec<u8>,
}

impl Template {
    pub fn rotated_size(&self, r: Rot) -> (i32, i32) {
        match r {
            Rot::R90 | Rot::R270 => (self.size[2], self.size[0]),
            _ => (self.size[0], self.size[2]),
        }
    }

    /// Non-void blocks (x, y, z, id) in the rotated frame. `patch` can swap a block by its
    /// unrotated position (PNX `setBlock` before `rotate`).
    pub fn for_each(&self, r: Rot, patch: impl Fn(i32, i32, i32) -> Option<i32>, mut f: impl FnMut(i32, i32, i32, i32)) {
        let turns = r.state_turns();
        let pal: Vec<Option<i32>> = self.palette.iter().map(|p| p.map(|v| rotate_state(v, turns))).collect();
        let [sx, sy, sz] = self.size;
        let mut i = 0;
        for z in 0..sz {
            for y in 0..sy {
                for x in 0..sx {
                    let p = self.blocks.get(i).copied().unwrap_or(0);
                    i += 1;
                    let v = match patch(x, y, z) {
                        Some(v) => rotate_state(v, turns),
                        None => match (p as usize).checked_sub(1).and_then(|k| pal.get(k)) {
                            Some(Some(v)) => *v,
                            _ => continue,
                        },
                    };
                    let (rx, rz) = match r {
                        Rot::None => (x, z),
                        Rot::R90 => (z, sx - 1 - x),
                        Rot::R180 => (sx - 1 - x, sz - 1 - z),
                        Rot::R270 => (sz - 1 - z, x),
                    };
                    f(rx, y, rz, v);
                }
            }
        }
    }
}

fn resolve(h: i32, legacy: &HashMap<i32, i32>) -> Option<i32> {
    let v = if crate::blocks::entry(h).is_some() { Some(h) } else { legacy.get(&h).copied() };
    v.filter(|&v| !bl::is(v, "structure_void"))
}

fn load(path: &str) -> HashMap<String, Template> {
    let mut out = HashMap::new();
    let Ok(file) = std::fs::File::open(path) else {
        println!("end: {path} missing; end cities disabled");
        return out;
    };
    let Ok(root) = nbtx::from_bytes::<nbtx::BigEndian, nbtx::Value>(&mut flate2::read::GzDecoder::new(std::io::BufReader::new(file))) else {
        println!("end: cannot read {path}; end cities disabled");
        return out;
    };
    let palette_of = |o: &nbtx::Value| -> Vec<i32> {
        match nbt_get(o, "palette") {
            Some(nbtx::Value::List(nbtx::ValueList::Int(v))) => v.clone(),
            _ => vec![],
        }
    };
    // PNX `registerFromTag`: nested compounds, an `object` child is a template.
    let mut raw: Vec<(String, &nbtx::Value)> = vec![];
    for top in ["end_city", "endcity"] {
        let Some(nbtx::Value::Compound(m)) = nbt_get(&root, top) else { continue };
        for (k, c) in m.iter() {
            if let Some(o) = nbt_get(c, "object") {
                raw.push((String::from_utf8_lossy(k).into_owned(), o));
            }
        }
    }
    let want: HashSet<i32> = raw.iter().flat_map(|(_, o)| palette_of(o)).filter(|&h| crate::blocks::entry(h).is_none()).collect();
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
        out.entry(name).or_insert(Template { size, palette, blocks });
    }
    out
}

/// An end city piece by name ("base_floor", "ship", ...).
pub fn get(name: &str) -> Option<&'static Template> {
    static R: OnceLock<HashMap<String, Template>> = OnceLock::new();
    R.get_or_init(|| load("data/structures.nbt")).get(name)
}

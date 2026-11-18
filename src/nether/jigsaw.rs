// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
// from PNX JigsawStructure, BastionStructure, StructurePool
// (assembly copied from overworld jigsaw/mod.rs)
//! Bastion remnants: jigsaw from `bastion/starts`, depth 6, no collision test.
//! TODO: loot, mobs and spawner types need block entities.

use std::collections::HashSet;

use super::rng::BedrockRandom;
use super::templates::{self, Blocks, Jigsaw, Mirror, Rotation, Template};
use crate::blocks::State;
use crate::overworld::feature::blocks as bl;

/// (name, fallback, entries with weights).
type Pool = (&'static str, &'static str, &'static [(&'static str, i32)]);

const MAX_DEPTH: i32 = 6;
const START: &str = "bastion/starts";

const POOLS: &[Pool] = &[
    ("bastion/starts", "", &[("bastion/units/air_base", 1), ("bastion/hoglin_stable/air_base", 1), ("bastion/treasure/big_air_full", 1), ("bastion/bridge/starting_pieces/entrance_base", 1)]),
    ("bastion/units/center_pieces", "", &[("bastion/units/center_pieces/center_0", 1), ("bastion/units/center_pieces/center_1", 1), ("bastion/units/center_pieces/center_2", 1)]),
    ("bastion/units/pathways", "", &[("bastion/units/pathways/pathway_0", 1), ("bastion/units/pathways/pathway_wall_0", 1)]),
    ("bastion/units/walls/wall_bases", "", &[("bastion/units/walls/wall_base", 1), ("bastion/units/walls/connected_wall", 1)]),
    ("bastion/units/stages/stage_0", "", &[("bastion/units/stages/stage_0_0", 1), ("bastion/units/stages/stage_0_1", 1), ("bastion/units/stages/stage_0_2", 1), ("bastion/units/stages/stage_0_3", 1)]),
    ("bastion/units/stages/stage_1", "", &[("bastion/units/stages/stage_1_0", 1), ("bastion/units/stages/stage_1_1", 1), ("bastion/units/stages/stage_1_2", 1), ("bastion/units/stages/stage_1_3", 1)]),
    ("bastion/units/stages/rot/stage_1", "", &[("bastion/units/stages/rot/stage_1_0", 1)]),
    ("bastion/units/stages/stage_2", "", &[("bastion/units/stages/stage_2_0", 1), ("bastion/units/stages/stage_2_1", 1)]),
    ("bastion/units/stages/stage_3", "", &[("bastion/units/stages/stage_3_0", 1), ("bastion/units/stages/stage_3_1", 1), ("bastion/units/stages/stage_3_2", 1), ("bastion/units/stages/stage_3_3", 1)]),
    ("bastion/units/fillers/stage_0", "", &[("bastion/units/fillers/stage_0", 1)]),
    ("bastion/units/edges", "", &[("bastion/units/edges/edge_0", 1)]),
    ("bastion/units/wall_units", "", &[("bastion/units/wall_units/unit_0", 1)]),
    ("bastion/units/edge_wall_units", "", &[("bastion/units/wall_units/edge_0_large", 1)]),
    ("bastion/units/ramparts", "", &[("bastion/units/ramparts/ramparts_0", 1), ("bastion/units/ramparts/ramparts_1", 1), ("bastion/units/ramparts/ramparts_2", 1)]),
    ("bastion/units/large_ramparts", "", &[("bastion/units/ramparts/ramparts_0", 1)]),
    ("bastion/units/rampart_plates", "", &[("bastion/units/rampart_plates/plate_0", 1)]),
    ("bastion/hoglin_stable/starting_pieces", "", &[("bastion/hoglin_stable/starting_pieces/starting_stairs_0", 1), ("bastion/hoglin_stable/starting_pieces/starting_stairs_1", 1), ("bastion/hoglin_stable/starting_pieces/starting_stairs_2", 1), ("bastion/hoglin_stable/starting_pieces/starting_stairs_3", 1), ("bastion/hoglin_stable/starting_pieces/starting_stairs_4", 1)]),
    ("bastion/hoglin_stable/mirrored_starting_pieces", "", &[("bastion/hoglin_stable/starting_pieces/stairs_0_mirrored", 1), ("bastion/hoglin_stable/starting_pieces/stairs_1_mirrored", 1), ("bastion/hoglin_stable/starting_pieces/stairs_2_mirrored", 1), ("bastion/hoglin_stable/starting_pieces/stairs_3_mirrored", 1), ("bastion/hoglin_stable/starting_pieces/stairs_4_mirrored", 1)]),
    ("bastion/hoglin_stable/wall_bases", "", &[("bastion/hoglin_stable/walls/wall_base", 1)]),
    ("bastion/hoglin_stable/walls", "", &[("bastion/hoglin_stable/walls/side_wall_0", 1), ("bastion/hoglin_stable/walls/side_wall_1", 1)]),
    ("bastion/hoglin_stable/stairs", "", &[("bastion/hoglin_stable/stairs/stairs_1_0", 1), ("bastion/hoglin_stable/stairs/stairs_1_1", 1), ("bastion/hoglin_stable/stairs/stairs_1_2", 1), ("bastion/hoglin_stable/stairs/stairs_1_3", 1), ("bastion/hoglin_stable/stairs/stairs_1_4", 1), ("bastion/hoglin_stable/stairs/stairs_2_0", 1), ("bastion/hoglin_stable/stairs/stairs_2_1", 1), ("bastion/hoglin_stable/stairs/stairs_2_2", 1), ("bastion/hoglin_stable/stairs/stairs_2_3", 1), ("bastion/hoglin_stable/stairs/stairs_2_4", 1), ("bastion/hoglin_stable/stairs/stairs_3_0", 1), ("bastion/hoglin_stable/stairs/stairs_3_1", 1), ("bastion/hoglin_stable/stairs/stairs_3_2", 1), ("bastion/hoglin_stable/stairs/stairs_3_3", 1), ("bastion/hoglin_stable/stairs/stairs_3_4", 1)]),
    ("bastion/hoglin_stable/small_stables/inner", "", &[("bastion/hoglin_stable/small_stables/inner_0", 1), ("bastion/hoglin_stable/small_stables/inner_1", 1), ("bastion/hoglin_stable/small_stables/inner_2", 1), ("bastion/hoglin_stable/small_stables/inner_3", 1)]),
    ("bastion/hoglin_stable/small_stables/outer", "", &[("bastion/hoglin_stable/small_stables/outer_0", 1), ("bastion/hoglin_stable/small_stables/outer_1", 1), ("bastion/hoglin_stable/small_stables/outer_2", 1), ("bastion/hoglin_stable/small_stables/outer_3", 1)]),
    ("bastion/hoglin_stable/large_stables/inner", "", &[("bastion/hoglin_stable/large_stables/inner_0", 1), ("bastion/hoglin_stable/large_stables/inner_1", 1), ("bastion/hoglin_stable/large_stables/inner_2", 1), ("bastion/hoglin_stable/large_stables/inner_3", 1), ("bastion/hoglin_stable/large_stables/inner_4", 1)]),
    ("bastion/hoglin_stable/large_stables/outer", "", &[("bastion/hoglin_stable/large_stables/outer_0", 1), ("bastion/hoglin_stable/large_stables/outer_1", 1), ("bastion/hoglin_stable/large_stables/outer_2", 1), ("bastion/hoglin_stable/large_stables/outer_3", 1), ("bastion/hoglin_stable/large_stables/outer_4", 1)]),
    ("bastion/hoglin_stable/posts", "", &[("bastion/hoglin_stable/posts/stair_post", 1), ("bastion/hoglin_stable/posts/end_post", 1)]),
    ("bastion/hoglin_stable/ramparts", "", &[("bastion/hoglin_stable/ramparts/ramparts_1", 1), ("bastion/hoglin_stable/ramparts/ramparts_2", 1), ("bastion/hoglin_stable/ramparts/ramparts_3", 1)]),
    ("bastion/hoglin_stable/rampart_plates", "", &[("bastion/hoglin_stable/rampart_plates/rampart_plate_1", 1)]),
    ("bastion/hoglin_stable/connectors", "", &[("bastion/hoglin_stable/connectors/end_post_connector", 1)]),
    ("bastion/treasure/bases", "", &[("bastion/treasure/bases/lava_basin", 1)]),
    ("bastion/treasure/stairs", "", &[("bastion/treasure/stairs/lower_stairs", 1)]),
    ("bastion/treasure/bases/centers", "", &[("bastion/treasure/bases/centers/center_0", 1), ("bastion/treasure/bases/centers/center_1", 1), ("bastion/treasure/bases/centers/center_2", 1), ("bastion/treasure/bases/centers/center_3", 1)]),
    ("bastion/treasure/brains", "", &[("bastion/treasure/brains/center_brain", 1)]),
    ("bastion/treasure/walls", "", &[("bastion/treasure/walls/lava_wall", 1), ("bastion/treasure/walls/entrance_wall", 1)]),
    ("bastion/treasure/walls/outer", "", &[("bastion/treasure/walls/outer/top_corner", 1), ("bastion/treasure/walls/outer/mid_corner", 1), ("bastion/treasure/walls/outer/bottom_corner", 1), ("bastion/treasure/walls/outer/outer_wall", 1), ("bastion/treasure/walls/outer/medium_outer_wall", 1), ("bastion/treasure/walls/outer/tall_outer_wall", 1)]),
    ("bastion/treasure/walls/bottom", "", &[("bastion/treasure/walls/bottom/wall_0", 1), ("bastion/treasure/walls/bottom/wall_1", 1), ("bastion/treasure/walls/bottom/wall_2", 1), ("bastion/treasure/walls/bottom/wall_3", 1)]),
    ("bastion/treasure/walls/mid", "", &[("bastion/treasure/walls/mid/wall_0", 1), ("bastion/treasure/walls/mid/wall_1", 1), ("bastion/treasure/walls/mid/wall_2", 1)]),
    ("bastion/treasure/walls/top", "", &[("bastion/treasure/walls/top/main_entrance", 1), ("bastion/treasure/walls/top/wall_0", 1), ("bastion/treasure/walls/top/wall_1", 1)]),
    ("bastion/treasure/connectors", "", &[("bastion/treasure/connectors/center_to_wall_middle", 1), ("bastion/treasure/connectors/center_to_wall_top", 1), ("bastion/treasure/connectors/center_to_wall_top_entrance", 1)]),
    ("bastion/treasure/entrances", "", &[("bastion/treasure/entrances/entrance_0", 1)]),
    ("bastion/treasure/ramparts", "", &[("bastion/treasure/ramparts/mid_wall_main", 1), ("bastion/treasure/ramparts/mid_wall_side", 1), ("bastion/treasure/ramparts/bottom_wall_0", 1), ("bastion/treasure/ramparts/top_wall", 1), ("bastion/treasure/ramparts/lava_basin_side", 1), ("bastion/treasure/ramparts/lava_basin_main", 1)]),
    ("bastion/treasure/corners/bottom", "", &[("bastion/treasure/corners/bottom/corner_0", 1), ("bastion/treasure/corners/bottom/corner_1", 1)]),
    ("bastion/treasure/corners/edges", "", &[("bastion/treasure/corners/edges/bottom", 1), ("bastion/treasure/corners/edges/middle", 1), ("bastion/treasure/corners/edges/top", 1)]),
    ("bastion/treasure/corners/middle", "", &[("bastion/treasure/corners/middle/corner_0", 1), ("bastion/treasure/corners/middle/corner_1", 1)]),
    ("bastion/treasure/corners/top", "", &[("bastion/treasure/corners/top/corner_0", 1), ("bastion/treasure/corners/top/corner_1", 1)]),
    ("bastion/treasure/extensions/large_pool", "", &[("bastion/treasure/extensions/empty", 1), ("bastion/treasure/extensions/empty", 1), ("bastion/treasure/extensions/fire_room", 1), ("bastion/treasure/extensions/large_bridge_0", 1), ("bastion/treasure/extensions/large_bridge_1", 1), ("bastion/treasure/extensions/large_bridge_2", 1), ("bastion/treasure/extensions/large_bridge_3", 1), ("bastion/treasure/extensions/roofed_bridge", 1), ("bastion/treasure/extensions/empty", 1)]),
    ("bastion/treasure/extensions/small_pool", "", &[("bastion/treasure/extensions/empty", 1), ("bastion/treasure/extensions/fire_room", 1), ("bastion/treasure/extensions/empty", 1), ("bastion/treasure/extensions/small_bridge_0", 1), ("bastion/treasure/extensions/small_bridge_1", 1), ("bastion/treasure/extensions/small_bridge_2", 1), ("bastion/treasure/extensions/small_bridge_3", 1)]),
    ("bastion/treasure/extensions/houses", "", &[("bastion/treasure/extensions/house_0", 1), ("bastion/treasure/extensions/house_1", 1)]),
    ("bastion/treasure/roofs", "", &[("bastion/treasure/roofs/wall_roof", 1), ("bastion/treasure/roofs/corner_roof", 1), ("bastion/treasure/roofs/center_roof", 1)]),
    ("bastion/bridge/starting_pieces", "", &[("bastion/bridge/starting_pieces/entrance", 1), ("bastion/bridge/starting_pieces/entrance_face", 1)]),
    ("bastion/bridge/bridge_pieces", "", &[("bastion/bridge/bridge_pieces/bridge", 1)]),
    ("bastion/bridge/legs", "", &[("bastion/bridge/legs/leg_0", 1), ("bastion/bridge/legs/leg_1", 1)]),
    ("bastion/bridge/walls", "", &[("bastion/bridge/walls/wall_base_0", 1), ("bastion/bridge/walls/wall_base_1", 1)]),
    ("bastion/bridge/ramparts", "", &[("bastion/bridge/ramparts/rampart_0", 1), ("bastion/bridge/ramparts/rampart_1", 1)]),
    ("bastion/bridge/rampart_plates", "", &[("bastion/bridge/rampart_plates/plate_0", 1)]),
    ("bastion/bridge/connectors", "", &[("bastion/bridge/connectors/back_bridge_top", 1), ("bastion/bridge/connectors/back_bridge_bottom", 1)]),
    ("bastion/mobs/piglin", "", &[("bastion/mobs/melee_piglin", 1), ("bastion/mobs/sword_piglin", 4), ("bastion/mobs/crossbow_piglin", 4), ("bastion/mobs/empty", 1)]),
    ("bastion/mobs/hoglin", "", &[("bastion/mobs/hoglin", 2), ("bastion/mobs/empty", 1)]),
    ("bastion/blocks/gold", "", &[("bastion/blocks/air", 3), ("bastion/blocks/gold", 1)]),
    ("bastion/mobs/piglin_melee", "", &[("bastion/mobs/melee_piglin_always", 1), ("bastion/mobs/melee_piglin", 5), ("bastion/mobs/sword_piglin", 1)]),
];

const ROTATIONS: [Rotation; 4] = [Rotation::None, Rotation::R90, Rotation::R180, Rotation::R270];

fn norm(k: &str) -> &str {
    k.find(':').map_or(k, |i| &k[i + 1..])
}

fn is_empty_entry(k: &str) -> bool {
    let n = norm(k);
    n.is_empty() || n == "empty"
}

/// PNX `nextExclusiveInt` on the bastion's `BedrockRandom`.
fn below(r: &mut BedrockRandom, n: i32) -> i32 {
    r.next_int_below(n)
}

fn shuffle_from_end<T>(v: &mut [T], r: &mut BedrockRandom) {
    for i in (1..v.len()).rev() {
        let j = below(r, i as i32 + 1) as usize;
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
    let top = if front == 0 { [2, 4, 3, 5][(rot & 3) as usize] } else { [2, 5, 3, 4][(rot & 3) as usize] };
    Some((front, rotate_face(top, r)))
}

/// PNX `getOrderedJigsaws`: shuffled, then stably sorted by selection priority (high first).
fn ordered(src: &[Jigsaw], r: &mut BedrockRandom) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..src.len()).collect();
    shuffle_from_end(&mut idx, r);
    idx.sort_by_key(|&i| std::cmp::Reverse(src[i].selection_priority));
    idx
}

struct Piece {
    t: &'static Template,
    pos: [i32; 3],
    rot: Rotation,
    /// Rotated jigsaws relative to `pos` (same order as `t.jigsaws`).
    placed: Vec<Jigsaw>,
}

fn pool(name: &str) -> Option<&'static Pool> {
    POOLS.iter().rev().find(|p| p.0 == name)
}

/// PNX `placeStructurePiece`: stage the template, replace its jigsaws with their final states.
fn place(staged: &mut Blocks, pos: [i32; 3], rot: Rotation, t: &'static Template) -> Piece {
    let mut tmp = Blocks::default();
    t.for_each(rot, Mirror::None, |x, y, z, v| tmp.set(pos[0] + x, pos[1] + y, pos[2] + z, v));
    let js = t.jigsaws_rotated(rot, Mirror::None);
    for j in &js {
        let fs = if bl::is(j.final_state, "jigsaw") { bl::air() } else { j.final_state };
        if !bl::is(fs, "structure_void") {
            tmp.set(pos[0] + j.pos[0], pos[1] + j.pos[1], pos[2] + j.pos[2], fs);
        }
    }
    staged.merge(&tmp);
    Piece { t, pos, rot, placed: js }
}

fn place_root(staged: &mut Blocks, origin: [i32; 3], rot: Rotation, p: &Pool, r: &mut BedrockRandom) -> Option<Piece> {
    let total: i32 = p.2.iter().map(|e| e.1).sum();
    let mut target = below(r, total);
    let entry = p.2.iter().find(|e| {
        let hit = target < e.1;
        target -= e.1;
        hit
    })?;
    if is_empty_entry(entry.0) {
        return None;
    }
    let t = templates::get(norm(entry.0))?;
    Some(place(staged, origin, rot, t))
}

type Found = (&'static Template, Rotation, [i32; 3], [i32; 3]);

/// PNX `findCandidate` (+ fallback pool): (template, rotation, position, child jigsaw pos).
fn candidate(p: &Pool, parent: &Piece, src: &Jigsaw, parent_pos: [i32; 3], connected: &HashSet<[i32; 3]>, r: &mut BedrockRandom) -> Option<Found> {
    match candidate_in(p, parent, src, parent_pos, connected, r) {
        Ok(v) => Some(v),
        Err(true) => None,
        Err(false) => {
            let fb = norm(p.1);
            if fb.is_empty() {
                return None;
            }
            candidate_in(pool(fb)?, parent, src, parent_pos, connected, r).ok()
        }
    }
}

/// Err(true) means an empty (terminal) entry. Bastions skip distance and overlap checks.
fn candidate_in(p: &Pool, parent: &Piece, src: &Jigsaw, parent_pos: [i32; 3], connected: &HashSet<[i32; 3]>, r: &mut BedrockRandom) -> Result<Found, bool> {
    let mut entries: Vec<&str> = p.2.iter().flat_map(|e| std::iter::repeat_n(e.0, e.1.max(0) as usize)).collect();
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
                if pf != cw(cw(cf)) || !(joint == "rollable" || ptop == ctop) {
                    continue;
                }
                let wp = side(parent_pos, pf);
                let q = rotated[ci].pos;
                conn = Some(([wp[0] - q[0], wp[1] - q[1], wp[2] - q[2]], wp));
                break;
            }
            let Some((cpos, cjig)) = conn else { continue };
            if connected.contains(&cjig) {
                continue;
            }
            return Ok((t, rot, cpos, cjig));
        }
    }
    Err(false)
}

/// `BastionStructure.placeWithBounds` at `origin`: every staged block (air included).
pub fn bastion(origin: [i32; 3], r: &mut BedrockRandom) -> Vec<(i32, i32, i32, i32)> {
    let mut staged = Blocks::default();
    let Some(start) = pool(START) else { return vec![] };
    let rot = ROTATIONS[below(r, 4) as usize];
    let Some(root) = place_root(&mut staged, origin, rot, start, r) else { return vec![] };
    let mut connected: HashSet<[i32; 3]> = HashSet::new();
    let mut pending: Vec<(Piece, i32, i32)> = vec![(root, 0, 0)];
    while !pending.is_empty() {
        let (piece, depth, _) = pending.remove(0);
        if depth >= MAX_DEPTH {
            continue;
        }
        for si in ordered(&piece.t.jigsaws, r) {
            let src = &piece.t.jigsaws[si];
            let pj = &piece.placed[si];
            let parent_pos = [piece.pos[0] + pj.pos[0], piece.pos[1] + pj.pos[1], piece.pos[2] + pj.pos[2]];
            if connected.contains(&parent_pos) {
                continue;
            }
            let Some(p) = pool(norm(&src.pool)) else { continue };
            let Some((t, crot, cpos, cjig)) = candidate(p, &piece, src, parent_pos, &connected, r) else { continue };
            let child = place(&mut staged, cpos, crot, t);
            connected.insert(parent_pos);
            connected.insert(cjig);
            if depth + 1 < MAX_DEPTH {
                let prio = src.placement_priority;
                let at = pending.iter().position(|p| prio > p.2).unwrap_or(pending.len());
                pending.insert(at, (child, depth + 1, prio));
            }
        }
    }
    staged.list()
}

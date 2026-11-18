// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! ObjectMangroveTree: roots, branching trunk, spread foliage, vines and propagules.

use super::java::{JavaHashSet, JavaRandom, P};
use super::misc::bee_nest_place_at;
use super::*;
use crate::overworld::feature::random_range;

/// ObjectMangroveTree. PNX rolls `tall` from an unseeded RNG, so the caller picks it.
#[derive(Clone, Copy)]
pub struct Mangrove {
    pub tall: bool,
    pub with_bee_nest: bool,
    pub bee_count: i32,
}

struct Props {
    base_height: i32,
    height_rand_a: i32,
    height_rand_b: i32,
    steps_min: i32,
    steps_max: i32,
    len_min: i32,
    len_max: i32,
    root_min: i32,
    root_max: i32,
}

const ROOT_WIDTH_LIMIT: i32 = 8;
const ROOT_LENGTH_LIMIT: usize = 15;

fn can_place_root(b: i32) -> bool {
    is_air(b) || matches!(nm(b), "water" | "flowing_water" | "mud" | "mangrove_roots" | "muddy_mangrove_roots" | "mangrove_leaves" | "mangrove_propagule" | "vine")
}

struct Ids {
    log: i32,
    leaves: i32,
}

impl Mangrove {
    pub fn new(tall: bool) -> Self {
        Self { tall, with_bee_nest: false, bee_count: 3 }
    }

    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        let p = if self.tall {
            Props { base_height: 4, height_rand_a: 1, height_rand_b: 9, steps_min: 1, steps_max: 6, len_min: 0, len_max: 1, root_min: 3, root_max: 7 }
        } else {
            Props { base_height: 2, height_rand_a: 1, height_rand_b: 4, steps_min: 1, steps_max: 4, len_min: 0, len_max: 1, root_min: 1, root_max: 3 }
        };
        let trunk_off = random_range(rand, p.root_min, p.root_max);
        let trunk = (x, y + trunk_off, z);
        let tree_height = p.base_height + rand.next_int_below(p.height_rand_a + 1) + rand.next_int_below(p.height_rand_b + 1);
        if !Self::place_roots(w, rand, (x, y, z), trunk) {
            return false;
        }
        let ids = Ids { log: axis_log("mangrove_log", "y"), leaves: id("mangrove_leaves") };
        let attachments = Self::place_trunk(w, rand, &ids, tree_height, trunk, &p);
        for &(ox, oy, oz) in &attachments {
            for _ in 0..70 {
                let lx = ox + rand.next_int_below(3) - rand.next_int_below(3);
                let ly = oy + rand.next_int_below(2) - rand.next_int_below(2);
                let lz = oz + rand.next_int_below(3) - rand.next_int_below(3);
                let b = w.get(lx, ly, lz);
                if is_air(b) || matches!(nm(b), "water" | "flowing_water" | "vine" | "mangrove_propagule") {
                    // TODO: PNX also puts water on layer 1 here, Region only has one layer.
                    w.set(lx, ly, lz, ids.leaves);
                }
            }
        }
        // vines
        let leaves = Self::collect_leaves(w, &attachments);
        for (lx, ly, lz) in leaves.iter() {
            for (dx, dz, attached) in [(-1, 0, Face::East), (1, 0, Face::West), (0, -1, Face::South), (0, 1, Face::North)] {
                let (vx, vz) = (lx + dx, lz + dz);
                if rand.next_float() < 0.125 && is_air(w.get(vx, ly, vz)) {
                    // getVineMeta(attachedTo): SOUTH 1, WEST 2, NORTH 4, EAST 8 (= vine_meta)
                    let v = vine(attached.vine_meta());
                    w.set(vx, ly, vz, v);
                    let (mut vy, mut max) = (ly - 1, 4);
                    while is_air(w.get(vx, vy, vz)) && max > 0 {
                        w.set(vx, vy, vz, v);
                        vy -= 1;
                        max -= 1;
                    }
                }
            }
        }
        // propagules
        let mut list: Vec<P> = Self::collect_leaves(w, &attachments).iter().collect();
        JavaRandom::new(rand.next_long()).shuffle(&mut list);
        let mut blacklist = std::collections::HashSet::new();
        let propagule = id("mangrove_propagule");
        for (lx, ly, lz) in list {
            let pp = (lx, ly - 1, lz);
            if !blacklist.contains(&pp) && rand.next_float() < 0.14 && is_air(w.get(lx, ly - 1, lz)) && is_air(w.get(lx, ly - 2, lz)) {
                for bx in pp.0 - 1..=pp.0 + 1 {
                    for bz in pp.2 - 1..=pp.2 + 1 {
                        blacklist.insert((bx, pp.1, bz));
                    }
                }
                let stage = random_range(rand, 0, 4);
                let s = blocks::with(propagule, &[("hanging", blocks::b(true)), ("propagule_stage", blocks::i(stage))]);
                w.set(pp.0, pp.1, pp.2, s);
            }
        }
        // bee nest
        if self.with_bee_nest && !attachments.is_empty() {
            let (ax, ay, az) = attachments[rand.next_int_below(attachments.len() as i32) as usize];
            let (nx, ny, nz) = (ax, ay - 1, az + 1);
            if is_air(w.get(nx, ny, nz)) && !is_air(w.get(nx, ny + 1, nz)) {
                bee_nest_place_at(w, nx, ny, nz);
            }
        }
        true
    }

    fn collect_leaves<W: World + ?Sized>(w: &W, attachments: &[P]) -> JavaHashSet {
        let mut set = JavaHashSet::default();
        for &(ox, oy, oz) in attachments {
            for lx in ox - 2..=ox + 2 {
                for ly in oy - 1..=oy + 1 {
                    for lz in oz - 2..=oz + 2 {
                        if nm(w.get(lx, ly, lz)) == "mangrove_leaves" {
                            set.insert((lx, ly, lz));
                        }
                    }
                }
            }
        }
        set
    }

    fn place_log<W: World + ?Sized>(w: &mut W, ids: &Ids, (x, y, z): P) -> bool {
        let b = w.get(x, y, z);
        if !(is_air(b) || matches!(nm(b), "water" | "flowing_water" | "mangrove_leaves" | "vine" | "mangrove_propagule")) {
            return false;
        }
        w.set(x, y, z, ids.log);
        true
    }

    fn place_trunk<W: World + ?Sized>(w: &mut W, rand: &mut Xoroshiro, ids: &Ids, tree_height: i32, origin: P, p: &Props) -> Vec<P> {
        let mut att = Vec::new();
        for hp in 0..tree_height {
            let cur = origin.1 + hp;
            let log_pos = (origin.0, cur, origin.2);
            if Self::place_log(w, ids, log_pos) && hp < tree_height - 1 && rand.next_float() < 0.5 {
                let dir = Face::HORIZONTALS[rand.next_bounded_int(3) as usize];
                let len = random_range(rand, p.len_min, p.len_max);
                let pos = 0.max(len - random_range(rand, p.len_min, p.len_max) - 1);
                let mut steps = random_range(rand, p.steps_min, p.steps_max);
                // placeBranch
                let mut along = cur + pos;
                let (mut lx, mut lz) = (log_pos.0, log_pos.2);
                let mut idx = pos;
                while idx < tree_height && steps > 0 {
                    if idx >= 1 {
                        let ph = cur + idx;
                        lx += dir.dx();
                        lz += dir.dz();
                        along = ph;
                        if Self::place_log(w, ids, (lx, ph, lz)) {
                            along = ph + 1;
                        }
                        att.push((lx, ph, lz));
                    }
                    idx += 1;
                    steps -= 1;
                }
                if along - cur > 1 {
                    att.push((lx, along, lz));
                    att.push((lx, along - 2, lz));
                }
            }
            if hp == tree_height - 1 {
                att.push((origin.0, cur + 1, origin.2));
            }
        }
        att
    }

    fn place_roots<W: World + ?Sized>(w: &mut W, rand: &mut Xoroshiro, origin: P, trunk: P) -> bool {
        let mut cy = origin.1;
        while cy < trunk.1 {
            if !can_place_root(w.get(origin.0, cy, origin.2)) {
                return false;
            }
            cy += 1;
        }
        let mut roots = vec![(trunk.0, trunk.1 - 1, trunk.2)];
        let mut dir_list = Vec::new();
        for dir in Face::PLANE {
            let pos = (trunk.0 + dir.dx(), trunk.1, trunk.2 + dir.dz());
            dir_list.clear();
            if !Self::simulate_roots(w, rand, pos, dir, trunk, &mut dir_list, 0) {
                return false;
            }
            roots.extend_from_slice(&dir_list);
            roots.push(pos);
        }
        let (roots_id, muddy, moss) = (id("mangrove_roots"), axis_log("muddy_mangrove_roots", "y"), id("moss_carpet"));
        for (rx, ry, rz) in roots {
            let prev = w.get(rx, ry, rz);
            w.set(rx, ry, rz, if matches!(nm(prev), "mud" | "muddy_mangrove_roots") { muddy } else { roots_id });
            if rand.next_float() < 0.5 && is_air(w.get(rx, ry + 1, rz)) {
                w.set(rx, ry + 1, rz, moss);
            }
        }
        true
    }

    fn simulate_roots<W: World + ?Sized>(w: &W, rand: &mut Xoroshiro, pos: P, dir: Face, origin: P, out: &mut Vec<P>, layer: usize) -> bool {
        if layer == ROOT_LENGTH_LIMIT || out.len() > ROOT_LENGTH_LIMIT {
            return false;
        }
        let below = (pos.0, pos.1 - 1, pos.2);
        let next_to = (pos.0 + dir.dx(), pos.1, pos.2 + dir.dz());
        let width = (pos.0 - origin.0).abs() + (pos.1 - origin.1).abs() + (pos.2 - origin.2).abs();
        let mut cands = [below; 2];
        let n = if width > ROOT_WIDTH_LIMIT - 3 && width <= ROOT_WIDTH_LIMIT {
            if rand.next_float() < 0.2 {
                cands[1] = (next_to.0, next_to.1 - 1, next_to.2);
                2
            } else {
                1
            }
        } else if width > ROOT_WIDTH_LIMIT || rand.next_float() < 0.2 {
            1
        } else {
            if rand.next_boolean() {
                cands[0] = next_to;
            }
            1
        };
        for &c in &cands[..n] {
            if can_place_root(w.get(c.0, c.1, c.2)) {
                out.push(c);
                if !Self::simulate_roots(w, rand, c, dir, origin, out, layer + 1) {
                    return false;
                }
            }
        }
        true
    }
}

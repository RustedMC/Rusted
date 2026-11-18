// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Leaning trees: ObjectSavannaTree, ObjectSavannaTreeWorldgen, ObjectDarkOakTree,
//! ObjectRoofedTreeWorldgen.

use super::spruce::conifer_may_replace;
use super::*;

/// ObjectSavannaTree: acacia with a leaning trunk and an optional second branch.
#[derive(Clone, Copy, Default)]
pub struct Savanna;

impl Savanna {
    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        let i = rand.next_int_below(3) + rand.next_int_below(3) + 5;
        if !(y >= 1 && y + i < 256) {
            return false;
        }
        for j in y..=y + 1 + i {
            let k = if j >= y + 1 + i - 2 { 2 } else if j == y { 0 } else { 1 };
            for l in x - k..=x + k {
                for i1 in z - k..=z + k {
                    if !(0..256).contains(&j) || !can_grow_into(w.get(l, j, i1)) {
                        return false;
                    }
                }
            }
        }
        if !(matches!(nm(w.get(x, y - 1, z)), "grass_block" | "dirt") && y < 256 - i - 1) {
            return false;
        }
        let trunk = axis_log("acacia_log", "y");
        let leaf = id("acacia_leaves");
        let place_leaf = |w: &mut W, x, y, z| {
            let b = w.get(x, y, z);
            if is_air(b) || is_leaves(b) {
                w.set(x, y, z, leaf);
            }
        };
        set_dirt(w, x, y - 1, z);
        let face = Face::random(rand);
        let k2 = i - rand.next_int_below(4) - 1;
        let mut l2 = 3 - rand.next_int_below(3);
        let (mut i3, mut j1) = (x, z);
        let mut k1 = 0;
        for l1 in 0..i {
            let i2 = y + l1;
            if l1 >= k2 && l2 > 0 {
                i3 += face.dx();
                j1 += face.dz();
                l2 -= 1;
            }
            let b = w.get(i3, i2, j1);
            if is_air(b) || is_leaves(b) {
                w.set(i3, i2, j1, trunk);
                k1 = i2;
            }
        }
        for j3 in -3..=3i32 {
            for i4 in -3..=3i32 {
                if j3.abs() != 3 || i4.abs() != 3 {
                    place_leaf(w, i3 + j3, k1, j1 + i4);
                }
            }
        }
        for k3 in -1..=1 {
            for j4 in -1..=1 {
                place_leaf(w, i3 + k3, k1 + 1, j1 + j4);
            }
        }
        place_leaf(w, i3 + 2, k1 + 1, j1);
        place_leaf(w, i3 - 2, k1 + 1, j1);
        place_leaf(w, i3, k1 + 1, j1 + 2);
        place_leaf(w, i3, k1 + 1, j1 - 2);
        let (mut i3, mut j1) = (x, z);
        let face1 = Face::random(rand);
        if face1 != face {
            let mut l4 = k2 - rand.next_int_below(2) - 1;
            let mut k4 = 1 + rand.next_int_below(3);
            let mut k1 = 0;
            while l4 < i && k4 > 0 {
                if l4 >= 1 {
                    let j2 = y + l4;
                    i3 += face1.dx();
                    j1 += face1.dz();
                    let b = w.get(i3, j2, j1);
                    if is_air(b) || is_leaves(b) {
                        w.set(i3, j2, j1, trunk);
                        k1 = j2;
                    }
                }
                l4 += 1;
                k4 -= 1;
            }
            if k1 > 0 {
                for i5 in -2..=2i32 {
                    for k5 in -2..=2i32 {
                        if i5.abs() != 2 || k5.abs() != 2 {
                            place_leaf(w, i3 + i5, k1, j1 + k5);
                        }
                    }
                }
                for j5 in -1..=1 {
                    for l5 in -1..=1 {
                        place_leaf(w, i3 + j5, k1 + 1, j1 + l5);
                    }
                }
            }
        }
        true
    }
}

const DIRECTION_X: [i32; 4] = [0, -1, 0, 1];
const DIRECTION_Z: [i32; 4] = [1, 0, -1, 0];

fn worldgen_may_grow_on(b: i32) -> bool {
    matches!(nm(b), "dirt" | "grass_block" | "podzol" | "coarse_dirt" | "farmland")
}
fn worldgen_may_grow_through(b: i32) -> bool {
    matches!(nm(b), "dirt" | "grass_block" | "coarse_dirt") || conifer_may_replace(b)
}
fn worldgen_clearance<W: World + ?Sized>(w: &W, x: i32, y: i32, z: i32, tree_height: i32) -> bool {
    for dy in 0..=tree_height + 1 {
        let r = if dy == 0 { 0 } else if dy >= tree_height - 1 { 2 } else { 1 };
        for dx in -r..=r {
            for dz in -r..=r {
                if !worldgen_may_grow_through(w.get(x + dx, y + dy, z + dz)) {
                    return false;
                }
            }
        }
    }
    true
}
fn leaf_square<W: World + ?Sized>(w: &mut W, leaves: i32, x: i32, y: i32, z: i32, radius: i32, omit_corners: bool) {
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if omit_corners && dx.abs() == radius && dz.abs() == radius {
                continue;
            }
            worldgen_leaf(w, leaves, x + dx, y, z + dz);
        }
    }
}
fn worldgen_leaf<W: World + ?Sized>(w: &mut W, leaves: i32, x: i32, y: i32, z: i32) {
    if (MIN_Y..MAX_Y).contains(&y) && conifer_may_replace(w.get(x, y, z)) {
        w.set(x, y, z, leaves);
    }
}
fn worldgen_log<W: World + ?Sized>(w: &mut W, log: i32, x: i32, y: i32, z: i32) -> bool {
    if !conifer_may_replace(w.get(x, y, z)) {
        return false;
    }
    w.set(x, y, z, log);
    true
}

/// ObjectSavannaTreeWorldgen: vanilla-style acacia.
#[derive(Clone, Copy, Default)]
pub struct SavannaWorldgen;

impl SavannaWorldgen {
    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        let tree_height = 5 + rand.next_exclusive_int(3) + rand.next_exclusive_int(3);
        if y <= MIN_Y || y + tree_height + 1 >= MAX_Y {
            return false;
        }
        if !worldgen_may_grow_on(w.get(x, y - 1, z)) || !worldgen_clearance(w, x, y, z, tree_height) {
            return false;
        }
        if !matches!(nm(w.get(x, y - 1, z)), "dirt" | "coarse_dirt") {
            w.set(x, y - 1, z, id("dirt"));
        }
        let (log, leaves) = (axis_log("acacia_log", "y"), id("acacia_leaves"));
        let main_dir = rand.next_exclusive_int(4) as usize;
        let lean_height = 1 + rand.next_exclusive_int(4);
        let lean_steps = 1 + rand.next_exclusive_int(3);
        let lean_start = tree_height - lean_height;
        let (mut tx, mut tz, mut remaining) = (x, z, lean_steps);
        let mut last = None;
        for dy in 0..tree_height {
            if dy >= lean_start && remaining > 0 {
                tx += DIRECTION_X[main_dir];
                tz += DIRECTION_Z[main_dir];
                remaining -= 1;
            }
            if worldgen_log(w, log, tx, y + dy, tz) {
                last = Some((tx, y + dy, tz));
            }
        }
        let Some((lx, ly, lz)) = last else { return false };
        leaf_square(w, leaves, lx, ly + 1, lz, 1, false);
        worldgen_leaf(w, leaves, lx - 2, ly + 1, lz);
        worldgen_leaf(w, leaves, lx + 2, ly + 1, lz);
        worldgen_leaf(w, leaves, lx, ly + 1, lz - 2);
        worldgen_leaf(w, leaves, lx, ly + 1, lz + 2);
        leaf_square(w, leaves, lx, ly, lz, 3, true);
        // branch
        let branch_dir = rand.next_exclusive_int(4) as usize;
        if branch_dir == main_dir {
            return true;
        }
        let branch_pos = 1 + rand.next_exclusive_int(2);
        let mut branch_len = 1 + rand.next_exclusive_int(3);
        let mut by = tree_height - lean_height - branch_pos;
        let (mut bx, mut bz) = (x, z);
        let mut last = None;
        while by < tree_height && branch_len > 0 {
            if by > 0 {
                bx += DIRECTION_X[branch_dir];
                bz += DIRECTION_Z[branch_dir];
                if worldgen_log(w, log, bx, y + by, bz) {
                    last = Some((bx, y + by, bz));
                }
            }
            by += 1;
            branch_len -= 1;
        }
        if let Some((lx, ly, lz)) = last {
            leaf_square(w, leaves, lx, ly + 1, lz, 1, false);
            leaf_square(w, leaves, lx, ly, lz, 2, true);
        }
        true
    }
}

/// ObjectDarkOakTree: 2x2 dark oak with roof canopy.
#[derive(Clone, Copy, Default)]
pub struct DarkOak;

impl DarkOak {
    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        let i = rand.next_int_below(3) + rand.next_int_below(2) + 6;
        let vines = rand.next_int_below(TREE_WITH_VINES_CHANCE) == 0;
        if !(y >= 1 && y + i + 1 < 256) {
            return false;
        }
        if !can_grow_into(w.get(x, y - 1, z)) || !roof_space(w, x, y, z, i) {
            return false;
        }
        for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            set_dirt(w, x + dx, y - 1, z + dz);
        }
        let log = axis_log("dark_oak_log", "y");
        let leaves = id("dark_oak_leaves");
        let place_log = |w: &mut W, x, y, z, vines: bool| {
            if can_grow_into(w.get(x, y, z)) {
                w.set(x, y, z, log);
                if vines {
                    add_vines_around_log(w, x, y, z);
                }
            }
        };
        let place_leaf = |w: &mut W, x, y, z| {
            let b = w.get(x, y, z);
            if is_air(b) || nm(b) == "vine" {
                w.set(x, y, z, leaves);
            }
        };
        let face = Face::random(rand);
        let i1 = i - rand.next_int_below(4);
        let mut j1 = 2 - rand.next_int_below(3);
        let (mut k1, mut l1) = (x, z);
        let i2 = y + i - 1;
        for j2 in 0..i {
            if j2 >= i1 && j1 > 0 {
                k1 += face.dx();
                l1 += face.dz();
                j1 -= 1;
            }
            let k2 = y + j2;
            let b = w.get(k1, k2, l1);
            if can_grow_into(b) || is_leaves(b) {
                for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    place_log(w, k1 + dx, k2, l1 + dz, vines);
                }
            }
        }
        roof_canopy(w, rand, &place_leaf, k1, i2, l1);
        for k3 in -1..=2 {
            for j4 in -1..=2 {
                if (!(0..=1).contains(&k3) || !(0..=1).contains(&j4)) && rand.next_int_below(3) <= 0 {
                    let l4 = rand.next_int_below(3) + 2;
                    for i5 in 0..l4 {
                        place_log(w, x + k3, i2 - i5 - 1, z + j4, false);
                    }
                    roof_branch_leaves(w, &place_leaf, k1 + k3, i2, l1 + j4);
                }
            }
        }
        true
    }
}

/// ObjectDarkOakTree/ObjectPaleOakTree.placeTreeOfHeight.
pub(crate) fn roof_space<W: World + ?Sized>(w: &W, x: i32, y: i32, z: i32, height: i32) -> bool {
    for l in 0..=height + 1 {
        let i1 = if l >= height - 1 { 2 } else if l == 0 { 0 } else { 1 };
        for j1 in -i1..=i1 {
            for k1 in -i1..=i1 {
                if !can_grow_into(w.get(x + j1, y + l, z + k1)) {
                    return false;
                }
            }
        }
    }
    true
}

/// Dark/pale oak main canopy (consumes one boolean).
pub(crate) fn roof_canopy<W: World + ?Sized>(w: &mut W, rand: &mut Xoroshiro, leaf: &dyn Fn(&mut W, i32, i32, i32), k1: i32, i2: i32, l1: i32) {
    for i3 in -2..=0 {
        for l3 in -2..=0 {
            leaf(w, k1 + i3, i2 - 1, l1 + l3);
            leaf(w, 1 + k1 - i3, i2 - 1, l1 + l3);
            leaf(w, k1 + i3, i2 - 1, 1 + l1 - l3);
            leaf(w, 1 + k1 - i3, i2 - 1, 1 + l1 - l3);
            if (i3 > -2 || l3 > -1) && (i3 != -1 || l3 != -2) {
                leaf(w, k1 + i3, i2 + 1, l1 + l3);
                leaf(w, 1 + k1 - i3, i2 + 1, l1 + l3);
                leaf(w, k1 + i3, i2 + 1, 1 + l1 - l3);
                leaf(w, 1 + k1 - i3, i2 + 1, 1 + l1 - l3);
            }
        }
    }
    if rand.next_boolean() {
        leaf(w, k1, i2 + 2, l1);
        leaf(w, k1 + 1, i2 + 2, l1);
        leaf(w, k1 + 1, i2 + 2, l1 + 1);
        leaf(w, k1, i2 + 2, l1 + 1);
    }
    for j3 in -3..=4i32 {
        for i4 in -3..=4i32 {
            if (j3 != -3 || i4 != -3) && (j3 != -3 || i4 != 4) && (j3 != 4 || i4 != -3) && (j3 != 4 || i4 != 4) && (j3.abs() < 3 || i4.abs() < 3) {
                leaf(w, k1 + j3, i2, l1 + i4);
            }
        }
    }
}

/// Dark/pale oak side-branch leaves.
pub(crate) fn roof_branch_leaves<W: World + ?Sized>(w: &mut W, leaf: &dyn Fn(&mut W, i32, i32, i32), cx: i32, i2: i32, cz: i32) {
    for j5 in -1..=1 {
        for l2 in -1..=1 {
            leaf(w, cx + j5, i2, cz + l2);
        }
    }
    for k5 in -2..=2i32 {
        for l5 in -2..=2i32 {
            if k5.abs() != 2 || l5.abs() != 2 {
                leaf(w, cx + k5, i2 - 1, cz + l5);
            }
        }
    }
}

/// ObjectRoofedTreeWorldgen: vanilla-style dark oak, optionally with trunk vines.
#[derive(Clone, Copy, Default)]
pub struct RoofedWorldgen {
    pub vines: bool,
}

impl RoofedWorldgen {
    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        let tree_height = 6 + rand.next_exclusive_int(3) + rand.next_exclusive_int(2);
        if y <= MIN_Y || y + tree_height >= MAX_Y {
            return false;
        }
        if !worldgen_may_grow_on(w.get(x, y - 1, z)) || !worldgen_clearance(w, x, y, z, tree_height) {
            return false;
        }
        let dirt = id("dirt");
        for (dx, dz) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            if !matches!(nm(w.get(x + dx, y - 1, z + dz)), "dirt" | "coarse_dirt") {
                w.set(x + dx, y - 1, z + dz, dirt);
            }
        }
        let (log, leaves) = (axis_log("dark_oak_log", "y"), id("dark_oak_leaves"));
        let dir = rand.next_exclusive_int(4) as usize;
        let lean_height = rand.next_exclusive_int(4);
        let lean_steps = rand.next_exclusive_int(3);
        let lean_start = tree_height - lean_height;
        let (mut tx, mut tz, mut remaining) = (x, z, lean_steps);
        let mut placed = false;
        let place_vine = |w: &mut W, rand: &mut Xoroshiro, x, y, z, face: Face| {
            if rand.next_exclusive_int(7) >= 6 {
                return;
            }
            if is_air(w.get(x, y, z)) {
                w.set(x, y, z, vine(face.vine_meta()));
            }
        };
        for dy in 0..tree_height {
            if dy >= lean_start && remaining > 0 {
                tx += DIRECTION_X[dir];
                tz += DIRECTION_Z[dir];
                remaining -= 1;
            }
            let ty = y + dy;
            for ox in 0..2 {
                for oz in 0..2 {
                    let (bx, bz) = (tx + ox, tz + oz);
                    if !worldgen_log(w, log, bx, ty, bz) {
                        continue;
                    }
                    placed = true;
                    if self.vines {
                        // VINE_WEST = vine(EAST), VINE_EAST = vine(WEST), VINE_NORTH = vine(SOUTH), VINE_SOUTH = vine(NORTH)
                        if ox == 0 {
                            place_vine(w, rand, bx - 1, ty, bz, Face::East);
                        }
                        if ox == 1 {
                            place_vine(w, rand, bx + 1, ty, bz, Face::West);
                        }
                        if oz == 0 {
                            place_vine(w, rand, bx, ty, bz - 1, Face::South);
                        }
                        if oz == 1 {
                            place_vine(w, rand, bx, ty, bz + 1, Face::North);
                        }
                    }
                }
            }
        }
        if !placed {
            return false;
        }
        let top = y + tree_height - 1;
        let leaf = |w: &mut W, x, y, z| worldgen_leaf(w, leaves, x, y, z);
        // placeMainCanopy: same shape as the dark oak roof.
        roof_canopy(w, rand, &leaf, tx, top, tz);
        for ox in -1..=2 {
            for oz in -1..=2 {
                if (0..2).contains(&ox) && (0..2).contains(&oz) {
                    continue;
                }
                if rand.next_exclusive_int(3) != 0 {
                    continue;
                }
                let len = 2 + rand.next_exclusive_int(3);
                let by = top - 1;
                for index in 0..len {
                    worldgen_log(w, log, x + ox, by - index, z + oz);
                }
                leaf_square(w, leaves, tx + ox, by + 1, tz + oz, 1, false);
                leaf_square(w, leaves, tx + ox, by, tz + oz, 2, true);
            }
        }
        true
    }
}

// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Spruce-family objects: ObjectSmallSpruceTree, ObjectBigSpruceTree, ObjectMegaConiferTree, ObjectPineTree.

use super::*;

/// ObjectSmallSpruceTree: 6-9 tall spruce with stacked rings.
#[derive(Clone, Copy, Default)]
pub struct SmallSpruce;

impl SmallSpruce {
    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        let height = 6 + rand.next_int_below(4);
        let vines = rand.next_int_below(TREE_WITH_VINES_CHANCE) == 0;
        if y < 1 || y + height + 2 >= 256 {
            return false;
        }
        if !matches!(nm(w.get(x, y - 1, z)), "grass_block" | "dirt" | "podzol") {
            return false;
        }
        for dy in 0..=height + 1 {
            let r = if dy == 0 { 0 } else if dy >= height - 1 { 2 } else { 1 };
            for dx in -r..=r {
                for dz in -r..=r {
                    if !can_grow_into(w.get(x + dx, y + dy, z + dz)) {
                        return false;
                    }
                }
            }
        }
        let log = axis_log("spruce_log", "y");
        let leaves = id("spruce_leaves");
        let trunk = height - rand.next_int_below(3);
        for dy in 0..trunk {
            if can_grow_into(w.get(x, y + dy, z)) {
                w.set(x, y + dy, z, log);
            }
            if vines {
                add_vines_around_log(w, x, y + dy, z);
            }
        }
        let top = height - (1 + rand.next_int_below(2));
        let l_radius = 2 + rand.next_int_below(2);
        let mut radius = rand.next_int_below(2);
        let (mut max_r, mut min_r) = (1, 0);
        for yy in 0..=top {
            let yyy = y + height - yy;
            for xx in x - radius..=x + radius {
                for zz in z - radius..=z + radius {
                    if (xx - x).abs() == radius && (zz - z).abs() == radius && radius > 0 {
                        continue;
                    }
                    let b = w.get(xx, yyy, zz);
                    if !can_grow_into(b) {
                        continue;
                    }
                    if is_air(b) || matches!(nm(b), "snow_layer" | "vine") {
                        w.set(xx, yyy, zz, leaves);
                    }
                }
            }
            if radius >= max_r {
                radius = min_r;
                min_r = 1;
                max_r += 1;
                if max_r > l_radius {
                    max_r = l_radius;
                }
            } else {
                radius += 1;
            }
        }
        true
    }
}

const BIG_SPRUCE_FOLIAGES: [&[i32]; 4] =
    [&[1, 0, 0, 1, 2, 1, 1, 2, 3, 2, 2, 3, 4, 3], &[1, 0, 1, 2, 1, 2, 1, 1, 2, 3, 2, 2, 3, 4, 3], &[1, 2, 3], &[1, 2, 1, 3, 2, 4, 3]];

/// ObjectBigSpruceTree: 2x2 spruce 24-31 tall that turns the ground around it to podzol.
#[derive(Clone, Copy, Default)]
pub struct BigSpruce;

impl BigSpruce {
    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        let height = 24 + rand.next_int_below(8);
        let (mid_x, mid_z) = (x + 1, z + 1);
        if y < 1 || y + height + 5 >= 256 {
            return false;
        }
        let podzol = id("podzol");
        let rad = 6;
        for dx in -rad - 1..=rad {
            for dz in -rad - 1..=rad {
                let (cx, cz, cr) = (dx as f32 + 0.5, dz as f32 + 0.5, rad as f32 + 0.8);
                if cx * cx + cz * cz < cr * cr {
                    let (px, pz) = (mid_x + dx, mid_z + dz);
                    let py = w.height(px, pz) - 1;
                    if blocks::is_dirt(w.get(px, py, pz)) {
                        w.set(px, py, pz, podzol);
                    }
                }
            }
        }
        if !matches!(nm(w.get(x, y - 1, z)), "grass_block" | "dirt" | "podzol") {
            return false;
        }
        let radii = BIG_SPRUCE_FOLIAGES[rand.next_int_below(3) as usize];
        let log = id("spruce_log");
        let leaves = id("spruce_leaves");
        let leaf = |w: &mut W, x, y, z| {
            let b = w.get(x, y, z);
            if is_air(b) || nm(b) == "snow_layer" {
                w.set(x, y, z, leaves);
            }
        };
        for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            leaf(w, x + dx, y + height + 1, z + dz);
        }
        for dy in (0..=height).rev() {
            for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                if can_grow_into(w.get(x + dx, y + dy, z + dz)) {
                    w.set(x + dx, y + dy, z + dz, log);
                }
            }
            let index = (height - dy) as usize;
            if let Some(&radius) = radii.get(index) {
                for lx in -radius - 1..=radius {
                    for lz in -radius - 1..=radius {
                        let (cx, cz, cr) = (lx as f32 + 0.5, lz as f32 + 0.5, radius as f32 + 0.7);
                        if cx * cx + cz * cz < cr * cr {
                            leaf(w, mid_x + lx, y + dy, mid_z + lz);
                        }
                    }
                }
            }
        }
        true
    }
}

/// ObjectMegaConiferTree / ObjectPineTree `mayReplace`.
pub(crate) fn conifer_may_replace(b: i32) -> bool {
    is_air(b) || matches!(nm(b), "oak_leaves" | "spruce_leaves" | "birch_leaves" | "jungle_leaves" | "acacia_leaves" | "dark_oak_leaves")
}
pub(crate) fn mega_may_grow_on(b: i32) -> bool {
    matches!(nm(b), "grass_block" | "dirt" | "podzol" | "coarse_dirt")
}
pub(crate) fn mega_may_grow_through(b: i32) -> bool {
    matches!(nm(b), "dirt" | "grass_block" | "coarse_dirt") || conifer_may_replace(b)
}

/// 2x2-trunk clearance check shared by the mega trees.
pub(crate) fn mega_has_clearance<W: World + ?Sized>(w: &W, x: i32, y: i32, z: i32, tree_height: i32) -> bool {
    for dy in 0..=tree_height + 1 {
        let by = y + dy;
        if !(MIN_Y..MAX_Y).contains(&by) {
            return false;
        }
        let r = if dy == 0 { 1 } else { 2 };
        for dx in -r..=r {
            for dz in -r..=r {
                if !mega_may_grow_through(w.get(x + dx, by, z + dz)) {
                    return false;
                }
            }
        }
    }
    true
}

pub(crate) fn inside_core_layer(x: i32, z: i32, r2: i32) -> bool {
    x * x + z * z <= r2 || (x - 1) * (x - 1) + z * z <= r2 || x * x + (z - 1) * (z - 1) <= r2 || (x - 1) * (x - 1) + (z - 1) * (z - 1) <= r2
}

/// ObjectMegaConiferTree: 2x2 mega spruce with a podzol skirt.
#[derive(Clone, Copy)]
pub struct MegaConifer {
    pub canopy_height_min: i32,
    pub canopy_height_max: i32,
}

impl MegaConifer {
    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        let tree_height = 13 + rand.next_exclusive_int(3) + rand.next_exclusive_int(15);
        if y <= MIN_Y || y + tree_height >= MAX_Y {
            return false;
        }
        if !mega_may_grow_on(w.get(x, y - 1, z)) || !mega_has_clearance(w, x, y, z, tree_height) {
            return false;
        }
        let (log, leaves, podzol) = (id("spruce_log"), id("spruce_leaves"), id("podzol"));
        // canopy
        let top = y + tree_height;
        let canopy_height = self.canopy_height_min + rand.next_exclusive_int(self.canopy_height_max - self.canopy_height_min);
        let mut prev = 0;
        for oy in -canopy_height..=0 {
            let ly = top + oy;
            let pre = if oy == 0 { 0 } else { ((-oy) as f32 / canopy_height as f32 * 3.5f32).floor() as i32 };
            let mut radius = pre;
            if oy != 0 && pre == prev && ly & 1 == 0 {
                radius += 1;
            }
            if (MIN_Y..MAX_Y).contains(&ly) {
                let r2 = radius * radius;
                for dx in -radius..=1 + radius {
                    for dz in -radius..=1 + radius {
                        if inside_core_layer(dx, dz, r2) && conifer_may_replace(w.get(x + dx, ly, z + dz)) {
                            w.set(x + dx, ly, z + dz, leaves);
                        }
                    }
                }
            }
            prev = pre;
        }
        // trunk
        let place_log = |w: &mut W, x, y, z| {
            if conifer_may_replace(w.get(x, y, z)) {
                w.set(x, y, z, log);
            }
        };
        for dy in 0..tree_height - 1 {
            for (dx, dz) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                place_log(w, x + dx, y + dy, z + dz);
            }
        }
        place_log(w, x, y + tree_height - 1, z);
        // base footprint
        let by = y - 1;
        for (dx, dz) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            if nm(w.get(x + dx, by, z + dz)) != "podzol" {
                w.set(x + dx, by, z + dz, podzol);
            }
        }
        // base clusters
        let circle = |w: &mut W, cx: i32, cz: i32| {
            for dx in -2..=2i32 {
                for dz in -2..=2i32 {
                    if dx.abs() == 2 && dz.abs() == 2 {
                        continue;
                    }
                    for oy in (-3..=2).rev() {
                        let b = w.get(cx + dx, by + oy, cz + dz);
                        let n = nm(b);
                        if n == "podzol" {
                            break;
                        }
                        if matches!(n, "grass_block" | "dirt" | "coarse_dirt") {
                            w.set(cx + dx, by + oy, cz + dz, podzol);
                            break;
                        }
                    }
                }
            }
        };
        circle(w, x - 1, z - 1);
        circle(w, x + 2, z - 1);
        circle(w, x - 1, z + 2);
        circle(w, x + 2, z + 2);
        for _ in 0..5 {
            let v = rand.next_exclusive_int(64);
            let (q, rem) = (v / 8, v % 8);
            if q == 0 || q == 7 || rem == 0 || rem == 7 {
                circle(w, x + rem - 3, z + q - 3);
            }
        }
        true
    }
}

/// ObjectPineTree: thin 7-11 tall spruce with a small cap.
#[derive(Clone, Copy, Default)]
pub struct Pine;

impl Pine {
    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        let tree_height = 7 + rand.next_exclusive_int(5);
        if y <= MIN_Y || y + tree_height >= MAX_Y {
            return false;
        }
        if !matches!(nm(w.get(x, y - 1, z)), "dirt" | "grass_block" | "podzol" | "dirt_with_roots" | "moss_block" | "snow" | "coarse_dirt") {
            return false;
        }
        for dy in 0..=tree_height + 1 {
            let r = if dy == 0 { 0 } else if dy >= tree_height - 1 { 2 } else { 1 };
            for dx in -r..=r {
                for dz in -r..=r {
                    if !conifer_may_replace(w.get(x + dx, y + dy, z + dz)) {
                        return false;
                    }
                }
            }
        }
        let (log, leaves) = (id("spruce_log"), id("spruce_leaves"));
        let mut last = None;
        for dy in 0..tree_height {
            if conifer_may_replace(w.get(x, y + dy, z)) {
                w.set(x, y + dy, z, log);
                last = Some(y + dy);
            }
        }
        let Some(last) = last else { return false };
        // canopy
        let cy = last + 1;
        let canopy_height = 3 + rand.next_exclusive_int(2);
        let target = 1 + rand.next_exclusive_int(canopy_height + 1);
        let mut radius: i32 = 0;
        let mut placed = false;
        let mut ry = 1;
        while ry > -canopy_height {
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    if radius > 0 && dx.abs() == radius && dz.abs() == radius {
                        continue;
                    }
                    if conifer_may_replace(w.get(x + dx, cy + ry, z + dz)) {
                        w.set(x + dx, cy + ry, z + dz, leaves);
                        placed = true;
                    }
                }
            }
            let mut next = if radius >= target { radius } else { radius + 1 };
            if ry == 2 - canopy_height && radius > 0 {
                next = radius - 1;
            }
            radius = next;
            ry -= 1;
        }
        if !placed {
            return false;
        }
        if !matches!(nm(w.get(x, y - 1, z)), "dirt" | "coarse_dirt") {
            w.set(x, y - 1, z, id("dirt"));
        }
        true
    }
}

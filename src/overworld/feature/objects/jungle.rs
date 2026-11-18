// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Jungle objects: ObjectJungleTree, ObjectJungleBigTree (HugeTreesGenerator), ObjectJungleBush,
//! ObjectMegaJungleTree.

use super::spruce::{inside_core_layer, mega_has_clearance, mega_may_grow_on, conifer_may_replace};
use super::*;

/// ObjectJungleTree: small jungle tree with vines and cocoa.
#[derive(Clone, Copy)]
pub struct Jungle {
    pub min_tree_height: i32,
    pub max_tree_height: i32,
}

impl Jungle {
    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        let i = rand.next_int_below(self.max_tree_height) + self.min_tree_height;
        let vines = rand.next_int_below(TREE_WITH_VINES_CHANCE) == 0;
        if !(y >= MIN_Y && y + i + 1 < MAX_Y) {
            return false;
        }
        for j in y..=y + 1 + i {
            let k = if j >= y + 1 + i - 2 { 2 } else if j == y { 0 } else { 1 };
            for l in x - k..=x + k {
                for i1 in z - k..=z + k {
                    if !(MIN_Y..MAX_Y).contains(&j) || !can_grow_into(w.get(l, j, i1)) {
                        return false;
                    }
                }
            }
        }
        if !(matches!(nm(w.get(x, y - 1, z)), "grass_block" | "dirt" | "farmland") && y < MAX_Y - i - 1) {
            return false;
        }
        let wood = axis_log("jungle_log", "y");
        let leaves = id("jungle_leaves");
        set_dirt(w, x, y - 1, z);
        for i3 in y - 3 + i..=y + i {
            let i4 = i3 - (y + i);
            let j1 = 1 - i4 / 2;
            for k1 in x - j1..=x + j1 {
                let l1 = k1 - x;
                for i2 in z - j1..=z + j1 {
                    let j2 = i2 - z;
                    if l1.abs() != j1 || j2.abs() != j1 || rand.next_int_below(2) != 0 && i4 != 0 {
                        let b = w.get(k1, i3, i2);
                        if is_air(b) || is_leaves(b) || nm(b) == "vine" {
                            w.set(k1, i3, i2, leaves);
                        }
                    }
                }
            }
        }
        for j3 in 0..i {
            let b = w.get(x, y + j3, z);
            if is_air(b) || is_leaves(b) || nm(b) == "vine" {
                w.set(x, y + j3, z, wood);
                if j3 > 0 {
                    if vines {
                        add_vines_around_log(w, x, y + j3, z);
                    } else {
                        for (dx, dz, m) in [(-1, 0, 8), (1, 0, 2), (0, -1, 1), (0, 1, 4)] {
                            if rand.next_int_below(3) > 0 && is_air(w.get(x + dx, y + j3, z + dz)) {
                                w.set(x + dx, y + j3, z + dz, vine(m));
                            }
                        }
                    }
                }
            }
        }
        for k3 in y - 3 + i..=y + i {
            let j4 = k3 - (y + i);
            let k4 = 2 - j4 / 2;
            for l4 in x - k4..=x + k4 {
                for i5 in z - k4..=z + k4 {
                    if is_leaves(w.get(l4, k3, i5)) {
                        for (dx, dz, m) in [(-1, 0, 8), (1, 0, 2), (0, -1, 1), (0, 1, 4)] {
                            if rand.next_int_below(4) == 0 && is_air(w.get(l4 + dx, k3, i5 + dz)) {
                                hanging_vine(w, l4 + dx, k3, i5 + dz, m);
                            }
                        }
                    }
                }
            }
        }
        if rand.next_int_below(5) == 0 && i > 5 {
            for l3 in 0..2 {
                for face in Face::PLANE {
                    if rand.next_int_below(4 - l3) == 0 {
                        let o = face.opposite();
                        let age = rand.next_int_below(2);
                        let cocoa = blocks::with(id("cocoa"), &[("age", blocks::i(age)), ("direction", blocks::i(face.horizontal_index()))]);
                        w.set(x + o.dx(), y + i - 5 + l3, z + o.dz(), cocoa);
                    }
                }
            }
        }
        true
    }
}

/// ObjectJungleTree.addHangingVine.
fn hanging_vine<W: World + ?Sized>(w: &mut W, x: i32, mut y: i32, z: i32, meta: i32) {
    w.set(x, y, z, vine(meta));
    y -= 1;
    let mut i = 4;
    while i > 0 && is_air(w.get(x, y, z)) {
        w.set(x, y, z, vine(meta));
        y -= 1;
        i -= 1;
    }
}

/// ObjectJungleBigTree: 2x2 jungle giant (PNX HugeTreesGenerator).
#[derive(Clone, Copy)]
pub struct JungleBig {
    pub base_height: i32,
    pub extra_random_height: i32,
}

impl JungleBig {
    fn leaves_layer<W: World + ?Sized>(w: &mut W, leaves: i32, cx: i32, cy: i32, cz: i32, width: i32, strict: bool) {
        let i = width * width;
        let hi = if strict { width + 1 } else { width };
        for j in -width..=hi {
            for k in -width..=hi {
                let (l, i1) = (j - 1, k - 1);
                let inside = if strict { j * j + k * k <= i || l * l + i1 * i1 <= i || j * j + i1 * i1 <= i || l * l + k * k <= i } else { j * j + k * k <= i };
                if inside {
                    let b = w.get(cx + j, cy, cz + k);
                    if is_air(b) || is_leaves(b) {
                        w.set(cx + j, cy, cz + k, leaves);
                    }
                }
            }
        }
    }

    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        let mut height = rand.next_int_below(3) + self.base_height;
        if self.extra_random_height > 1 {
            height += rand.next_int_below(self.extra_random_height);
        }
        // ensureGrowable: isSpaceAt && ensureDirtsUnderneath
        if !(y >= 1 && y + height < 256) {
            return false;
        }
        for i in 0..=1 + height {
            let j = if i == 0 { 1 } else { 2 };
            for k in -j..=j {
                for l in -j..=j {
                    if y + i < 0 || y + i >= 256 || !can_grow_into(w.get(x + k, y + i, z + l)) {
                        return false;
                    }
                }
            }
        }
        if !(matches!(nm(w.get(x, y - 1, z)), "grass_block" | "dirt") && y >= 2) {
            return false;
        }
        for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            set_dirt(w, x + dx, y - 1, z + dz);
        }
        let wood = axis_log("jungle_log", "y");
        let leaves = id("jungle_leaves");
        for j in -2..=0 {
            Self::leaves_layer(w, leaves, x, y + height + j, z, 2 + 1 - j, true);
        }
        let mut j = y + height - 2 - rand.next_int_below(4);
        while j as f64 > y as f64 + height as f64 / 2.0 {
            let f = rand.next_float() * (std::f32::consts::PI * 2.0f32);
            let (mut k, mut l);
            for i1 in 0..5 {
                k = (x as f64 + (1.5f32 + mh_cos(f) * i1 as f32) as f64) as i32;
                l = (z as f64 + (1.5f32 + mh_sin(f) * i1 as f32) as f64) as i32;
                w.set(k, j - 3 + i1 / 2, l, wood);
            }
            k = (x as f64 + (1.5f32 + mh_cos(f) * 4.0) as f64) as i32;
            l = (z as f64 + (1.5f32 + mh_sin(f) * 4.0) as f64) as i32;
            let j2 = 1 + rand.next_int_below(2);
            for k1 in j - j2..=j {
                Self::leaves_layer(w, leaves, k, k1, l, 1 - (k1 - j), false);
            }
            j -= 2 + rand.next_int_below(4);
        }
        let place_vine = |w: &mut W, rand: &mut Xoroshiro, x, y, z, meta| {
            if rand.next_int_below(3) > 0 && is_air(w.get(x, y, z)) {
                w.set(x, y, z, vine(meta));
            }
        };
        for i2 in 0..height {
            let by = y + i2;
            if can_grow_into(w.get(x, by, z)) {
                w.set(x, by, z, wood);
                if i2 > 0 {
                    place_vine(w, rand, x - 1, by, z, 8);
                    place_vine(w, rand, x, by, z - 1, 1);
                }
            }
            if i2 < height - 1 {
                if can_grow_into(w.get(x + 1, by, z)) {
                    w.set(x + 1, by, z, wood);
                    if i2 > 0 {
                        place_vine(w, rand, x + 2, by, z, 2);
                        place_vine(w, rand, x + 1, by, z - 1, 1);
                    }
                }
                if can_grow_into(w.get(x + 1, by, z + 1)) {
                    w.set(x + 1, by, z + 1, wood);
                    if i2 > 0 {
                        place_vine(w, rand, x + 2, by, z + 1, 2);
                        place_vine(w, rand, x + 1, by, z + 2, 4);
                    }
                }
                if can_grow_into(w.get(x, by, z + 1)) {
                    w.set(x, by, z + 1, wood);
                    if i2 > 0 {
                        place_vine(w, rand, x - 1, by, z + 1, 8);
                        place_vine(w, rand, x, by, z + 2, 4);
                    }
                }
            }
        }
        true
    }
}

/// ObjectJungleBush: one log under a small leaf dome.
#[derive(Clone, Copy, Default)]
pub struct JungleBush;

impl JungleBush {
    pub fn generate<W: World + ?Sized>(&self, w: &mut W, _rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        w.set(x, y, z, axis_log("jungle_log", "y"));
        let leaves = id("jungle_leaves");
        for dy in -2..=1 {
            let r = 2 - dy;
            for dx in -r..=r {
                for dz in -r..=r {
                    if dx * dx + dz * dz <= r * r {
                        let b = w.get(x + dx, y + dy, z + dz);
                        if is_air(b) || blocks::replaceable(b) || is_leaves(b) {
                            w.set(x + dx, y + dy, z + dz, leaves);
                        }
                    }
                }
            }
        }
        true
    }
}

/// ObjectMegaJungleTree: 2x2 mega jungle tree with branches and trunk vines.
#[derive(Clone, Copy, Default)]
pub struct MegaJungle;

impl MegaJungle {
    fn canopy<W: World + ?Sized>(w: &mut W, leaves: i32, x: i32, y: i32, z: i32, height: i32, base_radius: i32, core_width: i32, simplified: bool) {
        for countdown in (0..height).rev() {
            let ly = y - countdown;
            let radius = base_radius + core_width / 2 + countdown;
            let r2 = radius * radius;
            let max = if simplified { radius } else { core_width - 1 + radius };
            for dx in -radius..=max {
                for dz in -radius..=max {
                    let inside = if simplified { dx * dx + dz * dz <= r2 } else { inside_core_layer(dx, dz, r2) };
                    if inside && (MIN_Y..MAX_Y).contains(&ly) && conifer_may_replace(w.get(x + dx, ly, z + dz)) {
                        w.set(x + dx, ly, z + dz, leaves);
                    }
                }
            }
        }
    }

    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        let tree_height = 10 + rand.next_exclusive_int(3) + rand.next_exclusive_int(20);
        if y <= MIN_Y || y + tree_height >= MAX_Y {
            return false;
        }
        if !mega_may_grow_on(w.get(x, y - 1, z)) || !mega_has_clearance(w, x, y, z, tree_height) {
            return false;
        }
        let log = axis_log("jungle_log", "y");
        let leaves = id("jungle_leaves");
        Self::canopy(w, leaves, x, y + tree_height, z, 3, 2, 2, false);
        // branches
        let mut branch_y = y + tree_height - (2 + rand.next_exclusive_int(4));
        let min_branch_y = y + (tree_height as f32 * 0.5f32) as i32;
        while branch_y > min_branch_y {
            let mut angle = rand.next_float() * std::f32::consts::PI;
            angle += angle;
            let base_y = ((branch_y - 1) as f32 - 4.0 * 0.5f32) as i32;
            let (mut bx, mut bz) = (x, z);
            for index in 0..5 {
                let d = index as f32;
                bx = x + (mh_cos(angle) * d + 1.5f32) as i32;
                bz = z + (mh_sin(angle) * d + 1.5f32) as i32;
                let ly = (base_y as f32 + d * 0.5f32) as i32;
                if conifer_may_replace(w.get(bx, ly, bz)) {
                    w.set(bx, ly, bz, log);
                }
            }
            let ch = 2 + rand.next_exclusive_int(2);
            Self::canopy(w, leaves, bx, branch_y, bz, ch, 1, 0, true);
            branch_y -= 2 + rand.next_exclusive_int(4);
        }
        // trunk
        let place_log = |w: &mut W, x, y, z| {
            if !conifer_may_replace(w.get(x, y, z)) {
                return false;
            }
            w.set(x, y, z, log);
            true
        };
        let trunk_vine = |w: &mut W, rand: &mut Xoroshiro, x, y, z, face: Face| {
            if rand.next_exclusive_int(3) != 0 {
                return;
            }
            if is_air(w.get(x, y, z)) {
                w.set(x, y, z, vine(face.vine_meta()));
            }
        };
        for (dx, dz) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            place_log(w, x + dx, y, z + dz);
        }
        for dy in 1..tree_height - 1 {
            let ty = y + dy;
            // VINE_WEST = vine(EAST) etc.
            if place_log(w, x, ty, z) {
                trunk_vine(w, rand, x - 1, ty, z, Face::East);
                trunk_vine(w, rand, x, ty, z - 1, Face::South);
            }
            if place_log(w, x, ty, z + 1) {
                trunk_vine(w, rand, x - 1, ty, z + 1, Face::East);
                trunk_vine(w, rand, x, ty, z + 2, Face::North);
            }
            if place_log(w, x + 1, ty, z) {
                trunk_vine(w, rand, x + 2, ty, z, Face::West);
                trunk_vine(w, rand, x + 1, ty, z - 1, Face::South);
            }
            if place_log(w, x + 1, ty, z + 1) {
                trunk_vine(w, rand, x + 2, ty, z + 1, Face::West);
                trunk_vine(w, rand, x + 1, ty, z + 2, Face::North);
            }
        }
        place_log(w, x, y + tree_height - 1, z);
        let dirt = id("dirt");
        for (dx, dz) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            if nm(w.get(x + dx, y - 1, z + dz)) != "dirt" {
                w.set(x + dx, y - 1, z + dz, dirt);
            }
        }
        true
    }
}

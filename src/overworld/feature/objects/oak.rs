// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Oak-like objects: ObjectFancyOakTree, ObjectSwampOakTree, ObjectAzaleaTree, ObjectCherryTree,
//! ObjectPaleOakTree, ObjectSmallPaleOakTree.

use super::savanna::{roof_branch_leaves, roof_canopy, roof_space};
use super::*;
use crate::overworld::rng::NukkitRandom;

type V = (i32, i32, i32);

/// ObjectFancyOakTree: big branching oak.
#[derive(Clone, Copy)]
pub struct FancyOak {
    pub base_height: i32,
    pub height_rand_a: i32,
    pub height_rand_b: i32,
}

impl Default for FancyOak {
    fn default() -> Self {
        Self { base_height: 3, height_rand_a: 11, height_rand_b: 0 }
    }
}

impl FancyOak {
    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        let tree_height = self.base_height + rand.next_int_below(self.height_rand_a + 1) + rand.next_int_below(self.height_rand_b + 1);
        let height = tree_height + 2;
        if y < MIN_Y + 1 || y + height + 1 >= MAX_Y {
            return false;
        }
        if !matches!(nm(w.get(x, y - 1, z)), "grass_block" | "dirt") {
            return false;
        }
        let trunk_height = floor(height as f64 * 0.618);
        let clusters = 1.min(floor(1.382 + (height as f64 / 13.0).powf(2.0)));
        let trunk_top = y + trunk_height;
        let mut rel = height - 5;
        let mut foliage: Vec<(V, i32)> = vec![((x, y + rel, z), trunk_top)];
        while rel >= 0 {
            let shape = tree_shape(height, rel);
            if shape >= 0.0 {
                for _ in 0..clusters {
                    let radius = shape as f64 * (rand.next_float() as f64 + 0.328);
                    let angle = (rand.next_float() * 2.0f32) as f64 * std::f64::consts::PI;
                    let fx = radius * angle.sin() + 0.5;
                    let fz = radius * angle.cos() + 0.5;
                    let cs = (x + floor(fx), y + rel - 1, z + floor(fz));
                    let ce = (cs.0, cs.1 + 5, cs.2);
                    if make_limb(w, cs, ce, false) {
                        let (dx, dz) = (x - cs.0, z - cs.2);
                        let bh = cs.1 as f64 - ((dx * dx + dz * dz) as f64).sqrt() * 0.381;
                        let top = if bh > trunk_top as f64 { trunk_top } else { bh as i32 };
                        if make_limb(w, (x, top, z), cs, false) {
                            foliage.push((cs, top));
                        }
                    }
                }
            }
            rel -= 1;
        }
        set_dirt(w, x, y - 1, z);
        make_limb(w, (x, y, z), (x, y + trunk_height, z), true);
        for &(pos, base) in &foliage {
            let bc = (x, base, z);
            if bc != pos && trim(height, base - y) {
                make_limb(w, bc, pos, true);
            }
        }
        let leaves = id("oak_leaves");
        for &(pos, base) in &foliage {
            if trim(height, base - y) {
                for yo in (0..=4).rev() {
                    let r = 2 + if yo != 4 && yo != 0 { 1 } else { 0 };
                    for dx in -r..=r {
                        for dz in -r..=r {
                            let (fx, fz) = (dx as f32 + 0.5, dz as f32 + 0.5);
                            if fx * fx + fz * fz > (r * r) as f32 {
                                continue;
                            }
                            let b = w.get(pos.0 + dx, pos.1 + yo, pos.2 + dz);
                            if is_air(b) || is_leaves(b) {
                                w.set(pos.0 + dx, pos.1 + yo, pos.2 + dz, leaves);
                            }
                        }
                    }
                }
            }
        }
        true
    }
}

fn trim(height: i32, local_y: i32) -> bool {
    local_y as f64 >= height as f64 * 0.2
}

fn tree_shape(height: i32, y: i32) -> f32 {
    if (y as f32) < height as f32 * 0.3f32 {
        return -1.0;
    }
    let radius = height as f32 / 2.0;
    let adjacent = radius - y as f32;
    let mut distance = (radius * radius - adjacent * adjacent).sqrt();
    if adjacent == 0.0 {
        distance = radius;
    } else if adjacent.abs() >= radius {
        return 0.0;
    }
    distance * 0.5
}

fn make_limb<W: World + ?Sized>(w: &mut W, s: V, e: V, place: bool) -> bool {
    if !place && s == e {
        return true;
    }
    let d = (e.0 - s.0, e.1 - s.1, e.2 - s.2);
    let steps = d.0.abs().max(d.1.abs().max(d.2.abs()));
    if steps == 0 {
        return true;
    }
    let (dx, dy, dz) = (d.0 as f32 / steps as f32, d.1 as f32 / steps as f32, d.2 as f32 / steps as f32);
    let logs = if place { [axis_log("oak_log", "x"), axis_log("oak_log", "y"), axis_log("oak_log", "z")] } else { [0; 3] };
    for i in 0..=steps {
        let f = i as f32;
        let p = (
            s.0 + floor((0.5f32 + f * dx) as f64),
            s.1 + floor((0.5f32 + f * dy) as f64),
            s.2 + floor((0.5f32 + f * dz) as f64),
        );
        if place {
            if can_grow_into(w.get(p.0, p.1, p.2)) {
                let (xd, zd) = ((p.0 - s.0).abs(), (p.2 - s.2).abs());
                let m = xd.max(zd);
                let axis = if m > 0 { if xd == m { 0 } else { 2 } } else { 1 };
                w.set(p.0, p.1, p.2, logs[axis]);
            }
        } else if !can_grow_into(w.get(p.0, p.1, p.2)) {
            return false;
        }
    }
    true
}

/// ObjectSwampOakTree: oak with hanging vines that can root in shallow water.
#[derive(Clone, Copy)]
pub struct SwampOak {
    pub min_tree_height: i32,
    pub max_tree_height: i32,
}

const SEA_LEVEL: i32 = 63;

impl SwampOak {
    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        let mut height = rand.next_int_below(self.max_tree_height - self.min_tree_height + 1) + self.min_tree_height;
        if y < MIN_Y || y + height + 1 >= MAX_Y {
            return false;
        }
        if !blocks::is_dirt(w.get(x, y - 1, z)) {
            return false;
        }
        set_dirt(w, x, y - 1, z);
        let (wood, leaves) = (axis_log("oak_log", "y"), id("oak_leaves"));
        let mut dy = 0;
        while dy < height {
            let b = w.get(x, y + dy, z);
            let n = nm(b);
            if is_air(b) || matches!(n, "oak_leaves" | "vine" | "water") {
                w.set(x, y + dy, z, wood);
                if n == "water" {
                    height += 1;
                }
            }
            dy += 1;
        }
        for yy in y - 3 + height..=y + height {
            let y_off = (yy - (y + height)) as f64;
            let mid = (1.0 - y_off / 2.0) as i32;
            for xx in x - mid..=x + mid {
                for zz in z - mid..=z + mid {
                    if (xx - x).abs() == mid && (zz - z).abs() == mid && (y_off == 0.0 || rand.next_int_below(2) == 0) {
                        continue;
                    }
                    if !blocks::is_solid(w.get(xx, yy, zz)) {
                        w.set(xx, yy, zz, leaves);
                        if rand.next_int_below(4) == 0 {
                            let meta = [1, 2, 4, 8][rand.next_int_below(3) as usize];
                            // TODO: PNX uses Math.random() here; we hash the position instead, so only vine length differs.
                            let h = (xx.wrapping_mul(73_856_093) ^ yy.wrapping_mul(19_349_663) ^ zz.wrapping_mul(83_492_791)) as u32;
                            let length = 3 + (h % 3) as i32;
                            let mut vy = yy - 1;
                            for _ in 0..length {
                                if is_air(w.get(xx, vy, zz)) && vy > SEA_LEVEL {
                                    w.set(xx, vy, zz, vine(meta));
                                    vy -= 1;
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
        true
    }
}

/// ObjectAzaleaTree: short oak trunk on rooted dirt with (flowering) azalea leaves.
#[derive(Clone, Copy, Default)]
pub struct Azalea;

impl Azalea {
    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        let i = rand.next_int_below(2) + 2;
        let (j, k, l) = (x, y, z);
        let i2 = k + i;
        if !(k >= -63 && k + i + 2 < 320) {
            return false;
        }
        let log = axis_log("oak_log", "y");
        for il in 0..i + 1 {
            let b = w.get(j, il + k, l);
            if is_air(b) || matches!(nm(b), "azalea_leaves" | "azalea_leaves_flowered") {
                w.set(j, il + k, l, log);
            }
        }
        w.set(x, y - 1, z, id("dirt_with_roots"));
        let (flowered, plain) = (id("azalea_leaves_flowered"), id("azalea_leaves"));
        let leaf = |w: &mut W, rand: &mut Xoroshiro, x, y, z| {
            if is_air(w.get(x, y, z)) {
                w.set(x, y, z, if rand.next_int_below(3) == 1 { flowered } else { plain });
            }
        };
        for i3 in -2..=1 {
            for l3 in -2..=1 {
                let (ox, oy, oz) = (rand.next_int_range(0, 1), rand.next_int_range(0, 1), rand.next_int_range(0, 1));
                for (sx, sz) in [(1, 1), (-1, 1), (1, -1), (-1, -1)] {
                    leaf(w, rand, j + sx * i3 + ox, i2 + 1 + oy, l + sz * l3 + oz);
                }
                for k4 in [0, 1] {
                    for (sx, sz) in [(1, 1), (-1, 1), (1, -1), (-1, -1)] {
                        leaf(w, rand, j + sx * i3, i2 + k4, l + sz * l3);
                    }
                }
                let (ox, oy, oz) = (rand.next_int_range(-1, 0), rand.next_int_range(-1, 0), rand.next_int_range(-1, 0));
                for (sx, sz) in [(1, 1), (-1, 1), (1, -1), (-1, -1)] {
                    leaf(w, rand, j + sx * i3 + ox, i2 + 2 + oy, l + sz * l3 + oz);
                }
            }
        }
        true
    }
}

/// ObjectCherryTree: forked (big) or bent (small) cherry tree.
#[derive(Clone, Copy, Default)]
pub struct Cherry;

struct CherryIds {
    y: i32,
    x: i32,
    z: i32,
    leaves: i32,
}

impl Cherry {
    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        let ids = CherryIds { y: axis_log("cherry_log", "y"), x: axis_log("cherry_log", "x"), z: axis_log("cherry_log", "z"), leaves: id("cherry_leaves") };
        if rand.next_boolean() && Self::big(w, rand, &ids, x, y, z) {
            return true;
        }
        Self::small(w, rand, &ids, x, y, z)
    }

    /// ObjectCherryTree.canPlaceObject.
    pub fn can_place_object<W: World + ?Sized>(w: &W, tree_height: i32, x: i32, y: i32, z: i32) -> bool {
        let mut r = 0;
        for yy in 0..tree_height + 3 {
            if yy == 1 || yy == tree_height {
                r += 1;
            }
            for xx in -r..=r {
                for zz in -r..=r {
                    if !can_grow_into(w.get(x + xx, y + yy, z + zz)) {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn put<W: World + ?Sized>(w: &mut W, x: i32, y: i32, z: i32, b: i32) {
        if can_grow_into(w.get(x, y, z)) {
            w.set(x, y, z, b);
        }
    }

    fn big<W: World + ?Sized>(w: &mut W, rand: &mut Xoroshiro, ids: &CherryIds, x: i32, y: i32, z: i32) -> bool {
        let main = (if rand.next_boolean() { 1 } else { 0 }) + 10;
        if !Self::can_place_object(w, main, x, y, z) {
            return false;
        }
        let mut on_x = rand.next_boolean();
        let (mut xm, mut zm) = if on_x { (1, 0) } else { (0, 1) };
        let ll = rand.next_int_range(2, 4);
        let lh = rand.next_int_range(3, 5);
        let ls = rand.next_int_range(4, 5);
        if !Self::can_place_object(w, lh, x - ll * xm, y + ls, z - ll * zm) {
            on_x = !on_x;
            (xm, zm) = if on_x { (1, 0) } else { (0, 1) };
            if !Self::can_place_object(w, lh, x - ll * xm, y + ls, z - ll * zm) {
                return false;
            }
        }
        let rl = rand.next_int_range(2, 4);
        let rh = rand.next_int_range(3, 5);
        let rs = rand.next_int_range(4, 5);
        if !Self::can_place_object(w, rh, x + rl * xm, y + rs, z + rl * zm) {
            return false;
        }
        set_dirt(w, x, y - 1, z);
        for yy in 0..main {
            w.set(x, y + yy, z, ids.y);
        }
        let side = if on_x { ids.x } else { ids.z };
        // left (-1) then right (+1) side trunk
        for (sign, len, height, start) in [(-1, ll, lh, ls), (1, rl, rh, rs)] {
            for xx in 1..=len {
                Self::put(w, x + sign * xx * xm, y + start, z + sign * xx * zm, side);
            }
            let (ex, ez) = (x + sign * len * xm, z + sign * len * zm);
            for yy in 1..height {
                Self::put(w, ex, y + start + yy, ez, ids.y);
            }
            if start == 4 {
                w.set(ex, y + start, ez, air());
                let (tx, ty, tz) = (ex - sign * xm, y + start + 1, ez - sign * zm);
                Self::put(w, tx, ty, tz, ids.y);
                Self::put(w, tx + sign * xm, ty, tz + sign * zm, side);
            }
        }
        Self::leaves(w, rand, ids.leaves, x, y + main + 1, z);
        Self::leaves(w, rand, ids.leaves, x - ll * xm, y + ls + lh + 1, z - ll * zm);
        Self::leaves(w, rand, ids.leaves, x + rl * xm, y + rs + rh + 1, z + rl * zm);
        true
    }

    fn small<W: World + ?Sized>(w: &mut W, rand: &mut Xoroshiro, ids: &CherryIds, x: i32, y: i32, z: i32) -> bool {
        let main = (if rand.next_boolean() { 1 } else { 0 }) + 4;
        let side_h = rand.next_int_range(3, 5);
        if !Self::can_place_object(w, main + 1, x, y, z) {
            return false;
        }
        let mut dir = rand.next_int_range(0, 3);
        let (mut xm, mut zm) = (0, 0);
        let mut ok = false;
        for _ in 0..4 {
            dir = (dir + 1) % 4;
            xm = match dir {
                0 => -1,
                1 => 1,
                _ => 0,
            };
            zm = match dir {
                2 => -1,
                3 => 1,
                _ => 0,
            };
            if Self::can_place_object(w, side_h, x + xm * side_h, y, z + zm * side_h) {
                ok = true;
                break;
            }
        }
        if !ok {
            return false;
        }
        let side = if xm == 0 { ids.z } else { ids.x };
        for yy in 0..main {
            Self::put(w, x, y + yy, z, ids.y);
        }
        for yy in 1..=side_h {
            let (tx, ty, tz) = (x + yy * xm, y + main + yy - 2, z + yy * zm);
            Self::put(w, tx, ty, tz, side);
            if yy == side_h - 1 && side_h > 3 {
                continue;
            }
            Self::put(w, tx, ty + 1, tz, ids.y);
        }
        Self::leaves(w, rand, ids.leaves, x + side_h * xm, y + main + side_h, z + side_h * zm);
        true
    }

    fn leaves<W: World + ?Sized>(w: &mut W, rand: &mut Xoroshiro, leaves: i32, x: i32, y: i32, z: i32) {
        for dy in -2..=2i32 {
            for dx in -4..=4 {
                for dz in -4..=4 {
                    let r = 4 - dy.abs().max(1);
                    if dx * dx + dz * dz > r * r {
                        continue;
                    }
                    let b = w.get(x + dx, y + dy, z + dz);
                    if is_air(b) || is_leaves(b) {
                        w.set(x + dx, y + dy, z + dz, leaves);
                    }
                    if dy == -2 && rand.next_int_range(0, 2) == 0 {
                        let b = w.get(x + dx, y + dy - 1, z + dz);
                        if is_air(b) || is_leaves(b) {
                            w.set(x + dx, y + dy - 1, z + dz, leaves);
                        }
                    }
                }
            }
        }
    }
}

fn hanging_moss(tip: bool) -> i32 {
    blocks::with(id("pale_hanging_moss"), &[("tip", blocks::b(tip))])
}

/// ObjectPaleOakTree: dark-oak-shaped pale oak with hanging moss, optionally one creaking heart.
#[derive(Clone, Copy, Default)]
pub struct PaleOak {
    pub try_creaking_heart: bool,
}

impl PaleOak {
    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        let mut try_heart = self.try_creaking_heart;
        let i = rand.next_int_below(1) + rand.next_int_below(1) + 6;
        if !(y >= 1 && y + i + 1 < 256) {
            return false;
        }
        if !matches!(nm(w.get(x, y - 1, z)), "grass_block" | "dirt") || !roof_space(w, x, y, z, i) {
            return false;
        }
        for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            set_dirt(w, x + dx, y - 1, z + dz);
        }
        let log = axis_log("pale_oak_log", "y");
        let heart = blocks::with(id("creaking_heart"), &[("pillar_axis", blocks::s("y")), ("natural", blocks::b(true))]);
        let leaves = id("pale_oak_leaves");
        let (moss, moss_tip) = (hanging_moss(false), hanging_moss(true));
        let seed = w.seed();
        let nr = std::cell::RefCell::new(NukkitRandom::new(0));
        let place_log = |w: &mut W, x, y, z, creaking: bool| {
            if can_grow_into(w.get(x, y, z)) {
                w.set(x, y, z, if creaking { heart } else { log });
            }
        };
        let place_leaf = |w: &mut W, x: i32, y: i32, z: i32| {
            if !is_air(w.get(x, y, z)) {
                return;
            }
            w.set(x, y, z, leaves);
            let mut r = nr.borrow_mut();
            r.set_seed(seed.wrapping_add(x as i64).wrapping_add(y as i64).wrapping_add(z as i64));
            if r.next_bounded_int(2) == 0 {
                let depth = r.next_range(1, 6);
                for d in 1..depth {
                    if is_air(w.get(x, y - d, z)) {
                        w.set(x, y - d, z, if d == depth - 1 { moss_tip } else { moss });
                    } else {
                        break;
                    }
                }
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
            if can_grow_into(w.get(k1, k2, l1)) {
                let mut creaking = -1;
                if try_heart && k2 > y && rand.next_int_below(i) == 0 {
                    try_heart = false;
                    creaking = rand.next_int_below(3);
                }
                place_log(w, k1, k2, l1, creaking == 0);
                place_log(w, k1 + 1, k2, l1, creaking == 1);
                place_log(w, k1, k2, l1 + 1, creaking == 2);
                place_log(w, k1 + 1, k2, l1 + 1, creaking == 3);
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

/// ObjectSmallPaleOakTree: small pale oak with hanging moss.
#[derive(Clone, Copy)]
pub struct SmallPaleOak {
    pub min_tree_height: i32,
    pub max_tree_height: i32,
}

impl SmallPaleOak {
    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        let mut i = rand.next_int_below(self.max_tree_height) + self.min_tree_height;
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
        set_dirt(w, x, y - 1, z);
        let leaves = id("pale_oak_leaves");
        let (moss, moss_tip) = (hanging_moss(false), hanging_moss(true));
        let seed = w.seed();
        let mut nr = NukkitRandom::new(0);
        let mut i3 = y - 3 + i;
        while i3 <= y + i {
            let i4 = i3 - (y + i);
            let j1 = 1 - i4 / 2;
            for k1 in x - j1..=x + j1 {
                let l1 = k1 - x;
                for i2 in z - j1..=z + j1 {
                    let j2 = i2 - z;
                    if l1.abs() != j1 || j2.abs() != j1 || rand.next_int_below(2) != 0 && i4 != 0 {
                        let b = w.get(k1, i3, i2);
                        if is_air(b) || is_leaves(b) || nm(b) == "pale_hanging_moss" {
                            w.set(k1, i3, i2, leaves);
                            nr.set_seed(seed.wrapping_add(k1 as i64).wrapping_add(i3 as i64).wrapping_add(i2 as i64));
                            if nr.next_bounded_int(2) == 0 {
                                let depth = nr.next_range(1, 6);
                                // PNX bug kept: the moss loop advances the tree height `i`, not its counter.
                                if 1 < depth {
                                    loop {
                                        let py = i3 - i;
                                        // TODO: stop at the world bottom, PNX would loop forever over void.
                                        if py < MIN_Y || !is_air(w.get(k1, py, i2)) {
                                            break;
                                        }
                                        w.set(k1, py, i2, if i == depth - 1 { moss_tip } else { moss });
                                        i += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            i3 += 1;
        }
        true
    }
}

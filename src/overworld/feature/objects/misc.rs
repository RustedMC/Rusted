// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! ObjectBigMushroom, ObjectFallenTree, BeeNestGenerator.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MushroomType {
    Red,
    Brown,
}

/// ObjectBigMushroom: huge red/brown mushroom (`kind: None` rolls one).
#[derive(Clone, Copy, Default)]
pub struct BigMushroom {
    pub kind: Option<MushroomType>,
}

const NORTH_WEST: i32 = 1;
const NORTH_EAST: i32 = 3;
const CENTER: i32 = 5;
const SOUTH_WEST: i32 = 7;
const SOUTH_EAST: i32 = 9;
const STEM: i32 = 10;
const ALL_INSIDE: i32 = 0;

/// Approximates `Block.get(id) instanceof BlockSolid`.
fn block_solid(b: i32) -> bool {
    blocks::is_full(b)
}

impl BigMushroom {
    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        let kind = self.kind.unwrap_or_else(|| if rand.next_boolean() { MushroomType::Red } else { MushroomType::Brown });
        let base = id(if kind == MushroomType::Brown { "brown_mushroom_block" } else { "red_mushroom_block" });
        let state = |meta: i32| blocks::with(base, &[("huge_mushroom_bits", blocks::i(meta))]);
        let mut i = rand.next_int_below(3) + 4;
        if rand.next_int_below(12) == 0 {
            i *= 2;
        }
        if !(y >= 1 && y + i + 1 < 256) {
            return false;
        }
        for j in y..=y + 1 + i {
            let k = if j <= y + 3 { 0 } else { 3 };
            for l in x - k..=x + k {
                for i1 in z - k..=z + k {
                    if !(0..256).contains(&j) {
                        return false;
                    }
                    let b = w.get(l, j, i1);
                    if !blocks::replaceable(b) && !is_leaves(b) {
                        return false;
                    }
                }
            }
        }
        if !matches!(nm(w.get(x, y - 1, z)), "dirt" | "grass_block" | "mycelium") {
            return false;
        }
        let k2 = if kind == MushroomType::Red { y + i - 3 } else { y + i };
        for l2 in k2..=y + i {
            let mut j3 = if l2 < y + i { 2 } else { 1 };
            if kind == MushroomType::Brown {
                j3 = 3;
            }
            let (k3, l3, j1, k1) = (x - j3, x + j3, z - j3, z + j3);
            for l1 in k3..=l3 {
                for i2 in j1..=k1 {
                    let mut j2 = 5;
                    if l1 == k3 {
                        j2 -= 1;
                    } else if l1 == l3 {
                        j2 += 1;
                    }
                    if i2 == j1 {
                        j2 -= 3;
                    } else if i2 == k1 {
                        j2 += 3;
                    }
                    let mut meta = j2;
                    if kind == MushroomType::Brown || l2 < y + i {
                        if (l1 == k3 || l1 == l3) && (i2 == j1 || i2 == k1) {
                            continue;
                        }
                        if l1 == x - (j3 - 1) && i2 == j1 || l1 == k3 && i2 == z - (j3 - 1) {
                            meta = NORTH_WEST;
                        }
                        if l1 == x + (j3 - 1) && i2 == j1 || l1 == l3 && i2 == z - (j3 - 1) {
                            meta = NORTH_EAST;
                        }
                        if l1 == x - (j3 - 1) && i2 == k1 || l1 == k3 && i2 == z + (j3 - 1) {
                            meta = SOUTH_WEST;
                        }
                        if l1 == x + (j3 - 1) && i2 == k1 || l1 == l3 && i2 == z + (j3 - 1) {
                            meta = SOUTH_EAST;
                        }
                    }
                    if meta == CENTER && l2 < y + i {
                        meta = ALL_INSIDE;
                    }
                    if (y >= y + i - 1 || meta != ALL_INSIDE) && !block_solid(w.get(l1, l2, i2)) {
                        w.set(l1, l2, i2, state(meta));
                    }
                }
            }
        }
        let stem = state(STEM);
        for i3 in 0..i {
            if !block_solid(w.get(x, y + i3, z)) {
                w.set(x, y + i3, z, stem);
            }
        }
        true
    }
}

/// ObjectFallenTree: a stump plus a log lying 2-3 blocks away, maybe with mushrooms on it.
#[derive(Clone, Copy)]
pub struct FallenTree {
    pub wood: Wood,
    pub min_log_length: i32,
    pub max_log_length: i32,
}

impl Default for FallenTree {
    fn default() -> Self {
        Self::new(Wood::Oak)
    }
}

impl FallenTree {
    pub fn new(wood: Wood) -> Self {
        Self { wood, min_log_length: 3, max_log_length: 7 }
    }

    fn valid_pos<W: World + ?Sized>(w: &W, x: i32, y: i32, z: i32) -> bool {
        (MIN_Y..MAX_Y).contains(&y) && !blocks::is_solid(w.get(x, y, z))
    }
    fn over_solid<W: World + ?Sized>(w: &W, x: i32, y: i32, z: i32) -> bool {
        blocks::is_solid(w.get(x, y - 1, z))
    }
    fn may_place_on<W: World + ?Sized>(w: &W, x: i32, y: i32, z: i32) -> bool {
        Self::valid_pos(w, x, y, z) && Self::over_solid(w, x, y, z)
    }

    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        if !Self::may_place_on(w, x, y, z) {
            return false;
        }
        let name = self.wood.log_name();
        w.set(x, y, z, axis_log(name, "y"));
        if matches!(self.wood, Wood::Oak | Wood::Jungle) {
            for (dx, dz, m) in [(-1, 0, 8), (1, 0, 2), (0, -1, 1), (0, 1, 4)] {
                if rand.next_int_below(4) == 0 || !is_air(w.get(x + dx, y, z + dz)) {
                    continue;
                }
                w.set(x + dx, y, z + dz, vine(m));
            }
        }
        let dir = Face::random(rand);
        let len = if self.max_log_length <= self.min_log_length {
            1.max(self.min_log_length)
        } else {
            self.min_log_length + rand.next_int_below(self.max_log_length - self.min_log_length + 1)
        };
        let dist = 2 + rand.next_int_below(2);
        let (sx, mut sy, sz) = (x + dir.dx() * dist, y + 1, z + dir.dz() * dist);
        for _ in 0..6 {
            if Self::may_place_on(w, sx, sy, sz) {
                break;
            }
            sy -= 1;
        }
        // canPlaceEntireFallenLog
        let mut gap = 0;
        let mut ok = true;
        for i in 0..len {
            let (cx, cz) = (sx + dir.dx() * i, sz + dir.dz() * i);
            if !Self::valid_pos(w, cx, sy, cz) {
                ok = false;
                break;
            }
            if !Self::over_solid(w, cx, sy, cz) {
                gap += 1;
                if gap > 2 {
                    ok = false;
                    break;
                }
            } else {
                gap = 0;
            }
        }
        if ok {
            let log = axis_log(name, if dir.is_x() { "x" } else { "z" });
            for i in 0..len {
                w.set(sx + dir.dx() * i, sy, sz + dir.dz() * i, log);
            }
            if len > 0 && rand.next_int_below(4) == 0 {
                let n = 1 + rand.next_int_below(2);
                for _ in 0..n {
                    let i = rand.next_int_below(len);
                    let (mx, mz) = (sx + dir.dx() * i, sz + dir.dz() * i);
                    if is_air(w.get(mx, sy + 1, mz)) {
                        let m = if rand.next_boolean() { "red_mushroom" } else { "brown_mushroom" };
                        w.set(mx, sy + 1, mz, id(m));
                    }
                }
            }
        }
        // TODO: PNX's post hook that removes flowers on bad blocks isn't ported.
        true
    }
}

/// BeeNestGenerator.place: hangs a nest under the first leaf layer (bee count rolled, unused).
pub fn bee_nest_place<W: World + ?Sized>(w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
    let _bee_count = rand.next_int_range(2, 4);
    for leaf_y in y + 1..=y + 32 {
        let ny = leaf_y - 1;
        if is_air(w.get(x, ny, z)) || !is_air(w.get(x, ny, z + 1)) || is_air(w.get(x, ny + 1, z + 1)) {
            continue;
        }
        bee_nest_place_at(w, x, ny, z + 1);
        return true;
    }
    false
}

/// BeeNestGenerator.placeAt: a south-facing empty bee nest (no block entity / bees).
pub fn bee_nest_place_at<W: World + ?Sized>(w: &mut W, x: i32, y: i32, z: i32) {
    let nest = blocks::with(id("bee_nest"), &[("direction", blocks::i(Face::South.horizontal_index())), ("honey_level", blocks::i(0))]);
    w.set(x, y, z, nest);
}

/// BeeNestGenerator.hasNearbyFlower: any small flower (or flowering azalea) in the 5x5 ring at `y`.
pub fn bee_nest_has_nearby_flower<W: World + ?Sized>(w: &W, x: i32, y: i32, z: i32) -> bool {
    for cx in x - 2..=x + 2 {
        for cz in z - 2..=z + 2 {
            if cx == x && cz == z {
                continue;
            }
            let n = nm(w.get(cx, y, cz));
            if n.ends_with("_tulip")
                || matches!(
                    n,
                    "dandelion" | "poppy" | "blue_orchid" | "allium" | "azure_bluet" | "oxeye_daisy" | "cornflower" | "lily_of_the_valley"
                        | "wither_rose" | "torchflower" | "open_eyeblossom" | "closed_eyeblossom" | "flowering_azalea"
                )
            {
                return true;
            }
        }
    }
    false
}

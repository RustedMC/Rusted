// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Nukkit legacy trees (legacytree/: LegacyTreeGenerator and its oak, birch, tall birch, spruce,
//! big spruce, jungle and dark oak variants) and LegacyTallGrass.

use super::oak::FancyOak;
use super::*;

/// Which legacy tree.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LegacyKind {
    Oak,
    Birch,
    TallBirch,
    Spruce,
    Jungle,
    /// LegacyBigSpruceTree(leafStartHeightMultiplier, baseLeafRadius).
    BigSpruce { leaf_start_height_multiplier: f32, base_leaf_radius: i32 },
    /// LegacyDarkOakTree(leafStartHeightMultiplier, baseLeafRadius).
    DarkOak { leaf_start_height_multiplier: f32, base_leaf_radius: i32 },
}

/// `tree_height` is PNX's `treeHeight` (default 7). Variants that roll a height don't change it.
#[derive(Clone, Copy, Debug)]
pub struct LegacyTree {
    pub kind: LegacyKind,
    pub tree_height: i32,
}

/// LegacyTreeGenerator.overridable.
fn overridable(b: i32) -> bool {
    is_air(b)
        || matches!(
            nm(b),
            "acacia_leaves" | "azalea_leaves" | "birch_leaves" | "azalea_leaves_flowered" | "cherry_leaves" | "dark_oak_leaves"
                | "jungle_leaves" | "mangrove_leaves" | "oak_leaves" | "red_poplar_leaves" | "orange_poplar_leaves" | "yellow_poplar_leaves"
                | "poplar_sapling" | "spruce_leaves" | "snow_layer" | "acacia_sapling" | "cherry_sapling" | "spruce_sapling"
                | "bamboo_sapling" | "oak_sapling" | "jungle_sapling" | "dark_oak_sapling" | "leaf_litter" | "wildflowers"
                | "pink_petals" | "tall_grass" | "birch_sapling" | "short_grass" | "dandelion" | "lily_of_the_valley" | "lilac"
                | "peony" | "rose_bush" | "large_fern" | "fern"
        )
}

impl LegacyTree {
    pub fn new(kind: LegacyKind) -> Self {
        Self { kind, tree_height: 7 }
    }

    fn wood(&self) -> Wood {
        match self.kind {
            LegacyKind::Oak => Wood::Oak,
            LegacyKind::Birch | LegacyKind::TallBirch => Wood::Birch,
            LegacyKind::Spruce | LegacyKind::BigSpruce { .. } => Wood::Spruce,
            LegacyKind::Jungle => Wood::Jungle,
            LegacyKind::DarkOak { .. } => Wood::DarkOak,
        }
    }

    /// setRandomTreeWithVines (birch never rolls).
    fn roll_vines(&self, rand: &mut Xoroshiro) -> bool {
        self.wood() != Wood::Birch && rand.next_int_below(TREE_WITH_VINES_CHANCE) == 0
    }

    fn fancy_brace<W: World + ?Sized>(w: &W, x: i32, y: i32, z: i32) -> bool {
        for xx in -1..=1 {
            for zz in -1..=1 {
                if (xx != 0 || zz != 0) && !overridable(w.get(x + xx, y + 1, z + zz)) {
                    return true;
                }
            }
        }
        false
    }

    /// LegacyTreeGenerator.canPlaceObject (oak: or has a "fancy brace").
    pub fn can_place_object<W: World + ?Sized>(&self, w: &W, x: i32, y: i32, z: i32) -> bool {
        if self.kind == LegacyKind::Oak && Self::fancy_brace(w, x, y, z) {
            return true;
        }
        let mut r = 0;
        for yy in 0..self.tree_height + 3 {
            if yy == 1 || yy == self.tree_height {
                r += 1;
            }
            for xx in -r..=r {
                for zz in -r..=r {
                    if !overridable(w.get(x + xx, y + yy, z + zz)) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// ObjectGenerator.generate: places unconditionally, returns true.
    pub fn generate<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) -> bool {
        self.place_object(w, rand, x, y, z);
        true
    }

    pub fn place_object<W: World + ?Sized>(&self, w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) {
        let wood = self.wood();
        let log = axis_log(wood.log_name(), "y");
        let leaves = id(wood.leaves_name());
        match self.kind {
            LegacyKind::Oak if Self::fancy_brace(w, x, y, z) => {
                FancyOak::default().generate(w, rand, x, y, z);
            }
            LegacyKind::Oak | LegacyKind::Birch | LegacyKind::TallBirch | LegacyKind::Jungle => {
                let h = match self.kind {
                    LegacyKind::Oak => rand.next_int_below(3) + 4,
                    LegacyKind::Birch => rand.next_int_below(2) + 5,
                    LegacyKind::TallBirch => rand.next_int_below(3) + 10,
                    _ => rand.next_int_below(6) + 4,
                };
                let vines = self.roll_vines(rand);
                Self::trunk(w, log, x, y, z, h - 1, vines);
                for yy in y - 3 + h..=y + h {
                    let y_off = (yy - (y + h)) as f64;
                    let mid = (1.0 - y_off / 2.0) as i32;
                    for xx in x - mid..=x + mid {
                        for zz in z - mid..=z + mid {
                            if (xx - x).abs() == mid && (zz - z).abs() == mid && (y_off == 0.0 || rand.next_int_below(2) == 0) {
                                continue;
                            }
                            if !blocks::is_solid(w.get(xx, yy, zz)) {
                                w.set(xx, yy, zz, leaves);
                            }
                        }
                    }
                }
            }
            LegacyKind::Spruce => {
                let h = rand.next_int_below(4) + 6;
                let vines = self.roll_vines(rand);
                let top = h - (1 + rand.next_int_below(2));
                let l_radius = 2 + rand.next_int_below(2);
                let th = h - rand.next_int_below(3);
                Self::trunk(w, log, x, y, z, th, vines);
                Self::spruce_leaves(w, rand, leaves, h, top, l_radius, x, y, z, true);
            }
            LegacyKind::BigSpruce { leaf_start_height_multiplier: m, base_leaf_radius: r } => {
                let h = if self.tree_height == 7 { rand.next_int_below(15) + 20 } else { self.tree_height };
                let top = h - (h as f32 * m) as i32;
                let l_radius = r + rand.next_int_below(2);
                let th = h - rand.next_int_below(3);
                w.set(x, y - 1, z, id("dirt"));
                for yy in 0..th {
                    for (dx, dz) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                        if overridable(w.get(x + dx, y + yy, z + dz)) {
                            w.set(x + dx, y + yy, z + dz, log);
                        }
                    }
                }
                Self::spruce_leaves(w, rand, leaves, h, top, l_radius, x, y, z, false);
            }
            LegacyKind::DarkOak { leaf_start_height_multiplier: m, base_leaf_radius: r } => {
                let h = if self.tree_height == 0 { rand.next_int_below(15) + 20 } else { self.tree_height };
                let top = h - (h as f32 * m) as i32;
                let l_radius = r + rand.next_int_below(2);
                let vines = self.roll_vines(rand);
                let th = h - rand.next_int_below(3);
                w.set(x, y - 1, z, id("dirt"));
                for yy in 0..th {
                    for (dx, dz) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                        // PNX checks the corner column for every block (so only one column grows).
                        if overridable(w.get(x, y + yy, z)) {
                            w.set(x + dx, y + yy, z + dz, log);
                            if vines {
                                add_vines_around_log(w, x + dx, y + yy, z + dz);
                            }
                        }
                    }
                }
                Self::spruce_leaves(w, rand, leaves, h, top, l_radius, x, y, z, false);
            }
        }
    }

    /// LegacyTreeGenerator.placeTrunk.
    fn trunk<W: World + ?Sized>(w: &mut W, log: i32, x: i32, y: i32, z: i32, trunk_height: i32, vines: bool) {
        w.set(x, y - 1, z, id("dirt"));
        for yy in 0..trunk_height {
            if overridable(w.get(x, y + yy, z)) {
                w.set(x, y + yy, z, log);
                if vines {
                    add_vines_around_log(w, x, y + yy, z);
                }
            }
        }
    }

    /// LegacySpruceTree.placeLeaves (`dirt`: the small spruce also resets the block below).
    #[allow(clippy::too_many_arguments)]
    fn spruce_leaves<W: World + ?Sized>(w: &mut W, rand: &mut Xoroshiro, leaves: i32, h: i32, top: i32, l_radius: i32, x: i32, y: i32, z: i32, dirt: bool) {
        let mut radius = rand.next_int_below(2);
        let (mut max_r, mut min_r) = (1, 0);
        if dirt {
            w.set(x, y - 1, z, id("dirt"));
        }
        for yy in 0..=top {
            let yyy = y + h - yy;
            for xx in x - radius..=x + radius {
                for zz in z - radius..=z + radius {
                    if (xx - x).abs() == radius && (zz - z).abs() == radius && radius > 0 {
                        continue;
                    }
                    if !blocks::is_solid(w.get(xx, yyy, zz)) {
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
    }
}

/// LegacyTreeGenerator.growTree: picks the legacy tree for `wood` and places it if it fits.
pub fn legacy_grow_tree<W: World + ?Sized>(w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32, wood: Wood, tall: bool) {
    let kind = match wood {
        Wood::Spruce => LegacyKind::Spruce,
        Wood::Birch if tall => LegacyKind::TallBirch,
        Wood::Birch => LegacyKind::Birch,
        Wood::DarkOak => LegacyKind::DarkOak { leaf_start_height_multiplier: 6.0, base_leaf_radius: 3 },
        Wood::Jungle => LegacyKind::Jungle,
        _ => LegacyKind::Oak,
    };
    let tree = LegacyTree::new(kind);
    if tree.can_place_object(w, x, y, z) {
        tree.place_object(w, rand, x, y, z);
    }
}

/// LegacyTallGrass.growGrass: grass and flowers scattered on grass blocks around (x, y, z).
pub fn legacy_tall_grass<W: World + ?Sized>(w: &mut W, rand: &mut Xoroshiro, x: i32, y: i32, z: i32) {
    const PLACES: [&str; 14] = [
        "short_grass", "tall_grass", "dandelion", "poppy", "azure_bluet", "oxeye_daisy", "allium", "cornflower", "blue_orchid",
        "lily_of_the_valley", "red_tulip", "orange_tulip", "pink_tulip", "white_tulip",
    ];
    let y = y + 1;
    for cx in x - 2..=x + 2 {
        for cz in z - 2..=z + 2 {
            let ny = y + rand.next_int_below(2) * if rand.next_boolean() { -1 } else { 1 };
            if !rand.next_boolean() {
                continue;
            }
            if !(nm(w.get(cx, ny, cz)) == "air" && nm(w.get(cx, ny - 1, cz)) == "grass_block") {
                continue;
            }
            let n = (rand.next_gaussian() * 1000.0 + 0.5).floor() as i64 as i32;
            let a = n.abs();
            let pick = if (-300..=300).contains(&n) {
                0
            } else if (300..=500).contains(&a) {
                w.set(cx, ny, cz, id(PLACES[1]));
                w.set(cx, ny + 1, cz, blocks::with(id("tall_grass"), &[("upper_block_bit", blocks::b(true))]));
                continue;
            } else if (500..600).contains(&n) {
                2
            } else if (-600..=-500).contains(&n) {
                3
            } else if (600..700).contains(&n) {
                4
            } else if (-700..-600).contains(&n) {
                5
            } else if (-750..-700).contains(&n) {
                6
            } else if (-800..-750).contains(&n) {
                7
            } else if (-850..-800).contains(&n) {
                8
            } else if (-900..-850).contains(&n) {
                9
            } else if (-1000..-900).contains(&n) {
                10
            } else if (700..800).contains(&n) {
                11
            } else if (800..900).contains(&n) {
                12
            } else if (900..1000).contains(&n) {
                13
            } else {
                continue;
            };
            w.set(cx, ny, cz, id(PLACES[pick]));
        }
    }
}

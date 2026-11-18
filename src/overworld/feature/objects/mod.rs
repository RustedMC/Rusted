// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Trees, huge mushrooms etc. that features place (PNX `object/`). They work on any [`World`]
//! and use the RNG in the same order as the Java. `x, y, z` is where the trunk starts.

// Used by trees.rs once it lands; until then the library looks unused to rustc.
#![allow(dead_code, unused_imports)]

mod java;
mod jungle;
mod legacy;
mod mangrove;
mod misc;
mod oak;
mod savanna;
mod spruce;

pub use jungle::{Jungle, JungleBig, JungleBush, MegaJungle};
pub use legacy::{legacy_grow_tree, legacy_tall_grass, LegacyKind, LegacyTree};
pub use mangrove::Mangrove;
pub use misc::{bee_nest_has_nearby_flower, bee_nest_place, bee_nest_place_at, BigMushroom, FallenTree, MushroomType};
pub use oak::{Azalea, Cherry, FancyOak, PaleOak, SmallPaleOak, SwampOak};
pub use savanna::{DarkOak, RoofedWorldgen, Savanna, SavannaWorldgen};
pub use spruce::{BigSpruce, MegaConifer, Pine, SmallSpruce};

use super::{blocks, Region};
pub use super::Xoroshiro;

/// What objects read and write: PNX's `BlockManager`. Writes must be visible to later reads.
pub trait World {
    fn get(&self, x: i32, y: i32, z: i32) -> i32;
    fn set(&mut self, x: i32, y: i32, z: i32, id: i32);
    /// PNX `getHeightMap`: one above the highest non-air terrain block.
    fn height(&self, x: i32, z: i32) -> i32;
    /// World seed (`BlockManager.getSeed`).
    fn seed(&self) -> i64;
}

impl World for Region {
    fn get(&self, x: i32, y: i32, z: i32) -> i32 {
        Region::get(self, x, y, z)
    }
    fn set(&mut self, x: i32, y: i32, z: i32, id: i32) {
        Region::set(self, x, y, z, id)
    }
    fn height(&self, x: i32, z: i32) -> i32 {
        Region::height(self, x, z)
    }
    fn seed(&self) -> i64 {
        self.seed
    }
}

/// PNX WoodType.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wood {
    Oak,
    Spruce,
    Birch,
    Jungle,
    Acacia,
    DarkOak,
    Mangrove,
    Cherry,
    PaleOak,
}

impl Wood {
    pub fn log_name(self) -> &'static str {
        match self {
            Wood::Oak => "oak_log",
            Wood::Spruce => "spruce_log",
            Wood::Birch => "birch_log",
            Wood::Jungle => "jungle_log",
            Wood::Acacia => "acacia_log",
            Wood::DarkOak => "dark_oak_log",
            Wood::Mangrove => "mangrove_log",
            Wood::Cherry => "cherry_log",
            Wood::PaleOak => "pale_oak_log",
        }
    }
    pub fn leaves_name(self) -> &'static str {
        match self {
            Wood::Oak => "oak_leaves",
            Wood::Spruce => "spruce_leaves",
            Wood::Birch => "birch_leaves",
            Wood::Jungle => "jungle_leaves",
            Wood::Acacia => "acacia_leaves",
            Wood::DarkOak => "dark_oak_leaves",
            Wood::Mangrove => "mangrove_leaves",
            Wood::Cherry => "cherry_leaves",
            Wood::PaleOak => "pale_oak_leaves",
        }
    }
}

// ---- shared helpers (PNX TreeGenerator & friends) ----

/// Overworld `getMinHeight` / `getMaxHeight`.
pub(crate) const MIN_Y: i32 = -64;
pub(crate) const MAX_Y: i32 = 320;
const TREE_WITH_VINES_CHANCE: i32 = 20;

pub(crate) fn nm(b: i32) -> &'static str {
    blocks::short(b)
}
pub(crate) fn id(n: &str) -> i32 {
    blocks::id(n)
}
pub(crate) fn air() -> i32 {
    blocks::air()
}
/// PNX `Block.AIR` id check / `isAir`.
pub(crate) fn is_air(b: i32) -> bool {
    blocks::is_air(b)
}
/// `instanceof BlockLeaves` (flowered azalea leaves included).
pub(crate) fn is_leaves(b: i32) -> bool {
    blocks::is_leaves(b) || nm(b) == "azalea_leaves_flowered"
}
/// `log` with `pillar_axis` = `axis` ('x', 'y', 'z').
pub(crate) fn axis_log(name: &str, axis: &str) -> i32 {
    blocks::with(id(name), &[("pillar_axis", blocks::s(axis))])
}

/// TreeGenerator.canGrowInto.
pub(crate) fn can_grow_into(b: i32) -> bool {
    is_air(b)
        || matches!(
            nm(b),
            "acacia_leaves" | "azalea_leaves" | "birch_leaves" | "azalea_leaves_flowered" | "cherry_leaves" | "dark_oak_leaves"
                | "jungle_leaves" | "mangrove_leaves" | "oak_leaves" | "spruce_leaves" | "pale_oak_leaves" | "grass_block" | "dirt"
                | "acacia_log" | "birch_log" | "oak_log" | "pale_oak_log" | "dark_oak_log" | "jungle_log" | "mangrove_log"
                | "spruce_log" | "vine" | "dirt_with_roots" | "cherry_log" | "mangrove_roots" | "mangrove_propagule"
                | "acacia_sapling" | "cherry_sapling" | "spruce_sapling" | "bamboo_sapling" | "oak_sapling" | "jungle_sapling"
                | "dark_oak_sapling" | "pale_oak_sapling" | "birch_sapling" | "fern" | "short_grass" | "tall_grass"
                | "pale_hanging_moss" | "closed_eyeblossom" | "open_eyeblossom" | "leaf_litter" | "bamboo"
        )
}

/// TreeGenerator.setDirtAt.
pub(crate) fn set_dirt<W: World + ?Sized>(w: &mut W, x: i32, y: i32, z: i32) {
    w.set(x, y, z, id("dirt"));
}

/// `vine` with `vine_direction_bits` = meta (cached).
pub(crate) fn vine(meta: i32) -> i32 {
    static V: std::sync::OnceLock<[i32; 16]> = std::sync::OnceLock::new();
    V.get_or_init(|| std::array::from_fn(|m| blocks::with(id("vine"), &[("vine_direction_bits", blocks::i(m as i32))])))[(meta & 15) as usize]
}

/// TreeGenerator.addVinesAroundLog.
pub(crate) fn add_vines_around_log<W: World + ?Sized>(w: &mut W, x: i32, y: i32, z: i32) {
    for (dx, dz, m) in [(-1, 0, 8), (1, 0, 2), (0, -1, 1), (0, 1, 4)] {
        if is_air(w.get(x + dx, y, z + dz)) {
            w.set(x + dx, y, z + dz, vine(m));
        }
    }
}

/// Horizontal faces.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Face {
    North,
    East,
    South,
    West,
}
impl Face {
    /// BlockFace.Plane.HORIZONTAL order.
    pub(crate) const PLANE: [Face; 4] = [Face::North, Face::East, Face::South, Face::West];
    /// BlockFace.getHorizontals() order (S-W-N-E, by horizontal index).
    pub(crate) const HORIZONTALS: [Face; 4] = [Face::South, Face::West, Face::North, Face::East];
    pub(crate) fn dx(self) -> i32 {
        match self {
            Face::East => 1,
            Face::West => -1,
            _ => 0,
        }
    }
    pub(crate) fn dz(self) -> i32 {
        match self {
            Face::South => 1,
            Face::North => -1,
            _ => 0,
        }
    }
    pub(crate) fn opposite(self) -> Face {
        match self {
            Face::North => Face::South,
            Face::South => Face::North,
            Face::East => Face::West,
            Face::West => Face::East,
        }
    }
    pub(crate) fn horizontal_index(self) -> i32 {
        match self {
            Face::South => 0,
            Face::West => 1,
            Face::North => 2,
            Face::East => 3,
        }
    }
    /// BlockVine.getMetaFromFace.
    pub(crate) fn vine_meta(self) -> i32 {
        match self {
            Face::South => 1,
            Face::West => 2,
            Face::North => 4,
            Face::East => 8,
        }
    }
    pub(crate) fn is_x(self) -> bool {
        matches!(self, Face::East | Face::West)
    }
    /// BlockFace.Plane.HORIZONTAL.random(rand).
    pub(crate) fn random(r: &mut Xoroshiro) -> Face {
        Face::PLANE[r.next_int_below(4) as usize]
    }
}

/// MathHelper.floor(double).
pub(crate) fn floor(d: f64) -> i32 {
    let i = d as i32;
    if d < i as f64 { i - 1 } else { i }
}

fn sin_table() -> &'static [f32] {
    static T: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    T.get_or_init(|| (0..65536).map(|i| (i as f64 * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32).collect())
}
/// MathHelper.sin(float) (lookup table).
pub(crate) fn mh_sin(f: f32) -> f32 {
    sin_table()[((f * 10430.378f32) as i32 & 0xFFFF) as usize]
}
/// MathHelper.cos(float).
pub(crate) fn mh_cos(f: f32) -> f32 {
    sin_table()[((f * 10430.378f32 + 16384.0f32) as i32 & 0xFFFF) as usize]
}

#[cfg(test)]
mod tests;

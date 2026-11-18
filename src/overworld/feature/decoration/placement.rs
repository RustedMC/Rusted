// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! PNX placement graph (`feature/placement`): small nodes that move a position and place blocks,
//! plus the surface/volume candidate drivers. Trees use these too.
#![allow(dead_code)] // not every node is used by decoration; the rest are for other modules

use super::super::{chunk_hash, java_hash, Ctx, Region, Xoroshiro};
use super::super::blocks;

/// PNX `FeaturePlacementContext`: the region, the feature's shared random and the current position.
pub(crate) struct PlaceCtx<'a> {
    pub region: &'a mut Region,
    pub random: &'a mut Xoroshiro,
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl PlaceCtx<'_> {
    pub(crate) fn set_pos(&mut self, x: i32, y: i32, z: i32) {
        (self.x, self.y, self.z) = (x, y, z);
    }
    pub(crate) fn pos(&self) -> (i32, i32, i32) {
        (self.x, self.y, self.z)
    }
    /// Block at an offset from the current position.
    pub(crate) fn get(&self, dx: i32, dy: i32, dz: i32) -> i32 {
        self.region.get(self.x + dx, self.y + dy, self.z + dz)
    }
    /// Biome at the current position.
    pub(crate) fn biome(&self) -> u16 {
        self.region.biome(self.x, self.y, self.z)
    }
}

/// PNX `PlacementFeature`: places at the context position; returns success.
pub(crate) trait Placement {
    fn place(&self, c: &mut PlaceCtx) -> bool;
}

impl<F: Fn(&mut PlaceCtx) -> bool> Placement for F {
    fn place(&self, c: &mut PlaceCtx) -> bool {
        self(c)
    }
}

pub(crate) type Node = Box<dyn Placement>;
/// PNX `PlacementPredicate`.
pub(crate) type Pred = fn(&PlaceCtx) -> bool;

/// PNX `ScatterCoordinate`: one relative offset distribution.
#[derive(Clone, Copy)]
pub(crate) enum Coord {
    Fixed(i32),
    /// Bedrock "gaussian": center + r(half) - r(half).
    Gaussian { center: i32, half: i32 },
    /// Bedrock "triangle": min + r(left + 1) + r(right + 1).
    Triangle { min: i32, left: i32, right: i32 },
}

impl Coord {
    /// `ScatterCoordinate.gaussian(min, max)` (inclusive extents).
    pub(crate) fn gaussian(min: i32, max: i32) -> Self {
        let width = max - min;
        let half = width / 2;
        if width + 1 < 3 { Coord::Fixed(min + half) } else { Coord::Gaussian { center: min + half, half } }
    }
    /// `ScatterCoordinate.triangle(min, max)` (inclusive extents).
    pub(crate) fn triangle(min: i32, max: i32) -> Self {
        let left = (max - min) / 2;
        Coord::Triangle { min, left, right: max - min - left }
    }
    pub(crate) fn sample(self, r: &mut Xoroshiro) -> i32 {
        match self {
            Coord::Fixed(v) => v,
            Coord::Gaussian { center, half } => center + r.next_exclusive_int(half) - r.next_exclusive_int(half),
            Coord::Triangle { min, left, right } => min + r.next_exclusive_int(left + 1) + r.next_exclusive_int(right + 1),
        }
    }
}

/// PNX `ScatterEvaluationOrder`: the order the three coordinates are sampled in.
#[derive(Clone, Copy)]
pub(crate) enum Order {
    Xyz,
    Xzy,
    Yxz,
    Yzx,
    Zxy,
    Zyx,
}

/// PNX `ScatterPlacementFeature`: runs `child` `iterations` times at scattered offsets.
pub(crate) struct Scatter {
    pub iterations: i32,
    pub order: Order,
    pub x: Coord,
    pub y: Coord,
    pub z: Coord,
    pub child: Node,
}

impl Placement for Scatter {
    fn place(&self, c: &mut PlaceCtx) -> bool {
        let input = c.pos();
        let (mut any, mut out) = (false, input);
        for _ in 0..self.iterations {
            let (mut x, mut y, mut z) = input;
            let axes: [u8; 3] = match self.order {
                Order::Xyz => [0, 1, 2],
                Order::Xzy => [0, 2, 1],
                Order::Yxz => [1, 0, 2],
                Order::Yzx => [1, 2, 0],
                Order::Zxy => [2, 0, 1],
                Order::Zyx => [2, 1, 0],
            };
            for a in axes {
                match a {
                    0 => x = input.0 + self.x.sample(c.random),
                    1 => y = input.1 + self.y.sample(c.random),
                    _ => z = input.2 + self.z.sample(c.random),
                }
            }
            c.set_pos(x, y, z);
            if self.child.place(c) {
                any = true;
                out = c.pos();
            }
        }
        let p = if any { out } else { input };
        c.set_pos(p.0, p.1, p.2);
        any
    }
}

/// PNX `SnapToFloorPlacementFeature`: from an air origin, moves down (up to `range`) onto the
/// first non-air block, failing on liquid or a biome change.
pub(crate) struct SnapToFloor {
    pub range: i32,
    pub child: Node,
}

impl Placement for SnapToFloor {
    fn place(&self, c: &mut PlaceCtx) -> bool {
        let (x, y, z) = c.pos();
        let air = blocks::air();
        if c.region.get(x, y, z) != air {
            return false;
        }
        let biome = c.region.biome(x, y, z);
        for d in 1..=self.range {
            let fy = y - d;
            if fy < MIN_Y {
                break;
            }
            let b = c.region.get(x, fy, z);
            if b == air {
                continue;
            }
            if blocks::is_liquid(b) || c.region.biome(x, fy + 1, z) != biome {
                return false;
            }
            c.set_pos(x, fy + 1, z);
            if self.child.place(c) {
                return true;
            }
            c.set_pos(x, y, z);
            return false;
        }
        false
    }
}

/// PNX `AggregatePlacementFeature.EarlyOut`.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum EarlyOut {
    None,
    FirstFailure,
    FirstSuccess,
}

/// PNX `AggregatePlacementFeature`: every child from the same input position.
pub(crate) struct Aggregate {
    pub features: Vec<Node>,
    pub early_out: EarlyOut,
}

impl Placement for Aggregate {
    fn place(&self, c: &mut PlaceCtx) -> bool {
        let input = c.pos();
        let (mut any, mut out) = (false, input);
        for f in &self.features {
            c.set_pos(input.0, input.1, input.2);
            if f.place(c) {
                any = true;
                out = c.pos();
                if self.early_out == EarlyOut::FirstSuccess {
                    return true;
                }
            } else if self.early_out == EarlyOut::FirstFailure {
                break;
            }
        }
        let p = if any { out } else { input };
        c.set_pos(p.0, p.1, p.2);
        any
    }
}

/// PNX `SequencePlacementFeature`: children in order, each from the previous output; stops on failure.
pub(crate) struct Sequence(pub Vec<Node>);

impl Placement for Sequence {
    fn place(&self, c: &mut PlaceCtx) -> bool {
        self.0.iter().all(|f| f.place(c))
    }
}

/// PNX `ConditionalPlacementFeature`.
pub(crate) struct Conditional {
    pub pred: Pred,
    pub child: Node,
}

impl Placement for Conditional {
    fn place(&self, c: &mut PlaceCtx) -> bool {
        (self.pred)(c) && self.child.place(c)
    }
}

/// PNX `WeightedPlacementFeature`: picks exactly one child by relative weight.
pub(crate) struct Weighted(pub Vec<(Node, f32)>);

impl Placement for Weighted {
    fn place(&self, c: &mut PlaceCtx) -> bool {
        let total = self.0.iter().fold(0i32, |t, (_, w)| (t as f32 + w) as i32);
        let mut sel = c.random.next_exclusive_int(total);
        for (f, w) in &self.0 {
            sel = (sel as f32 - w) as i32;
            if sel < 0 {
                return f.place(c);
            }
        }
        false
    }
}

/// PNX `VegetationPlacementFeature`. State first (may use random), then the predicates.
pub(crate) struct Vegetation {
    pub state: fn(&mut PlaceCtx) -> i32,
    pub target: Pred,
    pub survival: Pred,
}

impl Placement for Vegetation {
    fn place(&self, c: &mut PlaceCtx) -> bool {
        let s = (self.state)(c);
        if !(self.target)(c) || !(self.survival)(c) {
            return false;
        }
        c.region.set(c.x, c.y, c.z, s);
        true
    }
}

/// Lowest world Y (PNX `level.getMinHeight()`).
pub(crate) const MIN_Y: i32 = -64;
/// PNX `level.getMaxHeight()` for the overworld.
pub(crate) const MAX_Y: i32 = 320;

/// Reseeds `ctx.random` the way most PNX features do: `seed ^ chunkHash ^ name.hashCode()`.
pub(crate) fn reseed(ctx: &mut Ctx, name: &str) {
    ctx.random.set_seed(ctx.seed ^ chunk_hash(ctx.cx, ctx.cz) ^ java_hash(name) as i64);
}

/// PNX `SurfaceCandidateFeature.apply`. `should_apply` is the rule chance, tested first.
pub(crate) fn surface_candidates(region: &mut Region, ctx: &mut Ctx, name: &str, iterations: i32, should_apply: fn(&mut Xoroshiro) -> bool, feature: &dyn Placement, pred: Pred) {
    reseed(ctx, name);
    if !should_apply(&mut ctx.random) {
        return;
    }
    let (ox, oz) = (ctx.cx << 4, ctx.cz << 4);
    let mut c = PlaceCtx { region, random: &mut ctx.random, x: ox, y: MIN_Y, z: oz };
    for _ in 0..iterations {
        let lx = c.random.next_exclusive_int(16);
        let lz = c.random.next_exclusive_int(16);
        let y = c.region.height(ox + lx, oz + lz);
        c.set_pos(ox + lx, y, oz + lz);
        if pred(&c) {
            feature.place(&mut c);
        }
    }
}

/// PNX `VolumeCandidateFeature.apply`: `iterations` origins with y in `min_y..max_y`.
pub(crate) fn volume_candidates(region: &mut Region, ctx: &mut Ctx, name: &str, iterations: i32, min_y: i32, max_y: i32, feature: &dyn Placement, pred: Pred) {
    reseed(ctx, name);
    let (ox, oz) = (ctx.cx << 4, ctx.cz << 4);
    let mut c = PlaceCtx { region, random: &mut ctx.random, x: ox, y: min_y, z: oz };
    for _ in 0..iterations {
        let lx = c.random.next_exclusive_int(16);
        let lz = c.random.next_exclusive_int(16);
        let y = min_y + c.random.next_exclusive_int(max_y - min_y);
        c.set_pos(ox + lx, y, oz + lz);
        if pred(&c) {
            feature.place(&mut c);
        }
    }
}

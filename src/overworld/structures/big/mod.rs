// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Ocean monuments and woodland mansions.

mod mansion;
mod monument;

use super::StructureType;
use crate::overworld::rng::Xoroshiro;

pub fn register(v: &mut Vec<StructureType>) {
    monument::register(v);
    mansion::register(v);
}

/// PNX `BoundingBox` (inclusive corners).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct BB {
    pub x0: i32,
    pub y0: i32,
    pub z0: i32,
    pub x1: i32,
    pub y1: i32,
    pub z1: i32,
}

impl BB {
    pub fn new(x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) -> Self {
        Self { x0, y0, z0, x1, y1, z1 }
    }

    /// `createProper`.
    pub fn proper(x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) -> Self {
        Self::new(x0.min(x1), y0.min(y1), z0.min(z1), x0.max(x1), y0.max(y1), z0.max(z1))
    }

    /// PNX `new BoundingBox(x0, z0, x1, z1)`: a chunk column box.
    pub fn column(cx: i32, cz: i32) -> Self {
        Self::new(cx << 4, -63, cz << 4, (cx << 4) + 15, 512, (cz << 4) + 15)
    }

    pub fn intersects(&self, o: &BB) -> bool {
        self.x1 >= o.x0 && self.x0 <= o.x1 && self.z1 >= o.z0 && self.z0 <= o.z1 && self.y1 >= o.y0 && self.y0 <= o.y1
    }

    pub fn intersects_xz(&self, x0: i32, z0: i32, x1: i32, z1: i32) -> bool {
        self.x1 >= x0 && self.x0 <= x1 && self.z1 >= z0 && self.z0 <= z1
    }

    pub fn inside(&self, x: i32, y: i32, z: i32) -> bool {
        x >= self.x0 && x <= self.x1 && z >= self.z0 && z <= self.z1 && y >= self.y0 && y <= self.y1
    }

    pub fn mv(&mut self, x: i32, y: i32, z: i32) {
        *self = Self::new(self.x0 + x, self.y0 + y, self.z0 + z, self.x1 + x, self.y1 + y, self.z1 + z);
    }
}

/// PNX `StructureStart` random: `Xoroshiro128(seed)` reseeded with the chunk mix.
pub(super) fn start_random(seed: i64, cx: i32, cz: i32) -> Xoroshiro {
    let mut r = Xoroshiro::new(seed);
    let a = (cx as i64).wrapping_mul(r.next_int() as i64);
    let b = (cz as i64).wrapping_mul(r.next_int() as i64);
    r.set_seed(a ^ b ^ seed);
    r
}

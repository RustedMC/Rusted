// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Dungeons: PNX `MonsterRoomFeature` + `ObjectMonsterRoom`, the last part of the
//! pregeneration carver (`OverworldCaveCarverFeature`).

use super::blocks::{self, s};
use super::{chunk_hash, java_hash, Ctx, Region};

const NAME: &str = "minecraft:monster_room";
/// PNX ObjectMonsterRoom.MOBS: zombies twice as likely.
const MOBS: [&str; 4] = ["skeleton", "zombie", "zombie", "spider"];
/// `MinecraftCardinalDirection.values()` / horizontal faces order, with their (dx, dz).
const FACES: [(&str, i32, i32); 4] = [("south", 0, 1), ("west", -1, 0), ("north", 0, -1), ("east", 1, 0)];

pub fn monster_rooms(r: &mut Region, ctx: &mut Ctx) {
    // Java precedence: seed ^ (chunkHash + name.hashCode()).
    ctx.random.set_seed(ctx.seed ^ chunk_hash(ctx.cx, ctx.cz).wrapping_add(java_hash(NAME) as i64));
    let (sx, sz) = (ctx.cx << 4, ctx.cz << 4);
    'chance: for _ in 0..8 {
        let rnd = &mut ctx.random;
        let x = sx + rnd.next_bounded_int(16) + 8;
        let y = rnd.next_bounded_int(384) - 64;
        let z = sz + rnd.next_bounded_int(16) + 8;
        let xv = rnd.next_bounded_int(2) + 2;
        let (x1, x2) = (-xv - 1, xv + 1);
        let zv = rnd.next_bounded_int(2) + 2;
        let (z1, z2) = (-zv - 1, zv + 1);
        let mut openings = 0;
        for dx in x1..=x2 {
            for dy in -1..=4 {
                for dz in z1..=z2 {
                    let (tx, ty, tz) = (x + dx, y + dy, z + dz);
                    let solid = blocks::is_solid(r.get(tx, ty, tz));
                    if (dy == -1 || dy == 4) && !solid {
                        continue 'chance;
                    }
                    if (dx == x1 || dx == x2 || dz == z1 || dz == z2) && dy == 0 && blocks::is_air(r.get(tx, ty + 1, tz)) {
                        openings += 1;
                    }
                }
            }
        }
        if (1..=5).contains(&openings) {
            room(r, ctx, (x, y, z));
        }
    }
}

/// PNX `ObjectMonsterRoom.generate`.
fn room(r: &mut Region, ctx: &mut Ctx, (x, y, z): (i32, i32, i32)) {
    let (cobble, mossy, air) = (blocks::id("cobblestone"), blocks::id("mossy_cobblestone"), blocks::id("air"));
    // fill(-3,-1,-3 .. 3,3,3, cobblestone walls, air inside)
    for dx in -3i32..=3 {
        for dy in -1..=3 {
            for dz in -3i32..=3 {
                let edge = dx.abs() == 3 || dz.abs() == 3 || dy == -1 || dy == 3;
                r.set(x + dx, y + dy, z + dz, if edge { cobble } else { air });
            }
        }
    }
    for dx in -2..=2 {
        for dz in -2..=2 {
            let floor = if ctx.random.next_int_below(4) == 0 { cobble } else { mossy };
            r.set(x + dx, y - 1, z + dz, floor);
            r.set(x + dx, y + 3, z + dz, air);
        }
    }
    for _ in 0..2 {
        let cx = if ctx.random.next_boolean() { -2 } else { 2 };
        let cz = ctx.random.next_int_below(5) - 2;
        let (px, pz) = (x + cx, z + cz);
        if let Some(&(face, _, _)) = FACES.iter().find(|(_, dx, dz)| blocks::is_solid(r.get(px + dx, y, pz + dz))) {
            r.set(px, y, pz, blocks::with(blocks::id("chest"), &[("minecraft:cardinal_direction", s(face))]));
            crate::loot::mark((px, y, pz), "monster_room");
        }
    }
    r.set(x, y, z, blocks::id("mob_spawner"));
    // PNX picks the mob with an unseeded random; a position hash keeps it the same every load.
    let h = (x as u32).wrapping_mul(0x9E37_79B9) ^ (y as u32).wrapping_mul(0x85EB_CA6B) ^ (z as u32).wrapping_mul(0xC2B2_AE35);
    crate::loot::spawners().lock().insert((x, y, z), MOBS[(h >> 16) as usize % MOBS.len()]);
}

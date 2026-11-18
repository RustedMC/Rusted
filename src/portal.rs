// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Nether and End portals (from PNX BlockNetherPortal.trySpawnPortal, PortalHelper,
//! Level.calculatePortalMirror, BlockEndPortalFrame and the End spawn platform).

use crate::blocks::{State, any_state, entry, state, with};
use crate::dims;
use crate::world::{AIR, World};

type Pos = (i32, i32, i32);

fn name(b: i32) -> &'static str {
    entry(b).map_or("", |e| e.name.as_str())
}

fn is(b: i32, n: &str) -> bool {
    name(b) == n
}

/// Lighting fire inside an obsidian frame (2-21 wide, 3-21 tall, either axis) fills it
/// with portal. None if there's no frame.
pub fn light_nether(w: &World, p: Pos) -> Option<Vec<(Pos, i32)>> {
    let portal = any_state("minecraft:portal")?;
    for (axis, (dx, dz)) in [("x", (1, 0)), ("z", (0, 1))] {
        // go down to the bottom of the frame and left to the edge
        let mut base = p;
        while w.get(base.0, base.1 - 1, base.2) == AIR && p.1 - base.1 < 21 {
            base.1 -= 1;
        }
        if !is(w.get(base.0, base.1 - 1, base.2), "minecraft:obsidian") {
            continue;
        }
        let mut left = base;
        while w.get(left.0 - dx, left.1, left.2 - dz) == AIR && (base.0 - left.0).abs() + (base.2 - left.2).abs() < 21 {
            left = (left.0 - dx, left.1, left.2 - dz);
        }
        if !is(w.get(left.0 - dx, left.1, left.2 - dz), "minecraft:obsidian") {
            continue;
        }
        let width = (0..22).find(|&i| w.get(left.0 + dx * i, left.1, left.2 + dz * i) != AIR)?;
        if !(2..=21).contains(&width) || !is(w.get(left.0 + dx * width, left.1, left.2 + dz * width), "minecraft:obsidian") {
            continue;
        }
        let height = (0..22).find(|&j| w.get(left.0, left.1 + j, left.2) != AIR)?;
        if !(3..=21).contains(&height) {
            continue;
        }
        // inside must be all air, edges all obsidian
        let mut ok = true;
        let mut out = Vec::new();
        for i in 0..width {
            for j in 0..height {
                let q = (left.0 + dx * i, left.1 + j, left.2 + dz * i);
                if w.get(q.0, q.1, q.2) != AIR && !is(w.get(q.0, q.1, q.2), "minecraft:fire") {
                    ok = false;
                }
                out.push((q, with(portal, &[("portal_axis", State::Str(axis.into()))])));
            }
            for y in [left.1 - 1, left.1 + height] {
                ok &= is(w.get(left.0 + dx * i, y, left.2 + dz * i), "minecraft:obsidian");
            }
        }
        for j in 0..height {
            ok &= is(w.get(left.0 - dx, left.1 + j, left.2 - dz), "minecraft:obsidian");
            ok &= is(w.get(left.0 + dx * width, left.1 + j, left.2 + dz * width), "minecraft:obsidian");
        }
        if ok {
            return Some(out);
        }
    }
    None
}

/// Portal destination. The nether is 8x smaller.
pub fn mirror(from: (f32, f32, f32)) -> (u8, i32, i32) {
    match dims::of_f(from.1) {
        dims::NETHER => (dims::OVERWORLD, (from.0 * 8.0).floor() as i32, (from.2 * 8.0).floor() as i32),
        _ => (dims::NETHER, (from.0 / 8.0).floor() as i32, (from.2 / 8.0).floor() as i32),
    }
}

/// Looks for an existing portal near (x, z). PNX searches 128 blocks in the overworld
/// and 16 in the nether.
pub fn find_portal(w: &World, dim: u8, x: i32, z: i32) -> Option<Pos> {
    let reach = if dim == dims::NETHER { 16 } else { 128 };
    w.placed(|b| is(b, "minecraft:portal"))
        .into_iter()
        .filter(|p| dims::of(p.1) == dim && (p.0 - x).abs() <= reach && (p.2 - z).abs() <= reach)
        .min_by_key(|p| (p.0 - x).pow(2) + (p.2 - z).pow(2) + p.1)
}

/// Builds a 4x5 portal somewhere safe (PNX PortalHelper). Takes the first 3-high air gap
/// over ground from the top down, or makes a platform in the middle.
/// Returns the changes and the bottom-left inner position.
pub fn build_portal(w: &World, dim: u8, x: i32, z: i32) -> (Vec<(Pos, i32)>, Pos) {
    let (lo, hi) = dims::range(dim);
    let top = if dim == dims::NETHER { lo + 120 } else { hi - 8 };
    let solid = |b: i32| crate::overworld::feature::blocks::is_solid(b);
    let y = (lo + 5..top)
        .rev()
        .find(|&y| solid(w.get(x, y - 1, z)) && (0..4).all(|j| (0..4).all(|i| w.get(x + i - 1, y + j, z) == AIR)))
        .unwrap_or(lo + if dim == dims::NETHER { 64 } else { 70 });
    let (Some(obs), Some(portal)) = (any_state("minecraft:obsidian"), any_state("minecraft:portal")) else { return (vec![], (x, y, z)) };
    let portal = with(portal, &[("portal_axis", State::Str("x".into()))]);
    let mut out = Vec::new();
    for i in -1..=2 {
        for j in -1..=3 {
            let edge = i == -1 || i == 2 || j == -1 || j == 3;
            out.push(((x + i, y + j, z), if edge { obs } else { portal }));
        }
        // air on both sides to walk out, ground to stand on
        for dz in [-1, 1] {
            for j in 0..3 {
                out.push(((x + i, y + j, z + dz), AIR));
            }
            if !solid(w.get(x + i, y - 1, z + dz)) {
                out.push(((x + i, y - 1, z + dz), obs));
            }
        }
    }
    (out, (x, y, z))
}

/// End arrival platform, PNX's 5x5 obsidian at 100, 48, 0 with air above.
/// Returns changes and the arrival eye position.
pub fn end_platform() -> (Vec<(Pos, i32)>, (f32, f32, f32)) {
    let base = dims::base(dims::END);
    let obs = any_state("minecraft:obsidian").unwrap_or(AIR);
    let mut out = Vec::new();
    for x in 98..=102 {
        for z in -2..=2 {
            out.push(((x, base + 48, z), obs));
            for y in 49..=51 {
                out.push(((x, base + y, z), AIR));
            }
        }
    }
    (out, (100.5, (base + 49) as f32 + 1.62, 0.5))
}

/// If this eye completes all 12 frames around a 3x3 hole, the hole turns into end portal
/// (PNX BlockEndPortalFrame.createPortal).
pub fn light_end(w: &World, frame: Pos) -> Option<Vec<(Pos, i32)>> {
    let portal = any_state("minecraft:end_portal")?;
    let eyed = |q: Pos| {
        let b = w.get(q.0, q.1, q.2);
        is(b, "minecraft:end_portal_frame") && matches!(state(b, "end_portal_eye_bit"), Some(State::Bool(true)))
    };
    for cx in frame.0 - 2..=frame.0 + 2 {
        for cz in frame.2 - 2..=frame.2 + 2 {
            let ring: Vec<Pos> = (-1..=1)
                .flat_map(|i| [(cx + i, frame.1, cz - 2), (cx + i, frame.1, cz + 2), (cx - 2, frame.1, cz + i), (cx + 2, frame.1, cz + i)])
                .collect();
            if ring.contains(&frame) && ring.iter().all(|&q| eyed(q)) {
                return Some((-1..=1).flat_map(|i| (-1..=1).map(move |k| ((cx + i, frame.1, cz + k), portal))).collect());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(n: &str) -> i32 {
        any_state(&format!("minecraft:{n}")).unwrap()
    }

    #[test]
    fn frame_lights_and_mirror() {
        crate::blocks::init();
        let w = World::flat(vec![]); // void
        let obs = block("obsidian");
        // 4x5 frame along x at y 100..104, 2x3 inside
        for i in 0..4 {
            w.set(i, 100, 0, obs);
            w.set(i, 104, 0, obs);
        }
        for j in 100..=104 {
            w.set(0, j, 0, obs);
            w.set(3, j, 0, obs);
        }
        let ch = light_nether(&w, (1, 101, 0)).expect("frame found");
        assert_eq!(ch.len(), 6, "2 x 3 inside");
        assert!(light_nether(&w, (10, 101, 10)).is_none());
        assert_eq!(mirror((80.0, 64.0, -16.0)), (dims::NETHER, 10, -2));
        assert_eq!(mirror((10.0, 1064.0, -2.0)), (dims::OVERWORLD, 80, -16));
    }

    #[test]
    fn end_ring_lights() {
        crate::blocks::init();
        let w = World::flat(vec![]);
        let frame = with(block("end_portal_frame"), &[("end_portal_eye_bit", State::Bool(true))]);
        for i in -1..=1 {
            for q in [(i, 50, -2), (i, 50, 2), (-2, 50, i), (2, 50, i)] {
                w.set(q.0, q.1, q.2, frame);
            }
        }
        assert_eq!(light_end(&w, (2, 50, 0)).map(|c| c.len()), Some(9));
    }
}

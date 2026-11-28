// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! from PNX SculkPatchFeature: deep dark sculk patches grown by charge cursors (the vanilla
//! sculk spreader), with sensors, shriekers and catalysts.
//! TODO: Region has no layer 1, so PNX's waterlogging writes (water/air on layer 1) are
//! dropped and layer-1 reads count as not waterlogged; RNG order is unaffected.

use super::super::blocks::{self, is_air, is_full, is_liquid, is_solid, replaceable, short};
use crate::overworld::biome::id as biome;
use super::super::{chunk_hash, Ctx, FeatureFn, Region, Xoroshiro};
use std::collections::HashMap;

pub fn register(m: &mut HashMap<&'static str, FeatureFn>) {
    m.insert("minecraft:sculk_patch_feature", apply);
}

type P = (i32, i32, i32);

/// PNX `BlockFace.values()` order: DOWN, UP, NORTH, SOUTH, WEST, EAST.
const FACES: [P; 6] = [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)];
const OPPOSITE: [usize; 6] = [1, 0, 3, 2, 5, 4];
/// `getDUSWNEIndex` of each face in `FACES` order.
const DUSWNE: [i32; 6] = [0, 1, 4, 2, 3, 5];
const DOWN: usize = 0;
const UP: usize = 1;
const NORTH: usize = 2;
const SOUTH: usize = 3;
const WEST: usize = 4;
const EAST: usize = 5;

const SCULK_FACING: i32 = -2;
const NO_FACING: i32 = -1;
const NO_GROWTH_RADIUS: i32 = 1;

fn side(p: P, f: usize) -> P {
    (p.0 + FACES[f].0, p.1 + FACES[f].1, p.2 + FACES[f].2)
}
fn axis(f: usize) -> usize {
    f / 2
}
fn bit(f: usize) -> i32 {
    1 << DUSWNE[f]
}
fn has(bits: i32, f: usize) -> bool {
    bits & bit(f) != 0
}
fn dist2(a: P, b: P) -> i32 {
    let (x, y, z) = (a.0 - b.0, a.1 - b.1, a.2 - b.2);
    x * x + y * y + z * z
}

#[derive(PartialEq, Clone, Copy)]
enum Behaviour {
    Default,
    Vein,
    Sculk,
}

struct Cursor {
    pos: P,
    charge: i32,
    update_delay: i32,
    decay_delay: i32,
    facings: i32,
}

struct S<'a> {
    r: &'a mut Region,
    rand: &'a mut Xoroshiro,
}

fn is_vein(b: i32) -> bool {
    short(b) == "sculk_vein"
}
fn is_sculkish(b: i32) -> bool {
    matches!(short(b), "sculk" | "sculk_vein")
}
fn behaviour(b: i32) -> Behaviour {
    match short(b) {
        "sculk_vein" => Behaviour::Vein,
        "sculk" => Behaviour::Sculk,
        _ => Behaviour::Default,
    }
}
fn is_water(b: i32) -> bool {
    blocks::is_water(b)
}
fn vein_bits(b: i32) -> i32 {
    match crate::blocks::state(b, "multi_face_direction_bits") {
        Some(crate::blocks::State::Int(v)) => *v,
        _ => 0,
    }
}
fn vein_state(bits: i32) -> i32 {
    blocks::with(blocks::id("sculk_vein"), &[("multi_face_direction_bits", blocks::i(bits))])
}
fn sculk_replaceable(n: &str) -> bool {
    matches!(
        n,
        "stone" | "granite" | "diorite" | "andesite" | "tuff" | "deepslate" | "dirt" | "grass_block" | "podzol" | "coarse_dirt" | "mycelium" | "dirt_with_roots"
            | "moss_block" | "pale_moss_block" | "mud" | "muddy_mangrove_roots" | "hardened_clay" | "crimson_nylium" | "warped_nylium" | "netherrack" | "basalt"
            | "blackstone" | "sand" | "red_sand" | "gravel" | "soul_sand" | "soul_soil" | "calcite" | "smooth_basalt" | "clay" | "dripstone_block" | "end_stone"
            | "red_sandstone" | "sandstone" | "sulfur" | "cinnabar"
    ) || (n.ends_with("_terracotta") && !n.contains("glazed"))
}
fn sculk_replaceable_world_gen(n: &str) -> bool {
    sculk_replaceable(n)
        || matches!(n, "deepslate_bricks" | "deepslate_tiles" | "cobbled_deepslate" | "cracked_deepslate_bricks" | "cracked_deepslate_tiles" | "polished_deepslate")
}

fn apply(r: &mut Region, ctx: &mut Ctx) {
    let (min_y, max_y) = (-63, 256);
    ctx.random.set_seed(ctx.seed ^ chunk_hash(ctx.cx, ctx.cz));
    let (bx, bz) = (ctx.cx << 4, ctx.cz << 4);
    let y_bound = max_y - min_y + 1;
    let mut s = S { r, rand: &mut ctx.random };
    for _ in 0..256 {
        let lx = s.rand.next_int_below(16);
        let y = min_y + s.rand.next_int_below(y_bound);
        let lz = s.rand.next_int_below(16);
        let origin = (bx + lx, y, bz + lz);
        if s.r.biome(origin.0, y, origin.2) != biome::DEEP_DARK {
            continue;
        }
        if s.can_spread_from(origin) {
            s.place_patch(origin);
        }
    }
}

impl S<'_> {
    fn get(&self, p: P) -> i32 {
        self.r.get(p.0, p.1, p.2)
    }
    fn set(&mut self, p: P, b: i32) {
        self.r.set(p.0, p.1, p.2, b)
    }

    fn place_patch(&mut self, origin: P) {
        // SPREAD_ROUNDS 1 + GROWTH_ROUNDS 0: one spreading round.
        let mut cursors: Vec<Cursor> = (0..10).map(|_| Cursor { pos: origin, charge: 32, update_delay: 0, decay_delay: 1, facings: NO_FACING }).collect();
        let mut attempt = 0;
        while attempt < 64 && !cursors.is_empty() {
            let mut i = 0;
            while i < cursors.len() {
                self.update_cursor(origin, &mut cursors[i], true);
                if cursors[i].charge <= 0 {
                    cursors.remove(i);
                } else {
                    i += 1;
                }
            }
            attempt += 1;
        }
        if self.rand.next_float() <= 0.5 && self.full_support(side(origin, DOWN)) {
            self.set(origin, blocks::id("sculk_catalyst"));
            // TODO: clearWaterloggedLayer (no layer 1).
        }
    }

    fn can_spread_from(&self, origin: P) -> bool {
        let start = self.get(origin);
        if is_sculkish(start) {
            return true;
        }
        if !is_air(start) && short(start) != "water" {
            return false;
        }
        (0..6).any(|f| self.full_support(side(origin, f)))
    }

    fn update_cursor(&mut self, origin: P, c: &mut Cursor, spread: bool) {
        if c.update_delay > 0 {
            c.update_delay -= 1;
            return;
        }
        let mut cur = self.get(c.pos);
        let mut beh = behaviour(cur);
        if spread && self.attempt_spread_vein(c, cur, beh) && beh != Behaviour::Sculk {
            cur = self.get(c.pos);
            beh = behaviour(cur);
        }
        c.charge = self.use_charge(origin, c, beh, spread);
        if c.charge <= 0 {
            self.on_discharged(c.pos, beh);
            return;
        }
        if let Some(t) = self.valid_movement_pos(c.pos) {
            self.on_discharged(c.pos, beh);
            c.pos = t;
            if dist2(c.pos, origin) >= 15 * 15 {
                c.charge = 0;
                return;
            }
            cur = self.get(c.pos);
            beh = behaviour(cur);
        }
        if is_sculkish(cur) {
            c.facings = if is_vein(cur) { vein_bits(cur) } else { SCULK_FACING };
        }
        c.decay_delay = if beh == Behaviour::Default { (c.decay_delay - 1).max(0) } else { 1 };
        c.update_delay = 1;
    }

    fn attempt_spread_vein(&mut self, c: &Cursor, cur: i32, beh: Behaviour) -> bool {
        if beh != Behaviour::Default {
            return self.spread_veins(c.pos);
        }
        if c.facings == NO_FACING {
            let mut spread = false;
            for f in 0..6 {
                spread |= self.place_vein_face(c.pos, c.pos, f);
            }
            return spread;
        }
        if c.facings == SCULK_FACING {
            return self.spread_veins(c.pos);
        }
        if c.facings != 0 {
            return (is_air(cur) || is_water(cur)) && self.regrow_vein(c.pos, c.facings);
        }
        self.spread_veins(c.pos)
    }

    fn use_charge(&mut self, origin: P, c: &Cursor, beh: Behaviour, spread: bool) -> i32 {
        match beh {
            Behaviour::Vein => {
                if spread && self.attempt_place_sculk(c.pos) {
                    return c.charge - 1;
                }
                if self.rand.next_int_below(5) == 0 { (c.charge as f32 * 0.5).floor() as i32 } else { c.charge }
            }
            Behaviour::Sculk => self.use_sculk_charge(origin, c),
            Behaviour::Default => {
                if c.decay_delay > 0 { c.charge } else { 0 }
            }
        }
    }

    fn use_sculk_charge(&mut self, origin: P, c: &Cursor) -> i32 {
        let charge = c.charge;
        if charge == 0 || self.rand.next_int_below(5) != 0 {
            return charge;
        }
        let close = dist2(c.pos, origin) < NO_GROWTH_RADIUS * NO_GROWTH_RADIUS;
        if !close && self.can_place_growth(c.pos) {
            if self.rand.next_int_below(50) < charge {
                let up = side(c.pos, UP);
                // TODO: waterlogging of the growth (no layer 1).
                let g = if self.rand.next_int_below(11) == 0 { blocks::with(blocks::id("sculk_shrieker"), &[("can_summon", blocks::b(true))]) } else { blocks::id("sculk_sensor") };
                self.set(up, g);
            }
            return (charge - 50).max(0);
        }
        if self.rand.next_int_below(10) != 0 {
            return charge;
        }
        charge - if close { 1 } else { decay_penalty(c.pos, origin, charge) }
    }

    fn can_place_growth(&self, p: P) -> bool {
        let above = self.get(side(p, UP));
        if !is_air(above) && !is_water(above) {
            return false;
        }
        let mut count = 0;
        for x in p.0 - 4..=p.0 + 4 {
            for y in p.1..=p.1 + 2 {
                for z in p.2 - 4..=p.2 + 4 {
                    if matches!(short(self.r.get(x, y, z)), "sculk_sensor" | "sculk_shrieker") {
                        count += 1;
                        if count > 2 {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    fn attempt_place_sculk(&mut self, p: P) -> bool {
        let vein = self.get(p);
        if !is_vein(vein) {
            return false;
        }
        let bits = vein_bits(vein);
        for sf in self.shuffled_faces() {
            if !has(bits, sf) {
                continue;
            }
            let sp = side(p, sf);
            if !sculk_replaceable_world_gen(short(self.get(sp))) {
                continue;
            }
            self.set(sp, blocks::id("sculk"));
            self.spread_veins(sp);
            let skip = OPPOSITE[sf];
            for f in 0..6 {
                if f == skip {
                    continue;
                }
                let vp = side(sp, f);
                if is_vein(self.get(vp)) {
                    self.discharge_vein(vp);
                }
            }
            return true;
        }
        false
    }

    fn regrow_vein(&mut self, p: P, facings: i32) -> bool {
        let mut valid = 0;
        for f in 0..6 {
            if has(facings, f) && is_solid(self.get(side(p, f))) {
                valid |= bit(f);
            }
        }
        if valid == 0 {
            return false;
        }
        self.set(p, vein_state(valid));
        true
    }

    fn spread_veins(&mut self, src: P) -> bool {
        let s = self.get(src);
        let sf = if is_vein(s) { vein_bits(s) } else { NO_FACING };
        let mut spread = false;
        for start in 0..6 {
            if sf != NO_FACING && !has(sf, start) {
                continue;
            }
            for dir in 0..6 {
                if axis(start) == axis(dir) {
                    continue;
                }
                spread |= self.spread_from_face(src, start, dir);
            }
        }
        spread
    }

    fn spread_from_face(&mut self, src: P, start: usize, dir: usize) -> bool {
        if self.place_vein_face(src, src, dir) {
            return true;
        }
        let same = side(src, dir);
        if self.place_vein_face(src, same, start) {
            return true;
        }
        self.place_vein_face(src, side(same, start), OPPOSITE[dir])
    }

    fn place_vein_face(&mut self, src: P, at: P, f: usize) -> bool {
        if !(-64..320).contains(&at.1) || !self.can_place_vein_face(src, at, f) {
            return false;
        }
        let existing = self.get(at);
        let bits = if is_vein(existing) { vein_bits(existing) } else { 0 };
        if bits & bit(f) != 0 {
            return false;
        }
        self.set(at, vein_state(bits | bit(f)));
        true
    }

    fn can_place_vein_face(&self, src: P, at: P, f: usize) -> bool {
        let e = self.get(at);
        let ok = is_vein(e) || is_air(e) || is_water(e) || !(is_liquid(e) || matches!(short(e), "fire" | "soul_fire")) && replaceable(e);
        if !ok {
            return false;
        }
        let support = self.get(side(at, f));
        if matches!(short(support), "sculk" | "sculk_catalyst" | "moving_block") {
            return false;
        }
        let m = (src.0 - at.0).abs() + (src.1 - at.1).abs() + (src.2 - at.2).abs();
        // shortcut: PNX's isSolid(face) is approximated with isSolid() (no per-face solidity).
        if m == 2 && is_solid(self.get(side(src, OPPOSITE[f]))) {
            return false;
        }
        is_solid(support)
    }

    fn valid_movement_pos(&mut self, p: P) -> Option<P> {
        let mut offsets = Vec::with_capacity(18);
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    if (x == 0 || y == 0 || z == 0) && (x != 0 || y != 0 || z != 0) {
                        offsets.push((x, y, z));
                    }
                }
            }
        }
        for i in (1..offsets.len()).rev() {
            let j = self.rand.next_int_below(i as i32 + 1) as usize;
            offsets.swap(i, j);
        }
        let mut sel = None;
        for o in offsets {
            let cand = (p.0 + o.0, p.1 + o.1, p.2 + o.2);
            let st = self.get(cand);
            if !is_sculkish(st) || !self.movement_unobstructed(p, cand) {
                continue;
            }
            sel = Some(cand);
            if is_vein(st) {
                let bits = vein_bits(st);
                if (0..6).any(|f| has(bits, f) && sculk_replaceable(short(self.get(side(cand, f))))) {
                    break;
                }
            }
        }
        sel
    }

    fn movement_unobstructed(&self, from: P, to: P) -> bool {
        let (dx, dy, dz) = (to.0 - from.0, to.1 - from.1, to.2 - from.2);
        if dx.abs() + dy.abs() + dz.abs() == 1 {
            return true;
        }
        let pick = |d: i32, n, p| if d < 0 { n } else { p };
        let first = if dx != 0 { pick(dx, WEST, EAST) } else { pick(dy, DOWN, UP) };
        let second = if dz != 0 { pick(dz, NORTH, SOUTH) } else { pick(dy, DOWN, UP) };
        !is_solid(self.get(side(from, first))) || !is_solid(self.get(side(from, second)))
    }

    fn shuffled_faces(&mut self) -> [usize; 6] {
        let mut f = [0, 1, 2, 3, 4, 5];
        for i in (1..6).rev() {
            let j = self.rand.next_int_below(i as i32 + 1) as usize;
            f.swap(i, j);
        }
        f
    }

    fn on_discharged(&mut self, p: P, beh: Behaviour) {
        if beh == Behaviour::Vein {
            self.discharge_vein(p);
        }
    }

    fn discharge_vein(&mut self, p: P) {
        let b = self.get(p);
        if !is_vein(b) {
            return;
        }
        let mut bits = vein_bits(b);
        for f in 0..6 {
            if has(bits, f) && short(self.get(side(p, f))) == "sculk" {
                bits &= !bit(f);
            }
        }
        // Not waterlogged (no layer 1), so PNX's water branch never runs.
        self.set(p, if bits != 0 { vein_state(bits) } else { blocks::air() });
    }

    fn full_support(&self, p: P) -> bool {
        let b = self.get(p);
        !is_air(b) && !replaceable(b) && is_full(b) && is_solid(b)
    }
}

fn decay_penalty(p: P, origin: P, charge: i32) -> i32 {
    let d = (dist2(p, origin) as f64).sqrt() as f32;
    let outer = d - NO_GROWTH_RADIUS as f32;
    let k = ((24 - NO_GROWTH_RADIUS) * (24 - NO_GROWTH_RADIUS)) as f32;
    let factor = (outer * outer / k).min(1.0);
    1.max((charge as f32 * factor * 0.5) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_and_states() {
        for f in 0..6 {
            assert_eq!(OPPOSITE[OPPOSITE[f]], f);
            assert_eq!(axis(f), axis(OPPOSITE[f]));
        }
        assert_eq!(vein_bits(vein_state(0b101)), 0b101);
        assert!(sculk_replaceable("red_terracotta") && !sculk_replaceable("red_glazed_terracotta"));
        assert!(sculk_replaceable_world_gen("polished_deepslate") && !sculk_replaceable("polished_deepslate"));
        assert!(decay_penalty((10, 0, 0), (0, 0, 0), 32) >= 1);
    }
}

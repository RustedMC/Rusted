// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
// Ported from PNX EndCityPieces
//! End city piece layout.
//! TODO: loot, banners, item frames and brewing stands need block entities.

use super::templates::{self, Rot};
use crate::overworld::feature::Xoroshiro;

const MAX_GEN_DEPTH: i32 = 8;
// PNX Rotation ordinal: NONE, ROTATE_90, ROTATE_180, ROTATE_270.
const TOWER_BRIDGES: [[i32; 4]; 4] = [[0, 1, -1, 0], [1, 6, -1, 1], [3, 0, -1, 5], [2, 5, -1, 6]];
const FAT_TOWER_BRIDGES: [[i32; 4]; 4] = [[0, 4, -1, 0], [1, 12, -1, 4], [3, 0, -1, 8], [2, 8, -1, 12]];

#[derive(Clone, Copy)]
struct Bb([i32; 6]);

impl Bb {
    fn intersects(&self, o: &Bb) -> bool {
        let (a, b) = (self.0, o.0);
        a[3] >= b[0] && a[0] <= b[3] && a[5] >= b[2] && a[2] <= b[5] && a[4] >= b[1] && a[1] <= b[4]
    }
}

#[derive(Clone)]
pub struct Piece {
    pub name: &'static str,
    size_x: i32,
    size_z: i32,
    pub rotation: i32,
    pub overwrite: bool,
    bb: Bb,
    pub pos: [i32; 3],
    gen_depth: i32,
}

/// PNX `toTemplateRotation`.
pub fn template_rot(r: i32) -> Rot {
    match r & 3 {
        1 => Rot::R270,
        2 => Rot::R180,
        3 => Rot::R90,
        _ => Rot::None,
    }
}

impl Piece {
    fn new(name: &'static str, pos: [i32; 3], rotation: i32, overwrite: bool) -> Option<Self> {
        let t = templates::get(name)?;
        let (rx, rz) = t.rotated_size(template_rot(rotation));
        let bb = Bb([pos[0], pos[1], pos[2], pos[0] + rx - 1, pos[1] + t.size[1] - 1, pos[2] + rz - 1]);
        Some(Self { name, size_x: t.size[0], size_z: t.size[2], rotation, overwrite, bb, pos, gen_depth: 0 })
    }

    fn mv(&mut self, x: i32, y: i32, z: i32) {
        self.pos = [self.pos[0] + x, self.pos[1] + y, self.pos[2] + z];
        for (i, d) in [x, y, z, x, y, z].into_iter().enumerate() {
            self.bb.0[i] += d;
        }
    }
}

fn transform(v: [i32; 3], sx: i32, sz: i32, r: i32) -> [i32; 3] {
    match r & 3 {
        1 => [sz - 1 - v[2], v[1], v[0]],
        2 => [sx - 1 - v[0], v[1], sz - 1 - v[2]],
        3 => [v[2], v[1], sx - 1 - v[0]],
        _ => v,
    }
}

/// `None` on a missing template (PNX throws, we skip the city).
fn add_piece(parent: &Piece, off: [i32; 3], name: &'static str, rotation: i32, overwrite: bool) -> Option<Piece> {
    let mut c = Piece::new(name, parent.pos, rotation, overwrite)?;
    let po = transform(off, parent.size_x, parent.size_z, parent.rotation);
    let co = transform([0, 0, 0], c.size_x, c.size_z, c.rotation);
    let o = [parent.pos[0] + po[0] - co[0], parent.pos[1] + po[1] - co[1], parent.pos[2] + po[2] - co[2]];
    c.mv(o[0] - c.pos[0], o[1] - c.pos[1], o[2] - c.pos[2]);
    Some(c)
}

/// Pushes and returns the index.
fn add(pieces: &mut Vec<Piece>, p: Piece) -> usize {
    pieces.push(p);
    pieces.len() - 1
}

#[derive(Clone, Copy)]
enum Gen {
    House,
    Tower,
    Bridge,
    Fat,
}

struct Ctx<'a> {
    r: &'a mut Xoroshiro,
    ship_created: bool,
}

type R<T> = Option<T>;

fn recursive(cx: &mut Ctx, g: Gen, depth: i32, parent: &Piece, off: Option<[i32; 3]>, pieces: &mut Vec<Piece>) -> R<bool> {
    if depth > MAX_GEN_DEPTH {
        return Some(false);
    }
    let mut child = vec![];
    if generate(cx, g, depth, parent, off, &mut child)? {
        let tag = cx.r.next_int();
        let mut collision = false;
        for c in child.iter_mut() {
            c.gen_depth = tag;
            if let Some(hit) = pieces.iter().find(|p| p.bb.intersects(&c.bb))
                && hit.gen_depth != parent.gen_depth {
                    collision = true;
                    break;
                }
        }
        if !collision {
            pieces.extend(child);
            return Some(true);
        }
    }
    Some(false)
}

fn generate(cx: &mut Ctx, g: Gen, depth: i32, parent: &Piece, off: Option<[i32; 3]>, pieces: &mut Vec<Piece>) -> R<bool> {
    let rot = parent.rotation;
    match g {
        Gen::House => {
            if depth > MAX_GEN_DEPTH {
                return Some(false);
            }
            let off = off.unwrap_or([0, 0, 0]);
            let mut last = add(pieces, add_piece(parent, off, "base_floor", rot, true)?);
            let floors = cx.r.next_int_below(3);
            if floors == 0 {
                add(pieces, add_piece(&pieces[last].clone(), [-1, 4, -1], "base_roof", rot, true)?);
            } else {
                last = add(pieces, add_piece(&pieces[last].clone(), [-1, 0, -1], "second_floor_2", rot, false)?);
                if floors == 1 {
                    last = add(pieces, add_piece(&pieces[last].clone(), [-1, 8, -1], "second_roof", rot, false)?);
                } else {
                    last = add(pieces, add_piece(&pieces[last].clone(), [-1, 4, -1], "third_floor_2", rot, false)?);
                    last = add(pieces, add_piece(&pieces[last].clone(), [-1, 8, -1], "third_roof", rot, true)?);
                }
                let p = pieces[last].clone();
                recursive(cx, Gen::Tower, depth + 1, &p, None, pieces)?;
            }
            Some(true)
        }
        Gen::Tower => {
            let ox = 3 + cx.r.next_int_below(2);
            let oz = 3 + cx.r.next_int_below(2);
            let mut last = add(pieces, add_piece(parent, [ox, -3, oz], "tower_base", rot, true)?);
            last = add(pieces, add_piece(&pieces[last].clone(), [0, 7, 0], "tower_piece", rot, true)?);
            let mut bridge = if cx.r.next_int_below(3) == 0 { Some(last) } else { None };
            let h = 1 + cx.r.next_int_below(3);
            for i in 0..h {
                last = add(pieces, add_piece(&pieces[last].clone(), [0, 4, 0], "tower_piece", rot, true)?);
                if i < h - 1 && cx.r.next_boolean() {
                    bridge = Some(last);
                }
            }
            if let Some(b) = bridge {
                for br in TOWER_BRIDGES {
                    if cx.r.next_boolean() {
                        let s = add(pieces, add_piece(&pieces[b].clone(), [br[1], br[2], br[3]], "bridge_end", (rot + br[0]) & 3, true)?);
                        let p = pieces[s].clone();
                        recursive(cx, Gen::Bridge, depth + 1, &p, None, pieces)?;
                    }
                }
                add(pieces, add_piece(&pieces[last].clone(), [-1, 4, -1], "tower_top", rot, true)?);
            } else {
                if depth != 7 {
                    let p = pieces[last].clone();
                    return recursive(cx, Gen::Fat, depth + 1, &p, None, pieces);
                }
                add(pieces, add_piece(&pieces[last].clone(), [-1, 4, -1], "tower_top", rot, true)?);
            }
            Some(true)
        }
        Gen::Bridge => {
            let len = cx.r.next_int_below(4) + 1;
            let mut last = add(pieces, add_piece(parent, [0, 0, -4], "bridge_piece", rot, true)?);
            pieces[last].gen_depth = -1;
            let mut ny = 0;
            for _ in 0..len {
                let p = pieces[last].clone();
                if cx.r.next_boolean() {
                    last = add(pieces, add_piece(&p, [0, ny, -4], "bridge_piece", rot, true)?);
                    ny = 0;
                } else {
                    last = if cx.r.next_boolean() {
                        add(pieces, add_piece(&p, [0, ny, -4], "bridge_steep_stairs", rot, true)?)
                    } else {
                        add(pieces, add_piece(&p, [0, ny, -8], "bridge_gentle_stairs", rot, true)?)
                    };
                    ny = 4;
                }
            }
            let p = pieces[last].clone();
            if !cx.ship_created && cx.r.next_int_below(10 - depth) == 0 {
                let sx = -8 + cx.r.next_int_below(8);
                let sz = -70 + cx.r.next_int_below(10);
                add(pieces, add_piece(&p, [sx, ny, sz], "ship", rot, true)?);
                cx.ship_created = true;
            } else if !recursive(cx, Gen::House, depth + 1, &p, Some([-3, ny + 1, -11]), pieces)? {
                return Some(false);
            }
            last = add(pieces, add_piece(&p, [4, ny, 0], "bridge_end", (rot + 2) & 3, true)?);
            pieces[last].gen_depth = -1;
            Some(true)
        }
        Gen::Fat => {
            let mut last = add(pieces, add_piece(parent, [-3, 4, -3], "fat_tower_base", rot, true)?);
            last = add(pieces, add_piece(&pieces[last].clone(), [0, 4, 0], "fat_tower_middle", rot, true)?);
            let mut i = 0;
            while i < 2 && cx.r.next_int_below(3) != 0 {
                last = add(pieces, add_piece(&pieces[last].clone(), [0, 8, 0], "fat_tower_middle", rot, true)?);
                for br in FAT_TOWER_BRIDGES {
                    if cx.r.next_boolean() {
                        let s = add(pieces, add_piece(&pieces[last].clone(), [br[1], br[2], br[3]], "bridge_end", (rot + br[0]) & 3, true)?);
                        let p = pieces[s].clone();
                        recursive(cx, Gen::Bridge, depth + 1, &p, None, pieces)?;
                    }
                }
                i += 1;
            }
            add(pieces, add_piece(&pieces[last].clone(), [-2, 8, -2], "fat_tower_top", rot, true)?);
            Some(true)
        }
    }
}

/// PNX `EndCityPieces.generate`; `None` when a template is missing.
pub fn pieces(origin: [i32; 3], rotation: i32, r: &mut Xoroshiro) -> Option<Vec<Piece>> {
    let mut cx = Ctx { r, ship_created: false };
    let mut pieces = vec![];
    let mut last = add(&mut pieces, Piece::new("base_floor", origin, rotation, true)?);
    last = { let p = pieces[last].clone(); add(&mut pieces, add_piece(&p, [-1, 0, -1], "second_floor_1", rotation, false)?) };
    last = { let p = pieces[last].clone(); add(&mut pieces, add_piece(&p, [-1, 4, -1], "third_floor_1", rotation, false)?) };
    last = { let p = pieces[last].clone(); add(&mut pieces, add_piece(&p, [-1, 8, -1], "third_roof", rotation, true)?) };
    let p = pieces[last].clone();
    recursive(&mut cx, Gen::Tower, 1, &p, None, &mut pieces)?;
    Some(pieces)
}

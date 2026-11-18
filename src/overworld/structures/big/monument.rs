// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Ocean monuments: PNX `OceanMonumentPopulator`, `OceanMonumentPlacement` and
//! `OceanMonumentPieces`. Elder guardians are not spawned.
#![allow(non_snake_case, unused_mut, unused_variables)]

use std::sync::OnceLock;

use super::{BB, start_random};
use crate::overworld::feature::blocks::{air, id, is, is_water};
use crate::overworld::feature::objects::{Face, World};
use crate::overworld::rng::{NukkitRandom, Xoroshiro};
use crate::overworld::structures::{Placement, Spread, StructWorld, StructureType};

const PLACEMENT: Placement = Placement::new(10_387_313, 5, 32, Spread::Triangular);
const SEA_LEVEL: i32 = 63;

pub fn register(v: &mut Vec<StructureType>) {
    // Building spans chunk offsets -21..=36, water boxes reach 5 more, so chunks -2..=2.
    v.push(StructureType { name: "normal_ocean_monument", radius: 2, hint: |s, x, z| PLACEMENT.is_candidate(s, x, z), build });
}

fn is_deep_ocean(b: u16) -> bool {
    matches!(b, 24 | 41 | 43 | 45 | 47)
}

fn is_ocean_or_river(b: u16) -> bool {
    matches!(b, 0 | 7 | 11 | 24 | 40 | 41 | 42 | 43 | 44 | 45 | 46 | 47)
}

/// `OceanMonumentPlacement.areBiomesValid`.
fn biomes_valid(w: &StructWorld, cx: i32, cz: i32, radius: i32, deep_only: bool) -> bool {
    let (x, z) = ((cx << 4) + 8, (cz << 4) + 8);
    for qx in (x - radius) >> 2..=(x + radius) >> 2 {
        for qz in (z - radius) >> 2..=(z + radius) >> 2 {
            let b = w.biome(qx << 2, SEA_LEVEL, qz << 2);
            if if deep_only { !is_deep_ocean(b) } else { !is_ocean_or_river(b) } {
                return false;
            }
        }
    }
    true
}

/// Raw NukkitRandom int. 32 nextBoolean calls on a fresh random use exactly one int.
fn nukkit_next_int(r: &mut NukkitRandom) -> i32 {
    (0..32).fold(0u32, |v, i| v | ((r.next_boolean() as u32) << i)) as i32
}

fn build(w: &mut StructWorld, _r: &mut Xoroshiro, cx: i32, cz: i32) {
    let seed = w.seed;
    if !PLACEMENT.is_candidate(seed, cx, cz) || !biomes_valid(w, cx, cz, 16, true) || !biomes_valid(w, cx, cz, 29, false) {
        return;
    }
    let mut r = start_random(seed, cx, cz);
    let o = Face::random(&mut r);
    let (pieces, rooms) = create(&mut r, (cx << 4) - 21, (cz << 4) - 21, o);
    let mut cr = NukkitRandom::new(seed);
    let (r1, r2) = (nukkit_next_int(&mut cr) as i64, nukkit_next_int(&mut cr) as i64);
    let bb = pieces[0].bb;
    for tx in bb.x0 >> 4..=bb.x1 >> 4 {
        for tz in bb.z0 >> 4..=bb.z1 >> 4 {
            let cb = BB::column(tx, tz);
            let mut pr = NukkitRandom::new((tx as i64).wrapping_mul(r1) ^ (tz as i64).wrapping_mul(r2) ^ seed);
            let mut c = Ctx { w, cb, pb: bb, o };
            pp_monument_building(&mut c, &rooms, &pieces[0], &mut pr);
            for p in &pieces[1..] {
                if p.bb.intersects(&cb) {
                    c.pb = p.bb;
                    let f = match p.kind {
                        Kind::Entry => pp_entry_room,
                        Kind::Simple => pp_simple_room,
                        Kind::SimpleTop => pp_simple_top_room,
                        Kind::DoubleY => pp_double_y_room,
                        Kind::DoubleX => pp_double_x_room,
                        Kind::DoubleZ => pp_double_z_room,
                        Kind::DoubleXY => pp_double_x_y_room,
                        Kind::DoubleYZ => pp_double_y_z_room,
                        Kind::Core => pp_core_room,
                        Kind::Wing => pp_wing_room,
                        Kind::Penthouse => pp_penthouse,
                    };
                    f(&mut c, &rooms, p, &mut pr);
                }
            }
        }
    }
}

// ---- room graph ---------------------------------------------------------------------------

const DOWN: usize = 0;
const UP: usize = 1;
const NORTH: usize = 2;
const SOUTH: usize = 3;
const WEST: usize = 4;
const EAST: usize = 5;
/// BlockFace unit vectors in D-U-N-S-W-E order.
const UNIT: [(i32, i32, i32); 6] = [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)];

struct Room {
    index: i32,
    conn: [Option<usize>; 6],
    open: [bool; 6],
    claimed: bool,
    is_source: bool,
    scan: i32,
}

impl Room {
    fn count_openings(&self) -> i32 {
        self.open.iter().filter(|&&o| o).count() as i32
    }
}

fn set_connection(rooms: &mut [Room], a: usize, face: usize, b: usize) {
    rooms[a].conn[face] = Some(b);
    rooms[b].conn[face ^ 1] = Some(a);
}

fn find_source(rooms: &mut [Room], i: usize, scan: i32) -> bool {
    if rooms[i].is_source {
        return true;
    }
    rooms[i].scan = scan;
    for f in 0..6 {
        if let Some(c) = rooms[i].conn[f]
            && rooms[i].open[f] && rooms[c].scan != scan && find_source(rooms, c, scan) {
                return true;
            }
    }
    false
}

fn room_index(x: i32, y: i32, z: i32) -> i32 {
    y * 25 + z * 5 + x
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Entry,
    Simple,
    SimpleTop,
    DoubleY,
    DoubleX,
    DoubleZ,
    DoubleXY,
    DoubleYZ,
    Core,
    Wing,
    Penthouse,
}

struct Piece {
    kind: Kind,
    bb: BB,
    room: usize,
    design: i32,
}

/// `OceanMonumentPiece(genDepth, orientation, roomDefinition, x, y, z)` bounding box.
fn room_box(o: Face, index: i32, x: i32, y: i32, z: i32) -> BB {
    let (ix, iz, iy) = (index % 5, index / 5 % 5, index / 25);
    let mut bb = if matches!(o, Face::North | Face::South) {
        BB::new(0, 0, 0, x * 8 - 1, y * 4 - 1, z * 8 - 1)
    } else {
        BB::new(0, 0, 0, z * 8 - 1, y * 4 - 1, x * 8 - 1)
    };
    match o {
        Face::North => bb.mv(ix * 8, iy * 4, -(iz + z) * 8 + 1),
        Face::South => bb.mv(ix * 8, iy * 4, iz * 8),
        Face::West => bb.mv(-(iz + z) * 8 + 1, iy * 4, ix * 8),
        Face::East => bb.mv(iz * 8, iy * 4, ix * 8),
    }
    bb
}

/// `MonumentBuilding` constructor: the building (piece 0) and its child pieces, in PNX order.
fn create(r: &mut Xoroshiro, x: i32, z: i32, o: Face) -> (Vec<Piece>, Vec<Room>) {
    let building = BB::new(x, 39, z, x + 57, 61, z + 57);
    let (rooms, order, source, core) = room_graph(r);
    let mut rooms = rooms;
    rooms[source].claimed = true;
    let mut pieces = vec![Piece { kind: Kind::Entry, bb: BB::new(0, 0, 0, 0, 0, 0), room: usize::MAX, design: 0 }];
    let mk = |kind, room: usize, rooms: &[Room], sx, sy, sz, design| Piece { kind, bb: room_box(o, rooms[room].index, sx, sy, sz), room, design };
    pieces.push(mk(Kind::Entry, source, &rooms, 1, 1, 1, 0));
    pieces.push(mk(Kind::Core, core, &rooms, 2, 2, 2, 0));
    for &i in &order {
        if rooms[i].claimed || rooms[i].index >= 75 {
            continue;
        }
        let rd = &rooms[i];
        let free = |f: usize| rd.open[f] && !rooms[rd.conn[f].unwrap()].claimed;
        let up_free = |j: usize| rooms[j].open[UP] && !rooms[rooms[j].conn[UP].unwrap()].claimed;
        let fit = if free(EAST) && free(UP) && up_free(rd.conn[EAST].unwrap()) {
            Kind::DoubleXY
        } else if free(NORTH) && free(UP) && up_free(rd.conn[NORTH].unwrap()) {
            Kind::DoubleYZ
        } else if free(NORTH) {
            Kind::DoubleZ
        } else if free(EAST) {
            Kind::DoubleX
        } else if free(UP) {
            Kind::DoubleY
        } else if !rd.open[WEST] && !rd.open[EAST] && !rd.open[NORTH] && !rd.open[SOUTH] && !rd.open[UP] {
            Kind::SimpleTop
        } else {
            Kind::Simple
        };
        let conn = |rooms: &[Room], j: usize, f: usize| rooms[j].conn[f].unwrap();
        let mut room = i;
        rooms[i].claimed = true;
        match fit {
            Kind::DoubleXY => {
                let e = conn(&rooms, i, EAST);
                let (u, eu) = (conn(&rooms, i, UP), conn(&rooms, e, UP));
                for j in [e, u, eu] {
                    rooms[j].claimed = true;
                }
            }
            Kind::DoubleYZ => {
                let n = conn(&rooms, i, NORTH);
                let (u, nu) = (conn(&rooms, i, UP), conn(&rooms, n, UP));
                for j in [n, u, nu] {
                    rooms[j].claimed = true;
                }
            }
            Kind::DoubleZ => {
                // FitDoubleZRoom.create re-checks the north room (always fits here).
                if !rooms[i].open[NORTH] || rooms[conn(&rooms, i, NORTH)].claimed {
                    room = conn(&rooms, i, SOUTH);
                }
                rooms[room].claimed = true;
                let n = conn(&rooms, room, NORTH);
                rooms[n].claimed = true;
            }
            Kind::DoubleX => {
                let e = conn(&rooms, i, EAST);
                rooms[e].claimed = true;
            }
            Kind::DoubleY => {
                let u = conn(&rooms, i, UP);
                rooms[u].claimed = true;
            }
            _ => {}
        }
        let (sx, sy, sz) = match fit {
            Kind::DoubleXY => (2, 2, 1),
            Kind::DoubleYZ => (1, 2, 2),
            Kind::DoubleZ => (1, 1, 2),
            Kind::DoubleX => (2, 1, 1),
            Kind::DoubleY => (1, 2, 1),
            _ => (1, 1, 1),
        };
        let design = if fit == Kind::Simple { r.next_bounded_int(3) } else { 0 };
        pieces.push(mk(fit, room, &rooms, sx, sy, sz, design));
    }
    let wb = Ctx::world_fns(building, o);
    let (dx, dz) = (wb.0(9, 22), wb.1(9, 22));
    for p in &mut pieces[1..] {
        p.bb.mv(dx, building.y0, dz);
    }
    let main_design = r.next_int();
    let (wx, wz, y0) = (wb.0, wb.1, building.y0);
    let sub = |ax, ay, az, bx, by, bz| BB::proper(wx(ax, az), y0 + ay, wz(ax, az), wx(bx, bz), y0 + by, wz(bx, bz));
    pieces.push(Piece { kind: Kind::Wing, bb: sub(1, 1, 1, 23, 8, 21), room: usize::MAX, design: main_design & 1 });
    pieces.push(Piece { kind: Kind::Wing, bb: sub(34, 1, 1, 56, 8, 21), room: usize::MAX, design: main_design.wrapping_add(1) & 1 });
    pieces.push(Piece { kind: Kind::Penthouse, bb: sub(22, 13, 22, 35, 17, 35), room: usize::MAX, design: 0 });
    pieces[0].bb = building;
    (pieces, rooms)
}

/// `MonumentBuilding.generateRoomGraph`: (arena, shuffled room order + specials, source, core).
fn room_graph(r: &mut Xoroshiro) -> (Vec<Room>, Vec<usize>, usize, usize) {
    let mut rooms: Vec<Room> = Vec::with_capacity(78);
    let mut defs = [None; 75];
    let mut add = |rooms: &mut Vec<Room>, index: i32| {
        rooms.push(Room { index, conn: [None; 6], open: [false; 6], claimed: false, is_source: false, scan: 0 });
        rooms.len() - 1
    };
    for (y, xs, zs) in [(0, 0..5, 0..4), (1, 0..5, 0..4), (2, 1..4, 0..2)] {
        for x in xs {
            for z in zs.clone() {
                let i = room_index(x, y, z);
                defs[i as usize] = Some(add(&mut rooms, i));
            }
        }
    }
    let source = defs[room_index(2, 0, 0) as usize].unwrap();
    for xi in 0..5 {
        for zi in 0..5 {
            for yi in 0..3 {
                let Some(a) = defs[room_index(xi, yi, zi) as usize] else { continue };
                for (f, (ux, uy, uz)) in UNIT.iter().enumerate() {
                    let (xx, yy, zz) = (xi + ux, yi + uy, zi + uz);
                    if (0..5).contains(&xx) && (0..5).contains(&zz) && (0..3).contains(&yy)
                        && let Some(b) = defs[room_index(xx, yy, zz) as usize] {
                            set_connection(&mut rooms, a, if zz == zi { f } else { f ^ 1 }, b);
                        }
                }
            }
        }
    }
    let r1003 = add(&mut rooms, 1003);
    let r1001 = add(&mut rooms, 1001);
    let r1002 = add(&mut rooms, 1002);
    set_connection(&mut rooms, defs[room_index(2, 2, 0) as usize].unwrap(), UP, r1003);
    set_connection(&mut rooms, defs[room_index(0, 1, 0) as usize].unwrap(), SOUTH, r1001);
    set_connection(&mut rooms, defs[room_index(4, 1, 0) as usize].unwrap(), SOUTH, r1002);
    for i in [r1003, r1001, r1002] {
        rooms[i].claimed = true;
    }
    rooms[source].is_source = true;
    let core = defs[room_index(r.next_bounded_int(3), 0, 2) as usize].unwrap();
    let c = |rooms: &[Room], j: usize, f: usize| rooms[j].conn[f].unwrap();
    let ce = c(&rooms, core, EAST);
    let cn = c(&rooms, core, NORTH);
    let cen = c(&rooms, ce, NORTH);
    let claim = [ce, cn, cen, c(&rooms, core, UP), c(&rooms, ce, UP), c(&rooms, cn, UP), c(&rooms, cen, UP), core];
    for j in claim {
        rooms[j].claimed = true;
    }
    let mut order: Vec<usize> = defs.iter().flatten().copied().collect();
    for &i in &order {
        for f in 0..6 {
            rooms[i].open[f] = rooms[i].conn[f].is_some();
        }
    }
    for f in 0..6 {
        rooms[r1003].open[f] = rooms[r1003].conn[f].is_some();
    }
    for i in (2..=order.len()).rev() {
        order.swap(i - 1, r.next_int_below(i as i32) as usize);
    }
    let mut find_count = 1;
    for &room in &order {
        let (mut found, mut count) = (0, 0);
        while found < 2 && count < 5 {
            count += 1;
            let f = r.next_bounded_int(5) as usize;
            if rooms[room].open[f] {
                let other = rooms[room].conn[f].unwrap();
                rooms[room].open[f] = false;
                rooms[other].open[f ^ 1] = false;
                let a = find_source(&mut rooms, room, find_count);
                find_count += 1;
                let ok = a && {
                    let b = find_source(&mut rooms, other, find_count);
                    find_count += 1;
                    b
                };
                if ok {
                    found += 1;
                } else {
                    rooms[room].open[f] = true;
                    rooms[other].open[f ^ 1] = true;
                }
            }
        }
    }
    order.extend([r1003, r1001, r1002]);
    (rooms, order, source, core)
}

// ---- piece drawing (StructurePiece / OceanMonumentPiece helpers) --------------------------

const AIR: u8 = 0;
const GRAY: u8 = 1;
const LIGHT: u8 = 2;
const BLACK: u8 = 3;
const LAMP: u8 = 4;
const WATER: u8 = 5;
const SPONGE: u8 = 6;
const GOLD: u8 = 7;

fn ids() -> &'static [i32; 8] {
    static I: OnceLock<[i32; 8]> = OnceLock::new();
    I.get_or_init(|| {
        ["prismarine", "prismarine_bricks", "dark_prismarine", "sea_lantern", "water", "wet_sponge", "gold_block"]
            .iter()
            .fold(([air(); 8], 1), |(mut a, n), name| {
                a[n] = id(name);
                (a, n + 1)
            })
            .0
    })
}

/// One chunk's pass over one piece: `cb` is the chunk box writes are clipped to.
struct Ctx<'a, 'w> {
    w: &'a mut StructWorld<'w>,
    cb: BB,
    pb: BB,
    o: Face,
}

impl Ctx<'_, '_> {
    /// `getWorldX` / `getWorldZ` for a piece box and orientation.
    fn world_fns(pb: BB, o: Face) -> (impl Fn(i32, i32) -> i32, impl Fn(i32, i32) -> i32) {
        let fx = move |x: i32, z: i32| match o {
            Face::North | Face::South => pb.x0 + x,
            Face::West => pb.x1 - z,
            Face::East => pb.x0 + z,
        };
        let fz = move |x: i32, z: i32| match o {
            Face::North => pb.z1 - z,
            Face::South => pb.z0 + z,
            Face::West | Face::East => pb.z0 + x,
        };
        (fx, fz)
    }

    fn pos(&self, x: i32, y: i32, z: i32) -> (i32, i32, i32) {
        let (fx, fz) = Self::world_fns(self.pb, self.o);
        (fx(x, z), y + self.pb.y0, fz(x, z))
    }

    fn get(&self, x: i32, y: i32, z: i32) -> i32 {
        let (x, y, z) = self.pos(x, y, z);
        if self.cb.inside(x, y, z) { self.w.get(x, y, z) } else { air() }
    }

    fn place(&mut self, b: u8, x: i32, y: i32, z: i32) {
        let (x, y, z) = self.pos(x, y, z);
        if self.cb.inside(x, y, z) {
            self.w.set(x, y, z, ids()[b as usize]);
        }
    }

    fn gbox(&mut self, x1: i32, y1: i32, z1: i32, x2: i32, y2: i32, z2: i32, out: u8, inn: u8) {
        for y in y1..=y2 {
            for x in x1..=x2 {
                for z in z1..=z2 {
                    let edge = y == y1 || y == y2 || x == x1 || x == x2 || z == z1 || z == z2;
                    self.place(if edge { out } else { inn }, x, y, z);
                }
            }
        }
    }

    /// `generateWaterBox`: water below sea level, air above, keeping ice and water.
    fn water(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    let b = self.get(x, y, z);
                    if !(is_water(b) || is(b, "ice") || is(b, "packed_ice")) {
                        self.place(if y + self.pb.y0 >= SEA_LEVEL { AIR } else { WATER }, x, y, z);
                    }
                }
            }
        }
    }

    /// `generateBoxOnFillOnly`.
    fn fill_only(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32, b: u8) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if is_water(self.get(x, y, z)) {
                        self.place(b, x, y, z);
                    }
                }
            }
        }
    }

    /// `generateDefaultFloor`.
    fn floor(&mut self, x: i32, z: i32, opening: bool) {
        if opening {
            self.gbox(x, 0, z, x + 2, 0, z + 7, GRAY, GRAY);
            self.gbox(x + 5, 0, z, x + 7, 0, z + 7, GRAY, GRAY);
            self.gbox(x + 3, 0, z, x + 4, 0, z + 2, GRAY, GRAY);
            self.gbox(x + 3, 0, z + 5, x + 4, 0, z + 7, GRAY, GRAY);
            self.gbox(x + 3, 0, z + 2, x + 4, 0, z + 2, LIGHT, LIGHT);
            self.gbox(x + 3, 0, z + 5, x + 4, 0, z + 5, LIGHT, LIGHT);
            self.gbox(x + 2, 0, z + 3, x + 2, 0, z + 4, LIGHT, LIGHT);
            self.gbox(x + 5, 0, z + 3, x + 5, 0, z + 4, LIGHT, LIGHT);
        } else {
            self.gbox(x, 0, z, x + 7, 0, z + 7, GRAY, GRAY);
        }
    }

    /// `chunkIntersects`.
    fn hits(&self, x0: i32, z0: i32, x1: i32, z1: i32) -> bool {
        let (ax, _, az) = self.pos(x0, 0, z0);
        let (bx, _, bz) = self.pos(x1, 0, z1);
        self.cb.intersects_xz(ax.min(bx), az.min(bz), ax.max(bx), az.max(bz))
    }

    /// `fillColumnDown` (stops above y 1, as in PNX).
    fn col_down(&mut self, b: u8, x: i32, y: i32, z: i32) {
        let (x, mut y, z) = self.pos(x, y, z);
        if self.cb.inside(x, y, z) {
            let fill = |v: i32| v == air() || crate::overworld::feature::blocks::is_liquid(v);
            while fill(self.w.get(x, y, z)) && y > 1 {
                self.w.set(x, y, z, ids()[b as usize]);
                y -= 1;
            }
        }
    }
}

// ---- piece bodies (mechanically ported from the Java) -------------------------------------

fn pp_monument_building(c: &mut Ctx, rooms: &[Room], p: &Piece, r: &mut NukkitRandom) {
    c.water(0, 0, 0, 58, 64 - c.pb.y0, 58);
    generate_wing(c, false, 0);
    generate_wing(c, true, 33);
    generate_entrance_archs(c);
    generate_entrance_wall(c);
    generate_roof_piece(c);
    generate_lower_wall(c);
    generate_middle_wall(c);
    generate_upper_wall(c);

    for xI in 0..7 {
        let mut zI = 0;
        while zI < 7 {
            if zI == 0 && xI == 3 {
                zI = 6;
            }

            let mut x = xI * 9;
            let mut z = zI * 9;
            for xOffset in 0..4 {
                for zOffset in 0..4 {
                    c.place(LIGHT, x + xOffset, 0, z + zOffset);
                    c.col_down(LIGHT, x + xOffset, -1, z + zOffset);
                }
            }

            if xI == 0 || xI == 6 {
                zI += 1;
            } else {
                zI += 6;
            }
        }
    }

    for i in 0..5 {
        c.water(-1 - i, i * 2, -1 - i, -1 - i, 23, 58 + i);
        c.water(58 + i, i * 2, -1 - i, 58 + i, 23, 58 + i);
        c.water(-i, i * 2, -1 - i, 57 + i, 23, -1 - i);
        c.water(-i, i * 2, 58 + i, 57 + i, 23, 58 + i);
    }
}

fn generate_wing(c: &mut Ctx, reverse: bool, x: i32) {
    if c.hits(x, 0, x + 23, 20) {
        c.gbox(x, 0, 0, x + 24, 0, 20, GRAY, GRAY);
        c.water(x, 1, 0, x + 24, 10, 20);

        for k in 0..4 {
            c.gbox(x + k, k + 1, k, x + k, k + 1, 20, LIGHT, LIGHT);
            c.gbox(x + k + 7, k + 5, k + 7, x + k + 7, k + 5, 20, LIGHT, LIGHT);
            c.gbox(x + 17 - k, k + 5, k + 7, x + 17 - k, k + 5, 20, LIGHT, LIGHT);
            c.gbox(x + 24 - k, k + 1, k, x + 24 - k, k + 1, 20, LIGHT, LIGHT);
            c.gbox(x + k + 1, k + 1, k, x + 23 - k, k + 1, k, LIGHT, LIGHT);
            c.gbox(x + k + 8, k + 5, k + 7, x + 16 - k, k + 5, k + 7, LIGHT, LIGHT);
        }

        c.gbox(x + 4, 4, 4, x + 6, 4, 20, GRAY, GRAY);
        c.gbox(x + 7, 4, 4, x + 17, 4, 6, GRAY, GRAY);
        c.gbox(x + 18, 4, 4, x + 20, 4, 20, GRAY, GRAY);
        c.gbox(x + 11, 8, 11, x + 13, 8, 20, GRAY, GRAY);

        c.place(LIGHT, x + 12, 9, 12);
        c.place(LIGHT, x + 12, 9, 15);
        c.place(LIGHT, x + 12, 9, 18);

        let mut xx = x + if reverse { 5 } else { 19 };

        for z in (5..=20).rev().step_by(3) {
            c.place(LIGHT, x + if reverse { 19 } else { 5 }, 5, z);
        }

        for z in (7..=19).rev().step_by(3) {
            c.place(LIGHT, xx, 5, z);
        }

        for i in 0..4 {
            c.place(LIGHT, if reverse { x + 24 - (17 - i * 3) } else { x + 17 - i * 3 }, 5, 5);
        }

        c.place(LIGHT, xx, 5, 5);
        c.gbox(x + 11, 1, 12, x + 13, 7, 12, GRAY, GRAY);
        c.gbox(x + 12, 1, 11, x + 12, 7, 13, GRAY, GRAY);
    }
}

fn generate_entrance_archs(c: &mut Ctx) {
    if c.hits(22, 5, 35, 17) {
        c.water(25, 0, 0, 32, 8, 20);

        for z in 0..4 {
            c.gbox(24, 2, 5 + z * 4, 24, 4, 5 + z * 4, LIGHT, LIGHT);
            c.gbox(22, 4, 5 + z * 4, 23, 4, 5 + z * 4, LIGHT, LIGHT);
            c.place(LIGHT, 25, 5, 5 + z * 4);
            c.place(LIGHT, 26, 6, 5 + z * 4);
            c.place(LAMP, 26, 5, 5 + z * 4);
            c.gbox(33, 2, 5 + z * 4, 33, 4, 5 + z * 4, LIGHT, LIGHT);
            c.gbox(34, 4, 5 + z * 4, 35, 4, 5 + z * 4, LIGHT, LIGHT);
            c.place(LIGHT, 32, 5, 5 + z * 4);
            c.place(LIGHT, 31, 6, 5 + z * 4);
            c.place(LAMP, 31, 5, 5 + z * 4);
            c.gbox(27, 6, 5 + z * 4, 30, 6, 5 + z * 4, GRAY, GRAY);
        }
    }
}

fn generate_entrance_wall(c: &mut Ctx) {
    if c.hits(15, 20, 42, 21) {
        c.gbox(15, 0, 21, 42, 0, 21, GRAY, GRAY);
        c.water(26, 1, 21, 31, 3, 21);
        c.gbox(21, 12, 21, 36, 12, 21, GRAY, GRAY);
        c.gbox(17, 11, 21, 40, 11, 21, GRAY, GRAY);
        c.gbox(16, 10, 21, 41, 10, 21, GRAY, GRAY);
        c.gbox(15, 7, 21, 42, 9, 21, GRAY, GRAY);
        c.gbox(16, 6, 21, 41, 6, 21, GRAY, GRAY);
        c.gbox(17, 5, 21, 40, 5, 21, GRAY, GRAY);
        c.gbox(21, 4, 21, 36, 4, 21, GRAY, GRAY);
        c.gbox(22, 3, 21, 26, 3, 21, GRAY, GRAY);
        c.gbox(31, 3, 21, 35, 3, 21, GRAY, GRAY);
        c.gbox(23, 2, 21, 25, 2, 21, GRAY, GRAY);
        c.gbox(32, 2, 21, 34, 2, 21, GRAY, GRAY);
        c.gbox(28, 4, 20, 29, 4, 21, LIGHT, LIGHT);

        c.place(LIGHT, 27, 3, 21);
        c.place(LIGHT, 30, 3, 21);
        c.place(LIGHT, 26, 2, 21);
        c.place(LIGHT, 31, 2, 21);
        c.place(LIGHT, 25, 1, 21);
        c.place(LIGHT, 32, 1, 21);

        for i in 0..7 {
            c.place(BLACK, 28 - i, 6 + i, 21);
            c.place(BLACK, 29 + i, 6 + i, 21);
        }

        for i in 0..4 {
            c.place(BLACK, 28 - i, 9 + i, 21);
            c.place(BLACK, 29 + i, 9 + i, 21);
        }

        c.place(BLACK, 28, 12, 21);
        c.place(BLACK, 29, 12, 21);

        for x in 0..3 {
            c.place(BLACK, 22 - x * 2, 8, 21);
            c.place(BLACK, 22 - x * 2, 9, 21);
            c.place(BLACK, 35 + x * 2, 8, 21);
            c.place(BLACK, 35 + x * 2, 9, 21);
        }

        c.water(15, 13, 21, 42, 15, 21);
        c.water(15, 1, 21, 15, 6, 21);
        c.water(16, 1, 21, 16, 5, 21);
        c.water(17, 1, 21, 20, 4, 21);
        c.water(21, 1, 21, 21, 3, 21);
        c.water(22, 1, 21, 22, 2, 21);
        c.water(23, 1, 21, 24, 1, 21);
        c.water(42, 1, 21, 42, 6, 21);
        c.water(41, 1, 21, 41, 5, 21);
        c.water(37, 1, 21, 40, 4, 21);
        c.water(36, 1, 21, 36, 3, 21);
        c.water(33, 1, 21, 34, 1, 21);
        c.water(35, 1, 21, 35, 2, 21);
    }
}

fn generate_roof_piece(c: &mut Ctx) {
    if c.hits(21, 21, 36, 36) {
        c.gbox(21, 0, 22, 36, 0, 36, GRAY, GRAY);
        c.water(21, 1, 22, 36, 23, 36);

        for i in 0..4 {
            c.gbox(21 + i, 13 + i, 21 + i, 36 - i, 13 + i, 21 + i, LIGHT, LIGHT);
            c.gbox(21 + i, 13 + i, 36 - i, 36 - i, 13 + i, 36 - i, LIGHT, LIGHT);
            c.gbox(21 + i, 13 + i, 22 + i, 21 + i, 13 + i, 35 - i, LIGHT, LIGHT);
            c.gbox(36 - i, 13 + i, 22 + i, 36 - i, 13 + i, 35 - i, LIGHT, LIGHT);
        }

        c.gbox(25, 16, 25, 32, 16, 32, GRAY, GRAY);
        c.gbox(25, 17, 25, 25, 19, 25, LIGHT, LIGHT);
        c.gbox(32, 17, 25, 32, 19, 25, LIGHT, LIGHT);
        c.gbox(25, 17, 32, 25, 19, 32, LIGHT, LIGHT);
        c.gbox(32, 17, 32, 32, 19, 32, LIGHT, LIGHT);

        c.place(LIGHT, 26, 20, 26);
        c.place(LIGHT, 27, 21, 27);
        c.place(LAMP, 27, 20, 27);
        c.place(LIGHT, 26, 20, 31);
        c.place(LIGHT, 27, 21, 30);
        c.place(LAMP, 27, 20, 30);
        c.place(LIGHT, 31, 20, 31);
        c.place(LIGHT, 30, 21, 30);
        c.place(LAMP, 30, 20, 30);
        c.place(LIGHT, 31, 20, 26);
        c.place(LIGHT, 30, 21, 27);
        c.place(LAMP, 30, 20, 27);

        c.gbox(28, 21, 27, 29, 21, 27, GRAY, GRAY);
        c.gbox(27, 21, 28, 27, 21, 29, GRAY, GRAY);
        c.gbox(28, 21, 30, 29, 21, 30, GRAY, GRAY);
        c.gbox(30, 21, 28, 30, 21, 29, GRAY, GRAY);
    }
}

fn generate_lower_wall(c: &mut Ctx) {
    if c.hits(0, 21, 6, 58) {
        c.gbox(0, 0, 21, 6, 0, 57, GRAY, GRAY);
        c.water(0, 1, 21, 6, 7, 57);
        c.gbox(4, 4, 21, 6, 4, 53, GRAY, GRAY);

        for i in 0..4 {
            c.gbox(i, i + 1, 21, i, i + 1, 57 - i, LIGHT, LIGHT);
        }

        for z in (23..53).step_by(3) {
            c.place(LIGHT, 5, 5, z);
        }

        c.place(LIGHT, 5, 5, 52);

        for i in 0..4 {
            c.gbox(i, i + 1, 21, i, i + 1, 57 - i, LIGHT, LIGHT);
        }

        c.gbox(4, 1, 52, 6, 3, 52, GRAY, GRAY);
        c.gbox(5, 1, 51, 5, 3, 53, GRAY, GRAY);
    }

    if c.hits(51, 21, 58, 58) {
        c.gbox(51, 0, 21, 57, 0, 57, GRAY, GRAY);
        c.water(51, 1, 21, 57, 7, 57);
        c.gbox(51, 4, 21, 53, 4, 53, GRAY, GRAY);

        for i in 0..4 {
            c.gbox(57 - i, i + 1, 21, 57 - i, i + 1, 57 - i, LIGHT, LIGHT);
        }

        for z in (23..53).step_by(3) {
            c.place(LIGHT, 52, 5, z);
        }

        c.place(LIGHT, 52, 5, 52);
        c.gbox(51, 1, 52, 53, 3, 52, GRAY, GRAY);
        c.gbox(52, 1, 51, 52, 3, 53, GRAY, GRAY);
    }

    if c.hits(0, 51, 57, 57) {
        c.gbox(7, 0, 51, 50, 0, 57, GRAY, GRAY);
        c.water(7, 1, 51, 50, 10, 57);

        for i in 0..4 {
            c.gbox(i + 1, i + 1, 57 - i, 56 - i, i + 1, 57 - i, LIGHT, LIGHT);
        }
    }
}

fn generate_middle_wall(c: &mut Ctx) {
    if c.hits(7, 21, 13, 50) {
        c.gbox(7, 0, 21, 13, 0, 50, GRAY, GRAY);
        c.water(7, 1, 21, 13, 10, 50);
        c.gbox(11, 8, 21, 13, 8, 53, GRAY, GRAY);

        for i in 0..4 {
            c.gbox(i + 7, i + 5, 21, i + 7, i + 5, 54, LIGHT, LIGHT);
        }

        for z in (21..=45).step_by(3) {
            c.place(LIGHT, 12, 9, z);
        }
    }

    if c.hits(44, 21, 50, 54) {
        c.gbox(44, 0, 21, 50, 0, 50, GRAY, GRAY);
        c.water(44, 1, 21, 50, 10, 50);
        c.gbox(44, 8, 21, 46, 8, 53, GRAY, GRAY);

        for i in 0..4 {
            c.gbox(50 - i, i + 5, 21, 50 - i, i + 5, 54, LIGHT, LIGHT);
        }

        for z in (21..=45).step_by(3) {
            c.place(LIGHT, 45, 9, z);
        }
    }

    if c.hits(8, 44, 49, 54) {
        c.gbox(14, 0, 44, 43, 0, 50, GRAY, GRAY);
        c.water(14, 1, 44, 43, 10, 50);

        for x in (12..=45).step_by(3) {
            c.place(LIGHT, x, 9, 45);
            c.place(LIGHT, x, 9, 52);
            if x == 12 || x == 18 || x == 24 || x == 33 || x == 39 || x == 45 {
                c.place(LIGHT, x, 9, 47);
                c.place(LIGHT, x, 9, 50);
                c.place(LIGHT, x, 10, 45);
                c.place(LIGHT, x, 10, 46);
                c.place(LIGHT, x, 10, 51);
                c.place(LIGHT, x, 10, 52);
                c.place(LIGHT, x, 11, 47);
                c.place(LIGHT, x, 11, 50);
                c.place(LIGHT, x, 12, 48);
                c.place(LIGHT, x, 12, 49);
            }
        }

        for i in 0..3 {
            c.gbox(8 + i, 5 + i, 54, 49 - i, 5 + i, 54, GRAY, GRAY);
        }

        c.gbox(11, 8, 54, 46, 8, 54, LIGHT, LIGHT);
        c.gbox(14, 8, 44, 43, 8, 53, GRAY, GRAY);
    }
}

fn generate_upper_wall(c: &mut Ctx) {
    if c.hits(14, 21, 20, 43) {
        c.gbox(14, 0, 21, 20, 0, 43, GRAY, GRAY);
        c.water(14, 1, 22, 20, 14, 43);
        c.gbox(18, 12, 22, 20, 12, 39, GRAY, GRAY);
        c.gbox(18, 12, 21, 20, 12, 21, LIGHT, LIGHT);

        for i in 0..4 {
            c.gbox(i + 14, i + 9, 21, i + 14, i + 9, 43 - i, LIGHT, LIGHT);
        }

        for z in (23..=39).step_by(3) {
            c.place(LIGHT, 19, 13, z);
        }
    }

    if c.hits(37, 21, 43, 43) {
        c.gbox(37, 0, 21, 43, 0, 43, GRAY, GRAY);
        c.water(37, 1, 22, 43, 14, 43);
        c.gbox(37, 12, 22, 39, 12, 39, GRAY, GRAY);
        c.gbox(37, 12, 21, 39, 12, 21, LIGHT, LIGHT);

        for i in 0..4 {
            c.gbox(43 - i, i + 9, 21, 43 - i, i + 9, 43 - i, LIGHT, LIGHT);
        }

        for z in (23..=39).step_by(3) {
            c.place(LIGHT, 38, 13, z);
        }
    }

    if c.hits(15, 37, 42, 43) {
        c.gbox(21, 0, 37, 36, 0, 43, GRAY, GRAY);
        c.water(21, 1, 37, 36, 14, 43);
        c.gbox(21, 12, 37, 36, 12, 39, GRAY, GRAY);

        for i in 0..4 {
            c.gbox(15 + i, i + 9, 43 - i, 42 - i, i + 9, 43 - i, LIGHT, LIGHT);
        }

        for x in (21..=36).step_by(3) {
            c.place(LIGHT, x, 13, 38);
        }
    }
}

fn pp_entry_room(c: &mut Ctx, rooms: &[Room], p: &Piece, r: &mut NukkitRandom) {
    let rd = &rooms[p.room];
    c.gbox(0, 3, 0, 2, 3, 7, LIGHT, LIGHT);
    c.gbox(5, 3, 0, 7, 3, 7, LIGHT, LIGHT);
    c.gbox(0, 2, 0, 1, 2, 7, LIGHT, LIGHT);
    c.gbox(6, 2, 0, 7, 2, 7, LIGHT, LIGHT);
    c.gbox(0, 1, 0, 0, 1, 7, LIGHT, LIGHT);
    c.gbox(7, 1, 0, 7, 1, 7, LIGHT, LIGHT);
    c.gbox(0, 1, 7, 7, 3, 7, LIGHT, LIGHT);
    c.gbox(1, 1, 0, 2, 3, 0, LIGHT, LIGHT);
    c.gbox(5, 1, 0, 6, 3, 0, LIGHT, LIGHT);

    if rd.open[NORTH] {
        c.water(3, 1, 7, 4, 2, 7);
    }
    if rd.open[WEST] {
        c.water(0, 1, 3, 1, 2, 4);
    }
    if rd.open[EAST] {
        c.water(6, 1, 3, 7, 2, 4);
    }
}

fn pp_simple_room(c: &mut Ctx, rooms: &[Room], p: &Piece, r: &mut NukkitRandom) {
    let rd = &rooms[p.room];
    if rd.index / 25 > 0 {
        c.floor(0, 0, rd.open[DOWN]);
    }

    if rd.conn[UP].is_none() {
        c.fill_only(1, 4, 1, 6, 4, 6, GRAY);
    }

    let mainDesign = p.design != 0 && r.next_boolean() && !rd.open[DOWN] && !rd.open[UP] && rd.count_openings() > 1;

    if p.design == 0 {
        c.gbox(0, 1, 0, 2, 1, 2, LIGHT, LIGHT);
        c.gbox(0, 3, 0, 2, 3, 2, LIGHT, LIGHT);
        c.gbox(0, 2, 0, 0, 2, 2, GRAY, GRAY);
        c.gbox(1, 2, 0, 2, 2, 0, GRAY, GRAY);
        c.place(LAMP, 1, 2, 1);
        c.gbox(5, 1, 0, 7, 1, 2, LIGHT, LIGHT);
        c.gbox(5, 3, 0, 7, 3, 2, LIGHT, LIGHT);
        c.gbox(7, 2, 0, 7, 2, 2, GRAY, GRAY);
        c.gbox(5, 2, 0, 6, 2, 0, GRAY, GRAY);
        c.place(LAMP, 6, 2, 1);
        c.gbox(0, 1, 5, 2, 1, 7, LIGHT, LIGHT);
        c.gbox(0, 3, 5, 2, 3, 7, LIGHT, LIGHT);
        c.gbox(0, 2, 5, 0, 2, 7, GRAY, GRAY);
        c.gbox(1, 2, 7, 2, 2, 7, GRAY, GRAY);
        c.place(LAMP, 1, 2, 6);
        c.gbox(5, 1, 5, 7, 1, 7, LIGHT, LIGHT);
        c.gbox(5, 3, 5, 7, 3, 7, LIGHT, LIGHT);
        c.gbox(7, 2, 5, 7, 2, 7, GRAY, GRAY);
        c.gbox(5, 2, 7, 6, 2, 7, GRAY, GRAY);
        c.place(LAMP, 6, 2, 6);

        if rd.open[SOUTH] {
            c.gbox(3, 3, 0, 4, 3, 0, LIGHT, LIGHT);
        } else {
            c.gbox(3, 3, 0, 4, 3, 1, LIGHT, LIGHT);
            c.gbox(3, 2, 0, 4, 2, 0, GRAY, GRAY);
            c.gbox(3, 1, 0, 4, 1, 1, LIGHT, LIGHT);
        }

        if rd.open[NORTH] {
            c.gbox(3, 3, 7, 4, 3, 7, LIGHT, LIGHT);
        } else {
            c.gbox(3, 3, 6, 4, 3, 7, LIGHT, LIGHT);
            c.gbox(3, 2, 7, 4, 2, 7, GRAY, GRAY);
            c.gbox(3, 1, 6, 4, 1, 7, LIGHT, LIGHT);
        }

        if rd.open[WEST] {
            c.gbox(0, 3, 3, 0, 3, 4, LIGHT, LIGHT);
        } else {
            c.gbox(0, 3, 3, 1, 3, 4, LIGHT, LIGHT);
            c.gbox(0, 2, 3, 0, 2, 4, GRAY, GRAY);
            c.gbox(0, 1, 3, 1, 1, 4, LIGHT, LIGHT);
        }

        if rd.open[EAST] {
            c.gbox(7, 3, 3, 7, 3, 4, LIGHT, LIGHT);
        } else {
            c.gbox(6, 3, 3, 7, 3, 4, LIGHT, LIGHT);
            c.gbox(7, 2, 3, 7, 2, 4, GRAY, GRAY);
            c.gbox(6, 1, 3, 7, 1, 4, LIGHT, LIGHT);
        }
    } else if p.design == 1 {
        c.gbox(2, 1, 2, 2, 3, 2, LIGHT, LIGHT);
        c.gbox(2, 1, 5, 2, 3, 5, LIGHT, LIGHT);
        c.gbox(5, 1, 5, 5, 3, 5, LIGHT, LIGHT);
        c.gbox(5, 1, 2, 5, 3, 2, LIGHT, LIGHT);
        c.place(LAMP, 2, 2, 2);
        c.place(LAMP, 2, 2, 5);
        c.place(LAMP, 5, 2, 5);
        c.place(LAMP, 5, 2, 2);
        c.gbox(0, 1, 0, 1, 3, 0, LIGHT, LIGHT);
        c.gbox(0, 1, 1, 0, 3, 1, LIGHT, LIGHT);
        c.gbox(0, 1, 7, 1, 3, 7, LIGHT, LIGHT);
        c.gbox(0, 1, 6, 0, 3, 6, LIGHT, LIGHT);
        c.gbox(6, 1, 7, 7, 3, 7, LIGHT, LIGHT);
        c.gbox(7, 1, 6, 7, 3, 6, LIGHT, LIGHT);
        c.gbox(6, 1, 0, 7, 3, 0, LIGHT, LIGHT);
        c.gbox(7, 1, 1, 7, 3, 1, LIGHT, LIGHT);
        c.place(GRAY, 1, 2, 0);
        c.place(GRAY, 0, 2, 1);
        c.place(GRAY, 1, 2, 7);
        c.place(GRAY, 0, 2, 6);
        c.place(GRAY, 6, 2, 7);
        c.place(GRAY, 7, 2, 6);
        c.place(GRAY, 6, 2, 0);
        c.place(GRAY, 7, 2, 1);

        if !rd.open[SOUTH] {
            c.gbox(1, 3, 0, 6, 3, 0, LIGHT, LIGHT);
            c.gbox(1, 2, 0, 6, 2, 0, GRAY, GRAY);
            c.gbox(1, 1, 0, 6, 1, 0, LIGHT, LIGHT);
        }
        if !rd.open[NORTH] {
            c.gbox(1, 3, 7, 6, 3, 7, LIGHT, LIGHT);
            c.gbox(1, 2, 7, 6, 2, 7, GRAY, GRAY);
            c.gbox(1, 1, 7, 6, 1, 7, LIGHT, LIGHT);
        }
        if !rd.open[WEST] {
            c.gbox(0, 3, 1, 0, 3, 6, LIGHT, LIGHT);
            c.gbox(0, 2, 1, 0, 2, 6, GRAY, GRAY);
            c.gbox(0, 1, 1, 0, 1, 6, LIGHT, LIGHT);
        }
        if !rd.open[EAST] {
            c.gbox(7, 3, 1, 7, 3, 6, LIGHT, LIGHT);
            c.gbox(7, 2, 1, 7, 2, 6, GRAY, GRAY);
            c.gbox(7, 1, 1, 7, 1, 6, LIGHT, LIGHT);
        }
    } else if p.design == 2 {
        c.gbox(0, 1, 0, 0, 1, 7, LIGHT, LIGHT);
        c.gbox(7, 1, 0, 7, 1, 7, LIGHT, LIGHT);
        c.gbox(1, 1, 0, 6, 1, 0, LIGHT, LIGHT);
        c.gbox(1, 1, 7, 6, 1, 7, LIGHT, LIGHT);
        c.gbox(0, 2, 0, 0, 2, 7, BLACK, BLACK);
        c.gbox(7, 2, 0, 7, 2, 7, BLACK, BLACK);
        c.gbox(1, 2, 0, 6, 2, 0, BLACK, BLACK);
        c.gbox(1, 2, 7, 6, 2, 7, BLACK, BLACK);
        c.gbox(0, 3, 0, 0, 3, 7, LIGHT, LIGHT);
        c.gbox(7, 3, 0, 7, 3, 7, LIGHT, LIGHT);
        c.gbox(1, 3, 0, 6, 3, 0, LIGHT, LIGHT);
        c.gbox(1, 3, 7, 6, 3, 7, LIGHT, LIGHT);
        c.gbox(0, 1, 3, 0, 2, 4, BLACK, BLACK);
        c.gbox(7, 1, 3, 7, 2, 4, BLACK, BLACK);
        c.gbox(3, 1, 0, 4, 2, 0, BLACK, BLACK);
        c.gbox(3, 1, 7, 4, 2, 7, BLACK, BLACK);

        if rd.open[SOUTH] {
            c.water(3, 1, 0, 4, 2, 0);
        }
        if rd.open[NORTH] {
            c.water(3, 1, 7, 4, 2, 7);
        }
        if rd.open[WEST] {
            c.water(0, 1, 3, 0, 2, 4);
        }
        if rd.open[EAST] {
            c.water(7, 1, 3, 7, 2, 4);
        }
    }

    if mainDesign {
        c.gbox(3, 1, 3, 4, 1, 4, LIGHT, LIGHT);
        c.gbox(3, 2, 3, 4, 2, 4, GRAY, GRAY);
        c.gbox(3, 3, 3, 4, 3, 4, LIGHT, LIGHT);
    }
}

fn pp_simple_top_room(c: &mut Ctx, rooms: &[Room], p: &Piece, r: &mut NukkitRandom) {
    let rd = &rooms[p.room];
    if rd.index / 25 > 0 {
        c.floor(0, 0, rd.open[DOWN]);
    }

    if rd.conn[UP].is_none() {
        c.fill_only(1, 4, 1, 6, 4, 6, GRAY);
    }

    for x in 1..=6 {
        for z in 1..=6 {
            if r.next_bounded_int(3) != 0 {
                c.gbox(x, 2 + (r.next_bounded_int(4) != 0) as i32, z, x, 3, z, SPONGE, SPONGE);
            }
        }
    }

    c.gbox(0, 1, 0, 0, 1, 7, LIGHT, LIGHT);
    c.gbox(7, 1, 0, 7, 1, 7, LIGHT, LIGHT);
    c.gbox(1, 1, 0, 6, 1, 0, LIGHT, LIGHT);
    c.gbox(1, 1, 7, 6, 1, 7, LIGHT, LIGHT);
    c.gbox(0, 2, 0, 0, 2, 7, BLACK, BLACK);
    c.gbox(7, 2, 0, 7, 2, 7, BLACK, BLACK);
    c.gbox(1, 2, 0, 6, 2, 0, BLACK, BLACK);
    c.gbox(1, 2, 7, 6, 2, 7, BLACK, BLACK);
    c.gbox(0, 3, 0, 0, 3, 7, LIGHT, LIGHT);
    c.gbox(7, 3, 0, 7, 3, 7, LIGHT, LIGHT);
    c.gbox(1, 3, 0, 6, 3, 0, LIGHT, LIGHT);
    c.gbox(1, 3, 7, 6, 3, 7, LIGHT, LIGHT);
    c.gbox(0, 1, 3, 0, 2, 4, BLACK, BLACK);
    c.gbox(7, 1, 3, 7, 2, 4, BLACK, BLACK);
    c.gbox(3, 1, 0, 4, 2, 0, BLACK, BLACK);
    c.gbox(3, 1, 7, 4, 2, 7, BLACK, BLACK);

    if rd.open[SOUTH] {
        c.water(3, 1, 0, 4, 2, 0);
    }
}

fn pp_double_y_room(c: &mut Ctx, rooms: &[Room], p: &Piece, r: &mut NukkitRandom) {
    let rd = &rooms[p.room];
    if rd.index / 25 > 0 {
        c.floor(0, 0, rd.open[DOWN]);
    }

    let roomU = &rooms[rd.conn[UP].unwrap()];
    if roomU.conn[UP].is_none() {
        c.fill_only(1, 8, 1, 6, 8, 6, GRAY);
    }

    c.gbox(0, 4, 0, 0, 4, 7, LIGHT, LIGHT);
    c.gbox(7, 4, 0, 7, 4, 7, LIGHT, LIGHT);
    c.gbox(1, 4, 0, 6, 4, 0, LIGHT, LIGHT);
    c.gbox(1, 4, 7, 6, 4, 7, LIGHT, LIGHT);
    c.gbox(2, 4, 1, 2, 4, 2, LIGHT, LIGHT);
    c.gbox(1, 4, 2, 1, 4, 2, LIGHT, LIGHT);
    c.gbox(5, 4, 1, 5, 4, 2, LIGHT, LIGHT);
    c.gbox(6, 4, 2, 6, 4, 2, LIGHT, LIGHT);
    c.gbox(2, 4, 5, 2, 4, 6, LIGHT, LIGHT);
    c.gbox(1, 4, 5, 1, 4, 5, LIGHT, LIGHT);
    c.gbox(5, 4, 5, 5, 4, 6, LIGHT, LIGHT);
    c.gbox(6, 4, 5, 6, 4, 5, LIGHT, LIGHT);

    let mut roomDefinition = rd;
    for y in (1..=5).step_by(4) {
        let mut z = 0;
        if roomDefinition.open[SOUTH] {
            c.gbox(2, y, z, 2, y + 2, z, LIGHT, LIGHT);
            c.gbox(5, y, z, 5, y + 2, z, LIGHT, LIGHT);
            c.gbox(3, y + 2, z, 4, y + 2, z, LIGHT, LIGHT);
        } else {
            c.gbox(0, y, z, 7, y + 2, z, LIGHT, LIGHT);
            c.gbox(0, y + 1, z, 7, y + 1, z, GRAY, GRAY);
        }

        z = 7;
        if roomDefinition.open[NORTH] {
            c.gbox(2, y, z, 2, y + 2, z, LIGHT, LIGHT);
            c.gbox(5, y, z, 5, y + 2, z, LIGHT, LIGHT);
            c.gbox(3, y + 2, z, 4, y + 2, z, LIGHT, LIGHT);
        } else {
            c.gbox(0, y, z, 7, y + 2, z, LIGHT, LIGHT);
            c.gbox(0, y + 1, z, 7, y + 1, z, GRAY, GRAY);
        }

        let mut x = 0;
        if roomDefinition.open[WEST] {
            c.gbox(x, y, 2, x, y + 2, 2, LIGHT, LIGHT);
            c.gbox(x, y, 5, x, y + 2, 5, LIGHT, LIGHT);
            c.gbox(x, y + 2, 3, x, y + 2, 4, LIGHT, LIGHT);
        } else {
            c.gbox(x, y, 0, x, y + 2, 7, LIGHT, LIGHT);
            c.gbox(x, y + 1, 0, x, y + 1, 7, GRAY, GRAY);
        }

        x = 7;
        if roomDefinition.open[EAST] {
            c.gbox(x, y, 2, x, y + 2, 2, LIGHT, LIGHT);
            c.gbox(x, y, 5, x, y + 2, 5, LIGHT, LIGHT);
            c.gbox(x, y + 2, 3, x, y + 2, 4, LIGHT, LIGHT);
        } else {
            c.gbox(x, y, 0, x, y + 2, 7, LIGHT, LIGHT);
            c.gbox(x, y + 1, 0, x, y + 1, 7, GRAY, GRAY);
        }

        roomDefinition = roomU;
    }
}

fn pp_double_x_room(c: &mut Ctx, rooms: &[Room], p: &Piece, r: &mut NukkitRandom) {
    let rd = &rooms[p.room];
    let roomE = &rooms[rd.conn[EAST].unwrap()];
    let mut roomDefinition = rd;

    if rd.index / 25 > 0 {
        c.floor(8, 0, roomE.open[DOWN]);
        c.floor(0, 0, roomDefinition.open[DOWN]);
    }

    if roomDefinition.conn[UP].is_none() {
        c.fill_only(1, 4, 1, 7, 4, 6, GRAY);
    }

    if roomE.conn[UP].is_none() {
        c.fill_only(8, 4, 1, 14, 4, 6, GRAY);
    }

    c.gbox(0, 3, 0, 0, 3, 7, LIGHT, LIGHT);
    c.gbox(15, 3, 0, 15, 3, 7, LIGHT, LIGHT);
    c.gbox(1, 3, 0, 15, 3, 0, LIGHT, LIGHT);
    c.gbox(1, 3, 7, 14, 3, 7, LIGHT, LIGHT);
    c.gbox(0, 2, 0, 0, 2, 7, GRAY, GRAY);
    c.gbox(15, 2, 0, 15, 2, 7, GRAY, GRAY);
    c.gbox(1, 2, 0, 15, 2, 0, GRAY, GRAY);
    c.gbox(1, 2, 7, 14, 2, 7, GRAY, GRAY);
    c.gbox(0, 1, 0, 0, 1, 7, LIGHT, LIGHT);
    c.gbox(15, 1, 0, 15, 1, 7, LIGHT, LIGHT);
    c.gbox(1, 1, 0, 15, 1, 0, LIGHT, LIGHT);
    c.gbox(1, 1, 7, 14, 1, 7, LIGHT, LIGHT);
    c.gbox(5, 1, 0, 10, 1, 4, LIGHT, LIGHT);
    c.gbox(6, 2, 0, 9, 2, 3, GRAY, GRAY);
    c.gbox(5, 3, 0, 10, 3, 4, LIGHT, LIGHT);

    c.place(LAMP, 6, 2, 3);
    c.place(LAMP, 9, 2, 3);

    if roomDefinition.open[SOUTH] {
        c.water(3, 1, 0, 4, 2, 0);
    }
    if roomDefinition.open[NORTH] {
        c.water(3, 1, 7, 4, 2, 7);
    }
    if roomDefinition.open[WEST] {
        c.water(0, 1, 3, 0, 2, 4);
    }

    if roomE.open[SOUTH] {
        c.water(11, 1, 0, 12, 2, 0);
    }
    if roomE.open[NORTH] {
        c.water(11, 1, 7, 12, 2, 7);
    }
    if roomE.open[EAST] {
        c.water(15, 1, 3, 15, 2, 4);
    }
}

fn pp_double_z_room(c: &mut Ctx, rooms: &[Room], p: &Piece, r: &mut NukkitRandom) {
    let rd = &rooms[p.room];
    let roomN = &rooms[rd.conn[NORTH].unwrap()];
    let mut roomDefinition = rd;

    if rd.index / 25 > 0 {
        c.floor(0, 8, roomN.open[DOWN]);
        c.floor(0, 0, roomDefinition.open[DOWN]);
    }

    if roomDefinition.conn[UP].is_none() {
        c.fill_only(1, 4, 1, 6, 4, 7, GRAY);
    }

    if roomN.conn[UP].is_none() {
        c.fill_only(1, 4, 8, 6, 4, 14, GRAY);
    }

    c.gbox(0, 3, 0, 0, 3, 15, LIGHT, LIGHT);
    c.gbox(7, 3, 0, 7, 3, 15, LIGHT, LIGHT);
    c.gbox(1, 3, 0, 7, 3, 0, LIGHT, LIGHT);
    c.gbox(1, 3, 15, 6, 3, 15, LIGHT, LIGHT);
    c.gbox(0, 2, 0, 0, 2, 15, GRAY, GRAY);
    c.gbox(7, 2, 0, 7, 2, 15, GRAY, GRAY);
    c.gbox(1, 2, 0, 7, 2, 0, GRAY, GRAY);
    c.gbox(1, 2, 15, 6, 2, 15, GRAY, GRAY);
    c.gbox(0, 1, 0, 0, 1, 15, LIGHT, LIGHT);
    c.gbox(7, 1, 0, 7, 1, 15, LIGHT, LIGHT);
    c.gbox(1, 1, 0, 7, 1, 0, LIGHT, LIGHT);
    c.gbox(1, 1, 15, 6, 1, 15, LIGHT, LIGHT);
    c.gbox(1, 1, 1, 1, 1, 2, LIGHT, LIGHT);
    c.gbox(6, 1, 1, 6, 1, 2, LIGHT, LIGHT);
    c.gbox(1, 3, 1, 1, 3, 2, LIGHT, LIGHT);
    c.gbox(6, 3, 1, 6, 3, 2, LIGHT, LIGHT);
    c.gbox(1, 1, 13, 1, 1, 14, LIGHT, LIGHT);
    c.gbox(6, 1, 13, 6, 1, 14, LIGHT, LIGHT);
    c.gbox(1, 3, 13, 1, 3, 14, LIGHT, LIGHT);
    c.gbox(6, 3, 13, 6, 3, 14, LIGHT, LIGHT);
    c.gbox(2, 1, 6, 2, 3, 6, LIGHT, LIGHT);
    c.gbox(5, 1, 6, 5, 3, 6, LIGHT, LIGHT);
    c.gbox(2, 1, 9, 2, 3, 9, LIGHT, LIGHT);
    c.gbox(5, 1, 9, 5, 3, 9, LIGHT, LIGHT);
    c.gbox(3, 2, 6, 4, 2, 6, LIGHT, LIGHT);
    c.gbox(3, 2, 9, 4, 2, 9, LIGHT, LIGHT);
    c.gbox(2, 2, 7, 2, 2, 8, LIGHT, LIGHT);
    c.gbox(5, 2, 7, 5, 2, 8, LIGHT, LIGHT);
    c.place(LAMP, 2, 2, 5);
    c.place(LAMP, 5, 2, 5);
    c.place(LAMP, 2, 2, 10);
    c.place(LAMP, 5, 2, 10);
    c.place(LIGHT, 2, 3, 5);
    c.place(LIGHT, 5, 3, 5);
    c.place(LIGHT, 2, 3, 10);
    c.place(LIGHT, 5, 3, 10);

    if roomDefinition.open[SOUTH] {
        c.water(3, 1, 0, 4, 2, 0);
    }
    if roomDefinition.open[EAST] {
        c.water(7, 1, 3, 7, 2, 4);
    }
    if roomDefinition.open[WEST] {
        c.water(0, 1, 3, 0, 2, 4);
    }

    if roomN.open[NORTH] {
        c.water(3, 1, 15, 4, 2, 15);
    }
    if roomN.open[WEST] {
        c.water(0, 1, 11, 0, 2, 12);
    }
    if roomN.open[EAST] {
        c.water(7, 1, 11, 7, 2, 12);
    }
}

fn pp_double_x_y_room(c: &mut Ctx, rooms: &[Room], p: &Piece, r: &mut NukkitRandom) {
    let rd = &rooms[p.room];
    let roomE = &rooms[rd.conn[EAST].unwrap()];
    let mut roomDefinition = rd;
    let roomU = &rooms[roomDefinition.conn[UP].unwrap()];
    let roomEU = &rooms[roomE.conn[UP].unwrap()];

    if rd.index / 25 > 0 {
        c.floor(8, 0, roomE.open[DOWN]);
        c.floor(0, 0, roomDefinition.open[DOWN]);
    }

    if roomU.conn[UP].is_none() {
        c.fill_only(1, 8, 1, 7, 8, 6, GRAY);
    }

    if roomEU.conn[UP].is_none() {
        c.fill_only(8, 8, 1, 14, 8, 6, GRAY);
    }

    for y in 1..=7 {
        let mut block = LIGHT;
        if y == 2 || y == 6 {
            block = GRAY;
        }

        c.gbox(0, y, 0, 0, y, 7, block, block);
        c.gbox(15, y, 0, 15, y, 7, block, block);
        c.gbox(1, y, 0, 15, y, 0, block, block);
        c.gbox(1, y, 7, 14, y, 7, block, block);
    }

    c.gbox(2, 1, 3, 2, 7, 4, LIGHT, LIGHT);
    c.gbox(3, 1, 2, 4, 7, 2, LIGHT, LIGHT);
    c.gbox(3, 1, 5, 4, 7, 5, LIGHT, LIGHT);
    c.gbox(13, 1, 3, 13, 7, 4, LIGHT, LIGHT);
    c.gbox(11, 1, 2, 12, 7, 2, LIGHT, LIGHT);
    c.gbox(11, 1, 5, 12, 7, 5, LIGHT, LIGHT);
    c.gbox(5, 1, 3, 5, 3, 4, LIGHT, LIGHT);
    c.gbox(10, 1, 3, 10, 3, 4, LIGHT, LIGHT);
    c.gbox(5, 7, 2, 10, 7, 5, LIGHT, LIGHT);
    c.gbox(5, 5, 2, 5, 7, 2, LIGHT, LIGHT);
    c.gbox(10, 5, 2, 10, 7, 2, LIGHT, LIGHT);
    c.gbox(5, 5, 5, 5, 7, 5, LIGHT, LIGHT);
    c.gbox(10, 5, 5, 10, 7, 5, LIGHT, LIGHT);
    c.place(LIGHT, 6, 6, 2);
    c.place(LIGHT, 9, 6, 2);
    c.place(LIGHT, 6, 6, 5);
    c.place(LIGHT, 9, 6, 5);
    c.gbox(5, 4, 3, 6, 4, 4, LIGHT, LIGHT);
    c.gbox(9, 4, 3, 10, 4, 4, LIGHT, LIGHT);
    c.place(LAMP, 5, 4, 2);
    c.place(LAMP, 5, 4, 5);
    c.place(LAMP, 10, 4, 2);
    c.place(LAMP, 10, 4, 5);

    if roomDefinition.open[SOUTH] {
        c.water(3, 1, 0, 4, 2, 0);
    }
    if roomDefinition.open[NORTH] {
        c.water(3, 1, 7, 4, 2, 7);
    }
    if roomDefinition.open[WEST] {
        c.water(0, 1, 3, 0, 2, 4);
    }

    if roomE.open[SOUTH] {
        c.water(11, 1, 0, 12, 2, 0);
    }
    if roomE.open[NORTH] {
        c.water(11, 1, 7, 12, 2, 7);
    }
    if roomE.open[EAST] {
        c.water(15, 1, 3, 15, 2, 4);
    }

    if roomU.open[SOUTH] {
        c.water(3, 5, 0, 4, 6, 0);
    }
    if roomU.open[NORTH] {
        c.water(3, 5, 7, 4, 6, 7);
    }
    if roomU.open[WEST] {
        c.water(0, 5, 3, 0, 6, 4);
    }

    if roomEU.open[SOUTH] {
        c.water(11, 5, 0, 12, 6, 0);
    }
    if roomEU.open[NORTH] {
        c.water(11, 5, 7, 12, 6, 7);
    }
    if roomEU.open[EAST] {
        c.water(15, 5, 3, 15, 6, 4);
    }
}

fn pp_double_y_z_room(c: &mut Ctx, rooms: &[Room], p: &Piece, r: &mut NukkitRandom) {
    let rd = &rooms[p.room];
    let roomN = &rooms[rd.conn[NORTH].unwrap()];
    let mut roomDefinition = rd;
    let roomNU = &rooms[roomN.conn[UP].unwrap()];
    let roomU = &rooms[roomDefinition.conn[UP].unwrap()];

    if rd.index / 25 > 0 {
        c.floor(0, 8, roomN.open[DOWN]);
        c.floor(0, 0, roomDefinition.open[DOWN]);
    }

    if roomU.conn[UP].is_none() {
        c.fill_only(1, 8, 1, 6, 8, 7, GRAY);
    }

    if roomNU.conn[UP].is_none() {
        c.fill_only(1, 8, 8, 6, 8, 14, GRAY);
    }

    for y in 1..=7 {
        let mut block = LIGHT;
        if y == 2 || y == 6 {
            block = GRAY;
        }

        c.gbox(0, y, 0, 0, y, 15, block, block);
        c.gbox(7, y, 0, 7, y, 15, block, block);
        c.gbox(1, y, 0, 6, y, 0, block, block);
        c.gbox(1, y, 15, 6, y, 15, block, block);
    }

    for y in 1..=7 {
        let mut block = BLACK;
        if y == 2 || y == 6 {
            block = LAMP;
        }

        c.gbox(3, y, 7, 4, y, 8, block, block);
    }

    if roomDefinition.open[SOUTH] {
        c.water(3, 1, 0, 4, 2, 0);
    }
    if roomDefinition.open[EAST] {
        c.water(7, 1, 3, 7, 2, 4);
    }
    if roomDefinition.open[WEST] {
        c.water(0, 1, 3, 0, 2, 4);
    }

    if roomN.open[NORTH] {
        c.water(3, 1, 15, 4, 2, 15);
    }
    if roomN.open[WEST] {
        c.water(0, 1, 11, 0, 2, 12);
    }
    if roomN.open[EAST] {
        c.water(7, 1, 11, 7, 2, 12);
    }

    if roomU.open[SOUTH] {
        c.water(3, 5, 0, 4, 6, 0);
    }
    if roomU.open[EAST] {
        c.water(7, 5, 3, 7, 6, 4);
        c.gbox(5, 4, 2, 6, 4, 5, LIGHT, LIGHT);
        c.gbox(6, 1, 2, 6, 3, 2, LIGHT, LIGHT);
        c.gbox(6, 1, 5, 6, 3, 5, LIGHT, LIGHT);
    }
    if roomU.open[WEST] {
        c.water(0, 5, 3, 0, 6, 4);
        c.gbox(1, 4, 2, 2, 4, 5, LIGHT, LIGHT);
        c.gbox(1, 1, 2, 1, 3, 2, LIGHT, LIGHT);
        c.gbox(1, 1, 5, 1, 3, 5, LIGHT, LIGHT);
    }

    if roomNU.open[NORTH] {
        c.water(3, 5, 15, 4, 6, 15);
    }
    if roomNU.open[WEST] {
        c.water(0, 5, 11, 0, 6, 12);
        c.gbox(1, 4, 10, 2, 4, 13, LIGHT, LIGHT);
        c.gbox(1, 1, 10, 1, 3, 10, LIGHT, LIGHT);
        c.gbox(1, 1, 13, 1, 3, 13, LIGHT, LIGHT);
    }
    if roomNU.open[EAST] {
        c.water(7, 5, 11, 7, 6, 12);
        c.gbox(5, 4, 10, 6, 4, 13, LIGHT, LIGHT);
        c.gbox(6, 1, 10, 6, 3, 10, LIGHT, LIGHT);
        c.gbox(6, 1, 13, 6, 3, 13, LIGHT, LIGHT);
    }
}

fn pp_core_room(c: &mut Ctx, rooms: &[Room], p: &Piece, r: &mut NukkitRandom) {
    c.fill_only(1, 8, 0, 14, 8, 14, GRAY);
    c.gbox(0, 7, 0, 0, 7, 15, LIGHT, LIGHT);
    c.gbox(15, 7, 0, 15, 7, 15, LIGHT, LIGHT);
    c.gbox(1, 7, 0, 15, 7, 0, LIGHT, LIGHT);
    c.gbox(1, 7, 15, 14, 7, 15, LIGHT, LIGHT);

    for y in 1..=6 {
        let mut block = LIGHT;
        if y == 2 || y == 6 {
            block = GRAY;
        }

        for x in (0..=15).step_by(15) {
            c.gbox(x, y, 0, x, y, 1, block, block);
            c.gbox(x, y, 6, x, y, 9, block, block);
            c.gbox(x, y, 14, x, y, 15, block, block);
        }

        c.gbox(1, y, 0, 1, y, 0, block, block);
        c.gbox(6, y, 0, 9, y, 0, block, block);
        c.gbox(14, y, 0, 14, y, 0, block, block);
        c.gbox(1, y, 15, 14, y, 15, block, block);
    }

    c.gbox(6, 3, 6, 9, 6, 9, BLACK, BLACK);
    c.gbox(7, 4, 7, 8, 5, 8, GOLD, GOLD);

    for y in (3..=6).step_by(3) {
        for x in (6..=9).step_by(3) {
            c.place(LAMP, x, y, 6);
            c.place(LAMP, x, y, 9);
        }
    }

    c.gbox(5, 1, 6, 5, 2, 6, LIGHT, LIGHT);
    c.gbox(5, 1, 9, 5, 2, 9, LIGHT, LIGHT);
    c.gbox(10, 1, 6, 10, 2, 6, LIGHT, LIGHT);
    c.gbox(10, 1, 9, 10, 2, 9, LIGHT, LIGHT);
    c.gbox(6, 1, 5, 6, 2, 5, LIGHT, LIGHT);
    c.gbox(9, 1, 5, 9, 2, 5, LIGHT, LIGHT);
    c.gbox(6, 1, 10, 6, 2, 10, LIGHT, LIGHT);
    c.gbox(9, 1, 10, 9, 2, 10, LIGHT, LIGHT);
    c.gbox(5, 2, 5, 5, 6, 5, LIGHT, LIGHT);
    c.gbox(5, 2, 10, 5, 6, 10, LIGHT, LIGHT);
    c.gbox(10, 2, 5, 10, 6, 5, LIGHT, LIGHT);
    c.gbox(10, 2, 10, 10, 6, 10, LIGHT, LIGHT);
    c.gbox(5, 7, 1, 5, 7, 6, LIGHT, LIGHT);
    c.gbox(10, 7, 1, 10, 7, 6, LIGHT, LIGHT);
    c.gbox(5, 7, 9, 5, 7, 14, LIGHT, LIGHT);
    c.gbox(10, 7, 9, 10, 7, 14, LIGHT, LIGHT);
    c.gbox(1, 7, 5, 6, 7, 5, LIGHT, LIGHT);
    c.gbox(1, 7, 10, 6, 7, 10, LIGHT, LIGHT);
    c.gbox(9, 7, 5, 14, 7, 5, LIGHT, LIGHT);
    c.gbox(9, 7, 10, 14, 7, 10, LIGHT, LIGHT);
    c.gbox(2, 1, 2, 2, 1, 3, LIGHT, LIGHT);
    c.gbox(3, 1, 2, 3, 1, 2, LIGHT, LIGHT);
    c.gbox(13, 1, 2, 13, 1, 3, LIGHT, LIGHT);
    c.gbox(12, 1, 2, 12, 1, 2, LIGHT, LIGHT);
    c.gbox(2, 1, 12, 2, 1, 13, LIGHT, LIGHT);
    c.gbox(3, 1, 13, 3, 1, 13, LIGHT, LIGHT);
    c.gbox(13, 1, 12, 13, 1, 13, LIGHT, LIGHT);
    c.gbox(12, 1, 13, 12, 1, 13, LIGHT, LIGHT);
}

fn pp_wing_room(c: &mut Ctx, rooms: &[Room], p: &Piece, r: &mut NukkitRandom) {
    if p.design == 0 {
        for i in 0..4 {
            c.gbox(10 - i, 3 - i, 20 - i, 12 + i, 3 - i, 20, LIGHT, LIGHT);
        }

        c.gbox(7, 0, 6, 15, 0, 16, LIGHT, LIGHT);
        c.gbox(6, 0, 6, 6, 3, 20, LIGHT, LIGHT);
        c.gbox(16, 0, 6, 16, 3, 20, LIGHT, LIGHT);
        c.gbox(7, 1, 7, 7, 1, 20, LIGHT, LIGHT);
        c.gbox(15, 1, 7, 15, 1, 20, LIGHT, LIGHT);
        c.gbox(7, 1, 6, 9, 3, 6, LIGHT, LIGHT);
        c.gbox(13, 1, 6, 15, 3, 6, LIGHT, LIGHT);
        c.gbox(8, 1, 7, 9, 1, 7, LIGHT, LIGHT);
        c.gbox(13, 1, 7, 14, 1, 7, LIGHT, LIGHT);
        c.gbox(9, 0, 5, 13, 0, 5, LIGHT, LIGHT);
        c.gbox(10, 0, 7, 12, 0, 7, BLACK, BLACK);
        c.gbox(8, 0, 10, 8, 0, 12, BLACK, BLACK);
        c.gbox(14, 0, 10, 14, 0, 12, BLACK, BLACK);

        for z in (7..=18).rev().step_by(3) {
            c.place(LAMP, 6, 3, z);
            c.place(LAMP, 16, 3, z);
        }

        c.place(LAMP, 10, 0, 10);
        c.place(LAMP, 12, 0, 10);
        c.place(LAMP, 10, 0, 12);
        c.place(LAMP, 12, 0, 12);
        c.place(LAMP, 8, 3, 6);
        c.place(LAMP, 14, 3, 6);
        c.place(LIGHT, 4, 2, 4);
        c.place(LAMP, 4, 1, 4);
        c.place(LIGHT, 4, 0, 4);
        c.place(LIGHT, 18, 2, 4);
        c.place(LAMP, 18, 1, 4);
        c.place(LIGHT, 18, 0, 4);
        c.place(LIGHT, 4, 2, 18);
        c.place(LAMP, 4, 1, 18);
        c.place(LIGHT, 4, 0, 18);
        c.place(LIGHT, 18, 2, 18);
        c.place(LAMP, 18, 1, 18);
        c.place(LIGHT, 18, 0, 18);
        c.place(LIGHT, 9, 7, 20);
        c.place(LIGHT, 13, 7, 20);
        c.gbox(6, 0, 21, 7, 4, 21, LIGHT, LIGHT);
        c.gbox(15, 0, 21, 16, 4, 21, LIGHT, LIGHT);
    } else if p.design == 1 {
        c.gbox(9, 3, 18, 13, 3, 20, LIGHT, LIGHT);
        c.gbox(9, 0, 18, 9, 2, 18, LIGHT, LIGHT);
        c.gbox(13, 0, 18, 13, 2, 18, LIGHT, LIGHT);

        let mut x = 9;
        for n in 0..2 {
            c.place(LIGHT, x, 6, 20);
            c.place(LAMP, x, 5, 20);
            c.place(LIGHT, x, 4, 20);
            x = 13;
        }

        c.gbox(7, 3, 7, 15, 3, 14, LIGHT, LIGHT);

        x = 10;
        for o in 0..2 {
            c.gbox(x, 0, 10, x, 6, 10, LIGHT, LIGHT);
            c.gbox(x, 0, 12, x, 6, 12, LIGHT, LIGHT);
            c.place(LAMP, x, 0, 10);
            c.place(LAMP, x, 0, 12);
            c.place(LAMP, x, 4, 10);
            c.place(LAMP, x, 4, 12);
            x = 12;
        }

        x = 8;
        for _p in 0..2 {
            c.gbox(x, 0, 7, x, 2, 7, LIGHT, LIGHT);
            c.gbox(x, 0, 14, x, 2, 14, LIGHT, LIGHT);
            x = 14;
        }

        c.gbox(8, 3, 8, 8, 3, 13, BLACK, BLACK);
        c.gbox(14, 3, 8, 14, 3, 13, BLACK, BLACK);
    }
}

fn pp_penthouse(c: &mut Ctx, rooms: &[Room], p: &Piece, r: &mut NukkitRandom) {
    c.gbox(2, -1, 2, 11, -1, 11, LIGHT, LIGHT);
    c.gbox(0, -1, 0, 1, -1, 11, GRAY, GRAY);
    c.gbox(12, -1, 0, 13, -1, 11, GRAY, GRAY);
    c.gbox(2, -1, 0, 11, -1, 1, GRAY, GRAY);
    c.gbox(2, -1, 12, 11, -1, 13, GRAY, GRAY);
    c.gbox(0, 0, 0, 0, 0, 13, LIGHT, LIGHT);
    c.gbox(13, 0, 0, 13, 0, 13, LIGHT, LIGHT);
    c.gbox(1, 0, 0, 12, 0, 0, LIGHT, LIGHT);
    c.gbox(1, 0, 13, 12, 0, 13, LIGHT, LIGHT);

    for i in (2..=11).step_by(3) {
        c.place(LAMP, 0, 0, i);
        c.place(LAMP, 13, 0, i);
        c.place(LAMP, i, 0, 0);
    }

    c.gbox(2, 0, 3, 4, 0, 9, LIGHT, LIGHT);
    c.gbox(9, 0, 3, 11, 0, 9, LIGHT, LIGHT);
    c.gbox(4, 0, 9, 9, 0, 11, LIGHT, LIGHT);
    c.place(LIGHT, 5, 0, 8);
    c.place(LIGHT, 8, 0, 8);
    c.place(LIGHT, 10, 0, 10);
    c.place(LIGHT, 3, 0, 10);
    c.gbox(3, 0, 3, 3, 0, 7, BLACK, BLACK);
    c.gbox(10, 0, 3, 10, 0, 7, BLACK, BLACK);
    c.gbox(6, 0, 10, 7, 0, 10, BLACK, BLACK);

    let mut x = 3;
    for i in 0..2 {
        for z in (2..=8).step_by(3) {
            c.gbox(x, 0, z, x, 2, z, LIGHT, LIGHT);
        }
        x = 10;
    }

    c.gbox(5, 0, 10, 5, 2, 10, LIGHT, LIGHT);
    c.gbox(8, 0, 10, 8, 2, 10, LIGHT, LIGHT);
    c.gbox(6, -1, 7, 7, -1, 8, BLACK, BLACK);
    c.water(6, -1, 3, 7, -1, 4);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overworld::Overworld;

    #[test]
    fn room_graph_is_deterministic_and_connected() {
        let mut r = start_random(42, 3, 7);
        let (pieces, rooms) = create(&mut r, 0, 0, Face::North);
        assert!(pieces.len() > 10, "{} pieces", pieces.len());
        assert!(pieces.iter().skip(1).all(|p| pieces[0].bb.intersects(&p.bb)));
        assert_eq!(rooms.len(), 49);
    }

    #[test]
    fn monument_generates() {
        let blocks = ["prismarine", "prismarine_bricks", "sea_lantern"].map(id);
        for seed in [1u64, 2, 3, 4, 5, 6] {
            let ow = Overworld::new(seed);
            for cx in -48..48 {
                for cz in -48..48 {
                    if !PLACEMENT.is_candidate(seed as i64, cx, cz) || !is_deep_ocean(ow.generate_terrain(cx, cz).biome(7, SEA_LEVEL, 7)) {
                        continue;
                    }
                    let ch = ow.generate(cx, cz);
                    let n = (0..16).flat_map(|x| (0..16).flat_map(move |z| (30..70).map(move |y| (x, y, z)))).filter(|&(x, y, z)| blocks.contains(&ch.get(x, y, z))).count();
                    if n > 50 {
                        return;
                    }
                }
            }
        }
        panic!("no monument found");
    }
}

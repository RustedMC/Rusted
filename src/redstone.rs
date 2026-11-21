//! Redstone, ported from PNX (RedstoneComponent, BlockRedstoneWire, BlockRedstoneDiode,
//! BlockRedstoneTorch, BlockObserver, BlockPistonBase, Level.getRedstonePower).
//! Block changes mark spots dirty and the next tick rechecks everything within 2 blocks.
//! Dust networks are solved in one go, diodes switch after their vanilla delay.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use parking_lot::Mutex;

use crate::blocks::{State, any_state, entry, state, with};
use crate::world::{AIR, World};

type Pos = (i32, i32, i32);

/// Bedrock face order: down, up, north, south, west, east.
pub const DIRS: [Pos; 6] = [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)];

fn add(a: Pos, b: Pos) -> Pos {
    (a.0 + b.0, a.1 + b.1, a.2 + b.2)
}

fn neg(a: Pos) -> Pos {
    (-a.0, -a.1, -a.2)
}

/// Changed since last tick.
pub static DIRTY: Mutex<Vec<Pos>> = Mutex::new(Vec::new());

pub fn mark(p: Pos) {
    DIRTY.lock().push(p);
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Job {
    /// recheck a torch/comparator
    Diode,
    /// a repeater turns on (true) or off, decided when its input changed
    Repeater(bool),
    /// dispenser/dropper/crafter fires (vanilla waits 4 game ticks)
    Dispense,
    /// a piston's move advances half a block (it takes 2 game ticks)
    Piston,
    /// observer pulse on, then off
    ObserverOn,
    ObserverOff,
}

#[derive(Default)]
struct Engine {
    tick: u64,
    due: BTreeMap<u64, Vec<(Pos, Job)>>,
    pending: HashSet<Pos>,
    /// Last powered state of edge-triggered stuff (doors, TNT, note blocks).
    last: HashMap<Pos, bool>,
    comparators: HashMap<Pos, u8>,
    /// pistons mid-move
    moving: HashMap<Pos, Move>,
    /// recent flips of each redstone torch (game ticks), and torches burnt out until a tick
    flips: HashMap<Pos, Vec<u64>>,
    burnt: HashMap<Pos, u64>,
}

/// A piston move in progress (PNX BlockEntityPistonArm): blocks sit as moving_block at their
/// destinations until it's done.
#[derive(Clone)]
struct Move {
    extending: bool,
    sticky: bool,
    /// (destination, the real block to put there when done)
    to: Vec<(Pos, i32)>,
    /// where the blocks started, for the arm's AttachedBlocks
    from: Vec<Pos>,
    dir: Pos,
    progress: f32,
}

/// What the client needs to draw a piston arm.
#[derive(Debug, Clone, PartialEq)]
pub struct Arm {
    pub at: Pos,
    pub sticky: bool,
    /// 0 retracted, 1 extending, 2 extended, 3 retracting
    pub state: i8,
    pub progress: f32,
    pub last: f32,
    pub attached: Vec<Pos>,
}

static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);

fn name(b: i32) -> &'static str {
    entry(b).map_or("", |e| e.name.as_str()).trim_start_matches("minecraft:")
}

fn int(b: i32, key: &str) -> i32 {
    match state(b, key) {
        Some(State::Int(i)) => *i,
        Some(State::Bool(v)) => *v as i32,
        _ => 0,
    }
}

fn text(b: i32, key: &str) -> &'static str {
    match state(b, key) {
        Some(State::Str(s)) => s.as_str(),
        _ => "",
    }
}

/// A full block that conducts power (not dust, not a component, not glass or leaves).
fn solid(b: i32) -> bool {
    crate::connect::conductor(b) && !is_component(b)
}

fn is_component(b: i32) -> bool {
    let n = name(b);
    n == "redstone_block" || n == "target" || n == "tripwire_hook" || n == "trapped_chest" || n == "detector_rail" || n.contains("repeater") || n.contains("comparator") || n == "observer" || n.contains("piston") || n.starts_with("daylight_detector") || n.ends_with("sculk_sensor")
}

/// Output side of a repeater/comparator. Its facing (south, west, north, east) is the input
/// side; it outputs the other way. Current Bedrock uses `minecraft:cardinal_direction`, older
/// data the `direction` number (0 south, 1 west, 2 north, 3 east).
fn diode_out(b: i32) -> Pos {
    let facing = match text(b, "minecraft:cardinal_direction") {
        "south" => 0,
        "west" => 1,
        "north" => 2,
        "east" => 3,
        _ => int(b, "direction"),
    };
    match facing {
        0 => (0, 0, -1),
        1 => (1, 0, 0),
        2 => (0, 0, 1),
        _ => (-1, 0, 0),
    }
}

/// Which way a tripwire hook points (its string runs this way): direction 0 south, 1 west,
/// 2 north, 3 east.
fn hook_out(b: i32) -> Pos {
    match int(b, "direction") {
        0 => (0, 0, 1),
        1 => (-1, 0, 0),
        2 => (0, 0, -1),
        _ => (1, 0, 0),
    }
}

/// Players looking into each trapped chest.
static TRAPPED: Mutex<Option<HashMap<Pos, u8>>> = Mutex::new(None);

/// Target blocks hit: (level, game tick it ends).
static TARGETS: Mutex<Option<HashMap<Pos, (u8, u64)>>> = Mutex::new(None);

/// A container's viewer count changed (trapped chests power redstone with it).
pub fn set_viewers(p: Pos, n: u32) {
    let mut t = TRAPPED.lock();
    let t = t.get_or_insert_with(HashMap::new);
    if n == 0 {
        t.remove(&p);
    } else {
        t.insert(p, n.min(15) as u8);
    }
    mark(p);
}

/// A projectile stuck in a target block at `hit` (block-relative 0..1 on each axis): the
/// closer to the middle of the face, the stronger, for `ticks` game ticks (vanilla: arrows 20,
/// other projectiles 8).
pub fn hit_target(p: Pos, hit: (f32, f32, f32), ticks: u64) {
    // Distance from the centre on the two axes the face spans; the face is the one nearest.
    let d = [(hit.0 - 0.5).abs(), (hit.1 - 0.5).abs(), (hit.2 - 0.5).abs()];
    let face = (0..3).max_by(|&a, &b| d[a].total_cmp(&d[b])).unwrap_or(0);
    let off = (0..3).filter(|&i| i != face).map(|i| d[i]).fold(0.0f32, f32::max);
    let level = ((1.0 - off / 0.5).clamp(0.0, 1.0) * 15.0).ceil().max(1.0) as u8;
    let now = ENGINE.lock().as_ref().map_or(0, |e| e.tick);
    TARGETS.lock().get_or_insert_with(HashMap::new).insert(p, (level, now + ticks));
    mark(p);
}

/// Observers look at this face and output out the back.
fn observer_face(b: i32) -> Pos {
    match text(b, "minecraft:facing_direction") {
        "down" => DIRS[0],
        "up" => DIRS[1],
        "north" => DIRS[2],
        "south" => DIRS[3],
        "west" => DIRS[4],
        _ => DIRS[5],
    }
}

/// PNX stores horizontal piston facings reversed.
fn piston_face(b: i32) -> Pos {
    match int(b, "facing_direction") {
        0 => DIRS[0],
        1 => DIRS[1],
        2 => DIRS[3],
        3 => DIRS[2],
        4 => DIRS[5],
        _ => DIRS[4],
    }
}

/// (level, strong) that `src` sends into `to`. Strong power also goes through the solid
/// block it hits.
fn emits(w: &World, src: Pos, to: Pos, comparators: &HashMap<Pos, u8>) -> (u8, bool) {
    let b = w.get(src.0, src.1, src.2);
    let n = name(b);
    let dir = (to.0 - src.0, to.1 - src.1, to.2 - src.2);
    let attached = crate::physics::support_of(b).map(|o| add(src, o));
    match n {
        "redstone_block" => (15, false),
        // Trapped chests: one level per player looking in, strong into the block below.
        "trapped_chest" => {
            let n = TRAPPED.lock().as_ref().and_then(|t| t.get(&src).copied()).unwrap_or(0);
            (n, n > 0 && dir == DIRS[0])
        }
        "target" => (TARGETS.lock().as_ref().and_then(|t| t.get(&src).map(|x| x.0)).unwrap_or(0), false),
        // A pulled hook powers everything around it and strongly the block it hangs on.
        "tripwire_hook" if int(b, "powered_bit") == 1 => (15, dir == neg(hook_out(b))),
        "sculk_sensor" | "calibrated_sculk_sensor" => (crate::sculk::power(src), false),
        "lever" if int(b, "open_bit") == 1 => (15, attached == Some(to)),
        n if n.ends_with("_button") && int(b, "button_pressed_bit") == 1 => (15, attached == Some(to)),
        n if n.ends_with("pressure_plate") => {
            let s = int(b, "redstone_signal").clamp(0, 15) as u8;
            (s, dir == DIRS[0] && s > 0)
        }
        // A cart on a detector rail powers its neighbours and strongly the block under it.
        "detector_rail" if int(b, "rail_data_bit") == 1 => (15, dir == DIRS[0]),
        "redstone_torch" => {
            if attached == Some(to) {
                (0, false)
            } else {
                (15, dir == DIRS[1])
            }
        }
        "redstone_wire" => {
            // Vanilla: the block under it, and the sides its shape points at (a lone cross
            // points everywhere).
            let s = int(b, "redstone_signal").clamp(0, 15) as u8;
            let side = match (dir.0, dir.1, dir.2) {
                (0, -1, 0) => return (s, false),
                (0, 0, -1) => "redstone_north",
                (0, 0, 1) => "redstone_south",
                (1, 0, 0) => "redstone_east",
                (-1, 0, 0) => "redstone_west",
                _ => return (0, false),
            };
            // Worked out from the neighbours, since saved dust may not carry its shape.
            let shaped = crate::connect::shape(&|x, y, z| w.get(x, y, z), src).unwrap_or(b);
            if text(shaped, side) == "none" { (0, false) } else { (s, false) }
        }
        "powered_repeater" if dir == diode_out(b) => (15, true),
        "powered_comparator" | "unpowered_comparator" if dir == diode_out(b) => {
            let s = comparators.get(&src).copied().unwrap_or(0);
            (s, s > 0)
        }
        "observer" if int(b, "powered_bit") == 1 && dir == neg(observer_face(b)) => (15, true),
        "daylight_detector" | "daylight_detector_inverted" => (int(b, "redstone_signal").clamp(0, 15) as u8, false),
        _ => (0, false),
    }
}

/// Strong power in a solid block, which is what dust and components pick up.
fn strong_in(w: &World, p: Pos, c: &HashMap<Pos, u8>) -> u8 {
    DIRS.iter().map(|&d| emits(w, add(p, d), p, c)).filter(|e| e.1).map(|e| e.0).max().unwrap_or(0)
}

/// Any power in a solid block, strong or weak from dust. Enough for lamps and doors
/// but not for feeding dust.
fn weak_in(w: &World, p: Pos, c: &HashMap<Pos, u8>) -> u8 {
    DIRS.iter().map(|&d| emits(w, add(p, d), p, c).0).max().unwrap_or(0)
}

/// Power coming into `p` from all sides (PNX isBlockPowered / getRedstonePower).
fn received(w: &World, p: Pos, c: &HashMap<Pos, u8>, except: Option<Pos>) -> u8 {
    DIRS.iter()
        .map(|&d| add(p, d))
        .filter(|&q| Some(q) != except)
        .map(|q| {
            let b = w.get(q.0, q.1, q.2);
            let direct = emits(w, q, p, c).0;
            if solid(b) { direct.max(weak_in(w, q, c)) } else { direct }
        })
        .max()
        .unwrap_or(0)
}

/// Dust connected to `start`, sideways plus one up or down a block edge.
fn network(w: &World, start: Pos) -> Vec<Pos> {
    let mut seen = HashSet::from([start]);
    let mut q = VecDeque::from([start]);
    while let Some(p) = q.pop_front() {
        if seen.len() > 1024 {
            break;
        }
        for &(dx, _, dz) in &DIRS[2..] {
            let side = (p.0 + dx, p.1, p.2 + dz);
            let mut cands = vec![side];
            if !solid(w.get(p.0, p.1 + 1, p.2)) {
                cands.push((side.0, side.1 + 1, side.2));
            }
            if !solid(w.get(side.0, side.1, side.2)) {
                cands.push((side.0, side.1 - 1, side.2));
            }
            for n in cands {
                if name(w.get(n.0, n.1, n.2)) == "redstone_wire" && seen.insert(n) {
                    q.push_back(n);
                }
            }
        }
    }
    seen.into_iter().collect()
}

/// Strongest input minus 1 per dust step.
fn solve_wire(w: &World, wires: &[Pos], c: &HashMap<Pos, u8>) -> HashMap<Pos, u8> {
    let set: HashSet<Pos> = wires.iter().copied().collect();
    let mut power: HashMap<Pos, u8> = HashMap::new();
    for &p in wires {
        let mut best = 0u8;
        for &d in &DIRS {
            let q = add(p, d);
            let b = w.get(q.0, q.1, q.2);
            if name(b) == "redstone_wire" {
                continue;
            }
            best = best.max(emits(w, q, p, c).0);
            if solid(b) {
                best = best.max(strong_in(w, q, c));
            }
        }
        power.insert(p, best);
    }
    // spread from strongest down, basically Dijkstra by level
    let mut order: Vec<Pos> = wires.to_vec();
    order.sort_by_key(|p| std::cmp::Reverse(power[p]));
    let mut q: VecDeque<Pos> = order.into_iter().collect();
    while let Some(p) = q.pop_front() {
        let v = power[&p];
        if v <= 1 {
            continue;
        }
        for &(dx, _, dz) in &DIRS[2..] {
            for dy in [-1, 0, 1] {
                let n = (p.0 + dx, p.1 + dy, p.2 + dz);
                // Up a step needs open air over this dust, down one needs the side block open,
                // the same cut rule `network` uses.
                let cut = (dy == 1 && solid(w.get(p.0, p.1 + 1, p.2))) || (dy == -1 && solid(w.get(p.0 + dx, p.1, p.2 + dz)));
                if !cut && set.contains(&n) && power[&n] < v - 1 {
                    power.insert(n, v - 1);
                    q.push_back(n);
                }
            }
        }
    }
    power
}

/// Changes and side effects from one redstone tick.
#[derive(Debug, Default, PartialEq)]
pub struct Step {
    pub blocks: Vec<(Pos, i32)>,
    pub ignite: Vec<Pos>,
    /// dispensers/droppers that just got a rising edge
    pub dispense: Vec<Pos>,
    pub notes: Vec<Pos>,
    pub sounds: Vec<(Pos, &'static str)>,
    /// piston arm block entity updates
    pub pistons: Vec<Arm>,
    /// moving_block block entities: (where, data)
    pub moving_blocks: Vec<(Pos, HashMap<String, nbtx::Value>)>,
    /// blocks a piston crushed: (where, what), to drop as items
    pub broken: Vec<(Pos, i32)>,
    /// mobs and items in these cells get pushed half a block this way
    pub shove: Vec<(Pos, Pos)>,
    /// containers a piston moved: (from, to), so their contents follow
    pub moves: Vec<(Pos, Pos)>,
}

fn sched(e: &mut Engine, p: Pos, job: Job, delay: u64) {
    if e.pending.insert(p) {
        e.due.entry(e.tick + delay).or_default().push((p, job));
    }
}

fn rename(b: i32, new: &str) -> i32 {
    crate::blocks::rename(b, new)
}

/// Should this diode be on now, and which block should it be.
/// A repeater is locked (keeps its state) while a powered repeater or comparator points
/// into its side (vanilla).
fn locked(w: &World, p: Pos, b: i32, c: &HashMap<Pos, u8>) -> bool {
    let (dx, _, dz) = diode_out(b);
    [(dz, 0, -dx), (-dz, 0, dx)].into_iter().any(|side| {
        let q = add(p, side);
        let qb = w.get(q.0, q.1, q.2);
        diode_out(qb) == neg(side)
            && match name(qb) {
                "powered_repeater" => true,
                "powered_comparator" | "unpowered_comparator" => c.get(&q).copied().unwrap_or(0) > 0,
                _ => false,
            }
    })
}

fn diode_target(w: &World, p: Pos, b: i32, c: &mut HashMap<Pos, u8>, fill: &dyn Fn(Pos) -> Option<u8>) -> Option<i32> {
    let n = name(b);
    match n {
        "unpowered_repeater" | "powered_repeater" if locked(w, p, b, c) => None,
        "unpowered_repeater" | "powered_repeater" => {
            let back = add(p, neg(diode_out(b)));
            let bb = w.get(back.0, back.1, back.2);
            let input = if name(bb) == "redstone_wire" { int(bb, "redstone_signal") as u8 } else if solid(bb) { weak_in(w, back, c) } else { emits(w, back, p, c).0 };
            let want = input > 0;
            let is = n == "powered_repeater";
            (want != is).then(|| rename(b, if want { "powered_repeater" } else { "unpowered_repeater" }))
        }
        "redstone_torch" | "unlit_redstone_torch" => {
            let support = crate::physics::support_of(b).map(|o| add(p, o))?;
            let powered = weak_in(w, support, c) > 0;
            let lit = n == "redstone_torch";
            (powered == lit).then(|| rename(b, if powered { "unlit_redstone_torch" } else { "redstone_torch" }))
        }
        "unpowered_comparator" | "powered_comparator" => {
            let back = add(p, neg(diode_out(b)));
            let bb = w.get(back.0, back.1, back.2);
            let mut input = if name(bb) == "redstone_wire" { int(bb, "redstone_signal") as u8 } else if solid(bb) { weak_in(w, back, c) } else { emits(w, back, p, c).0 };
            if let Some(fill) = fill(back) {
                input = input.max(fill);
            } else if solid(bb) && input < 15 {
                // A comparator reads a container through one solid block.
                let behind = add(back, neg(diode_out(b)));
                if let Some(fill) = fill(behind) {
                    input = input.max(fill);
                }
            }
            let out = diode_out(b);
            let sides = [(out.2, 0, -out.0), (-out.2, 0, out.0)];
            let side = sides
                .iter()
                .map(|&s| {
                    let q = add(p, s);
                    let qb = w.get(q.0, q.1, q.2);
                    match name(qb) {
                        "redstone_wire" => int(qb, "redstone_signal") as u8,
                        "redstone_block" => 15,
                        "powered_repeater" if diode_out(qb) == neg(s) => 15,
                        "powered_comparator" | "unpowered_comparator" if diode_out(qb) == neg(s) => c.get(&q).copied().unwrap_or(0),
                        _ => 0,
                    }
                })
                .max()
                .unwrap_or(0);
            let subtract = int(b, "output_subtract_bit") == 1;
            let output = if subtract { input.saturating_sub(side) } else if side > input { 0 } else { input };
            let old = c.get(&p).copied().unwrap_or(0);
            let lit = n == "powered_comparator";
            if output == old && lit == (output > 0) {
                return None;
            }
            c.insert(p, output);
            Some(rename(with(b, &[("output_lit_bit", State::Bool(output > 0))]), if output > 0 { "powered_comparator" } else { "unpowered_comparator" }))
        }
        _ => None,
    }
}

fn immovable(w: &World, at: Pos, b: i32) -> bool {
    let n = name(b);
    matches!(n, "obsidian" | "crying_obsidian" | "bedrock" | "barrier" | "end_portal_frame" | "enchanting_table" | "respawn_anchor" | "reinforced_deepslate" | "beacon" | "moving_block")
        || n.contains("piston_arm")
        || n.ends_with("_sign")
        || n == "jukebox"
        // An extended piston stays put.
        || ((n == "piston" || n == "sticky_piston") && {
            let f = add(at, piston_face(b));
            name(w.get(f.0, f.1, f.2)).contains("piston_arm")
        })
}

/// Pistons break these instead of pushing them (plants, torches, water...).
fn breaks(b: i32) -> bool {
    let n = name(b);
    n.ends_with("water") || n.ends_with("lava") || n.ends_with("torch") || n.contains("flower") || n.ends_with("grass") || n.ends_with("_sapling")
        || n == "fire" || n == "redstone_wire" || n.ends_with("_carpet") || n == "snow_layer" || n == "vine" || n.ends_with("_mushroom") || n == "deadbush"
        || matches!(n, "wheat" | "carrots" | "potatoes" | "beetroot" | "reeds" | "fern" | "tall_grass" | "short_grass")
        // Anything hanging on a block (buttons, levers, ladders, doors, rails, plates).
        || crate::physics::support_of(b).is_some()
}

/// Slime and honey drag the blocks touching them along.
fn sticky_block(b: i32) -> bool {
    matches!(name(b), "slime" | "honey_block")
}

/// Whether slime/honey can drag this block (glazed terracotta can't be).
fn sticks(b: i32) -> bool {
    !name(b).ends_with("glazed_terracotta")
}

/// Which blocks a piston moves and breaks, ported from PNX BlockPistonBase.BlocksCalculator
/// (vanilla's algorithm): the main line, then side branches off slime and honey, 12 blocks max.
struct Calc<'a> {
    w: &'a World,
    piston: Pos,
    dir: Pos,
    extending: bool,
    arm: Option<Pos>,
    to_move: Vec<Pos>,
    to_destroy: Vec<Pos>,
}

const PUSH_LIMIT: usize = 12;

impl Calc<'_> {
    fn get(&self, p: Pos) -> i32 {
        self.w.get(p.0, p.1, p.2)
    }

    fn can_push(&self, p: Pos, destroy: bool) -> bool {
        let b = self.get(p);
        let (min, max) = (-64, 319);
        if p.1 < min || p.1 > max || (self.dir == DIRS[0] && p.1 == min) || (self.dir == DIRS[1] && p.1 == max) || immovable(self.w, p, b) {
            return false;
        }
        if breaks(b) {
            return destroy || sticks(b);
        }
        true
    }

    fn can_stick(&self, a: Pos, b: Pos) -> bool {
        let (ga, gb) = (self.get(a), self.get(b));
        if !sticky_block(ga) || !sticks(gb) {
            return false;
        }
        if sticky_block(gb) { name(ga) == name(gb) } else { true }
    }

    fn can_move(&mut self, sticky: bool, face: Pos) -> bool {
        if !sticky && !self.extending {
            return true;
        }
        let start = if self.extending { add(self.piston, face) } else { add(add(self.piston, face), face) };
        if !self.can_push(start, true) {
            return false;
        }
        let b = self.get(start);
        if b == AIR {
            return true;
        }
        if breaks(b) {
            if self.extending || sticks(b) {
                self.to_destroy.push(start);
            }
            return true;
        }
        if !self.add_line(start, add(start, self.dir), true) {
            return false;
        }
        self.expand()
    }

    fn add_line(&mut self, origin: Pos, from: Pos, main: bool) -> bool {
        let b = self.get(origin);
        if b == AIR
            || (!main && !self.can_stick(from, origin))
            || !self.can_push(origin, false)
            || origin == self.piston
            || self.to_move.contains(&origin)
        {
            return true;
        }
        if self.to_move.len() >= PUSH_LIMIT {
            return false;
        }
        self.to_move.push(origin);
        // Blocks stuck behind it (against the move) come too.
        let mut count = 1;
        let mut stuck = Vec::new();
        let mut cur = origin;
        while sticky_block(self.get(cur)) {
            let old = cur;
            cur = (origin.0 - self.dir.0 * count, origin.1 - self.dir.1 * count, origin.2 - self.dir.2 * count);
            if !self.can_stick(old, cur) {
                break;
            }
            let cb = self.get(cur);
            if cb == AIR || !self.can_push(cur, false) || cur == self.piston {
                break;
            }
            if breaks(cb) && sticks(cb) {
                self.to_destroy.push(cur);
                break;
            }
            if count as usize + self.to_move.len() > PUSH_LIMIT {
                return false;
            }
            count += 1;
            stuck.push(cur);
        }
        let mut stuck_count = stuck.len();
        self.to_move.extend(stuck.into_iter().rev());
        // Then everything in front of it.
        let mut step = 1;
        loop {
            let next = (origin.0 + self.dir.0 * step, origin.1 + self.dir.1 * step, origin.2 + self.dir.2 * step);
            if let Some(i) = self.to_move.iter().position(|&q| q == next) {
                self.reorder(stuck_count, i);
                return self.expand();
            }
            let nb = self.get(next);
            if nb == AIR || Some(next) == self.arm {
                return true;
            }
            if !self.can_push(next, true) || next == self.piston {
                return false;
            }
            if breaks(nb) {
                self.to_destroy.push(next);
                return true;
            }
            if self.to_move.len() >= PUSH_LIMIT {
                return false;
            }
            self.to_move.push(next);
            stuck_count += 1;
            step += 1;
        }
    }

    fn expand(&mut self) -> bool {
        let mut i = 0;
        while i < self.to_move.len() {
            let p = self.to_move[i];
            if sticky_block(self.get(p)) {
                for d in DIRS {
                    let same_axis = (d.0 != 0 && self.dir.0 != 0) || (d.1 != 0 && self.dir.1 != 0) || (d.2 != 0 && self.dir.2 != 0);
                    if !same_axis && !self.add_line(add(p, d), p, false) {
                        return false;
                    }
                }
            }
            i += 1;
        }
        true
    }

    fn reorder(&mut self, count: usize, index: usize) {
        let len = self.to_move.len();
        let a = self.to_move[..index].to_vec();
        let b = self.to_move[len - count..].to_vec();
        let c = self.to_move[index..len - count].to_vec();
        self.to_move = [a, b, c].concat();
    }
}

/// Starts a piston move (PNX BlockPistonBase.doMove): crushed blocks go, moved blocks become
/// moving_block at their destinations, the arm starts its 2-tick slide. None if it can't extend.
fn piston(w: &World, p: Pos, b: i32, extend: bool, step: &mut Step) -> Option<Move> {
    let face = piston_face(b);
    let sticky = name(b) == "sticky_piston";
    let head = add(p, face);
    let dir = if extend { face } else { neg(face) };
    let mut calc = Calc { w, piston: p, dir, extending: extend, arm: (!extend).then_some(head), to_move: vec![], to_destroy: vec![] };
    let can = calc.can_move(sticky, face);
    if !can && extend {
        return None;
    }
    if !extend {
        step.blocks.push((head, AIR));
    }
    let mut mv = Move { extending: extend, sticky, to: vec![], from: vec![], dir, progress: if extend { 0.0 } else { 1.0 } };
    if can && (sticky || extend) {
        // Buttons, torches and the like on a moved block's sides fall off.
        for i in 0..calc.to_move.len() {
            let q = calc.to_move[i];
            for d in DIRS {
                let nb = add(q, d);
                if crate::physics::support_of(w.get(nb.0, nb.1, nb.2)) == Some(neg(d)) && !calc.to_move.contains(&nb) && !calc.to_destroy.contains(&nb) {
                    calc.to_destroy.push(nb);
                }
            }
        }
        for &q in &calc.to_destroy {
            step.broken.push((q, w.get(q.0, q.1, q.2)));
            step.blocks.push((q, AIR));
        }
        // Clear every old spot first, then fill the new ones, so chains overlap correctly.
        let moved: Vec<(Pos, i32)> = calc.to_move.iter().map(|&q| (q, w.get(q.0, q.1, q.2))).collect();
        for &(q, _) in &moved {
            step.blocks.push((q, AIR));
        }
        let moving = any_state("minecraft:moving_block").unwrap_or(AIR);
        for &(q, x) in &moved {
            let to = add(q, dir);
            step.blocks.push((to, moving));
            step.moving_blocks.push((to, moving_block_data(to, p, x, extend)));
            step.shove.push((to, dir));
            mv.to.push((to, x));
            mv.from.push(q);
            if crate::containers::kind(x).is_some() {
                step.moves.push((q, to));
            }
        }
    }
    if extend {
        let arm = any_state(if sticky { "minecraft:sticky_piston_arm_collision" } else { "minecraft:piston_arm_collision" })
            .map(|a| with(a, &[("facing_direction", State::Int(int(b, "facing_direction")))]))
            .unwrap_or(AIR);
        step.blocks.push((head, arm));
        step.shove.push((head, dir));
        step.sounds.push((p, "piston.out"));
    } else {
        step.sounds.push((p, "piston.in"));
    }
    step.pistons.push(Arm { at: p, sticky, state: if extend { 1 } else { 3 }, progress: mv.progress, last: mv.progress, attached: mv.from.clone() });
    Some(mv)
}

/// The MovingBlock block entity a pushed block rides in (PNX BlockEntityMovingBlock).
fn moving_block_data(at: Pos, piston: Pos, block: i32, expanding: bool) -> HashMap<String, nbtx::Value> {
    let nbt = entry(block).map_or(nbtx::Value::Int(0), |e| e.nbt.clone());
    let mut t = HashMap::new();
    t.insert("id".into(), nbtx::Value::String("MovingBlock".into()));
    t.insert("x".into(), nbtx::Value::Int(at.0));
    t.insert("y".into(), nbtx::Value::Int(at.1));
    t.insert("z".into(), nbtx::Value::Int(at.2));
    t.insert("isMovable".into(), nbtx::Value::Byte(1));
    t.insert("expanding".into(), nbtx::Value::Byte(expanding as i8));
    t.insert("pistonPosX".into(), nbtx::Value::Int(piston.0));
    t.insert("pistonPosY".into(), nbtx::Value::Int(piston.1));
    t.insert("pistonPosZ".into(), nbtx::Value::Int(piston.2));
    t.insert("movingBlock".into(), nbt);
    t
}

fn evaluate(w: &World, e: &mut Engine, p: Pos, wired: &mut HashSet<Pos>, step: &mut Step, fill: &dyn Fn(Pos) -> Option<u8>) {
    let b = w.get(p.0, p.1, p.2);
    let n = name(b);
    match n {
        "redstone_wire" => {
            if wired.contains(&p) {
                return;
            }
            let net = network(w, p);
            wired.extend(net.iter().copied());
            for (q, v) in solve_wire(w, &net, &e.comparators) {
                let qb = w.get(q.0, q.1, q.2);
                if int(qb, "redstone_signal") != v as i32 {
                    step.blocks.push((q, with(qb, &[("redstone_signal", State::Int(v as i32))])));
                }
            }
        }
        "unpowered_repeater" | "powered_repeater" => {
            if let Some(nb) = diode_target(w, p, b, &mut e.comparators.clone(), fill) {
                let delay = 2 * (int(b, "repeater_delay") as u64 + 1);
                sched(e, p, Job::Repeater(name(nb) == "powered_repeater"), delay);
            }
        }
        "redstone_torch" | "unlit_redstone_torch" | "unpowered_comparator" | "powered_comparator" => {
            if diode_target(w, p, b, &mut e.comparators.clone(), fill).is_some() {
                sched(e, p, Job::Diode, 2);
            }
        }
        "tripwire_hook" => tripwire(w, p, b, step),
        "trip_wire" => {
            // Re-check the hooks at either end of this string.
            for d in [DIRS[2], DIRS[3], DIRS[4], DIRS[5]] {
                let mut q = add(p, d);
                for _ in 0..42 {
                    let qb = w.get(q.0, q.1, q.2);
                    match name(qb) {
                        "trip_wire" => q = add(q, d),
                        "tripwire_hook" => {
                            tripwire(w, q, qb, step);
                            break;
                        }
                        _ => break,
                    }
                }
            }
        }
        "daylight_detector" | "daylight_detector_inverted" => {
            detectors().insert(p);
        }
        "redstone_lamp" | "lit_redstone_lamp" => {
            let on = received(w, p, &e.comparators, None) > 0;
            if on != (n == "lit_redstone_lamp") {
                step.blocks.push((p, rename(b, if on { "lit_redstone_lamp" } else { "redstone_lamp" })));
            }
        }
        "piston" | "sticky_piston" => {
            let front = add(p, piston_face(b));
            let on = received(w, p, &e.comparators, Some(front)) > 0;
            // Extended = its arm is in front (not a memory flag, so restarts keep it).
            let ext = name(w.get(front.0, front.1, front.2)).contains("piston_arm");
            if on != ext && !e.moving.contains_key(&p)
                && let Some(mv) = piston(w, p, b, on, step)
            {
                e.moving.insert(p, mv);
                sched(e, p, Job::Piston, 1);
            }
        }
        "hopper" => {
            let on = received(w, p, &e.comparators, None) > 0;
            if on != (int(b, "toggle_bit") == 1) {
                step.blocks.push((p, with(b, &[("toggle_bit", State::Bool(on))])));
            }
        }
        n if n == "tnt" || n == "noteblock" || n == "dispenser" || n == "dropper" || n == "crafter" || n.ends_with("copper_bulb") || n.ends_with("_door") || n.ends_with("trapdoor") || n.ends_with("fence_gate") || n == "wooden_door" => {
            // edge-triggered, only a change does anything (so players can still use doors)
            let key = if n.ends_with("_door") || n == "wooden_door" {
                if int(b, "upper_block_bit") == 1 { (p.0, p.1 - 1, p.2) } else { p }
            } else {
                p
            };
            let on = received(w, key, &e.comparators, None) > 0
                || (key != (p.0, p.1 - 1, p.2) && (n.ends_with("_door") || n == "wooden_door") && received(w, (key.0, key.1 + 1, key.2), &e.comparators, None) > 0);
            let was = e.last.get(&key).copied().unwrap_or(false);
            if on == was {
                return;
            }
            e.last.insert(key, on);
            match n {
                "tnt" if on => step.ignite.push(p),
                "noteblock" if on => step.notes.push(p),
                "dispenser" | "dropper" | "crafter" if on => sched(e, p, Job::Dispense, 4),
                "tnt" | "noteblock" | "dispenser" | "dropper" | "crafter" => {}
                // Copper bulbs flip on each new pulse and remember being powered.
                n if n.ends_with("copper_bulb") => {
                    let lit = int(b, "lit") == 1;
                    let flipped = if on { !lit } else { lit };
                    step.blocks.push((p, with(b, &[("lit", State::Bool(flipped)), ("powered_bit", State::Bool(on))])));
                    if on {
                        step.sounds.push((p, if flipped { "copper_bulb.turn_on" } else { "copper_bulb.turn_off" }));
                    }
                }
                _ => {
                    let lower = w.get(key.0, key.1, key.2);
                    if int(lower, "open_bit") == on as i32 {
                        return;
                    }
                    step.blocks.push((key, with(lower, &[("open_bit", State::Bool(on))])));
                    if n.ends_with("_door") || n == "wooden_door" {
                        let up = (key.0, key.1 + 1, key.2);
                        let ub = w.get(up.0, up.1, up.2);
                        if name(ub) == n {
                            step.blocks.push((up, with(ub, &[("open_bit", State::Bool(on))])));
                        }
                    }
                    step.sounds.push((key, if on { "random.door_open" } else { "random.door_close" }));
                }
            }
        }
        _ => {}
    }
}

/// A tripwire hook: attached when a string of up to 40 tripwire runs straight to a hook
/// facing back; powered while anything stands on that string (PNX BlockTripWireHook).
fn tripwire(w: &World, p: Pos, b: i32, step: &mut Step) {
    let d = hook_out(b);
    let mut wires = vec![];
    let mut q = add(p, d);
    let mut other = None;
    for _ in 0..41 {
        let qb = w.get(q.0, q.1, q.2);
        match name(qb) {
            "trip_wire" => wires.push((q, qb)),
            "tripwire_hook" if hook_out(qb) == neg(d) => {
                other = Some((q, qb));
                break;
            }
            _ => break,
        }
        q = add(q, d);
    }
    let attached = other.is_some() && !wires.is_empty();
    let powered = attached && wires.iter().any(|&(_, wb)| int(wb, "powered_bit") == 1 && int(wb, "disarmed_bit") == 0);
    for (hp, hb) in std::iter::once((p, b)).chain(other) {
        let nb = with(hb, &[("attached_bit", State::Bool(attached)), ("powered_bit", State::Bool(powered))]);
        if nb != hb {
            step.blocks.push((hp, nb));
            if int(hb, "powered_bit") != powered as i32 {
                step.sounds.push((hp, "random.click"));
            }
        }
    }
    for (wp, wb) in wires {
        let nb = with(wb, &[("attached_bit", State::Bool(attached))]);
        if nb != wb {
            step.blocks.push((wp, nb));
        }
    }
}

/// Daylight detectors seen so far (placed, or touched by a redstone update).
/// shortcut: forgotten on restart until something next to them changes; scan chunks on load
/// if that matters.
static DETECTORS: Mutex<Option<HashSet<Pos>>> = Mutex::new(None);

fn detectors() -> parking_lot::MappedMutexGuard<'static, HashSet<Pos>> {
    parking_lot::MutexGuard::map(DETECTORS.lock(), |d| d.get_or_insert_with(HashSet::new))
}

/// Level a daylight detector gives at `time` (0-24000): 15 around noon, 0 at night, and the
/// inverted one the opposite.
pub fn daylight_level(time: i32, inverted: bool) -> i32 {
    let angle = ((time.rem_euclid(24000) as f32 - 6000.0) / 24000.0) * std::f32::consts::TAU;
    let level = ((angle.cos() * 1.5).clamp(0.0, 1.0) * 15.0).round() as i32;
    if inverted { 15 - level } else { level }
}

/// Once a second: daylight detectors follow the time of day.
pub fn daylight(server: &crate::players::Server) {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 20 != 0 {
        return;
    }
    let time = server.time();
    let list: Vec<Pos> = DETECTORS.lock().get_or_insert_with(HashSet::new).iter().copied().collect();
    for p in list {
        let b = server.world.get(p.0, p.1, p.2);
        let n = name(b);
        if !n.starts_with("daylight_detector") {
            if let Some(d) = DETECTORS.lock().as_mut() {
                d.remove(&p);
            }
            continue;
        }
        let level = daylight_level(time, n == "daylight_detector_inverted");
        if int(b, "redstone_signal") != level {
            server.set_block_quiet(0, p.0, p.1, p.2, with(b, &[("redstone_signal", State::Int(level))]));
        }
    }
}

/// Blocks `evaluate` does something with, looked up in a set built once (every changed block
/// checks ~125 neighbours, so this has to be cheap).
fn relevant(b: i32) -> bool {
    static SET: std::sync::OnceLock<HashSet<i32>> = std::sync::OnceLock::new();
    SET.get_or_init(|| {
        crate::blocks::ids().iter().copied().filter(|&b| {
            let n = name(b);
            matches!(n, "redstone_wire" | "redstone_torch" | "unlit_redstone_torch" | "redstone_lamp" | "lit_redstone_lamp" | "piston" | "sticky_piston" | "hopper" | "tnt" | "noteblock" | "dispenser" | "dropper" | "crafter" | "wooden_door" | "tripwire_hook" | "trip_wire")
                || n.ends_with("copper_bulb")
                || n.contains("repeater")
                || n.contains("comparator")
                || n.starts_with("daylight_detector")
                || n.ends_with("_door")
                || n.ends_with("trapdoor")
                || n.ends_with("fence_gate")
        }).collect()
    }).contains(&b)
}

/// One redstone tick. Reads `w`, returns what to change. `fill` gives a container's
/// comparator reading (0-15), None if there isn't one.
pub fn tick(w: &World, fill: &dyn Fn(Pos) -> Option<u8>) -> Step {
    let mut guard = ENGINE.lock();
    let e = guard.get_or_insert_with(Engine::default);
    e.tick += 1;
    let mut step = Step::default();
    let mut wired = HashSet::new();

    let due: Vec<(Pos, Job)> = {
        let keys: Vec<u64> = e.due.range(..=e.tick).map(|(k, _)| *k).collect();
        keys.into_iter().flat_map(|k| e.due.remove(&k).unwrap_or_default()).collect()
    };
    for (p, job) in due {
        e.pending.remove(&p);
        let b = w.get(p.0, p.1, p.2);
        match job {
            Job::Diode => {
                // A burnt-out torch stays off until its time is up (vanilla: 8 flips in 60
                // ticks burns it out for 160).
                if e.burnt.get(&p).is_some_and(|&until| e.tick < until) {
                    continue;
                }
                e.burnt.remove(&p);
                let mut c = e.comparators.clone();
                if let Some(nb) = diode_target(w, p, b, &mut c, fill) {
                    e.comparators = c;
                    if name(b).ends_with("redstone_torch") {
                        let now = e.tick;
                        let flips = e.flips.entry(p).or_default();
                        flips.retain(|&t| now - t < 60);
                        flips.push(now);
                        if flips.len() >= 8 && name(nb) == "redstone_torch" {
                            e.flips.remove(&p);
                            e.burnt.insert(p, now + 160);
                            sched(e, p, Job::Diode, 161);
                            step.sounds.push((p, "random.fizz"));
                            continue;
                        }
                    }
                    step.blocks.push((p, nb));
                }
            }
            Job::Repeater(on) if name(b).contains("repeater") && !locked(w, p, b, &e.comparators) => {
                let is = name(b) == "powered_repeater";
                if on != is {
                    step.blocks.push((p, rename(b, if on { "powered_repeater" } else { "unpowered_repeater" })));
                }
            }
            Job::Dispense => step.dispense.push(p),
            Job::Piston => {
                let Some(mut mv) = e.moving.remove(&p) else { continue };
                let last = mv.progress;
                mv.progress = if mv.extending { (mv.progress + 0.5).min(1.0) } else { (mv.progress - 0.5).max(0.0) };
                let done = if mv.extending { mv.progress >= 1.0 } else { mv.progress <= 0.0 };
                for &(to, _) in &mv.to {
                    step.shove.push((to, mv.dir));
                }
                if done {
                    // The moving blocks become the real blocks.
                    for &(to, x) in &mv.to {
                        if name(w.get(to.0, to.1, to.2)) == "moving_block" {
                            step.blocks.push((to, x));
                        }
                    }
                    step.pistons.push(Arm { at: p, sticky: mv.sticky, state: if mv.extending { 2 } else { 0 }, progress: mv.progress, last, attached: vec![] });
                } else {
                    step.pistons.push(Arm { at: p, sticky: mv.sticky, state: if mv.extending { 1 } else { 3 }, progress: mv.progress, last, attached: mv.from.clone() });
                    e.moving.insert(p, mv);
                    sched(e, p, Job::Piston, 1);
                }
            }
            Job::ObserverOn if name(b) == "observer" => {
                step.blocks.push((p, with(b, &[("powered_bit", State::Bool(true))])));
                sched(e, p, Job::ObserverOff, 2);
            }
            Job::ObserverOff if name(b) == "observer" => step.blocks.push((p, with(b, &[("powered_bit", State::Bool(false))]))),
            _ => {}
        }
    }

    // Target blocks whose hit wore off.
    if let Some(t) = TARGETS.lock().as_mut() {
        let now = e.tick;
        t.retain(|&p, &mut (_, until)| {
            let keep = now < until;
            if !keep {
                mark(p);
            }
            keep
        });
    }
    let mut dirty: Vec<Pos> = std::mem::take(&mut *DIRTY.lock());
    dirty.sort_unstable();
    dirty.dedup();
    let mut area: HashSet<Pos> = HashSet::new();
    for &p in &dirty {
        // observers watching this block fire
        for &d in &DIRS {
            let o = add(p, d);
            let ob = w.get(o.0, o.1, o.2);
            if name(ob) == "observer" && add(o, observer_face(ob)) == p && int(ob, "powered_bit") == 0 {
                sched(e, o, Job::ObserverOn, 2);
            }
        }
        area.insert(p);
        for &d in &DIRS {
            let q = add(p, d);
            area.insert(q);
            for &d2 in &DIRS {
                area.insert(add(q, d2));
            }
        }
    }
    let mut area: Vec<Pos> = area.into_iter().collect();
    area.sort_unstable();
    for p in area {
        // Most changes (flowing water, chunk edits) are nowhere near redstone: skip quickly.
        if relevant(w.get(p.0, p.1, p.2)) {
            evaluate(w, e, p, &mut wired, &mut step, fill);
        }
    }
    // two changes to one block in a tick, last one wins
    let mut seen = HashSet::new();
    step.blocks.reverse();
    step.blocks.retain(|(p, _)| seen.insert(*p));
    step.blocks.reverse();
    step
}

/// Piston arm as block entity data so the client draws it (PNX BlockEntityPistonArm).
pub fn piston_data(arm: &Arm) -> HashMap<String, nbtx::Value> {
    let blocks = |list: &[Pos]| {
        let ints: Vec<i32> = list.iter().flat_map(|p| [p.0, p.1, p.2]).collect();
        nbtx::Value::List(nbtx::ValueList::Int(ints))
    };
    let mut t = HashMap::new();
    t.insert("id".into(), nbtx::Value::String("PistonArm".into()));
    t.insert("x".into(), nbtx::Value::Int(arm.at.0));
    t.insert("y".into(), nbtx::Value::Int(arm.at.1));
    t.insert("z".into(), nbtx::Value::Int(arm.at.2));
    t.insert("isMovable".into(), nbtx::Value::Byte((arm.state == 0) as i8));
    t.insert("Progress".into(), nbtx::Value::Float(arm.progress));
    t.insert("LastProgress".into(), nbtx::Value::Float(arm.last));
    t.insert("State".into(), nbtx::Value::Byte(arm.state));
    t.insert("NewState".into(), nbtx::Value::Byte(arm.state));
    t.insert("Sticky".into(), nbtx::Value::Byte(arm.sticky as i8));
    t.insert("AttachedBlocks".into(), blocks(&arm.attached));
    t.insert("BreakBlocks".into(), blocks(&[]));
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(n: &str) -> i32 {
        any_state(&format!("minecraft:{n}")).unwrap()
    }

    /// The engine is global, so tests that tick it can't run in parallel.
    static SERIAL: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    fn run(w: &World, ticks: usize) {
        for _ in 0..ticks {
            let s = tick(w, &|_| None);
            for (p, b) in s.blocks {
                w.set(p.0, p.1, p.2, b);
                mark(p);
            }
        }
    }

    #[test]
    fn daylight_follows_the_sun() {
        assert_eq!(daylight_level(6000, false), 15, "noon");
        assert_eq!(daylight_level(18000, false), 0, "midnight");
        assert_eq!(daylight_level(18000, true), 15, "inverted at midnight");
    }

    #[test]
    fn dust_only_powers_where_it_points() {
        let _serial = SERIAL.lock();
        crate::blocks::init();
        let y = -60;
        let w = World::default();
        // An east-west dust line with a redstone block at its west end; a lamp beside it
        // (north of the middle dust) must stay off, a lamp at its east end must light.
        let wire = |east: &str, west: &str| with(block("redstone_wire"), &[("redstone_east", State::Str(east.into())), ("redstone_west", State::Str(west.into())), ("redstone_north", State::Str("none".into())), ("redstone_south", State::Str("none".into()))]);
        w.set(60, y, 60, block("redstone_block"));
        for x in 61..=63 {
            w.set(x, y, 60, wire("side", "side"));
            mark((x, y, 60));
        }
        w.set(62, y, 59, block("redstone_lamp"));
        w.set(64, y, 60, block("redstone_lamp"));
        mark((62, y, 59));
        mark((64, y, 60));
        run(&w, 4);
        assert_eq!(name(w.get(64, y, 60)), "lit_redstone_lamp", "dust points into this lamp");
        assert_eq!(name(w.get(62, y, 59)), "redstone_lamp", "dust only runs past this one");
    }

    #[test]
    fn repeater_locks_and_detector_rail_powers() {
        let _serial = SERIAL.lock();
        crate::blocks::init();
        let w = World::default();
        let y = -60;
        let facing = ["south", "west", "north", "east"];
        let rep = |d: i32, powered: bool| with(block(if powered { "powered_repeater" } else { "unpowered_repeater" }), &[("minecraft:cardinal_direction", State::Str(facing[d as usize].into()))]);
        // A outputs north; B sits east of A and points west into A's side.
        let a = (40, y, 40);
        w.set(a.0, a.1, a.2, rep(0, false));
        let into_a = (0..4).find(|&d| diode_out(rep(d, true)) == (-1, 0, 0)).unwrap();
        let b = (41, y, 40);
        w.set(b.0, b.1, b.2, rep(into_a, true));
        let behind_b = add(b, neg(diode_out(rep(into_a, true))));
        w.set(behind_b.0, behind_b.1, behind_b.2, block("redstone_block")); // keeps B on
        mark(b);
        run(&w, 6);
        assert_eq!(name(w.get(b.0, b.1, b.2)), "powered_repeater");
        // Power A's input: it's locked, so it stays off.
        let behind_a = add(a, neg(diode_out(rep(0, false))));
        w.set(behind_a.0, behind_a.1, behind_a.2, block("redstone_block"));
        mark(behind_a);
        run(&w, 10);
        assert_eq!(name(w.get(a.0, a.1, a.2)), "unpowered_repeater", "locked");
        // Remove the lock and it switches on.
        w.set(b.0, b.1, b.2, AIR);
        mark(b);
        mark(a);
        run(&w, 10);
        assert_eq!(name(w.get(a.0, a.1, a.2)), "powered_repeater", "unlocked");

        // A detector rail with a cart on it lights the lamp next to it.
        let det = with(block("detector_rail"), &[("rail_data_bit", State::Bool(true))]);
        w.set(60, y, 60, det);
        w.set(61, y, 60, block("redstone_lamp"));
        mark((60, y, 60));
        run(&w, 3);
        assert_eq!(name(w.get(61, y, 60)), "lit_redstone_lamp");
    }

    #[test]
    fn dust_lamp_repeater_and_torch() {
        let _serial = SERIAL.lock();
        crate::blocks::init();
        let w = World::default(); // flat, grass at -61
        let y = -60;
        // redstone block -> 3 dust -> lamp
        w.set(0, y, 0, block("redstone_block"));
        for x in 1..=3 {
            w.set(x, y, 0, block("redstone_wire"));
        }
        w.set(4, y, 0, block("redstone_lamp"));
        mark((0, y, 0));
        run(&w, 3);
        assert_eq!(int(w.get(1, y, 0), "redstone_signal"), 15);
        assert_eq!(int(w.get(3, y, 0), "redstone_signal"), 13);
        assert_eq!(name(w.get(4, y, 0)), "lit_redstone_lamp", "lamp lit by dust");

        // pull the power and everything goes off
        w.set(0, y, 0, AIR);
        mark((0, y, 0));
        run(&w, 3);
        assert_eq!(int(w.get(3, y, 0), "redstone_signal"), 0);
        assert_eq!(name(w.get(4, y, 0)), "redstone_lamp");

        // repeater (input south, output north) switches after its delay
        let rep = with(block("unpowered_repeater"), &[("direction", State::Int(0))]);
        w.set(10, y, 10, block("redstone_block"));
        w.set(10, y, 9, rep);
        w.set(10, y, 8, block("redstone_lamp"));
        mark((10, y, 10));
        run(&w, 1);
        assert_eq!(name(w.get(10, y, 9)), "unpowered_repeater", "not yet");
        run(&w, 4);
        assert_eq!(name(w.get(10, y, 9)), "powered_repeater");
        assert_eq!(name(w.get(10, y, 8)), "lit_redstone_lamp", "repeater output lights the lamp");

        // torch goes out when the block it's on gets powered
        let stone = block("stone");
        w.set(20, y, 20, stone);
        let torch = with(block("redstone_torch"), &[("torch_facing_direction", State::Str("top".into()))]);
        w.set(20, y + 1, 20, torch);
        w.set(21, y, 20, block("lever"));
        let lever = with(w.get(21, y, 20), &[("open_bit", State::Bool(true)), ("lever_direction", State::Str("east".into()))]);
        w.set(21, y, 20, lever);
        mark((21, y, 20));
        run(&w, 4);
        assert_eq!(name(w.get(20, y + 1, 20)), "unlit_redstone_torch", "lever on the block turns the torch off");
    }

    #[test]
    fn piston_pushes_and_sticky_pulls() {
        let _serial = SERIAL.lock();
        crate::blocks::init();
        let w = World::default();
        let y = -60;
        // piston at x=30 pushing east, horizontals store facing reversed (4 = west)
        let p = with(block("sticky_piston"), &[("facing_direction", State::Int(4))]);
        w.set(30, y, 0, p);
        w.set(31, y, 0, block("stone"));
        w.set(29, y, 0, block("redstone_block"));
        mark((29, y, 0));
        run(&w, 2);
        assert_eq!(name(w.get(32, y, 0)), "moving_block", "mid-slide it's a moving block");
        run(&w, 2);
        assert_eq!(name(w.get(32, y, 0)), "stone", "pushed one block");
        assert!(name(w.get(31, y, 0)).contains("piston_arm"));
        w.set(29, y, 0, AIR);
        mark((29, y, 0));
        run(&w, 4);
        assert_eq!(name(w.get(31, y, 0)), "stone", "sticky pulled it back");
    }

    #[test]
    fn pistons_break_buttons_in_the_way_and_on_moved_blocks() {
        let _serial = SERIAL.lock();
        crate::blocks::init();
        let w = World::default();
        let y = -30;
        let p = with(block("piston"), &[("facing_direction", State::Int(4))]); // pushes east
        w.set(110, y, 0, p);
        w.set(111, y, 0, block("stone"));
        // a button on the stone's north side (facing north, held by the block to its south)
        w.set(111, y, -1, with(block("stone_button"), &[("facing_direction", State::Int(2))]));
        // and one right in front of the second piston's face
        let p2 = with(block("piston"), &[("facing_direction", State::Int(4))]);
        w.set(110, y, 5, p2);
        w.set(112, y, 5, block("stone"));
        w.set(111, y, 5, with(block("stone_button"), &[("facing_direction", State::Int(5))]));
        w.set(109, y, 0, block("redstone_block"));
        w.set(109, y, 5, block("redstone_block"));
        mark((109, y, 0));
        mark((109, y, 5));
        run(&w, 4);
        assert_eq!(name(w.get(112, y, 0)), "stone");
        assert_eq!(w.get(111, y, -1), AIR, "button fell off the pushed block");
        assert!(name(w.get(111, y, 5)).contains("piston_arm"), "button in front got crushed");
    }

    #[test]
    fn slime_drags_blocks_and_chests_move() {
        let _serial = SERIAL.lock();
        crate::blocks::init();
        let w = World::default();
        let y = -40; // in the air: slime would also grab the ground under it
        // Piston at x=70 pushing east into slime; stone sits north of the slime, a glazed
        // terracotta south of it (doesn't stick), a chest in front of the slime.
        let p = with(block("piston"), &[("facing_direction", State::Int(4))]);
        w.set(70, y, 0, p);
        w.set(71, y, 0, block("slime"));
        w.set(71, y, -1, block("stone"));
        w.set(71, y, 1, block("white_glazed_terracotta"));
        w.set(72, y, 0, block("chest"));
        w.set(69, y, 0, block("redstone_block"));
        mark((69, y, 0));
        let mut moves = vec![];
        for _ in 0..4 {
            let s = tick(&w, &|_| None);
            moves.extend(s.moves.iter().copied());
            for (q, b) in s.blocks {
                w.set(q.0, q.1, q.2, b);
                mark(q);
            }
        }
        assert_eq!(name(w.get(72, y, 0)), "slime", "slime pushed");
        assert_eq!(name(w.get(72, y, -1)), "stone", "stone stuck to the slime came along");
        assert_eq!(name(w.get(71, y, 1)), "white_glazed_terracotta", "glazed terracotta stays");
        assert_eq!(name(w.get(73, y, 0)), "chest", "chests can be pushed");
        assert_eq!(moves, vec![((72, y, 0), (73, y, 0))], "the chest's contents follow it");
    }

    #[test]
    fn push_limit_is_twelve() {
        let _serial = SERIAL.lock();
        crate::blocks::init();
        let w = World::default();
        let y = -60;
        let p = with(block("piston"), &[("facing_direction", State::Int(4))]);
        w.set(100, y, 0, p);
        for x in 101..=113 {
            w.set(x, y, 0, block("stone")); // 13 blocks
        }
        w.set(99, y, 0, block("redstone_block"));
        mark((99, y, 0));
        run(&w, 2);
        assert_eq!(name(w.get(101, y, 0)), "stone", "13 is too many, nothing moved");
    }

    #[test]
    fn copper_bulb_trapped_chest_target_and_tripwire() {
        let _serial = SERIAL.lock();
        crate::blocks::init();
        let w = World::default();
        let y = -60;
        // Copper bulb: a pulse turns it on, the next pulse off.
        w.set(150, y, 0, block("copper_bulb"));
        for (i, want) in [(0, true), (1, false)] {
            w.set(149, y, 0, block("redstone_block"));
            mark((149, y, 0));
            run(&w, 2);
            assert_eq!(int(w.get(150, y, 0), "lit") == 1, want, "pulse {i}");
            w.set(149, y, 0, AIR);
            mark((149, y, 0));
            run(&w, 2);
        }
        // Trapped chest with a viewer lights a lamp next to it.
        w.set(160, y, 0, block("trapped_chest"));
        w.set(161, y, 0, block("redstone_lamp"));
        set_viewers((160, y, 0), 1);
        run(&w, 2);
        assert_eq!(name(w.get(161, y, 0)), "lit_redstone_lamp", "chest open");
        set_viewers((160, y, 0), 0);
        run(&w, 2);
        assert_eq!(name(w.get(161, y, 0)), "redstone_lamp", "chest closed");
        // A dead-centre hit is level 15, an edge hit is weak.
        hit_target((170, y, 0), (0.5, 0.5, 1.0), 20);
        assert_eq!(TARGETS.lock().as_ref().unwrap()[&(170, y, 0)].0, 15);
        hit_target((171, y, 0), (0.95, 0.5, 1.0), 20);
        assert!(TARGETS.lock().as_ref().unwrap()[&(171, y, 0)].0 <= 2);
        // Two hooks facing each other with string between: attached; stepping in powers them.
        let hook = |d: i32| with(block("tripwire_hook"), &[("direction", State::Int(d))]);
        w.set(180, y, 0, hook(3)); // points east
        w.set(181, y, 0, block("trip_wire"));
        w.set(182, y, 0, block("trip_wire"));
        w.set(183, y, 0, hook(1)); // points west
        mark((181, y, 0));
        run(&w, 2);
        assert_eq!(int(w.get(180, y, 0), "attached_bit"), 1, "attached");
        assert_eq!(int(w.get(180, y, 0), "powered_bit"), 0);
        w.set(182, y, 0, with(w.get(182, y, 0), &[("powered_bit", State::Bool(true))]));
        mark((182, y, 0));
        run(&w, 2);
        assert_eq!(int(w.get(183, y, 0), "powered_bit"), 1, "something on the string");
    }

    #[test]
    fn torches_burn_out() {
        let _serial = SERIAL.lock();
        crate::blocks::init();
        let w = World::default();
        let y = -60;
        // Torch on a block; power the block on and off fast.
        w.set(200, y, 0, block("stone"));
        w.set(200, y + 1, 0, block("redstone_torch"));
        let mut lit_after = vec![];
        for _ in 0..12 {
            w.set(199, y, 0, block("redstone_block"));
            mark((199, y, 0));
            run(&w, 3);
            w.set(199, y, 0, AIR);
            mark((199, y, 0));
            run(&w, 3);
            lit_after.push(name(w.get(200, y + 1, 0)) == "redstone_torch");
        }
        assert!(!*lit_after.last().unwrap(), "burnt out and stays off: {lit_after:?}");
    }
}

//! Redstone, ported from PNX (RedstoneComponent, BlockRedstoneWire, BlockRedstoneDiode,
//! BlockRedstoneTorch, BlockObserver, BlockPistonBase, Level.getRedstonePower).
//! Block changes mark spots dirty and the next tick rechecks everything within 2 blocks.
//! Dust networks are solved in one go, diodes switch after their vanilla delay.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Mutex;

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
    DIRTY.lock().unwrap().push(p);
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Job {
    /// recheck a repeater/torch/comparator
    Diode,
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
    extended: HashSet<Pos>,
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

fn solid(b: i32) -> bool {
    crate::overworld::feature::blocks::is_solid(b) && !is_component(b)
}

fn is_component(b: i32) -> bool {
    let n = name(b);
    n == "redstone_block" || n.contains("repeater") || n.contains("comparator") || n == "observer" || n.contains("piston") || n.starts_with("daylight_detector")
}

/// Output side of a repeater/comparator. `direction` (0 south, 1 west, 2 north, 3 east)
/// is the input side, it outputs the other way.
fn diode_out(b: i32) -> Pos {
    match int(b, "direction") {
        0 => (0, 0, -1),
        1 => (1, 0, 0),
        2 => (0, 0, 1),
        _ => (-1, 0, 0),
    }
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
        "lever" if int(b, "open_bit") == 1 => (15, attached == Some(to)),
        n if n.ends_with("_button") && int(b, "button_pressed_bit") == 1 => (15, attached == Some(to)),
        n if n.ends_with("pressure_plate") => {
            let s = int(b, "redstone_signal").clamp(0, 15) as u8;
            (s, dir == DIRS[0] && s > 0)
        }
        "redstone_torch" => {
            if attached == Some(to) {
                (0, false)
            } else {
                (15, dir == DIRS[1])
            }
        }
        "redstone_wire" => {
            // TODO: dust powers the block under it and every side, vanilla only does the sides it points at
            let s = int(b, "redstone_signal").clamp(0, 15) as u8;
            if dir == DIRS[1] { (0, false) } else { (s, false) }
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
                if set.contains(&n) && power[&n] < v - 1 {
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
    /// (piston, extended, sticky) for the block entity data
    pub pistons: Vec<(Pos, bool, bool)>,
}

fn sched(e: &mut Engine, p: Pos, job: Job, delay: u64) {
    if e.pending.insert(p) {
        e.due.entry(e.tick + delay).or_default().push((p, job));
    }
}

fn rename(b: i32, new: &str) -> i32 {
    match any_state(&format!("minecraft:{new}")) {
        Some(base) => {
            let st: Vec<(&str, State)> = entry(b).map(|e| e.states.iter().map(|(k, v)| (k.as_str(), v.clone())).collect()).unwrap_or_default();
            with(base, &st)
        }
        None => b,
    }
}

/// Should this diode be on now, and which block should it be.
fn diode_target(w: &World, p: Pos, b: i32, c: &mut HashMap<Pos, u8>, fill: &dyn Fn(Pos) -> Option<u8>) -> Option<i32> {
    let n = name(b);
    match n {
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

fn immovable(b: i32) -> bool {
    let n = name(b);
    matches!(n, "obsidian" | "crying_obsidian" | "bedrock" | "barrier" | "end_portal_frame" | "enchanting_table" | "respawn_anchor" | "reinforced_deepslate" | "beacon")
        || n.contains("piston_arm")
        || crate::containers::kind(b).is_some()
        || n.ends_with("_sign")
        || n == "jukebox"
}

/// Pistons break these instead of pushing them (plants, torches, water...).
fn breaks(b: i32) -> bool {
    let n = name(b);
    n.ends_with("water") || n.ends_with("lava") || n.ends_with("torch") || n.contains("flower") || n.ends_with("grass") || n.ends_with("_sapling")
        || n == "fire" || n == "redstone_wire" || n.ends_with("_carpet") || n == "snow_layer" || n == "vine" || n.ends_with("_mushroom") || n == "deadbush"
        || matches!(n, "wheat" | "carrots" | "potatoes" | "beetroot" | "reeds" | "fern" | "tall_grass" | "short_grass")
}

/// Piston block moving (PNX BlocksCalculator). Pushes up to 12, sticky pulls one back.
fn piston(w: &World, p: Pos, b: i32, extend: bool, step: &mut Step) -> bool {
    let face = piston_face(b);
    let sticky = name(b) == "sticky_piston";
    let head = add(p, face);
    if extend {
        let mut line = Vec::new();
        let mut at = head;
        loop {
            let x = w.get(at.0, at.1, at.2);
            if x == AIR || breaks(x) {
                break;
            }
            if immovable(x) || line.len() >= 12 || !(-64..320).contains(&at.1) {
                return false;
            }
            line.push((at, x));
            at = add(at, face);
        }
        let end = w.get(at.0, at.1, at.2);
        if breaks(end) && end != AIR {
            step.blocks.push((at, AIR));
        }
        for &(q, x) in line.iter().rev() {
            step.blocks.push((add(q, face), x));
        }
        let arm = any_state(if sticky { "minecraft:sticky_piston_arm_collision" } else { "minecraft:piston_arm_collision" })
            .map(|a| with(a, &[("facing_direction", State::Int(int(b, "facing_direction")))]))
            .unwrap_or(AIR);
        step.blocks.push((head, arm));
        step.sounds.push((p, "tile.piston.out"));
    } else {
        step.blocks.push((head, AIR));
        if sticky {
            let pulled = add(head, face);
            let x = w.get(pulled.0, pulled.1, pulled.2);
            if x != AIR && !immovable(x) && !breaks(x) {
                step.blocks.push((pulled, AIR));
                step.blocks.push((head, x));
            }
        }
        step.sounds.push((p, "tile.piston.in"));
    }
    step.pistons.push((p, extend, sticky));
    true
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
            if diode_target(w, p, b, &mut e.comparators.clone(), fill).is_some() {
                let delay = 2 * (int(b, "repeater_delay") as u64 + 1);
                sched(e, p, Job::Diode, delay);
            }
        }
        "redstone_torch" | "unlit_redstone_torch" | "unpowered_comparator" | "powered_comparator" => {
            if diode_target(w, p, b, &mut e.comparators.clone(), fill).is_some() {
                sched(e, p, Job::Diode, 2);
            }
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
            let ext = e.extended.contains(&p);
            if on != ext && piston(w, p, b, on, step) {
                if on {
                    e.extended.insert(p);
                } else {
                    e.extended.remove(&p);
                }
            }
        }
        "hopper" => {
            let on = received(w, p, &e.comparators, None) > 0;
            if on != (int(b, "toggle_bit") == 1) {
                step.blocks.push((p, with(b, &[("toggle_bit", State::Bool(on))])));
            }
        }
        n if n == "tnt" || n == "noteblock" || n == "dispenser" || n == "dropper" || n.ends_with("_door") || n.ends_with("trapdoor") || n.ends_with("fence_gate") || n == "wooden_door" => {
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
                "dispenser" | "dropper" if on => step.dispense.push(p),
                "tnt" | "noteblock" | "dispenser" | "dropper" => {}
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

/// One redstone tick. Reads `w`, returns what to change. `fill` gives a container's
/// comparator reading (0-15), None if there isn't one.
pub fn tick(w: &World, fill: &dyn Fn(Pos) -> Option<u8>) -> Step {
    let mut guard = ENGINE.lock().unwrap();
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
                let mut c = e.comparators.clone();
                if let Some(nb) = diode_target(w, p, b, &mut c, fill) {
                    e.comparators = c;
                    step.blocks.push((p, nb));
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

    let dirty: Vec<Pos> = std::mem::take(&mut *DIRTY.lock().unwrap());
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
        evaluate(w, e, p, &mut wired, &mut step, fill);
    }
    // two changes to one block in a tick, last one wins
    let mut seen = HashSet::new();
    step.blocks.reverse();
    step.blocks.retain(|(p, _)| seen.insert(*p));
    step.blocks.reverse();
    step
}

/// Piston arm as block entity data so the client draws it right.
pub fn piston_data(p: Pos, extended: bool, sticky: bool) -> std::collections::HashMap<String, nbtx::Value> {
    let progress = if extended { 1.0 } else { 0.0 };
    let state = if extended { 2 } else { 0 };
    let mut t = std::collections::HashMap::new();
    t.insert("id".into(), nbtx::Value::String("PistonArm".into()));
    t.insert("x".into(), nbtx::Value::Int(p.0));
    t.insert("y".into(), nbtx::Value::Int(p.1));
    t.insert("z".into(), nbtx::Value::Int(p.2));
    t.insert("isMovable".into(), nbtx::Value::Byte(1));
    t.insert("Progress".into(), nbtx::Value::Float(progress));
    t.insert("LastProgress".into(), nbtx::Value::Float(progress));
    t.insert("State".into(), nbtx::Value::Byte(state));
    t.insert("NewState".into(), nbtx::Value::Byte(state));
    t.insert("Sticky".into(), nbtx::Value::Byte(sticky as i8));
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(n: &str) -> i32 {
        any_state(&format!("minecraft:{n}")).unwrap()
    }

    /// The engine is global, so tests that tick it can't run in parallel.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    fn dust_lamp_repeater_and_torch() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
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
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
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
        assert_eq!(name(w.get(32, y, 0)), "stone", "pushed one block");
        assert!(name(w.get(31, y, 0)).contains("piston_arm"));
        w.set(29, y, 0, AIR);
        mark((29, y, 0));
        run(&w, 2);
        assert_eq!(name(w.get(31, y, 0)), "stone", "sticky pulled it back");
    }
}

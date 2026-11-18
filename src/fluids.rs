//! Flowing water and lava, vanilla-ish cellular rules. Liquids fall first, only spread
//! sideways on solid ground, and drain without a source. Lava runs 6x slower than water.

use std::collections::HashSet;
use std::sync::OnceLock;
use parking_lot::Mutex;

use crate::blocks::{State, any_state, with};
use crate::world::{AIR, World};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Water,
    Lava,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Level {
    Source,
    Flow(i32), // 1..=7
    Falling,
}

struct Ids {
    still: [i32; 2],
    flowing: [[i32; 16]; 2],
}

fn ids() -> &'static Ids {
    static IDS: OnceLock<Ids> = OnceLock::new();
    IDS.get_or_init(|| {
        let get = |name: &str| any_state(&format!("minecraft:{name}")).unwrap();
        let depths = |name: &str| std::array::from_fn(|d| with(get(name), &[("liquid_depth", State::Int(d as i32))]));
        Ids {
            still: [with(get("water"), &[("liquid_depth", State::Int(0))]), with(get("lava"), &[("liquid_depth", State::Int(0))])],
            flowing: [depths("flowing_water"), depths("flowing_lava")],
        }
    })
}

impl Kind {
    fn index(self) -> usize {
        self as usize
    }

    /// levels lost per block sideways
    fn step(self) -> i32 {
        match self {
            Kind::Water => 1,
            Kind::Lava => 2,
        }
    }

    fn block(self, level: Level) -> i32 {
        let ids = ids();
        match level {
            Level::Source => ids.still[self.index()],
            Level::Flow(d) => ids.flowing[self.index()][d as usize],
            Level::Falling => ids.flowing[self.index()][8],
        }
    }

    /// still source block, for placing and tests
    pub fn source(self) -> i32 {
        self.block(Level::Source)
    }
}

fn read(id: i32) -> Option<(Kind, Level)> {
    let ids = ids();
    for kind in [Kind::Water, Kind::Lava] {
        let k = kind.index();
        let depth = if let Some(d) = ids.flowing[k].iter().position(|&b| b == id) {
            d as i32
        } else if crate::blocks::entry(id).is_some_and(|e| e.name == crate::blocks::entry(ids.still[k]).unwrap().name) {
            match crate::blocks::state(id, "liquid_depth") {
                Some(State::Int(d)) => *d,
                _ => 0,
            }
        } else {
            continue;
        };
        let level = match depth {
            0 => Level::Source,
            1..=7 => Level::Flow(depth),
            _ => Level::Falling,
        };
        return Some((kind, level));
    }
    None
}

/// Generated lava and water springs waiting for their first update.
static SPRINGS: Mutex<Vec<(i32, i32, i32)>> = Mutex::new(Vec::new());

/// The world generator placed a spring at `p`: it starts flowing on the next fluid tick.
pub fn spring(p: (i32, i32, i32)) {
    SPRINGS.lock().push(p);
}

/// Water or lava, flowing or still.
pub fn is_fluid(id: i32) -> bool {
    read(id).is_some()
}

fn solid(id: i32) -> bool {
    id != AIR && read(id).is_none() && !washable(id)
}

/// Blocks flowing water and lava break and replace: plants, torches, rails, redstone bits
/// (PNX `Block.canBeFlowedInto`).
fn washable(id: i32) -> bool {
    use crate::overworld::feature::blocks::is_plant;
    is_plant(id)
        || crate::blocks::entry(id).is_some_and(|e| {
            let n = e.name.trim_start_matches("minecraft:");
            n.contains("torch") || n.contains("rail") || n.contains("sapling") || n.ends_with("_button") || n.ends_with("carpet")
                || matches!(n, "redstone_wire" | "unpowered_repeater" | "powered_repeater" | "unpowered_comparator" | "powered_comparator" | "fire" | "soul_fire" | "tripwire" | "trip_wire" | "lever")
        })
}

const SIDES: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

/// Which ways a fluid at `p` spreads: toward the nearest spot it can fall from, looking up
/// to 4 blocks (water) or 2 (lava) away; every way if there's none (vanilla getOptimalFlowDirections).
fn flow_dirs(world: &World, p: (i32, i32, i32), kind: Kind) -> [bool; 4] {
    let reach = match kind {
        Kind::Water => 4,
        Kind::Lava => 2,
    };
    let open = |x: i32, y: i32, z: i32| !solid(world.get(x, y, z)) && read(world.get(x, y, z)).is_none_or(|(k, l)| k == kind && l != Level::Source);
    let drop_at = |x: i32, z: i32| !solid(world.get(x, p.1 - 1, z));
    let mut cost = [i32::MAX; 4];
    for (i, (dx, dz)) in SIDES.into_iter().enumerate() {
        let start = (p.0 + dx, p.2 + dz);
        if !open(start.0, p.1, start.1) {
            continue;
        }
        // Breadth-first over open blocks on this level.
        let mut seen = std::collections::HashSet::from([start]);
        let mut edge = vec![start];
        for d in 0..reach {
            if edge.iter().any(|&(x, z)| drop_at(x, z)) {
                cost[i] = d;
                break;
            }
            let mut next = Vec::new();
            for (x, z) in edge {
                for (ex, ez) in SIDES {
                    let q = (x + ex, z + ez);
                    if q != (p.0, p.2) && open(q.0, p.1, q.1) && seen.insert(q) {
                        next.push(q);
                    }
                }
            }
            edge = next;
        }
    }
    let best = *cost.iter().min().unwrap();
    if best == i32::MAX {
        return [true; 4];
    }
    cost.map(|c| c == best)
}

fn next_state(world: &World, (x, y, z): (i32, i32, i32)) -> Option<i32> {
    let current = world.get(x, y, z);
    let here = read(current);
    // Lava pouring down onto water makes stone.
    if here.is_some_and(|(k, _)| k == Kind::Water) && read(world.get(x, y + 1, z)).is_some_and(|(k, _)| k == Kind::Lava) {
        return crate::blocks::any_state("minecraft:stone");
    }
    // Lava touching water hardens: a source into obsidian, flowing lava into cobblestone.
    if let Some((Kind::Lava, level)) = here {
        let wet = [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, 1, 0)]
            .iter()
            .any(|(dx, dy, dz)| read(world.get(x + dx, y + dy, z + dz)).is_some_and(|(k, _)| k == Kind::Water));
        if wet {
            let name = if level == Level::Source { "minecraft:obsidian" } else { "minecraft:cobblestone" };
            return crate::blocks::any_state(name);
        }
    }
    if here.is_some_and(|(_, l)| l == Level::Source) || (current != AIR && here.is_none() && !washable(current)) {
        return None; // sources stay; solid blocks are never replaced
    }
    let kinds: &[Kind] = match here {
        Some((Kind::Water, _)) => &[Kind::Water],
        Some((Kind::Lava, _)) => &[Kind::Lava],
        None => &[Kind::Water, Kind::Lava],
    };
    let sides = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    for &kind in kinds {
        let is = |b: i32| read(b).filter(|(k, _)| *k == kind).map(|(_, l)| l);
        let want = if is(world.get(x, y + 1, z)).is_some() {
            Some(Level::Falling)
        } else {
            let mut sources = 0;
            let mut best: Option<i32> = None;
            for (dx, dz) in sides {
                let (nx, nz) = (x + dx, z + dz);
                let Some(level) = is(world.get(nx, y, nz)) else { continue };
                if level == Level::Source {
                    sources += 1;
                }
                // The neighbour only flows this way if it's one of its best ways to a drop.
                let back = SIDES.iter().position(|&d| d == (-dx, -dz)).unwrap();
                if !flow_dirs(world, (nx, y, nz), kind)[back] {
                    continue;
                }
                // only spread sideways once it can't fall any more
                let below = world.get(nx, y - 1, nz);
                let resting = solid(below) || is(below) == Some(Level::Source);
                let value = match level {
                    Level::Source => 0,
                    Level::Flow(d) if resting => d,
                    Level::Falling if resting => 0,
                    _ => continue,
                };
                best = Some(best.map_or(value, |b: i32| b.min(value)));
            }
            let below = world.get(x, y - 1, z);
            if kind == Kind::Water && sources >= 2 && (solid(below) || is(below) == Some(Level::Source)) {
                Some(Level::Source) // infinite water
            } else {
                best.map(|b| b + kind.step()).filter(|&d| d <= 7).map(Level::Flow)
            }
        };
        let new = match want {
            Some(level) => kind.block(level),
            None if here.is_some() => AIR, // cut off from its source: drain
            None => continue,
        };
        return (new != current).then_some(new);
    }
    None
}

/// Positions waiting to be recomputed (server-wide).
#[derive(Default)]
pub struct Fluids {
    pending: Mutex<HashSet<(i32, i32, i32)>>,
}

/// Per-tick cap so a big flood can't stall the server.
const PER_TICK: usize = 4096;

impl Fluids {
    /// Queue `p` and its neighbours.
    pub fn notify(&self, (x, y, z): (i32, i32, i32)) {
        let mut p = self.pending.lock();
        for (dx, dy, dz) in [(0, 0, 0), (1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
            p.insert((x + dx, y + dy, z + dz));
        }
    }

    /// One fluid step, lava only moves on `lava_turn`. The caller applies the changes,
    /// which queues the next step.
    pub fn tick(&self, world: &World, lava_turn: bool) -> Vec<((i32, i32, i32), i32)> {
        for p in std::mem::take(&mut *SPRINGS.lock()) {
            self.notify(p);
        }
        let batch: Vec<_> = {
            let mut p = self.pending.lock();
            let take: Vec<_> = p.iter().take(PER_TICK).copied().collect();
            for q in &take {
                p.remove(q);
            }
            take
        };
        let mut changes = Vec::new();
        let mut later = Vec::new();
        for pos in batch {
            if let Some(new) = next_state(world, pos) {
                let lava = read(new).is_some_and(|(k, _)| k == Kind::Lava)
                    || read(world.get(pos.0, pos.1, pos.2)).is_some_and(|(k, _)| k == Kind::Lava);
                if lava && !lava_turn {
                    later.push(pos);
                } else {
                    changes.push((pos, new));
                }
            }
        }
        if !later.is_empty() {
            self.pending.lock().extend(later);
        }
        changes
    }

    #[cfg(test)]
    pub fn is_idle(&self) -> bool {
        self.pending.lock().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ticks until it settles (or hits the cap), like the server would.
    fn run(world: &World, fluids: &Fluids) {
        for turn in 0..400 {
            let changes = fluids.tick(world, turn % 6 == 0);
            for &((x, y, z), b) in &changes {
                world.set(x, y, z, b);
                fluids.notify((x, y, z));
            }
            if changes.is_empty() && fluids.is_idle() {
                return;
            }
        }
        panic!("fluids never settled");
    }

    #[test]
    fn water_spreads_seven_blocks_then_drains() {
        let w = World::default();
        let f = Fluids::default();
        let src = Kind::Water.source();
        w.set(0, -60, 0, src);
        f.notify((0, -60, 0));
        run(&w, &f);
        assert_eq!(read(w.get(1, -60, 0)), Some((Kind::Water, Level::Flow(1))));
        assert_eq!(read(w.get(7, -60, 0)), Some((Kind::Water, Level::Flow(7))));
        assert_eq!(w.get(8, -60, 0), AIR, "spreads 7 blocks");
        assert_eq!(read(w.get(3, -60, 4)), Some((Kind::Water, Level::Flow(7))), "diamond shape");
        // remove the source and it should all drain
        w.set(0, -60, 0, AIR);
        f.notify((0, -60, 0));
        run(&w, &f);
        for x in -8..=8 {
            assert_eq!(w.get(x, -60, 0), AIR, "drained at x={x}");
        }
    }

    #[test]
    fn water_falls_into_holes_and_lava_spreads_less() {
        let w = World::default();
        let f = Fluids::default();
        // 2-deep hole next to the source, water should fall in
        w.set(2, -61, 0, AIR);
        w.set(2, -62, 0, AIR);
        w.set(0, -60, 0, Kind::Water.source());
        f.notify((0, -60, 0));
        run(&w, &f);
        assert_eq!(read(w.get(2, -61, 0)).map(|r| r.0), Some(Kind::Water));
        assert_eq!(read(w.get(2, -62, 0)).map(|r| r.0), Some(Kind::Water), "fell to the bottom");

        let w = World::default();
        let f = Fluids::default();
        w.set(0, -60, 0, Kind::Lava.source());
        f.notify((0, -60, 0));
        run(&w, &f);
        assert_eq!(read(w.get(3, -60, 0)).map(|r| r.0), Some(Kind::Lava));
        assert_eq!(w.get(4, -60, 0), AIR, "lava spreads 3 blocks (2 levels per block)");
    }

    #[test]
    fn two_sources_make_infinite_water() {
        let w = World::default();
        let f = Fluids::default();
        w.set(0, -60, 0, Kind::Water.source());
        w.set(2, -60, 0, Kind::Water.source());
        f.notify((0, -60, 0));
        f.notify((2, -60, 0));
        run(&w, &f);
        assert_eq!(read(w.get(1, -60, 0)), Some((Kind::Water, Level::Source)));
    }

    #[test]
    fn water_heads_for_holes_washes_plants_and_hardens_lava() {
        // A hole 2 blocks east: the water goes that way only, not west.
        let w = World::default();
        let f = Fluids::default();
        w.set(2, -61, 0, AIR);
        w.set(0, -60, 0, Kind::Water.source());
        f.notify((0, -60, 0));
        run(&w, &f);
        assert!(read(w.get(1, -60, 0)).is_some(), "flows toward the hole");
        assert_eq!(w.get(-1, -60, 0), AIR, "not away from it");

        // Tall grass gets washed away.
        let w = World::default();
        let f = Fluids::default();
        let grass = crate::blocks::any_state("minecraft:short_grass").unwrap();
        w.set(1, -60, 0, grass);
        w.set(0, -60, 0, Kind::Water.source());
        f.notify((0, -60, 0));
        run(&w, &f);
        assert!(read(w.get(1, -60, 0)).is_some(), "water replaced the grass");

        // Lava source next to water: obsidian.
        let w = World::default();
        let f = Fluids::default();
        w.set(5, -60, 5, Kind::Lava.source());
        w.set(6, -60, 5, Kind::Water.source());
        f.notify((5, -60, 5));
        run(&w, &f);
        assert_eq!(crate::blocks::entry(w.get(5, -60, 5)).map(|e| e.name.as_str()), Some("minecraft:obsidian"));
    }
}

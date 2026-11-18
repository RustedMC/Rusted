//! Flowing water and lava, vanilla-ish cellular rules. Liquids fall first, only spread
//! sideways on solid ground, and drain without a source. Lava runs 6x slower than water.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

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

fn solid(id: i32) -> bool {
    id != AIR && read(id).is_none()
}

fn next_state(world: &World, (x, y, z): (i32, i32, i32)) -> Option<i32> {
    let current = world.get(x, y, z);
    let here = read(current);
    if here.is_some_and(|(_, l)| l == Level::Source) || (current != AIR && here.is_none()) {
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
        let mut p = self.pending.lock().unwrap();
        for (dx, dy, dz) in [(0, 0, 0), (1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
            p.insert((x + dx, y + dy, z + dz));
        }
    }

    /// One fluid step, lava only moves on `lava_turn`. The caller applies the changes,
    /// which queues the next step.
    pub fn tick(&self, world: &World, lava_turn: bool) -> Vec<((i32, i32, i32), i32)> {
        let batch: Vec<_> = {
            let mut p = self.pending.lock().unwrap();
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
            self.pending.lock().unwrap().extend(later);
        }
        changes
    }

    #[cfg(test)]
    pub fn is_idle(&self) -> bool {
        self.pending.lock().unwrap().is_empty()
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
}

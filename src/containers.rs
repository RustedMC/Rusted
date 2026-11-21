//! Chest/barrel contents by block position, saved to containers.dat.
//! Format: "RSTC", version u32, count u32, then x y z (i32) + `CHEST_SLOTS` slots each.

use std::collections::HashMap;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::inventory::{CHEST_SLOTS, Stack, decode_slots, encode_slots};

#[derive(Default)]
pub struct Containers {
    map: Mutex<HashMap<(i32, i32, i32), Vec<Option<Stack>>>>,
    /// Furnace burn state and when each was last closed (they catch up on open).
    // TODO: save burn state, a restart currently pauses a lit furnace
    furnaces: Mutex<HashMap<(i32, i32, i32), (crate::recipes::Furnace, std::time::Instant)>>,
    /// Open counts per container (lid animation, live furnaces).
    viewers: Mutex<HashMap<(i32, i32, i32), u32>>,
    /// Brewing stand fuel/progress. TODO: not saved yet, a restart resets the brew.
    brewing: Mutex<HashMap<(i32, i32, i32), Brew>>,
    /// All known hoppers, ticked every 8 game ticks.
    pub hoppers: Mutex<std::collections::HashSet<(i32, i32, i32)>>,
    dirty: AtomicBool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Chest,
    Barrel,
    Furnace,
    Smoker,
    BlastFurnace,
    Workbench,
    Stonecutter,
    Hopper,
    Brewing,
    Anvil,
    Grindstone,
    Enchanting,
    Beacon,
    Loom,
    Crafter,
    Shulker,
    Dispenser,
    Dropper,
    Smithing,
    /// Each player's own 27 slots, kept at `ender_store(name)`.
    EnderChest,
}

/// Where a player's ender chest items live in the container store (below every real block,
/// like chest minecarts, keyed by name so they last across restarts).
pub fn ender_store(name: &str) -> (i32, i32, i32) {
    let h = name.to_lowercase().bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    (h as i32, -70_000, (h >> 32) as i32)
}

/// Slots shown when opened. Storage is always CHEST_SLOTS wide.
pub fn open_slots(k: Kind) -> usize {
    match k {
        Kind::Furnace | Kind::Smoker | Kind::BlastFurnace => 3,
        Kind::Hopper | Kind::Brewing => 5,
        Kind::Dispenser | Kind::Dropper => 9,
        _ => CHEST_SLOTS,
    }
}

/// Screens that keep their slots in the player's UI, nothing stored at the block.
pub fn ui_only(k: Kind) -> bool {
    matches!(k, Kind::Workbench | Kind::Stonecutter | Kind::Anvil | Kind::Grindstone | Kind::Enchanting | Kind::Smithing | Kind::Beacon | Kind::Loom)
}

/// from PNX BlockEntityBrewingStand
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Brew {
    pub fuel: i32,
    /// 0 = not brewing
    pub ticks: i32,
}

/// 400 ticks per brew, blaze powder lasts 20 brews.
pub const BREW_TICKS: i32 = 400;
pub const BREW_FUEL: i32 = 20;

fn item_name(s: &Option<Stack>) -> &'static str {
    s.as_ref().and_then(|s| crate::items::name_of(s.item.id)).unwrap_or("")
}

/// Slots: 0 ingredient, 1-3 potions, 4 fuel. True when a brew finishes.
pub fn brew(state: &mut Brew, slots: &mut [Option<Stack>], ticks: i32) -> bool {
    if slots.len() < 5 {
        return false;
    }
    let recipes = crate::recipes::get();
    let reagent = slots[0].as_ref().map(|s| s.item.id);
    let can = |slots: &[Option<Stack>]| {
        reagent.is_some_and(|r| (1..4).any(|i| slots[i].as_ref().is_some_and(|p| recipes.brewed(&p.item, r).is_some())))
    };
    if state.fuel <= 0 && item_name(&slots[4]) == "minecraft:blaze_powder" && can(slots) {
        state.fuel = BREW_FUEL;
        let f = slots[4].as_mut().unwrap();
        f.count -= 1;
        if f.count <= 0 {
            slots[4] = None;
        }
    }
    if !can(slots) || state.fuel <= 0 {
        state.ticks = 0;
        return false;
    }
    if state.ticks == 0 {
        state.ticks = BREW_TICKS;
    }
    state.ticks -= ticks;
    if state.ticks > 0 {
        return false;
    }
    state.ticks = 0;
    let r = reagent.unwrap();
    for slot in slots.iter_mut().take(4).skip(1) {
        if let Some(p) = slot.as_mut()
            && let Some(out) = recipes.brewed(&p.item, r)
        {
            p.item = out;
        }
    }
    let ing = slots[0].as_mut().unwrap();
    ing.count -= 1;
    if ing.count <= 0 {
        slots[0] = None;
    }
    state.fuel -= 1;
    true
}

pub fn kind(block: i32) -> Option<Kind> {
    match crate::blocks::entry(block)?.name.as_str() {
        "minecraft:chest" | "minecraft:trapped_chest" => Some(Kind::Chest),
        "minecraft:barrel" => Some(Kind::Barrel),
        "minecraft:ender_chest" => Some(Kind::EnderChest),
        "minecraft:furnace" | "minecraft:lit_furnace" => Some(Kind::Furnace),
        "minecraft:smoker" | "minecraft:lit_smoker" => Some(Kind::Smoker),
        "minecraft:blast_furnace" | "minecraft:lit_blast_furnace" => Some(Kind::BlastFurnace),
        "minecraft:crafting_table" => Some(Kind::Workbench),
        "minecraft:stonecutter_block" => Some(Kind::Stonecutter),
        "minecraft:hopper" => Some(Kind::Hopper),
        "minecraft:brewing_stand" => Some(Kind::Brewing),
        "minecraft:anvil" | "minecraft:chipped_anvil" | "minecraft:damaged_anvil" => Some(Kind::Anvil),
        "minecraft:grindstone" => Some(Kind::Grindstone),
        "minecraft:enchanting_table" => Some(Kind::Enchanting),
        "minecraft:dispenser" => Some(Kind::Dispenser),
        "minecraft:dropper" => Some(Kind::Dropper),
        "minecraft:smithing_table" => Some(Kind::Smithing),
        "minecraft:beacon" => Some(Kind::Beacon),
        "minecraft:loom" => Some(Kind::Loom),
        "minecraft:crafter" => Some(Kind::Crafter),
        n if n.ends_with("shulker_box") => Some(Kind::Shulker),
        _ => None,
    }
}

const MAGIC: &[u8; 4] = b"RSTC";
const VERSION: u32 = 1;

/// The other half of a double chest: same block beside it, facing the same way
/// (same rule as PNX BlockChest.findPair and the client).
pub fn chest_pair(world: &crate::world::World, (x, y, z): (i32, i32, i32)) -> Option<(i32, i32, i32)> {
    let b = world.get(x, y, z);
    let e = crate::blocks::entry(b)?;
    if !matches!(e.name.as_str(), "minecraft:chest" | "minecraft:trapped_chest") {
        return None;
    }
    let facing = crate::blocks::state(b, "minecraft:cardinal_direction");
    let sides: &[(i32, i32)] = match facing {
        Some(crate::blocks::State::Str(f)) if f == "north" || f == "south" => &[(1, 0), (-1, 0)],
        _ => &[(0, 1), (0, -1)],
    };
    sides.iter().map(|&(dx, dz)| (x + dx, y, z + dz)).find(|&(px, py, pz)| {
        let o = world.get(px, py, pz);
        crate::blocks::entry(o).is_some_and(|oe| oe.name == e.name) && crate::blocks::state(o, "minecraft:cardinal_direction") == facing
    })
}

#[cfg(test)]
pub fn is_container(block: i32) -> bool {
    kind(block).is_some_and(|k| !ui_only(k))
}

impl Containers {
    /// Contents at `pos`. Furnaces smelt the time since they were closed first.
    pub fn get(&self, pos: (i32, i32, i32)) -> Vec<Option<Stack>> {
        let mut slots = self.map.lock().get(&pos).cloned().unwrap_or_else(|| vec![None; CHEST_SLOTS]);
        if let Some((f, since)) = self.furnaces.lock().get_mut(&pos) {
            f.catch_up(&mut slots, since.elapsed().as_secs_f32());
            *since = std::time::Instant::now();
        }
        slots
    }

    pub fn close(&self, world: &crate::world::World, pos: (i32, i32, i32), slots: Vec<Option<Stack>>) {
        let oven = match kind(world.get(pos.0, pos.1, pos.2)) {
            Some(Kind::Furnace) => Some(crate::recipes::Oven::Furnace),
            Some(Kind::Smoker) => Some(crate::recipes::Oven::Smoker),
            Some(Kind::BlastFurnace) => Some(crate::recipes::Oven::Blast),
            _ => None,
        };
        match kind(world.get(pos.0, pos.1, pos.2)) {
            Some(Kind::Brewing) => self.track_brewing(pos),
            Some(Kind::Hopper) => {
                self.hoppers.lock().insert(pos);
            }
            _ => {}
        }
        if let Some(oven) = oven {
            self.put_furnace(pos, slots, oven);
        } else {
            self.put(pos, slots);
        }
    }

    /// One second of smelting for an open furnace. Returns (cook, lit, lit duration) ticks.
    /// +1 on open, -1 on close. Returns how many have it open now.
    pub fn viewers(&self, pos: (i32, i32, i32), delta: i32) -> u32 {
        let mut v = self.viewers.lock();
        let n = v.entry(pos).or_insert(0);
        *n = (*n as i32 + delta).max(0) as u32;
        let left = *n;
        if left == 0 {
            v.remove(&pos);
        }
        crate::redstone::set_viewers(pos, left);
        left
    }

    /// Smelts closed furnaces for `secs`, returns positions and whether each is lit.
    pub fn tick_closed(&self, secs: f32) -> Vec<((i32, i32, i32), bool)> {
        let viewers = self.viewers.lock().clone();
        let mut f = self.furnaces.lock();
        let mut map = self.map.lock();
        let mut out = Vec::new();
        for (pos, (state, since)) in f.iter_mut() {
            if viewers.contains_key(pos) {
                continue;
            }
            *since = std::time::Instant::now();
            if let Some(slots) = map.get_mut(pos) {
                let before: Vec<Option<i8>> = slots.iter().take(3).map(|s| s.as_ref().map(|s| s.count)).collect();
                state.catch_up(slots, secs);
                if slots.iter().take(3).map(|s| s.as_ref().map(|s| s.count)).collect::<Vec<_>>() != before {
                    self.dirty.store(true, Ordering::Relaxed);
                }
            } else {
                state.burn_left = (state.burn_left - secs).max(0.0);
            }
            out.push((*pos, state.burn_left > 0.0));
        }
        out
    }

    pub fn tick_open(&self, oven: crate::recipes::Oven, pos: (i32, i32, i32), slots: &mut [Option<Stack>], secs: f32) -> (i32, i32, i32) {
        let mut f = self.furnaces.lock();
        let (state, since) = f.entry(pos).or_insert_with(|| (crate::recipes::Furnace { kind: oven, ..Default::default() }, std::time::Instant::now()));
        state.kind = oven;
        state.catch_up(slots, secs);
        *since = std::time::Instant::now();
        let t = |s: f32| (s * 20.0) as i32;
        (t(state.progress), t(state.burn_left), t(state.burn_total))
    }

    /// Furnace closed, start smelting from now.
    pub fn put_furnace(&self, pos: (i32, i32, i32), slots: Vec<Option<Stack>>, oven: crate::recipes::Oven) {
        let mut f = self.furnaces.lock();
        let e = f.entry(pos).or_insert_with(|| (Default::default(), std::time::Instant::now()));
        e.0.kind = oven;
        e.1 = std::time::Instant::now();
        drop(f);
        self.put(pos, slots);
    }

    pub fn put(&self, pos: (i32, i32, i32), mut slots: Vec<Option<Stack>>) {
        // furnaces only use 3 slots, but the save format is always CHEST_SLOTS wide
        slots.resize(CHEST_SLOTS, None);
        let mut map = self.map.lock();
        if slots.iter().all(Option::is_none) {
            map.remove(&pos);
        } else {
            map.insert(pos, slots);
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// One second for an open brewing stand. Returns (brew left, fuel left, fuel total, finished).
    pub fn tick_open_brew(&self, pos: (i32, i32, i32), slots: &mut [Option<Stack>]) -> ((i32, i32, i32), bool) {
        let mut b = self.brewing.lock();
        let st = b.entry(pos).or_default();
        let done = brew(st, slots, 20);
        ((st.ticks, st.fuel, BREW_FUEL), done)
    }

    /// Ticks closed brewing stands, returns the ones that finished.
    pub fn tick_closed_brew(&self) -> Vec<(i32, i32, i32)> {
        let viewers = self.viewers.lock().clone();
        let mut b = self.brewing.lock();
        let mut map = self.map.lock();
        let mut done = Vec::new();
        for (pos, st) in b.iter_mut() {
            if viewers.contains_key(pos) {
                continue;
            }
            if let Some(slots) = map.get_mut(pos)
                && brew(st, slots, 20)
            {
                done.push(*pos);
                self.dirty.store(true, Ordering::Relaxed);
            }
        }
        done
    }

    pub fn track_brewing(&self, pos: (i32, i32, i32)) {
        self.brewing.lock().entry(pos).or_default();
    }

    /// Moves one item between containers (PNX hopper transfer), true if anything moved.
    pub(crate) fn move_one(&self, from: (i32, i32, i32), from_slots: std::ops::Range<usize>, to: (i32, i32, i32), to_slots: std::ops::Range<usize>) -> bool {
        let mut map = self.map.lock();
        let src = map.get(&from).cloned().unwrap_or_else(|| vec![None; CHEST_SLOTS]);
        let mut dst = map.get(&to).cloned().unwrap_or_else(|| vec![None; CHEST_SLOTS]);
        let mut src = src;
        for i in from_slots {
            let Some(s) = src[i].clone() else { continue };
            let max = crate::items::max_stack(s.item.id);
            // stack onto a matching item first, else first empty slot
            let target = to_slots.clone().find(|&j| dst[j].as_ref().is_some_and(|d| d.item.id == s.item.id && d.item.aux_value == s.item.aux_value && d.item.user_data_buffer == s.item.user_data_buffer && d.count < max))
                .or_else(|| to_slots.clone().find(|&j| dst[j].is_none()));
            let Some(j) = target else { continue };
            match dst[j].as_mut() {
                Some(d) => d.count += 1,
                None => dst[j] = Some(Stack { count: 1, ..s.clone() }),
            }
            let st = src[i].as_mut().unwrap();
            st.count -= 1;
            if st.count <= 0 {
                src[i] = None;
            }
            map.insert(from, src);
            map.insert(to, dst);
            self.dirty.store(true, Ordering::Relaxed);
            return true;
        }
        false
    }

    /// Takes one item from a random filled dispenser/dropper slot, like PNX.
    pub fn take_one(&self, pos: (i32, i32, i32), roll: u32) -> Option<Stack> {
        let mut map = self.map.lock();
        let slots = map.get_mut(&pos)?;
        let filled: Vec<usize> = (0..9).filter(|&i| slots[i].is_some()).collect();
        let i = *filled.get(roll as usize % filled.len().max(1))?;
        let st = slots[i].as_mut()?;
        let one = Stack { count: 1, ..st.clone() };
        st.count -= 1;
        if st.count <= 0 {
            slots[i] = None;
        }
        self.dirty.store(true, Ordering::Relaxed);
        Some(one)
    }

    /// Drops one item into the container at `pos`, false if it's full.
    pub fn put_one(&self, world: &crate::world::World, pos: (i32, i32, i32), item: &Stack) -> bool {
        let Some(k) = kind(world.get(pos.0, pos.1, pos.2)).filter(|k| !ui_only(*k)) else { return false };
        let n = open_slots(k);
        let mut map = self.map.lock();
        let slots = map.entry(pos).or_insert_with(|| vec![None; CHEST_SLOTS]);
        let max = crate::items::max_stack(item.item.id);
        let at = (0..n)
            .find(|&j| slots[j].as_ref().is_some_and(|d| d.item.id == item.item.id && d.item.aux_value == item.item.aux_value && d.item.user_data_buffer == item.item.user_data_buffer && d.count < max))
            .or_else(|| (0..n).find(|&j| slots[j].is_none()));
        let Some(j) = at else { return false };
        match slots[j].as_mut() {
            Some(d) => d.count += 1,
            None => slots[j] = Some(Stack { count: 1, ..item.clone() }),
        }
        self.dirty.store(true, Ordering::Relaxed);
        true
    }

    /// Hopper tick (PNX BlockEntityHopper): push one item forward, pull one from above.
    /// Skips containers someone has open since their slots live in that player's UI.
    pub fn tick_hoppers(&self, world: &crate::world::World) {
        let hoppers: Vec<_> = self.hoppers.lock().iter().copied().collect();
        let viewers = self.viewers.lock().clone();
        let mut gone = Vec::new();
        for h in hoppers {
            let b = world.get(h.0, h.1, h.2);
            if kind(b) != Some(Kind::Hopper) {
                gone.push(h);
                continue;
            }
            if viewers.contains_key(&h) || matches!(crate::blocks::state(b, "toggle_bit"), Some(crate::blocks::State::Bool(true))) {
                continue;
            }
            let face = match crate::blocks::state(b, "facing_direction") {
                Some(crate::blocks::State::Int(f)) => *f,
                _ => 0,
            };
            let (dx, dy, dz) = crate::placement::face_offset(face as u8);
            let target = (h.0 + dx, h.1 + dy, h.2 + dz);
            let tk = kind(world.get(target.0, target.1, target.2));
            if !viewers.contains_key(&target) {
                // furnaces: top goes into input, sides into fuel
                let to = match tk {
                    Some(Kind::Furnace | Kind::Smoker | Kind::BlastFurnace) => Some(if face == 0 { 0..1 } else { 1..2 }),
                    Some(Kind::Brewing) => Some(if face == 0 { 0..1 } else { 1..5 }),
                    Some(Kind::Hopper) => Some(0..5),
                    Some(Kind::Chest | Kind::Barrel) => Some(0..CHEST_SLOTS),
                    _ => None,
                };
                if let Some(to) = to {
                    self.move_one(h, 0..5, target, to);
                }
            }
            let above = (h.0, h.1 + 1, h.2);
            if !viewers.contains_key(&above) {
                let from = match kind(world.get(above.0, above.1, above.2)) {
                    Some(Kind::Furnace | Kind::Smoker | Kind::BlastFurnace) => Some(2..3),
                    Some(Kind::Brewing) => Some(1..4),
                    Some(Kind::Hopper) | None => None,
                    Some(Kind::Chest | Kind::Barrel) => Some(0..CHEST_SLOTS),
                    _ => None,
                };
                if let Some(from) = from {
                    self.move_one(above, from, h, 0..5);
                }
            }
        }
        let mut hs = self.hoppers.lock();
        for g in gone {
            hs.remove(&g);
        }
    }

    /// Returns how many didn't fit.
    pub fn hopper_insert(&self, h: (i32, i32, i32), item: &crate::inventory::Stack) -> i8 {
        let mut map = self.map.lock();
        let slots = map.entry(h).or_insert_with(|| vec![None; CHEST_SLOTS]);
        let max = crate::items::max_stack(item.item.id);
        let mut left = item.count;
        for s in slots.iter_mut().take(5) {
            if left == 0 {
                break;
            }
            match s {
                Some(d) if d.item.id == item.item.id && d.item.aux_value == item.item.aux_value && d.item.user_data_buffer == item.item.user_data_buffer && d.count < max => {
                    let n = left.min(max - d.count);
                    d.count += n;
                    left -= n;
                }
                None => {
                    *s = Some(Stack { count: left, ..item.clone() });
                    left = 0;
                }
                _ => {}
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
        left
    }

    /// Comparator reading, PNX formula 1 + fullness * 14 (0 when empty).
    pub fn comparator(&self, world: &crate::world::World, pos: (i32, i32, i32)) -> Option<u8> {
        // A lectern reads how far through its book it's open (vanilla: 1 + 14 * page / last page).
        if crate::lectern::is_lectern(world.get(pos.0, pos.1, pos.2)) {
            let book = self.map.lock().get(&pos).and_then(|s| s.first().cloned().flatten());
            let Some(book) = book else { return Some(0) };
            let last = (crate::lectern::pages(&book) - 1).max(0);
            let page = crate::lectern::pages_open().lock().get(&pos).copied().unwrap_or(0).clamp(0, last);
            return Some(if last == 0 { 15 } else { 1 + (14 * page / last) as u8 });
        }
        let k = kind(world.get(pos.0, pos.1, pos.2)).filter(|k| !ui_only(*k))?;
        let n = open_slots(k);
        let map = self.map.lock();
        let Some(slots) = map.get(&pos) else { return Some(0) };
        let full: f32 = slots.iter().take(n).flatten().map(|s| s.count as f32 / crate::items::max_stack(s.item.id) as f32).sum();
        Some(if full <= 0.0 { 0 } else { 1 + (full / n as f32 * 14.0) as u8 })
    }

    /// Container got broken, hand back its items to drop.
    pub fn remove(&self, pos: (i32, i32, i32)) -> Vec<Option<Stack>> {
        self.brewing.lock().remove(&pos);
        self.hoppers.lock().remove(&pos);
        let furnace = self.furnaces.lock().remove(&pos);
        let mut taken = self.map.lock().remove(&pos);
        // let it finish smelting what it had time for
        if let (Some(slots), Some((mut f, since))) = (taken.as_mut(), furnace) {
            f.catch_up(slots, since.elapsed().as_secs_f32());
        }
        if taken.is_some() {
            self.dirty.store(true, Ordering::Relaxed);
        }
        taken.unwrap_or_default()
    }

    pub fn save_if_dirty(&self, path: &std::path::Path) -> std::io::Result<bool> {
        if !self.dirty.swap(false, Ordering::Relaxed) {
            return Ok(false);
        }
        let mut b = Vec::new();
        {
            let map = self.map.lock();
            b.extend(MAGIC);
            b.extend(VERSION.to_le_bytes());
            b.extend((map.len() as u32).to_le_bytes());
            for (&(x, y, z), slots) in map.iter() {
                for v in [x, y, z] {
                    b.extend(v.to_le_bytes());
                }
                b.extend(encode_slots(slots));
            }
        }
        let tmp = path.with_extension("tmp");
        let result = std::fs::write(&tmp, &b).and_then(|()| std::fs::rename(&tmp, path));
        if result.is_err() {
            self.dirty.store(true, Ordering::Relaxed);
        }
        result.map(|()| true)
    }

    /// Missing file = no containers yet. A broken file is an error.
    pub fn load(path: &std::path::Path) -> std::io::Result<Self> {
        let b = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e),
        };
        let bad = || std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{}: damaged container file", path.display()));
        if b.len() < 12 || &b[..4] != MAGIC || u32::from_le_bytes(b[4..8].try_into().unwrap()) != VERSION {
            return Err(bad());
        }
        let count = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
        let mut map = HashMap::with_capacity(count);
        let mut i = 12;
        for _ in 0..count {
            let pos = b.get(i..i + 12).ok_or_else(bad)?;
            let int = |k: usize| i32::from_le_bytes(pos[k..k + 4].try_into().unwrap());
            let (slots, used) = decode_slots(&b[i + 12..], CHEST_SLOTS, CHEST_SLOTS).ok_or_else(bad)?;
            map.insert((int(0), int(4), int(8)), slots);
            i += 12 + used;
        }
        if i != b.len() {
            return Err(bad());
        }
        // furnaces with stuff in them pick up smelting from load time
        let now = std::time::Instant::now();
        let furnaces = map.keys().map(|&p| (p, (crate::recipes::Furnace::default(), now))).collect();
        Ok(Self { map: Mutex::new(map), furnaces: Mutex::new(furnaces), viewers: Mutex::default(), brewing: Mutex::default(), hoppers: Mutex::default(), dirty: AtomicBool::new(false) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bedrock::protocol::v2168::types::NetworkItemStackDescriptorV2;

    #[test]
    fn hopper_moves_chest_to_chest() {
        crate::blocks::init();
        let w = crate::world::World::default();
        let chest = crate::blocks::any_state("minecraft:chest").unwrap();
        let hopper = crate::blocks::with(crate::blocks::any_state("minecraft:hopper").unwrap(), &[("facing_direction", crate::blocks::State::Int(0))]);
        w.set(0, -59, 0, chest);
        w.set(0, -60, 0, hopper);
        w.set(0, -61, 0, chest);
        let c = Containers::default();
        let item = NetworkItemStackDescriptorV2 { id: 7, stack_size: 1, aux_value: 0, net_id: None, block_runtime_id: 0, user_data_buffer: vec![0; 10] };
        let mut top = vec![None; CHEST_SLOTS];
        top[0] = Some(Stack { item, count: 3, net_id: 0 });
        c.put((0, -59, 0), top);
        c.hoppers.lock().insert((0, -60, 0));
        for _ in 0..6 {
            c.tick_hoppers(&w);
        }
        assert_eq!(c.get((0, -61, 0))[0].as_ref().map(|s| s.count), Some(3), "all 3 passed through");
        assert!(c.get((0, -59, 0)).iter().all(Option::is_none));
    }

    #[test]
    fn save_load_and_damaged_files() {
        let path = std::env::temp_dir().join(format!("rusted_containers_{}.dat", std::process::id()));
        let c = Containers::default();
        let mut slots = vec![None; CHEST_SLOTS];
        let item = NetworkItemStackDescriptorV2 { id: 7, stack_size: 1, aux_value: 0, net_id: None, block_runtime_id: 0, user_data_buffer: vec![0; 10] };
        slots[26] = Some(Stack { item, count: 33, net_id: 0 });
        c.put((1, -60, -5), slots);
        c.put((9, 9, 9), vec![None; CHEST_SLOTS]); // empty: not stored
        assert!(c.save_if_dirty(&path).unwrap());
        let back = Containers::load(&path).unwrap();
        assert_eq!(back.get((1, -60, -5))[26].as_ref().unwrap().count, 33);
        assert!(back.get((9, 9, 9)).iter().all(Option::is_none));
        std::fs::write(&path, b"RSTC\x01\x00\x00\x00\x05\x00\x00\x00").unwrap();
        assert!(Containers::load(&path).is_err(), "damaged files are refused");
        std::fs::remove_file(&path).ok();
        assert!(is_container(crate::blocks::any_state("minecraft:chest").unwrap()));
        assert!(!is_container(crate::world::GRASS));
    }
}

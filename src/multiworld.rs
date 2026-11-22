//! Extra worlds (like Multiverse): `/mv create <name> [void|flat|normal] [seed]`,
//! `/mv tp <name> [player]`, `/mv list`. Each is an overworld to the players, stored in its own
//! height band (see dims.rs), listed in worlds/<level>/worlds.txt: `name dim type seed`.

use crate::players::Server;
use crate::world::{Column, DimGen};
use parking_lot::Mutex;
use std::sync::Arc;

pub struct Extra {
    pub name: String,
    pub dim: u8,
    pub kind: String,
    pub seed: u64,
}

static WORLDS: Mutex<Vec<Extra>> = Mutex::new(Vec::new());

fn file() -> std::path::PathBuf {
    std::path::Path::new("worlds").join(&crate::config::get().level_name).join("worlds.txt")
}

struct Void;
struct Air;
impl Column for Air {
    fn get(&self, _: i32, _: i32, _: i32) -> i32 {
        crate::world::AIR
    }
    fn biome(&self, _: i32, _: i32, _: i32) -> u16 {
        1 // plains
    }
}
impl DimGen for Void {
    fn column(&self, _: i32, _: i32) -> Arc<dyn Column> {
        Arc::new(Air)
    }
}

/// Grass on dirt on bedrock, like the default flat world.
struct Flat(Vec<i32>);
impl Column for Flat {
    fn get(&self, _: i32, y: i32, _: i32) -> i32 {
        self.0.get(y as usize).copied().unwrap_or(crate::world::AIR)
    }
    fn biome(&self, _: i32, _: i32, _: i32) -> u16 {
        1
    }
}
struct FlatGen(Arc<Flat>);
impl DimGen for FlatGen {
    fn column(&self, _: i32, _: i32) -> Arc<dyn Column> {
        self.0.clone()
    }
}

/// The vanilla generator with its own seed.
struct Normal(Arc<crate::overworld::Overworld>);
struct NormalCol(crate::overworld::Chunk);
impl Column for NormalCol {
    fn get(&self, lx: i32, y: i32, lz: i32) -> i32 {
        self.0.get(lx, y - 64, lz)
    }
    fn biome(&self, lx: i32, y: i32, lz: i32) -> u16 {
        self.0.biome(lx, y - 64, lz)
    }
}
impl DimGen for Normal {
    fn column(&self, cx: i32, cz: i32) -> Arc<dyn Column> {
        Arc::new(NormalCol(self.0.generate(cx, cz)))
    }
}

fn generator(kind: &str, seed: u64) -> Option<Box<dyn DimGen>> {
    let b = |n: &str| crate::blocks::any_state(n).unwrap_or(crate::world::AIR);
    Some(match kind {
        "void" => Box::new(Void),
        "flat" => Box::new(FlatGen(Arc::new(Flat(vec![b("minecraft:bedrock"), b("minecraft:dirt"), b("minecraft:dirt"), b("minecraft:grass_block")])))),
        "normal" => Box::new(Normal(Arc::new(crate::overworld::Overworld::new(seed)))),
        _ => return None,
    })
}

/// Loads worlds.txt and registers each world's generator (startup).
pub fn load(server: &Server) {
    let text = std::fs::read_to_string(file()).unwrap_or_default();
    let mut list = WORLDS.lock();
    for line in text.lines() {
        let p: Vec<&str> = line.split_whitespace().collect();
        let [name, dim, kind, seed] = p.as_slice() else { continue };
        let (Ok(dim), Ok(seed)) = (dim.parse::<u8>(), seed.parse::<u64>()) else { continue };
        if let Some(g) = generator(kind, seed) {
            server.world.set_dim(dim, g);
            list.push(Extra { name: name.to_string(), dim, kind: kind.to_string(), seed });
        }
    }
}

fn save(list: &[Extra]) {
    let text: String = list.iter().map(|w| format!("{} {} {} {}\n", w.name, w.dim, w.kind, w.seed)).collect();
    if let Err(e) = std::fs::write(file(), text) {
        println!("failed to save worlds.txt: {e}");
    }
}

/// Where players arrive in a world: the top block at 0, 0 (a platform is made in a void).
pub fn spawn_of(server: &Server, dim: u8) -> (f32, f32, f32) {
    let base = crate::dims::base(dim);
    let w = &server.world;
    let mut y = base + 319;
    while y > base - 64 && w.get(0, y, 0) == crate::world::AIR {
        y -= 1;
    }
    if y <= base - 64 {
        // Nothing to stand on: a small stone platform.
        y = base + 64;
        if let Some(stone) = crate::blocks::any_state("minecraft:stone") {
            for x in -2..=2 {
                for z in -2..=2 {
                    server.set_block(0, x, y, z, stone);
                }
            }
        }
    }
    (0.5, (y + 1) as f32, 0.5)
}

pub fn find(name: &str) -> Option<u8> {
    if name.eq_ignore_ascii_case("world") || name.eq_ignore_ascii_case("overworld") {
        return Some(crate::dims::OVERWORLD);
    }
    WORLDS.lock().iter().find(|w| w.name.eq_ignore_ascii_case(name)).map(|w| w.dim)
}

/// Which inventory a dimension uses: the main world, Nether and End share one; each extra
/// world has its own.
pub fn inventory_group(dim: u8) -> u8 {
    if crate::dims::is_extra(dim) { dim } else { 0 }
}

/// Swaps the player's inventory when they move between inventory groups: the old one is
/// saved to players/<name>.inv<group>, the new one loaded (empty the first time).
pub fn swap_inventory(name: &str, inv: &mut crate::inventory::Inventory, from: u8, to: u8) -> bool {
    let (a, b) = (inventory_group(from), inventory_group(to));
    if a == b {
        return false;
    }
    let path = |g: u8| crate::saves::player_path(name).with_extension(format!("inv{g}"));
    let _ = std::fs::create_dir_all("players").and_then(|()| std::fs::write(path(a), inv.to_bytes()));
    *inv = std::fs::read(path(b)).ok().and_then(|bytes| crate::inventory::Inventory::from_bytes(&bytes)).unwrap_or_default();
    true
}

/// The /mv command (ops).
pub fn command(server: &Server, sender: &str, args: &[&str]) -> String {
    match args {
        ["create", name, rest @ ..] => {
            let kind = rest.first().copied().unwrap_or("normal");
            let seed = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or_else(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64));
            if find(name).is_some() {
                return format!("§cThere's already a world called {name}");
            }
            let mut list = WORLDS.lock();
            let Some(dim) = (crate::dims::FIRST_EXTRA..crate::dims::FIRST_EXTRA + crate::dims::MAX_EXTRA).find(|d| !list.iter().any(|w| w.dim == *d)) else {
                return format!("§cAt most {} extra worlds", crate::dims::MAX_EXTRA);
            };
            let Some(g) = generator(kind, seed) else { return "§cType must be void, flat or normal".into() };
            server.world.set_dim(dim, g);
            list.push(Extra { name: name.to_string(), dim, kind: kind.to_string(), seed });
            save(&list);
            format!("§aCreated {kind} world {name}. §7/mv tp {name}")
        }
        ["tp", name, who @ ..] => {
            let Some(dim) = find(name) else { return format!("§cNo world called {name}") };
            let target = who.first().copied().unwrap_or(sender);
            let Some(id) = server.find_by_name(target) else { return format!("§c{target} isn't online") };
            let feet = if dim == crate::dims::OVERWORLD {
                let s = crate::commands::world_spawn(server);
                (s.0, s.1 - 1.62, s.2)
            } else {
                spawn_of(server, dim)
            };
            let _ = server.events.send(crate::players::Event::Teleport { id, feet });
            format!("§aSent {target} to {name}")
        }
        ["list"] => {
            let list = WORLDS.lock();
            let mut out = String::from("§aWorlds: §fworld (main)");
            for w in list.iter() {
                out.push_str(&format!("§7, §f{} §8({})", w.name, w.kind));
            }
            out
        }
        _ => "§cUsage: /mv create <name> [void|flat|normal] [seed], /mv tp <name> [player], /mv list".into(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn each_extra_world_has_its_own_inventory() {
        let name = "mv_inventory_test";
        let mut inv = crate::inventory::Inventory::default();
        let dirt = crate::items::item_by_name("minecraft:dirt").unwrap();
        inv.set_slot(0, dirt, 5);
        assert!(super::swap_inventory(name, &mut inv, 0, crate::dims::FIRST_EXTRA), "into the extra world");
        assert!(inv.held(0).is_none(), "starts empty there");
        assert!(!super::swap_inventory(name, &mut inv, 0, crate::dims::NETHER), "overworld and nether share");
        assert!(super::swap_inventory(name, &mut inv, crate::dims::FIRST_EXTRA, 0));
        assert_eq!(inv.held(0).map(|s| s.count), Some(5), "the main inventory came back");
        for g in [0, crate::dims::FIRST_EXTRA] {
            let _ = std::fs::remove_file(crate::saves::player_path(name).with_extension(format!("inv{g}")));
        }
    }

    #[test]
    fn extra_worlds_have_their_own_bands() {
        use crate::dims::*;
        assert_eq!(of(base(FIRST_EXTRA)), FIRST_EXTRA);
        assert_eq!(of(base(FIRST_EXTRA) - 64), FIRST_EXTRA);
        assert_eq!(of(base(FIRST_EXTRA) + 319), FIRST_EXTRA);
        assert_eq!(of(base(FIRST_EXTRA + 1) - 64), FIRST_EXTRA + 1);
        assert_eq!(of(2100), END);
        assert_eq!(network_id(FIRST_EXTRA + 2), 0);
        let (lo, hi) = range(FIRST_EXTRA);
        assert!(lo > range(END).1 && hi < range(FIRST_EXTRA + 1).0);
    }
}

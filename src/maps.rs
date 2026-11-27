//! Maps: using an empty map makes a filled map of the 128x128 area you're in (snapped to the
//! vanilla map grid), and the server draws its pixels from the top block of each column, shaded
//! by height like vanilla. Map ids and centres are kept in worlds/<level>/maps.txt.
//! The cartography table zooms maps out (scale 0-4, each step twice the area) and locks them:
//! a locked map keeps the picture it had, saved in worlds/<level>/maps/<id>.locked.

use crate::players::Server;
use parking_lot::Mutex;
use std::collections::HashMap;

struct MapData {
    center: (i32, i32),
    dim: u8,
    /// 0-4: each pixel covers 2^scale blocks.
    scale: u8,
    locked: bool,
    /// Drawn pixels and when (redrawn after a while so maps show new building).
    drawn: Option<(std::time::Instant, Vec<i32>)>,
}

static MAPS: Mutex<Option<HashMap<i64, MapData>>> = Mutex::new(None);

const SIZE: i32 = 128;

fn file() -> std::path::PathBuf {
    std::path::Path::new("worlds").join(&crate::config::get().level_name).join("maps.txt")
}

fn with_maps<R>(f: impl FnOnce(&mut HashMap<i64, MapData>) -> R) -> R {
    let mut g = MAPS.lock();
    let maps = g.get_or_insert_with(|| {
        let text = std::fs::read_to_string(file()).unwrap_or_default();
        text.lines()
            .filter_map(|l| {
                let p: Vec<i64> = l.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                // Older files have no scale or locked columns.
                let [id, x, z, dim] = *p.get(..4)? else { return None };
                let scale = p.get(4).copied().unwrap_or(0).clamp(0, 4) as u8;
                let locked = p.get(5).copied().unwrap_or(0) != 0;
                let drawn = if locked { load_locked(id).map(|px| (std::time::Instant::now(), px)) } else { None };
                Some((id, MapData { center: (x as i32, z as i32), dim: dim as u8, scale, locked, drawn }))
            })
            .collect()
    });
    f(maps)
}

/// Centre of the vanilla map grid square containing `v` at this scale (squares 128 << scale
/// wide, offset by 64).
fn grid(v: i32, scale: u8) -> i32 {
    let w = SIZE << scale;
    (v + 64).div_euclid(w) * w + w / 2 - 64
}

fn save(maps: &HashMap<i64, MapData>) {
    let mut ids: Vec<_> = maps.keys().copied().collect();
    ids.sort();
    let text: String = ids
        .iter()
        .map(|id| {
            let m = &maps[id];
            format!("{id} {} {} {} {} {}\n", m.center.0, m.center.1, m.dim, m.scale, m.locked as u8)
        })
        .collect();
    if let Err(e) = std::fs::write(file(), text) {
        println!("failed to save maps.txt: {e}");
    }
}

fn add(maps: &mut HashMap<i64, MapData>, m: MapData) -> i64 {
    let id = maps.keys().max().copied().unwrap_or(0) + 1;
    maps.insert(id, m);
    save(maps);
    id
}

/// A new map centred on the map grid square containing (x, z). Returns its id.
pub fn create(x: f32, z: f32, dim: u8) -> i64 {
    let center = (grid(x.floor() as i32, 0), grid(z.floor() as i32, 0));
    with_maps(|maps| add(maps, MapData { center, dim, scale: 0, locked: false, drawn: None }))
}

/// Cartography table with paper: a new map one scale further out, around the same spot.
/// None at the widest scale (4) or for a locked map, like vanilla.
pub fn zoom_out(id: i64) -> Option<i64> {
    with_maps(|maps| {
        let m = maps.get(&id).filter(|m| m.scale < 4 && !m.locked)?;
        let scale = m.scale + 1;
        let zoomed = MapData { center: (grid(m.center.0, scale), grid(m.center.1, scale)), dim: m.dim, scale, locked: false, drawn: None };
        Some(add(maps, zoomed))
    })
}

/// Cartography table with a glass pane: a locked copy. It takes its picture the first time
/// it's drawn and keeps it from then on.
pub fn lock(id: i64) -> Option<i64> {
    with_maps(|maps| {
        let m = maps.get(&id).filter(|m| !m.locked)?;
        // Keep the source's current picture if it has one, so the lock shows what you saw.
        let drawn = m.drawn.as_ref().map(|(_, p)| {
            save_locked_pixels(maps.keys().max().copied().unwrap_or(0) + 1, p);
            (std::time::Instant::now(), p.clone())
        });
        let locked = MapData { center: m.center, dim: m.dim, scale: m.scale, locked: true, drawn };
        Some(add(maps, locked))
    })
}

fn save_locked_pixels(id: i64, pixels: &[i32]) {
    let dir = file().with_file_name("maps");
    let bytes: Vec<u8> = pixels.iter().flat_map(|p| p.to_le_bytes()).collect();
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join(format!("{id}.locked")), bytes)) {
        println!("failed to save locked map {id}: {e}");
    }
}

fn load_locked(id: i64) -> Option<Vec<i32>> {
    let bytes = std::fs::read(file().with_file_name("maps").join(format!("{id}.locked"))).ok()?;
    Some(bytes.chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

/// The filled map item for map `id`.
pub fn filled_map(id: i64) -> Option<bedrock::protocol::v2168::types::NetworkItemStackDescriptorV2> {
    let mut item = crate::items::item_by_name("minecraft:filled_map")?;
    let mut d = crate::itemdata::ItemData::read(&item.user_data_buffer);
    d.set("map_uuid", crate::itemdata::Tag::Long(id));
    d.set("map_name_index", crate::itemdata::Tag::Int(id as i32));
    let scale = with_maps(|maps| maps.get(&id).map_or(0, |m| m.scale));
    d.set("map_scale", crate::itemdata::Tag::Int(scale as i32));
    item.user_data_buffer = d.write();
    Some(item)
}

/// Map colour of a block, as (r, g, b), from vanilla's map colour groups.
fn color(name: &str) -> Option<(u8, u8, u8)> {
    let n = name.trim_start_matches("minecraft:");
    Some(match n {
        "air" | "cave_air" | "void_air" | "barrier" => return None,
        n if n.contains("water") || n == "seagrass" || n == "kelp" || n == "bubble_column" => (64, 64, 255),
        n if n.contains("lava") || n == "fire" || n == "magma" => (255, 0, 0),
        n if n.contains("ice") => (160, 160, 255),
        n if n.contains("snow") || n == "powder_snow" => (255, 255, 255),
        "grass_block" | "grass_path" | "dirt_path" | "slime" => (127, 178, 56),
        n if n.contains("leaves") || n.contains("grass") || n.contains("fern") || n.contains("flower") || n.contains("sapling") || n == "vine" || n.contains("tulip") || n == "poppy" || n == "dandelion" => (0, 124, 0),
        n if n.contains("red_sand") || n.contains("terracotta") || n.contains("red_sandstone") => (216, 127, 51),
        n if n.contains("sand") || n == "birch_planks" || n.contains("bone_block") || n == "glowstone" => (247, 233, 163),
        n if n.contains("dirt") || n == "farmland" || n.contains("podzol") || n.contains("mud") || n == "coarse_dirt" || n == "rooted_dirt" => (151, 109, 77),
        "clay" => (164, 168, 184),
        n if n.contains("deepslate") || n.contains("tuff") => (100, 100, 100),
        n if n.contains("stone") || n.contains("cobble") || n.contains("andesite") || n.contains("gravel") || n.contains("ore") || n == "furnace" => (112, 112, 112),
        n if n.contains("diorite") || n.contains("quartz") || n == "calcite" => (255, 252, 245),
        n if n.contains("granite") => (151, 109, 77),
        n if n.contains("log") || n.contains("planks") || n.contains("wood") || n.contains("fence") || n.contains("door") || n.contains("chest") || n == "crafting_table" || n == "bookshelf" => (143, 119, 72),
        n if n.contains("netherrack") || n.contains("nether") || n.contains("crimson") => (112, 2, 0),
        n if n.contains("warped") => (22, 126, 134),
        n if n.contains("end_stone") || n.contains("purpur") => (247, 233, 163),
        "obsidian" | "crying_obsidian" | "blackstone" | "basalt" | "coal_block" => (25, 25, 25),
        n if n.contains("gold") => (250, 238, 77),
        n if n.contains("diamond") => (92, 219, 213),
        n if n.contains("emerald") => (0, 217, 58),
        n if n.contains("iron") => (167, 167, 167),
        n if n.contains("redstone") => (255, 0, 0),
        n if n.contains("lapis") => (74, 128, 255),
        "pumpkin" | "carved_pumpkin" | "melon_block" | "hay_block" => (216, 127, 51),
        n if n.contains("mushroom") || n.contains("mycelium") => (127, 63, 178),
        n if n.contains("wool") || n.contains("concrete") || n.contains("glass") || n.contains("carpet") => (199, 199, 199),
        _ => (112, 112, 112),
    })
}

/// The top block that shows on a map at (x, z): its height and colour.
fn top(server: &Server, x: i32, z: i32, dim: u8) -> (i32, Option<(u8, u8, u8)>) {
    let (lo, hi) = crate::dims::range(dim);
    let mut y = hi;
    while y >= lo {
        let b = server.world.get(x, y, z);
        if b != crate::world::AIR
            && let Some(c) = crate::blocks::entry(b).and_then(|e| color(&e.name))
        {
            return (y, Some(c));
        }
        y -= 1;
    }
    (lo, None)
}

/// Pixels (ABGR, the order Bedrock maps use), row by row: shaded lighter where the ground
/// steps up from the north and darker where it steps down, like vanilla.
/// Zoomed out, each pixel shows the block at its corner (shortcut: vanilla blends the area;
/// upgrade if zoomed maps look too noisy).
fn draw(server: &Server, center: (i32, i32), dim: u8, scale: u8) -> Vec<i32> {
    let step = 1 << scale;
    let (x0, z0) = (center.0 - SIZE / 2 * step, center.1 - SIZE / 2 * step);
    let mut out = Vec::with_capacity((SIZE * SIZE) as usize);
    for pz in 0..SIZE {
        for px in 0..SIZE {
            let (x, z) = (x0 + px * step, z0 + pz * step);
            let (h, c) = top(server, x, z, dim);
            let Some((r, g, b)) = c else {
                out.push(0); // transparent
                continue;
            };
            let (north, _) = top(server, x, z - step, dim);
            let shade: u32 = if h > north { 255 } else if h < north { 180 } else { 220 };
            let s = |v: u8| (v as u32 * shade / 255) as i32;
            out.push((0xFFu32 << 24) as i32 | s(b) << 16 | s(g) << 8 | s(r));
        }
    }
    out
}

/// The map data packet for map `id` (what the client asks for while holding it).
pub fn packet(server: &Server, id: i64) -> Option<crate::Protocol> {
    let (center, dim, scale, locked, cached) = with_maps(|maps| {
        let m = maps.get(&id)?;
        // Locked maps never redraw.
        let fresh = m.drawn.as_ref().filter(|(t, _)| m.locked || t.elapsed().as_secs() < 30).map(|(_, p)| p.clone());
        Some((m.center, m.dim, m.scale, m.locked, fresh))
    })?;
    let pixels = match cached {
        Some(p) => p,
        None => {
            let p = draw(server, center, dim, scale);
            with_maps(|maps| {
                if let Some(m) = maps.get_mut(&id) {
                    m.drawn = Some((std::time::Instant::now(), p.clone()));
                }
            });
            if locked {
                save_locked_pixels(id, &p);
            }
            p
        }
    };
    use bedrock::protocol::v662::types::ActorUniqueID;
    use bedrock::protocol::v944::types::NetworkBlockPosition;
    Some(crate::Protocol::ClientBoundMapItemDataPacket(Box::new(bedrock::protocol::v2168::packets::ClientBoundMapItemDataPacket {
        map_id: ActorUniqueID(id),
        dimension: crate::dims::network_id(dim) as i8,
        is_locked: locked,
        map_origin: NetworkBlockPosition { x: center.0, y: 0, z: center.1 },
        creation_map_ids: Some(vec![ActorUniqueID(id)]),
        scale: Some(scale as i8),
        tracked_actor_ids: Some(vec![]),
        decorations: Some(vec![]),
        width: Some(SIZE),
        height: Some(SIZE),
        start_x: Some(0),
        start_y: Some(0),
        pixels: Some(pixels),
    })))
}

#[cfg(test)]
mod tests {
    #[test]
    fn grid_matches_vanilla() {
        // Scale 0: 128-wide squares; (0,0) is in the square centred on 0.
        assert_eq!(super::grid(0, 0), 0);
        assert_eq!(super::grid(63, 0), 0);
        assert_eq!(super::grid(64, 0), 128);
        assert_eq!(super::grid(-65, 0), -128);
        // Scale 1: 256-wide squares centred on 64, -192...
        assert_eq!(super::grid(0, 1), 64);
        assert_eq!(super::grid(-65, 1), -192);
        assert_eq!(super::grid(191, 1), 64);
        assert_eq!(super::grid(192, 1), 320);
    }

    #[test]
    fn colours_for_common_blocks() {
        assert_eq!(super::color("minecraft:grass_block"), Some((127, 178, 56)));
        assert_eq!(super::color("minecraft:water"), Some((64, 64, 255)));
        assert_eq!(super::color("minecraft:air"), None);
        assert!(super::color("minecraft:oak_log").is_some());
    }
}

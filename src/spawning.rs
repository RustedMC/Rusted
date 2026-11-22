//! Natural mob spawning in the Overworld, ported from PNX's Bedrock spawn rules
//! (level/entity/spawners): each mob has a herd size, a weight and conditions (light, ground or
//! water, biome tags, height, the block underneath, a density limit), and each category has
//! Bedrock's population cap, counted over the 9x9 chunks around the spot, surface and
//! underground apart (ConditionPopulationControl).
//!
//! shortcut: a few PNX rules split by biome (sheep, wolves, rabbits, llamas, drowned, pandas,
//! salmon) use one combined rule here with vanilla's usual weights; split them if it shows.
//! shortcut: brightness uses block light plus "open sky by day = 15, by night = 4" until sky
//! light exists (roadmap 0.5.0).

use crate::mobs::Kind;
use crate::overworld::biome::id as b;
use crate::players::Server;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Cat {
    Monster,
    Animal,
    WaterAnimal,
    Ambient,
}

#[derive(Clone, Copy, PartialEq)]
enum Place {
    /// feet and head in air, a solid block underneath
    Ground,
    /// feet in water
    Water,
}

#[derive(Clone, Copy, PartialEq)]
enum Depth {
    Any,
    Underground,
}

struct Rule {
    kind: Kind,
    herd: (u32, u32),
    weight: u32,
    cat: Cat,
    place: Place,
    depth: Depth,
    light: (i32, i32),
    biome: fn(u16) -> bool,
    height: (i32, i32),
    /// block underneath must be one of these (empty = any solid)
    on: &'static [&'static str],
    /// at most this many of the kind within this many blocks
    density: Option<(usize, f32)>,
}

// Biome tags (Bedrock's biome definitions, as PNX's BiomeTags).
fn ocean(x: u16) -> bool {
    matches!(x, b::OCEAN | b::DEEP_OCEAN | b::WARM_OCEAN | b::LUKEWARM_OCEAN | b::DEEP_LUKEWARM_OCEAN | b::COLD_OCEAN | b::DEEP_COLD_OCEAN | b::FROZEN_OCEAN | b::DEEP_FROZEN_OCEAN | b::LEGACY_FROZEN_OCEAN)
}
fn warm(x: u16) -> bool {
    x == b::WARM_OCEAN || x == b::DESERT || x == b::BEACH
}
fn lukewarm(x: u16) -> bool {
    matches!(x, b::LUKEWARM_OCEAN | b::DEEP_LUKEWARM_OCEAN)
}
fn frozen(x: u16) -> bool {
    matches!(x, b::FROZEN_OCEAN | b::DEEP_FROZEN_OCEAN | b::LEGACY_FROZEN_OCEAN | b::FROZEN_RIVER | b::ICE_PLAINS | b::ICE_PLAINS_SPIKES | b::COLD_TAIGA | b::COLD_BEACH | b::FROZEN_PEAKS | b::SNOWY_SLOPES | b::GROVE | b::JAGGED_PEAKS)
}
fn river(x: u16) -> bool {
    x == b::RIVER || x == b::FROZEN_RIVER
}
fn swamp(x: u16) -> bool {
    x == b::SWAMPLAND || x == b::MANGROVE_SWAMP
}
fn desert(x: u16) -> bool {
    x == b::DESERT
}
fn taiga(x: u16) -> bool {
    matches!(x, b::TAIGA | b::COLD_TAIGA | b::MEGA_TAIGA | b::REDWOOD_TAIGA_MUTATED)
}
fn jungle(x: u16) -> bool {
    matches!(x, b::JUNGLE | b::JUNGLE_EDGE | b::BAMBOO_JUNGLE)
}
fn plains(x: u16) -> bool {
    x == b::PLAINS || x == b::SUNFLOWER_PLAINS
}
fn savanna(x: u16) -> bool {
    matches!(x, b::SAVANNA | b::SAVANNA_PLATEAU | b::SAVANNA_MUTATED)
}
fn mesa(x: u16) -> bool {
    matches!(x, b::MESA | b::MESA_PLATEAU_STONE | b::MESA_BRYCE)
}
fn peaks(x: u16) -> bool {
    matches!(x, b::SNOWY_SLOPES | b::JAGGED_PEAKS | b::FROZEN_PEAKS)
}
fn hills(x: u16) -> bool {
    matches!(x, b::EXTREME_HILLS | b::EXTREME_HILLS_PLUS_TREES | b::EXTREME_HILLS_MUTATED | b::STONE_BEACH)
}
/// Monsters spawn everywhere in the Overworld but mushroom fields and the deep dark.
fn monster(x: u16) -> bool {
    x != b::MUSHROOM_ISLAND && x != b::DEEP_DARK
}
/// Farm animals: land biomes with grass.
fn animal(x: u16) -> bool {
    !ocean(x) && !river(x) && !desert(x) && !mesa(x) && !matches!(x, b::BEACH | b::COLD_BEACH | b::STONE_BEACH | b::MUSHROOM_ISLAND | b::DEEP_DARK | b::LUSH_CAVES | b::DRIPSTONE_CAVES)
}
fn any(_: u16) -> bool {
    true
}

const GRASS: &[&str] = &["grass_block"];
const SAND: &[&str] = &["sand", "red_sand"];

const fn rule(kind: Kind, herd: (u32, u32), weight: u32, cat: Cat, place: Place, light: (i32, i32), biome: fn(u16) -> bool) -> Rule {
    Rule { kind, herd, weight, cat, place, depth: Depth::Any, light, biome, height: (-64, 320), on: &[], density: None }
}

fn rules() -> &'static [Rule] {
    static RULES: std::sync::OnceLock<Vec<Rule>> = std::sync::OnceLock::new();
    use Cat::*;
    use Place::*;
    const DARK: (i32, i32) = (0, 7);
    const LIT: (i32, i32) = (7, 15);
    const ANY: (i32, i32) = (0, 15);
    RULES.get_or_init(|| {
        vec![
            // Monsters (SpawnRuleZombie, Skeleton, Creeper...)
            rule(Kind::Zombie, (2, 4), 95, Monster, Ground, DARK, monster),
            rule(Kind::ZombieVillager, (2, 4), 5, Monster, Ground, DARK, monster),
            rule(Kind::Husk, (2, 4), 240, Monster, Ground, DARK, desert),
            rule(Kind::Skeleton, (1, 2), 80, Monster, Ground, DARK, |x| monster(x) && !frozen(x)),
            rule(Kind::Stray, (1, 2), 120, Monster, Ground, DARK, frozen),
            rule(Kind::Bogged, (1, 2), 40, Monster, Ground, DARK, swamp),
            rule(Kind::Creeper, (1, 1), 100, Monster, Ground, DARK, monster),
            rule(Kind::Spider, (1, 1), 100, Monster, Ground, DARK, monster),
            rule(Kind::Enderman, (1, 1), 10, Monster, Ground, DARK, monster),
            Rule { density: Some((1, 128.0)), ..rule(Kind::Witch, (1, 1), 5, Monster, Ground, DARK, monster) },
            rule(Kind::Slime, (1, 1), 100, Monster, Ground, ANY, swamp),
            rule(Kind::Drowned, (1, 2), 100, Monster, Water, DARK, river),
            rule(Kind::Drowned, (1, 2), 5, Monster, Water, DARK, ocean),
            // Animals
            Rule { on: GRASS, ..rule(Kind::Chicken, (2, 4), 10, Animal, Ground, LIT, animal) },
            Rule { on: GRASS, ..rule(Kind::Cow, (2, 3), 8, Animal, Ground, LIT, animal) },
            Rule { on: GRASS, ..rule(Kind::Pig, (1, 3), 10, Animal, Ground, LIT, |x| animal(x) || x == b::CHERRY_GROVE) },
            Rule { on: GRASS, ..rule(Kind::Sheep, (2, 3), 12, Animal, Ground, LIT, animal) },
            Rule { on: GRASS, ..rule(Kind::Horse, (2, 6), 4, Animal, Ground, LIT, |x| plains(x) || savanna(x)) },
            Rule { on: GRASS, ..rule(Kind::Donkey, (1, 3), 1, Animal, Ground, LIT, plains) },
            rule(Kind::Fox, (2, 4), 8, Animal, Ground, LIT, |x| taiga(x) || x == b::GROVE),
            rule(Kind::Frog, (2, 5), 10, Animal, Ground, ANY, swamp),
            Rule { on: &["stone", "snow", "powder_snow", "snow_layer", "packed_ice", "gravel"], ..rule(Kind::Goat, (1, 3), 5, Animal, Ground, LIT, peaks) },
            Rule { on: &["mycelium", "grass_block"], ..rule(Kind::Mooshroom, (4, 8), 8, Animal, Ground, (9, 15), |x| x == b::MUSHROOM_ISLAND) },
            Rule { on: GRASS, ..rule(Kind::Ocelot, (1, 2), 30, Animal, Ground, LIT, jungle) },
            Rule { on: GRASS, ..rule(Kind::Parrot, (1, 2), 40, Animal, Ground, LIT, jungle) },
            Rule { on: GRASS, ..rule(Kind::Panda, (1, 2), 1, Animal, Ground, LIT, |x| x == b::JUNGLE || x == b::JUNGLE_EDGE) },
            Rule { on: GRASS, ..rule(Kind::Panda, (1, 2), 80, Animal, Ground, LIT, |x| x == b::BAMBOO_JUNGLE) },
            rule(Kind::PolarBear, (1, 2), 1, Animal, Ground, LIT, frozen),
            Rule { on: &["grass_block", "sand", "red_sand", "snow", "snow_layer"], ..rule(Kind::Rabbit, (2, 3), 4, Animal, Ground, LIT, |x| desert(x) || taiga(x) || frozen(x) || x == b::FLOWER_FOREST || x == b::MEADOW) },
            Rule { on: &["grass_block", "dirt", "coarse_dirt", "podzol"], ..rule(Kind::Wolf, (4, 4), 8, Animal, Ground, LIT, |x| taiga(x) || x == b::GROVE) },
            Rule { on: GRASS, ..rule(Kind::Llama, (4, 6), 5, Animal, Ground, LIT, |x| hills(x) || savanna(x)) },
            Rule { on: SAND, height: (60, 67), ..rule(Kind::Turtle, (2, 6), 8, Animal, Ground, LIT, |x| x == b::BEACH) },
            Rule { on: &["sand", "red_sand", "sandstone"], density: Some((1, 128.0)), ..rule(Kind::Camel, (1, 1), 1, Animal, Ground, LIT, desert) },
            Rule { on: &["grass_block", "red_sand", "coarse_dirt", "hardened_clay", "brown_terracotta", "orange_terracotta", "red_terracotta", "white_terracotta", "yellow_terracotta", "light_gray_terracotta"], ..rule(Kind::Armadillo, (1, 2), 10, Animal, Ground, LIT, |x| savanna(x) || mesa(x)) },
            // Ambient
            Rule { depth: Depth::Underground, height: (-63, 63), ..rule(Kind::Bat, (8, 8), 10, Ambient, Ground, (0, 4), animal) },
            // Water
            Rule { height: (0, 64), density: Some((20, 128.0)), ..rule(Kind::Cod, (4, 7), 75, WaterAnimal, Water, ANY, |x| ocean(x) && !warm(x)) },
            Rule { density: Some((20, 128.0)), ..rule(Kind::TropicalFish, (3, 5), 75, WaterAnimal, Water, ANY, |x| warm(x) || lukewarm(x)) },
            Rule { height: (0, 64), density: Some((20, 128.0)), ..rule(Kind::Pufferfish, (3, 5), 25, WaterAnimal, Water, ANY, |x| x == b::WARM_OCEAN || lukewarm(x)) },
            Rule { density: Some((20, 128.0)), ..rule(Kind::Salmon, (3, 5), 15, WaterAnimal, Water, ANY, |x| river(x) || matches!(x, b::COLD_OCEAN | b::DEEP_COLD_OCEAN | b::FROZEN_OCEAN | b::DEEP_FROZEN_OCEAN)) },
            Rule { density: Some((4, 128.0)), ..rule(Kind::Squid, (2, 4), 8, WaterAnimal, Water, ANY, |x| ocean(x) || river(x)) },
            Rule { height: (0, 64), density: Some((5, 128.0)), ..rule(Kind::Dolphin, (3, 5), 7, WaterAnimal, Water, ANY, |x| ocean(x) && !frozen(x)) },
            Rule { depth: Depth::Underground, height: (-64, 30), density: Some((2, 128.0)), ..rule(Kind::GlowSquid, (2, 4), 10, WaterAnimal, Water, ANY, any) },
            Rule { depth: Depth::Underground, on: &["clay"], density: Some((5, 32.0)), ..rule(Kind::Axolotl, (4, 6), 10, WaterAnimal, Water, ANY, |x| x == b::LUSH_CAVES) },
        ]
    })
}

/// Bedrock's per-area caps (PNX ConditionPopulationControl, Overworld): (surface, underground).
fn cap(c: Cat) -> (usize, usize) {
    match c {
        Cat::Monster => (8, 8),
        Cat::Animal => (4, 0),
        Cat::WaterAnimal => (36, 36),
        Cat::Ambient => (0, 2),
    }
}

fn category(k: Kind) -> Option<Cat> {
    Some(match k {
        Kind::Bat => Cat::Ambient,
        k if k.aquatic() && !k.hostile() => Cat::WaterAnimal,
        k if k.hostile() => Cat::Monster,
        Kind::Villager | Kind::IronGolem | Kind::Boat | Kind::Minecart | Kind::ChestMinecart | Kind::EnderDragon | Kind::EndCrystal => return None,
        _ => Cat::Animal,
    })
}

/// Bedrock counts surface mobs as at or above sea level - 2.
fn on_surface(y: f32) -> bool {
    y >= 61.0
}

fn short(id: i32) -> &'static str {
    crate::blocks::entry(id).map_or("", |e| e.name.as_str()).trim_start_matches("minecraft:")
}

/// What's at a spot that rules care about.
struct Spot {
    pos: (i32, i32, i32),
    place: Place,
    surface: bool,
    light: i32,
    biome: u16,
    under: &'static str,
}

fn spot(server: &Server, (x, y, z): (i32, i32, i32), night: bool) -> Option<Spot> {
    let w = &server.world;
    let solid = |b: i32| crate::overworld::feature::blocks::is_solid(b);
    let (feet, head, below) = (w.get(x, y, z), w.get(x, y + 1, z), w.get(x, y - 1, z));
    let place = if crate::overworld::feature::blocks::is_water(feet) {
        Place::Water
    } else if !solid(feet) && !solid(head) && !crate::overworld::feature::blocks::is_water(head) && solid(below) && !crate::fluids::is_fluid(feet) {
        Place::Ground
    } else {
        return None;
    };
    let open = crate::mobs::sky(w, (x, y, z));
    // Real sky light (spreads into overhangs and cave mouths), darkened by 11 at night.
    let sky_light = (w.sky_light(x, y, z) as i32 - if night { 11 } else { 0 }).max(0);
    Some(Spot {
        pos: (x, y, z),
        place,
        surface: open,
        light: sky_light.max(w.block_light(x, y, z) as i32),
        // Flat worlds have no biome data: they're plains.
        biome: w.biome_at(x, y, z).unwrap_or(b::PLAINS),
        under: short(below),
    })
}

fn fits(r: &Rule, s: &Spot, peaceful: bool) -> bool {
    r.place == s.place
        && !(peaceful && r.cat == Cat::Monster)
        && match r.depth {
            Depth::Any => true,
            Depth::Underground => !s.surface,
        }
        && (r.light.0..=r.light.1).contains(&s.light)
        && (r.height.0..=r.height.1).contains(&s.pos.1)
        && (r.biome)(s.biome)
        && (r.on.is_empty() || r.on.contains(&s.under))
}

/// Every 2 s: a few tries per Overworld player, 24-44 blocks away, on the surface or in a cave.
pub fn tick(server: &Server, rand: &mut impl FnMut() -> u32) {
    let peaceful = *crate::commands::DIFFICULTY.lock() == 0;
    let t = server.time().rem_euclid(24_000);
    let night = (13_000..23_000).contains(&t);
    let w = &server.world;
    let mut mobs = server.mobs.snapshot();
    for p in server.all() {
        if crate::dims::of_f(p.pos.1) != crate::dims::OVERWORLD {
            continue;
        }
        for _ in 0..8 {
            let a = (rand() % 6283) as f32 / 1000.0;
            let r = 24.0 + (rand() % 21) as f32;
            let (x, z) = ((p.pos.0 + a.cos() * r).floor() as i32, (p.pos.2 + a.sin() * r).floor() as i32);
            // Surface: the first spot down from above the player; cave: a random height below.
            let top = (p.pos.1 as i32 + 40).min(318);
            let Some(surface_y) = (-62..top).rev().find(|&y| crate::overworld::feature::blocks::is_solid(w.get(x, y - 1, z)) || crate::overworld::feature::blocks::is_water(w.get(x, y, z))) else { continue };
            let y = if rand().is_multiple_of(2) {
                surface_y
            } else {
                let lo = -62;
                let hi = surface_y - 4;
                if hi <= lo {
                    continue;
                }
                lo + (rand() % (hi - lo) as u32) as i32
            };
            let Some(s) = spot(server, (x, y, z), night) else { continue };
            let picks: Vec<&Rule> = rules().iter().filter(|r| fits(r, &s, peaceful)).collect();
            let total: u32 = picks.iter().map(|r| r.weight).sum();
            if total == 0 {
                continue;
            }
            let mut roll = rand() % total;
            let Some(rule) = picks.into_iter().find(|r| {
                if roll < r.weight {
                    true
                } else {
                    roll -= r.weight;
                    false
                }
            }) else { continue };
            // Population cap for this category over the 9x9 chunks around, surface or not.
            let here = (x as f32, y as f32, z as f32);
            let surface = on_surface(here.1);
            let (cs, cu) = cap(rule.cat);
            let limit = if surface { cs } else { cu };
            let near = |k: Option<Kind>, radius: f32| {
                mobs.iter().filter(|(mk, mp)| (mp.0 - here.0).abs() <= radius && (mp.2 - here.2).abs() <= radius && k.is_none_or(|k| *mk == k)).collect::<Vec<_>>()
            };
            let in_cat = near(None, 72.0).into_iter().filter(|(mk, mp)| category(*mk) == Some(rule.cat) && on_surface(mp.1) == surface).count();
            if in_cat >= limit {
                continue;
            }
            if let Some((n, radius)) = rule.density
                && near(Some(rule.kind), radius).len() >= n
            {
                continue;
            }
            // The herd, around the spot where it fits too.
            let herd = rule.herd.0 + rand() % (rule.herd.1 - rule.herd.0 + 1);
            let mut made = 0;
            for i in 0..herd * 3 {
                if made >= herd || in_cat + made as usize >= limit {
                    break;
                }
                let at = if i == 0 { (x, y, z) } else { (x + (rand() % 9) as i32 - 4, y, z + (rand() % 9) as i32 - 4) };
                if let Some(sp) = spot(server, at, night)
                    && fits(rule, &sp, peaceful)
                {
                    let pos = (at.0 as f32 + 0.5, at.1 as f32, at.2 as f32 + 0.5);
                    server.mobs.spawn(server, rule.kind, pos);
                    // Counted right away, so later tries this round see it.
                    mobs.push((rule.kind, pos));
                    made += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_match_pnx_shape() {
        let zombie = rules().iter().find(|r| r.kind == Kind::Zombie).unwrap();
        assert_eq!((zombie.herd, zombie.weight, zombie.light), ((2, 4), 95, (0, 7)));
        assert_eq!(cap(Cat::Monster), (8, 8), "Bedrock's monster cap per area");
        assert!(monster(b::PLAINS) && !monster(b::MUSHROOM_ISLAND));
        assert!(frozen(b::ICE_PLAINS) && !frozen(b::PLAINS));
        assert!(!rules().iter().any(|r| r.kind == Kind::Skeleton && (r.biome)(b::ICE_PLAINS)), "strays, not skeletons, in frozen biomes");
    }

    #[test]
    fn night_brings_monsters_up_to_the_cap() {
        let server = Server::new(); // flat: grass at -61, stand at -60
        server.join(crate::players::tests::sample(7));
        server.moved(7, (0.5, -60.0 + 1.62, 0.5), (0.0, 0.0), 0.0);
        server.set_time(18_000); // midnight
        *crate::commands::DIFFICULTY.lock() = 2;
        let mut seed = 12345u32;
        let mut rand = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for _ in 0..60 {
            tick(&server, &mut rand);
        }
        let mobs = server.mobs.snapshot();
        let monsters = mobs.iter().filter(|(k, _)| k.hostile()).count();
        let animals = mobs.iter().filter(|(k, _)| category(*k) == Some(Cat::Animal)).count();
        assert!(monsters > 0, "something spawned in the dark");
        // The cap is per 9x9-chunk area around each spawn spot (spots are on all sides of the
        // player), so two areas' worth at most.
        assert!(monsters <= 16, "Bedrock's area caps: {monsters}");
        assert_eq!(animals, 0, "too dark for animals");
    }
}

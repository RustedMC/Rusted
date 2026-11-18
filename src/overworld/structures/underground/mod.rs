// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Mineshafts and strongholds (PNX `MineshaftPopulator`, `StrongholdPopulator`).
//!
//! Post-processing ignores other starts sharing the chunk random. Block entities, minecarts,
//! loot and liquid updates are skipped.

mod mineshaft;
pub(crate) mod piece;
mod stronghold;

use super::StructureType;
use crate::overworld::router::Router;

pub fn register(v: &mut Vec<StructureType>) {
    v.push(StructureType { name: "normal_mineshaft", radius: mineshaft::RADIUS, hint: mineshaft::is_start, build: mineshaft::build });
    v.push(StructureType { name: "normal_stronghold", radius: stronghold::RADIUS, hint: stronghold::is_start, build: stronghold::build });
}

/// PNX `OverworldBiomePicker.pickRaw`: the climate biome at a position.
fn raw_biome(r: &Router, x: i32, y: i32, z: i32) -> u16 {
    let mut c = crate::overworld::density::Ctx::default();
    let mut v = |f: &crate::overworld::density::F| f.eval(&mut c, x, y, z) as f32;
    let (cont, t, h, e, w) = (v(&r.continents), v(&r.temperature), v(&r.humidity), v(&r.erosion), v(&r.ridges));
    crate::overworld::biome::pick(cont, t, h, e, w).biome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overworld::Overworld;
    use crate::overworld::feature::blocks as bl;

    fn count(ow: &Overworld, cx: i32, cz: i32, r: i32, names: &[&str]) -> usize {
        let mut n = 0;
        for x in cx - r..=cx + r {
            for z in cz - r..=cz + r {
                let ch = ow.generate(x, z);
                for lx in 0..16 {
                    for lz in 0..16 {
                        for y in -64..120 {
                            if names.contains(&bl::short(ch.get(lx, y, lz))) {
                                n += 1;
                            }
                        }
                    }
                }
            }
        }
        n
    }

    #[test]
    fn mineshaft_generates_planks_and_fences() {
        let seed = 12345u64;
        let starts: Vec<_> = (-60..60).flat_map(|x| (-60..60).map(move |z| (x, z))).filter(|&(x, z)| mineshaft::is_start(seed as i64, x, z)).collect();
        assert!(!starts.is_empty(), "some mineshaft start");
        let ow = Overworld::new(seed);
        let found = starts.iter().take(3).map(|&(x, z)| count(&ow, x, z, 1, &["oak_planks", "oak_fence", "rail", "dark_oak_planks", "dark_oak_fence", "web"])).max().unwrap();
        println!("{} starts, best {found} mineshaft blocks", starts.len());
        assert!(found > 20, "mineshaft blocks near a start: {found}");
    }

    #[test]
    fn stronghold_has_stone_bricks() {
        let seed = 12345i64;
        let ps = stronghold::positions(seed);
        println!("initial strongholds {ps:?}");
        assert!(!ps.is_empty());
        let (cx, cz) = ps[0];
        assert!(stronghold::is_start(seed, cx, cz));
        let ow = Overworld::new(seed as u64);
        let n = count(&ow, cx, cz, 0, &["stone_bricks", "mossy_stone_bricks", "cracked_stone_bricks", "infested_stone_bricks"]);
        assert!(n > 50, "stone bricks at the stronghold start: {n}");
    }
}

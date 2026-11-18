// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
// nether generator tests (no PNX counterpart)

use super::*;
use crate::overworld::feature::blocks as bl;

const SEED: u64 = 12345;

fn count(ch: &NetherChunk, f: impl Fn(i32) -> bool) -> usize {
    let mut n = 0;
    for x in 0..16 {
        for z in 0..16 {
            for y in MIN_Y..=MAX_Y {
                n += f(ch.get(x, y, z)) as usize;
            }
        }
    }
    n
}

#[test]
fn terrain_is_sane() {
    let n = Nether::new(SEED);
    let i = ids();
    let mut lava_sea = 0;
    for (cx, cz) in [(0, 0), (5, -3), (-7, 9)] {
        let ch = n.generate(cx, cz);
        for x in 0..16 {
            for z in 0..16 {
                assert_eq!(ch.get(x, 0, z), i.bedrock, "bedrock floor");
                assert_eq!(ch.get(x, 127, z), i.bedrock, "bedrock roof");
                assert_eq!(ch.get(x, 128, z), i.air);
            }
        }
        assert!(count(&ch, |b| b == i.netherrack) > 10_000, "mostly netherrack");
        lava_sea += count(&ch, |b| b == i.lava);
        let t = n.terrain.generate(n.seed, cx, cz);
        for x in 0..16 {
            for z in 0..16 {
                for y in 32..127 {
                    assert_ne!(t.get(x, y, z), i.lava, "terrain lava only up to y 31");
                }
            }
        }
    }
    assert!(lava_sea > 0, "a lava sea below y 32");
}

#[test]
fn all_biomes_appear() {
    let n = Nether::new(SEED);
    let mut seen = std::collections::HashSet::new();
    for x in (-4096..4096).step_by(64) {
        for z in (-4096..4096).step_by(64) {
            seen.insert(n.terrain.picker.column(x, z));
        }
    }
    for b in [HELL, CRIMSON_FOREST, WARPED_FOREST, SOULSAND_VALLEY, BASALT_DELTAS] {
        assert!(seen.contains(&b), "biome {b} missing: {seen:?}");
    }
}

#[test]
fn deterministic_and_order_independent() {
    let a = Nether::new(SEED);
    let b = Nether::new(SEED);
    let x = a.generate(3, -2);
    let _ = b.generate(4, -2);
    let _ = b.generate(3, -1);
    let y = b.generate(3, -2);
    for lx in 0..16 {
        for lz in 0..16 {
            assert_eq!(x.biome(lx, 40, lz), y.biome(lx, 40, lz));
            for yy in MIN_Y..=MAX_Y {
                assert_eq!(x.get(lx, yy, lz), y.get(lx, yy, lz), "block at {lx} {yy} {lz}");
            }
        }
    }
}

#[test]
fn features_appear() {
    let n = Nether::new(SEED);
    let i = ids();
    let (mut quartz, mut glow, mut fungi) = (0, 0, 0);
    for cx in -3..3 {
        for cz in -3..3 {
            let ch = n.generate(cx, cz);
            quartz += count(&ch, |b| b == i.quartz_ore);
            glow += count(&ch, |b| b == i.glowstone);
            fungi += count(&ch, |b| b == i.crimson_roots || b == i.warped_roots || b == i.crimson_stem || b == i.warped_stem);
        }
    }
    assert!(quartz > 0 && glow > 0, "quartz {quartz}, glowstone {glow}");
    println!("quartz {quartz}, glowstone {glow}, fungi {fungi}");
}

#[test]
fn fortress_and_bastion_appear() {
    let n = Nether::new(SEED);
    let (mut fortress, mut bastion) = (None, None);
    for rx in -4..4 {
        for rz in -4..4 {
            let (x, z, b) = structures_complex(n.seed, rx, rz);
            if b && n.terrain.picker.column((x << 4) + 7, (z << 4) + 7) != BASALT_DELTAS {
                bastion.get_or_insert((x, z));
            } else if !b {
                fortress.get_or_insert((x, z));
            }
        }
    }
    let (fx, fz) = fortress.expect("a fortress start");
    let bricks = bl::id("nether_brick");
    let ch = n.generate(fx, fz);
    assert!(count(&ch, |b| b == bricks) > 100, "fortress bricks at {fx} {fz}");

    let (bx, bz) = bastion.expect("a bastion start");
    let mut stone = 0;
    for dx in -1..=1 {
        for dz in -1..=1 {
            let ch = n.generate(bx + dx, bz + dz);
            stone += count(&ch, |b| bl::short(b).contains("polished_blackstone") || bl::short(b) == "gilded_blackstone");
        }
    }
    assert!(stone > 100, "bastion blocks around {bx} {bz}: {stone}");
}

fn structures_complex(seed: i64, rx: i32, rz: i32) -> (i32, i32, bool) {
    super::structures::complex_for_tests(seed, rx, rz)
}

/// A chunk whose 16x16 columns all have `biome`.
fn chunk_in(n: &Nether, biome: u16) -> (i32, i32) {
    for r in 0..200 {
        for cx in -r..=r {
            for cz in [-r, r] {
                let all = (0..16).step_by(5).all(|x| (0..16).step_by(5).all(|z| n.terrain.picker.column((cx << 4) + x, (cz << 4) + z) == biome));
                if all {
                    return (cx, cz);
                }
            }
        }
    }
    panic!("no chunk of biome {biome}");
}

#[test]
fn biome_features_appear() {
    let n = Nether::new(SEED);
    let i = ids();
    let checks: [(u16, &dyn Fn(i32) -> bool, &str); 4] = [
        (CRIMSON_FOREST, &|b| b == i.crimson_roots || b == i.crimson_fungus || b == i.crimson_stem, "crimson plants"),
        (WARPED_FOREST, &|b| b == i.warped_roots || b == i.warped_fungus || b == i.warped_stem, "warped plants"),
        (BASALT_DELTAS, &|b| b == i.basalt, "basalt"),
        (SOULSAND_VALLEY, &|b| b == i.soul_soil || b == i.soul_sand, "soul soil"),
    ];
    for (biome, f, what) in checks {
        let (cx, cz) = chunk_in(&n, biome);
        let ch = n.generate(cx, cz);
        let c = count(&ch, f);
        println!("{what}: {c} in chunk {cx} {cz}");
        assert!(c > 0, "{what} in chunk {cx} {cz}");
    }
}

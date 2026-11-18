use super::*;
use crate::overworld::Overworld;

fn have_data() -> bool {
    if templates::get("village/plains/town_centers/plains_fountain_01").is_none() {
        println!("data/structures.nbt missing; skipping");
        return false;
    }
    true
}

/// Builds one structure into a fresh StructWorld; returns (blocks by name count, chunk reach).
fn build_at(ow: &Overworld, f: super::super::BuildFn, cx: i32, cz: i32) -> (HashMap<&'static str, usize>, i32) {
    let mut w = StructWorld { ow, writes: Default::default(), seed: ow.seed };
    f(&mut w, &mut Xoroshiro::new(0), cx, cz);
    let mut names = HashMap::new();
    let mut reach = 0;
    for (&k, &v) in &w.writes {
        let (x, _, z) = super::super::unkey(k);
        reach = reach.max(((x >> 4) - cx).abs()).max(((z >> 4) - cz).abs());
        *names.entry(bl::short(v)).or_insert(0) += 1;
    }
    (names, reach)
}

#[test]
fn villages_generate() {
    if !have_data() {
        return;
    }
    let mut found = 0;
    'seeds: for seed in [1u64, 2, 3, 4] {
        let ow = Overworld::new(seed);
        for rx in -3..3 {
            for rz in -3..3 {
                let Some((cx, cz)) = (0..34 * 34).map(|i| (rx * 34 + i / 34, rz * 34 + i % 34)).find(|&(x, z)| VILLAGE.is_candidate(ow.seed, x, z)) else { continue };
                assert!(candidate(&VILLAGE, ow.seed, cx, cz));
                let t = ow.generate_terrain(cx, cz);
                let biome = t.biome(7, t.heights[7 * 16 + 7] - 1, 7);
                if village_for(biome).is_none() {
                    continue;
                }
                let start = std::time::Instant::now();
                let (names, reach) = build_at(&ow, village, cx, cz);
                println!("seed {seed} village at chunk {cx},{cz} biome {biome}: {} kinds, reach {reach} chunks, {:?}", names.len(), start.elapsed());
                assert!(reach <= 9, "village reaches {reach} chunks");
                let total: usize = names.values().sum();
                assert!(total > 500, "only {total} blocks: {names:?}");
                assert!(["bell", "hay_block", "grass_path", "oak_planks", "spruce_planks", "acacia_planks", "sandstone", "cut_sandstone"].iter().any(|n| names.contains_key(n)), "{names:?}");
                // and it lands in real chunks
                let ch = ow.generate(cx, cz);
                let got = (0..16).flat_map(|x| (0..16).map(move |z| (x, z))).any(|(x, z)| (-64..320).any(|y| bl::is(ch.get(x, y, z), "grass_path") || bl::short(ch.get(x, y, z)).ends_with("planks")));
                println!("start chunk has village blocks: {got}");
                found += 1;
                if found >= 2 {
                    break 'seeds;
                }
            }
        }
    }
    assert!(found > 0, "no village candidate in a village biome");
}

#[test]
fn trial_chambers_generate() {
    if !have_data() {
        return;
    }
    let ow = Overworld::new(7);
    let (cx, cz) = (0..34 * 34).map(|i| (i / 34, i % 34)).find(|&(x, z)| TRIAL.is_candidate(ow.seed, x, z)).unwrap();
    let start = std::time::Instant::now();
    let (names, reach) = build_at(&ow, trial_chambers, cx, cz);
    println!("trial chambers at {cx},{cz}: {} blocks, reach {reach}, {:?}", names.values().sum::<usize>(), start.elapsed());
    assert!(reach <= 9);
    assert!(names.keys().any(|n| n.contains("tuff")), "{names:?}");
}

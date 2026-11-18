use std::collections::HashMap;

use super::*;

/// Air world with a grass (or `ground`) platform whose top is at y = 63.
struct TestWorld {
    blocks: HashMap<(i32, i32, i32), i32>,
}

impl TestWorld {
    fn new(ground: &str) -> Self {
        let mut blocks = HashMap::new();
        let (g, d) = (id(ground), id("dirt"));
        for x in -24..=24 {
            for z in -24..=24 {
                blocks.insert((x, 63, z), g);
                for y in 58..63 {
                    blocks.insert((x, y, z), d);
                }
            }
        }
        Self { blocks }
    }
    fn count(&self, f: impl Fn(i32) -> bool) -> usize {
        self.blocks.values().filter(|&&b| f(b)).count()
    }
    fn snapshot(&self) -> Vec<((i32, i32, i32), i32)> {
        let mut v: Vec<_> = self.blocks.iter().map(|(k, v)| (*k, *v)).filter(|(_, b)| !is_air(*b)).collect();
        v.sort();
        v
    }
}

impl World for TestWorld {
    fn get(&self, x: i32, y: i32, z: i32) -> i32 {
        self.blocks.get(&(x, y, z)).copied().unwrap_or_else(air)
    }
    fn set(&mut self, x: i32, y: i32, z: i32, id: i32) {
        self.blocks.insert((x, y, z), id);
    }
    fn height(&self, _x: i32, _z: i32) -> i32 {
        64
    }
    fn seed(&self) -> i64 {
        1234
    }
}

type Gen = Box<dyn Fn(&mut TestWorld, &mut Xoroshiro) -> bool>;

fn generators() -> Vec<(&'static str, &'static str, Gen)> {
    macro_rules! g {
        ($name:expr, $ground:expr, $e:expr) => {
            ($name, $ground, Box::new(move |w: &mut TestWorld, r: &mut Xoroshiro| $e.generate(w, r, 0, 64, 0)) as Gen)
        };
    }
    vec![
        g!("fancy_oak", "grass_block", FancyOak::default()),
        g!("small_spruce", "grass_block", SmallSpruce),
        g!("big_spruce", "grass_block", BigSpruce),
        g!("mega_conifer", "grass_block", MegaConifer { canopy_height_min: 4, canopy_height_max: 9 }),
        g!("pine", "grass_block", Pine),
        g!("savanna", "grass_block", Savanna),
        g!("savanna_worldgen", "grass_block", SavannaWorldgen),
        g!("dark_oak", "grass_block", DarkOak),
        g!("roofed", "grass_block", RoofedWorldgen { vines: true }),
        g!("jungle", "grass_block", Jungle { min_tree_height: 4, max_tree_height: 7 }),
        g!("jungle_big", "grass_block", JungleBig { base_height: 10, extra_random_height: 20 }),
        g!("mega_jungle", "grass_block", MegaJungle),
        g!("jungle_bush", "grass_block", JungleBush),
        g!("mangrove", "mud", Mangrove::new(true)),
        g!("mangrove_short", "mud", Mangrove { tall: false, with_bee_nest: true, bee_count: 3 }),
        g!("cherry", "grass_block", Cherry),
        g!("pale_oak", "grass_block", PaleOak { try_creaking_heart: true }),
        g!("small_pale_oak", "grass_block", SmallPaleOak { min_tree_height: 4, max_tree_height: 3 }),
        g!("swamp_oak", "grass_block", SwampOak { min_tree_height: 5, max_tree_height: 8 }),
        g!("azalea", "grass_block", Azalea),
        g!("legacy_oak", "grass_block", LegacyTree::new(LegacyKind::Oak)),
        g!("legacy_birch", "grass_block", LegacyTree::new(LegacyKind::Birch)),
        g!("legacy_tall_birch", "grass_block", LegacyTree::new(LegacyKind::TallBirch)),
        g!("legacy_spruce", "grass_block", LegacyTree::new(LegacyKind::Spruce)),
        g!("legacy_jungle", "grass_block", LegacyTree::new(LegacyKind::Jungle)),
        g!("legacy_big_spruce", "grass_block", LegacyTree::new(LegacyKind::BigSpruce { leaf_start_height_multiplier: 0.25, base_leaf_radius: 4 })),
        g!("legacy_dark_oak", "grass_block", LegacyTree { kind: LegacyKind::DarkOak { leaf_start_height_multiplier: 0.2, base_leaf_radius: 3 }, tree_height: 0 }),
    ]
}

#[test]
fn every_tree_grows_logs_and_leaves_deterministically() {
    for (name, ground, f) in generators() {
        let mut ok = 0;
        for seed in 0..8i64 {
            let mut w = TestWorld::new(ground);
            if !f(&mut w, &mut Xoroshiro::new(seed)) {
                continue;
            }
            ok += 1;
            let logs = w.count(blocks::is_log);
            let leaves = w.count(is_leaves);
            // PNX's small pale oak never places its trunk (metaWood is unused).
            assert!((logs > 0 || name == "small_pale_oak") && leaves > 0, "{name} seed {seed}: {logs} logs, {leaves} leaves");
            let mut w2 = TestWorld::new(ground);
            f(&mut w2, &mut Xoroshiro::new(seed));
            assert!(w.snapshot() == w2.snapshot(), "{name} seed {seed} not deterministic");
        }
        assert!(ok > 0, "{name} never generated");
    }
}

#[test]
fn big_mushroom_fallen_tree_bee_nest_grass() {
    for kind in [Some(MushroomType::Red), Some(MushroomType::Brown), None] {
        let mut w = TestWorld::new("mycelium");
        assert!(BigMushroom { kind }.generate(&mut w, &mut Xoroshiro::new(5), 0, 64, 0));
        assert!(w.count(|b| nm(b).ends_with("mushroom_block")) > 10);
    }
    for wood in [Wood::Oak, Wood::Birch, Wood::Spruce, Wood::Jungle, Wood::DarkOak, Wood::Acacia, Wood::Cherry, Wood::Mangrove, Wood::PaleOak] {
        let mut placed = 0;
        for seed in 0..8 {
            let mut w = TestWorld::new("grass_block");
            assert!(FallenTree::new(wood).generate(&mut w, &mut Xoroshiro::new(seed), 0, 64, 0));
            placed = placed.max(w.count(blocks::is_log));
        }
        assert!(placed > 3, "{wood:?} fallen log never placed");
    }
    let mut w = TestWorld::new("grass_block");
    assert!(LegacyTree::new(LegacyKind::Birch).generate(&mut w, &mut Xoroshiro::new(3), 0, 64, 0));
    assert!(bee_nest_place(&mut w, &mut Xoroshiro::new(3), 0, 64, 0));
    assert_eq!(w.count(|b| nm(b) == "bee_nest"), 1);
    legacy_tall_grass(&mut w, &mut Xoroshiro::new(9), 10, 63, 10);
    assert!(w.count(|b| nm(b) != "grass_block" && blocks::is_plant(b)) > 0);
    legacy_grow_tree(&mut w, &mut Xoroshiro::new(1), 15, 64, -15, Wood::Spruce, false);
    assert!(!bee_nest_has_nearby_flower(&w, 100, 64, 100));
}

use super::super::blocks;
use super::placement::Coord;
use crate::overworld::{Chunk, Overworld};

#[test]
fn block_names_exist() {
    let names = [
        "short_grass", "fern", "tall_grass", "large_fern", "bush", "melon_block", "pumpkin", "sweet_berry_bush", "closed_eyeblossom", "red_shrub", "mossy_cobblestone", "bamboo", "podzol",
        "deadbush", "red_mushroom", "brown_mushroom", "short_dry_grass", "tall_dry_grass", "firefly_bush", "blue_orchid", "reeds", "pink_petals", "pale_moss_block", "pale_moss_carpet", "cactus",
        "cactus_flower", "sunflower", "azure_bluet", "cornflower", "dandelion", "oxeye_daisy", "poppy", "orange_tulip", "pink_tulip", "red_tulip", "white_tulip", "allium", "lily_of_the_valley",
        "lilac", "peony", "rose_bush", "wildflowers", "seagrass", "kelp", "waterlily", "sea_pickle", "packed_ice", "blue_ice", "snow", "moss_block", "cave_vines",
        "cave_vines_body_with_berries", "cave_vines_head_with_berries", "spore_blossom", "clay", "big_dripleaf", "small_dripleaf_block", "flowering_azalea", "azalea", "moss_carpet",
    ];
    for n in names {
        blocks::id(n);
    }
    for c in super::water::CORAL {
        for suffix in ["coral_block", "coral_fan", "coral_wall_fan"] {
            blocks::id(&format!("{c}_{suffix}"));
        }
    }
    // states the features set
    for st in [
        super::double("sunflower", true),
        blocks::with(blocks::id("seagrass"), &[("sea_grass_type", blocks::s("double_top"))]),
        blocks::with(blocks::id("kelp"), &[("kelp_age", blocks::i(25))]),
        blocks::with(blocks::id("reeds"), &[("age", blocks::i(15))]),
    ] {
        assert!(crate::blocks::entry(st).is_some());
    }
}

#[test]
fn scatter_coordinates() {
    let mut r = super::Xoroshiro::new(1);
    for _ in 0..1000 {
        assert!((-6..=6).contains(&Coord::triangle(-6, 6).sample(&mut r)));
        assert!((-2..=2).contains(&Coord::gaussian(-2, 2).sample(&mut r)));
    }
    assert!(matches!(Coord::gaussian(3, 4), Coord::Fixed(3)));
}

fn count(c: &Chunk, names: &[&str]) -> usize {
    let mut n = 0;
    for x in 0..16 {
        for z in 0..16 {
            for y in -64..320 {
                if names.contains(&blocks::short(c.get(x, y, z))) {
                    n += 1;
                }
            }
        }
    }
    n
}

/// First chunk (on a coarse grid) whose centre surface biome is in `want`, decorated.
fn find(w: &Overworld, want: &[u16]) -> Option<Chunk> {
    for r in 0..12i32 {
        for cx in -r..=r {
            for cz in -r..=r {
                if cx.abs().max(cz.abs()) != r {
                    continue;
                }
                let (cx, cz) = (cx * 6, cz * 6);
                if want.contains(&w.generate_terrain(cx, cz).surface_biome(8, 8)) {
                    return Some(w.generate(cx, cz));
                }
            }
        }
    }
    None
}

#[test]
fn plants_and_sea_plants_appear() {
    let w = Overworld::new(12345);
    let land = find(&w, &[1, 4, 129, 132, 27, 35]).expect("a grassy chunk");
    assert!(count(&land, &["short_grass", "tall_grass", "dandelion", "poppy", "fern"]) > 0, "no grass/flowers on land");
    let sea = find(&w, &[0, 24, 42, 43, 44, 45]).expect("an ocean chunk");
    assert!(count(&sea, &["seagrass", "kelp"]) > 0, "no seagrass/kelp in the ocean");
}

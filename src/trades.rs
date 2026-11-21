// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
// from PNX Profession + Profession{Farmer,Fisherman,Shepherd,Fletcher,Librarian,Cartographer,
// Cleric,Armor,Weapon,Tool,Butcher,Leather,Mason}, TradeRecipeBuildUtils, Enchantment,
// DyeColor/BlockColor, EntityVillagerV2.Clothing
//! Villager professions and trade tables.
#![allow(dead_code)]

#[derive(Clone, Debug, PartialEq)]
pub struct Stack {
    pub name: String,
    pub count: i8,
    pub damage: i16,
    /// (id, level), e.g. librarian books or enchanted tools
    pub enchants: Vec<(i16, i16)>,
    /// extra bits PNX sets, like leather `customColor`, as (key, i32)
    pub extra: Vec<(String, i32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Trade {
    pub buy_a: Stack,
    pub buy_b: Option<Stack>,
    pub sell: Stack,
    pub max_uses: i32,
    pub reward_exp: bool,
    /// villager level 1..=5 (PNX setTier)
    pub tier: u8,
    pub trader_exp: i32,
    pub price_multiplier_a: f32,
    pub price_multiplier_b: f32,
}

pub struct Profession {
    pub index: i32,
    pub block: &'static str,
    pub name: &'static str,
    pub work_sound: &'static str,
}

const PROFESSIONS: [Profession; 13] = [
    Profession { index: 1, block: "minecraft:composter", name: "entity.villager.farmer", work_sound: "block.composter.fill" },
    Profession { index: 2, block: "minecraft:barrel", name: "entity.villager.fisherman", work_sound: "block.barrel.open" },
    Profession { index: 3, block: "minecraft:loom", name: "entity.villager.shepherd", work_sound: "block.loom.use" },
    Profession { index: 4, block: "minecraft:fletching_table", name: "entity.villager.fletcher", work_sound: "dig.wood" },
    Profession { index: 5, block: "minecraft:lectern", name: "entity.villager.librarian", work_sound: "item.book.put" },
    Profession { index: 6, block: "minecraft:cartography_table", name: "entity.villager.cartographer", work_sound: "block.cartography_table.use" },
    Profession { index: 7, block: "minecraft:brewing_stand", name: "entity.villager.cleric", work_sound: "random.potion.brewed" },
    Profession { index: 8, block: "minecraft:blast_furnace", name: "entity.villager.armor", work_sound: "block.blastfurnace.fire_crackle" },
    Profession { index: 9, block: "minecraft:grindstone", name: "entity.villager.weapon", work_sound: "block.grindstone.use" },
    Profession { index: 10, block: "minecraft:smithing_table", name: "entity.villager.tool", work_sound: "smithing_table.use" },
    Profession { index: 11, block: "minecraft:smoker", name: "entity.villager.butcher", work_sound: "block.smoker.smoke" },
    Profession { index: 12, block: "minecraft:cauldron", name: "entity.villager.leather", work_sound: "bucket.fill_water" },
    Profession { index: 13, block: "minecraft:stonecutter_block", name: "entity.villager.mason", work_sound: "block.stonecutter.use" },
];

pub fn professions() -> &'static [Profession] {
    &PROFESSIONS
}

pub fn profession_for_block(block_name: &str) -> Option<&'static Profession> {
    PROFESSIONS.iter().find(|p| p.block == block_name)
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clothing { Plains, Desert, Jungle, Savanna, Snow, Swamp, Taiga }

impl Clothing {
    pub fn from_index(i: i32) -> Clothing {
        use Clothing::*;
        [Plains, Desert, Jungle, Savanna, Snow, Swamp, Taiga].get(i as usize).copied().filter(|_| i >= 0).unwrap_or(Plains)
    }
    /// Same order PNX checks biome tags: desert|mesa, jungle, savanna, frozen, swamp, taiga|extreme_hills.
    // TODO: tags are hand-mapped from Bedrock biome ids, use a real biome tag registry once we have one
    pub fn from_biome(biome_id: u16) -> Clothing {
        match biome_id {
            2 | 17 | 130 | 37 | 38 | 39 | 165 | 166 | 167 => Clothing::Desert,
            21 | 22 | 23 | 48 | 49 | 149 | 151 => Clothing::Jungle,
            35 | 36 | 163 | 164 => Clothing::Savanna,
            10 | 11 | 12 | 13 | 46 | 47 | 140 | 182 | 183 | 184 | 185 => Clothing::Snow,
            6 | 134 => Clothing::Swamp,
            3 | 5 | 19 | 20 | 30 | 31 | 32 | 33 | 34 | 131 | 133 | 158 | 160 | 161 | 162 => Clothing::Taiga,
            _ => Clothing::Plains,
        }
    }
}

/// Same as java.util.Random.
pub struct JavaRandom(u64);

impl JavaRandom {
    const MUL: u64 = 0x5DEECE66D;
    const MASK: u64 = (1 << 48) - 1;
    pub fn new(seed: i64) -> Self {
        JavaRandom((seed as u64 ^ Self::MUL) & Self::MASK)
    }
    fn next(&mut self, bits: u32) -> i32 {
        self.0 = self.0.wrapping_mul(Self::MUL).wrapping_add(0xB) & Self::MASK;
        ((self.0 as i64) >> (48 - bits)) as i32
    }
    pub fn next_int(&mut self) -> i32 {
        self.next(32)
    }
    pub fn next_int_bound(&mut self, bound: i32) -> i32 {
        assert!(bound > 0);
        if bound.isolate_lowest_one() == bound {
            return ((bound as i64 * self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let val = bits % bound;
            if bits.wrapping_sub(val).wrapping_add(bound - 1) >= 0 {
                return val;
            }
        }
    }
    pub fn next_boolean(&mut self) -> bool {
        self.next(1) != 0
    }
    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / (1 << 24) as f32
    }
    pub fn next_double(&mut self) -> f64 {
        let hi = (self.next(26) as i64) << 27;
        (hi + self.next(27) as i64) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

// helpers like Item.get / TradeRecipeBuildUtils

fn it(name: &str, damage: i16, count: i8) -> Stack {
    Stack { name: format!("minecraft:{name}"), count, damage, enchants: vec![], extra: vec![] }
}
fn one(name: &str) -> Stack {
    it(name, 0, 1)
}
fn em(count: i8) -> Stack {
    it("emerald", 0, count)
}
fn ench(mut s: Stack, e: (i16, i16)) -> Stack {
    s.enchants.push(e);
    s
}

struct T(Vec<Trade>);
impl T {
    fn add(&mut self, a: Stack, b: Option<Stack>, sell: Stack, max_uses: i32, tier: u8, texp: i32, pma: f32, pmb: f32) {
        self.0.push(Trade {
            buy_a: a, buy_b: b, sell, max_uses, reward_exp: true, tier, trader_exp: texp,
            price_multiplier_a: pma, price_multiplier_b: pmb,
        });
    }
    /// `of(buyA, sell)`, PNX ignores priceMultiplierB here
    fn t(&mut self, a: Stack, sell: Stack, max_uses: i32, tier: u8, texp: i32, pma: f32) {
        self.add(a, None, sell, max_uses, tier, texp, pma, 0.0);
    }
}

// PNX Enchantment.ID_*, index is the id (registry iterates in id order)
const PROTECTION: i16 = 0; const FIRE_PROT: i16 = 1; const BLAST_PROT: i16 = 3; const PROJ_PROT: i16 = 4;
const THORNS: i16 = 5; const SHARPNESS: i16 = 9; const SMITE: i16 = 10; const BANE: i16 = 11;
const FIRE_ASPECT: i16 = 13; const LOOTING: i16 = 14; const EFFICIENCY: i16 = 15; const SILK_TOUCH: i16 = 16;
const UNBREAKING: i16 = 17; const FORTUNE: i16 = 18; const POWER: i16 = 19; const PUNCH: i16 = 20;
const FLAME: i16 = 21; const INFINITY: i16 = 22; const LUCK_OF_SEA: i16 = 23; const LURE: i16 = 24;
const VANISHING: i16 = 28; const MULTISHOT: i16 = 33; const PIERCING: i16 = 34; const QUICK_CHARGE: i16 = 35;

/// max level per enchant id 0..=41
const MAX_LEVEL: [i32; 42] = [
    4, 4, 4, 4, 4, 3, 3, 3, 1, 5, 5, 5, 2, 2, 3, 5, 1, 3, 3, 5, 2, 1, 1, 3, 3, 2, 1, 1, 1, 5, 3, 3, 1, 1, 4, 3, 3, 3, 3, 5, 4, 3,
];

/// `getEnchantment(ids[nextInt(len)])` then `setLevel(1 + nextInt(max))`.
fn pick(r: &mut JavaRandom, ids: &[i16]) -> (i16, i16) {
    let id = ids[r.next_int_bound(ids.len() as i32) as usize];
    (id, (1 + r.next_int_bound(MAX_LEVEL[id as usize])) as i16)
}

const DYES: [&str; 16] = [
    "black", "red", "green", "brown", "blue", "purple", "cyan", "light_gray", "gray", "pink", "lime", "yellow",
    "light_blue", "magenta", "orange", "white",
];
/// DyeColor.getColor(), for leather customColor
const DYE_RGB: [i32; 16] = [
    0x191919, 0x993333, 0x667F33, 0x664C33, 0x334CB2, 0x7F3FB2, 0x4C7F99, 0x999999, 0x4C4C4C, 0xF27FA5, 0x7FCC19,
    0xE5E533, 0x6699D8, 0xB24CD8, 0xD87F33, 0xFFFFFF,
];
const BLACK: usize = 0; const RED: usize = 1; const GREEN: usize = 2; const BROWN: usize = 3; const BLUE: usize = 4;
const PURPLE: usize = 5; const CYAN: usize = 6; const LIGHT_GRAY: usize = 7; const GRAY: usize = 8; const PINK: usize = 9;
const LIME: usize = 10; const YELLOW: usize = 11; const LIGHT_BLUE: usize = 12; const MAGENTA: usize = 13;
const ORANGE: usize = 14; const WHITE: usize = 15;

fn dyed(r: &mut JavaRandom, name: &str, suffix: &str) -> Stack {
    let _ = name;
    one(&format!("{}_{suffix}", DYES[r.next_int_bound(16) as usize]))
}
fn leather(r: &mut JavaRandom, piece: &str) -> Stack {
    let mut s = one(piece);
    s.extra.push(("customColor".into(), DYE_RGB[r.next_int_bound(16) as usize]));
    s
}

/// PNX buildTrades(seed, clothing). Same Random call order, so a villager seed gives the same offers as PNX.
pub fn trades(profession_index: i32, seed: i32, clothing: Clothing) -> Vec<Trade> {
    let r = &mut JavaRandom::new(seed as i64);
    let mut t = T(Vec::new());
    match profession_index {
        1 => farmer(r, &mut t),
        2 => fisherman(r, &mut t, clothing),
        3 => shepherd(r, &mut t),
        4 => fletcher(r, &mut t, seed),
        5 => librarian(r, &mut t),
        6 => cartographer(r, &mut t),
        7 => cleric(r, &mut t),
        8 => armor(r, &mut t),
        9 => weapon(r, &mut t),
        10 => tool(r, &mut t),
        11 => butcher(r, &mut t),
        12 => leatherworker(r, &mut t),
        13 => mason(r, &mut t),
        WANDERING => wandering(r, &mut t),
        _ => {}
    }
    t.0
}

/// The wandering trader's "profession" (not a villager job).
pub const WANDERING: i32 = 100;

/// Vanilla wandering trader: five of the common offers and one rare one, all for emeralds.
/// (PNX has no table for it; these are the Bedrock offers.)
fn wandering(r: &mut JavaRandom, t: &mut T) {
    const COMMON: &[(&str, i8, i8, i32)] = &[
        ("fern", 1, 1, 12), ("sugar_cane", 1, 1, 8), ("pumpkin", 1, 1, 4), ("kelp", 3, 1, 12), ("cactus", 3, 1, 8),
        ("dandelion", 1, 1, 12), ("poppy", 1, 1, 12), ("blue_orchid", 1, 1, 8), ("allium", 1, 1, 12), ("azure_bluet", 1, 1, 12),
        ("red_tulip", 1, 1, 12), ("orange_tulip", 1, 1, 12), ("white_tulip", 1, 1, 12), ("pink_tulip", 1, 1, 12),
        ("oxeye_daisy", 1, 1, 12), ("cornflower", 1, 1, 12), ("lily_of_the_valley", 1, 1, 7), ("wheat_seeds", 1, 1, 12),
        ("beetroot_seeds", 1, 1, 12), ("pumpkin_seeds", 1, 1, 12), ("melon_seeds", 1, 1, 12), ("oak_sapling", 5, 1, 8),
        ("spruce_sapling", 5, 1, 8), ("birch_sapling", 5, 1, 8), ("jungle_sapling", 5, 1, 8), ("acacia_sapling", 5, 1, 8),
        ("dark_oak_sapling", 5, 1, 8), ("cherry_sapling", 5, 1, 8), ("mangrove_propagule", 5, 1, 8), ("red_dye", 1, 3, 12),
        ("white_dye", 1, 3, 12), ("blue_dye", 1, 3, 12), ("pink_dye", 1, 3, 12), ("black_dye", 1, 3, 12), ("green_dye", 1, 3, 12),
        ("light_gray_dye", 1, 3, 12), ("magenta_dye", 1, 3, 12), ("yellow_dye", 1, 3, 12), ("gray_dye", 1, 3, 12),
        ("purple_dye", 1, 3, 12), ("light_blue_dye", 1, 3, 12), ("lime_dye", 1, 3, 12), ("orange_dye", 1, 3, 12),
        ("brown_dye", 1, 3, 12), ("cyan_dye", 1, 3, 12), ("brain_coral_block", 3, 1, 8), ("bubble_coral_block", 3, 1, 8),
        ("fire_coral_block", 3, 1, 8), ("horn_coral_block", 3, 1, 8), ("tube_coral_block", 3, 1, 8), ("vine", 1, 1, 12),
        ("brown_mushroom", 1, 1, 12), ("red_mushroom", 1, 1, 12), ("waterlily", 1, 2, 5), ("small_dripleaf_block", 1, 2, 5),
        ("sand", 1, 8, 8), ("red_sand", 1, 4, 6), ("pointed_dripstone", 1, 2, 5), ("dirt_with_roots", 1, 2, 5), ("moss_block", 1, 2, 5),
    ];
    const RARE: &[(&str, i8, i8, i32)] = &[
        ("tropical_fish_bucket", 5, 1, 4), ("pufferfish_bucket", 5, 1, 4), ("packed_ice", 3, 1, 6), ("blue_ice", 6, 1, 6),
        ("gunpowder", 1, 1, 8), ("podzol", 3, 3, 6), ("nautilus_shell", 5, 1, 5), ("slime_ball", 4, 1, 5),
    ];
    let mut picked: Vec<usize> = Vec::new();
    while picked.len() < 5 {
        let i = r.next_int_bound(COMMON.len() as i32) as usize;
        if !picked.contains(&i) {
            picked.push(i);
        }
    }
    for i in picked {
        let (item, price, n, uses) = COMMON[i];
        t.t(em(price), it(item, 0, n), uses, 1, 1, 0.05);
    }
    let (item, price, n, uses) = RARE[r.next_int_bound(RARE.len() as i32) as usize];
    t.t(em(price), it(item, 0, n), uses, 1, 1, 0.05);
}

fn farmer(r: &mut JavaRandom, t: &mut T) {
    let (crop, n) = [("wheat", 20), ("beetroot", 15), ("carrot", 22), ("potato", 26)][r.next_int_bound(4) as usize];
    t.t(it(crop, 0, n), em(1), 16, 1, 2, 0.05);
    t.t(em(1), it("bread", 0, 6), 16, 1, 1, 0.05);
    t.t(it("pumpkin", 0, 6), em(1), 12, 2, 10, 0.05);
    if r.next_boolean() {
        t.t(em(1), it("pumpkin_pie", 0, 4), 12, 2, 5, 0.05);
    } else {
        t.t(em(1), it("apple", 0, 4), 16, 2, 5, 0.05);
    }
    t.t(it("melon_block", 0, 4), em(1), 12, 3, 20, 0.05);
    t.t(em(3), it("cookie", 0, 18), 12, 3, 10, 0.05);
    let stew = r.next_int_bound(6) as i16;
    t.t(em(1), it("suspicious_stew", stew, 1), 12, 4, 15, 0.05);
    t.t(em(1), one("cake"), 12, 4, 15, 0.05);
    t.t(em(3), it("golden_carrot", 0, 3), 12, 5, 30, 0.05);
    t.t(em(4), it("glistering_melon_slice", 0, 3), 12, 3, 30, 0.05);
}

fn fisherman(r: &mut JavaRandom, t: &mut T, clothing: Clothing) {
    let id = [UNBREAKING, LURE, LUCK_OF_SEA][r.next_int_bound(3) as usize];
    let lvl = (r.next_int_bound(3) + 1).min(MAX_LEVEL[id as usize]) as i16;
    let rod = ench(one("fishing_rod"), (id, lvl));
    if r.next_boolean() {
        t.t(it("string", 0, 20), em(1), 16, 1, 2, 0.05);
    } else {
        t.t(it("coal", 0, 10), em(1), 16, 1, 2, 0.05);
    }
    if r.next_boolean() {
        t.t(em(3), one("cod_bucket"), 16, 1, 1, 0.05);
    } else {
        t.add(em(1), Some(it("cod", 0, 6)), it("cooked_cod", 0, 6), 16, 1, 1, 0.05, 0.0);
    }
    t.t(it("cod", 0, 15), em(1), 16, 2, 10, 0.05);
    if r.next_boolean() {
        t.t(em(2), one("campfire"), 12, 2, 5, 0.05);
    } else {
        t.add(em(1), Some(it("salmon", 0, 6)), it("cooked_salmon", 0, 6), 16, 2, 5, 0.05, 0.0);
    }
    t.t(it("salmon", 0, 13), em(1), 16, 3, 20, 0.05);
    let price = 8 + r.next_int_bound(15);
    t.t(em(price as i8), rod, 3, 3, 10, 0.2);
    t.t(it("tropical_fish", 0, 6), em(1), 16, 4, 30, 0.05);
    t.t(it("pufferfish", 0, 4), em(1), 16, 5, 30, 0.05);
    // PNX calls it minecraft:boat, Bedrock wants oak_boat
    let boat = match clothing {
        Clothing::Desert | Clothing::Jungle => "jungle_boat",
        Clothing::Savanna => "acacia_boat",
        Clothing::Snow | Clothing::Taiga => "spruce_boat",
        Clothing::Swamp => "dark_oak_boat",
        Clothing::Plains => "oak_boat",
    };
    t.t(one(boat), em(1), 16, 5, 30, 0.05);
}

fn shepherd(r: &mut JavaRandom, t: &mut T) {
    let wool = [WHITE, BROWN, BLACK, GRAY][r.next_int_bound(4) as usize];
    t.t(it(&format!("{}_wool", DYES[wool]), 0, 18), em(1), 16, 1, 2, 0.05);
    t.t(em(2), one("shears"), 16, 1, 1, 0.05);
    let d = [WHITE, LIGHT_BLUE, LIME, BLACK, GRAY][r.next_int_bound(5) as usize];
    t.t(it(&format!("{}_dye", DYES[d]), 0, 12), em(1), 16, 2, 10, 0.05);
    if r.next_boolean() {
        // PNX uses getName() here ("light gray_wool"), the underscore id is the real one
        t.t(em(1), dyed(r, "", "wool"), 16, 2, 5, 0.05);
    } else {
        t.t(em(1), dyed(r, "", "carpet"), 16, 2, 5, 0.05);
    }
    let d = [YELLOW, LIGHT_GRAY, ORANGE, RED, PINK][r.next_int_bound(5) as usize];
    t.t(it(&format!("{}_dye", DYES[d]), 0, 12), em(1), 16, 3, 20, 0.05);
    t.t(em(3), dyed(r, "", "bed"), 12, 3, 10, 0.05);
    let d = [BROWN, PURPLE, BLUE, GREEN, MAGENTA, CYAN][r.next_int_bound(6) as usize];
    t.t(it(&format!("{}_dye", DYES[d]), 0, 12), em(1), 16, 4, 30, 0.05);
    let banner = r.next_int_bound(16) as i16;
    t.t(em(1), it("banner", banner, 1), 12, 4, 15, 0.05);
    t.t(em(2), it("painting", 0, 3), 12, 5, 30, 0.05);
}

fn fletcher(r: &mut JavaRandom, t: &mut T, seed: i32) {
    let bow = ench(one("bow"), pick(r, &[FLAME, INFINITY, PUNCH, POWER, UNBREAKING]));
    let crossbow = ench(one("crossbow"), pick(r, &[MULTISHOT, PIERCING, QUICK_CHARGE, UNBREAKING]));
    t.t(it("stick", 0, 32), em(1), 16, 1, 2, 0.05);
    if r.next_boolean() {
        t.t(em(1), it("arrow", 0, 16), 12, 1, 1, 0.05);
    } else {
        t.add(em(1), Some(it("gravel", 0, 10)), it("flint", 0, 10), 12, 1, 1, 0.05, 0.0);
    }
    t.t(it("flint", 0, 26), em(1), 12, 2, 10, 0.05);
    t.t(em(2), one("bow"), 12, 2, 5, 0.05);
    t.t(it("string", 0, 14), em(1), 16, 3, 20, 0.05);
    t.t(em(3), one("crossbow"), 12, 3, 10, 0.05);
    t.t(it("feather", 0, 24), em(1), 16, 4, 30, 0.05);
    let price = 7 + r.next_int_bound(15);
    t.t(em(price as i8), bow, 3, 4, 15, 0.05);
    t.t(it("tripwire_hook", 0, 8), em(1), 12, 5, 30, 0.05);
    if r.next_boolean() {
        let price = 8 + r.next_int_bound(15);
        t.t(em(price as i8), crossbow, 3, 5, 15, 0.05);
    } else {
        // TODO: PNX picks the tipped arrow potion with unseeded Utils.rand(6, 43). We derive it from the seed instead.
        let potion = 6 + JavaRandom::new(seed as i64 ^ 0x07A1_10E5).next_int_bound(38);
        t.add(em(2), Some(it("arrow", 0, 5)), it("arrow", potion as i16, 5), 12, 5, 30, 0.05, 0.0);
    }
}

fn librarian(r: &mut JavaRandom, t: &mut T) {
    let all: Vec<i16> = (0..42).collect();
    let mut books: Vec<Stack> = (0..4).map(|_| ench(one("enchanted_book"), pick(r, &all))).collect();
    let mut book = || books.remove(0);
    t.t(it("paper", 0, 24), em(1), 16, 1, 2, 0.05);
    let b = book();
    if r.next_boolean() {
        t.t(em(9), one("bookshelf"), 12, 1, 1, 0.05);
    } else {
        let p = 5 + r.next_int_bound(60);
        t.add(em(p as i8), Some(one("book")), b, 12, 1, 1, 0.2, 0.0);
    }
    t.t(one("book"), em(1), 12, 2, 10, 0.05);
    let b = book();
    if r.next_boolean() {
        t.t(em(1), one("lantern"), 12, 2, 5, 0.05);
    } else {
        let p = 5 + r.next_int_bound(60);
        t.add(em(p as i8), Some(one("book")), b, 12, 2, 5, 0.2, 0.0);
    }
    t.t(it("ink_sac", 0, 5), em(1), 12, 3, 20, 0.05);
    let b = book();
    if r.next_boolean() {
        t.t(em(1), it("glass", 0, 4), 12, 3, 10, 0.05);
    } else {
        let p = 5 + r.next_int_bound(60);
        t.add(em(p as i8), Some(one("book")), b, 12, 3, 10, 0.2, 0.0);
    }
    t.add(one("writable_book"), Some(one("writable_book")), em(1), 12, 4, 30, 0.05, 0.0);
    let b = book();
    match r.next_int_bound(3) {
        0 => t.t(em(5), one("clock"), 12, 4, 15, 0.05),
        1 => t.t(em(4), one("compass"), 12, 4, 15, 0.05),
        _ => {
            let p = 5 + r.next_int_bound(60);
            t.add(em(p as i8), Some(one("book")), b, 12, 4, 15, 0.2, 0.0);
        }
    }
    let candle = if r.next_int_bound(4) == 3 { "yellow_candle" } else { "red_candle" };
    t.t(em(3), one(candle), 12, 5, 30, 0.05);
}

fn cartographer(r: &mut JavaRandom, t: &mut T) {
    // TODO: explorer maps are plain filled_map with PNX's damage (3/4/14), no structure search until we can locate structures
    t.t(it("paper", 0, 24), em(1), 16, 1, 2, 0.05);
    t.t(em(7), one("empty_map"), 12, 1, 1, 0.05);
    t.t(it("glass_pane", 0, 11), em(1), 16, 2, 10, 0.05);
    t.add(em(13), Some(one("compass")), it("filled_map", 3, 1), 12, 2, 5, 0.2, 0.2);
    t.t(one("compass"), em(1), 12, 3, 20, 0.05);
    t.add(em(14), Some(one("compass")), it("filled_map", 4, 1), 12, 3, 10, 0.2, 0.2);
    t.add(em(12), Some(one("compass")), it("filled_map", 14, 1), 12, 3, 10, 0.2, 0.2);
    t.t(em(7), one("frame"), 12, 4, 15, 0.05);
    let banner = r.next_int_bound(16) as i16;
    t.t(em(3), it("banner", banner, 1), 16, 4, 15, 0.05);
    t.t(em(8), it("banner_pattern", 7, 1), 16, 5, 30, 0.05);
}

fn cleric(r: &mut JavaRandom, t: &mut T) {
    t.t(it("rotten_flesh", 0, 32), em(2), 16, 1, 2, 0.05);
    t.t(em(1), it("redstone", 0, 2), 16, 1, 2, 0.05);
    t.t(it("gold_ingot", 0, 3), em(1), 12, 2, 10, 0.05);
    t.t(em(1), one("lapis_lazuli"), 12, 2, 5, 0.05);
    t.t(it("rabbit_foot", 0, 2), em(2), 12, 3, 20, 0.05);
    t.t(em(4), one("glowstone"), 12, 3, 10, 0.0);
    if r.next_boolean() {
        t.t(it("turtle_scute", 0, 4), em(1), 12, 4, 30, 0.05);
    } else {
        t.t(it("glass_bottle", 0, 9), em(1), 99, 4, 30, 0.05);
    }
    t.t(em(5), one("ender_pearl"), 12, 4, 15, 0.05);
    t.t(it("nether_wart", 0, 22), em(1), 12, 5, 30, 0.05);
    t.t(em(3), one("experience_bottle"), 99, 5, 30, 0.05);
}

fn armor(r: &mut JavaRandom, t: &mut T) {
    let e = [UNBREAKING, THORNS, PROTECTION, BLAST_PROT, PROJ_PROT, FIRE_PROT, VANISHING];
    let legs = ench(one("diamond_leggings"), pick(r, &e));
    let chest = ench(one("diamond_chestplate"), pick(r, &e));
    let helmet = ench(one("diamond_helmet"), pick(r, &e));
    let boots = ench(one("diamond_boots"), pick(r, &e));
    t.t(it("coal", 0, 15), em(1), 16, 1, 2, 0.05);
    match r.next_int_bound(4) {
        0 => t.t(em(4), one("iron_boots"), 12, 1, 1, 0.2),
        1 => t.t(em(7), one("iron_leggings"), 99, 1, 1, 0.2),
        2 => t.t(em(5), one("iron_helmet"), 99, 1, 1, 0.2),
        _ => t.t(em(9), one("iron_chestplate"), 99, 1, 1, 0.2),
    }
    t.t(it("iron_ingot", 0, 4), em(1), 12, 2, 10, 0.05);
    match r.next_int_bound(3) {
        0 => t.t(em(36), one("bell"), 12, 2, 5, 0.2),
        1 => t.t(em(3), one("chainmail_leggings"), 99, 2, 5, 0.2),
        _ => t.t(em(1), one("chainmail_boots"), 99, 2, 5, 0.2),
    }
    t.t(one("lava_bucket"), em(1), 12, 3, 20, 0.05);
    t.t(one("diamond"), em(1), 99, 3, 30, 0.05);
    match r.next_int_bound(3) {
        0 => t.t(em(4), one("chainmail_chestplate"), 12, 3, 10, 0.2),
        1 => t.t(em(1), one("chainmail_helmet"), 99, 3, 10, 0.2),
        _ => t.t(em(5), one("shield"), 99, 3, 10, 0.2),
    }
    if r.next_boolean() {
        let p = 19 + r.next_int_bound(15);
        t.t(em(p as i8), legs, 3, 4, 15, 0.0);
    } else {
        let p = 13 + r.next_int_bound(15);
        t.t(em(p as i8), boots, 99, 4, 15, 0.0);
    }
    if r.next_boolean() {
        let p = 21 + r.next_int_bound(15);
        t.t(em(p as i8), chest, 3, 5, 30, 0.0);
    } else {
        let p = 13 + r.next_int_bound(15);
        t.t(em(p as i8), helmet, 99, 5, 30, 0.0);
    }
}

fn weapon(r: &mut JavaRandom, t: &mut T) {
    let e = [UNBREAKING, SHARPNESS, VANISHING, SMITE, BANE, LOOTING, FIRE_ASPECT];
    let sword = ench(one("iron_sword"), pick(r, &e));
    let daxe = ench(one("diamond_axe"), pick(r, &e));
    let dsword = ench(one("diamond_sword"), pick(r, &e));
    t.t(it("coal", 0, 15), em(1), 16, 1, 2, 0.05);
    t.t(em(3), one("iron_axe"), 12, 1, 1, 0.2);
    let p = 7 + r.next_int_bound(15);
    t.t(em(p as i8), sword, 3, 1, 1, 0.05);
    t.t(it("iron_ingot", 0, 4), em(1), 12, 2, 10, 0.05);
    t.t(em(36), one("bell"), 12, 2, 5, 0.2);
    t.t(it("flint", 0, 24), em(1), 12, 3, 20, 0.05);
    t.t(one("diamond"), em(1), 12, 4, 30, 0.05);
    let p = 17 + r.next_int_bound(15);
    t.t(em(p as i8), daxe, 3, 4, 15, 0.2);
    let p = 13 + r.next_int_bound(14);
    t.t(em(p as i8), dsword, 3, 5, 30, 0.2);
}

fn tool(r: &mut JavaRandom, t: &mut T) {
    let e = [UNBREAKING, EFFICIENCY, FORTUNE, SILK_TOUCH];
    let iaxe = ench(one("iron_axe"), pick(r, &e));
    let ishovel = ench(one("iron_shovel"), pick(r, &e));
    let ipick = ench(one("iron_pickaxe"), pick(r, &e));
    let daxe = ench(one("diamond_axe"), pick(r, &e));
    let dshovel = ench(one("diamond_shovel"), pick(r, &e));
    let dpick = ench(one("diamond_pickaxe"), pick(r, &e));
    t.t(it("coal", 0, 15), em(1), 16, 1, 2, 0.05);
    let stone = ["stone_axe", "stone_shovel", "stone_pickaxe", "stone_hoe"][r.next_int_bound(4) as usize];
    t.t(em(1), one(stone), 12, 1, 1, 0.2);
    t.t(it("iron_ingot", 0, 4), em(1), 12, 2, 10, 0.05);
    t.t(em(36), one("bell"), 12, 2, 5, 0.2);
    t.t(it("flint", 0, 30), em(1), 12, 3, 20, 0.05);
    match r.next_int_bound(4) {
        0 => { let p = 6 + r.next_int_bound(15); t.t(em(p as i8), iaxe, 3, 3, 10, 0.2) }
        1 => { let p = 7 + r.next_int_bound(15); t.t(em(p as i8), ishovel, 3, 3, 10, 0.2) }
        2 => { let p = 8 + r.next_int_bound(15); t.t(em(p as i8), ipick, 3, 3, 10, 0.2) }
        _ => t.t(em(4), one("diamond_hoe"), 3, 3, 10, 0.2),
    }
    t.t(one("diamond"), em(1), 12, 4, 30, 0.05);
    if r.next_boolean() {
        let p = 17 + r.next_int_bound(15);
        t.t(em(p as i8), daxe, 3, 4, 15, 0.2);
    } else {
        let p = 10 + r.next_int_bound(15);
        t.t(em(p as i8), dshovel, 3, 4, 15, 0.2);
    }
    let p = 18 + r.next_int_bound(15);
    t.t(em(p as i8), dpick, 3, 5, 30, 0.2);
}

fn butcher(r: &mut JavaRandom, t: &mut T) {
    match r.next_int_bound(3) {
        0 => t.t(it("chicken", 0, 14), em(1), 16, 1, 2, 0.05),
        1 => t.t(it("porkchop", 0, 7), em(1), 99, 1, 2, 0.05),
        _ => t.t(it("rabbit", 0, 4), em(1), 99, 1, 2, 0.05),
    }
    t.t(em(1), one("rabbit_stew"), 12, 1, 1, 0.05);
    t.t(it("coal", 0, 15), em(1), 16, 2, 10, 0.05);
    if r.next_boolean() {
        t.t(em(1), it("cooked_porkchop", 0, 5), 16, 2, 5, 0.05);
    } else {
        t.t(em(1), it("cooked_chicken", 0, 8), 99, 2, 5, 0.05);
    }
    if r.next_boolean() {
        t.t(it("mutton", 0, 7), em(1), 16, 3, 20, 0.05);
    } else {
        t.t(it("beef", 0, 10), em(1), 99, 3, 20, 0.05);
    }
    t.t(it("dried_kelp_block", 0, 10), em(1), 12, 4, 30, 0.05);
    t.t(it("sweet_berries", 0, 10), em(1), 12, 5, 30, 0.05);
}

fn leatherworker(r: &mut JavaRandom, t: &mut T) {
    t.t(it("leather", 0, 6), em(1), 16, 1, 2, 0.05);
    if r.next_boolean() {
        t.t(em(7), leather(r, "leather_chestplate"), 12, 1, 1, 0.2);
    } else {
        t.t(em(3), leather(r, "leather_leggings"), 12, 1, 1, 0.2);
    }
    t.t(it("flint", 0, 26), em(1), 12, 2, 10, 0.05);
    if r.next_boolean() {
        t.t(em(5), leather(r, "leather_helmet"), 12, 2, 5, 0.2);
    } else {
        t.t(em(4), leather(r, "leather_boots"), 12, 2, 5, 0.2);
    }
    t.t(it("rabbit_hide", 0, 9), em(1), 12, 3, 20, 0.05);
    t.t(em(7), leather(r, "leather_chestplate"), 12, 3, 10, 0.2);
    t.t(it("turtle_helmet", 0, 4), em(1), 12, 4, 30, 0.05);
    t.t(em(6), one("leather_horse_armor"), 12, 4, 15, 0.2);
    if r.next_boolean() {
        t.t(em(6), one("saddle"), 12, 5, 30, 0.2);
    } else {
        t.t(em(5), leather(r, "leather_helmet"), 12, 5, 30, 0.2);
    }
}

fn mason(r: &mut JavaRandom, t: &mut T) {
    t.t(it("clay_ball", 0, 10), em(1), 16, 1, 2, 0.05);
    t.t(em(1), it("brick", 0, 10), 16, 1, 1, 0.05);
    t.t(it("stone", 0, 20), em(1), 16, 2, 10, 0.05);
    t.t(em(1), it("chiseled_stone_bricks", 0, 4), 16, 2, 5, 0.05);
    let s = ["granite", "andesite", "diorite"][r.next_int_bound(3) as usize];
    t.t(it(s, 0, 16), em(1), 16, 3, 20, 0.05);
    let p = ["polished_andesite", "polished_diorite", "polished_granite", "dripstone_block"][r.next_int_bound(4) as usize];
    t.t(em(1), it(p, 0, 4), 16, 3, 10, 0.05);
    t.t(it("quartz", 0, 12), em(1), 12, 4, 30, 0.05);
    if r.next_boolean() {
        t.t(em(1), dyed(r, "", "terracotta"), 12, 4, 15, 0.05);
    } else {
        t.t(em(1), dyed(r, "", "glazed_terracotta"), 12, 4, 15, 0.05);
    }
    let q = if r.next_boolean() { "quartz_pillar" } else { "quartz_block" };
    t.t(em(1), one(q), 12, 5, 30, 0.05);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trades_java_random_matches() {
        assert_eq!(JavaRandom::new(42).next_int(), -1170105035);
        assert_eq!(JavaRandom::new(0).next_int(), -1155484576);
        assert_eq!(JavaRandom::new(42).next_int_bound(10), 0);
        let mut r = JavaRandom::new(42);
        let v: Vec<i32> = (0..5).map(|_| r.next_int_bound(100)).collect();
        // worked out by hand: next(31) = (-1170105035 as u32) >> 1 = 1562431130, % 100 = 30
        assert_eq!(v, [30, 63, 48, 84, 70]);
        // nextInt(4) (power of two) on seed 0 = top 2 bits of next(31) of -1155484576
        assert_eq!(JavaRandom::new(0).next_int_bound(4), 2);
        let mut r = JavaRandom::new(42);
        let f = r.next_float();
        assert!((0.0..1.0).contains(&f));
        let d = JavaRandom::new(42).next_double();
        assert!((d - 0.7275636800328681).abs() < 1e-15, "{d}");
    }

    #[test]
    fn trades_every_profession_has_all_tiers() {
        for p in professions() {
            for seed in 0..64 {
                for c in 0..7 {
                    let ts = trades(p.index, seed, Clothing::from_index(c));
                    for tier in 1..=5 {
                        assert!(ts.iter().any(|t| t.tier == tier), "{} seed {seed} tier {tier}", p.name);
                    }
                    assert!(ts.iter().all(|t| t.buy_a.count > 0 && t.sell.name.starts_with("minecraft:")));
                }
            }
        }
    }

    #[test]
    fn trades_farmer_seed0_and_determinism() {
        let ts = trades(1, 0, Clothing::Plains);
        assert_eq!(ts.len(), 10);
        // Random(0).nextInt(4) == 2 -> carrot
        assert_eq!(ts[0].buy_a, it("carrot", 0, 22));
        assert_eq!(ts[0].sell, em(1));
        assert_eq!(ts[1].sell, it("bread", 0, 6));
        assert_eq!(ts, trades(1, 0, Clothing::Plains));
        assert_eq!(trades(5, 1234, Clothing::Taiga), trades(5, 1234, Clothing::Taiga));
        assert_eq!(profession_for_block("minecraft:lectern").unwrap().index, 5);
        assert_eq!(Clothing::from_index(99), Clothing::Plains);
        assert_eq!(Clothing::Taiga as i32, 6);
        assert_eq!(trades(2, 0, Clothing::Swamp).last().unwrap().buy_a.name, "minecraft:dark_oak_boat");
    }
}

#[cfg(test)]
mod wandering_tests {
    #[test]
    fn wandering_trader_has_five_common_and_one_rare() {
        let t = super::trades(super::WANDERING, 1234, super::Clothing::from_index(0));
        assert_eq!(t.len(), 6);
    }
}

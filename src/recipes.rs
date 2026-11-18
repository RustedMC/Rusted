//! Crafting and smelting results, read from the vanilla CraftingData we replay on join
//! so the recipe book always matches the server.

use std::collections::HashMap;
use std::sync::OnceLock;

use bedrock::protocol::v2168::types::NetworkItemStackDescriptorV2;

pub struct Recipes {
    /// recipe net id -> first output (item, count)
    crafting: HashMap<u32, (NetworkItemStackDescriptorV2, i8)>,
    /// Furnace table, input id -> output. 1.26 doesn't send furnace recipes so these are
    /// our own list of the common ones.
    furnace: HashMap<i16, (NetworkItemStackDescriptorV2, i8)>,
    /// (potion id, aux, reagent id) -> (potion id, aux)
    potion_mixes: HashMap<(i32, i32, i32), (i32, i32)>,
    /// (potion id, reagent id) -> potion id, for splash/lingering
    container_mixes: HashMap<(i32, i32), i32>,
    /// smithing transform recipes (netherite) by net id -> result
    pub smithing: HashMap<u32, NetworkItemStackDescriptorV2>,
    /// armor trim recipe net ids
    pub trims: std::collections::HashSet<u32>,
}

fn to_stack(d: &bedrock::protocol::v2168::types::NetworkItemInstanceDescriptor) -> (NetworkItemStackDescriptorV2, i8) {
    let item = NetworkItemStackDescriptorV2 {
        id: d.id as i16,
        stack_size: 1,
        aux_value: if d.aux_value == 0x7fff { 0 } else { d.aux_value },
        net_id: None,
        block_runtime_id: d.block_runtime_id as u32,
        user_data_buffer: vec![0; 10],
    };
    (item, d.stack_size.max(1) as i8)
}

pub fn get() -> &'static Recipes {
    static R: OnceLock<Recipes> = OnceLock::new();
    R.get_or_init(|| {
        let mut r = Recipes { crafting: HashMap::new(), furnace: HashMap::new(), potion_mixes: HashMap::new(), container_mixes: HashMap::new(), smithing: HashMap::new(), trims: Default::default() };
        let batch = crate::vanilla::batches().nth(8).expect("crafting data frame");
        let decoded = bedrock::network::codec::decode_packets_each::<crate::Protocol>(
            batch, None, None, usize::MAX, bedrock::network::codec::UnreadBytes::Warn,
        );
        let Some(crate::Protocol::CraftingDataPacket(p)) = decoded.ok().and_then(|mut d| d.pop()).and_then(|(_, p)| p.ok()) else {
            println!("recipes: could not read the vanilla crafting data");
            return r;
        };
        for t in &p.smithing_transform_recipes {
            r.smithing.insert(t.network_id, to_stack(&t.result).0);
        }
        for t in &p.smithing_trim_recipes {
            r.trims.insert(t.network_id);
        }
        for m in &p.potion_mixes {
            r.potion_mixes.insert((m.input_potion_id, m.input_item_aux, m.reagent_item_id), (m.output_potion_id, m.output_item_aux));
        }
        for m in &p.container_mixes {
            r.container_mixes.insert((m.input_item_id, m.reagent_item_id), m.output_item_id);
        }
        for s in p.shaped_recipes.iter().chain(&p.shaped_chemistry_recipes) {
            if let Some(out) = s.production_list.first() {
                r.crafting.insert(s.network_id, to_stack(out));
            }
        }
        for s in p.shapeless_recipes.iter().chain(&p.shapeless_chemistry_recipes).chain(&p.user_data_shapeless_recipes) {
            if let Some(out) = s.production_list.first() {
                r.crafting.insert(s.network_id, to_stack(out));
            }
        }
        for (from, to) in SMELTING {
            for from in from.split('|') {
                if let (Some(a), Some(b)) = (crate::items::item_by_name(&format!("minecraft:{from}")), crate::items::item_by_name(&format!("minecraft:{to}"))) {
                    r.furnace.insert(a.id, (b, 1));
                }
            }
        }
        r
    })
}

impl Recipes {
    /// Brewing result (PNX BrewingRecipe / ContainerRecipe).
    pub fn brewed(&self, potion: &NetworkItemStackDescriptorV2, reagent: i16) -> Option<NetworkItemStackDescriptorV2> {
        let (id, aux, r) = (potion.id as i32, potion.aux_value as i32, reagent as i32);
        if let Some(&(oid, oaux)) = self.potion_mixes.get(&(id, aux, r)) {
            return Some(NetworkItemStackDescriptorV2 { id: oid as i16, aux_value: oaux as u32, ..potion.clone() });
        }
        self.container_mixes.get(&(id, r)).map(|&oid| NetworkItemStackDescriptorV2 { id: oid as i16, ..potion.clone() })
    }

    pub fn crafted(&self, network_id: u32) -> Option<(NetworkItemStackDescriptorV2, i8)> {
        self.crafting.get(&network_id).cloned()
    }

    pub fn smelted(&self, input: &NetworkItemStackDescriptorV2) -> Option<(NetworkItemStackDescriptorV2, i8)> {
        self.furnace.get(&input.id).cloned()
    }
}

/// inputs (|-separated) -> output
const SMELTING: &[(&str, &str)] = &[
    ("raw_iron|iron_ore|deepslate_iron_ore", "iron_ingot"),
    ("raw_gold|gold_ore|deepslate_gold_ore|nether_gold_ore", "gold_ingot"),
    ("raw_copper|copper_ore|deepslate_copper_ore", "copper_ingot"),
    ("coal_ore|deepslate_coal_ore", "coal"),
    ("diamond_ore|deepslate_diamond_ore", "diamond"),
    ("emerald_ore|deepslate_emerald_ore", "emerald"),
    ("lapis_ore|deepslate_lapis_ore", "lapis_lazuli"),
    ("redstone_ore|deepslate_redstone_ore", "redstone"),
    ("quartz_ore", "quartz"),
    ("ancient_debris", "netherite_scrap"),
    ("sand|red_sand", "glass"),
    ("cobblestone", "stone"),
    ("stone", "smooth_stone"),
    ("cobbled_deepslate", "deepslate"),
    ("sandstone", "smooth_sandstone"),
    ("red_sandstone", "smooth_red_sandstone"),
    ("quartz_block", "smooth_quartz"),
    ("clay_ball", "brick"),
    ("clay", "hardened_clay"),
    ("netherrack", "netherbrick"),
    ("stone_bricks", "cracked_stone_bricks"),
    ("oak_log|spruce_log|birch_log|jungle_log|acacia_log|dark_oak_log|mangrove_log|cherry_log|pale_oak_log|oak_wood|spruce_wood|birch_wood|jungle_wood|acacia_wood|dark_oak_wood", "charcoal"),
    ("beef", "cooked_beef"),
    ("porkchop", "cooked_porkchop"),
    ("chicken", "cooked_chicken"),
    ("mutton", "cooked_mutton"),
    ("rabbit", "cooked_rabbit"),
    ("cod", "cooked_cod"),
    ("salmon", "cooked_salmon"),
    ("potato", "baked_potato"),
    ("kelp", "dried_kelp"),
    ("cactus", "green_dye"),
    ("sea_pickle", "lime_dye"),
    ("wet_sponge", "sponge"),
    ("chorus_fruit", "popped_chorus_fruit"),
];

/// Fuel burn time in seconds, vanilla values.
pub fn fuel_secs(name: &str) -> Option<f32> {
    let n = name.strip_prefix("minecraft:").unwrap_or(name);
    Some(match n {
        "lava_bucket" => 1000.0,
        "coal_block" => 800.0,
        "dried_kelp_block" => 200.0,
        "blaze_rod" => 120.0,
        "coal" | "charcoal" => 80.0,
        "stick" | "bamboo" => 5.0,
        n if n.ends_with("_log") || n.ends_with("_planks") || n.ends_with("_wood") || n.ends_with("_stem") || n.ends_with("_hyphae") => 15.0,
        n if n.starts_with("wooden_") || n.ends_with("_sapling") || n.ends_with("_slab") && n.contains("wood") => 10.0,
        "crafting_table" | "bookshelf" | "chest" | "trapped_chest" | "barrel" | "ladder" | "fence" | "jukebox" | "note_block" => 15.0,
        n if n.ends_with("_fence") || n.ends_with("_fence_gate") || n.ends_with("_stairs") && !n.contains("stone") => 15.0,
        n if n.ends_with("_wool") || n.ends_with("_carpet") => 5.0,
        _ => return None,
    })
}

pub const SMELT_SECS: f32 = 10.0;

/// Slots are 0 input, 1 fuel, 2 output. Updated lazily, `catch_up` runs the time since
/// it was last closed.
#[derive(Clone, Default, Debug)]
pub struct Furnace {
    /// fuel left in the current item
    pub burn_left: f32,
    /// progress on the current item
    pub progress: f32,
    /// total burn time of the current fuel, for the flame icon
    pub burn_total: f32,
    pub kind: Oven,
}

/// Smokers only do food and blast furnaces only ores/metal, both twice as fast.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub enum Oven {
    #[default]
    Furnace,
    Smoker,
    Blast,
}

impl Oven {
    fn speed(self) -> f32 {
        if self == Oven::Furnace { 1.0 } else { 2.0 }
    }

    fn allows(self, input: &NetworkItemStackDescriptorV2, out: &NetworkItemStackDescriptorV2) -> bool {
        let (i, o) = (crate::items::name_of(input.id).unwrap_or(""), crate::items::name_of(out.id).unwrap_or(""));
        match self {
            Oven::Furnace => true,
            Oven::Smoker => crate::hunger::food(o).is_some(),
            Oven::Blast => o.ends_with("_ingot") || o.ends_with("_nugget") || i.contains("ore") || i.contains("raw_"),
        }
    }
}

impl Furnace {
    /// `secs` of smelting on [input, fuel, output, ...].
    pub fn catch_up(&mut self, slots: &mut [Option<crate::inventory::Stack>], mut secs: f32) {
        let recipes = get();
        while secs > 0.0 {
            let Some(input) = slots[0].clone() else { self.progress = 0.0; break };
            let Some((out, n)) = recipes.smelted(&input.item) else { break };
            if !self.kind.allows(&input.item, &out) {
                break;
            }
            let room = match &slots[2] {
                None => true,
                Some(o) => o.item.id == out.id && o.item.aux_value == out.aux_value && o.count + n <= crate::items::max_stack(out.id),
            };
            if !room {
                break;
            }
            if self.burn_left <= 0.0 {
                let fuel = slots[1].as_ref().and_then(|f| crate::items::name_of(f.item.id)).and_then(fuel_secs);
                let Some(f) = fuel else { break };
                self.burn_left = f / self.kind.speed();
                self.burn_total = self.burn_left;
                let fuel_name = slots[1].as_ref().and_then(|f| crate::items::name_of(f.item.id)).unwrap_or("");
                if fuel_name == "minecraft:lava_bucket" {
                    if let (Some(s), Some(b)) = (slots[1].as_mut(), crate::items::item_by_name("minecraft:bucket")) {
                        s.item = b; // the bucket stays behind
                    }
                } else {
                    take_one(&mut slots[1]);
                }
            }
            let step = secs.min(self.burn_left).min((SMELT_SECS - self.progress) / self.kind.speed());
            secs -= step;
            self.burn_left -= step;
            self.progress += step * self.kind.speed();
            if self.progress >= SMELT_SECS - 1e-3 {
                self.progress = 0.0;
                take_one(&mut slots[0]);
                match &mut slots[2] {
                    Some(o) => o.count += n,
                    None => slots[2] = Some(crate::inventory::Stack { item: out, count: n, net_id: 0 }),
                }
            }
        }
        if slots[0].is_none() {
            self.progress = 0.0;
        }
        // idle furnaces still burn off fuel that's already lit
        self.burn_left = (self.burn_left - secs.max(0.0)).max(0.0);
    }
}

fn take_one(s: &mut Option<crate::inventory::Stack>) {
    if let Some(st) = s {
        st.count -= 1;
        if st.count <= 0 {
            *s = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(name: &str, count: i8) -> Option<crate::inventory::Stack> {
        Some(crate::inventory::Stack { item: crate::items::item_by_name(name).unwrap(), count, net_id: 0 })
    }

    #[test]
    fn recipes_load_and_furnace_smelts() {
        let r = get();
        assert!(r.crafting.len() > 500, "vanilla crafting recipes: {}", r.crafting.len());
        let iron = crate::items::item_by_name("minecraft:raw_iron").unwrap();
        let out = r.smelted(&iron).expect("raw iron smelts");
        assert_eq!(crate::items::name_of(out.0.id), Some("minecraft:iron_ingot"));

        let mut slots = vec![stack("minecraft:raw_iron", 3), stack("minecraft:coal", 1), None];
        let mut f = Furnace::default();
        f.catch_up(&mut slots, 25.0);
        assert_eq!(slots[2].as_ref().unwrap().count, 2, "2 smelted in 25s");
        assert_eq!(slots[0].as_ref().unwrap().count, 1);
        assert!(slots[1].is_none(), "coal consumed");
        f.catch_up(&mut slots, 100.0);
        assert_eq!(slots[2].as_ref().unwrap().count, 3);
        assert_eq!(fuel_secs("minecraft:oak_planks"), Some(15.0));
    }
}

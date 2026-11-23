//! Custom blocks, items and mobs from plugin code, like PNX's CustomItem/CustomEntity API. A plugin
//! returns them from `custom_items()` / `custom_entities()`; their textures and models come
//! from a resource pack in `resource_packs/` (icon = the pack's item_texture.json short name,
//! entity id = the pack's client entity identifier).
//!
//! ```ignore
//! fn custom_blocks(&self) -> Vec<custom::Block> {
//!     vec![custom::Block { id: "velt:ruby_block", name: "Block of Ruby", texture: "ruby_block", hardness: 5.0, light: 7, ..Default::default() }]
//! }
//! fn custom_items(&self) -> Vec<custom::Item> {
//!     vec![
//!         custom::Item { id: "velt:ruby", name: "Ruby", icon: "ruby", ..Default::default() },
//!         custom::Item { id: "velt:ruby_sword", name: "Ruby Sword", icon: "ruby_sword", stack: 1, category: 3,
//!             hand_equipped: true, damage: 10.0, durability: 2000, glint: true, ..Default::default() },
//!         custom::Item { id: "velt:ruby_helmet", name: "Ruby Helmet", icon: "ruby_helmet", stack: 1,
//!             armor: Some((0, 4.0)), durability: 600, ..Default::default() },
//!         custom::Item { id: "velt:burger", name: "Burger", icon: "burger", food: Some((8.0, 12.0)), ..Default::default() },
//!     ]
//! }
//! ```

use std::sync::OnceLock;

#[derive(Clone)]
pub struct Item {
    pub id: &'static str,
    pub name: &'static str,
    /// Short texture name from the resource pack's item_texture.json.
    pub icon: &'static str,
    pub stack: i8,
    /// Creative tab: 1 construction, 2 nature, 3 equipment, 4 items.
    pub category: u8,
    /// Held sideways like a tool.
    pub hand_equipped: bool,
    /// Melee damage when held (vanilla hand is 1).
    pub damage: f32,
    /// Uses before it breaks; 0 = never breaks.
    pub durability: i32,
    /// Eaten like food: (hunger points, saturation).
    pub food: Option<(f32, f32)>,
    /// Worn as armor: (slot 0 helmet .. 3 boots, armor points).
    pub armor: Option<(u8, f32)>,
    /// Enchanted shimmer.
    pub glint: bool,
    /// Anything else, added as-is to the item's client components
    /// (e.g. ("minecraft:cooldown", nbt), ("minecraft:throwable", nbt)).
    pub extra: Vec<(&'static str, nbtx::Value)>,
}

impl Default for Item {
    fn default() -> Self {
        Item { id: "", name: "", icon: "", stack: 64, category: 4, hand_equipped: false, damage: 1.0, durability: 0, food: None, armor: None, glint: false, extra: vec![] }
    }
}

/// A custom item by its id, if a plugin added one.
pub fn item(id: &str) -> Option<&'static Item> {
    get().items.iter().find(|i| i.id == id)
}

/// A custom full block. Its texture is the short name from the resource pack's
/// terrain_texture.json. Placing it, breaking it (drops itself) and saving it all work.
#[derive(Clone)]
pub struct Block {
    pub id: &'static str,
    pub name: &'static str,
    /// Texture on every face.
    pub texture: &'static str,
    /// Mining time base, like vanilla hardness (stone 1.5, obsidian 50).
    pub hardness: f32,
    /// Light it gives off, 0-15.
    pub light: u8,
    /// "opaque", "alpha_test" (cut-out, like leaves) or "blend" (see-through, like glass).
    pub render: &'static str,
    /// Creative tab: 1 construction, 2 nature, 3 equipment, 4 items.
    pub category: u8,
    /// Faces the player when placed (minecraft:cardinal_direction, like furnaces).
    pub rotatable: bool,
    /// Variants as (name, texture), e.g. [("red", "lamp_red"), ("blue", "lamp_blue")]: each gets
    /// its own creative entry. Stored in the "<namespace>:variant" state.
    pub variants: &'static [(&'static str, &'static str)],
    /// Anything else, added as-is to the block's client components
    /// (e.g. ("minecraft:geometry", nbt) for a custom model, ("minecraft:friction", nbt)).
    pub extra: Vec<(&'static str, nbtx::Value)>,
}

const DIRS: [&str; 4] = ["north", "west", "south", "east"];

impl Block {
    /// Name of the variant state.
    pub fn variant_key(&self) -> String {
        format!("{}:variant", self.id.split(':').next().unwrap_or("custom"))
    }

    /// Every state as (states, network id); the first is the default.
    pub fn states(&self) -> Vec<(Vec<(String, String)>, i32)> {
        let dirs: Vec<Option<&str>> = if self.rotatable { DIRS.iter().map(|d| Some(*d)).collect() } else { vec![None] };
        let vars: Vec<Option<&str>> = if self.variants.is_empty() { vec![None] } else { self.variants.iter().map(|v| Some(v.0)).collect() };
        let mut out = vec![];
        for v in &vars {
            for d in &dirs {
                let mut st: Vec<(String, String)> = vec![];
                if let Some(d) = d {
                    st.push(("minecraft:cardinal_direction".into(), d.to_string()));
                }
                if let Some(v) = v {
                    st.push((self.variant_key(), v.to_string()));
                }
                st.sort();
                let nbt = nbtx::Value::Compound(st.iter().map(|(k, v)| (k.as_str().into(), s(v))).collect());
                out.push((st, crate::blocks::hash(self.id, nbt)));
            }
        }
        out
    }
}

impl Default for Block {
    fn default() -> Self {
        Block { id: "", name: "", texture: "", hardness: 1.5, light: 0, render: "opaque", category: 1, rotatable: false, variants: &[], extra: vec![] }
    }
}

/// First legacy block id for custom blocks (what PNX uses too).
pub const FIRST_BLOCK_ID: i32 = 10000;

/// The network id of a custom block's default state.
pub fn block_hash(b: &Block) -> i32 {
    b.states()[0].1
}

/// A custom block by the network id of any of its states.
pub fn block(id: i32) -> Option<&'static Block> {
    let name = &crate::blocks::entry(id)?.name;
    get().blocks.iter().find(|b| b.id == name)
}

/// The block's StartGame definition: how the client draws and mines it.
pub fn block_nbt(b: &Block, n: usize) -> std::collections::HashMap<String, nbtx::Value> {
    use nbtx::Value::{Byte, Float, Int};
    let materials = |texture: &str| compound([("mappings", compound([])), ("materials", compound([("*", compound([("texture", s(texture)), ("render_method", s(b.render)), ("ambient_occlusion", Byte(1)), ("face_dimming", Byte(1))]))]))]);
    let mut comps = nbtx::Compound::new();
    comps.insert("minecraft:material_instances".into(), materials(b.texture));
    comps.insert("minecraft:geometry".into(), compound([("identifier", s("minecraft:geometry.full_block")), ("culling", s(""))]));
    comps.insert("minecraft:destructible_by_mining".into(), compound([("value", Float(b.hardness))]));
    comps.insert("minecraft:light_emission".into(), compound([("emission", Byte(b.light.min(15) as i8))]));
    comps.insert("minecraft:display_name".into(), compound([("value", s(b.name))]));
    for (k, v) in &b.extra {
        comps.insert((*k).into(), v.clone());
    }
    let cat = ["construction", "construction", "nature", "equipment", "items"][b.category.min(4) as usize];
    // States the client tracks: the variant as a property, the direction from the placement trait.
    let mut properties = vec![];
    let mut traits = vec![];
    let mut permutations = vec![];
    if !b.variants.is_empty() {
        let values = nbtx::ValueList::try_from(b.variants.iter().map(|v| s(v.0)).collect::<Vec<_>>()).unwrap_or_default();
        properties.push(compound([("name", s(&b.variant_key())), ("enum", nbtx::Value::List(values))]));
        for (v, tex) in b.variants {
            permutations.push(compound([("condition", s(&format!("q.block_state('{}') == '{v}'", b.variant_key()))), ("components", compound([("minecraft:material_instances", materials(tex))]))]));
        }
    }
    if b.rotatable {
        traits.push(compound([("name", s("minecraft:placement_direction")), ("enabled_states", compound([("cardinal_direction", Byte(1))])), ("y_rotation_offset", Float(0.0))]));
        for (i, d) in DIRS.iter().enumerate() {
            let t = compound([("RX", Int(0)), ("RY", Int(i as i32)), ("RZ", Int(0)), ("SX", Float(1.0)), ("SY", Float(1.0)), ("SZ", Float(1.0)), ("TX", Float(0.0)), ("TY", Float(0.0)), ("TZ", Float(0.0))]);
            permutations.push(compound([("condition", s(&format!("q.block_state('minecraft:cardinal_direction') == '{d}'"))), ("components", compound([("minecraft:transformation", t)]))]));
        }
    }
    let list = |v: Vec<nbtx::Value>| nbtx::Value::List(nbtx::ValueList::try_from(v).unwrap_or_default());
    [
        ("properties".to_string(), list(properties)),
        ("traits".to_string(), list(traits)),
        ("permutations".to_string(), list(permutations)),
        ("components".to_string(), nbtx::Value::Compound(comps)),
        ("menu_category".to_string(), compound([("category", s(cat)), ("group", s(""))])),
        ("molangVersion".to_string(), Int(12)),
        ("vanilla_block_data".to_string(), compound([("block_id", Int(FIRST_BLOCK_ID + n as i32))])),
    ]
    .into_iter()
    .collect()
}

pub struct Custom {
    pub blocks: Vec<Block>,
    pub items: Vec<Item>,
    pub entities: Vec<crate::mobs::Spec>,
}

/// Everything the plugins register, collected once at start-up.
pub fn get() -> &'static Custom {
    static C: OnceLock<Custom> = OnceLock::new();
    C.get_or_init(|| {
        let plugins = crate::plugin::all();
        Custom {
            blocks: plugins.iter().flat_map(|p| p.custom_blocks()).collect(),
            items: plugins.iter().flat_map(|p| p.custom_items()).collect(),
            entities: plugins.iter().flat_map(|p| p.custom_entities()).collect(),
        }
    })
}

fn s(v: &str) -> nbtx::Value {
    nbtx::Value::String(v.into())
}

fn compound<const N: usize>(kv: [(&str, nbtx::Value); N]) -> nbtx::Value {
    nbtx::Value::Compound(kv.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

/// The item's network definition (the shape vanilla and PNX send for data-driven items).
pub fn item_nbt(i: &Item, runtime_id: i16) -> nbtx::Value {
    use nbtx::Value::{Byte, Float, Int};
    let mut props = match compound([
        ("minecraft:icon", compound([("textures", compound([("default", s(i.icon))]))])),
        ("max_stack_size", Int(i.stack as i32)),
        ("hand_equipped", Byte(i.hand_equipped as i8)),
        ("creative_category", Int(i.category as i32)),
        ("creative_group", s("")),
        ("allow_off_hand", Byte(0)),
        ("foil", Byte(i.glint as i8)),
        ("damage", Int(i.damage as i32)),
    ]) {
        nbtx::Value::Compound(c) => c,
        _ => unreachable!(),
    };
    let mut comps = nbtx::Compound::new();
    comps.insert("minecraft:display_name".into(), compound([("value", s(i.name))]));
    if i.durability > 0 {
        comps.insert("minecraft:durability".into(), compound([("max_durability", Int(i.durability))]));
    }
    if let Some((nutrition, saturation)) = i.food {
        comps.insert("minecraft:food".into(), compound([("nutrition", Int(nutrition as i32)), ("saturation_modifier", Float(saturation / nutrition.max(1.0) / 2.0)), ("can_always_eat", Byte(0))]));
        props.insert("use_animation".into(), Int(1));
        props.insert("use_duration".into(), Int(32));
        // 1.21+ clients only finish eating (sound, consume packet) with this.
        comps.insert("minecraft:use_modifiers".into(), compound([("use_duration", Float(1.6)), ("movement_modifier", Float(0.35))]));
    }
    if let Some((slot, points)) = i.armor {
        let slot = ["slot.armor.head", "slot.armor.chest", "slot.armor.legs", "slot.armor.feet"][slot.min(3) as usize];
        comps.insert("minecraft:wearable".into(), compound([("slot", s(slot)), ("protection", Int(points as i32))]));
        comps.insert("minecraft:armor".into(), compound([("protection", Int(points as i32))]));
    }
    for (k, v) in &i.extra {
        comps.insert((*k).into(), v.clone());
    }
    comps.insert("item_properties".into(), nbtx::Value::Compound(props));
    compound([
        ("components", nbtx::Value::Compound(comps)),
        ("id", nbtx::Value::Int(runtime_id as i32)),
        ("name", s(i.id)),
    ])
}

/// Entry for the client's entity list, so it accepts the custom entity id.
pub fn entity_nbt(id: &str, rid: i32) -> nbtx::Value {
    compound([("bid", s("")), ("hasspawnegg", nbtx::Value::Byte(0)), ("id", s(id)), ("rid", nbtx::Value::Int(rid)), ("summonable", nbtx::Value::Byte(1))])
}

#[cfg(test)]
mod tests {
    #[test]
    fn item_definition_has_icon_and_name() {
        let i = super::Item { id: "velt:ruby", name: "Ruby", icon: "ruby", stack: 16, food: Some((4.0, 2.4)), ..Default::default() };
        let text = format!("{:?}", super::item_nbt(&i, 3000));
        assert!(text.contains("velt:ruby") && text.contains("\"ruby\"") && text.contains("Int(16)") && text.contains("Int(3000)") && text.contains("minecraft:food"), "{text}");
    }
}

#[cfg(all(test, feature = "example"))]
mod example_tests {
    use crate::blocks::{self, State};

    #[test]
    fn lamp_has_directions_variants_and_light() {
        let lamp = blocks::any_state("example:lamp").expect("lamp registered");
        let east = blocks::with(lamp, &[("minecraft:cardinal_direction", State::Str("east".into()))]);
        assert_ne!(east, lamp);
        let blue = blocks::with(east, &[("example:variant", State::Str("blue".into()))]);
        assert_eq!(blocks::state(blue, "minecraft:cardinal_direction"), Some(&State::Str("east".into())));
        assert_eq!(crate::world::emission(blue), 15);
        assert_eq!(super::block(blue).map(|b| b.id), Some("example:lamp"));
    }
}

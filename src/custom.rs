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
    /// Anything else, added as-is to the block's client components
    /// (e.g. ("minecraft:geometry", nbt) for a custom model, ("minecraft:friction", nbt)).
    pub extra: Vec<(&'static str, nbtx::Value)>,
}

impl Default for Block {
    fn default() -> Self {
        Block { id: "", name: "", texture: "", hardness: 1.5, light: 0, render: "opaque", category: 1, extra: vec![] }
    }
}

/// First legacy block id for custom blocks (what PNX uses too).
pub const FIRST_BLOCK_ID: i32 = 10000;

/// The network id of a custom block (its state hash), by index.
pub fn block_hash(b: &Block) -> i32 {
    crate::blocks::hash(b.id, nbtx::Value::Compound(Default::default()))
}

/// A custom block by its network id.
pub fn block(id: i32) -> Option<&'static Block> {
    get().blocks.iter().find(|b| block_hash(b) == id)
}

/// The block's StartGame definition: how the client draws and mines it.
pub fn block_nbt(b: &Block, n: usize) -> std::collections::HashMap<String, nbtx::Value> {
    use nbtx::Value::{Byte, Float, Int};
    let mut comps = nbtx::Compound::new();
    comps.insert(
        "minecraft:material_instances".into(),
        compound([("mappings", compound([])), ("materials", compound([("*", compound([("texture", s(b.texture)), ("render_method", s(b.render)), ("ambient_occlusion", Byte(1)), ("face_dimming", Byte(1))]))]))]),
    );
    comps.insert("minecraft:geometry".into(), compound([("identifier", s("minecraft:geometry.full_block")), ("culling", s(""))]));
    comps.insert("minecraft:destructible_by_mining".into(), compound([("value", Float(b.hardness))]));
    comps.insert("minecraft:light_emission".into(), compound([("emission", Byte(b.light.min(15) as i8))]));
    comps.insert("minecraft:display_name".into(), compound([("value", s(b.name))]));
    for (k, v) in &b.extra {
        comps.insert((*k).into(), v.clone());
    }
    let cat = ["construction", "construction", "nature", "equipment", "items"][b.category.min(4) as usize];
    [
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

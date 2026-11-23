//! Custom items and mobs from plugin code, like PNX's CustomItem/CustomEntity API. A plugin
//! returns them from `custom_items()` / `custom_entities()`; their textures and models come
//! from a resource pack in `resource_packs/` (icon = the pack's item_texture.json short name,
//! entity id = the pack's client entity identifier).
//!
//! ```ignore
//! fn custom_items(&self) -> Vec<custom::Item> {
//!     vec![custom::Item { id: "velt:ruby", name: "Ruby", icon: "ruby", stack: 64, category: 4, hand_equipped: false }]
//! }
//! ```

use std::sync::OnceLock;

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
}

pub struct Custom {
    pub items: Vec<Item>,
    pub entities: Vec<crate::mobs::Spec>,
}

/// Everything the plugins register, collected once at start-up.
pub fn get() -> &'static Custom {
    static C: OnceLock<Custom> = OnceLock::new();
    C.get_or_init(|| {
        let plugins = crate::plugin::all();
        Custom {
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
    compound([
        (
            "components",
            compound([
                (
                    "item_properties",
                    compound([
                        ("minecraft:icon", compound([("textures", compound([("default", s(i.icon))]))])),
                        ("max_stack_size", nbtx::Value::Int(i.stack as i32)),
                        ("hand_equipped", nbtx::Value::Byte(i.hand_equipped as i8)),
                        ("creative_category", nbtx::Value::Int(i.category as i32)),
                        ("creative_group", s("")),
                        ("allow_off_hand", nbtx::Value::Byte(0)),
                    ]),
                ),
                ("minecraft:display_name", compound([("value", s(i.name))])),
            ]),
        ),
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
        let i = super::Item { id: "velt:ruby", name: "Ruby", icon: "ruby", stack: 16, category: 4, hand_equipped: false };
        let text = format!("{:?}", super::item_nbt(&i, 3000));
        assert!(text.contains("velt:ruby") && text.contains("\"ruby\"") && text.contains("Int(16)") && text.contains("Int(3000)"), "{text}");
    }
}

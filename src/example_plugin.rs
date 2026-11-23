//! Example custom content (build with `cargo build --release --features example`). Pair it
//! with the resource pack in examples/custom-content/pack: copy that folder into
//! resource_packs/. Then try /give @s example:ruby_sword, example:ruby_block, and
//! /summon example:goblin.

use crate::custom::{Block, Item};
use crate::mobs::{Class, Spec};

pub struct Example;

impl crate::plugin::Plugin for Example {
    fn name(&self) -> &str {
        "ExampleContent"
    }

    fn custom_blocks(&self) -> Vec<Block> {
        vec![
            Block { id: "example:ruby_block", name: "Block of Ruby", texture: "ruby_block", hardness: 5.0, light: 7, ..Default::default() },
            // Faces the player, comes in two colours.
            Block { id: "example:lamp", name: "Lamp", texture: "lamp_red", light: 15, rotatable: true, variants: &[("red", "lamp_red"), ("blue", "lamp_blue")], ..Default::default() },
        ]
    }

    fn custom_items(&self) -> Vec<Item> {
        vec![
            Item { id: "example:ruby", name: "Ruby", icon: "ruby", ..Default::default() },
            Item { id: "example:ruby_sword", name: "Ruby Sword", icon: "ruby_sword", stack: 1, category: 3, hand_equipped: true, damage: 10.0, durability: 2000, glint: true, ..Default::default() },
            Item { id: "example:ruby_helmet", name: "Ruby Helmet", icon: "ruby_helmet", stack: 1, category: 3, armor: Some((0, 4.0)), durability: 600, ..Default::default() },
            Item { id: "example:burger", name: "Burger", icon: "burger", food: Some((8.0, 12.0)), ..Default::default() },
        ]
    }

    fn custom_entities(&self) -> Vec<Spec> {
        vec![Spec {
            id: "example:goblin", display: "Goblin", health: 30.0, size: (0.6, 1.9), speed: 0.3, melee: 4.0, class: Class::Hostile,
            food: &[], drops: &[("example:ruby", 0, 2)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None,
        }]
    }
}

# Custom blocks, items and mobs

Plugins can add new blocks, items and mobs, with no behavior pack. The server defines
how they work and a resource pack gives them their look. A full working example is in
[`src/example_plugin.rs`](https://github.com/RustedMC/Rusted/blob/main/src/example_plugin.rs)
with its pack in [`examples/custom-content/pack`](https://github.com/RustedMC/Rusted/tree/main/examples/custom-content/pack).

Try it:

```sh
cargo build --release --features example
cp -r examples/custom-content/pack resource_packs/example-content
```

Then in game: `/give @s example:ruby_sword`, `/give @s example:ruby_block`, `/summon example:goblin`.

## Blocks

```rust
fn custom_blocks(&self) -> Vec<custom::Block> {
    vec![custom::Block { id: "my:ruby_block", name: "Block of Ruby", texture: "ruby_block",
        hardness: 5.0, light: 7, ..Default::default() }]
}
```

| Field | What it does |
|---|---|
| `texture` | Short name from the pack's `textures/terrain_texture.json`, on every face |
| `hardness` | Mining time base (stone 1.5, obsidian 50) |
| `light` | Light it gives off, 0-15 |
| `render` | `"opaque"`, `"alpha_test"` (cut-out like leaves) or `"blend"` (see-through like glass) |
| `category` | Creative tab: 1 construction, 2 nature, 3 equipment, 4 items |
| `extra` | Any other Bedrock block component, sent as-is (custom geometry, friction...) |

Blocks can be placed, mined, dropped and saved like vanilla ones, and they show in the creative menu.
For now each block has one state, so it can't face different directions or have variants.

## Items

```rust
fn custom_items(&self) -> Vec<custom::Item> {
    vec![
        custom::Item { id: "my:ruby_sword", name: "Ruby Sword", icon: "ruby_sword", stack: 1, category: 3,
            hand_equipped: true, damage: 10.0, durability: 2000, glint: true, ..Default::default() },
        custom::Item { id: "my:burger", name: "Burger", icon: "burger", food: Some((8.0, 12.0)), ..Default::default() },
    ]
}
```

| Field | What it does |
|---|---|
| `icon` | Short name from the pack's `textures/item_texture.json` |
| `stack` | Max stack size (1-64) |
| `damage` | Melee damage when held |
| `durability` | Uses before it breaks (0 = never) |
| `food` | `Some((hunger, saturation))`: eaten like food |
| `armor` | `Some((slot, points))`: slot 0 helmet, 1 chest, 2 legs, 3 boots |
| `glint` | Enchanted shimmer |
| `hand_equipped` | Held sideways like a tool |
| `extra` | Any other Bedrock item component, sent as-is (cooldown, throwable...) |

Armor that shows on the player needs an attachable in the pack. See
`examples/custom-content/pack/attachables/ruby_helmet.json`.

## Mobs

```rust
fn custom_entities(&self) -> Vec<mobs::Spec> {
    vec![mobs::Spec { id: "my:goblin", display: "Goblin", health: 30.0, size: (0.6, 1.9), speed: 0.3,
        melee: 4.0, class: mobs::Class::Hostile, food: &[], drops: &[("my:ruby", 0, 2)],
        undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None }]
}
```

They use the built-in AI for their `class` (passive, neutral, hostile, flying, aquatic...), and
`/summon` works on them. The look comes from a client entity in the pack whose identifier
matches `id` (see `entity/goblin.entity.json`). For custom behavior, use `on_tick`,
`on_mob_target`, `on_entity_damage` and the other [events](events.md).

## Resource pack tips

- Bump the pack's `version` in `manifest.json` when you change it, or players keep the old cached copy.
- Set `force-resource-packs=true` in `server.properties` so nobody joins without the textures.

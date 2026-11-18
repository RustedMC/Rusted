# Notices and credits

Rusted builds on the work of others. Thank you.

## Libraries (linked, not copied)
- **bedrock-rs**: Bedrock protocol, networking, auth: Apache-2.0 —
  https://github.com/bedrock-crustaceans/bedrock-rs
  `src/main.rs` started from its `examples/server.rs` (Apache-2.0).
- **nbtx**, **raknet**, **nethernet** (bedrock-crustaceans): Apache-2.0
- Other crates: see `Cargo.lock`; each is under its own license.

## Game data (`data/`)
- `block_palette.nbt`, `item_palette.json`, `creative_items.json`, `item_components.nbt`,
  `entity_identifiers.nbt`, `jigsaw_structure_data.nbt`: from
  **Kaooot/bedrock-network-data** (as bundled in PowerNukkitX). That repository has no
  license file. *To do before 1.0: switch to pmmp/BedrockData (CC0) or get permission.*
- `data/vanilla/*.bin` (recipes, biomes, game rules, camera presets, trims): captured from a
  PowerNukkitX server's network traffic with `src/bin/record.rs`.
- All of this data describes Minecraft: Bedrock Edition, which belongs to Mojang Studios /
  Microsoft. Rusted is not affiliated with or endorsed by Mojang or Microsoft.

## Reference
- **PowerNukkitX** (LGPL-3.0, https://github.com/PowerNukkitX/PowerNukkitX): Rusted is a
  Rust port of PowerNukkitX, ported with permission and credit. Ported so far: the world
  generator (`src/overworld/`: noise, density functions, caves, aquifers, ore veins, biomes,
  surfaces, features, trees, structures) and PNX's behaviour for items, chests and drops;
  more of PNX is being ported. Rusted as a whole is therefore licensed **LGPL-3.0** (see
  LICENSE), like PowerNukkitX.
- **bedrock-rs** code Rusted started from (`src/main.rs`, from `examples/server.rs`) is
  Apache-2.0 (LICENSE-APACHE), which is compatible with LGPL-3.0; its notice is kept.

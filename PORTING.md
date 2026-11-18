# Porting PowerNukkitX to Rusted

Rusted is becoming a Rust port of [PowerNukkitX](https://github.com/PowerNukkitX/PowerNukkitX)
(LGPL-3.0, ported with permission and credit): same behaviour, written to be much lighter
(no garbage collector, data-oriented, parallel across cores).

**Rules for ported code**
- Port behaviour faithfully: same rules, numbers, random-number order. Where Java does
  something only because of Java (object graphs, boxing, virtual dispatch per block), use
  plain Rust data instead. Same result, less work.
- Credit the source with a `// from PNX <Class>` comment on ported code.
- Don't port PNX's own bugs unless the game depends on them; note any deliberate change.
- Each batch ends with tests that check the ported behaviour.

## PowerNukkitX packages and their status

| PNX package | Size | What it is | Rusted status |
|---|---|---|---|
| `level/generator` | ~2 MB | world generation, features, structures | ✅ ported (`src/overworld/`) |
| `level` (rest) | ~1.9 MB | ticking, light, weather, chunks, explosions, sounds, particles, game rules | 🟡 partial, own code: port next |
| `block` | 2.6 MB, 1517 files | per-block behaviour: crops, redstone, doors, rails, fluids, plants, interactions, drops, mining | 🟡 partial, own code |
| `entity` | 2.6 MB, 452 files | mobs, AI (behaviours, pathfinding), projectiles, items, XP, vehicles | 🟡 own simplified version (`src/mobs.rs`): replace with port |
| `item` | 0.8 MB, 713 files | tools, durability, food, armour, enchantments, special items | 🟡 partial |
| `blockentity` + `inventory` | 0.8 MB | chests, furnaces, hoppers, brewing, anvils, enchanting, beacons, signs, spawners | 🟡 chests/furnaces/signs only |
| `command` | 0.9 MB | full command system and every vanilla command | 🟡 ~30 commands, own parser |
| `recipe` | 0.1 MB | crafting, smelting, brewing, smithing, stonecutter | 🟡 crafting/smelting |
| `event` + `plugin` + `scheduler` | 0.4 MB | plugin API: 200+ events, scheduler | 🟡 small own API |
| `network` | 0.5 MB | packet handling, sessions, compression | ✅ via bedrock-rs (port the handling logic only) |
| `form`, `scoreboard`, `dialog`, `camera`, `ddui` | 0.2 MB | server UI: forms, scoreboards, NPC dialogs, cameras | ❌ not yet |
| `permission`, `lang`, `config` | 0.15 MB | permissions, translations, settings | 🟡 ops/whitelist/bans, server.properties |
| `migration`, `education`, `metrics`, `wizard` | — | world upgrades, Education Edition, stats, setup wizard | skip for now |

## Order

1. **Level core** (`level`): the 20 TPS world tick, scheduled block updates, random ticks,
   a real light engine (sky and block light), weather, explosions, particles and sounds.
   Everything else plugs into this.
2. **Blocks** (`block`): every block's behaviour: placing, breaking, mining speed, drops,
   interactions, crops and plants, fluids, falling blocks, doors and rails.
3. **Items** (`item`): tool tiers and durability, food, armour, special items.
4. **Block entities and inventories**: hoppers, brewing stands, anvils, enchanting tables,
   beacons, spawners, shulker boxes, crafters, loot tables.
5. **Redstone**: dust, torches, repeaters, comparators, pistons, observers (PNX `block` +
   `level` update order).
6. **Entities and AI** (`entity`): replace `src/mobs.rs` with PNX's behaviour and
   memory/sensor AI, every mob, projectiles, XP, minecarts and boats.
7. **Commands** (`command`): PNX's command tree and all vanilla commands.
8. **Plugin API** (`event`, `plugin`, `scheduler`): PNX-style events and scheduler, so PNX
   plugin authors feel at home.
9. **Server UI**: forms, scoreboards, boss bars, NPC dialogs, cameras.
10. **Nether and End** generators and portals (`level/generator`, rest).

## Speed targets (measured, not promised)
- No garbage-collector pauses.
- Chunks generated and ticked across all CPU cores.
- Entities stored as plain data (no object per mob in a garbage-collected heap) and AI on a
  time budget.
- Target: several times less CPU and memory per player than PNX on the same machine, and
  more mobs and players before lag. Each batch's benchmarks get written here.

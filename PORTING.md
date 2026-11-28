# Porting status

Rusted ports [PowerNukkitX](https://github.com/PowerNukkitX/PowerNukkitX) (LGPL-3.0) to Rust.
This file tracks how far each part of PNX has come over.

✅ ported · 🟡 partly there, or simpler than PNX · ❌ not started

| PNX package | What it covers | Status |
|---|---|---|
| `level/generator` | overworld, Nether, End, features, structures | ✅ seed-exact with PNX: every feature PNX has, carver caves, canyons, dungeons, springs, geodes, dripstone, sculk patches, azalea roots and sulfur caves included (no second block layer for waterlogging at generation) |
| `level` | ticking, light, weather, explosions, portals, dimensions | 🟡 block and sky light, lightning charges creepers; ticking areas |
| `block` | block behaviour, interactions, crops, fluids, fire | 🟡 most blocks work, rails and sculk included; trial spawners and vaults work (reward tables approximate) |
| `blockentity` + `inventory` | containers and their screens | 🟡 chests, furnaces, hoppers, brewing, anvil, grindstone, enchanting, smithing, shulkers, dispensers, beacons, looms, lecterns, crafters; maps and the cartography table (zoom, copy, lock) |
| `item` | tools, food, armour, enchantments, effects | 🟡 bows, tridents, potions, fishing, shields, totems, crossbows, elytra and fireworks work |
| `entity` | mobs and AI | 🟡 87 mob types on a simpler AI than PNX's behaviour system; riding, boats, chest boats, all minecarts, llama caravans; the dragon perches on the portal |
| `entity` (villagers) | jobs, trading, raids | 🟡 trading uses PNX's tables; breeding, wandering traders, gossip and reputation prices |
| redstone | dust, diodes, pistons, observers | 🟡 works for common builds; rails (powered, detector, activator), repeater locking and lectern comparator output and sculk sensors work |
| `command` | vanilla commands | 🟡 nearly every vanilla command, `/execute`, `/camera` and target selectors; `/dialogue` from scene files |
| `event` + `plugin` | plugin API | 🟡 113 of PNX's ~200 events, scheduler, config files; compiled-in or runtime `.wasm` plugins, which get every event |
| `form`, `scoreboard` | server UI | 🟡 forms, scoreboards, boss bars, camera control, NPC dialogs |
| loot tables | chest loot, spawners | 🟡 chests in 16 structures fill on first open, with enchanted books; structure spawners; trial spawner and vault rewards; `/loot`; dungeons |

## How ported code is written
- Same rules, numbers and random-number order as PNX. Where Java needed objects and virtual
  calls, plain Rust data does the same job.
- Credit the source with a `// from PNX <Class>` comment.
- Leave a `// TODO:` where something isn't ported properly yet.
- Add a test for the behaviour you ported.

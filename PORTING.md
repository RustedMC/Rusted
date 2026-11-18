# Porting status

Rusted ports [PowerNukkitX](https://github.com/PowerNukkitX/PowerNukkitX) (LGPL-3.0) to Rust.
This file tracks how far each part of PNX has come over.

✅ ported · 🟡 partly there, or simpler than PNX · ❌ not started

| PNX package | What it covers | Status |
|---|---|---|
| `level/generator` | overworld, Nether, End, features, structures | ✅ seed-exact with PNX |
| `level` | ticking, light, weather, explosions, portals, dimensions | 🟡 block light only, no sky light; no lightning-charged creepers |
| `block` | block behaviour, interactions, crops, fluids, fire | 🟡 most blocks work; no rails, sculk, trial spawners, vaults |
| `blockentity` + `inventory` | containers and their screens | 🟡 chests, furnaces, hoppers, brewing, anvil, grindstone, enchanting, smithing, shulkers, dispensers; no beacon, loom, cartography, crafter, lectern |
| `item` | tools, food, armour, enchantments, effects | 🟡 bows, tridents and potions work; no crossbow, fishing, elytra boosting, shields blocking |
| `entity` | mobs and AI | 🟡 71 mob types on a simpler AI than PNX's behaviour system; riding horses, donkeys, mules, pigs, striders and camels, horse taming and saddles, boats, minecarts on rails; no chest boats, chest/hopper/TNT minecarts, llama caravans or dragon perching |
| `entity` (villagers) | jobs, trading, raids | 🟡 trading uses PNX's tables; no gossip, breeding or wandering traders |
| redstone | dust, diodes, pistons, observers | 🟡 works for common builds; rails, powered and activator rails work; no repeater locking, detector rail power or quasi-connectivity |
| `command` | vanilla commands | 🟡 ~50 commands and target selectors; no `/execute`, `/locate`, `/function` |
| `event` + `plugin` | plugin API | 🟡 21 of PNX's ~200 events, scheduler, config files; plugins are compiled in |
| `form`, `scoreboard` | server UI | 🟡 forms, scoreboards, boss bars; no NPC dialogs or camera control |
| loot tables | chest loot, spawners | ❌ generated chests are empty, spawners don't spawn |

## How ported code is written
- Same rules, numbers and random-number order as PNX. Where Java needed objects and virtual
  calls, plain Rust data does the same job.
- Credit the source with a `// from PNX <Class>` comment.
- Leave a `// TODO:` where something isn't ported properly yet.
- Add a test for the behaviour you ported.

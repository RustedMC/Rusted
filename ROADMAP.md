# Roadmap

Each release finishes one step below. Patch releases (0.4.1, 0.4.2...) each close one batch;
a new minor version (0.5.0) starts when a whole group is done; 1.0.0 is the end of this list.
See [PORTING.md](PORTING.md) for where each part of the PowerNukkitX port stands.

✅ released · 🔨 in progress · ⏳ planned

## 0.4: vanilla gameplay

- ✅ **0.4.0** Redstone that connects and powers like vanilla, PNX pistons (slime, honey,
  containers), block support rules, vanilla sounds, knockback, mobs jumping, XP orbs, spawn
  eggs, sleeping
- ✅ **0.4.1** Skeletons draw and pillagers charge before shooting, vanilla spawn weights and
  cap, rarer patrols
- ✅ **0.4.2** Pistons slide like vanilla (moving blocks), crushed blocks drop, mobs ride
  along
- ✅ **0.4.3** Tripwire and hooks, target blocks, trapped chests, copper bulbs, redstone
  torch burnout
- ✅ **0.4.4** Mob spawning: PNX's per-mob Bedrock spawn rules (biome, light, surface or cave,
  herds); buttons on every face, pistons breaking attached blocks; multiplayer fixes (players,
  items and mobs reappear after teleports and deaths, live-synced chests, per-player ender
  chests, break cracks); critical hits, sprint knockback, bottles o' enchanting; vanilla
  explosions (rays, blast resistance), item physics (ice, currents, burning), trapdoors, gates,
  selectors on every command, sign input for plugins, swimming/crawling through 1x1 gaps
- ✅ **0.4.5** Mobs: 12 more types, snow and iron golems and the wither built from blocks, wandering
  traders (trades, visits), villager breeding, leads and llama caravans, polar bears guarding
  cubs, chest boats, TNT minecarts
- ✅ **0.4.6** Waterlogging (a second block layer in chunks and saves); crawling pose; explosions hurt mobs
- ✅ **0.5.0** Sky light (for spawning and growth), fireworks from the ground, `/execute`, `/locate`, `/function`
- ✅ **0.5.1** Multiple worlds (`/mv`), with per-world inventories
- ✅ **0.5.2** Maps: empty maps become maps of the area, drawn from the world
- ✅ README comparison with PowerNukkitX (start-up, RAM, CPU, size)
- ✅ **0.5.10** Block breaking on current clients: slow blocks break, crack progress and mining sound for the miner, no ghost breaks; spawn protection only once someone is op; air bubbles hide out of water; items land on leaves
- ✅ **0.5.9** Custom blocks can face the player and have variants; their light counts for spawning and growth
- ✅ **0.5.8** Example custom content (block, items, mob + resource pack) and a custom content guide
- ✅ **0.5.7** Custom blocks from plugin code (texture, hardness, light, render method, extra components)
- ✅ **0.5.6** Custom item damage, durability, food, armor, glint and raw extra components; on_tick plugin hook
- ✅ **0.5.5** Custom items and mobs from plugin code (custom_items / custom_entities), looks from a resource pack
- ✅ **0.5.11** The cartography table: make, copy, zoom out (4 levels) and lock maps; locator maps (was planned as 0.5.4)
- ✅ **0.5.3** Fireworks launched from the ground, with their effects (moved into 0.5.0)

## 0.6: servers and plugins

- ✅ **0.6.0** The remaining vanilla commands, carver caves, canyons and dungeons, every PNX world feature, trial spawners and vaults, villager gossip
  (also runtime plugins in `plugins/*.wasm`, about 60 events, camera control and NPC dialogs)
- ✅ **0.6.1** Furnaces and brewing stands keep their progress over a restart

## 1.0: stable

- ⏳ Block, item and creative data from properly licensed files
- ⏳ Long-running tests with real players
- ⏳ Benchmarks against PNX refreshed for the release

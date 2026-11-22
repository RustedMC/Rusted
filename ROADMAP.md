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
- ⏳ **0.5.1** Multiple worlds (Multiverse), with per-world inventories
- ⏳ **0.5.2** Maps and the cartography table
- ✅ **0.5.3** Fireworks launched from the ground, with their effects (moved into 0.5.0)

## 0.6: servers and plugins

- ⏳ **0.6.0** The remaining vanilla commands (`/execute` came early, in 0.5.0)
- ⏳ **0.6.1** Plugins loaded at runtime, more events
- ⏳ **0.6.2** Camera control, NPC dialogs

## 1.0: stable

- ⏳ Block, item and creative data from properly licensed files
- ⏳ Long-running tests with real players
- ⏳ Benchmarks against PNX refreshed for the release

# Roadmap

Each release finishes one step below. Patch releases (0.4.1, 0.4.2...) each close one batch;
a new minor version (0.5.0) starts when a whole group is done; 1.0.0 is the end of this list.
See [PORTING.md](PORTING.md) for where each part of the PowerNukkitX port stands.

✅ released · 🔨 in progress · ⏳ planned

## 0.4: vanilla gameplay

- ✅ **0.4.0** Redstone that connects and powers like vanilla, PNX pistons (slime, honey,
  containers), block support rules, vanilla sounds, knockback, mobs jumping, XP orbs, spawn
  eggs, sleeping, plugin suite
- ✅ **0.4.1** Skeletons draw and pillagers charge before shooting, vanilla spawn weights and
  cap, rarer patrols
- ✅ **0.4.2** Pistons slide like vanilla (moving blocks), crushed blocks drop, mobs ride
  along
- 🔨 **0.4.3** Missing redstone: tripwire, target block, trapped chest, powered and activator
  rails, copper bulb, torch burnout; buttons on pistons
- ⏳ **0.4.4** Mob spawning: PNX's per-mob Bedrock spawn rules (biome, light, surface or cave,
  herds)
- ⏳ **0.4.5** Mob AI: the rest of PNX's behaviours for neutrals, villagers, fish and flyers;
  arrows and tridents checked against PNX

## 0.5: the world

- ⏳ **0.5.0** Sky light
- ⏳ **0.5.1** Multiple worlds (Multiverse), with per-world inventories
- ⏳ **0.5.2** Maps and the cartography table
- ⏳ **0.5.3** Fireworks launched from the ground, with their effects

## 0.6: servers and plugins

- ⏳ **0.6.0** `/execute` and the remaining vanilla commands
- ⏳ **0.6.1** Plugins loaded at runtime, more events
- ⏳ **0.6.2** Camera control, NPC dialogs

## 1.0: stable

- ⏳ Block, item and creative data from properly licensed files
- ⏳ Long-running tests with real players
- ⏳ Benchmarks against PNX refreshed for the release

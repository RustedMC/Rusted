# Rusted roadmap: vanilla-feeling Bedrock server in Rust

**Goal:** play like vanilla Bedrock, run much lighter than Java servers, and stay easy to
extend for a lobby/minigame network.

**How we work:** I write and test each batch (automated tests and protocol checks), you play
and report what's broken, we fix it, then move on. PNX's source and data are the reference
(LGPL: credit PNX in a NOTICE file and keep ported parts open). The proxy and comparison
tools settle any "why doesn't the game accept this" question.

**Time estimates** assume roughly daily sessions like the ones so far. A *batch* is one
session of building plus your test round. Real calendar time depends mostly on how often
we test.

---

## ✅ Done (session 3)
`server.properties` (port, MOTD, max players, level name/type/seed, flat layers, game mode,
difficulty, whitelist), text or number seeds, flat/void/plugin world generators, per-level
folders, caves, operators, bans, whitelist, console commands, the vanilla command set plus
`/plugins` and `/version`, weather, difficulty, items on the ground (drops, Q, pickup, merging,
despawn), block drops with pickaxe tiers, chest and death drops (`keepInventory`), hunger,
eating, starving, crafting (2x2 and crafting table, vanilla recipes), furnaces, drowning,
lava and fire damage, beds set spawn, signs, hoes, crops, saplings, leaf decay, grass spread,
ice melting, snow, door/button sounds.

## ✅ Done (session 2)
Vanilla data replay (recipe book, biome colors, game rules, camera presets, trims), vanilla
stack sizes, day/night clock, encryption on RakNet, all four game modes, command
autocomplete with values, plugin API v0, MIT/Apache-2.0 licensing, **seeded terrain**
(oceans, beaches, plains, forests, deserts, snowy plains, trees, plants, ores, deepslate),
parallel chunk generation, fall/void damage, death and respawn, natural regeneration,
doors/trapdoors/gates/levers/buttons, falling sand and gravel, flowing water and lava with
buckets, chests and barrels with saved contents.

## ✅ Done (session 1)
Join on 1.26 over NetherNet, flat world, chunk streaming, chat, multiplayer (skins,
movement, player list), break/place with sounds and particles, vanilla block placement
orientation, creative inventory with dragging, armor and offhand, pick block, commands
(`/gamemode /tp /time /give /npc`), world and player saving, duplicate-login kicks,
shutdown saves, and the dev tools (recorder, proxy, comparison).

---

## Phase 1: Vanilla data and polish · ~1 week · 3–5 batches
Mostly data that's the same for every player: fast and low-risk.
- Replay PNX's static join data: **biome definitions** (sky, fog and water colors),
  **recipe book**, **game rules**, camera presets, trim data, difficulty
- Correct item names and icons for every creative item, including items with NBT (potions,
  enchanted books)
- Per-item stack sizes (tools 1, ender pearls 16, etc.)
- Encryption back on (needed before any public server)
- Real time and day/night cycle (server ticks the world clock)
- `/kick`, `/op`, `/list`, `/say`, `/weather`, permission levels

## Phase 2: World engine · ~2–3 weeks · 6–10 batches
The foundation everything else runs on. This is where the speed comes from.
- **Tick loop**: fixed 20 TPS, chunk-parallel ticking (independent regions run on separate
  CPU cores)
- **Real terrain generation**: vanilla-like overworld (noise, biomes, caves, ores, trees,
  structures), generated in parallel and cached
- **LevelDB world format**: open and save real Bedrock worlds, so you can use worlds from
  single-player or Realms
- **Block updates**: falling sand and gravel, water and lava flow, plant growth, leaf decay,
  fire spread, random ticks
- **Lighting** handled by the client (Bedrock computes its own; we only send blocks)
- Nether and End dimensions plus portals

## Phase 3: Survival core · ~2–3 weeks · 6–10 batches
- Health, hunger, saturation, fall damage, drowning, fire and lava, death screen, respawn
- **Block breaking**: vanilla mining times, tool tiers, block drops (loot tables from data)
- **Item entities**: dropped items on the ground, picked up, merged, despawning after 5 minutes
- XP orbs and levels
- **Containers**: chests (including double), barrels, shulker boxes, hoppers
- **Crafting**: 2×2 grid, crafting table, recipe book clicks
- Furnace, blast furnace, smoker (smelting with fuel timing)
- Doors, trapdoors, gates, buttons, levers, pressure plates (interaction plus state)
- Signs (edit text), beds (sleep, spawn point), ladders, scaffolding

## Phase 4: Entities and AI · ~3–5 weeks · 10–15 batches
Built to scale: a data-oriented entity system (ECS) and AI with a per-tick time budget, so
1,000 mobs don't slow the server.
- Entity framework: spawning, movement physics, collision, knockback, despawning
- **Pathfinding**: cached A* on a navigation grid, shared between mobs heading the same way,
  run on worker threads
- **Behaviors** built from reusable goals (wander, follow, flee, attack, breed, tempt, sit)
- Passive mobs: cow, pig, sheep, chicken (breeding, shearing, milking)
- Hostile mobs: zombie, skeleton, creeper, spider, enderman (day/night spawning, light levels)
- Combat: player vs mob, player vs player, critical hits, attack cooldown, armor and enchantments
- Projectiles: arrows, snowballs, tridents, ender pearls
- Villagers and trading, iron golems
- Mob spawning rules (vanilla caps per chunk group)

## Phase 5: Redstone and technical · ~2–4 weeks · 6–12 batches
- Redstone dust, torches, repeaters, comparators
- Pistons (sticky, block pushing limits), observers, dispensers, droppers, hoppers chains
- Rails and minecarts, TNT, note blocks
- Vanilla update order, so standard farms and contraptions work

## Phase 6: Full survival content · ~4–8 weeks · ongoing
- Enchanting table, anvil, grindstone, smithing table, brewing stand
- Farming, animals' full behavior, fishing, beehives
- Structures with loot (villages, dungeons, strongholds…), bosses (Ender Dragon, Wither)
- Advancement-style triggers, statistics, maps
- Every remaining block's special behavior (long tail)

## Phase 7: Network features (your end goal) · ~3–5 weeks · can start after Phase 3
- **Multi-server network**: proxy, transfer between lobby, games and SMP, shared player data
  (Redis/Postgres)
- Lobby kit: NPCs, holograms, forms/menus, scoreboards, boss bars, no-damage zones
- Minigame framework: arenas, teams, kits, map reset in milliseconds (copy-on-write worlds)
- Plugin API (Rust plugins plus optional scripting) so games aren't hard-coded
- Anti-cheat basics (movement and reach checks; the server already authoritates blocks)
- MCXboxBroadcast-style console joining

---

## Totals (honest estimate)

| Milestone | What it means | Rough time from now |
|---|---|---|
| **Lobby-ready** | Phases 1 + parts of 7 | **~3–5 weeks** |
| **Survival SMP that feels vanilla** (no redstone/bosses) | Phases 1–4 | **~2–3 months** |
| **Redstone + most content** | Phases 1–6 | **~5–8 months** |
| **Full network** (lobby + minigames + SMP) | add Phase 7 | **~6–9 months** |
| **Complete vanilla parity** | everything, every block quirk | 1 year+, and ongoing with every Minecraft update |

Biggest unknowns: Minecraft updates (each can break things for days), how often we test,
and the long tail of small vanilla behaviors nobody notices until they're missing.

## Performance plan (why it'll beat Java servers)
- No garbage collector: no lag spikes from GC pauses
- Chunks and regions ticked in parallel across all CPU cores
- Identical data encoded once and shared (already done for chunks and items)
- Entities in a cache-friendly ECS, so thousands of mobs stay cheap
- AI on a time budget: mobs far from players think less often
- Copy-on-write worlds for instant minigame map resets
- Target: 100+ players on a 4-core VPS for lobbies; Java servers typically manage far fewer

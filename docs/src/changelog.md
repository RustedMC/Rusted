# Changelog

## 0.6.12

- 25 more plugin events (138 now):
  - Players: `on_effect_add`, `on_effect_remove`, `on_ender_pearl`, `on_xp_orb_pickup`, `on_vault_open`, `on_world_change`
  - World: `on_item_spawn`, `on_golem_build`, `on_trader_spawn`, `on_patrol_spawn`, `on_night_skip`, `on_bonemeal`, `on_anchor_charge`
  - Sculk and trials: `on_shriek`, `on_warden_summon`, `on_trial_spawn`, `on_dragon_perch`
  - Server admin: `on_gamerule_change`, `on_difficulty_change`, `on_op_change`, `on_ban`, `on_unban`, `on_ban_ip`, `on_whitelist_change`, `on_backup`
- 17 of them can be cancelled, including every admin change, so a plugin can lock down ops, bans and game rules. Runtime `.wasm` plugins get them all.

## 0.6.11

- 25 more plugin events (113 now):
  - Players: `on_toggle_crawl`, `on_player_spawn`, `on_map_request`, `on_block_pick`, `on_lectern_page`, `on_beacon_change`, `on_farmland_trample`, `on_totem_use`
  - Blocks: `on_crafter_craft`, `on_dispense`, `on_note_play`, `on_bell_ring`, `on_furnace_smelt`, `on_tnt_prime`
  - Raids: `on_raid_start`, `on_raid_wave`, `on_raid_end`
  - Mobs: `on_sheep_shear`, `on_sheep_regrow`, `on_leash`, `on_villager_level_up`, `on_spawner_spawn`, `on_creeper_power`
  - Server: `on_rcon_command`, `on_console_command`
- 13 of them can be cancelled (stop TNT, totems, trampling, raids, spawners, dispensers and more). Runtime `.wasm` plugins get them all.

## 0.6.10

- Emotes and in-game skin changes now show to other players (they were dropped before).
- 7 more plugin events (88 now): `on_emote` (cancellable), `on_skin_change`, `on_lightning` (cancellable), `on_item_despawn`, `on_entity_transform`, `on_mob_shoot` and `on_chunk_unload`. Runtime `.wasm` plugins get them too.

## 0.6.9

- 7 more plugin events (81 now): `on_leaves_decay`, `on_block_spread`, `on_block_form` (snow, ice, and obsidian or cobblestone from lava), `on_block_fade`, `on_block_toggle` (doors, trapdoors, gates, levers), `on_vehicle_place` and `on_vehicle_destroy`. All but the last can be cancelled, and runtime `.wasm` plugins get them too.
- `on_block_grow` is now only for real growth (crops, saplings and the like). Leaves decaying, grass spreading and snow or ice forming or melting have their own events.

## 0.6.8

- 7 more plugin events (74 now): `on_table_craft` (enchanting, anvil, grindstone, smithing, loom, stonecutter, cartography and crafting tables, with the XP cost), `on_brew`, `on_entity_combust`, `on_entity_despawn`, `on_piston`, `on_health_regain` (cancellable) and `on_food_change`. Runtime `.wasm` plugins get them too.
- Mob and piston events are fired once per tick, after the mob list is unlocked, so plugins can look at mobs from them safely.

## 0.6.7

- 8 more plugin events (67 now): `on_jump`, `on_toggle_flight`, `on_animation` (cancellable), `on_form_response`, `on_packet_receive` (every packet by name, cancellable), `on_world_save`, `on_liquid_flow` (cancellable) and `on_projectile_launch`. Runtime `.wasm` plugins get them too.

## 0.6.6

**Load testing**
- New load tester (`loadtest`) that connects fake players who walk around and keep loading new chunks, and reads the server's tick times over RCON: `loadtest 127.0.0.1:19132 50 180 25575 <rcon password>` (address, bots, seconds, RCON port, password).
- `/status` and `logs/perf.log` show tick times for the last minute (p50, p95, p99, max), mobs and RAM.

**Faster with many players** (found with the load tester, 50 players on one PC)
- Chunk generation is shared fairly between players: at most one chunk per CPU core at once, instead of every player starting their own threads. Work for players who left is dropped.
- The chunk caches grow with the number of players and keep the chunks nearest to players, so chunks people stand in aren't thrown away and built again.
- A chunk wanted by several players at once is only generated once.
- Springs wait until the chunks around them exist before flowing. In 0.6.4 they could make the world loop generate terrain for up to 15 seconds.
- Result: 50 of 50 players joined with none dropped (45 of 50 before), almost 3x the chunks delivered, normal ticks under 1 ms at the 95th percentile, and no more 16 s stalls.

## 0.6.5

- Ominous bottles can be drunk. They give Bad Omen at the bottle's level (I to V) for 100 minutes, so you can start a raid without hunting a patrol captain.

## 0.6.4

- Water and lava springs in cave walls and cliffs now flow when you find them, instead of sitting as still source blocks.

## 0.6.3

- Runtime plugins (`.wasm`) get every server event now, not just the first 16: crafting, fishing, mob spawns and deaths, explosions, signs, buckets, beds, XP, ticks and more. See [Runtime plugins](plugins/runtime.md) for the full list.

## 0.6.2

- Fire Protection and Protection now reduce lava and fire damage, like vanilla.

## 0.6.1

- Lit furnaces, smokers, blast furnaces and brewing stands keep going after a restart instead of pausing or losing their brew.
- Fixed a rare crash in world generation when noise coordinates got very large.

## 0.6.0

**Plugins**
- Plugins load at runtime from `plugins/*.wasm`. They run sandboxed (WebAssembly, with a time limit per call), so a broken plugin can't crash the server. See [Runtime plugins](plugins/runtime.md).
- About 60 plugin events, including bed leave, bucket fill and empty, fishing, gliding, swimming, container close, item break, kick, trading, mounting and dismounting.
- NPC dialogs with buttons, and scenes loaded from `dialogues/<scene>.json`.

**Commands**
- The remaining vanilla commands: `/camera`, `/aimassist`, `/controlscheme`, `/ride`, `/dialogue`, `/hud`, `/fog`, `/playanimation`, `/inputpermission`, `/replaceitem entity`, `/loot`, `/particle`, `/spreadplayers`, `/testforblocks`, `/schedule`, `/clearspawnpoint`, `/tickingarea`, `/music`, `/setmaxplayers` and `/recipe`.

**World generation**
- Caves, extra underground caves and canyons are carved out like in PowerNukkitX.
- Dungeons (monster rooms) with spawners and chests.
- Every PowerNukkitX world feature: springs, geodes, dripstone clusters, sculk patches, azalea roots and the sulfur features.
- Trial chambers now join up and down properly and have working trial spawners and vaults, including ominous vaults.

**Mobs**
- Villager gossip: villagers remember who cured, hurt, killed or traded with them, pass it on to each other, and change their prices to match.
- Lightning makes charged creepers, which explode harder.
- The ender dragon lands on the portal now and then.

## 0.5.11

- The cartography table: make, copy, zoom out (4 levels) and lock maps. Locator maps.

## 0.5.10

- Block breaking works on current clients: slow blocks break, the miner sees crack progress and hears the mining sound, and there are no more ghost breaks.
- Spawn protection only applies once someone is op.
- Air bubbles hide when you're out of water.
- Dropped items land on leaves instead of falling through.

## 0.5.9

- Custom blocks can face the player and have variants. Their light counts for spawning and plant growth.

## 0.5.8

- Example custom content (a block, items and a mob with a resource pack) and a custom content guide.

## 0.5.7

- Custom blocks from plugin code: texture, hardness, light, render method and extra components.

## 0.5.6

- Custom item damage, durability, food, armor, glint and raw extra components.
- New `on_tick` plugin hook.

## 0.5.5

- Custom items and mobs from plugin code, with their looks from a resource pack.

## 0.5.2

- Maps: empty maps become maps of the area, drawn from the world.

## 0.5.1

- Multiple worlds (`/mv`), with separate inventories per world.

## 0.5.0

- Sky light, used for spawning and growth.
- Fireworks launched from the ground, with their effects.
- `/execute`, `/locate` and `/function`.

## 0.4.7

- The 19 bundled gameplay plugins (lifesteal, economy, ranks, lobby and others) are gone. The plugin API stays for your own plugins.
- `/ban-ip`, `/unban-ip` and `/banlist ips`. Banned addresses are refused at login.

## 0.4.6

- Waterlogging, with a second block layer in chunks and saves.
- Crawling pose.
- Explosions hurt mobs.

## 0.4.5

- 12 more mob types. Snow golems, iron golems and the wither can be built from blocks.
- Wandering traders with trades and visits, villager breeding, leads and llama caravans, and polar bears guarding their cubs.
- Chest boats and TNT minecarts.

## 0.4.4

- Mob spawning follows PowerNukkitX's Bedrock spawn rules for each mob (biome, light, surface or cave, herds).
- Buttons on every face, and pistons break attached blocks.
- Multiplayer fixes: players, items and mobs reappear after teleports and deaths, chests sync live, ender chests are per player, and everyone sees break cracks.
- Critical hits, sprint knockback and bottles o' enchanting.
- Vanilla explosions (rays and blast resistance) and item physics (ice, currents, burning).
- Trapdoors and fence gates, selectors on every command, sign input for plugins, and swimming or crawling through 1x1 gaps.

## 0.4.3

- Tripwire and tripwire hooks, target blocks, trapped chests, copper bulbs and redstone torch burnout.

## 0.4.2

- Pistons slide blocks like vanilla. Crushed blocks drop, and mobs ride along.

## 0.4.1

- Skeletons draw and pillagers charge before shooting.
- Vanilla spawn weights and mob cap, and rarer patrols.

## 0.4.0

- Redstone connects and powers like vanilla.
- PowerNukkitX pistons, including slime, honey and containers.
- Block support rules, vanilla sounds, knockback, mobs jumping, XP orbs, spawn eggs and sleeping.

## 0.3.0

**Mobs**
- Mob speeds and attack timing follow PowerNukkitX.
- Wolves, cats, iron golems and flying mobs use the mob brain. Pets sit, fight, follow their owner and catch up when left far behind.
- Skeletons, pillagers, vindicators and piglins hold their weapons.

**Blocks and items**
- Water and lava flow toward drops and wash away plants, torches and rails, which drop as items. Lava meeting water makes obsidian, cobblestone or stone.
- Pressure plates feel mobs and items. Weighted plates give a signal by count.
- Sculk sensors hear footsteps and blocks changing nearby, and give a redstone signal. Shriekers shriek, give Darkness and call a warden on the fourth warning.
- Firework rockets: craft them, launch them from the ground, fire them from crossbows, and see them while gliding. Bursts with stars hurt what's nearby.
- Arrows fall at vanilla speed.

**World generation**
- Nether ruined portals.
- End city chests have loot.
- Nether lava springs flow.
- Mineshafts have chest minecarts with loot. Hoppers empty them, and they keep their items over a restart.

**Fixes from testing**
- Fences, iron bars, glass panes, walls, redstone dust and stairs join their neighbours. Bedrock now expects the server to work these out.
- Structures have their fences back.
- Zombies don't burn at night anymore. The server's clock stops with `doDaylightCycle` off and resyncs with players every 12 seconds.
- Survival mining works like vanilla: other players see the cracks, and breaks finish properly.
- Breaking either half of tall grass or a large fern removes both halves.

**For server owners**
- `start.bat` and `start.sh` open a console you can type commands into, like PocketMine and PowerNukkitX. They restart the server after a crash.
- Automatic world backups (`backup-interval`, `backup-keep`).
- RCON for hosting panels (`enable-rcon`, `rcon.port`, `rcon.password`).
- `spawn-protection`, `player-idle-timeout`, and custom `join-message` and `leave-message`.
- New settings are added to an existing `server.properties` automatically.
- Quieter console output by default. Set `RUST_LOG=debug` for everything.

**Commands and plugins**
- Command replies use vanilla wording.
- New plugin events: `on_mob_damage`, `on_inventory_action` and `on_chunk_load`. `on_container_open` also fires for chest minecarts.

## 0.2.0

**Riding and vehicles**
- You now sit in the right spot on horses, donkeys, mules, pigs, striders, camels, boats and minecarts. Before, riders sank into the mob and drifted down into the void.
- Boats and minecarts no longer throw you off with a horse sound.
- In a boat you can only turn your head sideways and forward, as in vanilla.

**Blocks**
- Placing a block into water, lava, grass or snow layers replaces it.
- Torches, flowers, rails and other blocks you can walk through can be placed where you're standing.
- Redstone dust, repeaters, comparators, string and sugar cane are now placed by the server, so they power and light up.
- New rails now show their turned shape to everyone.
- Flint and steel only makes its own ignite sound.

**Items and combat**
- Shields and totems go in the offhand. Crouching with a shield raises it.
- A totem of undying plays its pop animation and sound.
- Crossbows load when fully charged.
- Fishing works as in vanilla: a fish swims toward the bobber and bites, and the catch flies out of the water to you. Switching slots reels the line in.
- Beacon effects now apply.
- The creative menu has banners, fireworks, firework stars, enchanted books and other items with data.
- Skeletons, pillagers, vindicators and piglins hold their weapons.
- Knockback uses PowerNukkitX's formula.

**World and mobs**
- Hostile mobs no longer spawn in daylight under leaves.
- More blocks give off light.
- Creative and spectator players only die from `/kill` and the void.
- Villagers run from zombies, go to their job block by day and look for cover at night.
- Fish dart away from players.

**Server**
- Going to the Nether or the End loads much faster. It uses the same dimension handshake as PowerNukkitX.
- Logging in again while your old connection is still open drops the old one, so "joined the game" no longer shows twice.
- Command autocomplete shows vanilla arguments, such as `/tp <victim> <destination>` and `/give <player> <item>`.
- `/version` no longer lists credits.

## 0.1.0

First release.

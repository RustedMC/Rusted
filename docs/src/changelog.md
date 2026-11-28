# Changelog

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

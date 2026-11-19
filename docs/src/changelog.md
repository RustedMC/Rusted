# Changelog

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

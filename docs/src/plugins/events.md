# Events

Override any of these methods on your `Plugin`. Every one has a default that does nothing.

**Cancellable** events return `bool`: return `false` to stop the action. Events that pass a
`&mut String` let you rewrite the text.

## Priority

When several plugins handle the same event, they run by priority:

```rust
fn priority(&self) -> Priority {
    Priority::High
}
```

| Priority | Use it for |
|---|---|
| `Lowest`, `Low` | Runs first. Good for setting things up that others may override. |
| `Normal` | The default. |
| `High`, `Highest` | Runs later, so it gets the final say. Protection plugins usually go here. |
| `Monitor` | Runs last, only for events nobody cancelled. Its `bool` is ignored. Use it for logging and stats, never to change anything. |

The first plugin that returns `false` cancels the event, and plugins after it don't see it.
Plugins with the same priority run in load order.

## Players

| Method | When | Cancel / edit |
|---|---|---|
| `on_pre_login(server, name, xuid) -> Option<String>` | Someone is logging in, before they join | Return a message to refuse them |
| `on_join(server, player)` | Player finished joining | |
| `on_leave(server, player)` | Player left, kicks and disconnects included | |
| `on_chat(server, player, message: &mut String) -> bool` | Before a chat message is shown | Edit `message`, or `false` to block it |
| `on_command_preprocess(server, player, line: &mut String) -> bool` | A typed command (without `/`), before it runs | Rewrite `line`, or `false` to drop it |
| `on_player_damage(server, player, cause: &str, amount: f32) -> bool` | About to take damage. `cause` is like `"fall"`, `"lava"` or `"Zombie"` | `false` cancels the damage |
| `on_player_death(server, player, message: &mut String)` | Player died | Edit the death message, empty hides it |
| `on_player_respawn(server, player)` | Player respawned | |
| `on_player_interact(server, player, pos, item: &str) -> bool` | Right-clicking a block holding `item` | `false` means nothing happens |
| `on_item_use(server, player, item: &Stack) -> bool` | Right-click with an item in hand, in the air or on a block | `false` cancels the normal use |
| `on_gamemode_change(server, player, mode: &str) -> bool` | Game mode about to change to `"survival"`, `"creative"`... | `false` keeps the old mode |
| `on_item_drop(server, player, item: &str, count: i8)` | Player threw items out of their inventory | |
| `on_item_pickup(server, player, item: &str, count: i8) -> bool` | About to pick up items off the ground | `false` leaves them there |
| `on_move(server, player, to) -> bool` | Moved (`to` is the eye position) | `false` puts them back |
| `on_sneak(server, player, sneaking: bool)` | Started or stopped sneaking | |
| `on_sprint(server, player, sprinting: bool)` | Started or stopped sprinting | |
| `on_held_change(server, player, slot: u32, item: Option<&str>)` | Switched hotbar slot or what's in hand | |
| `on_consume(server, player, item: &str) -> bool` | Finished eating or drinking | `false`: nothing is eaten |
| `on_container_open(server, player, pos, block: &str) -> bool` | Opening a chest, furnace or other block screen | `false` keeps it shut |
| `on_teleport(server, player, to)` | Teleported (commands, pearls, plugins), feet position | |
| `on_dimension_change(server, player, to: u8)` | Went to another dimension: 0 overworld, 1 Nether, 2 End | |
| `on_xp_change(server, player, points: &mut i32)` | Getting XP (negative takes it away) | Change `points`, 0 cancels |
| `on_sign_change(server, player, pos, front: &mut String, back: &mut String) -> bool` | Wrote a sign | Edit the text, or `false` keeps the old text |
| `on_bed_enter(server, player, pos) -> bool` | Right-clicked a bed (sets spawn) | `false` stops it |
| `on_bed_leave(server, player)` | Got out of bed | |
| `on_bucket_fill(server, player, pos, filled: &str) -> bool` | Filling an empty bucket; `filled` is what it becomes | `false` stops it |
| `on_bucket_empty(server, player, pos, bucket: &str) -> bool` | Emptying a water or lava bucket at `pos` | `false` stops it |
| `on_fish(server, player, item: &str) -> bool` | Reeled in a catch | `false` loses the catch |
| `on_glide(server, player, gliding: bool)` | Started or stopped gliding with an elytra | |
| `on_swim(server, player, swimming: bool)` | Started or stopped swimming | |
| `on_container_close(server, player, kind: &str)` | Closed a container screen (`"Chest"`, `"Anvil"`...) | |
| `on_item_break(server, player, item: &str)` | A worn-out tool or weapon broke | |
| `on_kick(server, player, message: &str)` | About to be kicked | |
| `on_trade(server, player, villager, index, times)` | Traded with a villager | |
| `on_mount(server, player, mob)` | Got on a horse, boat, minecart... | |
| `on_dismount(server, player)` | Got off what they were riding | |
| `on_craft(server, player, item: &str, count: i32) -> bool` | Crafting something at a table or in the inventory | `false` refuses the craft |
| `on_projectile_hit(server, shooter, target: Target) -> bool` | Their arrow or trident hit `Target::Mob(id)` or `Target::Player(id)` | `false`: no damage |

## Blocks

| Method | When | Cancel |
|---|---|---|
| `on_block_break(server, player, pos, block: i32) -> bool` | Player breaking a block | `false` cancels |
| `on_block_place(server, player, pos, block: i32) -> bool` | Player placing a block | `false` cancels |

`pos` is `(x, y, z)` as `i32`. `block` is the block's runtime id.

## Mobs and the world

| Method | When | Cancel |
|---|---|---|
| `on_entity_damage(server, player, mob: i64, kind: &str, damage: f32) -> bool` | Player hitting a mob, e.g. `kind = "minecraft:cow"` | `false` cancels the hit |
| `on_entity_death(server, kind, pos, killer: Option<i64>)` | Mob died. `killer` is the player id if a player did it | |
| `on_entity_spawn(server, kind, pos) -> bool` | Mob about to spawn (natural, `/summon`, breeding) | `false` stops it |
| `on_explosion(server, pos, power: f32) -> bool` | Creeper, TNT or respawn anchor about to explode | `false` stops it |
| `on_mob_target(server, mob: i64, kind: &str, player) -> bool` | A hostile mob wants to go after a player | `false` leaves them alone |
| `on_block_burn(server, pos, block: &str, spread: bool) -> bool` | Fire spreading to a block (`spread`) or burning one away | `false` stops it |
| `on_explosion_blocks(server, at, blocks: &mut Vec<pos>)` | An explosion is about to break these blocks | Remove positions to keep them, clear for no block damage |
| `on_entity_tame(server, player, mob, kind) -> bool` | Trying to tame a wolf, cat or horse | `false` stops it |
| `on_entity_breed(server, kind, parents) -> bool` | Two mobs are about to breed | `false`: no baby |
| `on_portal_create(server, player, kind: &str) -> bool` | Lighting a `"nether"` or `"end"` portal | `false` keeps it unlit |
| `on_block_grow(server, pos, from: &str, to: &str) -> bool` | A block changes by itself: crops, saplings, grass spreading, leaves decaying, ice melting | `false` keeps it as it is |
| `on_weather_change(server, weather: &str) -> bool` | Weather changing to `"clear"`, `"rain"` or `"thunder"` | `false` keeps it |

| `on_mob_damage(server, mob, kind: &str, damage, from) -> bool` | A mob is hurt by anything but a player: another mob, fire, a fall, an explosion | `false` cancels the damage |
| `on_inventory_action(server, player, action: &str, container: &str) -> bool` | A player moves items in any screen. `action` is `"take"`, `"place"`, `"swap"`, `"drop"`, `"destroy"`, `"craft"` or `"other"` | `false` undoes the move |
| `on_chunk_load(server, player, chunk: (i32, i32))` | A chunk is sent to a player as it comes into view | |

Mob positions are `(f32, f32, f32)`.

## Server

| Method | When |
|---|---|
| `on_server_start(server)` | Worlds loaded, about to accept players. A good place to start [scheduled tasks](scheduler.md). |
| `on_server_stop(server)` | Server stopping, before the final save |
| `on_jump(server, player)` | Player jumped | |
| `on_toggle_flight(server, player, flying: bool)` | Started or stopped flying | |
| `on_animation(server, player, action: i32)` | Arm swing and other animations | yes |
| `on_form_response(server, player, form_id: u32, answer: Option<&str>)` | Answered or closed a form | |
| `on_packet_receive(server, player, packet: &str)` | Any packet from the player, by name | yes |
| `on_world_save(server)` | World saved | |
| `on_liquid_flow(server, pos, block: &str)` | Water or lava about to flow | yes |
| `on_projectile_launch(server, player, trident: bool, force: f32)` | Fired an arrow, trident or rocket | |
| `on_table_craft(server, player, table: &str, item: &str, levels: i32)` | Took a result from an enchanting table, anvil, grindstone, smithing table, loom, stonecutter, cartography table or crafting table | |
| `on_brew(server, pos)` | A brewing stand finished | |
| `on_entity_combust(server, mob, kind: &str)` | A mob caught fire (after the mob tick) | |
| `on_entity_despawn(server, mob, kind: &str)` | A mob despawned (after the mob tick) | |
| `on_piston(server, pos, extending: bool)` | A piston started moving | |
| `on_health_regain(server, player, health: f32)` | About to heal from food | yes |
| `on_food_change(server, player, food: i32)` | Food level changed | |
| `on_leaves_decay(server, pos, block: &str)` | Leaves about to decay | yes |
| `on_block_spread(server, pos, from: &str, to: &str)` | Grass or mycelium spreading | yes |
| `on_block_form(server, pos, block: &str)` | Snow, ice, obsidian or cobblestone forming | yes |
| `on_block_fade(server, pos, from: &str, to: &str)` | Snow or ice melting | yes |
| `on_block_toggle(server, player, pos, block: &str, on: bool)` | Opening or closing a door, trapdoor or gate, or flipping a lever | yes |
| `on_vehicle_place(server, player, kind: &str, pos)` | Placing a boat or minecart | yes |
| `on_vehicle_destroy(server, kind: &str, pos, killer: Option<i64>)` | A boat or minecart broke | |
| `on_emote(server, player, emote: &str)` | Playing an emote | yes |
| `on_skin_change(server, player, skin: &str)` | Changed skin in game | |
| `on_lightning(server, pos)` | Lightning about to strike | yes |
| `on_item_despawn(server, item: &str, count: i8, pos)` | A dropped item despawned | |
| `on_entity_transform(server, mob, from: &str, to: &str, new_mob)` | A mob turned into another (cured zombie villager) | |
| `on_mob_shoot(server, mob, kind: &str)` | A mob fired a projectile | |
| `on_chunk_unload(server, player, chunk)` | A chunk left the player's view | |
| `on_toggle_crawl(server, player: &PlayerInfo, crawling: bool)` | Player started (`true`) or stopped crawling |  |
| `on_player_spawn(server, player: &PlayerInfo)` | Player finished loading in and can play (after join, before they move) |  |
| `on_map_request(server, player: &PlayerInfo, map: i64)` | Player's client asks for the pixels of map `map` | yes |
| `on_block_pick(server, player: &PlayerInfo, pos: (i32, i32, i32), block: &str)` | Player middle-clicked `block` at `pos` to pick it | yes |
| `on_lectern_page(server, player: &PlayerInfo, pos: (i32, i32, i32), page: i32)` | Player turned the book on the lectern at `pos` to `page` |  |
| `on_beacon_change(server, player: &PlayerInfo, pos: (i32, i32, i32), primary: i32, secondary: i32)` | Player is setting the beacon at `pos` to effects `primary` and `secondary` (effect ids, 0 for none) | yes |
| `on_farmland_trample(server, player: &PlayerInfo, pos: (i32, i32, i32))` | Player landed on the farmland at `pos` and is about to trample it | yes |
| `on_crafter_craft(server, pos: (i32, i32, i32), item: &str, count: i8)` | The crafter at `pos` is about to craft `count` `item` | yes |
| `on_dispense(server, pos: (i32, i32, i32), block: &str)` | The dispenser or dropper (`block`) at `pos` got a pulse and is about to fire | yes |
| `on_note_play(server, pos: (i32, i32, i32))` | The note block at `pos` is about to play from redstone | yes |
| `on_bell_ring(server, pos: (i32, i32, i32), player: Option<i64>)` | The bell at `pos` rang (`player` is who rang it, if a player did) |  |
| `on_raid_start(server, center: (f32, f32, f32))` | A raid is about to start around `center` | yes |
| `on_raid_wave(server, center: (f32, f32, f32), wave: i32)` | Wave `wave` (from 1) of the raid at `center` is coming in |  |
| `on_raid_end(server, center: (f32, f32, f32), victory: bool)` | The raid at `center` ended: `victory` when every wave was beaten |  |
| `on_totem_use(server, player: &PlayerInfo)` | Player is about to be saved by a totem of undying | yes |
| `on_sheep_shear(server, player: i64, mob: i64)` | Player `player` (entity id) sheared sheep `mob` |  |
| `on_sheep_regrow(server, mob: i64)` | Sheep `mob` grew its wool back |  |
| `on_leash(server, player: i64, mob: i64, attached: bool)` | Player `player` tied (`attached`) or untied a lead on mob `mob` |  |
| `on_villager_level_up(server, villager: i64, tier: i32)` | Villager `villager` levelled up to `tier` (1 novice to 5 master) from trading |  |
| `on_spawner_spawn(server, pos: (i32, i32, i32), kind: &str)` | The mob spawner at `pos` is about to spawn a `kind` | yes |
| `on_creeper_power(server, mob: i64)` | Creeper `mob` was struck by lightning and is now charged |  |
| `on_rcon_command(server, addr: &str, line: &str)` | `line` came in over RCON from `addr` | yes |
| `on_console_command(server, line: &str)` | `line` was typed into the server console | yes |
| `on_furnace_smelt(server, pos: (i32, i32, i32))` | The furnace, smoker or blast furnace at `pos` finished smelting an item |  |
| `on_tnt_prime(server, pos: (i32, i32, i32))` | TNT at `pos` is about to be lit (by fire, redstone, an explosion or a player) | yes |
| `on_effect_add(server, player: &PlayerInfo, effect: i32, amplifier: i32, ticks: i32)` | Player is getting effect `effect` (an effect id) at `amplifier` for `ticks` | yes |
| `on_effect_remove(server, player: &PlayerInfo, effect: i32)` | Player lost effect `effect` (it ran out, or milk cleared it) |  |
| `on_ender_pearl(server, player: &PlayerInfo, to: (f32, f32, f32))` | Player's ender pearl landed and is about to take them to `to` | yes |
| `on_item_spawn(server, item: &str, count: i8, pos: (f32, f32, f32))` | `count` `item` is about to drop into the world at `pos` (mob drops, broken blocks, thrown items) | yes |
| `on_xp_orb_pickup(server, player: &PlayerInfo, value: i32)` | Player picked up an experience orb worth `value` points |  |
| `on_golem_build(server, kind: &str, pos: (f32, f32, f32))` | A snow golem, iron golem or wither (`kind`) is about to be built at `pos` | yes |
| `on_trader_spawn(server, pos: (f32, f32, f32))` | A wandering trader is about to arrive at `pos` | yes |
| `on_patrol_spawn(server, pos: (f32, f32, f32))` | A pillager patrol is about to spawn at `pos` | yes |
| `on_shriek(server, pos: (i32, i32, i32), player: Option<i64>)` | The sculk shrieker at `pos` shrieked (`player` set when a player set it off) |  |
| `on_warden_summon(server, pos: (f32, f32, f32))` | A warden is about to be summoned at `pos` | yes |
| `on_vault_open(server, player: &PlayerInfo, pos: (i32, i32, i32))` | Player is opening the vault at `pos` with a trial key | yes |
| `on_trial_spawn(server, pos: (i32, i32, i32), kind: &str, mob: i64)` | The trial spawner at `pos` spawned `kind` (mob id `mob`) |  |
| `on_gamerule_change(server, rule: &str, value: &str)` | Game rule `rule` is about to be set to `value` with /gamerule | yes |
| `on_difficulty_change(server, difficulty: i32)` | Difficulty is about to change to `difficulty` (0 peaceful to 3 hard) | yes |
| `on_op_change(server, name: &str, op: bool)` | `name` is about to be made op (`op` true) or de-opped | yes |
| `on_ban(server, name: &str, reason: &str)` | `name` is about to be banned with `reason` (may be empty) | yes |
| `on_unban(server, name: &str)` | `name` is about to be unbanned | yes |
| `on_ban_ip(server, ip: &str)` | The address `ip` is about to be banned | yes |
| `on_whitelist_change(server, name: &str, added: bool)` | `name` is about to be added to (`added` true) or removed from the allowlist | yes |
| `on_world_change(server, player: &PlayerInfo, world: &str)` | Player is about to be sent to world `world` with /mv tp | yes |
| `on_backup(server, path: &str)` | A world backup was written to `path` |  |
| `on_dragon_perch(server, mob: i64)` | The ender dragon `mob` is landing on the exit portal |  |
| `on_night_skip(server)` | Enough players are asleep and the night is about to be skipped | yes |
| `on_bonemeal(server, pos: (i32, i32, i32), player: Option<i64>)` | Bone meal was used on `pos` (`player` set when a player used it) |  |
| `on_anchor_charge(server, pos: (i32, i32, i32), player: Option<i64>)` | The respawn anchor at `pos` was charged with glowstone |  |
| `on_xp_level_change(server, player: &PlayerInfo, old: i32, new: i32)` | Player's XP level went from `old` to `new` (checked once a second) |  |
| `on_shield_block(server, player: &PlayerInfo, damage: f32)` | Player is about to block `damage` with their shield | yes |
| `on_cake_eat(server, pos: (i32, i32, i32), player: Option<i64>)` | A slice of the cake at `pos` was eaten (`player` set when a player ate it) |  |
| `on_dragon_egg_teleport(server, pos: (i32, i32, i32), to: (i32, i32, i32))` | The dragon egg at `pos` is about to teleport to `to` | yes |
| `on_end_portal_fill(server, pos: (i32, i32, i32), player: Option<i64>)` | An eye of ender was put into the end portal frame at `pos` |  |
| `on_composter_use(server, pos: (i32, i32, i32), player: Option<i64>)` | The composter at `pos` was filled or emptied |  |
| `on_mob_heal(server, mob: i64, amount: f32)` | Mob `mob` healed `amount` |  |
| `on_spawn_point_set(server, player: &PlayerInfo, pos: (f32, f32, f32))` | Player's spawn point is about to be set to `pos` (/spawnpoint) | yes |
| `on_view_distance(server, player: &PlayerInfo, chunks: &mut i32)` | Player asked for a view distance of `chunks` |  |
| `on_chunk_generate(server, chunk: (i32, i32))` | The overworld chunk at `chunk` was generated for the first time |  |
| `on_time_set(server, time: i32)` | The time is about to be set to `time` with /time set | yes |
| `on_sit_toggle(server, mob: i64, sitting: bool)` | A tamed wolf or cat `mob` sat down (`sitting` true) or stood up |  |
| `on_slow_tick(server, ms: f32)` | A game tick took `ms` milliseconds, over the 50 ms a tick has (the server is lagging) |  |
| `on_armor_change(server, player: &PlayerInfo, slot: i32, item: &str)` | Player's armor in `slot` (0 helmet to 3 boots) changed to `item` ("" when taken off) |  |
| `on_player_combust(server, player: &PlayerInfo)` | Player caught fire (lava or fire) |  |
| `on_death_drops(server, player: &PlayerInfo)` | Player died and is about to drop their items | yes |
| `on_portal_enter(server, player: &PlayerInfo, to: i32)` | Player stepped into a portal to dimension `to` (0 overworld, 1 Nether, 2 End) | yes |
| `on_firework_explode(server, pos: (f32, f32, f32), stars: i32)` | A firework burst at `pos` with `stars` stars |  |
| `on_tnt_minecart_prime(server, mob: i64)` | The TNT minecart `mob` was lit by an activator rail |  |
| `on_fishing_cast(server, player: &PlayerInfo)` | Player is about to cast a fishing rod | yes |
| `on_elytra_boost(server, player: &PlayerInfo)` | Player is about to boost their elytra with a firework | yes |
| `on_item_throw(server, player: &PlayerInfo, item: &str)` | Player is about to throw `item` (snowball, egg, ender pearl, splash potion, bottle o' enchanting) | yes |
| `on_block_explode(server, pos: (i32, i32, i32), block: &str)` | The `block` at `pos` is about to blow up from being used (a respawn anchor outside the Nether) | yes |
| `on_sign_open(server, player: &PlayerInfo, pos: (i32, i32, i32))` | Player placed a sign at `pos` and its edit screen is opening |  |
| `on_mob_expire(server, mob: i64, kind: &str)` | Mob `mob` (a wandering trader and its llamas) left when its time ran out |  |
| `on_sapling_grow(server, pos: (i32, i32, i32), sapling: &str)` | A `sapling` at `pos` is about to grow into a tree | yes |
| `on_block_ignite(server, player: &PlayerInfo, pos: (i32, i32, i32))` | Player is about to light a fire at `pos` with flint and steel | yes |
| `on_button_press(server, player: &PlayerInfo, pos: (i32, i32, i32), block: &str)` | Player is pressing the `block` button at `pos` | yes |
| `on_pressure_plate(server, pos: (i32, i32, i32), signal: i32)` | The pressure plate at `pos` went down, giving `signal` (1-15) |  |
| `on_tripwire_trigger(server, pos: (i32, i32, i32))` | Something walked into the tripwire at `pos` |  |
| `on_target_hit(server, pos: (i32, i32, i32), signal: i32)` | A projectile hit the target block at `pos`, giving `signal` (1-15) |  |
| `on_sculk_sensor(server, pos: (i32, i32, i32), player: Option<i64>)` | The sculk sensor at `pos` heard something (`player` set when a player made it) |  |
| `on_log_strip(server, pos: (i32, i32, i32), player: Option<i64>)` | A log or stem at `pos` was stripped with an axe |  |
| `on_copper_wax(server, pos: (i32, i32, i32), player: Option<i64>, waxed: bool)` | Copper at `pos` was waxed (`waxed` true) or had its wax taken off |  |
| `on_copper_scrape(server, pos: (i32, i32, i32), player: Option<i64>)` | Copper at `pos` was scraped back one weathering stage |  |
| `on_path_create(server, pos: (i32, i32, i32), player: Option<i64>)` | A shovel turned the block at `pos` into a path |  |
| `on_till(server, pos: (i32, i32, i32), player: Option<i64>)` | A hoe tilled the block at `pos` |  |
| `on_campfire_extinguish(server, pos: (i32, i32, i32), player: Option<i64>)` | The campfire at `pos` was put out with a shovel |  |
| `on_villager_restock(server, villager: i64)` | Villager `villager` restocked its trades |  |
| `on_pvp_attack(server, attacker: &PlayerInfo, victim: &PlayerInfo, damage: f32)` | Player `attacker` is about to hit `victim` for `damage` | yes |
| `on_mending_repair(server, player: &PlayerInfo, points: i32)` | Player's mending gear soaked up `points` of picked-up XP to repair itself |  |
| `on_idle_kick(server, player: &PlayerInfo)` | Player is about to be kicked for being idle | yes |
| `on_spawn_protection(server, name: &str, pos: (i32, i32, i32))` | Spawn protection is about to stop `name` building or breaking at `pos` | yes |
| `on_hopper_move(server, from: (i32, i32, i32), to: (i32, i32, i32))` | A hopper moved an item from `from` to `to` |  |
| `on_jukebox_play(server, pos: (i32, i32, i32), record: &str, player: Option<i64>)` | The jukebox at `pos` started playing `record` (like "record.cat") |  |
| `on_note_tune(server, pos: (i32, i32, i32), player: Option<i64>)` | Player clicked the note block at `pos`, tuning it up a note |  |
| `on_honey_harvest(server, pos: (i32, i32, i32), player: Option<i64>)` | The beehive or nest at `pos` was sheared for honeycomb |  |
| `on_pumpkin_carve(server, pos: (i32, i32, i32), player: Option<i64>)` | The pumpkin at `pos` was carved with shears |  |
| `on_berry_pick(server, pos: (i32, i32, i32), player: Option<i64>)` | Berries were picked from the bush or cave vines at `pos` |  |
| `on_spawn_egg_use(server, player: &PlayerInfo, kind: &str, pos: (f32, f32, f32))` | Player is about to use a spawn egg for `kind` at `pos` | yes |
| `on_mob_feed(server, player: i64, mob: i64)` | Player `player` fed `mob` its breeding food, so it's ready to breed |  |
| `on_baby_feed(server, player: i64, mob: i64)` | Player `player` fed the baby `mob`, making it grow faster |  |
| `on_mob_saddle(server, player: i64, mob: i64)` | Player `player` put a saddle on `mob` |  |
| `on_cow_milk(server, player: &PlayerInfo, mob: i64)` | Player is about to milk cow `mob` with a bucket | yes |
| `on_creeper_ignite(server, player: i64, mob: i64)` | Player `player` lit creeper `mob` with flint and steel |  |
| `on_cure_start(server, player: i64, mob: i64)` | Player `player` gave zombie villager `mob` a golden apple and the cure started |  |
| `on_map_create(server, player: &PlayerInfo)` | Player is about to turn an empty map into a map of the area | yes |
| `on_dialog_open(server, player: &PlayerInfo, npc: &str)` | An NPC dialog from `npc` (its name) is about to open for the player | yes |
| `on_dialog_answer(server, player: &PlayerInfo, scene: &str, button: Option<usize>)` | Player answered NPC dialog `scene` with `button` (None when closed) |  |
| `on_player_kill(server, killer: &PlayerInfo, victim: &PlayerInfo)` | Player `killer` killed player `victim` |  |
| `on_say_command(server, sender: &str, args: &str)` | `sender` (a player's name, or CONSOLE) is about to run /say with `args` | yes |
| `on_me_command(server, sender: &str, args: &str)` | `sender` (a player's name, or CONSOLE) is about to run /me with `args` | yes |
| `on_whisper_command(server, sender: &str, args: &str)` | `sender` (a player's name, or CONSOLE) is about to run /tell, /msg and /w with `args` | yes |
| `on_give_command(server, sender: &str, args: &str)` | `sender` (a player's name, or CONSOLE) is about to run /give with `args` | yes |
| `on_summon_command(server, sender: &str, args: &str)` | `sender` (a player's name, or CONSOLE) is about to run /summon with `args` | yes |
| `on_setblock_command(server, sender: &str, args: &str)` | `sender` (a player's name, or CONSOLE) is about to run /setblock with `args` | yes |
| `on_fill_command(server, sender: &str, args: &str)` | `sender` (a player's name, or CONSOLE) is about to run /fill with `args` | yes |
| `on_kill_command(server, sender: &str, args: &str)` | `sender` (a player's name, or CONSOLE) is about to run /kill with `args` | yes |
| `on_enchant_command(server, sender: &str, args: &str)` | `sender` (a player's name, or CONSOLE) is about to run /enchant with `args` | yes |
| `on_clear_command(server, sender: &str, args: &str)` | `sender` (a player's name, or CONSOLE) is about to run /clear with `args` | yes |
| `on_setworldspawn_command(server, sender: &str, args: &str)` | `sender` (a player's name, or CONSOLE) is about to run /setworldspawn with `args` | yes |
| `on_title_command(server, sender: &str, args: &str)` | `sender` (a player's name, or CONSOLE) is about to run /title and /titleraw with `args` | yes |
| `on_tellraw_command(server, sender: &str, args: &str)` | `sender` (a player's name, or CONSOLE) is about to run /tellraw with `args` | yes |
| `on_scoreboard_command(server, sender: &str, args: &str)` | `sender` (a player's name, or CONSOLE) is about to run /scoreboard with `args` | yes |
| `on_effect_command(server, sender: &str, args: &str)` | `sender` (a player's name, or CONSOLE) is about to run /effect with `args` | yes |
| `on_damage_modify(server, player, cause: &str, amount: &mut f32)` | Change how much damage the player takes | |
| `on_mob_damage_modify(server, mob, kind: &str, attacker: Option<i64>, amount: &mut f32)` | Change how much damage a mob takes | |
| `on_block_drops(server, player, pos, block: &str, drops: &mut Vec<(String, i8)>)` | Change what a broken block drops | |
| `on_mob_drops(server, kind: &str, pos, killer: Option<i64>, drops: &mut Vec<(String, i8)>)` | Change what a dying mob drops | |
| `on_block_xp(server, player, pos, block: &str, xp: &mut i32)` | Change the XP from breaking a block | |
| `on_mob_xp(server, kind: &str, pos, killer: Option<i64>, xp: &mut i32)` | Change the XP a killed mob drops | |
| `on_knockback(server, player, knock: &mut (f32, f32, f32))` | Change the knockback from a hit | |
| `on_packet_send(server, player, packet: &str)` | A packet is about to go to the player | yes |
| `on_permission_check(server, player: &str, node: &str, allowed: &mut bool)` | Decide whether a player has a permission | |
| `on_plugin_enable(server, name: &str)` | A plugin was loaded, at startup | |
| `on_book_edit(server, player, page: i32, text: &str)` | Writing in a book and quill | yes |
| `on_book_sign(server, player, title: &str)` | Signing a book and quill | yes |

## Example: a no-grief zone

```rust
fn on_block_break(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32), _block: i32) -> bool {
    let (x, _, z) = pos;
    let in_spawn = x.abs() < 50 && z.abs() < 50;
    if in_spawn {
        server.tell(player.id, "§cYou can't build at spawn.");
    }
    !in_spawn
}
```

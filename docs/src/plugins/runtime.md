# Runtime plugins (.wasm)

Runtime plugins load without rebuilding Rusted: drop a `.wasm` file into the server's
`plugins/` folder and restart. They run in a sandbox, so a broken or hostile plugin can't
crash the server or touch files. If one gets stuck or panics, Rusted turns that plugin off
and keeps going.

Compiled-in plugins (the rest of this guide) can do more: custom blocks, items and mobs,
forms and scoreboards. Runtime plugins get every event too.

## Making one

You need Rust and the WebAssembly target:

```bash
rustup target add wasm32-unknown-unknown
```

Start from `examples/wasm-hello` in the Rusted repo, which uses the `sdk/rusted-plugin` library:

```rust
use rusted_plugin::{Event, plugin};

fn init() {
    rusted_plugin::register_command("hello", "Says hello back");
}

fn event(e: &Event) -> bool {
    match e.kind() {
        "join" => rusted_plugin::broadcast(&format!("Welcome, {}!", e.player_name())),
        "command" if e.command() == "hello" => rusted_plugin::reply("Hello!"),
        "block_break" if e.block() == "minecraft:bedrock" => return false, // cancel it
        _ => {}
    }
    true
}

plugin!(init, event);
```

Build it and copy the file over:

```bash
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/hello.wasm /path/to/server/plugins/
```

The plugin is named after its file.

## Events

Each event is JSON. `e.kind()` tells you which one; player events have `player` with `id`,
`name`, `xuid`, `pos` and `rot`. Return `false` to cancel the ones marked *cancellable*.

| Event | Extra fields | |
|---|---|---|
| `server_start`, `server_stop` | | |
| `join`, `leave`, `respawn` | | |
| `chat` | `message` | cancellable; `reply(text)` changes the message |
| `command_preprocess` | `line` | cancellable; `reply(text)` rewrites the command |
| `command` | `command`, `args` | one of your commands; `reply(text)` answers |
| `block_break`, `block_place` | `pos`, `block` | cancellable |
| `interact` | `pos`, `item` | cancellable |
| `player_damage` | `cause`, `amount` | cancellable |
| `player_death` | `message` | `reply(text)` changes the death message |
| `sneak` | `on` | |
| `mount` | `mob` | |
| `trade` | `villager`, `index`, `times` | |
| `tick` |  | 20 times a second, keep it quick |
| `gamemode_change` | `mode` | cancellable |
| `item_drop` | `item`, `count` | cancellable |
| `entity_damage` | `mob`, `kind`, `damage` | cancellable |
| `entity_death` | `kind`, `pos`, `killer` |  |
| `entity_spawn` | `kind`, `pos` | cancellable |
| `explosion` | `pos`, `power` | cancellable |
| `weather_change` | `weather` | cancellable |
| `item_use` | `item`, `count` | cancellable |
| `move` | `to` | cancellable; every movement packet, keep it quick |
| `item_pickup` | `item`, `count` | cancellable |
| `sprint` | `sprinting` |  |
| `container_open` | `pos`, `block` | cancellable |
| `consume` | `item` | cancellable |
| `held_change` | `slot`, `item` |  |
| `dimension_change` | `to` |  |
| `teleport` | `to` |  |
| `xp_change` | `points` | `reply(n)` sets the points |
| `sign_change` | `pos`, `front`, `back` | cancellable; reply with the front and back separated by a line break to rewrite them |
| `bed_enter` | `pos` | cancellable |
| `bed_leave` |  |  |
| `bucket_fill` | `pos`, `filled` | cancellable |
| `bucket_empty` | `pos`, `bucket` | cancellable |
| `fish` | `item` | cancellable |
| `glide` | `gliding` |  |
| `swim` | `swimming` |  |
| `container_close` | `kind` |  |
| `item_break` | `item` |  |
| `kick` | `message` |  |
| `dismount` |  |  |
| `craft` | `item`, `count` | cancellable |
| `projectile_hit` | `shooter`, `target` (`{"mob": id}` or `{"player": id}`) | cancellable |
| `block_grow` | `pos`, `from`, `to` | cancellable |
| `mob_target` | `mob`, `kind` | cancellable |
| `block_burn` | `pos`, `block`, `spread` | cancellable |
| `explosion_blocks` | `at`, `blocks` | `reply("[[x,y,z],...]")` replaces the list |
| `entity_tame` | `mob`, `kind` | cancellable |
| `entity_breed` | `kind`, `parents` | cancellable |
| `portal_create` | `kind` | cancellable |
| `mob_damage` | `mob`, `kind`, `damage`, `from` | cancellable |
| `inventory_action` | `action`, `container` | cancellable |
| `chunk_load` | `chunk` |  |
| `entity_click` | `entity`, `attack` |  |
| `jump` |  |  |
| `toggle_flight` | `flying` |  |
| `animation` | `action` | cancellable (hides it from other players) |
| `form_response` | `form_id`, `answer` | `answer` is the raw JSON, null when closed |
| `packet_receive` | `packet` | cancellable; every packet but movement (use `move`) |
| `world_save` |  |  |
| `liquid_flow` | `pos`, `block` | cancellable |
| `projectile_launch` | `trident`, `force` |  |
| `table_craft` | `table`, `item`, `levels` | enchanting, anvil, grindstone, smithing, loom, stonecutter, cartography, crafting table |
| `brew` | `pos` |  |
| `entity_combust` | `mob`, `kind` |  |
| `entity_despawn` | `mob`, `kind` |  |
| `piston` | `pos`, `extending` |  |
| `health_regain` | `health` | cancellable |
| `food_change` | `food` |  |
| `leaves_decay` | `pos`, `block` | cancellable |
| `block_spread` | `pos`, `from`, `to` | cancellable |
| `block_form` | `pos`, `block` | cancellable; snow, ice, and obsidian or cobblestone from lava |
| `block_fade` | `pos`, `from`, `to` | cancellable; snow and ice melting |
| `block_toggle` | `pos`, `block`, `on` | cancellable; doors, trapdoors, gates, levers |
| `vehicle_place` | `kind`, `pos` | cancellable |
| `vehicle_destroy` | `kind`, `pos`, `killer` |  |
| `emote` | `emote` | cancellable (others don't see it) |
| `skin_change` | `skin` |  |
| `lightning` | `pos` | cancellable |
| `item_despawn` | `item`, `count`, `pos` |  |
| `entity_transform` | `mob`, `from`, `to`, `new_mob` |  |
| `mob_shoot` | `mob`, `kind` |  |
| `chunk_unload` | `chunk` |  |
| `toggle_crawl` | `crawling` |  |
| `player_spawn` |  |  |
| `map_request` | `map` | cancellable |
| `block_pick` | `pos`, `block` | cancellable |
| `lectern_page` | `pos`, `page` |  |
| `beacon_change` | `pos`, `primary`, `secondary` | cancellable |
| `farmland_trample` | `pos` | cancellable |
| `crafter_craft` | `pos`, `item`, `count` | cancellable |
| `dispense` | `pos`, `block` | cancellable |
| `note_play` | `pos` | cancellable |
| `bell_ring` | `pos`, `player` |  |
| `raid_start` | `center` | cancellable |
| `raid_wave` | `center`, `wave` |  |
| `raid_end` | `center`, `victory` |  |
| `totem_use` |  | cancellable |
| `sheep_shear` | `player`, `mob` |  |
| `sheep_regrow` | `mob` |  |
| `leash` | `player`, `mob`, `attached` |  |
| `villager_level_up` | `villager`, `tier` |  |
| `spawner_spawn` | `pos`, `kind` | cancellable |
| `creeper_power` | `mob` |  |
| `rcon_command` | `addr`, `line` | cancellable |
| `console_command` | `line` | cancellable |
| `furnace_smelt` | `pos` |  |
| `tnt_prime` | `pos` | cancellable |
| `effect_add` | `effect`, `amplifier`, `ticks` | cancellable |
| `effect_remove` | `effect` |  |
| `ender_pearl` | `to` | cancellable |
| `item_spawn` | `item`, `count`, `pos` | cancellable |
| `xp_orb_pickup` | `value` |  |
| `golem_build` | `kind`, `pos` | cancellable |
| `trader_spawn` | `pos` | cancellable |
| `patrol_spawn` | `pos` | cancellable |
| `shriek` | `pos`, `player` |  |
| `warden_summon` | `pos` | cancellable |
| `vault_open` | `pos` | cancellable |
| `trial_spawn` | `pos`, `kind`, `mob` |  |
| `gamerule_change` | `rule`, `value` | cancellable |
| `difficulty_change` | `difficulty` | cancellable |
| `op_change` | `name`, `op` | cancellable |
| `ban` | `name`, `reason` | cancellable |
| `unban` | `name` | cancellable |
| `ban_ip` | `ip` | cancellable |
| `whitelist_change` | `name`, `added` | cancellable |
| `world_change` | `world` | cancellable |
| `backup` | `path` |  |
| `dragon_perch` | `mob` |  |
| `night_skip` |  | cancellable |
| `bonemeal` | `pos`, `player` |  |
| `anchor_charge` | `pos`, `player` |  |
| `xp_level_change` | `old`, `new` |  |
| `shield_block` | `damage` | cancellable |
| `cake_eat` | `pos`, `player` |  |
| `dragon_egg_teleport` | `pos`, `to` | cancellable |
| `end_portal_fill` | `pos`, `player` |  |
| `composter_use` | `pos`, `player` |  |
| `mob_heal` | `mob`, `amount` |  |
| `spawn_point_set` | `pos` | cancellable |
| `view_distance` | `chunks` | `reply(n)` sets it |
| `chunk_generate` | `chunk` |  |
| `time_set` | `time` | cancellable |
| `sit_toggle` | `mob`, `sitting` |  |
| `slow_tick` | `ms` |  |
| `armor_change` | `slot`, `item` |  |
| `player_combust` |  |  |
| `death_drops` |  | cancellable |
| `portal_enter` | `to` | cancellable |
| `firework_explode` | `pos`, `stars` |  |
| `tnt_minecart_prime` | `mob` |  |
| `fishing_cast` |  | cancellable |
| `elytra_boost` |  | cancellable |
| `item_throw` | `item` | cancellable |
| `block_explode` | `pos`, `block` | cancellable |
| `sign_open` | `pos` |  |
| `mob_expire` | `mob`, `kind` |  |
| `sapling_grow` | `pos`, `sapling` | cancellable |
| `block_ignite` | `pos` | cancellable |
| `button_press` | `pos`, `block` | cancellable |
| `pressure_plate` | `pos`, `signal` |  |
| `tripwire_trigger` | `pos` |  |
| `target_hit` | `pos`, `signal` |  |
| `sculk_sensor` | `pos`, `player` |  |
| `log_strip` | `pos`, `player` |  |
| `copper_wax` | `pos`, `player`, `waxed` |  |
| `copper_scrape` | `pos`, `player` |  |
| `path_create` | `pos`, `player` |  |
| `till` | `pos`, `player` |  |
| `campfire_extinguish` | `pos`, `player` |  |
| `villager_restock` | `villager` |  |
| `pvp_attack` | `attacker`, `victim`, `damage` | cancellable |
| `mending_repair` | `points` |  |
| `idle_kick` |  | cancellable |
| `spawn_protection` | `name`, `pos` | cancellable |
| `hopper_move` | `from`, `to` |  |
| `jukebox_play` | `pos`, `record`, `player` |  |
| `note_tune` | `pos`, `player` |  |
| `honey_harvest` | `pos`, `player` |  |
| `pumpkin_carve` | `pos`, `player` |  |
| `berry_pick` | `pos`, `player` |  |
| `spawn_egg_use` | `kind`, `pos` | cancellable |
| `mob_feed` | `player`, `mob` |  |
| `baby_feed` | `player`, `mob` |  |
| `mob_saddle` | `player`, `mob` |  |
| `cow_milk` | `mob` | cancellable |
| `creeper_ignite` | `player`, `mob` |  |
| `cure_start` | `player`, `mob` |  |
| `map_create` |  | cancellable |
| `dialog_open` | `npc` | cancellable |
| `dialog_answer` | `scene`, `button` |  |
| `player_kill` | `killer`, `victim` |  |
| `say_command` | `sender`, `args` | cancellable |
| `me_command` | `sender`, `args` | cancellable |
| `whisper_command` | `sender`, `args` | cancellable |
| `give_command` | `sender`, `args` | cancellable |
| `summon_command` | `sender`, `args` | cancellable |
| `setblock_command` | `sender`, `args` | cancellable |
| `fill_command` | `sender`, `args` | cancellable |
| `kill_command` | `sender`, `args` | cancellable |
| `enchant_command` | `sender`, `args` | cancellable |
| `clear_command` | `sender`, `args` | cancellable |
| `setworldspawn_command` | `sender`, `args` | cancellable |
| `title_command` | `sender`, `args` | cancellable |
| `tellraw_command` | `sender`, `args` | cancellable |
| `scoreboard_command` | `sender`, `args` | cancellable |
| `effect_command` | `sender`, `args` | cancellable |
| `damage_modify` | `cause`, `amount` | `reply("2.5")` sets the damage; 0 cancels it |
| `mob_damage_modify` | `mob`, `kind`, `attacker`, `amount` | `reply("2.5")` sets the damage |
| `block_drops` | `pos`, `block`, `drops` | `reply('[["minecraft:diamond", 1]]')` replaces the drops; `[]` for none |
| `mob_drops` | `kind`, `pos`, `killer`, `drops` | same as `block_drops` |
| `block_xp` | `pos`, `block`, `xp` | `reply("10")` sets the XP |
| `mob_xp` | `kind`, `pos`, `killer`, `xp` | `reply("10")` sets the XP |
| `knockback` | `knock` | `reply("[0, 0.4, 0]")` sets the motion |
| `packet_send` | `packet` | cancellable; not sent for chunks, movement and other busy packets |
| `permission_check` | `player`, `node`, `allowed` | `reply("true")` or `reply("false")` decides |
| `plugin_enable` | `name` |  |
| `book_edit` | `page`, `text` | cancellable |
| `book_sign` | `title` | cancellable |
| `frame_place` | `pos`, `item` | cancellable |
| `frame_rotate` | `pos`, `rotation` | cancellable |
| `frame_take` | `pos`, `item` | cancellable |
| `armor_stand_place` | `pos` | cancellable |
| `armor_stand_change` | `mob`, `item` | cancellable; `item` is "" when taking something off |
| `armor_stand_break` | `mob`, `pos`, `killer` |  |

### Changing values

Events whose last field is a value to change (`damage_modify`, `block_drops`, `mob_xp`,
`knockback`, `permission_check` and the like) take the new value as your `reply(...)`: a number,
`true`/`false`, or JSON for lists and positions, as the table shows. No reply leaves it as it is.

## What a plugin can do

| Function | Does |
|---|---|
| `log(text)` | writes to the server console |
| `tell(player_id, text)` | messages one player |
| `broadcast(text)` | messages everyone |
| `run_command(line)` | runs a command as the console |
| `run_as(player_id, line)` | runs a command as a player |
| `reply(text)` | answers a command, or edits a chat or death message |
| `register_command(name, description)` | adds a command (in `init` only) |

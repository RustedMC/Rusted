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

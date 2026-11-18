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

Mob positions are `(f32, f32, f32)`.

## Server

| Method | When |
|---|---|
| `on_server_start(server)` | Worlds loaded, about to accept players. A good place to start [scheduled tasks](scheduler.md). |
| `on_server_stop(server)` | Server stopping, before the final save |

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

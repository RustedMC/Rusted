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

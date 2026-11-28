# Runtime plugins (.wasm)

Runtime plugins load without rebuilding Rusted: drop a `.wasm` file into the server's
`plugins/` folder and restart. They run in a sandbox, so a broken or hostile plugin can't
crash the server or touch files. If one gets stuck or panics, Rusted turns that plugin off
and keeps going.

Compiled-in plugins (the rest of this guide) can do more: custom blocks, items and mobs,
forms, scoreboards and every event. Runtime plugins cover the common things.

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

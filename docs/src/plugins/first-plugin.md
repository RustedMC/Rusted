# Your first plugin

Plugins are Rust code compiled into the server. There's no separate plugin jar or loader
yet: you add your plugin to the source and rebuild.

## 1. Write it

Make a new file, `src/greeter.rs`:

```rust
use crate::players::{PlayerInfo, Server};
use crate::plugin::Plugin;

pub struct Greeter;

impl Plugin for Greeter {
    fn name(&self) -> &str {
        "greeter"
    }

    fn on_join(&self, server: &Server, player: &PlayerInfo) {
        server.broadcast(&format!("§e{} joined the game", player.name));
        server.tell(player.id, "§aWelcome! Type /help to get started.");
    }
}
```

Every event has a default that does nothing, so you only write the ones you need.

## 2. Register it

In `src/main.rs`, add the module next to the others:

```rust
mod greeter;
```

Then add it to `all()` in `src/plugin.rs`:

```rust
pub fn all() -> Vec<Box<dyn Plugin>> {
    vec![Box::new(crate::greeter::Greeter)]
}
```

## 3. Build and test

```bash
cargo run --release
```

Join the server and you'll see the greeting. `/pl` lists loaded plugins, so `greeter` should
show up there.

## Next

- [Events](events.md): everything a plugin can react to
- [Commands](commands.md): adding your own `/commands`
- [Scheduler](scheduler.md): running things later or on a timer

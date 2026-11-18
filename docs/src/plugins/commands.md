# Commands

A plugin adds commands with two methods: `commands()` lists them (for autocomplete), and
`on_command()` runs them.

```rust
impl Plugin for Spawn {
    fn name(&self) -> &str {
        "spawn"
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("spawn", "Teleport to spawn"),
            ("online", "List players online"),
        ]
    }

    fn on_command(&self, server: &Server, player: &PlayerInfo, name: &str, args: &[&str]) -> Option<String> {
        match name {
            "online" => {
                let names: Vec<String> = server.all().into_iter().map(|p| p.name).collect();
                Some(format!("Online ({}): {}", names.len(), names.join(", ")))
            }
            "spawn" => {
                // args are the words after the command: /spawn foo bar -> ["foo", "bar"]
                let _ = args;
                Some("§aOff you go!".into())
            }
            _ => None,
        }
    }
}
```

- `name` is the command without the `/`.
- `args` is the rest of the line, split on spaces.
- Whatever text you return is sent to the player who typed it. Return `None` to send nothing.

If two plugins register the same command, the first one in `all()` handles it.

To change or block *other* commands before they run, use
[`on_command_preprocess`](events.md#players).

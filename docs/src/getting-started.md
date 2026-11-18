# Getting started

## What you need

- [Rust](https://rustup.rs) (stable)
- On Windows, the Visual Studio C++ build tools (rustup offers to install them)
- Git

## Build and run

```bash
git clone https://github.com/RustedMC/Rusted.git
cd Rusted
cargo run --release
```

The first build takes a few minutes. After that the server starts in a couple of seconds.

## Join

In Minecraft, go to **Play → Servers → Add Server** and enter:

- **Address:** your machine's IP (`127.0.0.1` if Minecraft runs on the same PC)
- **Port:** `19132`

## Files it creates

| File / folder | What it is |
|---|---|
| `server.properties` | Settings, see [Configuration](configuration.md) |
| `identity.pem` | The server's private key. **Don't share it.** |
| `worlds/` | Your worlds |
| `players/` | Player inventories, XP and positions |
| `ops.txt`, `whitelist.txt`, `banned-players.txt` | Operators, allowlist and bans |
| `plugins/<name>/config.txt` | Plugin settings |

## Structures

Mansions, end cities, bastions and a few other structures need a template file,
`data/structures.nbt`, which isn't in the repo yet. Without it those structures are skipped
and everything else generates normally.

## Stopping

Press **Ctrl+C**. The server saves the world and every player before it exits.

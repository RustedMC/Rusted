<p align="center">
  <img src="assets/banner.png" alt="Rusted" width="100%">
</p>

<p align="center">
  <a href="https://github.com/RustedMC/Rusted/actions/workflows/build.yml"><img src="https://img.shields.io/github/actions/workflow/status/RustedMC/Rusted/build.yml?style=for-the-badge&label=build&color=b7410e" alt="build"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-LGPL--3.0-8b2e0c?style=for-the-badge" alt="license"></a>
  <img src="https://img.shields.io/badge/bedrock-1.26.60-de6e28?style=for-the-badge" alt="bedrock version">
</p>

A Minecraft: Bedrock Edition server written in Rust, ported from
[PowerNukkitX](https://github.com/PowerNukkitX/PowerNukkitX).

It's an alpha. You can join, survive, build redstone, go to the Nether and the End and trade
with villagers, but plenty is still missing or simpler than in PNX. [PORTING.md](PORTING.md)
has the honest breakdown, part by part.

## Running it

You need [Rust](https://rustup.rs) (and the Visual Studio C++ build tools on Windows).

```bash
git clone https://github.com/RustedMC/Rusted.git
cd Rusted
cargo run --release
```

Then add a server in Minecraft with your machine's address, port `19132`.

The first run creates `server.properties` (port, MOTD, seed, game mode, difficulty,
view distance, allowlist) and `identity.pem`. Don't share `identity.pem`, it's the server's
private key.

Some generated structures (mansions, end cities, bastions, ...) need a structure template
file, `data/structures.nbt`, which isn't in the repo yet because its license is unclear.
Without it those structures are just skipped.

## What's in

- **World**: PNX's overworld, Nether and End generators, seed-exact, with structures
- **Survival**: hunger, XP, enchanting, brewing, anvils, smithing, effects, bows and tridents
- **Redstone**: dust, repeaters, comparators, observers, pistons, hoppers, dispensers
- **Mobs**: 71 types with biome spawning, villager trading, raids, the dragon fight
- **Admin**: vanilla commands with target selectors, scoreboards, ops, allowlist, bans

What's missing (riding, loot tables, crossbows, beacons, ...) is listed in
[ROADMAP.md](ROADMAP.md).

## Plugins

Plugins are compiled in. Implement `Plugin` and override the events you want:

```rust
pub struct SpawnGreeter;

impl Plugin for SpawnGreeter {
    fn name(&self) -> &str { "spawn-greeter" }

    fn on_join(&self, server: &Server, player: &PlayerInfo) {
        server.broadcast(&format!("{} joined the game", player.name));
    }
}
```

Then add it to `plugin::all()` in [`src/plugin.rs`](src/plugin.rs), which lists the events.

## Contributing

Issues and pull requests are welcome, see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

By Gardery, under the [LGPL-3.0](LICENSE) like PowerNukkitX. Networking uses
[bedrock-rs](https://github.com/bedrock-crustaceans/bedrock-rs). Credits in
[NOTICE.md](NOTICE.md).

Not affiliated with Mojang or Microsoft.

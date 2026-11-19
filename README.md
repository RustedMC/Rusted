<p align="center">
  <img src="assets/banner.png" alt="Rusted" width="100%">
</p>

<p align="center">
  <a href="https://github.com/RustedMC/Rusted/actions/workflows/build.yml"><img src="https://img.shields.io/github/actions/workflow/status/RustedMC/Rusted/build.yml?style=for-the-badge&label=build&color=b7410e" alt="build"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-LGPL--3.0-8b2e0c?style=for-the-badge" alt="license"></a>
  <img src="https://img.shields.io/badge/bedrock-1.26.60-de6e28?style=for-the-badge" alt="bedrock version">
  <a href="https://discord.gg/HBegxHxREc"><img src="https://img.shields.io/badge/discord-join-5865F2?style=for-the-badge&logo=discord&logoColor=white" alt="discord"></a>
</p>

<p align="center">
  <a href="https://discord.gg/HBegxHxREc"><img src="assets/discord-button.svg" alt="Join the Discord" height="48"></a>&nbsp;&nbsp;
  <a href="https://rustedmc.github.io/Rusted/"><img src="assets/docs-button.svg" alt="Read the docs" height="48"></a>
</p>

A Minecraft: Bedrock Edition server written in Rust, ported from
[PowerNukkitX](https://github.com/PowerNukkitX/PowerNukkitX).

<p align="center"><img src="assets/promo/features.png" alt="What's in Rusted" width="100%"></p>

It's an alpha. You can join, survive, build redstone, go to the Nether and the End and trade
with villagers, but plenty is still missing or simpler than in PNX. [PORTING.md](PORTING.md)
has the honest breakdown, part by part.

## Running it

**Just want to play?** Download the server for Windows or Linux from the
[latest release](https://github.com/RustedMC/Rusted/releases/latest), unzip it and run
`start.bat` (Windows) or `./start.sh` (Linux). A console opens where you type commands like
`help`, `op <name>` and `stop`, the same as PocketMine or PowerNukkitX. Then add a server in
Minecraft with your machine's address, port `19132`.

To build it yourself, you need [Rust](https://rustup.rs) (and the Visual Studio C++ build tools on Windows).

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

## Hosting on a VPS

On a Linux server (Debian/Ubuntu shown):

```bash
sudo apt install build-essential git
curl https://sh.rustup.rs -sSf | sh
git clone https://github.com/RustedMC/Rusted.git && cd Rusted
cargo build --release

sudo useradd -r -m -d /opt/rusted rusted
sudo cp target/release/rusted /opt/rusted/ && sudo cp -r data /opt/rusted/
sudo chown -R rusted: /opt/rusted
sudo cp deploy/rusted.service /etc/systemd/system/
sudo systemctl enable --now rusted
```

Open UDP **and** TCP port `19132` in the firewall (`sudo ufw allow 19132`), and in your
provider's panel if it has one. Newer clients join over TCP, older ones over UDP.

- Logs: `journalctl -u rusted -f`
- Settings: edit `/opt/rusted/server.properties`, then `sudo systemctl restart rusted`
- Stopping saves the world and players first.

To update, pull, rebuild and copy the binary again, then restart.

At home, the same steps work on any Linux box. You'll also need to forward port 19132
(UDP and TCP) on your router so friends outside your network can join.

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

Then add it to `plugin::all()` in [`src/plugin.rs`](src/plugin.rs). The [plugin docs](https://rustedmc.github.io/Rusted/plugins/first-plugin.html)
cover every event, commands, the scheduler, config files and forms.

## Contributing

Issues and pull requests are welcome, see [CONTRIBUTING.md](CONTRIBUTING.md).
Questions or just want to hang out? Join the [Discord](https://discord.gg/HBegxHxREc).

## License

By Gardery, under the [LGPL-3.0](LICENSE) like PowerNukkitX. Networking uses
[bedrock-rs](https://github.com/bedrock-crustaceans/bedrock-rs). Credits in
[NOTICE.md](NOTICE.md).

Not affiliated with Mojang or Microsoft.

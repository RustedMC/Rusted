# Configuration

Settings live in `server.properties`, created with defaults on the first run. It uses the
same `key=value` format as vanilla and PNX. Restart the server after editing.

| Key | Default | What it does |
|---|---|---|
| `server-port` | `19132` | Port players connect to (UDP and TCP) |
| `motd` | `Rusted` | Server name in the server list |
| `sub-motd` | `Rusted` | Second line in the server list |
| `max-players` | `35` | Player limit |
| `level-name` | `world` | World folder under `worlds/` |
| `level-type` | `normal` | `normal`, `legacy`, `flat`, `void`, or a plugin generator. Only used when the world is created. |
| `level-seed` | *(empty)* | Number or text. Empty picks a random seed. Only used when the world is created. |
| `flat-layers` | `bedrock,2*dirt,grass_block` | Layers for `flat` worlds, bottom up |
| `gamemode` | `survival` | Default game mode for new players |
| `difficulty` | `normal` | `peaceful`, `easy`, `normal` or `hard` |
| `view-distance` | `16` | Max chunk radius sent to each player (2–96) |
| `white-list` | `false` | Only players in `whitelist.txt` can join |
| `spawn-protection` | `16` | Radius around the world spawn where only operators can build. `0` turns it off |
| `pvp` | `true` | Players can hurt each other |
| `plugins` | `false` | Loads the built-in plugins (lobby, economy, ranks, kits, homes, auction house, scoreboard...). Off so a fresh server is plain vanilla |
| `backup-interval` | `60` | Minutes between world backups into `backups/`. `0` turns them off |
| `backup-keep` | `5` | How many backups to keep; the oldest are deleted |
| `player-idle-timeout` | `0` | Kick players who don't move for this many minutes. `0` = never. Operators are never kicked |
| `join-message` | `§e{player} joined the game` | Chat message when someone joins. `{player}` is their name; empty for none |
| `leave-message` | `§e{player} left the game` | Same, when someone leaves |
| `enable-rcon` | `false` | Remote console for hosting panels and tools like `mcrcon` |
| `rcon.port` | `25575` | TCP port for RCON |
| `rcon.password` | *(empty)* | Required: RCON won't start without one |

## Console

Run `start.bat` (Windows) or `./start.sh` (Linux) and type commands straight into the window,
without a `/`: `help`, `op Steve`, `say hello`, `stop`. Commands from the console run as an
operator. If the server crashes, the start script starts it again; `stop` shuts it down for
good.

## Remote console (RCON)

Set `enable-rcon=true` and a strong `rcon.password`, restart, and connect with any RCON client,
for example `mcrcon -H your.server -P 25575 -p yourpassword "list"`. Don't open the RCON port to
the internet unless you need to.

## Operators

Players in `ops.txt` (one name per line) can use admin commands. Add someone in game with
`/op <name>`, or from the console.

## Allowlist and bans

- `/whitelist on`, `/whitelist add <name>`, `/whitelist remove <name>`
- `/ban <name>`, `/pardon <name>`

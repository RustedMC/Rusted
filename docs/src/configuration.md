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

## Operators

Players in `ops.txt` (one name per line) can use admin commands. Add someone in game with
`/op <name>`, or from the console.

## Allowlist and bans

- `/whitelist on`, `/whitelist add <name>`, `/whitelist remove <name>`
- `/ban <name>`, `/pardon <name>`

# The server handle

Every event gets `server: &Server`. It's how a plugin talks to the game.

## Players

| Call | Does |
|---|---|
| `server.tell(id, "text")` | Message one player |
| `server.broadcast("text")` | Message everyone |
| `server.get(id) -> Option<PlayerInfo>` | Look up a player by id |
| `server.find_by_name("Steve") -> Option<i64>` | Player id from a name |
| `server.all() -> Vec<PlayerInfo>` | Everyone online |

Messages support Minecraft colour codes with `§`, like `§a` green, `§c` red, `§e` yellow and
`§l` bold.

## `PlayerInfo`

| Field | Type | |
|---|---|---|
| `id` | `i64` | Runtime id, use it with `tell`, `get` and forms |
| `name` | `String` | Gamertag |
| `uuid` | `Uuid` | |
| `xuid` | `String` | Xbox user id, stable across name changes |
| `pos` | `(f32, f32, f32)` | Position |
| `rot` | `(f32, f32)` | Pitch and yaw |

## World

| Call | Does |
|---|---|
| `server.set_block(by, x, y, z, block)` | Place a block and update it. `by` is the player id responsible, or `0` |
| `server.set_block_quiet(by, x, y, z, block)` | Same, without block updates |
| `server.occupied(x, y, z) -> bool` | Is there a player or mob in that block |
| `server.time() -> i32` | Time of day in ticks |
| `server.set_time(ticks)` | Set the time of day |

## Other handles

- `server.scheduler`: run code later or on a timer, see [Scheduler](scheduler.md)
- `server.plugins`: the loaded plugins
- `server.world`, `server.mobs`, `server.containers`: lower-level access to the world, mobs
  and chests. These are less stable than the calls above and may change between versions.

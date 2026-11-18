# Commands

Rusted uses the vanilla Bedrock commands and syntax, so most guides for vanilla commands
apply here. A few examples:

| Command | Example |
|---|---|
| `/gamemode` | `/gamemode creative @s` |
| `/tp` | `/tp Steve 0 80 0` |
| `/give` | `/give @p diamond 5` |
| `/time` | `/time set night` |
| `/weather` | `/weather rain` |
| `/gamerule` | `/gamerule keepInventory true` |
| `/effect` | `/effect @a speed 30 1` |
| `/summon` | `/summon zombie ~ ~ ~` |
| `/scoreboard` | `/scoreboard objectives add kills playerKillCount Kills` |
| `/whitelist` | `/whitelist add Steve` |

Two commands are Rusted's own:

- `/pl` lists loaded plugins
- `/ver` shows the Rusted version

## Target selectors

`@p` (nearest), `@a` (everyone), `@r` (random) and `@s` (yourself) work, with filters like
`name=`, `r=`, `c=` and `scores=`:

```
/tp @a[name=!Steve] 0 80 0
/give @a[r=10] bread 3
```

Selectors only pick players for now. `@e` works like `@a`, mobs aren't selectable yet.

## Not there yet

`/execute`, `/locate` and `/function` aren't ported yet. See [Porting status](porting.md).

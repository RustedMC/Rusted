# Items, menus and player control

## Custom items

`plugin::item` builds an item with a name, lore and hidden tags only plugins can see:

```rust
use crate::plugin;

let compass = plugin::item("minecraft:compass")?
    .name("§aServer selector")
    .lore(&["§7Right-click to pick a game"])
    .tag("menu", "servers")
    .build();
server.give(player.id, compass);
```

The tags stay on the item in the player's inventory. Read them back with
`plugin::tag(&item, "menu")`.

## Reacting to an item

`on_item_use` runs whenever a player right-clicks with something in hand, in the air or on
a block. Return `false` to stop the normal action (placing, eating, shooting):

```rust
fn on_item_use(&self, server: &Server, player: &PlayerInfo, item: &Stack) -> bool {
    if plugin::tag(item, "menu").as_deref() == Some("servers") {
        open_selector(server, player.id);
        return false;
    }
    true
}
```

## Chest menus

`ui::menu` shows a chest-style menu. Players can't take items out of it. Each click calls
your function with the slot number (0-26). Return `false` to close the menu:

```rust
use crate::ui;

fn open_selector(server: &Server, to: i64) {
    let icon = |name: &str, label: &str| plugin::item(name).map(|i| i.name(label).build());
    let mut items = vec![None; 27];
    items[11] = icon("minecraft:diamond_sword", "§cPvP");
    items[13] = icon("minecraft:grass_block", "§aSurvival");
    items[15] = icon("minecraft:bow", "§eMinigames");
    ui::menu(server, to, "§lPick a game", items, |server, player, slot| {
        match slot {
            11 => server.transfer(player.id, "pvp.example.com", 19132),
            13 => server.transfer(player.id, "survival.example.com", 19132),
            15 => server.transfer(player.id, "games.example.com", 19132),
            _ => return true, // empty slot: keep the menu open
        }
        false
    });
}
```

Calling `ui::menu` again while one is open swaps its items, which is how you make
multi-page menus.

## Player control

All of these are on `server`:

| Call | Does |
|---|---|
| `give(id, item)` | Gives an item (from `plugin::item`); what doesn't fit drops at their feet |
| `teleport(id, (x, y, z))` | Moves them, feet position |
| `title(id, "Title", "subtitle")` | Big text on screen |
| `actionbar(id, "text")` | Text above the hotbar |
| `set_gamemode(id, "creative")` | Survival, creative, adventure or spectator |
| `effect(id, effect, amplifier, seconds)` | Status effect (ids in `effects`) |
| `sound(id, "random.levelup")` | A sound only they hear |
| `kick(id, "reason")` | Kicks them |
| `transfer(id, "address", port)` | Sends them to another server: lobbies, minigames, networks |
| `run_as(id, "any command")` | Runs any vanilla command as them, without the op check |

`run_as` is the catch-all: anything a command can do to a player, a plugin can do too.

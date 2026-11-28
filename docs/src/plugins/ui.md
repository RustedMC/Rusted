# Forms, scoreboards and boss bars

Everything here lives in `crate::ui`.

## Forms

Forms are the pop-up menus Bedrock players see. `ui::show` sends one, and your callback gets
the answer.

### Buttons

```rust
use crate::ui::{self, Form, Response};

let form = Form::Simple {
    title: "Warps".into(),
    content: "Where to?".into(),
    buttons: vec![
        ("Spawn".into(), Some("textures/items/compass_item".into())),
        ("Shop".into(), None),
    ],
};
ui::show(server, player.id, form, |server, player, answer| {
    match answer {
        Response::Button(0) => server.tell(player.id, "Going to spawn"),
        Response::Button(1) => server.tell(player.id, "Going to the shop"),
        _ => {} // closed
    }
});
```

A button image can be a texture path or an `https://` URL.

### Yes / no

```rust
let form = Form::Modal {
    title: "Reset".into(),
    content: "Reset your home?".into(),
    yes: "Reset".into(),
    no: "Cancel".into(),
};
ui::show(server, player.id, form, |server, player, answer| {
    if answer == Response::Choice(true) {
        server.tell(player.id, "Home reset.");
    }
});
```

### Inputs, toggles and sliders

```rust
use crate::ui::Element;

let form = Form::Custom {
    title: "Settings".into(),
    elements: vec![
        Element::Label("Pick your options".into()),
        Element::Input { text: "Nickname".into(), placeholder: "Steve".into(), default: String::new() },
        Element::Toggle { text: "Show sidebar".into(), default: true },
        Element::Slider { text: "Render distance".into(), min: 4.0, max: 32.0, step: 1.0, default: 16.0 },
        Element::Dropdown { text: "Team".into(), options: vec!["Red".into(), "Blue".into()], default: 0 },
    ],
};
ui::show(server, player.id, form, |server, player, answer| {
    if let Response::Values(values) = answer {
        // one JSON value per element, in order. Labels give null.
        let nickname = values[1].as_str().unwrap_or("");
        server.tell(player.id, &format!("Hi {nickname}"));
    }
});
```

| Response | Means |
|---|---|
| `Button(i)` | Simple form, button `i` pressed |
| `Choice(bool)` | Modal, `true` for the first button |
| `Values(vec)` | Custom form, one value per element |
| `Closed` | Player closed the form |

## Scoreboards

Scoreboards work the same as the vanilla `/scoreboard` command. From a plugin:

```rust
ui::set_score(server, "kills", &player.name, 10); // shows right away if displayed
let kills = ui::score("kills", &player.name);      // Option<i32>
```

Create objectives and pick where they show with `/scoreboard objectives add` and
`/scoreboard objectives setdisplay`. Scores are saved with the world.

## Boss bars

```rust
// colours: 0 pink, 1 blue, 2 red, 3 green, 4 yellow, 5 purple, 6 white
ui::boss_bar(server, player.id, "§6Event ends soon", 0.75, 4);
ui::remove_boss_bar(server, player.id);
```

`percent` goes from `0.0` (empty) to `1.0` (full).

## NPC dialogs

A dialog shows the NPC talk screen, with a name, some text and up to six buttons. The player
doesn't need to be near an NPC: Rusted shows them an invisible one while the dialog is open.

```rust
use crate::ui::{self, Dialog};

let dialog = Dialog {
    npc_name: "Quest Giver".into(),
    text: "The mine is full of zombies. Will you help?".into(),
    buttons: vec!["Yes!".into(), "Not now".into()],
};
ui::show_dialog(server, player.id, dialog, |server, player, button| match button {
    Some(0) => server.tell(player.id, "Good luck!"),
    Some(_) => {}
    None => {} // closed without picking
});
```

### `/dialogue` scenes, without code

`/dialogue open <npc> <player> <scene>` opens `dialogues/<scene>.json` from the server folder.
Each button's commands run as the player who pressed it:

```json
{
  "npc_name": "Guide",
  "text": "Welcome! Want a map of the area?",
  "buttons": [
    { "name": "Yes please", "commands": ["give @s empty_map", "say I got a map!"] },
    { "name": "No thanks", "commands": [] }
  ]
}
```

The `<npc>` argument is kept for vanilla compatibility and isn't used.

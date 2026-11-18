# Permissions

Permissions are dot-separated names like `homes.set` or `homes.admin.delete`. A plugin
declares the ones it uses and who gets them by default:

```rust
use crate::plugin::Access;

fn permissions(&self) -> Vec<(&'static str, Access)> {
    vec![
        ("homes.set", Access::Everyone),
        ("homes.others", Access::Ops),
    ]
}
```

| Access | Default |
|---|---|
| `Everyone` | Every player has it |
| `Ops` | Only operators (`ops.txt`) |
| `Nobody` | Nobody, unless granted in `permissions.txt` |

Permissions nobody declared count as `Ops`.

## Protecting a command

```rust
fn command_permission(&self, command: &str) -> Option<&'static str> {
    match command {
        "sethome" => Some("homes.set"),
        "home" => None, // anyone
        _ => None,
    }
}
```

Players without the permission get "You do not have permission to use this command" and your
`on_command` isn't called.

## Checking in code

```rust
if server.plugins.allowed(&player.name, "homes.others") {
    // ...
}
```

## `permissions.txt`

Server owners grant or take away permissions in `permissions.txt`, next to `ops.txt`. It has
one `<player> <permission>` per line:

```
# Steve can do everything in homes, except delete
Steve homes.*
Steve -homes.delete

# Alex has every permission
Alex *
```

- `-` in front takes a permission away, even from ops.
- `homes.*` matches every permission starting with `homes.`.
- The most specific line wins, so `homes.delete` beats `homes.*`, which beats `*`.
- Names aren't case-sensitive.
- The file is re-read automatically when it changes, so no restart is needed.

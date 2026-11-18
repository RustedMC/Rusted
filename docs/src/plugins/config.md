# Config files

Plugins get a simple settings file at `plugins/<name>/config.txt`, with one `key = value` per
line. Lines starting with `#` are comments.

```rust
use crate::plugin::Config;

fn on_server_start(&self, server: &Server) {
    let mut config = Config::load("greeter");
    let message = config.get_or("join-message", "Welcome to the server!");
    let _ = config.save(); // writes the defaults so server owners can see every setting
    // ...
}
```

| Call | Does |
|---|---|
| `Config::load("name")` | Read `plugins/name/config.txt`. A missing file is fine, you get an empty config. |
| `config.get("key") -> Option<&str>` | The value, if set |
| `config.get_or("key", "default") -> String` | The value, or `default`. The default is also added to the file on the next `save()`. |
| `config.set("key", "value")` | Change a value |
| `config.save()` | Write the file, creating the folder if needed |

Values are text. Parse numbers yourself:

```rust
let radius: i32 = config.get_or("spawn-radius", "50").parse().unwrap_or(50);
```

//! Placeholders for plugins, like PlaceholderAPI: `{player}`, `{online}`... in sidebars,
//! chat formats, menus and messages.
//!
//! Built in: `{player}` `{online}` `{max}` `{x}` `{y}` `{z}`. A plugin adds its own with `placeholders::register("kills", |server, p| ...)`.

#![allow(dead_code)] // the plugin API: nothing built in uses it, your plugins do
use crate::players::{PlayerInfo, Server};
use parking_lot::Mutex;

type Source = fn(&Server, &PlayerInfo) -> String;

static EXTRA: Mutex<Vec<(&'static str, Source)>> = Mutex::new(Vec::new());

/// Adds `{name}`. Registering the same name again replaces it.
#[allow(dead_code)] // for plugins
pub fn register(name: &'static str, source: Source) {
    let mut list = EXTRA.lock();
    list.retain(|(n, _)| *n != name);
    list.push((name, source));
}

fn builtin(name: &str, server: &Server, p: &PlayerInfo) -> Option<String> {
    Some(match name {
        "player" => p.name.clone(),
        "online" => server.all().len().to_string(),
        "max" => crate::commands::max_players().to_string(),
        "x" => format!("{:.0}", p.pos.0.floor()),
        "y" => format!("{:.0}", (p.pos.1 - 1.62).floor()),
        "z" => format!("{:.0}", p.pos.2.floor()),
        _ => return None,
    })
}

/// Replaces every known `{name}` in `text`. Unknown ones stay as they are, so plugins can fill
/// their own (`{count:Survival}`) afterwards.
pub fn fill(text: &str, server: &Server, p: &PlayerInfo) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(len) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &after[..len];
        let value = builtin(name, server, p).or_else(|| {
            let source = EXTRA.lock().iter().find(|(n, _)| *n == name).map(|(_, s)| *s);
            source.map(|s| s(server, p))
        });
        match value {
            Some(v) => out.push_str(&v),
            None => out.push_str(&rest[start..start + len + 2]),
        }
        rest = &after[len + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn fills_known_and_keeps_unknown() {
        crate::blocks::init();
        let server = crate::players::Server::new();
        let p = crate::players::tests::sample(1);
        super::register("answer", |_, _| "42".into());
        let out = super::fill("{player} {answer} {count:Survival} {", &server, &p);
        assert_eq!(out, format!("{} 42 {{count:Survival}} {{", p.name));
    }
}

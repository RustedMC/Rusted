//! Parsing command arguments: coordinates, block names, rawtext JSON.

use super::*;

/// Command coordinates: numbers or `~`/`~n` relative to `base` (feet).
pub(crate) fn coords(a: &[&str], base: (f32, f32, f32)) -> Option<(f32, f32, f32)> {
    let one = |s: &str, b: f32| match s.strip_prefix('~') {
        Some("") => Some(b),
        Some(r) => r.parse::<f32>().ok().map(|v| b + v),
        None => s.parse::<f32>().ok().map(|v| if s.contains('.') { v } else { v + 0.5 }),
    };
    let y = match a[1].strip_prefix('~') {
        Some("") => Some(base.1),
        Some(r) => r.parse::<f32>().ok().map(|v| base.1 + v),
        None => a[1].parse::<f32>().ok().map(|v| v + dims::base(dims::of_f(base.1)) as f32),
    };
    Some((one(a[0], base.0)?, y?, one(a[2], base.2)?))
}

/// A block by name ("stone", "minecraft:oak_log"), its default state.
pub(crate) fn block_by_name(n: &str) -> Option<i32> {
    let full = if n.contains(':') { n.to_string() } else { format!("minecraft:{n}") };
    if full == "minecraft:air" {
        return Some(world::AIR);
    }
    blocks::any_state(&full)
}

/// The plain text of a rawtext JSON message (`{"rawtext":[{"text":"..."}]}`); plain text as is.
pub(crate) fn rawtext(json: &str) -> String {
    let mut out = String::new();
    let mut rest = json;
    while let Some(i) = rest.find("\"text\"") {
        rest = &rest[i + 6..];
        let Some(q) = rest.find('"') else { break };
        rest = &rest[q + 1..];
        let mut it = rest.char_indices();
        let mut end = rest.len();
        while let Some((j, c)) = it.next() {
            match c {
                '\\' => {
                    if let Some((_, n)) = it.next() {
                        out.push(if n == 'n' { '\n' } else { n });
                    }
                }
                '"' => {
                    end = j;
                    break;
                }
                c => out.push(c),
            }
        }
        rest = &rest[(end + 1).min(rest.len())..];
    }
    if out.is_empty() && !json.trim_start().starts_with('{') { json.to_string() } else { out }
}

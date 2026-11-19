//! WorldEdit for ops. Type the commands with one or two slashes (`//set stone` works).
//!
//! - Wand (`/wand`, a wooden axe): break a block = position 1, right-click = position 2.
//!   Or `/pos1`, `/pos2` where you stand.
//! - `/set <block>`, `/replace <from> <to>`, `/walls <block>`, `/outline <block>`
//! - `/copy` (relative to where you stand), `/paste`, `/undo`, `/redo`, `/size`
//!
//! Blocks are names like `stone`, `oak_planks`, `minecraft:glass`. Edits are limited to
//! `max-blocks` (500,000) in plugins/worldedit/config.txt.

use crate::players::{PlayerInfo, Server};
use crate::plugin::{self, Config, Plugin};
use parking_lot::Mutex;
use std::collections::HashMap;

const NAME: &str = "worldedit";
type Pos = (i32, i32, i32);
type Change = Vec<(Pos, i32)>;

#[derive(Default)]
struct Session {
    pos1: Option<Pos>,
    pos2: Option<Pos>,
    /// (offset from where you stood, block)
    clipboard: Vec<(Pos, i32)>,
    undo: Vec<Change>,
    redo: Vec<Change>,
}

static SESSIONS: Mutex<Option<HashMap<String, Session>>> = Mutex::new(None);

fn with<T>(player: &str, f: impl FnOnce(&mut Session) -> T) -> T {
    let mut s = SESSIONS.lock();
    f(s.get_or_insert_with(HashMap::new).entry(player.to_lowercase()).or_default())
}

/// The player's selection as (min corner, max corner), for WorldGuard too.
pub fn selection(player: &str) -> Option<(Pos, Pos)> {
    with(player, |s| {
        let (a, b) = (s.pos1?, s.pos2?);
        Some(((a.0.min(b.0), a.1.min(b.1), a.2.min(b.2)), (a.0.max(b.0), a.1.max(b.1), a.2.max(b.2))))
    })
}

fn volume((a, b): (Pos, Pos)) -> i64 {
    (b.0 - a.0 + 1) as i64 * (b.1 - a.1 + 1) as i64 * (b.2 - a.2 + 1) as i64
}

fn max_blocks() -> i64 {
    let mut c = Config::load(NAME);
    let v = c.get_or("max-blocks", "500000").parse().unwrap_or(500_000);
    let _ = c.save();
    v
}

fn block(name: &str) -> Option<i32> {
    let full = if name.contains(':') { name.to_string() } else { format!("minecraft:{name}") };
    crate::blocks::any_state(&full)
}

fn feet(p: &PlayerInfo) -> Pos {
    (p.pos.0.floor() as i32, (p.pos.1 - 1.62).floor() as i32, p.pos.2.floor() as i32)
}

fn holding_wand(p: &PlayerInfo) -> bool {
    crate::players::equipment(p.id).held.as_ref().and_then(|h| plugin::tag(h, NAME)).as_deref() == Some("wand")
}

/// Sets blocks and records what was there for /undo.
fn apply(server: &Server, p: &PlayerInfo, edits: Vec<(Pos, i32)>) -> String {
    let mut before = Vec::with_capacity(edits.len());
    let mut n = 0;
    for ((x, y, z), id) in edits {
        let old = server.world.get(x, y, z);
        if old != id {
            before.push(((x, y, z), old));
            server.set_block_quiet(p.id, x, y, z, id);
            n += 1;
        }
    }
    with(&p.name, |s| {
        s.undo.push(before);
        if s.undo.len() > 10 {
            s.undo.remove(0);
        }
        s.redo.clear();
    });
    format!("§d{n} blocks changed.")
}

fn region(p: &PlayerInfo) -> Result<(Pos, Pos), String> {
    let sel = selection(&p.name).ok_or("§cMake a selection first: /wand, or /pos1 and /pos2.")?;
    if volume(sel) > max_blocks() {
        return Err(format!("§cThat's {} blocks; the limit is {}.", volume(sel), max_blocks()));
    }
    Ok(sel)
}

fn cuboid((a, b): (Pos, Pos), mut keep: impl FnMut(Pos) -> bool) -> Vec<Pos> {
    let mut out = Vec::new();
    for x in a.0..=b.0 {
        for y in a.1..=b.1 {
            for z in a.2..=b.2 {
                if keep((x, y, z)) {
                    out.push((x, y, z));
                }
            }
        }
    }
    out
}

fn command(server: &Server, p: &PlayerInfo, name: &str, args: &[&str]) -> String {
    let need_block = |i: usize| args.get(i).and_then(|b| block(b)).ok_or_else(|| format!("§cUnknown block {}", args.get(i).unwrap_or(&"(none)")));
    let result: Result<String, String> = (|| {
        Ok(match name {
            "wand" => {
                let wand = plugin::item("minecraft:wooden_axe").ok_or("§cNo wooden axe item")?.name("§d§lEdit Wand").lore(&["§7Break: position 1", "§7Right-click: position 2"]).tag(NAME, "wand").build();
                server.give(p.id, wand);
                "§dHere's your wand.".into()
            }
            "pos1" | "pos2" => {
                let at = feet(p);
                with(&p.name, |s| if name == "pos1" { s.pos1 = Some(at) } else { s.pos2 = Some(at) });
                format!("§dPosition {} set to {} {} {}.", &name[3..], at.0, at.1, at.2)
            }
            "size" => {
                let sel = selection(&p.name).ok_or("§cNo selection.")?;
                format!("§d{} blocks ({}x{}x{}).", volume(sel), sel.1 .0 - sel.0 .0 + 1, sel.1 .1 - sel.0 .1 + 1, sel.1 .2 - sel.0 .2 + 1)
            }
            "set" => {
                let id = need_block(0)?;
                let sel = region(p)?;
                apply(server, p, cuboid(sel, |_| true).into_iter().map(|q| (q, id)).collect())
            }
            "replace" => {
                let (from, to) = (need_block(0)?, need_block(1)?);
                let from_name = crate::blocks::entry(from).map(|e| e.name.clone());
                let sel = region(p)?;
                // Match by block name, so any state of the block counts.
                let hits = cuboid(sel, |q| crate::blocks::entry(server.world.get(q.0, q.1, q.2)).map(|e| e.name.clone()) == from_name);
                apply(server, p, hits.into_iter().map(|q| (q, to)).collect())
            }
            "walls" | "outline" => {
                let id = need_block(0)?;
                let sel = region(p)?;
                let (a, b) = sel;
                let edge = |q: Pos| q.0 == a.0 || q.0 == b.0 || q.2 == a.2 || q.2 == b.2 || (name == "outline" && (q.1 == a.1 || q.1 == b.1));
                apply(server, p, cuboid(sel, edge).into_iter().map(|q| (q, id)).collect())
            }
            "copy" => {
                let sel = region(p)?;
                let me = feet(p);
                let clip: Vec<(Pos, i32)> = cuboid(sel, |_| true).into_iter().map(|q| ((q.0 - me.0, q.1 - me.1, q.2 - me.2), server.world.get(q.0, q.1, q.2))).collect();
                let n = clip.len();
                with(&p.name, |s| s.clipboard = clip);
                format!("§d{n} blocks copied.")
            }
            "paste" => {
                let me = feet(p);
                let clip = with(&p.name, |s| s.clipboard.clone());
                if clip.is_empty() {
                    return Err("§cYour clipboard is empty. /copy first.".into());
                }
                apply(server, p, clip.into_iter().map(|(o, id)| ((me.0 + o.0, me.1 + o.1, me.2 + o.2), id)).collect())
            }
            "undo" | "redo" => {
                let change = with(&p.name, |s| if name == "undo" { s.undo.pop() } else { s.redo.pop() }).ok_or(format!("§cNothing to {name}."))?;
                let mut back = Vec::with_capacity(change.len());
                for ((x, y, z), id) in change {
                    back.push(((x, y, z), server.world.get(x, y, z)));
                    server.set_block_quiet(p.id, x, y, z, id);
                }
                let n = back.len();
                with(&p.name, |s| if name == "undo" { s.redo.push(back) } else { s.undo.push(back) });
                format!("§d{name}: {n} blocks.")
            }
            _ => return Err("§cUnknown edit command.".into()),
        })
    })();
    result.unwrap_or_else(|e| e)
}

pub struct WorldEdit;

impl Plugin for WorldEdit {
    fn name(&self) -> &str {
        NAME
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("wand", "WorldEdit: selection wand"),
            ("pos1", "WorldEdit: position 1 at your feet"),
            ("pos2", "WorldEdit: position 2 at your feet"),
            ("size", "WorldEdit: selection size"),
            ("set", "WorldEdit: fill the selection"),
            ("replace", "WorldEdit: replace one block with another"),
            ("walls", "WorldEdit: walls around the selection"),
            ("outline", "WorldEdit: hollow box"),
            ("copy", "WorldEdit: copy the selection"),
            ("paste", "WorldEdit: paste where you stand"),
            ("undo", "WorldEdit: undo your last edit"),
            ("redo", "WorldEdit: redo"),
        ]
    }

    fn on_block_break(&self, server: &Server, player: &PlayerInfo, pos: Pos, _block: i32) -> bool {
        if !holding_wand(player) || !crate::commands::is_op(&player.name) {
            return true;
        }
        with(&player.name, |s| s.pos1 = Some(pos));
        server.tell(player.id, &format!("§dPosition 1: {} {} {}", pos.0, pos.1, pos.2));
        false
    }

    fn on_player_interact(&self, server: &Server, player: &PlayerInfo, pos: Pos, _item: &str) -> bool {
        if !holding_wand(player) || !crate::commands::is_op(&player.name) {
            return true;
        }
        with(&player.name, |s| s.pos2 = Some(pos));
        server.tell(player.id, &format!("§dPosition 2: {} {} {}", pos.0, pos.1, pos.2));
        false
    }

    fn usage(&self, command: &str) -> &'static [&'static str] {
        match command {
            "set" => &["<block>"],
            "replace" => &["<from> <to>"],
            "walls" => &["<block>"],
            "outline" => &["<block>"],
            _ => &[],
        }
    }

    fn on_command(&self, server: &Server, player: &PlayerInfo, name: &str, args: &[&str]) -> Option<String> {
        if !crate::commands::is_op(&player.name) {
            return Some("§cYou do not have permission to use this command".into());
        }
        Some(command(server, player, name, args))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn selection_is_normalised() {
        super::with("we_test", |s| {
            s.pos1 = Some((5, 70, -2));
            s.pos2 = Some((1, 64, 3));
        });
        let sel = super::selection("we_test").unwrap();
        assert_eq!(sel, ((1, 64, -2), (5, 70, 3)));
        assert_eq!(super::volume(sel), 5 * 7 * 6);
    }
}

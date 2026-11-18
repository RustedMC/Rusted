//! Sign text. Placing a sign opens the editor, the client sends text back as block entity
//! data, and we save it in worlds/<level>/signs.txt and show it to everyone.

use std::collections::HashMap;
use std::sync::OnceLock;
use parking_lot::Mutex;

use bedrock::protocol::v662::packets::{BlockActorDataPacket, OpenSignPacket};
use bedrock::protocol::v944::types::NetworkBlockPosition;

use crate::Protocol;

type Pos = (i32, i32, i32);
/// Vanilla is roughly 4 short lines.
const MAX_TEXT: usize = 200;

fn path() -> std::path::PathBuf {
    std::path::Path::new("worlds").join(&crate::config::get().level_name).join("signs.txt")
}

fn all() -> &'static Mutex<HashMap<Pos, (String, String)>> {
    static S: OnceLock<Mutex<HashMap<Pos, (String, String)>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(parse(&std::fs::read_to_string(path()).unwrap_or_default())))
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace('\t', "\\t")
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn parse(text: &str) -> HashMap<Pos, (String, String)> {
    text.lines()
        .filter_map(|l| {
            let mut parts = l.split('\t');
            let mut xyz = parts.next()?.split(' ').map(|v| v.parse::<i32>().ok());
            let pos = (xyz.next()??, xyz.next()??, xyz.next()??);
            Some((pos, (unescape(parts.next().unwrap_or("")), unescape(parts.next().unwrap_or("")))))
        })
        .collect()
}

fn save(map: &HashMap<Pos, (String, String)>) {
    let text: String = map.iter().map(|(p, (f, b))| format!("{} {} {}\t{}\t{}\n", p.0, p.1, p.2, escape(f), escape(b))).collect();
    if let Err(e) = std::fs::write(path(), text) {
        println!("failed to save signs: {e}");
    }
}

pub fn is_sign(block: i32) -> bool {
    crate::blocks::entry(block).is_some_and(|e| e.name.ends_with("_sign") || e.name.ends_with("sign"))
}

fn pos(p: Pos) -> NetworkBlockPosition {
    NetworkBlockPosition { x: p.0, y: p.1, z: p.2 }
}

/// Opens the editor for whoever placed it.
pub fn open_packet(p: Pos) -> Protocol {
    Protocol::OpenSignPacket(Box::new(OpenSignPacket { pos: pos(p), is_front: true }))
}

fn side(text: &str) -> nbtx::Value {
    let c: nbtx::Compound = [
        ("Text", nbtx::Value::String(text.into())),
        ("TextOwner", nbtx::Value::String("".into())),
        ("SignTextColor", nbtx::Value::Int(-16_777_216)),
        ("GlowingText", nbtx::Value::Byte(0)),
        ("PersistFormatting", nbtx::Value::Byte(1)),
        ("HideGlowOutline", nbtx::Value::Byte(0)),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v))
    .collect();
    nbtx::Value::Compound(c)
}

pub fn data_packet(p: Pos, front: &str, back: &str) -> Protocol {
    let mut tags = HashMap::new();
    tags.insert("id".to_string(), nbtx::Value::String("Sign".into()));
    tags.insert("x".to_string(), nbtx::Value::Int(p.0));
    tags.insert("y".to_string(), nbtx::Value::Int(p.1));
    tags.insert("z".to_string(), nbtx::Value::Int(p.2));
    tags.insert("FrontText".to_string(), side(front));
    tags.insert("BackText".to_string(), side(back));
    tags.insert("IsWaxed".to_string(), nbtx::Value::Byte(0));
    Protocol::BlockActorDataPacket(Box::new(BlockActorDataPacket { block_position: pos(p), actor_data_tags: tags }))
}

fn text_of(tags: &HashMap<String, nbtx::Value>, key: &str) -> String {
    match tags.get(key) {
        Some(nbtx::Value::Compound(c)) => match c.iter().find(|(k, _)| k.as_slice() == b"Text") {
            Some((_, nbtx::Value::String(s))) => String::from_utf8_lossy(s).chars().take(MAX_TEXT).collect(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

/// Client edited a sign. Saves it and returns the packet for everyone, or None if there's
/// no sign there (clients can't write arbitrary block entities).
pub fn edited(world: &crate::world::World, p: Pos, tags: &HashMap<String, nbtx::Value>) -> Option<Protocol> {
    if !is_sign(world.get(p.0, p.1, p.2)) {
        return None;
    }
    let (front, back) = (text_of(tags, "FrontText"), text_of(tags, "BackText"));
    let packet = data_packet(p, &front, &back);
    let mut map = all().lock();
    map.insert(p, (front, back));
    save(&map);
    Some(packet)
}

pub fn removed(p: Pos) {
    let mut map = all().lock();
    if map.remove(&p).is_some() {
        save(&map);
    }
}

/// Sign data for these chunks, sent after the chunks.
pub fn in_chunks(chunks: &[(i32, i32)]) -> Vec<Protocol> {
    let map = all().lock();
    map.iter()
        .filter(|(p, _)| chunks.contains(&(p.0 >> 4, p.2 >> 4)))
        .map(|(p, (f, b))| data_packet(*p, f, b))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trips() {
        let m: HashMap<Pos, (String, String)> = [((1, -2, 3), ("Hello\nworld\\".to_string(), "back\tside".to_string()))].into();
        let text: String = m.iter().map(|(p, (f, b))| format!("{} {} {}\t{}\t{}\n", p.0, p.1, p.2, escape(f), escape(b))).collect();
        assert_eq!(parse(&text), m);
        let Protocol::BlockActorDataPacket(p) = data_packet((1, -2, 3), "Hi", "") else { panic!() };
        assert_eq!(text_of(&p.actor_data_tags, "FrontText"), "Hi");
    }
}

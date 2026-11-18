//! Every vanilla block state (kaooot block_palette.nbt, 23,930 of them), so we can go
//! from "this block but facing north" to a network ID.

use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Int(i32),
    Bool(bool), // byte states are all on/off bits
    Str(String),
}

pub struct Entry {
    pub name: String,
    pub states: Vec<(String, State)>,
}

struct Registry {
    entries: Vec<Entry>,
    ids: Vec<i32>,
    by_id: HashMap<i32, usize>,
    by_name: HashMap<String, Vec<usize>>,
}

fn get<'a>(v: &'a nbtx::Value, key: &str) -> Option<&'a nbtx::Value> {
    match v {
        nbtx::Value::Compound(m) => m.iter().find(|(k, _)| k.as_slice() == key.as_bytes()).map(|(_, v)| v),
        _ => None,
    }
}

fn registry() -> &'static Registry {
    static REG: OnceLock<Registry> = OnceLock::new();
    REG.get_or_init(|| {
        let gz = include_bytes!("../data/block_palette.nbt");
        let mut raw = flate2::read::GzDecoder::new(&gz[..]);
        let root = nbtx::from_bytes::<nbtx::BigEndian, nbtx::Value>(&mut raw).expect("block_palette.nbt");
        let Some(nbtx::Value::List(list)) = get(&root, "blocks") else { panic!("block_palette.nbt: no blocks list") };
        let mut reg = Registry { entries: vec![], ids: vec![], by_id: HashMap::new(), by_name: HashMap::new() };
        for b in list.to_values() {
            let Some(nbtx::Value::String(name)) = get(&b, "name") else { continue };
            let Some(nbtx::Value::Int(id)) = get(&b, "network_id") else { continue };
            let mut states: Vec<(String, State)> = match get(&b, "states") {
                Some(nbtx::Value::Compound(m)) => m
                    .iter()
                    .filter_map(|(k, v)| {
                        let v = match v {
                            nbtx::Value::Int(i) => State::Int(*i),
                            nbtx::Value::Byte(b) => State::Bool(*b != 0),
                            nbtx::Value::String(s) => State::Str(String::from_utf8_lossy(s).into_owned()),
                            _ => return None,
                        };
                        Some((String::from_utf8_lossy(k).into_owned(), v))
                    })
                    .collect(),
                _ => vec![],
            };
            states.sort_by(|a, b| a.0.cmp(&b.0));
            let name = String::from_utf8_lossy(name).into_owned();
            let i = reg.entries.len();
            reg.by_id.insert(*id, i);
            reg.by_name.entry(name.clone()).or_default().push(i);
            reg.entries.push(Entry { name, states });
            reg.ids.push(*id);
        }
        reg
    })
}

pub fn entry(id: i32) -> Option<&'static Entry> {
    let r = registry();
    r.by_id.get(&id).map(|&i| &r.entries[i])
}

pub fn has_state(id: i32, key: &str) -> bool {
    entry(id).is_some_and(|e| e.states.iter().any(|(k, _)| k == key))
}

pub fn state(id: i32, key: &str) -> Option<&'static State> {
    entry(id)?.states.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// `id` with some states swapped. Unknown keys are ignored, impossible combos return `id`.
pub fn with(id: i32, changes: &[(&str, State)]) -> i32 {
    let r = registry();
    let Some(&i) = r.by_id.get(&id) else { return id };
    let base = &r.entries[i];
    let mut want = base.states.clone();
    for (k, v) in changes {
        if let Some(slot) = want.iter_mut().find(|(key, _)| key == k) {
            slot.1 = v.clone();
        }
    }
    r.by_name[&base.name]
        .iter()
        .find(|&&j| r.entries[j].states == want)
        .map_or(id, |&j| r.ids[j])
}

/// Any state of a block by name; placement orients it afterwards.
/// `b` as the block `new` (short name), keeping whatever states the two share.
pub fn rename(b: i32, new: &str) -> i32 {
    match any_state(&format!("minecraft:{new}")) {
        Some(base) => {
            let st: Vec<(&str, State)> = entry(b).map(|e| e.states.iter().map(|(k, v)| (k.as_str(), v.clone())).collect()).unwrap_or_default();
            with(base, &st)
        }
        None => b,
    }
}

pub fn any_state(name: &str) -> Option<i32> {
    let r = registry();
    r.by_name.get(name).and_then(|v| v.first()).map(|&i| r.ids[i])
}

/// Decode the palette at startup so the first placement isn't slow.
pub fn init() -> usize {
    registry().entries.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_and_round_trips_states() {
        assert!(init() > 20_000);
        let grass = crate::world::GRASS;
        assert_eq!(entry(grass).unwrap().name, "minecraft:grass_block");
        // infiniburn bedrock is just another state of bedrock
        let lit = with(crate::world::BEDROCK, &[("infiniburn_bit", State::Bool(true))]);
        assert_ne!(lit, crate::world::BEDROCK);
        assert_eq!(state(lit, "infiniburn_bit"), Some(&State::Bool(true)));
        assert_eq!(with(lit, &[("infiniburn_bit", State::Bool(false))]), crate::world::BEDROCK);
        assert_eq!(with(grass, &[("nope", State::Int(1))]), grass);
    }
}

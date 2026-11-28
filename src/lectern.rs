//! Lecterns (PNX `BlockEntityLectern`). The book lives in container storage (one slot, saved
//! with chests); clients get it as block-entity data and open the book screen themselves.

use crate::Protocol;
use crate::inventory::Stack;
use crate::itemdata::{ItemData, Tag};
use bedrock::protocol::v944::types::NetworkBlockPosition;
use std::collections::HashMap;

pub fn is_lectern(block: i32) -> bool {
    crate::blocks::entry(block).is_some_and(|e| e.name == "minecraft:lectern")
}

pub fn is_book(name: &str) -> bool {
    matches!(name, "minecraft:writable_book" | "minecraft:written_book")
}

/// Rusted's item NBT to the protocol's NBT value.
pub(crate) fn value(t: &Tag) -> nbtx::Value {
    use nbtx::{Value as V, ValueList as L};
    match t {
        Tag::Byte(v) => V::Byte(*v),
        Tag::Short(v) => V::Short(*v),
        Tag::Int(v) => V::Int(*v),
        Tag::Long(v) => V::Long(*v),
        Tag::Float(v) => V::Float(*v),
        Tag::Double(v) => V::Double(*v),
        Tag::ByteArray(v) => V::ByteArray(v.clone()),
        Tag::String(v) => V::String(v.as_str().into()),
        Tag::IntArray(v) => V::IntArray(v.clone()),
        Tag::LongArray(v) => V::List(L::Long(v.clone())),
        Tag::Compound(fields) => V::Compound(compound(fields)),
        Tag::List(_, items) => V::List(list(items)),
    }
}

fn compound(fields: &[(String, Tag)]) -> nbtx::Compound {
    let mut c = nbtx::Compound::new();
    for (k, v) in fields {
        c.insert(k.as_str().into(), value(v));
    }
    c
}

fn list(items: &[Tag]) -> nbtx::ValueList {
    use nbtx::ValueList as L;
    match items.first() {
        None => L::End,
        Some(Tag::String(_)) => L::String(items.iter().filter_map(|t| if let Tag::String(s) = t { Some(s.as_str().into()) } else { None }).collect()),
        Some(Tag::Int(_)) => L::Int(items.iter().filter_map(|t| if let Tag::Int(v) = t { Some(*v) } else { None }).collect()),
        Some(Tag::Compound(_)) => L::Compound(items.iter().filter_map(|t| if let Tag::Compound(f) = t { Some(compound(f)) } else { None }).collect()),
        // Books only hold strings and compounds; anything else is dropped.
        _ => L::End,
    }
}

/// Page each lectern is open at.
pub fn pages_open() -> &'static parking_lot::Mutex<HashMap<(i32, i32, i32), i32>> {
    static P: std::sync::OnceLock<parking_lot::Mutex<HashMap<(i32, i32, i32), i32>>> = std::sync::OnceLock::new();
    P.get_or_init(Default::default)
}

/// The book's page count, from its `pages` list.
pub fn pages(book: &Stack) -> i32 {
    match ItemData::read(&book.item.user_data_buffer).get("pages") {
        Some(Tag::List(_, p)) => p.len() as i32,
        _ => 0,
    }
}

/// An item as block-entity NBT (lectern books, framed items): name, count 1, aux, and its
/// own data (pages, enchantments, map ids) as `tag`.
pub fn item_nbt(st: &Stack) -> nbtx::Value {
    let name = crate::items::name_of(st.item.id).unwrap_or("minecraft:air");
    let mut item = vec![
        ("Name".to_string(), Tag::String(name.into())),
        ("Count".to_string(), Tag::Byte(1)),
        ("Damage".to_string(), Tag::Short(st.item.aux_value as i16)),
        ("WasPickedUp".to_string(), Tag::Byte(0)),
    ];
    let nbt = ItemData::read(&st.item.user_data_buffer);
    if !nbt.nbt.is_empty() {
        item.push(("tag".into(), Tag::Compound(nbt.nbt)));
    }
    nbtx::Value::Compound(compound(&item))
}

/// Block-entity data for a lectern, holding `book` open at `page`.
pub fn data(pos: (i32, i32, i32), book: Option<&Stack>, page: i32) -> Protocol {
    use nbtx::Value as V;
    let mut tags: HashMap<String, V> = HashMap::new();
    tags.insert("id".into(), V::String("Lectern".into()));
    tags.insert("x".into(), V::Int(pos.0));
    tags.insert("y".into(), V::Int(pos.1));
    tags.insert("z".into(), V::Int(pos.2));
    tags.insert("isMovable".into(), V::Byte(1));
    tags.insert("hasBook".into(), V::Byte(book.is_some() as i8));
    if let Some(b) = book {
        tags.insert("book".into(), item_nbt(b));
        tags.insert("page".into(), V::Int(page));
        tags.insert("totalPages".into(), V::Int(pages(b).max(1)));
    }
    Protocol::BlockActorDataPacket(Box::new(bedrock::protocol::v662::packets::BlockActorDataPacket {
        block_position: NetworkBlockPosition { x: pos.0, y: pos.1, z: pos.2 },
        actor_data_tags: tags,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lectern_data_carries_the_book_and_page() {
        crate::blocks::init();
        let mut d = ItemData::default();
        let page = |t: &str| Tag::Compound(vec![("text".into(), Tag::String(t.into()))]);
        d.set("pages", Tag::List(10, vec![page("one"), page("two")]));
        let mut item = crate::items::item_by_name("minecraft:writable_book").unwrap();
        item.user_data_buffer = d.write();
        let book = Stack { item, count: 1, net_id: 0 };
        assert_eq!(pages(&book), 2);
        let Protocol::BlockActorDataPacket(p) = data((1, 2, 3), Some(&book), 1) else { panic!() };
        assert!(matches!(p.actor_data_tags.get("hasBook"), Some(nbtx::Value::Byte(1))));
        assert!(matches!(p.actor_data_tags.get("totalPages"), Some(nbtx::Value::Int(2))));
        assert!(matches!(p.actor_data_tags.get("book"), Some(nbtx::Value::Compound(_))));
        let Protocol::BlockActorDataPacket(empty) = data((1, 2, 3), None, 0) else { panic!() };
        assert!(matches!(empty.actor_data_tags.get("hasBook"), Some(nbtx::Value::Byte(0))));
    }
}

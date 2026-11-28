//! Book and quill editing and signing (PNX PlayerEditBookEvent / ItemBookWritable).
//! Pages live in the item's `pages` list as {text, photoname} compounds, like vanilla.

use crate::itemdata::{ItemData, Tag};

/// Bedrock caps a book at 50 pages of 256 characters.
pub const MAX_PAGES: usize = 50;
pub const MAX_PAGE_LEN: usize = 256;

/// One edit from the client's BookEditPacket.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    Replace(usize, String),
    Add(usize, String),
    Delete(usize),
    Swap(usize, usize),
    Sign { title: String, author: String, xuid: String },
}

fn page(text: &str) -> Tag {
    let text: String = text.chars().take(MAX_PAGE_LEN).collect();
    Tag::Compound(vec![("text".into(), Tag::String(text)), ("photoname".into(), Tag::String(String::new()))])
}

/// The pages' text.
pub fn pages(data: &[u8]) -> Vec<String> {
    match ItemData::read(data).get("pages") {
        Some(Tag::List(_, p)) => p
            .iter()
            .map(|t| match t {
                Tag::Compound(c) => c.iter().find(|(k, _)| k == "text").and_then(|(_, v)| if let Tag::String(s) = v { Some(s.clone()) } else { None }).unwrap_or_default(),
                _ => String::new(),
            })
            .collect(),
        _ => vec![],
    }
}

/// Applies a page edit to a writable book's data. None if it's out of range (the client sent
/// something odd) or signing, which `sign` handles.
pub fn edit(data: &[u8], e: &Edit) -> Option<Vec<u8>> {
    let mut p = pages(data);
    match e {
        Edit::Replace(i, t) => {
            if *i >= MAX_PAGES {
                return None;
            }
            if *i >= p.len() {
                p.resize(*i + 1, String::new());
            }
            p[*i] = t.clone();
        }
        Edit::Add(i, t) => {
            if p.len() >= MAX_PAGES || *i > p.len() {
                return None;
            }
            p.insert(*i, t.clone());
        }
        Edit::Delete(i) => {
            if *i >= p.len() {
                return None;
            }
            p.remove(*i);
        }
        Edit::Swap(a, b) => {
            if *a >= p.len() || *b >= p.len() {
                return None;
            }
            p.swap(*a, *b);
        }
        Edit::Sign { .. } => return None,
    }
    let mut d = ItemData::read(data);
    d.set("pages", Tag::List(10, p.iter().map(|t| page(t)).collect()));
    Some(d.write())
}

/// A signed (written) book's data: the pages plus title, author and xuid (vanilla
/// ItemBookWritten tags, generation 0 for an original).
pub fn sign(data: &[u8], title: &str, author: &str, xuid: &str) -> Vec<u8> {
    let mut d = ItemData::read(data);
    let title: String = title.chars().take(16).collect();
    d.set("title", Tag::String(title));
    d.set("author", Tag::String(author.into()));
    d.set("xuid", Tag::String(xuid.into()));
    d.set("generation", Tag::Int(0));
    d.write()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_move_delete_and_sign() {
        let d = edit(&ItemData::default().write(), &Edit::Replace(0, "hello".into())).unwrap();
        let d = edit(&d, &Edit::Add(1, "world".into())).unwrap();
        assert_eq!(pages(&d), vec!["hello", "world"]);
        let d = edit(&d, &Edit::Swap(0, 1)).unwrap();
        assert_eq!(pages(&d), vec!["world", "hello"]);
        let d = edit(&d, &Edit::Delete(0)).unwrap();
        assert_eq!(pages(&d), vec!["hello"]);
        assert!(edit(&d, &Edit::Delete(5)).is_none(), "out of range is refused");
        assert!(edit(&d, &Edit::Replace(MAX_PAGES, "x".into())).is_none());
        let long = "a".repeat(1000);
        assert_eq!(pages(&edit(&d, &Edit::Replace(0, long)).unwrap())[0].len(), MAX_PAGE_LEN);
        let signed = sign(&d, "My Book", "Steve", "123");
        let s = ItemData::read(&signed);
        assert!(matches!(s.get("title"), Some(Tag::String(t)) if t == "My Book"));
        assert!(matches!(s.get("author"), Some(Tag::String(a)) if a == "Steve"));
        assert_eq!(pages(&signed), vec!["hello"], "pages are kept");
    }
}

//! Item NBT (durability, enchants) from the network user-data buffer and back.
//! Layout: i16 -1, u8 version 1, LE NBT compound, then can-place-on / can-destroy as raw bytes.

#[derive(Clone, Debug, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    List(u8, Vec<Tag>),
    Compound(Vec<(String, Tag)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Tag {
    fn id(&self) -> u8 {
        match self {
            Tag::Byte(_) => 1,
            Tag::Short(_) => 2,
            Tag::Int(_) => 3,
            Tag::Long(_) => 4,
            Tag::Float(_) => 5,
            Tag::Double(_) => 6,
            Tag::ByteArray(_) => 7,
            Tag::String(_) => 8,
            Tag::List(..) => 9,
            Tag::Compound(_) => 10,
            Tag::IntArray(_) => 11,
            Tag::LongArray(_) => 12,
        }
    }

    pub fn int(&self) -> Option<i32> {
        match *self {
            Tag::Byte(v) => Some(v as i32),
            Tag::Short(v) => Some(v as i32),
            Tag::Int(v) => Some(v),
            _ => None,
        }
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        if self.0.len() < n {
            return None;
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Some(a)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn arr<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N).and_then(|b| b.try_into().ok())
    }
    fn string(&mut self) -> Option<String> {
        let n = u16::from_le_bytes(self.arr()?) as usize;
        Some(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn len(&mut self) -> Option<usize> {
        usize::try_from(i32::from_le_bytes(self.arr()?)).ok().filter(|&n| n <= 1 << 20)
    }
    fn tag(&mut self, id: u8, depth: u32) -> Option<Tag> {
        if depth > 64 {
            return None;
        }
        Some(match id {
            1 => Tag::Byte(self.u8()? as i8),
            2 => Tag::Short(i16::from_le_bytes(self.arr()?)),
            3 => Tag::Int(i32::from_le_bytes(self.arr()?)),
            4 => Tag::Long(i64::from_le_bytes(self.arr()?)),
            5 => Tag::Float(f32::from_le_bytes(self.arr()?)),
            6 => Tag::Double(f64::from_le_bytes(self.arr()?)),
            7 => {
                let n = self.len()?;
                Tag::ByteArray(self.take(n)?.to_vec())
            }
            8 => Tag::String(self.string()?),
            9 => {
                let t = self.u8()?;
                let n = self.len()?;
                Tag::List(t, (0..n).map(|_| self.tag(t, depth + 1)).collect::<Option<_>>()?)
            }
            10 => {
                let mut v = Vec::new();
                loop {
                    let t = self.u8()?;
                    if t == 0 {
                        break;
                    }
                    let k = self.string()?;
                    v.push((k, self.tag(t, depth + 1)?));
                }
                Tag::Compound(v)
            }
            11 => {
                let n = self.len()?;
                Tag::IntArray((0..n).map(|_| self.arr().map(i32::from_le_bytes)).collect::<Option<_>>()?)
            }
            12 => {
                let n = self.len()?;
                Tag::LongArray((0..n).map(|_| self.arr().map(i64::from_le_bytes)).collect::<Option<_>>()?)
            }
            _ => return None,
        })
    }
}

fn write_str(b: &mut Vec<u8>, s: &str) {
    b.extend((s.len() as u16).to_le_bytes());
    b.extend(s.as_bytes());
}

fn write(b: &mut Vec<u8>, t: &Tag) {
    match t {
        Tag::Byte(v) => b.push(*v as u8),
        Tag::Short(v) => b.extend(v.to_le_bytes()),
        Tag::Int(v) => b.extend(v.to_le_bytes()),
        Tag::Long(v) => b.extend(v.to_le_bytes()),
        Tag::Float(v) => b.extend(v.to_le_bytes()),
        Tag::Double(v) => b.extend(v.to_le_bytes()),
        Tag::ByteArray(v) => {
            b.extend((v.len() as i32).to_le_bytes());
            b.extend(v);
        }
        Tag::String(s) => write_str(b, s),
        Tag::List(t, v) => {
            b.push(if v.is_empty() { *t } else { v[0].id() });
            b.extend((v.len() as i32).to_le_bytes());
            for x in v {
                write(b, x);
            }
        }
        Tag::Compound(v) => {
            for (k, x) in v {
                b.push(x.id());
                write_str(b, k);
                write(b, x);
            }
            b.push(0);
        }
        Tag::IntArray(v) => {
            b.extend((v.len() as i32).to_le_bytes());
            v.iter().for_each(|x| b.extend(x.to_le_bytes()));
        }
        Tag::LongArray(v) => {
            b.extend((v.len() as i32).to_le_bytes());
            v.iter().for_each(|x| b.extend(x.to_le_bytes()));
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct ItemData {
    pub nbt: Vec<(String, Tag)>,
    rest: Vec<u8>,
}

impl ItemData {
    pub fn read(buf: &[u8]) -> ItemData {
        let mut r = Reader(buf);
        if buf.len() >= 2 && buf[..2] == [0xff, 0xff] {
            r.take(2);
            let ok = (|| {
                let _version = r.u8()?;
                if r.u8()? != 10 {
                    return None;
                }
                r.string()?;
                match r.tag(10, 0)? {
                    Tag::Compound(v) => Some(v),
                    _ => None,
                }
            })();
            if let Some(nbt) = ok {
                return ItemData { nbt, rest: r.0.to_vec() };
            }
            return ItemData { nbt: vec![], rest: vec![0; 8] };
        }
        // no NBT is just an i16 0 before the lists
        ItemData { nbt: vec![], rest: buf.get(2..).map_or(vec![0; 8], <[u8]>::to_vec) }
    }

    pub fn write(&self) -> Vec<u8> {
        let rest = if self.rest.len() >= 8 { &self.rest[..] } else { &[0u8; 8][..] };
        if self.nbt.is_empty() {
            let mut b = vec![0, 0];
            b.extend(rest);
            return b;
        }
        let mut b = vec![0xff, 0xff, 1, 10, 0, 0];
        write(&mut b, &Tag::Compound(self.nbt.clone()));
        b.extend(rest);
        b
    }

    pub fn get(&self, k: &str) -> Option<&Tag> {
        self.nbt.iter().find(|(key, _)| key == k).map(|(_, v)| v)
    }

    pub fn remove(&mut self, k: &str) {
        self.nbt.retain(|(key, _)| key != k);
    }

    pub fn set(&mut self, k: &str, v: Tag) {
        match self.nbt.iter_mut().find(|(key, _)| key == k) {
            Some(slot) => slot.1 = v,
            None => self.nbt.push((k.into(), v)),
        }
    }

    /// (id, level) pairs from `ench`.
    pub fn enchants(&self) -> Vec<(i16, i16)> {
        let Some(Tag::List(_, list)) = self.get("ench") else { return vec![] };
        list.iter()
            .filter_map(|e| match e {
                Tag::Compound(c) => {
                    let f = |k: &str| c.iter().find(|(key, _)| key == k).and_then(|(_, v)| v.int());
                    Some((f("id")? as i16, f("lvl")? as i16))
                }
                _ => None,
            })
            .collect()
    }

    pub fn set_enchants(&mut self, e: &[(i16, i16)]) {
        let list = e.iter().map(|&(id, lvl)| Tag::Compound(vec![("id".into(), Tag::Short(id)), ("lvl".into(), Tag::Short(lvl))])).collect();
        self.set("ench", Tag::List(10, list));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_damage_and_enchants() {
        let mut d = ItemData::read(&[0; 10]);
        assert!(d.nbt.is_empty());
        d.set("Damage", Tag::Int(7));
        d.set_enchants(&[(9, 5), (17, 3)]);
        let bytes = d.write();
        let back = ItemData::read(&bytes);
        assert_eq!(back.get("Damage"), Some(&Tag::Int(7)));
        assert_eq!(back.enchants(), vec![(9, 5), (17, 3)]);
        assert_eq!(&bytes[bytes.len() - 8..], &[0; 8], "lists kept");
        assert_eq!(ItemData::read(&[0; 10]).write(), vec![0; 10]);
    }
}

//! Diffs two proxy logs (reference server vs Rusted) per chat-marker section.
//! Usage: compare <reference log> <rusted log> [report.md]

use std::collections::BTreeMap;
use std::fmt::Write;

#[derive(Default)]
struct Section {
    /// (direction, packet name) -> (count, first example)
    packets: BTreeMap<(String, String), (usize, String)>,
}

fn load(path: &str) -> BTreeMap<String, Section> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut sections: BTreeMap<String, Section> = BTreeMap::new();
    for line in text.lines() {
        let mut f = line.splitn(4, '\t');
        let (Some(sec), Some(dir), Some(name), example) = (f.next(), f.next(), f.next(), f.next()) else { continue };
        let entry = sections
            .entry(sec.to_string())
            .or_default()
            .packets
            .entry((dir.to_string(), name.to_string()))
            .or_insert((0, example.unwrap_or_default().to_string()));
        entry.0 += 1;
    }
    sections
}

fn report(reference: &BTreeMap<String, Section>, rusted: &BTreeMap<String, Section>) -> String {
    let mut out = String::from("# Rusted vs reference\n");
    let empty = Section::default();
    for (sec, r) in reference {
        let u = rusted.get(sec).unwrap_or(&empty);
        let _ = writeln!(out, "\n## Section {sec}");
        let mut any = false;
        for ((dir, name), (count, example)) in &r.packets {
            let key = (dir.clone(), name.clone());
            match u.packets.get(&key) {
                None => {
                    any = true;
                    let who = if dir == "S>C" { "server sent, Rusted never did" } else { "game sent here, but not on Rusted" };
                    let _ = writeln!(out, "- **MISSING** `{name}` ({dir}, {count}x) — {who}\n  - e.g. `{example}`");
                }
                Some((ucount, _)) if *count >= 3 && (*ucount * 4 < *count || *ucount > *count * 4) => {
                    any = true;
                    let _ = writeln!(out, "- **COUNT** `{name}` ({dir}): reference {count}x, Rusted {ucount}x");
                }
                _ => {}
            }
        }
        for ((dir, name), (count, example)) in &u.packets {
            if !r.packets.contains_key(&(dir.clone(), name.clone())) {
                any = true;
                let _ = writeln!(out, "- **EXTRA** `{name}` ({dir}, {count}x) — only on Rusted\n  - e.g. `{example}`");
            }
        }
        if !any {
            let _ = writeln!(out, "- no differences");
        }
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [reference, rusted, ..] = args.as_slice() else {
        eprintln!("usage: compare <reference log> <rusted log> [report.md]");
        std::process::exit(2);
    };
    let text = report(&load(reference), &load(rusted));
    match args.get(2) {
        Some(path) => {
            std::fs::write(path, &text).unwrap();
            println!("wrote {path}");
        }
        None => print!("{text}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> BTreeMap<String, Section> {
        let p = std::env::temp_dir().join(format!("cmp_{}.txt", s.len()));
        std::fs::write(&p, s).unwrap();
        load(p.to_str().unwrap())
    }

    #[test]
    fn finds_missing_extra_and_counts() {
        let reference = parse("#3\tS>C\tContainerOpenPacket\tx\n#3\tC>S\tInteractPacket\ty\n#3\tS>C\tTextPacket\tt\n#3\tS>C\tTextPacket\tt\n#3\tS>C\tTextPacket\tt\n#3\tS>C\tTextPacket\tt\n");
        let rusted = parse("#3\tC>S\tInteractPacket\ty\n#3\tS>C\tFooPacket\tz\n");
        let r = report(&reference, &rusted);
        assert!(r.contains("**MISSING** `ContainerOpenPacket`"));
        assert!(r.contains("**MISSING** `TextPacket`"));
        assert!(r.contains("**EXTRA** `FooPacket`"));
        assert!(!r.contains("`InteractPacket`"));
    }
}

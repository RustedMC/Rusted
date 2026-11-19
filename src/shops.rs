//! Chest shops. Put a chest down, put a sign on (or next to) it and write:
//!
//! ```text
//! [shop]
//! diamond 4
//! B 100        <- players Buy 4 diamonds from your chest for 100
//! ```
//!
//! `S 100` instead makes it a Sell shop: players sell you 4 diamonds for 100 into the chest.
//! Clicking the sign trades. Only the owner (and ops) can open the chest or break the shop.
//! Shops are kept in plugins/shops/shops.txt.
//! shortcut: a shop trade while someone has the chest open can be overwritten when they close
//! it; lock shop chests during trades if that turns out to matter.

use crate::inventory::Stack;
use crate::players::{PlayerInfo, Server};
use crate::plugin::{self, Config, Plugin};

const NAME: &str = "shops";

type Pos = (i32, i32, i32);

#[derive(Clone)]
struct Shop {
    owner: String,
    item: String,
    amount: u32,
    buy: bool,
    price: i64,
    chest: Pos,
}

fn key(p: Pos) -> String {
    format!("{},{},{}", p.0, p.1, p.2)
}

fn parse_pos(s: &str) -> Option<Pos> {
    let n: Vec<i32> = s.split(',').filter_map(|x| x.parse().ok()).collect();
    (n.len() == 3).then(|| (n[0], n[1], n[2]))
}

fn load(book: &Config, sign: Pos) -> Option<Shop> {
    let v = book.get(&key(sign))?;
    let f: Vec<&str> = v.split('|').collect();
    Some(Shop {
        owner: f.first()?.to_string(),
        item: f.get(1)?.to_string(),
        amount: f.get(2)?.parse().ok()?,
        buy: *f.get(3)? == "B",
        price: f.get(4)?.parse().ok()?,
        chest: parse_pos(f.get(5)?)?,
    })
}

fn store(s: &Shop) -> String {
    format!("{}|{}|{}|{}|{}|{}", s.owner, s.item, s.amount, if s.buy { "B" } else { "S" }, s.price, key(s.chest))
}

fn shop_chest(book: &Config, pos: Pos) -> Option<Shop> {
    book.entries().filter_map(|(k, _)| parse_pos(k)).filter_map(|s| load(book, s)).find(|s| s.chest == pos)
}

fn neighbours(p: Pos) -> [Pos; 6] {
    [(p.0 + 1, p.1, p.2), (p.0 - 1, p.1, p.2), (p.0, p.1, p.2 + 1), (p.0, p.1, p.2 - 1), (p.0, p.1 - 1, p.2), (p.0, p.1 + 1, p.2)]
}

fn is(st: &Option<Stack>, item: &str) -> bool {
    st.as_ref().is_some_and(|s| crate::items::name_of(s.item.id) == Some(item))
}

fn stock(slots: &[Option<Stack>], item: &str) -> u32 {
    slots.iter().filter(|s| is(s, item)).map(|s| s.as_ref().map_or(0, |s| s.count.max(0) as u32)).sum()
}

/// Takes `n` of `item` out of `slots` (caller checked there are enough).
fn remove(slots: &mut [Option<Stack>], item: &str, mut n: u32) -> Option<Stack> {
    let template = slots.iter().find(|s| is(s, item))?.clone();
    for s in slots.iter_mut().filter(|s| is(s, item)) {
        let st = s.as_mut()?;
        let take = n.min(st.count as u32);
        st.count -= take as i8;
        n -= take;
        if st.count <= 0 {
            *s = None;
        }
        if n == 0 {
            break;
        }
    }
    template
}

/// Adds `n` of the item to `slots`, topping up stacks first. False (and unchanged) if it won't fit.
fn insert(slots: &mut [Option<Stack>], template: &Stack, mut n: u32) -> bool {
    let id = template.item.id;
    let room: u32 = slots
        .iter()
        .map(|s| match s {
            None => 64,
            Some(st) if st.item.id == id => (64 - st.count.max(0)) as u32,
            _ => 0,
        })
        .sum();
    if room < n {
        return false;
    }
    for s in slots.iter_mut() {
        if n == 0 {
            break;
        }
        match s {
            Some(st) if st.item.id == id && st.count < 64 => {
                let add = n.min((64 - st.count) as u32);
                st.count += add as i8;
                n -= add;
            }
            None => {
                let add = n.min(64);
                let mut st = template.clone();
                st.count = add as i8;
                *s = Some(st);
                n -= add;
            }
            _ => {}
        }
    }
    true
}

fn money(n: i64) -> String {
    format!("§6{} coins", crate::economy::format(n))
}

fn trade(server: &Server, p: &PlayerInfo, sign: Pos, shop: Shop) {
    let name = crate::market::pretty(&shop.item);
    if shop.owner == p.name {
        let left = stock(&server.containers.get(shop.chest), &shop.item);
        server.tell(p.id, &format!("§7Your shop has §f{left} {name}§7 in stock."));
        return;
    }
    if shop.buy {
        let mut slots = server.containers.get(shop.chest);
        if stock(&slots, &shop.item) < shop.amount {
            server.tell(p.id, "§cThis shop is out of stock.");
            return;
        }
        if crate::economy::withdraw(&p.name, shop.price).is_none() {
            server.tell(p.id, &format!("§cYou need {}§c.", money(shop.price)));
            return;
        }
        let Some(template) = remove(&mut slots, &shop.item, shop.amount) else { return };
        server.containers.put(shop.chest, slots);
        let mut st = template;
        st.count = shop.amount.min(64) as i8;
        server.give(p.id, st);
        let _ = crate::economy::deposit(&shop.owner, shop.price);
        server.sound(p.id, "random.orb");
        server.tell(p.id, &format!("§aBought §f{}x {name} §afor {}§a.", shop.amount, money(shop.price)));
        if let Some(id) = server.find_by_name(&shop.owner) {
            server.tell(id, &format!("§a{} bought {}x {name} from your shop.", p.name, shop.amount));
        }
        return;
    }
    // Sell shop: the owner pays, the items go in the chest.
    if crate::economy::withdraw(&shop.owner, shop.price).is_none() {
        server.tell(p.id, "§cThe owner can't afford this right now.");
        return;
    }
    let seller = p.name.clone();
    server.take_item(p.id, &shop.item.clone(), shop.amount, move |server, got| {
        let me = server.find_by_name(&seller);
        let Some((template, n)) = got else {
            let _ = crate::economy::deposit(&shop.owner, shop.price);
            if let Some(id) = me {
                server.tell(id, &format!("§cYou need {}x {name}.", shop.amount));
            }
            return;
        };
        let mut slots = server.containers.get(shop.chest);
        if !insert(&mut slots, &template, n) {
            let _ = crate::economy::deposit(&shop.owner, shop.price);
            if let Some(id) = me {
                let mut st = template;
                st.count = n as i8;
                server.give(id, st);
                server.tell(id, "§cThe shop's chest is full.");
            }
            return;
        }
        server.containers.put(shop.chest, slots);
        let _ = crate::economy::deposit(&seller, shop.price);
        if let Some(id) = me {
            server.sound(id, "random.orb");
            server.tell(id, &format!("§aSold §f{n}x {name} §afor {}§a.", money(shop.price)));
        }
        let _ = sign;
    });
}

pub struct Shops;

impl Plugin for Shops {
    fn name(&self) -> &str {
        NAME
    }

    fn on_sign_change(&self, server: &Server, player: &PlayerInfo, pos: Pos, front: &mut String, _back: &mut String) -> bool {
        let lines: Vec<String> = front.lines().map(|l| l.trim().to_string()).collect();
        if !lines.first().is_some_and(|l| l.eq_ignore_ascii_case("[shop]")) {
            return true;
        }
        let fail = |front: &mut String, why: &str| {
            *front = format!("§c[Shop]\n§c{why}");
            true
        };
        let Some(chest) = neighbours(pos).into_iter().find(|&c| crate::containers::kind(server.world.get(c.0, c.1, c.2)).is_some()) else {
            return fail(front, "No chest here");
        };
        let mut it = lines.get(1).map(|l| l.split_whitespace().collect::<Vec<_>>()).unwrap_or_default().into_iter();
        let item = it.next().map(|i| if i.contains(':') { i.to_string() } else { format!("minecraft:{i}") });
        let amount: u32 = it.next().and_then(|a| a.parse().ok()).unwrap_or(1).clamp(1, 64);
        let Some(item) = item.filter(|i| plugin::item(i).is_some()) else { return fail(front, "Unknown item") };
        let mode = lines.get(2).map(|l| l.split_whitespace().collect::<Vec<_>>()).unwrap_or_default();
        let (buy, price) = match mode.as_slice() {
            [m, p] if m.eq_ignore_ascii_case("b") => (true, p.parse::<i64>().ok()),
            [m, p] if m.eq_ignore_ascii_case("s") => (false, p.parse::<i64>().ok()),
            _ => (true, None),
        };
        let Some(price) = price.filter(|p| *p > 0) else { return fail(front, "B <price> or S <price>") };
        let mut book = Config::data(NAME, "shops");
        if shop_chest(&book, chest).is_some_and(|s| s.owner != player.name) {
            return fail(front, "Not your chest");
        }
        let shop = Shop { owner: player.name.clone(), item: item.clone(), amount, buy, price, chest };
        book.set(&key(pos), &store(&shop));
        let _ = book.save();
        *front = format!(
            "§1§l[Shop]\n§0{amount}x {}\n{} §6{}\n§8{}",
            crate::market::pretty(&item),
            if buy { "§2Buy" } else { "§4Sell" },
            crate::economy::format(price),
            player.name
        );
        server.tell(player.id, "§aShop created. Fill the chest and players can trade by clicking the sign.");
        true
    }

    fn on_player_interact(&self, server: &Server, player: &PlayerInfo, pos: Pos, _item: &str) -> bool {
        let book = Config::data(NAME, "shops");
        match load(&book, pos) {
            Some(shop) => {
                trade(server, player, pos, shop);
                false
            }
            None => true,
        }
    }

    fn on_container_open(&self, _server: &Server, player: &PlayerInfo, pos: Pos, _block: &str) -> bool {
        let book = Config::data(NAME, "shops");
        match shop_chest(&book, pos) {
            Some(shop) if shop.owner != player.name && !crate::commands::is_op(&player.name) => false,
            _ => true,
        }
    }

    fn on_block_break(&self, server: &Server, player: &PlayerInfo, pos: Pos, _block: i32) -> bool {
        let mut book = Config::data(NAME, "shops");
        let sign = load(&book, pos);
        let chest = shop_chest(&book, pos);
        let Some(shop) = sign.clone().or(chest) else { return true };
        if shop.owner != player.name && !crate::commands::is_op(&player.name) {
            server.tell(player.id, "§cThat's someone's shop.");
            return false;
        }
        // Breaking the sign or the chest closes every shop on it.
        let gone: Vec<String> = book
            .entries()
            .filter_map(|(k, _)| Some((k.to_string(), load(&book, parse_pos(k)?)?)))
            .filter(|(k, s)| *k == key(pos) || s.chest == pos)
            .map(|(k, _)| k)
            .collect();
        for k in gone {
            book.remove(&k);
        }
        let _ = book.save();
        server.tell(player.id, "§7Shop removed.");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(n: i8) -> Stack {
        crate::blocks::init();
        let mut s = plugin::item("minecraft:diamond").unwrap().build();
        s.count = n;
        s
    }

    #[test]
    fn insert_and_remove_respect_stacks() {
        let mut slots = vec![Some(stack(60)), None];
        assert!(insert(&mut slots, &stack(1), 10), "4 top up the 60, 6 go in the empty slot");
        assert_eq!(stock(&slots, "minecraft:diamond"), 70);
        assert!(!insert(&mut slots, &stack(1), 100), "no room");
        assert_eq!(stock(&slots, "minecraft:diamond"), 70, "unchanged when it won't fit");
        remove(&mut slots, "minecraft:diamond", 65);
        assert_eq!(stock(&slots, "minecraft:diamond"), 5);
    }

    #[test]
    fn round_trips() {
        let s = Shop { owner: "a".into(), item: "minecraft:dirt".into(), amount: 4, buy: false, price: 9, chest: (1, -2, 3) };
        let mut book = Config::data("shops-test", "x");
        book.set("5,6,7", &store(&s));
        let back = load(&book, (5, 6, 7)).unwrap();
        assert_eq!((back.amount, back.buy, back.price, back.chest), (4, false, 9, (1, -2, 3)));
    }
}

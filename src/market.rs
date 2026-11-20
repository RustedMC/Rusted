//! Auction House and buy Orders, paid with the economy plugin.
//!
//! Auction House:
//! - `/ah` browse and buy, `/ah sell <price> [count]` lists what you're holding,
//!   `/ah mine` your listings (click to take one back, expired ones too)
//! - Listings expire after `auction.hours` (48); the seller takes them back from `/ah mine`.
//!
//! Orders (players asking to buy):
//! - `/orders` browse and deliver, `/orders create <item> <amount> <price each>` (the money is
//!   held up front), `/orders mine` (collect what was delivered, or cancel for a refund)
//!
//! Data in plugins/market/auctions.txt and orders.txt. Settings in config.txt.

use crate::inventory::{decode_slots, encode_slots, Stack};
use crate::players::{PlayerInfo, Server};
use crate::plugin::{self, Config, Plugin};
use crate::ui::{self, Form, Response};
use parking_lot::Mutex;

const NAME: &str = "market";

/// auctions.txt and orders.txt are read-modify-written from menus and commands.
static LOCK: Mutex<()> = Mutex::new(());

fn setting(key: &str, default: &str) -> String {
    let mut c = Config::load(NAME);
    let v = c.get_or(key, default);
    let _ = c.save();
    v
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

fn save_stack(st: &Stack) -> String {
    hex(&encode_slots(&[Some(st.clone())]))
}

fn load_stack(s: &str) -> Option<Stack> {
    decode_slots(&unhex(s)?, 1, 1)?.0.into_iter().next()?
}

/// "minecraft:diamond_sword" -> "Diamond Sword"
pub fn pretty(id: &str) -> String {
    id.trim_start_matches("minecraft:")
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn item_id(st: &Stack) -> String {
    crate::items::name_of(st.item.id).unwrap_or("minecraft:air").to_string()
}

/// Best-guess menu icon: blocks are under textures/blocks, items under textures/items.
/// shortcut: names that differ from their texture (grass_block...) show blank; add a lookup
/// table if that matters.
pub fn icon(id: &str) -> String {
    let short = id.trim_start_matches("minecraft:");
    if crate::blocks::any_state(&format!("minecraft:{short}")).is_some() {
        format!("textures/blocks/{short}")
    } else {
        format!("textures/items/{short}")
    }
}

fn money(n: i64) -> String {
    format!("§6{} {}", crate::economy::format(n), setting("currency", "coins"))
}

fn give(server: &Server, to: i64, st: Stack) {
    server.give(to, st);
}

// ---------------------------------------------------------------------------------------------
// Auction House

#[derive(Clone)]
struct Listing {
    id: u64,
    seller: String,
    price: i64,
    item: Stack,
    count: u32,
    time: i64,
}

fn listings(book: &Config) -> Vec<Listing> {
    let mut ids: Vec<u64> = book.entries().filter_map(|(k, _)| k.strip_suffix(".seller")?.parse().ok()).collect();
    ids.sort_unstable_by(|a, b| b.cmp(a));
    ids.into_iter()
        .filter_map(|id| {
            let g = |f: &str| book.get(&format!("{id}.{f}")).map(str::to_string);
            Some(Listing {
                id,
                seller: g("seller")?,
                price: g("price")?.parse().ok()?,
                item: load_stack(&g("item")?)?,
                count: g("count")?.parse().ok()?,
                time: g("time")?.parse().ok()?,
            })
        })
        .collect()
}

fn remove_listing(book: &mut Config, id: u64) {
    for f in ["seller", "price", "item", "count", "time"] {
        book.remove(&format!("{id}.{f}"));
    }
}

fn expired(l: &Listing) -> bool {
    let hours: i64 = setting("auction.hours", "48").parse().unwrap_or(48);
    now() - l.time > hours * 3600
}

fn sell(server: &Server, p: &PlayerInfo, price: i64, count: i8) -> String {
    let max: usize = setting("auction.max-listings", "5").parse().unwrap_or(5);
    let mine = listings(&Config::data(NAME, "auctions")).iter().filter(|l| l.seller == p.name).count();
    if mine >= max {
        return format!("§cYou can have {max} listings at once. Take one back with /ah mine.");
    }
    if price <= 0 {
        return "§cThe price must be more than 0.".into();
    }
    let seller = p.name.clone();
    server.take_held(p.id, count, move |server, got| {
        let Some((item, n)) = got else {
            if let Some(id) = server.find_by_name(&seller) {
                server.tell(id, "§cHold the item you want to sell.");
            }
            return;
        };
        let _guard = LOCK.lock();
        let mut book = Config::data(NAME, "auctions");
        let id: u64 = book.get("next-id").and_then(|v| v.parse().ok()).unwrap_or(1);
        book.set("next-id", &(id + 1).to_string());
        book.set(&format!("{id}.seller"), &seller);
        book.set(&format!("{id}.price"), &price.to_string());
        book.set(&format!("{id}.item"), &save_stack(&item));
        book.set(&format!("{id}.count"), &n.to_string());
        book.set(&format!("{id}.time"), &now().to_string());
        let _ = book.save();
        if let Some(pid) = server.find_by_name(&seller) {
            server.tell(pid, &format!("§aListed §f{}x {} §afor {}§a.", n, pretty(&item_id(&item)), money(price)));
            server.sound(pid, "random.orb");
        }
    });
    String::new()
}

fn browse(server: &Server, p: &PlayerInfo) {
    let all: Vec<Listing> = listings(&Config::data(NAME, "auctions")).into_iter().filter(|l| !expired(l)).take(40).collect();
    let form = Form::Simple {
        title: "Auction House".into(),
        content: if all.is_empty() { "§7Nothing for sale yet. Sell with §f/ah sell <price>".into() } else { String::new() },
        buttons: all
            .iter()
            .map(|l| (format!("§f{}x {}\n{}", l.count, pretty(&item_id(&l.item)), money(l.price)), Some(icon(&item_id(&l.item)))))
            .collect(),
    };
    ui::show(server, p.id, form, move |server, player, answer| {
        if let Response::Button(i) = answer
            && let Some(l) = all.get(i).cloned()
        {
            confirm_buy(server, player, l);
        }
    });
}

fn confirm_buy(server: &Server, p: &PlayerInfo, l: Listing) {
    if l.seller == p.name {
        server.tell(p.id, "§cThat's your own listing. Take it back from /ah mine.");
        return;
    }
    let form = Form::Modal {
        title: "Confirm purchase".into(),
        content: format!("§fBuy §e{}x {} §ffrom §e{} §ffor {}§f?", l.count, pretty(&item_id(&l.item)), l.seller, money(l.price)),
        yes: "§aBuy".into(),
        no: "Cancel".into(),
    };
    ui::show(server, p.id, form, move |server, player, answer| {
        if answer != Response::Choice(true) {
            return;
        }
        let msg = buy(server, player, &l);
        server.tell(player.id, &msg);
    });
}

fn buy(server: &Server, p: &PlayerInfo, l: &Listing) -> String {
    let _guard = LOCK.lock();
    let mut book = Config::data(NAME, "auctions");
    // Still there? Someone may have bought it while the menu was open.
    if book.get(&format!("{}.seller", l.id)).is_none() {
        return "§cSomeone bought that first.".into();
    }
    if crate::economy::withdraw(&p.name, l.price).is_none() {
        return format!("§cYou need {}§c.", money(l.price));
    }
    remove_listing(&mut book, l.id);
    let _ = book.save();
    let tax: i64 = setting("auction.tax-percent", "0").parse().unwrap_or(0);
    let earned = l.price - l.price * tax.clamp(0, 100) / 100;
    if earned > 0 {
        let _ = crate::economy::deposit(&l.seller, earned);
    }
    let mut st = l.item.clone();
    st.count = l.count as i8;
    give(server, p.id, st);
    if let Some(id) = server.find_by_name(&l.seller) {
        server.tell(id, &format!("§a{} bought your {}x {} §afor {}§a.", p.name, l.count, pretty(&item_id(&l.item)), money(earned)));
    }
    server.sound(p.id, "random.levelup");
    format!("§aBought §f{}x {} §afor {}§a.", l.count, pretty(&item_id(&l.item)), money(l.price))
}

fn mine(server: &Server, p: &PlayerInfo) {
    let all: Vec<Listing> = listings(&Config::data(NAME, "auctions")).into_iter().filter(|l| l.seller == p.name).collect();
    let form = Form::Simple {
        title: "Your listings".into(),
        content: if all.is_empty() { "§7You aren't selling anything.".into() } else { "§7Click a listing to take it back.".into() },
        buttons: all
            .iter()
            .map(|l| {
                let state = if expired(l) { "§cExpired".to_string() } else { money(l.price) };
                (format!("§f{}x {}\n{}", l.count, pretty(&item_id(&l.item)), state), Some(icon(&item_id(&l.item))))
            })
            .collect(),
    };
    ui::show(server, p.id, form, move |server, player, answer| {
        let Response::Button(i) = answer else { return };
        let Some(l) = all.get(i) else { return };
        let _guard = LOCK.lock();
        let mut book = Config::data(NAME, "auctions");
        if book.get(&format!("{}.seller", l.id)).is_none() {
            server.tell(player.id, "§cThat already sold.");
            return;
        }
        remove_listing(&mut book, l.id);
        let _ = book.save();
        let mut st = l.item.clone();
        st.count = l.count as i8;
        give(server, player.id, st);
        server.tell(player.id, "§aTook your listing back.");
    });
}

// ---------------------------------------------------------------------------------------------
// Orders

#[derive(Clone)]
struct Order {
    id: u64,
    buyer: String,
    item: String,
    amount: u32,
    filled: u32,
    /// delivered but not collected yet
    stored: u32,
    each: i64,
}

fn orders(book: &Config) -> Vec<Order> {
    let mut ids: Vec<u64> = book.entries().filter_map(|(k, _)| k.strip_suffix(".buyer")?.parse().ok()).collect();
    ids.sort_unstable_by(|a, b| b.cmp(a));
    ids.into_iter()
        .filter_map(|id| {
            let g = |f: &str| book.get(&format!("{id}.{f}")).map(str::to_string);
            Some(Order {
                id,
                buyer: g("buyer")?,
                item: g("item")?,
                amount: g("amount")?.parse().ok()?,
                filled: g("filled")?.parse().ok()?,
                stored: g("stored")?.parse().ok()?,
                each: g("each")?.parse().ok()?,
            })
        })
        .collect()
}

fn remove_order(book: &mut Config, id: u64) {
    for f in ["buyer", "item", "amount", "filled", "stored", "each"] {
        book.remove(&format!("{id}.{f}"));
    }
}

fn create_order(p: &PlayerInfo, item: &str, amount: u32, each: i64) -> String {
    let id = if item.contains(':') { item.to_string() } else { format!("minecraft:{item}") };
    if plugin::item(&id).is_none() {
        return format!("§cNo item called {item}.");
    }
    if amount == 0 || each <= 0 || amount > 100_000 {
        return "§cUsage: /orders create <item> <amount> <price each>".into();
    }
    let Some(total) = each.checked_mul(amount as i64) else { return "§cThat's too much.".into() };
    if crate::economy::withdraw(&p.name, total).is_none() {
        return format!("§cThat order costs {}§c up front.", money(total));
    }
    let _guard = LOCK.lock();
    let mut book = Config::data(NAME, "orders");
    let oid: u64 = book.get("next-id").and_then(|v| v.parse().ok()).unwrap_or(1);
    book.set("next-id", &(oid + 1).to_string());
    for (f, v) in [("buyer", p.name.clone()), ("item", id.clone()), ("amount", amount.to_string()), ("filled", "0".into()), ("stored", "0".into()), ("each", each.to_string())] {
        book.set(&format!("{oid}.{f}"), &v);
    }
    let _ = book.save();
    format!("§aOrder placed: §f{amount}x {} §aat {} §aeach. {} §ais held until it's filled.", pretty(&id), money(each), money(total))
}

fn browse_orders(server: &Server, p: &PlayerInfo) {
    let all: Vec<Order> = orders(&Config::data(NAME, "orders")).into_iter().filter(|o| o.filled < o.amount).take(40).collect();
    let form = Form::Simple {
        title: "Orders".into(),
        content: if all.is_empty() { "§7No orders. Place one with §f/orders create <item> <amount> <price>".into() } else { "§7Click an order to deliver items from your inventory.".into() },
        buttons: all
            .iter()
            .map(|o| (format!("§f{} §7{}/{}\n{} §7each §8· §7{}", pretty(&o.item), o.filled, o.amount, money(o.each), o.buyer), Some(icon(&o.item))))
            .collect(),
    };
    ui::show(server, p.id, form, move |server, player, answer| {
        if let Response::Button(i) = answer
            && let Some(o) = all.get(i).cloned()
        {
            deliver(server, player, o);
        }
    });
}

fn deliver(server: &Server, p: &PlayerInfo, o: Order) {
    if o.buyer == p.name {
        server.tell(p.id, "§cThat's your own order.");
        return;
    }
    // How many they have, from the once-a-second snapshot; the take below is exact.
    let have: u32 = crate::players::equipment(p.id)
        .inventory
        .iter()
        .flatten()
        .filter(|st| crate::items::name_of(st.item.id) == Some(o.item.as_str()))
        .map(|st| st.count.max(0) as u32)
        .sum();
    let n = have.min(o.amount - o.filled);
    if n == 0 {
        server.tell(p.id, &format!("§cYou don't have any {}.", pretty(&o.item)));
        return;
    }
    let seller = p.name.clone();
    let item = o.item.clone();
    server.take_item(p.id, &item, n, move |server, got| {
        let me = server.find_by_name(&seller);
        let Some((_, n)) = got else {
            if let Some(id) = me {
                server.tell(id, "§cYour inventory changed, try again.");
            }
            return;
        };
        let _guard = LOCK.lock();
        let mut book = Config::data(NAME, "orders");
        let live = orders(&book).into_iter().find(|x| x.id == o.id);
        // Order gone or filled meanwhile: give the items back.
        let Some(live) = live.filter(|x| x.filled + n <= x.amount) else {
            if let (Some(id), Some(item)) = (me, plugin::item(&o.item)) {
                give_many(server, id, item.build(), n);
                server.tell(id, "§cThat order changed, your items are back.");
            }
            return;
        };
        book.set(&format!("{}.filled", o.id), &(live.filled + n).to_string());
        book.set(&format!("{}.stored", o.id), &(live.stored + n).to_string());
        let _ = book.save();
        let pay = live.each * n as i64;
        let _ = crate::economy::deposit(&seller, pay);
        if let Some(id) = me {
            server.tell(id, &format!("§aDelivered §f{n}x {} §aand earned {}§a.", pretty(&o.item), money(pay)));
            server.sound(id, "random.orb");
        }
        if let Some(id) = server.find_by_name(&live.buyer) {
            server.tell(id, &format!("§a{seller} delivered {n}x {} to your order. §7/orders mine to collect.", pretty(&o.item)));
        }
    });
}

fn give_many(server: &Server, to: i64, st: Stack, n: u32) {
    for chunk in chunks(n) {
        let mut s = st.clone();
        s.count = chunk;
        server.give(to, s);
    }
}

/// Splits a count into stacks of 64.
fn chunks(n: u32) -> Vec<i8> {
    let mut out = vec![64i8; (n / 64) as usize];
    if n % 64 > 0 {
        out.push((n % 64) as i8);
    }
    out
}

fn my_orders(server: &Server, p: &PlayerInfo) {
    let all: Vec<Order> = orders(&Config::data(NAME, "orders")).into_iter().filter(|o| o.buyer == p.name).collect();
    let form = Form::Simple {
        title: "Your orders".into(),
        content: if all.is_empty() { "§7You have no orders.".into() } else { "§7Click to collect. Unfilled orders are cancelled and refunded.".into() },
        buttons: all
            .iter()
            .map(|o| (format!("§f{} §7{}/{}\n§a{} ready to collect", pretty(&o.item), o.filled, o.amount, o.stored), Some(icon(&o.item))))
            .collect(),
    };
    ui::show(server, p.id, form, move |server, player, answer| {
        let Response::Button(i) = answer else { return };
        let Some(o) = all.get(i) else { return };
        let _guard = LOCK.lock();
        let mut book = Config::data(NAME, "orders");
        let Some(live) = orders(&book).into_iter().find(|x| x.id == o.id) else { return };
        if let Some(item) = plugin::item(&live.item) {
            give_many(server, player.id, item.build(), live.stored);
        }
        let refund = live.each * (live.amount - live.filled) as i64;
        remove_order(&mut book, live.id);
        let _ = book.save();
        if refund > 0 {
            let _ = crate::economy::deposit(&player.name, refund);
        }
        let mut msg = format!("§aCollected {}x {}.", live.stored, pretty(&live.item));
        if refund > 0 {
            msg.push_str(&format!(" §aRefunded {}§a.", money(refund)));
        }
        server.tell(player.id, &msg);
    });
}

pub struct Market;

impl Plugin for Market {
    fn name(&self) -> &str {
        NAME
    }

    fn version(&self) -> &str {
        "0.9.4"
    }

    fn description(&self) -> &str {
        "Auction House and buy orders"
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![("ah", "Auction House: browse, sell, your listings"), ("orders", "Buy orders: browse, create, your orders")]
    }

    fn usage(&self, command: &str) -> &'static [&'static str] {
        match command {
            "ah" => &["sell <price> [count]", "mine"],
            "orders" => &["create <item> <amount> <each>", "mine"],
            _ => &[],
        }
    }

    fn on_command(&self, server: &Server, player: &PlayerInfo, name: &str, args: &[&str]) -> Option<String> {
        if setting("enabled", "true") != "true" {
            return None;
        }
        Some(match (name, args) {
            ("ah", []) => {
                browse(server, player);
                String::new()
            }
            ("ah", ["sell", price, rest @ ..]) => {
                let count = rest.first().and_then(|c| c.parse().ok()).unwrap_or(64i8);
                match price.replace(',', "").parse() {
                    Ok(price) => sell(server, player, price, count),
                    Err(_) => "§cUsage: /ah sell <price> [count]".into(),
                }
            }
            ("ah", ["mine"]) => {
                mine(server, player);
                String::new()
            }
            ("ah", _) => "§6/ah §7browse  §6/ah sell §f<price> [count]  §6/ah mine".into(),
            ("orders", []) => {
                browse_orders(server, player);
                String::new()
            }
            ("orders", ["create", item, amount, each]) => match (amount.parse(), each.replace(',', "").parse()) {
                (Ok(a), Ok(e)) => create_order(player, item, a, e),
                _ => "§cUsage: /orders create <item> <amount> <price each>".into(),
            },
            ("orders", ["mine"]) => {
                my_orders(server, player);
                String::new()
            }
            ("orders", _) => "§6/orders §7browse  §6/orders create §f<item> <amount> <price each>  §6/orders mine".into(),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn helpers() {
        assert_eq!(super::pretty("minecraft:diamond_sword"), "Diamond Sword");
        assert_eq!(super::unhex(&super::hex(&[0, 15, 255])), Some(vec![0, 15, 255]));
        assert_eq!(super::chunks(130), vec![64, 64, 2]);
        assert_eq!(super::chunks(0), Vec::<i8>::new());
    }
}

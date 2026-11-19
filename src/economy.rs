//! Economy: one shared balance per player, for other plugins (shops, jobs, rewards) to use.
//!
//! - `/balance [player]` (`/bal`), `/pay <player> <amount>`, `/baltop`
//! - `/eco <give|take|set> <player> <amount>` for ops
//!
//! Balances are whole coins in plugins/economy/balances.txt, keyed by lowercase name.
//! Settings in plugins/economy/config.txt: `enabled`, `starting-balance`, `currency`.
//! Other plugins call `economy::balance`, `economy::deposit` and `economy::withdraw`.

use crate::players::{PlayerInfo, Server};
use crate::plugin::{Config, Plugin};
use parking_lot::Mutex;

const NAME: &str = "economy";

/// Balances file is read-modify-written; one lock keeps two payments from racing.
static LOCK: Mutex<()> = Mutex::new(());

fn on() -> bool {
    Config::load(NAME).get_or("enabled", "true") == "true"
}

fn currency() -> String {
    let mut c = Config::load(NAME);
    let v = c.get_or("currency", "coins");
    let _ = c.save();
    v
}

fn starting() -> i64 {
    let mut c = Config::load(NAME);
    let v = c.get_or("starting-balance", "100").parse().unwrap_or(100);
    let _ = c.save();
    v
}

fn read(book: &Config, name: &str) -> Option<i64> {
    book.get(&name.to_lowercase()).and_then(|v| v.parse().ok())
}

/// `name`'s balance. New players start with `starting-balance`.
pub fn balance(name: &str) -> i64 {
    read(&Config::data(NAME, "balances"), name).unwrap_or_else(starting)
}

fn change(name: &str, f: impl FnOnce(i64) -> Option<i64>) -> Option<i64> {
    let _guard = LOCK.lock();
    let mut book = Config::data(NAME, "balances");
    let new = f(read(&book, name).unwrap_or_else(starting))?;
    book.set(&name.to_lowercase(), &new.to_string());
    book.save().ok()?;
    Some(new)
}

/// Adds `amount` (must be positive). Returns the new balance.
pub fn deposit(name: &str, amount: i64) -> Option<i64> {
    (amount > 0).then_some(())?;
    change(name, |b| b.checked_add(amount))
}

/// Takes `amount` if they have it. Returns the new balance, or None if they can't afford it.
pub fn withdraw(name: &str, amount: i64) -> Option<i64> {
    (amount > 0).then_some(())?;
    change(name, |b| (b >= amount).then(|| b - amount))
}

fn set(name: &str, amount: i64) -> Option<i64> {
    (amount >= 0).then_some(())?;
    change(name, |_| Some(amount))
}

/// Moves money between two players, all or nothing.
fn transfer(from: &str, to: &str, amount: i64) -> Result<i64, &'static str> {
    if amount <= 0 {
        return Err("Amount must be more than 0");
    }
    let _guard = LOCK.lock();
    let mut book = Config::data(NAME, "balances");
    let a = read(&book, from).unwrap_or_else(starting);
    let b = read(&book, to).unwrap_or_else(starting);
    if a < amount {
        return Err("You don't have enough");
    }
    let b = b.checked_add(amount).ok_or("That's too much")?;
    book.set(&from.to_lowercase(), &(a - amount).to_string());
    book.set(&to.to_lowercase(), &b.to_string());
    book.save().map_err(|_| "Couldn't save balances")?;
    Ok(a - amount)
}

/// 1234567 -> "1,234,567"
pub fn format(amount: i64) -> String {
    let digits = amount.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if amount < 0 { format!("-{out}") } else { out }
}

/// An online player's exact name from a case-insensitive one.
fn online(server: &Server, name: &str) -> Option<String> {
    server.all().into_iter().find(|p| p.name.eq_ignore_ascii_case(name)).map(|p| p.name)
}

fn amount(arg: &str) -> Option<i64> {
    arg.replace(',', "").parse().ok()
}

pub struct Economy;

impl Plugin for Economy {
    fn name(&self) -> &str {
        NAME
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("balance", "Shows your balance"),
            ("bal", "Shows your balance"),
            ("pay", "Sends money to a player"),
            ("baltop", "The richest players"),
            ("eco", "Gives, takes or sets a player's money (ops)"),
        ]
    }

    fn usage(&self, command: &str) -> &'static [&'static str] {
        match command {
            "balance" => &["[player]"],
            "bal" => &["[player]"],
            "pay" => &["<player> <amount>"],
            "eco" => &["give|take|set <player> <amount>"],
            _ => &[],
        }
    }

    fn on_command(&self, server: &Server, player: &PlayerInfo, name: &str, args: &[&str]) -> Option<String> {
        if !on() {
            return None;
        }
        let cur = currency();
        Some(match (name, args) {
            ("balance" | "bal", []) => format!("§7Balance: §6{} {cur}", format(balance(&player.name))),
            ("balance" | "bal", [who]) => format!("§7{who}: §6{} {cur}", format(balance(who))),
            ("pay", [who, amt]) => {
                let Some(to) = online(server, who) else { return Some(format!("§c{who} isn't online")) };
                let Some(n) = amount(amt) else { return Some("§cUsage: /pay <player> <amount>".into()) };
                if to == player.name {
                    return Some("§cYou can't pay yourself".into());
                }
                match transfer(&player.name, &to, n) {
                    Ok(left) => {
                        if let Some(id) = server.find_by_name(&to) {
                            server.tell(id, &format!("§a+{} {cur} §7from {}", format(n), player.name));
                        }
                        format!("§aSent {} {cur} to {to}. §7Balance: §6{}", format(n), format(left))
                    }
                    Err(e) => format!("§c{e}"),
                }
            }
            ("pay", _) => "§cUsage: /pay <player> <amount>".into(),
            ("baltop", _) => {
                let book = Config::data(NAME, "balances");
                let mut all: Vec<(String, i64)> = book.entries().filter_map(|(k, v)| Some((k.to_string(), v.parse().ok()?))).collect();
                all.sort_by(|a, b| b.1.cmp(&a.1));
                let mut out = String::from("§6§lTop balances");
                for (i, (who, n)) in all.iter().take(10).enumerate() {
                    out.push_str(&format!("\n§7{}. §f{who} §8- §6{}", i + 1, format(*n)));
                }
                out
            }
            ("eco", [op, who, amt]) => {
                if !crate::commands::is_op(&player.name) {
                    return Some("§cYou do not have permission to use this command".into());
                }
                let Some(n) = amount(amt) else { return Some("§cUsage: /eco <give|take|set> <player> <amount>".into()) };
                let done = match *op {
                    "give" => deposit(who, n),
                    "take" => withdraw(who, n),
                    "set" => set(who, n),
                    _ => return Some("§cUsage: /eco <give|take|set> <player> <amount>".into()),
                };
                match done {
                    Some(b) => format!("§a{who} now has §6{} {cur}", format(b)),
                    None => "§cCouldn't do that (not enough money, or a bad amount)".into(),
                }
            }
            ("eco", _) => "§cUsage: /eco <give|take|set> <player> <amount>".into(),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats_thousands() {
        assert_eq!(super::format(0), "0");
        assert_eq!(super::format(999), "999");
        assert_eq!(super::format(1000), "1,000");
        assert_eq!(super::format(-1234567), "-1,234,567");
    }
}

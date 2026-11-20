//! Essentials: homes, warps, teleport requests and /spawn.
//!
//! - `/sethome [name]`, `/home [name]`, `/delhome <name>`, `/homes`
//! - `/warp <name>`, `/warps`, `/setwarp <name>` and `/delwarp <name>` (ops)
//! - `/tpa <player>`, `/tpahere <player>`, `/tpaccept`, `/tpdeny`
//! - `/spawn`
//!
//! Settings in plugins/essentials/config.txt: `enabled`, `max-homes`, `tpa-timeout-seconds`,
//! `teleport-cost` (coins from the economy plugin, 0 for free).
//! shortcut: homes and warps store a position only, not the dimension; add one when
//! cross-dimension teleports exist.

use crate::players::{PlayerInfo, Server};
use crate::plugin::{Config, Plugin};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::Instant;

const NAME: &str = "essentials";

fn setting(key: &str, default: &str) -> String {
    let mut c = Config::load(NAME);
    let v = c.get_or(key, default);
    let _ = c.save();
    v
}

fn on() -> bool {
    setting("enabled", "true") == "true"
}

fn feet(p: &PlayerInfo) -> (f32, f32, f32) {
    (p.pos.0, p.pos.1 - 1.62, p.pos.2)
}

fn encode(at: (f32, f32, f32)) -> String {
    format!("{:.2},{:.2},{:.2}", at.0, at.1, at.2)
}

fn decode(v: &str) -> Option<(f32, f32, f32)> {
    let n: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (n.len() == 3).then(|| (n[0], n[1], n[2]))
}

/// Home and warp names: short, letters/digits/_/- only, so they're safe as config keys.
fn clean(name: &str) -> Option<String> {
    let n = name.to_lowercase();
    (!n.is_empty() && n.len() <= 16 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')).then_some(n)
}

/// Charges `teleport-cost` if set. Err is the message to show.
fn pay_for_teleport(p: &PlayerInfo) -> Result<(), String> {
    let cost: i64 = setting("teleport-cost", "0").parse().unwrap_or(0);
    if cost <= 0 {
        return Ok(());
    }
    crate::economy::withdraw(&p.name, cost).map(|_| ()).ok_or_else(|| format!("§cTeleporting costs {} coins", crate::economy::format(cost)))
}

fn go(server: &Server, p: &PlayerInfo, at: (f32, f32, f32), what: &str) -> String {
    if let Err(e) = pay_for_teleport(p) {
        return e;
    }
    server.teleport(p.id, at);
    server.sound(p.id, "mob.endermen.portal");
    format!("§aTeleported to {what}.")
}

/// Pending /tpa requests: target id -> (requester id, "to" = requester goes to target,
/// "here" = target goes to requester, when it was sent).
static REQUESTS: Mutex<Option<HashMap<i64, (i64, &'static str, Instant)>>> = Mutex::new(None);

fn online(server: &Server, name: &str) -> Option<PlayerInfo> {
    server.all().into_iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

fn request(server: &Server, from: &PlayerInfo, who: &str, kind: &'static str) -> String {
    let Some(target) = online(server, who) else { return format!("§c{who} isn't online") };
    if target.id == from.id {
        return "§cThat's you".into();
    }
    REQUESTS.lock().get_or_insert_with(HashMap::new).insert(target.id, (from.id, kind, Instant::now()));
    let ask = if kind == "to" { "wants to teleport to you" } else { "wants you to teleport to them" };
    server.tell(target.id, &format!("§e{} §7{ask}. §a/tpaccept §7or §c/tpdeny", from.name));
    format!("§7Request sent to §e{}§7.", target.name)
}

fn answer(server: &Server, me: &PlayerInfo, accept: bool) -> String {
    let timeout: u64 = setting("tpa-timeout-seconds", "60").parse().unwrap_or(60);
    let Some((from, kind, at)) = REQUESTS.lock().get_or_insert_with(HashMap::new).remove(&me.id) else {
        return "§cNo pending request".into();
    };
    let Some(other) = server.get(from) else { return "§cThey left".into() };
    if at.elapsed().as_secs() > timeout {
        return "§cThat request expired".into();
    }
    if !accept {
        server.tell(other.id, &format!("§c{} denied your request.", me.name));
        return "§7Denied.".into();
    }
    let (mover, dest) = if kind == "to" { (&other, me) } else { (me, &other) };
    let msg = go(server, mover, feet(dest), &dest.name);
    if mover.id != me.id {
        server.tell(mover.id, &msg);
        return format!("§aAccepted. §7{} is on the way.", other.name);
    }
    msg
}

/// `/home` with no name: a card for each home (click to go or delete) and one to add a home.
fn homes_menu(server: &Server, p: &PlayerInfo) {
    let homes = Config::player(NAME, p);
    let list: Vec<(String, (f32, f32, f32))> = homes.entries().filter_map(|(k, v)| Some((k.to_string(), decode(v)?))).collect();
    let mut buttons: Vec<(String, Option<String>)> = list
        .iter()
        .map(|(n, at)| (format!("§f{n}\n§7{:.0} {:.0} {:.0}", at.0, at.1, at.2), Some("textures/items/bed_red".into())))
        .collect();
    buttons.push(("§aSet a new home\n§7Where you stand".into(), Some("textures/ui/color_plus".into())));
    let form = crate::ui::Form::Simple { title: "Homes".into(), content: String::new(), buttons };
    crate::ui::show(server, p.id, form, move |server, p, answer| {
        let crate::ui::Response::Button(i) = answer else { return };
        match list.get(i) {
            Some((name, _)) => home_options(server, p, name.clone()),
            None => new_home_form(server, p),
        }
    });
}

fn home_options(server: &Server, p: &PlayerInfo, name: String) {
    let form = crate::ui::Form::Simple {
        title: format!("Home {name}"),
        content: String::new(),
        buttons: vec![
            ("§fTeleport\n§7Go there now".into(), Some("textures/items/ender_pearl".into())),
            ("§cDelete\n§7Remove this home".into(), Some("textures/ui/trash".into())),
            ("§fBack".into(), Some("textures/ui/arrow_dark_left_stretch".into())),
        ],
    };
    crate::ui::show(server, p.id, form, move |server, p, answer| {
        let msg = match answer {
            crate::ui::Response::Button(0) => homes_cmd(server, p, "home", &[&name]),
            crate::ui::Response::Button(1) => homes_cmd(server, p, "delhome", &[&name]),
            crate::ui::Response::Button(2) => return homes_menu(server, p),
            _ => return,
        };
        server.tell(p.id, &msg);
    });
}

fn new_home_form(server: &Server, p: &PlayerInfo) {
    let form = crate::ui::Form::Custom {
        title: "New home".into(),
        elements: vec![crate::ui::Element::Input { text: "Name it".into(), placeholder: "base, farm, mine...".into(), default: "home".into() }],
    };
    crate::ui::show(server, p.id, form, |server, p, answer| {
        let crate::ui::Response::Values(v) = answer else { return };
        let name = v.first().and_then(|x| x.as_str()).unwrap_or("home").trim().to_string();
        let msg = homes_cmd(server, p, "sethome", &[&name]);
        server.tell(p.id, &msg);
    });
}

fn homes_cmd(server: &Server, p: &PlayerInfo, name: &str, args: &[&str]) -> String {
    // /home set|delete|list ... is the same as /sethome, /delhome, /homes.
    match (name, args) {
        ("home", []) => {
            homes_menu(server, p);
            return String::new();
        }
        ("home", ["set", rest @ ..]) => return homes_cmd(server, p, "sethome", rest),
        ("home", ["delete" | "del" | "remove", rest @ ..]) => return homes_cmd(server, p, "delhome", rest),
        ("home", ["list"]) => return homes_cmd(server, p, "homes", &[]),
        _ => {}
    }
    let mut homes = Config::player(NAME, p);
    let pick = args.first().copied().unwrap_or("home");
    match name {
        "sethome" => {
            let Some(key) = clean(pick) else { return "§cHome names are letters, numbers, _ and - (16 max)".into() };
            // Ranks can raise the limit with essentials.homes.<n>.
            let base: usize = setting("max-homes", "3").parse().unwrap_or(3);
            let max = (base..=100).rev().find(|n| crate::ranks::allowed(&p.name, &format!("essentials.homes.{n}")) == Some(true)).unwrap_or(base);
            if homes.get(&key).is_none() && homes.entries().count() >= max {
                return format!("§cYou can only have {max} homes. /delhome one first");
            }
            homes.set(&key, &encode(feet(p)));
            let _ = homes.save();
            format!("§aHome §f{key} §aset.")
        }
        "home" => match clean(pick).and_then(|k| homes.get(&k).and_then(decode)) {
            Some(at) => go(server, p, at, &format!("home {pick}")),
            None => format!("§cNo home called {pick}. §7Your homes: {}", list(&homes)),
        },
        "delhome" => {
            let Some(key) = args.first().and_then(|a| clean(a)) else { return "§cUsage: /delhome <name>".into() };
            if homes.get(&key).is_none() {
                return format!("§cNo home called {key}");
            }
            homes.remove(&key);
            let _ = homes.save();
            format!("§aHome §f{key} §adeleted.")
        }
        _ => format!("§7Your homes: {}", list(&homes)),
    }
}

fn list(c: &Config) -> String {
    let names: Vec<&str> = c.entries().map(|(k, _)| k).collect();
    if names.is_empty() { "§8none".into() } else { format!("§f{}", names.join("§7, §f")) }
}

fn warps_cmd(server: &Server, p: &PlayerInfo, name: &str, args: &[&str]) -> String {
    let mut warps = Config::data(NAME, "warps");
    let op = crate::commands::is_op(&p.name);
    match (name, args.first().and_then(|a| clean(a))) {
        ("warp", Some(key)) => match warps.get(&key).and_then(decode) {
            Some(at) => go(server, p, at, &format!("warp {key}")),
            None => format!("§cNo warp called {key}. §7Warps: {}", list(&warps)),
        },
        ("setwarp", Some(key)) if op => {
            warps.set(&key, &encode(feet(p)));
            let _ = warps.save();
            format!("§aWarp §f{key} §aset.")
        }
        ("delwarp", Some(key)) if op => {
            warps.remove(&key);
            let _ = warps.save();
            format!("§aWarp §f{key} §adeleted.")
        }
        ("setwarp" | "delwarp", _) if !op => "§cYou do not have permission to use this command".into(),
        ("warps" | "warp", _) => format!("§7Warps: {}", list(&warps)),
        _ => format!("§cUsage: /{name} <name>"),
    }
}

/// Read-only view of `target`'s inventory (as of the last second). Two pages of 27:
/// hotbar + top rows, then the last row + armor. Any click flips the page.
fn invsee(server: &Server, viewer: i64, target: PlayerInfo, page: usize) {
    let eq = crate::players::equipment(target.id);
    let mut inv = eq.inventory.clone();
    inv.resize(crate::inventory::MAIN, None);
    let items: Vec<_> = if page == 0 {
        inv[..27].to_vec()
    } else {
        let mut v = inv[27..].to_vec();
        v.extend(eq.armor.iter().cloned());
        v
    };
    let title = format!("{} ({}/2)", target.name, page + 1);
    crate::ui::menu(server, viewer, &title, items, move |server, me, _slot| {
        invsee(server, me.id, target.clone(), 1 - page);
        true
    });
}

pub struct Essentials;

impl Plugin for Essentials {
    fn name(&self) -> &str {
        NAME
    }

    fn version(&self) -> &str {
        "2.1.0"
    }

    fn description(&self) -> &str {
        "Homes with a menu, warps, teleport requests, /spawn and /invsee"
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("sethome", "Sets a home where you stand"),
            ("home", "Teleports to one of your homes"),
            ("delhome", "Deletes a home"),
            ("homes", "Lists your homes"),
            ("warp", "Teleports to a warp"),
            ("warps", "Lists the warps"),
            ("setwarp", "Sets a warp where you stand (ops)"),
            ("delwarp", "Deletes a warp (ops)"),
            ("tpa", "Asks to teleport to a player"),
            ("tpahere", "Asks a player to teleport to you"),
            ("tpaccept", "Accepts a teleport request"),
            ("tpdeny", "Denies a teleport request"),
            ("spawn", "Teleports to the world spawn"),
            ("invsee", "Looks at a player's inventory (ops)"),
        ]
    }

    fn on_leave(&self, _server: &Server, player: &PlayerInfo) {
        if let Some(r) = REQUESTS.lock().as_mut() {
            r.retain(|to, (from, _, _)| *to != player.id && *from != player.id);
        }
    }

    fn usage(&self, command: &str) -> &'static [&'static str] {
        match command {
            "home" => &["set <name>", "delete <name>", "list", "[name]"],
            "sethome" => &["[name]"],
            "delhome" => &["<name>"],
            "warp" => &["<name>"],
            "setwarp" => &["<name>"],
            "delwarp" => &["<name>"],
            "tpa" => &["<player>"],
            "tpahere" => &["<player>"],
            "invsee" => &["<player>"],
            _ => &[],
        }
    }

    fn on_command(&self, server: &Server, player: &PlayerInfo, name: &str, args: &[&str]) -> Option<String> {
        if !on() {
            return None;
        }
        Some(match name {
            "sethome" | "home" | "delhome" | "homes" => homes_cmd(server, player, name, args),
            "warp" | "warps" | "setwarp" | "delwarp" => warps_cmd(server, player, name, args),
            "tpa" | "tpahere" => match args.first() {
                Some(who) => request(server, player, who, if name == "tpa" { "to" } else { "here" }),
                None => format!("§cUsage: /{name} <player>"),
            },
            "tpaccept" => answer(server, player, true),
            "tpdeny" => answer(server, player, false),
            "spawn" => go(server, player, crate::commands::world_spawn(server), "spawn"),
            "invsee" if !crate::commands::is_op(&player.name) => "§cYou do not have permission to use this command".into(),
            "invsee" => match args.first().and_then(|w| online(server, w)) {
                Some(target) => {
                    invsee(server, player.id, target, 0);
                    String::new()
                }
                None => "§cUsage: /invsee <online player>".into(),
            },
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_and_positions() {
        assert_eq!(super::clean("Base_1"), Some("base_1".into()));
        assert_eq!(super::clean("../x"), None);
        assert_eq!(super::clean(""), None);
        assert_eq!(super::decode(&super::encode((1.5, 64.0, -3.25))), Some((1.5, 64.0, -3.25)));
        assert_eq!(super::decode("1,2"), None);
    }
}

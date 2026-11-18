//! Commands that don't need a player connection (so the console can run them too),
//! plus ops, whitelist, bans, weather and difficulty.

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

use crate::players::{Event, Kick, Server};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Name list stored one per line in a text file, case-insensitive.
pub struct NameList {
    path: &'static str,
    names: Mutex<BTreeSet<String>>,
}

impl NameList {
    fn load(path: &'static str) -> Self {
        let names = std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().to_lowercase())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        Self { path, names: Mutex::new(names) }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.names.lock().unwrap().contains(&name.to_lowercase())
    }

    /// false if nothing changed
    pub fn set(&self, name: &str, present: bool) -> bool {
        let mut names = self.names.lock().unwrap();
        let changed = if present { names.insert(name.to_lowercase()) } else { names.remove(&name.to_lowercase()) };
        if changed {
            let text: String = names.iter().map(|n| format!("{n}\n")).collect();
            if let Err(e) = std::fs::write(self.path, text) {
                println!("failed to save {}: {e}", self.path);
            }
        }
        changed
    }

    pub fn all(&self) -> Vec<String> {
        self.names.lock().unwrap().iter().cloned().collect()
    }
}

pub struct Lists {
    pub ops: NameList,
    pub whitelist: NameList,
    pub banned: NameList,
    pub whitelist_on: Mutex<bool>,
}

pub fn lists() -> &'static Lists {
    static L: OnceLock<Lists> = OnceLock::new();
    L.get_or_init(|| Lists {
        ops: NameList::load("ops.txt"),
        whitelist: NameList::load("whitelist.txt"),
        banned: NameList::load("banned-players.txt"),
        whitelist_on: Mutex::new(crate::config::get().white_list),
    })
}

/// Entity ids of online ops, so their abilities show operator perms.
pub fn op_ids() -> &'static Mutex<std::collections::HashSet<i64>> {
    static S: OnceLock<Mutex<std::collections::HashSet<i64>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

pub fn is_op(name: &str) -> bool {
    name == CONSOLE || lists().ops.contains(name)
}

pub const CONSOLE: &str = "CONSOLE";

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Weather {
    Clear,
    Rain,
    Thunder,
}

pub static WEATHER: Mutex<Weather> = Mutex::new(Weather::Clear);

pub fn weather_packets(w: Weather) -> Vec<crate::Protocol> {
    use bedrock::protocol::v662::packets::LevelEventPacket;
    let ev = |event_id: i32, data: i32| {
        crate::Protocol::LevelEventPacket(Box::new(LevelEventPacket { event_id, position: (0.0, 0.0, 0.0), data }))
    };
    // 3001/3003 start/stop rain, 3002/3004 thunder, data is intensity
    match w {
        Weather::Clear => vec![ev(3003, 0), ev(3004, 0)],
        Weather::Rain => vec![ev(3001, 65535), ev(3004, 0)],
        Weather::Thunder => vec![ev(3001, 65535), ev(3002, 65535)],
    }
}

pub fn set_weather(server: &crate::players::Server, new: Weather) {
    if !server.plugins.weather_change(server, &format!("{new:?}").to_lowercase()) {
        return;
    }
    *WEATHER.lock().unwrap() = new;
    for p in weather_packets(new) {
        let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(p) });
    }
}

/// Rain and thunder each count down a random number of ticks and then flip (PNX Level.updateWeather).
/// Thunder only shows while it's raining.
#[derive(Default)]
pub struct WeatherCycle {
    rain_time: i32,
    thunder_time: i32,
    raining: bool,
    thundering: bool,
    last: Option<Weather>,
}

impl WeatherCycle {
    /// Returns the new weather if it changed. `rand(lo, hi)` is inclusive.
    pub fn step(&mut self, ticks: i32, current: Weather, mut rand: impl FnMut(i32, i32) -> i32) -> Option<Weather> {
        if self.last != Some(current) {
            // first run, or /weather changed it, so restart the timers
            self.raining = current != Weather::Clear;
            self.thundering = current == Weather::Thunder;
            self.rain_time = if self.raining { rand(12000, 24000) } else { rand(12000, 180000) };
            self.thunder_time = if self.thundering { rand(3600, 15600) } else { rand(12000, 180000) };
            self.last = Some(current);
        }
        self.rain_time -= ticks;
        if self.rain_time <= 0 {
            self.raining = !self.raining;
            self.rain_time = if self.raining { rand(12000, 24000) } else { rand(12000, 180000) };
        }
        self.thunder_time -= ticks;
        if self.thunder_time <= 0 {
            self.thundering = !self.thundering;
            self.thunder_time = if self.thundering { rand(3600, 15600) } else { rand(12000, 180000) };
        }
        let new = match (self.raining, self.thundering) {
            (false, _) => Weather::Clear,
            (true, false) => Weather::Rain,
            (true, true) => Weather::Thunder,
        };
        (new != current).then(|| {
            self.last = Some(new);
            new
        })
    }
}

/// Resolves a target selector (PNX EntitySelector): @a @p @r @s @e or a plain name.
/// Supports name=, r=, c= and type=player.
pub fn select(server: &crate::players::Server, me: i64, arg: &str) -> Result<Vec<i64>, String> {
    let Some(rest) = arg.strip_prefix('@') else {
        return server.find_by_name(arg).map(|i| vec![i]).ok_or_else(|| format!("§cNo player was found: {arg}"));
    };
    let (kind, filters) = match rest.split_once('[') {
        Some((k, f)) => (k, f.trim_end_matches(']')),
        None => (rest, ""),
    };
    // scores={...} has commas inside the braces, so pull it out first
    let mut score_tests: Vec<(String, i32, i32)> = Vec::new();
    let filters_owned;
    let filters = if let Some(i) = filters.find("scores={") {
        let end = filters[i..].find('}').map_or(filters.len(), |e| i + e + 1);
        for t in filters[i + 8..end.saturating_sub(1)].split(',').filter(|t| !t.is_empty()) {
            let Some((obj, range)) = t.split_once('=') else { continue };
            let (lo, hi) = match range.split_once("..") {
                Some((a, b)) => (a.parse().unwrap_or(i32::MIN), b.parse().unwrap_or(i32::MAX)),
                None => {
                    let v = range.parse().map_err(|_| format!("§cBad score range: {range}"))?;
                    (v, v)
                }
            };
            score_tests.push((obj.trim().to_string(), lo, hi));
        }
        filters_owned = format!("{}{}", &filters[..i], &filters[end..]);
        filters_owned.as_str()
    } else {
        filters
    };
    let here = server.get(me).map(|p| p.pos);
    let mut all = server.all();
    let mut count: Option<usize> = None;
    for f in filters.split(',').filter(|f| !f.is_empty()) {
        let (k, v) = f.split_once('=').ok_or_else(|| format!("§cBad selector filter: {f}"))?;
        let (neg, v) = match v.strip_prefix('!') {
            Some(v) => (true, v),
            None => (false, v),
        };
        match k.trim() {
            "name" => all.retain(|p| (p.name == v) != neg),
            "type" => all.retain(|_| (v.trim_start_matches("minecraft:") == "player") != neg),
            "r" => {
                let r: f32 = v.parse().map_err(|_| format!("§cBad radius: {v}"))?;
                if let Some(h) = here {
                    all.retain(|p| ((p.pos.0 - h.0).powi(2) + (p.pos.1 - h.1).powi(2) + (p.pos.2 - h.2).powi(2)).sqrt() <= r);
                }
            }
            "c" => count = v.parse().ok(),
            _ => {} // other filters (scores, tags, x/y/z) aren't supported yet and match everything
        }
    }
    all.retain(|p| score_tests.iter().all(|(o, lo, hi)| crate::ui::score(o, &p.name).is_some_and(|v| (*lo..=*hi).contains(&v))));
    let dist = |p: &crate::players::PlayerInfo| here.map_or(0.0, |h| (p.pos.0 - h.0).powi(2) + (p.pos.1 - h.1).powi(2) + (p.pos.2 - h.2).powi(2));
    let mut ids: Vec<i64> = match kind {
        "a" | "e" => all.iter().map(|p| p.id).collect(),
        "s" => all.iter().filter(|p| p.id == me).map(|p| p.id).collect(),
        "p" => {
            all.sort_by(|a, b| dist(a).total_cmp(&dist(b)));
            all.first().map(|p| vec![p.id]).unwrap_or_default()
        }
        "r" => {
            let n = all.len().max(1);
            let pick = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() as usize) % n;
            all.get(pick).map(|p| vec![p.id]).unwrap_or_default()
        }
        _ => return Err(format!("§cUnknown selector: @{kind}")),
    };
    if let Some(c) = count {
        ids.truncate(c);
    }
    Ok(ids)
}

/// Which argument is the target player, per command.
fn target_index(cmd: &str, args: &[&str]) -> Option<usize> {
    match cmd {
        "gamemode" | "gm" => Some(2),
        "xp" | "playsound" => Some(2),
        "kill" | "clear" | "effect" | "enchant" | "title" | "titleraw" | "tellraw" | "stopsound" | "damage" | "spawnpoint" | "camerashake" | "transfer" | "give" => Some(1),
        // /tp only has a target if there's a destination after it
        "tp" | "teleport" if args.len() == 3 || args.len() == 5 => Some(1),
        _ => None,
    }
}

/// Expands a target selector into one run per player, rewriting the target as `@s`.
/// Commands without a target just run here.
pub fn expand(server: &crate::players::Server, me: i64, line: &str) -> Result<Vec<(i64, String)>, String> {
    let args: Vec<&str> = line.split_whitespace().collect();
    let Some(&cmd) = args.first() else { return Ok(vec![]) };
    // /kill @e hits mobs too
    if cmd == "kill" && args.get(1).is_some_and(|a| a.starts_with("@e")) {
        let ty = args[1].split("type=").nth(1).map(|t| t.split([',', ']']).next().unwrap_or("").trim_start_matches("minecraft:").to_string());
        let killed = server.mobs.kill_all(server, ty.as_deref().filter(|t| *t != "player"));
        if ty.as_deref().is_some_and(|t| t != "player") {
            return Err(format!("Killed {killed} entities"));
        }
    }
    // /testfor just counts
    if cmd == "testfor" {
        let Some(t) = args.get(1) else { return Err("§cUse /testfor <target>".into()) };
        let ids = select(server, me, t)?;
        let names: Vec<String> = ids.iter().filter_map(|i| server.get(*i)).map(|p| p.name).collect();
        return Err(if names.is_empty() { "§cNo targets matched selector".into() } else { format!("Found {}", names.join(", ")) });
    }
    let Some(i) = target_index(cmd, &args) else { return Ok(vec![(me, line.to_string())]) };
    let Some(target) = args.get(i) else { return Ok(vec![(me, line.to_string())]) };
    // old shortcut: /give <item> with no player
    if cmd == "give" && !target.starts_with('@') && server.find_by_name(target).is_none() {
        return Ok(vec![(me, line.to_string())]);
    }
    if (cmd == "gamemode" || cmd == "gm" || cmd == "xp")
        && *target == "@s" {
            return Ok(vec![(me, line.to_string())]);
        }
    let ids = select(server, me, target)?;
    if ids.is_empty() {
        return Err("§cNo targets matched selector".into());
    }
    Ok(ids
        .into_iter()
        .map(|id| {
            let mut a: Vec<&str> = args.clone();
            a[i] = "@s";
            (id, a.join(" "))
        })
        .collect())
}

/// 0 peaceful, 1 easy, 2 normal, 3 hard.
pub static DIFFICULTY: Mutex<u32> = Mutex::new(2);

pub fn difficulty_from(s: &str) -> Option<u32> {
    match s {
        "peaceful" | "p" | "0" => Some(0),
        "easy" | "e" | "1" => Some(1),
        "normal" | "n" | "2" => Some(2),
        "hard" | "h" | "3" => Some(3),
        _ => None,
    }
}

pub fn difficulty_packet(d: u32) -> crate::Protocol {
    crate::Protocol::SetDifficultyPacket(Box::new(bedrock::protocol::v662::packets::SetDifficultyPacket { difficulty: d }))
}

/// Op-only commands (console can run them too).
pub const OP_ONLY: &[&str] = &[
    "kick", "op", "deop", "ban", "unban", "pardon", "whitelist", "say", "weather", "difficulty", "gamemode", "gm",
    "tp", "teleport", "give", "summon", "effect", "enchant", "xp", "title", "titleraw", "tellraw", "playsound", "stopsound", "camerashake",
    "transfer", "damage", "setblock", "fill", "clone", "testfor", "testforblock", "toggledownfall", "daylock", "alwaysday", "scoreboard", "time", "gamerule", "kill", "clear", "spawnpoint", "setworldspawn", "stop", "status",
];

/// (name, description) for everything we advertise and show in /help.
pub const HELP: &[(&str, &str)] = &[
    ("ban", "Bans a player"),
    ("clear", "Clears items from your inventory"),
    ("deop", "Revokes operator status"),
    ("difficulty", "Sets the difficulty level"),
    ("effect", "Add or remove status effects"),
    ("enchant", "Adds an enchantment to a player's selected item"),
    ("xp", "Adds or removes player experience"),
    ("title", "Controls screen titles"),
    ("titleraw", "Controls screen titles with JSON messages"),
    ("tellraw", "Sends a JSON message to players"),
    ("playsound", "Plays a sound"),
    ("stopsound", "Stops a sound"),
    ("camerashake", "Applies shaking to the players' camera"),
    ("transfer", "Transfers a player to another server"),
    ("damage", "Apply damage to the specified entities"),
    ("setblock", "Changes a block to another block"),
    ("fill", "Fills all or parts of a region with a specific block"),
    ("clone", "Clones blocks from one region to another"),
    ("testfor", "Counts entities matching specified conditions"),
    ("testforblock", "Tests whether a certain block is in a specific location"),
    ("toggledownfall", "Toggles the weather"),
    ("daylock", "Locks and unlocks the day-night cycle"),
    ("scoreboard", "Tracks and displays scores for various objectives"),
    ("gamemode", "Sets a player's game mode"),
    ("gamerule", "Sets or queries a game rule value"),
    ("give", "Gives an item to a player"),
    ("help", "Provides help for commands"),
    ("kick", "Kicks a player off the server"),
    ("kill", "Kills entities (players, mobs, etc.)"),
    ("list", "Lists players on the server"),
    ("me", "Displays a message about yourself"),
    ("op", "Grants operator status to a player"),
    ("plugins", "Gets a list of plugins running on the server"),
    ("say", "Sends a message in the chat to other players"),
    ("seed", "Displays the world seed"),
    ("setworldspawn", "Sets the world spawn"),
    ("spawnpoint", "Sets the spawn point for a player"),
    ("status", "Reads back the server's performance"),
    ("stop", "Stops the server"),
    ("summon", "Summons an entity"),
    ("tell", "Sends a private message to one or more players"),
    ("time", "Changes or queries the world's game time"),
    ("tp", "Teleports entities"),
    ("unban", "Unbans a player"),
    ("version", "Gets the version of this server, including any plugins in use"),
    ("weather", "Sets the weather"),
    ("whitelist", "Manages the server allowlist"),
];

/// Spawn points from /spawnpoint and /setworldspawn, saved in spawnpoints.txt
/// as `name x y z` lines. The world spawn is stored under `*`.
fn spawns() -> &'static Mutex<std::collections::HashMap<String, (f32, f32, f32)>> {
    static S: OnceLock<Mutex<std::collections::HashMap<String, (f32, f32, f32)>>> = OnceLock::new();
    S.get_or_init(|| {
        let text = std::fs::read_to_string(spawn_file()).unwrap_or_default();
        Mutex::new(
            text.lines()
                .filter_map(|l| {
                    let mut it = l.split_whitespace();
                    let name = it.next()?.to_string();
                    let mut f = || it.next()?.parse::<f32>().ok().filter(|v| v.is_finite());
                    Some((name, (f()?, f()?, f()?)))
                })
                .collect(),
        )
    })
}

fn spawn_file() -> std::path::PathBuf {
    std::path::Path::new("worlds").join(&crate::config::get().level_name).join("spawnpoints.txt")
}

/// Set a player's spawn (None = world spawn) from an eye position.
pub fn set_spawn(player: Option<&str>, pos: (f32, f32, f32)) {
    let mut s = spawns().lock().unwrap();
    s.insert(player.map_or("*".into(), str::to_lowercase), pos);
    let text: String = s.iter().map(|(n, p)| format!("{n} {} {} {}
", p.0, p.1, p.2)).collect();
    if let Err(e) = std::fs::write(spawn_file(), text) {
        println!("failed to save spawn points: {e}");
    }
}

pub fn world_spawn(server: &Server) -> (f32, f32, f32) {
    spawns().lock().unwrap().get("*").copied().unwrap_or_else(|| server.world.spawn())
}

/// Their /spawnpoint, or the world spawn.
pub fn spawn_of(name: &str, server: &Server) -> (f32, f32, f32) {
    let own = spawns().lock().unwrap().get(&name.to_lowercase()).copied();
    // respawning always lands in the overworld, so only those count
    own.filter(|p| crate::dims::of_f(p.1) == crate::dims::OVERWORLD).unwrap_or_else(|| world_spawn(server))
}

pub fn started() -> std::time::Instant {
    static T: OnceLock<std::time::Instant> = OnceLock::new();
    *T.get_or_init(std::time::Instant::now)
}

/// Runs a console-safe command. None if it isn't one.
pub fn run(server: &Server, sender: &str, args: &[&str]) -> Option<String> {
    let l = lists();
    let reply = match args {
        ["list"] => {
            let names: Vec<String> = server.all().into_iter().map(|p| p.name).collect();
            format!("There are {}/{} players online:\n{}", names.len(), crate::config::get().max_players, names.join(", "))
        }
        ["say", words @ ..] if !words.is_empty() => {
            server.broadcast(&format!("[{sender}] {}", words.join(" ")));
            String::new()
        }
        ["me", words @ ..] if !words.is_empty() => {
            server.broadcast(&format!("* {sender} {}", words.join(" ")));
            String::new()
        }
        ["tell" | "msg" | "w", who, words @ ..] if !words.is_empty() => match server.find_by_name(who) {
            Some(target) => {
                let text = words.join(" ");
                server.tell(target, &format!("§7§o{sender} whispers to you: {text}"));
                format!("§7§oYou whisper to {who}: {text}")
            }
            None => format!("§cNo player was found: {who}"),
        },
        ["kick", who, why @ ..] => match server.find_by_name(who) {
            Some(target) => {
                let message = if why.is_empty() { "Kicked by an operator".to_string() } else { why.join(" ") };
                let _ = server.events.send(Event::Kick { id: target, message, reason: Kick::Kicked });
                format!("Kicked {who} from the game")
            }
            None => format!("§cNo player was found: {who}"),
        },
        ["op", who] => {
            l.ops.set(who, true);
            if let Some(t) = server.find_by_name(who) {
                op_ids().lock().unwrap().insert(t);
                server.tell(t, "§eYou are now op!");
            }
            format!("Opped: {who}")
        }
        ["deop", who] => {
            l.ops.set(who, false);
            if let Some(t) = server.find_by_name(who) {
                op_ids().lock().unwrap().remove(&t);
                server.tell(t, "§eYou are no longer op!");
            }
            format!("De-opped: {who}")
        }
        ["ban", who, why @ ..] => {
            l.banned.set(who, true);
            if let Some(t) = server.find_by_name(who) {
                let message = if why.is_empty() { "You are banned from this server".to_string() } else { why.join(" ") };
                let _ = server.events.send(Event::Kick { id: t, message, reason: Kick::Kicked });
            }
            format!("Banned player {who}")
        }
        ["unban" | "pardon", who] => {
            if l.banned.set(who, false) { format!("Unbanned player {who}") } else { format!("§c{who} is not banned") }
        }
        ["banlist"] => format!("Banned players: {}", l.banned.all().join(", ")),
        ["whitelist", "on"] => {
            *l.whitelist_on.lock().unwrap() = true;
            "Turned on the allowlist".into()
        }
        ["whitelist", "off"] => {
            *l.whitelist_on.lock().unwrap() = false;
            "Turned off the allowlist".into()
        }
        ["whitelist", "add", who] => {
            l.whitelist.set(who, true);
            format!("Added {who} to the allowlist")
        }
        ["whitelist", "remove", who] => {
            l.whitelist.set(who, false);
            format!("Removed {who} from the allowlist")
        }
        ["whitelist", "list"] => format!("Allowlisted players: {}", l.whitelist.all().join(", ")),
        ["weather", w, ..] => {
            let new = match *w {
                "clear" => Weather::Clear,
                "rain" => Weather::Rain,
                "thunder" => Weather::Thunder,
                _ => return Some("§cUse /weather clear|rain|thunder".into()),
            };
            set_weather(server, new);
            format!("Changing to {} weather", format!("{new:?}").to_lowercase())
        }
        ["difficulty"] => format!("Difficulty: {}", ["peaceful", "easy", "normal", "hard"][*DIFFICULTY.lock().unwrap() as usize]),
        ["difficulty", d] => match difficulty_from(d) {
            Some(d) => {
                *DIFFICULTY.lock().unwrap() = d;
                let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(difficulty_packet(d)) });
                format!("Set game difficulty to {}", ["peaceful", "easy", "normal", "hard"][d as usize])
            }
            None => "§cUse /difficulty peaceful|easy|normal|hard".into(),
        },
        ["seed"] => format!("Seed: [{}]", server.world.seed() as i64),
        ["plugins" | "pl"] => {
            let names = server.plugins.names();
            format!("Plugins ({}): {}", names.len(), names.iter().map(|n| format!("§a{n}§f")).collect::<Vec<_>>().join(", "))
        }
        ["version" | "ver" | "about"] => format!(
            "This server is running Rusted {VERSION} for Minecraft: Bedrock Edition v1.26.60 (by Gardery, ported in part from PowerNukkitX)"
        ),
        ["status"] => {
            let up = started().elapsed().as_secs();
            format!(
                "§6---- §fServer status §6----\n§6Uptime: §a{}h {}m {}s\n§6Players: §a{}/{}\n§6Chunks cached: §a{}",
                up / 3600,
                up / 60 % 60,
                up % 60,
                server.all().len(),
                crate::config::get().max_players,
                server.world.edited_chunks(),
            )
        }
        ["help" | "?", ..] => {
            let mut out = String::from("§2--- Showing help ---");
            for (n, d) in HELP {
                out.push_str(&format!("\n/{n}: {d}"));
            }
            for (n, d) in server.plugins.commands() {
                out.push_str(&format!("\n/{n}: {d}"));
            }
            out
        }
        ["stop"] => {
            server.broadcast("Stopping the server");
            crate::request_stop();
            "Stopping the server".into()
        }
        _ => return None,
    };
    Some(reply)
}

#[cfg(test)]
mod tests {

    #[test]
    fn weather_cycle_flips_after_its_timer() {
        let mut c = WeatherCycle::default();
        let mut w = Weather::Clear;
        let mut seen = vec![];
        for _ in 0..12000 / 5 * 3 {
            if let Some(n) = c.step(5, w, |lo, _| lo) {
                w = n;
                seen.push(n);
            }
        }
        assert!(seen.contains(&Weather::Rain) && seen.contains(&Weather::Clear), "rain came and went: {seen:?}");
        let mut c = WeatherCycle::default();
        assert_eq!(c.step(5, Weather::Rain, |lo, _| lo), None, "/weather keeps its weather for a while");
    }

    use super::*;

    #[test]
    fn name_list_and_difficulty() {
        let path: &'static str = Box::leak(std::env::temp_dir().join(format!("rusted_ops_{}.txt", std::process::id())).to_string_lossy().into_owned().into_boxed_str());
        let l = NameList::load(path);
        assert!(l.set("Steve", true) && !l.set("steve", true));
        assert!(NameList::load(path).contains("STEVE"), "saved and case-insensitive");
        assert!(l.set("steve", false) && !NameList::load(path).contains("steve"));
        std::fs::remove_file(path).ok();
        assert_eq!(difficulty_from("hard"), Some(3));
        assert_eq!(difficulty_from("x"), None);
    }
}
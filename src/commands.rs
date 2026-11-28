//! Commands that don't need a player connection (so the console can run them too),
//! plus ops, whitelist, bans, weather and difficulty.

use std::collections::BTreeSet;
use std::sync::OnceLock;
use parking_lot::Mutex;

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
        self.names.lock().contains(&name.to_lowercase())
    }

    pub fn is_empty(&self) -> bool {
        self.names.lock().is_empty()
    }

    /// false if nothing changed
    pub fn set(&self, name: &str, present: bool) -> bool {
        let mut names = self.names.lock();
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
        self.names.lock().iter().cloned().collect()
    }
}

pub struct Lists {
    pub ops: NameList,
    pub whitelist: NameList,
    pub banned: NameList,
    /// banned-ips.txt: addresses refused at login (/ban-ip).
    pub banned_ips: NameList,
    pub whitelist_on: Mutex<bool>,
}

/// The address each player logged in from (this run), for /ban-ip <player>.
static IPS: Mutex<Option<std::collections::HashMap<String, String>>> = Mutex::new(None);

pub fn record_ip(name: &str, ip: &str) {
    IPS.lock().get_or_insert_with(Default::default).insert(name.to_lowercase(), ip.to_string());
}

fn ip_of(name: &str) -> Option<String> {
    IPS.lock().as_ref().and_then(|m| m.get(&name.to_lowercase()).cloned())
}

pub fn lists() -> &'static Lists {
    static L: OnceLock<Lists> = OnceLock::new();
    L.get_or_init(|| Lists {
        ops: NameList::load("ops.txt"),
        whitelist: NameList::load("whitelist.txt"),
        banned: NameList::load("banned-players.txt"),
        banned_ips: NameList::load("banned-ips.txt"),
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
    *WEATHER.lock() = new;
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
        // @initiator: whoever clicked the NPC running the command, which is the sender here.
        "s" | "initiator" => all.iter().filter(|p| p.id == me).map(|p| p.id).collect(),
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
        "clearspawnpoint" | "hud" | "fog" | "playanimation" if args.len() > 1 => Some(1),
        "inputpermission" if matches!(args.get(1), Some(&"set" | &"query")) => Some(2),
        "replaceitem" if args.get(1) == Some(&"entity") => Some(2),
        "loot" if args.get(1) == Some(&"give") => Some(2),
        // /tp only has a target if there's a destination after it
        "tp" | "teleport" if args.len() == 3 || args.len() == 5 => Some(1),
        _ => None,
    }
}

/// Which argument is a player name the sender's own command acts on.
fn name_index(cmd: &str, args: &[&str]) -> Option<usize> {
    match cmd {
        "op" | "deop" | "kick" | "ban" | "ban-ip" | "pardon" | "unban" | "msg" | "tell" | "w" | "tag" | "invsee" | "heal" | "feed" | "fly" | "mute" | "unmute" => Some(1),
        "whitelist" if matches!(args.get(1), Some(&"add" | &"remove")) => Some(2),
        "scoreboard" if args.get(1) == Some(&"players") => Some(3),
        _ => None,
    }
}

/// Expands a target selector into one run per player, rewriting the target as `@s`.
/// Commands without a target just run here.
pub fn expand(server: &crate::players::Server, me: i64, line: &str) -> Result<Vec<(i64, String)>, String> {
    // /execute and /function turn into ordinary runs, at most 16 deep (a function calling itself).
    thread_local!(static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) });
    let first = line.split_whitespace().next().unwrap_or("");
    if matches!(first, "execute" | "function") {
        if DEPTH.with(|d| d.get()) >= 16 {
            return Err("§cToo many nested /execute or /function calls".into());
        }
        DEPTH.with(|d| d.set(d.get() + 1));
        let out = if first == "execute" { execute(server, me, line) } else { function(server, me, line) };
        DEPTH.with(|d| d.set(d.get() - 1));
        return out;
    }
    // /tp <who> @s, /tp @p: the destination selector means the sender's view, so it's turned
    // into a name here, before the command is split per player being moved.
    let resolved;
    let line = match line.split_whitespace().collect::<Vec<_>>().as_slice() {
        [cmd @ ("tp" | "teleport"), rest @ ..] if matches!(rest.len(), 1 | 2) && rest.last().is_some_and(|d| d.starts_with('@')) => {
            let dest = select(server, me, rest.last().unwrap())?.into_iter().next().and_then(|id| server.get(id)).ok_or("§cNo targets matched selector")?;
            resolved = format!("{cmd} {}{}", if rest.len() == 2 { format!("{} ", rest[0]) } else { String::new() }, dest.name);
            resolved.as_str()
        }
        _ => line,
    };
    let args: Vec<&str> = line.split_whitespace().collect();
    let Some(&cmd) = args.first() else { return Ok(vec![]) };
    // /tp @e <player | x y z> moves mobs too
    if matches!(cmd, "tp" | "teleport") && matches!(args.len(), 3 | 5) && args[1].starts_with("@e") {
        let ty = args[1].split("type=").nth(1).map(|t| t.split([',', ']']).next().unwrap_or("").trim_start_matches("minecraft:").to_string());
        let here = server.get(me).map_or((0.0, 0.0, 0.0), |p| (p.pos.0, p.pos.1 - 1.62, p.pos.2));
        let dest = if args.len() == 3 {
            server.find_by_name(args[2]).and_then(|id| server.get(id)).map(|p| (p.pos.0, p.pos.1 - 1.62, p.pos.2))
        } else {
            let c = |s: &str, base: f32| match s.strip_prefix('~') {
                Some(r) => r.parse::<f32>().ok().or(r.is_empty().then_some(0.0)).map(|d| base + d),
                None => s.parse::<f32>().ok(),
            };
            match (c(args[2], here.0), c(args[3], here.1), c(args[4], here.2)) {
                (Some(x), Some(y), Some(z)) => Some((x, y, z)),
                _ => None,
            }
        };
        if let Some(to) = dest {
            let moved = server.mobs.teleport_all(ty.as_deref().filter(|t| *t != "player"), to);
            if ty.as_deref().is_some_and(|t| t != "player") {
                return Err(format!("Teleported {moved} entities"));
            }
        }
    }
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
    // Commands that take a player's name but run here (op, kick, msg...): one run per name.
    if let Some(i) = name_index(cmd, &args)
        && let Some(sel) = args.get(i).filter(|a| a.starts_with('@'))
    {
        let names: Vec<String> = select(server, me, sel)?.into_iter().filter_map(|id| server.get(id)).map(|p| p.name).collect();
        if names.is_empty() {
            return Err("§cNo targets matched selector".into());
        }
        return Ok(names
            .iter()
            .map(|n| {
                let mut a = args.clone();
                a[i] = n;
                (me, a.join(" "))
            })
            .collect());
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
    "clearspawnpoint", "fog", "hud", "inputpermission", "loot", "music", "particle", "playanimation", "recipe", "replaceitem", "schedule",
    "setmaxplayers", "spreadplayers", "testforblocks", "tickingarea",
    "kick", "op", "deop", "ban", "unban", "pardon", "ban-ip", "banip", "unban-ip", "pardon-ip", "banlist", "whitelist", "say", "weather", "difficulty", "gamemode", "gm",
    "tp", "teleport", "give", "summon", "effect", "enchant", "xp", "title", "titleraw", "tellraw", "playsound", "stopsound", "camerashake",
    "transfer", "damage", "setblock", "fill", "clone", "testfor", "locate", "execute", "function", "mv", "multiverse", "testforblock", "toggledownfall", "daylock", "alwaysday", "scoreboard", "time", "gamerule", "kill", "clear", "spawnpoint", "setworldspawn", "stop", "status", "perf",
];

/// (name, description) for everything we advertise and show in /help.
pub const HELP: &[(&str, &str)] = &[
    ("clearspawnpoint", "Removes a player's spawn point"),
    ("fog", "Adds or removes fog settings for a player"),
    ("hud", "Hides or shows parts of a player's screen"),
    ("inputpermission", "Turns a player's camera or movement controls on or off"),
    ("loot", "Gives or drops the items from a loot table"),
    ("music", "Plays or stops music for everyone"),
    ("particle", "Shows a particle effect"),
    ("playanimation", "Plays an animation on a player"),
    ("recipe", "Unlocks or locks recipes"),
    ("replaceitem", "Puts an item in a player's inventory slot"),
    ("schedule", "Runs a function after a delay"),
    ("setmaxplayers", "Sets how many players can join"),
    ("spreadplayers", "Teleports players to random spots in an area"),
    ("testforblocks", "Checks whether two areas have the same blocks"),
    ("tickingarea", "Keeps chunks ticking with no players near"),
    ("ban", "Bans a player"),
    ("ban-ip", "Bans an IP address, or the address a player joined from"),
    ("locate", "Finds the nearest structure"),
    ("mv", "Makes and visits extra worlds: create, tp, list"),
    ("execute", "Runs a command as or at other players, or if a block or player is there"),
    ("function", "Runs the commands in functions/<name>.mcfunction"),
    ("unban-ip", "Lifts an IP ban"),
    ("banlist", "Lists bans (banlist ips for addresses)"),
    ("clear", "Clears items from your inventory"),
    ("perf", "Shows TPS, MSPT and RAM in a boss bar"),
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
    let mut s = spawns().lock();
    s.insert(player.map_or("*".into(), str::to_lowercase), pos);
    let text: String = s.iter().map(|(n, p)| format!("{n} {} {} {}
", p.0, p.1, p.2)).collect();
    if let Err(e) = std::fs::write(spawn_file(), text) {
        println!("failed to save spawn points: {e}");
    }
}

/// Whether `pos` is inside spawn protection for `player` (vanilla: a square of
/// spawn-protection blocks around the world spawn; operators can always build).
pub fn spawn_protected(server: &Server, player: &str, pos: (i32, i32, i32)) -> bool {
    let r = crate::config::get().spawn_protection;
    // Vanilla: spawn protection only applies once someone is op.
    if r <= 0 || is_op(player) || lists().ops.is_empty() || crate::dims::of(pos.1) != crate::dims::OVERWORLD {
        return false;
    }
    let s = world_spawn(server);
    (pos.0 - s.0.floor() as i32).abs() <= r && (pos.2 - s.2.floor() as i32).abs() <= r
}

pub fn world_spawn(server: &Server) -> (f32, f32, f32) {
    spawns().lock().get("*").copied().unwrap_or_else(|| server.world.spawn())
}

/// Their /spawnpoint, or the world spawn.
pub fn spawn_of(name: &str, server: &Server) -> (f32, f32, f32) {
    let own = spawns().lock().get(&name.to_lowercase()).copied();
    // respawning always lands in the overworld, so only those count
    own.filter(|p| crate::dims::of_f(p.1) == crate::dims::OVERWORLD).unwrap_or_else(|| world_spawn(server))
}

/// /setmaxplayers, until the next restart (0 = the max-players setting).
pub static MAX_PLAYERS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn max_players() -> u32 {
    match MAX_PLAYERS.load(std::sync::atomic::Ordering::Relaxed) {
        0 => crate::config::get().max_players,
        n => n,
    }
}

/// /clearspawnpoint: back to the world spawn.
pub fn clear_spawn(player: &str) {
    let mut s = spawns().lock();
    s.remove(&player.to_lowercase());
    let text: String = s.iter().map(|(n, p)| format!("{n} {} {} {}\n", p.0, p.1, p.2)).collect();
    if let Err(e) = std::fs::write(spawn_file(), text) {
        println!("failed to save spawn points: {e}");
    }
}

/// A /tickingarea: chunks that keep random-ticking (crops grow, leaves decay) with nobody near.
#[derive(Clone, Debug, PartialEq)]
pub struct TickingArea {
    pub name: String,
    /// Chunk corners, inclusive.
    pub min: (i32, i32),
    pub max: (i32, i32),
}

fn ticking_file() -> std::path::PathBuf {
    std::path::Path::new("worlds").join(&crate::config::get().level_name).join("tickingareas.txt")
}

fn ticking_areas() -> &'static Mutex<Vec<TickingArea>> {
    static A: OnceLock<Mutex<Vec<TickingArea>>> = OnceLock::new();
    A.get_or_init(|| {
        let text = std::fs::read_to_string(ticking_file()).unwrap_or_default();
        Mutex::new(
            text.lines()
                .filter_map(|l| {
                    let p: Vec<&str> = l.split_whitespace().collect();
                    let n = |i: usize| p.get(i)?.parse::<i32>().ok();
                    Some(TickingArea { name: p.first()?.to_string(), min: (n(1)?, n(2)?), max: (n(3)?, n(4)?) })
                })
                .collect(),
        )
    })
}

fn save_ticking(areas: &[TickingArea]) {
    let text: String = areas.iter().map(|a| format!("{} {} {} {} {}\n", a.name, a.min.0, a.min.1, a.max.0, a.max.1)).collect();
    if let Err(e) = std::fs::write(ticking_file(), text) {
        println!("failed to save ticking areas: {e}");
    }
}

/// Every chunk inside a ticking area.
pub fn ticking_chunks() -> Vec<(i32, i32)> {
    let areas = ticking_areas().lock();
    let mut out = Vec::new();
    for a in areas.iter() {
        for x in a.min.0..=a.max.0 {
            for z in a.min.1..=a.max.1 {
                out.push((x, z));
            }
        }
    }
    out
}

/// Vanilla limits: 10 areas, 100 chunks each.
const MAX_TICKING_AREAS: usize = 10;
const MAX_TICKING_CHUNKS: i64 = 100;

fn tickingarea(server: &Server, sender: &str, args: &[&str]) -> String {
    let here = server.find_by_name(sender).and_then(|id| server.get(id)).map_or((0.0, 0.0, 0.0), |p| (p.pos.0, p.pos.1 - 1.62, p.pos.2));
    let chunk = |p: (f32, f32, f32)| ((p.0.floor() as i32) >> 4, (p.2.floor() as i32) >> 4);
    let mut areas = ticking_areas().lock();
    let add = |areas: &mut Vec<TickingArea>, min: (i32, i32), max: (i32, i32), name: Option<&str>| -> String {
        if areas.len() >= MAX_TICKING_AREAS {
            return format!("§cThere are already {MAX_TICKING_AREAS} ticking areas");
        }
        let (min, max) = ((min.0.min(max.0), min.1.min(max.1)), (min.0.max(max.0), min.1.max(max.1)));
        let n = (max.0 - min.0 + 1) as i64 * (max.1 - min.1 + 1) as i64;
        if n > MAX_TICKING_CHUNKS {
            return format!("§cA ticking area can cover at most {MAX_TICKING_CHUNKS} chunks ({n} asked)");
        }
        let name = name.map(str::to_string).unwrap_or_else(|| format!("Area{}", areas.len() + 1));
        if areas.iter().any(|a| a.name.eq_ignore_ascii_case(&name)) {
            return format!("§cA ticking area named {name} already exists");
        }
        areas.push(TickingArea { name: name.clone(), min, max });
        save_ticking(areas);
        format!("Added ticking area {name} covering {n} chunks")
    };
    match args {
        ["add", "circle", x, y, z, r, rest @ ..] => match (crate::cmdargs::coords(&[x, y, z], here), r.parse::<i32>()) {
            (Some(c), Ok(r)) if (1..=4).contains(&r) => {
                let (cx, cz) = chunk(c);
                add(&mut areas, (cx - r, cz - r), (cx + r, cz + r), rest.first().copied())
            }
            _ => "§cUse /tickingarea add circle <x y z> <radius 1-4> [name]".into(),
        },
        ["add", x1, y1, z1, x2, y2, z2, rest @ ..] => match (crate::cmdargs::coords(&[x1, y1, z1], here), crate::cmdargs::coords(&[x2, y2, z2], here)) {
            (Some(a), Some(b)) => add(&mut areas, chunk(a), chunk(b), rest.first().copied()),
            _ => "§cUse /tickingarea add <from> <to> [name]".into(),
        },
        ["remove_all"] => {
            let n = areas.len();
            areas.clear();
            save_ticking(&areas);
            format!("Removed {n} ticking areas")
        }
        ["remove", x, y, z] if crate::cmdargs::coords(&[x, y, z], here).is_some() => {
            let (cx, cz) = chunk(crate::cmdargs::coords(&[x, y, z], here).unwrap());
            let before = areas.len();
            areas.retain(|a| !(a.min.0..=a.max.0).contains(&cx) || !(a.min.1..=a.max.1).contains(&cz));
            save_ticking(&areas);
            format!("Removed {} ticking areas", before - areas.len())
        }
        ["remove", name] => {
            let before = areas.len();
            areas.retain(|a| !a.name.eq_ignore_ascii_case(name));
            save_ticking(&areas);
            if areas.len() < before { format!("Removed ticking area {name}") } else { format!("§cNo ticking area named {name}") }
        }
        ["list", ..] => {
            if areas.is_empty() {
                "No ticking areas".into()
            } else {
                areas.iter().map(|a| format!("- {}: chunks {} {} to {} {}", a.name, a.min.0, a.min.1, a.max.0, a.max.1)).collect::<Vec<_>>().join("\n")
            }
        }
        _ => "§cUse /tickingarea add|remove|remove_all|list".into(),
    }
}

/// /music: plays to everyone (Bedrock's /music has no target).
fn music(server: &Server, args: &[&str]) -> String {
    use bedrock::protocol::v712::packets::StopSoundPacket;
    let stop = || crate::Protocol::StopSoundPacket(Box::new(StopSoundPacket { sound_name: String::new(), stop_all_sounds: false, stop_music_legacy: true }));
    match args {
        ["play" | "queue", track, rest @ ..] => {
            let volume: f32 = rest.first().and_then(|v| v.parse().ok()).unwrap_or(1.0);
            if args[0] == "play" {
                let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(stop()) });
            }
            for p in server.all() {
                let mut pkt = crate::packets::play_sound_pitch(track, p.pos, 1.0);
                if let crate::Protocol::PlaySoundPacket(s) = &mut pkt {
                    s.volume = volume.clamp(0.01, 1.0);
                }
                let _ = server.events.send(Event::Packet { to: p.id, packet: Box::new(pkt) });
            }
            format!("Now playing {track}")
        }
        ["stop", ..] => {
            let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(stop()) });
            "Stopped the music".into()
        }
        ["volume", _] => "Music volume follows each player's own settings".into(),
        _ => "§cUse /music play|queue <track> [volume] | stop".into(),
    }
}

pub fn started() -> std::time::Instant {
    static T: OnceLock<std::time::Instant> = OnceLock::new();
    *T.get_or_init(std::time::Instant::now)
}

/// Runs a console-safe command. None if it isn't one.
pub fn run(server: &Server, sender: &str, args: &[&str]) -> Option<String> {
    let l = lists();
    let reply = match args {
        ["scoreboard", rest @ ..] => crate::ui::command(server, sender, rest),
        ["list"] => {
            let names: Vec<String> = server.all().into_iter().map(|p| p.name).collect();
            format!("There are {}/{} players online:\n{}", names.len(), max_players(), names.join(", "))
        }
        ["setmaxplayers", n] => match n.parse::<u32>() {
            Ok(n) => {
                // Vanilla clamps to 1..=30; dedicated servers here allow more.
                let n = n.clamp(1, 1000);
                MAX_PLAYERS.store(n, std::sync::atomic::Ordering::Relaxed);
                format!("Set max players to {n}")
            }
            Err(_) => "§cUse /setmaxplayers <number>".into(),
        },
        ["tickingarea", rest @ ..] => tickingarea(server, sender, rest),
        ["recipe", "give" | "take", ..] => "All recipes are always unlocked on this server".into(),
        ["music", rest @ ..] => music(server, rest),
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
                op_ids().lock().insert(t);
                server.tell(t, "§eYou are now op!");
            }
            format!("Opped: {who}")
        }
        ["deop", who] => {
            l.ops.set(who, false);
            if let Some(t) = server.find_by_name(who) {
                op_ids().lock().remove(&t);
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
        ["ban-ip" | "banip", target, why @ ..] => {
            // A player's name (online this run) or an address.
            let Some(ip) = ip_of(target).or_else(|| target.parse::<std::net::IpAddr>().ok().map(|_| target.to_string())) else {
                return Some(format!("§c{target} isn't a known player or an IP address"));
            };
            l.banned_ips.set(&ip, true);
            let message = if why.is_empty() { "You are banned from this server".to_string() } else { why.join(" ") };
            let mut kicked = 0;
            for p in server.all() {
                if ip_of(&p.name).as_deref() == Some(ip.as_str()) {
                    let _ = server.events.send(Event::Kick { id: p.id, message: message.clone(), reason: Kick::Kicked });
                    kicked += 1;
                }
            }
            format!("Banned IP {ip} ({kicked} kicked)")
        }
        ["unban-ip" | "pardon-ip", ip] => {
            if l.banned_ips.set(ip, false) { format!("Unbanned IP {ip}") } else { format!("§c{ip} is not banned") }
        }
        ["banlist", "ips"] => format!("Banned IPs: {}", l.banned_ips.all().join(", ")),
        ["unban" | "pardon", who] => {
            if l.banned.set(who, false) { format!("Unbanned player {who}") } else { format!("§c{who} is not banned") }
        }
        ["banlist"] => format!("Banned players: {}", l.banned.all().join(", ")),
        ["mv" | "multiverse", rest @ ..] => crate::multiworld::command(server, sender, rest),
        ["locate", "structure", what] | ["locate", what] => {
            let names = crate::overworld::structures::names();
            let what = what.trim_start_matches("minecraft:");
            if !names.contains(&what) {
                return Some(format!("§cUnknown structure. Try: {}", names.join(", ")));
            }
            let here = server.find_by_name(sender).and_then(|id| server.get(id)).map_or((0.0, 0.0), |p| (p.pos.0, p.pos.2));
            match server.world.locate(what, here.0 as i32, here.1 as i32) {
                Some((x, z)) => {
                    let d = ((x as f32 - here.0).powi(2) + (z as f32 - here.1).powi(2)).sqrt() as i32;
                    format!("The nearest {what} is at [{x}, ~, {z}] ({d} blocks away)")
                }
                None => format!("§cCould not find a {what} nearby"),
            }
        }
        ["whitelist", "on"] => {
            *l.whitelist_on.lock() = true;
            "Turned on the allowlist".into()
        }
        ["whitelist", "off"] => {
            *l.whitelist_on.lock() = false;
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
            match new {
                Weather::Clear => "Changing to clear weather".to_string(),
                Weather::Rain => "Changing to rainy weather".to_string(),
                Weather::Thunder => "Changing to rain and thunder".to_string(),
            }
        }
        ["difficulty"] => format!("Difficulty: {}", ["peaceful", "easy", "normal", "hard"][*DIFFICULTY.lock() as usize]),
        ["difficulty", d] => match difficulty_from(d) {
            Some(d) => {
                *DIFFICULTY.lock() = d;
                let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(difficulty_packet(d)) });
                format!("Set game difficulty to {}", ["peaceful", "easy", "normal", "hard"][d as usize])
            }
            None => "§cUse /difficulty peaceful|easy|normal|hard".into(),
        },
        ["seed"] => format!("Seed: {}", server.world.seed() as i64),
        ["perf", "parts"] => crate::perf::parts_text(),
        ["perf"] => match server.find_by_name(sender) {
            Some(id) if crate::perf::toggle(server, id) => "Server stats bar on. /perf again to hide it.".into(),
            Some(_) => "Server stats bar off.".into(),
            None => "Only players can see the stats bar; use /status here.".into(),
        },
        ["plugins" | "pl"] => {
            let names = server.plugins.summaries();
            format!("Plugins ({}): {}", names.len(), names.iter().map(|n| format!("§a{n}§f")).collect::<Vec<_>>().join(", "))
        }
        ["version" | "ver" | "about"] => format!(
            "This server is running Rusted {VERSION} for Minecraft: Bedrock Edition v1.26.60, written in Rust"
        ),
        ["status"] => {
            let up = started().elapsed().as_secs();
            format!(
                "§6---- §fServer status §6----\n§6Uptime: §a{}h {}m {}s\n§6Players: §a{}/{}\n§6Chunks cached: §a{}",
                up / 3600,
                up / 60 % 60,
                up % 60,
                server.all().len(),
                max_players(),
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
/// Feet position of a player.
fn feet_of(server: &crate::players::Server, id: i64) -> (f32, f32, f32) {
    server.get(id).map_or((0.0, 0.0, 0.0), |p| (p.pos.0, p.pos.1 - 1.62, p.pos.2))
}

/// One coordinate: `~`, `~2.5` (relative to `base`) or a number.
fn coord(s: &str, base: f32) -> Option<f32> {
    match s.strip_prefix('~') {
        Some("") => Some(base),
        Some(r) => r.parse::<f32>().ok().map(|d| base + d),
        None => s.parse().ok(),
    }
}

/// Turns `~` coordinates in a command into numbers around `at`, so a command run "at" a
/// place works no matter whose session runs it.
fn absolute(line: &str, at: (f32, f32, f32)) -> String {
    let mut t: Vec<String> = line.split_whitespace().map(str::to_string).collect();
    let mut i = 0;
    while i + 2 < t.len() {
        let triple = [&t[i], &t[i + 1], &t[i + 2]];
        if triple.iter().any(|x| x.starts_with('~')) && triple.iter().all(|x| coord(x, 0.0).is_some()) {
            for (k, base) in [at.0, at.1, at.2].into_iter().enumerate() {
                if t[i + k].starts_with('~') {
                    t[i + k] = format!("{:.2}", coord(&t[i + k], base).unwrap_or(base));
                }
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    t.join(" ")
}

/// /execute: `as <targets>`, `at <targets>`, `positioned <x y z>`, `if|unless block <x y z>
/// <block>`, `if|unless entity <targets>`, then `run <command>`, chained like vanilla.
fn execute(server: &crate::players::Server, me: i64, line: &str) -> Result<Vec<(i64, String)>, String> {
    let args: Vec<&str> = line.split_whitespace().skip(1).collect();
    // (who runs it, where it runs)
    let mut ctx: Vec<(i64, (f32, f32, f32))> = vec![(me, feet_of(server, me))];
    let mut i = 0;
    let usage = || "§cUsage: /execute [as|at <targets>] [positioned <x y z>] [if|unless block <x y z> <block>|entity <targets>] run <command>".to_string();
    while i < args.len() {
        match args[i] {
            "as" => {
                let sel = args.get(i + 1).ok_or_else(usage)?;
                let mut next = Vec::new();
                for (who, at) in &ctx {
                    for id in select(server, *who, sel)? {
                        next.push((id, *at));
                    }
                }
                ctx = next;
                i += 2;
            }
            "at" => {
                let sel = args.get(i + 1).ok_or_else(usage)?;
                let mut next = Vec::new();
                for (who, _) in &ctx {
                    for id in select(server, *who, sel)? {
                        next.push((*who, feet_of(server, id)));
                    }
                }
                ctx = next;
                i += 2;
            }
            "positioned" => {
                let [x, y, z] = [args.get(i + 1), args.get(i + 2), args.get(i + 3)].map(|a| a.copied().unwrap_or(""));
                for c in &mut ctx {
                    c.1 = (coord(x, c.1.0).ok_or_else(usage)?, coord(y, c.1.1).ok_or_else(usage)?, coord(z, c.1.2).ok_or_else(usage)?);
                }
                i += 4;
            }
            cond @ ("if" | "unless") => {
                let want = cond == "if";
                match args.get(i + 1).copied() {
                    Some("block") => {
                        let [x, y, z, b] = [args.get(i + 2), args.get(i + 3), args.get(i + 4), args.get(i + 5)].map(|a| a.copied().unwrap_or(""));
                        let block = format!("minecraft:{}", b.trim_start_matches("minecraft:"));
                        let mut keep = Vec::new();
                        for c in ctx {
                            let (bx, by, bz) = (coord(x, c.1.0).ok_or_else(usage)?, coord(y, c.1.1).ok_or_else(usage)?, coord(z, c.1.2).ok_or_else(usage)?);
                            let here = server.world.get(bx.floor() as i32, by.floor() as i32, bz.floor() as i32);
                            if crate::blocks::entry(here).is_some_and(|e| e.name == block) == want {
                                keep.push(c);
                            }
                        }
                        ctx = keep;
                        i += 6;
                    }
                    Some("entity") => {
                        let sel = args.get(i + 2).ok_or_else(usage)?;
                        let mut keep = Vec::new();
                        for c in ctx {
                            if select(server, c.0, sel).is_ok_and(|v| !v.is_empty()) == want {
                                keep.push(c);
                            }
                        }
                        ctx = keep;
                        i += 3;
                    }
                    _ => return Err(usage()),
                }
            }
            "run" => {
                let rest = args[i + 1..].join(" ");
                if rest.is_empty() {
                    return Err(usage());
                }
                let mut runs = Vec::new();
                for (who, at) in ctx {
                    runs.extend(expand(server, who, &absolute(&rest, at))?);
                }
                return Ok(runs);
            }
            _ => return Err(usage()),
        }
    }
    // No `run`: vanilla reports whether the conditions passed.
    Err(if ctx.is_empty() { "§cTest failed".into() } else { format!("Test passed, count: {}", ctx.len()) })
}

/// /function <name>: runs each line of functions/<name>.mcfunction (blank lines and `#`
/// comments skipped), as the sender.
fn function(server: &crate::players::Server, me: i64, line: &str) -> Result<Vec<(i64, String)>, String> {
    let Some(name) = line.split_whitespace().nth(1) else { return Err("§cUsage: /function <name>".into()) };
    if name.contains("..") || name.starts_with(['/', '\\']) {
        return Err("§cBad function name".into());
    }
    let path = std::path::Path::new("functions").join(format!("{name}.mcfunction"));
    let text = std::fs::read_to_string(&path).map_err(|_| format!("§cUnknown function: {name} (looked for {})", path.display()))?;
    let mut runs = Vec::new();
    for l in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        runs.extend(expand(server, me, l.trim_start_matches('/'))?);
    }
    Ok(runs)
}

#[cfg(test)]
mod execute_tests {
    #[test]
    fn tilde_coordinates_become_absolute() {
        assert_eq!(super::absolute("setblock ~ ~1 ~-2 stone", (10.0, 64.0, 5.0)), "setblock 10.00 65.00 3.00 stone");
        assert_eq!(super::absolute("say hi ~ there", (1.0, 2.0, 3.0)), "say hi ~ there", "not a coordinate triple");
        assert_eq!(super::absolute("tp @s 5 ~ 7", (0.0, 70.0, 0.0)), "tp @s 5 70.00 7");
    }
}

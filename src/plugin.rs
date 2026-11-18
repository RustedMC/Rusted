//! Plugin API. Implement `Plugin` and override the events you care about. Cancellable
//! events return `false` to cancel.
//! Every hook gets the server handle (`tell`, `broadcast`, `set_block`, `get`, `all`, ...).
//! Timers go through `server.scheduler` (20 ticks a second), settings through
//! `Config::load("name")`, and forms/scoreboards/boss bars live in `crate::ui`.
//!
//! Register plugins in `all()` below.
//!
//! Third-party plugins depend on these signatures. New events are fine (give them default
//! bodies), changing or removing one breaks plugins and should go in the release notes.
#![allow(dead_code)] // plugin API: plugins call these, the server itself may not

use crate::players::{PlayerInfo, Server};

pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;

    /// Player finished joining.
    fn on_join(&self, _server: &Server, _player: &PlayerInfo) {}

    /// Player left, kicks and disconnects included.
    fn on_leave(&self, _server: &Server, _player: &PlayerInfo) {}

    /// Chat before it's shown. Edit `message`, or return false to block it.
    fn on_chat(&self, _server: &Server, _player: &PlayerInfo, _message: &mut String) -> bool {
        true
    }

    /// Player breaking `block` at `pos`. Return false to cancel.
    fn on_block_break(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32), _block: i32) -> bool {
        true
    }

    /// Player placing `block` at `pos`. Return false to cancel.
    fn on_block_place(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32), _block: i32) -> bool {
        true
    }

    /// Server is up, worlds loaded, about to accept players.
    fn on_server_start(&self, _server: &Server) {}

    /// Server stopping, before the final save.
    fn on_server_stop(&self, _server: &Server) {}

    /// Command a player typed (no `/`), before it runs. Rewrite it, or return false to drop it
    /// (PNX PlayerCommandPreprocessEvent).
    fn on_command_preprocess(&self, _server: &Server, _player: &PlayerInfo, _line: &mut String) -> bool {
        true
    }

    /// Player about to take `amount` damage from `cause` ("fall", "lava", "Zombie"...).
    /// Return false to cancel (PNX EntityDamageEvent).
    fn on_player_damage(&self, _server: &Server, _player: &PlayerInfo, _cause: &str, _amount: f32) -> bool {
        true
    }

    /// Player died. Edit the death message, empty hides it.
    fn on_player_death(&self, _server: &Server, _player: &PlayerInfo, _message: &mut String) {}

    /// Player respawned.
    fn on_player_respawn(&self, _server: &Server, _player: &PlayerInfo) {}

    /// Player right-clicking `pos` holding `item` (PNX PlayerInteractEvent). Return false and
    /// nothing happens or gets placed.
    fn on_player_interact(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32), _item: &str) -> bool {
        true
    }

    /// Game mode about to change to `mode` ("survival", "creative"...). Return false to keep the old one.
    fn on_gamemode_change(&self, _server: &Server, _player: &PlayerInfo, _mode: &str) -> bool {
        true
    }

    /// Player threw `count` of `item` out of their inventory.
    fn on_item_drop(&self, _server: &Server, _player: &PlayerInfo, _item: &str, _count: i8) {}

    /// Player hitting `mob` (e.g. "minecraft:cow") for `damage`. Return false to cancel the hit
    /// (PNX EntityDamageByEntityEvent).
    fn on_entity_damage(&self, _server: &Server, _player: &PlayerInfo, _mob: i64, _kind: &str, _damage: f32) -> bool {
        true
    }

    /// Mob of `kind` died at `pos`, `killer` is a player id if a player did it.
    fn on_entity_death(&self, _server: &Server, _kind: &str, _pos: (f32, f32, f32), _killer: Option<i64>) {}

    /// Mob of `kind` about to spawn at `pos` (natural, /summon, breeding). Return false to stop
    /// it (PNX CreatureSpawnEvent).
    fn on_entity_spawn(&self, _server: &Server, _kind: &str, _pos: (f32, f32, f32)) -> bool {
        true
    }

    /// Something about to blow up at `pos` with `power` (creeper, TNT, anchor). Return false to
    /// stop it (PNX ExplosionPrimeEvent).
    fn on_explosion(&self, _server: &Server, _pos: (f32, f32, f32), _power: f32) -> bool {
        true
    }

    /// Weather changing to "clear", "rain" or "thunder". Return false to keep it.
    fn on_weather_change(&self, _server: &Server, _weather: &str) -> bool {
        true
    }

    /// (name, description) of commands this plugin handles, for autocomplete.
    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![]
    }

    /// Runs `/name args...` for one of this plugin's commands. The returned text goes to the player.
    fn on_command(&self, _server: &Server, _player: &PlayerInfo, _name: &str, _args: &[&str]) -> Option<String> {
        None
    }
}

/// Every loaded plugin. Add yours here.
pub fn all() -> Vec<Box<dyn Plugin>> {
    vec![]
}

/// Registered plugins, plus helpers to run an event through all of them.
#[derive(Default)]
pub struct Plugins(pub Vec<Box<dyn Plugin>>);

impl Plugins {
    pub fn join(&self, s: &Server, p: &PlayerInfo) {
        self.0.iter().for_each(|pl| pl.on_join(s, p));
    }

    pub fn leave(&self, s: &Server, p: &PlayerInfo) {
        self.0.iter().for_each(|pl| pl.on_leave(s, p));
    }

    /// False if any plugin blocked it. Each plugin can rewrite the message in turn.
    pub fn chat(&self, s: &Server, p: &PlayerInfo, message: &mut String) -> bool {
        self.0.iter().all(|pl| pl.on_chat(s, p, message))
    }

    pub fn block_break(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32), block: i32) -> bool {
        self.0.iter().all(|pl| pl.on_block_break(s, p, pos, block))
    }

    pub fn block_place(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32), block: i32) -> bool {
        self.0.iter().all(|pl| pl.on_block_place(s, p, pos, block))
    }

    pub fn server_start(&self, s: &Server) {
        self.0.iter().for_each(|pl| pl.on_server_start(s));
    }

    pub fn server_stop(&self, s: &Server) {
        self.0.iter().for_each(|pl| pl.on_server_stop(s));
    }

    pub fn command_preprocess(&self, s: &Server, p: &PlayerInfo, line: &mut String) -> bool {
        self.0.iter().all(|pl| pl.on_command_preprocess(s, p, line))
    }

    pub fn player_damage(&self, s: &Server, p: &PlayerInfo, cause: &str, amount: f32) -> bool {
        self.0.iter().all(|pl| pl.on_player_damage(s, p, cause, amount))
    }

    pub fn player_death(&self, s: &Server, p: &PlayerInfo, message: &mut String) {
        self.0.iter().for_each(|pl| pl.on_player_death(s, p, message));
    }

    pub fn player_respawn(&self, s: &Server, p: &PlayerInfo) {
        self.0.iter().for_each(|pl| pl.on_player_respawn(s, p));
    }

    pub fn player_interact(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32), item: &str) -> bool {
        self.0.iter().all(|pl| pl.on_player_interact(s, p, pos, item))
    }

    pub fn gamemode_change(&self, s: &Server, p: &PlayerInfo, mode: &str) -> bool {
        self.0.iter().all(|pl| pl.on_gamemode_change(s, p, mode))
    }

    pub fn item_drop(&self, s: &Server, p: &PlayerInfo, item: &str, count: i8) {
        self.0.iter().for_each(|pl| pl.on_item_drop(s, p, item, count));
    }

    pub fn entity_damage(&self, s: &Server, p: &PlayerInfo, mob: i64, kind: &str, damage: f32) -> bool {
        self.0.iter().all(|pl| pl.on_entity_damage(s, p, mob, kind, damage))
    }

    pub fn entity_death(&self, s: &Server, kind: &str, pos: (f32, f32, f32), killer: Option<i64>) {
        self.0.iter().for_each(|pl| pl.on_entity_death(s, kind, pos, killer));
    }

    pub fn entity_spawn(&self, s: &Server, kind: &str, pos: (f32, f32, f32)) -> bool {
        self.0.iter().all(|pl| pl.on_entity_spawn(s, kind, pos))
    }

    pub fn explosion(&self, s: &Server, pos: (f32, f32, f32), power: f32) -> bool {
        self.0.iter().all(|pl| pl.on_explosion(s, pos, power))
    }

    pub fn weather_change(&self, s: &Server, weather: &str) -> bool {
        self.0.iter().all(|pl| pl.on_weather_change(s, weather))
    }

    pub fn names(&self) -> Vec<String> {
        self.0.iter().map(|p| p.name().to_string()).collect()
    }

    pub fn commands(&self) -> Vec<(&'static str, &'static str)> {
        self.0.iter().flat_map(|pl| pl.commands()).collect()
    }

    /// First plugin that owns `name` handles it.
    pub fn command(&self, s: &Server, p: &PlayerInfo, name: &str, args: &[&str]) -> Option<String> {
        self.0
            .iter()
            .filter(|pl| pl.commands().iter().any(|(n, _)| *n == name))
            .find_map(|pl| pl.on_command(s, p, name, args))
    }
}

/// Scheduled task. Return false to stop repeating.
type Job = Box<dyn FnMut(&Server) -> bool + Send>;

struct Task {
    id: u64,
    due: u64,
    /// ticks between runs, None = run once
    every: Option<u64>,
    job: Job,
}

/// Runs plugin tasks on the game tick (20/s), like PNX's ServerScheduler. Everything runs
/// on the tick thread one after another, so keep tasks short.
#[derive(Default)]
pub struct Scheduler {
    tasks: parking_lot::Mutex<Vec<Task>>,
    tick: std::sync::atomic::AtomicU64,
    next_id: std::sync::atomic::AtomicU64,
}

impl Scheduler {
    fn add(&self, delay: u64, every: Option<u64>, job: Job) -> u64 {
        use std::sync::atomic::Ordering::Relaxed;
        let id = self.next_id.fetch_add(1, Relaxed) + 1;
        let due = self.tick.load(Relaxed) + delay.max(1);
        self.tasks.lock().push(Task { id, due, every, job });
        id
    }

    /// Run `f` once after `delay` ticks. Returns an id for `cancel`.
    pub fn run_later(&self, delay: u64, f: impl FnOnce(&Server) + Send + 'static) -> u64 {
        let mut f = Some(f);
        self.add(delay, None, Box::new(move |s| {
            if let Some(f) = f.take() {
                f(s);
            }
            false
        }))
    }

    /// Run `f` every `period` ticks after `delay`, until it returns false or gets cancelled.
    pub fn run_repeating(&self, delay: u64, period: u64, f: impl FnMut(&Server) -> bool + Send + 'static) -> u64 {
        self.add(delay, Some(period.max(1)), Box::new(f))
    }

    pub fn cancel(&self, id: u64) {
        self.tasks.lock().retain(|t| t.id != id);
    }

    /// Runs whatever is due this tick.
    pub fn tick(&self, server: &Server) {
        use std::sync::atomic::Ordering::Relaxed;
        let now = self.tick.fetch_add(1, Relaxed) + 1;
        let due: Vec<Task> = {
            let mut t = self.tasks.lock();
            let (due, rest): (Vec<Task>, Vec<Task>) = t.drain(..).partition(|t| t.due <= now);
            *t = rest;
            due
        };
        for mut task in due {
            let again = (task.job)(server);
            if let (true, Some(every)) = (again, task.every) {
                task.due = now + every;
                self.tasks.lock().push(task);
            }
        }
    }
}

/// Plugin settings in plugins/<name>/config.txt as `key = value` lines (like PNX Config).
/// Missing keys fall back to the default you pass in.
pub struct Config {
    path: std::path::PathBuf,
    values: Vec<(String, String)>,
}

impl Config {
    pub fn load(plugin: &str) -> Config {
        let safe: String = plugin.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
        let path = std::path::Path::new("plugins").join(safe).join("config.txt");
        let values = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .filter_map(|l| l.split_once('=').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
            .collect();
        Config { path, values }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// The value or `default`. The default also gets written so the file lists every setting.
    pub fn get_or(&mut self, key: &str, default: &str) -> String {
        match self.get(key) {
            Some(v) => v.to_string(),
            None => {
                self.set(key, default);
                default.to_string()
            }
        }
    }

    pub fn set(&mut self, key: &str, value: &str) {
        match self.values.iter_mut().find(|(k, _)| k == key) {
            Some(v) => v.1 = value.to_string(),
            None => self.values.push((key.to_string(), value.to_string())),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text: String = self.values.iter().map(|(k, v)| format!("{k} = {v}\n")).collect();
        std::fs::write(&self.path, text)
    }
}

/// Example plugin (not loaded by default), greets players and adds `/online`.
#[allow(dead_code)]
struct Welcome;

impl Plugin for Welcome {
    fn name(&self) -> &str {
        "welcome"
    }

    fn on_join(&self, server: &Server, player: &PlayerInfo) {
        server.tell(player.id, &format!("§aWelcome to Rusted, {}!", player.name));
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![("online", "List players online")]
    }

    fn on_command(&self, server: &Server, _player: &PlayerInfo, _name: &str, _args: &[&str]) -> Option<String> {
        let names: Vec<String> = server.all().into_iter().map(|p| p.name).collect();
        Some(format!("Online ({}): {}", names.len(), names.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoDirt;
    impl Plugin for NoDirt {
        fn name(&self) -> &str {
            "nodirt"
        }
        fn on_block_place(&self, _: &Server, _: &PlayerInfo, _: (i32, i32, i32), block: i32) -> bool {
            block != crate::world::DIRT
        }
        fn on_chat(&self, _: &Server, _: &PlayerInfo, message: &mut String) -> bool {
            *message = message.replace("darn", "****");
            !message.contains("spam")
        }
    }

    #[test]
    fn events_cancel_and_edit() {
        let plugins = Plugins(vec![Box::new(NoDirt), Box::new(Welcome)]);
        let server = Server::new();
        let player = crate::players::tests::sample(1);
        assert!(!plugins.block_place(&server, &player, (0, 0, 0), crate::world::DIRT));
        assert!(plugins.block_place(&server, &player, (0, 0, 0), crate::world::GRASS));
        let mut msg = "oh darn".to_string();
        assert!(plugins.chat(&server, &player, &mut msg));
        assert_eq!(msg, "oh ****");
        assert!(!plugins.chat(&server, &player, &mut "buy spam".to_string()));
        // commands go to the plugin that owns them
        assert!(plugins.command(&server, &player, "online", &[]).unwrap().starts_with("Online (0)"));
        assert!(plugins.command(&server, &player, "nope", &[]).is_none());
    }

    #[test]
    fn scheduler_runs_later_and_repeats() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;
        let server = Server::new();
        let once = Arc::new(AtomicU32::new(0));
        let reps = Arc::new(AtomicU32::new(0));
        let (o, r) = (once.clone(), reps.clone());
        server.scheduler.run_later(3, move |_| {
            o.fetch_add(1, Ordering::Relaxed);
        });
        let id = server.scheduler.run_repeating(1, 2, move |_| {
            r.fetch_add(1, Ordering::Relaxed);
            true
        });
        for _ in 0..10 {
            server.scheduler.tick(&server);
        }
        assert_eq!(once.load(Ordering::Relaxed), 1);
        assert_eq!(reps.load(Ordering::Relaxed), 5, "ticks 1,3,5,7,9");
        server.scheduler.cancel(id);
        for _ in 0..10 {
            server.scheduler.tick(&server);
        }
        assert_eq!(reps.load(Ordering::Relaxed), 5);
    }
}

//! Write a Rusted plugin that loads at runtime: build it for `wasm32-unknown-unknown` and drop
//! the `.wasm` file into the server's `plugins/` folder.
//!
//! ```ignore
//! use rusted_plugin::{Event, plugin};
//!
//! fn init() {
//!     rusted_plugin::register_command("hello", "Says hello");
//! }
//!
//! fn event(e: &Event) -> bool {
//!     match e.kind() {
//!         "join" => rusted_plugin::broadcast(&format!("Welcome, {}!", e.player_name())),
//!         "command" if e.command() == "hello" => rusted_plugin::reply("Hello!"),
//!         _ => {}
//!     }
//!     true // false cancels events that can be cancelled
//! }
//!
//! plugin!(init, event);
//! ```
//!
//! Events: server_start, server_stop, join, leave, chat (message), command_preprocess (line),
//! command (command, args), block_break / block_place (pos, block), interact (pos, item),
//! player_damage (cause, amount), player_death (message), respawn, sneak (on), mount (mob),
//! trade (villager, index, times), and every other server event (see the runtime plugins
//! page of the docs). Player events carry `player`: id, name, xuid, pos, rot.

pub use serde_json::Value;

mod sys {
    #[link(wasm_import_module = "rusted")]
    unsafe extern "C" {
        pub fn log(ptr: *const u8, len: usize);
        pub fn tell(to: i64, ptr: *const u8, len: usize);
        pub fn broadcast(ptr: *const u8, len: usize);
        pub fn run_command(ptr: *const u8, len: usize);
        pub fn run_as(to: i64, ptr: *const u8, len: usize);
        pub fn reply(ptr: *const u8, len: usize);
        pub fn register_command(np: *const u8, nn: usize, dp: *const u8, dn: usize);
        pub fn set_block(x: i32, y: i32, z: i32, ptr: *const u8, len: usize);
        pub fn give(to: i64, ptr: *const u8, len: usize, count: i32);
        pub fn teleport(to: i64, x: f32, y: f32, z: f32);
        pub fn spawn_mob(ptr: *const u8, len: usize, x: f32, y: f32, z: f32);
        pub fn play_sound(ptr: *const u8, len: usize, x: f32, y: f32, z: f32);
        pub fn effect(to: i64, id: i32, amplifier: i32, secs: i32);
        pub fn damage(to: i64, amount: f32);
        pub fn give_xp(to: i64, points: i32);
        pub fn kick(to: i64, ptr: *const u8, len: usize);
        pub fn title(to: i64, ptr: *const u8, len: usize);
        pub fn actionbar(to: i64, ptr: *const u8, len: usize);
        pub fn show_form(to: i64, form_id: i32, ptr: *const u8, len: usize);
        pub fn query(ptr: *const u8, len: usize) -> i32;
        pub fn answer(ptr: *mut u8);
    }
}

/// Puts a block (like "minecraft:stone", or "stone") at x, y, z.
pub fn set_block(x: i32, y: i32, z: i32, block: &str) {
    unsafe { sys::set_block(x, y, z, block.as_ptr(), block.len()) }
}

/// Gives a player items ("diamond" or "minecraft:diamond"); count is capped at a full stack.
pub fn give(player: i64, item: &str, count: i32) {
    unsafe { sys::give(player, item.as_ptr(), item.len(), count) }
}

/// Moves a player's feet to x, y, z.
pub fn teleport(player: i64, x: f32, y: f32, z: f32) {
    unsafe { sys::teleport(player, x, y, z) }
}

/// Spawns a mob ("zombie" or "minecraft:zombie") at x, y, z.
pub fn spawn_mob(kind: &str, x: f32, y: f32, z: f32) {
    unsafe { sys::spawn_mob(kind.as_ptr(), kind.len(), x, y, z) }
}

/// Plays a sound (like "random.levelup") at x, y, z for everyone near.
pub fn play_sound(name: &str, x: f32, y: f32, z: f32) {
    unsafe { sys::play_sound(name.as_ptr(), name.len(), x, y, z) }
}

/// Gives a player an effect by Bedrock id (1 speed, 10 regeneration...) for `secs` seconds.
pub fn effect(player: i64, id: i32, amplifier: i32, secs: i32) {
    unsafe { sys::effect(player, id, amplifier, secs) }
}

/// Hurts a player (magic damage, armor doesn't help).
pub fn damage(player: i64, amount: f32) {
    unsafe { sys::damage(player, amount) }
}

/// Gives (or, negative, takes) experience points.
pub fn give_xp(player: i64, points: i32) {
    unsafe { sys::give_xp(player, points) }
}

/// Kicks a player with a message.
pub fn kick(player: i64, msg: &str) {
    unsafe { sys::kick(player, msg.as_ptr(), msg.len()) }
}

/// Shows big text in the middle of a player's screen.
pub fn title(player: i64, text: &str) {
    unsafe { sys::title(player, text.as_ptr(), text.len()) }
}

/// Shows text above a player's hotbar.
pub fn actionbar(player: i64, text: &str) {
    unsafe { sys::actionbar(player, text.as_ptr(), text.len()) }
}

/// Opens a form (Bedrock form JSON). The answer comes back as a `form_response` event with
/// this `form_id`; use ids above 1000000 so they don't clash with the server's own forms.
pub fn show_form(player: i64, form_id: i32, json: &str) {
    unsafe { sys::show_form(player, form_id, json.as_ptr(), json.len()) }
}

/// Asks the server something: "block x y z" (its name), "player <name>" (JSON or ""),
/// "players" (JSON list), "time", "get <key>", "set <key> <value>" (your plugin's saved data;
/// an empty value deletes the key).
pub fn query(request: &str) -> String {
    let len = unsafe { sys::query(request.as_ptr(), request.len()) };
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u8; len as usize];
    unsafe { sys::answer(buf.as_mut_ptr()) };
    String::from_utf8_lossy(&buf).into_owned()
}

/// Your plugin's saved value for `key` (kept in plugins/<name>.data.json), or "".
pub fn get_data(key: &str) -> String {
    query(&format!("get {key}"))
}

/// Saves `value` under `key` (no spaces in the key; "" deletes it).
pub fn set_data(key: &str, value: &str) {
    query(&format!("set {key} {value}"));
}

/// Writes a line to the server console, tagged with the plugin's name.
pub fn log(msg: &str) {
    unsafe { sys::log(msg.as_ptr(), msg.len()) }
}

/// Sends a chat message to one player.
pub fn tell(player: i64, msg: &str) {
    unsafe { sys::tell(player, msg.as_ptr(), msg.len()) }
}

/// Sends a chat message to everyone.
pub fn broadcast(msg: &str) {
    unsafe { sys::broadcast(msg.as_ptr(), msg.len()) }
}

/// Runs a command as the console (e.g. "time set day", "say hi").
pub fn run_command(line: &str) {
    unsafe { sys::run_command(line.as_ptr(), line.len()) }
}

/// Runs a command as a player (e.g. "tp @s 0 100 0").
pub fn run_as(player: i64, line: &str) {
    unsafe { sys::run_as(player, line.as_ptr(), line.len()) }
}

/// The answer to a `command` event, or the new text of a `chat`, `command_preprocess` or
/// `player_death` event.
pub fn reply(text: &str) {
    unsafe { sys::reply(text.as_ptr(), text.len()) }
}

/// Adds a command (call it from your init function).
pub fn register_command(name: &str, description: &str) {
    unsafe { sys::register_command(name.as_ptr(), name.len(), description.as_ptr(), description.len()) }
}

/// One event, as the server sent it.
pub struct Event(pub Value);

impl Event {
    pub fn kind(&self) -> &str {
        self.0["event"].as_str().unwrap_or("")
    }
    pub fn player_id(&self) -> i64 {
        self.0["player"]["id"].as_i64().unwrap_or(0)
    }
    pub fn player_name(&self) -> &str {
        self.0["player"]["name"].as_str().unwrap_or("")
    }
    pub fn command(&self) -> &str {
        self.0["command"].as_str().unwrap_or("")
    }
    pub fn args(&self) -> Vec<&str> {
        self.0["args"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default()
    }
    pub fn message(&self) -> &str {
        self.0["message"].as_str().unwrap_or("")
    }
    pub fn block(&self) -> &str {
        self.0["block"].as_str().unwrap_or("")
    }
    pub fn pos(&self) -> Option<(i64, i64, i64)> {
        let p = self.0["pos"].as_array()?;
        Some((p.first()?.as_f64()? as i64, p.get(1)?.as_f64()? as i64, p.get(2)?.as_f64()? as i64))
    }
}

#[doc(hidden)]
pub mod __export {
    /// Memory the server writes an event into.
    pub fn alloc(len: i32) -> i32 {
        let mut v = Vec::<u8>::with_capacity(len.max(0) as usize);
        let p = v.as_mut_ptr();
        std::mem::forget(v);
        p as i32
    }

    /// Reads (and frees) the event, then calls the plugin's handler.
    pub fn event(ptr: i32, len: i32, handler: fn(&super::Event) -> bool) -> i32 {
        let len = len.max(0) as usize;
        let bytes = unsafe { Vec::from_raw_parts(ptr as *mut u8, len, len) };
        let value = serde_json::from_slice(&bytes).unwrap_or(super::Value::Null);
        handler(&super::Event(value)) as i32
    }
}

/// Exports your init and event functions to the server.
#[macro_export]
macro_rules! plugin {
    ($init:path, $event:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn rusted_alloc(len: i32) -> i32 {
            $crate::__export::alloc(len)
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn rusted_init() {
            $init()
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn rusted_event(ptr: i32, len: i32) -> i32 {
            $crate::__export::event(ptr, len, $event)
        }
    };
}

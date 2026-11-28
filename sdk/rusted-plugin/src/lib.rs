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
//! trade (villager, index, times). Player events carry `player`: id, name, xuid, pos, rot.

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
    }
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

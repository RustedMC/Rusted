//! Runtime plugins: every `plugins/*.wasm` file loads at start-up and gets events, sandboxed.
//!
//! The plugin side (see `sdk/rusted-plugin`) exports `memory`, `rusted_alloc(len) -> ptr`,
//! optionally `rusted_init()`, and `rusted_event(ptr, len) -> i32`, which gets one event as JSON
//! and returns 1 to let it happen or 0 to cancel it. It can call back into these imports
//! (module "rusted"), all taking UTF-8 strings as (ptr, len):
//!   log(msg) · tell(player_id: i64, msg) · broadcast(msg) · run_command(line) (as the console)
//!   run_as(player_id: i64, line) · register_command(name, description) (in rusted_init only)
//!   reply(text): the answer to a command, or the new text of an editable event (chat, a
//!   command line before it runs).
//! Calls back into the server are queued and carried out once the plugin has returned, so a
//! plugin can't block the server or loop back into itself. Each call gets a fuel budget, so a
//! plugin stuck in a loop is cut off instead of freezing the server.

use crate::players::{Event, PlayerInfo, Server};
use crate::plugin::Plugin;
use parking_lot::Mutex;
use serde_json::{Value, json};
use wasmi::{Caller, Config, Engine, Extern, Instance, Linker, Module, Store, TypedFunc};

/// Work one event may do before the plugin is stopped (about 10-50 ms of interpreted code).
const FUEL_PER_CALL: u64 = 20_000_000;
/// Start-up gets more.
const FUEL_INIT: u64 = 200_000_000;

#[derive(Debug, PartialEq)]
enum Action {
    Log(String),
    Tell(i64, String),
    Broadcast(String),
    Console(String),
    RunAs(i64, String),
}

#[derive(Default)]
struct Host {
    actions: Vec<Action>,
    reply: Option<String>,
    commands: Vec<(String, String)>,
    initializing: bool,
}

struct Guest {
    store: Store<Host>,
    instance: Instance,
    alloc: TypedFunc<i32, i32>,
    event: TypedFunc<(i32, i32), i32>,
    broken: bool,
}

pub struct WasmPlugin {
    name: String,
    commands: Vec<(&'static str, &'static str)>,
    guest: Mutex<Guest>,
}

fn read_str(caller: &Caller<'_, Host>, ptr: i32, len: i32) -> Option<String> {
    let Some(Extern::Memory(mem)) = caller.get_export("memory") else { return None };
    let data = mem.data(caller);
    let (start, len) = (usize::try_from(ptr).ok()?, usize::try_from(len).ok()?);
    // Plugins can't make the server read outside their own memory.
    let bytes = data.get(start..start.checked_add(len)?)?;
    Some(String::from_utf8_lossy(bytes).into_owned())
}

fn linker(engine: &Engine) -> Result<Linker<Host>, wasmi::Error> {
    let mut l = Linker::<Host>::new(engine);
    l.func_wrap("rusted", "log", |c: Caller<'_, Host>, p: i32, n: i32| {
        if let Some(s) = read_str(&c, p, n) {
            let mut c = c;
            c.data_mut().actions.push(Action::Log(s));
        }
    })?;
    l.func_wrap("rusted", "tell", |mut c: Caller<'_, Host>, to: i64, p: i32, n: i32| {
        if let Some(s) = read_str(&c, p, n) {
            c.data_mut().actions.push(Action::Tell(to, s));
        }
    })?;
    l.func_wrap("rusted", "broadcast", |mut c: Caller<'_, Host>, p: i32, n: i32| {
        if let Some(s) = read_str(&c, p, n) {
            c.data_mut().actions.push(Action::Broadcast(s));
        }
    })?;
    l.func_wrap("rusted", "run_command", |mut c: Caller<'_, Host>, p: i32, n: i32| {
        if let Some(s) = read_str(&c, p, n) {
            c.data_mut().actions.push(Action::Console(s));
        }
    })?;
    l.func_wrap("rusted", "run_as", |mut c: Caller<'_, Host>, to: i64, p: i32, n: i32| {
        if let Some(s) = read_str(&c, p, n) {
            c.data_mut().actions.push(Action::RunAs(to, s));
        }
    })?;
    l.func_wrap("rusted", "reply", |mut c: Caller<'_, Host>, p: i32, n: i32| {
        if let Some(s) = read_str(&c, p, n) {
            c.data_mut().reply = Some(s);
        }
    })?;
    l.func_wrap("rusted", "register_command", |mut c: Caller<'_, Host>, np: i32, nn: i32, dp: i32, dn: i32| {
        if let (Some(name), Some(desc)) = (read_str(&c, np, nn), read_str(&c, dp, dn)) {
            let h = c.data_mut();
            let ok = h.initializing && !name.is_empty() && name.len() <= 32 && !name.contains(char::is_whitespace) && h.commands.len() < 64;
            if ok {
                h.commands.push((name.trim_start_matches('/').to_lowercase(), desc));
            }
        }
    })?;
    Ok(l)
}

/// Loads one plugin file. The plugin's name is its file name.
pub fn load(name: &str, bytes: &[u8]) -> Result<WasmPlugin, String> {
    let mut config = Config::default();
    config.consume_fuel(true);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, bytes).map_err(|e| format!("not a valid plugin: {e}"))?;
    let mut store = Store::new(&engine, Host::default());
    let instance = linker(&engine)
        .and_then(|l| l.instantiate_and_start(&mut store, &module))
        .map_err(|e| format!("could not start: {e}"))?;
    let alloc = instance.get_typed_func::<i32, i32>(&store, "rusted_alloc").map_err(|_| "missing export rusted_alloc".to_string())?;
    let event = instance.get_typed_func::<(i32, i32), i32>(&store, "rusted_event").map_err(|_| "missing export rusted_event".to_string())?;
    if instance.get_memory(&store, "memory").is_none() {
        return Err("missing export memory".into());
    }
    if let Ok(init) = instance.get_typed_func::<(), ()>(&store, "rusted_init") {
        store.data_mut().initializing = true;
        let _ = store.set_fuel(FUEL_INIT);
        init.call(&mut store, ()).map_err(|e| format!("rusted_init failed: {e}"))?;
        store.data_mut().initializing = false;
    }
    // The plugin trait wants commands for the server's whole life; there are at most 64 each.
    let commands = store.data().commands.iter().map(|(n, d)| (&*Box::leak(n.clone().into_boxed_str()), &*Box::leak(d.clone().into_boxed_str()))).collect();
    let early = std::mem::take(&mut store.data_mut().actions);
    for a in early {
        if let Action::Log(s) = a {
            println!("[{name}] {s}");
        }
    }
    Ok(WasmPlugin { name: name.to_string(), commands, guest: Mutex::new(Guest { store, instance, alloc, event, broken: false }) })
}

/// Every `.wasm` file in `dir`, in name order. Bad files are reported and skipped.
pub fn load_dir(dir: &str) -> Vec<Box<dyn Plugin>> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut paths: Vec<_> = rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "wasm")).collect();
    paths.sort();
    let mut out: Vec<Box<dyn Plugin>> = Vec::new();
    for p in paths {
        let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        match std::fs::read(&p).map_err(|e| e.to_string()).and_then(|b| load(&name, &b)) {
            Ok(pl) => {
                println!("Loaded plugin {name} ({} commands)", pl.commands.len());
                out.push(Box::new(pl));
            }
            Err(e) => println!("Plugin {}: {e}", p.display()),
        }
    }
    out
}

fn player_json(p: &PlayerInfo) -> Value {
    json!({ "id": p.id, "name": p.name, "xuid": p.xuid, "pos": [p.pos.0, p.pos.1 - 1.62, p.pos.2], "rot": [p.rot.0, p.rot.1] })
}

impl WasmPlugin {
    /// Sends one event. Returns (allowed, reply).
    fn call(&self, server: &Server, event: Value) -> (bool, Option<String>) {
        let mut g = self.guest.lock();
        if g.broken {
            return (true, None);
        }
        let text = event.to_string();
        let result = (|| -> Result<i32, String> {
            let Guest { store, instance, alloc, event, .. } = &mut *g;
            let _ = store.set_fuel(FUEL_PER_CALL);
            store.data_mut().reply = None;
            let len = i32::try_from(text.len()).map_err(|_| "event too big")?;
            let ptr = alloc.call(&mut *store, len).map_err(|e| e.to_string())?;
            let mem = instance.get_memory(&*store, "memory").ok_or("no memory")?;
            mem.write(&mut *store, ptr as u32 as usize, text.as_bytes()).map_err(|e| e.to_string())?;
            event.call(&mut *store, (ptr, len)).map_err(|e| e.to_string())
        })();
        let actions = std::mem::take(&mut g.store.data_mut().actions);
        let reply = g.store.data_mut().reply.take();
        let allowed = match result {
            Ok(v) => v != 0,
            Err(e) => {
                // A plugin that traps (a panic, running out of fuel) is switched off, not the server.
                println!("Plugin {} failed and was turned off: {e}", self.name);
                g.broken = true;
                true
            }
        };
        drop(g);
        for a in actions {
            match a {
                Action::Log(s) => println!("[{}] {s}", self.name),
                Action::Tell(to, s) => server.tell(to, &s),
                Action::Broadcast(s) => server.broadcast(&s),
                Action::Console(line) => {
                    let args: Vec<&str> = line.trim().trim_start_matches('/').split_whitespace().collect();
                    if let Some(r) = crate::commands::run(server, crate::commands::CONSOLE, &args)
                        && !r.is_empty()
                    {
                        println!("[{}] {r}", self.name);
                    }
                }
                Action::RunAs(to, line) => {
                    let _ = server.events.send(Event::RunAs { to, line: line.trim_start_matches('/').to_string(), reply_to: to });
                }
            }
        }
        (allowed, reply)
    }

    fn notify(&self, server: &Server, event: Value) {
        self.call(server, event);
    }

    fn ask(&self, server: &Server, event: Value) -> bool {
        self.call(server, event).0
    }
}

impl Plugin for WasmPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        self.commands.clone()
    }

    fn on_command(&self, server: &Server, player: &PlayerInfo, name: &str, args: &[&str]) -> Option<String> {
        let (_, reply) = self.call(server, json!({ "event": "command", "player": player_json(player), "command": name, "args": args }));
        Some(reply.unwrap_or_default())
    }

    fn on_server_start(&self, server: &Server) {
        self.notify(server, json!({ "event": "server_start" }));
    }

    fn on_server_stop(&self, server: &Server) {
        self.notify(server, json!({ "event": "server_stop" }));
    }

    fn on_join(&self, server: &Server, player: &PlayerInfo) {
        self.notify(server, json!({ "event": "join", "player": player_json(player) }));
    }

    fn on_leave(&self, server: &Server, player: &PlayerInfo) {
        self.notify(server, json!({ "event": "leave", "player": player_json(player) }));
    }

    fn on_chat(&self, server: &Server, player: &PlayerInfo, message: &mut String) -> bool {
        let (ok, reply) = self.call(server, json!({ "event": "chat", "player": player_json(player), "message": message }));
        if let Some(r) = reply {
            *message = r;
        }
        ok
    }

    fn on_command_preprocess(&self, server: &Server, player: &PlayerInfo, line: &mut String) -> bool {
        let (ok, reply) = self.call(server, json!({ "event": "command_preprocess", "player": player_json(player), "line": line }));
        if let Some(r) = reply {
            *line = r;
        }
        ok
    }

    fn on_block_break(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32), block: i32) -> bool {
        let name = crate::blocks::entry(block).map_or("", |e| e.name.as_str());
        self.ask(server, json!({ "event": "block_break", "player": player_json(player), "pos": [pos.0, pos.1, pos.2], "block": name }))
    }

    fn on_block_place(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32), block: i32) -> bool {
        let name = crate::blocks::entry(block).map_or("", |e| e.name.as_str());
        self.ask(server, json!({ "event": "block_place", "player": player_json(player), "pos": [pos.0, pos.1, pos.2], "block": name }))
    }

    fn on_player_interact(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32), item: &str) -> bool {
        self.ask(server, json!({ "event": "interact", "player": player_json(player), "pos": [pos.0, pos.1, pos.2], "item": item }))
    }

    fn on_player_damage(&self, server: &Server, player: &PlayerInfo, cause: &str, amount: f32) -> bool {
        self.ask(server, json!({ "event": "player_damage", "player": player_json(player), "cause": cause, "amount": amount }))
    }

    fn on_player_death(&self, server: &Server, player: &PlayerInfo, message: &mut String) {
        let (_, reply) = self.call(server, json!({ "event": "player_death", "player": player_json(player), "message": message }));
        if let Some(r) = reply {
            *message = r;
        }
    }

    fn on_player_respawn(&self, server: &Server, player: &PlayerInfo) {
        self.notify(server, json!({ "event": "respawn", "player": player_json(player) }));
    }

    fn on_sneak(&self, server: &Server, player: &PlayerInfo, on: bool) {
        self.notify(server, json!({ "event": "sneak", "player": player_json(player), "on": on }));
    }

    fn on_mount(&self, server: &Server, player: &PlayerInfo, mob: i64) {
        self.notify(server, json!({ "event": "mount", "player": player_json(player), "mob": mob }));
    }

    fn on_trade(&self, server: &Server, player: &PlayerInfo, villager: i64, index: i32, times: i32) {
        self.notify(server, json!({ "event": "trade", "player": player_json(player), "villager": villager, "index": index, "times": times }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny hand-written plugin (WebAssembly bytes): registers nothing, exports a bump
    /// allocator and an event handler that cancels every event. Built from this text:
    /// (module (memory (export "memory") 1)
    ///   (global $top (mut i32) (i32.const 1024))
    ///   (func (export "rusted_alloc") (param i32) (result i32)
    ///     (local i32) global.get $top local.set 1
    ///     global.get $top local.get 0 i32.add global.set $top local.get 1)
    ///   (func (export "rusted_event") (param i32 i32) (result i32) i32.const 0))
    const DENY_ALL: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic, version
        0x01, 0x0c, 0x02, 0x60, 0x01, 0x7f, 0x01, 0x7f, 0x60, 0x02, 0x7f, 0x7f, 0x01, 0x7f, // types
        0x03, 0x03, 0x02, 0x00, 0x01, // functions
        0x05, 0x03, 0x01, 0x00, 0x01, // memory
        0x06, 0x07, 0x01, 0x7f, 0x01, 0x41, 0x80, 0x08, 0x0b, // global top = 1024
        0x07, 0x28, 0x03, // exports
        0x06, b'm', b'e', b'm', b'o', b'r', b'y', 0x02, 0x00,
        0x0c, b'r', b'u', b's', b't', b'e', b'd', b'_', b'a', b'l', b'l', b'o', b'c', 0x00, 0x00,
        0x0c, b'r', b'u', b's', b't', b'e', b'd', b'_', b'e', b'v', b'e', b'n', b't', 0x00, 0x01,
        0x0a, 0x18, 0x02, // code
        0x11, 0x01, 0x01, 0x7f, 0x23, 0x00, 0x21, 0x01, 0x23, 0x00, 0x20, 0x00, 0x6a, 0x24, 0x00, 0x20, 0x01, 0x0b,
        0x04, 0x00, 0x41, 0x00, 0x0b,
    ];

    #[test]
    fn loads_a_plugin_and_it_can_cancel() {
        let p = load("deny", DENY_ALL).expect("loads");
        assert!(p.commands.is_empty());
        let server = Server::new();
        assert!(!p.ask(&server, json!({ "event": "block_break" })), "the plugin said no");
        assert!(load("junk", b"not wasm").is_err());
    }
}

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
//!   set_block(x, y, z, name) · give(player, item, count) · teleport(player, x, y, z)
//!   spawn_mob(kind, x, y, z) · play_sound(name, x, y, z) · effect(player, id, amplifier, secs)
//!   damage(player, amount) · give_xp(player, points) · kick(player, msg) · title(player, text)
//!   actionbar(player, text) · show_form(player, form_id, json): the answer comes back as a
//!   form_response event.
//!   query(request) -> length, then answer(ptr) copies the answer into a buffer that long.
//!   Requests: "block x y z", "player <name>", "players", "time", "get <key>", "set <key> <value>"
//!   (a plugin's own saved data, plugins/<name>.data.json).
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
    SetBlock((i32, i32, i32), String),
    Give(i64, String, i32),
    Teleport(i64, (f32, f32, f32)),
    Spawn(String, (f32, f32, f32)),
    Sound(String, (f32, f32, f32)),
    Effect(i64, i32, i32, i32),
    Damage(i64, f32),
    Xp(i64, i32),
    Kick(i64, String),
    Title(i64, String, bool),
    Form(i64, u32, String),
}

#[derive(Default)]
struct Host {
    actions: Vec<Action>,
    reply: Option<String>,
    commands: Vec<(String, String)>,
    initializing: bool,
    /// The plugin's name, for its data file.
    name: String,
    /// Set for the length of each call, so queries can look at the world.
    server: Option<Server>,
    /// The last query's answer, waiting for `answer(ptr)`.
    answer: String,
}

/// A plugin's saved data, plugins/<name>.data.json (a flat object of strings).
fn data_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new("plugins").join(format!("{name}.data.json"))
}

/// Biggest data file a plugin may keep, so a buggy one can't fill the disk.
const MAX_DATA: usize = 1 << 20;

/// Answers one query (see the module docs). Unknown or bad requests answer "".
fn answer(h: &Host, request: &str) -> String {
    let f: Vec<&str> = request.split(' ').collect();
    let num = |i: usize| f.get(i).and_then(|v| v.parse::<i32>().ok());
    let data = || -> serde_json::Map<String, Value> {
        std::fs::read_to_string(data_path(&h.name)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    };
    match (f.first().copied(), &h.server) {
        (Some("block"), Some(s)) => match (num(1), num(2), num(3)) {
            (Some(x), Some(y), Some(z)) => crate::blocks::entry(s.world.get(x, y, z)).map_or(String::new(), |e| e.name.clone()),
            _ => String::new(),
        },
        (Some("player"), Some(s)) => f.get(1).and_then(|n| s.find_by_name(n)).and_then(|id| s.get(id)).map_or(String::new(), |p| player_json(&p).to_string()),
        (Some("players"), Some(s)) => Value::Array(s.all().iter().map(player_json).collect()).to_string(),
        (Some("time"), Some(s)) => s.time().to_string(),
        (Some("get"), _) => f.get(1).and_then(|k| data().get(*k).and_then(|v| v.as_str().map(str::to_string))).unwrap_or_default(),
        (Some("set"), _) if f.len() >= 2 && !f[1].is_empty() => {
            let mut d = data();
            let value = f[2..].join(" ");
            if value.is_empty() {
                d.remove(f[1]);
            } else {
                d.insert(f[1].to_string(), Value::String(value));
            }
            let text = Value::Object(d).to_string();
            if text.len() > MAX_DATA {
                return "too big".into();
            }
            let _ = std::fs::create_dir_all("plugins");
            match std::fs::write(data_path(&h.name), text) {
                Ok(()) => "ok".into(),
                Err(_) => String::new(),
            }
        }
        _ => String::new(),
    }
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
    l.func_wrap("rusted", "set_block", |mut c: Caller<'_, Host>, x: i32, y: i32, z: i32, p: i32, n: i32| {
        if let Some(s) = read_str(&c, p, n) {
            c.data_mut().actions.push(Action::SetBlock((x, y, z), s));
        }
    })?;
    l.func_wrap("rusted", "give", |mut c: Caller<'_, Host>, to: i64, p: i32, n: i32, count: i32| {
        if let Some(s) = read_str(&c, p, n) {
            c.data_mut().actions.push(Action::Give(to, s, count));
        }
    })?;
    l.func_wrap("rusted", "teleport", |mut c: Caller<'_, Host>, to: i64, x: f32, y: f32, z: f32| {
        c.data_mut().actions.push(Action::Teleport(to, (x, y, z)));
    })?;
    l.func_wrap("rusted", "spawn_mob", |mut c: Caller<'_, Host>, p: i32, n: i32, x: f32, y: f32, z: f32| {
        if let Some(s) = read_str(&c, p, n) {
            c.data_mut().actions.push(Action::Spawn(s, (x, y, z)));
        }
    })?;
    l.func_wrap("rusted", "play_sound", |mut c: Caller<'_, Host>, p: i32, n: i32, x: f32, y: f32, z: f32| {
        if let Some(s) = read_str(&c, p, n) {
            c.data_mut().actions.push(Action::Sound(s, (x, y, z)));
        }
    })?;
    l.func_wrap("rusted", "effect", |mut c: Caller<'_, Host>, to: i64, id: i32, amplifier: i32, secs: i32| {
        c.data_mut().actions.push(Action::Effect(to, id, amplifier, secs));
    })?;
    l.func_wrap("rusted", "damage", |mut c: Caller<'_, Host>, to: i64, amount: f32| {
        c.data_mut().actions.push(Action::Damage(to, amount));
    })?;
    l.func_wrap("rusted", "give_xp", |mut c: Caller<'_, Host>, to: i64, points: i32| {
        c.data_mut().actions.push(Action::Xp(to, points));
    })?;
    l.func_wrap("rusted", "kick", |mut c: Caller<'_, Host>, to: i64, p: i32, n: i32| {
        if let Some(s) = read_str(&c, p, n) {
            c.data_mut().actions.push(Action::Kick(to, s));
        }
    })?;
    l.func_wrap("rusted", "title", |mut c: Caller<'_, Host>, to: i64, p: i32, n: i32| {
        if let Some(s) = read_str(&c, p, n) {
            c.data_mut().actions.push(Action::Title(to, s, false));
        }
    })?;
    l.func_wrap("rusted", "actionbar", |mut c: Caller<'_, Host>, to: i64, p: i32, n: i32| {
        if let Some(s) = read_str(&c, p, n) {
            c.data_mut().actions.push(Action::Title(to, s, true));
        }
    })?;
    l.func_wrap("rusted", "show_form", |mut c: Caller<'_, Host>, to: i64, id: i32, p: i32, n: i32| {
        if let Some(s) = read_str(&c, p, n) {
            c.data_mut().actions.push(Action::Form(to, id as u32, s));
        }
    })?;
    l.func_wrap("rusted", "query", |mut c: Caller<'_, Host>, p: i32, n: i32| -> i32 {
        let Some(req) = read_str(&c, p, n) else { return 0 };
        let a = answer(c.data(), &req);
        let len = a.len() as i32;
        c.data_mut().answer = a;
        len
    })?;
    l.func_wrap("rusted", "answer", |mut c: Caller<'_, Host>, p: i32| {
        let a = std::mem::take(&mut c.data_mut().answer);
        if let Some(Extern::Memory(mem)) = c.get_export("memory") {
            // a bad pointer just fails to write; the plugin can't reach outside its memory
            let _ = mem.write(&mut c, p as u32 as usize, a.as_bytes());
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
    let mut store = Store::new(&engine, Host { name: name.to_string(), ..Host::default() });
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

fn name_of(item: &str) -> String {
    if item.contains(':') { item.to_string() } else { format!("minecraft:{item}") }
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
            store.data_mut().server = Some(server.clone());
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
                Action::SetBlock((x, y, z), name) => {
                    if let Some(b) = crate::cmdargs::block_by_name(&name) {
                        server.set_block(0, x, y, z, b);
                    }
                }
                Action::Give(to, item, count) => {
                    if let Some(it) = crate::items::item_by_name(&name_of(&item)) {
                        let count = count.clamp(1, crate::items::max_stack(it.id) as i32) as i8;
                        let _ = server.events.send(Event::Give { to, item: Box::new(crate::inventory::Stack { item: it, count, net_id: 0 }) });
                    }
                }
                Action::Teleport(id, feet) => server.teleport(id, feet),
                Action::Spawn(kind, at) => {
                    if let Some(k) = crate::mobs::Kind::from_name(&kind) {
                        server.mobs.spawn(server, k, at);
                    }
                }
                Action::Sound(name, at) => {
                    let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(crate::packets::play_sound(&name, at)) });
                }
                Action::Effect(to, effect, amplifier, secs) => {
                    let _ = server.events.send(Event::Effect { to, effect, amplifier: amplifier.clamp(0, 255), ticks: secs.clamp(0, 1_000_000) * 20 });
                }
                Action::Damage(to, amount) if amount.is_finite() && amount > 0.0 => {
                    let _ = server.events.send(Event::Damage { to, damage: crate::health::Damage::Magic(amount), knock: (0.0, 0.0, 0.0) });
                }
                Action::Damage(..) => {}
                Action::Xp(to, points) => {
                    let _ = server.events.send(Event::Xp { to, points });
                }
                Action::Kick(id, message) => server.kick(id, &message),
                Action::Title(to, text, bar) => {
                    use bedrock::protocol::v712::packets::{SetTitlePacket, TitleType};
                    let packet = crate::Protocol::SetTitlePacket(Box::new(SetTitlePacket {
                        title_type: if bar { TitleType::Actionbar } else { TitleType::Title },
                        title_text: text.clone(),
                        fade_in_time: 10,
                        stay_time: 70,
                        fade_out_time: 20,
                        xuid: String::new(),
                        platform_online_id: String::new(),
                        filtered_title_text: text,
                    }));
                    let _ = server.events.send(Event::Packet { to, packet: Box::new(packet) });
                }
                Action::Form(to, form_id, json) => {
                    let packet = crate::Protocol::ModalFormRequestPacket(Box::new(bedrock::protocol::v662::packets::ModalFormRequestPacket { form_id, form_ui_json: json }));
                    let _ = server.events.send(Event::Packet { to, packet: Box::new(packet) });
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
    fn on_tick(&self, server: &Server) {
        self.notify(server, json!({ "event": "tick" }));
    }

    fn on_gamemode_change(&self, server: &Server, player: &PlayerInfo, mode: &str) -> bool {
        self.ask(server, json!({ "event": "gamemode_change", "player": player_json(player), "mode": mode }))
    }

    fn on_item_drop(&self, server: &Server, player: &PlayerInfo, item: &str, count: i8) -> bool {
        self.ask(server, json!({ "event": "item_drop", "player": player_json(player), "item": item, "count": count }))
    }

    fn on_entity_damage(&self, server: &Server, player: &PlayerInfo, mob: i64, kind: &str, damage: f32) -> bool {
        self.ask(server, json!({ "event": "entity_damage", "player": player_json(player), "mob": mob, "kind": kind, "damage": damage }))
    }

    fn on_entity_death(&self, server: &Server, kind: &str, pos: (f32, f32, f32), killer: Option<i64>) {
        self.notify(server, json!({ "event": "entity_death", "kind": kind, "pos": [pos.0, pos.1, pos.2], "killer": killer }));
    }

    fn on_entity_spawn(&self, server: &Server, kind: &str, pos: (f32, f32, f32)) -> bool {
        self.ask(server, json!({ "event": "entity_spawn", "kind": kind, "pos": [pos.0, pos.1, pos.2] }))
    }

    fn on_explosion(&self, server: &Server, pos: (f32, f32, f32), power: f32) -> bool {
        self.ask(server, json!({ "event": "explosion", "pos": [pos.0, pos.1, pos.2], "power": power }))
    }

    fn on_weather_change(&self, server: &Server, weather: &str) -> bool {
        self.ask(server, json!({ "event": "weather_change", "weather": weather }))
    }

    fn on_item_use(&self, server: &Server, player: &PlayerInfo, item: &crate::inventory::Stack) -> bool {
        self.ask(server, json!({ "event": "item_use", "player": player_json(player), "item": crate::items::name_of(item.item.id).unwrap_or(""), "count": item.count }))
    }

    fn on_move(&self, server: &Server, player: &PlayerInfo, to: (f32, f32, f32)) -> bool {
        self.ask(server, json!({ "event": "move", "player": player_json(player), "to": [to.0, to.1, to.2] }))
    }

    fn on_item_pickup(&self, server: &Server, player: &PlayerInfo, item: &str, count: i8) -> bool {
        self.ask(server, json!({ "event": "item_pickup", "player": player_json(player), "item": item, "count": count }))
    }

    fn on_sprint(&self, server: &Server, player: &PlayerInfo, sprinting: bool) {
        self.notify(server, json!({ "event": "sprint", "player": player_json(player), "sprinting": sprinting }));
    }

    fn on_container_open(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32), block: &str) -> bool {
        self.ask(server, json!({ "event": "container_open", "player": player_json(player), "pos": [pos.0, pos.1, pos.2], "block": block }))
    }

    fn on_consume(&self, server: &Server, player: &PlayerInfo, item: &str) -> bool {
        self.ask(server, json!({ "event": "consume", "player": player_json(player), "item": item }))
    }

    fn on_held_change(&self, server: &Server, player: &PlayerInfo, slot: u32, item: Option<&str>) {
        self.notify(server, json!({ "event": "held_change", "player": player_json(player), "slot": slot, "item": item }));
    }

    fn on_dimension_change(&self, server: &Server, player: &PlayerInfo, to: u8) {
        self.notify(server, json!({ "event": "dimension_change", "player": player_json(player), "to": to }));
    }

    fn on_teleport(&self, server: &Server, player: &PlayerInfo, to: (f32, f32, f32)) {
        self.notify(server, json!({ "event": "teleport", "player": player_json(player), "to": [to.0, to.1, to.2] }));
    }

    fn on_xp_change(&self, server: &Server, player: &PlayerInfo, points: &mut i32) {
        let (ok, reply) = self.call(server, json!({ "event": "xp_change", "player": player_json(player), "points": *points }));
        if let Some(v) = reply.and_then(|r| r.trim().parse().ok()) { *points = v; }
        let _ = ok;
    }

    fn on_sign_change(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32), front: &mut String, back: &mut String) -> bool {
        let (ok, reply) = self.call(server, json!({ "event": "sign_change", "player": player_json(player), "pos": [pos.0, pos.1, pos.2], "front": front.as_str(), "back": back.as_str() }));
        if let Some((f, b)) = reply.as_deref().and_then(|r| r.split_once('\n')) {
            // reply "front\nback" rewrites both sides
            *front = f.to_string();
            *back = b.to_string();
        }
        ok
    }

    fn on_bed_enter(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32)) -> bool {
        self.ask(server, json!({ "event": "bed_enter", "player": player_json(player), "pos": [pos.0, pos.1, pos.2] }))
    }

    fn on_bed_leave(&self, server: &Server, player: &PlayerInfo) {
        self.notify(server, json!({ "event": "bed_leave", "player": player_json(player) }));
    }

    fn on_bucket_fill(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32), filled: &str) -> bool {
        self.ask(server, json!({ "event": "bucket_fill", "player": player_json(player), "pos": [pos.0, pos.1, pos.2], "filled": filled }))
    }

    fn on_bucket_empty(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32), bucket: &str) -> bool {
        self.ask(server, json!({ "event": "bucket_empty", "player": player_json(player), "pos": [pos.0, pos.1, pos.2], "bucket": bucket }))
    }

    fn on_fish(&self, server: &Server, player: &PlayerInfo, item: &str) -> bool {
        self.ask(server, json!({ "event": "fish", "player": player_json(player), "item": item }))
    }

    fn on_glide(&self, server: &Server, player: &PlayerInfo, gliding: bool) {
        self.notify(server, json!({ "event": "glide", "player": player_json(player), "gliding": gliding }));
    }

    fn on_swim(&self, server: &Server, player: &PlayerInfo, swimming: bool) {
        self.notify(server, json!({ "event": "swim", "player": player_json(player), "swimming": swimming }));
    }

    fn on_container_close(&self, server: &Server, player: &PlayerInfo, kind: &str) {
        self.notify(server, json!({ "event": "container_close", "player": player_json(player), "kind": kind }));
    }

    fn on_item_break(&self, server: &Server, player: &PlayerInfo, item: &str) {
        self.notify(server, json!({ "event": "item_break", "player": player_json(player), "item": item }));
    }

    fn on_kick(&self, server: &Server, player: &PlayerInfo, message: &str) {
        self.notify(server, json!({ "event": "kick", "player": player_json(player), "message": message }));
    }

    fn on_dismount(&self, server: &Server, player: &PlayerInfo) {
        self.notify(server, json!({ "event": "dismount", "player": player_json(player) }));
    }

    fn on_craft(&self, server: &Server, player: &PlayerInfo, item: &str, count: i32) -> bool {
        self.ask(server, json!({ "event": "craft", "player": player_json(player), "item": item, "count": count }))
    }

    fn on_projectile_hit(&self, server: &Server, shooter: &PlayerInfo, target: crate::plugin::Target) -> bool {
        self.ask(server, json!({ "event": "projectile_hit", "shooter": player_json(shooter), "target": match target { crate::plugin::Target::Mob(i) => json!({"mob": i}), crate::plugin::Target::Player(i) => json!({"player": i}) } }))
    }

    fn on_block_grow(&self, server: &Server, pos: (i32, i32, i32), from: &str, to: &str) -> bool {
        self.ask(server, json!({ "event": "block_grow", "pos": [pos.0, pos.1, pos.2], "from": from, "to": to }))
    }

    fn on_mob_target(&self, server: &Server, mob: i64, kind: &str, player: &PlayerInfo) -> bool {
        self.ask(server, json!({ "event": "mob_target", "mob": mob, "kind": kind, "player": player_json(player) }))
    }

    fn on_block_burn(&self, server: &Server, pos: (i32, i32, i32), block: &str, spread: bool) -> bool {
        self.ask(server, json!({ "event": "block_burn", "pos": [pos.0, pos.1, pos.2], "block": block, "spread": spread }))
    }

    fn on_explosion_blocks(&self, server: &Server, at: (f32, f32, f32), blocks: &mut Vec<(i32, i32, i32)>) {
        let (ok, reply) = self.call(server, json!({ "event": "explosion_blocks", "at": [at.0, at.1, at.2], "blocks": blocks.iter().map(|p| [p.0, p.1, p.2]).collect::<Vec<_>>() }));
        if let Some(v) = reply.and_then(|r| serde_json::from_str::<Vec<(i32, i32, i32)>>(&r).ok()) { *blocks = v; }
        let _ = ok;
    }

    fn on_entity_tame(&self, server: &Server, player: &PlayerInfo, mob: i64, kind: &str) -> bool {
        self.ask(server, json!({ "event": "entity_tame", "player": player_json(player), "mob": mob, "kind": kind }))
    }

    fn on_entity_breed(&self, server: &Server, kind: &str, parents: (i64, i64)) -> bool {
        self.ask(server, json!({ "event": "entity_breed", "kind": kind, "parents": [parents.0, parents.1] }))
    }

    fn on_portal_create(&self, server: &Server, player: &PlayerInfo, kind: &str) -> bool {
        self.ask(server, json!({ "event": "portal_create", "player": player_json(player), "kind": kind }))
    }

    fn on_mob_damage(&self, server: &Server, mob: i64, kind: &str, damage: f32, from: (f32, f32, f32)) -> bool {
        self.ask(server, json!({ "event": "mob_damage", "mob": mob, "kind": kind, "damage": damage, "from": [from.0, from.1, from.2] }))
    }

    fn on_inventory_action(&self, server: &Server, player: &PlayerInfo, action: &str, container: &str) -> bool {
        self.ask(server, json!({ "event": "inventory_action", "player": player_json(player), "action": action, "container": container }))
    }

    fn on_chunk_load(&self, server: &Server, player: &PlayerInfo, chunk: (i32, i32)) {
        self.notify(server, json!({ "event": "chunk_load", "player": player_json(player), "chunk": [chunk.0, chunk.1] }));
    }

    fn on_entity_click(&self, server: &Server, player: &PlayerInfo, entity: i64, attack: bool) {
        self.notify(server, json!({ "event": "entity_click", "player": player_json(player), "entity": entity, "attack": attack }));
    }

    fn on_jump(&self, server: &Server, player: &PlayerInfo) {
        self.notify(server, json!({ "event": "jump", "player": player_json(player) }));
    }

    fn on_toggle_flight(&self, server: &Server, player: &PlayerInfo, flying: bool) {
        self.notify(server, json!({ "event": "toggle_flight", "player": player_json(player), "flying": flying }));
    }

    fn on_animation(&self, server: &Server, player: &PlayerInfo, action: i32) -> bool {
        self.ask(server, json!({ "event": "animation", "player": player_json(player), "action": action }))
    }

    fn on_form_response(&self, server: &Server, player: &PlayerInfo, form_id: u32, answer: Option<&str>) {
        self.notify(server, json!({ "event": "form_response", "player": player_json(player), "form_id": form_id, "answer": answer }));
    }

    fn on_packet_receive(&self, server: &Server, player: &PlayerInfo, packet: &str) -> bool {
        // 20 a second per player; runtime plugins get `move` for those instead.
        if packet == "PlayerAuthInputPacket" {
            return true;
        }
        self.ask(server, json!({ "event": "packet_receive", "player": player_json(player), "packet": packet }))
    }

    fn on_world_save(&self, server: &Server) {
        self.notify(server, json!({ "event": "world_save" }));
    }

    fn on_liquid_flow(&self, server: &Server, pos: (i32, i32, i32), block: &str) -> bool {
        self.ask(server, json!({ "event": "liquid_flow", "pos": [pos.0, pos.1, pos.2], "block": block }))
    }

    fn on_projectile_launch(&self, server: &Server, player: &PlayerInfo, trident: bool, force: f32) {
        self.notify(server, json!({ "event": "projectile_launch", "player": player_json(player), "trident": trident, "force": force }));
    }

    fn on_table_craft(&self, server: &Server, player: &PlayerInfo, table: &str, item: &str, levels: i32) {
        self.notify(server, json!({ "event": "table_craft", "player": player_json(player), "table": table, "item": item, "levels": levels }));
    }

    fn on_brew(&self, server: &Server, pos: (i32, i32, i32)) {
        self.notify(server, json!({ "event": "brew", "pos": [pos.0, pos.1, pos.2] }));
    }

    fn on_entity_combust(&self, server: &Server, mob: i64, kind: &str) {
        self.notify(server, json!({ "event": "entity_combust", "mob": mob, "kind": kind }));
    }

    fn on_entity_despawn(&self, server: &Server, mob: i64, kind: &str) {
        self.notify(server, json!({ "event": "entity_despawn", "mob": mob, "kind": kind }));
    }

    fn on_piston(&self, server: &Server, pos: (i32, i32, i32), extending: bool) {
        self.notify(server, json!({ "event": "piston", "pos": [pos.0, pos.1, pos.2], "extending": extending }));
    }

    fn on_health_regain(&self, server: &Server, player: &PlayerInfo, health: f32) -> bool {
        self.ask(server, json!({ "event": "health_regain", "player": player_json(player), "health": health }))
    }

    fn on_food_change(&self, server: &Server, player: &PlayerInfo, food: i32) {
        self.notify(server, json!({ "event": "food_change", "player": player_json(player), "food": food }));
    }

    fn on_leaves_decay(&self, server: &Server, pos: (i32, i32, i32), block: &str) -> bool {
        self.ask(server, json!({ "event": "leaves_decay", "pos": [pos.0, pos.1, pos.2], "block": block }))
    }

    fn on_block_spread(&self, server: &Server, pos: (i32, i32, i32), from: &str, to: &str) -> bool {
        self.ask(server, json!({ "event": "block_spread", "pos": [pos.0, pos.1, pos.2], "from": from, "to": to }))
    }

    fn on_block_form(&self, server: &Server, pos: (i32, i32, i32), block: &str) -> bool {
        self.ask(server, json!({ "event": "block_form", "pos": [pos.0, pos.1, pos.2], "block": block }))
    }

    fn on_block_fade(&self, server: &Server, pos: (i32, i32, i32), from: &str, to: &str) -> bool {
        self.ask(server, json!({ "event": "block_fade", "pos": [pos.0, pos.1, pos.2], "from": from, "to": to }))
    }

    fn on_block_toggle(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32), block: &str, on: bool) -> bool {
        self.ask(server, json!({ "event": "block_toggle", "player": player_json(player), "pos": [pos.0, pos.1, pos.2], "block": block, "on": on }))
    }

    fn on_vehicle_place(&self, server: &Server, player: &PlayerInfo, kind: &str, pos: (f32, f32, f32)) -> bool {
        self.ask(server, json!({ "event": "vehicle_place", "player": player_json(player), "kind": kind, "pos": [pos.0, pos.1, pos.2] }))
    }

    fn on_vehicle_destroy(&self, server: &Server, kind: &str, pos: (f32, f32, f32), killer: Option<i64>) {
        self.notify(server, json!({ "event": "vehicle_destroy", "kind": kind, "pos": [pos.0, pos.1, pos.2], "killer": killer }));
    }

    fn on_emote(&self, server: &Server, player: &PlayerInfo, emote: &str) -> bool {
        self.ask(server, json!({ "event": "emote", "player": player_json(player), "emote": emote }))
    }

    fn on_skin_change(&self, server: &Server, player: &PlayerInfo, skin: &str) {
        self.notify(server, json!({ "event": "skin_change", "player": player_json(player), "skin": skin }));
    }

    fn on_lightning(&self, server: &Server, pos: (f32, f32, f32)) -> bool {
        self.ask(server, json!({ "event": "lightning", "pos": [pos.0, pos.1, pos.2] }))
    }

    fn on_item_despawn(&self, server: &Server, item: &str, count: i8, pos: (f32, f32, f32)) {
        self.notify(server, json!({ "event": "item_despawn", "item": item, "count": count, "pos": [pos.0, pos.1, pos.2] }));
    }

    fn on_entity_transform(&self, server: &Server, mob: i64, from: &str, to: &str, new_mob: i64) {
        self.notify(server, json!({ "event": "entity_transform", "mob": mob, "from": from, "to": to, "new_mob": new_mob }));
    }

    fn on_mob_shoot(&self, server: &Server, mob: i64, kind: &str) {
        self.notify(server, json!({ "event": "mob_shoot", "mob": mob, "kind": kind }));
    }

    fn on_chunk_unload(&self, server: &Server, player: &PlayerInfo, chunk: (i32, i32)) {
        self.notify(server, json!({ "event": "chunk_unload", "player": player_json(player), "chunk": [chunk.0, chunk.1] }));
    }

    fn on_toggle_crawl(&self, server: &Server, player: &PlayerInfo, crawling: bool) {
        self.notify(server, json!({ "event": "toggle_crawl", "player": player_json(player), "crawling": crawling }));
    }

    fn on_player_spawn(&self, server: &Server, player: &PlayerInfo) {
        self.notify(server, json!({ "event": "player_spawn", "player": player_json(player) }));
    }

    fn on_map_request(&self, server: &Server, player: &PlayerInfo, map: i64) -> bool {
        self.ask(server, json!({ "event": "map_request", "player": player_json(player), "map": map }))
    }

    fn on_block_pick(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32), block: &str) -> bool {
        self.ask(server, json!({ "event": "block_pick", "player": player_json(player), "pos": [pos.0, pos.1, pos.2], "block": block }))
    }

    fn on_lectern_page(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32), page: i32) {
        self.notify(server, json!({ "event": "lectern_page", "player": player_json(player), "pos": [pos.0, pos.1, pos.2], "page": page }));
    }

    fn on_beacon_change(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32), primary: i32, secondary: i32) -> bool {
        self.ask(server, json!({ "event": "beacon_change", "player": player_json(player), "pos": [pos.0, pos.1, pos.2], "primary": primary, "secondary": secondary }))
    }

    fn on_farmland_trample(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32)) -> bool {
        self.ask(server, json!({ "event": "farmland_trample", "player": player_json(player), "pos": [pos.0, pos.1, pos.2] }))
    }

    fn on_crafter_craft(&self, server: &Server, pos: (i32, i32, i32), item: &str, count: i8) -> bool {
        self.ask(server, json!({ "event": "crafter_craft", "pos": [pos.0, pos.1, pos.2], "item": item, "count": count }))
    }

    fn on_dispense(&self, server: &Server, pos: (i32, i32, i32), block: &str) -> bool {
        self.ask(server, json!({ "event": "dispense", "pos": [pos.0, pos.1, pos.2], "block": block }))
    }

    fn on_note_play(&self, server: &Server, pos: (i32, i32, i32)) -> bool {
        self.ask(server, json!({ "event": "note_play", "pos": [pos.0, pos.1, pos.2] }))
    }

    fn on_bell_ring(&self, server: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.notify(server, json!({ "event": "bell_ring", "pos": [pos.0, pos.1, pos.2], "player": player }));
    }

    fn on_raid_start(&self, server: &Server, center: (f32, f32, f32)) -> bool {
        self.ask(server, json!({ "event": "raid_start", "center": [center.0, center.1, center.2] }))
    }

    fn on_raid_wave(&self, server: &Server, center: (f32, f32, f32), wave: i32) {
        self.notify(server, json!({ "event": "raid_wave", "center": [center.0, center.1, center.2], "wave": wave }));
    }

    fn on_raid_end(&self, server: &Server, center: (f32, f32, f32), victory: bool) {
        self.notify(server, json!({ "event": "raid_end", "center": [center.0, center.1, center.2], "victory": victory }));
    }

    fn on_totem_use(&self, server: &Server, player: &PlayerInfo) -> bool {
        self.ask(server, json!({ "event": "totem_use", "player": player_json(player) }))
    }

    fn on_sheep_shear(&self, server: &Server, player: i64, mob: i64) {
        self.notify(server, json!({ "event": "sheep_shear", "player": player, "mob": mob }));
    }

    fn on_sheep_regrow(&self, server: &Server, mob: i64) {
        self.notify(server, json!({ "event": "sheep_regrow", "mob": mob }));
    }

    fn on_leash(&self, server: &Server, player: i64, mob: i64, attached: bool) {
        self.notify(server, json!({ "event": "leash", "player": player, "mob": mob, "attached": attached }));
    }

    fn on_villager_level_up(&self, server: &Server, villager: i64, tier: i32) {
        self.notify(server, json!({ "event": "villager_level_up", "villager": villager, "tier": tier }));
    }

    fn on_spawner_spawn(&self, server: &Server, pos: (i32, i32, i32), kind: &str) -> bool {
        self.ask(server, json!({ "event": "spawner_spawn", "pos": [pos.0, pos.1, pos.2], "kind": kind }))
    }

    fn on_creeper_power(&self, server: &Server, mob: i64) {
        self.notify(server, json!({ "event": "creeper_power", "mob": mob }));
    }

    fn on_rcon_command(&self, server: &Server, addr: &str, line: &str) -> bool {
        self.ask(server, json!({ "event": "rcon_command", "addr": addr, "line": line }))
    }

    fn on_console_command(&self, server: &Server, line: &str) -> bool {
        self.ask(server, json!({ "event": "console_command", "line": line }))
    }

    fn on_furnace_smelt(&self, server: &Server, pos: (i32, i32, i32)) {
        self.notify(server, json!({ "event": "furnace_smelt", "pos": [pos.0, pos.1, pos.2] }));
    }

    fn on_tnt_prime(&self, server: &Server, pos: (i32, i32, i32)) -> bool {
        self.ask(server, json!({ "event": "tnt_prime", "pos": [pos.0, pos.1, pos.2] }))
    }

    fn on_effect_add(&self, server: &Server, player: &PlayerInfo, effect: i32, amplifier: i32, ticks: i32) -> bool {
        self.ask(server, json!({ "event": "effect_add", "player": player_json(player), "effect": effect, "amplifier": amplifier, "ticks": ticks }))
    }

    fn on_effect_remove(&self, server: &Server, player: &PlayerInfo, effect: i32) {
        self.notify(server, json!({ "event": "effect_remove", "player": player_json(player), "effect": effect }));
    }

    fn on_ender_pearl(&self, server: &Server, player: &PlayerInfo, to: (f32, f32, f32)) -> bool {
        self.ask(server, json!({ "event": "ender_pearl", "player": player_json(player), "to": [to.0, to.1, to.2] }))
    }

    fn on_item_spawn(&self, server: &Server, item: &str, count: i8, pos: (f32, f32, f32)) -> bool {
        self.ask(server, json!({ "event": "item_spawn", "item": item, "count": count, "pos": [pos.0, pos.1, pos.2] }))
    }

    fn on_xp_orb_pickup(&self, server: &Server, player: &PlayerInfo, value: i32) {
        self.notify(server, json!({ "event": "xp_orb_pickup", "player": player_json(player), "value": value }));
    }

    fn on_golem_build(&self, server: &Server, kind: &str, pos: (f32, f32, f32)) -> bool {
        self.ask(server, json!({ "event": "golem_build", "kind": kind, "pos": [pos.0, pos.1, pos.2] }))
    }

    fn on_trader_spawn(&self, server: &Server, pos: (f32, f32, f32)) -> bool {
        self.ask(server, json!({ "event": "trader_spawn", "pos": [pos.0, pos.1, pos.2] }))
    }

    fn on_patrol_spawn(&self, server: &Server, pos: (f32, f32, f32)) -> bool {
        self.ask(server, json!({ "event": "patrol_spawn", "pos": [pos.0, pos.1, pos.2] }))
    }

    fn on_shriek(&self, server: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.notify(server, json!({ "event": "shriek", "pos": [pos.0, pos.1, pos.2], "player": player }));
    }

    fn on_warden_summon(&self, server: &Server, pos: (f32, f32, f32)) -> bool {
        self.ask(server, json!({ "event": "warden_summon", "pos": [pos.0, pos.1, pos.2] }))
    }

    fn on_vault_open(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32)) -> bool {
        self.ask(server, json!({ "event": "vault_open", "player": player_json(player), "pos": [pos.0, pos.1, pos.2] }))
    }

    fn on_trial_spawn(&self, server: &Server, pos: (i32, i32, i32), kind: &str, mob: i64) {
        self.notify(server, json!({ "event": "trial_spawn", "pos": [pos.0, pos.1, pos.2], "kind": kind, "mob": mob }));
    }

    fn on_gamerule_change(&self, server: &Server, rule: &str, value: &str) -> bool {
        self.ask(server, json!({ "event": "gamerule_change", "rule": rule, "value": value }))
    }

    fn on_difficulty_change(&self, server: &Server, difficulty: i32) -> bool {
        self.ask(server, json!({ "event": "difficulty_change", "difficulty": difficulty }))
    }

    fn on_op_change(&self, server: &Server, name: &str, op: bool) -> bool {
        self.ask(server, json!({ "event": "op_change", "name": name, "op": op }))
    }

    fn on_ban(&self, server: &Server, name: &str, reason: &str) -> bool {
        self.ask(server, json!({ "event": "ban", "name": name, "reason": reason }))
    }

    fn on_unban(&self, server: &Server, name: &str) -> bool {
        self.ask(server, json!({ "event": "unban", "name": name }))
    }

    fn on_ban_ip(&self, server: &Server, ip: &str) -> bool {
        self.ask(server, json!({ "event": "ban_ip", "ip": ip }))
    }

    fn on_whitelist_change(&self, server: &Server, name: &str, added: bool) -> bool {
        self.ask(server, json!({ "event": "whitelist_change", "name": name, "added": added }))
    }

    fn on_world_change(&self, server: &Server, player: &PlayerInfo, world: &str) -> bool {
        self.ask(server, json!({ "event": "world_change", "player": player_json(player), "world": world }))
    }

    fn on_backup(&self, server: &Server, path: &str) {
        self.notify(server, json!({ "event": "backup", "path": path }));
    }

    fn on_dragon_perch(&self, server: &Server, mob: i64) {
        self.notify(server, json!({ "event": "dragon_perch", "mob": mob }));
    }

    fn on_night_skip(&self, server: &Server) -> bool {
        self.ask(server, json!({ "event": "night_skip" }))
    }

    fn on_bonemeal(&self, server: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.notify(server, json!({ "event": "bonemeal", "pos": [pos.0, pos.1, pos.2], "player": player }));
    }

    fn on_anchor_charge(&self, server: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.notify(server, json!({ "event": "anchor_charge", "pos": [pos.0, pos.1, pos.2], "player": player }));
    }

    fn on_xp_level_change(&self, server: &Server, player: &PlayerInfo, old: i32, new: i32) {
        self.notify(server, json!({ "event": "xp_level_change", "player": player_json(player), "old": old, "new": new }));
    }

    fn on_shield_block(&self, server: &Server, player: &PlayerInfo, damage: f32) -> bool {
        self.ask(server, json!({ "event": "shield_block", "player": player_json(player), "damage": damage }))
    }

    fn on_cake_eat(&self, server: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.notify(server, json!({ "event": "cake_eat", "pos": [pos.0, pos.1, pos.2], "player": player }));
    }

    fn on_dragon_egg_teleport(&self, server: &Server, pos: (i32, i32, i32), to: (i32, i32, i32)) -> bool {
        self.ask(server, json!({ "event": "dragon_egg_teleport", "pos": [pos.0, pos.1, pos.2], "to": [to.0, to.1, to.2] }))
    }

    fn on_end_portal_fill(&self, server: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.notify(server, json!({ "event": "end_portal_fill", "pos": [pos.0, pos.1, pos.2], "player": player }));
    }

    fn on_composter_use(&self, server: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.notify(server, json!({ "event": "composter_use", "pos": [pos.0, pos.1, pos.2], "player": player }));
    }

    fn on_mob_heal(&self, server: &Server, mob: i64, amount: f32) {
        self.notify(server, json!({ "event": "mob_heal", "mob": mob, "amount": amount }));
    }

    fn on_spawn_point_set(&self, server: &Server, player: &PlayerInfo, pos: (f32, f32, f32)) -> bool {
        self.ask(server, json!({ "event": "spawn_point_set", "player": player_json(player), "pos": [pos.0, pos.1, pos.2] }))
    }

    fn on_view_distance(&self, server: &Server, player: &PlayerInfo, chunks: &mut i32) {
        let (ok, reply) = self.call(server, json!({ "event": "view_distance", "player": player_json(player), "chunks": *chunks }));
        if let Some(v) = reply.and_then(|r| r.trim().parse().ok()) { *chunks = v; }
        let _ = ok;
    }

    fn on_chunk_generate(&self, server: &Server, chunk: (i32, i32)) {
        self.notify(server, json!({ "event": "chunk_generate", "chunk": [chunk.0, chunk.1] }));
    }

    fn on_time_set(&self, server: &Server, time: i32) -> bool {
        self.ask(server, json!({ "event": "time_set", "time": time }))
    }

    fn on_sit_toggle(&self, server: &Server, mob: i64, sitting: bool) {
        self.notify(server, json!({ "event": "sit_toggle", "mob": mob, "sitting": sitting }));
    }

    fn on_slow_tick(&self, server: &Server, ms: f32) {
        self.notify(server, json!({ "event": "slow_tick", "ms": ms }));
    }

    fn on_armor_change(&self, server: &Server, player: &PlayerInfo, slot: i32, item: &str) {
        self.notify(server, json!({ "event": "armor_change", "player": player_json(player), "slot": slot, "item": item }));
    }

    fn on_player_combust(&self, server: &Server, player: &PlayerInfo) {
        self.notify(server, json!({ "event": "player_combust", "player": player_json(player) }));
    }

    fn on_death_drops(&self, server: &Server, player: &PlayerInfo) -> bool {
        self.ask(server, json!({ "event": "death_drops", "player": player_json(player) }))
    }

    fn on_portal_enter(&self, server: &Server, player: &PlayerInfo, to: i32) -> bool {
        self.ask(server, json!({ "event": "portal_enter", "player": player_json(player), "to": to }))
    }

    fn on_firework_explode(&self, server: &Server, pos: (f32, f32, f32), stars: i32) {
        self.notify(server, json!({ "event": "firework_explode", "pos": [pos.0, pos.1, pos.2], "stars": stars }));
    }

    fn on_tnt_minecart_prime(&self, server: &Server, mob: i64) {
        self.notify(server, json!({ "event": "tnt_minecart_prime", "mob": mob }));
    }

    fn on_fishing_cast(&self, server: &Server, player: &PlayerInfo) -> bool {
        self.ask(server, json!({ "event": "fishing_cast", "player": player_json(player) }))
    }

    fn on_elytra_boost(&self, server: &Server, player: &PlayerInfo) -> bool {
        self.ask(server, json!({ "event": "elytra_boost", "player": player_json(player) }))
    }

    fn on_item_throw(&self, server: &Server, player: &PlayerInfo, item: &str) -> bool {
        self.ask(server, json!({ "event": "item_throw", "player": player_json(player), "item": item }))
    }

    fn on_block_explode(&self, server: &Server, pos: (i32, i32, i32), block: &str) -> bool {
        self.ask(server, json!({ "event": "block_explode", "pos": [pos.0, pos.1, pos.2], "block": block }))
    }

    fn on_sign_open(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32)) {
        self.notify(server, json!({ "event": "sign_open", "player": player_json(player), "pos": [pos.0, pos.1, pos.2] }));
    }

    fn on_mob_expire(&self, server: &Server, mob: i64, kind: &str) {
        self.notify(server, json!({ "event": "mob_expire", "mob": mob, "kind": kind }));
    }

    fn on_sapling_grow(&self, server: &Server, pos: (i32, i32, i32), sapling: &str) -> bool {
        self.ask(server, json!({ "event": "sapling_grow", "pos": [pos.0, pos.1, pos.2], "sapling": sapling }))
    }

    fn on_block_ignite(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32)) -> bool {
        self.ask(server, json!({ "event": "block_ignite", "player": player_json(player), "pos": [pos.0, pos.1, pos.2] }))
    }

    fn on_button_press(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32), block: &str) -> bool {
        self.ask(server, json!({ "event": "button_press", "player": player_json(player), "pos": [pos.0, pos.1, pos.2], "block": block }))
    }

    fn on_pressure_plate(&self, server: &Server, pos: (i32, i32, i32), signal: i32) {
        self.notify(server, json!({ "event": "pressure_plate", "pos": [pos.0, pos.1, pos.2], "signal": signal }));
    }

    fn on_tripwire_trigger(&self, server: &Server, pos: (i32, i32, i32)) {
        self.notify(server, json!({ "event": "tripwire_trigger", "pos": [pos.0, pos.1, pos.2] }));
    }

    fn on_target_hit(&self, server: &Server, pos: (i32, i32, i32), signal: i32) {
        self.notify(server, json!({ "event": "target_hit", "pos": [pos.0, pos.1, pos.2], "signal": signal }));
    }

    fn on_sculk_sensor(&self, server: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.notify(server, json!({ "event": "sculk_sensor", "pos": [pos.0, pos.1, pos.2], "player": player }));
    }

    fn on_log_strip(&self, server: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.notify(server, json!({ "event": "log_strip", "pos": [pos.0, pos.1, pos.2], "player": player }));
    }

    fn on_copper_wax(&self, server: &Server, pos: (i32, i32, i32), player: Option<i64>, waxed: bool) {
        self.notify(server, json!({ "event": "copper_wax", "pos": [pos.0, pos.1, pos.2], "player": player, "waxed": waxed }));
    }

    fn on_copper_scrape(&self, server: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.notify(server, json!({ "event": "copper_scrape", "pos": [pos.0, pos.1, pos.2], "player": player }));
    }

    fn on_path_create(&self, server: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.notify(server, json!({ "event": "path_create", "pos": [pos.0, pos.1, pos.2], "player": player }));
    }

    fn on_till(&self, server: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.notify(server, json!({ "event": "till", "pos": [pos.0, pos.1, pos.2], "player": player }));
    }

    fn on_campfire_extinguish(&self, server: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.notify(server, json!({ "event": "campfire_extinguish", "pos": [pos.0, pos.1, pos.2], "player": player }));
    }

    fn on_villager_restock(&self, server: &Server, villager: i64) {
        self.notify(server, json!({ "event": "villager_restock", "villager": villager }));
    }

    fn on_pvp_attack(&self, server: &Server, attacker: &PlayerInfo, victim: &PlayerInfo, damage: f32) -> bool {
        self.ask(server, json!({ "event": "pvp_attack", "attacker": player_json(attacker), "victim": player_json(victim), "damage": damage }))
    }

    fn on_mending_repair(&self, server: &Server, player: &PlayerInfo, points: i32) {
        self.notify(server, json!({ "event": "mending_repair", "player": player_json(player), "points": points }));
    }

    fn on_idle_kick(&self, server: &Server, player: &PlayerInfo) -> bool {
        self.ask(server, json!({ "event": "idle_kick", "player": player_json(player) }))
    }

    fn on_spawn_protection(&self, server: &Server, name: &str, pos: (i32, i32, i32)) -> bool {
        self.ask(server, json!({ "event": "spawn_protection", "name": name, "pos": [pos.0, pos.1, pos.2] }))
    }

    fn on_hopper_move(&self, server: &Server, from: (i32, i32, i32), to: (i32, i32, i32)) {
        self.notify(server, json!({ "event": "hopper_move", "from": [from.0, from.1, from.2], "to": [to.0, to.1, to.2] }));
    }

    fn on_jukebox_play(&self, server: &Server, pos: (i32, i32, i32), record: &str, player: Option<i64>) {
        self.notify(server, json!({ "event": "jukebox_play", "pos": [pos.0, pos.1, pos.2], "record": record, "player": player }));
    }

    fn on_note_tune(&self, server: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.notify(server, json!({ "event": "note_tune", "pos": [pos.0, pos.1, pos.2], "player": player }));
    }

    fn on_honey_harvest(&self, server: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.notify(server, json!({ "event": "honey_harvest", "pos": [pos.0, pos.1, pos.2], "player": player }));
    }

    fn on_pumpkin_carve(&self, server: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.notify(server, json!({ "event": "pumpkin_carve", "pos": [pos.0, pos.1, pos.2], "player": player }));
    }

    fn on_berry_pick(&self, server: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.notify(server, json!({ "event": "berry_pick", "pos": [pos.0, pos.1, pos.2], "player": player }));
    }

    fn on_spawn_egg_use(&self, server: &Server, player: &PlayerInfo, kind: &str, pos: (f32, f32, f32)) -> bool {
        self.ask(server, json!({ "event": "spawn_egg_use", "player": player_json(player), "kind": kind, "pos": [pos.0, pos.1, pos.2] }))
    }

    fn on_mob_feed(&self, server: &Server, player: i64, mob: i64) {
        self.notify(server, json!({ "event": "mob_feed", "player": player, "mob": mob }));
    }

    fn on_baby_feed(&self, server: &Server, player: i64, mob: i64) {
        self.notify(server, json!({ "event": "baby_feed", "player": player, "mob": mob }));
    }

    fn on_mob_saddle(&self, server: &Server, player: i64, mob: i64) {
        self.notify(server, json!({ "event": "mob_saddle", "player": player, "mob": mob }));
    }

    fn on_cow_milk(&self, server: &Server, player: &PlayerInfo, mob: i64) -> bool {
        self.ask(server, json!({ "event": "cow_milk", "player": player_json(player), "mob": mob }))
    }

    fn on_creeper_ignite(&self, server: &Server, player: i64, mob: i64) {
        self.notify(server, json!({ "event": "creeper_ignite", "player": player, "mob": mob }));
    }

    fn on_cure_start(&self, server: &Server, player: i64, mob: i64) {
        self.notify(server, json!({ "event": "cure_start", "player": player, "mob": mob }));
    }

    fn on_map_create(&self, server: &Server, player: &PlayerInfo) -> bool {
        self.ask(server, json!({ "event": "map_create", "player": player_json(player) }))
    }

    fn on_dialog_open(&self, server: &Server, player: &PlayerInfo, npc: &str) -> bool {
        self.ask(server, json!({ "event": "dialog_open", "player": player_json(player), "npc": npc }))
    }

    fn on_dialog_answer(&self, server: &Server, player: &PlayerInfo, scene: &str, button: Option<usize>) {
        self.notify(server, json!({ "event": "dialog_answer", "player": player_json(player), "scene": scene, "button": button }));
    }

    fn on_player_kill(&self, server: &Server, killer: &PlayerInfo, victim: &PlayerInfo) {
        self.notify(server, json!({ "event": "player_kill", "killer": player_json(killer), "victim": player_json(victim) }));
    }

    fn on_say_command(&self, server: &Server, sender: &str, args: &str) -> bool {
        self.ask(server, json!({ "event": "say_command", "sender": sender, "args": args }))
    }

    fn on_me_command(&self, server: &Server, sender: &str, args: &str) -> bool {
        self.ask(server, json!({ "event": "me_command", "sender": sender, "args": args }))
    }

    fn on_whisper_command(&self, server: &Server, sender: &str, args: &str) -> bool {
        self.ask(server, json!({ "event": "whisper_command", "sender": sender, "args": args }))
    }

    fn on_give_command(&self, server: &Server, sender: &str, args: &str) -> bool {
        self.ask(server, json!({ "event": "give_command", "sender": sender, "args": args }))
    }

    fn on_summon_command(&self, server: &Server, sender: &str, args: &str) -> bool {
        self.ask(server, json!({ "event": "summon_command", "sender": sender, "args": args }))
    }

    fn on_setblock_command(&self, server: &Server, sender: &str, args: &str) -> bool {
        self.ask(server, json!({ "event": "setblock_command", "sender": sender, "args": args }))
    }

    fn on_fill_command(&self, server: &Server, sender: &str, args: &str) -> bool {
        self.ask(server, json!({ "event": "fill_command", "sender": sender, "args": args }))
    }

    fn on_kill_command(&self, server: &Server, sender: &str, args: &str) -> bool {
        self.ask(server, json!({ "event": "kill_command", "sender": sender, "args": args }))
    }

    fn on_enchant_command(&self, server: &Server, sender: &str, args: &str) -> bool {
        self.ask(server, json!({ "event": "enchant_command", "sender": sender, "args": args }))
    }

    fn on_clear_command(&self, server: &Server, sender: &str, args: &str) -> bool {
        self.ask(server, json!({ "event": "clear_command", "sender": sender, "args": args }))
    }

    fn on_setworldspawn_command(&self, server: &Server, sender: &str, args: &str) -> bool {
        self.ask(server, json!({ "event": "setworldspawn_command", "sender": sender, "args": args }))
    }

    fn on_title_command(&self, server: &Server, sender: &str, args: &str) -> bool {
        self.ask(server, json!({ "event": "title_command", "sender": sender, "args": args }))
    }

    fn on_tellraw_command(&self, server: &Server, sender: &str, args: &str) -> bool {
        self.ask(server, json!({ "event": "tellraw_command", "sender": sender, "args": args }))
    }

    fn on_scoreboard_command(&self, server: &Server, sender: &str, args: &str) -> bool {
        self.ask(server, json!({ "event": "scoreboard_command", "sender": sender, "args": args }))
    }

    fn on_effect_command(&self, server: &Server, sender: &str, args: &str) -> bool {
        self.ask(server, json!({ "event": "effect_command", "sender": sender, "args": args }))
    }

    fn on_damage_modify(&self, server: &Server, player: &PlayerInfo, cause: &str, amount: &mut f32) {
        let (_, reply) = self.call(server, json!({ "event": "damage_modify", "player": player_json(player), "cause": cause, "amount": *amount }));
        if let Some(v) = reply.and_then(|r| r.trim().parse::<f32>().ok()).filter(|v| v.is_finite()) {
            *amount = v;
        }
    }

    fn on_mob_damage_modify(&self, server: &Server, mob: i64, kind: &str, attacker: Option<i64>, amount: &mut f32) {
        let (_, reply) = self.call(server, json!({ "event": "mob_damage_modify", "mob": mob, "kind": kind, "attacker": attacker, "amount": *amount }));
        if let Some(v) = reply.and_then(|r| r.trim().parse::<f32>().ok()).filter(|v| v.is_finite()) {
            *amount = v;
        }
    }

    fn on_block_drops(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32), block: &str, drops: &mut Vec<(String, i8)>) {
        let (_, reply) = self.call(server, json!({ "event": "block_drops", "player": player_json(player), "pos": [pos.0, pos.1, pos.2], "block": block, "drops": drops }));
        if let Some(v) = reply.and_then(|r| serde_json::from_str::<Vec<(String, i8)>>(&r).ok()) {
            *drops = v;
        }
    }

    fn on_mob_drops(&self, server: &Server, kind: &str, pos: (f32, f32, f32), killer: Option<i64>, drops: &mut Vec<(String, i8)>) {
        let (_, reply) = self.call(server, json!({ "event": "mob_drops", "kind": kind, "pos": [pos.0, pos.1, pos.2], "killer": killer, "drops": drops }));
        if let Some(v) = reply.and_then(|r| serde_json::from_str::<Vec<(String, i8)>>(&r).ok()) {
            *drops = v;
        }
    }

    fn on_block_xp(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32), block: &str, xp: &mut i32) {
        let (ok, reply) = self.call(server, json!({ "event": "block_xp", "player": player_json(player), "pos": [pos.0, pos.1, pos.2], "block": block, "xp": *xp }));
        if let Some(v) = reply.and_then(|r| r.trim().parse().ok()) { *xp = v; }
        let _ = ok;
    }

    fn on_mob_xp(&self, server: &Server, kind: &str, pos: (f32, f32, f32), killer: Option<i64>, xp: &mut i32) {
        let (ok, reply) = self.call(server, json!({ "event": "mob_xp", "kind": kind, "pos": [pos.0, pos.1, pos.2], "killer": killer, "xp": *xp }));
        if let Some(v) = reply.and_then(|r| r.trim().parse().ok()) { *xp = v; }
        let _ = ok;
    }

    fn on_knockback(&self, server: &Server, player: &PlayerInfo, knock: &mut (f32, f32, f32)) {
        let (_, reply) = self.call(server, json!({ "event": "knockback", "player": player_json(player), "knock": [knock.0, knock.1, knock.2] }));
        if let Some(v) = reply.and_then(|r| serde_json::from_str::<(f32, f32, f32)>(&r).ok()).filter(|v| v.0.is_finite() && v.1.is_finite() && v.2.is_finite()) {
            *knock = v;
        }
    }

    fn on_packet_send(&self, server: &Server, player: &PlayerInfo, packet: &str) -> bool {
        // The busy ones (chunks, movement, block and entity updates) don't go to runtime plugins:
        // a sandbox call per packet would cost more than the packet.
        if matches!(packet, "LevelChunkPacket" | "MoveActorAbsolutePacket" | "MoveActorDeltaPacket" | "SetActorMotionPacket" | "UpdateBlockPacket" | "SetActorDataPacket" | "NetworkChunkPublisherUpdatePacket" | "MovePlayerPacket" | "UpdateAttributesPacket" | "LevelSoundEventPacket" | "AnimatePacket") {
            return true;
        }
        self.ask(server, json!({ "event": "packet_send", "player": player_json(player), "packet": packet }))
    }

    fn on_permission_check(&self, server: &Server, player: &str, node: &str, allowed: &mut bool) {
        let (_, reply) = self.call(server, json!({ "event": "permission_check", "player": player, "node": node, "allowed": *allowed }));
        match reply.as_deref().map(str::trim) {
            Some("true") => *allowed = true,
            Some("false") => *allowed = false,
            _ => {}
        }
    }

    fn on_plugin_enable(&self, server: &Server, name: &str) {
        self.notify(server, json!({ "event": "plugin_enable", "name": name }));
    }

    fn on_book_edit(&self, server: &Server, player: &PlayerInfo, page: i32, text: &str) -> bool {
        self.ask(server, json!({ "event": "book_edit", "player": player_json(player), "page": page, "text": text }))
    }

    fn on_book_sign(&self, server: &Server, player: &PlayerInfo, title: &str) -> bool {
        self.ask(server, json!({ "event": "book_sign", "player": player_json(player), "title": title }))
    }

    fn on_frame_place(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32), item: &str) -> bool {
        self.ask(server, json!({ "event": "frame_place", "player": player_json(player), "pos": [pos.0, pos.1, pos.2], "item": item }))
    }

    fn on_frame_rotate(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32), rotation: i32) -> bool {
        self.ask(server, json!({ "event": "frame_rotate", "player": player_json(player), "pos": [pos.0, pos.1, pos.2], "rotation": rotation }))
    }

    fn on_frame_take(&self, server: &Server, player: &PlayerInfo, pos: (i32, i32, i32), item: &str) -> bool {
        self.ask(server, json!({ "event": "frame_take", "player": player_json(player), "pos": [pos.0, pos.1, pos.2], "item": item }))
    }

    fn on_armor_stand_place(&self, server: &Server, player: &PlayerInfo, pos: (f32, f32, f32)) -> bool {
        self.ask(server, json!({ "event": "armor_stand_place", "player": player_json(player), "pos": [pos.0, pos.1, pos.2] }))
    }

    fn on_armor_stand_change(&self, server: &Server, player: &PlayerInfo, mob: i64, item: &str) -> bool {
        self.ask(server, json!({ "event": "armor_stand_change", "player": player_json(player), "mob": mob, "item": item }))
    }

    fn on_armor_stand_break(&self, server: &Server, mob: i64, pos: (f32, f32, f32), killer: Option<i64>) {
        self.notify(server, json!({ "event": "armor_stand_break", "mob": mob, "pos": [pos.0, pos.1, pos.2], "killer": killer }));
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
    fn queries_answer_and_data_saves() {
        crate::blocks::init();
        let name = format!("rusted_query_test_{}", std::process::id());
        let h = Host { name: name.clone(), server: Some(Server::new()), ..Host::default() };
        assert!(answer(&h, "time").parse::<i32>().is_ok());
        assert_eq!(answer(&h, "players"), "[]");
        assert!(!answer(&h, "block 0 -64 0").is_empty(), "a block name");
        assert_eq!(answer(&h, "player Nobody"), "");
        assert_eq!(answer(&h, "set coins 12 and more"), "ok");
        assert_eq!(answer(&h, "get coins"), "12 and more");
        answer(&h, "set coins");
        assert_eq!(answer(&h, "get coins"), "", "an empty value deletes it");
        assert_eq!(answer(&h, "nonsense"), "");
        let _ = std::fs::remove_file(data_path(&name));
    }

    /// The example plugin, built for real (examples/wasm-hello), uses the new host calls.
    /// Skipped when it hasn't been built.
    #[test]
    fn example_plugin_gives_a_kit_once() {
        let Ok(bytes) = std::fs::read("examples/wasm-hello/target/wasm32-unknown-unknown/release/hello.wasm") else { return };
        crate::blocks::init();
        let name = format!("hello_test_{}", std::process::id());
        let p = load(&name, &bytes).expect("the example loads");
        assert!(p.commands.iter().any(|(c, _)| *c == "kit"));
        let server = Server::new();
        let me = crate::players::tests::sample(1);
        assert_eq!(p.on_command(&server, &me, "kit", &[]).as_deref(), Some("§aEnjoy your kit!"));
        assert_eq!(p.on_command(&server, &me, "kit", &[]).as_deref(), Some("§cYou already took your kit."), "saved data remembers");
        let _ = std::fs::remove_file(data_path(&name));
    }

    #[test]
    fn loads_a_plugin_and_it_can_cancel() {
        let p = load("deny", DENY_ALL).expect("loads");
        assert!(p.commands.is_empty());
        let server = Server::new();
        assert!(!p.ask(&server, json!({ "event": "block_break" })), "the plugin said no");
        // every event reaches runtime plugins, not just the first few
        let me = crate::players::tests::sample(1);
        assert!(!p.on_craft(&server, &me, "minecraft:stick", 4));
        assert!(!p.on_block_grow(&server, (0, 64, 0), "minecraft:wheat", "minecraft:wheat"));
        let mut xp = 5;
        p.on_xp_change(&server, &me, &mut xp);
        assert_eq!(xp, 5, "no reply leaves it alone");
        assert!(load("junk", b"not wasm").is_err());
    }
}

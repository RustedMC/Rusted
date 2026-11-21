// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! Forms, scoreboards and boss bars (from PNX form, scoreboard and DummyBossBar).
#![allow(dead_code)] // plugin API: plugins call these, the server itself may not

use std::collections::HashMap;
use parking_lot::Mutex;

use serde_json::{Value, json};

use crate::Protocol;
use crate::players::{Event, PlayerInfo, Server};

// ---- forms

#[derive(Clone, Debug)]
pub enum Element {
    Label(String),
    Input { text: String, placeholder: String, default: String },
    Toggle { text: String, default: bool },
    Slider { text: String, min: f32, max: f32, step: f32, default: f32 },
    Dropdown { text: String, options: Vec<String>, default: usize },
    StepSlider { text: String, steps: Vec<String>, default: usize },
}

/// PNX FormWindowSimple / Modal / Custom.
#[derive(Clone, Debug)]
pub enum Form {
    /// Buttons with text and an optional image (texture path like "textures/items/diamond", or a URL).
    Simple { title: String, content: String, buttons: Vec<(String, Option<String>)> },
    /// yes / no
    Modal { title: String, content: String, yes: String, no: String },
    Custom { title: String, elements: Vec<Element> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Response {
    /// index of the pressed button
    Button(usize),
    /// true for the first button
    Choice(bool),
    /// one value per element in order, labels give null
    Values(Vec<Value>),
    /// closed without answering
    Closed,
}

impl Form {
    pub fn json(&self) -> String {
        match self {
            Form::Simple { title, content, buttons } => json!({
                "type": "form", "title": title, "content": content,
                "buttons": buttons.iter().map(|(text, image)| match image {
                    Some(i) => json!({"text": text, "image": {"type": if i.starts_with("http") { "url" } else { "path" }, "data": i}}),
                    None => json!({"text": text}),
                }).collect::<Vec<_>>(),
            }),
            Form::Modal { title, content, yes, no } => json!({"type": "modal", "title": title, "content": content, "button1": yes, "button2": no}),
            Form::Custom { title, elements } => json!({
                "type": "custom_form", "title": title,
                "content": elements.iter().map(|e| match e {
                    Element::Label(t) => json!({"type": "label", "text": t}),
                    Element::Input { text, placeholder, default } => json!({"type": "input", "text": text, "placeholder": placeholder, "default": default}),
                    Element::Toggle { text, default } => json!({"type": "toggle", "text": text, "default": default}),
                    Element::Slider { text, min, max, step, default } => json!({"type": "slider", "text": text, "min": min, "max": max, "step": step, "default": default}),
                    Element::Dropdown { text, options, default } => json!({"type": "dropdown", "text": text, "options": options, "default": default}),
                    Element::StepSlider { text, steps, default } => json!({"type": "step_slider", "text": text, "steps": steps, "default": default}),
                }).collect::<Vec<_>>(),
            }),
        }
        .to_string()
    }

    /// `None` json means it was closed.
    pub fn parse(&self, answer: Option<&str>) -> Response {
        let Some(v) = answer.and_then(|a| serde_json::from_str::<Value>(a.trim()).ok()) else { return Response::Closed };
        match (self, v) {
            (_, Value::Null) => Response::Closed,
            (Form::Simple { buttons, .. }, Value::Number(n)) => match n.as_u64() {
                Some(i) if (i as usize) < buttons.len() => Response::Button(i as usize),
                _ => Response::Closed,
            },
            (Form::Modal { .. }, Value::Bool(b)) => Response::Choice(b),
            (Form::Custom { .. }, Value::Array(a)) => Response::Values(a),
            _ => Response::Closed,
        }
    }
}

type Callback = Box<dyn FnOnce(&Server, &PlayerInfo, Response) + Send>;

static FORMS: Mutex<Option<HashMap<(i64, u32), (Form, Callback)>>> = Mutex::new(None);
static NEXT_FORM: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

/// Shows `form` to `to`, `then` gets their answer.
pub fn show(server: &Server, to: i64, form: Form, then: impl FnOnce(&Server, &PlayerInfo, Response) + Send + 'static) {
    let id = NEXT_FORM.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let packet = Protocol::ModalFormRequestPacket(Box::new(bedrock::protocol::v662::packets::ModalFormRequestPacket { form_id: id, form_ui_json: form.json() }));
    FORMS.lock().get_or_insert_with(HashMap::new).insert((to, id), (form, Box::new(then)));
    let _ = server.events.send(Event::Packet { to, packet: Box::new(packet) });
}

pub fn answered(server: &Server, player: i64, form_id: u32, answer: Option<&str>) {
    let Some((form, then)) = FORMS.lock().as_mut().and_then(|f| f.remove(&(player, form_id))) else { return };
    if let Some(p) = server.get(player) {
        then(server, &p, form.parse(answer));
    }
}

/// Drop open forms when a player leaves.
pub fn left(player: i64) {
    if let Some(f) = FORMS.lock().as_mut() {
        f.retain(|(p, _), _| *p != player);
    }
}

// ---- scoreboards

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Objective {
    pub display: String,
    pub criteria: String,
    pub scores: HashMap<String, i32>,
}

#[derive(Default)]
struct Board {
    objectives: HashMap<String, Objective>,
    /// slot ("sidebar", "list", "belowname") -> (objective, descending)
    slots: HashMap<String, (String, bool)>,
}

static BOARD: Mutex<Option<Board>> = Mutex::new(None);

fn board_path() -> std::path::PathBuf {
    std::path::Path::new("worlds").join(&crate::config::get().level_name).join("scoreboard.txt")
}

fn with_board<T>(f: impl FnOnce(&mut Board) -> T) -> T {
    let mut b = BOARD.lock();
    let b = b.get_or_insert_with(|| load(&std::fs::read_to_string(board_path()).unwrap_or_default()));
    f(b)
}

/// File format: `o <name>\t<criteria>\t<display>`, `s <objective>\t<player>\t<score>`, `d <slot>\t<objective>\t<desc>`.
fn load(text: &str) -> Board {
    let mut b = Board::default();
    for line in text.lines() {
        let (kind, rest) = line.split_at(line.len().min(2));
        let f: Vec<&str> = rest.split('\t').collect();
        match (kind, f.as_slice()) {
            ("o ", [name, criteria, display]) => {
                b.objectives.insert(name.to_string(), Objective { display: display.to_string(), criteria: criteria.to_string(), scores: HashMap::new() });
            }
            ("s ", [obj, who, score]) => {
                if let (Some(o), Ok(v)) = (b.objectives.get_mut(*obj), score.parse()) {
                    o.scores.insert(who.to_string(), v);
                }
            }
            ("d ", [slot, obj, desc]) => {
                b.slots.insert(slot.to_string(), (obj.to_string(), *desc == "1"));
            }
            _ => {}
        }
    }
    b
}

fn save(b: &Board) {
    let clean = |s: &str| s.replace(['\t', '\n'], " ");
    let mut out = String::new();
    for (name, o) in &b.objectives {
        out += &format!("o {}\t{}\t{}\n", clean(name), clean(&o.criteria), clean(&o.display));
        for (who, v) in &o.scores {
            out += &format!("s {}\t{}\t{v}\n", clean(name), clean(who));
        }
    }
    for (slot, (obj, desc)) in &b.slots {
        out += &format!("d {slot}\t{}\t{}\n", clean(obj), *desc as u8);
    }
    let path = board_path();
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(path, out);
}

/// Stable line id per (objective, holder).
fn line_id(obj: &str, who: &str) -> i64 {
    let mut h: i64 = 1_469_598_103_934_665_603u64 as i64;
    for b in obj.bytes().chain([0]).chain(who.bytes()) {
        h = (h ^ b as i64).wrapping_mul(1_099_511_628_211);
    }
    h & i64::MAX
}

fn display_packet(slot: &str, name: &str, o: &Objective, desc: bool) -> Protocol {
    use bedrock::protocol::v662::enums::ObjectiveSortOrder;
    Protocol::SetDisplayObjectivePacket(Box::new(bedrock::protocol::v662::packets::SetDisplayObjectivePacket {
        display_slot_name: slot.into(),
        objective_name: name.into(),
        objective_display_name: o.display.clone(),
        criteria_name: "dummy".into(),
        sort_order: if desc { ObjectiveSortOrder::Descending } else { ObjectiveSortOrder::Ascending },
    }))
}

fn scores_packet(name: &str, o: &Objective) -> Protocol {
    use bedrock::protocol::v2168::packets::{ScorePacketEntry, SetScorePacket};
    use bedrock::protocol::v662::types::ScoreboardId;
    Protocol::SetScorePacket(Box::new(SetScorePacket {
        score_info: o.scores.iter().map(|(who, &v)| ScorePacketEntry::ChangeFakePlayer {
            scoreboard_id: ScoreboardId { id: line_id(name, who) },
            objective_name: name.into(),
            score_value: v,
            fake_player_name: who.clone(),
        }).collect(),
    }))
}

fn remove_packet(name: &str) -> Protocol {
    Protocol::RemoveObjectivePacket(Box::new(bedrock::protocol::v662::packets::RemoveObjectivePacket { objective_name: name.into() }))
}

fn everyone(server: &Server, p: Protocol) {
    let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(p) });
}

/// Resend an objective to everyone if it's on display.
fn refresh(server: &Server, b: &Board, name: &str) {
    let Some(o) = b.objectives.get(name) else { return };
    for (slot, (obj, desc)) in &b.slots {
        if obj == name {
            everyone(server, remove_packet(name));
            everyone(server, display_packet(slot, name, o, *desc));
            everyone(server, scores_packet(name, o));
        }
    }
}

/// Scoreboards for a player who just joined.
pub fn join_packets() -> Vec<Protocol> {
    with_board(|b| {
        b.slots.iter().filter_map(|(slot, (obj, desc))| b.objectives.get(obj).map(|o| (slot, obj, o, *desc)))
            .flat_map(|(slot, obj, o, desc)| [display_packet(slot, obj, o, desc), scores_packet(obj, o)])
            .collect()
    })
}

/// For `scores=` selectors and plugins.
pub fn score(objective: &str, who: &str) -> Option<i32> {
    with_board(|b| b.objectives.get(objective)?.scores.get(who).copied())
}

/// For plugins, shows it right away if the objective is displayed.
pub fn set_score(server: &Server, objective: &str, who: &str, value: i32) -> bool {
    with_board(|b| {
        let Some(o) = b.objectives.get_mut(objective) else { return false };
        o.scores.insert(who.to_string(), value);
        refresh(server, b, objective);
        save(b);
        true
    })
}

/// `/scoreboard ...` in Bedrock syntax. `me` is whoever ran it, the caller already
/// expanded `@s` style holders into names.
pub fn command(server: &Server, me: &str, args: &[&str]) -> String {
    let holders = |who: &str| -> Vec<String> {
        match who {
            "*" => with_board(|b| {
                let mut all: Vec<String> = b.objectives.values().flat_map(|o| o.scores.keys().cloned()).collect();
                all.sort();
                all.dedup();
                all
            }),
            "@s" => vec![me.to_string()],
            w if w.starts_with('@') => {
                let id = server.find_by_name(me).unwrap_or(0);
                crate::commands::select(server, id, w).unwrap_or_default().into_iter().filter_map(|i| server.get(i)).map(|p| p.name).collect()
            }
            w => vec![w.trim_matches('"').to_string()],
        }
    };
    with_board(|b| {
        let out = match args {
            ["objectives", "add", name, criteria, display @ ..] => {
                if *criteria != "dummy" {
                    return "§cOnly the 'dummy' criteria is supported".into();
                }
                if b.objectives.contains_key(*name) {
                    return format!("§cAn objective already exists by the name '{name}'");
                }
                let display = if display.is_empty() { name.to_string() } else { display.join(" ").trim_matches('"').to_string() };
                b.objectives.insert(name.to_string(), Objective { display, criteria: criteria.to_string(), scores: HashMap::new() });
                format!("Added new objective '{name}' successfully")
            }
            ["objectives", "remove", name] => {
                if b.objectives.remove(*name).is_none() {
                    return format!("§cNo objective was found by the name '{name}'");
                }
                b.slots.retain(|_, (o, _)| o != name);
                everyone(server, remove_packet(name));
                format!("Removed objective '{name}' successfully")
            }
            ["objectives", "list"] => {
                let list: Vec<String> = b.objectives.iter().map(|(n, o)| format!("- {n}: displays as '{}' and is type '{}'", o.display, o.criteria)).collect();
                format!("There are {} objectives:\n{}", list.len(), list.join("\n"))
            }
            ["objectives", "setdisplay", slot, rest @ ..] => {
                if !matches!(*slot, "sidebar" | "list" | "belowname") {
                    return "§cDisplay slot must be sidebar, list or belowname".into();
                }
                match rest.first() {
                    None => {
                        if let Some((old, _)) = b.slots.remove(*slot) {
                            everyone(server, remove_packet(&old));
                        }
                        format!("Cleared objective display slot '{slot}'")
                    }
                    Some(obj) if b.objectives.contains_key(*obj) => {
                        let desc = rest.get(1).is_none_or(|o| *o != "ascending");
                        if let Some((old, _)) = b.slots.insert(slot.to_string(), (obj.to_string(), desc)) {
                            everyone(server, remove_packet(&old));
                        }
                        refresh(server, b, obj);
                        format!("Set display slot '{slot}' to show objective '{obj}'")
                    }
                    Some(obj) => return format!("§cNo objective was found by the name '{obj}'"),
                }
            }
            ["players", "list", rest @ ..] => match rest.first() {
                Some(who) => {
                    let lines: Vec<String> = holders(who).iter().flat_map(|h| b.objectives.iter().filter_map(move |(n, o)| o.scores.get(h).map(|v| format!("- {h}: {v} ({n})")))).collect();
                    format!("Scores:\n{}", lines.join("\n"))
                }
                None => {
                    let mut all: Vec<&String> = b.objectives.values().flat_map(|o| o.scores.keys()).collect();
                    all.sort();
                    all.dedup();
                    format!("There are {} tracked players: {}", all.len(), all.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))
                }
            },
            ["players", op @ ("set" | "add" | "remove"), who, obj, n] => {
                let Ok(n) = n.parse::<i32>() else { return "§cScore must be a number".into() };
                let Some(o) = b.objectives.get_mut(*obj) else { return format!("§cNo objective was found by the name '{obj}'") };
                let hs = holders(who);
                for h in &hs {
                    let s = o.scores.entry(h.clone()).or_insert(0);
                    *s = match *op {
                        "set" => n,
                        "add" => s.saturating_add(n),
                        _ => s.saturating_sub(n),
                    };
                }
                refresh(server, b, obj);
                format!("{} score for {} in objective {obj}", match *op { "set" => "Set", "add" => "Added to", _ => "Removed from" }, hs.join(", "))
            }
            ["players", "random", who, obj, lo, hi] => {
                let (Ok(lo), Ok(hi)) = (lo.parse::<i32>(), hi.parse::<i32>()) else { return "§cMin and max must be numbers".into() };
                let Some(o) = b.objectives.get_mut(*obj) else { return format!("§cNo objective was found by the name '{obj}'") };
                let span = (hi as i64 - lo as i64 + 1).max(1) as u64;
                for h in holders(who) {
                    let r = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() as u64 ^ line_id(obj, &h) as u64;
                    o.scores.insert(h, lo + (r % span) as i32);
                }
                refresh(server, b, obj);
                format!("Set random scores in objective {obj}")
            }
            ["players", "reset", who, rest @ ..] => {
                let hs = holders(who);
                for (n, o) in b.objectives.iter_mut() {
                    if rest.first().is_none_or(|r| r == n) {
                        for h in &hs {
                            o.scores.remove(h);
                        }
                    }
                }
                let shown: Vec<String> = b.slots.values().map(|(o, _)| o.clone()).collect();
                for o in shown {
                    refresh(server, b, &o);
                }
                format!("Reset scores of {}", hs.join(", "))
            }
            ["players", "operation", target, tobj, op, source, sobj] => {
                let src: Vec<i32> = holders(source).iter().filter_map(|h| b.objectives.get(*sobj)?.scores.get(h).copied()).collect();
                let Some(&s) = src.first() else { return "§cThe source has no score".into() };
                let Some(o) = b.objectives.get_mut(*tobj) else { return format!("§cNo objective was found by the name '{tobj}'") };
                for h in holders(target) {
                    let t = o.scores.entry(h).or_insert(0);
                    *t = match *op {
                        "=" => s,
                        "+=" => t.saturating_add(s),
                        "-=" => t.saturating_sub(s),
                        "*=" => t.saturating_mul(s),
                        "/=" if s != 0 => t.div_euclid(s),
                        "%=" if s != 0 => t.rem_euclid(s),
                        "<" => (*t).min(s),
                        ">" => (*t).max(s),
                        "><" => {
                            let old = *t;
                            *t = s;
                            let _ = old; // TODO: swap only writes the target side
                            s
                        }
                        _ => return format!("§cUnknown operation: {op}"),
                    };
                }
                refresh(server, b, tobj);
                "Operation applied".into()
            }
            ["players", "test", who, obj, lo, rest @ ..] => {
                let parse = |v: &str, d: i32| if v == "*" { Some(d) } else { v.parse().ok() };
                let (Some(lo), Some(hi)) = (parse(lo, i32::MIN), parse(rest.first().copied().unwrap_or("*"), i32::MAX)) else { return "§cBad range".into() };
                let hs = holders(who);
                let ok: Vec<&String> = hs.iter().filter(|h| b.objectives.get(*obj).and_then(|o| o.scores.get(*h)).is_some_and(|v| (lo..=hi).contains(v))).collect();
                if ok.is_empty() { "§cNo score matched".into() } else { format!("Score in range for {}", ok.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")) }
            }
            _ => return "§cUse /scoreboard objectives add|remove|list|setdisplay ... or /scoreboard players set|add|remove|reset|list|random|operation|test ...".into(),
        };
        save(b);
        out
    })
}

// ---- boss bars

/// Bedrock colours: 0 pink, 1 blue, 2 red, 3 green, 4 yellow, 5 purple, 6 white.
/// Each player's boss bar entity: an invisible, zero-size creeper the bar belongs to, the way
/// PNX's DummyBossBar does it (the game only shows a bar for an entity it knows about).
static BOSS_ENTITIES: Mutex<Option<HashMap<i64, i64>>> = Mutex::new(None);

pub fn boss_bar(server: &Server, to: i64, text: &str, percent: f32, color: u8) {
    use bedrock::protocol::v2193::packets::{BossEventPacket, BossEventUpdateType};
    use bedrock::protocol::v662::types::{ActorRuntimeID, ActorUniqueID, DataItem, PropertySyncData};
    use bedrock::protocol::v924::enums::ActorDataIDs;
    use bedrock::protocol::v2168::enums::DataItemType;
    let Some(p) = server.get(to) else { return };
    // Just under the player, so it stays in range as they move.
    let at = (p.pos.0, p.pos.1 - 10.0, p.pos.2);
    let (boss, new) = {
        let mut m = BOSS_ENTITIES.lock();
        let m = m.get_or_insert_with(HashMap::new);
        match m.get(&to) {
            Some(&id) => (id, false),
            None => {
                let id = crate::players::next_id();
                m.insert(to, id);
                (id, true)
            }
        }
    };
    let send = |packet: Protocol| {
        let _ = server.events.send(Event::Packet { to, packet: Box::new(packet) });
    };
    if new {
        send(Protocol::AddActorPacket(Box::new(bedrock::protocol::v2225::packets::AddActorPacket {
            target_actor_id: ActorUniqueID(boss),
            target_runtime_id: ActorRuntimeID(boss as u64),
            actor_type: "minecraft:creeper".into(),
            position: at,
            velocity: (0.0, 0.0, 0.0),
            rotation: (0.0, 0.0),
            y_head_rotation: 0.0,
            y_body_rotation: 0.0,
            attributes: vec![],
            actor_data: vec![
                DataItem { data_item_id: ActorDataIDs::Reserved038, data_item_type: DataItemType::Float(0.0) },
                DataItem { data_item_id: ActorDataIDs::Name, data_item_type: DataItemType::String(text.into()) },
            ],
            synced_properties: PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
            actor_links: vec![],
            passenger_of_block_data: None,
        })));
    } else {
        send(Protocol::MoveActorAbsolutePacket(Box::new(bedrock::protocol::v662::packets::MoveActorAbsolutePacket {
            move_data: bedrock::protocol::v662::types::MoveActorAbsoluteData { actor_runtime_id: ActorRuntimeID(boss as u64), header: 0, position: at, rotation_x: 0, rotation_y: 0, rotation_y_head: 0 },
        })));
    }
    for event_type in [BossEventUpdateType::Add, BossEventUpdateType::UpdatePercent, BossEventUpdateType::UpdateName] {
        send(Protocol::BossEventPacket(Box::new(BossEventPacket {
            target_actor_id: ActorUniqueID(boss),
            event_type,
            name: text.into(),
            filtered_name: text.into(),
            health_percent: percent.clamp(0.0, 1.0),
            color,
            overlay: 0,
        })));
    }
}

/// Shows a sidebar named `objective` to `to`: a title and up to 15 lines, top to bottom.
/// Call again to update it (it replaces the old lines).
pub fn sidebar(server: &Server, to: i64, objective: &str, title: &str, lines: Vec<String>) {
    use bedrock::protocol::v2168::packets::{ScorePacketEntry, SetScorePacket};
    use bedrock::protocol::v662::enums::ObjectiveSortOrder;
    use bedrock::protocol::v662::types::ScoreboardId;
    let send = |packet: Protocol| {
        let _ = server.events.send(Event::Packet { to, packet: Box::new(packet) });
    };
    send(Protocol::SetDisplayObjectivePacket(Box::new(bedrock::protocol::v662::packets::SetDisplayObjectivePacket {
        display_slot_name: "sidebar".into(),
        objective_name: objective.into(),
        objective_display_name: title.into(),
        criteria_name: "dummy".into(),
        sort_order: ObjectiveSortOrder::Descending,
    })));
    // Highest score shows first; each line needs a different text, so blanks get § padding.
    let count = lines.len().min(15) as i32;
    let entries = lines.into_iter().take(15).enumerate().map(|(i, mut text)| {
        if text.trim().is_empty() {
            text = "§r".repeat(i + 1);
        }
        ScorePacketEntry::ChangeFakePlayer { scoreboard_id: ScoreboardId { id: 9000 + i as i64 }, objective_name: objective.into(), score_value: count - i as i32, fake_player_name: text }
    }).collect();
    send(Protocol::SetScorePacket(Box::new(SetScorePacket { score_info: entries })));
}

pub fn remove_sidebar(server: &Server, to: i64, objective: &str) {
    let packet = Protocol::RemoveObjectivePacket(Box::new(bedrock::protocol::v662::packets::RemoveObjectivePacket { objective_name: objective.into() }));
    let _ = server.events.send(Event::Packet { to, packet: Box::new(packet) });
}

pub fn remove_boss_bar(server: &Server, to: i64) {
    use bedrock::protocol::v2193::packets::{BossEventPacket, BossEventUpdateType};
    use bedrock::protocol::v662::types::ActorUniqueID;
    let Some(boss) = BOSS_ENTITIES.lock().as_mut().and_then(|m| m.remove(&to)) else { return };
    let send = |packet: Protocol| {
        let _ = server.events.send(Event::Packet { to, packet: Box::new(packet) });
    };
    send(Protocol::BossEventPacket(Box::new(BossEventPacket {
        target_actor_id: ActorUniqueID(boss),
        event_type: BossEventUpdateType::Remove,
        name: String::new(),
        filtered_name: String::new(),
        health_percent: 0.0,
        color: 0,
        overlay: 0,
    })));
    send(Protocol::RemoveActorPacket(Box::new(bedrock::protocol::v662::packets::RemoveActorPacket { target_actor_id: ActorUniqueID(boss) })));
}

// ---- chest menus

/// Called with the clicked slot; return false to close the menu.
type MenuClick = Box<dyn FnMut(&Server, &PlayerInfo, usize) -> bool + Send>;

struct Menu {
    title: String,
    slots: Vec<Option<crate::inventory::Stack>>,
    on_click: Option<MenuClick>,
}

static MENUS: Mutex<Option<HashMap<i64, Menu>>> = Mutex::new(None);

/// Opens a chest menu for `to` with these items (27 slots, extra are cut). Players can't take
/// anything out; each click calls `on_click` with the slot, which returns false to close it.
/// Opening another menu while one is open swaps it (multi-page menus).
pub fn menu(
    server: &Server,
    to: i64,
    title: &str,
    mut items: Vec<Option<crate::inventory::Stack>>,
    on_click: impl FnMut(&Server, &PlayerInfo, usize) -> bool + Send + 'static,
) {
    items.resize(crate::inventory::CHEST_SLOTS, None);
    MENUS.lock().get_or_insert_with(HashMap::new).insert(to, Menu { title: title.into(), slots: items, on_click: Some(Box::new(on_click)) });
    let _ = server.events.send(Event::Menu { to });
}

/// The open menu's title and items, for the session to show.
pub fn menu_view(player: i64) -> Option<(String, Vec<Option<crate::inventory::Stack>>)> {
    MENUS.lock().as_ref()?.get(&player).map(|m| (m.title.clone(), m.slots.clone()))
}

/// A click in `player`'s menu. False if the menu should close.
pub fn menu_click(server: &Server, player: &PlayerInfo, slot: usize) -> bool {
    let Some(mut f) = MENUS.lock().as_mut().and_then(|m| m.get_mut(&player.id)).and_then(|m| m.on_click.take()) else { return false };
    let keep = f(server, player, slot);
    // The callback may have opened a new menu; only give the handler back to the same one.
    if let Some(m) = MENUS.lock().as_mut().and_then(|m| m.get_mut(&player.id))
        && m.on_click.is_none()
    {
        m.on_click = Some(f);
    }
    keep
}

pub fn menu_closed(player: i64) {
    if let Some(m) = MENUS.lock().as_mut() {
        m.remove(&player);
    }
}

// ---- sign input

/// Gets the four lines back (locked ones as they were sent).
type SignDone = Box<dyn FnOnce(&Server, &PlayerInfo, [String; 4]) + Send>;

struct SignInput {
    lines: [String; 4],
    locked: [bool; 4],
    on_done: Option<SignDone>,
    /// Where the session put the fake sign, and the real block there.
    at: Option<((i32, i32, i32), i32)>,
}

static SIGNS: Mutex<Option<HashMap<i64, SignInput>>> = Mutex::new(None);

/// Opens a sign editor for `to` with these lines. Lines marked `locked` come back unchanged
/// whatever the player typed there; `on_done` gets all four lines when they close it.
/// Example: a search box, `["", "^^^^^^", "Type a name", "above"]` with lines 2-4 locked.
pub fn sign_input(server: &Server, to: i64, lines: [&str; 4], locked: [bool; 4], on_done: impl FnOnce(&Server, &PlayerInfo, [String; 4]) + Send + 'static) {
    let lines = lines.map(String::from);
    SIGNS.lock().get_or_insert_with(HashMap::new).insert(to, SignInput { lines, locked, on_done: Some(Box::new(on_done)), at: None });
    let _ = server.events.send(Event::SignInput { to });
}

/// The text to show and records where the session put the fake sign.
pub fn sign_input_open(player: i64, at: (i32, i32, i32), real: i32) -> Option<String> {
    let mut s = SIGNS.lock();
    let i = s.as_mut()?.get_mut(&player)?;
    i.at = Some((at, real));
    Some(i.lines.join("\n"))
}

/// The player wrote the fake sign at `pos`: hand the lines to the plugin. Returns the real
/// block to put back, or None if this isn't the player's sign input.
pub fn sign_input_done(server: &Server, player: &PlayerInfo, pos: (i32, i32, i32), text: &str) -> Option<i32> {
    let input = {
        let mut s = SIGNS.lock();
        let m = s.as_mut()?;
        if m.get(&player.id)?.at.map(|a| a.0) != Some(pos) {
            return None;
        }
        m.remove(&player.id)?
    };
    let typed: Vec<&str> = text.split('\n').collect();
    let lines: [String; 4] = std::array::from_fn(|i| if input.locked[i] { input.lines[i].clone() } else { typed.get(i).unwrap_or(&"").to_string() });
    if let Some(f) = input.on_done {
        f(server, player, lines);
    }
    input.at.map(|a| a.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_input_keeps_locked_lines() {
        let server = Server::new();
        let got = std::sync::Arc::new(Mutex::new(None));
        let g = got.clone();
        sign_input(&server, 77, ["", "^^^", "Type", "above"], [false, true, true, true], move |_, _, l| *g.lock() = Some(l));
        assert_eq!(sign_input_open(77, (1, 2, 3), 5).as_deref(), Some("\n^^^\nType\nabove"));
        let me = crate::players::tests::sample(77);
        assert_eq!(sign_input_done(&server, &me, (1, 2, 3), "hello\nchanged\nx\ny"), Some(5));
        assert_eq!(got.lock().clone().unwrap(), ["hello", "^^^", "Type", "above"].map(String::from));
    }

    #[test]
    fn forms_json_and_answers() {
        let f = Form::Simple { title: "Shop".into(), content: "Pick".into(), buttons: vec![("Sword".into(), Some("textures/items/diamond_sword".into())), ("Bow".into(), None)] };
        let j: Value = serde_json::from_str(&f.json()).unwrap();
        assert_eq!(j["type"], "form");
        assert_eq!(j["buttons"][0]["image"]["type"], "path");
        assert_eq!(f.parse(Some("1\n")), Response::Button(1));
        assert_eq!(f.parse(Some("7")), Response::Closed);
        assert_eq!(f.parse(None), Response::Closed);
        let m = Form::Modal { title: "?".into(), content: "Sure?".into(), yes: "Yes".into(), no: "No".into() };
        assert_eq!(m.parse(Some("true")), Response::Choice(true));
        let c = Form::Custom { title: "Settings".into(), elements: vec![Element::Label("hi".into()), Element::Toggle { text: "PvP".into(), default: true }] };
        assert_eq!(c.parse(Some("[null,false]")), Response::Values(vec![Value::Null, Value::Bool(false)]));
    }

    #[test]
    fn scoreboard_file_round_trip() {
        let text = "o kills\tdummy\tKills\ns kills\tSteve\t3\nd sidebar\tkills\t1\n";
        let b = load(text);
        assert_eq!(b.objectives["kills"].scores["Steve"], 3);
        assert_eq!(b.slots["sidebar"], ("kills".to_string(), true));
        assert_ne!(line_id("kills", "Steve"), line_id("kills", "Alex"));
    }
}


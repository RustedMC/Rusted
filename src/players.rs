//! Who's online and what each session needs to show them to the others.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use parking_lot::Mutex;

use base64::Engine;
use bedrock::auth::client_data::ClientData;
use bedrock::protocol::ProtoCodec;
use bedrock::protocol::V2225;
use bedrock::protocol::v2168::enums::{AnimatedTextureType, AnimationExpression, ArmSizeType};
use bedrock::protocol::v2225::enums::PersonaPieceType;
use bedrock::protocol::v2225::types::{
    PersonaPiecesEntry, PieceTintColorsEntry, SerializedSkin, SerializedSkinAnimationFrame,
};
use uuid::Uuid;

/// Server-wide events that every session listens to.
#[derive(Clone, Debug)]
pub enum Event {
    /// empty `from` = server announcement
    Chat { from: String, message: String },
    Join(i64),
    Move { id: i64, pos: (f32, f32, f32), rot: (f32, f32), head_yaw: f32 },
    Leave { id: i64, uuid: Uuid },
    /// `old` is the previous block, for break particles and sounds.
    /// `by` already saw the effects on their own client.
    Block { x: i32, y: i32, z: i32, id: i32, old: i32, #[allow(dead_code)] by: i64, effects: bool },
    /// `by` changed the slots of the container at `pos`: other viewers reload it
    Container { pos: (i32, i32, i32), by: i64 },
    /// shown to everyone but `from` (arm swings, held items)
    Relay { from: i64, packet: Box<crate::Protocol> },
    Tell { to: i64, message: String },
    /// autosave
    SaveAll,
    /// mob, arrow or explosion damage, with knockback
    Damage { to: i64, damage: crate::health::Damage, knock: (f32, f32, f32) },
    /// effect from a mob, e.g. cave spider poison (id, amplifier, ticks)
    Effect { to: i64, effect: i32, amplifier: i32, ticks: i32 },
    /// Command aimed at `to` (`@a`, a name) that runs in their session, replies go to `reply_to`.
    RunAs { to: i64, line: String, reply_to: i64 },
    /// forms, boss bars
    Packet { to: i64, packet: Box<crate::Protocol> },
    /// negative takes XP away
    Xp { to: i64, points: i32 },
    /// teleport to these feet coords (ender pearls)
    Teleport { id: i64, feet: (f32, f32, f32) },
    /// kick `id` (0 = everyone) after saving
    Kick { id: i64, message: String, reason: Kick },
    /// put this exact item (custom name, lore, plugin tags) in a player's inventory
    #[allow(dead_code)] // sent by plugins
    Give { to: i64, item: Box<crate::inventory::Stack> },
    /// a plugin menu for `to` was opened or changed (see `ui::menu`)
    Menu { to: i64 },
    /// set `to` on fire for this many seconds (fire aspect, flame arrows)
    Ignite { to: i64, secs: u32 },
    /// a plugin asked `to` to type on a sign (see `ui::sign_input`)
    SignInput { to: i64 },
    /// take items from a player, then hand what was taken to the plugin (see `Server::take_held`)
    Take { to: i64, what: TakeWhat, then: Taken },
}

#[derive(Clone, Debug)]
pub enum TakeWhat {
    /// up to this many from the selected hotbar slot
    Held(i8),
    /// exactly this many of an item id from anywhere in the inventory, or nothing
    Item(String, u32),
}

/// What was taken: one stack of the item (for its name, enchantments...) and the total count.
pub type TakeResult = Option<(crate::inventory::Stack, u32)>;
type TakeFn = Box<dyn FnOnce(&Server, TakeResult) + Send>;

/// Runs once in the player's session after the items are taken.
#[derive(Clone)]
pub struct Taken(pub Arc<Mutex<Option<TakeFn>>>);

impl std::fmt::Debug for Taken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Taken")
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Kick {
    OtherLocation,
    Shutdown,
    Kicked,
    Full,
}

#[derive(Clone)]
pub struct PlayerInfo {
    pub id: i64,
    pub uuid: Uuid,
    pub name: String,
    pub xuid: String,
    pub skin: Arc<SerializedSkin<V2225>>,
    /// eye position, as the client reports it
    pub pos: (f32, f32, f32),
    pub rot: (f32, f32),
    pub head_yaw: f32,
}

/// Each player's held item and armor (helmet, chestplate, leggings, boots), refreshed every
/// second by their session. For plugins that copy how a player looks.
pub static EQUIPMENT: Mutex<Option<HashMap<i64, Equipment>>> = Mutex::new(None);

#[derive(Clone, Default)]
#[allow(dead_code)] // read by plugins
pub struct Equipment {
    pub held: Option<crate::inventory::Stack>,
    pub armor: [Option<crate::inventory::Stack>; 4],
    /// the 36 main slots (hotbar first), for /invsee
    pub inventory: Vec<Option<crate::inventory::Stack>>,
}

/// What player `id` is holding and wearing, as of the last second.
pub fn equipment(id: i64) -> Equipment {
    EQUIPMENT.lock().as_ref().and_then(|m| m.get(&id).cloned()).unwrap_or_default()
}

#[derive(Clone)]
pub struct Server {
    pub players: Arc<Mutex<HashMap<i64, PlayerInfo>>>,
    pub events: tokio::sync::broadcast::Sender<Event>,
    pub world: crate::world::World,
    /// Ticks at the last /time set and when that happened. Runs at 20 ticks/s.
    /// Time of day at `.1`, and whether the clock is stopped (doDaylightCycle off).
    clock: Arc<Mutex<(i32, std::time::Instant, bool)>>,
    /// /gamerule overrides on top of vanilla defaults, not saved.
    pub game_rules: Arc<Mutex<Vec<bedrock::protocol::v844::types::GameRuleChanged>>>,
    pub plugins: Arc<crate::plugin::Plugins>,
    pub fluids: Arc<crate::fluids::Fluids>,
    pub entities: Arc<crate::entities::Entities>,
    pub mobs: Arc<crate::mobs::Mobs>,
    pub containers: Arc<crate::containers::Containers>,
    pub scheduler: Arc<crate::plugin::Scheduler>,
}

static NEXT_ID: AtomicI64 = AtomicI64::new(1);

/// Players and NPCs share ids, 0 means "none" to the client.
pub fn next_id() -> i64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

impl Server {
    #[cfg(test)]
    pub fn new() -> Self {
        Self::with_world(crate::world::World::default())
    }

    pub fn with_world(world: crate::world::World) -> Self {
        Self {
            players: Arc::default(),
            events: tokio::sync::broadcast::channel(1024).0,
            world,
            clock: Arc::new(Mutex::new((1000, std::time::Instant::now(), false))), // morning
            game_rules: Arc::default(),
            plugins: Arc::new(crate::plugin::Plugins::new(crate::plugin::all())),
            fluids: Arc::default(),
            entities: Arc::default(),
            mobs: Arc::default(),
            containers: Arc::default(),
            scheduler: Arc::default(),
        }
    }

    pub fn join(&self, info: PlayerInfo) {
        let id = info.id;
        self.players.lock().insert(id, info);
        let _ = self.events.send(Event::Join(id));
    }

    pub fn leave(&self, id: i64) {
        if let Some(info) = self.players.lock().remove(&id) {
            let _ = self.events.send(Event::Leave { id, uuid: info.uuid });
        }
    }

    pub fn moved(&self, id: i64, pos: (f32, f32, f32), rot: (f32, f32), head_yaw: f32) {
        if let Some(p) = self.players.lock().get_mut(&id) {
            if p.pos == pos && p.rot == rot && p.head_yaw == head_yaw {
                return; // standing still: don't spam everyone
            }
            (p.pos, p.rot, p.head_yaw) = (pos, rot, head_yaw);
        } else {
            return;
        }
        let _ = self.events.send(Event::Move { id, pos, rot, head_yaw });
    }

    /// Sets a block and tells everyone.
    pub fn set_block(&self, by: i64, x: i32, y: i32, z: i32, id: i32) {
        self.change_block(by, x, y, z, id, true);
    }

    /// No sounds or particles (doors, falling sand).
    pub fn set_block_quiet(&self, by: i64, x: i32, y: i32, z: i32, id: i32) {
        self.change_block(by, x, y, z, id, false);
    }

    fn change_block(&self, by: i64, x: i32, y: i32, z: i32, id: i32, effects: bool) {
        let old = self.world.get(x, y, z);
        let wet = self.world.waterlogged(x, y, z);
        if old != id && self.world.set(x, y, z, id) {
            if wet && !self.world.waterlogged(x, y, z) {
                // The new block can't hold water: clients drop the water layer too.
                let _ = self.events.send(Event::Relay { from: 0, packet: Box::new(crate::packets::update_layer(x, y, z, crate::world::AIR, 1)) });
            }
            self.fluids.notify((x, y, z)); // nearby water/lava may flow now
            crate::redstone::mark((x, y, z));
            if effects {
                crate::sculk::block_changed(self, (x, y, z), old, id); // placing and breaking are heard
            }
            let _ = self.events.send(Event::Block { x, y, z, id, old, by, effects });
            // Fences, panes, walls, dust and stairs around here re-join their neighbours.
            let get = |x, y, z| self.world.get(x, y, z);
            for q in crate::connect::around((x, y, z)) {
                if let Some(nb) = crate::connect::shape(&get, q) {
                    let was = self.world.get(q.0, q.1, q.2);
                    if self.world.set(q.0, q.1, q.2, nb) {
                        let _ = self.events.send(Event::Block { x: q.0, y: q.1, z: q.2, id: nb, old: was, by: 0, effects: false });
                    }
                }
            }
        }
    }

    /// Waterlogs (or drains) the block at this spot: everyone sees the water layer change and
    /// the water can flow from it.
    pub fn set_water(&self, x: i32, y: i32, z: i32, on: bool) {
        if self.world.set_water(x, y, z, on) {
            let b = if on { crate::fluids::Kind::Water.source() } else { crate::world::AIR };
            let _ = self.events.send(Event::Relay { from: 0, packet: Box::new(crate::packets::update_layer(x, y, z, b, 1)) });
            self.fluids.notify((x, y, z));
        }
    }

    /// Does a player's 0.6x1.8 hitbox overlap this block?
    pub fn occupied(&self, x: i32, y: i32, z: i32) -> bool {
        const HALF_WIDTH: f32 = 0.3;
        const HEIGHT: f32 = 1.8;
        const EYE: f32 = 1.62;
        let (bx, by, bz) = (x as f32, y as f32, z as f32);
        self.players.lock().values().any(|p| {
            let (px, feet, pz) = (p.pos.0, p.pos.1 - EYE, p.pos.2);
            px + HALF_WIDTH > bx && px - HALF_WIDTH < bx + 1.0
                && feet + HEIGHT > by && feet < by + 1.0
                && pz + HALF_WIDTH > bz && pz - HALF_WIDTH < bz + 1.0
        })
    }

    pub fn tell(&self, id: i64, message: &str) {
        let _ = self.events.send(Event::Tell { to: id, message: message.into() });
    }

    pub fn broadcast(&self, message: &str) {
        let _ = self.events.send(Event::Chat { from: String::new(), message: message.into() });
    }

    /// 24000 per day
    /// 24000 per day. Stands still while doDaylightCycle is off, like the players' clocks do.
    pub fn time(&self) -> i32 {
        use bedrock::protocol::v844::types::Type;
        let running = self.game_rules.lock().iter().find(|r| r.rule_name == "doDaylightCycle").is_none_or(|r| matches!(r.rule_type, Type::Bool(true)));
        let mut c = self.clock.lock();
        let now = c.0.wrapping_add((c.1.elapsed().as_millis() / 50) as i32);
        if running && !c.2 {
            return now;
        }
        // Stopping or restarting: fold the time so far in, so it carries on from here.
        *c = (if c.2 { c.0 } else { now }, std::time::Instant::now(), !running);
        c.0
    }

    pub fn set_time(&self, ticks: i32) {
        let mut c = self.clock.lock();
        (c.0, c.1) = (ticks, std::time::Instant::now());
    }

    /// Players only, not NPCs.
    pub fn find_by_name(&self, name: &str) -> Option<i64> {
        self.players.lock().values().find(|p| p.name == name).map(|p| p.id)
    }

    pub fn get(&self, id: i64) -> Option<PlayerInfo> {
        self.players.lock().get(&id).cloned()
    }

    pub fn all(&self) -> Vec<PlayerInfo> {
        self.players.lock().values().cloned().collect()
    }
}

fn b64(s: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(s).unwrap_or_default()
}

fn b64_text(s: &str) -> String {
    String::from_utf8(b64(s)).unwrap_or_default()
}


/// Player control for plugins (and anything else that wants it).
#[allow(dead_code)] // plugin API: plugins call these, the server itself may not
impl Server {
    /// Runs a command as if `id` typed it, without the op check. Any vanilla command works,
    /// so this reaches everything a player can be made to do.
    pub fn run_as(&self, id: i64, command: &str) {
        let _ = self.events.send(Event::RunAs { to: id, line: command.trim_start_matches('/').to_string(), reply_to: 0 });
    }

    /// Takes up to `count` of whatever `id` is holding, then calls `then` with it (None if their
    /// hand was empty). The session does the taking, so it can't race with the player.
    pub fn take_held(&self, id: i64, count: i8, then: impl FnOnce(&Server, TakeResult) + Send + 'static) {
        let then = Taken(Arc::new(Mutex::new(Some(Box::new(then)))));
        let _ = self.events.send(Event::Take { to: id, what: TakeWhat::Held(count), then });
    }

    /// Takes exactly `count` of item `name` from `id`'s inventory, or nothing if they have fewer.
    pub fn take_item(&self, id: i64, name: &str, count: u32, then: impl FnOnce(&Server, TakeResult) + Send + 'static) {
        let then = Taken(Arc::new(Mutex::new(Some(Box::new(then)))));
        let _ = self.events.send(Event::Take { to: id, what: TakeWhat::Item(name.to_string(), count), then });
    }

    /// Gives `item` (from `plugin::item`) to a player; what doesn't fit drops at their feet.
    pub fn give(&self, id: i64, item: crate::inventory::Stack) {
        let _ = self.events.send(Event::Give { to: id, item: Box::new(item) });
    }

    pub fn teleport(&self, id: i64, feet: (f32, f32, f32)) {
        let _ = self.events.send(Event::Teleport { id, feet });
    }

    /// Big title text, with an optional smaller line under it.
    pub fn title(&self, id: i64, title: &str, subtitle: &str) {
        if !subtitle.is_empty() {
            self.run_as(id, &format!("title @s subtitle {subtitle}"));
        }
        self.run_as(id, &format!("title @s title {title}"));
    }

    /// Text above the hotbar.
    pub fn actionbar(&self, id: i64, text: &str) {
        self.run_as(id, &format!("title @s actionbar {text}"));
    }

    /// `survival`, `creative`, `adventure` or `spectator`.
    pub fn set_gamemode(&self, id: i64, mode: &str) {
        self.run_as(id, &format!("gamemode {mode}"));
    }

    /// Effect id (see `effects`), amplifier 0 = level I, for `seconds`.
    pub fn effect(&self, id: i64, effect: i32, amplifier: i32, seconds: i32) {
        let _ = self.events.send(Event::Effect { to: id, effect, amplifier, ticks: seconds * 20 });
    }

    /// Plays a sound only that player hears, at their position.
    pub fn sound(&self, id: i64, sound: &str) {
        if let Some(p) = self.get(id) {
            let _ = self.events.send(Event::Packet { to: id, packet: Box::new(crate::packets::play_sound(sound, p.pos)) });
        }
    }

    pub fn kick(&self, id: i64, message: &str) {
        let _ = self.events.send(Event::Kick { id, message: message.into(), reason: Kick::Kicked });
    }

    /// Sends the player to another server (a lobby, a minigame), like a proxy hop.
    pub fn transfer(&self, id: i64, address: &str, port: u16) {
        self.run_as(id, &format!("transfer {address} {port}"));
    }
}

/// Login skin converted to what other clients expect.
pub fn skin_from_client(c: &ClientData) -> SerializedSkin<V2225> {
    // Clients without a skin (bots, test clients) still need valid JSON and an image or
    // everyone else rejects the player list entry, so fall back to a plain Steve skin.
    let mut patch = b64_text(&c.skin_resource_patch);
    if serde_json::from_str::<serde_json::Value>(&patch).is_err() {
        patch = r#"{"geometry":{"default":"geometry.humanoid.custom"}}"#.into();
    }
    let (mut image, mut w, mut h) = (b64(&c.skin_data), c.skin_image_width as u32, c.skin_image_height as u32);
    if image.is_empty() || image.len() != (w * h * 4) as usize {
        (image, w, h) = (vec![0xff; 64 * 64 * 4], 64, 64);
    }
    SerializedSkin {
        skin_id: if c.skin_id.is_empty() { "Standard_Custom".into() } else { c.skin_id.clone() },
        skin_resource_patch: patch,
        skin_image_width: w,
        skin_image_height: h,
        skin_image_bytes: image,
        animations: c
            .animated_image_data
            .iter()
            .map(|a| SerializedSkinAnimationFrame {
                image_width: a.image_width as u32,
                image_height: a.image_height as u32,
                image_bytes: b64(&a.image),
                animation_type: match a.animation_type {
                    1 => AnimatedTextureType::Face,
                    2 => AnimatedTextureType::Body32x32,
                    3 => AnimatedTextureType::Body128x128,
                    _ => AnimatedTextureType::None,
                },
                frame_count: a.frames as f32,
                animation_expression: if a.animation_expression == 1 {
                    AnimationExpression::Blinking
                } else {
                    AnimationExpression::Linear
                },
            })
            .collect(),
        cape_image_width: c.cape_image_width as u32,
        cape_image_height: c.cape_image_height as u32,
        cape_image_bytes: b64(&c.cape_data),
        geometry_data: json_or_empty_object(b64_text(&c.skin_geometry)),
        geometry_data_engine_version: b64_text(&c.skin_geometry_version),
        animation_data: json_or_empty_object(b64_text(&c.skin_animation_data)),
        cape_id: c.cape_id.clone(),
        full_id: format!("{}{}", c.skin_id, c.cape_id),
        arm_size: if c.arm_size == "slim" { ArmSizeType::Slim } else { ArmSizeType::Wide },
        skin_color: 0,
        // character creator skins have to go back exactly as sent or clients reject them
        persona_pieces: c
            .persona_pieces
            .iter()
            .map(|p| PersonaPiecesEntry {
                piece_id: p.piece_id.clone(),
                piece_type: piece_type(&p.piece_type),
                pack_id: Uuid::parse_str(&p.pack_id).unwrap_or_default(),
                is_default_piece: p.default,
                product_id: p.product_id.clone(),
            })
            .collect(),
        piece_tint_colors: c
            .piece_tint_colours
            .iter()
            .map(|t| PieceTintColorsEntry {
                // Login data says "persona_eyes"; tints are sent with the game's own name, "eyes".
                piece_type: tint_name(&t.piece_type),
                colors: t.colours.clone().map(|hex| parse_argb(&hex)),
            })
            .collect(),
        is_premium_skin: c.premium_skin,
        is_persona_skin: c.persona_skin,
        is_persona_cape_on_classic_skin: c.cape_on_classic_skin,
        is_primary_user: true,
        overrides_player_appearance: c.override_skin,
        // must be "unset", "false" or "true", anything else (even "") gets us kicked
        trusted_skin_flag: if c.trusted_skin { "true" } else { "false" }.into(),
        profile_hash: c.profile_hash.clone(),
    }
}

/// Skin JSON has to parse on the client even if the player sent nothing.
fn json_or_empty_object(s: String) -> String {
    if serde_json::from_str::<serde_json::Value>(&s).is_ok() { s } else { "{}".into() }
}

/// "#ffefbbb0" to ARGB i32
fn parse_argb(hex: &str) -> i32 {
    u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0) as i32
}

/// A piece type's name as the game writes it in tint colours (Cloudburst PersonaPieceType's
/// serialize names): "persona_facial_hair" is "facialhair", "persona_hand" is "hands".
fn tint_name(login: &str) -> String {
    let n = login.trim_start_matches("persona_");
    match n {
        "hand" => "hands".into(),
        "high_pants" => n.into(),
        _ => n.replace('_', ""),
    }
}

/// Piece types look like "persona_left_arm".
fn piece_type(name: &str) -> PersonaPieceType {
    use PersonaPieceType::*;
    match name.trim_start_matches("persona_") {
        "skeleton" => Skeleton,
        "body" => Body,
        "skin" => Skin,
        "bottom" => Bottom,
        "feet" => Feet,
        "dress" => Dress,
        "top" => Top,
        "high_pants" => HighPants,
        "hands" => Hands,
        "outerwear" => Outerwear,
        "facial_hair" => FacialHair,
        "mouth" => Mouth,
        "eyes" => Eyes,
        "hair" => Hair,
        "hood" => Hood,
        "back" => Back,
        "face_accessory" => FaceAccessory,
        "head" => Head,
        "legs" => Legs,
        "left_leg" => LeftLeg,
        "right_leg" => RightLeg,
        "arms" => Arms,
        "left_arm" => LeftArm,
        "right_arm" => RightArm,
        "capes" => Capes,
        "classic_skin" => ClassicSkin,
        "emote" => Emote,
        _ => Unknown,
    }
}

/// bedrock-rs `Color` has no constructor, so build it from the 4 wire bytes.
pub fn white() -> <V2225 as bedrock::protocol::ProtoVersionTypes>::Color {
    ProtoCodec::deserialize(&mut &[0xffu8, 0xff, 0xff, 0xff][..]).expect("4-byte color")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn base64_helpers() {
        assert_eq!(b64_text("aGk="), "hi");
        assert!(b64("not base64!").is_empty());
        assert_eq!(parse_argb("#ff000001"), 0xff00_0001_u32 as i32);
        assert!(matches!(piece_type("persona_left_arm"), PersonaPieceType::LeftArm));
        // skinless client still gets valid JSON and a 64x64 image
        let blank = skin_from_client(&ClientData::default());
        for json in [&blank.skin_resource_patch, &blank.geometry_data, &blank.animation_data] {
            assert!(serde_json::from_str::<serde_json::Value>(json).is_ok(), "{json:?}");
        }
        assert_eq!(blank.skin_image_bytes.len(), 64 * 64 * 4);
        let flag = blank.trusted_skin_flag;
        assert!(["unset", "false", "true"].contains(&flag.as_str()), "client rejects {flag:?}");
    }

    #[test]
    fn cannot_place_inside_a_player() {
        let s = Server::new();
        let mut p = sample(1);
        p.pos = (0.5, -60.0 + 1.62, 0.5); // standing on grass at block (0, -61, 0)
        s.players.lock().insert(1, p);
        assert!(s.occupied(0, -60, 0), "feet block");
        assert!(s.occupied(0, -59, 0), "head block");
        assert!(!s.occupied(0, -61, 0), "block underfoot");
        assert!(!s.occupied(1, -60, 0), "next to the player");
        assert!(!s.occupied(0, -58, 0), "above the head");
    }

    pub(crate) fn sample(id: i64) -> PlayerInfo {
        PlayerInfo {
            id,
            uuid: Uuid::nil(),
            name: "a".into(),
            xuid: String::new(),
            skin: Arc::new(skin_from_client(&ClientData::default())),
            pos: (0.0, 0.0, 0.0),
            rot: (0.0, 0.0),
            head_yaw: 0.0,
        }
    }

    #[test]
    fn clock_advances_and_resets() {
        let s = Server::new();
        s.set_time(13000);
        let t = s.time();
        assert!((13000..13010).contains(&t), "{t}");
        std::thread::sleep(std::time::Duration::from_millis(120));
        assert!(s.time() >= t + 2, "20 ticks per second");
    }

    #[test]
    fn moves_only_broadcast_on_change() {
        let s = Server::new();
        let mut rx = s.events.subscribe();
        s.players.lock().insert(
            7,
            PlayerInfo {
                id: 7,
                uuid: Uuid::nil(),
                name: "a".into(),
                xuid: String::new(),
                skin: Arc::new(skin_from_client(&ClientData::default())),
                pos: (0.0, 0.0, 0.0),
                rot: (0.0, 0.0),
                head_yaw: 0.0,
            },
        );
        s.moved(7, (0.0, 0.0, 0.0), (0.0, 0.0), 0.0);
        s.moved(7, (1.0, 0.0, 0.0), (0.0, 0.0), 0.0);
        assert!(matches!(rx.try_recv(), Ok(Event::Move { id: 7, pos: (1.0, _, _), .. })));
        assert!(rx.try_recv().is_err());
    }
}

#[cfg(test)]
mod tint_tests {
    #[test]
    fn tint_names_match_the_game() {
        assert_eq!(super::tint_name("persona_eyes"), "eyes");
        assert_eq!(super::tint_name("persona_facial_hair"), "facialhair");
        assert_eq!(super::tint_name("persona_hand"), "hands");
        assert_eq!(super::tint_name("persona_high_pants"), "high_pants");
        assert_eq!(super::tint_name("persona_left_leg"), "leftleg");
    }
}

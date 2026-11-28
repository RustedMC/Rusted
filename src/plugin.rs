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

/// When a plugin sees events compared to other plugins. `Lowest` runs first, so later plugins
/// get the final say. `Monitor` runs last, only sees events nobody cancelled, and can't cancel
/// them itself: use it for logging and stats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Priority {
    Lowest,
    Low,
    #[default]
    Normal,
    High,
    Highest,
    Monitor,
}

/// What a projectile hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Mob(i64),
    Player(i64),
}

/// Events raised where calling plugins right away isn't safe (inside the mob list's lock) or
/// there's no server at hand (redstone); fired once per game tick by `fire_later`.
pub enum Later {
    Combust(i64, &'static str),
    Despawn(i64, &'static str),
    Piston((i32, i32, i32), bool),
    Transform(i64, &'static str, &'static str, i64),
    MobShoot(i64, &'static str),
    RaidWave((f32, f32, f32), i32),
    RaidEnd((f32, f32, f32), bool),
    Shear(i64, i64),
    Regrow(i64),
    Leash(i64, i64, bool),
    CreeperPower(i64),
    Smelt((i32, i32, i32)),
    TrialSpawn((i32, i32, i32), &'static str, i64),
    DragonPerch(i64),
    Heal(i64, f32),
    ChunkGen((i32, i32)),
    Sit(i64, bool),
    TntCart(i64),
    Expire(i64, &'static str),
    TargetHit((i32, i32, i32), i32),
    Restock(i64),
    Hopper((i32, i32, i32), (i32, i32, i32)),
    Feed(i64, i64),
    BabyFeed(i64, i64),
    Saddle(i64, i64),
    CreeperIgnite(i64, i64),
    CureStart(i64, i64),
}

static LATER: parking_lot::Mutex<Vec<Later>> = parking_lot::Mutex::new(Vec::new());

pub fn later(e: Later) {
    let mut q = LATER.lock();
    if q.len() < 10_000 {
        q.push(e);
    }
}

/// A packet's name ("TextPacket") from its Debug output, stopping at the first `(` so big
/// packets (chunks) aren't formatted.
pub fn packet_name<T: std::fmt::Debug>(p: &T) -> String {
    struct Name(String);
    impl std::fmt::Write for Name {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            match s.find(['(', ' ', '{']) {
                Some(i) => {
                    self.0.push_str(&s[..i]);
                    Err(std::fmt::Error)
                }
                None => {
                    self.0.push_str(s);
                    Ok(())
                }
            }
        }
    }
    let mut n = Name(String::new());
    let _ = std::fmt::write(&mut n, format_args!("{p:?}"));
    n.0
}

/// Who gets a permission when nobody granted or denied it in `permissions.txt`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Everyone,
    Ops,
    Nobody,
}

pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;

    /// Custom blocks this plugin adds (textures from a resource pack). See custom.rs.
    fn custom_blocks(&self) -> Vec<crate::custom::Block> {
        vec![]
    }

    /// Custom items this plugin adds (textures from a resource pack). See custom.rs.
    fn custom_items(&self) -> Vec<crate::custom::Item> {
        vec![]
    }

    /// Custom mobs this plugin adds, by their resource-pack entity id. /summon works on them.
    fn custom_entities(&self) -> Vec<crate::mobs::Spec> {
        vec![]
    }

    /// Shown by `/pl`.
    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        ""
    }

    fn authors(&self) -> &[&str] {
        &[]
    }

    /// Plugins this one needs. If one is missing, this plugin isn't loaded.
    fn depends(&self) -> &[&str] {
        &[]
    }

    /// Plugins this one works with if they're there. They load first when present.
    fn soft_depends(&self) -> &[&str] {
        &[]
    }

    fn priority(&self) -> Priority {
        Priority::Normal
    }

    /// Permission nodes this plugin uses, like `("homes.set", Access::Everyone)`.
    fn permissions(&self) -> Vec<(&'static str, Access)> {
        vec![]
    }

    /// How a command is typed, one line per form, for autocomplete: plain words are
    /// subcommands (`a|b` offers both), `<x>` is required, `[x]` optional, `<x...>` takes the
    /// rest of the line. `<player>` suggests players, `<item>` items, `<amount>` and other
    /// counts expect a number. E.g. `&["set <name>", "delete <name>", "list", "[name]"]`.
    fn usage(&self, _command: &str) -> &'static [&'static str] {
        &[]
    }

    /// Permission a player needs to run one of this plugin's commands. `None` = anyone.
    fn command_permission(&self, _command: &str) -> Option<&'static str> {
        None
    }

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

    /// Every game tick (20 a second): custom mob AI, timers, particles, anything ongoing.
    fn on_tick(&self, _server: &Server) {}

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

    /// Player threw `count` of `item` out of their inventory. Return false to keep it in their
    /// hand (only a Q drop from the hotbar can be stopped; drops in a screen go through
    /// `on_inventory_action` first).
    fn on_item_drop(&self, _server: &Server, _player: &PlayerInfo, _item: &str, _count: i8) -> bool {
        true
    }

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

    /// Player used (right-clicked) the item in their hand, in the air or on a block. Return
    /// false to cancel the normal use. Pair with `plugin::tag` to find your custom items.
    fn on_item_use(&self, _server: &Server, _player: &PlayerInfo, _item: &crate::inventory::Stack) -> bool {
        true
    }

    /// Someone is logging in, before they join. Return a message to refuse them (bans,
    /// maintenance, a full network), or `None` to let them in.
    fn on_pre_login(&self, _server: &Server, _name: &str, _xuid: &str) -> Option<String> {
        None
    }

    /// Player moved to `to` (eye position). Return false to put them back where they were.
    fn on_move(&self, _server: &Server, _player: &PlayerInfo, _to: (f32, f32, f32)) -> bool {
        true
    }

    /// Player is about to pick up `count` of `item` off the ground. Return false to leave it.
    fn on_item_pickup(&self, _server: &Server, _player: &PlayerInfo, _item: &str, _count: i8) -> bool {
        true
    }

    /// Player started (`true`) or stopped sneaking.
    fn on_sneak(&self, _server: &Server, _player: &PlayerInfo, _sneaking: bool) {}

    /// Player started (`true`) or stopped sprinting.
    fn on_sprint(&self, _server: &Server, _player: &PlayerInfo, _sprinting: bool) {}

    /// Player is opening a chest, furnace or other block screen at `pos`. Return false to keep it shut.
    fn on_container_open(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32), _block: &str) -> bool {
        true
    }

    /// Player finished eating or drinking `item`. Return false and nothing is eaten.
    fn on_consume(&self, _server: &Server, _player: &PlayerInfo, _item: &str) -> bool {
        true
    }

    /// Player switched hotbar slot or what's in hand (`item` is `None` for an empty hand).
    fn on_held_change(&self, _server: &Server, _player: &PlayerInfo, _slot: u32, _item: Option<&str>) {}

    /// Player went to dimension `to` (0 overworld, 1 Nether, 2 End).
    fn on_dimension_change(&self, _server: &Server, _player: &PlayerInfo, _to: u8) {}

    /// Player was teleported to `to` (feet position): commands, pearls, plugins.
    fn on_teleport(&self, _server: &Server, _player: &PlayerInfo, _to: (f32, f32, f32)) {}

    /// Player is getting `points` of XP (negative takes it away). Change it, or set 0 to cancel.
    fn on_xp_change(&self, _server: &Server, _player: &PlayerInfo, _points: &mut i32) {}

    /// Player wrote a sign at `pos`. Edit the text, or return false to keep the old text.
    fn on_sign_change(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32), _front: &mut String, _back: &mut String) -> bool {
        true
    }

    /// Player right-clicked a bed at `pos` (sets their spawn). Return false to stop it.
    fn on_bed_enter(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32)) -> bool {
        true
    }

    /// Player got out of bed.
    fn on_bed_leave(&self, _server: &Server, _player: &PlayerInfo) {}

    /// Player is filling an empty bucket at `pos`; `filled` is what it becomes
    /// ("minecraft:water_bucket"...). Return false to stop it.
    fn on_bucket_fill(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32), _filled: &str) -> bool {
        true
    }

    /// Player is emptying `bucket` ("minecraft:water_bucket"...) at `pos`. Return false to stop it.
    fn on_bucket_empty(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32), _bucket: &str) -> bool {
        true
    }

    /// Player reeled in `item` with a fishing rod. Return false to lose the catch.
    fn on_fish(&self, _server: &Server, _player: &PlayerInfo, _item: &str) -> bool {
        true
    }

    /// Player started (`true`) or stopped gliding with an elytra.
    fn on_glide(&self, _server: &Server, _player: &PlayerInfo, _gliding: bool) {}

    /// Player started (`true`) or stopped swimming.
    fn on_swim(&self, _server: &Server, _player: &PlayerInfo, _swimming: bool) {}

    /// Player closed a container screen (`kind` like "Chest", "Furnace", "Anvil").
    fn on_container_close(&self, _server: &Server, _player: &PlayerInfo, _kind: &str) {}

    /// The tool or armour `item` in the player's hand wore out and broke.
    fn on_item_break(&self, _server: &Server, _player: &PlayerInfo, _item: &str) {}

    /// Player is being kicked with `message` (by a command, a ban or a plugin).
    fn on_kick(&self, _server: &Server, _player: &PlayerInfo, _message: &str) {}

    /// Player traded with villager `villager` (offer `index`, `times` times).
    fn on_trade(&self, _server: &Server, _player: &PlayerInfo, _villager: i64, _index: i32, _times: i32) {}

    /// Player got on mob `mob` (a horse, boat, minecart...).
    fn on_mount(&self, _server: &Server, _player: &PlayerInfo, _mob: i64) {}

    /// Player got off what they were riding.
    fn on_dismount(&self, _server: &Server, _player: &PlayerInfo) {}

    /// Player is crafting `count` of `item`. Return false to refuse the craft.
    fn on_craft(&self, _server: &Server, _player: &PlayerInfo, _item: &str, _count: i32) -> bool {
        true
    }

    /// An arrow or trident `shooter` fired hit a mob or player. Return false: no damage.
    fn on_projectile_hit(&self, _server: &Server, _shooter: &PlayerInfo, _target: Target) -> bool {
        true
    }

    /// A block changes on its own (crops and saplings growing, grass spreading, leaves
    /// decaying, ice melting). Return false to keep it as it is.
    fn on_block_grow(&self, _server: &Server, _pos: (i32, i32, i32), _from: &str, _to: &str) -> bool {
        true
    }

    /// Mob `mob` (of `kind`, e.g. "minecraft:zombie") wants to go after `player`. Return
    /// false and it leaves them alone (spawn protection, invisible staff, pets).
    fn on_mob_target(&self, _server: &Server, _mob: i64, _kind: &str, _player: &PlayerInfo) -> bool {
        true
    }

    /// Fire is spreading to `pos` (`spread`) or burning away the block there. Return false to stop it.
    fn on_block_burn(&self, _server: &Server, _pos: (i32, i32, i32), _block: &str, _spread: bool) -> bool {
        true
    }

    /// An explosion at `at` is about to break `blocks`. Remove positions to keep those blocks
    /// (clear it for a blast that only hurts mobs and players).
    fn on_explosion_blocks(&self, _server: &Server, _at: (f32, f32, f32), _blocks: &mut Vec<(i32, i32, i32)>) {}

    /// Player is trying to tame mob `mob` of `kind` (feeding a wolf or cat, riding a horse).
    /// Return false to stop it.
    fn on_entity_tame(&self, _server: &Server, _player: &PlayerInfo, _mob: i64, _kind: &str) -> bool {
        true
    }

    /// Two mobs of `kind` are about to breed. Return false: no baby.
    fn on_entity_breed(&self, _server: &Server, _kind: &str, _parents: (i64, i64)) -> bool {
        true
    }

    /// Player is lighting a portal, `"nether"` or `"end"`. Return false to keep it unlit.
    fn on_portal_create(&self, _server: &Server, _player: &PlayerInfo, _kind: &str) -> bool {
        true
    }

    /// Mob `mob` of `kind` is about to take `damage` from something that isn't a player: another
    /// mob, fire, a fall, an explosion. `from` is where it came from. Return false to cancel.
    fn on_mob_damage(&self, _server: &Server, _mob: i64, _kind: &str, _damage: f32, _from: (f32, f32, f32)) -> bool {
        true
    }

    /// Player is moving items in a screen. `action` is "take", "place", "swap", "drop",
    /// "destroy", "craft" or "other"; `container` is the screen's slot group, e.g.
    /// "LevelEntityContainer" for a chest. Return false to undo it.
    fn on_inventory_action(&self, _server: &Server, _player: &PlayerInfo, _action: &str, _container: &str) -> bool {
        true
    }

    /// Chunk (cx, cz) was sent to the player (it came into their view).
    fn on_chunk_load(&self, _server: &Server, _player: &PlayerInfo, _chunk: (i32, i32)) {}

    /// Player right-clicked (`attack` false) or hit (`attack` true) entity `entity`, which isn't
    /// one of Rusted's mobs: a plugin's own NPC, say.
    fn on_entity_click(&self, _server: &Server, _player: &PlayerInfo, _entity: i64, _attack: bool) {}

    /// Player took the result of a table: `table` is "Enchanting", "Anvil", "Grindstone",
    /// "Smithing", "Loom", "Stonecutter", "Cartography" or "Workbench"; `levels` the XP levels
    /// it cost (negative when a grindstone gave XP back).
    fn on_table_craft(&self, _server: &Server, _player: &PlayerInfo, _table: &str, _item: &str, _levels: i32) {}

    /// A brewing stand at `pos` finished a brew.
    fn on_brew(&self, _server: &Server, _pos: (i32, i32, i32)) {}

    /// Mob `mob` caught fire (sunlight, lava, fire). Fired after the mob tick.
    fn on_entity_combust(&self, _server: &Server, _mob: i64, _kind: &str) {}

    /// Mob `mob` despawned (too far from players, or peaceful). Fired after the mob tick.
    fn on_entity_despawn(&self, _server: &Server, _mob: i64, _kind: &str) {}

    /// The piston at `pos` started extending (`true`) or retracting.
    fn on_piston(&self, _server: &Server, _pos: (i32, i32, i32), _extending: bool) {}

    /// Player is about to heal from a full food bar. Return false to stop it.
    fn on_health_regain(&self, _server: &Server, _player: &PlayerInfo, _health: f32) -> bool {
        true
    }

    /// Player's food level changed to `food` (0-20).
    fn on_food_change(&self, _server: &Server, _player: &PlayerInfo, _food: i32) {}

    /// Leaves at `pos` are about to decay. Return false to keep them.
    fn on_leaves_decay(&self, _server: &Server, _pos: (i32, i32, i32), _block: &str) -> bool {
        true
    }

    /// Grass or mycelium is spreading onto `pos` (`from` becomes `to`). Return false to stop it.
    fn on_block_spread(&self, _server: &Server, _pos: (i32, i32, i32), _from: &str, _to: &str) -> bool {
        true
    }

    /// Snow, ice, obsidian or cobblestone is forming at `pos`. Return false to stop it.
    fn on_block_form(&self, _server: &Server, _pos: (i32, i32, i32), _block: &str) -> bool {
        true
    }

    /// Snow or ice at `pos` is melting (`from` becomes `to`). Return false to stop it.
    fn on_block_fade(&self, _server: &Server, _pos: (i32, i32, i32), _from: &str, _to: &str) -> bool {
        true
    }

    /// Player is opening (`on` true) or closing a door, trapdoor or gate, or flipping a lever.
    /// Return false to stop it.
    fn on_block_toggle(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32), _block: &str, _on: bool) -> bool {
        true
    }

    /// Player is placing a boat or minecart (`kind` like "minecraft:boat"). Return false to stop it.
    fn on_vehicle_place(&self, _server: &Server, _player: &PlayerInfo, _kind: &str, _pos: (f32, f32, f32)) -> bool {
        true
    }

    /// A boat or minecart was broken at `pos` (by `killer`, an entity id, if anyone).
    fn on_vehicle_destroy(&self, _server: &Server, _kind: &str, _pos: (f32, f32, f32), _killer: Option<i64>) {}

    /// Player is playing emote `emote` (its id). Return false to stop others seeing it.
    fn on_emote(&self, _server: &Server, _player: &PlayerInfo, _emote: &str) -> bool {
        true
    }

    /// Player changed their skin in game (`skin` is the new skin's name).
    fn on_skin_change(&self, _server: &Server, _player: &PlayerInfo, _skin: &str) {}

    /// Lightning is about to strike at `pos`. Return false to stop it.
    fn on_lightning(&self, _server: &Server, _pos: (f32, f32, f32)) -> bool {
        true
    }

    /// A dropped item at `pos` despawned after 5 minutes.
    fn on_item_despawn(&self, _server: &Server, _item: &str, _count: i8, _pos: (f32, f32, f32)) {}

    /// Mob `mob` turned into another (`from` and `to` kinds, `new_mob` its new id), such as a
    /// cured zombie villager. Fired after the mob tick.
    fn on_entity_transform(&self, _server: &Server, _mob: i64, _from: &str, _to: &str, _new_mob: i64) {}

    /// Mob `mob` fired a projectile (skeleton arrows and the like). Fired after the mob tick.
    fn on_mob_shoot(&self, _server: &Server, _mob: i64, _kind: &str) {}

    /// Player's client dropped the chunk at `chunk` (it's out of their view now).
    fn on_chunk_unload(&self, _server: &Server, _player: &PlayerInfo, _chunk: (i32, i32)) {}

    /// Player started (`true`) or stopped crawling.
    fn on_toggle_crawl(&self, _server: &Server, _player: &PlayerInfo, _crawling: bool) {}

    /// Player finished loading in and can play (after join, before they move).
    fn on_player_spawn(&self, _server: &Server, _player: &PlayerInfo) {}

    /// Player's client asks for the pixels of map `map`. Return false to send nothing.
    fn on_map_request(&self, _server: &Server, _player: &PlayerInfo, _map: i64) -> bool {
        true
    }

    /// Player middle-clicked `block` at `pos` to pick it. Return false to stop it.
    fn on_block_pick(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32), _block: &str) -> bool {
        true
    }

    /// Player turned the book on the lectern at `pos` to `page`.
    fn on_lectern_page(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32), _page: i32) {}

    /// Player is setting the beacon at `pos` to effects `primary` and `secondary` (effect ids,
    /// 0 for none). Return false to stop it.
    fn on_beacon_change(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32), _primary: i32, _secondary: i32) -> bool {
        true
    }

    /// Player landed on the farmland at `pos` and is about to trample it. Return false to stop it.
    fn on_farmland_trample(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32)) -> bool {
        true
    }

    /// The crafter at `pos` is about to craft `count` `item`. Return false to stop it.
    fn on_crafter_craft(&self, _server: &Server, _pos: (i32, i32, i32), _item: &str, _count: i8) -> bool {
        true
    }

    /// The dispenser or dropper (`block`) at `pos` got a pulse and is about to fire. Return false to stop it.
    fn on_dispense(&self, _server: &Server, _pos: (i32, i32, i32), _block: &str) -> bool {
        true
    }

    /// The note block at `pos` is about to play from redstone. Return false to keep it quiet.
    fn on_note_play(&self, _server: &Server, _pos: (i32, i32, i32)) -> bool {
        true
    }

    /// The bell at `pos` rang (`player` is who rang it, if a player did).
    fn on_bell_ring(&self, _server: &Server, _pos: (i32, i32, i32), _player: Option<i64>) {}

    /// A raid is about to start around `center`. Return false to stop it (the player keeps Bad Omen).
    fn on_raid_start(&self, _server: &Server, _center: (f32, f32, f32)) -> bool {
        true
    }

    /// Wave `wave` (from 1) of the raid at `center` is coming in. Fired after the raid tick.
    fn on_raid_wave(&self, _server: &Server, _center: (f32, f32, f32), _wave: i32) {}

    /// The raid at `center` ended: `victory` when every wave was beaten. Fired after the raid tick.
    fn on_raid_end(&self, _server: &Server, _center: (f32, f32, f32), _victory: bool) {}

    /// Player is about to be saved by a totem of undying. Return false and they die.
    fn on_totem_use(&self, _server: &Server, _player: &PlayerInfo) -> bool {
        true
    }

    /// Player `player` (entity id) sheared sheep `mob`. Fired after the mob tick.
    fn on_sheep_shear(&self, _server: &Server, _player: i64, _mob: i64) {}

    /// Sheep `mob` grew its wool back. Fired after the mob tick.
    fn on_sheep_regrow(&self, _server: &Server, _mob: i64) {}

    /// Player `player` tied (`attached`) or untied a lead on mob `mob`. Fired after the mob tick.
    fn on_leash(&self, _server: &Server, _player: i64, _mob: i64, _attached: bool) {}

    /// Villager `villager` levelled up to `tier` (1 novice to 5 master) from trading.
    fn on_villager_level_up(&self, _server: &Server, _villager: i64, _tier: i32) {}

    /// The mob spawner at `pos` is about to spawn a `kind`. Return false to stop it.
    fn on_spawner_spawn(&self, _server: &Server, _pos: (i32, i32, i32), _kind: &str) -> bool {
        true
    }

    /// Creeper `mob` was struck by lightning and is now charged. Fired after the mob tick.
    fn on_creeper_power(&self, _server: &Server, _mob: i64) {}

    /// `line` came in over RCON from `addr`. Return false to refuse it.
    fn on_rcon_command(&self, _server: &Server, _addr: &str, _line: &str) -> bool {
        true
    }

    /// `line` was typed into the server console. Return false to ignore it.
    fn on_console_command(&self, _server: &Server, _line: &str) -> bool {
        true
    }

    /// The furnace, smoker or blast furnace at `pos` finished smelting an item.
    fn on_furnace_smelt(&self, _server: &Server, _pos: (i32, i32, i32)) {}

    /// TNT at `pos` is about to be lit (by fire, redstone, an explosion or a player). Return false to stop it.
    fn on_tnt_prime(&self, _server: &Server, _pos: (i32, i32, i32)) -> bool {
        true
    }

    /// Player is getting effect `effect` (an effect id) at `amplifier` for `ticks`. Return false to stop it.
    fn on_effect_add(&self, _server: &Server, _player: &PlayerInfo, _effect: i32, _amplifier: i32, _ticks: i32) -> bool {
        true
    }

    /// Player lost effect `effect` (it ran out, or milk cleared it).
    fn on_effect_remove(&self, _server: &Server, _player: &PlayerInfo, _effect: i32) {}

    /// Player's ender pearl landed and is about to take them to `to`. Return false to stay put.
    fn on_ender_pearl(&self, _server: &Server, _player: &PlayerInfo, _to: (f32, f32, f32)) -> bool {
        true
    }

    /// `count` `item` is about to drop into the world at `pos` (mob drops, broken blocks,
    /// thrown items). Return false and it never appears.
    fn on_item_spawn(&self, _server: &Server, _item: &str, _count: i8, _pos: (f32, f32, f32)) -> bool {
        true
    }

    /// Player picked up an experience orb worth `value` points.
    fn on_xp_orb_pickup(&self, _server: &Server, _player: &PlayerInfo, _value: i32) {}

    /// A snow golem, iron golem or wither (`kind`) is about to be built at `pos`. Return false to stop it.
    fn on_golem_build(&self, _server: &Server, _kind: &str, _pos: (f32, f32, f32)) -> bool {
        true
    }

    /// A wandering trader is about to arrive at `pos`. Return false to stop it.
    fn on_trader_spawn(&self, _server: &Server, _pos: (f32, f32, f32)) -> bool {
        true
    }

    /// A pillager patrol is about to spawn at `pos`. Return false to stop it.
    fn on_patrol_spawn(&self, _server: &Server, _pos: (f32, f32, f32)) -> bool {
        true
    }

    /// The sculk shrieker at `pos` shrieked (`player` set when a player set it off).
    fn on_shriek(&self, _server: &Server, _pos: (i32, i32, i32), _player: Option<i64>) {}

    /// A warden is about to be summoned at `pos`. Return false to stop it.
    fn on_warden_summon(&self, _server: &Server, _pos: (f32, f32, f32)) -> bool {
        true
    }

    /// Player is opening the vault at `pos` with a trial key. Return false to stop it.
    fn on_vault_open(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32)) -> bool {
        true
    }

    /// The trial spawner at `pos` spawned `kind` (mob id `mob`). Fired after the trial tick.
    fn on_trial_spawn(&self, _server: &Server, _pos: (i32, i32, i32), _kind: &str, _mob: i64) {}

    /// Game rule `rule` is about to be set to `value` with /gamerule. Return false to stop it.
    fn on_gamerule_change(&self, _server: &Server, _rule: &str, _value: &str) -> bool {
        true
    }

    /// Difficulty is about to change to `difficulty` (0 peaceful to 3 hard). Return false to stop it.
    fn on_difficulty_change(&self, _server: &Server, _difficulty: i32) -> bool {
        true
    }

    /// `name` is about to be made op (`op` true) or de-opped. Return false to stop it.
    fn on_op_change(&self, _server: &Server, _name: &str, _op: bool) -> bool {
        true
    }

    /// `name` is about to be banned with `reason` (may be empty). Return false to stop it.
    fn on_ban(&self, _server: &Server, _name: &str, _reason: &str) -> bool {
        true
    }

    /// `name` is about to be unbanned. Return false to stop it.
    fn on_unban(&self, _server: &Server, _name: &str) -> bool {
        true
    }

    /// The address `ip` is about to be banned. Return false to stop it.
    fn on_ban_ip(&self, _server: &Server, _ip: &str) -> bool {
        true
    }

    /// `name` is about to be added to (`added` true) or removed from the allowlist. Return false to stop it.
    fn on_whitelist_change(&self, _server: &Server, _name: &str, _added: bool) -> bool {
        true
    }

    /// Player is about to be sent to world `world` with /mv tp. Return false to stop it.
    fn on_world_change(&self, _server: &Server, _player: &PlayerInfo, _world: &str) -> bool {
        true
    }

    /// A world backup was written to `path`.
    fn on_backup(&self, _server: &Server, _path: &str) {}

    /// The ender dragon `mob` is landing on the exit portal. Fired after the mob tick.
    fn on_dragon_perch(&self, _server: &Server, _mob: i64) {}

    /// Enough players are asleep and the night is about to be skipped. Return false to keep it night.
    fn on_night_skip(&self, _server: &Server) -> bool {
        true
    }

    /// Bone meal was used on `pos` (`player` set when a player used it).
    fn on_bonemeal(&self, _server: &Server, _pos: (i32, i32, i32), _player: Option<i64>) {}

    /// The respawn anchor at `pos` was charged with glowstone.
    fn on_anchor_charge(&self, _server: &Server, _pos: (i32, i32, i32), _player: Option<i64>) {}

    /// Player's XP level went from `old` to `new` (checked once a second).
    fn on_xp_level_change(&self, _server: &Server, _player: &PlayerInfo, _old: i32, _new: i32) {}

    /// Player is about to block `damage` with their shield. Return false to take the hit.
    fn on_shield_block(&self, _server: &Server, _player: &PlayerInfo, _damage: f32) -> bool {
        true
    }

    /// A slice of the cake at `pos` was eaten (`player` set when a player ate it).
    fn on_cake_eat(&self, _server: &Server, _pos: (i32, i32, i32), _player: Option<i64>) {}

    /// The dragon egg at `pos` is about to teleport to `to`. Return false to keep it there.
    fn on_dragon_egg_teleport(&self, _server: &Server, _pos: (i32, i32, i32), _to: (i32, i32, i32)) -> bool {
        true
    }

    /// An eye of ender was put into the end portal frame at `pos`.
    fn on_end_portal_fill(&self, _server: &Server, _pos: (i32, i32, i32), _player: Option<i64>) {}

    /// The composter at `pos` was filled or emptied.
    fn on_composter_use(&self, _server: &Server, _pos: (i32, i32, i32), _player: Option<i64>) {}

    /// Mob `mob` healed `amount`. Fired after the mob tick.
    fn on_mob_heal(&self, _server: &Server, _mob: i64, _amount: f32) {}

    /// Player's spawn point is about to be set to `pos` (/spawnpoint). Return false to stop it.
    fn on_spawn_point_set(&self, _server: &Server, _player: &PlayerInfo, _pos: (f32, f32, f32)) -> bool {
        true
    }

    /// Player asked for a view distance of `chunks`. Change it to give them another (the
    /// server's view-distance still caps it).
    fn on_view_distance(&self, _server: &Server, _player: &PlayerInfo, _chunks: &mut i32) {}

    /// The overworld chunk at `chunk` was generated for the first time. Fired after the tick.
    fn on_chunk_generate(&self, _server: &Server, _chunk: (i32, i32)) {}

    /// The time is about to be set to `time` with /time set. Return false to stop it.
    fn on_time_set(&self, _server: &Server, _time: i32) -> bool {
        true
    }

    /// A tamed wolf or cat `mob` sat down (`sitting` true) or stood up. Fired after the mob tick.
    fn on_sit_toggle(&self, _server: &Server, _mob: i64, _sitting: bool) {}

    /// A game tick took `ms` milliseconds, over the 50 ms a tick has (the server is lagging).
    fn on_slow_tick(&self, _server: &Server, _ms: f32) {}

    /// Player's armor in `slot` (0 helmet to 3 boots) changed to `item` ("" when taken off).
    /// Checked once a second.
    fn on_armor_change(&self, _server: &Server, _player: &PlayerInfo, _slot: i32, _item: &str) {}

    /// Player caught fire (lava or fire).
    fn on_player_combust(&self, _server: &Server, _player: &PlayerInfo) {}

    /// Player died and is about to drop their items. Return false to keep them (like keepInventory).
    fn on_death_drops(&self, _server: &Server, _player: &PlayerInfo) -> bool {
        true
    }

    /// Player stepped into a portal to dimension `to` (0 overworld, 1 Nether, 2 End). Return false to stay.
    fn on_portal_enter(&self, _server: &Server, _player: &PlayerInfo, _to: i32) -> bool {
        true
    }

    /// A firework burst at `pos` with `stars` stars.
    fn on_firework_explode(&self, _server: &Server, _pos: (f32, f32, f32), _stars: i32) {}

    /// The TNT minecart `mob` was lit by an activator rail. Fired after the mob tick.
    fn on_tnt_minecart_prime(&self, _server: &Server, _mob: i64) {}

    /// Player is about to cast a fishing rod. Return false to stop it.
    fn on_fishing_cast(&self, _server: &Server, _player: &PlayerInfo) -> bool {
        true
    }

    /// Player is about to boost their elytra with a firework. Return false to stop it.
    fn on_elytra_boost(&self, _server: &Server, _player: &PlayerInfo) -> bool {
        true
    }

    /// Player is about to throw `item` (snowball, egg, ender pearl, splash potion, bottle o'
    /// enchanting). Return false to stop it.
    fn on_item_throw(&self, _server: &Server, _player: &PlayerInfo, _item: &str) -> bool {
        true
    }

    /// The `block` at `pos` is about to blow up from being used (a respawn anchor outside the
    /// Nether). Return false to stop it.
    fn on_block_explode(&self, _server: &Server, _pos: (i32, i32, i32), _block: &str) -> bool {
        true
    }

    /// Player placed a sign at `pos` and its edit screen is opening.
    fn on_sign_open(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32)) {}

    /// Mob `mob` (a wandering trader and its llamas) left when its time ran out. Fired after the mob tick.
    fn on_mob_expire(&self, _server: &Server, _mob: i64, _kind: &str) {}

    /// A `sapling` at `pos` is about to grow into a tree. Return false to stop it.
    fn on_sapling_grow(&self, _server: &Server, _pos: (i32, i32, i32), _sapling: &str) -> bool {
        true
    }

    /// Player is about to light a fire at `pos` with flint and steel. Return false to stop it.
    fn on_block_ignite(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32)) -> bool {
        true
    }

    /// Player is pressing the `block` button at `pos`. Return false to stop it.
    fn on_button_press(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32), _block: &str) -> bool {
        true
    }

    /// The pressure plate at `pos` went down, giving `signal` (1-15).
    fn on_pressure_plate(&self, _server: &Server, _pos: (i32, i32, i32), _signal: i32) {}

    /// Something walked into the tripwire at `pos`.
    fn on_tripwire_trigger(&self, _server: &Server, _pos: (i32, i32, i32)) {}

    /// A projectile hit the target block at `pos`, giving `signal` (1-15). Fired after the tick.
    fn on_target_hit(&self, _server: &Server, _pos: (i32, i32, i32), _signal: i32) {}

    /// The sculk sensor at `pos` heard something (`player` set when a player made it).
    fn on_sculk_sensor(&self, _server: &Server, _pos: (i32, i32, i32), _player: Option<i64>) {}

    /// A log or stem at `pos` was stripped with an axe.
    fn on_log_strip(&self, _server: &Server, _pos: (i32, i32, i32), _player: Option<i64>) {}

    /// Copper at `pos` was waxed (`waxed` true) or had its wax taken off.
    fn on_copper_wax(&self, _server: &Server, _pos: (i32, i32, i32), _player: Option<i64>, _waxed: bool) {}

    /// Copper at `pos` was scraped back one weathering stage.
    fn on_copper_scrape(&self, _server: &Server, _pos: (i32, i32, i32), _player: Option<i64>) {}

    /// A shovel turned the block at `pos` into a path.
    fn on_path_create(&self, _server: &Server, _pos: (i32, i32, i32), _player: Option<i64>) {}

    /// A hoe tilled the block at `pos`.
    fn on_till(&self, _server: &Server, _pos: (i32, i32, i32), _player: Option<i64>) {}

    /// The campfire at `pos` was put out with a shovel.
    fn on_campfire_extinguish(&self, _server: &Server, _pos: (i32, i32, i32), _player: Option<i64>) {}

    /// Villager `villager` restocked its trades. Fired after the mob tick.
    fn on_villager_restock(&self, _server: &Server, _villager: i64) {}

    /// Player `attacker` is about to hit `victim` for `damage`. Return false to stop the hit.
    fn on_pvp_attack(&self, _server: &Server, _attacker: &PlayerInfo, _victim: &PlayerInfo, _damage: f32) -> bool {
        true
    }

    /// Player's mending gear soaked up `points` of picked-up XP to repair itself.
    fn on_mending_repair(&self, _server: &Server, _player: &PlayerInfo, _points: i32) {}

    /// Player is about to be kicked for being idle. Return false to keep them.
    fn on_idle_kick(&self, _server: &Server, _player: &PlayerInfo) -> bool {
        true
    }

    /// Spawn protection is about to stop `name` building or breaking at `pos`. Return false to let them.
    fn on_spawn_protection(&self, _server: &Server, _name: &str, _pos: (i32, i32, i32)) -> bool {
        true
    }

    /// A hopper moved an item from `from` to `to`. Fired after the tick.
    fn on_hopper_move(&self, _server: &Server, _from: (i32, i32, i32), _to: (i32, i32, i32)) {}

    /// The jukebox at `pos` started playing `record` (like "record.cat").
    fn on_jukebox_play(&self, _server: &Server, _pos: (i32, i32, i32), _record: &str, _player: Option<i64>) {}

    /// Player clicked the note block at `pos`, tuning it up a note.
    fn on_note_tune(&self, _server: &Server, _pos: (i32, i32, i32), _player: Option<i64>) {}

    /// The beehive or nest at `pos` was sheared for honeycomb.
    fn on_honey_harvest(&self, _server: &Server, _pos: (i32, i32, i32), _player: Option<i64>) {}

    /// The pumpkin at `pos` was carved with shears.
    fn on_pumpkin_carve(&self, _server: &Server, _pos: (i32, i32, i32), _player: Option<i64>) {}

    /// Berries were picked from the bush or cave vines at `pos`.
    fn on_berry_pick(&self, _server: &Server, _pos: (i32, i32, i32), _player: Option<i64>) {}

    /// Player is about to use a spawn egg for `kind` at `pos`. Return false to stop it.
    fn on_spawn_egg_use(&self, _server: &Server, _player: &PlayerInfo, _kind: &str, _pos: (f32, f32, f32)) -> bool {
        true
    }

    /// Player `player` fed `mob` its breeding food, so it's ready to breed. Fired after the mob tick.
    fn on_mob_feed(&self, _server: &Server, _player: i64, _mob: i64) {}

    /// Player `player` fed the baby `mob`, making it grow faster. Fired after the mob tick.
    fn on_baby_feed(&self, _server: &Server, _player: i64, _mob: i64) {}

    /// Player `player` put a saddle on `mob`. Fired after the mob tick.
    fn on_mob_saddle(&self, _server: &Server, _player: i64, _mob: i64) {}

    /// Player is about to milk cow `mob` with a bucket. Return false to stop it.
    fn on_cow_milk(&self, _server: &Server, _player: &PlayerInfo, _mob: i64) -> bool {
        true
    }

    /// Player `player` lit creeper `mob` with flint and steel. Fired after the mob tick.
    fn on_creeper_ignite(&self, _server: &Server, _player: i64, _mob: i64) {}

    /// Player `player` gave zombie villager `mob` a golden apple and the cure started. Fired after the mob tick.
    fn on_cure_start(&self, _server: &Server, _player: i64, _mob: i64) {}

    /// Player is about to turn an empty map into a map of the area. Return false to stop it.
    fn on_map_create(&self, _server: &Server, _player: &PlayerInfo) -> bool {
        true
    }

    /// An NPC dialog from `npc` (its name) is about to open for the player. Return false to stop it.
    fn on_dialog_open(&self, _server: &Server, _player: &PlayerInfo, _npc: &str) -> bool {
        true
    }

    /// Player answered NPC dialog `scene` with `button` (None when closed).
    fn on_dialog_answer(&self, _server: &Server, _player: &PlayerInfo, _scene: &str, _button: Option<usize>) {}

    /// Player `killer` killed player `victim`.
    fn on_player_kill(&self, _server: &Server, _killer: &PlayerInfo, _victim: &PlayerInfo) {}

    /// `sender` (a player's name, or CONSOLE) is about to run /say with `args`. Return false to stop it.
    fn on_say_command(&self, _server: &Server, _sender: &str, _args: &str) -> bool {
        true
    }

    /// `sender` (a player's name, or CONSOLE) is about to run /me with `args`. Return false to stop it.
    fn on_me_command(&self, _server: &Server, _sender: &str, _args: &str) -> bool {
        true
    }

    /// `sender` (a player's name, or CONSOLE) is about to run /tell, /msg and /w with `args`. Return false to stop it.
    fn on_whisper_command(&self, _server: &Server, _sender: &str, _args: &str) -> bool {
        true
    }

    /// `sender` (a player's name, or CONSOLE) is about to run /give with `args`. Return false to stop it.
    fn on_give_command(&self, _server: &Server, _sender: &str, _args: &str) -> bool {
        true
    }

    /// `sender` (a player's name, or CONSOLE) is about to run /summon with `args`. Return false to stop it.
    fn on_summon_command(&self, _server: &Server, _sender: &str, _args: &str) -> bool {
        true
    }

    /// `sender` (a player's name, or CONSOLE) is about to run /setblock with `args`. Return false to stop it.
    fn on_setblock_command(&self, _server: &Server, _sender: &str, _args: &str) -> bool {
        true
    }

    /// `sender` (a player's name, or CONSOLE) is about to run /fill with `args`. Return false to stop it.
    fn on_fill_command(&self, _server: &Server, _sender: &str, _args: &str) -> bool {
        true
    }

    /// `sender` (a player's name, or CONSOLE) is about to run /kill with `args`. Return false to stop it.
    fn on_kill_command(&self, _server: &Server, _sender: &str, _args: &str) -> bool {
        true
    }

    /// `sender` (a player's name, or CONSOLE) is about to run /enchant with `args`. Return false to stop it.
    fn on_enchant_command(&self, _server: &Server, _sender: &str, _args: &str) -> bool {
        true
    }

    /// `sender` (a player's name, or CONSOLE) is about to run /clear with `args`. Return false to stop it.
    fn on_clear_command(&self, _server: &Server, _sender: &str, _args: &str) -> bool {
        true
    }

    /// `sender` (a player's name, or CONSOLE) is about to run /setworldspawn with `args`. Return false to stop it.
    fn on_setworldspawn_command(&self, _server: &Server, _sender: &str, _args: &str) -> bool {
        true
    }

    /// `sender` (a player's name, or CONSOLE) is about to run /title and /titleraw with `args`. Return false to stop it.
    fn on_title_command(&self, _server: &Server, _sender: &str, _args: &str) -> bool {
        true
    }

    /// `sender` (a player's name, or CONSOLE) is about to run /tellraw with `args`. Return false to stop it.
    fn on_tellraw_command(&self, _server: &Server, _sender: &str, _args: &str) -> bool {
        true
    }

    /// `sender` (a player's name, or CONSOLE) is about to run /scoreboard with `args`. Return false to stop it.
    fn on_scoreboard_command(&self, _server: &Server, _sender: &str, _args: &str) -> bool {
        true
    }

    /// `sender` (a player's name, or CONSOLE) is about to run /effect with `args`. Return false to stop it.
    fn on_effect_command(&self, _server: &Server, _sender: &str, _args: &str) -> bool {
        true
    }

    /// Change how much damage the player takes (`cause` like "fall", "lava" or a mob's name).
    /// Runs after `on_player_damage` allowed the hit. Set `amount` to 0 to make it harmless.
    fn on_damage_modify(&self, _server: &Server, _player: &PlayerInfo, _cause: &str, _amount: &mut f32) {}

    /// Change how much damage mob `mob` takes (`attacker` is the entity id of who hit it, if anyone).
    fn on_mob_damage_modify(&self, _server: &Server, _mob: i64, _kind: &str, _attacker: Option<i64>, _amount: &mut f32) {}

    /// Change what the `block` the player broke at `pos` drops: (item name, count) pairs. Clear it for no drops.
    fn on_block_drops(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32), _block: &str, _drops: &mut Vec<(String, i8)>) {}

    /// Change what a dying mob of `kind` drops at `pos`: (item name, count) pairs.
    fn on_mob_drops(&self, _server: &Server, _kind: &str, _pos: (f32, f32, f32), _killer: Option<i64>, _drops: &mut Vec<(String, i8)>) {}

    /// Change the XP the player gets for breaking `block` (ores, spawners and the like).
    fn on_block_xp(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32), _block: &str, _xp: &mut i32) {}

    /// Change the XP a mob of `kind` killed by a player drops at `pos`.
    fn on_mob_xp(&self, _server: &Server, _kind: &str, _pos: (f32, f32, f32), _killer: Option<i64>, _xp: &mut i32) {}

    /// Change the knockback the player gets from a hit, as (x, y, z) motion. Zero it for none.
    fn on_knockback(&self, _server: &Server, _player: &PlayerInfo, _knock: &mut (f32, f32, f32)) {}

    /// A packet is about to go to the player (`packet` is its name, like "TextPacket"). Return
    /// false to hold it back. Only called when a plugin is loaded.
    fn on_packet_send(&self, _server: &Server, _player: &PlayerInfo, _packet: &str) -> bool {
        true
    }

    /// The server is checking whether `player` has permission `node`. `allowed` holds the
    /// answer from permissions.txt and plugin defaults; change it to decide yourself.
    fn on_permission_check(&self, _server: &Server, _player: &str, _node: &str, _allowed: &mut bool) {}

    /// Plugin `name` was loaded (every plugin hears about every plugin, itself included), at startup.
    fn on_plugin_enable(&self, _server: &Server, _name: &str) {}

    /// Player is writing `text` on page `page` of a book and quill (or deleting or moving pages,
    /// with empty `text`). Return false to undo it.
    fn on_book_edit(&self, _server: &Server, _player: &PlayerInfo, _page: i32, _text: &str) -> bool {
        true
    }

    /// Player is signing their book and quill as `title`. Return false to stop it.
    fn on_book_sign(&self, _server: &Server, _player: &PlayerInfo, _title: &str) -> bool {
        true
    }

    /// Player is putting `item` into the empty item frame at `pos`. Return false to stop it.
    fn on_frame_place(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32), _item: &str) -> bool {
        true
    }

    /// Player is turning the item in the frame at `pos` to `rotation` (0-7, 45 degrees each). Return false to stop it.
    fn on_frame_rotate(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32), _rotation: i32) -> bool {
        true
    }

    /// Player is hitting `item` out of the frame at `pos`. Return false to keep it in.
    fn on_frame_take(&self, _server: &Server, _player: &PlayerInfo, _pos: (i32, i32, i32), _item: &str) -> bool {
        true
    }

    /// Player is placing an armor stand at `pos`. Return false to stop it.
    fn on_armor_stand_place(&self, _server: &Server, _player: &PlayerInfo, _pos: (f32, f32, f32)) -> bool {
        true
    }

    /// Player is putting `item` on armor stand `mob`, or taking something off (`item` ""). Return false to stop it.
    fn on_armor_stand_change(&self, _server: &Server, _player: &PlayerInfo, _mob: i64, _item: &str) -> bool {
        true
    }

    /// Armor stand `mob` was broken at `pos` (by `killer`, if anyone), dropping itself and its gear.
    fn on_armor_stand_break(&self, _server: &Server, _mob: i64, _pos: (f32, f32, f32), _killer: Option<i64>) {}

    /// Player jumped.
    fn on_jump(&self, _server: &Server, _player: &PlayerInfo) {}

    /// Player started (`true`) or stopped flying.
    fn on_toggle_flight(&self, _server: &Server, _player: &PlayerInfo, _flying: bool) {}

    /// Player swung their arm (or another animation, by Bedrock action id). Return false to hide it from others.
    fn on_animation(&self, _server: &Server, _player: &PlayerInfo, _action: i32) -> bool {
        true
    }

    /// Player answered (or closed, `answer` None) form `form_id`, raw JSON as the client sent it.
    fn on_form_response(&self, _server: &Server, _player: &PlayerInfo, _form_id: u32, _answer: Option<&str>) {}

    /// A packet arrived from the player (`packet` is its name, like "TextPacket"). Return false
    /// to drop it before the server handles it. Only called when a plugin is loaded.
    fn on_packet_receive(&self, _server: &Server, _player: &PlayerInfo, _packet: &str) -> bool {
        true
    }

    /// The world was saved (autosave or shutdown).
    fn on_world_save(&self, _server: &Server) {}

    /// Water or lava is about to flow into `pos`, becoming `block`. Return false to stop it.
    fn on_liquid_flow(&self, _server: &Server, _pos: (i32, i32, i32), _block: &str) -> bool {
        true
    }

    /// Player fired a projectile (arrow, trident, crossbow bolt or rocket) with `force` 0-1.
    fn on_projectile_launch(&self, _server: &Server, _player: &PlayerInfo, _trident: bool, _force: f32) {}

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
    // Rusted ships no gameplay plugins: it's a vanilla server. Add your own here.
    #[cfg(feature = "example")]
    return vec![Box::new(crate::example_plugin::Example)];
    #[cfg(not(feature = "example"))]
    vec![]
}

/// Registered plugins, plus helpers to run an event through all of them.
#[derive(Default)]
pub struct Plugins {
    /// In load order: everything a plugin depends on comes before it.
    list: Vec<Box<dyn Plugin>>,
    /// Indexes into `list` sorted by priority, the order events run in.
    by_priority: Vec<usize>,
    /// Every declared permission and who gets it by default.
    defaults: Vec<(&'static str, Access)>,
}

impl Plugins {
    /// Drops plugins with a missing dependency, orders the rest so dependencies load first.
    pub fn new(plugins: Vec<Box<dyn Plugin>>) -> Plugins {
        let mut pending = plugins;
        loop {
            let names: Vec<String> = pending.iter().map(|p| p.name().to_string()).collect();
            let before = pending.len();
            pending.retain(|p| match p.depends().iter().find(|d| !names.iter().any(|n| n == *d)) {
                Some(missing) => {
                    println!("Plugin {} needs {missing}, which isn't loaded. Skipping it.", p.name());
                    false
                }
                None => true,
            });
            if pending.len() == before {
                break;
            }
        }
        let mut list: Vec<Box<dyn Plugin>> = Vec::new();
        while !pending.is_empty() {
            let waits_on = |p: &dyn Plugin, other: &dyn Plugin| {
                other.name() != p.name() && p.depends().iter().chain(p.soft_depends()).any(|d| *d == other.name())
            };
            let i = (0..pending.len())
                .find(|&i| !pending.iter().any(|q| waits_on(pending[i].as_ref(), q.as_ref())))
                .unwrap_or_else(|| {
                    println!("Plugin {} is in a dependency loop, loading it anyway.", pending[0].name());
                    0
                });
            list.push(pending.remove(i));
        }
        let mut by_priority: Vec<usize> = (0..list.len()).collect();
        by_priority.sort_by_key(|&i| list[i].priority());
        let defaults = list.iter().flat_map(|p| p.permissions()).collect();
        Plugins { list, by_priority, defaults }
    }

    /// Event order. Monitor plugins come last.
    fn ordered(&self) -> impl Iterator<Item = &dyn Plugin> {
        self.by_priority.iter().map(|&i| self.list[i].as_ref())
    }

    /// Runs a cancellable event: stops at the first plugin that cancels, then lets Monitor
    /// plugins see it if it went through. Monitor results are ignored.
    fn cancellable(&self, mut f: impl FnMut(&dyn Plugin) -> bool) -> bool {
        let (monitors, rest): (Vec<&dyn Plugin>, Vec<&dyn Plugin>) =
            self.ordered().partition(|p| p.priority() == Priority::Monitor);
        let allowed = rest.into_iter().all(&mut f);
        if allowed {
            for p in monitors {
                f(p);
            }
        }
        allowed
    }

    fn each(&self, f: impl FnMut(&dyn Plugin)) {
        self.ordered().for_each(f);
    }

    /// Another loaded plugin, by name.
    pub fn get(&self, name: &str) -> Option<&dyn Plugin> {
        self.list.iter().find(|p| p.name() == name).map(|p| p.as_ref())
    }

    pub fn join(&self, s: &Server, p: &PlayerInfo) {
        self.each(|pl| pl.on_join(s, p));
    }

    pub fn leave(&self, s: &Server, p: &PlayerInfo) {
        self.each(|pl| pl.on_leave(s, p));
    }

    /// False if any plugin blocked it. Each plugin can rewrite the message in turn.
    pub fn chat(&self, s: &Server, p: &PlayerInfo, message: &mut String) -> bool {
        self.cancellable(|pl| pl.on_chat(s, p, message))
    }

    pub fn block_break(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32), block: i32) -> bool {
        self.cancellable(|pl| pl.on_block_break(s, p, pos, block))
    }

    pub fn block_place(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32), block: i32) -> bool {
        self.cancellable(|pl| pl.on_block_place(s, p, pos, block))
    }

    /// In load order, so a plugin's dependencies start before it.
    pub fn tick(&self, s: &Server) {
        self.list.iter().for_each(|pl| pl.on_tick(s));
    }

    pub fn server_start(&self, s: &Server) {
        for loaded in &self.list {
            self.list.iter().for_each(|pl| pl.on_plugin_enable(s, loaded.name()));
        }
        self.list.iter().for_each(|pl| pl.on_server_start(s));
    }

    /// Reverse load order, so nothing stops before the plugins that use it.
    pub fn server_stop(&self, s: &Server) {
        self.list.iter().rev().for_each(|pl| pl.on_server_stop(s));
    }

    pub fn command_preprocess(&self, s: &Server, p: &PlayerInfo, line: &mut String) -> bool {
        self.cancellable(|pl| pl.on_command_preprocess(s, p, line))
    }

    pub fn player_damage(&self, s: &Server, p: &PlayerInfo, cause: &str, amount: f32) -> bool {
        self.cancellable(|pl| pl.on_player_damage(s, p, cause, amount))
    }

    pub fn player_death(&self, s: &Server, p: &PlayerInfo, message: &mut String) {
        self.each(|pl| pl.on_player_death(s, p, message));
    }

    pub fn player_respawn(&self, s: &Server, p: &PlayerInfo) {
        self.each(|pl| pl.on_player_respawn(s, p));
    }

    pub fn player_interact(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32), item: &str) -> bool {
        self.cancellable(|pl| pl.on_player_interact(s, p, pos, item))
    }

    pub fn gamemode_change(&self, s: &Server, p: &PlayerInfo, mode: &str) -> bool {
        self.cancellable(|pl| pl.on_gamemode_change(s, p, mode))
    }

    pub fn item_drop(&self, s: &Server, p: &PlayerInfo, item: &str, count: i8) -> bool {
        self.cancellable(|pl| pl.on_item_drop(s, p, item, count))
    }

    pub fn entity_click(&self, s: &Server, p: &PlayerInfo, entity: i64, attack: bool) {
        self.each(|pl| pl.on_entity_click(s, p, entity, attack));
    }

    pub fn mob_damage(&self, s: &Server, mob: i64, kind: &str, damage: f32, from: (f32, f32, f32)) -> bool {
        self.cancellable(|pl| pl.on_mob_damage(s, mob, kind, damage, from))
    }

    pub fn inventory_action(&self, s: &Server, p: &PlayerInfo, action: &str, container: &str) -> bool {
        self.cancellable(|pl| pl.on_inventory_action(s, p, action, container))
    }

    pub fn chunk_load(&self, s: &Server, p: &PlayerInfo, chunk: (i32, i32)) {
        self.each(|pl| pl.on_chunk_load(s, p, chunk));
    }

    pub fn entity_damage(&self, s: &Server, p: &PlayerInfo, mob: i64, kind: &str, damage: f32) -> bool {
        self.cancellable(|pl| pl.on_entity_damage(s, p, mob, kind, damage))
    }

    pub fn entity_death(&self, s: &Server, kind: &str, pos: (f32, f32, f32), killer: Option<i64>) {
        self.each(|pl| pl.on_entity_death(s, kind, pos, killer));
    }

    pub fn entity_spawn(&self, s: &Server, kind: &str, pos: (f32, f32, f32)) -> bool {
        self.cancellable(|pl| pl.on_entity_spawn(s, kind, pos))
    }

    pub fn explosion(&self, s: &Server, pos: (f32, f32, f32), power: f32) -> bool {
        self.cancellable(|pl| pl.on_explosion(s, pos, power))
    }

    pub fn weather_change(&self, s: &Server, weather: &str) -> bool {
        self.cancellable(|pl| pl.on_weather_change(s, weather))
    }

    pub fn item_use(&self, s: &Server, p: &PlayerInfo, item: &crate::inventory::Stack) -> bool {
        self.cancellable(|pl| pl.on_item_use(s, p, item))
    }

    pub fn container_open(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32), block: &str) -> bool {
        self.cancellable(|pl| pl.on_container_open(s, p, pos, block))
    }

    pub fn consume(&self, s: &Server, p: &PlayerInfo, item: &str) -> bool {
        self.cancellable(|pl| pl.on_consume(s, p, item))
    }

    pub fn held_change(&self, s: &Server, p: &PlayerInfo, slot: u32, item: Option<&str>) {
        self.each(|pl| pl.on_held_change(s, p, slot, item));
    }

    pub fn dimension_change(&self, s: &Server, p: &PlayerInfo, to: u8) {
        self.each(|pl| pl.on_dimension_change(s, p, to));
    }

    pub fn teleport(&self, s: &Server, p: &PlayerInfo, to: (f32, f32, f32)) {
        self.each(|pl| pl.on_teleport(s, p, to));
    }

    pub fn xp_change(&self, s: &Server, p: &PlayerInfo, points: &mut i32) {
        self.each(|pl| pl.on_xp_change(s, p, points));
    }

    pub fn sign_change(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32), front: &mut String, back: &mut String) -> bool {
        self.cancellable(|pl| pl.on_sign_change(s, p, pos, front, back))
    }

    pub fn bed_enter(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32)) -> bool {
        self.cancellable(|pl| pl.on_bed_enter(s, p, pos))
    }

    pub fn bed_leave(&self, s: &Server, p: &PlayerInfo) {
        self.each(|pl| pl.on_bed_leave(s, p));
    }

    pub fn bucket_fill(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32), filled: &str) -> bool {
        self.cancellable(|pl| pl.on_bucket_fill(s, p, pos, filled))
    }

    pub fn bucket_empty(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32), bucket: &str) -> bool {
        self.cancellable(|pl| pl.on_bucket_empty(s, p, pos, bucket))
    }

    pub fn fish(&self, s: &Server, p: &PlayerInfo, item: &str) -> bool {
        self.cancellable(|pl| pl.on_fish(s, p, item))
    }

    pub fn glide(&self, s: &Server, p: &PlayerInfo, on: bool) {
        self.each(|pl| pl.on_glide(s, p, on));
    }

    pub fn has_any(&self) -> bool {
        !self.list.is_empty()
    }

    pub fn table_craft(&self, s: &Server, p: &PlayerInfo, table: &str, item: &str, levels: i32) {
        self.each(|pl| pl.on_table_craft(s, p, table, item, levels));
    }

    pub fn brew(&self, s: &Server, pos: (i32, i32, i32)) {
        self.each(|pl| pl.on_brew(s, pos));
    }

    pub fn health_regain(&self, s: &Server, p: &PlayerInfo, health: f32) -> bool {
        self.cancellable(|pl| pl.on_health_regain(s, p, health))
    }

    pub fn food_change(&self, s: &Server, p: &PlayerInfo, food: i32) {
        self.each(|pl| pl.on_food_change(s, p, food));
    }

    /// Fires the events queued with `later` (from code that holds locks or has no server).
    pub fn fire_later(&self, s: &Server) {
        let queued = std::mem::take(&mut *LATER.lock());
        if !self.has_any() {
            return;
        }
        for e in queued {
            match e {
                Later::Combust(id, kind) => self.each(|pl| pl.on_entity_combust(s, id, kind)),
                Later::Despawn(id, kind) => self.each(|pl| pl.on_entity_despawn(s, id, kind)),
                Later::Piston(pos, ext) => self.each(|pl| pl.on_piston(s, pos, ext)),
                Later::Transform(id, from, to, new) => self.each(|pl| pl.on_entity_transform(s, id, from, to, new)),
                Later::MobShoot(id, kind) => self.each(|pl| pl.on_mob_shoot(s, id, kind)),
                Later::RaidWave(c, w) => self.each(|pl| pl.on_raid_wave(s, c, w)),
                Later::RaidEnd(c, v) => self.each(|pl| pl.on_raid_end(s, c, v)),
                Later::Shear(p, m) => self.each(|pl| pl.on_sheep_shear(s, p, m)),
                Later::Regrow(m) => self.each(|pl| pl.on_sheep_regrow(s, m)),
                Later::Leash(p, m, on) => self.each(|pl| pl.on_leash(s, p, m, on)),
                Later::CreeperPower(m) => self.each(|pl| pl.on_creeper_power(s, m)),
                Later::Smelt(pos) => self.each(|pl| pl.on_furnace_smelt(s, pos)),
                Later::TrialSpawn(pos, kind, mob) => self.each(|pl| pl.on_trial_spawn(s, pos, kind, mob)),
                Later::DragonPerch(mob) => self.each(|pl| pl.on_dragon_perch(s, mob)),
                Later::Heal(mob, n) => self.each(|pl| pl.on_mob_heal(s, mob, n)),
                Later::ChunkGen(c) => self.each(|pl| pl.on_chunk_generate(s, c)),
                Later::Sit(mob, on) => self.each(|pl| pl.on_sit_toggle(s, mob, on)),
                Later::TntCart(mob) => self.each(|pl| pl.on_tnt_minecart_prime(s, mob)),
                Later::Expire(mob, kind) => self.each(|pl| pl.on_mob_expire(s, mob, kind)),
                Later::TargetHit(pos, signal) => self.each(|pl| pl.on_target_hit(s, pos, signal)),
                Later::Restock(v) => self.each(|pl| pl.on_villager_restock(s, v)),
                Later::Hopper(from, to) => self.each(|pl| pl.on_hopper_move(s, from, to)),
                Later::Feed(p, m) => self.each(|pl| pl.on_mob_feed(s, p, m)),
                Later::BabyFeed(p, m) => self.each(|pl| pl.on_baby_feed(s, p, m)),
                Later::Saddle(p, m) => self.each(|pl| pl.on_mob_saddle(s, p, m)),
                Later::CreeperIgnite(p, m) => self.each(|pl| pl.on_creeper_ignite(s, p, m)),
                Later::CureStart(p, m) => self.each(|pl| pl.on_cure_start(s, p, m)),
            }
        }
    }

    /// A random-tick change, as the matching event: leaves decaying, grass spreading, snow or
    /// ice forming or melting, else `on_block_grow`. False if a plugin cancelled it.
    pub fn block_change(&self, s: &Server, pos: (i32, i32, i32), from: &str, to: &str) -> bool {
        let frozen = |n: &str| n.ends_with("snow_layer") || n.ends_with(":ice") || n.ends_with(":snow");
        if from.ends_with("leaves") && to == "minecraft:air" {
            self.cancellable(|pl| pl.on_leaves_decay(s, pos, from))
        } else if from == "minecraft:dirt" && matches!(to, "minecraft:grass_block" | "minecraft:mycelium") {
            self.cancellable(|pl| pl.on_block_spread(s, pos, from, to))
        } else if frozen(to) && !frozen(from) {
            self.cancellable(|pl| pl.on_block_form(s, pos, to))
        } else if frozen(from) && !frozen(to) {
            self.cancellable(|pl| pl.on_block_fade(s, pos, from, to))
        } else {
            self.block_grow(s, pos, from, to)
        }
    }

    pub fn block_form(&self, s: &Server, pos: (i32, i32, i32), block: &str) -> bool {
        self.cancellable(|pl| pl.on_block_form(s, pos, block))
    }

    pub fn block_toggle(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32), block: &str, on: bool) -> bool {
        self.cancellable(|pl| pl.on_block_toggle(s, p, pos, block, on))
    }

    pub fn vehicle_place(&self, s: &Server, p: &PlayerInfo, kind: &str, pos: (f32, f32, f32)) -> bool {
        self.cancellable(|pl| pl.on_vehicle_place(s, p, kind, pos))
    }

    pub fn vehicle_destroy(&self, s: &Server, kind: &str, pos: (f32, f32, f32), killer: Option<i64>) {
        self.each(|pl| pl.on_vehicle_destroy(s, kind, pos, killer));
    }

    pub fn emote(&self, s: &Server, p: &PlayerInfo, emote: &str) -> bool {
        self.cancellable(|pl| pl.on_emote(s, p, emote))
    }

    pub fn skin_change(&self, s: &Server, p: &PlayerInfo, skin: &str) {
        self.each(|pl| pl.on_skin_change(s, p, skin));
    }

    pub fn lightning(&self, s: &Server, pos: (f32, f32, f32)) -> bool {
        self.cancellable(|pl| pl.on_lightning(s, pos))
    }

    pub fn item_despawn(&self, s: &Server, item: &str, count: i8, pos: (f32, f32, f32)) {
        self.each(|pl| pl.on_item_despawn(s, item, count, pos));
    }

    pub fn chunk_unload(&self, s: &Server, p: &PlayerInfo, chunk: (i32, i32)) {
        self.each(|pl| pl.on_chunk_unload(s, p, chunk));
    }

    pub fn toggle_crawl(&self, s: &Server, p: &PlayerInfo, on: bool) {
        self.each(|pl| pl.on_toggle_crawl(s, p, on));
    }

    pub fn player_spawn(&self, s: &Server, p: &PlayerInfo) {
        self.each(|pl| pl.on_player_spawn(s, p));
    }

    pub fn map_request(&self, s: &Server, p: &PlayerInfo, map: i64) -> bool {
        self.cancellable(|pl| pl.on_map_request(s, p, map))
    }

    pub fn block_pick(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32), block: &str) -> bool {
        self.cancellable(|pl| pl.on_block_pick(s, p, pos, block))
    }

    pub fn lectern_page(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32), page: i32) {
        self.each(|pl| pl.on_lectern_page(s, p, pos, page));
    }

    pub fn beacon_change(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32), primary: i32, secondary: i32) -> bool {
        self.cancellable(|pl| pl.on_beacon_change(s, p, pos, primary, secondary))
    }

    pub fn farmland_trample(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32)) -> bool {
        self.cancellable(|pl| pl.on_farmland_trample(s, p, pos))
    }

    pub fn crafter_craft(&self, s: &Server, pos: (i32, i32, i32), item: &str, count: i8) -> bool {
        self.cancellable(|pl| pl.on_crafter_craft(s, pos, item, count))
    }

    pub fn dispense(&self, s: &Server, pos: (i32, i32, i32), block: &str) -> bool {
        self.cancellable(|pl| pl.on_dispense(s, pos, block))
    }

    pub fn note_play(&self, s: &Server, pos: (i32, i32, i32)) -> bool {
        self.cancellable(|pl| pl.on_note_play(s, pos))
    }

    pub fn bell_ring(&self, s: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.each(|pl| pl.on_bell_ring(s, pos, player));
    }

    pub fn raid_start(&self, s: &Server, center: (f32, f32, f32)) -> bool {
        self.cancellable(|pl| pl.on_raid_start(s, center))
    }

    pub fn totem_use(&self, s: &Server, p: &PlayerInfo) -> bool {
        self.cancellable(|pl| pl.on_totem_use(s, p))
    }

    pub fn villager_level_up(&self, s: &Server, villager: i64, tier: i32) {
        self.each(|pl| pl.on_villager_level_up(s, villager, tier));
    }

    pub fn spawner_spawn(&self, s: &Server, pos: (i32, i32, i32), kind: &str) -> bool {
        self.cancellable(|pl| pl.on_spawner_spawn(s, pos, kind))
    }

    pub fn rcon_command(&self, s: &Server, addr: &str, line: &str) -> bool {
        self.cancellable(|pl| pl.on_rcon_command(s, addr, line))
    }

    pub fn console_command(&self, s: &Server, line: &str) -> bool {
        self.cancellable(|pl| pl.on_console_command(s, line))
    }

    pub fn tnt_prime(&self, s: &Server, pos: (i32, i32, i32)) -> bool {
        self.cancellable(|pl| pl.on_tnt_prime(s, pos))
    }

    pub fn effect_add(&self, s: &Server, p: &PlayerInfo, effect: i32, amplifier: i32, ticks: i32) -> bool {
        self.cancellable(|pl| pl.on_effect_add(s, p, effect, amplifier, ticks))
    }

    pub fn effect_remove(&self, s: &Server, p: &PlayerInfo, effect: i32) {
        self.each(|pl| pl.on_effect_remove(s, p, effect));
    }

    pub fn ender_pearl(&self, s: &Server, p: &PlayerInfo, to: (f32, f32, f32)) -> bool {
        self.cancellable(|pl| pl.on_ender_pearl(s, p, to))
    }

    pub fn item_spawn(&self, s: &Server, item: &str, count: i8, pos: (f32, f32, f32)) -> bool {
        self.cancellable(|pl| pl.on_item_spawn(s, item, count, pos))
    }

    pub fn xp_orb_pickup(&self, s: &Server, p: &PlayerInfo, value: i32) {
        self.each(|pl| pl.on_xp_orb_pickup(s, p, value));
    }

    pub fn golem_build(&self, s: &Server, kind: &str, pos: (f32, f32, f32)) -> bool {
        self.cancellable(|pl| pl.on_golem_build(s, kind, pos))
    }

    pub fn trader_spawn(&self, s: &Server, pos: (f32, f32, f32)) -> bool {
        self.cancellable(|pl| pl.on_trader_spawn(s, pos))
    }

    pub fn patrol_spawn(&self, s: &Server, pos: (f32, f32, f32)) -> bool {
        self.cancellable(|pl| pl.on_patrol_spawn(s, pos))
    }

    pub fn shriek(&self, s: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.each(|pl| pl.on_shriek(s, pos, player));
    }

    pub fn warden_summon(&self, s: &Server, pos: (f32, f32, f32)) -> bool {
        self.cancellable(|pl| pl.on_warden_summon(s, pos))
    }

    pub fn vault_open(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32)) -> bool {
        self.cancellable(|pl| pl.on_vault_open(s, p, pos))
    }

    pub fn gamerule_change(&self, s: &Server, rule: &str, value: &str) -> bool {
        self.cancellable(|pl| pl.on_gamerule_change(s, rule, value))
    }

    pub fn difficulty_change(&self, s: &Server, d: i32) -> bool {
        self.cancellable(|pl| pl.on_difficulty_change(s, d))
    }

    pub fn op_change(&self, s: &Server, name: &str, op: bool) -> bool {
        self.cancellable(|pl| pl.on_op_change(s, name, op))
    }

    pub fn ban(&self, s: &Server, name: &str, reason: &str) -> bool {
        self.cancellable(|pl| pl.on_ban(s, name, reason))
    }

    pub fn unban(&self, s: &Server, name: &str) -> bool {
        self.cancellable(|pl| pl.on_unban(s, name))
    }

    pub fn ban_ip(&self, s: &Server, ip: &str) -> bool {
        self.cancellable(|pl| pl.on_ban_ip(s, ip))
    }

    pub fn whitelist_change(&self, s: &Server, name: &str, added: bool) -> bool {
        self.cancellable(|pl| pl.on_whitelist_change(s, name, added))
    }

    pub fn world_change(&self, s: &Server, p: &PlayerInfo, world: &str) -> bool {
        self.cancellable(|pl| pl.on_world_change(s, p, world))
    }

    pub fn backup(&self, s: &Server, path: &str) {
        self.each(|pl| pl.on_backup(s, path));
    }

    pub fn night_skip(&self, s: &Server) -> bool {
        self.cancellable(|pl| pl.on_night_skip(s))
    }

    pub fn bonemeal(&self, s: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.each(|pl| pl.on_bonemeal(s, pos, player));
    }

    pub fn anchor_charge(&self, s: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.each(|pl| pl.on_anchor_charge(s, pos, player));
    }

    pub fn xp_level_change(&self, s: &Server, p: &PlayerInfo, old: i32, new: i32) {
        self.each(|pl| pl.on_xp_level_change(s, p, old, new));
    }

    pub fn shield_block(&self, s: &Server, p: &PlayerInfo, damage: f32) -> bool {
        self.cancellable(|pl| pl.on_shield_block(s, p, damage))
    }

    pub fn cake_eat(&self, s: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.each(|pl| pl.on_cake_eat(s, pos, player));
    }

    pub fn dragon_egg_teleport(&self, s: &Server, pos: (i32, i32, i32), to: (i32, i32, i32)) -> bool {
        self.cancellable(|pl| pl.on_dragon_egg_teleport(s, pos, to))
    }

    pub fn end_portal_fill(&self, s: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.each(|pl| pl.on_end_portal_fill(s, pos, player));
    }

    pub fn composter_use(&self, s: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.each(|pl| pl.on_composter_use(s, pos, player));
    }

    pub fn spawn_point_set(&self, s: &Server, p: &PlayerInfo, pos: (f32, f32, f32)) -> bool {
        self.cancellable(|pl| pl.on_spawn_point_set(s, p, pos))
    }

    pub fn view_distance(&self, s: &Server, p: &PlayerInfo, chunks: &mut i32) {
        self.each(|pl| pl.on_view_distance(s, p, chunks));
    }

    pub fn time_set(&self, s: &Server, time: i32) -> bool {
        self.cancellable(|pl| pl.on_time_set(s, time))
    }

    pub fn slow_tick(&self, s: &Server, ms: f32) {
        self.each(|pl| pl.on_slow_tick(s, ms));
    }

    pub fn armor_change(&self, s: &Server, p: &PlayerInfo, slot: i32, item: &str) {
        self.each(|pl| pl.on_armor_change(s, p, slot, item));
    }

    pub fn player_combust(&self, s: &Server, p: &PlayerInfo) {
        self.each(|pl| pl.on_player_combust(s, p));
    }

    pub fn death_drops(&self, s: &Server, p: &PlayerInfo) -> bool {
        self.cancellable(|pl| pl.on_death_drops(s, p))
    }

    pub fn portal_enter(&self, s: &Server, p: &PlayerInfo, to: i32) -> bool {
        self.cancellable(|pl| pl.on_portal_enter(s, p, to))
    }

    pub fn firework_explode(&self, s: &Server, pos: (f32, f32, f32), stars: i32) {
        self.each(|pl| pl.on_firework_explode(s, pos, stars));
    }

    pub fn fishing_cast(&self, s: &Server, p: &PlayerInfo) -> bool {
        self.cancellable(|pl| pl.on_fishing_cast(s, p))
    }

    pub fn elytra_boost(&self, s: &Server, p: &PlayerInfo) -> bool {
        self.cancellable(|pl| pl.on_elytra_boost(s, p))
    }

    pub fn item_throw(&self, s: &Server, p: &PlayerInfo, item: &str) -> bool {
        self.cancellable(|pl| pl.on_item_throw(s, p, item))
    }

    pub fn block_explode(&self, s: &Server, pos: (i32, i32, i32), block: &str) -> bool {
        self.cancellable(|pl| pl.on_block_explode(s, pos, block))
    }

    pub fn sign_open(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32)) {
        self.each(|pl| pl.on_sign_open(s, p, pos));
    }

    pub fn sapling_grow(&self, s: &Server, pos: (i32, i32, i32), sapling: &str) -> bool {
        self.cancellable(|pl| pl.on_sapling_grow(s, pos, sapling))
    }

    pub fn block_ignite(&self, s: &Server, player: &PlayerInfo, pos: (i32, i32, i32)) -> bool {
        self.cancellable(|pl| pl.on_block_ignite(s, player, pos))
    }

    pub fn button_press(&self, s: &Server, player: &PlayerInfo, pos: (i32, i32, i32), block: &str) -> bool {
        self.cancellable(|pl| pl.on_button_press(s, player, pos, block))
    }

    pub fn pressure_plate(&self, s: &Server, pos: (i32, i32, i32), signal: i32) {
        self.each(|pl| pl.on_pressure_plate(s, pos, signal));
    }

    pub fn tripwire_trigger(&self, s: &Server, pos: (i32, i32, i32)) {
        self.each(|pl| pl.on_tripwire_trigger(s, pos));
    }

    pub fn target_hit(&self, s: &Server, pos: (i32, i32, i32), signal: i32) {
        self.each(|pl| pl.on_target_hit(s, pos, signal));
    }

    pub fn sculk_sensor(&self, s: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.each(|pl| pl.on_sculk_sensor(s, pos, player));
    }

    pub fn log_strip(&self, s: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.each(|pl| pl.on_log_strip(s, pos, player));
    }

    pub fn copper_wax(&self, s: &Server, pos: (i32, i32, i32), player: Option<i64>, waxed: bool) {
        self.each(|pl| pl.on_copper_wax(s, pos, player, waxed));
    }

    pub fn copper_scrape(&self, s: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.each(|pl| pl.on_copper_scrape(s, pos, player));
    }

    pub fn path_create(&self, s: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.each(|pl| pl.on_path_create(s, pos, player));
    }

    pub fn till(&self, s: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.each(|pl| pl.on_till(s, pos, player));
    }

    pub fn campfire_extinguish(&self, s: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.each(|pl| pl.on_campfire_extinguish(s, pos, player));
    }

    pub fn villager_restock(&self, s: &Server, villager: i64) {
        self.each(|pl| pl.on_villager_restock(s, villager));
    }

    pub fn pvp_attack(&self, s: &Server, attacker: &PlayerInfo, victim: &PlayerInfo, damage: f32) -> bool {
        self.cancellable(|pl| pl.on_pvp_attack(s, attacker, victim, damage))
    }

    pub fn mending_repair(&self, s: &Server, player: &PlayerInfo, points: i32) {
        self.each(|pl| pl.on_mending_repair(s, player, points));
    }

    pub fn idle_kick(&self, s: &Server, player: &PlayerInfo) -> bool {
        self.cancellable(|pl| pl.on_idle_kick(s, player))
    }

    pub fn spawn_protection(&self, s: &Server, name: &str, pos: (i32, i32, i32)) -> bool {
        self.cancellable(|pl| pl.on_spawn_protection(s, name, pos))
    }

    pub fn hopper_move(&self, s: &Server, from: (i32, i32, i32), to: (i32, i32, i32)) {
        self.each(|pl| pl.on_hopper_move(s, from, to));
    }

    pub fn jukebox_play(&self, s: &Server, pos: (i32, i32, i32), record: &str, player: Option<i64>) {
        self.each(|pl| pl.on_jukebox_play(s, pos, record, player));
    }

    pub fn note_tune(&self, s: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.each(|pl| pl.on_note_tune(s, pos, player));
    }

    pub fn honey_harvest(&self, s: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.each(|pl| pl.on_honey_harvest(s, pos, player));
    }

    pub fn pumpkin_carve(&self, s: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.each(|pl| pl.on_pumpkin_carve(s, pos, player));
    }

    pub fn berry_pick(&self, s: &Server, pos: (i32, i32, i32), player: Option<i64>) {
        self.each(|pl| pl.on_berry_pick(s, pos, player));
    }

    pub fn spawn_egg_use(&self, s: &Server, player: &PlayerInfo, kind: &str, pos: (f32, f32, f32)) -> bool {
        self.cancellable(|pl| pl.on_spawn_egg_use(s, player, kind, pos))
    }

    pub fn mob_feed(&self, s: &Server, player: i64, mob: i64) {
        self.each(|pl| pl.on_mob_feed(s, player, mob));
    }

    pub fn baby_feed(&self, s: &Server, player: i64, mob: i64) {
        self.each(|pl| pl.on_baby_feed(s, player, mob));
    }

    pub fn mob_saddle(&self, s: &Server, player: i64, mob: i64) {
        self.each(|pl| pl.on_mob_saddle(s, player, mob));
    }

    pub fn cow_milk(&self, s: &Server, player: &PlayerInfo, mob: i64) -> bool {
        self.cancellable(|pl| pl.on_cow_milk(s, player, mob))
    }

    pub fn creeper_ignite(&self, s: &Server, player: i64, mob: i64) {
        self.each(|pl| pl.on_creeper_ignite(s, player, mob));
    }

    pub fn cure_start(&self, s: &Server, player: i64, mob: i64) {
        self.each(|pl| pl.on_cure_start(s, player, mob));
    }

    pub fn map_create(&self, s: &Server, player: &PlayerInfo) -> bool {
        self.cancellable(|pl| pl.on_map_create(s, player))
    }

    pub fn dialog_open(&self, s: &Server, player: &PlayerInfo, npc: &str) -> bool {
        self.cancellable(|pl| pl.on_dialog_open(s, player, npc))
    }

    pub fn dialog_answer(&self, s: &Server, player: &PlayerInfo, scene: &str, button: Option<usize>) {
        self.each(|pl| pl.on_dialog_answer(s, player, scene, button));
    }

    pub fn player_kill(&self, s: &Server, killer: &PlayerInfo, victim: &PlayerInfo) {
        self.each(|pl| pl.on_player_kill(s, killer, victim));
    }

    pub fn say_command(&self, s: &Server, sender: &str, args: &str) -> bool {
        self.cancellable(|pl| pl.on_say_command(s, sender, args))
    }

    pub fn me_command(&self, s: &Server, sender: &str, args: &str) -> bool {
        self.cancellable(|pl| pl.on_me_command(s, sender, args))
    }

    pub fn whisper_command(&self, s: &Server, sender: &str, args: &str) -> bool {
        self.cancellable(|pl| pl.on_whisper_command(s, sender, args))
    }

    pub fn give_command(&self, s: &Server, sender: &str, args: &str) -> bool {
        self.cancellable(|pl| pl.on_give_command(s, sender, args))
    }

    pub fn summon_command(&self, s: &Server, sender: &str, args: &str) -> bool {
        self.cancellable(|pl| pl.on_summon_command(s, sender, args))
    }

    pub fn setblock_command(&self, s: &Server, sender: &str, args: &str) -> bool {
        self.cancellable(|pl| pl.on_setblock_command(s, sender, args))
    }

    pub fn fill_command(&self, s: &Server, sender: &str, args: &str) -> bool {
        self.cancellable(|pl| pl.on_fill_command(s, sender, args))
    }

    pub fn kill_command(&self, s: &Server, sender: &str, args: &str) -> bool {
        self.cancellable(|pl| pl.on_kill_command(s, sender, args))
    }

    pub fn enchant_command(&self, s: &Server, sender: &str, args: &str) -> bool {
        self.cancellable(|pl| pl.on_enchant_command(s, sender, args))
    }

    pub fn clear_command(&self, s: &Server, sender: &str, args: &str) -> bool {
        self.cancellable(|pl| pl.on_clear_command(s, sender, args))
    }

    pub fn setworldspawn_command(&self, s: &Server, sender: &str, args: &str) -> bool {
        self.cancellable(|pl| pl.on_setworldspawn_command(s, sender, args))
    }

    pub fn title_command(&self, s: &Server, sender: &str, args: &str) -> bool {
        self.cancellable(|pl| pl.on_title_command(s, sender, args))
    }

    pub fn tellraw_command(&self, s: &Server, sender: &str, args: &str) -> bool {
        self.cancellable(|pl| pl.on_tellraw_command(s, sender, args))
    }

    pub fn scoreboard_command(&self, s: &Server, sender: &str, args: &str) -> bool {
        self.cancellable(|pl| pl.on_scoreboard_command(s, sender, args))
    }

    pub fn effect_command(&self, s: &Server, sender: &str, args: &str) -> bool {
        self.cancellable(|pl| pl.on_effect_command(s, sender, args))
    }

    /// The matching command event for a command line, before it runs. False if a plugin stopped it.
    pub fn command_event(&self, s: &Server, sender: &str, args: &[&str]) -> bool {
        let Some((cmd, rest)) = args.split_first() else { return true };
        let rest = rest.join(" ");
        match cmd.trim_start_matches('/') {
            "say" => self.say_command(s, sender, &rest),
            "me" => self.me_command(s, sender, &rest),
            "give" => self.give_command(s, sender, &rest),
            "summon" => self.summon_command(s, sender, &rest),
            "setblock" => self.setblock_command(s, sender, &rest),
            "fill" => self.fill_command(s, sender, &rest),
            "kill" => self.kill_command(s, sender, &rest),
            "enchant" => self.enchant_command(s, sender, &rest),
            "clear" => self.clear_command(s, sender, &rest),
            "setworldspawn" => self.setworldspawn_command(s, sender, &rest),
            "tellraw" => self.tellraw_command(s, sender, &rest),
            "scoreboard" => self.scoreboard_command(s, sender, &rest),
            "effect" => self.effect_command(s, sender, &rest),
            "tell" | "msg" | "w" => self.whisper_command(s, sender, &rest),
            "title" | "titleraw" => self.title_command(s, sender, &rest),
            _ => true,
        }
    }

    pub fn damage_modify(&self, s: &Server, p: &PlayerInfo, cause: &str, amount: &mut f32) {
        self.each(|pl| pl.on_damage_modify(s, p, cause, amount));
    }

    pub fn mob_damage_modify(&self, s: &Server, mob: i64, kind: &str, attacker: Option<i64>, amount: &mut f32) {
        self.each(|pl| pl.on_mob_damage_modify(s, mob, kind, attacker, amount));
    }

    pub fn block_drops(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32), block: &str, drops: &mut Vec<(String, i8)>) {
        self.each(|pl| pl.on_block_drops(s, p, pos, block, drops));
    }

    pub fn mob_drops(&self, s: &Server, kind: &str, pos: (f32, f32, f32), killer: Option<i64>, drops: &mut Vec<(String, i8)>) {
        self.each(|pl| pl.on_mob_drops(s, kind, pos, killer, drops));
    }

    pub fn block_xp(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32), block: &str, xp: &mut i32) {
        self.each(|pl| pl.on_block_xp(s, p, pos, block, xp));
    }

    pub fn mob_xp(&self, s: &Server, kind: &str, pos: (f32, f32, f32), killer: Option<i64>, xp: &mut i32) {
        self.each(|pl| pl.on_mob_xp(s, kind, pos, killer, xp));
    }

    pub fn knockback(&self, s: &Server, p: &PlayerInfo, knock: &mut (f32, f32, f32)) {
        self.each(|pl| pl.on_knockback(s, p, knock));
    }

    pub fn packet_send(&self, s: &Server, p: &PlayerInfo, packet: &str) -> bool {
        self.cancellable(|pl| pl.on_packet_send(s, p, packet))
    }

    pub fn book_edit(&self, s: &Server, p: &PlayerInfo, page: i32, text: &str) -> bool {
        self.cancellable(|pl| pl.on_book_edit(s, p, page, text))
    }

    pub fn book_sign(&self, s: &Server, p: &PlayerInfo, title: &str) -> bool {
        self.cancellable(|pl| pl.on_book_sign(s, p, title))
    }

    /// `allowed`, then plugins get the last word (`on_permission_check`).
    pub fn allowed_for(&self, s: &Server, player: &str, node: &str) -> bool {
        let mut ok = self.allowed(player, node);
        self.each(|pl| pl.on_permission_check(s, player, node, &mut ok));
        ok
    }

    pub fn frame_place(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32), item: &str) -> bool {
        self.cancellable(|pl| pl.on_frame_place(s, p, pos, item))
    }

    pub fn frame_rotate(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32), rotation: i32) -> bool {
        self.cancellable(|pl| pl.on_frame_rotate(s, p, pos, rotation))
    }

    pub fn frame_take(&self, s: &Server, p: &PlayerInfo, pos: (i32, i32, i32), item: &str) -> bool {
        self.cancellable(|pl| pl.on_frame_take(s, p, pos, item))
    }

    pub fn armor_stand_place(&self, s: &Server, p: &PlayerInfo, pos: (f32, f32, f32)) -> bool {
        self.cancellable(|pl| pl.on_armor_stand_place(s, p, pos))
    }

    pub fn armor_stand_change(&self, s: &Server, p: &PlayerInfo, mob: i64, item: &str) -> bool {
        self.cancellable(|pl| pl.on_armor_stand_change(s, p, mob, item))
    }

    pub fn armor_stand_break(&self, s: &Server, mob: i64, pos: (f32, f32, f32), killer: Option<i64>) {
        self.each(|pl| pl.on_armor_stand_break(s, mob, pos, killer));
    }

    pub fn jump(&self, s: &Server, p: &PlayerInfo) {
        self.each(|pl| pl.on_jump(s, p));
    }

    pub fn toggle_flight(&self, s: &Server, p: &PlayerInfo, on: bool) {
        self.each(|pl| pl.on_toggle_flight(s, p, on));
    }

    pub fn animation(&self, s: &Server, p: &PlayerInfo, action: i32) -> bool {
        self.cancellable(|pl| pl.on_animation(s, p, action))
    }

    pub fn form_response(&self, s: &Server, p: &PlayerInfo, form_id: u32, answer: Option<&str>) {
        self.each(|pl| pl.on_form_response(s, p, form_id, answer));
    }

    pub fn packet_receive(&self, s: &Server, p: &PlayerInfo, packet: &str) -> bool {
        self.cancellable(|pl| pl.on_packet_receive(s, p, packet))
    }

    pub fn world_save(&self, s: &Server) {
        self.each(|pl| pl.on_world_save(s));
    }

    pub fn liquid_flow(&self, s: &Server, pos: (i32, i32, i32), block: &str) -> bool {
        self.cancellable(|pl| pl.on_liquid_flow(s, pos, block))
    }

    pub fn projectile_launch(&self, s: &Server, p: &PlayerInfo, trident: bool, force: f32) {
        self.each(|pl| pl.on_projectile_launch(s, p, trident, force));
    }

    pub fn swim(&self, s: &Server, p: &PlayerInfo, on: bool) {
        self.each(|pl| pl.on_swim(s, p, on));
    }

    pub fn container_close(&self, s: &Server, p: &PlayerInfo, kind: &str) {
        self.each(|pl| pl.on_container_close(s, p, kind));
    }

    pub fn item_break(&self, s: &Server, p: &PlayerInfo, item: &str) {
        self.each(|pl| pl.on_item_break(s, p, item));
    }

    pub fn kick(&self, s: &Server, p: &PlayerInfo, message: &str) {
        self.each(|pl| pl.on_kick(s, p, message));
    }

    pub fn trade(&self, s: &Server, p: &PlayerInfo, villager: i64, index: i32, times: i32) {
        self.each(|pl| pl.on_trade(s, p, villager, index, times));
    }

    pub fn mount(&self, s: &Server, p: &PlayerInfo, mob: i64) {
        self.each(|pl| pl.on_mount(s, p, mob));
    }

    pub fn dismount(&self, s: &Server, p: &PlayerInfo) {
        self.each(|pl| pl.on_dismount(s, p));
    }

    pub fn craft(&self, s: &Server, p: &PlayerInfo, item: &str, count: i32) -> bool {
        self.cancellable(|pl| pl.on_craft(s, p, item, count))
    }

    pub fn projectile_hit(&self, s: &Server, shooter: &PlayerInfo, target: Target) -> bool {
        self.cancellable(|pl| pl.on_projectile_hit(s, shooter, target))
    }

    pub fn block_grow(&self, s: &Server, pos: (i32, i32, i32), from: &str, to: &str) -> bool {
        self.cancellable(|pl| pl.on_block_grow(s, pos, from, to))
    }

    pub fn mob_target(&self, s: &Server, mob: i64, kind: &str, player: &PlayerInfo) -> bool {
        self.cancellable(|pl| pl.on_mob_target(s, mob, kind, player))
    }

    pub fn block_burn(&self, s: &Server, pos: (i32, i32, i32), block: &str, spread: bool) -> bool {
        self.cancellable(|pl| pl.on_block_burn(s, pos, block, spread))
    }

    pub fn explosion_blocks(&self, s: &Server, at: (f32, f32, f32), blocks: &mut Vec<(i32, i32, i32)>) {
        self.each(|pl| pl.on_explosion_blocks(s, at, blocks));
    }

    pub fn entity_tame(&self, s: &Server, p: &PlayerInfo, mob: i64, kind: &str) -> bool {
        self.cancellable(|pl| pl.on_entity_tame(s, p, mob, kind))
    }

    pub fn entity_breed(&self, s: &Server, kind: &str, parents: (i64, i64)) -> bool {
        self.cancellable(|pl| pl.on_entity_breed(s, kind, parents))
    }

    pub fn portal_create(&self, s: &Server, p: &PlayerInfo, kind: &str) -> bool {
        self.cancellable(|pl| pl.on_portal_create(s, p, kind))
    }

    /// The first refusal message any plugin gives, if one does.
    pub fn pre_login(&self, s: &Server, name: &str, xuid: &str) -> Option<String> {
        self.ordered().filter(|p| p.priority() != Priority::Monitor).find_map(|pl| pl.on_pre_login(s, name, xuid))
    }

    pub fn moved(&self, s: &Server, p: &PlayerInfo, to: (f32, f32, f32)) -> bool {
        self.cancellable(|pl| pl.on_move(s, p, to))
    }

    pub fn item_pickup(&self, s: &Server, p: &PlayerInfo, item: &str, count: i8) -> bool {
        self.cancellable(|pl| pl.on_item_pickup(s, p, item, count))
    }

    pub fn sneak(&self, s: &Server, p: &PlayerInfo, on: bool) {
        self.each(|pl| pl.on_sneak(s, p, on));
    }

    pub fn sprint(&self, s: &Server, p: &PlayerInfo, on: bool) {
        self.each(|pl| pl.on_sprint(s, p, on));
    }

    pub fn names(&self) -> Vec<String> {
        self.list.iter().map(|p| p.name().to_string()).collect()
    }

    /// `name v1.0.0` for `/pl`.
    pub fn summaries(&self) -> Vec<String> {
        self.list.iter().map(|p| format!("{} v{}", p.name(), p.version())).collect()
    }

    pub fn commands(&self) -> Vec<(&'static str, &'static str)> {
        self.list.iter().flat_map(|pl| pl.commands()).collect()
    }

    /// Usage lines of whichever plugin owns `name`.
    pub fn usage(&self, name: &str) -> &'static [&'static str] {
        self.list.iter().find(|pl| pl.commands().iter().any(|(n, _)| *n == name)).map_or(&[], |pl| pl.usage(name))
    }

    /// First plugin that owns `name` handles it, if the player has its permission.
    pub fn command(&self, s: &Server, p: &PlayerInfo, name: &str, args: &[&str]) -> Option<String> {
        let owner = self.list.iter().find(|pl| pl.commands().iter().any(|(n, _)| *n == name))?;
        if let Some(node) = owner.command_permission(name)
            && !self.allowed_for(s, &p.name, node)
        {
            return Some("§cYou do not have permission to use this command".into());
        }
        // A selector (@a, @p, @r, @s, @e, @initiator) runs the command once per player it finds.
        let Some(i) = args.iter().position(|a| a.starts_with('@')) else { return owner.on_command(s, p, name, args) };
        let ids = match crate::commands::select(s, p.id, args[i]) {
            Ok(ids) if !ids.is_empty() => ids,
            Ok(_) => return Some("§cNo targets matched selector".into()),
            Err(e) => return Some(e),
        };
        let mut out: Vec<String> = Vec::new();
        for who in ids.into_iter().filter_map(|id| s.get(id)) {
            let mut each = args.to_vec();
            each[i] = &who.name;
            if let Some(reply) = owner.on_command(s, p, name, &each).filter(|r| !r.is_empty()) {
                out.push(reply);
            }
        }
        Some(out.join("
"))
    }

    /// Does `player` have `node`? `permissions.txt` wins, then the plugin's default for it.
    /// Nodes nobody declared are ops-only.
    pub fn allowed(&self, player: &str, node: &str) -> bool {
        if let Some(set) = permissions().get(player, node) {
            return set;
        }
        match self.defaults.iter().find(|(n, _)| *n == node).map_or(Access::Ops, |(_, a)| *a) {
            Access::Everyone => true,
            Access::Ops => crate::commands::is_op(player),
            Access::Nobody => false,
        }
    }
}

/// `permissions.txt`: one `<player> <node>` per line, `-node` to take it away, `plugin.*` for
/// every node under `plugin.`, `*` for everything. Re-read when the file changes.
pub struct PermissionFile {
    path: std::path::PathBuf,
    cache: parking_lot::Mutex<(Option<std::time::SystemTime>, Vec<(String, String, bool)>)>,
}

impl PermissionFile {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        PermissionFile { path: path.into(), cache: Default::default() }
    }

    fn rules(&self) -> Vec<(String, String, bool)> {
        let modified = std::fs::metadata(&self.path).and_then(|m| m.modified()).ok();
        let mut cache = self.cache.lock();
        if cache.0 != modified || modified.is_none() {
            let text = std::fs::read_to_string(&self.path).unwrap_or_default();
            cache.1 = text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .filter_map(|l| l.split_once(char::is_whitespace))
                .map(|(who, node)| {
                    let node = node.trim();
                    let (node, grant) = node.strip_prefix('-').map_or((node, true), |n| (n, false));
                    (who.to_lowercase(), node.to_string(), grant)
                })
                .collect();
            cache.0 = modified;
        }
        cache.1.clone()
    }

    /// `Some(true/false)` if the file says anything about this player and node. The most
    /// specific rule wins: `homes.set` beats `homes.*` beats `*`.
    pub fn get(&self, player: &str, node: &str) -> Option<bool> {
        let player = player.to_lowercase();
        self.rules()
            .into_iter()
            .filter(|(who, _, _)| *who == player)
            .filter_map(|(_, rule, grant)| {
                let hit = rule == node
                    || rule == "*"
                    || rule.strip_suffix(".*").is_some_and(|base| node.len() > base.len() && node.starts_with(base) && node[base.len()..].starts_with('.'));
                hit.then_some((rule.len(), grant))
            })
            .max_by_key(|(len, _)| *len)
            .map(|(_, grant)| grant)
    }
}

pub fn permissions() -> &'static PermissionFile {
    static P: std::sync::OnceLock<PermissionFile> = std::sync::OnceLock::new();
    P.get_or_init(|| PermissionFile::new("permissions.txt"))
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

    /// Run slow `work` (a web request, a database, a big file) on its own thread, then `done`
    /// with its result back on the tick, where touching the world is safe.
    pub fn run_async<T: Send + 'static>(
        self: &std::sync::Arc<Self>,
        work: impl FnOnce() -> T + Send + 'static,
        done: impl FnOnce(&Server, T) + Send + 'static,
    ) {
        let me = self.clone();
        std::thread::spawn(move || {
            let result = work();
            me.run_later(1, move |s| done(s, result));
        });
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

/// Keeps a name usable as a file name: no slashes or dots, so it can't leave its folder.
fn safe_name(name: &str) -> String {
    name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect()
}

/// Plugin settings in plugins/<name>/config.txt as `key = value` lines (like PNX Config).
/// Missing keys fall back to the default you pass in.
pub struct Config {
    path: std::path::PathBuf,
    values: Vec<(String, String)>,
}

impl Config {
    pub fn load(plugin: &str) -> Config {
        Config::at(std::path::Path::new("plugins").join(safe_name(plugin)).join("config.txt"))
    }

    /// This plugin's saved data for one player, in plugins/<name>/players/<player>.txt. Keyed by
    /// Xbox id when there is one, so it survives name changes. Call `save()` after changing it.
    /// Another file in the plugin's folder, e.g. `Config::data("warps", "warps")` is
    /// plugins/warps/warps.txt.
    pub fn data(plugin: &str, file: &str) -> Config {
        Config::at(std::path::Path::new("plugins").join(safe_name(plugin)).join(format!("{}.txt", safe_name(file))))
    }

    pub fn player(plugin: &str, player: &PlayerInfo) -> Config {
        let who = if player.xuid.is_empty() { player.name.to_lowercase() } else { player.xuid.clone() };
        Config::at(std::path::Path::new("plugins").join(safe_name(plugin)).join("players").join(format!("{}.txt", safe_name(&who))))
    }

    fn at(path: std::path::PathBuf) -> Config {
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

    pub fn remove(&mut self, key: &str) {
        self.values.retain(|(k, _)| k != key);
    }

    /// Every (key, value), in file order.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values.iter().map(|(k, v)| (k.as_str(), v.as_str()))
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
    #[test]
    fn later_events_queue_and_clear() {
        super::later(super::Later::Piston((1, 2, 3), true));
        super::later(super::Later::Despawn(7, "minecraft:zombie"));
        let server = crate::players::Server::new();
        server.plugins.fire_later(&server); // no plugins: just drains
        assert!(super::LATER.lock().is_empty());
    }

    #[test]
    fn packet_names_skip_the_contents() {
        #[derive(Debug)]
        #[allow(dead_code)]
        enum P {
            TextPacket(Vec<u8>),
            Plain,
        }
        assert_eq!(super::packet_name(&P::TextPacket(vec![1; 100_000])), "TextPacket");
        assert_eq!(super::packet_name(&P::Plain), "Plain");
    }

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

    struct KeepLeaves;
    impl Plugin for KeepLeaves {
        fn name(&self) -> &str {
            "keepleaves"
        }
        fn on_leaves_decay(&self, _: &Server, _: (i32, i32, i32), _: &str) -> bool {
            false
        }
    }

    struct NoBoom;
    impl Plugin for NoBoom {
        fn name(&self) -> &str {
            "noboom"
        }
        fn on_tnt_prime(&self, _: &Server, _: (i32, i32, i32)) -> bool {
            false
        }
        fn on_raid_start(&self, _: &Server, _: (f32, f32, f32)) -> bool {
            false
        }
    }

    struct Strict;
    impl Plugin for Strict {
        fn name(&self) -> &str {
            "strict"
        }
        fn on_item_spawn(&self, _: &Server, item: &str, _: i8, _: (f32, f32, f32)) -> bool {
            item != "minecraft:dirt"
        }
        fn on_op_change(&self, _: &Server, _: &str, _: bool) -> bool {
            false
        }
    }

    struct Hardcore;
    impl Plugin for Hardcore {
        fn name(&self) -> &str {
            "hardcore"
        }
        fn on_death_drops(&self, _: &Server, _: &PlayerInfo) -> bool {
            false
        }
        fn on_view_distance(&self, _: &Server, _: &PlayerInfo, chunks: &mut i32) {
            *chunks = (*chunks).min(4);
        }
    }

    struct Peaceful;
    impl Plugin for Peaceful {
        fn name(&self) -> &str {
            "peaceful"
        }
        fn on_pvp_attack(&self, _: &Server, _: &PlayerInfo, _: &PlayerInfo, _: f32) -> bool {
            false
        }
        fn on_spawn_protection(&self, _: &Server, name: &str, _: (i32, i32, i32)) -> bool {
            name != "Builder"
        }
    }

    #[test]
    fn pvp_off_and_spawn_protection_exceptions() {
        let server = Server::new();
        let plugins = Plugins::new(vec![Box::new(Peaceful)]);
        let (a, b) = (crate::players::tests::sample(1), crate::players::tests::sample(2));
        assert!(!plugins.pvp_attack(&server, &a, &b, 5.0));
        assert!(!plugins.spawn_protection(&server, "Builder", (0, 64, 0)), "Builder may build at spawn");
        assert!(plugins.spawn_protection(&server, "Steve", (0, 64, 0)));
    }

    struct NoGive;
    impl Plugin for NoGive {
        fn name(&self) -> &str {
            "nogive"
        }
        fn on_give_command(&self, _: &Server, sender: &str, _: &str) -> bool {
            sender == "Admin"
        }
    }

    struct Editor;
    impl Plugin for Editor {
        fn name(&self) -> &str {
            "editor"
        }
        fn on_damage_modify(&self, _: &Server, _: &PlayerInfo, cause: &str, amount: &mut f32) {
            if cause == "fall" {
                *amount = 0.0;
            } else {
                *amount /= 2.0;
            }
        }
        fn on_block_drops(&self, _: &Server, _: &PlayerInfo, _: (i32, i32, i32), block: &str, drops: &mut Vec<(String, i8)>) {
            if block == "minecraft:stone" {
                *drops = vec![("minecraft:diamond".into(), 1)];
            }
        }
        fn on_mob_xp(&self, _: &Server, _: &str, _: (f32, f32, f32), _: Option<i64>, xp: &mut i32) {
            *xp *= 2;
        }
        fn on_knockback(&self, _: &Server, _: &PlayerInfo, knock: &mut (f32, f32, f32)) {
            *knock = (0.0, 0.0, 0.0);
        }
        fn on_permission_check(&self, _: &Server, player: &str, node: &str, allowed: &mut bool) {
            if player == "Vip" && node == "vip.fly" {
                *allowed = true;
            }
        }
    }

    #[test]
    fn plugins_can_edit_damage_drops_xp_knockback_and_permissions() {
        let server = Server::new();
        let plugins = Plugins::new(vec![Box::new(Editor)]);
        let me = crate::players::tests::sample(1);
        let mut a = 8.0;
        plugins.damage_modify(&server, &me, "minecraft:zombie", &mut a);
        assert_eq!(a, 4.0);
        let mut f = 5.0;
        plugins.damage_modify(&server, &me, "fall", &mut f);
        assert_eq!(f, 0.0);
        let mut d = vec![("minecraft:cobblestone".to_string(), 1)];
        plugins.block_drops(&server, &me, (0, 0, 0), "minecraft:stone", &mut d);
        assert_eq!(d, vec![("minecraft:diamond".to_string(), 1)]);
        let mut xp = 5;
        plugins.mob_xp(&server, "minecraft:zombie", (0.0, 0.0, 0.0), Some(1), &mut xp);
        assert_eq!(xp, 10);
        let mut k = (0.4, 0.4, 0.4);
        plugins.knockback(&server, &me, &mut k);
        assert_eq!(k, (0.0, 0.0, 0.0));
        assert!(plugins.allowed_for(&server, "Vip", "vip.fly"), "the plugin granted it");
        assert!(!plugins.allowed_for(&server, "Steve", "vip.fly"), "undeclared nodes stay ops-only");
        assert_eq!(crate::health::Damage::Mob(6.0, "zombie").with_amount(2.0), crate::health::Damage::Mob(2.0, "zombie"));
    }

    #[test]
    fn commands_go_to_their_events() {
        let server = Server::new();
        let plugins = Plugins::new(vec![Box::new(NoGive)]);
        assert!(!plugins.command_event(&server, "Steve", &["give", "diamond", "64"]), "only Admin may /give");
        assert!(plugins.command_event(&server, "Admin", &["/give", "diamond"]));
        assert!(plugins.command_event(&server, "Steve", &["say", "hi"]), "other commands go through");
        assert!(plugins.command_event(&server, "Steve", &[]));
    }

    #[test]
    fn death_drops_stop_and_view_distance_changes() {
        let server = Server::new();
        let plugins = Plugins::new(vec![Box::new(Hardcore)]);
        let me = crate::players::tests::sample(1);
        assert!(!plugins.death_drops(&server, &me));
        let mut chunks = 12;
        plugins.view_distance(&server, &me, &mut chunks);
        assert_eq!(chunks, 4);
        assert!(plugins.item_throw(&server, &me, "minecraft:snowball"), "others still go through");
    }

    #[test]
    fn item_drops_and_ops_can_be_stopped() {
        crate::blocks::init();
        let server = Server::new();
        let plugins = Plugins::new(vec![Box::new(Strict)]);
        assert!(!plugins.item_spawn(&server, "minecraft:dirt", 1, (0.0, 0.0, 0.0)));
        assert!(plugins.item_spawn(&server, "minecraft:stone", 1, (0.0, 0.0, 0.0)));
        assert!(!plugins.op_change(&server, "Steve", true));
    }

    #[test]
    fn tnt_and_raids_can_be_stopped() {
        let plugins = Plugins::new(vec![Box::new(NoBoom)]);
        let server = Server::new();
        assert!(!plugins.tnt_prime(&server, (0, 0, 0)));
        assert!(!plugins.raid_start(&server, (0.0, 0.0, 0.0)));
        assert!(plugins.dispense(&server, (0, 0, 0), "minecraft:dispenser"), "others still go through");
    }

    #[test]
    fn random_tick_changes_go_to_the_right_event() {
        let plugins = Plugins::new(vec![Box::new(KeepLeaves)]);
        let server = Server::new();
        assert!(!plugins.block_change(&server, (0, 0, 0), "minecraft:oak_leaves", "minecraft:air"), "decay cancelled");
        assert!(plugins.block_change(&server, (0, 0, 0), "minecraft:dirt", "minecraft:grass_block"), "spread allowed");
        assert!(plugins.block_change(&server, (0, 0, 0), "minecraft:wheat", "minecraft:wheat"), "growth allowed");
    }

    #[test]
    fn events_cancel_and_edit() {
        let plugins = Plugins::new(vec![Box::new(NoDirt), Box::new(Welcome)]);
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

    /// Records the order plugins see chat in, and blocks it if told to.
    struct Order {
        name: &'static str,
        priority: Priority,
        block: bool,
        depends: &'static [&'static str],
        log: std::sync::Arc<parking_lot::Mutex<Vec<&'static str>>>,
    }
    impl Plugin for Order {
        fn name(&self) -> &str {
            self.name
        }
        fn priority(&self) -> Priority {
            self.priority
        }
        fn depends(&self) -> &[&str] {
            self.depends
        }
        fn on_chat(&self, _: &Server, _: &PlayerInfo, _: &mut String) -> bool {
            self.log.lock().push(self.name);
            !self.block
        }
        fn on_server_start(&self, _: &Server) {
            self.log.lock().push(self.name);
        }
        fn permissions(&self) -> Vec<(&'static str, Access)> {
            vec![("order.use", Access::Everyone), ("order.admin", Access::Ops)]
        }
        fn commands(&self) -> Vec<(&'static str, &'static str)> {
            vec![("secret", "")]
        }
        fn command_permission(&self, _: &str) -> Option<&'static str> {
            Some("order.admin")
        }
        fn on_command(&self, _: &Server, _: &PlayerInfo, _: &str, _: &[&str]) -> Option<String> {
            Some("ok".into())
        }
    }

    fn order(name: &'static str, priority: Priority, block: bool, depends: &'static [&'static str], log: &std::sync::Arc<parking_lot::Mutex<Vec<&'static str>>>) -> Box<dyn Plugin> {
        Box::new(Order { name, priority, block, depends, log: log.clone() })
    }

    #[test]
    fn priorities_monitor_and_depends() {
        let server = Server::new();
        let player = crate::players::tests::sample(1);
        let log = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
        let plugins = Plugins::new(vec![
            order("mon", Priority::Monitor, false, &[], &log),
            order("high", Priority::High, false, &["low"], &log),
            order("low", Priority::Low, false, &[], &log),
            order("orphan", Priority::Normal, false, &["missing"], &log),
        ]);
        assert_eq!(plugins.names(), ["mon", "low", "high"], "orphan dropped, low loads before high");
        plugins.server_start(&server);
        assert_eq!(*log.lock(), ["mon", "low", "high"]);
        log.lock().clear();
        assert!(plugins.chat(&server, &player, &mut String::new()));
        assert_eq!(*log.lock(), ["low", "high", "mon"], "events by priority, monitor last");

        let log = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
        let plugins = Plugins::new(vec![order("mon", Priority::Monitor, true, &[], &log), order("blocker", Priority::Low, true, &[], &log)]);
        assert!(!plugins.chat(&server, &player, &mut String::new()));
        assert_eq!(*log.lock(), ["blocker"], "monitor doesn't see cancelled events");
        let log = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
        let plugins = Plugins::new(vec![order("mon", Priority::Monitor, true, &[], &log)]);
        assert!(plugins.chat(&server, &player, &mut String::new()), "monitor can't cancel");

        // declared defaults: player 1 isn't an op
        assert!(plugins.allowed(&player.name, "order.use"));
        assert!(!plugins.allowed(&player.name, "order.admin"));
        assert!(!plugins.allowed(&player.name, "never.declared"));
        assert!(plugins.command(&server, &player, "secret", &[]).unwrap().contains("permission"));
    }

    struct Gate;
    impl Plugin for Gate {
        fn name(&self) -> &str {
            "gate"
        }
        fn on_pre_login(&self, _: &Server, name: &str, _: &str) -> Option<String> {
            (name == "Griefer").then(|| "Banned from the network".into())
        }
        fn on_item_pickup(&self, _: &Server, _: &PlayerInfo, item: &str, _: i8) -> bool {
            item != "minecraft:diamond"
        }
    }

    #[test]
    fn pre_login_and_pickup_events() {
        let server = Server::new();
        let player = crate::players::tests::sample(1);
        let plugins = Plugins::new(vec![Box::new(Gate)]);
        assert_eq!(plugins.pre_login(&server, "Griefer", "").as_deref(), Some("Banned from the network"));
        assert_eq!(plugins.pre_login(&server, "Steve", ""), None);
        assert!(!plugins.item_pickup(&server, &player, "minecraft:diamond", 1));
        assert!(plugins.item_pickup(&server, &player, "minecraft:dirt", 1));
    }

    #[test]
    fn permission_file_rules() {
        let path = std::env::temp_dir().join(format!("rusted-perms-{}.txt", std::process::id()));
        std::fs::write(&path, "# comment\nSteve homes.*\nsteve -homes.delete\nAlex *\n").unwrap();
        let file = PermissionFile::new(&path);
        assert_eq!(file.get("steve", "homes.set"), Some(true), "wildcard, names ignore case");
        assert_eq!(file.get("Steve", "homes.delete"), Some(false), "exact rule beats wildcard");
        assert_eq!(file.get("Steve", "homesick"), None, "homes.* doesn't match homesick");
        assert_eq!(file.get("Alex", "anything.at.all"), Some(true));
        assert_eq!(file.get("Nobody", "homes.set"), None);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn player_data_path_is_safe() {
        let mut p = crate::players::tests::sample(1);
        p.xuid = "../../evil".into();
        let c = Config::player("my plugin/..", &p);
        assert_eq!(c.path, std::path::Path::new("plugins").join("myplugin").join("players").join("evil.txt"));
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

/// Builds an item for plugins: custom name, lore lines and hidden tags that survive in the
/// player's inventory. `plugin::item("minecraft:compass")?.name("§aServers").tag("menu", "servers").build()`
pub struct ItemBuilder {
    stack: crate::inventory::Stack,
    nbt: crate::itemdata::ItemData,
}

/// Starts an item from its name (`minecraft:diamond_sword` or `diamond_sword`).
pub fn item(name: &str) -> Option<ItemBuilder> {
    let item = crate::items::item_by_name(name)?;
    let nbt = crate::itemdata::ItemData::read(&item.user_data_buffer);
    Some(ItemBuilder { stack: crate::inventory::Stack { item, count: 1, net_id: 0 }, nbt })
}

impl ItemBuilder {
    fn display(&mut self) -> Vec<(String, crate::itemdata::Tag)> {
        match self.nbt.get("display") {
            Some(crate::itemdata::Tag::Compound(d)) => d.clone(),
            _ => vec![],
        }
    }

    fn set_display(&mut self, key: &str, v: crate::itemdata::Tag) {
        let mut d = self.display();
        d.retain(|(k, _)| k != key);
        d.push((key.into(), v));
        self.nbt.set("display", crate::itemdata::Tag::Compound(d));
    }

    pub fn name(mut self, name: &str) -> Self {
        self.set_display("Name", crate::itemdata::Tag::String(name.into()));
        self
    }

    pub fn lore(mut self, lines: &[&str]) -> Self {
        let lines = lines.iter().map(|l| crate::itemdata::Tag::String((*l).into())).collect();
        self.set_display("Lore", crate::itemdata::Tag::List(8, lines));
        self
    }

    /// A hidden key = value only plugins see, e.g. which menu a compass opens.
    pub fn tag(mut self, key: &str, value: &str) -> Self {
        let mut tags = match self.nbt.get("rusted") {
            Some(crate::itemdata::Tag::Compound(t)) => t.clone(),
            _ => vec![],
        };
        tags.retain(|(k, _)| k != key);
        tags.push((key.into(), crate::itemdata::Tag::String(value.into())));
        self.nbt.set("rusted", crate::itemdata::Tag::Compound(tags));
        self
    }

    pub fn count(mut self, n: i8) -> Self {
        self.stack.count = n.max(1);
        self
    }

    pub fn build(mut self) -> crate::inventory::Stack {
        self.stack.item.user_data_buffer = self.nbt.write();
        self.stack
    }
}

/// A hidden plugin tag on an item (set with `ItemBuilder::tag`).
pub fn tag(item: &crate::inventory::Stack, key: &str) -> Option<String> {
    match crate::itemdata::ItemData::read(&item.item.user_data_buffer).get("rusted") {
        Some(crate::itemdata::Tag::Compound(t)) => t.iter().find(|(k, _)| k == key).and_then(|(_, v)| match v {
            crate::itemdata::Tag::String(s) => Some(s.clone()),
            _ => None,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod item_tests {
    #[test]
    fn built_items_keep_name_lore_and_tags() {
        crate::blocks::init();
        let it = super::item("compass").unwrap().name("§aServers").lore(&["Right-click to pick"]).tag("menu", "servers").count(2).build();
        assert_eq!(it.count, 2);
        assert_eq!(super::tag(&it, "menu").as_deref(), Some("servers"));
        assert_eq!(super::tag(&it, "other"), None);
        let nbt = crate::itemdata::ItemData::read(&it.item.user_data_buffer);
        assert!(matches!(nbt.get("display"), Some(crate::itemdata::Tag::Compound(d)) if d.iter().any(|(k, _)| k == "Name") && d.iter().any(|(k, _)| k == "Lore")));
    }
}

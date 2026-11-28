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
            && !self.allowed(&p.name, node)
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

#![allow(clippy::too_many_arguments, clippy::type_complexity)] // game code passes a lot of state around

mod blocks;
mod login;
mod chunks;
mod ticks;
mod packets;
mod saves;
mod blockops;
mod session;
mod cmdargs;
mod commands;
mod config;
mod containers;
mod drops;
mod entities;
mod hunger;
mod fishing;
mod loot;
mod minecart;
mod mobs;
mod overworld;
mod fluids;
mod growth;
mod health;
mod inventory;
mod items;
mod physics;
mod placement;
mod recipes;
mod players;
mod plugin;
mod signs;
mod spawn;
mod terrain;
mod vanilla;
mod dims;
mod portal;
mod effects;
mod endfight;
mod end;
mod nether;
mod raids;
mod redstone;
mod tables;
mod trades;
mod xp;
mod enchant;
mod itemdata;
mod ui;
mod useblock;
mod villagers;
mod world;

// Rusted: minimal Bedrock server. Based on bedrock-rs examples/server.rs (Apache-2.0).
use bedrock::auth::Login;
use bedrock::core::world::dimension::Dimension;
use bedrock::network::compression::Compression;
use bedrock::network::login::{LoginOptions, NetworkSettings, begin_encryption, check_protocol};
use bedrock::network::tokio::Connection;
use bedrock::network::tokio::Listener;
use bedrock::network::tokio::error::ConnectionError;
use bedrock::protocol::v662::enums::PacketCompressionAlgorithm;
use bedrock::protocol::v662::enums::{
    ChatRestrictionLevel, Difficulty, EditorWorldType, GamePublishSetting, GameType, GeneratorType,
    PlayStatus, SpawnBiomeType,
};
use bedrock::protocol::v662::packets::{
    NetworkSettingsPacket, PlayStatusPacket, ServerToClientHandshakePacket,
};
use bedrock::protocol::v662::types::{
    ActorRuntimeID, ActorUniqueID, BaseGameVersion, EduSharedUriResource, Experiments,
    NetworkPermissions, SpawnSettings,
};
use bedrock::protocol::v818::types::SyncedPlayerMovementSettings;
use bedrock::protocol::v898::packets::ResourcePackStackPacket;
use bedrock::protocol::v944::packets::VoxelShapesPacket;
use bedrock::protocol::v712::packets::JigsawStructureDataPacket;
use bedrock::protocol::v944::types::NetworkBlockPosition;
use bedrock::protocol::v2168::enums::EducationEditionOffer;
use bedrock::protocol::v2168::packets::ResourcePacksInfoPacket;
use bedrock::protocol::{ProtoVersion, Unknown, V2225};
use bedrock::protocol::v2168::packets::StartGamePacket;
use bedrock::protocol::v2225::types::{
    EditorLevelMigrationVersion, GameRuleLegacyData, LevelSettings,
};
use bedrock::protocol::v662::packets::{ChunkRadiusUpdatedPacket, NetworkChunkPublisherUpdatePacket};
use bedrock::protocol::v662::types::{BlockPos, ChunkPos};
use bedrock::protocol::v2225::packets::LevelChunkPacket;
use bedrock::protocol::v898::packets::TextPacket;
use bedrock::protocol::v924::enums::TextPacketType;
use bedrock::protocol::v662::enums::{BuildPlatform, CommandPermissionLevel};
use bedrock::protocol::v662::packets::RemoveActorPacket;
use bedrock::protocol::v662::types::PropertySyncData;
use bedrock::protocol::v776::types::SerializedAbilitiesData;
use bedrock::protocol::v2168::enums::PlayerPositionMode;
use bedrock::protocol::v2168::packets::MovePlayerPacket;
use bedrock::protocol::v2168::types::NetworkItemStackDescriptor;
use bedrock::protocol::v2225::packets::{AddPlayerPacket, PlayerListPacket};
use bedrock::protocol::v2225::packets::{AddPlayerListEntry, PlayerListEntry};
use bedrock::protocol::v662::enums::PlayerActionType;
use bedrock::protocol::v662::packets::UpdateBlockPacket;
use bedrock::protocol::v662::types::InventoryTransactionData;
use bedrock::protocol::v662::packets::LevelEventPacket;
use bedrock::protocol::v1001::packets::LevelSoundEventPacket;
use bedrock::protocol::v975::packets::InventorySlotPacket;
use bedrock::protocol::v662::packets::AvailableActorIdentifiersPacket;
use bedrock::protocol::v944::packets::{SyncWorldClockState, SyncWorldClocks, SyncWorldClocksPacket};
use bedrock::protocol::v662::enums::ContainerID;
use bedrock::protocol::v662::packets::ContainerOpenPacket;
use bedrock::protocol::v898::packets::InteractPacketAction as InteractAction;
use bedrock::protocol::v2225::enums::ContainerType;
use bedrock::protocol::v662::packets::ItemStackResponsePacket;
use bedrock::protocol::v2168::types::MovePlayerTeleportData;
use bedrock::protocol::v662::enums::PlayerRespawnState;
use bedrock::protocol::v662::packets::RespawnPacket;
use bedrock::protocol::v975::enums::ActorEvent;
use bedrock::protocol::v975::packets::ActorEventPacket;
use bedrock::protocol::v2225::enums::ContainerEnumName;
use std::collections::HashMap;
use tokio::time::Instant;
use uuid::Uuid;

use login::*;
use chunks::*;
use ticks::*;
use packets::*;
use saves::*;
use blockops::*;
use session::*;
use cmdargs::*;

type Protocol = V2225;

const PROTOCOL: i32 = Protocol::PROTOCOL_VERSION.cast_signed();

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "debug,rtc=warn,rtc_sctp=warn,rtc_dtls=warn,rtc_ice=warn,webrtc=warn".into())).init();
    let _ = jigsaw_structure_data(); // fail fast if the bundled data is bad

    // Kept on disk so players are only asked "Trust this server?" once.
    let identity = bedrock::network::nethernet::IdentityStore::load_or_generate("identity.pem")
        .expect("failed to create NetherNet identity")
        .identity()
        .expect("invalid NetherNet identity");

    let cfg = config::get();
    commands::started();
    *commands::DIFFICULTY.lock() = commands::difficulty_from(&cfg.difficulty).unwrap_or(2);
    // RUSTED_PORT overrides server-port, handy for running next to PNX.
    let port = cfg.port;
    let addr: std::net::SocketAddr = ([0, 0, 0, 0], port).into();
    let mut listener = Listener::builder()
        // Classic RakNet (UDP) for older clients and other tools.
        .raknet(addr)
        // NetherNet signalling (TCP) on the same port: what 1.26+ clients use to join servers.
        .nethernet_http(addr, identity)
        .name(&cfg.motd)
        .sub_name(&cfg.sub_motd)
        .protocol::<Protocol>()
        .max_players(cfg.max_players as i32)
        .player_count(0)
        .bind()
        .await
        .unwrap();

    println!("Rusted listening on {addr} (RakNet UDP + NetherNet TCP)");
    // Each level lives in worlds/<level-name>/. A world from before levels (./world.dat) moves in.
    let level_dir = std::path::Path::new("worlds").join(&cfg.level_name);
    std::fs::create_dir_all(&level_dir).expect("failed to create the world folder");
    let world_path = level_dir.join("world.dat");
    let containers_path = level_dir.join("containers.dat");
    if !world_path.exists() && std::path::Path::new("world.dat").exists() {
        std::fs::rename("world.dat", &world_path).expect("failed to move world.dat into worlds/");
        if std::path::Path::new("containers.dat").exists() {
            std::fs::rename("containers.dat", &containers_path).expect("failed to move containers.dat into worlds/");
        }
        println!("Moved world.dat into {}", level_dir.display());
    }
    let chunk_dir = level_dir.join("chunks");
    std::fs::create_dir_all(&chunk_dir).expect("failed to create the chunk folder");
    let _ = world::CHUNK_DIR.set(chunk_dir);
    let world = world::World::load(&world_path, new_world).expect("failed to load world.dat (left untouched)");
    println!("Level '{}': seed {}", cfg.level_name, world.seed() as i64);
    let mut server = players::Server::with_world(world);
    server.containers = std::sync::Arc::new(
        containers::Containers::load(&containers_path).expect("failed to load containers.dat (left untouched)"),
    );
    // The Nether and End generators (ported from PNX), on the same seed.
    {
        struct N(nether::Nether);
        struct NC(nether::NetherChunk);
        impl world::Column for NC {
            fn get(&self, lx: i32, y: i32, lz: i32) -> i32 { self.0.get(lx, y, lz) }
            fn biome(&self, lx: i32, y: i32, lz: i32) -> u16 { self.0.biome(lx, y, lz) }
        }
        impl world::DimGen for N {
            fn column(&self, cx: i32, cz: i32) -> std::sync::Arc<dyn world::Column> { std::sync::Arc::new(NC(self.0.generate(cx, cz))) }
        }
        struct E(std::sync::Arc<end::End>);
        struct EC(end::EndChunk);
        impl world::Column for EC {
            fn get(&self, lx: i32, y: i32, lz: i32) -> i32 { self.0.get(lx, y, lz) }
            fn biome(&self, lx: i32, y: i32, lz: i32) -> u16 { self.0.biome(lx, y, lz) }
        }
        impl world::DimGen for E {
            fn column(&self, cx: i32, cz: i32) -> std::sync::Arc<dyn world::Column> { std::sync::Arc::new(EC(self.0.generate(cx, cz))) }
        }
        let seed = server.world.seed();
        server.world.set_dim(dims::NETHER, Box::new(N(nether::Nether::new(seed))));
        let the_end = std::sync::Arc::new(end::End::new(seed));
        let _ = endfight::END.set(the_end.clone());
        server.world.set_dim(dims::END, Box::new(E(the_end)));
    }
    blocks::init();
    server.plugins.server_start(&server);
    // Hoppers players placed keep working after a restart.
    server.containers.hoppers.lock().extend(server.world.placed(|b| containers::kind(b) == Some(containers::Kind::Hopper)));
    let mobs_path = level_dir.join("mobs.txt");
    let loaded = server.mobs.load(&mobs_path, &HashMap::new());
    if loaded > 0 {
        println!("Loaded {loaded} mobs");
    }

    // Liquids every 250 ms (water speed), lava every 6th tick.
    {
        let server = server.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(250));
            let mut rng = 0x2545_F491_4F6C_DD1Du64 ^ std::process::id() as u64;
            let mut weather = commands::WeatherCycle::default();
            for turn in 0u64..=u64::MAX {
                tick.tick().await;
                if game_rule_on(&server, "doWeatherCycle", true) {
                    let now = *commands::WEATHER.lock();
                    let mut r = rng;
                    let next = weather.step(5, now, |lo, hi| {
                        r ^= r << 13;
                        r ^= r >> 7;
                        r ^= r << 17;
                        lo + (r % (hi - lo + 1) as u64) as i32
                    });
                    rng = r;
                    if let Some(w) = next {
                        commands::set_weather(&server, w);
                    }
                }
                for ((x, y, z), b) in server.fluids.tick(&server.world, turn % 6 == 0) {
                    server.set_block_quiet(0, x, y, z, b);
                }
                if turn % 16 == 0 {
                    server.entities.despawn_old(&server);
                }
                random_ticks(&server, &mut rng);
                if turn % 6 == 0 {
                    fire_ticks(&server, &mut rng);
                }
                if *commands::WEATHER.lock() == commands::Weather::Thunder {
                    lightning(&server, &mut rng);
                }
                if turn % 2 == 0 {
                    hopper_ticks(&server);
                }
                if turn % 4 == 0 {
                    endfight::tick(&server);
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    raids::tick(&server, rng as u32);
                    for pos in server.containers.tick_closed_brew() {
                        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound("random.potion.brewed", (pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5))) });
                    }
                }
                if turn % 4 == 0 {
                    // Closed furnaces keep smelting; their block shows whether they're lit.
                    for (pos, lit) in server.containers.tick_closed(1.0) {
                        set_lit(&server, pos, lit);
                    }
                }
            }
        });
    }

    // Redstone: one step per game tick (20 Hz).
    {
        let server = server.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(50));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut plates: HashMap<(i32, i32, i32), Instant> = HashMap::new();
            loop {
                tick.tick().await;
                let s = server.clone();
                let mut p = std::mem::take(&mut plates);
                let back = tokio::task::spawn_blocking(move || {
                    redstone_tick(&s, &mut p);
                    p
                })
                .await;
                if let Ok(p) = back {
                    plates = p;
                }
            }
        });
    }

    // Mobs: 10 ticks a second; natural spawning near players every 2 s.
    {
        let server = server.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(100));
            for turn in 0u64..=u64::MAX {
                tick.tick().await;
                let s = server.clone();
                // Mob physics reads the world (may generate chunks): keep it off the async threads.
                let _ = tokio::task::spawn_blocking(move || {
                    s.mobs.tick(&s);
                    fishing::tick(&s);
                    s.entities.tick(&s);
                    if turn % 20 == 0 {
                        s.mobs.spawn_near_players(&s);
                    }
                })
                .await;
            }
        });
    }

    // Autosave every 10s when something changed, and once more on Ctrl+C.
    {
        let (world, path, events) = (server.world.clone(), world_path.clone(), server.events.clone());
        let (chests, chests_path) = (server.containers.clone(), containers_path.clone());
        let (mobs, mobs_file, srv) = (server.mobs.clone(), mobs_path.clone(), server.clone());
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(10));
            loop {
                tick.tick().await;
                if let Err(e) = world.save_if_dirty(&path) {
                    println!("autosave failed: {e}");
                }
                if let Err(e) = chests.save_if_dirty(&chests_path) {
                    println!("container autosave failed: {e}");
                }
                if let Err(e) = mobs.save(&mobs_file, &owner_keys(&srv)) {
                    println!("mob autosave failed: {e}");
                }
                let _ = events.send(players::Event::SaveAll);
            }
        });
        let (world, path, events) = (server.world.clone(), world_path.clone(), server.events.clone());
        let (chests, chests_path) = (server.containers.clone(), containers_path.clone());
        let (mobs, mobs_file, srv) = (server.mobs.clone(), mobs_path.clone(), server.clone());
        tokio::spawn(async move {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = STOP.notified() => {}
            }
            srv.plugins.server_stop(&srv);
            {
                // Everyone saves on the way out (sessions save when kicked).
                let _ = events.send(players::Event::Kick { id: 0, message: "Server closed".into(), reason: players::Kick::Shutdown });
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if let Err(e) = chests.save_if_dirty(&chests_path) {
                    println!("final container save failed: {e}");
                }
                if let Err(e) = mobs.save(&mobs_file, &owner_keys(&srv)) {
                    println!("final mob save failed: {e}");
                }
                match world.save_if_dirty(&path) {
                    Ok(_) => println!("world saved, shutting down"),
                    Err(e) => println!("final save failed: {e}"),
                }
                std::process::exit(0);
            }
        });
    }
    // Console commands (stdin), run as an operator.
    {
        let server = server.clone();
        std::thread::spawn(move || {
            for line in std::io::stdin().lines().map_while(Result::ok) {
                let line = line.trim().trim_start_matches('/');
                if line.is_empty() {
                    continue;
                }
                let args: Vec<&str> = line.split_whitespace().collect();
                match commands::run(&server, commands::CONSOLE, &args) {
                    Some(r) if r.is_empty() => {}
                    Some(r) => println!("{}", strip_colors(&r)),
                    None => println!("Unknown command. Type \"help\" for help."),
                }
            }
        });
    }
    let mut motd_tick = tokio::time::interval(std::time::Duration::from_secs(5));
    let mut shown = -1;
    loop {
        let conn = tokio::select! {
            c = listener.accept() => c.unwrap(),
            _ = motd_tick.tick() => {
                // Keep the server list's online count current.
                let n = server.all().len() as i32;
                if n != shown {
                    shown = n;
                    listener.update_motd(|m| m.player_count = n);
                }
                continue;
            }
        };
        let server = server.clone();

        tokio::spawn(async move {
            handle_login(conn, server).await;
        });
    }
}

static STOP: tokio::sync::Notify = tokio::sync::Notify::const_new();

/// Saves everything and exits (what /stop does).
pub fn request_stop() {
    STOP.notify_one();
}

fn strip_colors(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '§' {
            it.next();
        } else {
            out.push(c);
        }
    }
    out
}

impl Default for ChunkTracker {
    fn default() -> Self {
        Self { sent: Default::default(), center: None, radius: VIEW_RADIUS.min(config::get().view_distance), pending: Default::default(), last_batch: None }
    }
}

async fn play(
    conn: dims::DimConn,
    chunks: ChunkTracker,
    me: players::PlayerInfo,
    server: players::Server,
    inv: inventory::Inventory,
    mode: GameType,
) {
    let Some(mut s) = Session::join(conn, chunks, me, server, inv, mode).await else { return };
    loop {
        let flow = tokio::select! {
            batch = s.conn.recv() => match batch {
                Ok(batch) => s.on_batch(batch).await,
                Err(e) => {
                    println!("{}: connection ended at {:?}: {e}", s.name, std::time::SystemTime::now());
                    END
                }
            },
            _ = s.regen.tick() => {
                s.every_second().await;
                GO
            }
            Some((line, reply_to)) = s.cmd_rx.recv() => {
                s.run_command(line, reply_to).await;
                GO
            }
            event = s.events.recv() => s.on_event(event).await,
        };
        if flow.is_break() {
            break;
        }
    }
    s.leave();
}

fn survival_like(mode: &GameType) -> bool {
    matches!(mode, GameType::Survival | GameType::Adventure)
}

/// A boolean game rule's current value (`default` if never changed).
fn game_rule_on(server: &players::Server, rule: &str, default: bool) -> bool {
    use bedrock::protocol::v844::types::Type;
    server.game_rules.lock().iter().find(|r| r.rule_name == rule).map_or(default, |r| matches!(r.rule_type, Type::Bool(true)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_coords_and_rawtext() {
        assert_eq!(coords(&["~", "~1", "~-2"], (10.5, 64.0, 3.5)), Some((10.5, 65.0, 1.5)));
        assert_eq!(coords(&["5", "70", "-3"], (0.0, 0.0, 0.0)), Some((5.5, 70.0, -2.5)), "block centre");
        assert!(coords(&["x", "1", "2"], (0.0, 0.0, 0.0)).is_none());
        assert_eq!(rawtext(r#"{"rawtext":[{"text":"Hello "},{"text":"world"}]}"#), "Hello world");
        assert_eq!(rawtext("plain"), "plain");
    }

    #[test]
    fn walking_away_and_back_resends_dropped_chunks() {
        let mut t = ChunkTracker::default();
        let first = t.recenter(0, 0).len();
        assert!(first > 0);
        assert!(t.recenter(0, 0).is_empty(), "nothing new when standing still");
        // Walk 9 chunks east: chunk (-8, 0) is now outside the client's view and gets dropped.
        t.recenter(9, 0);
        assert!(!t.sent.contains(&(-8, 0)));
        // Coming back must resend it.
        assert!(t.recenter(0, 0).contains(&(-8, 0)));
        assert_eq!(t.sent.len(), first);
    }
}

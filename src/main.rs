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
    *commands::DIFFICULTY.lock().unwrap() = commands::difficulty_from(&cfg.difficulty).unwrap_or(2);
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
    server.containers.hoppers.lock().unwrap().extend(server.world.placed(|b| containers::kind(b) == Some(containers::Kind::Hopper)));
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
                    let now = *commands::WEATHER.lock().unwrap();
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
                if *commands::WEATHER.lock().unwrap() == commands::Weather::Thunder {
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
    mut conn: dims::DimConn,
    mut chunks: ChunkTracker,
    me: players::PlayerInfo,
    server: players::Server,
    mut inv: inventory::Inventory,
    mut mode: GameType,
) {
    use players::Event;
    let (id, name) = (me.id, me.name.clone());
    let mut events = server.events.subscribe();
    // Commands to run here: (line, who to answer, None = us).
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<(String, Option<i64>)>();
    // Show everyone already online (and ourselves in the player list), then announce us.
    let mut intro = vec![available_commands(&server.plugins), player_list_add(&me), commands::difficulty_packet(*commands::DIFFICULTY.lock().unwrap())];
    intro.extend(commands::weather_packets(*commands::WEATHER.lock().unwrap()));
    intro.extend(server.entities.spawn_packets());
    intro.extend(server.mobs.spawn_packets());
    intro.extend(ui::join_packets());
    for other in server.all() {
        intro.push(player_list_add(&other));
        intro.push(add_player(&other));
    }
    if let Err(e) = conn.send(&intro).await {
        println!("{name}: failed to send player list: {e}");
        return;
    }
    let me_info = me.clone();
    let start_dim = dims::of_f(me.pos.1);
    let (start_pos, start_rot, start_head) = (me.pos, me.rot, me.head_yaw);
    server.join(me);
    if start_dim != dims::OVERWORLD && change_dim(&mut conn, &server, &mut chunks, id, start_dim, start_pos, start_rot, start_head).await.is_err() {
        return;
    }
    if commands::is_op(&name) {
        commands::op_ids().lock().unwrap().insert(id);
    }
    server.mobs.set_targetable(id, survival_like(&mode));
    server.mobs.owner_joined(mobs::owner_key(&name), id);
    server.plugins.join(&server, &me_info);
    println!("{name} joined");
    let _ = server.events.send(Event::Chat {
        from: String::new(),
        message: format!("§e{name} joined the game"),
    });
    let mut selected_slot = 0u32; // hotbar slot in hand, for pick block
    let mut window_id: i8 = 1; // container windows; PNX starts at 2
    let mut open_container: Option<(i32, i32, i32)> = None; // chest/barrel this player has open
    let mut open_kind: Option<(containers::Kind, ContainerEnumName)> = None;
    let mut open_pair: Option<(i32, i32, i32)> = None; // other half of an open double chest
    let mut hp = health::Health::default();
    let mut flying = false;
    let mut sneaking = false;
    let mut sprinting = false;
    let (mut head_in_water, mut in_lava, mut in_fire) = (false, false, false);
    let mut air: i16 = 300; // ticks of air left (vanilla 15s)
    let mut food = hunger::Hunger::default();
    let mut effects = effects::Effects::default();
    let (mut table_changed, mut enchant_changed, mut next_offer_id) = (false, false, 0x5000u32);
    // A trade just happened: refresh the screen (uses, level) afterwards.
    let mut traded_job: Option<(i64, villagers::Job)> = None;
    let mut xp = xp::Xp::from_text(&std::fs::read_to_string(player_path(&name).with_extension("xp")).unwrap_or_default());
    let _ = conn.send(&[xp.packet(id)]).await;
    let mut last_pearl = Instant::now() - std::time::Duration::from_secs(5);
    // When the player started drawing a bow or raising a trident.
    let mut drawing: Option<Instant> = None;
    let mut last_pos: Option<(f32, f32, f32)> = None;
    // Input ticks spent standing in a nether portal, and when the last trip ended.
    let (mut portal_ticks, mut portal_done) = (0u32, Instant::now());
    let mut regen = tokio::time::interval(std::time::Duration::from_secs(1));
    let mut regen_ticks = 0u32;

    'session: loop {
        tokio::select! {
            batch = conn.recv() => {
                let batch = match batch {
                    Ok(batch) => batch,
                    Err(e) => {
                        println!("{name}: connection ended at {:?}: {e}", std::time::SystemTime::now());
                        break;
                    }
                };
                for packet in batch {
                    match packet {
                        Protocol::RequestChunkRadiusPacket(r) => {
                            chunks.radius = r.chunk_radius.clamp(2, config::get().view_distance);
                            println!("{name}: view distance {} chunks", chunks.radius);
                            let _ = conn.send(&[Protocol::ChunkRadiusUpdatedPacket(Box::new(
                                ChunkRadiusUpdatedPacket { chunk_radius: chunks.radius },
                            ))]).await;
                            // Send the newly visible chunks now.
                            if let Some((cx, cz)) = chunks.center.take()
                                && chunks.update(&mut conn, &server.world, cx, cz, CHUNK_BATCH).await.is_err()
                            {
                                break 'session;
                            }
                        }
                        Protocol::PlayerAuthInputPacket(input) => {
                            use bedrock::protocol::v2168::enums::PlayerAuthInputData as Input;
                            for flag in &input.input_data {
                                match flag {
                                    Input::StartFlying => flying = true,
                                    Input::StopFlying => flying = false,
                                    Input::StartSneaking => sneaking = true,
                                    Input::StopSneaking => sneaking = false,
                                    Input::StartSprinting => sprinting = true,
                                    Input::StopSprinting => sprinting = false,
                                    Input::StartJumping if survival_like(&mode) && food.exhaust(if sprinting { 0.2 } else { 0.05 }) => {
                                        let _ = conn.send(&[food.packet(id)]).await;
                                    }
                                    _ => {}
                                }
                            }
                            let pos = input.player_position;
                            let feet = (pos.0.floor() as i32, (pos.1 - 1.62).floor() as i32, pos.2.floor() as i32);
                            let takes_damage = matches!(mode, GameType::Survival | GameType::Adventure) && !flying;
                            let in_water = server.world.get(feet.0, feet.1, feet.2) == terrain::ids().water;
                            let head = server.world.get(pos.0.floor() as i32, pos.1.floor() as i32, pos.2.floor() as i32);
                            head_in_water = blocks::entry(head).is_some_and(|e| e.name.ends_with("water"));
                            let body = blocks::entry(server.world.get(feet.0, feet.1, feet.2)).map_or("", |e| e.name.as_str());
                            in_lava = body.ends_with("lava");
                            in_fire = body == "minecraft:fire";
                            if let Some(damage) = hp.on_move(pos.1 - 1.62, takes_damage, in_water) {
                                hurt(&mut conn, &server, id, &name, &mut hp, damage, &mut inv, &effects).await;
                            }
                            if let Some(last) = last_pos.replace(pos) && survival_like(&mode) && !flying {
                                let walked = ((pos.0 - last.0).powi(2) + (pos.2 - last.2).powi(2)).sqrt().min(10.0);
                                if food.exhaust(walked * if sprinting { 0.1 } else { 0.01 }) {
                                    let _ = conn.send(&[food.packet(id)]).await;
                                }
                            }
                            if !hp.dead && !matches!(mode, GameType::Spectator)
                                && server.entities.pickup(&server, id, (pos.0, pos.1 - 1.62, pos.2), &mut inv)
                            {
                                let _ = conn.send(&[inv.content_packet()]).await;
                            }
                            let rot = input.player_rotation;
                            let head = input.player_head_rotation;
                            server.moved(id, pos, rot, head);
                            // Portals: nether portals after 4 s standing in them (instant in
                            // creative), end portals at once (PNX `Player.checkNearPortal`).
                            let feet = (pos.0.floor() as i32, (pos.1 - 1.62 + 0.1).floor() as i32, pos.2.floor() as i32);
                            let inside = |n: &str| (0..2).any(|dy| blocks::entry(server.world.get(feet.0, feet.1 + dy, feet.2)).is_some_and(|e| e.name == n));
                            if portal_done.elapsed() > std::time::Duration::from_secs(3) {
                                if inside("minecraft:portal") {
                                    portal_ticks += 1;
                                    if portal_ticks >= if matches!(mode, GameType::Creative) { 2 } else { 80 } {
                                        portal_ticks = 0;
                                        portal_done = Instant::now();
                                        let (to, x, z) = portal::mirror(pos);
                                        let inner = match portal::find_portal(&server.world, to, x, z) {
                                            Some(p) => p,
                                            None => {
                                                let (changes, inner) = portal::build_portal(&server.world, to, x, z);
                                                for ((bx, by, bz), b) in changes {
                                                    server.set_block_quiet(0, bx, by, bz, b);
                                                }
                                                inner
                                            }
                                        };
                                        let eye = (inner.0 as f32 + 0.5, inner.1 as f32 + 1.62, inner.2 as f32 + 0.5);
                                        if change_dim(&mut conn, &server, &mut chunks, id, to, eye, rot, head).await.is_err() {
                                            break 'session;
                                        }
                                        continue;
                                    }
                                } else {
                                    portal_ticks = 0;
                                }
                                if inside("minecraft:end_gateway") && let Some(dest) = endfight::gateway_exit(&server, pos) {
                                    portal_done = Instant::now();
                                    let mut tp = move_player(id, dest, rot, head);
                                    if let Protocol::MovePlayerPacket(m) = &mut tp {
                                        m.position_mode = PlayerPositionMode::Teleport;
                                        m.teleport_data = Some(MovePlayerTeleportData { teleportation_cause: 0, source_actor_type: 0 });
                                    }
                                    let _ = conn.send(&[tp]).await;
                                    server.moved(id, dest, rot, head);
                                    continue;
                                }
                                if inside("minecraft:end_portal") {
                                    portal_done = Instant::now();
                                    let (to, eye) = if dims::of_f(pos.1) == dims::END {
                                        (dims::OVERWORLD, commands::spawn_of(&name, &server))
                                    } else {
                                        let (changes, eye) = portal::end_platform();
                                        for ((bx, by, bz), b) in changes {
                                            server.set_block_quiet(0, bx, by, bz, b);
                                        }
                                        (dims::END, eye)
                                    };
                                    if change_dim(&mut conn, &server, &mut chunks, id, to, eye, rot, head).await.is_err() {
                                        break 'session;
                                    }
                                    continue;
                                }
                            }
                            let (cx, cz) = ((pos.0.floor() as i32) >> 4, (pos.2.floor() as i32) >> 4);
                            if chunks.update(&mut conn, &server.world, cx, cz, CHUNK_BATCH).await.is_err() {
                                break 'session;
                            }
                        }
                        Protocol::ModalFormResponsePacket(f) => {
                            ui::answered(&server, id, f.form_id, f.json_response.as_deref());
                        }
                        Protocol::CommandRequestPacket(cmd) => {
                            let mut line = cmd.command.trim().trim_start_matches('/').to_string();
                            if let Some(me) = server.get(id) && !server.plugins.command_preprocess(&server, &me, &mut line) {
                                continue;
                            }
                            println!("{name} issued server command: /{line}");
                            let first = line.split_whitespace().next().unwrap_or("");
                            if commands::OP_ONLY.contains(&first) && !commands::is_op(&name) {
                                let _ = conn.send(&[chat_packet(String::new(), "§cYou do not have permission to use this command".to_string())]).await;
                                continue;
                            }
                            // A player argument (@a, @p, a name...) runs the command in each target's session.
                            match commands::expand(&server, id, &line) {
                                Ok(runs) => {
                                    for (to, l) in runs {
                                        if to == id {
                                            let _ = cmd_tx.send((l, None));
                                        } else {
                                            let _ = server.events.send(Event::RunAs { to, line: l, reply_to: id });
                                        }
                                    }
                                }
                                Err(msg) => {
                                    let _ = conn.send(&[chat_packet(String::new(), msg)]).await;
                                }
                            }
                        }
                        Protocol::TextPacket(text) => {
                            let TextPacketType::Chat { mut message, .. } = text.message_type else { continue };
                            if let Some(me) = server.get(id) && !server.plugins.chat(&server, &me, &mut message) {
                                continue;
                            }
                            println!("<{name}> {message}");
                            let _ = server.events.send(Event::Chat { from: name.clone(), message });
                        }
                        Protocol::InventoryTransactionPacket(t) if matches!(&t.data, InventoryTransactionData::ItemRelease(r) if r.action_type == 0) => {
                            // Let go of a drawn bow or raised trident (PNX ItemBow/ItemTrident.onRelease).
                            let slot = selected_slot as usize;
                            let Some(started) = drawing.take() else { continue };
                            let Some(st) = inv.held(slot).cloned() else { continue };
                            let held = items::name_of(st.item.id).unwrap_or("");
                            let Some(me) = server.get(id) else { continue };
                            let ticks = started.elapsed().as_secs_f32() * 20.0;
                            let data = st.item.user_data_buffer.clone();
                            if held == "minecraft:trident" && ticks >= 10.0 {
                                let mut item = st.item.clone();
                                item.stack_size = 1;
                                server.mobs.player_shoot(&server, id, me.pos, me.rot, 1.0, true, 0, 0, false, Some(item));
                                if survival_like(&mode) {
                                    inv.use_one(slot);
                                    let _ = conn.send(&[inv.content_packet()]).await;
                                }
                            } else if held == "minecraft:bow" {
                                let f = ticks / 20.0;
                                let force = ((f * f + f * 2.0) / 3.0).min(1.0);
                                if force < 0.1 {
                                    continue;
                                }
                                let infinity = enchant::level(&data, 22) > 0;
                                let arrow_slot = (0..36).find(|&i| inv.held(i).and_then(|s| items::name_of(s.item.id)) == Some("minecraft:arrow"));
                                if survival_like(&mode) && arrow_slot.is_none() {
                                    continue; // nothing to shoot
                                }
                                let keep = !survival_like(&mode) || infinity;
                                if !keep && let Some(a) = arrow_slot {
                                    inv.use_one(a);
                                }
                                let drop = (!keep).then(|| items::item_by_name("minecraft:arrow")).flatten();
                                server.mobs.player_shoot(&server, id, me.pos, me.rot, force, false, enchant::level(&data, 19), enchant::level(&data, 20), enchant::level(&data, 21) > 0, drop);
                                wear(&mut conn, &server, id, &mut inv, slot, 1, &mode).await;
                                let _ = conn.send(&[inv.content_packet()]).await;
                            }
                        }
                        Protocol::InventoryTransactionPacket(t) if matches!(&t.data, InventoryTransactionData::ItemRelease(r) if r.action_type == 1) => {
                            // Finished eating (release action 1 = consume).
                            let slot = selected_slot as usize;
                            let held = inv.held(slot).and_then(|s| items::name_of(s.item.id)).unwrap_or("");
                            let eaten = hunger::food(held);
                            // Drinks and bowls give their container back (PNX ItemFood/ItemPotion/ItemBucket).
                            let leftover = match held {
                                "minecraft:milk_bucket" => Some("minecraft:bucket"),
                                "minecraft:honey_bottle" | "minecraft:potion" => Some("minecraft:glass_bottle"),
                                "minecraft:mushroom_stew" | "minecraft:rabbit_stew" | "minecraft:beetroot_soup" | "minecraft:suspicious_stew" => Some("minecraft:bowl"),
                                _ => None,
                            };
                            if (eaten.is_some() || leftover.is_some()) && survival_like(&mode) {
                                if let Some((points, sat)) = eaten {
                                    food.eat(points, sat);
                                }
                                let roll = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() % 100;
                                let mut gained: Vec<(i32, effects::Effect)> = effects::food(held).into_iter()
                                    .filter(|e| roll < e.3)
                                    .map(|(e, amplifier, ticks, _)| (e, effects::Effect { amplifier, ticks, particles: true }))
                                    .collect();
                                if held == "minecraft:potion" {
                                    let aux = inv.held(slot).map_or(0, |s| s.item.aux_value);
                                    gained.extend(effects::potion(aux).into_iter().map(|(e, amplifier, ticks)| (e, effects::Effect { amplifier, ticks, particles: true })));
                                }
                                // Milk clears everything; honey cures poison.
                                let cured: Vec<i32> = match held {
                                    "minecraft:milk_bucket" => effects.active.keys().copied().collect(),
                                    "minecraft:honey_bottle" => vec![effects::POISON],
                                    _ => vec![],
                                };
                                for e in cured {
                                    effects.active.remove(&e);
                                    let _ = conn.send(&[effect_packet(id, e, None)]).await;
                                }
                                let _ = conn.send(&[movement_packet(id, effects.speed())]).await;
                                for (e, x) in gained {
                                    give_effect(&mut conn, &server, id, &name, &mut hp, &mut food, &mut effects, &mut inv, e, x).await;
                                }
                                inv.use_one(slot);
                                if let Some(it) = leftover.and_then(items::item_by_name) {
                                    if inv.held(slot).is_none() {
                                        inv.set_slot(slot, it, 1);
                                    } else {
                                        let left = inv.add(it.clone(), 1);
                                        if left > 0 && let Some(me) = server.get(id) {
                                            server.entities.drop_item(&server, it, left, (me.pos.0, me.pos.1 - 1.0, me.pos.2));
                                        }
                                    }
                                }
                                let _ = conn.send(&[food.packet(id), inv.content_packet()]).await;
                            }
                        }
                        Protocol::InventoryTransactionPacket(t) if matches!(t.data, InventoryTransactionData::Normal) => {
                            // Q outside the inventory screen: a world-interaction action (the thrown
                            // stack) plus a container action (the hotbar slot it came from).
                            let thrown = t.actions.iter().find(|a| format!("{:?}", a.source.source_type) == "WorldInteraction").map_or(0, |a| a.to_item_descriptor.stack_size as i8);
                            let slot = t.actions.iter().find(|a| format!("{:?}", a.source.source_type) == "Container").map(|a| a.slot as usize);
                            if thrown > 0
                                && let Some(slot) = slot
                                && let Some(st) = inv.held(slot).cloned()
                                && let Some(me) = server.get(id)
                            {
                                let n = thrown.min(st.count);
                                for _ in 0..n {
                                    inv.use_one(slot);
                                }
                                server.plugins.item_drop(&server, &me, items::name_of(st.item.id).unwrap_or(""), n);
                                let (at, vel) = throw_from(&me);
                                server.entities.drop_item_vel(&server, st.item, n, at, vel);
                                let item = inv.held(slot).map_or_else(spawn::air, inventory::Stack::to_network);
                                let _ = conn.send(&[Protocol::InventorySlotPacket(Box::new(InventorySlotPacket {
                                    container_id: 0, slot: slot as u32, container_name_data: None, storage_item: None, item,
                                }))]).await;
                            }
                        }
                        Protocol::InventoryTransactionPacket(t) if matches!(&t.data, InventoryTransactionData::ItemUseOnEntity(e) if e.action_type == 0) => {
                            // Right-click on a mob: feed it, or milk a cow.
                            let InventoryTransactionData::ItemUseOnEntity(e) = &t.data else { continue };
                            let slot = selected_slot as usize;
                            let held = inv.held(slot).and_then(|s| items::name_of(s.item.id));
                            let changed = match server.mobs.interact(&server, e.actor_runtime_id as i64, held, id) {
                                mobs::Use::Trade => {
                                    let vid = e.actor_runtime_id as i64;
                                    if let Some(job) = server.mobs.job_of(vid) {
                                        window_id = if window_id >= 99 { 2 } else { window_id + 1 };
                                        let (tags, offers) = villagers::screen(vid, &job);
                                        inv.trading = Some((vid, offers));
                                        let _ = conn.send(&[trade_packet(window_id, vid, id, &job, tags)]).await;
                                    }
                                    false
                                }
                                mobs::Use::Consume if survival_like(&mode) => {
                                    inv.use_one(slot);
                                    true
                                }
                                mobs::Use::Milk if survival_like(&mode) => {
                                    inv.use_one(slot);
                                    if let Some(milk) = items::item_by_name("minecraft:milk_bucket") {
                                        if inv.held(slot).is_none() {
                                            inv.set_slot(slot, milk, 1);
                                        } else {
                                            inv.add(milk, 1);
                                        }
                                    }
                                    true
                                }
                                _ => false,
                            };
                            if changed {
                                let _ = conn.send(&[inv.content_packet()]).await;
                            }
                        }
                        Protocol::InventoryTransactionPacket(t) if matches!(&t.data, InventoryTransactionData::ItemUseOnEntity(e) if e.action_type == 1) => {
                            // Attack (action 1) on a mob.
                            let InventoryTransactionData::ItemUseOnEntity(e) = &t.data else { continue };
                            let target = e.actor_runtime_id as i64;
                            if !matches!(mode, GameType::Spectator) && server.mobs.contains(target)
                                && let Some(me) = server.get(id)
                            {
                                let held = inv.held(selected_slot as usize).and_then(|s| items::name_of(s.item.id));
                                let data = inv.held(selected_slot as usize).map(|s| s.item.user_data_buffer.clone()).unwrap_or_default();
                                let kind = server.mobs.kind_of(target);
                                let bonus = enchant::melee_bonus(&data, kind.is_some_and(|k| k.undead()), kind.is_some_and(|k| k.arthropod()));
                                let hit = mobs::Hit {
                                    knockback: enchant::level(&data, enchant::KNOCKBACK),
                                    fire: 40 * enchant::level(&data, enchant::FIRE_ASPECT) as u32, // 4 s per level
                                    looting: enchant::level(&data, enchant::LOOTING),
                                };
                                let dmg = effects.melee(mobs::attack_damage(held) + bonus);
                                if kind.is_none_or(|k| server.plugins.entity_damage(&server, &me, target, k.identifier(), dmg)) {
                                    server.mobs.hurt_with(&server, target, dmg, me.pos, Some(id), hit);
                                }
                                let uses = attack_wear(held);
                                wear(&mut conn, &server, id, &mut inv, selected_slot as usize, uses, &mode).await;
                                if survival_like(&mode) {
                                    food.exhaust(0.1);
                                }
                            }
                        }
                        Protocol::InventoryTransactionPacket(t) => {
                            // Creative placing: the client tells us the block it's holding.
                            if let InventoryTransactionData::ItemUse(u) = &t.data
                                && u.action_type == 2 // break block (survival)
                            {
                                let p = &u.block_position;
                                if spawn::can_build(&mode)
                                    && server.get(id).is_some_and(|me| server.plugins.block_break(&server, &me, (p.x, p.y, p.z), server.world.get(p.x, p.y, p.z)))
                                {
                                    let held = inv.held(selected_slot as usize).and_then(|s| items::name_of(s.item.id));
                                    let tool = inv.held(selected_slot as usize).map(|s| s.item.user_data_buffer.clone()).unwrap_or_default();
                                    break_block(&server, id, (p.x, p.y, p.z), survival_like(&mode).then_some(held), &tool);
                                    let uses = mining_wear(held);
                                    wear(&mut conn, &server, id, &mut inv, selected_slot as usize, uses, &mode).await;
                                } else {
                                    let _ = conn.send(&[update_block(p.x, p.y, p.z, server.world.get(p.x, p.y, p.z))]).await;
                                }
                            }
                            // Use in the air (action 1) with a bow or trident: start drawing it.
                            if let InventoryTransactionData::ItemUse(u) = &t.data
                                && u.action_type == 1
                                && inv.held(selected_slot as usize).and_then(|s| items::name_of(s.item.id)).is_some_and(|n| matches!(n, "minecraft:bow" | "minecraft:trident"))
                            {
                                drawing = Some(Instant::now());
                                continue;
                            }
                            // Splash and lingering potions are thrown.
                            if let InventoryTransactionData::ItemUse(u) = &t.data
                                && u.action_type == 1
                                && let Some(st) = inv.held(selected_slot as usize).cloned()
                                && items::name_of(st.item.id).is_some_and(|n| matches!(n, "minecraft:splash_potion" | "minecraft:lingering_potion"))
                                && let Some(me) = server.get(id)
                            {
                                mobs::throw(&server, mobs::Throw::Potion(st.item.aux_value), id, me.pos, me.rot);
                                if survival_like(&mode) {
                                    inv.use_one(selected_slot as usize);
                                    let _ = conn.send(&[inv.content_packet()]).await;
                                }
                                continue;
                            }
                            // Use in the air (action 1): throw snowballs, eggs, ender pearls.
                            if let InventoryTransactionData::ItemUse(u) = &t.data
                                && u.action_type == 1
                                && !matches!(mode, GameType::Spectator)
                                && let Some(kind) = inv.held(selected_slot as usize).and_then(|s| items::name_of(s.item.id)).and_then(mobs::Throw::from_item)
                                && let Some(me) = server.get(id)
                                && (kind != mobs::Throw::Pearl || last_pearl.elapsed() >= std::time::Duration::from_secs(1))
                            {
                                if kind == mobs::Throw::Pearl {
                                    last_pearl = Instant::now();
                                }
                                mobs::throw(&server, kind, id, me.pos, me.rot);
                                if survival_like(&mode) {
                                    inv.use_one(selected_slot as usize);
                                    let _ = conn.send(&[inv.content_packet()]).await;
                                }
                                continue;
                            }
                            if let InventoryTransactionData::ItemUse(u) = t.data
                                && u.action_type == 0 // click block
                            {
                                let p = u.block_position;
                                // Right-click a chest/barrel/furnace/crafting table: open it (sneak to place against it).
                                let clicked = server.world.get(p.x, p.y, p.z);
                                if !sneaking && !matches!(mode, GameType::Spectator) && let Some(kind) = containers::kind(clicked) {
                                    if let (Some(pos), Some(slots)) = (open_container.take(), inv.open.take()) {
                                        close_open(&server, pos, slots, open_pair.take());
                                    }
                                    let pos = (p.x, p.y, p.z);
                                    window_id = if window_id >= 99 { 2 } else { window_id + 1 };
                                    let (container_type, name) = match kind {
                                        containers::Kind::Chest => (ContainerType::Container, ContainerEnumName::LevelEntityContainer),
                                        containers::Kind::Barrel => (ContainerType::Container, ContainerEnumName::BarrelContainer),
                                        containers::Kind::Furnace => (ContainerType::Furnace, ContainerEnumName::FurnaceIngredientContainer),
                                        containers::Kind::Smoker => (ContainerType::Smoker, ContainerEnumName::SmokerIngredientContainer),
                                        containers::Kind::BlastFurnace => (ContainerType::BlastFurnace, ContainerEnumName::BlastFurnaceIngredientContainer),
                                        containers::Kind::Workbench => (ContainerType::Workbench, ContainerEnumName::CraftingInputContainer),
                                        containers::Kind::Stonecutter => (ContainerType::StoneCutter, ContainerEnumName::StonecutterInputContainer),
                                        containers::Kind::Hopper => (ContainerType::Hopper, ContainerEnumName::LevelEntityContainer),
                                        containers::Kind::Brewing => (ContainerType::BrewingStand, ContainerEnumName::BrewingStandInputContainer),
                                        containers::Kind::Anvil => (ContainerType::Anvil, ContainerEnumName::AnvilInputContainer),
                                        containers::Kind::Grindstone => (ContainerType::Grindstone, ContainerEnumName::GrindstoneInputContainer),
                                        containers::Kind::Shulker => (ContainerType::Container, ContainerEnumName::ShulkerBoxContainer),
                                        containers::Kind::Dispenser => (ContainerType::Dispenser, ContainerEnumName::LevelEntityContainer),
                                        containers::Kind::Dropper => (ContainerType::Dropper, ContainerEnumName::LevelEntityContainer),
                                        containers::Kind::Smithing => (ContainerType::SmithingTable, ContainerEnumName::SmithingTableInputContainer),
                                        containers::Kind::Enchanting => (ContainerType::Enchantment, ContainerEnumName::EnchantingInputContainer),
                                    };
                                    open_kind = Some((kind, name.clone()));
                                    let mut out = vec![Protocol::ContainerOpenPacket(Box::new(ContainerOpenPacket {
                                        container_id: ContainerID(window_id),
                                        container_type,
                                        position: NetworkBlockPosition { x: p.x, y: p.y, z: p.z },
                                        target_actor_id: ActorUniqueID(-1),
                                    }))];
                                    // Double chest: open from the lead half with both halves' 54 slots.
                                    let pair = (kind == containers::Kind::Chest).then(|| containers::chest_pair(&server.world, pos)).flatten();
                                    let pos = match pair {
                                        Some(o) => pos.min(o),
                                        None => pos,
                                    };
                                    if let (Some(o), Protocol::ContainerOpenPacket(c)) = (pair, &mut out[0]) {
                                        let lead = pos.min(o);
                                        c.position = NetworkBlockPosition { x: lead.0, y: lead.1, z: lead.2 };
                                    }
                                    if !containers::ui_only(kind) {
                                        let mut slots = server.containers.get(pos);
                                        slots.truncate(containers::open_slots(kind));
                                        open_pair = None;
                                        if let Some(o) = pair {
                                            let other = if o == pos { (p.x, p.y, p.z) } else { o };
                                            slots.extend(server.containers.get(other));
                                            open_pair = Some(other);
                                            container_opened(&server, other);
                                        }
                                        inv.open_container(slots);
                                        open_container = Some(pos);
                                        container_opened(&server, pos);
                                        out.extend(inv.open_packet(window_id as u32, name));
                                    }
                                    let _ = conn.send(&out).await;
                                    continue;
                                }
                                let held_name = items::name_of(u.held_item.id).unwrap_or("");
                                let clicked_name = blocks::entry(clicked).map_or("", |e| e.name.as_str());
                                // Beds set your spawn point (sleeping isn't simulated yet).
                                if !sneaking && (clicked_name.ends_with("_bed") || clicked_name == "minecraft:bed") {
                                    // Beds blow up outside the overworld (vanilla: power 5, sets fires).
                                    if dims::of(p.y) != dims::OVERWORLD {
                                        server.set_block(id, p.x, p.y, p.z, world::AIR);
                                        mobs::explode(&server, (p.x as f32 + 0.5, p.y as f32 + 0.5, p.z as f32 + 0.5), 5.0, "[Intentional Game Design]");
                                        continue;
                                    }
                                    commands::set_spawn(Some(&name), (p.x as f32 + 0.5, p.y as f32 + 1.0 + 1.62, p.z as f32 + 0.5));
                                    let t = server.time().rem_euclid(24_000);
                                    let msg = if (12_542..23_460).contains(&t) {
                                        // Sleeping: once everyone online has lain down, skip to morning.
                                        let mut sleepers = sleepers().lock().unwrap();
                                        sleepers.insert(id, Instant::now());
                                        sleepers.retain(|_, at| at.elapsed() < std::time::Duration::from_secs(8));
                                        let all = server.all();
                                        if all.iter().all(|p| sleepers.contains_key(&p.id)) {
                                            sleepers.clear();
                                            let morning = server.time() - t + 24_000;
                                            server.set_time(morning);
                                            let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(clock_packet(morning)) });
                                            let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(time_packet(morning)) });
                                            *commands::WEATHER.lock().unwrap() = commands::Weather::Clear;
                                            for w in commands::weather_packets(commands::Weather::Clear) {
                                                let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(w) });
                                            }
                                            server.broadcast("Good morning!");
                                            "Respawn point set".to_string()
                                        } else {
                                            format!("Respawn point set. Sleeping: {}/{} players", sleepers.len(), all.len())
                                        }
                                    } else {
                                        "Respawn point set. You can only sleep at night".to_string()
                                    };
                                    let _ = conn.send(&[chat_packet(String::new(), msg)]).await;
                                    continue;
                                }
                                // Blocks that react to a click live in useblock.rs.
                                if server.get(id).is_some_and(|me| !server.plugins.player_interact(&server, &me, (p.x, p.y, p.z), held_name)) {
                                    let _ = conn.send(&[update_block(p.x, p.y, p.z, server.world.get(p.x, p.y, p.z))]).await;
                                    continue;
                                }
                                if spawn::can_build(&mode) {
                                    let ctx = useblock::Ctx {
                                        held: held_name,
                                        sneaking,
                                        survival: survival_like(&mode),
                                        hungry: food.food < hunger::MAX_FOOD,
                                        face: u.block_face,
                                        roll: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() ^ (p.x as u32).wrapping_mul(2_654_435_761),
                                        overworld: true,
                                    };
                                    if let Some(o) = useblock::use_block(&server.world, (p.x, p.y, p.z), &ctx) {
                                        apply_use(&server, id, (p.x, p.y, p.z), &o);
                                        if clicked_name == "minecraft:end_portal_frame" && let Some(portal) = portal::light_end(&server.world, (p.x, p.y, p.z)) {
                                            for ((bx, by, bz), b) in portal {
                                                server.set_block_quiet(id, bx, by, bz, b);
                                            }
                                            let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(play_sound("block.end_portal.spawn", (p.x as f32 + 0.5, p.y as f32, p.z as f32 + 0.5))) });
                                        }
                                        if let Some((f, sat)) = o.eat && (ctx.hungry || !ctx.survival) {
                                            food.eat(f, sat);
                                            let _ = conn.send(&[food.packet(id)]).await;
                                        }
                                        if ctx.survival {
                                            let slot = selected_slot as usize;
                                            match o.held {
                                                useblock::Held::Keep if !o.changes.is_empty() || !o.drops.is_empty() => {
                                                    wear(&mut conn, &server, id, &mut inv, slot, 1, &mode).await;
                                                }
                                                useblock::Held::Keep => {}
                                                useblock::Held::UseOne => {
                                                    inv.use_one(slot);
                                                    let _ = conn.send(&[inv.content_packet()]).await;
                                                }
                                                useblock::Held::Become(item) => swap_bucket(&mut conn, &mut inv, selected_slot, &mode, item).await,
                                                useblock::Held::Trade(item) => {
                                                    inv.use_one(slot);
                                                    if let Some(it) = items::item_by_name(item) {
                                                        let left = inv.add(it.clone(), 1);
                                                        if left > 0 && let Some(me) = server.get(id) {
                                                            server.entities.drop_item(&server, it, left, (me.pos.0, me.pos.1 - 1.0, me.pos.2));
                                                        }
                                                    }
                                                    let _ = conn.send(&[inv.content_packet()]).await;
                                                }
                                            }
                                        }
                                        continue;
                                    }
                                }
                                // Flint and steel lights a fire on the clicked face.
                                if held_name == "minecraft:flint_and_steel" && spawn::can_build(&mode) && clicked_name == "minecraft:tnt" {
                                    mobs::ignite_tnt(&server, (p.x, p.y, p.z), 40);
                                    wear(&mut conn, &server, id, &mut inv, selected_slot as usize, 1, &mode).await;
                                    continue;
                                }
                                if held_name == "minecraft:flint_and_steel" && spawn::can_build(&mode) {
                                    let (ox, oy, oz) = placement::face_offset(u.block_face);
                                    let at = (p.x + ox, p.y + oy, p.z + oz);
                                    if let Some(portal) = portal::light_nether(&server.world, at) {
                                        for ((bx, by, bz), b) in portal {
                                            server.set_block_quiet(id, bx, by, bz, b);
                                        }
                                        let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(play_sound("fire.ignite", (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5))) });
                                        wear(&mut conn, &server, id, &mut inv, selected_slot as usize, 1, &mode).await;
                                        continue;
                                    }
                                    if server.world.get(at.0, at.1, at.2) == world::AIR && let Some(fire) = blocks::any_state("minecraft:fire") {
                                        server.set_block(id, at.0, at.1, at.2, fire);
                                        growth::FIRES.lock().unwrap().push(at);
                                        wear(&mut conn, &server, id, &mut inv, selected_slot as usize, 1, &mode).await;
                                        let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(play_sound("fire.ignite", (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5))) });
                                    }
                                    continue;
                                }
                                // Seeds plant crops on farmland.
                                let crop = match held_name {
                                    "minecraft:wheat_seeds" => Some("minecraft:wheat"),
                                    "minecraft:carrot" => Some("minecraft:carrots"),
                                    "minecraft:potato" => Some("minecraft:potatoes"),
                                    "minecraft:beetroot_seeds" => Some("minecraft:beetroot"),
                                    _ => None,
                                };
                                if let Some(crop) = crop && spawn::can_build(&mode) {
                                    let above = (p.x, p.y + 1, p.z);
                                    if clicked_name == "minecraft:farmland" && server.world.get(above.0, above.1, above.2) == world::AIR
                                        && let Some(b) = blocks::any_state(crop)
                                    {
                                        server.set_block(id, above.0, above.1, above.2, blocks::with(b, &[("growth", blocks::State::Int(0))]));
                                        if survival_like(&mode) {
                                            inv.use_one(selected_slot as usize);
                                        }
                                    }
                                    continue;
                                }
                                // Doors/levers/buttons get used instead of placed (sneak to place).
                                if !sneaking && !matches!(mode, GameType::Spectator)
                                    && let Some(action) = physics::interact(&server.world, (p.x, p.y, p.z))
                                {
                                    match action {
                                        physics::Interaction::Toggle(changes) => {
                                            // 1003: door/trapdoor/gate sound; levers click.
                                            let lever = blocks::entry(server.world.get(p.x, p.y, p.z)).is_some_and(|e| e.name == "minecraft:lever");
                                            let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(level_event(if lever { 1000 } else { 1003 }, (p.x, p.y, p.z))) });
                                            for ((bx, by, bz), b) in changes {
                                                server.set_block_quiet(id, bx, by, bz, b);
                                            }
                                        }
                                        physics::Interaction::Press { pressed, released } => {
                                            // 1000: click.
                                            let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(level_event(1000, (p.x, p.y, p.z))) });
                                            for ((bx, by, bz), b) in pressed {
                                                server.set_block_quiet(id, bx, by, bz, b);
                                            }
                                            let server = server.clone();
                                            tokio::spawn(async move {
                                                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                                for ((bx, by, bz), b) in released {
                                                    server.set_block_quiet(id, bx, by, bz, b);
                                                }
                                            });
                                        }
                                    }
                                    continue;
                                }
                                let (dx, dy, dz) = placement::face_offset(u.block_face);
                                // Buckets: empty one picks up a source, full one places it.
                                let held = items::name_of(u.held_item.id).unwrap_or("");
                                if spawn::can_build(&mode) && matches!(held, "minecraft:water_bucket" | "minecraft:lava_bucket" | "minecraft:bucket") {
                                    let target = (p.x + dx, p.y + dy, p.z + dz);
                                    if held == "minecraft:bucket" {
                                        let clicked = server.world.get(p.x, p.y, p.z);
                                        if clicked == fluids::Kind::Water.source() || clicked == fluids::Kind::Lava.source() {
                                            server.set_block(id, p.x, p.y, p.z, world::AIR);
                                            let full = if clicked == fluids::Kind::Water.source() { "minecraft:water_bucket" } else { "minecraft:lava_bucket" };
                                            let s = if full == "minecraft:water_bucket" { "bucket.fill_water" } else { "bucket.fill_lava" };
                                            let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(play_sound(s, (p.x as f32 + 0.5, p.y as f32 + 0.5, p.z as f32 + 0.5))) });
                                            swap_bucket(&mut conn, &mut inv, selected_slot, &mode, full).await;
                                        }
                                    } else if server.world.get(target.0, target.1, target.2) == world::AIR {
                                        if held == "minecraft:water_bucket" && dims::of(target.1) == dims::NETHER {
                                            let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(play_sound("random.fizz", (target.0 as f32 + 0.5, target.1 as f32 + 0.5, target.2 as f32 + 0.5))) });
                                            swap_bucket(&mut conn, &mut inv, selected_slot, &mode, "minecraft:bucket").await;
                                            continue;
                                        }
                                        let kind = if held == "minecraft:water_bucket" { fluids::Kind::Water } else { fluids::Kind::Lava };
                                        server.set_block(id, target.0, target.1, target.2, kind.source());
                                        let s = if held == "minecraft:water_bucket" { "bucket.empty_water" } else { "bucket.empty_lava" };
                                        let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(play_sound(s, (target.0 as f32 + 0.5, target.1 as f32 + 0.5, target.2 as f32 + 0.5))) });
                                        swap_bucket(&mut conn, &mut inv, selected_slot, &mode, "minecraft:bucket").await;
                                    }
                                    continue;
                                }
                                if u.held_item.block_runtime_id == 0 {
                                    continue; // nothing placeable in hand
                                }
                                let (x, y, z) = (p.x + dx, p.y + dy, p.z + dz);
                                let (pitch, yaw) = server.get(id).map(|me| me.rot).unwrap_or_default();
                                let click = placement::Click { face: u.block_face, click_y: u.click_position.1, yaw, pitch };
                                let parts: Vec<_> = placement::orient(u.held_item.block_runtime_id as i32, &click)
                                    .into_iter()
                                    .map(|(o, b)| (o, if blocks::has_state(b, "persistent_bit") { blocks::with(b, &[("persistent_bit", blocks::State::Bool(true))]) } else { b }))
                                    .collect();
                                // All parts (both door halves, both bed halves) need free space.
                                let fits = spawn::can_build(&mode) && !parts.is_empty() && parts.iter().all(|((ox, oy, oz), _)| {
                                    let (bx, by, bz) = (x + ox, y + oy, z + oz);
                                    server.world.get(bx, by, bz) == world::AIR && !server.occupied(bx, by, bz)
                                }) && server.get(id).is_some_and(|me| {
                                    parts.iter().all(|((ox, oy, oz), b)| server.plugins.block_place(&server, &me, (x + ox, y + oy, z + oz), *b))
                                });
                                if fits {
                                    if survival_like(&mode) {
                                        inv.use_one(selected_slot as usize);
                                    }
                                    if let Some(((ox, oy, oz), _)) = parts.iter().find(|(_, b)| signs::is_sign(*b)) {
                                        let _ = conn.send(&[signs::open_packet((x + ox, y + oy, z + oz))]).await;
                                    }
                                    let placed_chest = parts.iter().find(|(_, b)| containers::kind(*b) == Some(containers::Kind::Chest)).map(|((ox, oy, oz), _)| (x + ox, y + oy, z + oz));
                                    let held_contents = inv.held(selected_slot as usize).map(|s| items::contents(&s.item));
                                    for ((ox, oy, oz), block) in parts {
                                        if containers::kind(block) == Some(containers::Kind::Shulker)
                                            && let Some(c) = held_contents.clone()
                                            && c.iter().any(Option::is_some)
                                        {
                                            server.containers.put((x + ox, y + oy, z + oz), c);
                                        }
                                        if containers::kind(block) == Some(containers::Kind::Hopper) {
                                            server.containers.hoppers.lock().unwrap().insert((x + ox, y + oy, z + oz));
                                        }
                                        server.set_block(id, x + ox, y + oy, z + oz, block);
                                        for ((fx, fy, fz), b) in physics::settle(&server.world, (x + ox, y + oy, z + oz)) {
                                            server.set_block_quiet(id, fx, fy, fz, b);
                                        }
                                    }
                                    if let Some(c) = placed_chest {
                                        announce_chest(&server, c);
                                    }
                                } else {
                                    // Undo the client's predicted placement.
                                    let _ = conn.send(&[update_block(x, y, z, server.world.get(x, y, z))]).await;
                                }
                            }
                        }
                        Protocol::BlockActorDataPacket(b) => {
                            let p = (b.block_position.x, b.block_position.y, b.block_position.z);
                            if spawn::can_build(&mode) && let Some(packet) = signs::edited(&server.world, p, &b.actor_data_tags) {
                                let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(packet) });
                            }
                        }
                        Protocol::SetLocalPlayerAsInitializedPacket(_) => {
                            // Physics flags (gravity, collision), attributes, abilities, inventory.
                            let r = conn.send(&spawn::after_load(id, &name, inv.all_packets())).await;
                            println!("{name}: client loaded; sent player state: {r:?}");
                        }
                        Protocol::InteractPacket(i) if matches!(i.action, InteractAction::OpenInventory) => {
                            // Same reply PNX gives: open the player's own inventory (type -1).
                            window_id = if window_id >= 99 { 2 } else { window_id + 1 };
                            let pos = server.get(id).map(|p| p.pos).unwrap_or_default();
                            let _ = conn.send(&[Protocol::ContainerOpenPacket(Box::new(ContainerOpenPacket {
                                container_id: ContainerID(window_id),
                                container_type: ContainerType::Inventory,
                                position: NetworkBlockPosition {
                                    x: pos.0.floor() as i32,
                                    y: (pos.1 - 1.62).floor() as i32,
                                    z: pos.2.floor() as i32,
                                },
                                target_actor_id: ActorUniqueID(-1),
                            }))]).await;
                        }
                        Protocol::ContainerClosePacket(c) => {
                            // A closing chest keeps its contents on the server.
                            if let (Some(pos), Some(slots)) = (open_container.take(), inv.open.take()) {
                                close_open(&server, pos, slots, open_pair.take());
                            }
                            // Items left in the crafting grid and on the cursor go back into the inventory.
                            let spilled = inv.return_craft();
                            if let Some(me) = server.get(id) {
                                for st in spilled {
                                    server.entities.drop_item(&server, st.item, st.count, me.pos);
                                }
                            }
                            inv.return_cursor();
                            let returned = true;
                            // PNX echoes the client's close back to confirm it.
                            let mut out = vec![Protocol::ContainerClosePacket(c)];
                            if returned {
                                out.push(inv.content_packet());
                            }
                            let _ = conn.send(&out).await;
                        }
                        Protocol::ItemStackRequestPacket(r) => {
                            let mut thrown = Vec::new();
                            let responses = r
                                .requests
                                .iter()
                                .map(|req| {
                                    inv.levels = xp.level;
                                    inv.creative = matches!(mode, GameType::Creative);
                                    let before = inv.enchant_input().map(|s| (s.item.id, s.item.user_data_buffer.clone()));
                                    let r = inv.handle(req, items::creative_item);
                                    thrown.append(&mut inv.dropped);
                                    if inv.spent_levels > 0 {
                                        xp.add_levels(-inv.spent_levels);
                                        table_changed = true;
                                    }
                                    if inv.gained_xp > 0 {
                                        xp.add_points(inv.gained_xp);
                                        table_changed = true;
                                    }
                                    if !inv.traded.is_empty() && let Some((vid, _)) = inv.trading.clone() {
                                        let mut job = None;
                                        for (index, n, texp) in std::mem::take(&mut inv.traded) {
                                            job = server.mobs.traded(&server, vid, index, n, texp).or(job);
                                        }
                                        traded_job = job.map(|j| (vid, j));
                                    }
                                    if inv.enchant_input().map(|s| (s.item.id, s.item.user_data_buffer.clone())) != before {
                                        enchant_changed = true;
                                    }
                                    r
                                })
                                .collect();
                            let _ = conn.send(&[Protocol::ItemStackResponsePacket(Box::new(ItemStackResponsePacket { responses }))]).await;
                            if std::mem::take(&mut table_changed) {
                                save_xp(&name, xp);
                                let _ = conn.send(&[xp.packet(id)]).await;
                                if let Some(pos) = open_container {
                                    let s = match open_kind.as_ref().map(|k| k.0) {
                                        Some(containers::Kind::Anvil) => "random.anvil_use",
                                        Some(containers::Kind::Grindstone) => "grindstone.use",
                                        _ => "random.enchant",
                                    };
                                    let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(play_sound(s, (pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5))) });
                                }
                            }
                            if let Some((vid, job)) = traded_job.take() {
                                let (tags, offers) = villagers::screen(vid, &job);
                                inv.trading = Some((vid, offers));
                                let _ = conn.send(&[xp.packet(id), trade_packet(window_id, vid, id, &job, tags)]).await;
                                save_xp(&name, xp);
                            }
                            // A new item in the enchanting table: show its three offers.
                            if std::mem::take(&mut enchant_changed) && open_kind.as_ref().is_some_and(|k| k.0 == containers::Kind::Enchanting) {
                                let shelves = open_container.map_or(0, |p| bookshelves(&server.world, p));
                                let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64 | 1;
                                inv.offers.clear();
                                let offers = inv.enchant_input().map(|s| tables::offers(&s.item, shelves, seed)).unwrap_or_default();
                                for o in offers {
                                    next_offer_id += 1;
                                    inv.offers.push((next_offer_id, o));
                                }
                                let _ = conn.send(&[enchant_options(&inv.offers)]).await;
                            }
                            if let Some(me) = server.get(id) {
                                for st in thrown.drain(..) {
                                    server.plugins.item_drop(&server, &me, items::name_of(st.item.id).unwrap_or(""), st.count);
                                    let (at, vel) = throw_from(&me);
                                    server.entities.drop_item_vel(&server, st.item, st.count, at, vel);
                                }
                            }
                        }
                        Protocol::MobEquipmentPacket(m) => {
                            selected_slot = m.selected_slot.max(0) as u32;
                            server.mobs.set_held(id, items::name_of(m.item.id));
                            // Others see what we're holding.
                            let _ = server.events.send(Event::Relay { from: id, packet: Box::new(Protocol::MobEquipmentPacket(m)) });
                        }
                        Protocol::AnimatePacket(a) => {
                            let _ = server.events.send(Event::Relay { from: id, packet: Box::new(Protocol::AnimatePacket(a)) });
                        }
                        Protocol::BlockPickRequestPacket(r) => {
                            let p = r.position;
                            if let Some(item) = items::item_for_block(server.world.get(p.x, p.y, p.z))
                                && let Some(stack) = inv.set_slot(selected_slot as usize, item.clone(), items::max_stack(item.id))
                            {
                                let item = stack.to_network();
                                let _ = conn.send(&[Protocol::InventorySlotPacket(Box::new(InventorySlotPacket {
                                    container_id: 0,
                                    slot: selected_slot,
                                    container_name_data: None,
                                    storage_item: None,
                                    item,
                                }))]).await;
                            }
                        }
                        Protocol::RespawnPacket(r) if hp.dead && matches!(r.state, PlayerRespawnState::ClientReadyToSpawn) => {
                            // Dying in the Nether or End sends you back to the overworld spawn.
                            if conn.dim != dims::OVERWORLD {
                                let (rot, head) = server.get(id).map_or(((0.0, 0.0), 0.0), |p| (p.rot, p.head_yaw));
                                if change_dim(&mut conn, &server, &mut chunks, id, dims::OVERWORLD, commands::spawn_of(&name, &server), rot, head).await.is_err() {
                                    break 'session;
                                }
                            }
                            respawn(&mut conn, &server, id, &name, &mut hp, &mode, &inv).await;
                            if let Some(me) = server.get(id) {
                                server.plugins.player_respawn(&server, &me);
                            }
                            food = hunger::Hunger::default();
                            if !game_rule_on(&server, "keepInventory", false) {
                                xp = xp::Xp::default();
                                save_xp(&name, xp);
                                let _ = conn.send(&[xp.packet(id)]).await;
                            }
                            last_pos = None;
                            let _ = conn.send(&[food.packet(id)]).await;
                        }
                        Protocol::PlayerActionPacket(a) if hp.dead && matches!(a.action, PlayerActionType::Respawn) => {
                            // Dying in the Nether or End sends you back to the overworld spawn.
                            if conn.dim != dims::OVERWORLD {
                                let (rot, head) = server.get(id).map_or(((0.0, 0.0), 0.0), |p| (p.rot, p.head_yaw));
                                if change_dim(&mut conn, &server, &mut chunks, id, dims::OVERWORLD, commands::spawn_of(&name, &server), rot, head).await.is_err() {
                                    break 'session;
                                }
                            }
                            respawn(&mut conn, &server, id, &name, &mut hp, &mode, &inv).await;
                            if let Some(me) = server.get(id) {
                                server.plugins.player_respawn(&server, &me);
                            }
                            food = hunger::Hunger::default();
                            if !game_rule_on(&server, "keepInventory", false) {
                                xp = xp::Xp::default();
                                save_xp(&name, xp);
                                let _ = conn.send(&[xp.packet(id)]).await;
                            }
                            last_pos = None;
                            let _ = conn.send(&[food.packet(id)]).await;
                        }
                        Protocol::PlayerActionPacket(a) => {
                            // Creative breaks instantly; survival reports the finished break as a prediction.
                            if let PlayerActionType::CreativeDestroyBlock | PlayerActionType::PredictDestroyBlock = a.action {
                                let p = a.block_position;
                                if spawn::can_build(&mode)
                                    && server.get(id).is_some_and(|me| server.plugins.block_break(&server, &me, (p.x, p.y, p.z), server.world.get(p.x, p.y, p.z)))
                                {
                                    let held = inv.held(selected_slot as usize).and_then(|s| items::name_of(s.item.id));
                                    let tool = inv.held(selected_slot as usize).map(|s| s.item.user_data_buffer.clone()).unwrap_or_default();
                                    break_block(&server, id, (p.x, p.y, p.z), survival_like(&mode).then_some(held), &tool);
                                    let uses = mining_wear(held);
                                    wear(&mut conn, &server, id, &mut inv, selected_slot as usize, uses, &mode).await;
                                } else {
                                    let _ = conn.send(&[update_block(p.x, p.y, p.z, server.world.get(p.x, p.y, p.z))]).await;
                                }
                            }
                        }
                        other => {
                            // debug: log packets we don't handle yet
                            let d = format!("{other:?}");
                            if !d.starts_with("ServerBoundDiagnostics") && !d.starts_with("Animate") {
                                println!("PACKET {}", &d[..d.len().min(300)]);
                            }
                        }
                    }
                }
            }
            _ = regen.tick() => {
                regen_ticks += 1;
                let rule_on = !server.game_rules.lock().unwrap().iter().any(|r| {
                    r.rule_name == "naturalRegeneration" && matches!(r.rule_type, bedrock::protocol::v844::types::Type::Bool(false))
                });
                let _ = regen_ticks;
                // Furnaces smelt live while open: progress arrow and flame update every second.
                if let (Some(pos), Some((kind, cname)), Some(slots)) = (open_container, open_kind.clone(), inv.open.as_mut())
                    && let Some(o) = oven(kind)
                {
                    let (cook, lit, total) = server.containers.tick_open(o, pos, slots, 1.0);
                    set_lit(&server, pos, lit > 0);
                    let set = |id: i32, value: i32| Protocol::ContainerSetDataPacket(Box::new(bedrock::protocol::v662::packets::ContainerSetDataPacket {
                        container_id: ContainerID(window_id),
                        id,
                        value,
                    }));
                    let mut out: Vec<Protocol> = inv.open_packet(window_id as u32, cname).into_iter().collect();
                    out.extend([set(0, cook), set(1, lit), set(2, total)]);
                    let _ = conn.send(&out).await;
                }
                // Brewing stands brew live while open: bubbles and fuel bar.
                if let (Some(pos), Some((containers::Kind::Brewing, cname)), Some(slots)) = (open_container, open_kind.clone(), inv.open.as_mut()) {
                    let ((ticks, fuel, total), done) = server.containers.tick_open_brew(pos, slots);
                    let set = |id: i32, value: i32| Protocol::ContainerSetDataPacket(Box::new(bedrock::protocol::v662::packets::ContainerSetDataPacket {
                        container_id: ContainerID(window_id),
                        id,
                        value,
                    }));
                    let mut out: Vec<Protocol> = inv.open_packet(window_id as u32, cname).into_iter().collect();
                    out.extend([set(0, ticks), set(1, fuel), set(2, total)]);
                    if done {
                        out.push(play_sound("random.potion.brewed", (pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5)));
                    }
                    let _ = conn.send(&out).await;
                }
                // /effect for us, then a second of effects.
                let queued: Vec<effects::Queued> = {
                    let mut q = effects::QUEUE.lock().unwrap();
                    let (mine, rest): (Vec<_>, Vec<_>) = q.drain(..).partition(|(who, _)| *who == id);
                    *q = rest;
                    mine.into_iter().map(|(_, c)| c).collect()
                };
                let enchants: Vec<(i16, i16)> = {
                    let mut q = enchant::QUEUE.lock().unwrap();
                    let (mine, rest): (Vec<_>, Vec<_>) = q.drain(..).partition(|(who, ..)| *who == id);
                    *q = rest;
                    mine.into_iter().map(|(_, e, l)| (e, l)).collect()
                };
                for (e, l) in enchants {
                    let slot = selected_slot as usize;
                    let msg = match inv.slots.get_mut(slot).and_then(Option::as_mut) {
                        Some(st) if items::name_of(st.item.id).is_some_and(|n| enchant::applies(e, n)) => {
                            st.item.user_data_buffer = enchant::add(&st.item.user_data_buffer, e, l);
                            "Your held item was enchanted".to_string()
                        }
                        Some(_) => "§cThat enchantment can't go on this item".to_string(),
                        None => "§cYou aren't holding anything to enchant".to_string(),
                    };
                    let _ = conn.send(&[inv.content_packet(), chat_packet(String::new(), msg)]).await;
                }
                for c in queued {
                    match c {
                        effects::Queued::Give(e, x) => give_effect(&mut conn, &server, id, &name, &mut hp, &mut food, &mut effects, &mut inv, e, x).await,
                        effects::Queued::Clear => {
                            let all: Vec<i32> = effects.active.keys().copied().collect();
                            effects.active.clear();
                            let mut out: Vec<Protocol> = all.into_iter().map(|e| effect_packet(id, e, None)).collect();
                            out.push(movement_packet(id, effects.speed()));
                            let _ = conn.send(&out).await;
                        }
                    }
                }
                if effects.level(effects::BAD_OMEN).is_some()
                    && let Some(me) = server.get(id)
                    && dims::of_f(me.pos.1) == dims::OVERWORLD
                    && let Some(center) = raids::village_near(&server, (me.pos.0, me.pos.1 - 1.62, me.pos.2))
                    && raids::start(&server, center)
                {
                    effects.active.remove(&effects::BAD_OMEN);
                    let _ = conn.send(&[effect_packet(id, effects::BAD_OMEN, None)]).await;
                }
                if !hp.dead && !effects.active.is_empty() {
                    let t = effects.second();
                    if t.heal > 0.0 && hp.health < health::MAX_HEALTH {
                        hp.health = (hp.health + t.heal).min(health::MAX_HEALTH);
                        let _ = conn.send(&[spawn::health_packet(id, hp.health)]).await;
                    }
                    if t.poison > 0.0 && hp.health > 1.0 && survival_like(&mode) {
                        let d = t.poison.min(hp.health - 1.0); // poison never kills
                        hurt(&mut conn, &server, id, &name, &mut hp, health::Damage::Magic(d), &mut inv, &effects).await;
                    }
                    if t.wither > 0.0 && survival_like(&mode) {
                        hurt(&mut conn, &server, id, &name, &mut hp, health::Damage::Wither(t.wither), &mut inv, &effects).await;
                    }
                    if t.exhaustion > 0.0 && survival_like(&mode) && food.exhaust(t.exhaustion) {
                        let _ = conn.send(&[food.packet(id)]).await;
                    }
                    if t.expired.contains(&effects::ABSORPTION) {
                        effects.set_absorb(0.0);
                        let _ = conn.send(&[absorption_packet(id, 0.0)]).await;
                    }
                    if !t.expired.is_empty() {
                        let mut out: Vec<Protocol> = t.expired.iter().map(|&e| effect_packet(id, e, None)).collect();
                        out.push(movement_packet(id, effects.speed()));
                        let _ = conn.send(&out).await;
                    }
                }
                // Drowning: 15s of air, then 2 damage a second. Lava 4/s, fire 1/s.
                if survival_like(&mode) && !hp.dead {
                    let was = air;
                    air = if head_in_water { (air - 20).max(-20) } else { 300 };
                    if air != was {
                        let _ = conn.send(&[spawn::air_packet(id, air.max(0))]).await;
                    }
                    if air <= -20 {
                        air = 0;
                        hurt(&mut conn, &server, id, &name, &mut hp, health::Damage::Drown, &mut inv, &effects).await;
                    }
                    if in_lava && !hp.dead {
                        hurt(&mut conn, &server, id, &name, &mut hp, health::Damage::Lava, &mut inv, &effects).await;
                    } else if in_fire && !hp.dead {
                        hurt(&mut conn, &server, id, &name, &mut hp, health::Damage::Fire, &mut inv, &effects).await;
                    }
                }
                if survival_like(&mode) && !hp.dead {
                    let before = (food.food, food.saturation, food.exhaustion);
                    let difficulty = *commands::DIFFICULTY.lock().unwrap();
                    match food.tick(difficulty, hp.health, rule_on) {
                        hunger::Step::Heal if hp.regenerate() => {
                            let _ = conn.send(&[spawn::health_packet(id, hp.health)]).await;
                        }
                        hunger::Step::Starve => hurt(&mut conn, &server, id, &name, &mut hp, health::Damage::Starve, &mut inv, &effects).await,
                        _ => {}
                    }
                    if (food.food, food.saturation, food.exhaustion) != before {
                        let _ = conn.send(&[food.packet(id)]).await;
                    }
                }
            }
            Some((line, reply_to)) = cmd_rx.recv() => {
                let mut args: Vec<&str> = line.split_whitespace().collect();
                if args.len() > 1 && args[1] == "@s" && matches!(args[0], "kill" | "clear" | "give" | "spawnpoint" | "tp" | "teleport" | "title" | "titleraw" | "tellraw" | "stopsound" | "damage" | "camerashake" | "transfer") {
                    args.remove(1);
                }
                let here = server.get(id).map_or((0.0, 0.0, 0.0), |p| (p.pos.0, p.pos.1 - 1.62, p.pos.2));
                let plugin_reply = match (server.get(id), args.split_first()) {
                    (Some(me), Some((cmd, rest))) => server.plugins.command(&server, &me, cmd, rest),
                    _ => None,
                };
                let denied = false; // checked when the command was issued
                let reply = if let Some(r) = plugin_reply { r } else if denied {
                    "§cYou do not have permission to use this command".to_string()
                } else if let Some(r) = commands::run(&server, &name, &args) { r } else { match args.as_slice() {
                    ["gamemode" | "gm", arg, ..] => {
                        match game_type_from(arg) {
                            Some(new) if server.get(id).is_some_and(|me| !server.plugins.gamemode_change(&server, &me, &format!("{new:?}").to_lowercase())) => {
                                let _ = new;
                                "§cGame mode change was cancelled".to_string()
                            }
                            Some(new) => {
                                set_game_mode(&mut conn, id, &new).await;
                                let msg = format!("Game mode: {new:?}");
                                mode = new;
                                server.mobs.set_targetable(id, survival_like(&mode));
                                save_mode(&name, &mode);
                                msg
                            }
                            None => "§cUse /gamemode creative|survival|adventure|spectator".into(),
                        }
                    }
                    ["summon", what] => match mobs::Kind::from_name(what) {
                        Some(kind) => {
                            let pos = server.get(id).map_or(server.world.spawn(), |p| p.pos);
                            server.mobs.spawn(&server, kind, (pos.0, pos.1 - 1.62, pos.2));
                            "Object successfully summoned".to_string()
                        }
                        None => format!("§cUnknown entity: {what}. Known: {}", mobs::Kind::ALL.iter().map(|k| k.identifier().trim_start_matches("minecraft:")).collect::<Vec<_>>().join(", ")),
                    },
                    ["effect", who, rest @ ..] => {
                        let target = if *who == "@s" || who.eq_ignore_ascii_case(&name) { Some(id) } else { server.find_by_name(who) };
                        match (target, rest) {
                            (None, _) => format!("§cNo player named {who}"),
                            (Some(t), ["clear"]) => {
                                effects::QUEUE.lock().unwrap().push((t, effects::Queued::Clear));
                                format!("Took all effects from {who}")
                            }
                            (Some(t), args) => match effects::parse(args) {
                                Some((e, x)) => {
                                    effects::QUEUE.lock().unwrap().push((t, effects::Queued::Give(e, x)));
                                    format!("Gave {} * {} to {who} for {} seconds", args[0], x.amplifier, x.ticks / 20)
                                }
                                None => "§cUse /effect <player> <effect> [seconds] [amplifier] [hideParticles] or /effect <player> clear".into(),
                            },
                        }
                    }
                    ["enchant", who, ench, rest @ ..] => {
                        let target = if *who == "@s" || who.eq_ignore_ascii_case(&name) { Some(id) } else { server.find_by_name(who) };
                        let lvl: Option<i16> = rest.first().map_or(Some(1), |l| l.parse().ok());
                        match (target, enchant::by_name(ench), lvl) {
                            (None, ..) => format!("§cNo player named {who}"),
                            (_, None, _) => format!("§cUnknown enchantment: {ench}"),
                            (_, _, None) => "§cUse /enchant <player> <enchantment> [level]".into(),
                            (_, Some((_, max)), Some(l)) if l < 1 || l > max => format!("§cLevel {l} is not supported by that enchantment (max {max})"),
                            (Some(t), Some((e, _)), Some(l)) => {
                                enchant::QUEUE.lock().unwrap().push((t, e, l));
                                format!("Enchanting held item of {who}")
                            }
                        }
                    }
                    ["xp", amount, rest @ ..] => {
                        let who = rest.first().copied().unwrap_or("@s");
                        let target = if who == "@s" || who.eq_ignore_ascii_case(&name) { Some(id) } else { server.find_by_name(who) };
                        let (levels, n) = match amount.strip_suffix(['L', 'l']) {
                            Some(l) => (true, l.parse::<i32>().ok()),
                            None => (false, amount.parse::<i32>().ok()),
                        };
                        match (target, n) {
                            (None, _) => format!("§cNo player named {who}"),
                            (_, None) => "§cUse /xp <amount>[L] [player]".into(),
                            (Some(t), Some(n)) if levels => {
                                if t == id {
                                    xp.add_levels(n);
                                    save_xp(&name, xp);
                                    let _ = conn.send(&[xp.packet(id)]).await;
                                } else {
                                    // Another player: give the points those levels take from level 0.
                                    let points: i32 = (0..n.max(0)).map(xp::needed).sum();
                                    let _ = server.events.send(Event::Xp { to: t, points });
                                }
                                format!("Gave {n} levels to {who}")
                            }
                            (Some(t), Some(n)) => {
                                let _ = server.events.send(Event::Xp { to: t, points: n });
                                format!("Gave {n} experience to {who}")
                            }
                        }
                    }
                    ["give", item, rest @ ..] if items::item_by_name(item).is_none() && rest.first().is_some_and(|i| items::item_by_name(i).is_some()) => {
                        // `/give <someone> <item>` that couldn't resolve the player.
                        format!("§cNo player was found: {item}")
                    }
                    ["title" | "titleraw", kind, text @ ..] => {
                        use bedrock::protocol::v712::packets::{SetTitlePacket, TitleType};
                        let raw = args[0] == "titleraw";
                        let text = if raw { rawtext(&text.join(" ")) } else { text.join(" ") };
                        let (title_type, times) = match *kind {
                            "title" => (Some(TitleType::Title), (10, 70, 20)),
                            "subtitle" => (Some(TitleType::Subtitle), (10, 70, 20)),
                            "actionbar" => (Some(TitleType::Actionbar), (10, 70, 20)),
                            "clear" => (Some(TitleType::Clear), (0, 0, 0)),
                            "reset" => (Some(TitleType::Reset), (0, 0, 0)),
                            "times" => {
                                let n: Vec<i32> = text.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                                (Some(TitleType::Times), (n.first().copied().unwrap_or(10), n.get(1).copied().unwrap_or(70), n.get(2).copied().unwrap_or(20)))
                            }
                            _ => (None, (0, 0, 0)),
                        };
                        match title_type {
                            Some(title_type) => {
                                let _ = conn.send(&[Protocol::SetTitlePacket(Box::new(SetTitlePacket {
                                    title_type,
                                    title_text: text.clone(),
                                    fade_in_time: times.0,
                                    stay_time: times.1,
                                    fade_out_time: times.2,
                                    xuid: String::new(),
                                    platform_online_id: String::new(),
                                    filtered_title_text: text,
                                }))]).await;
                                String::new()
                            }
                            None => "§cUse /title <player> title|subtitle|actionbar|clear|reset|times <text>".into(),
                        }
                    }
                    ["tellraw", json @ ..] => {
                        let _ = conn.send(&[chat_packet(String::new(), rawtext(&json.join(" ")))]).await;
                        String::new()
                    }
                    ["playsound", sound, rest @ ..] => {
                        // /playsound <sound> [player] [x y z] [volume] [pitch]
                        let rest = if rest.first() == Some(&"@s") { &rest[1..] } else { rest };
                        let at = if rest.len() >= 3 { coords(&rest[..3], here) } else { None }.unwrap_or(here);
                        let pitch = rest.get(4).and_then(|p| p.parse().ok()).unwrap_or(1.0);
                        let _ = conn.send(&[play_sound_pitch(sound, at, pitch)]).await;
                        String::new()
                    }
                    ["stopsound", rest @ ..] => {
                        let stop = Protocol::StopSoundPacket(Box::new(bedrock::protocol::v712::packets::StopSoundPacket {
                            sound_name: rest.first().map(|s| s.to_string()).unwrap_or_default(),
                            stop_all_sounds: rest.is_empty(),
                            stop_music_legacy: false,
                        }));
                        let _ = conn.send(&[stop]).await;
                        String::new()
                    }
                    ["camerashake", rest @ ..] => {
                        // /camerashake add <player> [intensity] [seconds] [positional|rotational] | stop <player>
                        use bedrock::protocol::v662::enums::{CameraShakeAction, CameraShakeType};
                        let (action, rest) = match rest.first() {
                            Some(&"stop") => (CameraShakeAction::Stop, &rest[1..]),
                            Some(&"add") => (CameraShakeAction::Add, &rest[1..]),
                            _ => (CameraShakeAction::Add, rest),
                        };
                        let rest = if rest.first() == Some(&"@s") { &rest[1..] } else { rest };
                        let shake = Protocol::CameraShakePacket(Box::new(bedrock::protocol::v662::packets::CameraShakePacket {
                            intensity: rest.first().and_then(|v| v.parse().ok()).unwrap_or(0.5),
                            seconds: rest.get(1).and_then(|v| v.parse().ok()).unwrap_or(1.0),
                            shake_type: if rest.get(2) == Some(&"rotational") { CameraShakeType::Rotational } else { CameraShakeType::Positional },
                            shake_action: action,
                        }));
                        let _ = conn.send(&[shake]).await;
                        String::new()
                    }
                    ["transfer", address, rest @ ..] => {
                        let port = rest.first().and_then(|p| p.parse().ok()).unwrap_or(19132);
                        let _ = conn.send(&[Protocol::TransferPlayerPacket(Box::new(bedrock::protocol::v2168::packets::TransferPlayerPacket {
                            server_address: address.to_string(),
                            server_port: port,
                            reload_world: false,
                                        gatherings_config: None,
                        }))]).await;
                        format!("Transferring to {address}:{port}")
                    }
                    ["damage", amount, ..] => match amount.parse::<f32>() {
                        Ok(d) if d >= 0.0 => {
                            if survival_like(&mode) {
                                hurt(&mut conn, &server, id, &name, &mut hp, health::Damage::Magic(d), &mut inv, &effects).await;
                            }
                            format!("Applied damage to {name}")
                        }
                        _ => "§cUse /damage <target> <amount>".into(),
                    },
                    ["setblock", x, y, z, block, rest @ ..] => match (coords(&[x, y, z], here), block_by_name(block)) {
                        (None, _) => "§cBad position".into(),
                        (_, None) => format!("§cUnknown block: {block}"),
                        (Some(p), Some(b)) => {
                            let p = (p.0.floor() as i32, p.1.floor() as i32, p.2.floor() as i32);
                            let current = server.world.get(p.0, p.1, p.2);
                            match rest.first().copied().unwrap_or("replace") {
                                "keep" if current != world::AIR => "§cCould not set the block".into(),
                                mode => {
                                    if mode == "destroy" {
                                        break_block(&server, id, p, Some(None), &[]);
                                    }
                                    server.set_block(id, p.0, p.1, p.2, b);
                                    "Block placed".into()
                                }
                            }
                        }
                    },
                    ["fill", x1, y1, z1, x2, y2, z2, block, rest @ ..] => match (coords(&[x1, y1, z1], here), coords(&[x2, y2, z2], here), block_by_name(block)) {
                        (Some(a), Some(b), Some(bl)) => {
                            let (lo, hi) = (
                                (a.0.min(b.0).floor() as i32, a.1.min(b.1).floor() as i32, a.2.min(b.2).floor() as i32),
                                (a.0.max(b.0).floor() as i32, a.1.max(b.1).floor() as i32, a.2.max(b.2).floor() as i32),
                            );
                            let n = (hi.0 - lo.0 + 1) as i64 * (hi.1 - lo.1 + 1) as i64 * (hi.2 - lo.2 + 1) as i64;
                            if n > 32_768 {
                                format!("§cToo many blocks in the specified area ({n} > 32768)")
                            } else {
                                let mode = rest.first().copied().unwrap_or("replace");
                                let mut changed = 0;
                                for x in lo.0..=hi.0 {
                                    for y in lo.1..=hi.1 {
                                        for z in lo.2..=hi.2 {
                                            let edge = x == lo.0 || x == hi.0 || y == lo.1 || y == hi.1 || z == lo.2 || z == hi.2;
                                            let cur = server.world.get(x, y, z);
                                            let want = match mode {
                                                "keep" if cur != world::AIR => continue,
                                                "outline" if !edge => continue,
                                                "hollow" if !edge => world::AIR,
                                                _ => bl,
                                            };
                                            if cur != want {
                                                server.set_block_quiet(id, x, y, z, want);
                                                changed += 1;
                                            }
                                        }
                                    }
                                }
                                format!("{changed} blocks filled")
                            }
                        }
                        (_, _, None) => format!("§cUnknown block: {block}"),
                        _ => "§cBad position".into(),
                    },
                    ["clone", x1, y1, z1, x2, y2, z2, dx, dy, dz, ..] => match (coords(&[x1, y1, z1], here), coords(&[x2, y2, z2], here), coords(&[dx, dy, dz], here)) {
                        (Some(a), Some(b), Some(d)) => {
                            let lo = (a.0.min(b.0).floor() as i32, a.1.min(b.1).floor() as i32, a.2.min(b.2).floor() as i32);
                            let hi = (a.0.max(b.0).floor() as i32, a.1.max(b.1).floor() as i32, a.2.max(b.2).floor() as i32);
                            let d = (d.0.floor() as i32, d.1.floor() as i32, d.2.floor() as i32);
                            let n = (hi.0 - lo.0 + 1) as i64 * (hi.1 - lo.1 + 1) as i64 * (hi.2 - lo.2 + 1) as i64;
                            if n > 32_768 {
                                format!("§cToo many blocks in the specified area ({n} > 32768)")
                            } else {
                                let mut copy = Vec::new();
                                for x in lo.0..=hi.0 {
                                    for y in lo.1..=hi.1 {
                                        for z in lo.2..=hi.2 {
                                            copy.push(((x - lo.0, y - lo.1, z - lo.2), server.world.get(x, y, z)));
                                        }
                                    }
                                }
                                for ((ox, oy, oz), b) in &copy {
                                    server.set_block_quiet(id, d.0 + ox, d.1 + oy, d.2 + oz, *b);
                                }
                                format!("{} blocks cloned", copy.len())
                            }
                        }
                        _ => "§cBad position".into(),
                    },
                    ["testforblock", x, y, z, block, ..] => match (coords(&[x, y, z], here), block_by_name(block)) {
                        (Some(p), Some(b)) => {
                            let cur = server.world.get(p.0.floor() as i32, p.1.floor() as i32, p.2.floor() as i32);
                            if blocks::entry(cur).map(|e| e.name.as_str()) == blocks::entry(b).map(|e| e.name.as_str()) {
                                "Successfully found the block".into()
                            } else {
                                format!("§cThe block is {}", blocks::entry(cur).map_or("unknown", |e| e.name.as_str()))
                            }
                        }
                        _ => "§cUse /testforblock <x> <y> <z> <block>".into(),
                    },
                    ["time", "add", n] => match n.parse::<i32>() {
                        Ok(n) => {
                            let t = server.time() + n;
                            server.set_time(t);
                            let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(clock_packet(t)) });
                            let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(time_packet(t)) });
                            format!("Added {n} to the time")
                        }
                        Err(_) => "§cUse /time add <ticks>".into(),
                    },
                    ["time", "query", what] => {
                        let t = server.time();
                        match *what {
                            "daytime" => format!("Daytime is {}", t.rem_euclid(24_000)),
                            "day" => format!("Day is {}", t.div_euclid(24_000)),
                            _ => format!("Gametime is {t}"),
                        }
                    }
                    ["toggledownfall"] => {
                        let now = *commands::WEATHER.lock().unwrap();
                        let next = if now == commands::Weather::Clear { commands::Weather::Rain } else { commands::Weather::Clear };
                        commands::set_weather(&server, next);
                        "Toggled downfall".into()
                    }
                    ["daylock" | "alwaysday", rest @ ..] => {
                        let lock = rest.first().is_none_or(|v| *v != "false");
                        use bedrock::protocol::v844::types::Type;
                        if let Some(mut r) = vanilla::game_rules().into_iter().find(|r| r.rule_name == "doDaylightCycle") {
                            r.rule_type = Type::Bool(!lock);
                            {
                                let mut rules = server.game_rules.lock().unwrap();
                                rules.retain(|c| c.rule_name != r.rule_name);
                                rules.push(r.clone());
                            }
                            let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(game_rules_packet(vec![r])) });
                        }
                        if lock {
                            server.set_time(5000);
                            let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(time_packet(5000)) });
                        }
                        if lock { "Day-Night cycle locked".into() } else { "Day-Night cycle unlocked".into() }
                    }
                    ["summon", what, x, y, z, ..] => match (mobs::Kind::from_name(what), coords(&[x, y, z], here)) {
                        (Some(kind), Some(p)) => {
                            server.mobs.spawn(&server, kind, p);
                            "Object successfully summoned".into()
                        }
                        (None, _) => format!("§cUnknown entity: {what}"),
                        _ => "§cBad position".into(),
                    },
                    ["spawnpoint", x, y, z] => match coords(&[x, y, z], here) {
                        Some(p) => {
                            commands::set_spawn(Some(&name), (p.0, p.1 + 1.62, p.2));
                            format!("Set spawn point to {:.0} {:.0} {:.0}", p.0, p.1, p.2)
                        }
                        None => "§cBad position".into(),
                    },
                    ["clear", item, ..] => {
                        let n = inv.clear_item(item);
                        let _ = conn.send(&inv.all_packets()).await;
                        format!("Cleared the inventory of {name}, removing {n} items")
                    }
                    ["scoreboard", rest @ ..] => ui::command(&server, &name, rest),
                    ["kill"] => {
                        hurt(&mut conn, &server, id, &name, &mut hp, health::Damage::Kill, &mut inv, &effects).await;
                        String::new()
                    }
                    ["clear"] => {
                        let n = inv.clear();
                        let _ = conn.send(&inv.all_packets()).await;
                        format!("Cleared the inventory of {name}, removing {n} items")
                    }
                    ["spawnpoint"] => {
                        let pos = server.get(id).map_or(server.world.spawn(), |p| p.pos);
                        commands::set_spawn(Some(&name), pos);
                        format!("Set spawn point to {:.0} {:.0} {:.0}", pos.0, pos.1 - 1.62, pos.2)
                    }
                    ["setworldspawn"] => {
                        let pos = server.get(id).map_or(server.world.spawn(), |p| p.pos);
                        commands::set_spawn(None, pos);
                        format!("Set the world spawn point to {:.0} {:.0} {:.0}", pos.0, pos.1 - 1.62, pos.2)
                    }
                    ["tp" | "teleport", who] => match server.find_by_name(who).and_then(|t| server.get(t)) {
                        Some(t) => {
                            let (rot, head) = server.get(id).map_or(((0.0, 0.0), 0.0), |p| (p.rot, p.head_yaw));
                            let mut tp = move_player(id, t.pos, rot, head);
                            if let Protocol::MovePlayerPacket(m) = &mut tp {
                                m.position_mode = PlayerPositionMode::Teleport;
                                m.teleport_data = Some(MovePlayerTeleportData { teleportation_cause: 0, source_actor_type: 0 });
                            }
                            let _ = conn.send(&[tp]).await;
                            server.moved(id, t.pos, rot, head);
                            format!("Teleported {name} to {}", t.name)
                        }
                        None => format!("§cNo player was found: {who}"),
                    },
                    ["tp" | "teleport", x, y, z] => match coords(&[x, y, z], here) {
                        Some((x, y, z)) => {
                            let eye = (x, y + 1.62, z);
                            let (rot, head) = server.get(id).map_or(((0.0, 0.0), 0.0), |p| (p.rot, p.head_yaw));
                            let mut tp = move_player(id, eye, rot, head);
                            if let Protocol::MovePlayerPacket(m) = &mut tp {
                                m.position_mode = PlayerPositionMode::Teleport;
                                m.teleport_data = Some(MovePlayerTeleportData { teleportation_cause: 0, source_actor_type: 0 });
                            }
                            let _ = conn.send(&[tp]).await;
                            server.moved(id, eye, rot, head);
                            format!("Teleported to {x:.1} {y:.1} {z:.1}")
                        }
                        None => "§cUse /tp <x> <y> <z>".into(),
                    },
                    ["time", "set", value] => {
                        let t = match *value {
                            "day" => Some(1000),
                            "noon" => Some(6000),
                            "night" => Some(13000),
                            "midnight" => Some(18000),
                            n => n.parse().ok(),
                        };
                        match t {
                            Some(t) => {
                                server.set_time(t);
                                let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(clock_packet(t)) });
                                let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(time_packet(t)) });
                                format!("Time set to {t}")
                            }
                            None => "§cUse /time set day|noon|night|midnight|<ticks>".into(),
                        }
                    }
                    ["gamerule", rule, rest @ ..] => {
                        let known = vanilla::game_rules().into_iter().find(|r| r.rule_name.eq_ignore_ascii_case(rule));
                        match (known, rest.first()) {
                            (None, _) => format!("§cUnknown game rule: {rule}"),
                            (Some(r), None) => {
                                let current = server.game_rules.lock().unwrap().iter().find(|c| c.rule_name == r.rule_name).cloned().unwrap_or(r);
                                format!("{} = {:?}", current.rule_name, current.rule_type)
                            }
                            (Some(mut r), Some(value)) => {
                                use bedrock::protocol::v844::types::Type;
                                let parsed = match r.rule_type {
                                    Type::Bool(_) => value.parse::<bool>().ok().map(Type::Bool),
                                    Type::Int(_) => value.parse::<i32>().ok().map(Type::Int),
                                    Type::Float(_) => value.parse::<f32>().ok().map(Type::Float),
                                };
                                match parsed {
                                    Some(v) => {
                                        r.rule_type = v;
                                        {
                                            let mut rules = server.game_rules.lock().unwrap();
                                            rules.retain(|c| c.rule_name != r.rule_name);
                                            rules.push(r.clone());
                                        }
                                        let msg = format!("Game rule {} set to {value}", r.rule_name);
                                        let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(game_rules_packet(vec![r])) });
                                        msg
                                    }
                                    None => format!("§cBad value for {}: {value}", r.rule_name),
                                }
                            }
                        }
                    }
                    ["give", item, rest @ ..] => {
                        let count = rest.first().and_then(|c| c.parse::<i8>().ok()).unwrap_or(1);
                        match (items::item_by_name(item), inv.first_empty()) {
                            (Some(it), Some(slot)) => {
                                let count = count.clamp(1, items::max_stack(it.id));
                                let stack = inv.set_slot(slot, it, count).unwrap().to_network();
                                let _ = conn.send(&[Protocol::InventorySlotPacket(Box::new(InventorySlotPacket {
                                    container_id: 0, slot: slot as u32, container_name_data: None, storage_item: None, item: stack,
                                }))]).await;
                                format!("Gave {count} {item}")
                            }
                            (None, _) => format!("§cUnknown item: {item}"),
                            (_, None) => "§cYour inventory is full".into(),
                        }
                    }
                    _ => format!("§cUnknown command: {}. Please check that the command exists and that you have permission to use it.", args.first().unwrap_or(&"")),
                } };
                if !reply.is_empty() {
                    match reply_to {
                        Some(to) => {
                            let _ = server.events.send(Event::Tell { to, message: reply });
                        }
                        None => {
                            let _ = conn.send(&[chat_packet(String::new(), reply)]).await;
                        }
                    }
                }
            }
            event = events.recv() => {
                let packets = match event {
                    Ok(Event::Chat { from, message }) => vec![chat_packet(from, message)],
                    Ok(Event::Tell { to, message }) if to == id => vec![chat_packet(String::new(), message)],
                    Ok(Event::Relay { from, packet }) if from != id => vec![*packet],
                    Ok(Event::SaveAll) => {
                        if let Some(me) = server.get(id) {
                            save_player(&name, me.pos, &inv);
                        }
                        continue;
                    }
                    Ok(Event::Damage { to, damage, knock }) if to == id => {
                        if survival_like(&mode) && !hp.dead {
                            hurt(&mut conn, &server, id, &name, &mut hp, damage, &mut inv, &effects).await;
                            let motion = Protocol::SetActorMotionPacket(Box::new(bedrock::protocol::v662::packets::SetActorMotionPacket {
                                target_runtime_id: ActorRuntimeID(id as u64),
                                motion: knock,
                                server_tick: 0,
                            }));
                            let _ = conn.send(&[motion]).await;
                        }
                        continue;
                    }
                    Ok(Event::Effect { to, effect, amplifier, ticks }) if to == id => {
                        if survival_like(&mode) && !hp.dead {
                            let x = effects::Effect { amplifier, ticks, particles: true };
                            give_effect(&mut conn, &server, id, &name, &mut hp, &mut food, &mut effects, &mut inv, effect, x).await;
                        }
                        continue;
                    }
                    Ok(Event::Packet { to, packet }) if to == id => vec![*packet],
                    Ok(Event::RunAs { to, line, reply_to }) if to == id => {
                        let _ = cmd_tx.send((line, Some(reply_to)));
                        continue;
                    }
                    Ok(Event::Xp { to, points }) if to == id => {
                        xp.add_points(points);
                        save_xp(&name, xp);
                        let mut out = vec![xp.packet(id)];
                        if points > 0 && let Some(me) = server.get(id) {
                            out.push(play_sound("random.orb", me.pos));
                        }
                        let _ = conn.send(&out).await;
                        continue;
                    }
                    Ok(Event::Teleport { id: who, feet }) if who == id => {
                        let eye = (feet.0, feet.1 + 1.62, feet.2);
                        let (rot, head) = server.get(id).map_or(((0.0, 0.0), 0.0), |p| (p.rot, p.head_yaw));
                        let mut tp = move_player(id, eye, rot, head);
                        if let Protocol::MovePlayerPacket(m) = &mut tp {
                            m.position_mode = PlayerPositionMode::Teleport;
                            m.teleport_data = Some(MovePlayerTeleportData { teleportation_cause: 2, source_actor_type: 0 });
                        }
                        let _ = conn.send(&[tp]).await;
                        server.moved(id, eye, rot, head);
                        continue;
                    }
                    Ok(Event::Kick { id: who, message, reason }) if who == id || who == 0 => {
                        let _ = conn.send(&[disconnect_packet(reason, &message)]).await;
                        println!("{name}: kicked ({reason:?}): {message}");
                        break;
                    }
                    Ok(Event::Block { x, y, z, id: block, old, by: _, effects }) => {
                        let center = (x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                        let mut packets = vec![update_block(x, y, z, block)];
                        // The acting client shows nothing itself, so everyone gets the effects:
                        // breaking = event 2001 (particles + sound), placing = "place" sound.
                        if effects && block == world::AIR {
                            packets.push(Protocol::LevelEventPacket(Box::new(LevelEventPacket { event_id: 2001, position: center, data: old })));
                        } else if effects {
                            packets.push(Protocol::LevelSoundEventPacket(Box::new(LevelSoundEventPacket {
                                event_name: "place".into(),
                                position: center,
                                data: block,
                                actor_identifier: String::new(),
                                is_baby_mob: false,
                                is_global: false,
                                entity_unique_id: u64::MAX, // -1: no entity
                                fire_at_position: None,
                            })));
                        }
                        packets
                    }
                    Ok(Event::Join(other)) if other != id => match server.get(other) {
                        Some(p) => vec![player_list_add(&p), add_player(&p)],
                        None => continue,
                    },
                    Ok(Event::Move { id: other, pos, rot, head_yaw }) if other != id => {
                        vec![move_player(other, pos, rot, head_yaw)]
                    }
                    Ok(Event::Leave { id: other, uuid }) if other != id => vec![
                        Protocol::RemoveActorPacket(Box::new(RemoveActorPacket {
                            target_actor_id: ActorUniqueID(other),
                        })),
                        Protocol::PlayerListPacket(Box::new(PlayerListPacket {
                            entries: vec![PlayerListEntry::Remove { uuid }],
                        })),
                    ],
                    // Fell behind the event stream; positions catch up on the next move.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                    _ => continue,
                };
                if let Err(e) = conn.send(&packets).await {
                    println!("{name}: send failed: {e}");
                    break;
                }
            }
        }
    }

    if let (Some(pos), Some(slots)) = (open_container.take(), inv.open.take()) {
        close_open(&server, pos, slots, open_pair.take());
    }
    if let Some(me) = server.get(id) {
        save_player(&name, me.pos, &inv);
        server.plugins.leave(&server, &me);
        ui::left(id);
    }
    server.leave(id);
    commands::op_ids().lock().unwrap().remove(&id);
    server.mobs.set_held(id, None);
    server.mobs.set_targetable(id, false);
    server.mobs.owner_left(id, mobs::owner_key(&name));
    println!("{name} left");
    let _ = server.events.send(Event::Chat {
        from: String::new(),
        message: format!("§e{name} left the game"),
    });
}

fn survival_like(mode: &GameType) -> bool {
    matches!(mode, GameType::Survival | GameType::Adventure)
}

/// A boolean game rule's current value (`default` if never changed).
fn game_rule_on(server: &players::Server, rule: &str, default: bool) -> bool {
    use bedrock::protocol::v844::types::Type;
    server.game_rules.lock().unwrap().iter().find(|r| r.rule_name == rule).map_or(default, |r| matches!(r.rule_type, Type::Bool(true)))
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

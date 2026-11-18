#![allow(clippy::too_many_arguments, clippy::type_complexity)] // game code passes a lot of state around

mod blocks;
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

type Protocol = V2225;

const PROTOCOL: i32 = Protocol::PROTOCOL_VERSION.cast_signed();

async fn recv_one(conn: &mut Connection<Protocol>) -> Result<Protocol, ConnectionError> {
    let batch = conn.recv().await?;
    batch
        .into_iter()
        .next()
        .ok_or(ConnectionError::ConnectionClosed)
}

fn play_status(status: PlayStatus) -> Protocol {
    Protocol::PlayStatusPacket(Box::new(PlayStatusPacket { status }))
}

async fn login(
    conn: &mut Connection<Protocol>,
) -> Result<Login, Box<dyn std::error::Error + Send + Sync>> {
    let settings = NetworkSettings::default();
    let options = LoginOptions::default().require_authentication(false);

    let Protocol::RequestNetworkSettingsPacket(request) = recv_one(conn).await? else {
        return Err("expected a network settings request".into());
    };
    if let Err(error) = check_protocol(request.client_network_version, PROTOCOL) {
        let status = if request.client_network_version < PROTOCOL {
            PlayStatus::LoginFailedClientOld
        } else {
            PlayStatus::LoginFailedServerOld
        };
        conn.send(&[play_status(status)]).await?;
        return Err(error.into());
    }

    let algorithm = match settings.compression {
        Compression::Zlib { .. } => PacketCompressionAlgorithm::ZLib,
        Compression::Snappy { .. } => PacketCompressionAlgorithm::Snappy,
        Compression::None => PacketCompressionAlgorithm::None,
    };
    conn.send(&[Protocol::NetworkSettingsPacket(Box::new(
        NetworkSettingsPacket {
            compression_threshold: settings.threshold(),
            compression_algorithm: algorithm,
            client_throttle_enabled: false,
            client_throttle_threshold: 0,
            client_throttle_scalar: 0.0,
        },
    ))])
    .await?;
    conn.enable_compression(settings.compression);

    let Protocol::LoginPacket(packet) = recv_one(conn).await? else {
        return Err("expected a login".into());
    };
    check_protocol(packet.client_network_version, PROTOCOL)?;

    let login = options.verify(&packet.connection_request)?;
    // NetherNet is already encrypted end to end (DTLS) and 1.26 clients skip the handshake on
    // it; plain RakNet (UDP) gets the vanilla AES handshake so it isn't readable on the wire.
    if matches!(conn.get_transport_conn(), bedrock::network::tokio::transport::TransportLayerConnection::RakNet(_)) {
        let (token, encryption) = begin_encryption(&login)?;
        conn.send(&[Protocol::ServerToClientHandshakePacket(Box::new(ServerToClientHandshakePacket {
            handshake_web_token: token,
        }))])
        .await?;
        conn.enable_encryption(encryption);
        let Protocol::ClientToServerHandshakePacket(_) = recv_one(conn).await? else {
            return Err("expected the client handshake".into());
        };
    }
    conn.send(&[play_status(PlayStatus::LoginSuccess)]).await?;
    Ok(login)
}

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

/// Random ticks near players every 250 ms: 3 blocks per subchunk, 3 chunks out, 32 blocks tall.
/// Real game does that every tick; 5x slower keeps forests cheap.
fn random_ticks(server: &players::Server, rng: &mut u64) {
    let mut next = || {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 7;
        *rng ^= *rng << 17;
        *rng as u32
    };
    let raining = *commands::WEATHER.lock().unwrap() != commands::Weather::Clear;
    let speed = {
        use bedrock::protocol::v844::types::Type;
        server.game_rules.lock().unwrap().iter().find(|r| r.rule_name == "randomTickSpeed").map_or(1, |r| match r.rule_type {
            Type::Int(n) => n.clamp(0, 4096),
            _ => 1,
        })
    };
    let mut seen = std::collections::HashSet::new();
    for p in server.all() {
        let (cx, cz, sy) = ((p.pos.0.floor() as i32) >> 4, (p.pos.2.floor() as i32) >> 4, (p.pos.1.floor() as i32) >> 4);
        for x in cx - 3..=cx + 3 {
            for z in cz - 3..=cz + 3 {
                for s in sy - 2..=sy + 2 {
                    if !(-4..20).contains(&s) || !seen.insert((x, s, z)) {
                        continue;
                    }
                    // Bedrock: randomTickSpeed blocks per sub-chunk per game tick; this runs every 5 ticks.
                    for _ in 0..speed * 5 {
                        let r = next();
                        let pos = (x * 16 + (r & 15) as i32, s * 16 + ((r >> 4) & 15) as i32, z * 16 + ((r >> 8) & 15) as i32);
                        let snowy = raining && server.world.is_snowy(pos.0, pos.2);
                        let before = server.world.get(pos.0, pos.1, pos.2);
                        for ((bx, by, bz), b) in growth::random_tick(&server.world, pos, next(), raining, snowy) {
                            // Decaying leaves drop saplings/apples.
                            if (bx, by, bz) == pos && b == world::AIR && game_rule_on(server, "doTileDrops", true) {
                                for (item, count) in drops::drops(before, pos, None) {
                                    server.entities.drop_item(server, item, count, (bx as f32 + 0.5, by as f32 + 0.5, bz as f32 + 0.5));
                                }
                            }
                            server.set_block_quiet(0, bx, by, bz, b);
                            if blocks::entry(b).is_some_and(|e| e.name == "minecraft:fire") {
                                growth::FIRES.lock().unwrap().push((bx, by, bz));
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Command coordinates: numbers or `~`/`~n` relative to `base` (feet).
fn coords(a: &[&str], base: (f32, f32, f32)) -> Option<(f32, f32, f32)> {
    let one = |s: &str, b: f32| match s.strip_prefix('~') {
        Some("") => Some(b),
        Some(r) => r.parse::<f32>().ok().map(|v| b + v),
        None => s.parse::<f32>().ok().map(|v| if s.contains('.') { v } else { v + 0.5 }),
    };
    let y = match a[1].strip_prefix('~') {
        Some("") => Some(base.1),
        Some(r) => r.parse::<f32>().ok().map(|v| base.1 + v),
        None => a[1].parse::<f32>().ok().map(|v| v + dims::base(dims::of_f(base.1)) as f32),
    };
    Some((one(a[0], base.0)?, y?, one(a[2], base.2)?))
}

/// A block by name ("stone", "minecraft:oak_log"), its default state.
fn block_by_name(n: &str) -> Option<i32> {
    let full = if n.contains(':') { n.to_string() } else { format!("minecraft:{n}") };
    if full == "minecraft:air" {
        return Some(world::AIR);
    }
    blocks::any_state(&full)
}

/// The plain text of a rawtext JSON message (`{"rawtext":[{"text":"..."}]}`); plain text as is.
fn rawtext(json: &str) -> String {
    let mut out = String::new();
    let mut rest = json;
    while let Some(i) = rest.find("\"text\"") {
        rest = &rest[i + 6..];
        let Some(q) = rest.find('"') else { break };
        rest = &rest[q + 1..];
        let mut it = rest.char_indices();
        let mut end = rest.len();
        while let Some((j, c)) = it.next() {
            match c {
                '\\' => {
                    if let Some((_, n)) = it.next() {
                        out.push(if n == 'n' { '\n' } else { n });
                    }
                }
                '"' => {
                    end = j;
                    break;
                }
                c => out.push(c),
            }
        }
        rest = &rest[(end + 1).min(rest.len())..];
    }
    if out.is_empty() && !json.trim_start().starts_with('{') { json.to_string() } else { out }
}

/// Opens a villager's trade screen (PNX `EntityVillagerV2.updateTrades`).
fn trade_packet(window: i8, villager: i64, player: i64, job: &villagers::Job, tags: HashMap<String, nbtx::Value>) -> Protocol {
    Protocol::UpdateTradePacket(Box::new(bedrock::protocol::v662::packets::UpdateTradePacket {
        container_id: ContainerID(window),
        container_type: ContainerType::Trade,
        size: 0,
        trade_tier: job.tier as i32 - 1,
        target_actor_id: ActorUniqueID(villager),
        last_trading_player_id: ActorUniqueID(player),
        display_name: villagers::title(job.profession).into(),
        use_new_trade_ui: true,
        using_economy_trade: true,
        data_tags: tags,
    }))
}

/// Bookshelves in the ring two blocks out, on the table level and one up.
/// TODO: check for blocks in between.
fn bookshelves(w: &world::World, (x, y, z): (i32, i32, i32)) -> i32 {
    let mut n = 0;
    for dy in 0..=1 {
        for dx in -2..=2i32 {
            for dz in -2..=2i32 {
                if (dx.abs() == 2 || dz.abs() == 2) && blocks::entry(w.get(x + dx, y + dy, z + dz)).is_some_and(|e| e.name == "minecraft:bookshelf") {
                    n += 1;
                }
            }
        }
    }
    n.min(15)
}

fn enchant_options(offers: &[(u32, tables::Offer)]) -> Protocol {
    use bedrock::protocol::ProtoCodec;
    use bedrock::protocol::v662::packets::{OptionsEntry, PlayerEnchantOptionsPacket};
    use bedrock::protocol::v662::types::{ItemEnchant, ItemEnchants};
    const NAMES: [&str; 12] = ["the elder scrolls", "klaatu berata", "free enchant", "abyss", "phnglui", "mglw nafh", "cthulhu", "rlyeh", "wgah nagl", "fhtagn", "bless", "grumm"];
    let options = offers
        .iter()
        .map(|(net_id, o)| OptionsEntry {
            cost: o.level as u32,
            enchants: ItemEnchants {
                slot: 0,
                enchants_for_given_activation: o
                    .enchants
                    .iter()
                    .filter_map(|&(e, l)| {
                        let t = ProtoCodec::deserialize(&mut &[e as u8][..]).ok()?;
                        Some(ItemEnchant { enchant_type: t, enchant_level: l as i8 })
                    })
                    .collect(),
            },
            enchant_name: NAMES[*net_id as usize % NAMES.len()].into(),
            enchant_net_id: *net_id,
        })
        .collect();
    Protocol::PlayerEnchantOptionsPacket(Box::new(PlayerEnchantOptionsPacket { options }))
}

fn redstone_tick(server: &players::Server, plates: &mut HashMap<(i32, i32, i32), Instant>) {
    // Plates stay pressed while someone stands on them, then 1 s more (vanilla 20 ticks).
    for p in server.all() {
        let feet = (p.pos.0.floor() as i32, (p.pos.1 - 1.62 + 0.05).floor() as i32, p.pos.2.floor() as i32);
        let b = server.world.get(feet.0, feet.1, feet.2);
        if blocks::entry(b).is_some_and(|e| e.name.ends_with("pressure_plate"))
            && (plates.insert(feet, Instant::now()).is_none() || !matches!(blocks::state(b, "redstone_signal"), Some(blocks::State::Int(15)))) {
                server.set_block_quiet(0, feet.0, feet.1, feet.2, blocks::with(b, &[("redstone_signal", blocks::State::Int(15))]));
                let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound("random.click", (feet.0 as f32 + 0.5, feet.1 as f32, feet.2 as f32 + 0.5))) });
            }
    }
    plates.retain(|&q, t| {
        if t.elapsed() < std::time::Duration::from_secs(1) {
            return true;
        }
        let b = server.world.get(q.0, q.1, q.2);
        if blocks::entry(b).is_some_and(|e| e.name.ends_with("pressure_plate")) {
            server.set_block_quiet(0, q.0, q.1, q.2, blocks::with(b, &[("redstone_signal", blocks::State::Int(0))]));
        }
        false
    });
    server.scheduler.tick(server);
    let step = redstone::tick(&server.world, &|p| server.containers.comparator(&server.world, p));
    for (p, b) in step.blocks {
        server.set_block_quiet(0, p.0, p.1, p.2, b);
    }
    for p in step.dispense {
        dispense(server, p);
    }
    for p in step.ignite {
        mobs::ignite_tnt(server, p, 40);
    }
    for p in step.notes {
        apply_use(server, 0, p, &useblock::play_note(&server.world, p));
    }
    for (p, s) in step.sounds {
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound(s, (p.0 as f32 + 0.5, p.1 as f32 + 0.5, p.2 as f32 + 0.5))) });
    }
    for (p, ext, sticky) in step.pistons {
        let data = Protocol::BlockActorDataPacket(Box::new(bedrock::protocol::v662::packets::BlockActorDataPacket {
            block_position: NetworkBlockPosition { x: p.0, y: p.1, z: p.2 },
            actor_data_tags: redstone::piston_data(p, ext, sticky),
        }));
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(data) });
    }
}

/// Dispenser/dropper firing (PNX BlockDispenser). Droppers pass items on; dispensers
/// shoot, place liquids, light TNT/fire, bone meal, or drop the item.
fn dispense(server: &players::Server, p: (i32, i32, i32)) {
    let b = server.world.get(p.0, p.1, p.2);
    let Some(kind) = containers::kind(b) else { return };
    let face = match blocks::state(b, "facing_direction") {
        Some(blocks::State::Int(f)) => *f as u8,
        _ => 0,
    };
    let (dx, dy, dz) = placement::face_offset(face);
    let front = (p.0 + dx, p.1 + dy, p.2 + dz);
    let roll = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
    let Some(st) = server.containers.take_one(p, roll) else {
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(level_event(1001, p)) }); // empty click
        return;
    };
    let at = (front.0 as f32 + 0.5, front.1 as f32 + 0.3, front.2 as f32 + 0.5);
    let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(level_event(1000, p)) });
    let name = items::name_of(st.item.id).unwrap_or("");
    let fire_dir = |dx: i32, dy: i32, dz: i32| {
        let yaw = (-(dx as f32)).atan2(dz as f32).to_degrees();
        let pitch = -(dy as f32) * 90.0;
        (pitch, yaw)
    };
    if kind == containers::Kind::Dropper {
        if !server.containers.put_one(&server.world, front, &st) {
            server.entities.drop_item_vel(server, st.item, 1, at, (dx as f32 * 0.2, dy as f32 * 0.2 + 0.1, dz as f32 * 0.2));
        }
        return;
    }
    let eye = (p.0 as f32 + 0.5 + dx as f32 * 0.7, p.1 as f32 + 0.5 + dy as f32 * 0.7 + 1.62 - 0.3, p.2 as f32 + 0.5 + dz as f32 * 0.7);
    let back = |item: &str| {
        if let Some(it) = items::item_by_name(item) {
            server.containers.put_one(&server.world, p, &inventory::Stack { item: it, count: 1, net_id: 0 });
        }
    };
    match name {
        "minecraft:arrow" => server.mobs.player_shoot(server, 0, eye, fire_dir(dx, dy, dz), 1.0, false, 0, 0, false, items::item_by_name("minecraft:arrow")),
        "minecraft:snowball" | "minecraft:egg" => mobs::throw(server, mobs::Throw::from_item(name).unwrap(), 0, eye, fire_dir(dx, dy, dz)),
        "minecraft:splash_potion" | "minecraft:lingering_potion" => mobs::throw(server, mobs::Throw::Potion(st.item.aux_value), 0, eye, fire_dir(dx, dy, dz)),
        "minecraft:water_bucket" | "minecraft:lava_bucket" if server.world.get(front.0, front.1, front.2) == world::AIR => {
            let k = if name == "minecraft:water_bucket" { fluids::Kind::Water } else { fluids::Kind::Lava };
            server.set_block(0, front.0, front.1, front.2, k.source());
            back("minecraft:bucket");
        }
        "minecraft:bucket" if server.world.get(front.0, front.1, front.2) == fluids::Kind::Water.source() => {
            server.set_block(0, front.0, front.1, front.2, world::AIR);
            back("minecraft:water_bucket");
        }
        "minecraft:tnt" => {
            if let Some(t) = blocks::any_state("minecraft:tnt") {
                server.set_block_quiet(0, front.0, front.1, front.2, t);
                mobs::ignite_tnt(server, front, 40);
            }
        }
        "minecraft:flint_and_steel" | "minecraft:fire_charge" if server.world.get(front.0, front.1, front.2) == world::AIR => {
            if let Some(f) = blocks::any_state("minecraft:fire") {
                server.set_block(0, front.0, front.1, front.2, f);
                growth::FIRES.lock().unwrap().push(front);
            }
            if name == "minecraft:flint_and_steel" {
                back(name); // TODO: dispensers should wear out flint and steel
            }
        }
        "minecraft:bone_meal" => {
            match growth::bone_meal(&server.world, front, roll) {
                Some(ch) => {
                    for ((x, y, z), b) in ch {
                        server.set_block(0, x, y, z, b);
                    }
                }
                None => back(name),
            }
        }
        _ => server.entities.drop_item_vel(server, st.item, 1, at, (dx as f32 * 0.2, dy as f32 * 0.2 + 0.1, dz as f32 * 0.2)),
    }
}

/// Hoppers move items, then each pulls in items lying on top of it.
fn hopper_ticks(server: &players::Server) {
    server.containers.tick_hoppers(&server.world);
    let hoppers: Vec<_> = server.containers.hoppers.lock().unwrap().iter().copied().collect();
    for h in hoppers {
        server.entities.suck(server, h, |stack| server.containers.hopper_insert(h, stack));
    }
}

/// Lightning near players, ~1 strike per 100000 ticks per chunk (PNX Level.updateWeather).
/// Hurts anyone within 3 blocks and starts a fire.
fn lightning(server: &players::Server, rng: &mut u64) {
    let mut next = || {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 7;
        *rng ^= *rng << 17;
        *rng
    };
    for p in server.all() {
        if dims::of_f(p.pos.1) != dims::OVERWORLD || next() % 70 != 0 {
            continue; // ~289 chunks around a player, 5 ticks per call
        }
        let x = p.pos.0.floor() as i32 + (next() % 97) as i32 - 48;
        let z = p.pos.2.floor() as i32 + (next() % 97) as i32 - 48;
        let mut y = 318;
        while y > -63 && server.world.get(x, y, z) == world::AIR {
            y -= 1;
        }
        let at = (x as f32 + 0.5, (y + 1) as f32, z as f32 + 0.5);
        let bolt = players::next_id();
        let add = Protocol::AddActorPacket(Box::new(bedrock::protocol::v2225::packets::AddActorPacket {
            target_actor_id: ActorUniqueID(bolt),
            target_runtime_id: ActorRuntimeID(bolt as u64),
            actor_type: "minecraft:lightning_bolt".into(),
            position: at,
            velocity: (0.0, 0.0, 0.0),
            rotation: (0.0, 0.0),
            y_head_rotation: 0.0,
            y_body_rotation: 0.0,
            attributes: vec![],
            actor_data: vec![],
            synced_properties: bedrock::protocol::v662::types::PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
            actor_links: vec![],
            passenger_of_block_data: None,
        }));
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(add) });
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound("ambient.weather.thunder", at)) });
        server.scheduler.run_later(20, move |s| {
            let _ = s.events.send(players::Event::Relay { from: 0, packet: Box::new(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(bolt) }))) });
        });
        for q in server.all() {
            let d = ((q.pos.0 - at.0).powi(2) + (q.pos.1 - 1.62 - at.1).powi(2) + (q.pos.2 - at.2).powi(2)).sqrt();
            if d < 3.0 {
                let _ = server.events.send(players::Event::Damage { to: q.id, damage: health::Damage::Explosion(5.0, "Lightning"), knock: (0.0, 0.1, 0.0) });
            }
        }
        if game_rule_on(server, "doFireTick", true)
            && server.world.get(x, y + 1, z) == world::AIR
            && let Some(fire) = blocks::any_state("minecraft:fire")
        {
            server.set_block_quiet(0, x, y + 1, z, fire);
            growth::FIRES.lock().unwrap().push((x, y + 1, z));
        }
    }
}

/// Fire spread, roughly every 30 game ticks like PNX.
fn fire_ticks(server: &players::Server, rng: &mut u64) {
    let fires = std::mem::take(&mut *growth::FIRES.lock().unwrap());
    let raining = *commands::WEATHER.lock().unwrap() != commands::Weather::Clear;
    let tick = game_rule_on(server, "doFireTick", true);
    let mut keep = Vec::new();
    for p in fires {
        let changes = if tick {
            growth::fire_tick(&server.world, p, raining, |n| {
                *rng ^= *rng << 13;
                *rng ^= *rng >> 7;
                *rng ^= *rng << 17;
                (*rng % n.max(1) as u64) as u32
            })
        } else {
            vec![]
        };
        if !changes.iter().any(|(q, b)| *q == p && *b == world::AIR) && blocks::entry(server.world.get(p.0, p.1, p.2)).is_some_and(|e| e.name == "minecraft:fire") {
            keep.push(p);
        }
        for (q, b) in changes {
            let old = server.world.get(q.0, q.1, q.2);
            if q != p && blocks::entry(old).is_some_and(|e| e.name == "minecraft:tnt") {
                mobs::ignite_tnt(server, q, 40);
                continue;
            }
            server.set_block_quiet(0, q.0, q.1, q.2, b);
            if q != p && blocks::entry(b).is_some_and(|e| e.name == "minecraft:fire") {
                keep.push(q);
            }
            if q != p && b == world::AIR && old != world::AIR {
                pop_unsupported(server, 0, q);
            }
        }
    }
    keep.sort_unstable();
    keep.dedup();
    growth::FIRES.lock().unwrap().extend(keep);
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

/// A new level from server.properties (level-type, level-seed, flat-layers).
fn new_world() -> std::io::Result<world::World> {
    let cfg = config::get();
    let bad = |why: String| std::io::Error::new(std::io::ErrorKind::InvalidInput, why);
    let seed = config::seed_from(&cfg.level_seed)
        .unwrap_or_else(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64));
    Ok(match cfg.level_type.as_str() {
        "normal" | "default" | "overworld" | "infinite" => world::World::vanilla(seed),
        "legacy" => world::World::normal(seed),
        "flat" => world::World::flat(config::flat_layers(&cfg.flat_layers).ok_or_else(|| bad(format!("bad flat-layers: {}", cfg.flat_layers)))?),
        "void" => world::World::flat(vec![]),
        other => world::World::custom(other, seed).ok_or_else(|| bad(format!("unknown level-type: {other}")))?,
    })
}

fn game_type_from(s: &str) -> Option<GameType> {
    match s {
        "creative" | "c" | "1" => Some(GameType::Creative),
        "survival" | "s" | "0" => Some(GameType::Survival),
        "adventure" | "a" | "2" => Some(GameType::Adventure),
        "spectator" | "sp" | "6" => Some(GameType::Spectator),
        _ => None,
    }
}

/// Our item/block data (PNX 1.26 set) has data-driven blocks; without this experiment
/// the client shows them with no textures or names.
fn data_driven_experiments() -> Experiments {
    Experiments {
        experiments: vec![bedrock::protocol::v662::types::Experiment {
            name: "data_driven_vanilla_blocks_and_items".into(),
            enabled: true,
        }],
        ever_toggled: false,
    }
}

fn level_settings(time: i32, spawn: (f32, f32, f32), seed: u64, mode: GameType, op: bool) -> LevelSettings<Protocol> {
    LevelSettings {
        seed: seed as _,
        spawn_settings: SpawnSettings {
            spawn_type: SpawnBiomeType::Default,
            user_defined_biome_name: String::from("RandomBiome"),
            dimension: i32::from(Dimension::Overworld),
        },
        generator_type: GeneratorType::Overworld,
        game_type: mode,
        is_hardcore_enabled: false,
        game_difficulty: match *commands::DIFFICULTY.lock().unwrap() {
            0 => Difficulty::Peaceful,
            1 => Difficulty::Easy,
            3 => Difficulty::Hard,
            _ => Difficulty::Normal,
        },
        default_spawn_block_position: NetworkBlockPosition {
            x: spawn.0.floor() as i32,
            y: (spawn.1 - 1.62).floor() as i32,
            z: spawn.2.floor() as i32,
        },
        achievements_disabled: false,
        editor_world_type: EditorWorldType::NonEditor,
        is_created_in_editor: false,
        is_exported_from_editor: false,
        editor_level_migration_version: EditorLevelMigrationVersion::Legacy,
        day_cycle_stop_time: time, // PNX sends the current time here
        education_edition_offer: EducationEditionOffer::None,
        education_features_enabled: false,
        education_product_id: String::new(),
        // Weather starts clear; /weather sends the level events.
        rain_level: 0.0,
        lightning_level: 0.0,
        has_confirmed_platform_locked_content: false,
        multiplayer_enabled: true,
        lan_broadcasting_enabled: true,
        xbox_live_broadcast_setting: GamePublishSetting::FriendsOnly,
        platform_broadcast_setting: GamePublishSetting::FriendsOnly,
        commands_enabled: true,
        texture_packs_required: false,
        rule_data: GameRuleLegacyData { rules_list: vec![] },
        experiments: data_driven_experiments(),
        bonus_chest_enabled: false,
        starting_map_enabled: false,
        player_permissions: if op { 2 } else { 1 }, // operator / member
        server_chunk_tick_range: 4, // vanilla simulation distance
        locked_behaviour_pack: false,
        locked_resource_pack: false,
        from_locked_template: false,
        use_msa_gamer_tags: false,
        from_template: false,
        has_locked_template_settings: false,
        only_spawn_v1_villagers: false,
        persona_disabled: false,
        custom_skins_disabled: false,
        emote_chat_muted: false,
        base_game_version: BaseGameVersion("*".into()),
        limited_world_width: 16,
        limited_world_depth: 16,
        nether_type: true,
        edu_shared_uri_resource: EduSharedUriResource {
            button_name: String::new(),
            link_uri: String::new(),
        },
        override_force_experimental_gameplay: None,
        chat_restriction_level: ChatRestrictionLevel::None,
        disable_player_interactions: false,
        server_editor_connection_policy: 2,
        allow_anonymous_block_drops_in_editor_worlds: false,
    }
}

fn start_game_packet(id: i64, name: &str, position: (f32, f32, f32), time: i32, spawn: (f32, f32, f32), seed: u64, mode: GameType) -> StartGamePacket<Protocol> {
    StartGamePacket {
        target_actor_id: ActorUniqueID(id),
        target_runtime_id: ActorRuntimeID(id as u64),
        actor_game_type: mode.clone(),
        position, // eye position (saved, or above the spawn grass)
        rotation: Default::default(),
        settings: level_settings(time, spawn, seed, mode, commands::is_op(name)),
        level_id: String::new(),
        level_name: config::get().level_name.clone(),
        template_content_identity: String::new(),
        is_trial: false,
        movement_settings: SyncedPlayerMovementSettings {
            // PNX sends 0; a big rewind history left the client waiting for corrections.
            rewind_history_size: 0,
            server_authoritative_block_breaking: false,
        },
        current_level_time: time.max(0) as u64,
        enchantment_seed: 99000,
        block_properties: vec![],
        multiplayer_correlation_id: String::new(),
        // Modern clients drive the inventory screen through the item stack system.
        enable_item_stack_net_manager: true,
        server_version: String::new(),
        player_property_data: HashMap::new(),
        server_block_type_registry_checksum: 0,
        world_template_id: Uuid::nil(),
        server_enabled_client_side_generation: false,
        block_network_ids_are_hashes: true,
        network_permissions: NetworkPermissions {
            server_auth_sound_enabled: false,
        },
        server_join_information: None,
        server_id: String::new(),
        world_id: String::new(),
        scenario_id: String::new(),
        owner_id: String::new(),
    }
}

/// Jigsaw structure rules. Client won't spawn without them ("Missing structure data").
/// Data from kaooot/bedrock-network-data, via PNX.
fn jigsaw_structure_data() -> nbtx::Value {
    let gz = include_bytes!("../data/jigsaw_structure_data.nbt");
    let mut raw = flate2::read::GzDecoder::new(&gz[..]);
    nbtx::from_bytes::<nbtx::BigEndian, nbtx::Value>(&mut raw).expect("bad jigsaw_structure_data.nbt")
}

async fn handle_login(unknown_conn: Connection<Unknown>, server: players::Server) {
    let time_start = Instant::now();

    let mut conn = unknown_conn.into_ver::<Protocol>();
    let login = match login(&mut conn).await {
        Ok(login) => login,
        Err(error) => {
            println!("Login failed: {error}");
            conn.close().await;
            return;
        }
    };
    println!("Login: {}", login.authentication.identity().display_name);
    let id = players::next_id();
    let display_name = login.authentication.identity().display_name.clone();
    let lists = commands::lists();
    let refuse = if lists.banned.contains(&display_name) {
        Some((players::Kick::Kicked, "You are banned from this server"))
    } else if *lists.whitelist_on.lock().unwrap() && !lists.whitelist.contains(&display_name) && !lists.ops.contains(&display_name) {
        Some((players::Kick::Kicked, "You are not white-listed on this server"))
    } else if server.find_by_name(&display_name).is_none() && server.all().len() >= config::get().max_players as usize {
        Some((players::Kick::Full, "The server is full"))
    } else {
        None
    };
    if let Some((reason, message)) = refuse {
        println!("{display_name} refused: {message}");
        let _ = conn.send(&[disconnect_packet(reason, message)]).await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        conn.close().await;
        return;
    }
    if let Some(old) = server.find_by_name(&display_name) {
        let _ = server.events.send(players::Event::Kick {
            id: old,
            message: "You logged in from another location".into(),
            reason: players::Kick::OtherLocation,
        });
        // Wait (briefly) for the old session to save, so we load its latest inventory.
        for _ in 0..20 {
            if server.get(old).is_none() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
    let (saved_pos, saved_inv) = load_player(&display_name, commands::spawn_of(&display_name, &server));
    // The player's last game mode (players/<name>.mode), else the server default.
    let saved_mode = std::fs::read_to_string(player_path(&display_name).with_extension("mode")).ok();
    let start_mode = saved_mode.as_deref().and_then(|m| game_type_from(m.trim())).or_else(|| game_type_from(&config::get().gamemode)).unwrap_or(GameType::Survival);

    conn.send(&[
        Protocol::ResourcePacksInfoPacket(Box::new(ResourcePacksInfoPacket {
            resource_pack_required: false,
            has_addon_packs: false,
            has_scripts: false,
            force_disable_vibrant_visuals: false,
            world_template_uuid: Uuid::nil(),
            resource_packs: vec![],
            world_template_version: String::new(),
        })),
        Protocol::ResourcePackStackPacket(Box::new(ResourcePackStackPacket {
            texture_pack_required: false,
            addon_list: vec![],
            base_game_version: BaseGameVersion(String::from("*")),
            experiments: data_driven_experiments(),
            include_editor_packs: false,
        })),
    ])
    .await
    .unwrap();
    println!("ResourcePacksInfo");
    println!("ResourcePackStack");

    println!("{:#?}", conn.recv().await.unwrap());
    println!("ClientCacheStatus");
    println!("{:#?}", conn.recv().await.unwrap());
    println!("ResourcePackClientResponse");

    // 1.26 world clock; PNX sends it before StartGame. Clock id copied from PNX's join.
    conn.send(&[clock_packet(server.time())]).await.unwrap();

    conn.send(&[Protocol::VoxelShapesPacket(Box::new(VoxelShapesPacket {
        shapes: vec![],
        names: vec![],
        custom_shape_count: 0,
    }))])
    .await
    .unwrap();
    println!("VoxelShapes");

    // 1.26.50+ clients need this before StartGame or they fail with "Missing structure data".
    conn.send(&[Protocol::JigsawStructureDataPacket(Box::new(JigsawStructureDataPacket {
        jigsaw_structure_data_tag: jigsaw_structure_data(),
    }))])
    .await
    .unwrap();
    println!("JigsawStructureData");

    // Entity definitions: the player's physics and inventory come from "minecraft:player".
    conn.send(&[Protocol::AvailableActorIdentifiersPacket(Box::new(AvailableActorIdentifiersPacket {
        actor_info_list: items::actor_identifiers(),
    }))])
    .await
    .unwrap();
    println!("AvailableActorIdentifiers");

    conn.send(&[Protocol::StartGamePacket(Box::new(start_game_packet(id, &display_name, saved_pos, server.time(), commands::world_spawn(&server), server.world.seed(), start_mode.clone())))])
        .await
        .unwrap();
    println!("StartGame");

    // Item definitions must follow StartGame; without them the client has no items at all.
    conn.send(&[
        Protocol::ItemComponentPacket(Box::new(items::registry())),
        Protocol::CreativeContentPacket(Box::new(items::creative())),
    ])
    .await
    .unwrap();
    println!("ItemRegistry + CreativeContent");

    // Recipes, biome colors, game rules, camera presets, trims: same bytes as vanilla.
    for batch in vanilla::batches() {
        if let Err(e) = conn.send_batch(batch).await {
            println!("Login: failed to send vanilla data: {e}");
            return;
        }
    }
    // Rules changed with /gamerule since startup override the vanilla defaults.
    let changed = server.game_rules.lock().unwrap().clone();
    if !changed.is_empty() {
        let _ = conn.send(&[game_rules_packet(changed)]).await;
    }

    // From here on the connection speaks in the player's dimension (see dims.rs).
    let mut conn = dims::DimConn::new(conn);
    let mut chunks = ChunkTracker::default();
    if chunks.update(&mut conn, &server.world, (saved_pos.0.floor() as i32) >> 4, (saved_pos.2.floor() as i32) >> 4, usize::MAX).await.is_err() {
        return;
    }

    conn.send(&[Protocol::PlayStatusPacket(Box::new(PlayStatusPacket {
        status: PlayStatus::PlayerSpawn,
    }))])
    .await
    .unwrap();
    println!("PlayStatusPacket (PlayerSpawn)");

    println!("Finished request in {:?}", time_start.elapsed());
    let identity = login.authentication.identity();
    let me = players::PlayerInfo {
        id,
        uuid: Uuid::new_v4(),
        name: identity.display_name.clone(),
        xuid: identity.xuid.clone(),
        skin: std::sync::Arc::new(players::skin_from_client(&login.client_data)),
        pos: saved_pos,
        rot: (0.0, 0.0),
        head_yaw: 0.0,
    };
    play(conn, chunks, me, server, saved_inv, start_mode).await;
}

const VIEW_RADIUS: i32 = 8;

/// Which chunks this player already has, so moving only sends the new ones.
struct ChunkTracker {
    sent: std::collections::HashSet<(i32, i32)>,
    center: Option<(i32, i32)>,
    /// Capped by view-distance in server.properties.
    radius: i32,
    /// Nearest first, sent in batches.
    pending: std::collections::VecDeque<(i32, i32)>,
    last_batch: Option<std::time::Instant>,
}

impl Default for ChunkTracker {
    fn default() -> Self {
        Self { sent: Default::default(), center: None, radius: VIEW_RADIUS.min(config::get().view_distance), pending: Default::default(), last_batch: None }
    }
}

/// At most 160 chunks/s. At ~1000/s the client froze and timed out.
const CHUNK_BATCH: usize = 16;
const BATCH_GAP: std::time::Duration = std::time::Duration::from_millis(100);

impl ChunkTracker {
    fn recenter(&mut self, cx: i32, cz: i32) -> Vec<(i32, i32)> {
        let mut new = Vec::new();
        let r = self.radius;
        let in_view = |dx: i32, dz: i32| dx * dx + dz * dz <= r * r;
        for x in cx - r..=cx + r {
            for z in cz - r..=cz + r {
                if in_view(x - cx, z - cz) && self.sent.insert((x, z)) {
                    new.push((x, z));
                }
            }
        }
        // The client drops chunks outside the publisher radius, so forget exactly those too;
        // otherwise walking back leaves holes we never resend.
        self.sent.retain(|&(x, z)| in_view(x - cx, z - cz));
        new
    }

    /// Sends up to `limit` missing chunks, nearest first. Cheap to call often.
    async fn update(
        &mut self,
        conn: &mut dims::DimConn,
        world: &world::World,
        cx: i32,
        cz: i32,
        limit: usize,
    ) -> Result<(), ConnectionError> {
        let mut packets = Vec::new();
        if self.center != Some((cx, cz)) {
            self.center = Some((cx, cz));
            packets.push(Protocol::NetworkChunkPublisherUpdatePacket(Box::new(NetworkChunkPublisherUpdatePacket {
                new_view_position: BlockPos { x: cx * 16 + 8, y: 64, z: cz * 16 + 8 },
                new_view_radius: (self.radius * 16) as u32,
                server_built_chunks: vec![],
            })));
            let mut new = self.recenter(cx, cz);
            // Still-pending chunks that are in view stay queued (they're already in `sent`).
            new.extend(self.pending.drain(..).filter(|c| self.sent.contains(c)));
            new.sort_by_key(|&(x, z)| (x - cx).pow(2) + (z - cz).pow(2));
            new.dedup();
            self.pending = new.into();
        }
        let paced = limit != usize::MAX && self.last_batch.is_some_and(|t| t.elapsed() < BATCH_GAP);
        if self.pending.is_empty() || paced {
            return if packets.is_empty() { Ok(()) } else { conn.send(&packets).await };
        }
        self.last_batch = Some(std::time::Instant::now());
        let n = limit.min(self.pending.len());
        let coords: Vec<(i32, i32)> = self.pending.drain(..n).collect();
        // Generate on blocking threads so no connection loop stalls on terrain.
        let mut sign_data = signs::in_chunks(&coords);
        sign_data.extend(chest_pairs_in(world, &coords));
        let world = world.clone();
        let dim = conn.dim;
        let encoded = tokio::task::spawn_blocking(move || {
            let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).min(coords.len().max(1));
            let per = coords.len().div_ceil(threads.max(1)).max(1);
            std::thread::scope(|scope| {
                coords
                    .chunks(per)
                    .map(|part| scope.spawn(|| part.iter().map(|&(x, z)| ((x, z), world.chunk_payload_dim(x, z, dim))).collect::<Vec<_>>()))
                    .collect::<Vec<_>>()
                    .into_iter()
                    .flat_map(|h| h.join().unwrap_or_default())
                    .collect::<Vec<_>>()
            })
        })
        .await
        .unwrap_or_default();
        for ((x, z), (payload, sub_chunk_count)) in encoded {
            packets.push(Protocol::LevelChunkPacket(Box::new(LevelChunkPacket {
                chunk_position: ChunkPos { x, z },
                dimension_id: dims::network_id(dim),
                sub_chunk_count,
                client_request_sub_chunk_limit: None,
                cache_enabled: false,
                cache_blobs: vec![],
                serialized_chunk_data: payload,
                is_client_biome_update: false,
            })));
        }
        packets.extend(sign_data);
        conn.send(&packets).await
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

/// Our slash commands plus argument enums, for client autocomplete.
fn available_commands(plugins: &plugin::Plugins) -> Protocol {
    use bedrock::protocol::v844::types::Type;
    use bedrock::protocol::v898::packets::{AvailableCommandsPacket, EnumDataEntry};

    let rules = vanilla::game_rules();
    let rules_of = |want: fn(&Type) -> bool| rules.iter().filter(|r| want(&r.rule_type)).map(|r| r.rule_name.clone()).collect();
    let enums: Vec<(&str, Vec<String>)> = vec![
        ("BoolGameRule", rules_of(|t| matches!(t, Type::Bool(_)))),
        ("IntGameRule", rules_of(|t| matches!(t, Type::Int(_)))),
        ("Boolean", vec!["true".into(), "false".into()]),
        ("GameMode", ["creative", "survival", "adventure", "spectator"].map(String::from).to_vec()),
        ("TimeSet", vec!["set".into()]),
        ("TimeSpec", ["day", "noon", "night", "midnight"].map(String::from).to_vec()),
        ("Item", items::names()),
        ("Weather", ["clear", "rain", "thunder"].map(String::from).to_vec()),
        ("Difficulty", ["peaceful", "easy", "normal", "hard"].map(String::from).to_vec()),
        ("WhitelistAction", ["add", "remove", "on", "off", "list"].map(String::from).to_vec()),
    ];
    // An enum argument: valid flag | enum flag | index into enum_data.
    let e = |name: &str| ARG_VALID | ARG_ENUM | enums.iter().position(|(n, _)| *n == name).unwrap() as u32;

    let mut enum_values: Vec<String> = Vec::new();
    let enum_data = enums
        .iter()
        .map(|(name, values)| EnumDataEntry {
            name: (*name).into(),
            values: values
                .iter()
                .map(|v| match enum_values.iter().position(|x| x == v) {
                    Some(i) => i as u32,
                    None => {
                        enum_values.push(v.clone());
                        enum_values.len() as u32 - 1
                    }
                })
                .collect(),
        })
        .collect();

    let desc = |n: &str| commands::HELP.iter().find(|(c, _)| *c == n).map_or("", |(_, d)| d);
    let special = ["gamemode", "tp", "time", "give", "gamerule", "weather", "difficulty", "whitelist"];
    let mut commands = vec![
        command("gamemode", desc("gamemode"), &[&[("gameMode", e("GameMode"), false), ("player", MESSAGE, true)]]),
        command("tp", desc("tp"), &[&[("x", INT, false), ("y", INT, false), ("z", INT, false)], &[("destination", MESSAGE, false)]]),
        command("time", desc("time"), &[
            &[("action", e("TimeSet"), false), ("time", e("TimeSpec"), false)],
            &[("action", e("TimeSet"), false), ("time", INT, false)],
        ]),
        command("give", desc("give"), &[&[("itemName", e("Item"), false), ("amount", INT, true)], &[("player", MESSAGE, false)]]),
        command("gamerule", desc("gamerule"), &[
            &[("rule", e("BoolGameRule"), false), ("value", e("Boolean"), true)],
            &[("rule", e("IntGameRule"), false), ("value", INT, true)],
        ]),
        command("weather", desc("weather"), &[&[("type", e("Weather"), false)]]),
        command("difficulty", desc("difficulty"), &[&[("difficulty", e("Difficulty"), false)]]),
        command("whitelist", desc("whitelist"), &[&[("action", e("WhitelistAction"), false), ("name", MESSAGE, true)]]),
    ];
    for (n, d) in commands::HELP.iter().filter(|(n, _)| !special.contains(n)) {
        commands.push(command(n, d, &[&[("args", MESSAGE, true)]]));
    }
    // Aliases PNX/vanilla also accept.
    for (alias, of) in [("pl", "plugins"), ("ver", "version"), ("msg", "tell"), ("w", "tell"), ("teleport", "tp"), ("pardon", "unban")] {
        commands.push(command(alias, desc(of), &[&[("args", MESSAGE, true)]]));
    }
    for (name, description) in plugins.commands() {
        commands.push(command(name, description, &[&[("args", MESSAGE, true)]]));
    }
    Protocol::AvailableCommandsPacket(Box::new(AvailableCommandsPacket {
        enum_values,
        sub_command_values: vec![],
        post_fixes: vec![],
        enum_data,
        chained_sub_command_data: vec![],
        commands,
        soft_enums: vec![],
        constraints: vec![],
    }))
}

// Param types as PNX sends them on 1.26.
const ARG_VALID: u32 = 0x10_0000;
const ARG_ENUM: u32 = 0x20_0000;
const INT: u32 = ARG_VALID | 1;
const MESSAGE: u32 = ARG_VALID | 68; // rest of the line, as PNX's /tell uses

/// One command with one or more overloads (alternative argument lists).
fn command(name: &str, description: &str, overloads: &[&[(&str, u32, bool)]]) -> bedrock::protocol::v898::packets::CommandsEntry {
    use bedrock::protocol::v898::packets::{CommandPermissionLevelString, CommandsEntry, OverloadsEntry, ParameterDataEntry};
    CommandsEntry {
        name: name.into(),
        description: description.into(),
        flags: 0,
        permission_level: CommandPermissionLevelString::Any,
        alias_enum: -1,
        chained_sub_command_indices: vec![],
        overloads: overloads
            .iter()
            .map(|params| OverloadsEntry {
                is_chaining: false,
                parameter_data: params
                    .iter()
                    .map(|(n, t, optional)| ParameterDataEntry { name: (*n).into(), parse_symbol: *t, is_optional: *optional, options: 0 })
                    .collect(),
            })
            .collect(),
    }
}

async fn set_game_mode(conn: &mut dims::DimConn, id: i64, mode: &GameType) {
    use bedrock::protocol::v662::packets::SetPlayerGameTypePacket;
    let _ = conn
        .send(&[
            Protocol::SetPlayerGameTypePacket(Box::new(SetPlayerGameTypePacket { player_game_type: mode.clone() })),
            spawn::abilities(id, mode),
        ])
        .await;
}

fn disconnect_packet(reason: players::Kick, message: &str) -> Protocol {
    use bedrock::protocol::v2225::enums::ConnectionFailReason;
    use bedrock::protocol::v712::packets::{DisconnectMessage, DisconnectPacket};
    Protocol::DisconnectPacket(Box::new(DisconnectPacket {
        reason: match reason {
            players::Kick::OtherLocation => ConnectionFailReason::LoggedInOtherLocation,
            players::Kick::Shutdown => ConnectionFailReason::Shutdown,
            players::Kick::Kicked => ConnectionFailReason::Kicked,
            players::Kick::Full => ConnectionFailReason::ServerFull,
        },
        message: Some(DisconnectMessage { kick_message: message.into(), filtered_message: message.into() }),
    }))
}

/// 1.26 world clock sync (clock id copied from PNX's join); the client advances it itself.
fn clock_packet(time: i32) -> Protocol {
    Protocol::SyncWorldClocksPacket(Box::new(SyncWorldClocksPacket {
        data: SyncWorldClocks::SyncState {
            clock_data: vec![SyncWorldClockState { clock_id: 7_194_480_507_151_251_734, time, paused: false }],
        },
    }))
}

fn game_rules_packet(rules_list: Vec<bedrock::protocol::v844::types::GameRuleChanged>) -> Protocol {
    Protocol::GameRulesChangedPacket(Box::new(bedrock::protocol::v662::packets::GameRulesChangedPacket {
        rules_data: bedrock::protocol::v844::types::GameRulesChangedPacketData { rules_list },
    }))
}

fn time_packet(time: i32) -> Protocol {
    Protocol::SetTimePacket(Box::new(bedrock::protocol::v662::packets::SetTimePacket { time }))
}

/// players/<name>.dat: eye position (3 x f32), then the inventory.
fn player_path(name: &str) -> std::path::PathBuf {
    let safe: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    std::path::Path::new("players").join(format!("{safe}.dat"))
}

fn load_player(name: &str, spawn: (f32, f32, f32)) -> ((f32, f32, f32), inventory::Inventory) {
    let fresh = (spawn, inventory::Inventory::default());
    let Ok(b) = std::fs::read(player_path(name)) else { return fresh };
    let f = |i: usize| b.get(i..i + 4).map(|x| f32::from_le_bytes(x.try_into().unwrap()));
    match (f(0), f(4), f(8), b.get(12..).and_then(inventory::Inventory::from_bytes)) {
        (Some(x), Some(y), Some(z), Some(inv)) if x.is_finite() && y.is_finite() && z.is_finite() => ((x, y, z), inv),
        _ => {
            println!("{name}: player save is damaged; starting fresh");
            fresh
        }
    }
}

fn save_xp(name: &str, xp: xp::Xp) {
    let _ = std::fs::create_dir_all("players").and_then(|()| std::fs::write(player_path(name).with_extension("xp"), xp.to_text()));
}

fn save_mode(name: &str, mode: &GameType) {
    let m = match mode {
        GameType::Creative => "creative",
        GameType::Adventure => "adventure",
        GameType::Spectator => "spectator",
        _ => "survival",
    };
    let _ = std::fs::create_dir_all("players").and_then(|()| std::fs::write(player_path(name).with_extension("mode"), m));
}

/// Armor points worn (vanilla Bedrock values).
fn armor_points(inv: &inventory::Inventory) -> f32 {
    (inventory::ARMOR..inventory::ARMOR + 4)
        .filter_map(|i| inv.held(i).and_then(|s| items::name_of(s.item.id)))
        .map(|n| match n.trim_start_matches("minecraft:") {
            "leather_helmet" | "leather_boots" | "golden_boots" | "chainmail_boots" => 1.0,
            "leather_leggings" | "golden_helmet" | "chainmail_helmet" | "iron_helmet" | "iron_boots" | "turtle_helmet" => 2.0,
            "leather_chestplate" | "golden_leggings" | "diamond_helmet" | "netherite_helmet" | "diamond_boots" | "netherite_boots" => 3.0,
            "chainmail_leggings" => 4.0,
            "golden_chestplate" | "chainmail_chestplate" | "iron_leggings" => 5.0,
            "iron_chestplate" | "diamond_leggings" | "netherite_leggings" => 6.0,
            "diamond_chestplate" | "netherite_chestplate" => 8.0,
            _ => 0.0,
        })
        .sum()
}

/// Armor soaks mob, arrow and explosion damage (vanilla: up to 80%).
fn after_armor(damage: health::Damage, inv: &inventory::Inventory) -> health::Damage {
    use health::Damage::*;
    let a = armor_points(inv);
    let soak = |d: f32| d * (1.0 - (a - d / 2.0).max(a / 5.0).min(20.0) / 25.0);
    match damage {
        Mob(d, by) => Mob(soak(d), by),
        Shot(d, by) => Shot(soak(d), by),
        Explosion(d, by) => Explosion(soak(d), by),
        other => other,
    }
}

fn save_player(name: &str, pos: (f32, f32, f32), inv: &inventory::Inventory) {
    let mut b = Vec::new();
    for v in [pos.0, pos.1, pos.2] {
        b.extend(v.to_le_bytes());
    }
    b.extend(inv.to_bytes());
    let path = player_path(name);
    let tmp = path.with_extension("tmp");
    let result = std::fs::create_dir_all("players").and_then(|()| std::fs::write(&tmp, &b)).and_then(|()| std::fs::rename(&tmp, &path));
    if let Err(e) = result {
        println!("{name}: failed to save player: {e}");
    }
}

fn update_block(x: i32, y: i32, z: i32, block: i32) -> Protocol {
    Protocol::UpdateBlockPacket(Box::new(UpdateBlockPacket {
        block_position: NetworkBlockPosition { x, y, z },
        block_runtime_id: block as u32, // hashed IDs go over the wire as their bit pattern
        flags: 0b11, // neighbours + network
        layer: 0,
    }))
}

fn level_event(event_id: i32, p: (i32, i32, i32)) -> Protocol {
    Protocol::LevelEventPacket(Box::new(LevelEventPacket { event_id, position: (p.0 as f32 + 0.5, p.1 as f32 + 0.5, p.2 as f32 + 0.5), data: 0 }))
}

/// Players who used a bed at night recently (all of them = skip the night).
fn sleepers() -> &'static std::sync::Mutex<HashMap<i64, Instant>> {
    static S: std::sync::OnceLock<std::sync::Mutex<HashMap<i64, Instant>>> = std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}

/// Lid/door animation and sound when the first player opens a chest or barrel.
fn container_opened(server: &players::Server, pos: (i32, i32, i32)) {
    if server.containers.viewers(pos, 1) == 1 {
        container_anim(server, pos, true);
    }
}

fn container_closed(server: &players::Server, pos: (i32, i32, i32)) {
    if server.containers.viewers(pos, -1) == 0 {
        container_anim(server, pos, false);
    }
}

fn container_anim(server: &players::Server, (x, y, z): (i32, i32, i32), open: bool) {
    let block = server.world.get(x, y, z);
    let name = blocks::entry(block).map_or("", |e| e.name.as_str());
    let center = (x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
    match name {
        "minecraft:chest" | "minecraft:trapped_chest" | "minecraft:ender_chest" => {
            // Block event 1 = chest lid (value 1 open, 0 closed); PNX's sounds.
            let lid = Protocol::BlockEventPacket(Box::new(bedrock::protocol::v662::packets::BlockEventPacket {
                block_position: NetworkBlockPosition { x, y, z },
                event_type: 1,
                event_value: open as i32,
            }));
            let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(lid) });
            let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound(if open { "random.chestopen" } else { "random.chestclosed" }, center)) });
        }
        n if n.ends_with("shulker_box") => {
            let lid = Protocol::BlockEventPacket(Box::new(bedrock::protocol::v662::packets::BlockEventPacket {
                block_position: NetworkBlockPosition { x, y, z },
                event_type: 1,
                event_value: open as i32,
            }));
            let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(lid) });
            let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound(if open { "random.shulkerboxopen" } else { "random.shulkerboxclosed" }, center)) });
        }
        "minecraft:barrel" => {
            server.set_block_quiet(0, x, y, z, blocks::with(block, &[("open_bit", blocks::State::Bool(open))]));
            let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound(if open { "block.barrel.open" } else { "block.barrel.close" }, center)) });
        }
        _ => {}
    }
}

fn apply_use(server: &players::Server, id: i64, p: (i32, i32, i32), o: &useblock::Outcome) {
    let center = (p.0 as f32 + 0.5, p.1 as f32 + 0.5, p.2 as f32 + 0.5);
    for &((x, y, z), b) in &o.changes {
        server.set_block(id, x, y, z, b);
    }
    if let Some(s) = &o.stop_sound {
        let stop = Protocol::StopSoundPacket(Box::new(bedrock::protocol::v712::packets::StopSoundPacket { sound_name: s.clone(), stop_all_sounds: false, stop_music_legacy: false }));
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(stop) });
    }
    if let Some((s, pitch)) = &o.sound {
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound_pitch(s, center, *pitch)) });
    }
    if let Some((event_type, event_value)) = o.block_event {
        let ev = Protocol::BlockEventPacket(Box::new(bedrock::protocol::v662::packets::BlockEventPacket {
            block_position: NetworkBlockPosition { x: p.0, y: p.1, z: p.2 },
            event_type,
            event_value,
        }));
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(ev) });
    }
    for &(item, n) in &o.drops {
        if let Some(it) = items::item_by_name(item) {
            server.entities.drop_item(server, it, n, (center.0, p.1 as f32 + 1.0, center.2));
        }
    }
    if let Some(power) = o.explode {
        mobs::explode(server, center, power, "Respawn Anchor");
    }
}

/// A named sound at a position (PNX `Level.addSound` -> PlaySoundPacket).
fn play_sound(name: &str, at: (f32, f32, f32)) -> Protocol {
    play_sound_pitch(name, at, 1.0)
}

fn play_sound_pitch(name: &str, at: (f32, f32, f32), pitch: f32) -> Protocol {
    use bedrock::protocol::v2193::packets::{PlaySoundPacket, PlaySoundPosition};
    Protocol::PlaySoundPacket(Box::new(PlaySoundPacket {
        name: name.into(),
        position: PlaySoundPosition::from_raw_x8((at.0 * 8.0) as i32, (at.1 * 8.0) as i32, (at.2 * 8.0) as i32),
        volume: 1.0,
        pitch,
        loop_count: 0,
        bypass_listener_range_check: false,
        server_sound_handle: None,
        playback_position_seconds: None,
    }))
}

/// Closes whatever container a player had open; a double chest goes back as two halves.
fn close_open(server: &players::Server, pos: (i32, i32, i32), mut slots: Vec<Option<inventory::Stack>>, pair: Option<(i32, i32, i32)>) {
    if let Some(other) = pair
        && slots.len() > inventory::CHEST_SLOTS
    {
        let rest = slots.split_off(inventory::CHEST_SLOTS);
        server.containers.close(&server.world, other, rest);
        container_closed(server, other);
    }
    server.containers.close(&server.world, pos, slots);
    container_closed(server, pos);
}

/// Chest block-entity data: tells clients which chests form a double chest (PNX
/// `BlockEntityChest` spawn compound: pairx/pairz/pairlead).
fn chest_data(pos: (i32, i32, i32), pair: Option<(i32, i32, i32)>) -> Protocol {
    let mut tags = HashMap::new();
    tags.insert("id".to_string(), nbtx::Value::String("Chest".into()));
    tags.insert("x".to_string(), nbtx::Value::Int(pos.0));
    tags.insert("y".to_string(), nbtx::Value::Int(pos.1));
    tags.insert("z".to_string(), nbtx::Value::Int(pos.2));
    tags.insert("Findable".to_string(), nbtx::Value::Byte(0));
    if let Some(p) = pair {
        tags.insert("pairx".to_string(), nbtx::Value::Int(p.0));
        tags.insert("pairz".to_string(), nbtx::Value::Int(p.2));
        tags.insert("pairlead".to_string(), nbtx::Value::Byte((pos < p) as i8));
    }
    Protocol::BlockActorDataPacket(Box::new(bedrock::protocol::v662::packets::BlockActorDataPacket {
        block_position: NetworkBlockPosition { x: pos.0, y: pos.1, z: pos.2 },
        actor_data_tags: tags,
    }))
}

/// Pair data for a chest and its partner, to everyone (after placing or breaking one).
fn announce_chest(server: &players::Server, pos: (i32, i32, i32)) {
    let pair = containers::chest_pair(&server.world, pos);
    let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(chest_data(pos, pair)) });
    if let Some(p) = pair {
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(chest_data(p, Some(pos))) });
    }
}

/// Chest pair data for player-placed chests in these chunks (sent with the chunks).
fn chest_pairs_in(world: &world::World, chunks: &[(i32, i32)]) -> Vec<Protocol> {
    let mut out = Vec::new();
    for &(cx, cz) in chunks {
        for (pos, id) in world.edits_in(cx, cz) {
            if containers::kind(id) == Some(containers::Kind::Chest)
                && let Some(pair) = containers::chest_pair(world, pos)
            {
                out.push(chest_data(pos, Some(pair)));
            }
        }
    }
    out
}

/// Furnaces, smokers and blast furnaces switch to their lit block while burning.
fn set_lit(server: &players::Server, (x, y, z): (i32, i32, i32), lit: bool) {
    let block = server.world.get(x, y, z);
    let Some(e) = blocks::entry(block) else { return };
    let base = e.name.trim_start_matches("minecraft:").trim_start_matches("lit_");
    if !matches!(base, "furnace" | "smoker" | "blast_furnace") {
        return;
    }
    let want = if lit { format!("minecraft:lit_{base}") } else { format!("minecraft:{base}") };
    if e.name == want {
        return;
    }
    let Some(mut new) = blocks::any_state(&want) else { return };
    // Keep the facing.
    for (k, v) in &e.states {
        new = blocks::with(new, &[(k.as_str(), v.clone())]);
    }
    server.set_block_quiet(0, x, y, z, new);
}

/// Where a thrown item starts and how fast it goes (PNX values).
fn throw_from(me: &players::PlayerInfo) -> ((f32, f32, f32), (f32, f32, f32)) {
    // PNX Player.dropItem: from feet + 1.3, motion = look direction * 0.4 (per game tick).
    let (pitch, yaw) = (me.rot.0.to_radians(), me.rot.1.to_radians());
    let dir = (-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos());
    let at = (me.pos.0, me.pos.1 - 1.62 + 1.3, me.pos.2);
    (at, (dir.0 * 0.4, dir.1 * 0.4, dir.2 * 0.4))
}

fn oven(kind: containers::Kind) -> Option<recipes::Oven> {
    match kind {
        containers::Kind::Furnace => Some(recipes::Oven::Furnace),
        containers::Kind::Smoker => Some(recipes::Oven::Smoker),
        containers::Kind::BlastFurnace => Some(recipes::Oven::Blast),
        _ => None,
    }
}

/// Online players' entity ids -> stable pet-owner keys.
fn owner_keys(server: &players::Server) -> HashMap<i64, i64> {
    server.all().into_iter().map(|p| (p.id, mobs::owner_key(&p.name))).collect()
}

fn survival_like(mode: &GameType) -> bool {
    matches!(mode, GameType::Survival | GameType::Adventure)
}

/// A boolean game rule's current value (`default` if never changed).
fn game_rule_on(server: &players::Server, rule: &str, default: bool) -> bool {
    use bedrock::protocol::v844::types::Type;
    server.game_rules.lock().unwrap().iter().find(|r| r.rule_name == rule).map_or(default, |r| matches!(r.rule_type, Type::Bool(true)))
}

/// Chests spill, survival drops items (`drops` = held item), gravity blocks above fall.
fn break_block(server: &players::Server, id: i64, p: (i32, i32, i32), drops: Option<Option<&str>>, tool: &[u8]) {
    let old = server.world.get(p.0, p.1, p.2);
    let center = (p.0 as f32 + 0.5, p.1 as f32 + 0.25, p.2 as f32 + 0.5);
    let is_shulker = containers::kind(old) == Some(containers::Kind::Shulker);
    let contents = server.containers.remove(p);
    if is_shulker {
        // Keeps its items (PNX `BlockShulkerBox.toItem`), even in creative.
        if let Some(it) = items::item_for_block(old) {
            server.entities.drop_item(server, items::with_contents(it, &contents), 1, center);
        }
    } else {
        for s in contents.into_iter().flatten() {
            server.entities.drop_item(server, s.item, s.count, center);
        }
    }
    let drops = if is_shulker { None } else { drops };
    if let Some(held) = drops && game_rule_on(server, "doTileDrops", true) {
        let normal = drops::drops(old, p, held);
        let block_item = items::item_for_block(old);
        // Silk touch drops the block itself (when it drops anything at all with this tool).
        let silk = enchant::level(tool, enchant::SILK_TOUCH) > 0 && !normal.is_empty() && containers::kind(old).is_none();
        let fortune = enchant::level(tool, enchant::FORTUNE);
        if silk && let Some(it) = block_item.clone() {
            server.entities.drop_item(server, it, 1, center);
        } else {
            let roll = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() / 7;
            let points = xp::from_block(blocks::entry(old).map_or("", |e| e.name.as_str()), roll);
            if points > 0 {
                let _ = server.events.send(players::Event::Xp { to: id, points });
            }
            for (item, count) in normal {
                // Fortune multiplies ore drops (anything that isn't the block itself).
                let roll = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() / 13;
                let ore = items::name_of(item.id).is_some_and(|n| {
                    matches!(n, "minecraft:diamond" | "minecraft:emerald" | "minecraft:coal" | "minecraft:lapis_lazuli" | "minecraft:redstone" | "minecraft:quartz"
                        | "minecraft:raw_iron" | "minecraft:raw_gold" | "minecraft:raw_copper" | "minecraft:gold_nugget" | "minecraft:amethyst_shard")
                });
                let count = if ore { count.saturating_mul(enchant::fortune(fortune, roll)) } else { count };
                server.entities.drop_item(server, item, count, center);
            }
        }
    }
    signs::removed(p);
    if let Some((disc, rec)) = useblock::removed(p) {
        if let Some(it) = items::item_by_name(disc) {
            server.entities.drop_item(server, it, 1, center);
        }
        let stop = Protocol::StopSoundPacket(Box::new(bedrock::protocol::v712::packets::StopSoundPacket { sound_name: rec, stop_all_sounds: false, stop_music_legacy: false }));
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(stop) });
    }
    let partner = containers::chest_pair(&server.world, p);
    server.set_block(id, p.0, p.1, p.2, world::AIR);
    if let Some(o) = partner {
        announce_chest(server, o);
    }
    for ((fx, fy, fz), b) in physics::settle(&server.world, p) {
        server.set_block_quiet(id, fx, fy, fz, b);
    }
    pop_unsupported(server, id, p);
    // Infested stone hides a silverfish.
    if drops.is_some() && blocks::entry(old).is_some_and(|e| e.name.starts_with("minecraft:infested_")) {
        server.mobs.spawn(server, mobs::Kind::Silverfish, (p.0 as f32 + 0.5, p.1 as f32, p.2 as f32 + 0.5));
    }
}

/// Crops, flowers, torches, ladders... that hung on the removed block at `p` pop off with drops.
fn pop_unsupported(server: &players::Server, id: i64, p: (i32, i32, i32)) {
    // A nether portal falls apart when its frame (or any portal block) breaks.
    let is_portal = |q: (i32, i32, i32)| blocks::entry(server.world.get(q.0, q.1, q.2)).is_some_and(|e| e.name == "minecraft:portal");
    let mut todo: Vec<(i32, i32, i32)> = redstone::DIRS.iter().map(|d| (p.0 + d.0, p.1 + d.1, p.2 + d.2)).filter(|&q| is_portal(q)).collect();
    let mut n = 0;
    while let Some(q) = todo.pop() {
        if !is_portal(q) || n > 441 {
            continue;
        }
        n += 1;
        server.set_block(id, q.0, q.1, q.2, world::AIR);
        todo.extend(redstone::DIRS.iter().map(|d| (q.0 + d.0, q.1 + d.1, q.2 + d.2)));
    }
    for (q, b) in physics::unsupported(&server.world, p) {
        if game_rule_on(server, "doTileDrops", true) {
            for (item, count) in drops::drops(b, q, None) {
                server.entities.drop_item(server, item, count, (q.0 as f32 + 0.5, q.1 as f32 + 0.25, q.2 as f32 + 0.5));
            }
        }
        server.set_block(id, q.0, q.1, q.2, world::AIR);
    }
}

/// MobEffectPacket: add/update (`Some`) or remove (`None`) effect `e` on player `id`.
fn effect_packet(id: i64, e: i32, x: Option<effects::Effect>) -> Protocol {
    use bedrock::protocol::v898::packets::{MobEffectEvent, MobEffectPacket};
    Protocol::MobEffectPacket(Box::new(MobEffectPacket {
        target_runtime_id: ActorRuntimeID(id as u64),
        event_id: if x.is_some() { MobEffectEvent::Add } else { MobEffectEvent::Remove },
        effect_id: e,
        effect_amplifier: x.map_or(0, |x| x.amplifier),
        show_particles: x.is_some_and(|x| x.particles),
        effect_duration_ticks: x.map_or(0, |x| x.ticks),
        tick: 0,
        ambient: false,
    }))
}

/// Absorption hearts on the client.
fn absorption_packet(id: i64, value: f32) -> Protocol {
    use bedrock::protocol::v729::packets::{AttributeData, UpdateAttributesPacket};
    Protocol::UpdateAttributesPacket(Box::new(UpdateAttributesPacket {
        target_runtime_id: ActorRuntimeID(id as u64),
        attribute_list: vec![AttributeData {
            min_value: 0.0,
            max_value: 16.0,
            current_value: value,
            default_min: 0.0,
            default_max: 16.0,
            default_value: 0.0,
            attribute_name: "minecraft:absorption".into(),
            attribute_modifiers: vec![],
        }],
        ticks_since_sim_started: 0,
    }))
}

/// The player's walk speed (speed and slowness change it).
fn movement_packet(id: i64, speed: f32) -> Protocol {
    use bedrock::protocol::v729::packets::{AttributeData, UpdateAttributesPacket};
    Protocol::UpdateAttributesPacket(Box::new(UpdateAttributesPacket {
        target_runtime_id: ActorRuntimeID(id as u64),
        attribute_list: vec![AttributeData {
            min_value: 0.0,
            max_value: f32::MAX,
            current_value: speed,
            default_min: 0.0,
            default_max: f32::MAX,
            default_value: 0.1,
            attribute_name: "minecraft:movement".into(),
            attribute_modifiers: vec![],
        }],
        ticks_since_sim_started: 0,
    }))
}

/// Instant effects act right away (PNX: heal 4 << amp, harm 6 << amp), others get sent.
#[allow(clippy::too_many_arguments)]
async fn give_effect(conn: &mut dims::DimConn, server: &players::Server, id: i64, name: &str, hp: &mut health::Health, food: &mut hunger::Hunger, fx: &mut effects::Effects, inv: &mut inventory::Inventory, e: i32, x: effects::Effect) {
    match e {
        effects::INSTANT_HEALTH => {
            hp.health = (hp.health + (4 << x.amplifier.min(5)) as f32).min(health::MAX_HEALTH);
            let _ = conn.send(&[spawn::health_packet(id, hp.health)]).await;
        }
        effects::INSTANT_DAMAGE => hurt(conn, server, id, name, hp, health::Damage::Magic((6 << x.amplifier.min(5)) as f32), inv, fx).await,
        effects::SATURATION => {
            food.eat((x.amplifier + 1) as f32, (x.amplifier + 1) as f32 * 2.0);
            let _ = conn.send(&[food.packet(id)]).await;
        }
        _ => {
            if fx.add(e, x) {
                let mut out = vec![effect_packet(id, e, Some(x)), movement_packet(id, fx.speed())];
                if e == effects::ABSORPTION {
                    fx.set_absorb(4.0 * (x.amplifier + 1) as f32);
                    out.push(absorption_packet(id, fx.absorb()));
                }
                let _ = conn.send(&out).await;
            }
        }
    }
}

/// Cross-dimension teleport to `eye`: resend chunks, show/hide us (PNX Player.teleport).
#[allow(clippy::too_many_arguments)]
async fn change_dim(conn: &mut dims::DimConn, server: &players::Server, chunks: &mut ChunkTracker, id: i64, to: u8, eye: (f32, f32, f32), rot: (f32, f32), head: f32) -> Result<(), ConnectionError> {
    // Others stop seeing us where we were.
    let gone = Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) }));
    let _ = server.events.send(players::Event::Relay { from: id, packet: Box::new(gone) });
    conn.dim = to;
    conn.send(&[Protocol::ChangeDimensionPacket(Box::new(bedrock::protocol::v712::packets::ChangeDimensionPacket {
        dimension_id: dims::network_id(to),
        position: eye,
        respawn: false,
        loading_screen_id: None,
    }))]).await?;
    chunks.sent.clear();
    chunks.pending.clear();
    chunks.center = None;
    server.moved(id, eye, rot, head);
    let (cx, cz) = ((eye.0.floor() as i32) >> 4, (eye.2.floor() as i32) >> 4);
    chunks.update(conn, &server.world, cx, cz, usize::MAX).await?;
    let mut tp = move_player(id, eye, rot, head);
    if let Protocol::MovePlayerPacket(m) = &mut tp {
        m.position_mode = PlayerPositionMode::Teleport;
        m.teleport_data = Some(MovePlayerTeleportData { teleportation_cause: 0, source_actor_type: 0 });
    }
    conn.send(&[tp, play_status(PlayStatus::PlayerSpawn)]).await?;
    // Everyone in the new dimension sees us arrive.
    let _ = server.events.send(players::Event::Join(id));
    if to == dims::END {
        endfight::entered(server);
    } else {
        ui::remove_boss_bar(server, id);
    }
    Ok(())
}

/// How much breaking a block wears the held item (PNX: tools 1, swords 2).
fn mining_wear(held: Option<&str>) -> i32 {
    match held {
        Some(h) if h.ends_with("_sword") || h == "minecraft:trident" => 2,
        Some(h) if h.ends_with("_pickaxe") || h.ends_with("_axe") || h.ends_with("_shovel") || h.ends_with("_hoe") || h == "minecraft:shears" => 1,
        _ => 0,
    }
}

/// How much hitting a mob wears the held item (PNX: swords 1, other tools 2).
fn attack_wear(held: Option<&str>) -> i32 {
    match held {
        Some(h) if h.ends_with("_sword") || h == "minecraft:trident" || h == "minecraft:mace" => 1,
        Some(h) if h.ends_with("_pickaxe") || h.ends_with("_axe") || h.ends_with("_shovel") || h.ends_with("_hoe") => 2,
        _ => 0,
    }
}

/// Wears the item in `slot` in survival; tells the client, and plays the break sound if it broke.
async fn wear(conn: &mut dims::DimConn, server: &players::Server, id: i64, inv: &mut inventory::Inventory, slot: usize, uses: i32, mode: &GameType) {
    if !survival_like(mode) || uses <= 0 {
        return;
    }
    let Some(broke) = inv.wear(slot, uses) else { return };
    let _ = conn.send(&[inv.content_packet()]).await;
    if broke && let Some(me) = server.get(id) {
        let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound("random.break", me.pos)) });
    }
}

/// Survival bucket use: the held bucket becomes `to` (creative keeps its bucket).
async fn swap_bucket(conn: &mut dims::DimConn, inv: &mut inventory::Inventory, slot: u32, mode: &GameType, to: &str) {
    if !survival_like(mode) {
        return;
    }
    if let Some(item) = items::item_by_name(to)
        && let Some(stack) = inv.set_slot(slot as usize, item, 1)
    {
        let item = stack.to_network();
        let _ = conn.send(&[Protocol::InventorySlotPacket(Box::new(InventorySlotPacket {
            container_id: 0, slot, container_name_data: None, storage_item: None, item,
        }))]).await;
    }
}

/// Health, red flash for everyone watching, and death if it kills.
async fn hurt(conn: &mut dims::DimConn, server: &players::Server, id: i64, name: &str, hp: &mut health::Health, damage: health::Damage, inv: &mut inventory::Inventory, fx: &effects::Effects) {
    use health::Damage as D;
    if fx.level(effects::FIRE_RESISTANCE).is_some() && matches!(damage, D::Lava | D::Fire) || fx.level(effects::WATER_BREATHING).is_some() && damage == D::Drown {
        return;
    }
    if let Some(me) = server.get(id) {
        let (cause, amount) = damage_cause(damage);
        if !server.plugins.player_damage(server, &me, cause, amount) {
            return;
        }
    }
    let damage = match damage {
        D::Mob(d, by) => D::Mob(fx.resist(d), by),
        D::Shot(d, by) => D::Shot(fx.resist(d), by),
        D::Explosion(d, by) => D::Explosion(fx.resist(d), by),
        D::Fall(d) => D::Fall(fx.resist(d)),
        other => other,
    };
    if let health::Damage::Mob(d, _) | health::Damage::Shot(d, _) | health::Damage::Explosion(d, _) = damage {
        let mut broke = false;
        for slot in inventory::ARMOR..inventory::ARMOR + 4 {
            broke |= inv.wear(slot, ((d / 4.0) as i32).max(1)) == Some(true);
        }
        if broke && let Some(me) = server.get(id) {
            let _ = server.events.send(players::Event::Relay { from: 0, packet: Box::new(play_sound("random.break", me.pos)) });
        }
        let _ = conn.send(&inv.all_packets()[1..2]).await;
    }
    let damage = after_armor(damage, inv);
    let armor: Vec<Vec<u8>> = (inventory::ARMOR..inventory::ARMOR + 4).filter_map(|i| inv.held(i).map(|s| s.item.user_data_buffer.clone())).collect();
    let armor: Vec<&[u8]> = armor.iter().map(Vec::as_slice).collect();
    let damage = match damage {
        D::Mob(d, by) => D::Mob(d * enchant::protection(&armor, enchant::Hit::Other), by),
        D::Shot(d, by) => D::Shot(d * enchant::protection(&armor, enchant::Hit::Projectile), by),
        D::Explosion(d, by) => D::Explosion(d * enchant::protection(&armor, enchant::Hit::Explosion), by),
        D::Fall(d) => D::Fall((d * enchant::protection(&armor, enchant::Hit::Fall)).round()),
        other => other,
    };
    // Absorption hearts go first (PNX `EntityHuman.applyDamage`).
    let soak = |d: f32| {
        let a = fx.absorb();
        let take = a.min(d);
        fx.set_absorb(a - take);
        d - take
    };
    let had = fx.absorb();
    let damage = if had > 0.0 {
        match damage {
            D::Mob(d, by) => D::Mob(soak(d), by),
            D::Shot(d, by) => D::Shot(soak(d), by),
            D::Explosion(d, by) => D::Explosion(soak(d), by),
            D::Magic(d) => D::Magic(soak(d)),
            D::Fall(d) => D::Fall(soak(d)),
            D::Wither(d) => D::Wither(soak(d)),
            other => other,
        }
    } else {
        damage
    };
    if had != fx.absorb() {
        let _ = conn.send(&[absorption_packet(id, fx.absorb())]).await;
    }
    let died = hp.hurt(damage);
    let event = |e: ActorEvent| {
        Protocol::ActorEventPacket(Box::new(ActorEventPacket { target_runtime_id: ActorRuntimeID(id as u64), event_id: e, data: 0, fire_at_position: None }))
    };
    let mut out = vec![spawn::health_packet(id, hp.health), event(ActorEvent::Hurt)];
    let _ = server.events.send(players::Event::Relay { from: id, packet: Box::new(event(if died { ActorEvent::Death } else { ActorEvent::Hurt })) });
    if died {
        out.push(event(ActorEvent::Death));
        // Death screen: tell the client where it will respawn.
        out.push(Protocol::RespawnPacket(Box::new(RespawnPacket {
            position: commands::spawn_of(name, server),
            state: PlayerRespawnState::SearchingForSpawn,
            player_runtime_id: ActorRuntimeID(id as u64),
        })));
        let mut msg = health::death_message(name, damage);
        if let Some(me) = server.get(id) {
            server.plugins.player_death(server, &me, &mut msg);
        }
        if !msg.is_empty() {
            server.broadcast(&msg);
        }
        if !game_rule_on(server, "keepInventory", false) {
            let at = server.get(id).map_or(server.world.spawn(), |p| (p.pos.0, p.pos.1 - 1.0, p.pos.2));
            for s in inv.take_all() {
                server.entities.drop_item(server, s.item, s.count, at);
            }
            out.extend(inv.all_packets());
        }
    }
    let _ = conn.send(&out).await;
}

/// A damage's cause name and amount, for plugins.
fn damage_cause(d: health::Damage) -> (&'static str, f32) {
    use health::Damage as D;
    match d {
        D::Fall(n) => ("fall", n),
        D::Void => ("void", 1000.0),
        D::Kill => ("kill", 1000.0),
        D::Starve => ("starve", 1.0),
        D::Drown => ("drown", 2.0),
        D::Lava => ("lava", 4.0),
        D::Fire => ("fire", 1.0),
        D::Mob(n, by) | D::Shot(n, by) | D::Explosion(n, by) => (by, n),
        D::Magic(n) => ("magic", n),
        D::Wither(n) => ("wither", n),
    }
}

/// Respawn at world spawn, in PNX order: spawn pos, attributes, data, abilities, inventory, teleport.
async fn respawn(
    conn: &mut dims::DimConn,
    server: &players::Server,
    id: i64,
    name: &str,
    hp: &mut health::Health,
    mode: &GameType,
    inv: &inventory::Inventory,
) {
    hp.respawn();
    let at = commands::spawn_of(name, server);
    let mut out = vec![
        Protocol::RespawnPacket(Box::new(RespawnPacket { position: at, state: PlayerRespawnState::ReadyToSpawn, player_runtime_id: ActorRuntimeID(id as u64) })),
        spawn::attributes(id),
        spawn::set_data(id, name),
        spawn::abilities(id, mode),
    ];
    out.extend(inv.all_packets());
    let (rot, head) = server.get(id).map_or(((0.0, 0.0), 0.0), |p| (p.rot, p.head_yaw));
    let mut tp = move_player(id, at, rot, head);
    if let Protocol::MovePlayerPacket(m) = &mut tp {
        m.position_mode = PlayerPositionMode::Respawn;
    }
    out.push(tp);
    let _ = conn.send(&out).await;
    server.moved(id, at, rot, head);
}

fn chat_packet(from: String, message: String) -> Protocol {
    // Empty sender = server announcement.
    let message_type = if from.is_empty() {
        TextPacketType::Raw(message)
    } else {
        TextPacketType::Chat { player_name: from, message }
    };
    Protocol::TextPacket(Box::new(TextPacket {
        localize: false,
        message_type,
        sender_xuid: String::new(),
        platform_id: String::new(),
        filtered_message: None,
    }))
}

fn player_list_add(p: &players::PlayerInfo) -> Protocol {
    Protocol::PlayerListPacket(Box::new(PlayerListPacket {
        entries: vec![PlayerListEntry::Add(AddPlayerListEntry {
            uuid: p.uuid,
            target_actor_id: ActorUniqueID(p.id),
            player_name: p.name.clone(),
            xbl_xuid: p.xuid.clone(),
            play_fab_id: String::new(),
            platform_chat_id: String::new(),
            build_platform: BuildPlatform::Win32,
            serialized_skin: (*p.skin).clone(),
            is_teacher: false,
            is_host: false,
            is_sub_client: false,
            color: players::white(),
        })],
    }))
}

fn add_player(p: &players::PlayerInfo) -> Protocol {
    Protocol::AddPlayerPacket(Box::new(AddPlayerPacket {
        uuid: p.uuid,
        player_name: p.name.clone(),
        target_runtime_id: ActorRuntimeID(p.id as u64),
        platform_chat_id: String::new(),
        position: (p.pos.0, p.pos.1 - 1.62, p.pos.2), // feet; MovePlayer uses eye height
        velocity: (0.0, 0.0, 0.0),
        rotation: p.rot,
        y_head_rotation: p.head_yaw,
        carried_item: NetworkItemStackDescriptor {
            id: 0,
            stack_size: 0,
            aux_value: 0,
            net_id: None,
            block_runtime_id: 0,
            user_data_buffer: vec![],
        },
        player_game_type: GameType::Creative,
        entity_data: spawn::player_data(&p.name),
        synced_properties: PropertySyncData {
            int_entries_list: vec![],
            float_entries_list: vec![],
        },
        abilities_data: SerializedAbilitiesData {
            target_player_raw_id: p.id,
            player_permissions: 1,
            command_permissions: CommandPermissionLevel::Any,
            layers: vec![],
        },
        actor_links: vec![],
        passenger_of_block_data: None,
        device_id: String::new(),
        build_platform: BuildPlatform::Win32,
    }))
}

fn move_player(id: i64, pos: (f32, f32, f32), rot: (f32, f32), head_yaw: f32) -> Protocol {
    Protocol::MovePlayerPacket(Box::new(MovePlayerPacket {
        player_runtime_id: ActorRuntimeID(id as u64),
        position: pos,
        rotation: rot,
        y_head_rotation: head_yaw,
        position_mode: PlayerPositionMode::Normal,
        on_ground: true,
        riding_runtime_id: ActorRuntimeID(0),
        teleport_data: None,
        tick: 0,
    }))
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

//! Rusted load tester: connects fake players that walk around and keep loading new chunks,
//! and prints the server's tick times over RCON.
//!
//!   cargo run --release --bin loadtest -- [address] [bots] [seconds] [rcon-port] [rcon-password]
//!   cargo run --release --bin loadtest -- 127.0.0.1:19132 50 300 25575 secret
//!
//! The server must not require Xbox sign-in for these names, and should allow that many players
//! (`max-players`). Without RCON details it only reports what the bots see.

use bedrock::auth::ClientData;
use bedrock::network::login::{ClientIdentity, NetworkSettings};
use bedrock::network::tokio::{Client, Connection};
use bedrock::protocol::v662::enums::PlayStatus;
use bedrock::protocol::v662::packets::{ClientToServerHandshakePacket, LoginPacket, RequestNetworkSettingsPacket};
use bedrock::protocol::{ProtoVersion, V2225};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

type Protocol = V2225;
const PROTOCOL: i32 = Protocol::PROTOCOL_VERSION.cast_signed();

#[derive(Default)]
struct Stats {
    joined: AtomicU64,
    failed: AtomicU64,
    dropped: AtomicU64,
    chunks: AtomicU64,
    packets: AtomicU64,
    /// Sum of join times in ms, for the average.
    join_ms: AtomicU64,
}

type Err = Box<dyn std::error::Error + Send + Sync>;

async fn recv_one(conn: &mut Connection<Protocol>) -> Result<Protocol, Err> {
    conn.recv().await?.into_iter().next().ok_or_else(|| "connection closed".into())
}

/// Network settings, login, encryption, resource packs and spawn. Returns the runtime id and
/// where the bot spawned.
async fn join(conn: &mut Connection<Protocol>, identity: &ClientIdentity, stats: &Stats) -> Result<(u64, (f32, f32, f32)), Err> {
    conn.send(&[Protocol::RequestNetworkSettingsPacket(Box::new(RequestNetworkSettingsPacket { client_network_version: PROTOCOL }))]).await?;
    let Protocol::NetworkSettingsPacket(settings) = recv_one(conn).await? else { return Err("expected network settings".into()) };
    conn.enable_compression(NetworkSettings::from_wire(settings.compression_algorithm as u16, settings.compression_threshold)?.compression);
    conn.send(&[Protocol::LoginPacket(Box::new(LoginPacket { client_network_version: PROTOCOL, connection_request: identity.request().to_vec() }))]).await?;

    use bedrock::protocol::v2168::packets::ResourcePackClientResponsePacket as R;
    let mut runtime = None;
    let mut pos = (0.0, 0.0, 0.0);
    loop {
        for packet in conn.recv().await? {
            stats.packets.fetch_add(1, Ordering::Relaxed);
            match packet {
                Protocol::ServerToClientHandshakePacket(h) => {
                    conn.enable_encryption(identity.finish_encryption(&h.handshake_web_token)?);
                    conn.send(&[Protocol::ClientToServerHandshakePacket(Box::new(ClientToServerHandshakePacket {}))]).await?;
                }
                Protocol::PlayStatusPacket(p) => match p.status {
                    PlayStatus::LoginSuccess => {}
                    PlayStatus::PlayerSpawn => {
                        let id = runtime.ok_or("spawned before StartGame")?;
                        conn.send(&[Protocol::SetLocalPlayerAsInitializedPacket(Box::new(bedrock::protocol::v662::packets::SetLocalPlayerAsInitializedPacket { player_id: bedrock::protocol::v662::types::ActorRuntimeID(id) }))]).await?;
                        return Ok((id, pos));
                    }
                    other => return Err(format!("refused: {other:?}").into()),
                },
                // Pretend to have every pack already, then accept the stack.
                Protocol::ResourcePacksInfoPacket(_) => conn.send(&[Protocol::ResourcePackClientResponsePacket(Box::new(R::DownloadingFinished))]).await?,
                Protocol::ResourcePackStackPacket(_) => conn.send(&[Protocol::ResourcePackClientResponsePacket(Box::new(R::ResourcePackStackFinished))]).await?,
                Protocol::StartGamePacket(s) => {
                    runtime = Some(s.target_runtime_id.0);
                    pos = s.position;
                    conn.send(&[Protocol::RequestChunkRadiusPacket(Box::new(bedrock::protocol::v662::packets::RequestChunkRadiusPacket { chunk_radius: 8, max_chunk_radius: 8 }))]).await?;
                }
                Protocol::LevelChunkPacket(_) => {
                    stats.chunks.fetch_add(1, Ordering::Relaxed);
                }
                Protocol::DisconnectPacket(d) => return Err(format!("kicked: {:?}", d.message).into()),
                _ => {}
            }
        }
    }
}

/// One fake player: joins, then walks in a slowly turning circle at walking speed, reading
/// everything the server sends.
async fn bot(n: usize, addr: SocketAddr, until: Instant, stats: Arc<Stats>) {
    let name = format!("Bot{n:03}");
    let client_data = ClientData::offline(Protocol::GAME_VERSION, &addr.to_string(), &name);
    let Ok(identity) = ClientIdentity::offline(ClientIdentity::generate_key(), &name, &client_data) else { return };
    let started = Instant::now();
    let mut conn = match Client::new().timeout(Duration::from_secs(30)).connect::<Protocol>(addr).await {
        Ok(c) => c,
        Err(e) => {
            println!("{name}: connect failed: {e}");
            stats.failed.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };
    let (id, mut pos) = match tokio::time::timeout(Duration::from_secs(60), join(&mut conn, &identity, &stats)).await {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            println!("{name}: join failed: {e}");
            stats.failed.fetch_add(1, Ordering::Relaxed);
            conn.close().await;
            return;
        }
        Err(_) => {
            println!("{name}: join timed out");
            stats.failed.fetch_add(1, Ordering::Relaxed);
            conn.close().await;
            return;
        }
    };
    let _ = id;
    stats.joined.fetch_add(1, Ordering::Relaxed);
    stats.join_ms.fetch_add(started.elapsed().as_millis() as u64, Ordering::Relaxed);

    // Each bot heads off in its own direction, so together they keep generating new terrain.
    let mut yaw = n as f32 * 137.5;
    let mut tick = 0u64;
    let mut timer = tokio::time::interval(Duration::from_millis(50));
    while Instant::now() < until {
        tokio::select! {
            _ = timer.tick() => {
                tick += 1;
                yaw += 0.15; // a wide circle, about 2400 ticks a lap
                let (dx, dz) = (-yaw.to_radians().sin() * 0.2, yaw.to_radians().cos() * 0.2);
                pos = (pos.0 + dx, pos.1, pos.2 + dz);
                if conn.send(&[input(pos, yaw, tick, (dx, dz))]).await.is_err() {
                    stats.dropped.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            }
            got = conn.recv() => match got {
                Ok(batch) => {
                    for p in batch {
                        stats.packets.fetch_add(1, Ordering::Relaxed);
                        match p {
                            Protocol::LevelChunkPacket(_) => { stats.chunks.fetch_add(1, Ordering::Relaxed); }
                            // Follow the server's corrections (landing on terrain, teleports).
                            Protocol::MovePlayerPacket(m) => pos = m.position,
                            Protocol::DisconnectPacket(d) => {
                                println!("{name}: kicked: {:?}", d.message);
                                stats.dropped.fetch_add(1, Ordering::Relaxed);
                                return;
                            }
                            _ => {}
                        }
                    }
                }
                Err(_) => {
                    stats.dropped.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            }
        }
    }
    conn.close().await;
}

fn input(pos: (f32, f32, f32), yaw: f32, tick: u64, delta: (f32, f32)) -> Protocol {
    use bedrock::protocol::v2168::enums::PlayerAuthInputData as Input;
    Protocol::PlayerAuthInputPacket(Box::new(bedrock::protocol::v2193::packets::PlayerAuthInputPacket {
        player_rotation: (0.0, yaw),
        player_position: (pos.0, pos.1, pos.2),
        move_vector: (0.0, 1.0),
        player_head_rotation: yaw,
        input_data: vec![Input::Up],
        input_mode: bedrock::protocol::v662::enums::InputMode::Mouse,
        play_mode: bedrock::protocol::v2193::packets::ClientPlayMode::Normal,
        new_interaction_model: bedrock::protocol::v2168::enums::NewInteractionModel::Crosshair,
        interact_rotation: (0.0, yaw),
        client_tick: tick,
        pos_delta: (delta.0, 0.0, delta.1),
        item_use_transaction: None,
        item_stack_request: None,
        player_block_actions: None,
        vehicle_rotation: None,
        client_predicted_vehicle: None,
        analog_move_vector: (0.0, 1.0),
        camera_orientation: (0.0, 0.0, 1.0),
        raw_move_vector: (0.0, 1.0),
    }))
}

/// Source RCON (what Rusted speaks): log in, run `status`, return the reply text.
async fn rcon_status(port: u16, password: &str, host: std::net::IpAddr) -> Result<String, Err> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::TcpStream::connect((host, port)).await?;
    let packet = |id: i32, kind: i32, body: &str| {
        let mut b = Vec::new();
        b.extend((10 + body.len() as i32).to_le_bytes());
        b.extend(id.to_le_bytes());
        b.extend(kind.to_le_bytes());
        b.extend(body.as_bytes());
        b.extend([0, 0]);
        b
    };
    async fn read(s: &mut tokio::net::TcpStream) -> Result<(i32, String), Err> {
        let mut len = [0; 4];
        s.read_exact(&mut len).await?;
        let mut buf = vec![0; i32::from_le_bytes(len).clamp(10, 1 << 20) as usize];
        s.read_exact(&mut buf).await?;
        let id = i32::from_le_bytes(buf[0..4].try_into()?);
        Ok((id, String::from_utf8_lossy(&buf[8..buf.len() - 2]).into_owned()))
    }
    s.write_all(&packet(1, 3, password)).await?;
    if read(&mut s).await?.0 == -1 {
        return Err("wrong RCON password".into());
    }
    s.write_all(&packet(2, 2, "status")).await?;
    let (_, text) = read(&mut s).await?;
    // Drop colour codes for the console.
    let mut out = String::new();
    let mut skip = false;
    for c in text.chars() {
        if skip {
            skip = false;
        } else if c == '§' {
            skip = true;
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let addr: SocketAddr = args.get(1).map_or("127.0.0.1:19132", |s| s.as_str()).parse().expect("address must look like 127.0.0.1:19132");
    let bots: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
    let secs: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(120);
    let rcon = args.get(4).and_then(|p| p.parse::<u16>().ok()).map(|p| (p, args.get(5).cloned().unwrap_or_default()));

    println!("Load test: {bots} bots on {addr} for {secs}s");
    let stats = Arc::new(Stats::default());
    let start = Instant::now();
    let until = start + Duration::from_secs(secs);
    let mut tasks = Vec::new();
    for n in 0..bots {
        tasks.push(tokio::spawn(bot(n, addr, until, stats.clone())));
        // Join a few a second, like players arriving, so login isn't one burst.
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let mut report = tokio::time::interval(Duration::from_secs(10));
    report.tick().await;
    let mut last_status = String::new();
    while Instant::now() < until {
        report.tick().await;
        let joined = stats.joined.load(Ordering::Relaxed);
        let online = joined - stats.dropped.load(Ordering::Relaxed);
        println!(
            "[{:>4}s] online {online}/{bots}, failed {}, dropped {}, chunks {}, packets {}",
            start.elapsed().as_secs(),
            stats.failed.load(Ordering::Relaxed),
            stats.dropped.load(Ordering::Relaxed),
            stats.chunks.load(Ordering::Relaxed),
            stats.packets.load(Ordering::Relaxed),
        );
        if let Some((port, password)) = &rcon {
            match rcon_status(*port, password, addr.ip()).await {
                Ok(s) => {
                    for line in s.lines().filter(|l| l.contains("Tick") || l.contains("RAM") || l.contains("Mobs")) {
                        println!("         server: {line}");
                    }
                    last_status = s;
                }
                Err(e) => println!("         server: RCON failed: {e}"),
            }
        }
    }
    for t in tasks {
        let _ = t.await;
    }
    let joined = stats.joined.load(Ordering::Relaxed);
    println!("\n==== Load test finished ====");
    println!("Bots joined: {joined}/{bots} (failed {}, dropped {})", stats.failed.load(Ordering::Relaxed), stats.dropped.load(Ordering::Relaxed));
    if joined > 0 {
        println!("Average join time: {} ms", stats.join_ms.load(Ordering::Relaxed) / joined);
    }
    println!("Chunks received: {}", stats.chunks.load(Ordering::Relaxed));
    if !last_status.is_empty() {
        println!("Last server status:\n{last_status}");
    }
}

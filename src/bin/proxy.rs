//! Logging proxy for debugging. The game joins on 19134, we log into PNX on 19132
//! (offline auth) and pass every batch through untouched, logging ids and sizes.
//! Usage: proxy <listen port> <upstream addr> <log file>
//! Log lines: `<section> TAB <dir> TAB <PacketName> TAB <short debug>`; typing `#3` in chat starts section 3.

use bedrock::auth::ClientData;
use bedrock::network::compression::Compression;
use bedrock::network::login::{ClientIdentity, LoginOptions, NetworkSettings, check_protocol};
use bedrock::network::tokio::transport::{NetherNetConnection, TransportLayerConnection};
use bedrock::network::tokio::{Connection, Listener};
use bedrock::protocol::v662::enums::{PacketCompressionAlgorithm, PlayStatus};
use bedrock::protocol::v662::packets::{
    ClientToServerHandshakePacket, LoginPacket, NetworkSettingsPacket, PlayStatusPacket,
    RequestNetworkSettingsPacket,
};
use bedrock::protocol::{ProtoVersion, V2225};
use std::io::Write;
use std::sync::Arc;
use parking_lot::Mutex;

type Protocol = V2225;
type Log = Arc<Mutex<std::fs::File>>;
const PROTOCOL: i32 = Protocol::PROTOCOL_VERSION.cast_signed();

/// Per-tick spam we don't want in the log.
const SKIP: [&str; 6] = [
    "PlayerAuthInputPacket", "NetworkStackLatencyPacket", "LevelChunkPacket",
    "ServerBoundDiagnosticsPacket", "MovePlayerPacket", "MoveActorDeltaPacket",
];

fn log_batch(log: &Log, section: &Mutex<String>, dir: &str, batch: &[u8]) {
    let decoded = bedrock::network::codec::decode_packets_each::<Protocol>(
        batch.to_vec(), None, None, usize::MAX, bedrock::network::codec::UnreadBytes::Warn,
    );
    let Ok(packets) = decoded else { return };
    let mut out = log.lock();
    for (header, packet) in packets {
        let text = match &packet {
            Ok(p) => format!("{p:?}"),
            Err(e) => format!("UndecodablePacket id={} {e}", header.packet_id),
        };
        let name = text.split(['(', ' ']).next().unwrap_or("?").to_string();
        if SKIP.contains(&name.as_str()) {
            continue;
        }
        if dir == "C>S" && let Ok(Protocol::TextPacket(t)) = &packet {
            let msg = format!("{:?}", t.message_type);
            if let Some(i) = msg.find("message: \"#") {
                let marker: String = msg[i + 10..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '#').collect();
                *section.lock() = marker;
            }
        }
        let short: String = text.chars().take(400).collect();
        let _ = writeln!(out, "{}	{dir}	{name}	{short}", section.lock());
    }
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let listen: u16 = args.next().unwrap_or_else(|| "19134".into()).parse().unwrap();
    let upstream: std::net::SocketAddr = args.next().unwrap_or_else(|| "127.0.0.1:19132".into()).parse().unwrap();
    let log: Log = Arc::new(Mutex::new(std::fs::File::create(args.next().unwrap_or_else(|| "proxy_log.txt".into())).unwrap()));

    let identity = bedrock::network::nethernet::IdentityStore::load_or_generate(format!("proxy_identity_{listen}.pem"))
        .unwrap()
        .identity()
        .unwrap();
    let mut listener = Listener::builder()
        .nethernet_http(([0, 0, 0, 0], listen).into(), identity)
        .name(format!("proxy -> {upstream}"))
        .sub_name("proxy")
        .protocol::<Protocol>()
        .max_players(10)
        .bind()
        .await
        .unwrap();
    println!("proxy on {listen} -> {upstream}");

    loop {
        let conn = listener.accept::<bedrock::protocol::Unknown>().await.unwrap();
        let log = log.clone();
        tokio::spawn(async move {
            if let Err(e) = run(conn.into_ver::<Protocol>(), upstream, log).await {
                println!("session ended: {e}");
            }
        });
    }
}

async fn recv_one(conn: &mut Connection<Protocol>) -> Result<Protocol, Box<dyn std::error::Error + Send + Sync>> {
    conn.recv().await?.into_iter().next().ok_or_else(|| "closed".into())
}

async fn run(
    mut client: Connection<Protocol>,
    upstream: std::net::SocketAddr,
    log: Log,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Client side login, same as Rusted (compression, no encryption).
    let Protocol::RequestNetworkSettingsPacket(_) = recv_one(&mut client).await? else {
        return Err("expected network settings request".into());
    };
    let settings = NetworkSettings::default();
    client
        .send(&[Protocol::NetworkSettingsPacket(Box::new(NetworkSettingsPacket {
            compression_threshold: settings.threshold(),
            compression_algorithm: match settings.compression {
                Compression::Zlib { .. } => PacketCompressionAlgorithm::ZLib,
                Compression::Snappy { .. } => PacketCompressionAlgorithm::Snappy,
                Compression::None => PacketCompressionAlgorithm::None,
            },
            client_throttle_enabled: false,
            client_throttle_threshold: 0,
            client_throttle_scalar: 0.0,
        }))])
        .await?;
    client.enable_compression(settings.compression);
    let Protocol::LoginPacket(login) = recv_one(&mut client).await? else {
        return Err("expected login".into());
    };
    check_protocol(login.client_network_version, PROTOCOL)?;
    let player = LoginOptions::default().require_authentication(false).verify(&login.connection_request)?;
    println!("client {} connected; logging into upstream", player.authentication.identity().display_name);

    // Log into upstream as an offline client.
    let client_data = ClientData::offline(Protocol::GAME_VERSION, &upstream.to_string(), "ProxyUser");
    let id = ClientIdentity::offline(ClientIdentity::generate_key(), "ProxyUser", &client_data)?;
    let mut server: Connection<Protocol> =
        Connection::from_transport_conn(TransportLayerConnection::NetherNet(NetherNetConnection::connect_http(upstream).await?));
    server
        .send(&[Protocol::RequestNetworkSettingsPacket(Box::new(RequestNetworkSettingsPacket {
            client_network_version: PROTOCOL,
        }))])
        .await?;
    loop {
        match recv_one(&mut server).await? {
            Protocol::NetworkSettingsPacket(s) => {
                let s = NetworkSettings::from_wire(s.compression_algorithm as u16, s.compression_threshold)?;
                server.enable_compression(s.compression);
                server
                    .send(&[Protocol::LoginPacket(Box::new(LoginPacket {
                        client_network_version: PROTOCOL,
                        connection_request: id.request().to_vec(),
                    }))])
                    .await?;
            }
            Protocol::ServerToClientHandshakePacket(h) => {
                server.enable_encryption(id.finish_encryption(&h.handshake_web_token)?);
                server.send(&[Protocol::ClientToServerHandshakePacket(Box::new(ClientToServerHandshakePacket {}))]).await?;
            }
            Protocol::PlayStatusPacket(p) if matches!(p.status, PlayStatus::LoginSuccess) => break,
            other => return Err(format!("upstream login: unexpected {other:?}").into()),
        }
    }
    client.send(&[Protocol::PlayStatusPacket(Box::new(PlayStatusPacket { status: PlayStatus::LoginSuccess }))]).await?;
    println!("both logged in; forwarding");

    let section = Mutex::new("#0".to_string());
    loop {
        tokio::select! {
            b = client.recv_batch() => {
                let b = b?;
                log_batch(&log, &section, "C>S", &b);
                server.send_batch(b).await?;
            }
            b = server.recv_batch() => {
                let b = b?;
                log_batch(&log, &section, "S>C", &b);
                client.send_batch(b).await?;
            }
        }
    }
}

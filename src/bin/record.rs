//! Joins another Bedrock server as a fake offline client and logs every packet it sends,
//! so we can compare Rusted's join against it.
//! Usage: cargo run --release --bin record -- 127.0.0.1:19132 out.txt [dump dir]
//! With a dump dir the static join packets in `DUMP` get saved as raw frames for replay.

use bedrock::auth::ClientData;
use bedrock::network::login::{ClientIdentity, NetworkSettings};
use bedrock::network::tokio::Connection;
use bedrock::protocol::v662::enums::PlayStatus;
use bedrock::protocol::v662::packets::{
    ClientCacheStatusPacket, ClientToServerHandshakePacket, LoginPacket, RequestChunkRadiusPacket,
    RequestNetworkSettingsPacket, SetLocalPlayerAsInitializedPacket,
};
use bedrock::protocol::v2168::packets::ResourcePackClientResponsePacket;
use bedrock::protocol::{ProtoVersion, V2225};
use std::io::Write;
use std::time::Duration;

type Protocol = V2225;

/// Join packets that are the same for everyone, worth replaying as-is.
const DUMP: [&str; 9] = [
    "BiomeDefinitionListPacket", "CraftingDataPacket", "GameRulesChangedPacket", "SetDifficultyPacket",
    "CameraPresetsPacket", "CameraAimAssistPresetsPacket", "CameraSplinePacket", "TrimDataPacket", "PlayerFogPacket",
];

fn frames(batch: &[u8]) -> Vec<&[u8]> {
    let (mut out, mut i) = (vec![], 0);
    while i < batch.len() {
        let (mut len, mut shift) = (0usize, 0);
        loop {
            let Some(&b) = batch.get(i) else { return out };
            i += 1;
            len |= usize::from(b & 0x7f) << shift;
            shift += 7;
            if b & 0x80 == 0 {
                break;
            }
        }
        out.push(&batch[i..(i + len).min(batch.len())]);
        i += len;
    }
    out
}
const PROTOCOL: i32 = Protocol::PROTOCOL_VERSION.cast_signed();

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    // "raknet:127.0.0.1:19132" connects over classic RakNet (UDP) instead of NetherNet.
    let target = args.next().unwrap_or_else(|| "127.0.0.1:19132".into());
    let raknet = target.starts_with("raknet:");
    let addr: std::net::SocketAddr = target.trim_start_matches("raknet:").parse().unwrap();
    let out_path = args.next().unwrap_or_else(|| "record.txt".into());
    let dump_dir = args.next();
    if let Some(d) = &dump_dir {
        std::fs::create_dir_all(d).unwrap();
    }
    let mut out = std::fs::File::create(&out_path).unwrap();

    let client_data = ClientData::offline(Protocol::GAME_VERSION, &addr.to_string(), "Recorder");
    let identity = ClientIdentity::offline(ClientIdentity::generate_key(), "Recorder", &client_data).unwrap();
    // 1.26 servers (PNX too) only take NetherNet, signalled over HTTP on the game port.
    let mut conn: Connection<Protocol> = if raknet {
        bedrock::network::tokio::Client::new().timeout(Duration::from_secs(30)).connect::<Protocol>(addr).await.unwrap()
    } else {
        let transport = bedrock::network::tokio::transport::NetherNetConnection::connect_http(addr).await.unwrap();
        Connection::from_transport_conn(bedrock::network::tokio::transport::TransportLayerConnection::NetherNet(transport))
    };

    conn.send(&[Protocol::RequestNetworkSettingsPacket(Box::new(RequestNetworkSettingsPacket {
        client_network_version: PROTOCOL,
    }))])
    .await
    .unwrap();
    // Don't die on packets we can't decode, just log them.
    conn.bad_packets = bedrock::network::tokio::connection::BadPacketPolicy::Skip;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut n = 0;
    while let Ok(raw) = tokio::time::timeout_at(deadline, conn.recv_batch()).await {
        let raw = match raw {
            Ok(b) => b,
            Err(e) => {
                writeln!(out, "!! connection ended: {e}").unwrap();
                break;
            }
        };
        let decoded = bedrock::network::codec::decode_packets_each::<Protocol>(
            raw.clone(), None, None, usize::MAX, bedrock::network::codec::UnreadBytes::Warn,
        )
        .unwrap_or_default();
        let raw_frames = frames(&raw);
        for (k, (_, packet)) in decoded.into_iter().enumerate() {
            let Ok(packet) = packet else { continue };
            n += 1;
            let text = match &packet {
                Protocol::StartGamePacket(sg) => {
                    let mut sg = sg.clone();
                    sg.block_properties.clear();
                    sg.settings.rule_data.rules_list.clear();
                    format!("StartGamePacket(trimmed) {sg:#?}")
                }
                _ => format!("{packet:#?}"),
            };
            let name = text.split(['(', ' ']).next().unwrap_or("?").to_string();
            // huge packets, keep the head only
            let keep = if text.len() > 6000 { &text[..6000] } else { &text };
            writeln!(out, "===== #{n} {name} ({} chars)\n{keep}", text.len()).unwrap();
            println!("#{n} {name}");
            if let (Some(d), Some(frame)) = (&dump_dir, raw_frames.get(k))
                && DUMP.contains(&name.as_str())
            {
                std::fs::write(format!("{d}/{name}.bin"), frame).unwrap();
            }

            let reply: Vec<Protocol> = match &packet {
                Protocol::NetworkSettingsPacket(s) => {
                    let settings =
                        NetworkSettings::from_wire(s.compression_algorithm.clone() as u16, s.compression_threshold).unwrap();
                    conn.enable_compression(settings.compression);
                    vec![Protocol::LoginPacket(Box::new(LoginPacket {
                        client_network_version: PROTOCOL,
                        connection_request: identity.request().to_vec(),
                    }))]
                }
                Protocol::ServerToClientHandshakePacket(h) => {
                    conn.enable_encryption(identity.finish_encryption(&h.handshake_web_token).unwrap());
                    vec![Protocol::ClientToServerHandshakePacket(Box::new(ClientToServerHandshakePacket {}))]
                }
                Protocol::PlayStatusPacket(p) if matches!(p.status, PlayStatus::LoginSuccess) => {
                    vec![Protocol::ClientCacheStatusPacket(Box::new(ClientCacheStatusPacket { is_cache_supported: false }))]
                }
                Protocol::ResourcePacksInfoPacket(_) => vec![Protocol::ResourcePackClientResponsePacket(Box::new(ResourcePackClientResponsePacket::DownloadingFinished))],
                Protocol::ResourcePackStackPacket(_) => vec![Protocol::ResourcePackClientResponsePacket(Box::new(ResourcePackClientResponsePacket::ResourcePackStackFinished))],
                Protocol::StartGamePacket(s) => vec![
                    Protocol::RequestChunkRadiusPacket(Box::new(RequestChunkRadiusPacket {
                        chunk_radius: 4,
                        max_chunk_radius: 4,
                    })),
                    Protocol::SetLocalPlayerAsInitializedPacket(Box::new(SetLocalPlayerAsInitializedPacket {
                        player_id: s.target_runtime_id.clone(),
                    })),
                ],
                _ => vec![],
            };
            if !reply.is_empty() {
                conn.send(&reply).await.unwrap();
            }
        }
    }
    println!("wrote {n} packets to {out_path}");
}


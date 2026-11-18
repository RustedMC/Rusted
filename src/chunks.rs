//! Which chunks each player has, and streaming the missing ones.

use super::*;

pub(crate) const VIEW_RADIUS: i32 = 8;

/// Which chunks this player already has, so moving only sends the new ones.
pub(crate) struct ChunkTracker {
    pub(crate) sent: std::collections::HashSet<(i32, i32)>,
    pub(crate) center: Option<(i32, i32)>,
    /// Capped by view-distance in server.properties.
    pub(crate) radius: i32,
    /// Nearest first, sent in batches.
    pub(crate) pending: std::collections::VecDeque<(i32, i32)>,
    pub(crate) last_batch: Option<std::time::Instant>,
}

impl ChunkTracker {
    pub(crate) fn recenter(&mut self, cx: i32, cz: i32) -> Vec<(i32, i32)> {
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
    pub(crate) async fn update(
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

/// At most 160 chunks/s. At ~1000/s the client froze and timed out.
pub(crate) const CHUNK_BATCH: usize = 16;

pub(crate) const BATCH_GAP: std::time::Duration = std::time::Duration::from_millis(100);

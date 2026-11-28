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
    /// Chunks the last `update` sent, for the chunk-load plugin event.
    pub(crate) just_sent: Vec<(i32, i32)>,
    /// Chunks the client dropped on the last recenter, for the chunk-unload plugin event.
    pub(crate) just_dropped: Vec<(i32, i32)>,
    /// A streaming batch is being generated in the background; its packets arrive on `ready`
    /// so the player's connection keeps running meanwhile (fast elytra flight froze it).
    pub(crate) inflight: bool,
    pub(crate) ready_tx: tokio::sync::mpsc::UnboundedSender<Vec<Protocol>>,
    pub(crate) ready: tokio::sync::mpsc::UnboundedReceiver<Vec<Protocol>>,
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
        let dropped = &mut self.just_dropped;
        self.sent.retain(|&(x, z)| {
            let keep = in_view(x - cx, z - cz);
            if !keep {
                dropped.push((x, z));
            }
            keep
        });
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
        let background = limit == CHUNK_BATCH;
        let paced = limit != usize::MAX && (self.last_batch.is_some_and(|t| t.elapsed() < BATCH_GAP) || (background && self.inflight));
        if self.pending.is_empty() || paced {
            return if packets.is_empty() { Ok(()) } else { conn.send(&packets).await };
        }
        self.last_batch = Some(std::time::Instant::now());
        let n = limit.min(self.pending.len());
        let coords: Vec<(i32, i32)> = self.pending.drain(..n).collect();
        self.just_sent.extend(&coords);
        // Generate on blocking threads so no connection loop stalls on terrain.
        let mut sign_data = signs::in_chunks(&coords);
        sign_data.extend(chest_pairs_in(world, &coords));
        sign_data.extend(crate::frames::in_chunks(&coords, |p| crate::blockops::glow_frame(world, p)));
        let world = world.clone();
        let dim = conn.dim;
        // A background batch is dropped once the player is gone (their receiver closes).
        let gone = self.ready_tx.clone();
        let generate = move || {
            let threads = gen_slots().min(coords.len().max(1));
            let per = coords.len().div_ceil(threads.max(1)).max(1);
            std::thread::scope(|scope| {
                coords
                    .chunks(per)
                    .map(|part| {
                        let (world, gone) = (&world, &gone);
                        scope.spawn(move || {
                            part.iter()
                                .take_while(|_| !gone.is_closed())
                                .map(|&(x, z)| ((x, z), with_gen_slot(|| world.chunk_payload_dim(x, z, dim))))
                                .collect::<Vec<_>>()
                        })
                    })
                    .collect::<Vec<_>>()
                    .into_iter()
                    .flat_map(|h| h.join().unwrap_or_default())
                    .collect::<Vec<_>>()
            })
        };
        if background {
            // Publisher update now; the chunks follow on `ready` when they're built.
            self.inflight = true;
            let tx = self.ready_tx.clone();
            tokio::task::spawn_blocking(move || {
                let mut out = chunk_packets(generate(), dim);
                out.extend(sign_data);
                let _ = tx.send(out);
            });
            return if packets.is_empty() { Ok(()) } else { conn.send(&packets).await };
        }
        let encoded = tokio::task::spawn_blocking(generate).await.unwrap_or_default();
        packets.extend(chunk_packets(encoded, dim));
        packets.extend(sign_data);
        conn.send(&packets).await
    }
}

/// Chunks generating at once across every player: one per core, leaving one for the game tick.
/// Without this each player's batch started its own threads and 45 players meant hundreds of
/// threads fighting over the CPU (found with the load tester).
fn gen_slots() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get()).saturating_sub(1).max(1)
}

static GEN_BUSY: std::sync::Mutex<usize> = std::sync::Mutex::new(0);
static GEN_FREE: std::sync::Condvar = std::sync::Condvar::new();

/// Runs `f` once a generation slot is free.
fn with_gen_slot<T>(f: impl FnOnce() -> T) -> T {
    {
        let mut busy = GEN_BUSY.lock().unwrap_or_else(|e| e.into_inner());
        while *busy >= gen_slots() {
            busy = GEN_FREE.wait(busy).unwrap_or_else(|e| e.into_inner());
        }
        *busy += 1;
    }
    struct Release;
    impl Drop for Release {
        fn drop(&mut self) {
            *GEN_BUSY.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
            GEN_FREE.notify_one();
        }
    }
    let _release = Release; // freed even if generation panics
    f()
}

fn chunk_packets(encoded: Vec<((i32, i32), (Vec<u8>, u32))>, dim: u8) -> Vec<Protocol> {
    let mut packets = Vec::new();
    {
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
    }
    packets
}

/// At most 160 chunks/s. At ~1000/s the client froze and timed out.
pub(crate) const CHUNK_BATCH: usize = 16;

pub(crate) const BATCH_GAP: std::time::Duration = std::time::Duration::from_millis(100);

#[cfg(test)]
mod tests {
    #[test]
    fn walking_away_reports_dropped_chunks() {
        let mut t = super::ChunkTracker::default();
        t.radius = 2;
        t.recenter(0, 0);
        assert!(t.just_dropped.is_empty());
        t.recenter(10, 0);
        assert!(t.just_dropped.contains(&(0, 0)), "the old centre went out of view");
        assert!(!t.sent.contains(&(0, 0)));
    }
}

//! All dimensions live in one world, the Nether 1000 blocks up and the End 2000 up, and extra
//! worlds (/mv) 1000 apart from 3000 up, so everything position-based just works. [`DimConn`]
//! shifts coords for the client.

use bedrock::network::tokio::Connection;
use bedrock::network::tokio::error::ConnectionError;

use crate::Protocol;

pub const OVERWORLD: u8 = 0;
pub const NETHER: u8 = 1;
pub const END: u8 = 2;
/// Extra overworld-style worlds are dimensions 3.. (see `crate::multiworld`).
pub const FIRST_EXTRA: u8 = 3;
pub const MAX_EXTRA: u8 = 16;

/// An extra world made with /mv (an overworld to the client).
pub const fn is_extra(dim: u8) -> bool {
    dim >= FIRST_EXTRA
}

/// Stored y of each dimension's y = 0.
pub const fn base(dim: u8) -> i32 {
    match dim {
        NETHER => 1000,
        END => 2000,
        d if d >= FIRST_EXTRA => 3064 + (d - FIRST_EXTRA) as i32 * 1000,
        _ => 0,
    }
}

pub const fn of(y: i32) -> u8 {
    if y >= 2900 {
        let d = (y - 2900) / 1000;
        FIRST_EXTRA + if d > (MAX_EXTRA - 1) as i32 { MAX_EXTRA - 1 } else { d as u8 }
    } else if y >= 1900 {
        END
    } else if y >= 900 {
        NETHER
    } else {
        OVERWORLD
    }
}

pub fn of_f(y: f32) -> u8 {
    of(y.floor() as i32)
}

/// Bedrock dimension id (ChangeDimension / LevelChunk).
pub fn network_id(dim: u8) -> i32 {
    if is_extra(dim) { 0 } else { dim as i32 }
}

/// Inclusive stored y range.
pub const fn range(dim: u8) -> (i32, i32) {
    match dim {
        NETHER => (1000, 1127),
        END => (2000, 2255),
        d if d >= FIRST_EXTRA => (base(d) - 64, base(d) + 319),
        _ => (-64, 319),
    }
}

/// Connection wrapper that talks in the player's dimension coords.
pub struct DimConn {
    pub conn: Connection<Protocol>,
    pub dim: u8,
}

/// Entities further than this from the client's band are in another dimension and get dropped.
const BAND: std::ops::Range<f32> = -400.0..800.0;

impl DimConn {
    pub fn new(conn: Connection<Protocol>) -> Self {
        DimConn { conn, dim: OVERWORLD }
    }

    fn dy(&self) -> f32 {
        base(self.dim) as f32
    }

    pub async fn send(&mut self, packets: &[Protocol]) -> Result<(), ConnectionError> {
        let dy = self.dy();
        let out: Vec<Protocol> = packets.iter().filter_map(|p| shift(p.clone(), -dy)).collect();
        if out.is_empty() {
            return Ok(());
        }
        self.conn.send(&out).await
    }

    pub async fn recv(&mut self) -> Result<Vec<Protocol>, ConnectionError> {
        let dy = self.dy();
        let batch = self.conn.recv().await?;
        Ok(if dy == 0.0 { batch } else { batch.into_iter().filter_map(|p| shift_in(p, dy)).collect() })
    }
}

fn keep(y: f32) -> bool {
    BAND.contains(&y)
}

/// Shifts a server packet by `d`. None means drop it (entity in another dimension).
fn shift(mut p: Protocol, d: f32) -> Option<Protocol> {
    let di = d as i32;
    match &mut p {
        Protocol::UpdateBlockPacket(u) => {
            u.block_position.y += di;
            if !keep(u.block_position.y as f32) {
                return None;
            }
        }
        Protocol::BlockActorDataPacket(b) => {
            b.block_position.y += di;
            if let Some(nbtx::Value::Int(y)) = b.actor_data_tags.get_mut("y") {
                *y += di;
            }
            if !keep(b.block_position.y as f32) {
                return None;
            }
        }
        Protocol::BlockEventPacket(b) => b.block_position.y += di,
        Protocol::ContainerOpenPacket(c) => c.position.y += di,
        Protocol::OpenSignPacket(o) => o.pos.y += di,
        Protocol::LevelEventPacket(l) => {
            if l.position != (0.0, 0.0, 0.0) {
                l.position.1 += d;
                if !keep(l.position.1) {
                    return None;
                }
            }
        }
        Protocol::LevelSoundEventPacket(l) => {
            l.position.1 += d;
            if !keep(l.position.1) {
                return None;
            }
        }
        Protocol::PlaySoundPacket(s) => {
            let (x, y, z) = s.position.as_block_coords();
            if !keep(y + d) {
                return None;
            }
            s.position = bedrock::protocol::v2193::packets::PlaySoundPosition::from_raw_x8((x * 8.0) as i32, ((y + d) * 8.0) as i32, (z * 8.0) as i32);
        }
        Protocol::MovePlayerPacket(m) => {
            m.position.1 += d;
            if !keep(m.position.1) {
                return None;
            }
        }
        Protocol::AddPlayerPacket(a) => {
            a.position.1 += d;
            if !keep(a.position.1) {
                return None;
            }
        }
        Protocol::AddActorPacket(a) => {
            a.position.1 += d;
            if !keep(a.position.1) {
                return None;
            }
        }
        Protocol::AddItemActorPacket(a) => {
            a.position.1 += d;
            if !keep(a.position.1) {
                return None;
            }
        }
        Protocol::MoveActorAbsolutePacket(m) => {
            m.move_data.position.1 += d;
            if !keep(m.move_data.position.1) {
                return None;
            }
        }
        Protocol::RespawnPacket(r) => r.position.1 += d,
        Protocol::ChangeDimensionPacket(c) => c.position.1 += d,
        Protocol::NetworkChunkPublisherUpdatePacket(n) => n.new_view_position.y = (n.new_view_position.y + di).clamp(-64, 320),
        _ => {}
    }
    Some(p)
}

/// Client packets go the other way, up into the stored band.
fn shift_in(mut p: Protocol, d: f32) -> Option<Protocol> {
    let di = d as i32;
    use bedrock::protocol::v662::types::InventoryTransactionData as T;
    match &mut p {
        Protocol::PlayerAuthInputPacket(i) => i.player_position.1 += d,
        Protocol::PlayerActionPacket(a) => {
            a.block_position.y += di;
            a.result_pos.y += di;
        }
        Protocol::BlockPickRequestPacket(b) => b.position.y += di,
        Protocol::BlockActorDataPacket(b) => {
            b.block_position.y += di;
            if let Some(nbtx::Value::Int(y)) = b.actor_data_tags.get_mut("y") {
                *y += di;
            }
        }
        Protocol::LevelSoundEventPacket(l) => l.position.1 += d,
        Protocol::MovePlayerPacket(m) => m.position.1 += d,
        Protocol::MoveActorAbsolutePacket(m) => m.move_data.position.1 += d,
        Protocol::InventoryTransactionPacket(t) => match &mut t.data {
            T::ItemUse(u) => {
                u.block_position.y += di;
                u.player_position.1 += d;
            }
            T::ItemUseOnEntity(e) => e.player_position.1 += d,
            T::ItemRelease(r) => r.head_position.1 += d,
            _ => {}
        },
        _ => {}
    }
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands() {
        assert_eq!((of(64), of(1064), of(2064)), (OVERWORLD, NETHER, END));
        assert_eq!(of(-64), OVERWORLD);
        assert_eq!(range(NETHER).0, base(NETHER));
    }
}

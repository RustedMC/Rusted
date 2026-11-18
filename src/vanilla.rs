//! Join packets that are the same for everyone, replayed byte-for-byte. Captured from PNX
//! on 1.26.6x with `record <addr> <log> data/vanilla`, redo it after a Minecraft update.
//! Each file is one raw uncompressed frame (header + body).

/// Recipe book, biome colors/fog, game rules, difficulty, camera presets, armor trims.
const FRAMES: [&[u8]; 9] = [
    include_bytes!("../data/vanilla/SetDifficultyPacket.bin"),
    include_bytes!("../data/vanilla/GameRulesChangedPacket.bin"),
    include_bytes!("../data/vanilla/BiomeDefinitionListPacket.bin"),
    include_bytes!("../data/vanilla/PlayerFogPacket.bin"),
    include_bytes!("../data/vanilla/CameraPresetsPacket.bin"),
    include_bytes!("../data/vanilla/CameraAimAssistPresetsPacket.bin"),
    include_bytes!("../data/vanilla/CameraSplinePacket.bin"),
    include_bytes!("../data/vanilla/TrimDataPacket.bin"),
    include_bytes!("../data/vanilla/CraftingDataPacket.bin"),
];

/// One batch per packet since the recipe book alone is ~1 MB. varuint length + frame.
pub fn batches() -> impl Iterator<Item = Vec<u8>> {
    FRAMES.iter().map(|frame| {
        let mut batch = Vec::with_capacity(frame.len() + 5);
        let mut len = frame.len();
        loop {
            let byte = (len & 0x7f) as u8;
            len >>= 7;
            if len == 0 {
                batch.push(byte);
                break;
            }
            batch.push(byte | 0x80);
        }
        batch.extend_from_slice(frame);
        batch
    })
}

/// Vanilla gamerules (name, type, default) from the captured GameRulesChanged.
pub fn game_rules() -> Vec<bedrock::protocol::v844::types::GameRuleChanged> {
    let batch = batches().nth(1).expect("game rules frame");
    let decoded = bedrock::network::codec::decode_packets_each::<crate::Protocol>(
        batch, None, None, usize::MAX, bedrock::network::codec::UnreadBytes::Warn,
    );
    match decoded.ok().and_then(|mut d| d.pop()).and_then(|(_, p)| p.ok()) {
        Some(crate::Protocol::GameRulesChangedPacket(p)) => p.rules_data.rules_list,
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn game_rules_are_read_from_the_capture() {
        let rules = super::game_rules();
        assert!(rules.iter().any(|r| r.rule_name == "keepInventory"));
        assert!(rules.len() > 20);
    }

    #[test]
    fn every_frame_decodes_as_a_packet() {
        for batch in super::batches() {
            let decoded = bedrock::network::codec::decode_packets_each::<crate::Protocol>(
                batch, None, None, usize::MAX, bedrock::network::codec::UnreadBytes::Warn,
            )
            .unwrap();
            assert_eq!(decoded.len(), 1);
            assert!(decoded[0].1.is_ok(), "frame failed to decode: {:?}", decoded[0].1.as_ref().err());
        }
    }
}


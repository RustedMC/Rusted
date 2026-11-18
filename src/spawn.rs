//! What the client needs right after spawning (flags, attributes, abilities, inventory).
//! Skip these and there's no gravity, no jumping and the inventory won't open.

use bedrock::protocol::V2225;
use bedrock::protocol::v662::packets::{SetActorDataPacket, UpdateAbilitiesPacket};
use bedrock::protocol::v662::types::{ActorRuntimeID, DataItem, PropertySyncData};
use bedrock::protocol::v729::packets::{AttributeData, UpdateAttributesPacket};
use bedrock::protocol::v776::types::{SerializedAbilitiesData, SerializedAbilitiesLayer, SerializedLayer};
use bedrock::protocol::v924::enums::ActorDataIDs;
use bedrock::protocol::v2168::enums::DataItemType;
use bedrock::protocol::v2168::types::NetworkItemStackDescriptorV2;
use bedrock::protocol::v662::enums::CommandPermissionLevel;

type Protocol = V2225;

// Flag words copied from PNX's SetActorData via the proxy. Guessing from Cloudburst's
// enum was off by one and put "gravity" on collision.
// FLAGS = bits 19, 22, 35, 48 (collision), 49 (gravity); FLAGS_EXTENDED = bits 89, 109.
pub const FLAGS: i64 = 844_459_294_588_928;
pub const FLAGS_EXTENDED: i64 = 35_184_405_643_264;

/// Same entity data PNX sends for a player.
pub fn player_data(name: &str) -> Vec<DataItem<Protocol>> {
    use ActorDataIDs::*;
    use DataItemType as T;
    let item = |id, v| DataItem { data_item_id: id, data_item_type: v };
    vec![
        item(Reserved0, T::Int64(FLAGS)),
        item(Reserved092, T::Int64(FLAGS_EXTENDED)),
        item(Reserved054, T::Float(1.8)), // height
        item(Reserved053, T::Float(0.6)), // width
        item(BedPosition, T::Pos(bedrock::protocol::v662::types::BlockPos { x: 0, y: 0, z: 0 })),
        item(AirSupply, T::Short(400)),
        item(AirSupplyMax, T::Short(400)),
        item(Name, T::String(name.into())),
        item(LeashHolder, T::Int64(-1)),
        item(Reserved038, T::Float(1.0)), // scale
        item(StructuralIntegrity, T::Int(20)),
        item(Unknown(139), T::Int64(0)),
        item(Unknown(140), T::Float(64.0)),
        item(ContainerType, T::Byte(0)),
        item(ContainerSize, T::Int(0)),
        item(ContainerStrengthModifier, T::Int(0)),
        item(NametagAlwaysShow, T::Byte(1)),
    ]
}

pub fn set_data(id: i64, name: &str) -> Protocol {
    Protocol::SetActorDataPacket(Box::new(SetActorDataPacket {
        target_runtime_id: ActorRuntimeID(id as u64),
        actor_data: player_data(name),
        synced_properties: PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
        tick: 0,
    }))
}

fn attr(name: &str, min: f32, max: f32, value: f32) -> AttributeData<Protocol> {
    AttributeData {
        min_value: min,
        max_value: max,
        current_value: value,
        default_min: min,
        default_max: max,
        default_value: value,
        attribute_name: name.into(),
        attribute_modifiers: vec![],
    }
}

/// Only the health attribute, for updates after damage/regen.
pub fn health_packet(id: i64, health: f32) -> Protocol {
    let mut a = attr("minecraft:health", 0.0, 20.0, health);
    a.default_value = 20.0;
    Protocol::UpdateAttributesPacket(Box::new(UpdateAttributesPacket {
        target_runtime_id: ActorRuntimeID(id as u64),
        attribute_list: vec![a],
        ticks_since_sim_started: 0,
    }))
}

/// Air bubbles, in ticks (300 = full).
pub fn air_packet(id: i64, air: i16) -> Protocol {
    Protocol::SetActorDataPacket(Box::new(SetActorDataPacket {
        target_runtime_id: ActorRuntimeID(id as u64),
        actor_data: vec![DataItem { data_item_id: ActorDataIDs::AirSupply, data_item_type: DataItemType::Short(air) }],
        synced_properties: PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
        tick: 0,
    }))
}

/// Vanilla attributes. Movement speed is what makes walking feel right.
pub fn attributes(id: i64) -> Protocol {
    Protocol::UpdateAttributesPacket(Box::new(UpdateAttributesPacket {
        target_runtime_id: ActorRuntimeID(id as u64),
        attribute_list: vec![
            attr("minecraft:health", 0.0, 20.0, 20.0),
            attr("minecraft:movement", 0.0, f32::MAX, 0.1),
            attr("minecraft:underwater_movement", 0.0, f32::MAX, 0.02),
            attr("minecraft:lava_movement", 0.0, f32::MAX, 0.02),
            attr("minecraft:absorption", 0.0, 16.0, 0.0),
            attr("minecraft:player.hunger", 0.0, 20.0, 20.0),
            attr("minecraft:player.saturation", 0.0, 20.0, 20.0),
            attr("minecraft:player.exhaustion", 0.0, 5.0, 0.0),
            attr("minecraft:player.level", 0.0, 24791.0, 0.0),
            attr("minecraft:player.experience", 0.0, 1.0, 0.0),
        ],
        ticks_since_sim_started: 0,
    }))
}

// AbilitiesIndex bits for creative
const BUILD: u32 = 0;
const MINE: u32 = 1;
const DOORS: u32 = 2;
const CONTAINERS: u32 = 3;
const ATTACK_PLAYERS: u32 = 4;
const ATTACK_MOBS: u32 = 5;
const INVULNERABLE: u32 = 8;
const FLYING: u32 = 9;
const MAY_FLY: u32 = 10;
const INSTABUILD: u32 = 11;
const NO_CLIP: u32 = 17;
const ABILITY_COUNT: u32 = 19;

pub fn can_build(mode: &bedrock::protocol::v662::enums::GameType) -> bool {
    use bedrock::protocol::v662::enums::GameType;
    matches!(mode, GameType::Survival | GameType::Creative)
}

pub fn abilities(id: i64, mode: &bedrock::protocol::v662::enums::GameType) -> Protocol {
    let op = crate::commands::op_ids().lock().unwrap().contains(&id);
    use bedrock::protocol::v662::enums::GameType;
    let bits: &[u32] = match mode {
        GameType::Creative => &[BUILD, MINE, DOORS, CONTAINERS, ATTACK_PLAYERS, ATTACK_MOBS, INVULNERABLE, MAY_FLY, INSTABUILD],
        GameType::Adventure => &[DOORS, CONTAINERS, ATTACK_PLAYERS, ATTACK_MOBS],
        GameType::Spectator => &[INVULNERABLE, MAY_FLY, FLYING, NO_CLIP],
        _ => &[BUILD, MINE, DOORS, CONTAINERS, ATTACK_PLAYERS, ATTACK_MOBS],
    };
    let values = bits.iter().fold(0u32, |v, bit| v | 1 << bit);
    Protocol::UpdateAbilitiesPacket(Box::new(UpdateAbilitiesPacket {
        data: SerializedAbilitiesData {
            target_player_raw_id: id,
            // ops get operator permissions (op UI and command suggestions)
            player_permissions: if op { 2 } else { 1 },
            command_permissions: if op { CommandPermissionLevel::Admin } else { CommandPermissionLevel::Any },
            layers: vec![SerializedLayer {
                serialized_layer: SerializedAbilitiesLayer::Base,
                abilities_set: (1 << ABILITY_COUNT) - 1,
                ability_values: values,
                fly_speed: 0.05,
                vertical_fly_speed: 1.0,
                walk_speed: 0.1,
            }],
        },
    }))
}

pub fn air() -> NetworkItemStackDescriptorV2 {
    NetworkItemStackDescriptorV2 {
        id: 0,
        stack_size: 0,
        aux_value: 0,
        net_id: None,
        block_runtime_id: 0,
        user_data_buffer: vec![0; 10],
    }
}

/// Inventory tabs and layout (PNX sendInitialInventoryOptions).
pub fn inventory_options() -> Protocol {
    use bedrock::protocol::v662::enums::{InventoryLayout, InventoryLeftTabIndex, InventoryRightTabIndex};
    use bedrock::protocol::v662::packets::SetPlayerInventoryOptionsPacket;
    Protocol::SetPlayerInventoryOptionsPacket(Box::new(SetPlayerInventoryOptionsPacket {
        left_inventory_tab: InventoryLeftTabIndex::None,
        right_inventory_tab: InventoryRightTabIndex::None,
        filtering: false,
        layout_inv: InventoryLayout::Creative,
        layout_craft: InventoryLayout::Creative,
    }))
}

/// Sent once the client says it finished loading. PNX sends these on
/// SetLocalPlayerAsInitialized, the client ignores them any earlier.
pub fn after_load(id: i64, name: &str, inventory: Vec<Protocol>) -> Vec<Protocol> {
    use bedrock::protocol::v662::enums::ContainerID;
    use bedrock::protocol::v662::packets::PlayerHotbarPacket;
    let hotbar = Protocol::PlayerHotbarPacket(Box::new(PlayerHotbarPacket {
        selected_slot: 0,
        container_id: ContainerID(0),
        should_select_slot: true,
    }));
    let mut out = vec![inventory_options()];
    out.extend(inventory);
    out.extend([hotbar, set_data(id, name), attributes(id), abilities(id, &bedrock::protocol::v662::enums::GameType::Creative)]);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn gravity_and_collision_bits_set() {
        assert_ne!(super::FLAGS & (1 << 49), 0, "gravity");
        assert_ne!(super::FLAGS & (1 << 48), 0, "collision");
    }
}

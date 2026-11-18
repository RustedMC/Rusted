//! Small packet builders used all over the session code.

use super::*;

/// Opens a villager's trade screen (PNX `EntityVillagerV2.updateTrades`).
pub(crate) fn trade_packet(window: i8, villager: i64, player: i64, job: &villagers::Job, tags: HashMap<String, nbtx::Value>) -> Protocol {
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

pub(crate) fn enchant_options(offers: &[(u32, tables::Offer)]) -> Protocol {
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

/// Our slash commands plus argument enums, for client autocomplete.
pub(crate) fn available_commands(plugins: &plugin::Plugins) -> Protocol {
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
pub(crate) const ARG_VALID: u32 = 0x10_0000;

pub(crate) const ARG_ENUM: u32 = 0x20_0000;

pub(crate) const INT: u32 = ARG_VALID | 1;

pub(crate) const MESSAGE: u32 = ARG_VALID | 68; // rest of the line, as PNX's /tell uses

/// One command with one or more overloads (alternative argument lists).
pub(crate) fn command(name: &str, description: &str, overloads: &[&[(&str, u32, bool)]]) -> bedrock::protocol::v898::packets::CommandsEntry {
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

pub(crate) fn disconnect_packet(reason: players::Kick, message: &str) -> Protocol {
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
pub(crate) fn clock_packet(time: i32) -> Protocol {
    Protocol::SyncWorldClocksPacket(Box::new(SyncWorldClocksPacket {
        data: SyncWorldClocks::SyncState {
            clock_data: vec![SyncWorldClockState { clock_id: 7_194_480_507_151_251_734, time, paused: false }],
        },
    }))
}

pub(crate) fn game_rules_packet(rules_list: Vec<bedrock::protocol::v844::types::GameRuleChanged>) -> Protocol {
    Protocol::GameRulesChangedPacket(Box::new(bedrock::protocol::v662::packets::GameRulesChangedPacket {
        rules_data: bedrock::protocol::v844::types::GameRulesChangedPacketData { rules_list },
    }))
}

pub(crate) fn time_packet(time: i32) -> Protocol {
    Protocol::SetTimePacket(Box::new(bedrock::protocol::v662::packets::SetTimePacket { time }))
}

pub(crate) fn update_block(x: i32, y: i32, z: i32, block: i32) -> Protocol {
    Protocol::UpdateBlockPacket(Box::new(UpdateBlockPacket {
        block_position: NetworkBlockPosition { x, y, z },
        block_runtime_id: block as u32, // hashed IDs go over the wire as their bit pattern
        flags: 0b11, // neighbours + network
        layer: 0,
    }))
}

pub(crate) fn level_event(event_id: i32, p: (i32, i32, i32)) -> Protocol {
    Protocol::LevelEventPacket(Box::new(LevelEventPacket { event_id, position: (p.0 as f32 + 0.5, p.1 as f32 + 0.5, p.2 as f32 + 0.5), data: 0 }))
}

/// A named sound at a position (PNX `Level.addSound` -> PlaySoundPacket).
pub(crate) fn play_sound(name: &str, at: (f32, f32, f32)) -> Protocol {
    play_sound_pitch(name, at, 1.0)
}

pub(crate) fn play_sound_pitch(name: &str, at: (f32, f32, f32), pitch: f32) -> Protocol {
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

/// Chest block-entity data: tells clients which chests form a double chest (PNX
/// `BlockEntityChest` spawn compound: pairx/pairz/pairlead).
pub(crate) fn chest_data(pos: (i32, i32, i32), pair: Option<(i32, i32, i32)>) -> Protocol {
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

/// A chest's block-entity data with a custom title (plugin menus).
pub(crate) fn named_chest(pos: (i32, i32, i32), title: &str) -> Protocol {
    let mut tags = HashMap::new();
    tags.insert("id".to_string(), nbtx::Value::String("Chest".into()));
    tags.insert("x".to_string(), nbtx::Value::Int(pos.0));
    tags.insert("y".to_string(), nbtx::Value::Int(pos.1));
    tags.insert("z".to_string(), nbtx::Value::Int(pos.2));
    tags.insert("CustomName".to_string(), nbtx::Value::String(title.into()));
    Protocol::BlockActorDataPacket(Box::new(bedrock::protocol::v662::packets::BlockActorDataPacket {
        block_position: NetworkBlockPosition { x: pos.0, y: pos.1, z: pos.2 },
        actor_data_tags: tags,
    }))
}

/// MobEffectPacket: add/update (`Some`) or remove (`None`) effect `e` on player `id`.
pub(crate) fn effect_packet(id: i64, e: i32, x: Option<effects::Effect>) -> Protocol {
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
pub(crate) fn absorption_packet(id: i64, value: f32) -> Protocol {
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
pub(crate) fn movement_packet(id: i64, speed: f32) -> Protocol {
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

pub(crate) fn chat_packet(from: String, message: String) -> Protocol {
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

pub(crate) fn player_list_add(p: &players::PlayerInfo) -> Protocol {
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

pub(crate) fn add_player(p: &players::PlayerInfo) -> Protocol {
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

pub(crate) fn move_player(id: i64, pos: (f32, f32, f32), rot: (f32, f32), head_yaw: f32) -> Protocol {
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

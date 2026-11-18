//! Login, encryption and the StartGame sequence.

use super::*;

pub(crate) async fn recv_one(conn: &mut Connection<Protocol>) -> Result<Protocol, ConnectionError> {
    let batch = conn.recv().await?;
    batch
        .into_iter()
        .next()
        .ok_or(ConnectionError::ConnectionClosed)
}

pub(crate) fn play_status(status: PlayStatus) -> Protocol {
    Protocol::PlayStatusPacket(Box::new(PlayStatusPacket { status }))
}

pub(crate) async fn login(
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

pub(crate) async fn handle_login(unknown_conn: Connection<Unknown>, server: players::Server) {
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

/// A new level from server.properties (level-type, level-seed, flat-layers).
pub(crate) fn new_world() -> std::io::Result<world::World> {
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

pub(crate) fn game_type_from(s: &str) -> Option<GameType> {
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
pub(crate) fn data_driven_experiments() -> Experiments {
    Experiments {
        experiments: vec![bedrock::protocol::v662::types::Experiment {
            name: "data_driven_vanilla_blocks_and_items".into(),
            enabled: true,
        }],
        ever_toggled: false,
    }
}

pub(crate) fn level_settings(time: i32, spawn: (f32, f32, f32), seed: u64, mode: GameType, op: bool) -> LevelSettings<Protocol> {
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

pub(crate) fn start_game_packet(id: i64, name: &str, position: (f32, f32, f32), time: i32, spawn: (f32, f32, f32), seed: u64, mode: GameType) -> StartGamePacket<Protocol> {
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
pub(crate) fn jigsaw_structure_data() -> nbtx::Value {
    let gz = include_bytes!("../data/jigsaw_structure_data.nbt");
    let mut raw = flate2::read::GzDecoder::new(&gz[..]);
    nbtx::from_bytes::<nbtx::BigEndian, nbtx::Value>(&mut raw).expect("bad jigsaw_structure_data.nbt")
}

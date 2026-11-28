//! Chat and commands run in this player's session.

use super::*;
use bedrock::protocol::ProtoVersionPackets;
use players::Event;

type CommandRequest = <Protocol as ProtoVersionPackets>::CommandRequestPacket;
type Text = <Protocol as ProtoVersionPackets>::TextPacket;

impl Session {
    pub(super) async fn on_command_request(&mut self, cmd: &CommandRequest) {
        let mut line = cmd.command.trim().trim_start_matches('/').to_string();
        if let Some(me) = self.server.get(self.id) && !self.server.plugins.command_preprocess(&self.server, &me, &mut line) {
            return;
        }
        println!("{} issued server command: /{line}", self.name);
        let first = line.split_whitespace().next().unwrap_or("");
        if commands::OP_ONLY.contains(&first) && !commands::is_op(&self.name) {
            let _ = self.conn.send(&[chat_packet(String::new(), "§cYou do not have permission to use this command".to_string())]).await;
            return;
        }
        // A player argument (@a, @p, a name...) runs the command in each target's session.
        match commands::expand(&self.server, self.id, &line) {
            Ok(runs) => {
                for (to, l) in runs {
                    if to == self.id {
                        let _ = self.cmd_tx.send((l, None));
                    } else {
                        let _ = self.server.events.send(Event::RunAs { to, line: l, reply_to: self.id });
                    }
                }
            }
            Err(msg) => {
                let _ = self.conn.send(&[chat_packet(String::new(), msg)]).await;
            }
        }
    }

    pub(super) fn on_chat(&mut self, text: Text) {
        let TextPacketType::Chat { mut message, .. } = text.message_type else { return };
        if let Some(me) = self.server.get(self.id) && !self.server.plugins.chat(&self.server, &me, &mut message) {
            return;
        }
        println!("<{}> {message}", self.name);
        let _ = self.server.events.send(Event::Chat { from: self.name.clone(), message });
    }

    pub async fn run_command(&mut self, line: String, reply_to: Option<i64>) {
        let mut args: Vec<&str> = line.split_whitespace().collect();
        if args.len() > 1 && args[1] == "@s" && matches!(args[0], "kill" | "clear" | "give" | "spawnpoint" | "tp" | "teleport" | "title" | "titleraw" | "tellraw" | "stopsound" | "damage" | "camerashake" | "transfer" | "clearspawnpoint") {
            args.remove(1);
        }
        let here = self.server.get(self.id).map_or((0.0, 0.0, 0.0), |p| (p.pos.0, p.pos.1 - 1.62, p.pos.2));
        let plugin_reply = match (self.server.get(self.id), args.split_first()) {
            (Some(me), Some((cmd, rest))) => self.server.plugins.command(&self.server, &me, cmd, rest),
            _ => None,
        };
        let denied = false; // checked when the command was issued
        let reply = if let Some(r) = plugin_reply { r } else if denied {
            "§cYou do not have permission to use this command".to_string()
        } else if !self.server.plugins.command_event(&self.server, &self.name, &args) {
            "§cA plugin stopped that".to_string()
        } else if let Some(r) = commands::run(&self.server, &self.name, &args) {
            r
        } else if let Some(r) = self.player_command(&args, here).await {
            r
        } else if let Some(r) = self.world_command(&args, here) {
            r
        } else if let Some(r) = self.vanilla_command(&args, here).await {
            r
        } else if let Some(r) = self.camera_command(&args, here).await {
            r
        } else {
            self.misc_command(&args, here).await
        };
        if !reply.is_empty() {
            match reply_to {
                Some(to) => {
                    let _ = self.server.events.send(Event::Tell { to, message: reply });
                }
                None => {
                    let _ = self.conn.send(&[chat_packet(String::new(), reply)]).await;
                }
            }
        }
    }

    /// Commands about this player. None if `args` isn't one of them.
    async fn player_command(&mut self, args: &[&str], here: (f32, f32, f32)) -> Option<String> {
        Some(match args {
            ["gamemode" | "gm", arg, ..] => {
                match game_type_from(arg) {
                    Some(new) if self.server.get(self.id).is_some_and(|me| !self.server.plugins.gamemode_change(&self.server, &me, &format!("{new:?}").to_lowercase())) => {
                        let _ = new;
                        "§cGame mode change was cancelled".to_string()
                    }
                    Some(new) => {
                        set_game_mode(&mut self.conn, self.id, &new).await;
                        let msg = format!("Set own game mode to {new:?}");
                        self.mode = new;
                        self.server.mobs.set_targetable(self.id, survival_like(&self.mode));
                        save_mode(&self.name, &self.mode);
                        msg
                    }
                    None => "§cUse /gamemode creative|survival|adventure|spectator".into(),
                }
            }
            ["summon", what] => match mobs::Kind::from_name(what) {
                Some(kind) => {
                    let pos = self.server.get(self.id).map_or(self.server.world.spawn(), |p| p.pos);
                    self.server.mobs.spawn(&self.server, kind, (pos.0, pos.1 - 1.62, pos.2));
                    "Object successfully summoned".to_string()
                }
                None => format!("§cUnknown entity: {what}. Known: {}", mobs::Kind::ALL.iter().map(|k| k.identifier().trim_start_matches("minecraft:")).collect::<Vec<_>>().join(", ")),
            },
            ["effect", who, rest @ ..] => {
                let target = if *who == "@s" || who.eq_ignore_ascii_case(&self.name) { Some(self.id) } else { self.server.find_by_name(who) };
                match (target, rest) {
                    (None, _) => format!("§cNo player named {who}"),
                    (Some(t), ["clear"]) => {
                        effects::QUEUE.lock().push((t, effects::Queued::Clear));
                        format!("Took all effects from {who}")
                    }
                    (Some(t), args) => match effects::parse(args) {
                        Some((e, x)) => {
                            effects::QUEUE.lock().push((t, effects::Queued::Give(e, x)));
                            format!("Gave {} * {} to {who} for {} seconds", args[0], x.amplifier, x.ticks / 20)
                        }
                        None => "§cUse /effect <player> <effect> [seconds] [amplifier] [hideParticles] or /effect <player> clear".into(),
                    },
                }
            }
            ["enchant", who, ench, rest @ ..] => {
                let target = if *who == "@s" || who.eq_ignore_ascii_case(&self.name) { Some(self.id) } else { self.server.find_by_name(who) };
                let lvl: Option<i16> = rest.first().map_or(Some(1), |l| l.parse().ok());
                match (target, enchant::by_name(ench), lvl) {
                    (None, ..) => format!("§cNo player named {who}"),
                    (_, None, _) => format!("§cUnknown enchantment: {ench}"),
                    (_, _, None) => "§cUse /enchant <player> <enchantment> [level]".into(),
                    (_, Some((_, max)), Some(l)) if l < 1 || l > max => format!("§cLevel {l} is not supported by that enchantment (max {max})"),
                    (Some(t), Some((e, _)), Some(l)) => {
                        enchant::QUEUE.lock().push((t, e, l));
                        let _ = &who;
                        "Enchanting succeeded".to_string()
                    }
                }
            }
            ["xp", amount, rest @ ..] => {
                let who = rest.first().copied().unwrap_or("@s");
                let target = if who == "@s" || who.eq_ignore_ascii_case(&self.name) { Some(self.id) } else { self.server.find_by_name(who) };
                let (levels, n) = match amount.strip_suffix(['L', 'l']) {
                    Some(l) => (true, l.parse::<i32>().ok()),
                    None => (false, amount.parse::<i32>().ok()),
                };
                match (target, n) {
                    (None, _) => format!("§cNo player named {who}"),
                    (_, None) => "§cUse /xp <amount>[L] [player]".into(),
                    (Some(t), Some(n)) if levels => {
                        if t == self.id {
                            self.xp.add_levels(n);
                            save_xp(&self.name, self.xp);
                            let _ = self.conn.send(&[self.xp.packet(self.id)]).await;
                        } else {
                            // Another player: give the points those levels take from level 0.
                            let points: i32 = (0..n.max(0)).map(xp::needed).sum();
                            let _ = self.server.events.send(Event::Xp { to: t, points });
                        }
                        format!("Gave {n} levels to {who}")
                    }
                    (Some(t), Some(n)) => {
                        let _ = self.server.events.send(Event::Xp { to: t, points: n });
                        format!("Gave {n} experience to {who}")
                    }
                }
            }
            ["give", item, rest @ ..] if items::item_by_name(item).is_none() && rest.first().is_some_and(|i| items::item_by_name(i).is_some()) => {
                // `/give <someone> <item>` that couldn't resolve the player.
                format!("§cNo player was found: {item}")
            }
            ["title" | "titleraw", kind, text @ ..] => {
                use bedrock::protocol::v712::packets::{SetTitlePacket, TitleType};
                let raw = args[0] == "titleraw";
                let text = if raw { rawtext(&text.join(" ")) } else { text.join(" ") };
                let (title_type, times) = match *kind {
                    "title" => (Some(TitleType::Title), (10, 70, 20)),
                    "subtitle" => (Some(TitleType::Subtitle), (10, 70, 20)),
                    "actionbar" => (Some(TitleType::Actionbar), (10, 70, 20)),
                    "clear" => (Some(TitleType::Clear), (0, 0, 0)),
                    "reset" => (Some(TitleType::Reset), (0, 0, 0)),
                    "times" => {
                        let n: Vec<i32> = text.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                        (Some(TitleType::Times), (n.first().copied().unwrap_or(10), n.get(1).copied().unwrap_or(70), n.get(2).copied().unwrap_or(20)))
                    }
                    _ => (None, (0, 0, 0)),
                };
                match title_type {
                    Some(title_type) => {
                        let _ = self.conn.send(&[Protocol::SetTitlePacket(Box::new(SetTitlePacket {
                            title_type,
                            title_text: text.clone(),
                            fade_in_time: times.0,
                            stay_time: times.1,
                            fade_out_time: times.2,
                            xuid: String::new(),
                            platform_online_id: String::new(),
                            filtered_title_text: text,
                        }))]).await;
                        String::new()
                    }
                    None => "§cUse /title <player> title|subtitle|actionbar|clear|reset|times <text>".into(),
                }
            }
            ["tellraw", json @ ..] => {
                let _ = self.conn.send(&[chat_packet(String::new(), rawtext(&json.join(" ")))]).await;
                String::new()
            }
            ["playsound", sound, rest @ ..] => {
                // /playsound <sound> [player] [x y z] [volume] [pitch]
                let rest = if rest.first() == Some(&"@s") { &rest[1..] } else { rest };
                let at = if rest.len() >= 3 { coords(&rest[..3], here) } else { None }.unwrap_or(here);
                let pitch = rest.get(4).and_then(|p| p.parse().ok()).unwrap_or(1.0);
                let _ = self.conn.send(&[play_sound_pitch(sound, at, pitch)]).await;
                String::new()
            }
            ["stopsound", rest @ ..] => {
                let stop = Protocol::StopSoundPacket(Box::new(bedrock::protocol::v712::packets::StopSoundPacket {
                    sound_name: rest.first().map(|s| s.to_string()).unwrap_or_default(),
                    stop_all_sounds: rest.is_empty(),
                    stop_music_legacy: false,
                }));
                let _ = self.conn.send(&[stop]).await;
                String::new()
            }
            ["camerashake", rest @ ..] => {
                // /camerashake add <player> [intensity] [seconds] [positional|rotational] | stop <player>
                use bedrock::protocol::v662::enums::{CameraShakeAction, CameraShakeType};
                let (action, rest) = match rest.first() {
                    Some(&"stop") => (CameraShakeAction::Stop, &rest[1..]),
                    Some(&"add") => (CameraShakeAction::Add, &rest[1..]),
                    _ => (CameraShakeAction::Add, rest),
                };
                let rest = if rest.first() == Some(&"@s") { &rest[1..] } else { rest };
                let shake = Protocol::CameraShakePacket(Box::new(bedrock::protocol::v662::packets::CameraShakePacket {
                    intensity: rest.first().and_then(|v| v.parse().ok()).unwrap_or(0.5),
                    seconds: rest.get(1).and_then(|v| v.parse().ok()).unwrap_or(1.0),
                    shake_type: if rest.get(2) == Some(&"rotational") { CameraShakeType::Rotational } else { CameraShakeType::Positional },
                    shake_action: action,
                }));
                let _ = self.conn.send(&[shake]).await;
                String::new()
            }
            ["transfer", address, rest @ ..] => {
                let port = rest.first().and_then(|p| p.parse().ok()).unwrap_or(19132);
                let _ = self.conn.send(&[Protocol::TransferPlayerPacket(Box::new(bedrock::protocol::v2168::packets::TransferPlayerPacket {
                    server_address: address.to_string(),
                    server_port: port,
                    reload_world: false,
                                gatherings_config: None,
                }))]).await;
                format!("Transferring to {address}:{port}")
            }
            ["damage", amount, ..] => match amount.parse::<f32>() {
                Ok(d) if d >= 0.0 => {
                    if survival_like(&self.mode) {
                        hurt(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, health::Damage::Magic(d), &mut self.inv, &self.effects, &self.mode).await;
                    }
                    format!("Applied damage to {}", self.name)
                }
                _ => "§cUse /damage <target> <amount>".into(),
            },
            _ => return None,
        })
    }

    /// Commands that change the world. None if `args` isn't one of them.
    fn world_command(&mut self, args: &[&str], here: (f32, f32, f32)) -> Option<String> {
        Some(match args {
            ["setblock", x, y, z, block, rest @ ..] => match (coords(&[x, y, z], here), block_by_name(block)) {
                (None, _) => "§cBad position".into(),
                (_, None) => format!("§cUnknown block: {block}"),
                (Some(p), Some(b)) => {
                    let p = (p.0.floor() as i32, p.1.floor() as i32, p.2.floor() as i32);
                    let current = self.server.world.get(p.0, p.1, p.2);
                    match rest.first().copied().unwrap_or("replace") {
                        "keep" if current != world::AIR => "§cCould not set the block".into(),
                        mode => {
                            if mode == "destroy" {
                                break_block(&self.server, self.id, p, Some(None), &[]);
                            }
                            self.server.set_block(self.id, p.0, p.1, p.2, b);
                            "Block placed".into()
                        }
                    }
                }
            },
            ["fill", x1, y1, z1, x2, y2, z2, block, rest @ ..] => match (coords(&[x1, y1, z1], here), coords(&[x2, y2, z2], here), block_by_name(block)) {
                (Some(a), Some(b), Some(bl)) => {
                    let (lo, hi) = (
                        (a.0.min(b.0).floor() as i32, a.1.min(b.1).floor() as i32, a.2.min(b.2).floor() as i32),
                        (a.0.max(b.0).floor() as i32, a.1.max(b.1).floor() as i32, a.2.max(b.2).floor() as i32),
                    );
                    let n = (hi.0 - lo.0 + 1) as i64 * (hi.1 - lo.1 + 1) as i64 * (hi.2 - lo.2 + 1) as i64;
                    if n > 32_768 {
                        format!("§cToo many blocks in the specified area ({n} > 32768)")
                    } else {
                        let mode = rest.first().copied().unwrap_or("replace");
                        let mut changed = 0;
                        for x in lo.0..=hi.0 {
                            for y in lo.1..=hi.1 {
                                for z in lo.2..=hi.2 {
                                    let edge = x == lo.0 || x == hi.0 || y == lo.1 || y == hi.1 || z == lo.2 || z == hi.2;
                                    let cur = self.server.world.get(x, y, z);
                                    let want = match mode {
                                        "keep" if cur != world::AIR => continue,
                                        "outline" if !edge => continue,
                                        "hollow" if !edge => world::AIR,
                                        _ => bl,
                                    };
                                    if cur != want {
                                        self.server.set_block_quiet(self.id, x, y, z, want);
                                        changed += 1;
                                    }
                                }
                            }
                        }
                        format!("{changed} blocks filled")
                    }
                }
                (_, _, None) => format!("§cUnknown block: {block}"),
                _ => "§cBad position".into(),
            },
            ["clone", x1, y1, z1, x2, y2, z2, dx, dy, dz, ..] => match (coords(&[x1, y1, z1], here), coords(&[x2, y2, z2], here), coords(&[dx, dy, dz], here)) {
                (Some(a), Some(b), Some(d)) => {
                    let lo = (a.0.min(b.0).floor() as i32, a.1.min(b.1).floor() as i32, a.2.min(b.2).floor() as i32);
                    let hi = (a.0.max(b.0).floor() as i32, a.1.max(b.1).floor() as i32, a.2.max(b.2).floor() as i32);
                    let d = (d.0.floor() as i32, d.1.floor() as i32, d.2.floor() as i32);
                    let n = (hi.0 - lo.0 + 1) as i64 * (hi.1 - lo.1 + 1) as i64 * (hi.2 - lo.2 + 1) as i64;
                    if n > 32_768 {
                        format!("§cToo many blocks in the specified area ({n} > 32768)")
                    } else {
                        let mut copy = Vec::new();
                        for x in lo.0..=hi.0 {
                            for y in lo.1..=hi.1 {
                                for z in lo.2..=hi.2 {
                                    copy.push(((x - lo.0, y - lo.1, z - lo.2), self.server.world.get(x, y, z)));
                                }
                            }
                        }
                        for ((ox, oy, oz), b) in &copy {
                            self.server.set_block_quiet(self.id, d.0 + ox, d.1 + oy, d.2 + oz, *b);
                        }
                        format!("{} blocks cloned", copy.len())
                    }
                }
                _ => "§cBad position".into(),
            },
            ["testforblock", x, y, z, block, ..] => match (coords(&[x, y, z], here), block_by_name(block)) {
                (Some(p), Some(b)) => {
                    let cur = self.server.world.get(p.0.floor() as i32, p.1.floor() as i32, p.2.floor() as i32);
                    if blocks::entry(cur).map(|e| e.name.as_str()) == blocks::entry(b).map(|e| e.name.as_str()) {
                        "Successfully found the block".into()
                    } else {
                        format!("§cThe block is {}", blocks::entry(cur).map_or("unknown", |e| e.name.as_str()))
                    }
                }
                _ => "§cUse /testforblock <x> <y> <z> <block>".into(),
            },
            ["time", "add", n] => match n.parse::<i32>() {
                Ok(n) => {
                    let t = self.server.time() + n;
                    self.server.set_time(t);
                    let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(clock_packet(t)) });
                    let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(time_packet(t)) });
                    format!("Added {n} to the time")
                }
                Err(_) => "§cUse /time add <ticks>".into(),
            },
            ["time", "query", what] => {
                let t = self.server.time();
                match *what {
                    "daytime" => format!("Daytime is {}", t.rem_euclid(24_000)),
                    "day" => format!("Day is {}", t.div_euclid(24_000)),
                    _ => format!("Gametime is {t}"),
                }
            }
            ["toggledownfall"] => {
                let now = *commands::WEATHER.lock();
                let next = if now == commands::Weather::Clear { commands::Weather::Rain } else { commands::Weather::Clear };
                commands::set_weather(&self.server, next);
                "Toggled downfall".into()
            }
            ["daylock" | "alwaysday", rest @ ..] => {
                let lock = rest.first().is_none_or(|v| *v != "false");
                use bedrock::protocol::v844::types::Type;
                if let Some(mut r) = vanilla::game_rules().into_iter().find(|r| r.rule_name == "doDaylightCycle") {
                    r.rule_type = Type::Bool(!lock);
                    {
                        let mut rules = self.server.game_rules.lock();
                        rules.retain(|c| c.rule_name != r.rule_name);
                        rules.push(r.clone());
                    }
                    let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(game_rules_packet(vec![r])) });
                }
                if lock {
                    self.server.set_time(5000);
                    let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(time_packet(5000)) });
                }
                if lock { "Day-Night cycle locked".into() } else { "Day-Night cycle unlocked".into() }
            }
            ["summon", what, x, y, z, ..] => match (mobs::Kind::from_name(what), coords(&[x, y, z], here)) {
                (Some(kind), Some(p)) => {
                    self.server.mobs.spawn(&self.server, kind, p);
                    "Object successfully summoned".into()
                }
                (None, _) => format!("§cUnknown entity: {what}"),
                _ => "§cBad position".into(),
            },
            ["spawnpoint", x, y, z] => match coords(&[x, y, z], here) {
                Some(p) if self.server.get(self.id).is_some_and(|me| !self.server.plugins.spawn_point_set(&self.server, &me, p)) => "§cA plugin stopped that".into(),
                Some(p) => {
                    commands::set_spawn(Some(&self.name), (p.0, p.1 + 1.62, p.2));
                    format!("Set spawn point to {:.0} {:.0} {:.0}", p.0, p.1, p.2)
                }
                None => "§cBad position".into(),
            },
            _ => return None,
        })
    }

    async fn misc_command(&mut self, args: &[&str], here: (f32, f32, f32)) -> String {
        match args {
            ["clear", item, ..] => {
                let n = self.inv.clear_item(item);
                let _ = self.conn.send(&self.inv.all_packets()).await;
                format!("Cleared the inventory of {}, removing {n} items", self.name)
            }
            ["scoreboard", rest @ ..] => ui::command(&self.server, &self.name, rest),
            ["kill"] => {
                hurt(&mut self.conn, &self.server, self.id, &self.name, &mut self.hp, health::Damage::Kill, &mut self.inv, &self.effects, &self.mode).await;
                String::new()
            }
            ["clear"] => {
                let n = self.inv.clear();
                let _ = self.conn.send(&self.inv.all_packets()).await;
                format!("Cleared the inventory of {}, removing {n} items", self.name)
            }
            ["spawnpoint"] if self.server.get(self.id).is_some_and(|me| !self.server.plugins.spawn_point_set(&self.server, &me, me.pos)) => "§cA plugin stopped that".into(),
            ["spawnpoint"] => {
                let pos = self.server.get(self.id).map_or(self.server.world.spawn(), |p| p.pos);
                commands::set_spawn(Some(&self.name), pos);
                format!("Set {}'s spawn point to ({:.0}, {:.0}, {:.0})", self.name, pos.0, pos.1 - 1.62, pos.2)
            }
            ["setworldspawn"] => {
                let pos = self.server.get(self.id).map_or(self.server.world.spawn(), |p| p.pos);
                commands::set_spawn(None, pos);
                format!("Set the world spawn point to ({:.0}, {:.0}, {:.0})", pos.0, pos.1 - 1.62, pos.2)
            }
            ["tp" | "teleport", who] => match self.server.find_by_name(who).and_then(|t| self.server.get(t)) {
                Some(t) => {
                    let (rot, head) = self.server.get(self.id).map_or(((0.0, 0.0), 0.0), |p| (p.rot, p.head_yaw));
                    let mut tp = move_player(self.id, t.pos, rot, head);
                    if let Protocol::MovePlayerPacket(m) = &mut tp {
                        m.position_mode = PlayerPositionMode::Teleport;
                        m.teleport_data = Some(MovePlayerTeleportData { teleportation_cause: 0, source_actor_type: 0 });
                    }
                    let _ = self.conn.send(&[tp]).await;
                    self.server.moved(self.id, t.pos, rot, head);
                    format!("Teleported {} to {}", self.name, t.name)
                }
                None => format!("§cNo player was found: {who}"),
            },
            ["tp" | "teleport", x, y, z] => match coords(&[x, y, z], here) {
                Some((x, y, z)) => {
                    let eye = (x, y + 1.62, z);
                    let (rot, head) = self.server.get(self.id).map_or(((0.0, 0.0), 0.0), |p| (p.rot, p.head_yaw));
                    let mut tp = move_player(self.id, eye, rot, head);
                    if let Protocol::MovePlayerPacket(m) = &mut tp {
                        m.position_mode = PlayerPositionMode::Teleport;
                        m.teleport_data = Some(MovePlayerTeleportData { teleportation_cause: 0, source_actor_type: 0 });
                    }
                    let _ = self.conn.send(&[tp]).await;
                    self.server.moved(self.id, eye, rot, head);
                    format!("Teleported {} to {x:.2}, {y:.2}, {z:.2}", self.name)
                }
                None => "§cUse /tp <x> <y> <z>".into(),
            },
            ["time", "set", value] => {
                let t = match *value {
                    "day" => Some(1000),
                    "noon" => Some(6000),
                    "night" => Some(13000),
                    "midnight" => Some(18000),
                    n => n.parse().ok(),
                };
                match t {
                    Some(t) if !self.server.plugins.time_set(&self.server, t) => "§cA plugin stopped that".into(),
                    Some(t) => {
                        self.server.set_time(t);
                        let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(clock_packet(t)) });
                        let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(time_packet(t)) });
                        format!("Set the time to {t}")
                    }
                    None => "§cUse /time set day|noon|night|midnight|<ticks>".into(),
                }
            }
            ["gamerule", rule, rest @ ..] => {
                let known = vanilla::game_rules().into_iter().find(|r| r.rule_name.eq_ignore_ascii_case(rule));
                match (known, rest.first()) {
                    (None, _) => format!("§cUnknown game rule: {rule}"),
                    (Some(r), None) => {
                        let current = self.server.game_rules.lock().iter().find(|c| c.rule_name == r.rule_name).cloned().unwrap_or(r);
                        format!("{} = {:?}", current.rule_name, current.rule_type)
                    }
                    (Some(mut r), Some(value)) => {
                        use bedrock::protocol::v844::types::Type;
                        let parsed = match r.rule_type {
                            Type::Bool(_) => value.parse::<bool>().ok().map(Type::Bool),
                            Type::Int(_) => value.parse::<i32>().ok().map(Type::Int),
                            Type::Float(_) => value.parse::<f32>().ok().map(Type::Float),
                        };
                        match parsed {
                            Some(_) if !self.server.plugins.gamerule_change(&self.server, &r.rule_name, value) => "§cA plugin stopped that game rule change".to_string(),
                            Some(v) => {
                                r.rule_type = v;
                                {
                                    let mut rules = self.server.game_rules.lock();
                                    rules.retain(|c| c.rule_name != r.rule_name);
                                    rules.push(r.clone());
                                }
                                let msg = format!("Game rule {} has been updated to {value}", r.rule_name);
                                let _ = self.server.events.send(Event::Relay { from: 0, packet: Box::new(game_rules_packet(vec![r])) });
                                msg
                            }
                            None => format!("§cBad value for {}: {value}", r.rule_name),
                        }
                    }
                }
            }
            ["give", item, rest @ ..] => {
                let count = rest.first().and_then(|c| c.parse::<i8>().ok()).unwrap_or(1);
                match (items::item_by_name(item), self.inv.first_empty()) {
                    (Some(it), Some(slot)) => {
                        let count = count.clamp(1, items::max_stack(it.id));
                        let stack = self.inv.set_slot(slot, it, count).unwrap().to_network();
                        let _ = self.conn.send(&[Protocol::InventorySlotPacket(Box::new(InventorySlotPacket {
                            container_id: 0, slot: slot as u32, container_name_data: None, storage_item: None, item: stack,
                        }))]).await;
                        format!("Gave {} * {count} to {}", display_name(item), self.name)
                    }
                    (None, _) => format!("§cUnknown item: {item}"),
                    (_, None) => "§cYour inventory is full".into(),
                }
            }
            _ => format!("§cUnknown command: {}. Please check that the command exists and that you have permission to use it.", args.first().unwrap_or(&"")),
        }
    }
}

/// "minecraft:oak_planks" -> "Oak Planks", the way vanilla names items in command replies.
fn display_name(id: &str) -> String {
    id.trim_start_matches("minecraft:").split('_').map(|w| {
        let mut c = w.chars();
        c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
    }).collect::<Vec<_>>().join(" ")
}

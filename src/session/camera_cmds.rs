//! /camera, /aimassist, /controlscheme and /ride. They arrive with the player already turned
//! into `@s` in each target's session (commands::expand).

use super::*;
use bedrock::protocol::ProtoVersionPackets;

type CameraPacket = <Protocol as ProtoVersionPackets>::CameraInstructionPacket;
type AimAssistPacket = <Protocol as ProtoVersionPackets>::CameraAimAssistPacket;
type ControlSchemePacket = <Protocol as ProtoVersionPackets>::ClientBoundControlSchemeSetPacket;

/// Camera presets in the order CameraPresetsPacket.bin lists them (their runtime ids).
const PRESETS: &[&str] = &["first_person", "fixed_boom", "follow_orbit", "free", "third_person", "third_person_front"];

/// /camera ease types, in EasingType order.
const EASES: &[&str] = &[
    "linear", "spring", "in_quad", "out_quad", "in_out_quad", "in_cubic", "out_cubic", "in_out_cubic", "in_quart", "out_quart",
    "in_out_quart", "in_quint", "out_quint", "in_out_quint", "in_sine", "out_sine", "in_out_sine", "in_expo", "out_expo",
    "in_out_expo", "in_circ", "out_circ", "in_out_circ", "in_bounce", "out_bounce", "in_out_bounce", "in_back", "out_back",
    "in_out_back", "in_elastic", "out_elastic", "in_out_elastic",
];

fn ease(name: &str) -> Option<bedrock::protocol::v662::enums::EasingType> {
    use bedrock::protocol::v662::enums::EasingType as E;
    const ALL: [E; 32] = [
        E::Linear, E::Spring, E::InQuad, E::OutQuad, E::InOutQuad, E::InCubic, E::OutCubic, E::InOutCubic, E::InQuart, E::OutQuart,
        E::InOutQuart, E::InQuint, E::OutQuint, E::InOutQuint, E::InSine, E::OutSine, E::InOutSine, E::InExpo, E::OutExpo,
        E::InOutExpo, E::InCirc, E::OutCirc, E::InOutCirc, E::InBounce, E::OutBounce, E::InOutBounce, E::InBack, E::OutBack,
        E::InOutBack, E::InElastic, E::OutElastic, E::InOutElastic,
    ];
    EASES.iter().position(|e| *e == name).map(|i| ALL[i].clone())
}

fn empty_camera() -> CameraPacket {
    CameraPacket { set: None, clear: None, fade: None, target: None, remove_target: None, field_of_view: None, spline: None, attach: None, detach: None }
}

impl Session {
    /// None if `args` isn't one of these commands.
    pub(super) async fn camera_command(&mut self, args: &[&str], here: (f32, f32, f32)) -> Option<String> {
        use bedrock::protocol::v924::types::{Color, EaseData, FadeInstruction, FieldOfViewInstruction, SetInstruction, TimeData};
        Some(match args {
            ["camera", _, "clear"] => {
                let pkt = CameraPacket { clear: Some(true), ..empty_camera() };
                let _ = self.conn.send(&[Protocol::CameraInstructionPacket(Box::new(pkt))]).await;
                String::new()
            }
            // /camera <player> fade [time <in> <hold> <out>] [color <r> <g> <b>]
            ["camera", _, "fade", rest @ ..] => {
                let mut fade = FadeInstruction { time_data: None, color: None };
                let mut i = 0;
                while i < rest.len() {
                    let n = |k: usize| rest.get(i + k).and_then(|v| v.parse::<f32>().ok());
                    match rest[i] {
                        "time" => match (n(1), n(2), n(3)) {
                            (Some(a), Some(b), Some(c)) => {
                                fade.time_data = Some(TimeData { fade_in_time: a, wait_time: b, fade_out_time: c });
                                i += 4;
                            }
                            _ => return Some("§cUse /camera <player> fade time <in> <hold> <out>".into()),
                        },
                        "color" => match (n(1), n(2), n(3)) {
                            (Some(r), Some(g), Some(b)) => {
                                fade.color = Some(Color { r: r / 255.0, g: g / 255.0, b: b / 255.0 });
                                i += 4;
                            }
                            _ => return Some("§cUse /camera <player> fade color <red> <green> <blue> (0-255)".into()),
                        },
                        other => return Some(format!("§cUnknown fade option: {other}")),
                    }
                }
                let pkt = CameraPacket { fade: Some(fade), ..empty_camera() };
                let _ = self.conn.send(&[Protocol::CameraInstructionPacket(Box::new(pkt))]).await;
                String::new()
            }
            // /camera <player> set <preset> [ease <time> <type>] [pos <x y z>] [rot <x y>] [facing <x y z>] [default]
            ["camera", _, "set", preset, rest @ ..] => {
                let preset = preset.trim_start_matches("minecraft:");
                let Some(runtime_id) = PRESETS.iter().position(|p| *p == preset) else {
                    return Some(format!("§cUnknown camera preset: {preset}. Known: {}", PRESETS.join(", ")));
                };
                let mut set = SetInstruction {
                    runtime_id: runtime_id as i32,
                    ease_data: None,
                    position: None,
                    rotation: None,
                    facing: None,
                    view_offset: None,
                    entity_offset: None,
                    default_preset: None,
                    remove_ignore_starting_values_component: false,
                };
                let mut i = 0;
                while i < rest.len() {
                    match rest[i] {
                        "ease" => match (rest.get(i + 1).and_then(|t| t.parse::<f32>().ok()), rest.get(i + 2).and_then(|e| ease(e))) {
                            (Some(time), Some(ease_type)) => {
                                set.ease_data = Some(EaseData { ease_type, ease_time: time });
                                i += 3;
                            }
                            _ => return Some(format!("§cUse ease <seconds> <type>; types: {}", EASES.join(", "))),
                        },
                        "pos" if rest.len() >= i + 4 => match coords(&rest[i + 1..i + 4], here) {
                            Some(p) => {
                                set.position = Some(p);
                                i += 4;
                            }
                            None => return Some("§cBad camera position".into()),
                        },
                        "rot" => match (rest.get(i + 1).and_then(|v| v.parse::<f32>().ok()), rest.get(i + 2).and_then(|v| v.parse::<f32>().ok())) {
                            (Some(x), Some(y)) => {
                                set.rotation = Some((x, y));
                                i += 3;
                            }
                            _ => return Some("§cUse rot <x rotation> <y rotation>".into()),
                        },
                        "facing" if rest.len() >= i + 4 => match coords(&rest[i + 1..i + 4], here) {
                            Some(p) => {
                                set.facing = Some(p);
                                i += 4;
                            }
                            None => return Some("§cBad facing position".into()),
                        },
                        // facing <player>: look at where they are now
                        "facing" => match rest.get(i + 1).and_then(|n| self.server.find_by_name(n)).and_then(|id| self.server.get(id)) {
                            Some(p) => {
                                set.facing = Some(p.pos);
                                i += 2;
                            }
                            None => return Some("§cNo player to face".into()),
                        },
                        "default" => {
                            set.default_preset = Some(true);
                            i += 1;
                        }
                        other => return Some(format!("§cUnknown camera option: {other}")),
                    }
                }
                let pkt = CameraPacket { set: Some(set), ..empty_camera() };
                let _ = self.conn.send(&[Protocol::CameraInstructionPacket(Box::new(pkt))]).await;
                String::new()
            }
            // /camera <player> fov_set <fov> [ease time] [ease type], /camera <player> fov_clear
            ["camera", _, "fov_set", fov, rest @ ..] => match fov.parse::<f32>() {
                Ok(fov) if (30.0..=110.0).contains(&fov) => {
                    let ease_type = rest.get(1).and_then(|e| ease(e)).unwrap_or(bedrock::protocol::v662::enums::EasingType::Linear);
                    let fov = FieldOfViewInstruction { field_of_view: fov, ease_time: rest.first().and_then(|t| t.parse().ok()).unwrap_or(0.0), ease_type, clear: false };
                    let pkt = CameraPacket { field_of_view: Some(fov), ..empty_camera() };
                    let _ = self.conn.send(&[Protocol::CameraInstructionPacket(Box::new(pkt))]).await;
                    String::new()
                }
                _ => "§cUse /camera <player> fov_set <30-110> [ease time] [ease type]".into(),
            },
            ["camera", _, "fov_clear", ..] => {
                let fov = FieldOfViewInstruction { field_of_view: 0.0, ease_time: 0.0, ease_type: bedrock::protocol::v662::enums::EasingType::Linear, clear: true };
                let pkt = CameraPacket { field_of_view: Some(fov), ..empty_camera() };
                let _ = self.conn.send(&[Protocol::CameraInstructionPacket(Box::new(pkt))]).await;
                String::new()
            }
            ["camera", ..] => "§cUse /camera <player> set|fade|clear|fov_set|fov_clear".into(),
            // /aimassist <player> set [x angle] [y angle] [max distance] [angle|distance] [preset] | clear
            ["aimassist", _, "clear"] | ["aimassist", _, "set", ..] => {
                use bedrock::protocol::v729::enums::AimAssistAction;
                use bedrock::protocol::v827::packets::TargetMode;
                let rest = &args[3..];
                let f = |i: usize, d: f32| rest.get(i).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
                let clear = args[2] == "clear";
                let pkt = AimAssistPacket {
                    preset_id: rest.get(4).copied().unwrap_or("minecraft:aim_assist_default").to_string(),
                    view_angle: (f(0, 50.0).clamp(10.0, 90.0), f(1, 50.0).clamp(10.0, 90.0)),
                    distance: f(2, 8.0).clamp(1.0, 16.0),
                    target_mode: if rest.get(3) == Some(&"distance") { TargetMode::Distance } else { TargetMode::Angle },
                    action: if clear { AimAssistAction::Clear } else { AimAssistAction::Set },
                    show_debug_render: false,
                };
                let _ = self.conn.send(&[Protocol::CameraAimAssistPacket(Box::new(pkt))]).await;
                String::new()
            }
            ["aimassist", ..] => "§cUse /aimassist <player> set [x angle] [y angle] [max distance] [angle|distance] [preset] or clear".into(),
            // /controlscheme <player> set <scheme> | clear
            ["controlscheme", _, action, rest @ ..] => {
                use bedrock::protocol::v800::enums::ControlScheme as C;
                let scheme = match (*action, rest.first().copied()) {
                    ("clear", _) => C::PlayerRelative,
                    ("set", Some("locked_player_relative_strafe")) => C::LockedPlayerRelativeStrafe,
                    ("set", Some("camera_relative")) => C::CameraRelative,
                    ("set", Some("camera_relative_strafe")) => C::CameraRelativeStrafe,
                    ("set", Some("player_relative")) => C::PlayerRelative,
                    ("set", Some("player_relative_strafe")) => C::PlayerRelativeStrafe,
                    _ => return Some("§cUse /controlscheme <player> set locked_player_relative_strafe|camera_relative|camera_relative_strafe|player_relative|player_relative_strafe, or clear".into()),
                };
                let _ = self.conn.send(&[Protocol::ClientBoundControlSchemeSetPacket(Box::new(ControlSchemePacket { scheme }))]).await;
                String::new()
            }
            // /ride <player> start_riding <mob type or @e[type=...]>, stop_riding, summon_ride <type>
            ["ride", _, "start_riding", ride, ..] => {
                let kind_name = ride.split("type=").nth(1).map_or(*ride, |t| t.split([',', ']']).next().unwrap_or(""));
                let me = self.server.get(self.id).map_or(here, |p| p.pos);
                match mobs::Kind::from_name(kind_name) {
                    None => format!("§cUnknown entity: {kind_name}"),
                    Some(kind) => match self.server.mobs.nearest_of(kind, me, 32.0) {
                        Some(id) if self.server.mobs.force_mount(&self.server, id, self.id) => format!("{} is now riding a {kind_name}", self.name),
                        Some(_) => format!("§cThat {kind_name} already has a rider"),
                        None => format!("§cNo {kind_name} within 32 blocks"),
                    },
                }
            }
            ["ride", _, "summon_ride", kind_name, ..] => match mobs::Kind::from_name(kind_name) {
                Some(kind) => {
                    let id = self.server.mobs.spawn(&self.server, kind, here);
                    if self.server.mobs.force_mount(&self.server, id, self.id) {
                        format!("Summoned a {kind_name} for {} to ride", self.name)
                    } else {
                        format!("§cCould not ride the {kind_name}")
                    }
                }
                None => format!("§cUnknown entity: {kind_name}"),
            },
            ["ride", _, "stop_riding" | "evict_riders", ..] => {
                self.server.mobs.dismount(&self.server, self.id);
                format!("{} stopped riding", self.name)
            }
            ["ride", ..] => "§cUse /ride <player> start_riding <mob>, summon_ride <mob> or stop_riding".into(),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_ease_name_maps() {
        for e in super::EASES {
            assert!(super::ease(e).is_some(), "{e}");
        }
        assert!(super::ease("nope").is_none());
    }
}

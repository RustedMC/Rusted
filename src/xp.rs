//! Player XP (from PNX Player.addExperience / calculateRequireExperience), saved to
//! players/<name>.xp.

use bedrock::protocol::v662::types::ActorRuntimeID;
use bedrock::protocol::v729::packets::{AttributeData, UpdateAttributesPacket};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Xp {
    pub level: i32,
    /// points into the current level
    pub points: i32,
}

/// Bedrock/PNX formula.
pub fn needed(level: i32) -> i32 {
    if level >= 30 {
        9 * level - 158
    } else if level >= 15 {
        5 * level - 38
    } else {
        2 * level + 7
    }
}

impl Xp {
    pub fn add_points(&mut self, n: i32) {
        self.points += n;
        while self.points >= needed(self.level) {
            self.points -= needed(self.level);
            self.level += 1;
        }
        while self.points < 0 && self.level > 0 {
            self.level -= 1;
            self.points += needed(self.level);
        }
        self.points = self.points.max(0);
    }

    pub fn add_levels(&mut self, n: i32) {
        self.level = (self.level + n).clamp(0, 24_791);
        self.points = self.points.min(needed(self.level) - 1);
    }

    pub fn packet(&self, id: i64) -> crate::Protocol {
        let a = |name: &str, max: f32, value: f32| AttributeData {
            min_value: 0.0,
            max_value: max,
            current_value: value,
            default_min: 0.0,
            default_max: max,
            default_value: 0.0,
            attribute_name: name.into(),
            attribute_modifiers: vec![],
        };
        crate::Protocol::UpdateAttributesPacket(Box::new(UpdateAttributesPacket {
            target_runtime_id: ActorRuntimeID(id as u64),
            attribute_list: vec![
                a("minecraft:player.level", 24_791.0, self.level as f32),
                a("minecraft:player.experience", 1.0, self.points as f32 / needed(self.level) as f32),
            ],
            ticks_since_sim_started: 0,
        }))
    }

    pub fn to_text(self) -> String {
        format!("{} {}", self.level, self.points)
    }

    pub fn from_text(s: &str) -> Xp {
        let mut it = s.split_whitespace().map(|v| v.parse::<i32>().unwrap_or(0).max(0));
        Xp { level: it.next().unwrap_or(0).min(24_791), points: it.next().unwrap_or(0) }
    }
}

/// Mining XP for non-silk tools (PNX getDropExp). `roll` can be any random number.
pub fn from_block(name: &str, roll: u32) -> i32 {
    let r = |lo: u32, hi: u32| (lo + roll % (hi - lo + 1)) as i32;
    match name.trim_start_matches("minecraft:").trim_start_matches("deepslate_").trim_start_matches("lit_") {
        "coal_ore" => r(0, 2),
        "diamond_ore" | "emerald_ore" => r(3, 7),
        "lapis_ore" | "nether_quartz_ore" | "quartz_ore" => r(2, 5),
        "redstone_ore" => r(1, 5),
        "nether_gold_ore" => r(0, 1),
        "sculk" => 1,
        "sculk_catalyst" | "sculk_sensor" | "sculk_shrieker" => 5,
        "spawner" | "mob_spawner" => r(15, 43),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_up_and_down() {
        let mut x = Xp::default();
        x.add_points(7);
        assert_eq!((x.level, x.points), (1, 0));
        x.add_points(9 + 11 + 3);
        assert_eq!((x.level, x.points), (3, 3));
        x.add_levels(-5);
        assert_eq!(x.level, 0);
        assert_eq!(Xp::from_text(&Xp { level: 30, points: 4 }.to_text()), Xp { level: 30, points: 4 });
        assert_eq!(needed(30), 112);
    }
}

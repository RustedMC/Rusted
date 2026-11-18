//! Hunger, saturation and exhaustion with vanilla rules, plus food values.

use bedrock::protocol::V2225;
use bedrock::protocol::v662::types::ActorRuntimeID;
use bedrock::protocol::v729::packets::{AttributeData, UpdateAttributesPacket};

pub const MAX_FOOD: f32 = 20.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Hunger {
    pub food: f32,
    pub saturation: f32,
    pub exhaustion: f32,
    timer: u32,
}

impl Default for Hunger {
    fn default() -> Self {
        Self { food: MAX_FOOD, saturation: 5.0, exhaustion: 0.0, timer: 0 }
    }
}

#[derive(Debug, PartialEq)]
pub enum Step {
    Nothing,
    Heal,
    Starve,
}

impl Hunger {
    /// Vanilla exhaustion: walking 0.01/m, sprinting 0.1/m, jump 0.05, sprint-jump 0.2.
    /// True if food or saturation changed.
    pub fn exhaust(&mut self, amount: f32) -> bool {
        self.exhaustion += amount;
        let mut changed = false;
        while self.exhaustion >= 4.0 {
            self.exhaustion -= 4.0;
            if self.saturation > 0.0 {
                self.saturation = (self.saturation - 1.0).max(0.0);
            } else {
                self.food = (self.food - 1.0).max(0.0);
            }
            changed = true;
        }
        changed
    }

    /// Called once a second. `regen_rule` is the naturalRegeneration gamerule.
    pub fn tick(&mut self, difficulty: u32, health: f32, regen_rule: bool) -> Step {
        if difficulty == 0 {
            // peaceful refills food and heals
            self.food = (self.food + 1.0).min(MAX_FOOD);
        }
        self.timer += 1;
        if self.timer < 4 {
            return Step::Nothing;
        }
        self.timer = 0;
        if self.food >= 18.0 && health < 20.0 && regen_rule {
            self.exhaust(6.0);
            return Step::Heal;
        }
        if self.food <= 0.0 {
            // starving stops at 10 health on easy, 1 on normal, and kills on hard
            let floor = match difficulty {
                0 => return Step::Nothing,
                1 => 10.0,
                2 => 1.0,
                _ => 0.0,
            };
            if health > floor {
                return Step::Starve;
            }
        }
        Step::Nothing
    }

    pub fn eat(&mut self, food: f32, saturation: f32) {
        self.food = (self.food + food).min(MAX_FOOD);
        self.saturation = (self.saturation + saturation).min(self.food);
    }

    pub fn packet(&self, id: i64) -> V2225 {
        let a = |name: &str, max: f32, value: f32, default: f32| AttributeData {
            min_value: 0.0,
            max_value: max,
            current_value: value,
            default_min: 0.0,
            default_max: max,
            default_value: default,
            attribute_name: name.into(),
            attribute_modifiers: vec![],
        };
        V2225::UpdateAttributesPacket(Box::new(UpdateAttributesPacket {
            target_runtime_id: ActorRuntimeID(id as u64),
            attribute_list: vec![
                a("minecraft:player.hunger", 20.0, self.food, 20.0),
                a("minecraft:player.saturation", 20.0, self.saturation, 5.0),
                a("minecraft:player.exhaustion", 5.0, self.exhaustion, 0.0),
            ],
            ticks_since_sim_started: 0,
        }))
    }
}

/// (food, saturation), vanilla values.
pub fn food(name: &str) -> Option<(f32, f32)> {
    let n = name.strip_prefix("minecraft:").unwrap_or(name);
    Some(match n {
        "apple" => (4.0, 2.4),
        "golden_apple" | "enchanted_golden_apple" => (4.0, 9.6),
        "golden_carrot" => (6.0, 14.4),
        "bread" => (5.0, 6.0),
        "baked_potato" => (5.0, 6.0),
        "potato" => (1.0, 0.6),
        "poisonous_potato" => (2.0, 1.2),
        "carrot" => (3.0, 3.6),
        "beetroot" => (1.0, 1.2),
        "beetroot_soup" | "mushroom_stew" | "rabbit_stew" | "suspicious_stew" => (6.0, 7.2),
        "melon_slice" => (2.0, 1.2),
        "sweet_berries" | "glow_berries" => (2.0, 0.4),
        "cookie" => (2.0, 0.4),
        "pumpkin_pie" => (8.0, 4.8),
        "dried_kelp" => (1.0, 0.6),
        "beef" | "porkchop" | "mutton" | "rabbit" | "chicken" | "cod" | "salmon" => (2.0, 1.2),
        "cooked_beef" | "cooked_porkchop" => (8.0, 12.8),
        "cooked_mutton" | "cooked_salmon" => (6.0, 9.6),
        "cooked_chicken" => (6.0, 7.2),
        "cooked_rabbit" | "cooked_cod" => (5.0, 6.0),
        "tropical_fish" | "pufferfish" => (1.0, 0.2),
        "rotten_flesh" => (4.0, 0.8),
        "spider_eye" => (2.0, 3.2),
        "honey_bottle" => (6.0, 1.2),
        "chorus_fruit" => (4.0, 2.4),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhaustion_regen_and_starving() {
        let mut h = Hunger::default();
        assert!(h.exhaust(4.0), "saturation goes first");
        assert_eq!((h.food, h.saturation), (20.0, 4.0));
        h.saturation = 0.0;
        h.exhaust(8.0);
        assert_eq!(h.food, 18.0);
        let steps: Vec<Step> = (0..4).map(|_| h.tick(2, 10.0, true)).collect();
        assert_eq!(steps.last(), Some(&Step::Heal), "food >= 18 heals every 4s");
        h.food = 0.0;
        let last = (0..4).map(|_| h.tick(2, 5.0, true)).last();
        assert_eq!(last, Some(Step::Starve));
        let last = (0..4).map(|_| h.tick(2, 1.0, true)).last();
        assert_eq!(last, Some(Step::Nothing), "normal stops at 1 health");
        h.eat(8.0, 12.8);
        assert_eq!((h.food, h.saturation), (8.0, 8.0), "saturation capped by food");
        assert_eq!(food("minecraft:cooked_beef"), Some((8.0, 12.8)));
        assert_eq!(food("minecraft:stone"), None);
    }
}

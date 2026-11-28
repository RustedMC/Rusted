//! Mobs: animals, hostiles, villagers, pets. A* pathing, breeding, drops.
//! 10 ticks a second, and every player sees every mob (fine at a few hundred).

use std::collections::{HashMap, HashSet};
use parking_lot::Mutex;

use bedrock::protocol::v662::packets::RemoveActorPacket;
use bedrock::protocol::v662::types::{ActorRuntimeID, ActorUniqueID, DataItem, PropertySyncData};
use bedrock::protocol::v924::enums::ActorDataIDs;
use bedrock::protocol::v2168::enums::DataItemType;

use crate::Protocol;
use crate::overworld::feature::blocks;
use crate::players::{Event, Server};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    /// A plugin custom mob: index into custom::get().entities.
    Custom(u16),
    Cow,
    Pig,
    Sheep,
    Chicken,
    Zombie,
    Skeleton,
    Creeper,
    Spider,
    Enderman,
    Villager,
    IronGolem,
    Wolf,
    Cat,
    Squid,
    Cod,
    Salmon,
    Drowned,
    Rabbit,
    Horse,
    Donkey,
    Mule,
    Llama,
    Goat,
    Fox,
    Panda,
    PolarBear,
    Mooshroom,
    Ocelot,
    Turtle,
    Armadillo,
    Camel,
    Sniffer,
    Frog,
    Axolotl,
    GlowSquid,
    TropicalFish,
    Pufferfish,
    Dolphin,
    Bat,
    Parrot,
    Bee,
    Allay,
    Husk,
    Stray,
    Bogged,
    ZombieVillager,
    CaveSpider,
    Silverfish,
    Witch,
    Slime,
    Phantom,
    Pillager,
    Vindicator,
    Evoker,
    Ravager,
    Vex,
    Guardian,
    ElderGuardian,
    Breeze,
    Warden,
    WitherSkeleton,
    Blaze,
    Ghast,
    MagmaCube,
    Piglin,
    ZombiePigman,
    Hoglin,
    Strider,
    EnderDragon,
    EndCrystal,
    Shulker,
    Boat,
    Minecart,
    ChestMinecart,
    WanderingTrader,
    TraderLlama,
    Zoglin,
    PiglinBrute,
    SkeletonHorse,
    ZombieHorse,
    Endermite,
    SnowGolem,
    Wither,
    Tadpole,
    Creaking,
    HappyGhast,
    TntMinecart,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Passive,
    Hostile,
    /// Peaceful until hit, then fights back (llamas, goats, bears, pandas, piglins).
    Neutral,
    Aquatic,
    AquaticHostile,
    Companion,
    Villager,
    Flying,
    FlyingHostile,
}

/// Per-kind stats (health, size, speed, damage, foods, drops, traits).
pub struct Spec {
    pub id: &'static str,
    pub display: &'static str,
    pub health: f32,
    pub size: (f32, f32),
    /// Blocks per 10 Hz tick.
    pub speed: f32,
    pub melee: f32,
    pub class: Class,
    pub food: &'static [&'static str],
    pub drops: &'static [(&'static str, i8, i8)],
    pub undead: bool,
    pub arthropod: bool,
    /// Catches fire in sunlight.
    pub burns: bool,
    /// Slimes and magma cubes split when they die.
    pub splits: bool,
    /// Ranged attack: the projectile it fires.
    pub ranged: Option<&'static str>,
    /// Effect its hits give: (effect id, seconds).
    pub hit_effect: Option<(i32, i32)>,
}

impl Kind {
    /// Horses, donkeys and mules: tamed by riding them until they stop bucking (PNX EntityHorse).
    pub fn horse_like(self) -> bool {
        matches!(self, Kind::Horse | Kind::Donkey | Kind::Mule)
    }

    /// Minecarts of any kind: they move on rails, not by walking.
    pub fn cart(self) -> bool {
        matches!(self, Kind::Minecart | Kind::ChestMinecart | Kind::TntMinecart)
    }

    /// Mobs a player can sit on. Pigs, striders and camels need a saddle first.
    pub fn rideable(self) -> bool {
        self.horse_like() || matches!(self, Kind::Pig | Kind::Strider | Kind::Camel | Kind::Boat | Kind::Minecart | Kind::SkeletonHorse | Kind::ZombieHorse | Kind::HappyGhast)
    }

    /// Where the rider's position (their eye) sits above the mob's feet: the seat from the
    /// mob's ride component plus a seated player's eye height (PNX getSeatOffsetFor).
    fn seat_offset(self) -> f32 {
        const PLAYER: f32 = 1.8 * 0.9 * 0.753_086_4;
        PLAYER
            + match self {
                Kind::Horse | Kind::SkeletonHorse | Kind::ZombieHorse => 1.1,
                Kind::HappyGhast => 3.6,
                Kind::Donkey => 0.925,
                Kind::Mule => 0.975,
                Kind::Pig => 0.63,
                Kind::Strider => 1.7 - 0.13,
                Kind::Camel => 1.905,
                Kind::Boat => -0.2,
                _ => 0.0,
            }
    }

    /// Temper an untamed horse gains from a snack (vanilla horse feeding values).
    fn temper_food(self, held: &str) -> u32 {
        if !self.horse_like() {
            return 0;
        }
        match held {
            "minecraft:sugar" | "minecraft:wheat" | "minecraft:apple" => 3,
            "minecraft:golden_carrot" => 5,
            "minecraft:golden_apple" | "minecraft:enchanted_golden_apple" => 10,
            _ => 0,
        }
    }

    pub const PASSIVE: [Kind; 4] = [Kind::Cow, Kind::Pig, Kind::Sheep, Kind::Chicken];
    pub const ALL: [Kind; 87] = [Kind::Cow, Kind::Pig, Kind::Sheep, Kind::Chicken, Kind::Zombie, Kind::Skeleton, Kind::Creeper, Kind::Spider, Kind::Enderman, Kind::Villager, Kind::IronGolem, Kind::Wolf, Kind::Cat, Kind::Squid, Kind::Cod, Kind::Salmon, Kind::Drowned, Kind::Rabbit, Kind::Horse, Kind::Donkey, Kind::Mule, Kind::Llama, Kind::Goat, Kind::Fox, Kind::Panda, Kind::PolarBear, Kind::Mooshroom, Kind::Ocelot, Kind::Turtle, Kind::Armadillo, Kind::Camel, Kind::Sniffer, Kind::Frog, Kind::Axolotl, Kind::GlowSquid, Kind::TropicalFish, Kind::Pufferfish, Kind::Dolphin, Kind::Bat, Kind::Parrot, Kind::Bee, Kind::Allay, Kind::Husk, Kind::Stray, Kind::Bogged, Kind::ZombieVillager, Kind::CaveSpider, Kind::Silverfish, Kind::Witch, Kind::Slime, Kind::Phantom, Kind::Pillager, Kind::Vindicator, Kind::Evoker, Kind::Ravager, Kind::Vex, Kind::Guardian, Kind::ElderGuardian, Kind::Breeze, Kind::Warden, Kind::WitherSkeleton, Kind::Blaze, Kind::Ghast, Kind::MagmaCube, Kind::Piglin, Kind::ZombiePigman, Kind::Hoglin, Kind::Strider, Kind::EnderDragon, Kind::EndCrystal, Kind::Shulker, Kind::Boat, Kind::Minecart, Kind::ChestMinecart, Kind::WanderingTrader, Kind::TraderLlama, Kind::Zoglin, Kind::PiglinBrute, Kind::SkeletonHorse, Kind::ZombieHorse, Kind::Endermite, Kind::SnowGolem, Kind::Wither, Kind::Tadpole, Kind::Creaking, Kind::HappyGhast, Kind::TntMinecart];

    pub fn spec(self) -> &'static Spec {
        match self {
            Kind::Custom(i) => &crate::custom::get().entities[i as usize],
            Kind::Cow => &Spec { id: "minecraft:cow", display: "Cow", health: 10.0, size: (0.9, 1.4), speed: 0.25, melee: 0.0, class: Class::Passive, food: &["minecraft:wheat"], drops: &[("minecraft:beef", 1, 3), ("minecraft:leather", 0, 2)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Pig => &Spec { id: "minecraft:pig", display: "Pig", health: 10.0, size: (0.9, 0.9), speed: 0.25, melee: 0.0, class: Class::Passive, food: &["minecraft:carrot", "minecraft:potato", "minecraft:beetroot"], drops: &[("minecraft:porkchop", 1, 3)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Sheep => &Spec { id: "minecraft:sheep", display: "Sheep", health: 8.0, size: (0.9, 1.3), speed: 0.25, melee: 0.0, class: Class::Passive, food: &["minecraft:wheat"], drops: &[("minecraft:mutton", 1, 2), ("minecraft:white_wool", 1, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Chicken => &Spec { id: "minecraft:chicken", display: "Chicken", health: 4.0, size: (0.4, 0.7), speed: 0.25, melee: 0.0, class: Class::Passive, food: &["minecraft:wheat_seeds", "minecraft:pumpkin_seeds", "minecraft:melon_seeds", "minecraft:beetroot_seeds", "minecraft:torchflower_seeds", "minecraft:pitcher_pod"], drops: &[("minecraft:chicken", 1, 1), ("minecraft:feather", 0, 2)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Zombie => &Spec { id: "minecraft:zombie", display: "Zombie", health: 20.0, size: (0.6, 1.9), speed: 0.23, melee: 3.0, class: Class::Hostile, food: &[], drops: &[("minecraft:rotten_flesh", 0, 2)], undead: true, arthropod: false, burns: true, splits: false, ranged: None, hit_effect: None },
            Kind::Skeleton => &Spec { id: "minecraft:skeleton", display: "Skeleton", health: 20.0, size: (0.6, 1.99), speed: 0.35, melee: 0.0, class: Class::Hostile, food: &[], drops: &[("minecraft:bone", 0, 2), ("minecraft:arrow", 0, 2)], undead: true, arthropod: false, burns: true, splits: false, ranged: Some("minecraft:arrow"), hit_effect: None },
            Kind::Creeper => &Spec { id: "minecraft:creeper", display: "Creeper", health: 20.0, size: (0.6, 1.7), speed: 0.2, melee: 0.0, class: Class::Hostile, food: &[], drops: &[("minecraft:gunpowder", 0, 2)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Spider => &Spec { id: "minecraft:spider", display: "Spider", health: 16.0, size: (1.4, 0.9), speed: 0.3, melee: 2.0, class: Class::Hostile, food: &[], drops: &[("minecraft:string", 0, 2), ("minecraft:spider_eye", 0, 1)], undead: false, arthropod: true, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Enderman => &Spec { id: "minecraft:enderman", display: "Enderman", health: 40.0, size: (0.6, 2.9), speed: 0.3, melee: 7.0, class: Class::Hostile, food: &[], drops: &[("minecraft:ender_pearl", 0, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Villager => &Spec { id: "minecraft:villager_v2", display: "Villager", health: 20.0, size: (0.6, 1.9), speed: 0.15, melee: 0.0, class: Class::Villager, food: &[], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::IronGolem => &Spec { id: "minecraft:iron_golem", display: "Iron Golem", health: 100.0, size: (1.4, 2.9), speed: 0.25, melee: 10.0, class: Class::Companion, food: &[], drops: &[("minecraft:iron_ingot", 3, 5), ("minecraft:poppy", 0, 2)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Wolf => &Spec { id: "minecraft:wolf", display: "Wolf", health: 10.0, size: (0.6, 0.85), speed: 0.3, melee: 4.0, class: Class::Companion, food: &[], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Cat => &Spec { id: "minecraft:cat", display: "Cat", health: 10.0, size: (0.6, 0.7), speed: 0.3, melee: 3.0, class: Class::Companion, food: &[], drops: &[("minecraft:string", 0, 2)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Squid => &Spec { id: "minecraft:squid", display: "Squid", health: 10.0, size: (0.95, 0.95), speed: 0.15, melee: 0.0, class: Class::Aquatic, food: &[], drops: &[("minecraft:ink_sac", 1, 3)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Cod => &Spec { id: "minecraft:cod", display: "Cod", health: 3.0, size: (0.5, 0.3), speed: 0.15, melee: 0.0, class: Class::Aquatic, food: &[], drops: &[("minecraft:cod", 1, 1), ("minecraft:bone_meal", 0, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Salmon => &Spec { id: "minecraft:salmon", display: "Salmon", health: 3.0, size: (0.5, 0.3), speed: 0.15, melee: 0.0, class: Class::Aquatic, food: &[], drops: &[("minecraft:salmon", 1, 1), ("minecraft:bone_meal", 0, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Drowned => &Spec { id: "minecraft:drowned", display: "Drowned", health: 20.0, size: (0.6, 1.9), speed: 0.23, melee: 3.0, class: Class::Hostile, food: &[], drops: &[("minecraft:rotten_flesh", 0, 2), ("minecraft:copper_ingot", 0, 1)], undead: true, arthropod: false, burns: true, splits: false, ranged: None, hit_effect: None },
            Kind::Rabbit => &Spec { id: "minecraft:rabbit", display: "Rabbit", health: 3.0, size: (0.4, 0.5), speed: 0.3, melee: 0.0, class: Class::Passive, food: &["minecraft:carrot", "minecraft:golden_carrot", "minecraft:dandelion"], drops: &[("minecraft:rabbit", 0, 1), ("minecraft:rabbit_hide", 0, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Horse => &Spec { id: "minecraft:horse", display: "Horse", health: 22.0, size: (1.4, 1.6), speed: 0.22, melee: 0.0, class: Class::Passive, food: &["minecraft:golden_apple", "minecraft:golden_carrot"], drops: &[("minecraft:leather", 0, 2)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Donkey => &Spec { id: "minecraft:donkey", display: "Donkey", health: 22.0, size: (1.4, 1.5), speed: 0.175, melee: 0.0, class: Class::Passive, food: &["minecraft:golden_apple", "minecraft:golden_carrot"], drops: &[("minecraft:leather", 0, 2)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Mule => &Spec { id: "minecraft:mule", display: "Mule", health: 22.0, size: (1.4, 1.6), speed: 0.175, melee: 0.0, class: Class::Passive, food: &[], drops: &[("minecraft:leather", 0, 2)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Llama => &Spec { id: "minecraft:llama", display: "Llama", health: 22.0, size: (0.9, 1.87), speed: 0.25, melee: 1.0, class: Class::Neutral, food: &["minecraft:hay_block"], drops: &[("minecraft:leather", 0, 2)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Goat => &Spec { id: "minecraft:goat", display: "Goat", health: 10.0, size: (0.9, 1.3), speed: 0.4, melee: 2.0, class: Class::Neutral, food: &["minecraft:wheat"], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Fox => &Spec { id: "minecraft:fox", display: "Fox", health: 10.0, size: (0.6, 0.7), speed: 0.3, melee: 2.0, class: Class::Passive, food: &["minecraft:sweet_berries", "minecraft:glow_berries"], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Panda => &Spec { id: "minecraft:panda", display: "Panda", health: 20.0, size: (1.3, 1.25), speed: 0.1, melee: 6.0, class: Class::Neutral, food: &["minecraft:bamboo"], drops: &[("minecraft:bamboo", 0, 2)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::PolarBear => &Spec { id: "minecraft:polar_bear", display: "Polar Bear", health: 30.0, size: (1.3, 1.4), speed: 0.25, melee: 6.0, class: Class::Neutral, food: &[], drops: &[("minecraft:cod", 0, 2), ("minecraft:salmon", 0, 2)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Mooshroom => &Spec { id: "minecraft:mooshroom", display: "Mooshroom", health: 10.0, size: (0.9, 1.4), speed: 0.25, melee: 0.0, class: Class::Passive, food: &["minecraft:wheat"], drops: &[("minecraft:beef", 1, 3), ("minecraft:leather", 0, 2)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Ocelot => &Spec { id: "minecraft:ocelot", display: "Ocelot", health: 10.0, size: (0.6, 0.7), speed: 0.3, melee: 0.0, class: Class::Passive, food: &["minecraft:cod", "minecraft:salmon"], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Turtle => &Spec { id: "minecraft:turtle", display: "Turtle", health: 30.0, size: (1.2, 0.4), speed: 0.1, melee: 0.0, class: Class::Passive, food: &["minecraft:seagrass"], drops: &[("minecraft:seagrass", 0, 2)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Armadillo => &Spec { id: "minecraft:armadillo", display: "Armadillo", health: 12.0, size: (0.7, 0.65), speed: 0.14, melee: 0.0, class: Class::Passive, food: &["minecraft:spider_eye"], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Camel => &Spec { id: "minecraft:camel", display: "Camel", health: 32.0, size: (1.7, 2.375), speed: 0.15, melee: 0.0, class: Class::Passive, food: &["minecraft:cactus"], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Sniffer => &Spec { id: "minecraft:sniffer", display: "Sniffer", health: 14.0, size: (1.9, 1.75), speed: 0.09, melee: 0.0, class: Class::Passive, food: &["minecraft:torchflower_seeds"], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Frog => &Spec { id: "minecraft:frog", display: "Frog", health: 10.0, size: (0.5, 0.55), speed: 0.1, melee: 0.0, class: Class::Passive, food: &["minecraft:slime_ball"], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Axolotl => &Spec { id: "minecraft:axolotl", display: "Axolotl", health: 14.0, size: (0.75, 0.42), speed: 0.15, melee: 2.0, class: Class::Aquatic, food: &["minecraft:tropical_fish_bucket"], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::GlowSquid => &Spec { id: "minecraft:glow_squid", display: "Glow Squid", health: 10.0, size: (0.95, 0.95), speed: 0.15, melee: 0.0, class: Class::Aquatic, food: &[], drops: &[("minecraft:glow_ink_sac", 1, 3)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::TropicalFish => &Spec { id: "minecraft:tropicalfish", display: "Tropical Fish", health: 3.0, size: (0.4, 0.4), speed: 0.15, melee: 0.0, class: Class::Aquatic, food: &[], drops: &[("minecraft:tropical_fish", 1, 1), ("minecraft:bone_meal", 0, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Pufferfish => &Spec { id: "minecraft:pufferfish", display: "Pufferfish", health: 3.0, size: (0.7, 0.7), speed: 0.1, melee: 0.0, class: Class::Aquatic, food: &[], drops: &[("minecraft:pufferfish", 1, 1), ("minecraft:bone_meal", 0, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: Some((19, 7)) },
            Kind::Dolphin => &Spec { id: "minecraft:dolphin", display: "Dolphin", health: 10.0, size: (0.9, 0.6), speed: 0.3, melee: 3.0, class: Class::Aquatic, food: &["minecraft:cod", "minecraft:salmon"], drops: &[("minecraft:cod", 0, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Bat => &Spec { id: "minecraft:bat", display: "Bat", health: 6.0, size: (0.5, 0.9), speed: 0.15, melee: 0.0, class: Class::Flying, food: &[], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Parrot => &Spec { id: "minecraft:parrot", display: "Parrot", health: 6.0, size: (0.5, 0.9), speed: 0.15, melee: 0.0, class: Class::Flying, food: &["minecraft:wheat_seeds", "minecraft:melon_seeds", "minecraft:pumpkin_seeds", "minecraft:beetroot_seeds"], drops: &[("minecraft:feather", 1, 2)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Bee => &Spec { id: "minecraft:bee", display: "Bee", health: 10.0, size: (0.55, 0.5), speed: 0.15, melee: 2.0, class: Class::Flying, food: &["minecraft:dandelion", "minecraft:poppy", "minecraft:cornflower", "minecraft:allium", "minecraft:oxeye_daisy", "minecraft:azure_bluet"], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: Some((19, 10)) },
            Kind::Allay => &Spec { id: "minecraft:allay", display: "Allay", health: 20.0, size: (0.35, 0.6), speed: 0.15, melee: 0.0, class: Class::Flying, food: &[], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Husk => &Spec { id: "minecraft:husk", display: "Husk", health: 20.0, size: (0.6, 1.9), speed: 0.23, melee: 3.0, class: Class::Hostile, food: &[], drops: &[("minecraft:rotten_flesh", 0, 2)], undead: true, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: Some((17, 7)) },
            Kind::Stray => &Spec { id: "minecraft:stray", display: "Stray", health: 20.0, size: (0.6, 1.99), speed: 0.25, melee: 0.0, class: Class::Hostile, food: &[], drops: &[("minecraft:bone", 0, 2), ("minecraft:arrow", 0, 2)], undead: true, arthropod: false, burns: true, splits: false, ranged: Some("minecraft:arrow"), hit_effect: None },
            Kind::Bogged => &Spec { id: "minecraft:bogged", display: "Bogged", health: 16.0, size: (0.6, 1.99), speed: 0.25, melee: 0.0, class: Class::Hostile, food: &[], drops: &[("minecraft:bone", 0, 2), ("minecraft:arrow", 0, 2)], undead: true, arthropod: false, burns: true, splits: false, ranged: Some("minecraft:arrow"), hit_effect: None },
            Kind::ZombieVillager => &Spec { id: "minecraft:zombie_villager_v2", display: "Zombie Villager", health: 20.0, size: (0.6, 1.9), speed: 0.23, melee: 3.0, class: Class::Hostile, food: &[], drops: &[("minecraft:rotten_flesh", 0, 2)], undead: true, arthropod: false, burns: true, splits: false, ranged: None, hit_effect: None },
            Kind::CaveSpider => &Spec { id: "minecraft:cave_spider", display: "Cave Spider", health: 12.0, size: (0.7, 0.5), speed: 0.3, melee: 2.0, class: Class::Hostile, food: &[], drops: &[("minecraft:string", 0, 2), ("minecraft:spider_eye", 0, 1)], undead: false, arthropod: true, burns: false, splits: false, ranged: None, hit_effect: Some((19, 7)) },
            Kind::Silverfish => &Spec { id: "minecraft:silverfish", display: "Silverfish", health: 8.0, size: (0.4, 0.3), speed: 0.25, melee: 1.0, class: Class::Hostile, food: &[], drops: &[], undead: false, arthropod: true, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Witch => &Spec { id: "minecraft:witch", display: "Witch", health: 26.0, size: (0.6, 1.95), speed: 0.25, melee: 0.0, class: Class::Hostile, food: &[], drops: &[("minecraft:glass_bottle", 0, 6), ("minecraft:redstone", 0, 6), ("minecraft:glowstone_dust", 0, 6), ("minecraft:sugar", 0, 6), ("minecraft:stick", 0, 6)], undead: false, arthropod: false, burns: false, splits: false, ranged: Some("minecraft:splash_potion"), hit_effect: None },
            Kind::Slime => &Spec { id: "minecraft:slime", display: "Slime", health: 1.0, size: (1.04, 1.04), speed: 0.15, melee: 1.0, class: Class::Hostile, food: &[], drops: &[("minecraft:slime_ball", 0, 2)], undead: false, arthropod: false, burns: false, splits: true, ranged: None, hit_effect: None },
            Kind::Phantom => &Spec { id: "minecraft:phantom", display: "Phantom", health: 20.0, size: (0.9, 0.5), speed: 0.35, melee: 2.0, class: Class::FlyingHostile, food: &[], drops: &[("minecraft:phantom_membrane", 0, 1)], undead: true, arthropod: false, burns: true, splits: false, ranged: None, hit_effect: None },
            Kind::Pillager => &Spec { id: "minecraft:pillager", display: "Pillager", health: 24.0, size: (0.6, 1.9), speed: 0.35, melee: 0.0, class: Class::Hostile, food: &[], drops: &[("minecraft:arrow", 0, 2)], undead: false, arthropod: false, burns: false, splits: false, ranged: Some("minecraft:arrow"), hit_effect: None },
            Kind::Vindicator => &Spec { id: "minecraft:vindicator", display: "Vindicator", health: 24.0, size: (0.6, 1.9), speed: 0.35, melee: 13.0, class: Class::Hostile, food: &[], drops: &[("minecraft:emerald", 0, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Evoker => &Spec { id: "minecraft:evocation_illager", display: "Evoker", health: 24.0, size: (0.6, 1.9), speed: 0.5, melee: 6.0, class: Class::Hostile, food: &[], drops: &[("minecraft:totem_of_undying", 1, 1), ("minecraft:emerald", 0, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Ravager => &Spec { id: "minecraft:ravager", display: "Ravager", health: 100.0, size: (1.95, 2.2), speed: 0.4, melee: 12.0, class: Class::Hostile, food: &[], drops: &[("minecraft:saddle", 1, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Vex => &Spec { id: "minecraft:vex", display: "Vex", health: 14.0, size: (0.4, 0.8), speed: 0.35, melee: 9.0, class: Class::FlyingHostile, food: &[], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Guardian => &Spec { id: "minecraft:guardian", display: "Guardian", health: 30.0, size: (0.85, 0.85), speed: 0.15, melee: 6.0, class: Class::AquaticHostile, food: &[], drops: &[("minecraft:prismarine_shard", 0, 2), ("minecraft:cod", 0, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::ElderGuardian => &Spec { id: "minecraft:elder_guardian", display: "Elder Guardian", health: 80.0, size: (2.0, 2.0), speed: 0.1, melee: 8.0, class: Class::AquaticHostile, food: &[], drops: &[("minecraft:prismarine_shard", 0, 2), ("minecraft:wet_sponge", 1, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: Some((4, 300)) },
            Kind::Breeze => &Spec { id: "minecraft:breeze", display: "Breeze", health: 30.0, size: (0.6, 1.77), speed: 0.25, melee: 0.0, class: Class::Hostile, food: &[], drops: &[("minecraft:breeze_rod", 1, 2)], undead: false, arthropod: false, burns: false, splits: false, ranged: Some("minecraft:wind_charge_projectile"), hit_effect: None },
            Kind::Warden => &Spec { id: "minecraft:warden", display: "Warden", health: 500.0, size: (0.9, 2.9), speed: 0.3, melee: 30.0, class: Class::Hostile, food: &[], drops: &[("minecraft:sculk_catalyst", 1, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: Some((33, 12)) },
            Kind::WitherSkeleton => &Spec { id: "minecraft:wither_skeleton", display: "Wither Skeleton", health: 20.0, size: (0.7, 2.4), speed: 0.25, melee: 8.0, class: Class::Hostile, food: &[], drops: &[("minecraft:coal", 0, 1), ("minecraft:bone", 0, 2)], undead: true, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: Some((20, 10)) },
            Kind::Blaze => &Spec { id: "minecraft:blaze", display: "Blaze", health: 20.0, size: (0.6, 1.8), speed: 0.2, melee: 6.0, class: Class::FlyingHostile, food: &[], drops: &[("minecraft:blaze_rod", 0, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: Some("minecraft:small_fireball"), hit_effect: None },
            Kind::Ghast => &Spec { id: "minecraft:ghast", display: "Ghast", health: 10.0, size: (4.0, 4.0), speed: 0.1, melee: 0.0, class: Class::FlyingHostile, food: &[], drops: &[("minecraft:ghast_tear", 0, 1), ("minecraft:gunpowder", 0, 2)], undead: false, arthropod: false, burns: false, splits: false, ranged: Some("minecraft:fireball"), hit_effect: None },
            Kind::MagmaCube => &Spec { id: "minecraft:magma_cube", display: "Magma Cube", health: 1.0, size: (1.04, 1.04), speed: 0.15, melee: 3.0, class: Class::Hostile, food: &[], drops: &[("minecraft:magma_cream", 0, 1)], undead: false, arthropod: false, burns: false, splits: true, ranged: None, hit_effect: None },
            Kind::Piglin => &Spec { id: "minecraft:piglin", display: "Piglin", health: 16.0, size: (0.6, 1.9), speed: 0.35, melee: 5.0, class: Class::Neutral, food: &["minecraft:gold_ingot"], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::ZombiePigman => &Spec { id: "minecraft:zombie_pigman", display: "Zombified Piglin", health: 20.0, size: (0.6, 1.9), speed: 0.23, melee: 5.0, class: Class::Neutral, food: &[], drops: &[("minecraft:rotten_flesh", 0, 1), ("minecraft:gold_nugget", 0, 1)], undead: true, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Hoglin => &Spec { id: "minecraft:hoglin", display: "Hoglin", health: 40.0, size: (1.4, 1.4), speed: 0.36, melee: 6.0, class: Class::Hostile, food: &["minecraft:crimson_fungus"], drops: &[("minecraft:porkchop", 2, 4), ("minecraft:leather", 0, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::EnderDragon => &Spec { id: "minecraft:ender_dragon", display: "Ender Dragon", health: 200.0, size: (13.0, 4.0), speed: 0.6, melee: 10.0, class: Class::FlyingHostile, food: &[], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::EndCrystal => &Spec { id: "minecraft:ender_crystal", display: "End Crystal", health: 1.0, size: (2.0, 2.0), speed: 0.0, melee: 0.0, class: Class::Passive, food: &[], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Shulker => &Spec { id: "minecraft:shulker", display: "Shulker", health: 30.0, size: (1.0, 1.0), speed: 0.0, melee: 0.0, class: Class::Hostile, food: &[], drops: &[("minecraft:shulker_shell", 0, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: Some("minecraft:shulker_bullet"), hit_effect: None },
            Kind::Strider => &Spec { id: "minecraft:strider", display: "Strider", health: 20.0, size: (0.9, 1.7), speed: 0.1, melee: 0.0, class: Class::Passive, food: &["minecraft:warped_fungus"], drops: &[("minecraft:string", 0, 5)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Boat => &Spec { id: "minecraft:boat", display: "Boat", health: 4.0, size: (1.4, 0.455), speed: 0.0, melee: 0.0, class: Class::Passive, food: &[], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::ChestMinecart => &Spec { id: "minecraft:chest_minecart", display: "Minecart with Chest", health: 3.0, size: (0.98, 0.7), speed: 0.0, melee: 0.0, class: Class::Passive, food: &[], drops: &[("minecraft:minecart", 1, 1), ("minecraft:chest", 1, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            // Vanilla Bedrock stats; AI from the closest existing class (from PNX's entity list).
            Kind::WanderingTrader => &Spec { id: "minecraft:wandering_trader", display: "Wandering Trader", health: 20.0, size: (0.6, 1.9), speed: 0.25, melee: 0.0, class: Class::Passive, food: &[], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::TraderLlama => &Spec { id: "minecraft:trader_llama", display: "Trader Llama", health: 22.0, size: (0.9, 1.87), speed: 0.25, melee: 1.0, class: Class::Neutral, food: &["minecraft:wheat", "minecraft:hay_block"], drops: &[("minecraft:leather", 0, 2)], undead: false, arthropod: false, burns: false, splits: false, ranged: Some("minecraft:llama_spit"), hit_effect: None },
            Kind::Zoglin => &Spec { id: "minecraft:zoglin", display: "Zoglin", health: 40.0, size: (1.4, 1.4), speed: 0.3, melee: 8.0, class: Class::Hostile, food: &[], drops: &[("minecraft:rotten_flesh", 1, 3)], undead: true, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::PiglinBrute => &Spec { id: "minecraft:piglin_brute", display: "Piglin Brute", health: 50.0, size: (0.6, 1.9), speed: 0.35, melee: 13.0, class: Class::Hostile, food: &[], drops: &[("minecraft:golden_axe", 0, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::SkeletonHorse => &Spec { id: "minecraft:skeleton_horse", display: "Skeleton Horse", health: 15.0, size: (1.4, 1.6), speed: 0.3, melee: 0.0, class: Class::Passive, food: &[], drops: &[("minecraft:bone", 0, 2)], undead: true, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::ZombieHorse => &Spec { id: "minecraft:zombie_horse", display: "Zombie Horse", health: 15.0, size: (1.4, 1.6), speed: 0.25, melee: 0.0, class: Class::Passive, food: &[], drops: &[("minecraft:rotten_flesh", 0, 2)], undead: true, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Endermite => &Spec { id: "minecraft:endermite", display: "Endermite", health: 8.0, size: (0.4, 0.3), speed: 0.3, melee: 2.0, class: Class::Hostile, food: &[], drops: &[], undead: false, arthropod: true, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::SnowGolem => &Spec { id: "minecraft:snow_golem", display: "Snow Golem", health: 4.0, size: (0.7, 1.9), speed: 0.2, melee: 0.0, class: Class::Passive, food: &[], drops: &[("minecraft:snowball", 0, 15)], undead: false, arthropod: false, burns: false, splits: false, ranged: Some("minecraft:snowball"), hit_effect: None },
            Kind::Wither => &Spec { id: "minecraft:wither", display: "Wither", health: 600.0, size: (1.0, 3.0), speed: 0.3, melee: 8.0, class: Class::FlyingHostile, food: &[], drops: &[("minecraft:nether_star", 1, 1)], undead: true, arthropod: false, burns: false, splits: false, ranged: Some("minecraft:wither_skull"), hit_effect: Some((20, 10)) },
            Kind::Tadpole => &Spec { id: "minecraft:tadpole", display: "Tadpole", health: 6.0, size: (0.4, 0.3), speed: 0.2, melee: 0.0, class: Class::Aquatic, food: &["minecraft:slime_ball"], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Creaking => &Spec { id: "minecraft:creaking", display: "Creaking", health: 1.0, size: (0.9, 2.7), speed: 0.3, melee: 3.0, class: Class::Hostile, food: &[], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::TntMinecart => &Spec { id: "minecraft:tnt_minecart", display: "Minecart with TNT", health: 3.0, size: (0.98, 0.7), speed: 0.0, melee: 0.0, class: Class::Passive, food: &[], drops: &[("minecraft:tnt_minecart", 1, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::HappyGhast => &Spec { id: "minecraft:happy_ghast", display: "Happy Ghast", health: 20.0, size: (4.0, 4.0), speed: 0.15, melee: 0.0, class: Class::Flying, food: &["minecraft:snowball"], drops: &[], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
            Kind::Minecart => &Spec { id: "minecraft:minecart", display: "Minecart", health: 3.0, size: (0.98, 0.7), speed: 0.0, melee: 0.0, class: Class::Passive, food: &[], drops: &[("minecraft:minecart", 1, 1)], undead: false, arthropod: false, burns: false, splits: false, ranged: None, hit_effect: None },
        }
    }

    /// Lives in water: swims, suffocates on land.
    pub fn aquatic(self) -> bool {
        matches!(self.spec().class, Class::Aquatic | Class::AquaticHostile)
    }

    /// Fights for players or its owner (golems, wolves, cats).
    fn companion(self) -> bool {
        self.spec().class == Class::Companion
    }

    pub fn flies(self) -> bool {
        matches!(self.spec().class, Class::Flying | Class::FlyingHostile)
    }

    pub fn neutral(self) -> bool {
        self.spec().class == Class::Neutral
    }

    pub fn identifier(self) -> &'static str {
        self.spec().id
    }

    pub fn hostile(self) -> bool {
        matches!(self.spec().class, Class::Hostile | Class::AquaticHostile | Class::FlyingHostile)
    }

    /// Name in death messages.
    fn display(self) -> &'static str {
        if matches!(self.spec().class, Class::Passive | Class::Aquatic | Class::Flying) { "an animal" } else { self.spec().display }
    }

    pub fn arthropod(self) -> bool {
        self.spec().arthropod
    }

    pub fn undead(self) -> bool {
        self.spec().undead
    }

    /// Melee damage on normal difficulty.
    fn melee(self) -> f32 {
        self.spec().melee
    }

    pub fn from_name(n: &str) -> Option<Kind> {
        let n = n.trim_start_matches("minecraft:");
        let n = match n {
            "villager" => "villager_v2",
            "zombie_villager" => "zombie_villager_v2",
            "evoker" => "evocation_illager",
            "zombified_piglin" => "zombie_pigman",
            "tropical_fish" => "tropicalfish",
            n => n,
        };
        Kind::ALL.into_iter().find(|k| k.identifier().trim_start_matches("minecraft:") == n)
            .or_else(|| crate::custom::get().entities.iter().position(|e| e.id == n || e.id.rsplit(':').next() == Some(n)).map(|i| Kind::Custom(i as u16)))
    }

    fn max_health(self) -> f32 {
        self.spec().health
    }

    /// (width, height) of the collision box.
    fn size(self) -> (f32, f32) {
        self.spec().size
    }

    /// Walking speed in blocks per tick (10 ticks/s).
    fn speed(self) -> f32 {
        self.spec().speed
    }

    /// Food that tempts and breeds it.
    fn food(self, item: &str) -> bool {
        self.spec().food.contains(&item)
    }

    /// (item, min, max) dropped on death.
    fn drops(self) -> &'static [(&'static str, i8, i8)] {
        self.spec().drops
    }
}

#[derive(Clone)]
pub struct Mob {
    pub id: i64,
    pub kind: Kind,
    pub pos: (f32, f32, f32),
    vel: (f32, f32, f32),
    yaw: f32,
    pub health: f32,
    on_ground: bool,
    target: Option<(f32, f32)>,
    /// Ticks left running away after being hit.
    panic: u32,
    /// Ticks until removal after dying (0 = alive).
    dying: u32,
    sent: ((f32, f32, f32), f32, f32),
    head_yaw: f32,
    /// Waypoints (block positions to stand on) toward the current goal.
    path: Vec<(i32, i32, i32)>,
    /// Ticks left in love mode (after being fed).
    love: u32,
    /// Ticks until it may breed again.
    cooldown: u32,
    /// Ticks until a baby grows up (0 = adult).
    baby: u32,
    /// Player it's after (hostiles; neutral ones only once provoked).
    aggro: Option<i64>,
    attack_cd: u32,
    /// Creeper fuse ticks (0 = not lit).
    fuse: u32,
    burning: bool,
    /// Mob it's attacking (golems, wolves, zombies after villagers).
    prey: Option<i64>,
    /// Tamed by this player (wolves, cats).
    owner: Option<i64>,
    sitting: bool,
    /// Sheep wool colour (0 white ... 15 black, Bedrock colour index).
    color: u8,
    sheared: bool,
    /// Ticks until a sheared sheep's wool grows back.
    regrow: u32,
    /// When it was last hurt (invulnerability window).
    hurt_at: Option<std::time::Instant>,
    /// Ticks left on fire (fire aspect).
    fire: u32,
    /// A villager's job and trades.
    pub job: Option<Box<crate::villagers::Job>>,
    /// Zombie villagers: ticks of weakness left, and ticks until cured (0 = not curing).
    weak: u32,
    curing: u32,
    /// Leads a pillager patrol (killing it gives Bad Omen).
    pub captain: bool,
    /// Player riding it.
    pub rider: Option<i64>,
    /// Second seat (boats, camels): rides along, doesn't steer.
    passenger: Option<i64>,
    /// When the driver's client last sent this vehicle's position (it steers boats and horses).
    steered: Option<std::time::Instant>,
    saddled: bool,
    /// A player fed or bred it (or it was born from breeding): saved across restarts.
    kept: bool,
    /// A creeper lit with flint and steel: blows up whatever happens.
    lit: bool,
    /// A creeper struck by lightning: bigger blast, blue glow (PNX EntityCreeper.setPowered).
    charged: bool,
    /// Ender dragon: ticks left sitting on the exit portal (PNX DragonPhase landing/sitting).
    perch: u32,
    /// Mob ticks until it leaves on its own (wandering traders and their llamas), 0 = never.
    lifetime: u32,
    /// On a lead: who holds it (a player, or the llama ahead in a caravan).
    leash: Option<i64>,
    /// Ticks into drawing a bow / charging a crossbow (0 = not drawing), and who at.
    draw: u32,
    aim: i64,
    /// Horses: chance out of 100 to accept a rider, goes up with feeding and each buck.
    temper: u32,
    /// Ticks until an untamed horse decides whether to keep its rider.
    buck: u32,
    /// Minecarts: the rider's yaw while they hold forward.
    push: Option<f32>,
    /// Chest minecarts: where their items are kept in the container store (see `cart_store`).
    home: Option<(i32, i32, i32)>,
    /// Memory and current behaviour (passive mobs so far; see `ai`).
    brain: crate::ai::Brain,
}

/// Weapon enchantment effects on a hit.
#[derive(Clone, Copy, Default)]
pub struct Hit {
    pub knockback: i16,
    /// 10 Hz ticks on fire.
    pub fire: u32,
    pub looting: i16,
}

/// What a player threw.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Throw {
    Snowball,
    Egg,
    Pearl,
    /// Splash or lingering potion with this potion id.
    Potion(u32),
    /// Bottle o' enchanting.
    XpBottle,
}

impl Throw {
    pub fn from_item(name: &str) -> Option<Throw> {
        match name {
            "minecraft:snowball" => Some(Throw::Snowball),
            "minecraft:egg" | "minecraft:blue_egg" | "minecraft:brown_egg" => Some(Throw::Egg),
            "minecraft:ender_pearl" => Some(Throw::Pearl),
            "minecraft:experience_bottle" => Some(Throw::XpBottle),
            _ => None,
        }
    }
    fn identifier(self) -> &'static str {
        match self {
            Throw::Snowball => "minecraft:snowball",
            Throw::Egg => "minecraft:egg",
            Throw::Pearl => "minecraft:ender_pearl",
            Throw::Potion(_) => "minecraft:splash_potion",
            Throw::XpBottle => "minecraft:xp_bottle",
        }
    }
}

/// Thrown snowball/egg/pearl (PNX EntityProjectile: gravity 0.03, drag 0.01 per game tick).
struct Thrown {
    id: i64,
    pos: (f32, f32, f32),
    /// Blocks per game tick.
    vel: (f32, f32, f32),
    kind: Throw,
    owner: i64,
    life: u32,
}

static THROWN: Mutex<Vec<Thrown>> = Mutex::new(Vec::new());

/// Throws `kind` from a player's eyes along their look (PNX: speed 1.5 per game tick).
pub fn throw(server: &Server, kind: Throw, owner: i64, eye: (f32, f32, f32), rot: (f32, f32)) {
    let (pitch, yaw) = (rot.0.to_radians(), rot.1.to_radians());
    let dir = (-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos());
    let vel = (dir.0 * 1.5, dir.1 * 1.5, dir.2 * 1.5);
    let pos = (eye.0, eye.1 - 0.1, eye.2);
    let id = crate::players::next_id();
    let add = Protocol::AddActorPacket(Box::new(bedrock::protocol::v2225::packets::AddActorPacket {
        target_actor_id: ActorUniqueID(id),
        target_runtime_id: ActorRuntimeID(id as u64),
        actor_type: kind.identifier().into(),
        position: pos,
        velocity: vel,
        rotation: (0.0, 0.0),
        y_head_rotation: 0.0,
        y_body_rotation: 0.0,
        attributes: vec![],
        actor_data: vec![],
        synced_properties: PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
        actor_links: vec![],
        passenger_of_block_data: None,
    }));
    let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(add) });
    let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(sound("throw", pos)) });
    THROWN.lock().push(Thrown { id, pos, vel, kind, owner, life: 30 * TPS });
}

struct Arrow {
    id: i64,
    pos: (f32, f32, f32),
    vel: (f32, f32, f32),
    shooter: Kind,
    life: u32,
    stuck: bool,
    /// A player's shot: who, and the bow/trident numbers.
    by: Option<PlayerShot>,
}

#[derive(Clone)]
struct PlayerShot {
    owner: i64,
    /// Damage on hit (fixed for tridents; arrows scale with speed).
    damage: Option<f32>,
    power: i16,
    punch: i16,
    flame: bool,
    /// Fully drawn bow: a critical arrow (bonus damage).
    critical: bool,
    /// Item it turns back into where it lands (arrow, the trident itself).
    drop: Option<bedrock::protocol::v2168::types::NetworkItemStackDescriptorV2>,
    /// The bow/crossbow/trident's enchantments (piercing, loyalty, channeling, impaling).
    weapon: Vec<u8>,
    /// Mobs a piercing arrow already went through.
    pierced: Vec<i64>,
    /// A Loyalty trident flying back to its thrower.
    returning: bool,
}

/// Mob AI ticks per second.
const TPS: u32 = 10;
/// Babies grow up in 20 minutes; parents can breed again after 5.
const GROW: u32 = 20 * 60 * TPS;
const BREED_COOLDOWN: u32 = 5 * 60 * TPS;
const LOVE: u32 = 30 * TPS;

#[derive(Default)]
pub struct Mobs {
    mobs: Mutex<HashMap<i64, Mob>>,
    /// What each online player holds (for tempting), by entity id.
    held: Mutex<HashMap<i64, String>>,
    /// Online players hostiles may target (survival/adventure).
    targetable: Mutex<HashSet<i64>>,
    arrows: Mutex<Vec<Arrow>>,
    /// Chunks that already had their chance of animals.
    rolled: Mutex<HashSet<(i32, i32)>>,
    rng: Mutex<u64>,
}

/// Which animal herd a chunk gets, by biome (vanilla spawn lists, trimmed to the main ones).
fn animal_for(biome: Option<u16>, ground: i32, roll: u32) -> Option<Kind> {
    use crate::overworld::biome::id as b;
    let g = blocks::short(ground);
    let pick = |list: &[Kind]| Some(list[(roll % list.len() as u32) as usize]);
    match biome? {
        b::MUSHROOM_ISLAND if g == "mycelium" => pick(&[Kind::Mooshroom]),
        b::DESERT if g == "sand" => pick(&[Kind::Rabbit, Kind::Camel]),
        b::BEACH if g == "sand" => pick(&[Kind::Turtle]),
        _ if g != "grass_block" && g != "snow" && g != "snow_layer" && g != "podzol" => None,
        b::PLAINS | b::SUNFLOWER_PLAINS => pick(&[Kind::Cow, Kind::Pig, Kind::Sheep, Kind::Chicken, Kind::Horse, Kind::Donkey]),
        b::SAVANNA | b::SAVANNA_PLATEAU | b::SAVANNA_MUTATED => pick(&[Kind::Cow, Kind::Sheep, Kind::Horse, Kind::Llama, Kind::Armadillo]),
        b::FOREST | b::BIRCH_FOREST | b::BIRCH_FOREST_MUTATED | b::FLOWER_FOREST | b::ROOFED_FOREST => pick(&[Kind::Cow, Kind::Pig, Kind::Sheep, Kind::Chicken, Kind::Wolf, Kind::Rabbit]),
        b::TAIGA | b::MEGA_TAIGA | b::REDWOOD_TAIGA_MUTATED => pick(&[Kind::Wolf, Kind::Fox, Kind::Rabbit, Kind::Sheep, Kind::Pig]),
        b::COLD_TAIGA | b::ICE_PLAINS | b::ICE_PLAINS_SPIKES | b::GROVE | b::SNOWY_SLOPES => pick(&[Kind::Rabbit, Kind::Fox, Kind::PolarBear, Kind::Goat, Kind::Wolf]),
        b::JUNGLE | b::JUNGLE_EDGE | b::BAMBOO_JUNGLE => pick(&[Kind::Parrot, Kind::Ocelot, Kind::Panda, Kind::Chicken, Kind::Pig]),
        b::EXTREME_HILLS | b::EXTREME_HILLS_MUTATED | b::EXTREME_HILLS_PLUS_TREES | b::JAGGED_PEAKS | b::FROZEN_PEAKS | b::STONY_PEAKS => pick(&[Kind::Goat, Kind::Llama, Kind::Sheep]),
        b::MEADOW | b::CHERRY_GROVE => pick(&[Kind::Donkey, Kind::Rabbit, Kind::Sheep, Kind::Bee]),
        b::SWAMPLAND | b::MANGROVE_SWAMP => pick(&[Kind::Frog, Kind::Cow, Kind::Pig, Kind::Chicken]),
        _ => pick(&Kind::PASSIVE),
    }
}

/// Most mobs alive at once (passive cap; vanilla's is per player area).
const CAP: usize = 300;
/// Mob gravity per 10 Hz tick (vanilla 0.08 per 20 Hz tick, twice).
const GRAVITY: f32 = 0.16;
/// Upward speed of a jump at 10 Hz with GRAVITY: 0.55 + 0.39 + 0.23 + 0.07 = 1.24 blocks.
const JUMP: f32 = 0.55;

fn broadcast(server: &Server, p: Protocol) {
    let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(p) });
}

/// (pitch, yaw) in degrees for something flying with `vel` (PNX EntityProjectile.updateRotation).
fn projectile_rotation(vel: (f32, f32, f32)) -> (f32, f32) {
    let f = (vel.0 * vel.0 + vel.2 * vel.2).sqrt();
    (vel.1.atan2(f).to_degrees(), vel.0.atan2(vel.2).to_degrees())
}

fn angle(deg: f32) -> i8 {
    (deg.rem_euclid(360.0) / 360.0 * 256.0) as i32 as u8 as i8
}

/// Entity data for a mob in its current state.
fn data_of(m: &Mob) -> Vec<DataItem<Protocol>> {
    use ActorDataIDs::*;
    use DataItemType as T;
    let mut d = data_with(m.kind, m.baby > 0, m.burning, m.fuse > 0);
    if m.charged
        && let Some(DataItem { data_item_type: T::Int64(f), .. }) = d.first_mut()
    {
        *f |= 1 << 9; // POWERED
    }
    // SADDLED 8, SITTING 24, TAMED 28, SHEARED 31, WASD_CONTROLLED 44, CAN_POWER_JUMP 45
    let bits = (m.saddled as i64) << 8
        | (m.sitting as i64) << 24
        | (m.owner.is_some() as i64) << 28
        | (m.sheared as i64) << 31
        | (m.rider.is_some() as i64) << 44
        | (m.kind.horse_like() as i64) << 45;
    if let Some(DataItem { data_item_type: T::Int64(f), .. }) = d.first_mut() {
        *f |= bits;
    }
    // LEASHED (30) and who holds the lead, so the client draws the rope.
    if let Some(h) = m.leash
        && let Some(DataItem { data_item_type: T::Int64(f), .. }) = d.first_mut()
    {
        *f |= 1 << 30;
        d.push(DataItem { data_item_id: LeashHolder, data_item_type: T::Int64(h) });
    } else {
        d.push(DataItem { data_item_id: LeashHolder, data_item_type: T::Int64(-1) });
    }
    if m.draw > 0 {
        // PNX Bow/CrossBowShootExecutor: TARGET = who it aims at; bows set
        // FACING_TARGET_TO_RANGE_ATTACK (87), crossbows USING_ITEM (4), then CHARGED (27)
        // once loaded, with CHARGE_AMOUNT growing.
        let crossbow = m.kind == Kind::Pillager;
        if crossbow {
            let charged = m.draw >= CROSSBOW_CHARGED;
            if let Some(DataItem { data_item_type: T::Int64(f), .. }) = d.first_mut() {
                *f |= 1 << 4 | (charged as i64) << 27;
            }
            d.push(DataItem { data_item_id: ChargeAmount, data_item_type: T::Byte((m.draw * 4).min(127) as i8) });
        } else {
            d.push(DataItem { data_item_id: Reserved092, data_item_type: T::Int64(crate::spawn::FLAGS_EXTENDED | 1 << (87 - 64)) });
        }
        d.push(DataItem { data_item_id: Target, data_item_type: T::Int64(m.aim) });
    }
    if m.kind == Kind::Boat {
        d.push(DataItem { data_item_id: Variant, data_item_type: T::Int(m.color as i32) });
    }
    if m.kind == Kind::Sheep {
        d.push(DataItem { data_item_id: ColorIndex, data_item_type: T::Byte(m.color as i8) });
    }
    if m.kind.spec().splits {
        // Slime size shows as scale; its box grows with it.
        let s = m.color.max(1) as f32;
        for it in d.iter_mut() {
            match it.data_item_id {
                Reserved038 => it.data_item_type = T::Float(s),
                Reserved053 | Reserved054 => it.data_item_type = T::Float(0.52 * s),
                _ => {}
            }
        }
    }
    if let Some(o) = m.owner {
        d.push(DataItem { data_item_id: Owner, data_item_type: T::Int64(o) });
    }
    if let Some(j) = &m.job {
        d.push(DataItem { data_item_id: Variant, data_item_type: T::Int(j.profession) });
        d.push(DataItem { data_item_id: MarkVariant, data_item_type: T::Int(j.clothing as i32) });
        d.push(DataItem { data_item_id: TradeTier, data_item_type: T::Int(j.tier as i32 - 1) });
        d.push(DataItem { data_item_id: MaxTradeTier, data_item_type: T::Int(4) });
        d.push(DataItem { data_item_id: TradeExperience, data_item_type: T::Int(j.exp) });
    }
    d
}

/// Natural sheep colours (vanilla odds): mostly white, some greys, black, brown, rare pink.
fn sheep_color(r: u32) -> u8 {
    match r % 10_000 {
        0..8_184 => 0,
        8_184..8_684 => 8,
        8_684..9_184 => 7,
        9_184..9_684 => 15,
        9_684..9_984 => 12,
        _ => 6,
    }
}

/// Boat wood types in Bedrock's variant order.
pub const BOAT_WOOD: [&str; 10] = ["oak", "spruce", "birch", "jungle", "acacia", "dark_oak", "mangrove", "bamboo", "cherry", "pale_oak"];

/// The item a boat of this wood drops as (bamboo ones are rafts).
pub fn boat_item(wood: u8) -> String {
    match BOAT_WOOD.get(wood as usize).copied().unwrap_or("oak") {
        "bamboo" => "minecraft:bamboo_raft".into(),
        w => format!("minecraft:{w}_boat"),
    }
}

/// Which boat a held item places, if any: (wood, chest boat).
pub fn boat_wood(item: &str) -> Option<(u8, bool)> {
    let n = item.strip_prefix("minecraft:")?;
    let chest = n.ends_with("_chest_boat") || n == "bamboo_chest_raft";
    let wood = match n {
        "bamboo_raft" | "bamboo_chest_raft" => "bamboo",
        n => n.strip_suffix("_chest_boat").or_else(|| n.strip_suffix("_boat"))?,
    };
    BOAT_WOOD.iter().position(|w| *w == wood).map(|i| (i as u8, chest))
}

/// A chest boat's item.
pub fn chest_boat_item(wood: u8) -> String {
    match BOAT_WOOD.get(wood as usize).copied().unwrap_or("oak") {
        "bamboo" => "minecraft:bamboo_chest_raft".into(),
        w => format!("minecraft:{w}_chest_boat"),
    }
}

const WOOL: [&str; 16] = [
    "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black",
];

fn data_with(kind: Kind, baby: bool, burning: bool, ignited: bool) -> Vec<DataItem<Protocol>> {
    use ActorDataIDs::*;
    use DataItemType as T;
    let (w, h) = kind.size();
    let s = if baby { 0.5 } else { 1.0 };
    let item = |id, v| DataItem { data_item_id: id, data_item_type: v };
    vec![
        // Same flag words as players (gravity, collision, breathing), plus BABY (bit 11).
        item(Reserved0, T::Int64(crate::spawn::FLAGS | if baby { 1 << 11 } else { 0 } | burning as i64 | (ignited as i64) << 10)),
        item(Reserved092, T::Int64(0)),
        item(Reserved054, T::Float(h * s)),
        item(Reserved053, T::Float(w * s)),
        item(Reserved038, T::Float(s)),
        item(AirSupply, T::Short(300)),
        item(AirSupplyMax, T::Short(300)),
        item(LeashHolder, T::Int64(-1)),
    ]
}

fn add_packet(m: &Mob) -> Protocol {
    use bedrock::protocol::v662::packets::AttributeEntry;
    Protocol::AddActorPacket(Box::new(bedrock::protocol::v2225::packets::AddActorPacket {
        target_actor_id: ActorUniqueID(m.id),
        target_runtime_id: ActorRuntimeID(m.id as u64),
        // A boat with a chest store is a chest boat.
        actor_type: if m.kind == Kind::Boat && m.home.is_some() { "minecraft:chest_boat".into() } else { m.kind.identifier().into() },
        position: m.pos,
        velocity: (0.0, 0.0, 0.0),
        rotation: (0.0, m.yaw),
        y_head_rotation: m.yaw,
        y_body_rotation: m.yaw,
        attributes: vec![AttributeEntry { attribute_name: "minecraft:health".into(), min_value: 0.0, current_value: m.health, max_value: m.kind.max_health() }],
        actor_data: data_of(m),
        synced_properties: PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
        actor_links: vec![],
        passenger_of_block_data: None,
    }))
}

fn move_packet(m: &Mob) -> Protocol {
    use bedrock::protocol::v662::packets::MoveActorAbsolutePacket;
    use bedrock::protocol::v662::types::MoveActorAbsoluteData;
    let (a, head) = (angle(m.yaw), angle(m.head_yaw));
    Protocol::MoveActorAbsolutePacket(Box::new(MoveActorAbsolutePacket {
        move_data: MoveActorAbsoluteData {
            actor_runtime_id: ActorRuntimeID(m.id as u64),
            header: m.on_ground as i8,
            position: m.pos,
            rotation_x: 0,
            rotation_y: a,
            rotation_y_head: head,
        },
    }))
}

fn data_packet(m: &Mob) -> Protocol {
    Protocol::SetActorDataPacket(Box::new(bedrock::protocol::v662::packets::SetActorDataPacket {
        target_runtime_id: ActorRuntimeID(m.id as u64),
        actor_data: data_of(m),
        synced_properties: PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
        tick: 0,
    }))
}

/// Detector rails with a cart on them: position -> ticks until they switch off.
static DETECTORS: Mutex<Option<HashMap<(i32, i32, i32), u32>>> = Mutex::new(None);

/// A cart is on this detector rail: power it, and keep it on while carts pass.
fn detector_on(server: &Server, rail: crate::minecart::Rail) {
    let p = rail.at;
    DETECTORS.lock().get_or_insert_with(HashMap::new).insert(p, 2);
    if !rail.on {
        let b = server.world.get(p.0, p.1, p.2);
        server.set_block_quiet(0, p.0, p.1, p.2, crate::blocks::with(b, &[("rail_data_bit", crate::blocks::State::Bool(true))]));
    }
}

/// What a mob holds in its hand, so skeletons are drawn with a bow (vanilla spawn equipment).
fn held_packet(m: &Mob) -> Option<Protocol> {
    let item = match m.kind {
        Kind::Skeleton | Kind::Stray | Kind::Bogged => "minecraft:bow",
        Kind::Pillager => "minecraft:crossbow",
        Kind::Vindicator => "minecraft:iron_axe",
        Kind::WitherSkeleton => "minecraft:stone_sword",
        Kind::Piglin => "minecraft:golden_sword",
        _ => return None,
    };
    Some(Protocol::MobEquipmentPacket(Box::new(bedrock::protocol::v975::packets::MobEquipmentPacket {
        target_runtime_id: ActorRuntimeID(m.id as u64),
        item: crate::items::item_by_name(item)?,
        slot: 0,
        selected_slot: 0,
        container_id: bedrock::protocol::v662::enums::ContainerID(0),
    })))
}

/// Puts `rider` on `vehicle`, or takes them off.
fn link_packet(vehicle: i64, rider: i64, on: bool) -> Protocol {
    link_packet_as(vehicle, rider, on, true)
}

/// `driver` false links a passenger (second seat).
fn link_packet_as(vehicle: i64, rider: i64, on: bool, driver: bool) -> Protocol {
    use bedrock::protocol::v662::enums::ActorLinkType;
    use bedrock::protocol::v662::packets::SetActorLinkPacket;
    use bedrock::protocol::v712::types::ActorLink;
    Protocol::SetActorLinkPacket(Box::new(SetActorLinkPacket {
        link: ActorLink {
            actor_unique_id_a: ActorUniqueID(vehicle),
            actor_unique_id_b: ActorUniqueID(rider),
            link_type: if !on { ActorLinkType::None } else if driver { ActorLinkType::Riding } else { ActorLinkType::Passenger },
            immediate: !on,
            passenger_initiated: true,
            vehicle_angular_velocity: 0.0,
        },
    }))
}

/// The player's RIDING flag (bit 2), so they sit instead of stand, and their seat: where the
/// client draws them on the mob, and for boats how far they can turn (PNX applySeatOffsets).
fn rider_flag(player: i64, on: bool, kind: Kind) -> Protocol {
    seat_flag(player, on, kind, false)
}

/// Same for either seat: the passenger sits behind (boat -0.6 along it, camel z -0.5; the
/// camel's driver sits forward at +0.5), PNX EntityBoat/EntityCamel seats.
fn seat_flag(player: i64, on: bool, kind: Kind, passenger: bool) -> Protocol {
    let item = |id, t| DataItem { data_item_id: id, data_item_type: t };
    let boat = on && kind == Kind::Boat;
    let (sx, sz) = match (kind, passenger) {
        (Kind::Boat, true) => (-0.6, 0.0),
        (Kind::Camel, false) => (0.0, 0.5),
        (Kind::Camel, true) => (0.0, -0.5),
        _ => (0.0, 0.0),
    };
    Protocol::SetActorDataPacket(Box::new(bedrock::protocol::v662::packets::SetActorDataPacket {
        target_runtime_id: ActorRuntimeID(player as u64),
        actor_data: vec![
            item(ActorDataIDs::Reserved0, DataItemType::Int64(crate::spawn::FLAGS | (on as i64) << 2)),
            item(ActorDataIDs::Reserved056, DataItemType::Vec3(if on { (sx, kind.seat_offset(), sz) } else { (0.0, 0.0, 0.0) })),
            item(ActorDataIDs::SeatLockPassengerRotation, DataItemType::Byte(boat as i8)),
            item(ActorDataIDs::SeatLockPassengerRotationDegrees, DataItemType::Float(if boat { 90.0 } else { 0.0 })),
            item(ActorDataIDs::SeatRotationOffset, DataItemType::Byte(boat as i8)),
            item(ActorDataIDs::SeatRotationOffsetDegrees, DataItemType::Float(if boat { -90.0 } else { 0.0 })),
        ],
        synced_properties: PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
        tick: 0,
    }))
}

fn event(id: i64, e: bedrock::protocol::v975::enums::ActorEvent) -> Protocol {
    Protocol::ActorEventPacket(Box::new(bedrock::protocol::v975::packets::ActorEventPacket {
        target_runtime_id: ActorRuntimeID(id as u64),
        event_id: e,
        data: 0,
        fire_at_position: None,
    }))
}

/// Zombie villager (then the villager it turns into) -> the player who cured it.
static CURED_BY: std::sync::LazyLock<Mutex<HashMap<i64, String>>> = std::sync::LazyLock::new(Default::default);

/// About once a minute of circling the dragon decides to land.
const PERCH_CHANCE: u32 = 60 * TPS;
/// How long it sits on the portal.
const PERCH_TICKS: u32 = 20 * TPS;

/// Blocks mobs collide with.
fn solid(b: i32) -> bool {
    blocks::is_solid(b) || blocks::is_leaves(b)
}

fn walkable(b: i32) -> bool {
    !solid(b) && !blocks::is_lava(b)
}

impl Mobs {
    fn rand(&self) -> u32 {
        let mut s = self.rng.lock();
        if *s == 0 {
            *s = 0x2545_F491_4F6C_DD1D ^ std::process::id() as u64;
        }
        *s ^= *s << 13;
        *s ^= *s >> 7;
        *s ^= *s << 17;
        (*s >> 16) as u32
    }

    fn randf(&self) -> f32 {
        (self.rand() % 10_000) as f32 / 10_000.0
    }

    /// Every living mob's kind and position, for spawn caps.
    pub fn snapshot(&self) -> Vec<(Kind, (f32, f32, f32))> {
        self.mobs.lock().values().filter(|m| m.dying == 0).map(|m| (m.kind, m.pos)).collect()
    }

    pub fn count(&self) -> usize {
        self.mobs.lock().len()
    }

    /// Puts a boat of `wood` (an index into BOAT_WOOD) at `pos`, facing `yaw`.
    pub fn spawn_boat(&self, server: &Server, wood: u8, pos: (f32, f32, f32), yaw: f32, chest: bool) -> i64 {
        let id = self.spawn(server, Kind::Boat, pos);
        let packets = self.mobs.lock().get_mut(&id).map(|m| {
            (m.baby, m.color, m.yaw, m.head_yaw) = (0, wood, yaw, yaw);
            let mut p = vec![];
            if chest {
                // Its 27 slots live in the container store under a key of its own.
                m.home = Some((id as i32, i32::MIN / 2, (id >> 32) as i32));
                p.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) })));
                p.push(add_packet(m));
            }
            p.push(data_packet(m));
            p
        });
        for p in packets.unwrap_or_default() {
            broadcast(server, p);
        }
        id
    }

    /// The chest store of the chest boat `id`, if it is one.
    pub fn chest_boat_store(&self, id: i64) -> Option<(i32, i32, i32)> {
        self.mobs.lock().get(&id).filter(|m| m.kind == Kind::Boat).and_then(|m| m.home).map(cart_store)
    }

    pub fn spawn(&self, server: &Server, kind: Kind, pos: (f32, f32, f32)) -> i64 {
        self.spawn_with(server, kind, pos, false)
    }

    /// Spawns a mob, a baby if `baby` (trial spawners' baby zombies). 0 if a plugin stopped it.
    pub fn spawn_baby(&self, server: &Server, kind: Kind, pos: (f32, f32, f32), baby: bool) -> i64 {
        self.spawn_with(server, kind, pos, baby)
    }

    fn spawn_with(&self, server: &Server, kind: Kind, pos: (f32, f32, f32), baby: bool) -> i64 {
        if !server.plugins.entity_spawn(server, kind.identifier(), pos) {
            return 0;
        }
        let id = crate::players::next_id();
        let yaw = self.randf() * 360.0;
        let m = Mob {
            id, kind, pos, vel: (0.0, 0.0, 0.0), yaw, health: kind.max_health(), on_ground: false, target: None, panic: 0, dying: 0,
            sent: (pos, yaw, yaw), head_yaw: yaw, path: vec![], love: 0, cooldown: if baby { GROW } else { 0 }, baby: if baby { GROW } else { 0 },
            aggro: None, attack_cd: 0, fuse: 0, burning: false, prey: None, owner: None, sitting: false,
            color: if kind == Kind::Sheep { sheep_color(self.rand()) } else { 0 }, sheared: false, regrow: 0, hurt_at: None, fire: 0, job: None, weak: 0, curing: 0, captain: false, rider: None, passenger: None, steered: None, saddled: false, kept: false, lit: false, charged: false, perch: 0, lifetime: 0, leash: None, draw: 0, aim: 0, temper: 0, buck: 0, push: None, home: None, brain: Default::default(),
        };
        let size = [1u8, 2, 4][(self.rand() % 3) as usize];
        if kind.spec().splits {
            return self.spawn_slime(server, kind, pos, size);
        }
        // 5% of zombies are babies (faster, don't burn).
        let m = if matches!(kind, Kind::Zombie | Kind::Husk | Kind::ZombieVillager | Kind::Drowned) && !baby && self.rand().is_multiple_of(20) {
            Mob { baby: u32::MAX / 2, ..m }
        } else {
            m
        };
        broadcast(server, add_packet(&m));
        if let Some(p) = held_packet(&m) {
            broadcast(server, p);
        }
        self.mobs.lock().insert(id, m);
        id
    }

    /// A slime or magma cube of `size` (1, 2 or 4): health size², kept in `color`.
    fn spawn_slime(&self, server: &Server, kind: Kind, pos: (f32, f32, f32), size: u8) -> i64 {
        let id = crate::players::next_id();
        let yaw = self.randf() * 360.0;
        let m = Mob {
            id, kind, pos, vel: (0.0, 0.0, 0.0), yaw, health: (size as f32).powi(2), on_ground: false, target: None, panic: 0, dying: 0,
            sent: (pos, yaw, yaw), head_yaw: yaw, path: vec![], love: 0, cooldown: 0, baby: 0,
            aggro: None, attack_cd: 0, fuse: 0, burning: false, prey: None, owner: None, sitting: false,
            color: size.max(1), sheared: false, regrow: 0, hurt_at: None, fire: 0, job: None, weak: 0, curing: 0, captain: false, rider: None, passenger: None, steered: None, saddled: false, kept: false, lit: false, charged: false, perch: 0, lifetime: 0, leash: None, draw: 0, aim: 0, temper: 0, buck: 0, push: None, home: None, brain: Default::default(),
        };
        broadcast(server, add_packet(&m));
        if let Some(p) = held_packet(&m) {
            broadcast(server, p);
        }
        self.mobs.lock().insert(id, m);
        id
    }

    pub fn spawn_packets(&self) -> Vec<Protocol> {
        self.mobs.lock().values().filter(|m| m.dying == 0).flat_map(|m| std::iter::once(add_packet(m)).chain(held_packet(m))).collect()
    }

    /// A player's held item changed (tempting).
    pub fn set_held(&self, player: i64, item: Option<&str>) {
        let mut h = self.held.lock();
        match item {
            Some(i) => {
                h.insert(player, i.to_string());
            }
            None => {
                h.remove(&player);
            }
        }
    }

    /// A player right-clicked mob `id` holding `held`. Returns what to do with the held item.
    pub fn interact(&self, server: &Server, id: i64, held: Option<&str>, player: i64) -> Use {
        // Asked before locking the mob list, so a plugin can look at mobs without a deadlock.
        let tame_ok = match (self.kind_of(id), server.get(player)) {
            (Some(k), Some(p)) if matches!(k, Kind::Wolf | Kind::Cat) || k.horse_like() => server.plugins.entity_tame(server, &p, id, k.identifier()),
            _ => true,
        };
        let mut mobs = self.mobs.lock();
        let Some(m) = mobs.get_mut(&id) else { return Use::Nothing };
        let held = held.unwrap_or("");
        if m.dying > 0 {
            return Use::Nothing;
        }
        use bedrock::protocol::v975::enums::ActorEvent;
        // Leads: tie a passive or neutral mob to the player; a llama brings the llamas around it
        // along as a caravan, each following the one ahead (vanilla, up to 10).
        if m.leash == Some(player) && held != "minecraft:lead" {
            m.leash = None;
            crate::plugin::later(crate::plugin::Later::Leash(player, m.id, false));
            let (pos, packet) = (m.pos, data_packet(m));
            drop(mobs);
            broadcast(server, packet);
            if let Some(lead) = crate::items::item_by_name("minecraft:lead") {
                server.entities.drop_item(server, lead, 1, (pos.0, pos.1 + 0.5, pos.2));
            }
            return Use::Nothing;
        }
        if held == "minecraft:lead" && m.leash.is_none() && !m.kind.hostile() && !m.kind.aquatic() && !matches!(m.kind, Kind::Villager | Kind::WanderingTrader | Kind::Boat | Kind::Minecart | Kind::ChestMinecart | Kind::EnderDragon | Kind::EndCrystal | Kind::Shulker | Kind::Wither) {
            m.leash = Some(player);
            m.kept = true;
            crate::plugin::later(crate::plugin::Later::Leash(player, m.id, true));
            let (kind, pos) = (m.kind, m.pos);
            let mut packets = vec![data_packet(m)];
            if matches!(kind, Kind::Llama | Kind::TraderLlama) {
                let mut ahead = id;
                let mut free: Vec<(i64, f32)> = mobs.values().filter(|o| o.id != id && matches!(o.kind, Kind::Llama | Kind::TraderLlama) && o.leash.is_none() && o.dying == 0).map(|o| (o.id, dist2(o.pos, pos))).filter(|(_, d)| *d < 64.0).collect();
                free.sort_by(|a, b| a.1.total_cmp(&b.1));
                for (lid, _) in free.into_iter().take(9) {
                    if let Some(o) = mobs.get_mut(&lid) {
                        o.leash = Some(ahead);
                        packets.push(data_packet(o));
                        ahead = lid;
                    }
                }
            }
            drop(mobs);
            for p in packets {
                broadcast(server, p);
            }
            return Use::Consume;
        }
        if m.kind == Kind::Creeper && held == "minecraft:flint_and_steel" {
            m.lit = true;
            return Use::Wear;
        }
        if m.kind == Kind::ChestMinecart {
            return m.home.map_or(Use::Nothing, |h| Use::OpenCart(cart_store(h)));
        }
        if m.kind == Kind::WanderingTrader {
            // Its offers are rolled the first time someone trades with it.
            if m.job.is_none() {
                let seed = self.rand() as i32;
                m.job = Some(Box::new(crate::villagers::Job { profession: crate::trades::WANDERING, tier: 1, exp: 0, seed, clothing: crate::trades::Clothing::from_index(0), uses: vec![], site: None, gossip: vec![] }));
            }
            return Use::Trade;
        }
        if m.kind == Kind::Villager && m.baby == 0 && m.job.as_ref().is_some_and(|j| j.profession != 0) {
            return Use::Trade;
        }
        // A golden apple on a weakened zombie villager starts curing it (3-5 minutes).
        if m.kind == Kind::ZombieVillager && held == "minecraft:golden_apple" && m.weak > 0 && m.curing == 0 {
            m.curing = (180 + self.rand() % 120) * TPS;
            if let Some(p) = server.get(player) {
                CURED_BY.lock().insert(id, p.name);
            }
            broadcast(server, sound("mob.zombie.remedy", m.pos));
            return Use::Consume;
        }
        // Shearing.
        if m.kind == Kind::Sheep && held.ends_with(":shears") && !m.sheared && m.baby == 0 {
            m.sheared = true;
            m.regrow = 3 * 60 * TPS;
            crate::plugin::later(crate::plugin::Later::Shear(player, m.id));
            let (pos, color) = (m.pos, m.color);
            let p = data_packet(m);
            drop(mobs);
            broadcast(server, p);
            if let Some(w) = crate::items::item_by_name(&format!("minecraft:{}_wool", WOOL[color as usize])) {
                server.entities.drop_item(server, w, 1 + (self.rand() % 3) as i8, (pos.0, pos.1 + 1.0, pos.2));
            }
            return Use::Nothing;
        }
        if m.kind.rideable() && m.baby == 0 {
            let tame = !m.kind.horse_like() || m.owner.is_some();
            // A saddle goes on tamed horses, and on any adult pig, strider or camel.
            if held == "minecraft:saddle" && !m.saddled && tame {
                m.saddled = true;
                broadcast(server, data_packet(m));
                broadcast(server, sound("mob.horse.armor", m.pos));
                return Use::Consume;
            }
            // Snacks calm an untamed horse down: more temper, better odds next ride.
            let snack = m.kind.temper_food(held);
            if snack > 0 && m.owner.is_none() {
                m.temper = (m.temper + snack).min(100);
                broadcast(server, event(id, ActorEvent::Feed));
                return Use::Consume;
            }
            let can_ride = m.kind.horse_like() || m.saddled || matches!(m.kind, Kind::Boat | Kind::Minecart | Kind::SkeletonHorse | Kind::ZombieHorse);
            // Boats and camels take a second rider behind the driver (PNX RideableComponent seats).
            // (a chest boat's back seat is its chest)
            if can_ride && m.rider.is_some_and(|r| r != player) && m.passenger.is_none() && matches!(m.kind, Kind::Boat | Kind::Camel) && !(m.kind == Kind::Boat && m.home.is_some()) && !m.kind.food(held) {
                m.passenger = Some(player);
                let packets = [link_packet_as(id, player, true, false), seat_flag(player, true, m.kind, true), data_packet(m)];
                drop(mobs);
                for p in packets {
                    broadcast(server, p);
                }
                return Use::Mount;
            }
            if can_ride && m.rider.is_none() && !m.kind.food(held) {
                m.rider = Some(player);
                m.path.clear();
                if m.owner.is_none() && m.kind.horse_like() {
                    m.buck = 40 + self.rand() % 60;
                }
                let packets = [link_packet(id, player, true), rider_flag(player, true, m.kind), data_packet(m)];
                drop(mobs);
                for p in packets {
                    broadcast(server, p);
                }
                return Use::Mount;
            }
        }
        // Taming: bones for wolves, raw fish for cats (1 in 3 tries).
        let tame_food = match m.kind {
            Kind::Wolf => held.ends_with(":bone"),
            Kind::Cat => held.ends_with(":cod") || held.ends_with(":salmon"),
            _ => false,
        };
        if tame_food && m.owner.is_none() && tame_ok {
            if self.rand().is_multiple_of(3) {
                m.owner = Some(player);
                m.aggro = None;
                m.sitting = true;
                broadcast(server, event(id, ActorEvent::TamingSucceeded));
                broadcast(server, data_packet(m));
            } else {
                broadcast(server, event(id, ActorEvent::TamingFailed));
            }
            return Use::Consume;
        }
        // The owner toggles sitting.
        if m.owner == Some(player) && matches!(m.kind, Kind::Wolf | Kind::Cat) {
            m.sitting = !m.sitting;
            m.path.clear();
            broadcast(server, data_packet(m));
            return Use::Nothing;
        }
        if m.kind == Kind::Cow && held.ends_with(":bucket") && m.baby == 0 {
            return Use::Milk;
        }
        if m.kind.food(held) {
            use bedrock::protocol::v975::enums::ActorEvent;
            m.kept = true;
            if m.baby > 0 {
                m.baby = m.baby.saturating_sub(m.baby / 10); // feeding babies speeds growth 10%
                broadcast(server, event(id, ActorEvent::Feed));
                return Use::Consume;
            }
            if m.love == 0 && m.cooldown == 0 {
                m.love = LOVE;
                broadcast(server, event(id, ActorEvent::InLoveHearts));
                return Use::Consume;
            }
        }
        Use::Nothing
    }

    /// Feet positions of living mobs (for pressure plates).
    pub fn feet(&self) -> Vec<(f32, f32, f32)> {
        self.mobs.lock().values().filter(|m| m.dying == 0 && !matches!(m.kind, Kind::Boat | Kind::Minecart | Kind::ChestMinecart)).map(|m| m.pos).collect()
    }

    pub fn contains(&self, id: i64) -> bool {
        self.mobs.lock().contains_key(&id)
    }

    /// Whether `player` sits on mob `id`, driving or as the passenger.
    pub fn carries(&self, id: i64, player: i64) -> bool {
        self.mobs.lock().get(&id).is_some_and(|m| m.rider == Some(player) || m.passenger == Some(player))
    }

    /// The driver's client moved the vehicle it steers (boats, horses, camels): that's where it is.
    pub fn client_move(&self, id: i64, player: i64, pos: (f32, f32, f32), yaw: f32) {
        if let Some(m) = self.mobs.lock().get_mut(&id).filter(|m| m.rider == Some(player) && !m.kind.cart()) {
            m.pos = pos;
            m.yaw = yaw;
            m.head_yaw = yaw;
            m.vel = (0.0, 0.0, 0.0);
            m.steered = Some(std::time::Instant::now());
        }
    }

    /// The rider moved: the client steers ridden mobs, so the mob follows its rider.
    /// `eye` is the rider's eye position.
    pub fn ride(&self, id: i64, player: i64, eye: (f32, f32, f32), yaw: f32, forward: bool) {
        if let Some(m) = self.mobs.lock().get_mut(&id).filter(|m| m.rider == Some(player)) {
            if m.kind == Kind::Minecart {
                // The server drives minecarts; holding forward only gives them a push.
                m.push = forward.then_some(yaw);
                return;
            }
            // The client's own vehicle moves win; the rider's eye is only a fallback (pigs, striders).
            if m.steered.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(1)) {
                return;
            }
            m.pos = (eye.0, eye.1 - m.kind.seat_offset(), eye.2);
            // A boat's rider sits turned 90 degrees from it (the seat's rotation offset).
            let yaw = if m.kind == Kind::Boat { yaw + 90.0 } else { yaw };
            m.yaw = yaw;
            m.head_yaw = yaw;
            m.vel = (0.0, 0.0, 0.0);
        }
    }

    /// /ride: puts `player` on mob `id` as its rider, saddle or not. False if it's taken.
    pub fn force_mount(&self, server: &Server, id: i64, player: i64) -> bool {
        let mut mobs = self.mobs.lock();
        let Some(m) = mobs.get_mut(&id).filter(|m| m.rider.is_none() && m.dying == 0) else { return false };
        m.rider = Some(player);
        m.path.clear();
        let packets = [link_packet(id, player, true), rider_flag(player, true, m.kind), data_packet(m)];
        drop(mobs);
        for p in packets {
            broadcast(server, p);
        }
        if let Some(me) = server.get(player) {
            server.plugins.mount(server, &me, id);
        }
        true
    }

    /// Gossip about `player` for villager `id` (a trade, say).
    pub fn gossip(&self, id: i64, player: &str, kind: crate::villagers::Gossip, value: i32) {
        if let Some(j) = self.mobs.lock().get_mut(&id).and_then(|m| m.job.as_mut()) {
            j.add_gossip(player, kind, value);
        }
    }

    /// A player killed a villager at `at`: every villager within 16 blocks hears (PNX kill()).
    pub fn villager_killed(&self, at: (f32, f32, f32), player: &str) {
        for m in self.mobs.lock().values_mut() {
            if m.kind == Kind::Villager && m.dying == 0 && dist2(m.pos, at) <= 16.0 * 16.0 && let Some(j) = m.job.as_mut() {
                j.add_gossip(player, crate::villagers::Gossip::MajorNegative, 25);
            }
        }
    }

    /// Villagers that meet (within 2 blocks) pass gossip both ways, now and then; once a game
    /// day it all fades a little (PNX GossipExecutor, EntityVillagerV2.onUpdate).
    fn gossip_tick(&self, server: &Server) {
        static DAY: Mutex<Option<i64>> = Mutex::new(None);
        let day = server.time().div_euclid(24_000) as i64;
        let new_day = DAY.lock().replace(day).is_some_and(|d| d != day);
        let mut mobs = self.mobs.lock();
        if new_day {
            for j in mobs.values_mut().filter_map(|m| m.job.as_mut()) {
                j.decay_gossip();
            }
        }
        if !self.rand().is_multiple_of(5 * TPS) {
            return;
        }
        let talkers: Vec<(i64, (f32, f32, f32))> = mobs.values().filter(|m| m.kind == Kind::Villager && m.dying == 0 && m.job.is_some()).map(|m| (m.id, m.pos)).collect();
        for (i, &(a, pa)) in talkers.iter().enumerate() {
            for &(b, pb) in &talkers[i + 1..] {
                if (pa.0 - pb.0).hypot(pa.2 - pb.2) >= 2.0 {
                    continue;
                }
                let (Some(ja), Some(jb)) = (mobs.get(&a).and_then(|m| m.job.as_deref().cloned()), mobs.get(&b).and_then(|m| m.job.as_deref().cloned())) else { continue };
                if let Some(j) = mobs.get_mut(&a).and_then(|m| m.job.as_mut()) {
                    j.hear(&jb);
                }
                if let Some(j) = mobs.get_mut(&b).and_then(|m| m.job.as_mut()) {
                    j.hear(&ja);
                }
            }
        }
    }

    /// Lightning hit at `at`: creepers within 4 blocks become charged (PNX EntityCreeper
    /// onStruckByLightning).
    pub fn struck_by_lightning(&self, server: &Server, at: (f32, f32, f32)) {
        let mut out = Vec::new();
        for m in self.mobs.lock().values_mut() {
            if m.kind == Kind::Creeper && !m.charged && m.dying == 0 && dist2(m.pos, at) <= 16.0 {
                m.charged = true;
                crate::plugin::later(crate::plugin::Later::CreeperPower(m.id));
                out.push(data_packet(m));
            }
        }
        for p in out {
            broadcast(server, p);
        }
    }

    /// The nearest living mob of `kind` within `radius` of `at`.
    pub fn nearest_of(&self, kind: Kind, at: (f32, f32, f32), radius: f32) -> Option<i64> {
        self.mobs.lock().values()
            .filter(|m| m.kind == kind && m.dying == 0 && dist2(m.pos, at) <= radius * radius)
            .min_by(|a, b| dist2(a.pos, at).total_cmp(&dist2(b.pos, at)))
            .map(|m| m.id)
    }

    /// Takes `player` off whatever they're riding.
    pub fn dismount(&self, server: &Server, player: i64) {
        let riding = self.mobs.lock().values().any(|m| m.rider == Some(player) || m.passenger == Some(player));
        if riding && let Some(me) = server.get(player) {
            server.plugins.dismount(server, &me);
        }
        let mut mobs = self.mobs.lock();
        if let Some(m) = mobs.values_mut().find(|m| m.passenger == Some(player)) {
            m.passenger = None;
            let packets = [link_packet(m.id, player, false), rider_flag(player, false, m.kind), data_packet(m)];
            drop(mobs);
            for p in packets {
                broadcast(server, p);
            }
            return;
        }
        let Some(m) = mobs.values_mut().find(|m| m.rider == Some(player)) else { return };
        m.rider = None;
        m.buck = 0;
        m.steered = None;
        let mut packets = vec![link_packet(m.id, player, false), rider_flag(player, false, m.kind)];
        // The passenger moves up to the driver's seat.
        if let Some(p) = m.passenger.take() {
            m.rider = Some(p);
            packets.extend([link_packet(m.id, p, true), rider_flag(p, true, m.kind)]);
        }
        packets.push(data_packet(m));
        drop(mobs);
        for p in packets {
            broadcast(server, p);
        }
    }

    /// Whether hostiles may target this player (call on join and game mode changes).
    pub fn set_targetable(&self, player: i64, yes: bool) {
        let mut t = self.targetable.lock();
        if yes {
            t.insert(player);
        } else {
            t.remove(&player);
        }
    }

    /// Chest minecarts: the block each is in and its container-store key.
    pub fn chest_carts(&self) -> Vec<((i32, i32, i32), (i32, i32, i32))> {
        self.mobs.lock().values()
            .filter(|m| m.dying == 0 && m.kind == Kind::ChestMinecart)
            .filter_map(|m| m.home.map(|h| ((m.pos.0.floor() as i32, m.pos.1.floor() as i32, m.pos.2.floor() as i32), cart_store(h))))
            .collect()
    }

    /// Living mobs (not carts, boats or crystals) within `radius` of `at`: (id, distance).
    pub fn near(&self, at: (f32, f32, f32), radius: f32) -> Vec<(i64, f32)> {
        self.mobs.lock().values()
            .filter(|m| m.dying == 0 && !matches!(m.kind, Kind::Boat | Kind::Minecart | Kind::ChestMinecart | Kind::EndCrystal))
            .map(|m| (m.id, dist2(m.pos, at).sqrt()))
            .filter(|&(_, d)| d <= radius)
            .collect()
    }

    pub fn hurt(&self, server: &Server, id: i64, damage: f32, from: (f32, f32, f32), attacker: Option<i64>) {
        self.hurt_with(server, id, damage, from, attacker, Hit::default());
    }

    /// Kills every mob (of one type if given), for /kill @e. Returns how many.
    pub fn kill_all(&self, server: &Server, kind: Option<&str>) -> usize {
        let ids: Vec<i64> = self.mobs.lock().values()
            .filter(|m| m.dying == 0 && kind.is_none_or(|k| m.kind.identifier().trim_start_matches("minecraft:") == k || Kind::from_name(k) == Some(m.kind)))
            .map(|m| m.id)
            .collect();
        for &id in &ids {
            if let Some(m) = self.mobs.lock().get_mut(&id) {
                m.hurt_at = None;
                m.health = 0.0;
            }
            self.hurt(server, id, 1.0, (0.0, 0.0, 0.0), None);
        }
        ids.len()
    }

    /// Moves mobs standing in (or on top of) block `cell` by `by`, for pistons (PNX
    /// BlockEntityPistonArm.moveCollidedEntities). Never pushes down.
    pub fn shove(&self, cell: (i32, i32, i32), by: (f32, f32, f32)) {
        if by.1 < 0.0 {
            return;
        }
        for m in self.mobs.lock().values_mut() {
            let feet = (m.pos.0.floor() as i32, m.pos.1.floor() as i32, m.pos.2.floor() as i32);
            if feet == cell || feet == (cell.0, cell.1 + 1, cell.2) {
                m.pos = (m.pos.0 + by.0, m.pos.1 + by.1 * 2.0, m.pos.2 + by.2);
            }
        }
    }

    /// Moves every living mob (or one kind) to `to`, for /tp @e. Returns how many moved.
    pub fn teleport_all(&self, kind: Option<&str>, to: (f32, f32, f32)) -> usize {
        let mut n = 0;
        for m in self.mobs.lock().values_mut().filter(|m| m.dying == 0 && kind.is_none_or(|k| m.kind.identifier().trim_start_matches("minecraft:") == k || Kind::from_name(k) == Some(m.kind))) {
            m.pos = to;
            m.vel = (0.0, 0.0, 0.0);
            n += 1;
        }
        n
    }

    /// Living mobs of one kind: (id, position, health).
    pub fn of_kind(&self, kind: Kind) -> Vec<(i64, (f32, f32, f32), f32)> {
        self.mobs.lock().values().filter(|m| m.kind == kind && m.dying == 0).map(|m| (m.id, m.pos, m.health)).collect()
    }

    /// Heals mob `id` by `n` (up to its max).
    pub fn heal(&self, id: i64, n: f32) {
        if let Some(m) = self.mobs.lock().get_mut(&id) {
            m.health = (m.health + n).min(m.kind.max_health());
        }
    }

    /// Makes mob `id` a patrol captain.
    pub fn set_captain(&self, id: i64) {
        if let Some(m) = self.mobs.lock().get_mut(&id) {
            m.captain = true;
        }
    }

    /// A villager's job (for the trade screen).
    pub fn job_of(&self, id: i64) -> Option<crate::villagers::Job> {
        self.mobs.lock().get(&id).and_then(|m| m.job.as_deref().cloned())
    }

    /// A trade happened: offer `index` used `n` times; the villager gains XP and may level up.
    /// Returns the updated job.
    pub fn traded(&self, server: &Server, id: i64, index: usize, n: i32, trader_exp: i32) -> Option<crate::villagers::Job> {
        let mut mobs = self.mobs.lock();
        let m = mobs.get_mut(&id)?;
        let j = m.job.as_mut()?;
        if j.uses.len() <= index {
            j.uses.resize(index + 1, 0);
        }
        j.uses[index] += n;
        let up = j.add_exp(trader_exp * n);
        let job = (**j).clone();
        let pos = m.pos;
        let packet = data_packet(m);
        drop(mobs);
        broadcast(server, packet);
        if up {
            server.plugins.villager_level_up(server, id, job.tier as i32);
            broadcast(server, sound("random.levelup", pos));
            broadcast(server, event(id, bedrock::protocol::v975::enums::ActorEvent::VillagerHappy));
        }
        Some(job)
    }

    /// The kind of mob `id`, if it exists.
    pub fn kind_of(&self, id: i64) -> Option<Kind> {
        self.mobs.lock().get(&id).map(|m| m.kind)
    }

    /// A hit with weapon enchantments: knockback, fire aspect (ticks on fire), looting.
    pub fn hurt_with(&self, server: &Server, id: i64, damage: f32, from: (f32, f32, f32), attacker: Option<i64>, hit: Hit) {
        // Player hits already went through on_entity_damage; ask about everything else here,
        // before locking the mob list so the plugin can look at mobs.
        if attacker.is_none()
            && let Some(kind) = self.kind_of(id)
            && !server.plugins.mob_damage(server, id, kind.identifier(), damage, from)
        {
            return;
        }
        let mut mobs = self.mobs.lock();
        let Some(m) = mobs.get_mut(&id) else { return };
        // A player hitting a villager is talked about (PNX EntityVillagerV2.attack).
        if m.kind == Kind::Villager
            && m.dying == 0
            && let Some(p) = attacker.and_then(|a| server.get(a))
            && let Some(j) = m.job.as_mut()
        {
            j.add_gossip(&p.name, crate::villagers::Gossip::MinorNegative, 25);
        }
        if m.kind == Kind::EndCrystal {
            let at = m.pos;
            mobs.remove(&id);
            drop(mobs);
            broadcast(server, Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) })));
            explode(server, at, 6.0, "End Crystal");
            return;
        }
        if m.dying > 0 {
            return;
        }
        // Half a second of invulnerability after each hit (vanilla 10 ticks), so spam-clicking
        // doesn't stack damage.
        if m.hurt_at.is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(500)) {
            return;
        }
        if m.kind.hostile() || m.kind.neutral() || matches!(m.kind, Kind::IronGolem) || m.kind == Kind::Wolf && m.owner.is_none() {
            m.aggro = attacker.or(m.aggro);
        }
        let (victim_kind, victim_owner, victim_pos) = (m.kind, m.owner, m.pos);
        // Tamed wolves join in on whatever their owner hits; golems defend villagers.
        if let Some(p) = attacker {
            for o in mobs.values_mut() {
                if o.kind == Kind::Wolf && o.owner == Some(p) && o.owner != victim_owner && o.id != id {
                    o.prey = Some(id);
                    o.sitting = false;
                }
                if o.kind == Kind::IronGolem && victim_kind == Kind::Villager {
                    o.aggro = Some(p);
                }
                // Pack anger (vanilla): zombified piglins within 32 blocks, a wild wolf's pack
                // within 16.
                let pack = match victim_kind {
                    Kind::ZombiePigman => 32.0,
                    Kind::Wolf if victim_owner.is_none() && o.owner.is_none() => 16.0,
                    _ => 0.0,
                };
                if o.kind == victim_kind && pack > 0.0 && dist2(o.pos, victim_pos) < pack * pack {
                    o.aggro = Some(p);
                }
            }
        }
        let Some(m) = mobs.get_mut(&id) else { return };
        if m.kind == Kind::Enderman {
            // Teleports away when hit, then comes back for you.
            let a = self.randf() * std::f32::consts::TAU;
            m.pos = (m.pos.0 + a.cos() * 8.0, m.pos.1 + 2.0, m.pos.2 + a.sin() * 8.0);
            m.path.clear();
        }
        m.health -= damage;
        let (dx, dz) = (m.pos.0 - from.0, m.pos.2 - from.2);
        let len = (dx * dx + dz * dz).sqrt().max(0.01);
        // PNX EntityLiving.knockBack: half the old motion plus `base` away and up (up capped at
        // 0.4). base is EntityDamageByEntityEvent's 0.3, Knockback adds 0.5 a level.
        // Horizontal is doubled since mobs tick at 10 Hz, not 20.
        let base = 0.3 + 0.5 * hit.knockback as f32;
        m.vel = (m.vel.0 / 2.0 + dx / len * base * 2.0, (m.vel.1 / 2.0 + base).min(0.4), m.vel.2 / 2.0 + dz / len * base * 2.0);
        m.hurt_at = Some(std::time::Instant::now());
        m.fire = m.fire.max(hit.fire);
        if !m.kind.hostile() {
            m.panic = 50;
        }
        m.target = None;
        use bedrock::protocol::v975::enums::ActorEvent;
        broadcast(server, event(id, ActorEvent::Hurt));
        broadcast(server, mob_sound(if m.health <= 0.0 { "death" } else { "hurt" }, m.kind, m.pos, m.baby > 0));
        if m.health <= 0.0 {
            m.dying = 10;
            broadcast(server, event(id, ActorEvent::Death));
            if m.kind == Kind::Wither {
                for p in server.all() {
                    crate::ui::remove_boss_bar(server, p.id);
                }
            }
            let (kind, pos, baby, slime, captain, home) = (m.kind, m.pos, m.baby > 0, m.color, m.captain, m.home);
            drop(mobs);
            // A broken chest minecart spills what's in it (rolling its loot first if never opened).
            if let Some(h) = home {
                let store = cart_store(h);
                if let Some(loot) = crate::loot::take(store, crate::inventory::CHEST_SLOTS) {
                    server.containers.put(store, loot);
                }
                for s in server.containers.remove(store).into_iter().flatten() {
                    server.entities.drop_item(server, s.item, s.count, (pos.0, pos.1 + 0.5, pos.2));
                }
            }
            // Slimes and magma cubes split into 2-4 smaller ones (PNX `EntitySlime.close`).
            if kind.spec().splits && slime > 1 {
                for i in 0..2 + self.rand() % 3 {
                    let a = i as f32 * 1.7;
                    self.spawn_slime(server, kind, (pos.0 + a.cos() * 0.4, pos.1 + 0.3, pos.2 + a.sin() * 0.4), slime / 2);
                }
            }
            server.plugins.entity_death(server, kind.identifier(), pos, attacker);
            if matches!(kind, Kind::Boat | Kind::Minecart | Kind::ChestMinecart | Kind::TntMinecart) {
                server.plugins.vehicle_destroy(server, kind.identifier(), pos, attacker);
            }
            if kind == Kind::Villager {
                crate::villagers::release(id);
                if let Some(p) = attacker.and_then(|a| server.get(a)) {
                    self.villager_killed(pos, &p.name);
                }
            }
            if captain && let Some(p) = attacker {
                // Killing a patrol captain gives Bad Omen for 100 minutes (vanilla).
                let _ = server.events.send(Event::Effect { to: p, effect: crate::effects::BAD_OMEN, amplifier: 0, ticks: 120_000 });
            }
            if kind == Kind::EnderDragon {
                crate::endfight::dragon_died(server, attacker);
            }
            // XP only for a player kill, as orbs at the body (vanilla), not straight into the bar.
            if attacker.is_some_and(|a| server.get(a).is_some()) && !baby {
                let points = match kind {
                    Kind::IronGolem | Kind::Villager | Kind::Boat | Kind::Minecart | Kind::ChestMinecart => 0,
                    k if k.hostile() => 5,
                    _ => 1 + (self.rand() % 3) as i32,
                };
                if points > 0 {
                    server.entities.drop_xp(server, pos, points);
                }
            }
            if kind == Kind::Boat && let Some(it) = crate::items::item_by_name(&if home.is_some() { chest_boat_item(slime) } else { boat_item(slime) }) {
                server.entities.drop_item(server, it, 1, (pos.0, pos.1 + 0.5, pos.2));
            }
            for &(item, lo, hi) in kind.drops() {
                // Looting raises the most of each drop by its level (PNX `EntityLiving.getDrops`).
                let hi = hi + hit.looting as i8;
                let n = lo + (self.rand() % (hi - lo + 1) as u32) as i8;
                if n > 0
                    && let Some(it) = crate::items::item_by_name(item)
                {
                    server.entities.drop_item(server, it, n, (pos.0, pos.1 + 0.5, pos.2));
                }
            }
        }
    }

    pub fn tick(&self, server: &Server) {
        tick_tnt(server);
        self.tick_wandering_trader(server);
        self.tick_spawners(server);
        self.tick_detectors(server);
        let mut out = Vec::new();
        let ids: Vec<i64> = self.mobs.lock().keys().copied().collect();
        let players: Vec<(f32, f32)> = server.all().iter().map(|p| (p.pos.0, p.pos.2)).collect();
        for id in ids {
            let Some(mut m) = self.mobs.lock().get(&id).cloned() else { continue };
            // Like vanilla's simulation distance: mobs nobody is near don't think or move (and
            // don't pathfind into chunks nobody needs). Tests run without players, so not there.
            let near = players.iter().any(|p| (p.0 - m.pos.0).powi(2) + (p.1 - m.pos.2).powi(2) < SIMULATION * SIMULATION);
            if !near && m.dying == 0 && !cfg!(test) {
                // Still despawn like vanilla past 128 blocks.
                if self.despawn(server, &m) {
                    self.mobs.lock().remove(&id);
                    out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) })));
                }
                continue;
            }
            // Animals too (Bedrock despawns natural passive mobs), not just hostiles.
            if m.dying == 0 && !m.kind.hostile() && !m.kind.aquatic() && !m.kind.flies() && !cfg!(test) && self.despawn(server, &m) {
                self.mobs.lock().remove(&id);
                out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) })));
                continue;
            }
            // Idle sounds now and then (vanilla: about every 8 seconds on average).
            if m.dying == 0 && !matches!(m.kind, Kind::Boat | Kind::Minecart | Kind::ChestMinecart | Kind::EndCrystal) && self.rand().is_multiple_of(80) {
                out.push(mob_sound("ambient", m.kind, m.pos, m.baby > 0));
            }
            if m.dying > 0 {
                m.dying -= 1;
                if m.dying == 0 {
                    self.mobs.lock().remove(&id);
                    out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) })));
                } else {
                    self.mobs.lock().insert(id, m);
                }
                continue;
            }
            if m.baby > 0 {
                m.baby -= 1;
                if m.baby == 0 {
                    out.push(data_packet(&m)); // grew up
                }
            }
            m.cooldown = m.cooldown.saturating_sub(1);
            if let Some(p) = m.passenger
                && (server.get(p).is_none() || m.health <= 0.0)
            {
                m.passenger = None;
                out.extend([link_packet(id, p, false), rider_flag(p, false, m.kind)]);
            }
            if let Some(r) = m.rider {
                use bedrock::protocol::v975::enums::ActorEvent;
                if server.get(r).is_none() || m.health <= 0.0 {
                    m.rider = None;
                    out.extend([link_packet(id, r, false), rider_flag(r, false, m.kind), data_packet(&m)]);
                } else {
                    // An untamed horse decides after a few seconds: keep the rider (tamed) or buck.
                    if m.buck > 0 {
                        m.buck -= 1;
                        if m.buck == 0 {
                            let tame_ok = server.get(r).is_none_or(|p| server.plugins.entity_tame(server, &p, id, m.kind.identifier()));
                            if tame_ok && self.rand() % 100 < m.temper {
                                m.owner = Some(r);
                                out.extend([event(id, ActorEvent::TamingSucceeded), data_packet(&m)]);
                            } else {
                                m.temper = (m.temper + 5).min(100);
                                m.rider = None;
                                out.extend([event(id, ActorEvent::TamingFailed), link_packet(id, r, false), rider_flag(r, false, m.kind), data_packet(&m)]);
                                out.push(sound("mob.horse.angry", m.pos));
                            }
                        }
                    }
                    if m.kind.cart() {
                        self.cart(server, &mut m, &mut out);
                        if let Some(slot) = self.mobs.lock().get_mut(&id) {
                            (slot.pos, slot.vel, slot.yaw, slot.on_ground, slot.sent, slot.rider) = (m.pos, m.vel, m.yaw, m.on_ground, m.sent, m.rider);
                        }
                        continue;
                    }
                    // Ridden: the rider steers, so no AI or physics of its own.
                    if m.sent.0 != m.pos || (m.sent.1 - m.yaw).abs() > 2.0 {
                        m.sent = (m.pos, m.yaw, m.head_yaw);
                        out.push(move_packet(&m));
                    }
                    // Only write back what this tick changed: `ride` moves the mob meanwhile.
                    if let Some(slot) = self.mobs.lock().get_mut(&id) {
                        (slot.rider, slot.owner, slot.temper, slot.buck, slot.sent) = (m.rider, m.owner, m.temper, m.buck, m.sent);
                    }
                    continue;
                }
            }
            if m.love > 0 {
                m.love -= 1;
                if m.love % 20 == 0 {
                    use bedrock::protocol::v975::enums::ActorEvent;
                    out.push(event(id, ActorEvent::InLoveHearts));
                }
            }
            if m.kind.cart() && m.rider.is_none() {
                self.cart(server, &mut m, &mut out);
                // A lit TNT minecart goes off after 4 seconds (power 4).
                if m.kind == Kind::TntMinecart && m.fuse > 0 {
                    m.fuse += 1;
                    if m.fuse > 4 * TPS {
                        self.mobs.lock().remove(&id);
                        out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) })));
                        explode(server, m.pos, 4.0, "TNT");
                        continue;
                    }
                }
                if let Some(slot) = self.mobs.lock().get_mut(&id) {
                    (slot.pos, slot.vel, slot.yaw, slot.on_ground, slot.sent, slot.fuse) = (m.pos, m.vel, m.yaw, m.on_ground, m.sent, m.fuse);
                }
                continue;
            }
            if matches!(m.kind, Kind::EndCrystal | Kind::Boat) {
                continue; // crystals sit on their pillar, boats stay where they were left
            }
            m.weak = m.weak.saturating_sub(1);
            if m.curing > 0 {
                m.curing -= 1;
                if m.curing == 0 {
                    // Cured: the zombie villager becomes a villager (PNX EntityZombieVillager).
                    self.mobs.lock().remove(&id);
                    out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) })));
                    let pos = m.pos;
                    out.push(sound("mob.zombie.unfect", pos));
                    let nid = self.spawn(server, Kind::Villager, pos);
                    crate::plugin::later(crate::plugin::Later::Transform(id, Kind::ZombieVillager.identifier(), Kind::Villager.identifier(), nid));
                    if let Some(p) = CURED_BY.lock().remove(&id) {
                        CURED_BY.lock().insert(nid, p);
                    }
                    continue;
                }
            }
            if m.kind == Kind::SnowGolem {
                self.snow_golem(server, &mut m);
            }
            if let Some(h) = m.leash {
                self.follow_lead(server, &mut m, h, &mut out);
            }
            // Polar bears go for players who come near their cubs (vanilla).
            if m.kind == Kind::PolarBear && m.baby == 0 && m.aggro.is_none() && self.rand().is_multiple_of(10) {
                let cub = self.nearest(&m, 16.0, |o| o.kind == Kind::PolarBear && o.baby > 0).map(|c| c.1);
                if let Some(c) = cub
                    && let Some(p) = server.all().into_iter().find(|p| self.targetable.lock().contains(&p.id) && dist2((p.pos.0, p.pos.1 - 1.62, p.pos.2), c) < 8.0 * 8.0)
                {
                    m.aggro = Some(p.id);
                }
            }
            // Wandering traders and their llamas leave when their time is up.
            if m.lifetime > 0 {
                m.lifetime -= 1;
                if m.lifetime == 0 && m.owner.is_none() && !m.kept {
                    self.mobs.lock().remove(&id);
                    out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) })));
                    continue;
                }
                if let Some(slot) = self.mobs.lock().get_mut(&id) {
                    slot.lifetime = m.lifetime;
                }
            }
            // Tadpoles grow into frogs after about 20 minutes (12000 ticks, vanilla).
            if m.kind == Kind::Tadpole && m.dying == 0 && self.rand() % (12_000 / 2) == 0 {
                self.mobs.lock().remove(&id);
                out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) })));
                self.spawn(server, Kind::Frog, m.pos);
                continue;
            }
            // The wither's boss bar for everyone within 64 blocks.
            if m.kind == Kind::Wither && m.dying == 0 && self.rand().is_multiple_of(5) {
                for p in server.all() {
                    if dist2(p.pos, m.pos) < 64.0 * 64.0 {
                        crate::ui::boss_bar(server, p.id, "Wither", (m.health / m.kind.max_health()).clamp(0.0, 1.0), 5);
                    }
                }
            }
            if m.kind.flies() {
                self.fly(server, &mut m, &mut out);
                if (m.kind.hostile() || m.kind == Kind::Bat) && self.despawn(server, &m) {
                    self.mobs.lock().remove(&id);
                    out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) })));
                    continue;
                }
            } else if (m.kind.hostile() && !m.kind.aquatic()) || (m.kind.neutral() && m.aggro.is_some()) {
                if self.hostile(server, &mut m, &mut out) {
                    // Exploded: gone without the death animation.
                    self.mobs.lock().remove(&id);
                    out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) })));
                    continue;
                }
                if m.health <= 0.0 && m.dying == 0 {
                    use bedrock::protocol::v975::enums::ActorEvent;
                    m.dying = 10;
                    out.push(event(id, ActorEvent::Death));
                    self.mobs.lock().insert(id, m);
                    continue;
                }
                if m.kind.hostile() && self.despawn(server, &m) {
                    self.mobs.lock().remove(&id);
                    out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) })));
                    continue;
                }
            } else if m.kind.aquatic() {
                if self.despawn(server, &m) {
                    self.mobs.lock().remove(&id);
                    out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) })));
                    continue;
                }
                self.swim(server, &mut m, &mut out);
                if m.kind.hostile() {
                    self.laser(server, &mut m);
                }
            } else if m.kind.companion() {
                self.defend(server, &mut m);
            } else if m.kind == Kind::Villager {
                self.villager(server, &mut m, &mut out);
            } else if let Some(partner) = self.think(server, &mut m) {
                self.breed(server, &mut m, partner, &mut out);
            }
            if m.sheared {
                m.regrow = m.regrow.saturating_sub(1);
                if m.regrow == 0 {
                    m.sheared = false; // grazed and grew its wool back
                    crate::plugin::later(crate::plugin::Later::Regrow(m.id));
                    out.push(data_packet(&m));
                }
            }
            if m.health <= 0.0 && m.dying == 0 {
                use bedrock::protocol::v975::enums::ActorEvent;
                m.dying = 10;
                out.push(event(id, ActorEvent::Death));
                self.mobs.lock().insert(id, m);
                continue;
            }
            if m.kind.flies() {
                self.fly_physics(server, &mut m);
            } else if m.kind.aquatic() {
                self.swim_physics(server, &mut m);
            } else {
                self.physics(server, &mut m);
            }
            let ((sp, sy, sh), p) = (m.sent, m.pos);
            if (sp.0 - p.0).abs() + (sp.1 - p.1).abs() + (sp.2 - p.2).abs() > 0.02 || (sy - m.yaw).abs() > 2.0 || (sh - m.head_yaw).abs() > 4.0 {
                m.sent = (p, m.yaw, m.head_yaw);
                out.push(move_packet(&m));
            }
            if m.pos.1 < -100.0 {
                self.mobs.lock().remove(&id);
                out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) })));
                continue;
            }
            if let Some(slot) = self.mobs.lock().get_mut(&id) {
                *slot = m;
            }
        }
        self.tick_arrows(server, &mut out);
        self.tick_thrown(server, &mut out);
        for p in out {
            broadcast(server, p);
        }
    }

    /// Detector rails switch off a moment after the last cart leaves them.
    fn tick_detectors(&self, server: &Server) {
        let mut off = Vec::new();
        DETECTORS.lock().get_or_insert_with(HashMap::new).retain(|&p, left| {
            *left = left.saturating_sub(1);
            if *left == 0 {
                off.push(p);
            }
            *left > 0
        });
        for p in off {
            let b = server.world.get(p.0, p.1, p.2);
            if blocks::short(b) == "detector_rail" {
                server.set_block_quiet(0, p.0, p.1, p.2, crate::blocks::with(b, &[("rail_data_bit", crate::blocks::State::Bool(false))]));
            }
        }
    }

    /// Structure spawners (vanilla `MobSpawnerBaseLogic`): with a player within 16 blocks,
    /// every 10-40 s spawn up to 4 of their mob within 4 blocks, unless 6 are already close.
    fn tick_spawners(&self, server: &Server) {
        static DELAY: Mutex<Option<HashMap<(i32, i32, i32), u32>>> = Mutex::new(None);
        crate::trial::tick(server, self, || self.rand());
        self.gossip_tick(server);
        let spawners: Vec<((i32, i32, i32), &str)> = crate::loot::spawners().lock().iter().map(|(p, m)| (*p, *m)).collect();
        if spawners.is_empty() {
            return;
        }
        let players: Vec<(f32, f32, f32)> = server.all().iter().map(|p| (p.pos.0, p.pos.1 - 1.62, p.pos.2)).collect();
        let mut spawn = Vec::new();
        {
            let mut delays = DELAY.lock();
            let delays = delays.get_or_insert_with(HashMap::new);
            for (at, mob) in spawners {
                let centre = (at.0 as f32 + 0.5, at.1 as f32, at.2 as f32 + 0.5);
                if !players.iter().any(|&p| dist2(p, centre) < 16.0 * 16.0) {
                    continue;
                }
                let d = delays.entry(at).or_insert(0);
                if *d > 0 {
                    *d -= 1;
                    continue;
                }
                *d = (10 + self.rand() % 30) * TPS;
                if let Some(kind) = Kind::from_name(mob) {
                    spawn.push((at, kind));
                }
            }
        }
        for (at, kind) in spawn {
            if blocks::short(server.world.get(at.0, at.1, at.2)) != "mob_spawner" {
                crate::loot::spawners().lock().remove(&at); // broken
                continue;
            }
            if !server.plugins.spawner_spawn(server, at, kind.identifier()) {
                continue;
            }
            let centre = (at.0 as f32 + 0.5, at.1 as f32, at.2 as f32 + 0.5);
            let near = self.mobs.lock().values().filter(|m| m.kind == kind && dist2(m.pos, centre) < 8.0 * 8.0).count();
            for _ in 0..6usize.saturating_sub(near).min(4) {
                let p = (at.0 + (self.rand() % 9) as i32 - 4, at.1 + (self.rand() % 3) as i32 - 1, at.2 + (self.rand() % 9) as i32 - 4);
                if standable(&server.world, p) {
                    self.spawn(server, kind, (p.0 as f32 + 0.5, p.1 as f32, p.2 as f32 + 0.5));
                }
            }
        }
    }

    /// A minecart's tick: follow the rails, or roll and fall off them. Sends its movement.
    fn cart(&self, server: &Server, m: &mut Mob, out: &mut Vec<Protocol>) {
        use crate::minecart::{RailKind, find, off_rail, on_rail};
        let w = &server.world;
        match find(w, m.pos) {
            Some(rail) => {
                (m.pos, m.vel) = on_rail(w, rail, m.pos, m.vel, m.rider.is_some(), m.push.take(), solid);
                m.on_ground = true;
                if rail.kind == RailKind::Detector {
                    detector_on(server, rail);
                }
                // A TNT minecart on a powered activator rail lights (4 s fuse, vanilla).
                if rail.kind == RailKind::Activator && rail.on && m.kind == Kind::TntMinecart && m.fuse == 0 {
                    m.fuse = 1;
                    out.push(data_packet(m));
                    out.push(crate::packets::play_sound("random.fuse", m.pos));
                }
                if rail.kind == RailKind::Activator && rail.on && let Some(r) = m.rider.take() {
                    // A powered activator rail throws the rider out.
                    out.extend([link_packet(m.id, r, false), rider_flag(r, false, m.kind), data_packet(m)]);
                }
            }
            None => {
                m.vel = off_rail(m.vel, m.on_ground);
                let next = (m.pos.0 + m.vel.0, m.pos.1 + m.vel.1, m.pos.2 + m.vel.2);
                let (nx, ny, nz) = (next.0.floor() as i32, next.1.floor() as i32, next.2.floor() as i32);
                m.on_ground = solid(w.get(nx, ny, nz));
                if m.on_ground {
                    m.vel.1 = 0.0;
                    m.pos = (m.pos.0, ny as f32 + 1.0, m.pos.2);
                } else if solid(w.get(nx, m.pos.1.floor() as i32, nz)) {
                    m.vel = (0.0, m.vel.1, 0.0);
                    m.pos.1 = next.1;
                } else {
                    m.pos = next;
                }
            }
        }
        if m.vel.0.abs() + m.vel.2.abs() > 1e-3 {
            m.yaw = (-m.vel.0).atan2(m.vel.2).to_degrees();
        }
        if m.sent.0 != m.pos || (m.sent.1 - m.yaw).abs() > 2.0 {
            m.sent = (m.pos, m.yaw, m.head_yaw);
            out.push(move_packet(m));
        }
    }

    /// Puts a minecart on the rail at `at`.
    /// Chest minecarts the world generator put in mineshafts, waiting to be spawned.
    pub fn spawn_chest_carts(&self, server: &Server) {
        for at in crate::loot::take_carts() {
            let id = self.spawn(server, Kind::ChestMinecart, (at.0 as f32 + 0.5, at.1 as f32, at.2 as f32 + 0.5));
            if let Some(m) = self.mobs.lock().get_mut(&id) {
                (m.baby, m.home) = (0, Some(at));
            }
        }
    }

    /// Any minecart (plain or TNT) placed on the rail at `at`.
    pub fn spawn_cart(&self, server: &Server, kind: Kind, at: (i32, i32, i32)) -> i64 {
        let id = self.spawn(server, kind, (at.0 as f32 + 0.5, at.1 as f32, at.2 as f32 + 0.5));
        if let Some(m) = self.mobs.lock().get_mut(&id) {
            m.baby = 0;
        }
        id
    }

    /// Hostile AI: burn in sun, pick a target, chase, melee/shoot/fuse. True if it blew up.
    fn hostile(&self, server: &Server, m: &mut Mob, out: &mut Vec<Protocol>) -> bool {
        let w = &server.world;
        let feet = (m.pos.0.floor() as i32, m.pos.1.floor() as i32, m.pos.2.floor() as i32);
        // The creaking can't move while any player is looking at it (vanilla).
        if m.kind == Kind::Creaking && server.all().iter().any(|p| stares_at(p.pos, p.rot, (m.pos.0, m.pos.1 + 2.0, m.pos.2))) {
            m.path.clear();
            m.vel = (0.0, m.vel.1, 0.0);
            return false;
        }
        if m.kind == Kind::Creeper {
            // Lit with flint and steel (PNX EntityCreeper): the fuse runs out no matter what.
            if m.lit {
                if m.fuse == 0 {
                    out.push(data_packet(&Mob { fuse: 1, ..m.clone() }));
                    out.push(crate::packets::play_sound("random.fuse", m.pos));
                }
                m.fuse += 1;
                if m.fuse >= 15 {
                    explode(server, m.pos, if m.charged { 6.0 } else { 3.0 }, Kind::Creeper.display());
                    return true;
                }
                return false;
            }
            // Creepers run from cats and ocelots within 6 blocks (vanilla AvoidEntityGoal).
            if let Some((_, c)) = self.snapshot().into_iter().find(|(k, p)| matches!(k, Kind::Cat | Kind::Ocelot) && dist2(*p, m.pos) < 36.0) {
                if m.fuse > 0 {
                    m.fuse = 0;
                    out.push(data_packet(m));
                }
                let (dx, dz) = (m.pos.0 - c.0, m.pos.2 - c.2);
                let len = (dx * dx + dz * dz).sqrt().max(0.01);
                let away = ((m.pos.0 + dx / len * 8.0).floor() as i32, feet.1, (m.pos.2 + dz / len * 8.0).floor() as i32);
                if m.path.is_empty() || self.rand().is_multiple_of(10) {
                    m.path = path(w, feet, away, 80);
                }
                self.steer(m);
                return false;
            }
        }
        let t = server.time().rem_euclid(24_000);
        let day = !(12_500..23_500).contains(&t);
        let open_sky = sky(w, feet);
        // Sunlight sets zombies and skeletons on fire (1 damage a second).
        let in_water = blocks::is_water(w.get(feet.0, feet.1, feet.2));
        if in_water {
            m.fire = 0;
        }
        m.fire = m.fire.saturating_sub(1);
        let burning = (m.kind.spec().burns && day && open_sky && !in_water && m.baby == 0) || m.fire > 0;
        if burning != m.burning {
            if burning {
                crate::plugin::later(crate::plugin::Later::Combust(m.id, m.kind.identifier()));
            }
            m.burning = burning;
            out.push(data_packet(m));
        }
        if burning && self.rand().is_multiple_of(TPS) {
            m.health -= 1.0;
            use bedrock::protocol::v975::enums::ActorEvent;
            out.push(event(m.id, ActorEvent::Hurt));
        }
        // Target: nearest targetable player in range. Spiders are peaceful in daylight and
        // endermen until provoked (provoking sets `aggro` in `hurt`).
        let targets = self.targetable.lock().clone();
        let aggressive = match m.kind {
            Kind::Spider => !day || !open_sky,
            Kind::Enderman => false,
            Kind::Piglin => true, // only at players without gold armour, below
            k => !k.neutral(),
        };
        let players = server.all();
        // Endermen turn on a player who looks them in the eye (no carved pumpkin on).
        if m.kind == Kind::Enderman && m.aggro.is_none()
            && let Some(p) = players.iter().find(|p| {
                targets.contains(&p.id) && !wears(p.id, |n| n == "minecraft:carved_pumpkin") && stares_at(p.pos, p.rot, (m.pos.0, m.pos.1 + 2.55, m.pos.2))
            })
        {
            m.aggro = Some(p.id);
        }
        let target = m
            .aggro
            .and_then(|a| players.iter().find(|p| p.id == a && targets.contains(&a)))
            .or_else(|| {
                if !aggressive {
                    return None;
                }
                players
                    .iter()
                    .filter(|p| targets.contains(&p.id))
                    // Piglins leave players in any gold armour alone.
                    .filter(|p| m.kind != Kind::Piglin || !wears(p.id, |n| n.starts_with("minecraft:golden_")))
                    .map(|p| (p, dist2(p.pos, m.pos)))
                    .filter(|(_, d)| *d < 16.0 * 16.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(p, _)| p)
            })
            .filter(|p| server.plugins.mob_target(server, m.id, m.kind.identifier(), p))
            .cloned();
        m.attack_cd = m.attack_cd.saturating_sub(1);
        // Memory: the target, where it was last seen (looked for for 10 s), a villager to go for.
        let mem = &mut m.brain.memory;
        mem.target = target.as_ref().map(|p| (p.id, (p.pos.0, p.pos.1 - 1.62, p.pos.2), dist2((p.pos.0, p.pos.1 - 1.62, p.pos.2), m.pos)));
        match &mem.target {
            Some((_, at, _)) => mem.last_seen = Some((*at, 10 * TPS)),
            None => {
                if let Some((_, left)) = &mut mem.last_seen {
                    *left = left.saturating_sub(1);
                }
            }
        }
        let hunts = matches!(m.kind, Kind::Zombie | Kind::Drowned | Kind::Husk | Kind::ZombieVillager | Kind::Pillager | Kind::Vindicator | Kind::Ravager | Kind::Evoker);
        m.brain.memory.villager = (target.is_none() && hunts).then(|| self.nearest(m, 16.0, |o| o.kind == Kind::Villager)).flatten();
        let facts = crate::ai::Facts {
            has_path: !m.path.is_empty(),
            roll: self.rand() % 1000,
            // Skeletons and strays look for a roof when the sun sets them alight (vanilla FleeSunGoal).
            sunburnt: m.burning && matches!(m.kind, Kind::Skeleton | Kind::Stray) && open_sky,
            ..Default::default()
        };
        if m.brain.decide(&facts).is_some() {
            m.path.clear();
        }
        use crate::ai::Behavior as B;
        if m.brain.doing != B::Attack && m.fuse > 0 {
            m.fuse = 0; // a creeper that lost its target stops hissing
            out.push(data_packet(m));
        }
        match m.brain.doing {
            B::Attack => {}
            B::HuntVillager => {
                if let Some((vid, vpos)) = m.brain.memory.villager {
                    self.chase_and_hit(server, m, vid, vpos);
                }
                return false;
            }
            B::Search => {
                let Some((at, _)) = m.brain.memory.last_seen else { return false };
                if dist2(at, m.pos) < 2.0 {
                    m.brain.memory.last_seen = None; // got there, nobody around
                } else if m.path.is_empty() {
                    m.path = path(w, feet, (at.0.floor() as i32, at.1.floor() as i32, at.2.floor() as i32), 160);
                }
                self.steer(m);
                return false;
            }
            B::SeekShade => {
                if m.path.is_empty()
                    && let Some(q) = self.find_cover(w, feet)
                {
                    m.path = path(w, feet, q, 160);
                }
                self.steer(m);
                return false;
            }
            _ => {
                m.aggro = None;
                self.wander(server, m);
                return false;
            }
        }
        let Some(p) = target else { return false };
        let feet_p = (p.pos.0, p.pos.1 - 1.62, p.pos.2);
        let d2 = dist2(feet_p, m.pos);
        m.head_yaw = (-(p.pos.0 - m.pos.0)).atan2(p.pos.2 - m.pos.2).to_degrees();
        let goal = (feet_p.0.floor() as i32, feet_p.1.floor() as i32, feet_p.2.floor() as i32);
        match m.kind {
            k if k.spec().ranged.is_some() => {
                // Keep about 8 blocks away and shoot.
                if d2 > 100.0 && (m.path.is_empty() || self.rand().is_multiple_of(10)) {
                    m.path = path(w, feet, goal, 120);
                } else if d2 < 36.0 {
                    m.path.clear();
                }
                // Draw, then shoot (PNX: skeleton 20 gt draw, pillager 80 gt charge with
                // loading sounds, 30 gt cooldown, 15 block range).
                let crossbow = m.kind == Kind::Pillager;
                if d2 >= 15.0 * 15.0 {
                    if m.draw > 0 {
                        m.draw = 0;
                        out.push(data_packet(m));
                    }
                } else if m.draw == 0 && m.attack_cd == 0 {
                    m.draw = 1;
                    m.aim = p.id;
                    out.push(data_packet(m));
                    if crossbow {
                        out.push(sound("crossbow.loading.start", m.pos));
                    }
                } else if m.draw > 0 {
                    m.draw += 1;
                    if crossbow && m.draw == CROSSBOW_CHARGED / 2 {
                        out.push(sound("crossbow.loading.middle", m.pos));
                    }
                    if crossbow && m.draw == CROSSBOW_CHARGED {
                        out.push(sound("crossbow.loading.end", m.pos));
                        out.push(data_packet(m));
                    }
                    if m.draw > if crossbow { CROSSBOW_DRAW } else { BOW_DRAW } {
                        self.shoot(server, m, (p.pos.0, p.pos.1 - 0.4, p.pos.2), out);
                        m.draw = 0;
                        m.attack_cd = BOW_COOLDOWN;
                        out.push(data_packet(m));
                    }
                }
            }
            Kind::Creeper => {
                if d2 < 9.0 {
                    m.path.clear();
                    if m.fuse == 0 {
                        out.push(data_packet(&Mob { fuse: 1, ..m.clone() }));
                        out.push(crate::packets::play_sound("random.fuse", m.pos));
                    }
                    m.fuse += 1;
                    if m.fuse >= 15 {
                        explode(server, m.pos, if m.charged { 6.0 } else { 3.0 }, Kind::Creeper.display());
                        return true;
                    }
                } else {
                    if m.fuse > 0 && d2 > 49.0 {
                        m.fuse = 0;
                        out.push(data_packet(m));
                    }
                    if m.path.is_empty() || self.rand().is_multiple_of(10) {
                        m.path = path(w, feet, goal, 160);
                    }
                }
            }
            _ => {
                if d2 < 1.6 * 1.6 + (m.kind.size().0 / 2.0).powi(2) && (feet_p.1 - m.pos.1).abs() < 2.0 {
                    m.path.clear();
                    if m.attack_cd == 0 {
                        m.attack_cd = MELEE_COOLDOWN;
                        let dmg = m.kind.melee() * difficulty_scale();
                        let (dx, dz) = (feet_p.0 - m.pos.0, feet_p.2 - m.pos.2);
                        let len = (dx * dx + dz * dz).sqrt().max(0.01);
                        let _ = server.events.send(Event::Damage {
                            to: p.id,
                            damage: crate::health::Damage::Mob(dmg, m.kind.display()),
                            knock: (dx / len * 0.3, 0.3, dz / len * 0.3), // PNX default knockback 0.3
                        });
                        self.thorns(m, p.id, out);
                        if let Some((effect, secs)) = m.kind.spec().hit_effect {
                            let _ = server.events.send(Event::Effect { to: p.id, effect, amplifier: 0, ticks: secs * 20 });
                        }
                    }
                } else if m.path.is_empty() || self.rand().is_multiple_of(8) {
                    m.path = path(w, feet, goal, 160);
                }
            }
        }
        self.steer(m);
        false
    }

    /// Flyers drift between random points; hostile ones dive or shoot at the nearest player.
    fn fly(&self, server: &Server, m: &mut Mob, out: &mut Vec<Protocol>) {
        m.attack_cd = m.attack_cd.saturating_sub(1);
        let t = server.time().rem_euclid(24_000);
        let day = !(12_500..23_500).contains(&t);
        let feet = (m.pos.0.floor() as i32, m.pos.1.floor() as i32, m.pos.2.floor() as i32);
        let burning = m.kind.spec().burns && day && sky(&server.world, feet);
        if burning != m.burning {
            if burning {
                crate::plugin::later(crate::plugin::Later::Combust(m.id, m.kind.identifier()));
            }
            m.burning = burning;
            out.push(data_packet(m));
        }
        if burning && self.rand().is_multiple_of(TPS) {
            m.health -= 1.0;
        }
        let targets = self.targetable.lock().clone();
        let reach = if m.kind == Kind::EnderDragon { 150.0 } else { 24.0 };
        let mut target = m.kind.hostile().then(|| {
            server.all().into_iter().filter(|p| targets.contains(&p.id)).map(|p| (dist2(p.pos, m.pos), p)).filter(|(d, _)| *d < reach * reach).min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, p)| p)
        }).flatten();
        // Now and then, while circling, the dragon lands on the exit portal and sits there for
        // 20 s, where it can be hit up close (PNX DragonPhase landing approach and sitting).
        if m.kind == Kind::EnderDragon && m.perch == 0 && m.attack_cd > 0 && self.rand().is_multiple_of(PERCH_CHANCE) {
            m.perch = PERCH_TICKS;
        }
        if m.kind == Kind::EnderDragon && m.perch > 0 && let Some(spot) = crate::endfight::perch_point() {
            m.perch -= 1;
            target = None;
            m.path = vec![(spot.0.floor() as i32, spot.1.floor() as i32, spot.2.floor() as i32)];
            if dist2(spot, m.pos) < 4.0 {
                // Landed: sit still facing the nearest player.
                m.pos = spot;
                m.vel = (0.0, 0.0, 0.0);
                if let Some(p) = server.all().into_iter().min_by(|a, b| dist2(a.pos, m.pos).total_cmp(&dist2(b.pos, m.pos))) {
                    m.head_yaw = (-(p.pos.0 - m.pos.0)).atan2(p.pos.2 - m.pos.2).to_degrees();
                }
            }
        } else if m.kind == Kind::EnderDragon && m.attack_cd > 0 {
            // Circles the main island between dives.
            target = None;
            let base = crate::dims::base(crate::dims::END) as f32;
            let a = (m.attack_cd as f32 * 0.03) + m.id as f32;
            m.path = vec![((a.cos() * 50.0) as i32, (base + 80.0) as i32, (a.sin() * 50.0) as i32)];
        }
        // The brain: a target means attacking, a hit sends passive flyers fleeing, otherwise roam.
        m.brain.memory.target = target.as_ref().map(|p| (p.id, (p.pos.0, p.pos.1 - 1.62, p.pos.2), dist2(p.pos, m.pos)));
        let facts = crate::ai::Facts { panicking: m.panic > 0 && !m.kind.hostile(), has_path: !m.path.is_empty(), roll: 0, ..Default::default() };
        if m.brain.decide(&facts).is_some() && m.kind != Kind::EnderDragon {
            m.path.clear();
        }
        m.panic = m.panic.saturating_sub(1);
        let fleeing = m.brain.doing == crate::ai::Behavior::Panic;
        if m.brain.doing != crate::ai::Behavior::Attack {
            target = None;
        }
        let goal = if let Some(p) = &target {
            let aim = (p.pos.0, p.pos.1 - 0.6, p.pos.2);
            let d2 = dist2(aim, m.pos);
            m.head_yaw = (-(p.pos.0 - m.pos.0)).atan2(p.pos.2 - m.pos.2).to_degrees();
            if m.kind.spec().ranged.is_some() {
                if d2 < 20.0 * 20.0 && m.attack_cd == 0 {
                    m.attack_cd = 3 * TPS;
                    self.shoot(server, m, aim, out);
                }
                // Hover about 8 blocks off, a little above.
                let back = ((m.pos.0 - aim.0), (m.pos.2 - aim.2));
                let len = (back.0 * back.0 + back.1 * back.1).sqrt().max(0.01);
                (aim.0 + back.0 / len * 8.0, aim.1 + 4.0, aim.2 + back.1 / len * 8.0)
            } else {
                let hit_reach = if m.kind == Kind::EnderDragon { 6.0 } else { 1.8 };
                if d2 < hit_reach * hit_reach && m.attack_cd == 0 {
                    m.attack_cd = if m.kind == Kind::EnderDragon { 12 * TPS } else { MELEE_COOLDOWN };
                    let _ = server.events.send(Event::Damage {
                        to: p.id,
                        damage: crate::health::Damage::Mob(m.kind.melee() * difficulty_scale(), m.kind.display()),
                        knock: ((aim.0 - m.pos.0) * 0.3, 0.2, (aim.2 - m.pos.2) * 0.3),
                    });
                }
                aim
            }
        } else {
            match m.path.first() {
                Some(&(x, y, z)) if dist2((x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5), m.pos) > 1.0 && !self.rand().is_multiple_of(60) => (x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5),
                _ => {
                    // Fleeing picks points twice as far off.
                    let r: i32 = if fleeing { 14 } else { 7 };
                    let g = (feet.0 + (self.rand() % (2 * r + 1) as u32) as i32 - r, feet.1 + (self.rand() % 7) as i32 - 3, feet.2 + (self.rand() % (2 * r + 1) as u32) as i32 - r);
                    m.path = vec![g];
                    (g.0 as f32 + 0.5, g.1 as f32 + 0.5, g.2 as f32 + 0.5)
                }
            }
        };
        let (dx, dy, dz) = (goal.0 - m.pos.0, goal.1 - m.pos.1, goal.2 - m.pos.2);
        let len = (dx * dx + dy * dy + dz * dz).sqrt().max(0.01);
        let sp = m.kind.speed() * m.brain.doing.speed();
        m.vel = (m.vel.0 * 0.7 + dx / len * sp * 0.3, m.vel.1 * 0.7 + dy / len * sp * 0.3, m.vel.2 * 0.7 + dz / len * sp * 0.3);
        m.yaw = (-m.vel.0).atan2(m.vel.2).to_degrees();
        if target.is_none() {
            m.head_yaw = m.yaw;
        }
    }

    /// Flying movement: no gravity; bump off blocks.
    fn fly_physics(&self, server: &Server, m: &mut Mob) {
        let next = (m.pos.0 + m.vel.0, m.pos.1 + m.vel.1, m.pos.2 + m.vel.2);
        let h = m.kind.size().1;
        let blocked = [0.0, h * 0.9].iter().any(|dy| solid(server.world.get(next.0.floor() as i32, (next.1 + dy).floor() as i32, next.2.floor() as i32)));
        let (lo, hi) = crate::dims::range(crate::dims::of_f(m.pos.1));
        if blocked || !((lo + 1) as f32..(hi - 1) as f32).contains(&next.1) {
            m.vel = (-m.vel.0 * 0.5, 0.05, -m.vel.2 * 0.5);
            m.path.clear();
        } else {
            m.pos = next;
        }
        m.on_ground = false;
    }

    /// Guardians zap players in reach every 3 s (their laser, without the charge-up beam).
    fn laser(&self, server: &Server, m: &mut Mob) {
        m.attack_cd = m.attack_cd.saturating_sub(1);
        if m.attack_cd > 0 {
            return;
        }
        let targets = self.targetable.lock().clone();
        if let Some(p) = server.all().into_iter().find(|p| targets.contains(&p.id) && dist2(p.pos, m.pos) < 14.0 * 14.0) {
            m.attack_cd = 3 * TPS;
            let _ = server.events.send(Event::Damage { to: p.id, damage: crate::health::Damage::Mob(m.kind.melee() * difficulty_scale(), m.kind.display()), knock: (0.0, 0.0, 0.0) });
            if let Some((effect, secs)) = m.kind.spec().hit_effect {
                let _ = server.events.send(Event::Effect { to: p.id, effect, amplifier: 2, ticks: secs * 20 });
            }
        }
    }

    /// Nearest other living mob within `range` matching `f`: (id, position).
    fn nearest(&self, m: &Mob, range: f32, f: impl Fn(&Mob) -> bool) -> Option<(i64, (f32, f32, f32))> {
        self.mobs
            .lock()
            .values()
            .filter(|o| o.id != m.id && o.dying == 0 && f(o))
            .map(|o| (o.id, o.pos, dist2(o.pos, m.pos)))
            .filter(|o| o.2 < range * range)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(id, p, _)| (id, p))
    }

    /// Wandering trader visits (vanilla Bedrock): every 20 minutes there's a chance (25%, rising
    /// by 25% each time it fails, up to 75%) one turns up near a random overworld player with
    /// two trader llamas, and leaves 40 minutes later.
    fn tick_wandering_trader(&self, server: &Server) {
        static CLOCK: Mutex<(u32, u32)> = Mutex::new((0, 25));
        let roll = {
            let mut c = CLOCK.lock();
            c.0 += 1;
            if c.0 < 20 * 60 * TPS {
                return;
            }
            c.0 = 0;
            let chance = c.1;
            let hit = self.rand() % 100 < chance;
            c.1 = if hit { 25 } else { (chance + 25).min(75) };
            hit
        };
        let players: Vec<_> = server.all().into_iter().filter(|p| crate::dims::of_f(p.pos.1) == crate::dims::OVERWORLD).collect();
        if !roll || players.is_empty() || self.mobs.lock().values().any(|m| m.kind == Kind::WanderingTrader) {
            return;
        }
        let p = &players[(self.rand() as usize) % players.len()];
        let a = self.randf() * std::f32::consts::TAU;
        let (x, z) = ((p.pos.0 + a.cos() * 24.0).floor() as i32, (p.pos.2 + a.sin() * 24.0).floor() as i32);
        let w = &server.world;
        let mut y = (p.pos.1 as i32 + 20).min(318);
        while y > -63 && !blocks::is_full(w.get(x, y - 1, z)) {
            y -= 1;
        }
        if !blocks::is_air(w.get(x, y, z)) {
            return;
        }
        let life = 40 * 60 * TPS;
        let at = (x as f32 + 0.5, y as f32, z as f32 + 0.5);
        for (kind, off) in [(Kind::WanderingTrader, 0.0), (Kind::TraderLlama, 1.5), (Kind::TraderLlama, -1.5)] {
            let id = self.spawn(server, kind, (at.0 + off, at.1, at.2));
            if let Some(m) = self.mobs.lock().get_mut(&id) {
                m.lifetime = life;
            }
        }
    }

    /// A leashed mob keeps within the lead's reach: it walks toward the holder past 3 blocks,
    /// is pulled past 6, and the lead snaps past 10 (dropping it, vanilla).
    fn follow_lead(&self, server: &Server, m: &mut Mob, holder: i64, out: &mut Vec<Protocol>) {
        let at = server.get(holder).map(|p| (p.pos.0, p.pos.1 - 1.62, p.pos.2)).or_else(|| self.mobs.lock().get(&holder).filter(|o| o.dying == 0).map(|o| o.pos));
        let player = server.get(holder).is_some();
        let snap = |m: &mut Mob, out: &mut Vec<Protocol>| {
            m.leash = None;
            out.push(data_packet(m));
            if player && let Some(lead) = crate::items::item_by_name("minecraft:lead") {
                server.entities.drop_item(server, lead, 1, m.pos);
            }
        };
        let Some(at) = at else {
            snap(m, out);
            return;
        };
        let (dx, dy, dz) = (at.0 - m.pos.0, at.1 - m.pos.1, at.2 - m.pos.2);
        let d = (dx * dx + dy * dy + dz * dz).sqrt();
        if d > 10.0 {
            snap(m, out);
        } else if d > 3.0 {
            let feet = (m.pos.0.floor() as i32, m.pos.1.floor() as i32, m.pos.2.floor() as i32);
            if m.path.is_empty() || self.rand().is_multiple_of(5) {
                m.path = path(&server.world, feet, (at.0.floor() as i32, at.1.floor() as i32, at.2.floor() as i32), 64);
            }
            if d > 6.0 {
                let pull = 0.08 * (d - 6.0);
                m.vel = (m.vel.0 + dx / d * pull, m.vel.1 + (dy / d * pull).max(0.0), m.vel.2 + dz / d * pull);
            }
        }
    }

    /// Snow golem (PNX EntitySnowGolem): leaves snow where it walks outside the Nether, and
    /// every 2 seconds throws a snowball at the nearest hostile within 10 blocks.
    fn snow_golem(&self, server: &Server, m: &mut Mob) {
        let w = &server.world;
        let feet = (m.pos.0.floor() as i32, m.pos.1.floor() as i32, m.pos.2.floor() as i32);
        if crate::dims::of(feet.1) == crate::dims::OVERWORLD
            && blocks::is_air(w.get(feet.0, feet.1, feet.2))
            && blocks::is_full(w.get(feet.0, feet.1 - 1, feet.2))
            && let Some(snow) = crate::blocks::any_state("minecraft:snow_layer")
        {
            server.set_block_quiet(0, feet.0, feet.1, feet.2, snow);
        }
        m.attack_cd = m.attack_cd.saturating_sub(1);
        if m.attack_cd > 0 {
            return;
        }
        if let Some((_, at)) = self.nearest(m, 10.0, |o| o.kind.hostile() && o.kind != Kind::Creeper) {
            m.attack_cd = 2 * TPS;
            let eye = (m.pos.0, m.pos.1 + 1.7, m.pos.2);
            let (dx, dy, dz) = (at.0 - eye.0, at.1 + 1.0 - eye.1, at.2 - eye.2);
            let h = (dx * dx + dz * dz).sqrt();
            // Aim a bit high for the drop, like the golem does.
            let pitch = -(dy + h * 0.2).atan2(h).to_degrees();
            let yaw = (-dx).atan2(dz).to_degrees();
            m.yaw = yaw;
            m.head_yaw = yaw;
            throw(server, Throw::Snowball, m.id, eye, (pitch, yaw));
        }
    }

    /// Walks to mob `prey` and hits it when in reach.
    fn chase_and_hit(&self, server: &Server, m: &mut Mob, prey: i64, at: (f32, f32, f32)) {
        let feet = (m.pos.0.floor() as i32, m.pos.1.floor() as i32, m.pos.2.floor() as i32);
        let reach = 1.6 + m.kind.size().0 / 2.0;
        if dist2(at, m.pos) < reach * reach {
            m.path.clear();
            m.yaw = (-(at.0 - m.pos.0)).atan2(at.2 - m.pos.2).to_degrees();
            if m.attack_cd == 0 {
                m.attack_cd = MELEE_COOLDOWN;
                self.hurt(server, prey, m.kind.melee(), m.pos, None);
            }
        } else if m.path.is_empty() || self.rand().is_multiple_of(8) {
            m.path = path(&server.world, feet, (at.0.floor() as i32, at.1.floor() as i32, at.2.floor() as i32), 160);
        }
        self.steer(m);
    }

    /// Golems guard (hostile mobs nearby, or a player who attacked a villager), wolves and
    /// cats follow and defend their owner. Untamed wolves bite back when hit.
    fn defend(&self, server: &Server, m: &mut Mob) {
        use crate::ai::Behavior as B;
        m.attack_cd = m.attack_cd.saturating_sub(1);
        // Sense: prey still alive and close, a golem spotting hostiles, an angry-at player, the owner.
        let prey = m.prey.and_then(|id| self.mobs.lock().get(&id).filter(|o| o.dying == 0).map(|o| (id, o.pos))).filter(|(_, at)| dist2(*at, m.pos) < 24.0 * 24.0);
        m.prey = prey.map(|p| p.0);
        if m.prey.is_none() && m.kind == Kind::IronGolem && self.rand().is_multiple_of(10) {
            m.prey = self.nearest(m, 16.0, |o| o.kind.hostile() && o.kind != Kind::Creeper).map(|(id, _)| id);
        }
        let prey = prey.or_else(|| m.prey.and_then(|id| self.mobs.lock().get(&id).map(|o| (id, o.pos))));
        let angry = m.aggro.and_then(|pid| server.get(pid)).filter(|p| self.targetable.lock().contains(&p.id));
        if angry.is_none() {
            m.aggro = None;
        }
        let owner = m.owner.filter(|_| !m.kind.rideable()).and_then(|o| server.get(o));
        let pos = m.pos;
        let mem = &mut m.brain.memory;
        mem.prey = prey;
        mem.target = angry.as_ref().map(|p| (p.id, (p.pos.0, p.pos.1 - 1.62, p.pos.2), dist2((p.pos.0, p.pos.1 - 1.62, p.pos.2), pos)));
        mem.owner_at = owner.as_ref().map(|o| ((o.pos.0, o.pos.1 - 1.62, o.pos.2), dist2((o.pos.0, o.pos.1 - 1.62, o.pos.2), pos)));
        let facts = crate::ai::Facts { sitting: m.sitting, has_path: !m.path.is_empty(), roll: self.rand() % 1000, ..Default::default() };
        if m.brain.decide(&facts).is_some() {
            m.path.clear();
        }
        let feet = (m.pos.0.floor() as i32, m.pos.1.floor() as i32, m.pos.2.floor() as i32);
        match m.brain.doing {
            B::Sit => {
                m.path.clear();
                m.vel.0 = 0.0;
                m.vel.2 = 0.0;
            }
            B::Attack => {
                if let Some((id, at)) = m.brain.memory.prey {
                    return self.chase_and_hit(server, m, id, at);
                }
                let Some((pid, feet_p, d2)) = m.brain.memory.target else { return };
                let reach = 1.6 + m.kind.size().0 / 2.0;
                if d2 < reach * reach {
                    m.path.clear();
                    if m.attack_cd == 0 {
                        m.attack_cd = MELEE_COOLDOWN;
                        let (dx, dz) = (feet_p.0 - m.pos.0, feet_p.2 - m.pos.2);
                        let len = (dx * dx + dz * dz).sqrt().max(0.01);
                        let fling = if m.kind == Kind::IronGolem { 0.6 } else { 0.3 };
                        let _ = server.events.send(Event::Damage {
                            to: pid,
                            damage: crate::health::Damage::Mob(m.kind.melee() * difficulty_scale().max(0.5), m.kind.display()),
                            knock: (dx / len * 0.4, fling, dz / len * 0.4),
                        });
                        let mut fx = Vec::new();
                        self.thorns(m, pid, &mut fx);
                        for f in fx {
                            let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(f) });
                        }
                    }
                } else if m.path.is_empty() || self.rand().is_multiple_of(8) {
                    m.path = path(&server.world, feet, (feet_p.0.floor() as i32, feet_p.1.floor() as i32, feet_p.2.floor() as i32), 160);
                }
                self.steer(m);
            }
            B::FollowOwner => {
                let Some((feet_o, d2)) = m.brain.memory.owner_at else { return };
                if d2 > 20.0 * 20.0 {
                    // Left far behind: appear next to the owner.
                    m.pos = (feet_o.0 + 1.0, feet_o.1, feet_o.2 + 1.0);
                    m.path.clear();
                } else {
                    if m.path.is_empty() || self.rand().is_multiple_of(10) {
                        m.path = path(&server.world, feet, (feet_o.0.floor() as i32, feet_o.1.floor() as i32, feet_o.2.floor() as i32), 160);
                    }
                    self.steer(m);
                }
            }
            _ => match m.brain.memory.owner_at {
                // Near the owner: stay by them, looking their way.
                Some((o, _)) => {
                    m.path.clear();
                    m.head_yaw = (-(o.0 - m.pos.0)).atan2(o.2 - m.pos.2).to_degrees();
                }
                None => self.wander(server, m),
            },
        }
    }

    /// Villagers wander and run from zombies.
    // TODO: schedules
    fn villager(&self, server: &Server, m: &mut Mob, out: &mut Vec<Protocol>) {
        // Breeding (vanilla): two adults close together make a baby when the village has a
        // spare bed (more beds within 16 blocks than villagers), then wait 5 minutes.
        if m.baby == 0 && m.cooldown == 0 && self.rand().is_multiple_of(600)
            && let Some((partner, _)) = self.nearest(m, 6.0, |o| o.kind == Kind::Villager && o.baby == 0 && o.cooldown == 0)
        {
            let w = &server.world;
            let c = (m.pos.0.floor() as i32, m.pos.1.floor() as i32, m.pos.2.floor() as i32);
            let mut beds = 0;
            for x in c.0 - 16..=c.0 + 16 {
                for z in c.2 - 16..=c.2 + 16 {
                    for y in c.1 - 4..=c.1 + 4 {
                        let b = w.get(x, y, z);
                        if crate::blocks::entry(b).is_some_and(|e| e.name == "minecraft:bed") && matches!(crate::blocks::state(b, "head_piece_bit"), Some(crate::blocks::State::Bool(true))) {
                            beds += 1;
                        }
                    }
                }
            }
            let villagers = self.mobs.lock().values().filter(|o| o.kind == Kind::Villager && dist2(o.pos, m.pos) < 32.0 * 32.0).count();
            if beds > villagers {
                m.cooldown = 5 * 60 * TPS;
                if let Some(o) = self.mobs.lock().get_mut(&partner) {
                    o.cooldown = 5 * 60 * TPS;
                }
                out.push(event(m.id, bedrock::protocol::v975::enums::ActorEvent::LoveHearts));
                self.spawn_with(server, Kind::Villager, m.pos, true);
            }
        }
        // Every few seconds an adult without a job looks for a job block (PNX WorkExecutor).
        if m.baby == 0 && self.rand().is_multiple_of(50) {
            let feet = (m.pos.0.floor() as i32, m.pos.1.floor() as i32, m.pos.2.floor() as i32);
            let lost = m.job.as_ref().and_then(|j| j.site).is_some_and(|s| crate::villagers::find_job(&server.world, m.id, s).is_none_or(|(p, _)| p != s));
            if lost && m.job.as_ref().is_some_and(|j| j.exp == 0) {
                // Its job block is gone and it never traded: back to unemployed.
                m.job = None;
                crate::villagers::release(m.id);
                out.push(data_packet(m));
            }
            if m.job.is_none()
                && let Some((site, profession)) = crate::villagers::find_job(&server.world, m.id, feet)
            {
                let biome = server.world.biome_at(feet.0, feet.1, feet.2).unwrap_or(1);
                m.job = Some(Box::new(crate::villagers::Job {
                    profession,
                    tier: 1,
                    exp: 0,
                    seed: self.rand() as i32,
                    clothing: crate::trades::Clothing::from_biome(biome),
                    uses: vec![],
                    site: Some(site),
                    // A villager cured by a player remembers it (PNX purifyPlayer).
                    gossip: CURED_BY.lock().remove(&m.id).map(|p| {
                        let mut j = crate::villagers::Job { profession: 0, tier: 1, exp: 0, seed: 0, clothing: crate::trades::Clothing::from_index(0), uses: vec![], site: None, gossip: vec![] };
                        j.add_gossip(&p, crate::villagers::Gossip::MajorPositive, 20);
                        j.add_gossip(&p, crate::villagers::Gossip::MinorPositive, 25);
                        j.gossip
                    }).unwrap_or_default(),
                }));
                out.push(data_packet(m));
                out.push(event(m.id, bedrock::protocol::v975::enums::ActorEvent::VillagerHappy));
            }
        }
        // Restocks a couple of times a day (vanilla: twice per day at its job site).
        if let Some(j) = m.job.as_mut()
            && !j.uses.is_empty()
            && self.rand().is_multiple_of(6000)
        {
            j.uses.clear();
        }
        // What the brain needs on top of the usual: zombies to run from, the job block by day,
        // and whether it's caught outside at night.
        let feet = (m.pos.0.floor() as i32, m.pos.1.floor() as i32, m.pos.2.floor() as i32);
        let t = server.time().rem_euclid(24_000);
        let night = (12_500..23_500).contains(&t);
        m.brain.memory.threat = self.nearest(m, 8.0, |o| matches!(o.kind, Kind::Zombie | Kind::Drowned | Kind::Husk | Kind::ZombieVillager)).map(|(_, p)| p);
        m.brain.memory.work = m.job.as_ref().and_then(|j| j.site).filter(|s| !night && dist2(m.pos, (s.0 as f32 + 0.5, s.1 as f32, s.2 as f32 + 0.5)) > 9.0);
        m.brain.memory.wants_cover = night && m.baby == 0 && sky(&server.world, feet);
        let _ = self.think(server, m);
    }

    /// Fish and squid: drift to random spots in the water; flop and suffocate on land.
    fn swim(&self, server: &Server, m: &mut Mob, out: &mut Vec<Protocol>) {
        let w = &server.world;
        let feet = (m.pos.0.floor() as i32, m.pos.1.floor() as i32, m.pos.2.floor() as i32);
        if !blocks::is_water(w.get(feet.0, feet.1, feet.2)) {
            if self.rand().is_multiple_of(TPS) {
                m.health -= 1.0;
                use bedrock::protocol::v975::enums::ActorEvent;
                out.push(event(m.id, ActorEvent::Hurt));
            }
            return;
        }
        // A player within 4 blocks: dart the other way (vanilla fish flee goal).
        if m.panic == 0
            && let Some(p) = server.all().into_iter().find(|p| dist2((p.pos.0, p.pos.1 - 1.0, p.pos.2), m.pos) < 16.0)
        {
            let (dx, dz) = (m.pos.0 - p.pos.0, m.pos.2 - p.pos.2);
            let len = (dx * dx + dz * dz).sqrt().max(0.01);
            m.target = Some((m.pos.0 + dx / len * 6.0, m.pos.2 + dz / len * 6.0));
            m.panic = 2 * TPS;
        }
        let near_target = m.target.is_some_and(|(tx, tz)| (tx - m.pos.0).powi(2) + (tz - m.pos.2).powi(2) < 1.0);
        if m.target.is_none() || near_target || self.rand().is_multiple_of(60) {
            // A water block within a few blocks.
            for _ in 0..6 {
                let (dx, dy, dz) = ((self.rand() % 9) as i32 - 4, (self.rand() % 5) as i32 - 2, (self.rand() % 9) as i32 - 4);
                if blocks::is_water(w.get(feet.0 + dx, feet.1 + dy, feet.2 + dz)) {
                    m.target = Some(((feet.0 + dx) as f32 + 0.5, (feet.2 + dz) as f32 + 0.5));
                    m.vel.1 = dy as f32 * 0.03;
                    break;
                }
            }
        }
        if let Some((tx, tz)) = m.target {
            let (dx, dz) = (tx - m.pos.0, tz - m.pos.2);
            let d = (dx * dx + dz * dz).sqrt().max(0.001);
            let s = m.kind.speed() * if m.panic > 0 { 2.0 } else { 1.0 };
            m.vel.0 = m.vel.0 * 0.7 + dx / d * s * 0.3;
            m.vel.2 = m.vel.2 * 0.7 + dz / d * s * 0.3;
            m.yaw = (-dx).atan2(dz).to_degrees();
            m.head_yaw = m.yaw;
        }
        m.panic = m.panic.saturating_sub(1);
    }

    /// Water movement: no gravity in water, stays inside water blocks; falls on land.
    fn swim_physics(&self, server: &Server, m: &mut Mob) {
        let w = &server.world;
        let next = (m.pos.0 + m.vel.0, m.pos.1 + m.vel.1, m.pos.2 + m.vel.2);
        let here = (m.pos.0.floor() as i32, m.pos.1.floor() as i32, m.pos.2.floor() as i32);
        if blocks::is_water(w.get(here.0, here.1, here.2)) {
            if blocks::is_water(w.get(next.0.floor() as i32, next.1.floor() as i32, next.2.floor() as i32)) {
                m.pos = next;
            } else {
                m.vel = (-m.vel.0 * 0.5, 0.0, -m.vel.2 * 0.5);
                m.target = None;
            }
            m.vel.1 *= 0.9;
        } else {
            self.physics(server, m);
        }
    }

    /// Idle hostiles drift about.
    fn wander(&self, server: &Server, m: &mut Mob) {
        let feet = (m.pos.0.floor() as i32, m.pos.1.floor() as i32, m.pos.2.floor() as i32);
        if m.path.is_empty() && self.rand().is_multiple_of(100) {
            let a = self.randf() * std::f32::consts::TAU;
            let r = 3.0 + self.randf() * 7.0;
            m.path = path(&server.world, feet, (feet.0 + (a.cos() * r) as i32, feet.1, feet.2 + (a.sin() * r) as i32), 120);
        }
        self.steer(m);
    }

    /// Walks along the current path.
    fn steer(&self, m: &mut Mob) {
        while let Some(&(wx, _, wz)) = m.path.first() {
            let (dx, dz) = (wx as f32 + 0.5 - m.pos.0, wz as f32 + 0.5 - m.pos.2);
            if dx * dx + dz * dz < 0.15 {
                m.path.remove(0);
            } else {
                break;
            }
        }
        m.target = m.path.first().map(|&(x, _, z)| (x as f32 + 0.5, z as f32 + 0.5));
        if let Some((tx, tz)) = m.target {
            let (dx, dz) = (tx - m.pos.0, tz - m.pos.2);
            let d = (dx * dx + dz * dz).sqrt().max(0.001);
            if m.on_ground {
                let speed = m.kind.speed();
                m.vel.0 = m.vel.0 * 0.5 + dx / d * speed * 0.5;
                m.vel.2 = m.vel.2 * 0.5 + dz / d * speed * 0.5;
            }
            m.yaw = (-dx).atan2(dz).to_degrees();
        }
    }

    /// Thorns on the player `m` just hit: the damage comes straight back at the mob.
    fn thorns(&self, m: &mut Mob, player: i64, out: &mut Vec<Protocol>) {
        let armor: Vec<Vec<u8>> = crate::players::EQUIPMENT.lock().as_ref().and_then(|e| e.get(&player)).map(|e| e.armor.iter().flatten().map(|s| s.item.user_data_buffer.clone()).collect()).unwrap_or_default();
        let refs: Vec<&[u8]> = armor.iter().map(|a| a.as_slice()).collect();
        let d = crate::enchant::thorns(&refs, || self.randf());
        if d > 0.0 {
            use bedrock::protocol::v975::enums::ActorEvent;
            m.health -= d;
            out.push(event(m.id, ActorEvent::Hurt));
        }
    }

    /// Mobs vanish far from players (instantly past 128 blocks, now and then past 32), unless
    /// something made them persistent: tamed, named, bred, saddled, ridden, a villager...
    fn despawn(&self, server: &Server, m: &Mob) -> bool {
        if m.owner.is_some() || m.kept || m.saddled || m.temper > 0 || m.rider.is_some() || m.home.is_some() || matches!(m.kind, Kind::Villager | Kind::IronGolem | Kind::EnderDragon | Kind::EndCrystal | Kind::Shulker | Kind::Wither) {
            return false;
        }
        let players = server.all();
        if players.is_empty() {
            return *crate::commands::DIFFICULTY.lock() == 0; // nobody online: keep them
        }
        let nearest = players.iter().map(|p| dist2(p.pos, m.pos)).fold(f32::MAX, f32::min);
        let gone = nearest > 128.0 * 128.0 || (nearest > 32.0 * 32.0 && self.rand().is_multiple_of(800)) || *crate::commands::DIFFICULTY.lock() == 0;
        if gone {
            crate::plugin::later(crate::plugin::Later::Despawn(m.id, m.kind.identifier()));
        }
        gone
    }

    fn shoot(&self, server: &Server, m: &Mob, at: (f32, f32, f32), out: &mut Vec<Protocol>) {
        crate::plugin::later(crate::plugin::Later::MobShoot(m.id, m.kind.identifier()));
        let from = (m.pos.0, m.pos.1 + 1.5, m.pos.2);
        let (dx, dy, dz) = (at.0 - from.0, at.1 - from.1, at.2 - from.2);
        let horiz = (dx * dx + dz * dz).sqrt().max(0.01);
        // ~1.6 blocks/tick, aimed a little high for the drop; a bit of spread.
        let speed = 1.6;
        let flight = horiz / speed;
        let lift = 0.5 * ARROW_GRAVITY * flight * flight;
        let len = (dx * dx + (dy + lift).powi(2) + dz * dz).sqrt().max(0.01);
        let spread = || (self.randf() - 0.5) * 0.06;
        let vel = (dx / len * speed + spread(), (dy + lift) / len * speed + spread(), dz / len * speed + spread());
        let id = crate::players::next_id();
        out.push(Protocol::AddActorPacket(Box::new(bedrock::protocol::v2225::packets::AddActorPacket {
            target_actor_id: ActorUniqueID(id),
            target_runtime_id: ActorRuntimeID(id as u64),
            actor_type: m.kind.spec().ranged.unwrap_or("minecraft:arrow").into(),
            position: from,
            velocity: vel,
            rotation: (0.0, 0.0),
            y_head_rotation: 0.0,
            y_body_rotation: 0.0,
            attributes: vec![],
            actor_data: vec![],
            synced_properties: PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
            actor_links: vec![],
            passenger_of_block_data: None,
        })));
        out.push(sound("bow", from));
        let _ = server;
        self.arrows.lock().push(Arrow { id, pos: from, vel, shooter: m.kind, life: 60 * TPS, stuck: false, by: None });
    }

    /// Thrown items: snowballs knock back, eggs hatch a chick 1 in 8 (PNX EntityEgg),
    /// pearls teleport the thrower and hurt them 5.
    fn tick_thrown(&self, server: &Server, out: &mut Vec<Protocol>) {
        let players = server.all();
        let mut hits = Vec::new();
        THROWN.lock().retain_mut(|t| {
            t.life = t.life.saturating_sub(1);
            if t.life == 0 {
                out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(t.id) })));
                return false;
            }
            // Two game ticks per 10 Hz tick, each in 4 small steps.
            for _ in 0..8 {
                let next = (t.pos.0 + t.vel.0 / 4.0, t.pos.1 + t.vel.1 / 4.0, t.pos.2 + t.vel.2 / 4.0);
                let player = players.iter().find(|p| {
                    let (fx, fy, fz) = (p.pos.0, p.pos.1 - 1.62, p.pos.2);
                    p.id != t.owner && (next.0 - fx).abs() < 0.45 && (next.2 - fz).abs() < 0.45 && next.1 > fy && next.1 < fy + 1.85
                });
                let mob = self.mobs.lock().values().find(|m| {
                    if m.id == t.owner || m.dying > 0 {
                        return false; // not the snow golem that threw it
                    }
                    let (w, h) = m.kind.size();
                    (next.0 - m.pos.0).abs() < w / 2.0 + 0.15 && (next.2 - m.pos.2).abs() < w / 2.0 + 0.15 && next.1 > m.pos.1 && next.1 < m.pos.1 + h
                }).map(|m| m.id);
                let block = !blocks::is_air(server.world.get(next.0.floor() as i32, next.1.floor() as i32, next.2.floor() as i32))
                    && !blocks::is_liquid(server.world.get(next.0.floor() as i32, next.1.floor() as i32, next.2.floor() as i32));
                if player.is_some() || mob.is_some() || block {
                    hits.push((t.kind, t.owner, t.pos, t.vel, player.map(|p| p.id), mob));
                    out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(t.id) })));
                    return false;
                }
                t.pos = next;
            }
            for _ in 0..2 {
                t.vel.1 -= 0.03;
                t.vel = (t.vel.0 * 0.99, t.vel.1 * 0.99, t.vel.2 * 0.99);
            }
            out.push(Protocol::MoveActorAbsolutePacket(Box::new(bedrock::protocol::v662::packets::MoveActorAbsolutePacket {
                move_data: bedrock::protocol::v662::types::MoveActorAbsoluteData {
                    actor_runtime_id: ActorRuntimeID(t.id as u64),
                    header: 0,
                    position: t.pos,
                    rotation_x: 0,
                    rotation_y: 0,
                    rotation_y_head: 0,
                },
            })));
            true
        });
        for (kind, owner, pos, vel, player, mob) in hits {
            let knock = (vel.0 * 0.2, 0.2, vel.2 * 0.2);
            if let Some(p) = player {
                let _ = server.events.send(Event::Damage { to: p, damage: crate::health::Damage::Shot(0.0, "Snowball"), knock });
            }
            if let Some(m) = mob {
                // Snowballs only sting blazes (3 damage), vanilla.
                let dmg = if kind == Throw::Snowball && self.kind_of(m) == Some(Kind::Blaze) { 3.0 } else { 0.0 };
                self.hurt(server, m, dmg, pos, Some(owner));
            }
            match kind {
                Throw::Snowball => {}
                Throw::XpBottle => {
                    // PNX EntityExpBottle: 3 + 0..8 points, the splash in its blue-yellow colour.
                    let splash = Protocol::LevelEventPacket(Box::new(bedrock::protocol::v662::packets::LevelEventPacket { event_id: 2002, position: pos, data: 0x00385dc6 }));
                    let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(splash) });
                    let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(sound("glass", pos)) });
                    server.entities.drop_xp(server, pos, 3 + (self.rand() % 9) as i32);
                }
                Throw::Potion(aux) => {
                    if crate::effects::potion(aux).iter().any(|e| e.0 == crate::effects::WEAKNESS) {
                        for m in self.mobs.lock().values_mut() {
                            if m.kind == Kind::ZombieVillager && dist2(m.pos, pos) < 16.0 {
                                m.weak = 90 * TPS;
                            }
                        }
                    }
                    // PNX EntityPotion: everyone within 4 blocks gets the effects, weaker farther out.
                    let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(sound("glass", pos)) });
                    for pl in server.all() {
                        let d = dist2((pl.pos.0, pl.pos.1 - 1.0, pl.pos.2), pos).sqrt();
                        if d > 4.0 {
                            continue;
                        }
                        let scale = 1.0 - d / 4.0;
                        for (effect, amplifier, ticks) in crate::effects::potion(aux) {
                            let ticks = if ticks <= 1 { 1 } else { ((ticks as f32) * 0.75 * scale) as i32 };
                            let _ = server.events.send(Event::Effect { to: pl.id, effect, amplifier, ticks: ticks.max(1) });
                        }
                    }
                }
                Throw::Egg => {
                    if self.rand().is_multiple_of(8) {
                        self.spawn_with(server, Kind::Chicken, (pos.0, pos.1.floor(), pos.2), true);
                    }
                }
                Throw::Pearl => {
                    let _ = server.events.send(Event::Teleport { id: owner, feet: (pos.0, pos.1.floor(), pos.2) }); // last open block before the hit
                    let _ = server.events.send(Event::Damage { to: owner, damage: crate::health::Damage::Fall(5.0), knock: (0.0, 0.0, 0.0) });
                    let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(sound("mob.endermen.portal", pos)) });
                }
            }
        }
    }

    /// Bow/trident release (PNX ItemBow/ItemTrident). Arrows do 3 blocks a game tick at full draw.
    #[allow(clippy::too_many_arguments)]
    pub fn player_shoot(&self, server: &Server, owner: i64, eye: (f32, f32, f32), rot: (f32, f32), force: f32, trident: bool, power: i16, punch: i16, flame: bool, drop: Option<bedrock::protocol::v2168::types::NetworkItemStackDescriptorV2>, weapon: &[u8]) {
        if let Some(p) = server.get(owner) {
            server.plugins.projectile_launch(server, &p, trident, force);
        }
        let (pitch, yaw) = (rot.0.to_radians(), rot.1.to_radians());
        let dir = (-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos());
        let speed = if trident { 5.0 } else { 6.0 * force }; // per 10 Hz tick
        let vel = (dir.0 * speed, dir.1 * speed, dir.2 * speed);
        let from = (eye.0 + dir.0 * 0.5, eye.1 - 0.1, eye.2 + dir.2 * 0.5);
        let id = crate::players::next_id();
        broadcast(server, Protocol::AddActorPacket(Box::new(bedrock::protocol::v2225::packets::AddActorPacket {
            target_actor_id: ActorUniqueID(id),
            target_runtime_id: ActorRuntimeID(id as u64),
            actor_type: if trident { "minecraft:thrown_trident" } else { "minecraft:arrow" }.into(),
            position: from,
            velocity: (vel.0 / 2.0, vel.1 / 2.0, vel.2 / 2.0),
            // Pointing along its flight from the start (PNX EntityProjectile.updateRotation).
            rotation: projectile_rotation(vel),
            y_head_rotation: 0.0,
            y_body_rotation: projectile_rotation(vel).1,
            attributes: vec![],
            actor_data: vec![],
            synced_properties: PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
            actor_links: vec![],
            passenger_of_block_data: None,
        })));
        broadcast(server, sound(if trident { "item.trident.throw" } else { "bow" }, from));
        let by = PlayerShot { owner, damage: trident.then_some(8.0), power, punch, flame, critical: !trident && force >= 1.0, drop, weapon: weapon.to_vec(), pierced: vec![], returning: false };
        self.arrows.lock().push(Arrow { id, pos: from, vel, shooter: Kind::Skeleton, life: 60 * TPS, stuck: false, by: Some(by) });
    }

    fn tick_arrows(&self, server: &Server, out: &mut Vec<Protocol>) {
        let players = server.all();
        let targets = self.targetable.lock().clone();
        let mut mob_hits: Vec<(i64, f32, (f32, f32, f32), PlayerShot)> = Vec::new();
        let mut drops = Vec::new();
        let mut strikes = Vec::new();
        // Ghast fireballs and wither skulls burst where they hit (power 1, vanilla).
        let mut blasts: Vec<((f32, f32, f32), Kind)> = Vec::new();
        let thunder = *crate::commands::WEATHER.lock() == crate::commands::Weather::Thunder;
        let boxes: Vec<(i64, (f32, f32, f32), (f32, f32))> = self.mobs.lock().values().filter(|m| m.dying == 0).map(|m| (m.id, m.pos, m.kind.size())).collect();
        let mut arrows = self.arrows.lock();
        arrows.retain_mut(|a| {
            a.life = a.life.saturating_sub(1);
            if a.life == 0 {
                out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(a.id) })));
                return false;
            }
            // Loyalty: fly straight back to the thrower, faster with each level, through blocks
            // (vanilla returns the same way), and land in their inventory.
            if let Some(by) = &a.by
                && by.returning
            {
                let Some(o) = server.get(by.owner) else {
                    out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(a.id) })));
                    return false;
                };
                let to = (o.pos.0, o.pos.1 - 0.5, o.pos.2);
                let (dx, dy, dz) = (to.0 - a.pos.0, to.1 - a.pos.1, to.2 - a.pos.2);
                let d = (dx * dx + dy * dy + dz * dz).sqrt();
                if d < 1.5 {
                    if let Some(item) = by.drop.clone() {
                        drops.push((item, (o.pos.0, o.pos.1 - 1.2, o.pos.2)));
                    }
                    out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(a.id) })));
                    return false;
                }
                let speed = 0.6 + 0.4 * crate::enchant::level(&by.weapon, crate::enchant::LOYALTY) as f32;
                a.vel = (dx / d * speed, dy / d * speed, dz / d * speed);
                a.pos = (a.pos.0 + a.vel.0, a.pos.1 + a.vel.1, a.pos.2 + a.vel.2);
                a.stuck = false;
                out.push(Protocol::MoveActorAbsolutePacket(Box::new(bedrock::protocol::v662::packets::MoveActorAbsolutePacket {
                    move_data: bedrock::protocol::v662::types::MoveActorAbsoluteData {
                        actor_runtime_id: ActorRuntimeID(a.id as u64),
                        header: 0,
                        position: a.pos,
                        rotation_x: angle(projectile_rotation(a.vel).0),
                        rotation_y: angle(projectile_rotation(a.vel).1),
                        rotation_y_head: 0,
                    },
                })));
                return true;
            }
            if a.stuck {
                // A landed player arrow or trident waits to be picked up by walking into it.
                if let Some(by) = &a.by
                    && let Some(d) = &by.drop
                    && let Some(p) = players.iter().find(|p| dist2((p.pos.0, p.pos.1 - 1.0, p.pos.2), a.pos) < 2.25)
                {
                    drops.push((d.clone(), (p.pos.0, p.pos.1 - 1.2, p.pos.2)));
                    out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(a.id) })));
                    return false;
                }
                return true;
            }
            // Move in small steps so fast arrows don't skip through blocks or players.
            for _ in 0..4 {
                let next = (a.pos.0 + a.vel.0 / 4.0, a.pos.1 + a.vel.1 / 4.0, a.pos.2 + a.vel.2 / 4.0);
                if let Some(mut by) = a.by.clone() {
                    let speed = (a.vel.0.powi(2) + a.vel.1.powi(2) + a.vel.2.powi(2)).sqrt() / 2.0; // per game tick
                    let dmg = by.damage.unwrap_or_else(|| arrow_damage(speed, by.power, by.critical.then(|| self.rand())));
                    let hit_mob = boxes.iter().find(|(mid, mp, (w, h))| !by.pierced.contains(mid) && (next.0 - mp.0).abs() < w / 2.0 + 0.1 && (next.2 - mp.2).abs() < w / 2.0 + 0.1 && next.1 > mp.1 && next.1 < mp.1 + h);
                    let hit_player = players.iter().find(|p| {
                        let (fx, fy, fz) = (p.pos.0, p.pos.1 - 1.62, p.pos.2);
                        p.id != by.owner && (next.0 - fx).abs() < 0.45 && (next.2 - fz).abs() < 0.45 && next.1 > fy && next.1 < fy + 1.85
                    });
                    let allowed = |target| server.get(by.owner).is_none_or(|shooter| server.plugins.projectile_hit(server, &shooter, target));
                    let hit_mob = hit_mob.filter(|(mid, ..)| allowed(crate::plugin::Target::Mob(*mid)));
                    let hit_player = hit_player.filter(|p| allowed(crate::plugin::Target::Player(p.id)));
                    if hit_mob.is_some() || hit_player.is_some() {
                        // Channeling: a trident hit in a thunderstorm calls lightning down.
                        if thunder && by.damage.is_some() && crate::enchant::level(&by.weapon, crate::enchant::CHANNELING) > 0 {
                            strikes.push(next);
                        }
                        if let Some(&(mid, ..)) = hit_mob {
                            mob_hits.push((mid, dmg, a.pos, by.clone()));
                            // Piercing: the arrow goes on through up to `level` more mobs.
                            if by.damage.is_none() && (by.pierced.len() as i16) < crate::enchant::level(&by.weapon, crate::enchant::PIERCING) {
                                by.pierced.push(mid);
                                a.by = Some(by);
                                a.pos = next;
                                continue;
                            }
                        } else if let Some(p) = hit_player && targets.contains(&p.id) {
                            let _ = server.events.send(Event::Damage { to: p.id, damage: crate::health::Damage::Shot(dmg, "a player"), knock: (a.vel.0 * 0.1, 0.2, a.vel.2 * 0.1) });
                            if by.flame {
                                let _ = server.events.send(Event::Ignite { to: p.id, secs: 5 });
                            }
                        }
                        // Tridents bounce off what they hit and fall (PNX EntityThrownTrident:
                        // motion x -0.01, y -0.1), to be picked up where they land; Loyalty brings
                        // them straight back. Arrows that hit something are gone.
                        if by.damage.is_some() && let Some(d) = by.drop.clone() {
                            if crate::enchant::level(&by.weapon, crate::enchant::LOYALTY) > 0 && server.get(by.owner).is_some() {
                                let _ = d;
                                by.returning = true;
                                by.damage = Some(0.0);
                                a.by = Some(by);
                                break;
                            } else {
                                if let Some(&(mid, ..)) = hit_mob {
                                    by.pierced.push(mid);
                                }
                                by.damage = Some(0.0);
                                a.by = Some(by);
                                a.vel = (a.vel.0 * -0.01, a.vel.1 * -0.1, a.vel.2 * -0.01);
                                break;
                            }
                        }
                        out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(a.id) })));
                        return false;
                    }
                    if solid(server.world.get(next.0.floor() as i32, next.1.floor() as i32, next.2.floor() as i32)) {
                        // Loyalty tridents come back; everything else sticks where it landed for a
                        // minute (PNX despawns arrows after 1200 ticks) until someone picks it up.
                        if by.damage.is_some() && crate::enchant::level(&by.weapon, crate::enchant::LOYALTY) > 0 && server.get(by.owner).is_some() {
                            by.returning = true;
                            a.by = Some(by);
                            break;
                        }
                        a.by = Some(by);
                        a.stuck = true;
                        a.life = a.life.max(60 * TPS);
                        a.vel = (0.0, 0.0, 0.0);
                        break;
                    }
                    a.pos = next;
                    continue;
                }
                if let Some(p) = players.iter().find(|p| {
                    let (fx, fy, fz) = (p.pos.0, p.pos.1 - 1.62, p.pos.2);
                    (next.0 - fx).abs() < 0.45 && (next.2 - fz).abs() < 0.45 && next.1 > fy && next.1 < fy + 1.85
                }) {
                    if targets.contains(&p.id) {
                        let speed = (a.vel.0.powi(2) + a.vel.1.powi(2) + a.vel.2.powi(2)).sqrt();
                        let dmg = (speed * 2.0).ceil().clamp(2.0, 5.0) * difficulty_scale();
                        // Fireballs and potions hit harder or poison (PNX projectile classes).
                        let dmg = match a.shooter {
                            Kind::Ghast => 6.0 * difficulty_scale(),
                            Kind::Blaze => 5.0 * difficulty_scale(),
                            Kind::Wither => 8.0 * difficulty_scale(),
                            Kind::Witch => 0.0,
                            _ => dmg,
                        };
                        let _ = server.events.send(Event::Damage {
                            to: p.id,
                            damage: crate::health::Damage::Shot(dmg, a.shooter.display()),
                            knock: (a.vel.0 * 0.2, 0.2, a.vel.2 * 0.2),
                        });
                        let effect = match a.shooter {
                            Kind::Stray => Some((2, 0, 600)),  // slowness 30 s
                            Kind::Bogged => Some((19, 0, 80)), // poison 4 s
                            Kind::Shulker => Some((24, 0, 200)), // levitation 10 s
                            Kind::Wither => Some((20, 1, 200)),   // wither II 10 s from its skulls
                            Kind::Witch => Some([(7, 0, 1), (19, 0, 900), (2, 0, 1800), (18, 0, 1800)][(self.rand() % 4) as usize]),
                            _ => None,
                        };
                        if let Some((effect, amplifier, ticks)) = effect {
                            let _ = server.events.send(Event::Effect { to: p.id, effect, amplifier, ticks });
                        }
                    }
                    if matches!(a.shooter, Kind::Ghast | Kind::Wither) {
                        blasts.push((next, a.shooter));
                    }
                    out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(a.id) })));
                    return false;
                }
                let hit = (next.0.floor() as i32, next.1.floor() as i32, next.2.floor() as i32);
                if solid(server.world.get(hit.0, hit.1, hit.2)) {
                    if crate::blocks::entry(server.world.get(hit.0, hit.1, hit.2)).is_some_and(|e| e.name == "minecraft:target") {
                        let rel = (next.0 - hit.0 as f32, next.1 - hit.1 as f32, next.2 - hit.2 as f32);
                        crate::redstone::hit_target(hit, rel, 20);
                    }
                    if matches!(a.shooter, Kind::Ghast | Kind::Wither) {
                        blasts.push((a.pos, a.shooter));
                        out.push(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(a.id) })));
                        return false;
                    }
                    a.stuck = true;
                    a.life = a.life.min(5 * TPS); // stays stuck a few seconds
                    a.vel = (0.0, 0.0, 0.0);
                    break;
                }
                a.pos = next;
            }
            a.vel.1 -= ARROW_GRAVITY;
            a.vel = (a.vel.0 * 0.9801, a.vel.1 * 0.9801, a.vel.2 * 0.9801); // 0.99 drag, twice
            out.push(Protocol::MoveActorAbsolutePacket(Box::new(bedrock::protocol::v662::packets::MoveActorAbsolutePacket {
                move_data: bedrock::protocol::v662::types::MoveActorAbsoluteData {
                    actor_runtime_id: ActorRuntimeID(a.id as u64),
                    header: 0,
                    position: a.pos,
                    rotation_x: angle(projectile_rotation(a.vel).0),
                    rotation_y: angle(projectile_rotation(a.vel).1),
                    rotation_y_head: 0,
                },
            })));
            true
        });
        drop(arrows);
        for (mid, dmg, from, by) in mob_hits {
            let hit = Hit { knockback: by.punch, fire: if by.flame { 50 } else { 0 }, looting: 0 };
            // Impaling: +2.5 a level against water mobs (tridents).
            // Below half health the wither's armor turns arrows away (vanilla second phase).
            if by.damage.is_none() && self.mobs.lock().get(&mid).is_some_and(|m| m.kind == Kind::Wither && m.health <= m.kind.max_health() / 2.0) {
                continue;
            }
            let impaling = if by.damage.is_some() && self.kind_of(mid).is_some_and(|k| k.aquatic()) { 2.5 * crate::enchant::level(&by.weapon, crate::enchant::IMPALING) as f32 } else { 0.0 };
            self.hurt_with(server, mid, dmg + impaling, from, Some(by.owner), hit);
        }
        for (at, by) in blasts {
            explode(server, at, 1.0, by.display());
        }
        for at in strikes {
            crate::ticks::strike(server, at);
        }
        for (item, at) in drops {
            server.entities.drop_item(server, item, 1, at);
        }
    }

    /// Animal AI: panic, follow food, find a partner, wander, look around. Returns a partner
    /// id when close enough to breed.
    /// What a passive mob notices, every few ticks (PNX NearestPlayerSensor,
    /// NearestFeedingPlayerSensor, the breeding and parent sensors).
    fn sense(&self, server: &Server, m: &mut Mob) {
        let feet_of = |p: &crate::players::PlayerInfo| (p.pos.0, p.pos.1 - 1.62, p.pos.2);
        let held = self.held.lock().clone();
        let mem = &mut m.brain.memory;
        let players = server.all();
        mem.nearest_player = players.iter().map(|p| (p.id, feet_of(p), dist2(feet_of(p), m.pos))).min_by(|a, b| a.2.total_cmp(&b.2));
        mem.feeder = players
            .iter()
            .filter(|p| held.get(&p.id).is_some_and(|i| m.kind.food(i)))
            .map(|p| (p.id, feet_of(p), dist2(feet_of(p), m.pos)))
            .filter(|p| p.2 < 64.0)
            .min_by(|a, b| a.2.total_cmp(&b.2));
        let mobs = self.mobs.lock();
        mem.mate = (m.love > 0)
            .then(|| {
                mobs.values()
                    .filter(|o| o.id != m.id && o.kind == m.kind && o.love > 0 && o.dying == 0)
                    .map(|o| (o.id, o.pos, dist2(o.pos, m.pos)))
                    .filter(|o| o.2 < 64.0)
                    .min_by(|a, b| a.2.total_cmp(&b.2))
            })
            .flatten();
        mem.parent = m.brain.parent_id.filter(|_| m.baby > 0).and_then(|p| mobs.get(&p)).filter(|p| p.dying == 0).map(|p| (p.id, p.pos, dist2(p.pos, m.pos)));
        drop(mobs);
        let below = server.world.get(m.pos.0.floor() as i32, m.pos.1.floor() as i32 - 1, m.pos.2.floor() as i32);
        m.brain.memory.on_grass = blocks::short(below) == "grass_block";
    }

    /// A spot within 8 blocks with something overhead (shade by day, shelter at night).
    fn find_cover(&self, w: &crate::world::World, feet: (i32, i32, i32)) -> Option<(i32, i32, i32)> {
        (0..40)
            .map(|_| (feet.0 + (self.rand() % 17) as i32 - 8, feet.1, feet.2 + (self.rand() % 17) as i32 - 8))
            .find(|&q| standable(w, q) && !sky(w, q))
    }

    /// Passive mob AI: sense, pick a behaviour (see `ai::Brain::decide`), act, steer.
    /// Returns a partner when two animals in love meet.
    fn think(&self, server: &Server, m: &mut Mob) -> Option<i64> {
        use crate::ai::{Behavior as B, Facts};
        let feet = (m.pos.0.floor() as i32, m.pos.1.floor() as i32, m.pos.2.floor() as i32);
        let block = |p: (f32, f32, f32)| (p.0.floor() as i32, p.1.floor() as i32, p.2.floor() as i32);
        m.panic = m.panic.saturating_sub(1);
        if m.brain.sense_in == 0 {
            m.brain.sense_in = 4; // twice a second
            self.sense(server, m);
        } else {
            m.brain.sense_in -= 1;
        }
        let facts = Facts {
            panicking: m.panic > 0 || m.brain.memory.threat.is_some(),
            in_love: m.love > 0,
            baby: m.baby > 0,
            sheared: m.kind == Kind::Sheep && m.sheared,
            has_path: !m.path.is_empty(),
            roll: self.rand() % 1000,
            sunburnt: m.brain.memory.wants_cover,
            sitting: false,
        };
        if m.brain.decide(&facts).is_some() {
            m.path.clear(); // the old behaviour's walk is over
        }
        let mut partner = None;
        let mem = m.brain.memory.clone();
        match m.brain.doing {
            B::Panic => {
                if let Some(t) = mem.threat {
                    // Run directly away from the threat.
                    if m.path.is_empty() || self.rand().is_multiple_of(10) {
                        let (dx, dz) = (m.pos.0 - t.0, m.pos.2 - t.2);
                        let len = (dx * dx + dz * dz).sqrt().max(0.01);
                        m.path = path(&server.world, feet, (feet.0 + (dx / len * 8.0) as i32, feet.1, feet.2 + (dz / len * 8.0) as i32), 120);
                    }
                } else if m.path.is_empty() {
                    let a = self.randf() * std::f32::consts::TAU;
                    m.path = path(&server.world, feet, (feet.0 + (a.cos() * 7.0) as i32, feet.1, feet.2 + (a.sin() * 7.0) as i32), 120);
                }
            }
            B::SeekShade => {
                if m.path.is_empty()
                    && let Some(q) = self.find_cover(&server.world, feet)
                {
                    m.path = path(&server.world, feet, q, 160);
                }
            }
            B::GoToWork => {
                if let Some(site) = mem.work
                    && (m.path.is_empty() || self.rand().is_multiple_of(20))
                {
                    m.path = path(&server.world, feet, site, 200);
                }
            }
            B::Breed => {
                if let Some((mate, at, d2)) = mem.mate {
                    if d2 < 2.5 {
                        partner = Some(mate);
                    } else if m.path.is_empty() || self.rand().is_multiple_of(20) {
                        m.path = path(&server.world, feet, block(at), 160);
                    }
                }
            }
            B::Tempt => {
                // Follow the player, stopping a couple of blocks away.
                if let Some((_, at, d2)) = mem.feeder {
                    if d2 < 6.0 {
                        m.path.clear();
                    } else if m.path.is_empty() || self.rand().is_multiple_of(10) {
                        m.path = path(&server.world, feet, block(at), 160);
                    }
                }
            }
            B::FollowParent => {
                if let Some((_, at, _)) = mem.parent
                    && (m.path.is_empty() || self.rand().is_multiple_of(20))
                {
                    m.path = path(&server.world, feet, block(at), 160);
                }
            }
            B::EatGrass => {
                // Head down for 2 s, then the grass becomes dirt and the wool grows back (vanilla EatBlockGoal).
                m.path.clear();
                use bedrock::protocol::v975::enums::ActorEvent;
                if m.brain.ticks == 0 {
                    broadcast(server, event(m.id, ActorEvent::EatGrass));
                } else if m.brain.ticks >= 2 * TPS {
                    let g = (feet.0, feet.1 - 1, feet.2);
                    if blocks::short(server.world.get(g.0, g.1, g.2)) == "grass_block"
                        && let Some(dirt) = crate::blocks::any_state("minecraft:dirt")
                    {
                        server.set_block(0, g.0, g.1, g.2, dirt);
                    }
                    m.sheared = false;
                    m.regrow = 0;
                    m.brain.memory.on_grass = false;
                    broadcast(server, data_packet(m));
                }
            }
            B::Roam => {
                if m.path.is_empty() && m.brain.ticks == 0 {
                    let a = self.randf() * std::f32::consts::TAU;
                    let r = 3.0 + self.randf() * 7.0;
                    m.path = path(&server.world, feet, (feet.0 + (a.cos() * r) as i32, feet.1, feet.2 + (a.sin() * r) as i32), 120);
                }
            }
            // hostile behaviours; passive memory never has a target, villager or sunburn
            B::Idle | B::Attack | B::HuntVillager | B::Search | B::Sit | B::FollowOwner => {}
        }
        // Core behaviour, alongside the rest: look at a nearby player when standing still,
        // glance around now and then, otherwise look where it's going.
        if m.path.is_empty() {
            match mem.nearest_player {
                Some((_, at, d2)) if d2 < 36.0 => m.head_yaw = (-(at.0 - m.pos.0)).atan2(at.2 - m.pos.2).to_degrees(),
                _ if self.rand().is_multiple_of(40) => m.head_yaw = m.yaw + self.randf() * 120.0 - 60.0,
                _ => {}
            }
        } else {
            m.head_yaw = m.yaw;
        }
        // Follow the path: drop reached waypoints, steer toward the next.
        while let Some(&(wx, _, wz)) = m.path.first() {
            let (dx, dz) = (wx as f32 + 0.5 - m.pos.0, wz as f32 + 0.5 - m.pos.2);
            if dx * dx + dz * dz < 0.15 {
                m.path.remove(0);
            } else {
                break;
            }
        }
        m.target = m.path.first().map(|&(x, _, z)| (x as f32 + 0.5, z as f32 + 0.5));
        if let Some((tx, tz)) = m.target {
            let (dx, dz) = (tx - m.pos.0, tz - m.pos.2);
            let d = (dx * dx + dz * dz).sqrt().max(0.001);
            let speed = m.kind.speed() * m.brain.doing.speed();
            if m.on_ground {
                m.vel.0 = m.vel.0 * 0.5 + dx / d * speed * 0.5;
                m.vel.2 = m.vel.2 * 0.5 + dz / d * speed * 0.5;
            }
            m.yaw = (-dx).atan2(dz).to_degrees();
        }
        partner
    }

    /// Two animals in love met: both cool down and a baby appears between them.
    fn breed(&self, server: &Server, m: &mut Mob, partner: i64, out: &mut Vec<Protocol>) {
        if !server.plugins.entity_breed(server, m.kind.identifier(), (m.id, partner)) {
            m.love = 0;
            m.cooldown = BREED_COOLDOWN;
            return;
        }
        let Some(p) = self.mobs.lock().get_mut(&partner).map(|p| {
            p.love = 0;
            p.cooldown = BREED_COOLDOWN;
            p.pos
        }) else {
            return;
        };
        m.love = 0;
        m.cooldown = BREED_COOLDOWN;
        let at = ((m.pos.0 + p.0) / 2.0, m.pos.1.max(p.1), (m.pos.2 + p.2) / 2.0);
        let id = self.spawn_with(server, m.kind, at, true);
        if let Some(baby) = self.mobs.lock().get_mut(&id) {
            baby.brain.parent_id = Some(m.id);
            baby.kept = true;
        }
        use bedrock::protocol::v975::enums::ActorEvent;
        out.push(event(id, ActorEvent::LoveHearts));
    }

    fn physics(&self, server: &Server, m: &mut Mob) {
        let w = &server.world;
        let (x, y, z) = m.pos;
        let feet = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
        let in_water = blocks::is_water(w.get(feet.0, feet.1, feet.2));
        if in_water && m.kind == Kind::Drowned {
            m.vel.1 = (m.vel.1 - 0.02).max(-0.1); // sinks and walks the sea floor
            if m.target.is_some() && !m.on_ground {
                m.vel.1 += 0.03;
            }
        } else if in_water {
            m.vel.1 = (m.vel.1 + 0.04).min(0.12); // float
        } else {
            m.vel.1 -= GRAVITY;
        }
        // Spiders climb walls they walk into.
        if m.kind == Kind::Spider && m.target.is_some() {
            let ahead = (x + m.vel.0.signum() * 0.8, z + m.vel.2.signum() * 0.8);
            if solid(w.get(ahead.0.floor() as i32, feet.1, ahead.1.floor() as i32)) {
                m.vel.1 = 0.2;
            }
        }
        // Horizontal: blocked by solid blocks at feet/head; a one-block step is jumped onto
        // (vanilla jump, about 1.25 blocks), not stepped through.
        let (nx, nz) = (x + m.vel.0, z + m.vel.2);
        let (bx, bz) = (nx.floor() as i32, nz.floor() as i32);
        let fy = y.floor() as i32;
        let h = m.kind.size().1.ceil() as i32;
        let clear = |yy: i32| (0..h.max(1)).all(|dy| walkable(w.get(bx, yy + dy, bz)));
        let (mut px, mut py, mut pz) = (x, y, z);
        if clear(fy) {
            px = nx;
            pz = nz;
        } else if m.on_ground && clear(fy + 1) && walkable(w.get(feet.0, fy + h + 1, feet.2)) {
            m.vel.1 = JUMP;
            m.on_ground = false;
        } else if !m.on_ground && clear(fy + 1) {
            // Mid-jump: wait to clear the edge, keep heading there.
        } else {
            m.vel.0 = 0.0;
            m.vel.2 = 0.0;
            m.target = None;
        }
        // Vertical.
        let ny = py + m.vel.1;
        let (cx, cz) = (px.floor() as i32, pz.floor() as i32);
        if m.vel.1 <= 0.0 && solid(w.get(cx, ny.floor() as i32, cz)) {
            py = ny.floor() + 1.0;
            m.vel.1 = 0.0;
            m.on_ground = true;
        } else if m.vel.1 > 0.0 && solid(w.get(cx, (ny + m.kind.size().1).floor() as i32, cz)) {
            m.vel.1 = 0.0;
            m.on_ground = false;
        } else {
            py = ny;
            m.on_ground = false;
        }
        if m.on_ground {
            m.vel.0 *= 0.6;
            m.vel.2 *= 0.6;
        }
        m.pos = (px, py, pz);
    }

    /// Nether and End spawns near a player, roughly by biome.
    fn spawn_other_dim(&self, server: &Server, p: &crate::players::PlayerInfo, dim: u8) {
        let w = &server.world;
        let (lo, hi) = crate::dims::range(dim);
        for _ in 0..2 {
            let a = self.randf() * std::f32::consts::TAU;
            let r = 20.0 + self.randf() * 24.0;
            let (x, z) = ((p.pos.0 + a.cos() * r).floor() as i32, (p.pos.2 + a.sin() * r).floor() as i32);
            let start = (p.pos.1 as i32 + 16).min(hi - 2);
            let Some(y) = (lo + 2..start).rev().find(|&y| solid(w.get(x, y - 1, z)) && standable(w, (x, y, z))) else { continue };
            let roll = self.rand() % 100;
            let kind = if dim == crate::dims::END {
                Kind::Enderman
            } else {
                match w.biome_at(x, y, z) {
                    Some(179) => if roll < 60 { Kind::Hoglin } else if roll < 90 { Kind::Piglin } else { Kind::ZombiePigman },
                    Some(180) => Kind::Enderman,
                    Some(178) => if roll < 70 { Kind::Skeleton } else if roll < 85 { Kind::Ghast } else { Kind::Enderman },
                    Some(181) => if roll < 80 { Kind::MagmaCube } else { Kind::Ghast },
                    _ => match roll {
                        0..50 => Kind::ZombiePigman,
                        50..70 => Kind::Piglin,
                        70..80 => Kind::Ghast,
                        80..90 => Kind::MagmaCube,
                        _ => Kind::Enderman,
                    },
                }
            };
            if kind == Kind::Ghast && !(0..5).all(|dy| blocks::is_air(w.get(x, y + dy, z))) {
                continue; // ghasts need room
            }
            let at = if kind.flies() { (x as f32 + 0.5, y as f32 + 3.0, z as f32 + 0.5) } else { (x as f32 + 0.5, y as f32, z as f32 + 0.5) };
            self.spawn(server, kind, at);
        }
    }

    /// Each chunk near a player gets one roll for a herd on grass; hostiles via spawn_hostiles.
    pub fn spawn_near_players(&self, server: &Server) {
        // Overworld: Bedrock's spawn rules (spawning.rs). Nether and End: the older spawner.
        crate::spawning::tick(server, &mut || self.rand());
        if *crate::commands::DIFFICULTY.lock() != 0 {
            for p in server.all() {
                let dim = crate::dims::of_f(p.pos.1);
                if dim != crate::dims::OVERWORLD {
                    self.spawn_other_dim(server, &p, dim);
                }
            }
        }
        if self.count() >= CAP {
            return;
        }
        for p in server.all() {
            let (pcx, pcz) = ((p.pos.0.floor() as i32) >> 4, (p.pos.2.floor() as i32) >> 4);
            for _ in 0..4 {
                let (cx, cz) = (pcx + (self.rand() % 13) as i32 - 6, pcz + (self.rand() % 13) as i32 - 6);
                if !self.rolled.lock().insert((cx, cz)) {
                    continue;
                }
                if self.populate_village(server, cx, cz) {
                    continue;
                }
                if !self.rand().is_multiple_of(10) {
                    continue; // ~10% of chunks get animals
                }
                let (bx, bz) = (cx * 16 + (self.rand() % 16) as i32, cz * 16 + (self.rand() % 16) as i32);
                let mut y = 200;
                while y > -60 && blocks::is_air(server.world.get(bx, y, bz)) {
                    y -= 1;
                }
                let ground = server.world.get(bx, y, bz);
                let Some(kind) = animal_for(server.world.biome_at(bx, y + 1, bz), ground, self.rand()) else { continue };
                for _ in 0..2 + self.rand() % 3 {
                    let (ox, oz) = ((self.rand() % 5) as f32 - 2.0, (self.rand() % 5) as f32 - 2.0);
                    self.spawn(server, kind, (bx as f32 + 0.5 + ox, (y + 1) as f32, bz as f32 + 0.5 + oz));
                }
            }
        }
    }
}

/// Blocks from the nearest player beyond which mobs pause (Bedrock's default simulation
/// distance is 4 chunks; a bit more so mobs at the edge of view still move).
const SIMULATION: f32 = 96.0;
/// PNX MeleeAttackExecutor: a hit every 30 game ticks.
const MELEE_COOLDOWN: u32 = 3 * TPS / 2;
/// PNX BowShootExecutor: 20 ticks drawing plus 30 cooling down.
/// PNX BowShootExecutor: 30 game ticks between shots, a 20-tick draw; pillagers charge their
/// crossbow for 80 ticks, loaded (CHARGED) at 60. In our 10 Hz ticks.
const BOW_COOLDOWN: u32 = 15;
const BOW_DRAW: u32 = 10;
const CROSSBOW_DRAW: u32 = 40;
const CROSSBOW_CHARGED: u32 = 30;
// PNX EntityArrow: 0.05 a game tick. Velocities here are per 10 Hz tick (two game ticks of
// travel), so the speed lost each tick is 2 x 2 x 0.05.
const ARROW_GRAVITY: f32 = 0.2;

/// Is player `id` wearing armour whose item name matches?
fn wears(id: i64, f: impl Fn(&str) -> bool) -> bool {
    crate::players::equipment(id).armor.iter().flatten().any(|st| crate::items::name_of(st.item.id).is_some_and(&f))
}

/// Looking at `head` from `eye` (vanilla EnderMan.isLookingAtMe: within 64 blocks, the view
/// pointing within a narrow cone that tightens with distance).
fn stares_at(eye: (f32, f32, f32), rot: (f32, f32), head: (f32, f32, f32)) -> bool {
    let (dx, dy, dz) = (head.0 - eye.0, head.1 - eye.1, head.2 - eye.2);
    let len = (dx * dx + dy * dy + dz * dz).sqrt();
    if !(0.5..64.0).contains(&len) {
        return false;
    }
    let (pitch, yaw) = (rot.0.to_radians(), rot.1.to_radians());
    let look = (-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos());
    let dot = (look.0 * dx + look.1 * dy + look.2 * dz) / len;
    dot > 1.0 - 0.025 / len
}

/// PNX EntityArrow: ceil(speed per game tick x base), base 2 plus 0.5 per Power level + 0.5
/// (ItemBow); a critical (fully drawn) arrow adds a random 0..base/2+1 on top.
fn arrow_damage(speed: f32, power: i16, critical_roll: Option<u32>) -> f32 {
    let base = 2.0 + if power > 0 { power as f32 * 0.5 + 0.5 } else { 0.0 };
    let dmg = (speed * base).ceil();
    match critical_roll {
        Some(r) => dmg + (r % (dmg as u32 / 2 + 2)) as f32,
        None => dmg,
    }
}

pub(crate) fn dist2(a: (f32, f32, f32), b: (f32, f32, f32)) -> f32 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)
}

/// Mob damage multiplier: easy 2/3, normal 1, hard 1.5.
fn difficulty_scale() -> f32 {
    match *crate::commands::DIFFICULTY.lock() {
        0 => 0.0,
        1 => 2.0 / 3.0,
        3 => 1.5,
        _ => 1.0,
    }
}

/// Whether nothing solid is above `p` (64 blocks up): open sky.
pub(crate) fn sky(w: &crate::world::World, p: (i32, i32, i32)) -> bool {
    (p.1 + 1..=(p.1 + 64).min(319)).all(|y| !solid(w.get(p.0, y, p.2)) && !blocks::is_water(w.get(p.0, y, p.2)))
}

/// A mob's own sound (ambient, hurt, death...): the client picks the clip from the mob type.
pub fn mob_sound(name: &str, kind: Kind, at: (f32, f32, f32), baby: bool) -> Protocol {
    Protocol::LevelSoundEventPacket(Box::new(bedrock::protocol::v1001::packets::LevelSoundEventPacket {
        event_name: name.into(),
        position: at,
        data: -1,
        actor_identifier: kind.identifier().into(),
        is_baby_mob: baby,
        is_global: false,
        entity_unique_id: u64::MAX,
        fire_at_position: None,
    }))
}

fn sound(name: &str, at: (f32, f32, f32)) -> Protocol {
    Protocol::LevelSoundEventPacket(Box::new(bedrock::protocol::v1001::packets::LevelSoundEventPacket {
        event_name: name.into(),
        position: at,
        data: -1,
        actor_identifier: String::new(),
        is_baby_mob: false,
        is_global: false,
        entity_unique_id: u64::MAX,
        fire_at_position: None,
    }))
}

/// Lit TNT: (entity id, position, velocity per game tick, 10 Hz ticks left).
static PRIMED: Mutex<Vec<(i64, (f32, f32, f32), (f32, f32, f32), u32)>> = Mutex::new(Vec::new());

/// Primes the TNT at `p` (PNX BlockTNT.prime), going off with power 4 after `fuse` 10 Hz ticks.
pub fn ignite_tnt(server: &Server, p: (i32, i32, i32), fuse: u32) {
    if !server.plugins.tnt_prime(server, p) {
        return;
    }
    server.set_block_quiet(0, p.0, p.1, p.2, crate::world::AIR);
    let id = crate::players::next_id();
    let pos = (p.0 as f32 + 0.5, p.1 as f32, p.2 as f32 + 0.5);
    // Vanilla hop: 0.2 up and a random 0.02 sideways.
    let a = ((p.0 * 73 + p.2 * 31 + fuse as i32) as f32).sin() * std::f32::consts::TAU;
    let vel = (a.cos() * 0.02, 0.2, a.sin() * 0.02);
    let packet = Protocol::AddActorPacket(Box::new(bedrock::protocol::v2225::packets::AddActorPacket {
        target_actor_id: ActorUniqueID(id),
        target_runtime_id: ActorRuntimeID(id as u64),
        actor_type: "minecraft:tnt".into(),
        position: pos,
        velocity: vel,
        rotation: (0.0, 0.0),
        y_head_rotation: 0.0,
        y_body_rotation: 0.0,
        attributes: vec![],
        // Same flag word as mobs, with IGNITED (bit 10) so it flashes.
        actor_data: vec![DataItem { data_item_id: ActorDataIDs::Reserved0, data_item_type: DataItemType::Int64(crate::spawn::FLAGS | 1 << 10) }],
        synced_properties: PropertySyncData { int_entries_list: vec![], float_entries_list: vec![] },
        actor_links: vec![],
        passenger_of_block_data: None,
    }));
    let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(packet) });
    let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(crate::packets::play_sound("random.fuse", pos)) });
    PRIMED.lock().push((id, pos, vel, fuse));
}

/// Counts down lit TNT and sets it off.
fn tick_tnt(server: &Server) {
    let mut due = Vec::new();
    let mut moves = Vec::new();
    let w = &server.world;
    let solid = |p: (f32, f32, f32)| crate::overworld::feature::blocks::is_solid(w.get(p.0.floor() as i32, p.1.floor() as i32, p.2.floor() as i32));
    PRIMED.lock().retain_mut(|(id, pos, vel, fuse)| {
        // Falls, slides and gets thrown around like vanilla primed TNT (two game ticks).
        let before = *pos;
        for _ in 0..2 {
            vel.1 -= 0.04;
            let next = (pos.0 + vel.0, pos.1, pos.2 + vel.2);
            if solid(next) || solid((next.0, next.1 + 0.9, next.2)) {
                vel.0 = 0.0;
                vel.2 = 0.0;
            } else {
                pos.0 = next.0;
                pos.2 = next.2;
            }
            let ny = pos.1 + vel.1;
            let ground = vel.1 < 0.0 && solid((pos.0, ny, pos.2));
            if ground {
                pos.1 = ny.floor() + 1.0;
                vel.1 *= -0.5;
                vel.0 *= 0.7;
                vel.2 *= 0.7;
            } else if vel.1 > 0.0 && solid((pos.0, ny + 0.98, pos.2)) {
                vel.1 = 0.0;
            } else {
                pos.1 = ny;
            }
            *vel = (vel.0 * 0.98, vel.1 * 0.98, vel.2 * 0.98);
        }
        if *pos != before {
            moves.push((*id, *pos));
        }
        *fuse = fuse.saturating_sub(1);
        if *fuse == 0 {
            due.push((*id, *pos));
        }
        *fuse > 0
    });
    for (id, pos) in moves {
        let mv = Protocol::MoveActorAbsolutePacket(Box::new(bedrock::protocol::v662::packets::MoveActorAbsolutePacket {
            move_data: bedrock::protocol::v662::types::MoveActorAbsoluteData { actor_runtime_id: ActorRuntimeID(id as u64), header: 0, position: pos, rotation_x: 0, rotation_y: 0, rotation_y_head: 0 },
        }));
        let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(mv) });
    }
    for (id, pos) in due {
        let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(Protocol::RemoveActorPacket(Box::new(RemoveActorPacket { target_actor_id: ActorUniqueID(id) }))) });
        explode(server, (pos.0, pos.1 + 0.06, pos.2), 4.0, "TNT");
    }
}

/// Blast resistance (Java-scale values, Bedrock's / 5), what eats an explosion ray.
fn blast_resistance(b: i32) -> f32 {
    let n = blocks::short(b);
    match n {
        "bedrock" | "barrier" | "end_portal_frame" | "end_portal" | "reinforced_deepslate" | "command_block" | "structure_block" | "jigsaw" | "light_block" => 3_600_000.0,
        "obsidian" | "crying_obsidian" | "anvil" | "chipped_anvil" | "damaged_anvil" | "ancient_debris" | "netherite_block" | "respawn_anchor" | "enchanting_table" => 1200.0,
        "ender_chest" => 600.0,
        "water" | "flowing_water" | "lava" | "flowing_lava" => 100.0,
        "end_stone" | "end_bricks" => 9.0,
        "iron_block" | "gold_block" | "diamond_block" | "emerald_block" | "copper_block" | "iron_bars" | "iron_door" | "iron_trapdoor" | "mob_spawner" | "lodestone" => 6.0,
        n if n.ends_with("_ore") => 3.0,
        n if n.contains("deepslate") || n.contains("blackstone") || n == "cobblestone" || n == "mossy_cobblestone" || n == "stone" || n.contains("brick") && n != "nether_brick_fence"
            || matches!(n, "andesite" | "diorite" | "granite" | "polished_andesite" | "polished_diorite" | "polished_granite" | "tuff" | "furnace" | "lit_furnace" | "smoker" | "blast_furnace" | "stonecutter_block" | "dispenser" | "dropper" | "observer") => 6.0,
        n if n.contains("stone") && !n.contains("sandstone") && !n.contains("glowstone") && !n.contains("redstone") => 6.0,
        n if n.contains("planks") || n.contains("fence") || n.ends_with("_door") || n.ends_with("_slab") || n.ends_with("_stairs") => 3.0,
        n if n.ends_with("_log") || n.ends_with("_wood") || n.ends_with("_stem") || n.ends_with("_hyphae") || n.contains("chest") || n == "crafting_table" || n == "barrel" => 2.5,
        n if n.contains("terracotta") || n.ends_with("_concrete") => 4.2,
        n if n.contains("sandstone") || n.contains("wool") => 0.8,
        n if n.contains("glass") || n == "glowstone" || n == "sea_lantern" => 0.3,
        n if n.contains("leaves") => 0.2,
        "netherrack" => 0.4,
        "gravel" | "clay" | "grass_block" | "farmland" | "mycelium" | "podzol" | "dirt_path" | "grass_path" => 0.6,
        n if n.contains("dirt") || n.contains("sand") || n.contains("mud") || n.starts_with("soul_") || n.contains("concrete_powder") => 0.5,
        n if n.contains("torch") || n.contains("flower") || n.contains("sapling") || n.ends_with("grass") || n == "redstone_wire" || n.contains("carpet") || n == "fire" => 0.0,
        _ => 1.0,
    }
}

/// Vanilla explosion (Java/PNX Explosion.explodeA): rays from a 16x16x16 grid's surface, each
/// starting at power x 0.7-1.3 and losing (resistance + 0.3) x 0.3 per 0.3-block step, plus
/// 0.225; every solid block a ray reaches with strength left breaks. Water stops it.
pub(crate) fn explosion_rays(w: &crate::world::World, at: (f32, f32, f32), power: f32, mut seed: u64) -> Vec<(i32, i32, i32)> {
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f32 / (1u64 << 53) as f32
    };
    let mut out = std::collections::HashSet::new();
    for i in 0..16 {
        for j in 0..16 {
            for k in 0..16 {
                if !(i == 0 || i == 15 || j == 0 || j == 15 || k == 0 || k == 15) {
                    continue;
                }
                let (dx, dy, dz) = (i as f32 / 15.0 * 2.0 - 1.0, j as f32 / 15.0 * 2.0 - 1.0, k as f32 / 15.0 * 2.0 - 1.0);
                let len = (dx * dx + dy * dy + dz * dz).sqrt();
                let (dx, dy, dz) = (dx / len * 0.3, dy / len * 0.3, dz / len * 0.3);
                let mut f = power * (0.7 + rand() * 0.6);
                let mut p = at;
                while f > 0.0 {
                    let c = (p.0.floor() as i32, p.1.floor() as i32, p.2.floor() as i32);
                    let b = w.get(c.0, c.1, c.2);
                    if !blocks::is_air(b) {
                        f -= (blast_resistance(b) + 0.3) * 0.3;
                        if f > 0.0 && !blocks::is_liquid(b) {
                            out.insert(c);
                        }
                    }
                    p = (p.0 + dx, p.1 + dy, p.2 + dz);
                    f -= 0.225;
                }
            }
        }
    }
    out.into_iter().collect()
}

pub fn explode(server: &Server, at: (f32, f32, f32), power: f32, by: &'static str) {
    if !server.plugins.explosion(server, at, power) {
        return;
    }
    let w = &server.world;
    let mut blown = explosion_rays(w, at, power, (at.0.to_bits() ^ at.2.to_bits().rotate_left(13)) as u64 | 1 ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64));
    blown.retain(|&(x, y, z)| {
        if blocks::short(w.get(x, y, z)) == "tnt" {
            // PNX: TNT caught in a blast gets a random short fuse (10-30 ticks).
            ignite_tnt(server, (x, y, z), 5 + ((x ^ y ^ z) & 7) as u32 * 2);
            false
        } else {
            true
        }
    });
    server.plugins.explosion_blocks(server, at, &mut blown);
    for &(x, y, z) in &blown {
        let b = w.get(x, y, z);
        server.set_block_quiet(0, x, y, z, crate::world::AIR);
        // Vanilla yield: each broken block drops with a 1 in `power` chance.
        if (x * 31 + y * 17 + z * 7).rem_euclid(power.round().max(1.0) as i32) == 0 {
            for (item, n) in crate::drops::drops(b, (x, y, z), Some("minecraft:diamond_pickaxe")) {
                server.entities.drop_item(server, item, n, (x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5));
            }
        }
    }
    for p in blown {
        crate::pop_unsupported(server, 0, p);
    }
    let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(sound("explode", at)) });
    // Mobs in range take the same vanilla damage (and get knocked away from the blast).
    let reach = power * 2.0;
    // Lit TNT nearby gets thrown (TNT cannons).
    for (_, pos, vel, _) in PRIMED.lock().iter_mut() {
        let (dx, dy, dz) = (pos.0 - at.0, pos.1 - at.1, pos.2 - at.2);
        let d = (dx * dx + dy * dy + dz * dz).sqrt();
        if d < reach && d > 0.01 {
            let impact = 1.0 - d / reach;
            *vel = (vel.0 + dx / d * impact, vel.1 + dy / d * impact, vel.2 + dz / d * impact);
        }
    }
    let near: Vec<(i64, f32)> = server.mobs.mobs.lock().values().filter(|m| m.dying == 0).map(|m| (m.id, dist2(m.pos, at).sqrt())).filter(|(_, d)| *d < reach).collect();
    for (id, d) in near {
        let impact = 1.0 - d / reach;
        server.mobs.hurt(server, id, ((impact * impact + impact) / 2.0 * 7.0 * reach + 1.0).floor(), at, None);
    }
    for p in server.all() {
        let feet = (p.pos.0, p.pos.1 - 1.62, p.pos.2);
        let d = dist2(feet, at).sqrt();
        let reach = power * 2.0;
        if d >= reach {
            continue;
        }
        // Vanilla: impact falls off over twice the power; damage (i^2 + i) / 2 * 7 * 2 * power + 1,
        // pushed straight away from the centre.
        let impact = 1.0 - d / reach;
        let damage = ((impact * impact + impact) / 2.0 * 7.0 * reach + 1.0).floor() * difficulty_scale().max(0.5);
        let (dx, dy, dz) = (feet.0 - at.0, p.pos.1 - 0.81 - at.1, feet.2 - at.2);
        let len = (dx * dx + dy * dy + dz * dz).sqrt().max(0.01);
        let _ = server.events.send(Event::Damage {
            to: p.id,
            damage: crate::health::Damage::Explosion(damage, by),
            knock: (dx / len * impact, (dy / len * impact).max(0.1 * impact), dz / len * impact),
        });
    }
}

impl Mobs {
    /// A chunk with village beds gets villagers (one per bed, up to 4), a cat now and then,
    /// and an iron golem if it has the bell. Returns true if it's a village chunk.
    fn populate_village(&self, server: &Server, cx: i32, cz: i32) -> bool {
        let w = &server.world;
        let (mut beds, mut bell, mut spots) = (0, false, Vec::new());
        for lx in 0..16 {
            for lz in 0..16 {
                let (x, z) = (cx * 16 + lx, cz * 16 + lz);
                let mut top = 200;
                while top > -60 && blocks::is_air(w.get(x, top, z)) {
                    top -= 1;
                }
                for y in (top - 12).max(-60)..=top {
                    let n = blocks::short(w.get(x, y, z));
                    if n == "bed" || n.ends_with("_bed") {
                        beds += 1;
                    }
                    if n == "bell" {
                        bell = true;
                    }
                }
                if lx % 4 == 2 && lz % 4 == 2 && standable(w, (x, top + 1, z)) {
                    spots.push((x as f32 + 0.5, (top + 1) as f32, z as f32 + 0.5));
                }
            }
        }
        // Beds are two blocks each.
        let villagers = (beds / 2).min(4);
        if villagers == 0 && !bell {
            return false;
        }
        let spot = || spots.get(self.rand() as usize % spots.len().max(1)).copied();
        for _ in 0..villagers {
            if let Some(s) = spot() {
                self.spawn(server, Kind::Villager, s);
            }
        }
        if villagers > 0 && self.rand().is_multiple_of(3)
            && let Some(s) = spot()
        {
            self.spawn(server, Kind::Cat, s);
        }
        if bell && let Some(s) = spot() {
            self.spawn(server, Kind::IronGolem, s);
        }
        true
    }
}


impl Mobs {
    /// Saves only mobs players care about: villagers, iron golems, pets, saddled mounts and
    /// animals a player fed or bred. Wild mobs (and spawn-egg mobs) are left out, and so are
    /// the "chunk already got its animals" marks, so fresh animals spawn after a restart.
    /// `keys` maps online entity ids to owner keys.
    pub fn save(&self, path: &std::path::Path, keys: &HashMap<i64, i64>) -> std::io::Result<()> {
        use std::fmt::Write;
        let mut out = String::from("# Rusted mobs v1: kind x y z yaw health baby owner sitting color sheared job saddled temper\n");
        for m in self.mobs.lock().values() {
            let keep = matches!(m.kind, Kind::Villager | Kind::IronGolem) || m.owner.is_some() || m.saddled || m.kept || m.temper > 0;
            if m.dying > 0 || !keep {
                continue;
            }
            let _ = writeln!(
                out,
                "m {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
                m.kind.identifier(), m.pos.0, m.pos.1, m.pos.2, m.yaw, m.health, m.baby, m.owner.map_or(0, |o| keys.get(&o).copied().unwrap_or(o)), m.sitting as u8, m.color, m.sheared as u8,
                m.job.as_ref().map_or("-".to_string(), |j| j.to_text()), m.saddled as u8, m.temper
            );
            if let Some(h) = m.home {
                out.pop();
                let _ = writeln!(out, " {},{},{}", h.0, h.1, h.2);
            }
            if m.kept {
                out.pop();
                out.push_str(" kept\n");
            }
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, out)?;
        std::fs::rename(&tmp, path)
    }

    /// Loads the save file. Pets track owners by name since entity ids change per session.
    pub fn load(&self, path: &std::path::Path, owners: &HashMap<i64, String>) -> usize {
        let Ok(text) = std::fs::read_to_string(path) else { return 0 };
        let _ = owners;
        let mut n = 0;
        let mut mobs = self.mobs.lock();
        for line in text.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            match f.as_slice() {
                ["c", x, z] => {
                    if let (Ok(x), Ok(z)) = (x.parse(), z.parse()) {
                        self.rolled.lock().insert((x, z));
                    }
                }
                ["m", kind, x, y, z, yaw, health, baby, owner, sitting, color, sheared, extra @ ..] => {
                    let job = extra.first().and_then(|t| crate::villagers::Job::from_text(t)).map(Box::new);
                    let saddled = extra.get(1) == Some(&"1");
                    let kept = extra.contains(&"kept");
                    let temper = extra.get(2).and_then(|t| t.parse().ok()).unwrap_or(0u32).min(100);
                    let home = extra.get(3).and_then(|t| {
                        let v: Vec<i32> = t.split(',').filter_map(|n| n.parse().ok()).collect();
                        (v.len() == 3).then(|| (v[0], v[1], v[2]))
                    });
                    let num = |s: &str| s.parse::<f32>().ok().filter(|v| v.is_finite());
                    let (Some(kind), Some(x), Some(y), Some(z), Some(yaw), Some(health)) = (Kind::from_name(kind), num(x), num(y), num(z), num(yaw), num(health)) else {
                        continue;
                    };
                    let id = crate::players::next_id();
                    let pos = (x, y, z);
                    let owner = owner.parse::<i64>().ok().filter(|&o| o != 0);
                    mobs.insert(id, Mob {
                        id, kind, pos, vel: (0.0, 0.0, 0.0), yaw, health, on_ground: false, target: None, panic: 0, dying: 0,
                        sent: (pos, yaw, yaw), head_yaw: yaw, path: vec![], love: 0, cooldown: 0, baby: baby.parse().unwrap_or(0),
                        aggro: None, attack_cd: 0, fuse: 0, burning: false, prey: None, owner, sitting: *sitting == "1",
                        color: color.parse().unwrap_or(0).min(15), sheared: *sheared == "1", regrow: if *sheared == "1" { 3 * 60 * TPS } else { 0 }, hurt_at: None, fire: 0, job: job.clone(), weak: 0, curing: 0, captain: false, rider: None, passenger: None, steered: None, saddled, kept, lit: false, charged: false, perch: 0, lifetime: 0, leash: None, draw: 0, aim: 0, temper, buck: 0, push: None, home, brain: Default::default(),
                    });
                    if let Some(site) = job.and_then(|j| j.site) {
                        crate::villagers::claim(site, id);
                    }
                    n += 1;
                }
                _ => {}
            }
        }
        n
    }

    /// A player (re)joined: pets saved with their old owner key follow them again.
    pub fn owner_joined(&self, old_key: i64, new_id: i64) {
        for m in self.mobs.lock().values_mut() {
            if m.owner == Some(old_key) {
                m.owner = Some(new_id);
            }
        }
    }

    /// Before saving: pets point at a stable per-name key instead of a session id.
    pub fn owner_left(&self, id: i64, key: i64) {
        for m in self.mobs.lock().values_mut() {
            if m.owner == Some(id) {
                m.owner = Some(key);
            }
        }
    }
}

/// A stable id for a player name (pets remember owners across sessions with it).
pub fn owner_key(name: &str) -> i64 {
    // Negative so it never collides with live entity ids.
    -((name.to_lowercase().bytes().fold(1_469_598_103_934_665_603u64, |h, b| (h ^ b as u64).wrapping_mul(1_099_511_628_211)) >> 1) as i64) - 1
}

/// What an interaction does with the player's held item.
#[derive(Debug, PartialEq)]
pub enum Use {
    Nothing,
    /// Right-clicked a villager with a job: open its trade screen.
    Trade,
    /// Fed: use up one.
    Consume,
    /// Bucket on a cow: becomes a milk bucket.
    Milk,
    /// The player is now riding it.
    Mount,
    /// Used a tool on it (flint and steel on a creeper): one durability.
    Wear,
    /// A chest minecart: open the chest kept at this container-store key.
    OpenCart((i32, i32, i32)),
}

/// Where a chest minecart from mineshaft spot `home` keeps its items: far below the world, so
/// it can't clash with a real block's container.
pub fn cart_store(home: (i32, i32, i32)) -> (i32, i32, i32) {
    (home.0, home.1 - 100_000, home.2)
}

/// Whether a mob can stand at block `p` (feet there and above free, solid ground below).
pub(crate) fn standable(w: &crate::world::World, p: (i32, i32, i32)) -> bool {
    walkable(w.get(p.0, p.1, p.2)) && walkable(w.get(p.0, p.1 + 1, p.2)) && {
        let below = w.get(p.0, p.1 - 1, p.2);
        solid(below) || blocks::is_water(below)
    }
}

/// A* toward `goal`: 8 directions, step up 1, drop up to 3, water costs more, avoids lava.
/// Gives up after `budget` nodes and returns the path to the closest point.
pub fn path(w: &crate::world::World, start: (i32, i32, i32), goal: (i32, i32, i32), budget: usize) -> Vec<(i32, i32, i32)> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let h = |p: (i32, i32, i32)| ((p.0 - goal.0).abs() + (p.2 - goal.2).abs() + (p.1 - goal.1).abs()) as u32 * 10;
    let mut open = BinaryHeap::new();
    let mut came: HashMap<(i32, i32, i32), ((i32, i32, i32), u32)> = HashMap::new();
    came.insert(start, (start, 0));
    open.push(Reverse((h(start), start)));
    let (mut best, mut best_h) = (start, h(start));
    let mut expanded = 0;
    while let Some(Reverse((_, p))) = open.pop() {
        expanded += 1;
        if p == goal || expanded > budget {
            break;
        }
        let g = came[&p].1;
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let diag = dx != 0 && dz != 0;
            // Diagonals only when both sides are open (no corner cutting).
            if diag && !(walkable(w.get(p.0 + dx, p.1, p.2)) && walkable(w.get(p.0, p.1, p.2 + dz))) {
                continue;
            }
            let (nx, nz) = (p.0 + dx, p.2 + dz);
            let next = [0, 1, -1, -2, -3].into_iter().map(|dy| (nx, p.1 + dy, nz)).find(|&q| {
                standable(w, q) && (q.1 <= p.1 || walkable(w.get(p.0, p.1 + 2, p.2)))
            });
            let Some(q) = next else { continue };
            let water = blocks::is_water(w.get(q.0, q.1, q.2));
            let cost = g + if diag { 14 } else { 10 } + if water { 20 } else { 0 } + ((p.1 - q.1).max(0) as u32) * 4;
            if came.get(&q).is_none_or(|&(_, c)| cost < c) {
                came.insert(q, (p, cost));
                let hq = h(q);
                if hq < best_h {
                    (best, best_h) = (q, hq);
                }
                open.push(Reverse((cost + hq, q)));
            }
        }
    }
    let end = if came.contains_key(&goal) { goal } else { best };
    let mut out = vec![];
    let mut c = end;
    while c != start {
        out.push(c);
        c = came[&c].0;
    }
    out.reverse();
    out
}

/// Melee damage of the held item (vanilla Bedrock values).
pub fn attack_damage(held: Option<&str>) -> f32 {
    if let Some(c) = held.and_then(crate::custom::item) {
        return c.damage;
    }
    match held.unwrap_or("").trim_start_matches("minecraft:") {
        "wooden_sword" | "golden_sword" => 5.0,
        "stone_sword" => 6.0,
        "iron_sword" => 7.0,
        "diamond_sword" => 8.0,
        "netherite_sword" => 9.0,
        "wooden_axe" | "golden_axe" => 4.0,
        "stone_axe" => 5.0,
        "iron_axe" => 6.0,
        "diamond_axe" => 7.0,
        "netherite_axe" => 8.0,
        "trident" => 9.0,
        "mace" => 6.0,
        n if n.ends_with("_pickaxe") => 3.0,
        n if n.ends_with("_shovel") => 2.5,
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    /// Thrown items and lit TNT are server-wide lists any server's tick moves, so tests that
    /// tick mobs take turns.
    static SERIAL: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    use super::*;

    #[test]
    fn endermen_notice_a_stare() {
        // Looking straight east (yaw -90) at a head 10 blocks east: a stare.
        assert!(stares_at((0.0, 0.0, 0.0), (0.0, -90.0), (10.0, 0.0, 0.0)));
        // A few degrees off at that range isn't.
        assert!(!stares_at((0.0, 0.0, 0.0), (0.0, -80.0), (10.0, 0.0, 0.0)));
        assert!(!stares_at((0.0, 0.0, 0.0), (0.0, -90.0), (70.0, 0.0, 0.0)), "too far");
    }

    #[test]
    fn arrow_damage_like_pnx() {
        assert_eq!(arrow_damage(3.0, 0, None), 6.0, "full draw, no Power");
        assert_eq!(arrow_damage(3.0, 1, None), 9.0, "Power I: base 3");
        assert_eq!(arrow_damage(3.0, 5, None), 15.0, "Power V: base 5");
        let crit = arrow_damage(3.0, 0, Some(4));
        assert!((6.0..=10.0).contains(&crit), "critical adds 0..4: {crit}");
    }

    #[test]
    fn mobs_jump_onto_a_step_instead_of_teleporting() {
        let _serial = SERIAL.lock();
        let server = Server::new(); // flat: stand at y -60
        let stone = crate::blocks::any_state("minecraft:stone").unwrap();
        server.world.set(201, -60, 200, stone);
        let id = server.mobs.spawn(&server, Kind::Zombie, (200.5, -60.0, 200.5));
        let mut heights = vec![];
        for _ in 0..12 {
            let mut mobs = server.mobs.mobs.lock();
            let m = mobs.get_mut(&id).unwrap();
            m.vel.0 = 0.2; // walk east into the step
            m.vel.2 = 0.0;
            server.mobs.physics(&server, m);
            heights.push(m.pos.1);
        }
        let first_rise = heights.iter().position(|&y| y > -60.0).unwrap();
        assert!(heights[first_rise] < -59.0, "rises gradually, not straight onto the step: {heights:?}");
        // It lands on the step (then walks off its far side, since it keeps going east).
        assert!(heights.iter().any(|&y| (y - -59.0).abs() < 0.01), "landed on the step: {heights:?}");
        assert!(server.mobs.mobs.lock()[&id].pos.0 > 201.0, "and walked over it");
    }

    #[test]
    fn mobs_fall_wander_and_die_with_drops() {
        let _serial = SERIAL.lock();
        let server = Server::new(); // flat world: grass top at y -61, stand at -60
        let id = server.mobs.spawn(&server, Kind::Cow, (0.5, -55.0, 0.5));
        for _ in 0..40 {
            server.mobs.tick(&server);
        }
        let y = server.mobs.mobs.lock()[&id].pos.1;
        assert!((y - -60.0).abs() < 0.01, "landed on the grass: {y}");
        server.mobs.hurt(&server, id, 4.0, (2.0, -60.0, 0.5), None);
        assert!(server.mobs.contains(id));
        server.mobs.hurt(&server, id, 20.0, (2.0, -60.0, 0.5), None);
        assert_eq!(server.mobs.mobs.lock()[&id].dying, 0, "second hit inside 0.5 s is ignored");
        std::thread::sleep(std::time::Duration::from_millis(520));
        server.mobs.hurt(&server, id, 20.0, (2.0, -60.0, 0.5), None);
        for _ in 0..12 {
            server.mobs.tick(&server);
        }
        assert!(!server.mobs.contains(id), "removed after dying");
        assert_eq!(Kind::from_name("minecraft:chicken"), Some(Kind::Chicken));

        // Pathfinding goes around a wall.
        let w = &server.world;
        let stone = blocks::id("stone");
        for z in -3..=3 {
            w.set(3, -60, z, stone);
            w.set(3, -59, z, stone);
        }
        let p = path(w, (0, -60, 0), (6, -60, 0), 400);
        assert_eq!(p.last(), Some(&(6, -60, 0)), "reaches the far side: {p:?}");
        assert!(p.iter().all(|&(x, _, z)| !(x == 3 && (-3..=3).contains(&z))), "never walks through the wall");

        // Feeding two cows makes a baby.
        let a = server.mobs.spawn(&server, Kind::Cow, (10.5, -60.0, 10.5));
        let b = server.mobs.spawn(&server, Kind::Cow, (11.5, -60.0, 10.5));
        assert_eq!(server.mobs.interact(&server, a, Some("minecraft:wheat"), 1), Use::Consume);
        assert_eq!(server.mobs.interact(&server, b, Some("minecraft:wheat"), 1), Use::Consume);
        assert_eq!(server.mobs.interact(&server, b, Some("minecraft:bucket"), 1), Use::Milk);
        let before = server.mobs.count();
        for _ in 0..30 {
            server.mobs.tick(&server);
        }
        assert_eq!(server.mobs.count(), before + 1, "a calf was born");

        // A creeper blast makes a crater; bedrock survives.
        explode(&server, (20.5, -60.0, 20.5), 3.0, Kind::Creeper.display());
        assert!(blocks::is_air(server.world.get(20, -61, 20)), "grass blown away");
        assert!(!blocks::is_air(server.world.get(20, -64, 20)), "bedrock stays");

        // A big slime splits into 2-4 smaller ones when it dies.
        let before = server.mobs.count();
        let slime = server.mobs.spawn_slime(&server, Kind::Slime, (120.5, -60.0, 120.5), 4);
        server.mobs.hurt(&server, slime, 100.0, (121.5, -60.0, 120.5), None);
        let kids = server.mobs.mobs.lock().values().filter(|m| m.kind == Kind::Slime && m.color == 2).count();
        assert!((2..=4).contains(&kids), "split into {kids}");
        assert!(server.mobs.count() >= before + 3);
        assert_eq!(Kind::from_name("cave_spider").map(|k| k.spec().hit_effect), Some(Some((19, 7))));
        assert_eq!(Kind::ALL.len(), 87);

        // An ender pearl thrown straight down lands and teleports its thrower.
        let mut rx = server.events.subscribe();
        throw(&server, Throw::Pearl, 4242, (50.5, -55.0, 50.5), (90.0, 0.0));
        // Read events every tick, so a busy test run can't push the teleport out of the channel.
        let mut landed = None;
        for _ in 0..30 {
            server.mobs.tick(&server);
            loop {
                match rx.try_recv() {
                    Ok(Event::Teleport { id: 4242, feet }) => landed = Some(feet),
                    Ok(_) | Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
            }
            if landed.is_some() {
                break;
            }
        }
        assert_eq!(landed.map(|f| f.1), Some(-60.0), "pearl teleports onto the grass");

        // Lit TNT blows up after its 4 s fuse and leaves a crater.
        ignite_tnt(&server, (30, -60, 30), 40);
        assert!(!blocks::is_air(server.world.get(30, -61, 30)));
        for _ in 0..40 {
            server.mobs.tick(&server);
        }
        assert!(blocks::is_air(server.world.get(30, -61, 30)), "TNT crater");

        // Zombies burn under the noon sun and eventually die.
        server.set_time(6000);
        let z = server.mobs.spawn(&server, Kind::Zombie, (40.5, -60.0, 40.5));
        server.mobs.mobs.lock().get_mut(&z).unwrap().baby = 0; // 1 in 20 spawn as babies, which don't burn
        // Burn damage is 1-in-10 per tick; 30 ticks missed it ~4% of runs.
        for _ in 0..100 {
            server.mobs.tick(&server);
        }
        let hp = server.mobs.mobs.lock().get(&z).map(|m| (m.health, m.burning));
        assert!(hp.is_some_and(|(h, b)| b && h < 20.0), "burning in daylight: {hp:?}");

        // A golem goes after a zombie and hurts it.
        server.set_time(18_000);
        let zombie = server.mobs.spawn(&server, Kind::Zombie, (60.5, -60.0, 60.5));
        let _golem = server.mobs.spawn(&server, Kind::IronGolem, (62.5, -60.0, 60.5));
        for _ in 0..60 {
            server.mobs.tick(&server);
        }
        let zh = server.mobs.mobs.lock().get(&zombie).map(|m| m.health);
        assert!(zh.is_none_or(|h| h < 20.0), "golem fought the zombie: {zh:?}");

        // Shearing drops wool once; taming a wolf eventually works.
        let sheep = server.mobs.spawn(&server, Kind::Sheep, (80.5, -60.0, 80.5));
        server.mobs.interact(&server, sheep, Some("minecraft:shears"), 1);
        assert!(server.mobs.mobs.lock()[&sheep].sheared);
        let wolf = server.mobs.spawn(&server, Kind::Wolf, (90.5, -60.0, 90.5));
        for _ in 0..50 {
            server.mobs.interact(&server, wolf, Some("minecraft:bone"), 7);
        }
        assert_eq!(server.mobs.mobs.lock()[&wolf].owner, Some(7), "tamed");

        // Saved mobs come back (hostiles don't); the tamed wolf keeps its owner key.
        let file = std::env::temp_dir().join(format!("rusted_mobs_{}.txt", std::process::id()));
        let keys: HashMap<i64, i64> = [(7, owner_key("Steve"))].into();
        server.mobs.save(&file, &keys).unwrap();
        let again = Mobs::default();
        let n = again.load(&file, &HashMap::new());
        std::fs::remove_file(&file).ok();
        let saved: Vec<Kind> = again.mobs.lock().values().map(|m| m.kind).collect();
        assert!(n > 0 && !saved.iter().any(|k| k.hostile()), "{saved:?}");
        assert!(again.mobs.lock().values().any(|m| m.kind == Kind::Wolf && m.owner == Some(owner_key("Steve"))));
        assert_eq!(attack_damage(Some("minecraft:diamond_sword")), 8.0);
    }

    #[test]
    fn sheep_eat_grass_and_babies_follow_parents() {
        let _serial = SERIAL.lock();
        let server = Server::new();
        // A sheared sheep on grass eats it within a minute, long before the 3-minute timer.
        let sheep = server.mobs.spawn(&server, Kind::Sheep, (500.5, -60.0, 0.5));
        if let Some(s) = server.mobs.mobs.lock().get_mut(&sheep) {
            (s.sheared, s.regrow, s.baby) = (true, 10_000, 0);
        }
        for _ in 0..(60 * TPS) {
            server.mobs.tick(&server);
            if !server.mobs.mobs.lock()[&sheep].sheared {
                break;
            }
        }
        assert!(!server.mobs.mobs.lock()[&sheep].sheared, "ate grass and grew wool back");

        // A baby walks back toward its parent.
        let cow = server.mobs.spawn(&server, Kind::Cow, (600.5, -60.0, 0.5));
        let calf = server.mobs.spawn(&server, Kind::Cow, (612.5, -60.0, 0.5));
        if let Some(c) = server.mobs.mobs.lock().get_mut(&calf) {
            (c.baby, c.brain.parent_id) = (GROW, Some(cow));
        }
        if let Some(c) = server.mobs.mobs.lock().get_mut(&cow) {
            c.baby = 0;
        }
        let gap = |s: &Mobs| {
            let m = s.mobs.lock();
            (m[&calf].pos.0 - m[&cow].pos.0).abs()
        };
        let before = gap(&server.mobs);
        for _ in 0..(20 * TPS) {
            server.mobs.tick(&server);
        }
        assert!(gap(&server.mobs) < before - 3.0, "calf came back: {before} -> {}", gap(&server.mobs));
    }

    #[test]
    fn lightning_charges_nearby_creepers() {
        let _serial = SERIAL.lock();
        let server = Server::new();
        let near = server.mobs.spawn(&server, Kind::Creeper, (0.5, -60.0, 0.5));
        let far = server.mobs.spawn(&server, Kind::Creeper, (20.5, -60.0, 0.5));
        server.mobs.struck_by_lightning(&server, (1.5, -60.0, 0.5));
        let mobs = server.mobs.mobs.lock();
        assert!(mobs[&near].charged, "within 4 blocks");
        assert!(!mobs[&far].charged, "20 blocks away");
    }

    #[test]
    fn riding_saddles_and_horse_taming() {
        let _serial = SERIAL.lock();
        let server = Server::new();
        server.join(crate::players::tests::sample(7));
        let at = |x: f32| (x, -60.0, 0.5);
        let adult = |id: i64| server.mobs.mobs.lock().get_mut(&id).unwrap().baby = 0;

        // Pigs need a saddle before they can be ridden.
        let pig = server.mobs.spawn(&server, Kind::Pig, at(0.5));
        adult(pig);
        assert!(matches!(server.mobs.interact(&server, pig, None, 7), Use::Nothing));
        assert!(matches!(server.mobs.interact(&server, pig, Some("minecraft:saddle"), 7), Use::Consume));
        assert!(matches!(server.mobs.interact(&server, pig, None, 7), Use::Mount));
        assert!(server.mobs.carries(pig, 7));
        // Boats seat two; the passenger moves up when the driver gets off.
        server.join(crate::players::tests::sample(8));
        server.join(crate::players::tests::sample(9));
        let boat = server.mobs.spawn(&server, Kind::Boat, at(20.5));
        assert!(matches!(server.mobs.interact(&server, boat, None, 9), Use::Mount));
        assert!(matches!(server.mobs.interact(&server, boat, None, 8), Use::Mount));
        assert!(server.mobs.carries(boat, 8));
        server.mobs.dismount(&server, 9);
        assert_eq!(server.mobs.mobs.lock()[&boat].rider, Some(8), "passenger drives now");
        server.mobs.dismount(&server, 8);
        // The ridden pig follows its rider and doesn't wander off on its own.
        server.mobs.ride(pig, 7, (10.0, -57.0, 3.0), 90.0, false);
        server.mobs.tick(&server);
        let p = server.mobs.mobs.lock()[&pig].pos;
        assert!((p.0 - 10.0).abs() < 1e-3 && (p.2 - 3.0).abs() < 1e-3, "{p:?}");
        server.mobs.dismount(&server, 7);
        assert!(!server.mobs.carries(pig, 7));

        // An untamed horse takes no saddle, but can be ridden to tame it.
        let horse = server.mobs.spawn(&server, Kind::Horse, at(20.5));
        adult(horse);
        // (vanilla: you just climb on instead)
        assert!(matches!(server.mobs.interact(&server, horse, Some("minecraft:saddle"), 7), Use::Mount));
        assert!(!server.mobs.mobs.lock()[&horse].saddled);
        server.mobs.dismount(&server, 7);
        // Snacks raise its temper; at 100 it can't refuse.
        for _ in 0..12 {
            server.mobs.interact(&server, horse, Some("minecraft:golden_apple"), 7);
        }
        assert_eq!(server.mobs.mobs.lock()[&horse].temper, 100);
        assert!(matches!(server.mobs.interact(&server, horse, None, 7), Use::Mount));
        for _ in 0..120 {
            server.mobs.tick(&server);
        }
        assert_eq!(server.mobs.mobs.lock()[&horse].owner, Some(7), "tamed after riding it");
        assert!(server.mobs.carries(horse, 7), "and still riding");
        server.mobs.dismount(&server, 7);
        assert!(matches!(server.mobs.interact(&server, horse, Some("minecraft:saddle"), 7), Use::Consume));

        // A horse with no temper always bucks its rider and gets a little calmer.
        let wild = server.mobs.spawn(&server, Kind::Horse, at(40.5));
        adult(wild);
        server.mobs.interact(&server, wild, None, 7);
        for _ in 0..120 {
            server.mobs.tick(&server);
        }
        let m = server.mobs.mobs.lock()[&wild].clone();
        assert!(m.rider.is_none() && m.owner.is_none() && m.temper == 5, "bucked: {:?} {:?} {}", m.rider, m.owner, m.temper);

        // Boats: every wood maps to its item and back, and so do chest boats.
        for w in 0..BOAT_WOOD.len() as u8 {
            assert_eq!(boat_wood(&boat_item(w)), Some((w, false)), "{}", boat_item(w));
            assert_eq!(boat_wood(&chest_boat_item(w)), Some((w, true)), "{}", chest_boat_item(w));
        }
        assert_eq!(boat_item(7), "minecraft:bamboo_raft");
        assert_eq!(boat_wood("minecraft:oak_chest_boat"), Some((0, true)));
        assert_eq!(boat_wood("minecraft:boat"), None);
        // Placed boats are never babies, need no saddle, and don't drift off on their own.
        let boat = server.mobs.spawn_boat(&server, 2, at(60.5), 45.0, false);
        server.mobs.tick(&server);
        let b = server.mobs.mobs.lock()[&boat].clone();
        assert!(b.baby == 0 && b.color == 2 && b.pos == at(60.5), "{:?}", b.pos);
        assert!(matches!(server.mobs.interact(&server, boat, None, 7), Use::Mount));
        server.mobs.dismount(&server, 7);

        // Minecarts: placed on a rail, the server rolls them along it.
        use crate::blocks::{State, any_state, with};
        let rail = |d: i32, kind: &str, on: bool| {
            let base = with(any_state(&format!("minecraft:{kind}")).unwrap(), &[("rail_direction", State::Int(d))]);
            if kind == "rail" { base } else { with(base, &[("rail_data_bit", State::Bool(on))]) }
        };
        for x in 100..120 {
            server.world.set(x, -60, 0, rail(1, "rail", false));
        }
        server.world.set(115, -60, 0, rail(1, "activator_rail", true));
        let cart = server.mobs.spawn_cart(&server, Kind::Minecart, (101, -60, 0));
        assert!(matches!(server.mobs.interact(&server, cart, None, 7), Use::Mount));
        server.mobs.mobs.lock().get_mut(&cart).unwrap().vel = (0.4, 0.0, 0.0);
        for _ in 0..60 {
            server.mobs.tick(&server);
        }
        let c = server.mobs.mobs.lock()[&cart].clone();
        assert!(c.pos.0 > 110.0 && (c.pos.2 - 0.5).abs() < 1e-3, "rolled along the rail: {:?}", c.pos);
        assert!(c.rider.is_none(), "the powered activator rail threw the rider out");

        // A mineshaft spawner makes cave spiders while a player is near, and stops at 6.
        let spawner = (200, -60, 200);
        server.world.set(spawner.0, spawner.1, spawner.2, any_state("minecraft:mob_spawner").unwrap());
        crate::loot::generated(spawner, server.world.get(spawner.0, spawner.1, spawner.2), "normal_mineshaft");
        server.moved(7, (200.5, -60.0 + 1.62, 205.5), (0.0, 0.0), 0.0);
        let spiders = || server.mobs.mobs.lock().values().filter(|m| m.kind == Kind::CaveSpider && m.dying == 0).count();
        // Each try picks 4 random spots, so a try can come up empty; give it a few cycles.
        for _ in 0..(3 * 40 * TPS) {
            if spiders() > 0 {
                break;
            }
            server.mobs.tick(&server);
        }
        assert!(spiders() > 0, "the spawner made cave spiders");
        for _ in 0..(60 * TPS) {
            server.mobs.tick(&server);
        }
        // The cap counts spiders within 8 blocks, so wanderers make room for more, but
        // without it 60 s would make well over 12.
        assert!(spiders() <= 12, "capped: {}", spiders());

        // Saddles and temper are saved with the world.
        let file = std::env::temp_dir().join(format!("rusted_ride_{}.txt", std::process::id()));
        server.mobs.save(&file, &HashMap::new()).unwrap();
        let again = Mobs::default();
        again.load(&file, &HashMap::new());
        std::fs::remove_file(&file).ok();
        let back = again.mobs.lock();
        assert!(back.values().any(|m| m.kind == Kind::Pig && m.saddled));
        assert!(back.values().any(|m| m.kind == Kind::Horse && m.temper == 5));
    }

    #[test]
    fn mineshaft_chest_minecarts_spawn_once_with_loot() {
        let _serial = SERIAL.lock();
        crate::blocks::init();
        let server = Server::new();
        let at = (900, -60, 900);
        crate::loot::chest_cart(at);
        server.mobs.spawn_chest_carts(&server);
        let carts: Vec<i64> = server.mobs.mobs.lock().values().filter(|m| m.kind == Kind::ChestMinecart && m.home == Some(at)).map(|m| m.id).collect();
        assert_eq!(carts.len(), 1, "one cart on the rail");
        // The chunk generates again: no second cart.
        crate::loot::chest_cart(at);
        server.mobs.spawn_chest_carts(&server);
        assert_eq!(server.mobs.mobs.lock().values().filter(|m| m.kind == Kind::ChestMinecart && m.home == Some(at)).count(), 1);
        // Right-click opens its store, which has mineshaft loot the first time.
        let Use::OpenCart(store) = server.mobs.interact(&server, carts[0], None, 1) else { panic!("should open") };
        let loot = crate::loot::take(store, crate::inventory::CHEST_SLOTS).expect("loot on first open");
        assert!(loot.iter().any(Option::is_some));
    }
}


#[cfg(test)]
mod explosion_tests {
    use super::*;

    #[test]
    fn tnt_craters_stone_but_not_under_water() {
        crate::blocks::init();
        let w = crate::world::World::default();
        let stone = crate::blocks::any_state("minecraft:stone").unwrap();
        let water = crate::blocks::any_state("minecraft:water").unwrap();
        for x in -8..=8 {
            for y in -50..=-34 {
                for z in -8..=8 {
                    w.set(x, y, z, stone);
                }
            }
        }
        w.set(0, -42, 0, crate::world::AIR);
        let n = explosion_rays(&w, (0.5, -41.5, 0.5), 4.0, 12345).len();
        // vanilla TNT in stone breaks a few dozen blocks, not a whole radius-4 ball (~270)
        assert!((10..150).contains(&n), "{n} blocks");
        w.set(0, -42, 0, water);
        for d in [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
            w.set(d.0, -42 + d.1, d.2, water);
        }
        assert!(explosion_rays(&w, (0.5, -41.5, 0.5), 4.0, 12345).is_empty(), "water soaks it up");
    }
}

/// Built mobs (vanilla): a carved pumpkin on two snow blocks makes a snow golem; on a T of
/// iron blocks an iron golem; three wither skeleton skulls on a T of soul sand or soul soil
/// the wither. `p` is the block just placed; the blocks are used up.
pub fn summon_from_blocks(server: &Server, p: (i32, i32, i32)) {
    let w = &server.world;
    let name = |q: (i32, i32, i32)| crate::blocks::entry(w.get(q.0, q.1, q.2)).map_or(String::new(), |e| e.name.trim_start_matches("minecraft:").to_string());
    let head = name(p);
    let pumpkin = |n: &str| n == "carved_pumpkin" || n == "lit_pumpkin";
    let soul = |n: &str| n == "soul_sand" || n == "soul_soil";
    let mut built: Option<(Kind, Vec<(i32, i32, i32)>, (i32, i32, i32))> = None;
    if pumpkin(&head) {
        let (b1, b2) = ((p.0, p.1 - 1, p.2), (p.0, p.1 - 2, p.2));
        if name(b1) == "snow" && name(b2) == "snow" {
            built = Some((Kind::SnowGolem, vec![p, b1, b2], b2));
        }
        for (ax, az) in [(1, 0), (0, 1)] {
            let arms = [(b1.0 + ax, b1.1, b1.2 + az), (b1.0 - ax, b1.1, b1.2 - az)];
            if built.is_none() && name(b1) == "iron_block" && name(b2) == "iron_block" && arms.iter().all(|a| name(*a) == "iron_block") {
                built = Some((Kind::IronGolem, vec![p, b1, b2, arms[0], arms[1]], b2));
            }
        }
    }
    if head == "wither_skeleton_skull" {
        'find: for (ax, az) in [(1, 0), (0, 1)] {
            for o in -1..=1 {
                let c = (p.0 - ax * o, p.1, p.2 - az * o);
                let skulls = [(c.0 - ax, c.1, c.2 - az), c, (c.0 + ax, c.1, c.2 + az)];
                let body = [(c.0, c.1 - 1, c.2), (c.0 - ax, c.1 - 1, c.2 - az), (c.0 + ax, c.1 - 1, c.2 + az), (c.0, c.1 - 2, c.2)];
                if skulls.iter().all(|s| name(*s) == "wither_skeleton_skull") && body.iter().all(|b| soul(&name(*b))) {
                    built = Some((Kind::Wither, skulls.iter().chain(body.iter()).copied().collect(), (c.0, c.1 - 2, c.2)));
                    break 'find;
                }
            }
        }
    }
    let Some((kind, blocks_used, feet)) = built else { return };
    for q in blocks_used {
        server.set_block(0, q.0, q.1, q.2, crate::world::AIR);
    }
    let at = (feet.0 as f32 + 0.5, feet.1 as f32, feet.2 as f32 + 0.5);
    let id = server.mobs.spawn(server, kind, at);
    if kind == Kind::IronGolem
        && let Some(m) = server.mobs.mobs.lock().get_mut(&id)
    {
        m.kept = true; // player-built golems stay
    }
    if kind == Kind::Wither {
        // It wakes with a bang (vanilla: power 7 blast once it's charged up).
        let _ = server.events.send(Event::Relay { from: 0, packet: Box::new(crate::packets::play_sound("mob.wither.spawn", at)) });
        let s = server.clone();
        server.scheduler.run_later(200, move |_| explode(&s, (at.0, at.1 + 1.5, at.2), 7.0, "Wither"));
    }
}

#[cfg(test)]
mod summon_tests {
    use super::*;

    #[test]
    fn pumpkin_on_snow_and_skulls_on_soul_sand() {
        crate::blocks::init();
        let server = Server::new();
        let b = |n: &str| crate::blocks::any_state(n).unwrap();
        server.world.set(500, -60, 0, b("minecraft:snow"));
        server.world.set(500, -59, 0, b("minecraft:snow"));
        server.world.set(500, -58, 0, b("minecraft:carved_pumpkin"));
        summon_from_blocks(&server, (500, -58, 0));
        assert!(server.mobs.snapshot().iter().any(|(k, _)| *k == Kind::SnowGolem));
        assert_eq!(server.world.get(500, -58, 0), crate::world::AIR, "blocks used up");
        for x in 599..=601 {
            server.world.set(x, -59, 0, b("minecraft:soul_sand"));
            server.world.set(x, -58, 0, b("minecraft:wither_skeleton_skull"));
        }
        server.world.set(600, -60, 0, b("minecraft:soul_sand"));
        summon_from_blocks(&server, (601, -58, 0));
        assert!(server.mobs.snapshot().iter().any(|(k, _)| *k == Kind::Wither));
    }
}

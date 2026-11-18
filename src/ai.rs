//! Mob brains, after PNX's behaviour system (`entity/ai`): sensors fill a mob's memory now and
//! then, and each tick the highest-priority behaviour that applies runs. A higher one
//! interrupts a lower one; a running one keeps going until it's done or stops applying.
//!
//! PNX keys memory by string and keeps behaviours as objects. Here memory is a plain struct
//! and behaviours an enum, which is all the mobs need and is easy to test.

/// What a mob can be doing, highest priority first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Behavior {
    /// Hurt: run away for a while.
    Panic,
    /// Hostiles: go after the target player.
    Attack,
    /// In love with a partner nearby: walk to it and breed.
    Breed,
    /// A player holds this mob's food: follow them.
    Tempt,
    /// Lost sight of the target: walk to where it was last seen.
    Search,
    /// Zombies and illagers with no player around: go for a villager.
    HuntVillager,
    /// A baby too far from its parent: walk back to it.
    FollowParent,
    /// Burning in daylight (skeletons, strays), or a villager outside at night: find a roof.
    SeekShade,
    /// Villagers by day: walk back to their job block.
    GoToWork,
    /// A sheared sheep on grass: eat it and grow wool back.
    EatGrass,
    /// Walk somewhere random nearby.
    Roam,
    #[default]
    Idle,
}

impl Behavior {
    pub fn priority(self) -> u8 {
        match self {
            Behavior::Panic => 11,
            Behavior::Attack => 10,
            Behavior::Breed => 9,
            Behavior::Tempt => 8,
            Behavior::Search => 7,
            Behavior::HuntVillager => 6,
            Behavior::FollowParent => 5,
            Behavior::SeekShade => 4,
            Behavior::GoToWork => 3,
            Behavior::EatGrass => 2,
            Behavior::Roam => 1,
            Behavior::Idle => 0,
        }
    }

    /// How much faster than walking pace the mob moves doing this.
    pub fn speed(self) -> f32 {
        match self {
            Behavior::Panic => 2.0,
            Behavior::Breed | Behavior::FollowParent | Behavior::SeekShade => 1.2,
            _ => 1.0,
        }
    }
}

/// What the mob's sensors last saw. Positions are feet positions.
#[derive(Clone, Debug, Default)]
pub struct Memory {
    pub nearest_player: Option<(i64, (f32, f32, f32), f32)>,
    /// The nearest player holding this mob's food, within 8 blocks.
    pub feeder: Option<(i64, (f32, f32, f32), f32)>,
    /// The nearest same-kind mob in love, within 8 blocks.
    pub mate: Option<(i64, (f32, f32, f32), f32)>,
    /// Babies: the parent and where it is.
    pub parent: Option<(i64, (f32, f32, f32), f32)>,
    /// Standing on grass it could eat.
    pub on_grass: bool,
    /// Hostiles: the player they're after.
    pub target: Option<(i64, (f32, f32, f32), f32)>,
    /// Where the target was last seen, and ticks left to look for them there.
    pub last_seen: Option<((f32, f32, f32), u32)>,
    /// Zombies and illagers: a villager nearby.
    pub villager: Option<(i64, (f32, f32, f32))>,
    /// Something to run from (villagers: a zombie).
    pub threat: Option<(f32, f32, f32)>,
    /// Villagers: their job block, while it's daytime and they're away from it.
    pub work: Option<(i32, i32, i32)>,
    /// Villagers: outside at night, wanting a roof.
    pub wants_cover: bool,
}

/// A mob's AI state: its memory, what it's doing and since when.
#[derive(Clone, Debug, Default)]
pub struct Brain {
    pub memory: Memory,
    pub doing: Behavior,
    /// Ticks spent on the current behaviour.
    pub ticks: u32,
    /// Ticks until the sensors run again.
    pub sense_in: u32,
    /// Babies: who their parent is (kept across sensor runs).
    pub parent_id: Option<i64>,
}

/// The facts behaviours are chosen from, read off the mob and its memory.
#[derive(Clone, Copy, Debug, Default)]
pub struct Facts {
    pub panicking: bool,
    pub in_love: bool,
    pub baby: bool,
    /// Sheep with no wool.
    pub sheared: bool,
    /// Still walking somewhere (a roam or eat isn't finished).
    pub has_path: bool,
    /// A random roll in 0..1000 this tick, for behaviours that start by chance.
    pub roll: u32,
    /// Burning in daylight, and the kind that looks for shade.
    pub sunburnt: bool,
}

/// Babies wander back when their parent is further than this (squared).
const PARENT_FAR: f32 = 6.0 * 6.0;

impl Brain {
    /// Whether `b` could start or keep running right now.
    fn applies(&self, b: Behavior, f: &Facts) -> bool {
        let m = &self.memory;
        match b {
            Behavior::Panic => f.panicking,
            Behavior::Attack => m.target.is_some(),
            Behavior::HuntVillager => m.villager.is_some(),
            Behavior::Search => m.last_seen.is_some_and(|(_, left)| left > 0),
            Behavior::SeekShade => f.sunburnt,
            Behavior::GoToWork => m.work.is_some(),
            Behavior::Breed => f.in_love && m.mate.is_some(),
            Behavior::Tempt => !f.baby && m.feeder.is_some(),
            Behavior::FollowParent => f.baby && m.parent.is_some_and(|p| p.2 > PARENT_FAR),
            // Starting is a 1-in-50 chance a tick (vanilla EatBlockGoal); once eating it keeps going.
            Behavior::EatGrass => f.sheared && m.on_grass && (self.doing == Behavior::EatGrass || f.roll < 20),
            // Roams start now and then and run until the walk is done.
            Behavior::Roam => (self.doing == Behavior::Roam && f.has_path) || f.roll < 12,
            Behavior::Idle => true,
        }
    }

    /// Picks what to do this tick. Returns the new behaviour if it changed (the caller then
    /// drops the old one's path and starts the new one).
    pub fn decide(&mut self, f: &Facts) -> Option<Behavior> {
        const ALL: [Behavior; 12] = [
            Behavior::Panic,
            Behavior::Attack,
            Behavior::Breed,
            Behavior::Tempt,
            Behavior::Search,
            Behavior::HuntVillager,
            Behavior::FollowParent,
            Behavior::SeekShade,
            Behavior::GoToWork,
            Behavior::EatGrass,
            Behavior::Roam,
            Behavior::Idle,
        ];
        let keep = self.applies(self.doing, f);
        let best = ALL.into_iter().find(|&b| self.applies(b, f)).unwrap_or(Behavior::Idle);
        // Keep going unless something more important comes up or this stopped applying.
        let next = if keep && best.priority() <= self.doing.priority() { self.doing } else { best };
        self.ticks += 1;
        if next == self.doing {
            return None;
        }
        self.doing = next;
        self.ticks = 0;
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brain() -> Brain {
        Brain::default()
    }

    #[test]
    fn priorities_and_interruptions() {
        let mut b = brain();
        let calm = Facts { roll: 999, ..Default::default() };
        assert_eq!(b.decide(&calm), None, "idle stays idle");

        // A lucky roll starts a roam, which continues while it has somewhere to go.
        assert_eq!(b.decide(&Facts { roll: 0, ..calm }), Some(Behavior::Roam));
        assert_eq!(b.decide(&Facts { has_path: true, ..calm }), None);
        assert_eq!(b.decide(&calm), Some(Behavior::Idle), "walk finished");

        // A player with food interrupts a roam.
        b.decide(&Facts { roll: 0, ..calm });
        b.memory.feeder = Some((1, (0.0, 0.0, 0.0), 9.0));
        assert_eq!(b.decide(&Facts { has_path: true, ..calm }), Some(Behavior::Tempt));
        // Getting hurt beats everything.
        assert_eq!(b.decide(&Facts { panicking: true, ..calm }), Some(Behavior::Panic));
        // Once the panic is over, back to the food.
        assert_eq!(b.decide(&calm), Some(Behavior::Tempt));

        // Love needs a mate nearby.
        b.memory.feeder = None;
        assert_eq!(b.decide(&Facts { in_love: true, ..calm }), Some(Behavior::Idle));
        b.memory.mate = Some((2, (1.0, 0.0, 1.0), 4.0));
        assert_eq!(b.decide(&Facts { in_love: true, ..calm }), Some(Behavior::Breed));
    }

    #[test]
    fn babies_follow_parents_and_sheep_eat_grass() {
        let mut b = brain();
        let calm = Facts { roll: 999, ..Default::default() };
        b.memory.parent = Some((5, (10.0, 0.0, 0.0), 100.0));
        assert_eq!(b.decide(&Facts { baby: true, ..calm }), Some(Behavior::FollowParent));
        b.memory.parent = Some((5, (1.0, 0.0, 0.0), 1.0));
        assert_eq!(b.decide(&Facts { baby: true, ..calm }), Some(Behavior::Idle), "close enough");
        // Babies aren't tempted by food.
        b.memory.feeder = Some((1, (0.0, 0.0, 0.0), 4.0));
        assert_eq!(b.decide(&Facts { baby: true, ..calm }), None);

        let mut s = brain();
        s.memory.on_grass = true;
        assert_eq!(s.decide(&Facts { sheared: true, ..calm }), None, "eating starts by chance");
        assert_eq!(s.decide(&Facts { sheared: true, roll: 5, ..calm }), Some(Behavior::EatGrass));
        assert_eq!(s.decide(&Facts { sheared: true, ..calm }), None, "keeps eating");
        assert_eq!(s.decide(&calm), Some(Behavior::Idle), "wool's back");
    }

    #[test]
    fn hostiles_attack_search_hunt_and_hide() {
        let mut z = brain();
        let calm = Facts { roll: 999, ..Default::default() };
        z.memory.villager = Some((9, (5.0, 0.0, 5.0)));
        assert_eq!(z.decide(&calm), Some(Behavior::HuntVillager));
        // A player shows up: they come first.
        z.memory.target = Some((1, (3.0, 0.0, 0.0), 9.0));
        assert_eq!(z.decide(&calm), Some(Behavior::Attack));
        // Lost them: go look where they were, ahead of the villager.
        z.memory.target = None;
        z.memory.last_seen = Some(((3.0, 0.0, 0.0), 50));
        assert_eq!(z.decide(&calm), Some(Behavior::Search));
        // Gave up looking: back to the villager.
        z.memory.last_seen = Some(((3.0, 0.0, 0.0), 0));
        assert_eq!(z.decide(&calm), Some(Behavior::HuntVillager));

        let mut s = brain();
        assert_eq!(s.decide(&Facts { sunburnt: true, ..calm }), Some(Behavior::SeekShade));
        s.memory.target = Some((1, (3.0, 0.0, 0.0), 9.0));
        assert_eq!(s.decide(&Facts { sunburnt: true, ..calm }), Some(Behavior::Attack), "a target beats hiding");
    }

    #[test]
    fn villagers_work_by_day_and_hide_by_night() {
        let mut v = brain();
        let calm = Facts { roll: 999, ..Default::default() };
        v.memory.work = Some((3, 64, 3));
        assert_eq!(v.decide(&calm), Some(Behavior::GoToWork));
        // Night outside: cover first.
        assert_eq!(v.decide(&Facts { sunburnt: true, ..calm }), Some(Behavior::SeekShade));
        // A zombie: run.
        assert_eq!(v.decide(&Facts { panicking: true, ..calm }), Some(Behavior::Panic));
    }
}

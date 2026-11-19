//! Survival health: fall and void damage, death, regen. No packets in here, it just eats
//! movement reports, which makes it easy to test.

pub const MAX_HEALTH: f32 = 20.0;

/// Max health per player id when it isn't 20 (lifesteal hearts). Sessions pick changes up
/// within a second.
static MAXES: parking_lot::Mutex<Option<std::collections::HashMap<i64, f32>>> = parking_lot::Mutex::new(None);

pub fn max_of(id: i64) -> f32 {
    MAXES.lock().as_ref().and_then(|m| m.get(&id).copied()).unwrap_or(MAX_HEALTH)
}

/// Sets a player's max health (2 to 200, i.e. 1 to 100 hearts).
pub fn set_max(id: i64, max: f32) {
    MAXES.lock().get_or_insert_with(Default::default).insert(id, max.clamp(2.0, 200.0));
}
/// Vanilla kills you 64 blocks under the world floor.
pub const VOID_Y: f32 = -128.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Damage {
    Fall(f32),
    Void,
    /// /kill
    Kill,
    /// empty food bar, 1 per step
    Starve,
    Drown,
    Lava,
    Fire,
    /// mob hit (damage, mob name)
    Mob(f32, &'static str),
    Shot(f32, &'static str),
    Explosion(f32, &'static str),
    Magic(f32),
    Wither(f32),
}

pub struct Health {
    pub health: f32,
    /// this player's max health (20 unless a plugin changed it)
    pub max: f32,
    /// Highest feet y while falling, reset once vertical movement stops.
    fall_from: Option<f32>,
    last_y: Option<f32>,
    pub dead: bool,
}

impl Default for Health {
    fn default() -> Self {
        Self { health: MAX_HEALTH, max: MAX_HEALTH, fall_from: None, last_y: None, dead: false }
    }
}

impl Health {
    /// Feed one movement report. `takes_damage` is off in creative/spectator or when flying,
    /// landing in water cancels fall damage. Returns damage to apply.
    pub fn on_move(&mut self, feet_y: f32, takes_damage: bool, in_liquid: bool) -> Option<Damage> {
        let last = self.last_y.replace(feet_y);
        if self.dead {
            return None;
        }
        if feet_y < VOID_Y {
            return takes_damage.then_some(Damage::Void);
        }
        let last = last?;
        if !takes_damage || in_liquid {
            self.fall_from = None; // flying, creative or water: falls don't count
            return None;
        }
        if feet_y < last - 0.001 {
            self.fall_from = Some(self.fall_from.map_or(last, |f| f.max(last)));
            return None;
        }
        // stopped going down, so we landed (or hit a ceiling)
        let fell = self.fall_from.take().map_or(0.0, |from| from - feet_y);
        let damage = (fell - 3.0).ceil();
        (damage > 0.0).then_some(Damage::Fall(damage))
    }

    /// True if this killed the player.
    pub fn hurt(&mut self, d: Damage) -> bool {
        if self.dead {
            return false;
        }
        self.health = match d {
            Damage::Fall(n) => (self.health - n).max(0.0),
            Damage::Void | Damage::Kill => 0.0,
            Damage::Starve | Damage::Fire => (self.health - 1.0).max(0.0),
            Damage::Drown => (self.health - 2.0).max(0.0),
            Damage::Lava => (self.health - 4.0).max(0.0),
            Damage::Mob(n, _) | Damage::Shot(n, _) | Damage::Explosion(n, _) | Damage::Magic(n) | Damage::Wither(n) => (self.health - n).max(0.0),
        };
        self.dead = self.health <= 0.0;
        self.dead
    }

    /// Natural regen step, true if health changed.
    pub fn regenerate(&mut self) -> bool {
        if self.dead || self.health >= self.max {
            return false;
        }
        self.health = (self.health + 1.0).min(self.max);
        true
    }

    pub fn respawn(&mut self) {
        let max = self.max;
        *self = Self { health: max, max, ..Self::default() };
    }
}

pub fn death_message(name: &str, d: Damage) -> String {
    match d {
        Damage::Fall(_) => format!("{name} fell from a high place"),
        Damage::Void => format!("{name} fell out of the world"),
        Damage::Kill => format!("{name} died"),
        Damage::Starve => format!("{name} starved to death"),
        Damage::Drown => format!("{name} drowned"),
        Damage::Lava => format!("{name} tried to swim in lava"),
        Damage::Fire => format!("{name} went up in flames"),
        Damage::Mob(_, by) => format!("{name} was slain by {by}"),
        Damage::Shot(_, by) => format!("{name} was shot by {by}"),
        Damage::Explosion(_, by) => format!("{name} was blown up by {by}"),
        Damage::Magic(_) => format!("{name} was killed by magic"),
        Damage::Wither(_) => format!("{name} withered away"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fall(h: &mut Health, from: f32, to: f32) -> Option<Damage> {
        let mut y = from;
        h.on_move(y, true, false);
        while y > to {
            y = (y - 0.5).max(to);
            if let Some(d) = h.on_move(y, true, false) {
                return Some(d);
            }
        }
        h.on_move(to, true, false) // standing on the ground
    }

    #[test]
    fn short_falls_are_free_long_ones_hurt() {
        let mut h = Health::default();
        assert_eq!(fall(&mut h, 70.0, 67.0), None, "3 blocks: no damage");
        assert_eq!(fall(&mut h, 70.0, 60.0), Some(Damage::Fall(7.0)), "10 blocks: 7 damage");
    }

    #[test]
    fn no_damage_when_flying_or_into_water() {
        let mut h = Health::default();
        h.on_move(100.0, false, false);
        assert_eq!(h.on_move(50.0, false, false), None);
        assert_eq!(h.on_move(50.0, true, false), None, "fall while flying doesn't count after landing");
        let mut h = Health::default();
        h.on_move(100.0, true, false);
        h.on_move(80.0, true, false);
        assert_eq!(h.on_move(60.0, true, true), None, "landed in water");
        assert_eq!(h.on_move(60.0, true, true), None);
    }

    #[test]
    fn death_void_regen_and_respawn() {
        let mut h = Health::default();
        assert!(!h.hurt(Damage::Fall(15.0)));
        assert!(h.regenerate());
        assert_eq!(h.health, 6.0);
        assert!(h.hurt(Damage::Fall(10.0)), "dies at 0");
        assert!(h.dead && !h.regenerate());
        assert_eq!(h.on_move(-200.0, true, false), None, "dead players take no more damage");
        h.respawn();
        assert_eq!(h.health, MAX_HEALTH);
        assert_eq!(h.on_move(-200.0, true, false), Some(Damage::Void));
        assert_eq!(death_message("Steve", Damage::Void), "Steve fell out of the world");
    }
}

//! Minecart movement on rails. Numbers from PNX `EntityMinecartAbstract`, rail shapes from
//! the Minecraft Wiki. Minecarts aren't steered by the client like horses and boats, so the
//! server moves them.

use crate::blocks::{State, entry, state};
use crate::world::World;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RailKind {
    Plain,
    Powered,
    Detector,
    Activator,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rail {
    /// `rail_direction`: 0 N-S, 1 E-W, 2-5 ascending east/west/north/south, 6-9 curves.
    pub shape: u8,
    pub kind: RailKind,
    /// `rail_data_bit`: powered / activated.
    pub on: bool,
    pub at: (i32, i32, i32),
}

/// The two neighbours each rail shape connects, as (dx, dy, dz). dy -1 is the low end of a ramp.
const ENDS: [[(i32, i32, i32); 2]; 10] = [
    [(0, 0, -1), (0, 0, 1)],
    [(-1, 0, 0), (1, 0, 0)],
    [(-1, -1, 0), (1, 0, 0)],
    [(-1, 0, 0), (1, -1, 0)],
    [(0, 0, -1), (0, -1, 1)],
    [(0, -1, -1), (0, 0, 1)],
    [(0, 0, 1), (1, 0, 0)],
    [(0, 0, 1), (-1, 0, 0)],
    [(0, 0, -1), (-1, 0, 0)],
    [(0, 0, -1), (1, 0, 0)],
];

/// Blocks per tick on each axis (8 blocks a second).
pub const MAX_SPEED: f32 = 0.4;
const GRAVITY: f32 = 0.04;

pub fn rail_at(w: &World, x: i32, y: i32, z: i32) -> Option<Rail> {
    let b = w.get(x, y, z);
    let kind = match entry(b)?.name.as_str() {
        "minecraft:rail" => RailKind::Plain,
        "minecraft:golden_rail" => RailKind::Powered,
        "minecraft:detector_rail" => RailKind::Detector,
        "minecraft:activator_rail" => RailKind::Activator,
        _ => return None,
    };
    let shape = match state(b, "rail_direction") {
        Some(State::Int(d)) => (*d).clamp(0, 9) as u8,
        _ => 0,
    };
    let on = matches!(state(b, "rail_data_bit"), Some(State::Bool(true)));
    Some(Rail { shape, kind, on, at: (x, y, z) })
}

/// The rail a cart at `pos` (its feet) is on: in its block, or one below when it's just left a ramp.
pub fn find(w: &World, pos: (f32, f32, f32)) -> Option<Rail> {
    let (x, y, z) = (pos.0.floor() as i32, pos.1.floor() as i32, pos.2.floor() as i32);
    rail_at(w, x, y, z).or_else(|| rail_at(w, x, y - 1, z))
}

/// One tick on `rail`. `push` is the rider's yaw while they hold forward. Returns the new
/// position and velocity, snapped to the track.
pub fn on_rail(
    w: &World,
    rail: Rail,
    pos: (f32, f32, f32),
    vel: (f32, f32, f32),
    ridden: bool,
    push: Option<f32>,
    solid: impl Fn(i32) -> bool,
) -> ((f32, f32, f32), (f32, f32, f32)) {
    let [a, b] = ENDS[rail.shape as usize];
    let (cx, cz) = ((b.0 - a.0) as f32, (b.2 - a.2) as f32);
    let len = (cx * cx + cz * cz).sqrt();
    let (cx, cz) = (cx / len, cz / len);
    // Velocity follows the track: keep the speed, point it along the rail.
    let mut speed = (vel.0 * vel.0 + vel.2 * vel.2).sqrt().min(2.0);
    let mut dir = if vel.0 * cx + vel.2 * cz < 0.0 { -1.0 } else { 1.0 };
    // A rider holding forward gets a nearly stopped cart going the way they look.
    if let Some(yaw) = push
        && speed * speed < 0.01
    {
        let (px, pz) = (-yaw.to_radians().sin(), yaw.to_radians().cos());
        let along = px * cx + pz * cz;
        if along.abs() > 0.1 {
            dir = along.signum();
            speed = 0.1;
        }
    }
    let ascending = (2..=5).contains(&rail.shape);
    if ascending {
        // Downhill runs from the high end (dy 0) to the low end (dy -1).
        let downhill = if a.1 < b.1 { -1.0 } else { 1.0 };
        speed += if dir == downhill { 1.0 / 128.0 } else { -1.0 / 128.0 };
        if speed < 0.0 {
            speed = -speed;
            dir = -dir;
        }
    }
    match (rail.kind, rail.on) {
        (RailKind::Powered, false) => speed = if speed < 0.03 { 0.0 } else { speed * 0.5 },
        (RailKind::Powered, true) if speed > 0.01 => speed += 0.06,
        (RailKind::Powered, true) if rail.shape < 2 => {
            // Stopped on a powered rail: kick it away from a solid block at one end.
            let (x, y, z) = rail.at;
            let behind = |d: f32| solid(w.get(x - (cx * d) as i32, y, z - (cz * d) as i32));
            if behind(1.0) {
                (dir, speed) = (1.0, 0.02);
            } else if behind(-1.0) {
                (dir, speed) = (-1.0, 0.02);
            }
        }
        _ => {}
    }
    speed *= if ridden { 0.997 } else { 0.96 };
    let (vx, vz) = (cx * dir * speed, cz * dir * speed);
    let step = (vx.clamp(-MAX_SPEED, MAX_SPEED), vz.clamp(-MAX_SPEED, MAX_SPEED));
    // Stop against a solid block instead of driving into it.
    let ahead = (pos.0 + step.0, pos.2 + step.1);
    let (x, y, z) = rail.at;
    let wall = solid(w.get(ahead.0.floor() as i32, y, ahead.1.floor() as i32))
        && (ahead.0.floor() as i32, ahead.1.floor() as i32) != (x, z);
    if wall {
        return (snap(rail, pos), (0.0, 0.0, 0.0));
    }
    (snap(rail, (ahead.0, pos.1, ahead.1)), (vx, 0.0, vz))
}

/// Puts `p` on the rail's centre line, at the ramp's height if it's sloped.
fn snap(rail: Rail, p: (f32, f32, f32)) -> (f32, f32, f32) {
    let [a, b] = ENDS[rail.shape as usize];
    let (x, y, z) = rail.at;
    let (ox, oz) = (x as f32 + 0.5, z as f32 + 0.5);
    let start = (ox + a.0 as f32 * 0.5, oz + a.2 as f32 * 0.5);
    let end = (ox + b.0 as f32 * 0.5, oz + b.2 as f32 * 0.5);
    let (dx, dz) = (end.0 - start.0, end.1 - start.1);
    let span = dx * dx + dz * dz;
    let t = ((p.0 - start.0) * dx + (p.2 - start.1) * dz) / span;
    let ascending = (2..=5).contains(&rail.shape);
    let (ha, hb) = if ascending { ((a.1 == 0) as i32 as f32, (b.1 == 0) as i32 as f32) } else { (0.0, 0.0) };
    let h = ha + (hb - ha) * t.clamp(0.0, 1.0);
    (start.0 + dx * t, y as f32 + h, start.1 + dz * t)
}

/// How many of this rail's two ends lead to another rail (level, or a block up or down).
fn links(w: &World, r: Rail) -> usize {
    let (x, y, z) = r.at;
    ENDS[r.shape as usize]
        .iter()
        .filter(|(dx, _, dz)| (-1..=1).any(|dy| rail_at(w, x + dx, y + dy, z + dz).is_some()))
        .count()
}

/// The shape a rail at `at` should take from the rails around it (Minecraft Wiki rules):
/// ramp up toward a rail one block higher, join two neighbours (curving if allowed), or
/// line up with one. Alone it runs north-south.
pub fn shape_for(w: &World, at: (i32, i32, i32), can_curve: bool) -> u8 {
    let (x, y, z) = at;
    let level = |dx: i32, dz: i32| rail_at(w, x + dx, y, z + dz).is_some() || rail_at(w, x + dx, y - 1, z + dz).is_some();
    // Ramps first, preferring west, east, south, north.
    for (dx, dz, shape) in [(-1, 0, 3), (1, 0, 2), (0, 1, 5), (0, -1, 4)] {
        if rail_at(w, x + dx, y + 1, z + dz).is_some() {
            return shape;
        }
    }
    let (n, s, west, e) = (level(0, -1), level(0, 1), level(-1, 0), level(1, 0));
    if can_curve && (n || s) && (west || e) {
        return match (s, e) {
            (true, true) => 6,
            (true, false) => 7,
            (false, false) => 8,
            (false, true) => 9,
        };
    }
    if (west || e) && !(n || s) || !can_curve && (west || e) { 1 } else { 0 }
}

/// A rail was placed at `at`: its shape, plus any neighbour with a loose end turning to meet it.
pub fn placed(w: &World, at: (i32, i32, i32)) -> Vec<((i32, i32, i32), i32)> {
    let Some(r) = rail_at(w, at.0, at.1, at.2) else { return vec![] };
    let with_shape = |r: Rail, shape: u8| crate::blocks::with(w.get(r.at.0, r.at.1, r.at.2), &[("rail_direction", State::Int(shape as i32))]);
    let curves = |r: Rail| r.kind == RailKind::Plain;
    let mut out = vec![(at, with_shape(r, shape_for(w, at, curves(r))))];
    // Apply the new rail's shape before reshaping neighbours toward it.
    w.set(at.0, at.1, at.2, out[0].1);
    for (dx, dz) in [(0, -1), (0, 1), (-1, 0), (1, 0)] {
        for dy in -1..=1 {
            let n = (at.0 + dx, at.1 + dy, at.2 + dz);
            if let Some(nr) = rail_at(w, n.0, n.1, n.2)
                && links(w, nr) < 2
            {
                let shape = shape_for(w, n, curves(nr));
                if shape != nr.shape {
                    out.push((n, with_shape(nr, shape)));
                }
            }
        }
    }
    out
}

/// Off the rails: falls and slows down fast (PNX derailed and flying drag).
pub fn off_rail(vel: (f32, f32, f32), on_ground: bool) -> (f32, f32, f32) {
    let v = (vel.0.clamp(-MAX_SPEED, MAX_SPEED), vel.1 - GRAVITY, vel.2.clamp(-MAX_SPEED, MAX_SPEED));
    let k = if on_ground { 0.5 } else { 0.95 };
    (v.0 * k, v.1 * 0.98, v.2 * k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::{any_state, with};

    fn rail(shape: i32) -> i32 {
        with(any_state("minecraft:rail").unwrap(), &[("rail_direction", State::Int(shape))])
    }

    fn powered(shape: i32, on: bool) -> i32 {
        with(any_state("minecraft:golden_rail").unwrap(), &[("rail_direction", State::Int(shape)), ("rail_data_bit", State::Bool(on))])
    }

    fn run(w: &World, mut pos: (f32, f32, f32), mut vel: (f32, f32, f32), ticks: u32, push: Option<f32>) -> ((f32, f32, f32), (f32, f32, f32)) {
        let solid = |b: i32| entry(b).is_some_and(|e| e.name == "minecraft:stone");
        for _ in 0..ticks {
            match find(w, pos) {
                Some(r) => (pos, vel) = on_rail(w, r, pos, vel, true, push, solid),
                None => break,
            }
        }
        (pos, vel)
    }

    #[test]
    fn rails_carry_brake_boost_and_climb() {
        crate::blocks::init();
        let w = World::flat(vec![]);
        let y = 10;
        // A straight east-west line: a pushed cart rolls along it and stays centred.
        for x in 0..20 {
            w.set(x, y, 0, rail(1));
        }
        let (p, v) = run(&w, (2.5, y as f32, 0.5), (0.2, 0.0, 0.05), 10, None);
        assert!(p.0 > 4.0 && (p.2 - 0.5).abs() < 1e-4 && v.2 == 0.0, "{p:?} {v:?}");

        // An unpowered powered rail stops it within a block or two.
        w.set(10, y, 0, powered(1, false));
        w.set(11, y, 0, powered(1, false));
        let (p, v) = run(&w, (8.5, y as f32, 0.5), (0.3, 0.0, 0.0), 30, None);
        assert!(v.0 == 0.0 && p.0 < 12.5, "braked: {p:?} {v:?}");

        // Powered, it speeds the cart up to the 8 blocks/s cap.
        w.set(10, y, 0, powered(1, true));
        w.set(11, y, 0, powered(1, true));
        let (_, v) = run(&w, (10.2, y as f32, 0.5), (0.1, 0.0, 0.0), 3, None);
        assert!(v.0 > 0.15, "boosted: {v:?}");

        // A rider holding forward starts a stopped cart.
        let (p, _) = run(&w, (3.5, y as f32, 0.5), (0.0, 0.0, 0.0), 5, Some(-90.0));
        assert!(p.0 > 3.6, "pushed east: {p:?}");

        // Up a ramp: the cart rises by a block.
        for x in 0..6 {
            w.set(x, y, 5, rail(1));
        }
        w.set(6, y, 5, rail(2)); // ascending east
        for x in 7..12 {
            w.set(x, y + 1, 5, rail(1));
        }
        let (p, _) = run(&w, (3.5, y as f32, 5.5), (0.4, 0.0, 0.0), 12, None);
        assert!(p.1 >= y as f32 + 0.99 && p.0 > 7.0, "climbed: {p:?}");

        // A curve turns it from east-west to north-south.
        for x in 0..5 {
            w.set(x, y, 10, rail(1));
        }
        w.set(5, y, 10, rail(7)); // south + west
        for z in 11..16 {
            w.set(5, y, z, rail(0));
        }
        let (p, _) = run(&w, (1.5, y as f32, 10.5), (0.4, 0.0, 0.0), 20, None);
        assert!((p.0 - 5.5).abs() < 1e-3 && p.2 > 11.0, "turned south: {p:?}");

        // Placing: a lone rail runs north-south, the next one east of it turns both east-west,
        // a third one south of the end makes a curve, and a rail one block up makes a ramp.
        let place = |at: (i32, i32, i32), b: i32| {
            w.set(at.0, at.1, at.2, b);
            for (p, b) in placed(&w, at) {
                w.set(p.0, p.1, p.2, b);
            }
        };
        let shape = |at: (i32, i32, i32)| rail_at(&w, at.0, at.1, at.2).unwrap().shape;
        let py = 40;
        place((0, py, 0), rail(0));
        assert_eq!(shape((0, py, 0)), 0, "alone: north-south");
        place((1, py, 0), rail(0));
        assert_eq!((shape((0, py, 0)), shape((1, py, 0))), (1, 1), "both east-west");
        place((1, py, 1), rail(0));
        assert_eq!(shape((1, py, 0)), 7, "end turned into a south-west curve");
        place((3, py + 1, 0), rail(0));
        place((2, py, 0), rail(0));
        assert_eq!(shape((2, py, 0)), 2, "ramp up east toward the higher rail");
        // Powered rails never curve.
        place((10, py, 0), powered(0, false));
        place((11, py, 0), rail(0));
        place((10, py, 1), rail(0));
        assert!(shape((10, py, 0)) < 2, "powered rail stays straight");

        // A block at the end of the line stops it.
        w.set(20, y, 0, any_state("minecraft:stone").unwrap());
        let (p, v) = run(&w, (17.5, y as f32, 0.5), (0.4, 0.0, 0.0), 20, None);
        assert!(p.0 < 20.0 && v.0 == 0.0, "stopped at the wall: {p:?}");
    }
}

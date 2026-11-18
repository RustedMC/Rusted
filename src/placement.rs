//! Vanilla block placement: facing, slab/stair halves, doors and beds.
//! Callers check for space and apply the result.

use crate::blocks::{State, has_state, with};

/// Horizontal look direction from yaw (0 = +z south, 90 = -x west).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    South,
    West,
    North,
    East,
}

impl Facing {
    pub fn from_yaw(yaw: f32) -> Self {
        match (((yaw.rem_euclid(360.0) + 45.0) / 90.0) as i32) % 4 {
            0 => Facing::South,
            1 => Facing::West,
            2 => Facing::North,
            _ => Facing::East,
        }
    }

    fn opposite(self) -> Self {
        match self {
            Facing::South => Facing::North,
            Facing::West => Facing::East,
            Facing::North => Facing::South,
            Facing::East => Facing::West,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Facing::South => "south",
            Facing::West => "west",
            Facing::North => "north",
            Facing::East => "east",
        }
    }

    /// Legacy horizontal index (south 0, west 1, north 2, east 3).
    fn legacy(self) -> i32 {
        self as i32
    }

    fn step(self) -> (i32, i32) {
        match self {
            Facing::South => (0, 1),
            Facing::West => (-1, 0),
            Facing::North => (0, -1),
            Facing::East => (1, 0),
        }
    }
}

/// Clicked face (0 down, 1 up, 2 north, 3 south, 4 west, 5 east) and the hit spot on it,
/// 0..1 within the block.
pub struct Click {
    pub face: u8,
    pub click_y: f32,
    pub yaw: f32,
    pub pitch: f32,
}

pub fn face_offset(face: u8) -> (i32, i32, i32) {
    match face {
        0 => (0, -1, 0),
        1 => (0, 1, 0),
        2 => (0, 0, -1),
        3 => (0, 0, 1),
        4 => (-1, 0, 0),
        _ => (1, 0, 0),
    }
}

/// Blocks to set relative to the target. Empty means you can't place it there
/// (like a torch on a ceiling).
pub fn orient(block: i32, click: &Click) -> Vec<((i32, i32, i32), i32)> {
    let look = Facing::from_yaw(click.yaw);
    let top_half = click.face == 0 || (click.face != 1 && click.click_y > 0.5);
    let mut b = block;

    if has_state(b, "pillar_axis") {
        let axis = match click.face {
            0 | 1 => "y",
            2 | 3 => "z",
            _ => "x",
        };
        b = with(b, &[("pillar_axis", State::Str(axis.into()))]);
    }
    if has_state(b, "minecraft:cardinal_direction") {
        // furnaces, chests etc. face the player
        b = with(b, &[("minecraft:cardinal_direction", State::Str(look.opposite().name().into()))]);
    }
    if has_state(b, "minecraft:facing_direction") {
        // newer 6-way blocks like observers use a text direction
        let dir = if click.pitch > 45.0 {
            "up"
        } else if click.pitch < -45.0 {
            "down"
        } else {
            look.opposite().name()
        };
        b = with(b, &[("minecraft:facing_direction", State::Str(dir.into()))]);
    }
    let bname = crate::blocks::entry(b).map_or("", |e| e.name.as_str());
    if bname.contains("repeater") || bname.contains("comparator") {
        // PNX diodes face the player and output the way the player looks
        return vec![((0, 0, 0), with(b, &[("direction", State::Int(look.opposite().legacy()))]))];
    }
    if bname == "minecraft:observer" {
        // face looks where the player looks, output goes back toward them
        let dir = if click.pitch > 45.0 { "down" } else if click.pitch < -45.0 { "up" } else { look.name() };
        return vec![((0, 0, 0), with(b, &[("minecraft:facing_direction", State::Str(dir.into()))]))];
    }
    if bname == "minecraft:piston" || bname == "minecraft:sticky_piston" {
        // PNX stores player facing, so horizontal pistons push back toward the player
        let dir = if click.pitch > 45.0 {
            1
        } else if click.pitch < -45.0 {
            0
        } else {
            match look {
                Facing::North => 2,
                Facing::South => 3,
                Facing::West => 4,
                Facing::East => 5,
            }
        };
        return vec![((0, 0, 0), with(b, &[("facing_direction", State::Int(dir))]))];
    }
    if bname == "minecraft:lever" {
        let along_z = matches!(look, Facing::North | Facing::South);
        let dir = match click.face {
            0 => if along_z { "down_north_south" } else { "down_east_west" },
            1 => if along_z { "up_north_south" } else { "up_east_west" },
            2 => "north",
            3 => "south",
            4 => "west",
            _ => "east",
        };
        return vec![((0, 0, 0), with(b, &[("lever_direction", State::Str(dir.into()))]))];
    }
    if crate::blocks::entry(b).is_some_and(|e| e.name == "minecraft:hopper") {
        // points into the clicked block, or down if placed on top of something
        let dir = match click.face {
            2 => 3,
            3 => 2,
            4 => 5,
            5 => 4,
            _ => 0,
        };
        return vec![((0, 0, 0), with(b, &[("facing_direction", State::Int(dir))]))];
    }
    if has_state(b, "facing_direction") {
        // 6-way blocks face the player, or up/down when looking steeply
        let dir = if click.pitch > 45.0 {
            1
        } else if click.pitch < -45.0 {
            0
        } else {
            match look.opposite() {
                Facing::North => 2,
                Facing::South => 3,
                Facing::West => 4,
                Facing::East => 5,
            }
        };
        b = with(b, &[("facing_direction", State::Int(dir))]);
    }
    if has_state(b, "weirdo_direction") {
        // stairs go up the way you're looking
        let d = match look {
            Facing::East => 0,
            Facing::West => 1,
            Facing::South => 2,
            Facing::North => 3,
        };
        b = with(b, &[("weirdo_direction", State::Int(d)), ("upside_down_bit", State::Bool(top_half))]);
    }
    if has_state(b, "minecraft:vertical_half") {
        let half = if top_half { "top" } else { "bottom" };
        b = with(b, &[("minecraft:vertical_half", State::Str(half.into()))]);
    }
    if has_state(b, "torch_facing_direction") {
        let dir = match click.face {
            1 => "top",
            2 => "north",
            3 => "south",
            4 => "west",
            5 => "east",
            _ => return vec![], // no torches on ceilings
        };
        b = with(b, &[("torch_facing_direction", State::Str(dir.into()))]);
    }

    if has_state(b, "upper_block_bit") {
        // door: bottom here, top above
        let dir = State::Str(look.name().into());
        let lower = with(b, &[("minecraft:cardinal_direction", dir.clone()), ("upper_block_bit", State::Bool(false))]);
        let upper = with(b, &[("minecraft:cardinal_direction", dir), ("upper_block_bit", State::Bool(true))]);
        return vec![((0, 0, 0), lower), ((0, 1, 0), upper)];
    }
    if has_state(b, "head_piece_bit") && has_state(b, "direction") {
        // bed: foot here, head one further in the look direction
        let dir = State::Int(look.legacy());
        let foot = with(b, &[("direction", dir.clone()), ("head_piece_bit", State::Bool(false))]);
        let head = with(b, &[("direction", dir), ("head_piece_bit", State::Bool(true))]);
        let (dx, dz) = look.step();
        return vec![((0, 0, 0), foot), ((dx, 0, dz), head)];
    }
    if has_state(b, "direction") {
        // everything else 4-way (gates, pumpkins, anvils) faces the player
        b = with(b, &[("direction", State::Int(look.legacy()))]);
    }
    vec![((0, 0, 0), b)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::{entry, state};

    fn id_of(name: &str) -> i32 {
        crate::blocks::any_state(name).unwrap_or_else(|| panic!("no {name}"))
    }

    fn click(face: u8, click_y: f32, yaw: f32) -> Click {
        Click { face, click_y, yaw, pitch: 0.0 }
    }

    #[test]
    fn yaw_to_facing() {
        assert_eq!(Facing::from_yaw(0.0), Facing::South);
        assert_eq!(Facing::from_yaw(90.0), Facing::West);
        assert_eq!(Facing::from_yaw(-90.0), Facing::East);
        assert_eq!(Facing::from_yaw(179.0), Facing::North);
    }

    #[test]
    fn logs_follow_clicked_face() {
        let log = id_of("minecraft:oak_log");
        let side = orient(log, &click(4, 0.5, 0.0))[0].1;
        assert_eq!(state(side, "pillar_axis"), Some(&State::Str("x".into())));
        let up = orient(log, &click(1, 0.5, 0.0))[0].1;
        assert_eq!(state(up, "pillar_axis"), Some(&State::Str("y".into())));
    }

    #[test]
    fn stairs_face_look_and_flip_on_top_half() {
        let stairs = id_of("minecraft:oak_stairs");
        let north = orient(stairs, &click(1, 0.0, 180.0))[0].1;
        assert_eq!(state(north, "weirdo_direction"), Some(&State::Int(3)));
        assert_eq!(state(north, "upside_down_bit"), Some(&State::Bool(false)));
        let flipped = orient(stairs, &click(2, 0.8, 180.0))[0].1;
        assert_eq!(state(flipped, "upside_down_bit"), Some(&State::Bool(true)));
    }

    #[test]
    fn doors_and_beds_place_two_halves() {
        let door = id_of("minecraft:wooden_door");
        let parts = orient(door, &click(1, 0.0, 0.0));
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1].0, (0, 1, 0));
        assert_eq!(state(parts[1].1, "upper_block_bit"), Some(&State::Bool(true)));
        assert_eq!(entry(parts[0].1).unwrap().name, entry(parts[1].1).unwrap().name);
    }

    #[test]
    fn beds_have_head_in_look_direction() {
        let bed = id_of("minecraft:bed");
        let parts = orient(bed, &click(1, 0.0, 180.0)); // looking north
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1].0, (0, 0, -1));
        assert_eq!(state(parts[1].1, "head_piece_bit"), Some(&State::Bool(true)));
    }

    #[test]
    fn furnace_faces_player() {
        let furnace = id_of("minecraft:furnace");
        let placed = orient(furnace, &click(1, 0.0, 180.0))[0].1; // player looks north
        assert_eq!(state(placed, "minecraft:cardinal_direction"), Some(&State::Str("south".into())));
    }

    #[test]
    fn no_torch_on_ceiling() {
        let torch = id_of("minecraft:torch");
        assert!(orient(torch, &click(0, 0.0, 0.0)).is_empty());
        let wall = orient(torch, &click(3, 0.5, 0.0))[0].1;
        assert_eq!(state(wall, "torch_facing_direction"), Some(&State::Str("south".into())));
    }
}

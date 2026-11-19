//! Blocks that join their neighbours. Since Bedrock 1.21 the server sends these as block states
//! (fences, iron bars and panes, walls, redstone dust, chorus plants, stair corners), so
//! Rusted works them out whenever something changes next to them, and when chunks generate.

use crate::blocks::{self, State};

type Pos = (i32, i32, i32);

/// north, east, south, west
const SIDES: [(&str, (i32, i32)); 4] = [("north", (0, -1)), ("east", (1, 0)), ("south", (0, 1)), ("west", (-1, 0))];

fn name(b: i32) -> &'static str {
    blocks::entry(b).map_or("", |e| e.name.as_str()).trim_start_matches("minecraft:")
}

fn is_fence(n: &str) -> bool {
    n.ends_with("_fence")
}

fn is_gate(n: &str) -> bool {
    n.ends_with("fence_gate")
}

fn is_pane(n: &str) -> bool {
    n.ends_with("glass_pane") || n == "iron_bars"
}

fn is_wall(n: &str) -> bool {
    n.ends_with("_wall")
}

fn full(b: i32) -> bool {
    crate::overworld::feature::blocks::is_full(b) && !crate::overworld::feature::blocks::is_leaves(b)
}

/// Whether block `b` has states this module sets (looked up in a set, since chunk encoding
/// asks for every block).
pub fn connects(b: i32) -> bool {
    static SET: std::sync::OnceLock<std::collections::HashSet<i32>> = std::sync::OnceLock::new();
    SET.get_or_init(|| {
        blocks::ids().iter().copied().filter(|&b| {
            let n = name(b);
            n != "fire" && n != "soul_fire"
                && (blocks::has_state(b, "minecraft:connection_north") || blocks::has_state(b, "minecraft:corner") || blocks::has_state(b, "wall_connection_type_north"))
        }).collect()
    }).contains(&b)
}

/// Stairs: Bedrock `weirdo_direction` (0 east, 1 west, 2 south, 3 north) as the direction the
/// tall back faces, and upside-down.
fn stair(b: i32) -> Option<((i32, i32), bool)> {
    if !blocks::has_state(b, "minecraft:corner") {
        return None;
    }
    let facing = match blocks::state(b, "weirdo_direction") {
        Some(State::Int(0)) => (1, 0),
        Some(State::Int(1)) => (-1, 0),
        Some(State::Int(2)) => (0, 1),
        _ => (0, -1),
    };
    Some((facing, matches!(blocks::state(b, "upside_down_bit"), Some(State::Bool(true)))))
}

fn ccw(d: (i32, i32)) -> (i32, i32) {
    (d.1, -d.0)
}

/// Vanilla StairBlock.getStairsShape.
fn stair_corner(get: &dyn Fn(i32, i32, i32) -> i32, p: Pos, facing: (i32, i32), top: bool) -> &'static str {
    let at = |d: (i32, i32)| stair(get(p.0 + d.0, p.1, p.2 + d.1));
    let can_take = |d: (i32, i32)| at(d).is_none_or(|(f, t)| f != facing || t != top);
    if let Some((f, t)) = at(facing)
        && t == top
        && (f.0 == 0) != (facing.0 == 0)
        && can_take((-f.0, -f.1))
    {
        return if f == ccw(facing) { "outer_left" } else { "outer_right" };
    }
    if let Some((f, t)) = at((-facing.0, -facing.1))
        && t == top
        && (f.0 == 0) != (facing.0 == 0)
        && can_take(f)
    {
        return if f == ccw(facing) { "inner_left" } else { "inner_right" };
    }
    "none"
}

/// Redstone parts dust links up to (not plain blocks).
fn redstone_part(n: &str) -> bool {
    n == "redstone_wire" || n.contains("redstone_torch") || n == "lever" || n.ends_with("_button") || n.ends_with("pressure_plate") || n == "redstone_block"
        || n.contains("repeater") || n.contains("comparator") || n == "observer" || n.starts_with("daylight_detector") || n == "target" || n.ends_with("sculk_sensor")
        || n == "trapped_chest" || n == "lectern" || n == "detector_rail"
}

/// The block at `p` with its connections worked out, or None if it doesn't change.
pub fn shape(get: &dyn Fn(i32, i32, i32) -> i32, p: Pos) -> Option<i32> {
    let b = get(p.0, p.1, p.2);
    if !connects(b) {
        return None;
    }
    let n = name(b);
    let mut st: Vec<(String, State)> = Vec::new();
    if let Some((facing, top)) = stair(b) {
        st.push(("minecraft:corner".into(), State::Str(stair_corner(get, p, facing, top).into())));
    } else if is_wall(n) {
        let above = get(p.0, p.1 + 1, p.2);
        let mut joined = [false; 4];
        for (i, (side, (dx, dz))) in SIDES.iter().enumerate() {
            let o = get(p.0 + dx, p.1, p.2 + dz);
            let on = name(o);
            joined[i] = is_wall(on) || is_gate(on) || is_pane(on) || full(o);
            // Tall where the block above also reaches over that side (vanilla wall shape).
            let tall = joined[i] && (full(above) || is_wall(name(above)));
            let kind = if !joined[i] { "none" } else if tall { "tall" } else { "short" };
            st.push((format!("wall_connection_type_{side}"), State::Str(kind.into())));
        }
        // A post unless it's a straight run with nothing on top.
        let straight = (joined[0] && joined[2] && !joined[1] && !joined[3]) || (joined[1] && joined[3] && !joined[0] && !joined[2]);
        st.push(("wall_post_bit".into(), State::Bool(!straight || is_wall(name(above)) || full(above))));
    } else if n == "chorus_plant" {
        for (side, (dx, dz)) in SIDES {
            let on = name(get(p.0 + dx, p.1, p.2 + dz));
            st.push((format!("minecraft:connection_{side}"), State::Bool(on == "chorus_plant" || on == "chorus_flower")));
        }
        let up = name(get(p.0, p.1 + 1, p.2));
        let down = name(get(p.0, p.1 - 1, p.2));
        st.push(("minecraft:connection_up".into(), State::Bool(up == "chorus_plant" || up == "chorus_flower")));
        st.push(("minecraft:connection_down".into(), State::Bool(down == "chorus_plant" || down == "end_stone")));
    } else if n == "redstone_wire" {
        let above_clear = !full(get(p.0, p.1 + 1, p.2));
        for (side, (dx, dz)) in SIDES {
            let o = get(p.0 + dx, p.1, p.2 + dz);
            let level = redstone_part(name(o));
            // Dust also runs up a block side (if nothing covers this dust) and down off an edge.
            let up = above_clear && full(o) && name(get(p.0 + dx, p.1 + 1, p.2 + dz)) == "redstone_wire";
            let down = !full(o) && name(get(p.0 + dx, p.1 - 1, p.2 + dz)) == "redstone_wire";
            st.push((format!("minecraft:connection_{side}"), State::Bool(level || up || down)));
        }
    } else if n == "fire" || n == "soul_fire" {
        return None; // the client draws fire from its own neighbours
    } else {
        // Fences join fences of their family, gates and solid blocks; panes and bars join panes,
        // bars, walls and solid blocks.
        let nether = n.starts_with("nether_brick");
        for (side, (dx, dz)) in SIDES {
            let o = get(p.0 + dx, p.1, p.2 + dz);
            let on = name(o);
            let joined = if is_fence(n) {
                (is_fence(on) && on.starts_with("nether_brick") == nether) || is_gate(on) || full(o)
            } else {
                is_pane(on) || is_wall(on) || full(o)
            };
            st.push((format!("minecraft:connection_{side}"), State::Bool(joined)));
        }
    }
    let refs: Vec<(&str, State)> = st.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    let new = blocks::with(b, &refs);
    (new != b).then_some(new)
}

/// What around `p` might need reshaping after `p` changed: itself, its four sides, and the
/// blocks above and below (dust on steps, walls under walls).
pub fn around(p: Pos) -> [Pos; 7] {
    [p, (p.0, p.1, p.2 - 1), (p.0 + 1, p.1, p.2), (p.0, p.1, p.2 + 1), (p.0 - 1, p.1, p.2), (p.0, p.1 + 1, p.2), (p.0, p.1 - 1, p.2)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fences_panes_and_stairs_join_up() {
        crate::blocks::init();
        let w = crate::world::World::default();
        let get = |x, y, z| w.get(x, y, z);
        let fence = blocks::any_state("minecraft:oak_fence").unwrap();
        let y = -60;
        w.set(0, y, 0, fence);
        w.set(1, y, 0, fence);
        let a = shape(&get, (0, y, 0)).expect("joins its neighbour");
        assert!(matches!(blocks::state(a, "minecraft:connection_east"), Some(State::Bool(true))));
        assert!(matches!(blocks::state(a, "minecraft:connection_west"), Some(State::Bool(false))));

        let pane = blocks::any_state("minecraft:glass_pane").unwrap();
        w.set(10, y, 10, pane);
        w.set(10, y, 9, blocks::any_state("minecraft:stone").unwrap());
        let p = shape(&get, (10, y, 10)).unwrap();
        assert!(matches!(blocks::state(p, "minecraft:connection_north"), Some(State::Bool(true))));

        // Two stairs at a right angle make a corner.
        let stairs = blocks::any_state("minecraft:oak_stairs").unwrap();
        let east = blocks::with(stairs, &[("weirdo_direction", State::Int(0))]);
        let north = blocks::with(stairs, &[("weirdo_direction", State::Int(3))]);
        w.set(20, y, 20, east);
        w.set(21, y, 20, north);
        let c = shape(&get, (20, y, 20)).unwrap();
        assert!(matches!(blocks::state(c, "minecraft:corner"), Some(State::Str(s)) if s.starts_with("outer")));
    }
}

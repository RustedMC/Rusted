//! Overworld biome picking from the climate noises (PNX `OverworldBiomePicker`), with the
//! cave-biome correction underground. Ids are Bedrock's numeric biome ids.

#![allow(clippy::too_many_arguments)]

pub mod id {
    pub const OCEAN: u16 = 0;
    pub const PLAINS: u16 = 1;
    pub const DESERT: u16 = 2;
    pub const EXTREME_HILLS: u16 = 3;
    pub const FOREST: u16 = 4;
    pub const TAIGA: u16 = 5;
    pub const SWAMPLAND: u16 = 6;
    pub const RIVER: u16 = 7;
    pub const LEGACY_FROZEN_OCEAN: u16 = 10;
    pub const FROZEN_RIVER: u16 = 11;
    pub const ICE_PLAINS: u16 = 12;
    pub const MUSHROOM_ISLAND: u16 = 14;
    pub const BEACH: u16 = 16;
    pub const JUNGLE: u16 = 21;
    pub const JUNGLE_EDGE: u16 = 23;
    pub const DEEP_OCEAN: u16 = 24;
    pub const STONE_BEACH: u16 = 25;
    pub const COLD_BEACH: u16 = 26;
    pub const BIRCH_FOREST: u16 = 27;
    pub const ROOFED_FOREST: u16 = 29;
    pub const COLD_TAIGA: u16 = 30;
    pub const MEGA_TAIGA: u16 = 32;
    pub const EXTREME_HILLS_PLUS_TREES: u16 = 34;
    pub const SAVANNA: u16 = 35;
    pub const SAVANNA_PLATEAU: u16 = 36;
    pub const MESA: u16 = 37;
    pub const MESA_PLATEAU_STONE: u16 = 38;
    pub const WARM_OCEAN: u16 = 40;
    pub const LUKEWARM_OCEAN: u16 = 42;
    pub const DEEP_LUKEWARM_OCEAN: u16 = 43;
    pub const COLD_OCEAN: u16 = 44;
    pub const DEEP_COLD_OCEAN: u16 = 45;
    pub const FROZEN_OCEAN: u16 = 46;
    pub const DEEP_FROZEN_OCEAN: u16 = 47;
    pub const BAMBOO_JUNGLE: u16 = 48;
    pub const SUNFLOWER_PLAINS: u16 = 129;
    pub const EXTREME_HILLS_MUTATED: u16 = 131;
    pub const FLOWER_FOREST: u16 = 132;
    pub const ICE_PLAINS_SPIKES: u16 = 140;
    pub const BIRCH_FOREST_MUTATED: u16 = 155;
    pub const REDWOOD_TAIGA_MUTATED: u16 = 160;
    pub const SAVANNA_MUTATED: u16 = 163;
    pub const MESA_BRYCE: u16 = 165;
    pub const JAGGED_PEAKS: u16 = 182;
    pub const FROZEN_PEAKS: u16 = 183;
    pub const SNOWY_SLOPES: u16 = 184;
    pub const GROVE: u16 = 185;
    pub const MEADOW: u16 = 186;
    pub const LUSH_CAVES: u16 = 187;
    pub const DRIPSTONE_CAVES: u16 = 188;
    pub const STONY_PEAKS: u16 = 189;
    pub const DEEP_DARK: u16 = 190;
    pub const MANGROVE_SWAMP: u16 = 191;
    pub const CHERRY_GROVE: u16 = 192;
    pub const PALE_GARDEN: u16 = 193;
    pub const SULFUR_CAVES: u16 = 194;
    pub const DAPPLED_FOREST: u16 = 195;
}
use id::*;

#[derive(Clone, Copy, Debug)]
pub struct Pick {
    pub biome: u16,
    pub continental: f32,
    pub temperature: f32,
    pub humidity: f32,
    pub erosion: f32,
}

impl Pick {
    /// The biome `depth` blocks below the surface (cave biomes replace it deep down).
    pub fn at_depth(&self, below_surface: i32) -> u16 {
        let d = below_surface as f32 / 128.0;
        if d >= 0.2 {
            if d < 0.99 {
                if self.continental > 0.8 && self.continental < 1.0 {
                    return DRIPSTONE_CAVES;
                } else if self.temperature > 0.55 && self.humidity < -0.1 {
                    return SULFUR_CAVES;
                } else if self.humidity > 0.3 {
                    return LUSH_CAVES;
                }
            } else if d > 0.9 && self.erosion < -0.225 {
                return DEEP_DARK;
            }
        }
        self.biome
    }
}

/// Picks the surface biome from the five climate values.
pub fn pick(continental: f32, temperature: f32, humidity: f32, erosion: f32, weirdness: f32) -> Pick {
    let c = if continental < -1.05 { 0 } else if continental < -0.455 { 1 } else if (continental as f64) < -0.19 { 2 } else if (continental as f64) < -0.11 { 3 } else if (continental as f64) < 0.03 { 4 } else if (continental as f64) < 0.3 { 5 } else { 6 };
    let t = if temperature < -0.45 { 0 } else if temperature < -0.15 { 1 } else if temperature < 0.2 { 2 } else if temperature < 0.55 { 3 } else { 4 };
    let h = if humidity < -0.35 { 0 } else if humidity < -0.1 { 1 } else if humidity < 0.1 { 2 } else if humidity < 0.3 { 3 } else { 4 };
    let e = if erosion < -0.78 { 0 } else if erosion < -0.375 { 1 } else if erosion < -0.2225 { 2 } else if erosion < 0.05 { 3 } else if erosion < 0.45 { 4 } else if erosion < 0.55 { 5 } else { 6 };
    let biome = match c {
        0 => MUSHROOM_ISLAND,
        1 | 2 => non_inland(t, c),
        _ => inland(t, h, c, e, weirdness),
    };
    Pick { biome, continental, temperature, humidity, erosion }
}

const COAST: i32 = 3;
const NEAR: i32 = 4;
const MID: i32 = 5;
const FAR: i32 = 6;

fn non_inland(t: i32, c: i32) -> u16 {
    let shallow = c == 2;
    match t {
        0 => if shallow { FROZEN_OCEAN } else { DEEP_FROZEN_OCEAN },
        1 => if shallow { COLD_OCEAN } else { DEEP_COLD_OCEAN },
        2 => if shallow { OCEAN } else { DEEP_OCEAN },
        3 => if shallow { LUKEWARM_OCEAN } else { DEEP_LUKEWARM_OCEAN },
        _ => WARM_OCEAN,
    }
}

fn inland(t: i32, h: i32, c: i32, e: i32, w: f32) -> u16 {
    if w < -0.93333334 {
        mid_slice(t, h, c, e, w)
    } else if w < -0.7666667 {
        high_slice(t, h, c, e, w)
    } else if w < -0.56666666 {
        peaks(t, h, c, e, w)
    } else if w < -0.4 {
        high_slice(t, h, c, e, w)
    } else if w < -0.26666668 {
        mid_slice(t, h, c, e, w)
    } else if w < -0.05 {
        low_slice(t, h, c, e, w)
    } else if w < 0.05 {
        valleys(t, h, c, e, w)
    } else if w < 0.26666668 {
        low_slice(t, h, c, e, w)
    } else if w < 0.4 {
        mid_slice(t, h, c, e, w)
    } else if w < 0.56666666 {
        high_slice(t, h, c, e, w)
    } else if w < 0.7666667 {
        peaks(t, h, c, e, w)
    } else if w < 0.93333334 {
        high_slice(t, h, c, e, w)
    } else {
        mid_slice(t, h, c, e, w)
    }
}

fn coast_to_near(c: i32) -> bool {
    (COAST..=NEAR).contains(&c)
}
fn coast_to_far(c: i32) -> bool {
    (COAST..=FAR).contains(&c)
}
fn near_to_mid(c: i32) -> bool {
    (NEAR..=MID).contains(&c)
}
fn near_to_far(c: i32) -> bool {
    (NEAR..=FAR).contains(&c)
}
fn mid_to_far(c: i32) -> bool {
    (MID..=FAR).contains(&c)
}

fn peaks(t: i32, h: i32, c: i32, e: i32, w: f32) -> u16 {
    let weird = w >= 0.0;
    let middle = middle(t, h, weird);
    let mob = middle_or_badlands(t, h, weird);
    let mobs = middle_badlands_slope(t, h, weird);
    let plateau = plateau(t, h, weird);
    let shattered = shattered(t, h, weird);
    let shattered_ws = windswept_savanna(t, h, weird, shattered);
    let peak = peak(t, h, weird);
    if coast_to_far(c) && e == 0 { return peak; }
    if coast_to_near(c) && e == 1 { return mobs; }
    if mid_to_far(c) && e == 1 { return peak; }
    if coast_to_near(c) && (e == 2 || e == 3) { return middle; }
    if mid_to_far(c) && e == 2 { return plateau; }
    if c == MID && e == 3 { return mob; }
    if c == FAR && e == 3 { return plateau; }
    if coast_to_far(c) && e == 4 { return middle; }
    if coast_to_near(c) && e == 5 { return shattered_ws; }
    if mid_to_far(c) && e == 5 { return shattered; }
    middle
}

fn high_slice(t: i32, h: i32, c: i32, e: i32, w: f32) -> u16 {
    let weird = w >= 0.0;
    let middle = middle(t, h, weird);
    let mob = middle_or_badlands(t, h, weird);
    let mobs = middle_badlands_slope(t, h, weird);
    let plateau = plateau(t, h, weird);
    let shattered = shattered(t, h, weird);
    let middle_ws = windswept_savanna(t, h, weird, middle);
    let slope = slope(t, h, weird);
    let peak = peak(t, h, weird);
    if c == COAST && (e == 0 || e == 1) { return middle; }
    if c == NEAR && e == 0 { return slope; }
    if mid_to_far(c) && e == 0 { return peak; }
    if c == NEAR && e == 1 { return mobs; }
    if mid_to_far(c) && e == 1 { return slope; }
    if coast_to_near(c) && (e == 2 || e == 3) { return middle; }
    if mid_to_far(c) && e == 2 { return plateau; }
    if c == MID && e == 3 { return mob; }
    if c == FAR && e == 3 { return plateau; }
    if coast_to_far(c) && e == 4 { return middle; }
    if coast_to_near(c) && e == 5 { return middle_ws; }
    if mid_to_far(c) && e == 5 { return shattered; }
    middle
}

fn mid_slice(t: i32, h: i32, c: i32, e: i32, w: f32) -> u16 {
    let weird = w >= 0.0;
    if c == COAST && e <= 2 { return STONE_BEACH; }
    if (t == 1 || t == 2) && near_to_far(c) && e == 6 { return SWAMPLAND; }
    if (t == 3 || t == 4) && near_to_far(c) && e == 6 { return MANGROVE_SWAMP; }
    let middle = middle(t, h, weird);
    let mob = middle_or_badlands(t, h, weird);
    let mobs = middle_badlands_slope(t, h, weird);
    let shattered = shattered(t, h, weird);
    let plateau = plateau(t, h, weird);
    let beach = beach(t);
    let middle_ws = windswept_savanna(t, h, weird, middle);
    let shattered_coast = shattered_coast(t, h, weird);
    let slope = slope(t, h, weird);
    if near_to_far(c) && e == 0 { return slope; }
    if near_to_mid(c) && e == 1 { return mobs; }
    if c == FAR && e == 1 { return if t == 0 { slope } else { plateau }; }
    if c == NEAR && e == 2 { return middle; }
    if c == MID && e == 2 { return mob; }
    if c == FAR && e == 2 { return plateau; }
    if coast_to_near(c) && e == 3 { return middle; }
    if mid_to_far(c) && e == 3 { return mob; }
    if e == 4 {
        if !weird {
            if c == COAST { return beach; }
            if near_to_far(c) { return middle; }
        } else if coast_to_far(c) {
            return middle;
        }
    }
    if c == COAST && e == 5 { return shattered_coast; }
    if c == NEAR && e == 5 { return middle_ws; }
    if mid_to_far(c) && e == 5 { return shattered; }
    if c == COAST && e == 6 { return if weird { middle } else { beach }; }
    middle
}

fn low_slice(t: i32, h: i32, c: i32, e: i32, w: f32) -> u16 {
    let weird = w >= 0.0;
    if c == COAST && e <= 2 { return STONE_BEACH; }
    if (t == 1 || t == 2) && near_to_far(c) && e == 6 { return SWAMPLAND; }
    if (t == 3 || t == 4) && near_to_far(c) && e == 6 { return MANGROVE_SWAMP; }
    let middle = middle(t, h, weird);
    let mob = middle_or_badlands(t, h, weird);
    let mobs = middle_badlands_slope(t, h, weird);
    let beach = beach(t);
    let middle_ws = windswept_savanna(t, h, weird, middle);
    let shattered_coast = shattered_coast(t, h, weird);
    if c == NEAR && (e == 0 || e == 1) { return mob; }
    if mid_to_far(c) && (e == 0 || e == 1) { return mobs; }
    if c == NEAR && (e == 2 || e == 3) { return middle; }
    if mid_to_far(c) && (e == 2 || e == 3) { return mob; }
    if c == COAST && (e == 3 || e == 4) { return beach; }
    if near_to_far(c) && e == 4 { return middle; }
    if c == COAST && e == 5 { return shattered_coast; }
    if c == NEAR && e == 5 { return middle_ws; }
    if mid_to_far(c) && e == 5 { return middle; }
    if c == COAST && e == 6 { return beach; }
    middle
}

fn valleys(t: i32, h: i32, c: i32, e: i32, w: f32) -> u16 {
    let weird = w >= 0.0;
    let frozen = t == 0;
    let river = if frozen { FROZEN_RIVER } else { RIVER };
    if c == COAST && (e == 0 || e == 1) {
        return if weird { river } else { STONE_BEACH };
    }
    if c == NEAR && (e == 0 || e == 1) { return river; }
    if coast_to_far(c) && (2..=5).contains(&e) { return river; }
    if c == COAST && e == 6 { return river; }
    if (t == 1 || t == 2) && near_to_far(c) && e == 6 { return SWAMPLAND; }
    if (t == 3 || t == 4) && near_to_far(c) && e == 6 { return MANGROVE_SWAMP; }
    if frozen && near_to_far(c) && e == 6 { return FROZEN_RIVER; }
    if mid_to_far(c) && (e == 0 || e == 1) { return middle_or_badlands(t, h, weird); }
    river
}

fn middle_or_badlands(t: i32, h: i32, weird: bool) -> u16 {
    if t == 4 { badlands(h, weird) } else { middle(t, h, weird) }
}

fn middle_badlands_slope(t: i32, h: i32, weird: bool) -> u16 {
    if t == 0 { slope(t, h, weird) } else { middle_or_badlands(t, h, weird) }
}

fn windswept_savanna(t: i32, h: i32, weird: bool, under: u16) -> u16 {
    if weird && t > 1 && h < 4 { SAVANNA_MUTATED } else { under }
}

fn shattered_coast(t: i32, h: i32, weird: bool) -> u16 {
    let b = if weird { middle(t, h, true) } else { beach(t) };
    windswept_savanna(t, h, weird, b)
}

fn peak(t: i32, h: i32, weird: bool) -> u16 {
    if t <= 2 {
        if weird { FROZEN_PEAKS } else { JAGGED_PEAKS }
    } else if t == 3 {
        STONY_PEAKS
    } else {
        badlands(h, weird)
    }
}

fn slope(t: i32, h: i32, weird: bool) -> u16 {
    if t >= 3 { plateau(t, h, weird) } else if h <= 1 { SNOWY_SLOPES } else { GROVE }
}

fn beach(t: i32) -> u16 {
    match t {
        0 => COLD_BEACH,
        1..=3 => BEACH,
        _ => DESERT,
    }
}

fn badlands(h: i32, weird: bool) -> u16 {
    match h {
        0 | 1 => if weird { MESA_BRYCE } else { MESA },
        2 => MESA,
        _ => MESA_PLATEAU_STONE,
    }
}

fn middle(t: i32, h: i32, weird: bool) -> u16 {
    match t {
        0 => match h {
            0 => if weird { ICE_PLAINS_SPIKES } else { ICE_PLAINS },
            1 => ICE_PLAINS,
            2 => if weird { COLD_TAIGA } else { ICE_PLAINS },
            3 => COLD_TAIGA,
            _ => TAIGA,
        },
        1 => match h {
            0 => if weird { DAPPLED_FOREST } else { PLAINS },
            1 => PLAINS,
            2 => FOREST,
            3 => TAIGA,
            _ => if weird { MEGA_TAIGA } else { REDWOOD_TAIGA_MUTATED },
        },
        2 => match h {
            0 => if weird { SUNFLOWER_PLAINS } else { FLOWER_FOREST },
            1 => PLAINS,
            2 => FOREST,
            3 => if weird { BIRCH_FOREST_MUTATED } else { BIRCH_FOREST },
            _ => ROOFED_FOREST,
        },
        3 => match h {
            0 | 1 => SAVANNA,
            2 => if weird { PLAINS } else { FOREST },
            3 => if weird { JUNGLE_EDGE } else { JUNGLE },
            _ => if weird { BAMBOO_JUNGLE } else { JUNGLE },
        },
        _ => DESERT,
    }
}

fn plateau(t: i32, h: i32, weird: bool) -> u16 {
    match t {
        0 => match h {
            0 => if weird { ICE_PLAINS_SPIKES } else { ICE_PLAINS },
            1 | 2 => ICE_PLAINS,
            _ => COLD_TAIGA,
        },
        1 => match h {
            0 => if weird { CHERRY_GROVE } else { MEADOW },
            1 => MEADOW,
            2 => if weird { MEADOW } else { FOREST },
            3 => if weird { MEADOW } else { TAIGA },
            _ => if weird { MEGA_TAIGA } else { REDWOOD_TAIGA_MUTATED },
        },
        2 => match h {
            0 | 1 => if weird { CHERRY_GROVE } else { MEADOW },
            2 => if weird { FOREST } else { MEADOW },
            3 => if weird { BIRCH_FOREST } else { MEADOW },
            _ => PALE_GARDEN,
        },
        3 => match h {
            0 | 1 => SAVANNA_PLATEAU,
            2 | 3 => FOREST,
            _ => JUNGLE,
        },
        _ => match h {
            0 | 1 => if weird { MESA_BRYCE } else { MESA },
            2 => MESA,
            _ => MESA_PLATEAU_STONE,
        },
    }
}

fn shattered(t: i32, h: i32, weird: bool) -> u16 {
    match t {
        0 | 1 => match h {
            0 | 1 => EXTREME_HILLS_MUTATED,
            2 => EXTREME_HILLS,
            _ => EXTREME_HILLS_PLUS_TREES,
        },
        2 => if h <= 2 { EXTREME_HILLS } else { EXTREME_HILLS_PLUS_TREES },
        3 => match h {
            0 | 1 => SAVANNA,
            2 => if weird { PLAINS } else { FOREST },
            3 => if weird { JUNGLE_EDGE } else { JUNGLE },
            _ => if weird { BAMBOO_JUNGLE } else { JUNGLE },
        },
        _ => DESERT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn climate_maps_to_biomes() {
        assert_eq!(pick(-1.2, 0.0, 0.0, 0.0, 0.0).biome, MUSHROOM_ISLAND);
        assert_eq!(pick(-0.3, -0.6, 0.0, 0.0, 0.0).biome, FROZEN_OCEAN);
        assert_eq!(pick(-0.6, 0.0, 0.0, 0.0, 0.0).biome, DEEP_OCEAN);
        assert_eq!(pick(0.5, 0.0, 0.0, 0.2, 0.0).biome, RIVER, "valleys are rivers");
        assert_eq!(pick(0.5, 0.0, -0.2, 0.2, 0.15).biome, PLAINS);
        assert_eq!(pick(0.5, 0.0, -0.2, -0.9, 0.6).biome, FROZEN_PEAKS, "weird side of the peaks");
        assert_eq!(pick(0.5, 0.0, -0.2, -0.9, -0.6).biome, JAGGED_PEAKS);
        let p = pick(0.5, 0.0, 0.5, 0.2, 0.15);
        assert_eq!(p.at_depth(60), LUSH_CAVES, "humid: lush caves below");
        assert_eq!(p.at_depth(10), p.biome);
    }
}

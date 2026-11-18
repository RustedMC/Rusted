//! Builds all overworld density functions from the seed, in PNX's draw order
//! (`NormalObjectHolder`).

use std::sync::Arc;

use super::density::*;
use super::noise::{NormalNoise, OctaveList};
use super::rng::Xoroshiro;

pub struct Router {
    pub continents: F,
    pub temperature: F,
    pub humidity: F,
    pub erosion: F,
    pub ridges: F,
    pub depth: F,
    /// Final density (> 0 = stone), cached per cell.
    pub terrain: F,
    pub prelim_surface: F,
    pub prelim_upper: F,
    pub vein_toggle: F,
    pub vein_ridged: F,
    pub vein_gap: F,
    pub ore_vein_seed: i64,
    pub barrier: F,
    pub floodedness: F,
    pub spread: F,
    pub lava: F,
    pub surface: NormalNoise,
    pub overwrite: Overwrite,
}

/// Noises the surface rules use (PNX `SurfaceOverwriteHolder`).
pub struct Overwrite {
    pub surface: NormalNoise,
    pub swamp: NormalNoise,
    pub clay_offset: NormalNoise,
    pub badlands_pillar: NormalNoise,
    pub badlands_roof: NormalNoise,
    pub badlands_surface: NormalNoise,
    pub iceberg_pillar: NormalNoise,
    pub iceberg_roof: NormalNoise,
    pub iceberg_surface: NormalNoise,
}

fn nn(r: Xoroshiro, first: i32, amps: &[f32]) -> Arc<NormalNoise> {
    Arc::new(NormalNoise::new(r, first, amps))
}

macro_rules! s {
    ($coord:expr, $( ($loc:expr, $v:expr, $d:expr) ),+ $(,)?) => {
        spline(&$coord, &[ $( ($loc, $v, $d) ),+ ])
    };
}

fn c(v: f64) -> P {
    P::C(v)
}
fn f(v: &F) -> P {
    P::F(v.clone())
}

impl Router {
    pub fn new(seed: i64) -> Self {
        let mut r = Xoroshiro::new(seed);
        // BiomeHolder
        let continental = nn(r.fork(), -9, &[1.0, 1.0, 2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0]);
        let temperature_n = nn(r.fork(), -10, &[1.5, 0.0, 1.0, 0.0, 0.0, 0.0]);
        let humidity_n = nn(r.fork(), -8, &[1.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
        let erosion_n = nn(r.fork(), -9, &[1.0, 1.0, 0.0, 1.0, 1.0]);
        let weirdness_n = nn(r.fork(), -7, &[1.0, 2.0, 1.0, 0.0, 0.0, 0.0]);
        let offset_n = nn(r.fork(), -3, &[1.0, 1.0, 1.0, 0.0]);
        let jagged_n = nn(r.fork(), -16, &[1.0; 16]);

        // TerrainHolder
        let shift_x = flat(&shift_a(&offset_n));
        let shift_z = flat(&shift_b(&offset_n));
        let z0 = zero();
        let climate = |n: &Arc<NormalNoise>| flat(&shifted(&shift_x, &z0, &shift_z, 0.25, 0.0, n));
        let continents = climate(&continental);
        let temperature = climate(&temperature_n);
        let humidity = climate(&humidity_n);
        let erosion = climate(&erosion_n);
        let ridges = climate(&weirdness_n);
        let ridges_folded = mulc(-3.0, &addc(-0.33333334, &map(&addc(-0.6666667, &map(&ridges, Map::Abs)), Map::Abs)));
        let offset = overworld_offset(&continents, &erosion, &ridges_folded);
        let depth = add(&y_grad(-64, 320, 1.5, -1.5), &offset);
        let factor = overworld_factor(&continents, &erosion, &ridges, &ridges_folded);
        let jaggedness = overworld_jaggedness(&continents, &erosion, &ridges, &ridges_folded);
        let mut id = r.identical();
        let base3d = blended(
            OctaveList::new(&mut id.fork(), -15),
            OctaveList::new(&mut id.fork(), -15),
            OctaveList::new(&mut id.fork(), -7),
            0.25,
            0.125,
            80.0,
            160.0,
            8.0,
        );
        let barrier_n = nn(r.fork(), -3, &[1.0, 1.0, 1.0]);
        let flood_n = nn(r.fork(), -7, &[1.0, 1.0, 0.0, 1.0]);
        let spread_n = nn(r.fork(), -5, &[1.0, 1.0, 1.0]);
        let lava_n = nn(r.fork(), -1, &[1.0, 1.0]);
        let pillar = nn(r.fork(), -7, &[1.0, 1.0]);
        let pillar_rareness = nn(r.fork(), -8, &[1.0]);
        let pillar_thickness = nn(r.fork(), -8, &[1.0]);
        let sp2d = nn(r.fork(), -7, &[1.0]);
        let sp2d_elevation = nn(r.fork(), -8, &[1.0]);
        let sp2d_modulator = nn(r.fork(), -11, &[1.0]);
        let sp2d_thickness = nn(r.fork(), -11, &[1.0]);
        let sp3d_first = nn(r.fork(), -7, &[1.0]);
        let sp3d_second = nn(r.fork(), -7, &[1.0]);
        let sp3d_rarity = nn(r.fork(), -11, &[1.0]);
        let sp3d_thickness = nn(r.fork(), -8, &[1.0]);
        let roughness = nn(r.fork(), -5, &[1.0]);
        let roughness_mod = nn(r.fork(), -8, &[1.0]);
        let cave_entrance = nn(r.fork(), -7, &[0.4, 0.5, 1.0]);
        let cave_layer = nn(r.fork(), -8, &[1.0]);
        let cave_cheese = nn(r.fork(), -8, &[0.5, 1.0, 2.0, 1.0, 2.0, 1.0, 0.0, 2.0, 0.0]);
        let noodle_n = nn(r.fork(), -8, &[1.0]);
        let noodle_thickness = nn(r.fork(), -8, &[1.0]);
        let noodle_a = nn(r.fork(), -7, &[1.0]);
        let noodle_b = nn(r.fork(), -7, &[1.0]);
        let vein_toggle_n = nn(r.fork(), -8, &[1.0]);
        let vein_a = nn(r.fork(), -7, &[1.0]);
        let vein_b = nn(r.fork(), -7, &[1.0]);
        let ore_gap = nn(r.fork(), -5, &[1.0]);

        // Sloped cheese: the base terrain shape.
        let jagged_sample = map(&noise(&jagged_n, 1500.0, 0.0), Map::HalfNegative);
        let combined_depth = add(&depth, &flat(&mul(&jaggedness, &jagged_sample)));
        let sloped = once(&add(&mulc(4.0, &map(&mul(&combined_depth, &factor), Map::QuarterNegative)), &base3d));

        // Caves.
        let rough_fn = once(&mul(&mapped_noise(&roughness_mod, 1.0, 1.0, 0.0, -0.1), &addc(-0.4, &map(&noise(&roughness, 1.0, 1.0), Map::Abs))));
        let sp2d_thick_mod = once(&mapped_noise(&sp2d_thickness, 2.0, 1.0, -0.6, -1.3));
        let sp2d_fn = {
            let sampled = weird(&noise(&sp2d_modulator, 2.0, 1.0), &sp2d, false);
            let elevation = flat(&mulc(8.0, &noise(&sp2d_elevation, 1.0, 0.0)));
            let grad = map(&add(&map(&add(&elevation, &y_grad(-64, 320, 8.0, -40.0)), Map::Abs), &sp2d_thick_mod), Map::Cube);
            let adjusted = add(&sampled, &mulc(0.083, &sp2d_thick_mod));
            clamp(&max(&adjusted, &grad), -1.0, 1.0)
        };
        let entrances = {
            let rarity = once(&noise(&sp3d_rarity, 2.0, 1.0));
            let thickness = mapped_noise(&sp3d_thickness, 1.0, 1.0, -0.065, -0.088);
            let first = weird(&rarity, &sp3d_first, true);
            let second = weird(&rarity, &sp3d_second, true);
            let spaghetti = clamp(&add(&max(&first, &second), &thickness), -1.0, 1.0);
            let entrance = noise(&cave_entrance, 0.75, 0.5);
            let grad = add(&addc(0.37, &entrance), &y_grad(-10, 30, 0.3, 0.0));
            once(&min(&grad, &add(&rough_fn, &spaghetti)))
        };
        let pillars = {
            let p = noise(&pillar, 25.0, 0.3);
            let rarity = mapped_noise(&pillar_rareness, 1.0, 1.0, 0.0, -2.0);
            let thickness = mapped_noise(&pillar_thickness, 1.0, 1.0, 0.0, 1.1);
            once(&mul(&add(&mulc(2.0, &p), &rarity), &map(&thickness, Map::Cube)))
        };
        let underground = {
            let layer = mulc(4.0, &map(&noise(&cave_layer, 1.0, 8.0), Map::Square));
            let cheese = add(
                &clamp(&addc(0.27, &noise(&cave_cheese, 1.0, 0.6666666666666666)), -1.0, 1.0),
                &clamp(&addc(1.5, &mulc(-0.64, &sloped)), 0.0, 0.5),
            );
            let cave_density = add(&layer, &cheese);
            let passages = min(&min(&cave_density, &entrances), &add(&sp2d_fn, &rough_fn));
            let pillar_filter = range(&pillars, -1_000_000.0, 0.03, &constant(-1_000_000.0), &pillars);
            max(&passages, &pillar_filter)
        };
        let caves = range(&sloped, -1_000_000.0, 1.5625, &min(&sloped, &mulc(5.0, &entrances)), &underground);
        let post = map(
            &mulc(0.64, &interpolated(&slide(&caves))),
            Map::Squeeze,
        );
        let noodle = {
            let yl = |d: F, oor: f64| interpolated(&range(&y(), -60.0, 321.0, &d, &constant(oor)));
            let toggle = yl(noise(&noodle_n, 1.0, 1.0), -1.0);
            let thickness = yl(mapped_noise(&noodle_thickness, 1.0, 1.0, -0.05, -0.1), 0.0);
            let ra = yl(noise(&noodle_a, 2.6666666666666665, 2.6666666666666665), 0.0);
            let rb = yl(noise(&noodle_b, 2.6666666666666665, 2.6666666666666665), 0.0);
            let ridges = mulc(1.5, &max(&map(&ra, Map::Abs), &map(&rb, Map::Abs)));
            range(&toggle, -1_000_000.0, 0.0, &constant(64.0), &add(&thickness, &ridges))
        };
        let final_density = min(&post, &noodle);

        let prelim_surface = {
            let base = add(&y_grad(-64, 320, 1.5, -1.5), &cache2d(&offset));
            let scaled = map(&mul(&base, &cache2d(&factor)), Map::QuarterNegative);
            let clamped = clamp(&addc(-0.703125, &mulc(4.0, &scaled)), -64.0, 64.0);
            addc(-0.390625, &slide(&clamped))
        };
        let prelim_upper = clamp(
            &addc(128.0, &mulc(-128.0, &add(&mulc(0.2734375, &map(&cache2d(&factor), Map::Invert)), &mulc(-1.0, &cache2d(&offset))))),
            -40.0,
            320.0,
        );

        let vein_y = |d: F| interpolated(&range(&y(), -60.0, 51.0, &d, &zero()));
        let vein_toggle = vein_y(noise(&vein_toggle_n, 1.5, 1.5));
        let vein_ridged = addc(-0.08, &max(&map(&vein_y(noise(&vein_a, 4.0, 4.0)), Map::Abs), &map(&vein_y(noise(&vein_b, 4.0, 4.0)), Map::Abs)));
        let vein_gap = noise(&ore_gap, 1.0, 1.0);
        let ore_vein_seed = r.next_long();

        // SurfaceHolder, SurfaceOverwriteHolder: all from the world seed itself.
        let ident = || r.identical();
        let plain = |first: i32, amps: &[f32]| NormalNoise::new(ident(), first, amps);
        let surface = plain(-6, &[1.0, 1.0, 1.0]);
        let overwrite = Overwrite {
            surface: plain(-6, &[1.0, 1.0, 1.0]),
            swamp: plain(-2, &[1.0]),
            clay_offset: plain(-8, &[1.0]),
            badlands_pillar: plain(-2, &[1.0, 1.0, 1.0]),
            badlands_roof: plain(-8, &[1.0]),
            badlands_surface: plain(-6, &[1.0, 1.0, 1.0]),
            iceberg_pillar: plain(-6, &[1.0, 1.0, 1.0, 1.0]),
            iceberg_roof: plain(-3, &[1.0]),
            iceberg_surface: plain(-6, &[1.0, 1.0, 1.0]),
        };

        Self {
            continents,
            temperature,
            humidity,
            erosion,
            ridges,
            depth,
            terrain: all_in_cell(&final_density),
            prelim_surface,
            prelim_upper,
            vein_toggle,
            vein_ridged,
            vein_gap,
            ore_vein_seed,
            barrier: noise(&barrier_n, 1.0, 0.5),
            floodedness: noise(&flood_n, 1.0, 0.67),
            spread: noise(&spread_n, 1.0, 0.7142857142857143),
            lava: noise(&lava_n, 1.0, 1.0),
            surface,
            overwrite,
        }
    }
}

/// Vanilla's top and bottom slides: fade density to fixed values near y -64 and 256.
fn slide(d: &F) -> F {
    addc(
        0.1171875,
        &mul(
            &y_grad(-64, -40, 0.0, 1.0),
            &addc(-0.1171875, &addc(-0.078125, &mul(&y_grad(240, 256, 1.0, 0.0), &addc(0.078125, d)))),
        ),
    )
}

fn overworld_offset(continents: &F, erosion: &F, rf: &F) -> F {
    // blendAlpha = 1 and blendOffset = 0 outside blended (old-world) chunks.
    let blend_alpha = once(&constant(1.0));
    let blend_term = mul(&constant(0.0), &addc(1.0, &mulc(-1.0, &blend_alpha)));
    let offset_term = mul(&addc(-0.50375, &offset_spline(continents, erosion, rf)), &blend_alpha);
    flat(&cache2d(&add(&blend_term, &offset_term)))
}

#[allow(clippy::too_many_lines)]
fn offset_spline(continents: &F, erosion: &F, rf: &F) -> F {
    let r1 = s!(rf, (-1.0, c(-0.08880186), 0.38940096), (1.0, c(0.69000006), 0.38940096));
    let r2 = s!(rf, (-1.0, c(-0.115760356), 0.37788022), (1.0, c(0.6400001), 0.37788022));
    let r3 = s!(rf, (-1.0, c(-0.2222), 0.0), (-0.75, c(-0.2222), 0.0), (-0.65, c(0.0), 0.0), (0.5954547, c(2.9802322E-8), 0.0), (0.6054547, c(2.9802322E-8), 0.2534563), (1.0, c(0.100000024), 0.2534563));
    let r4 = s!(rf, (-1.0, c(-0.3), 0.5), (-0.4, c(0.05), 0.0), (0.0, c(0.05), 0.0), (0.4, c(0.05), 0.0), (1.0, c(0.060000002), 0.007000001));
    let r5 = s!(rf, (-1.0, c(-0.15), 0.5), (-0.4, c(0.0), 0.0), (0.0, c(0.0), 0.0), (0.4, c(0.05), 0.1), (1.0, c(0.060000002), 0.007000001));
    let r6 = s!(rf, (-1.0, c(-0.15), 0.5), (-0.4, c(0.0), 0.0), (0.0, c(0.0), 0.0), (0.4, c(0.0), 0.0), (1.0, c(0.0), 0.0));
    let r7 = s!(rf, (-1.0, c(-0.02), 0.0), (-0.4, c(-0.03), 0.0), (0.0, c(-0.03), 0.0), (0.4, c(0.0), 0.06), (1.0, c(0.0), 0.0));
    let r7u = s!(rf, (-1.0, c(-0.02), 0.0), (-0.4, c(-0.03), 0.0), (0.0, c(-0.03), 0.0), (0.4, c(0.03), 0.12), (1.0, c(0.1), 0.049));
    let r8 = s!(rf, (-1.0, c(-0.25), 0.5), (-0.4, c(0.05), 0.0), (0.0, c(0.05), 0.0), (0.4, c(0.05), 0.0), (1.0, c(0.060000002), 0.007000001));
    let r9 = s!(rf, (-1.0, c(-0.1), 0.5), (-0.4, c(0.001), 0.01), (0.0, c(0.003), 0.01), (0.4, c(0.05), 0.094000004), (1.0, c(0.060000002), 0.007000001));
    let r10 = s!(rf, (-1.0, c(-0.1), 0.5), (-0.4, c(0.01), 0.0), (0.0, c(0.01), 0.0), (0.4, c(0.03), 0.04), (1.0, c(0.1), 0.049));
    let r12 = s!(rf, (-1.0, c(0.20235021), 0.0), (0.0, c(0.7161751), 0.5138249), (1.0, c(1.23), 0.5138249));
    let r13 = s!(rf, (-1.0, c(0.2), 0.0), (0.0, c(0.44682026), 0.43317974), (1.0, c(0.88), 0.43317974));
    let r14 = s!(rf, (-1.0, c(0.2), 0.0), (0.0, c(0.30829495), 0.3917051), (1.0, c(0.70000005), 0.3917051));
    let r15 = s!(rf, (-1.0, c(-0.25), 0.5), (-0.4, c(0.35), 0.0), (0.0, c(0.35), 0.0), (0.4, c(0.35), 0.0), (1.0, c(0.42000002), 0.049000014));
    let r16 = s!(rf, (-1.0, c(-0.1), 0.5), (-0.4, c(0.0069999998), 0.07), (0.0, c(0.021), 0.07), (0.4, c(0.35), 0.658), (1.0, c(0.42000002), 0.049000014));
    let r17 = s!(rf, (-1.0, c(-0.1), 0.5), (-0.4, c(0.01), 0.0), (0.0, c(0.01), 0.0), (0.4, c(0.03), 0.04), (1.0, c(0.1), 0.049));
    let r18 = s!(rf, (-1.0, c(-0.05), 0.5), (-0.4, c(0.01), 0.0), (0.0, c(0.01), 0.0), (0.4, c(0.03), 0.04), (1.0, c(0.1), 0.049));
    let r19 = s!(rf, (-1.0, c(0.2), 0.0), (0.0, c(0.5391705), 0.4608295), (1.0, c(1.0), 0.4608295));
    let r20 = s!(rf, (-1.0, c(-0.2), 0.5), (-0.4, c(0.5), 0.0), (0.0, c(0.5), 0.0), (0.4, c(0.5), 0.0), (1.0, c(0.6), 0.070000015));
    let r21 = s!(rf, (-1.0, c(-0.05), 0.5), (-0.4, c(0.01), 0.099999994), (0.0, c(0.03), 0.099999994), (0.4, c(0.5), 0.94), (1.0, c(0.6), 0.070000015));
    let r22 = s!(rf, (-1.0, c(-0.05), 0.5), (-0.4, c(0.01), 0.0), (0.0, c(0.01), 0.0), (0.4, c(0.03), 0.04), (1.0, c(0.1), 0.049));
    let r23 = s!(rf, (-1.0, c(-0.02), 0.015), (-0.4, c(0.01), 0.0), (0.0, c(0.01), 0.0), (0.4, c(0.03), 0.04), (1.0, c(0.1), 0.049));
    let r24 = s!(rf, (-1.0, c(0.34792626), 0.0), (0.0, c(0.9239631), 0.5760369), (1.0, c(1.5), 0.5760369));
    let r25 = s!(rf, (-1.0, c(-0.1), 0.0), (-0.4, f(&r17), 0.0), (0.0, c(0.17), 0.0));
    let r26 = s!(rf, (-1.0, c(-0.05), 0.0), (-0.4, f(&r18), 0.0), (0.0, c(0.17), 0.0));
    let e1 = s!(erosion, (-0.85, f(&r1), 0.0), (-0.7, f(&r2), 0.0), (-0.4, f(&r3), 0.0), (-0.35, f(&r4), 0.0), (-0.1, f(&r5), 0.0), (0.2, f(&r6), 0.0), (0.7, f(&r7), 0.0));
    let e2 = s!(erosion, (-0.85, f(&r1), 0.0), (-0.7, f(&r2), 0.0), (-0.4, f(&r3), 0.0), (-0.35, f(&r8), 0.0), (-0.1, f(&r9), 0.0), (0.2, f(&r10), 0.0), (0.7, f(&r7u), 0.0));
    let e3 = s!(erosion, (-0.85, f(&r12), 0.0), (-0.7, f(&r13), 0.0), (-0.4, f(&r14), 0.0), (-0.35, f(&r15), 0.0), (-0.1, f(&r16), 0.0), (0.2, f(&r17), 0.0), (0.4, f(&r17), 0.0), (0.45, f(&r25), 0.0), (0.55, f(&r25), 0.0), (0.58, f(&r18), 0.0), (0.7, f(&r7u), 0.0));
    let e4 = s!(erosion, (-0.85, f(&r24), 0.0), (-0.7, f(&r19), 0.0), (-0.4, f(&r19), 0.0), (-0.35, f(&r20), 0.0), (-0.1, f(&r21), 0.0), (0.2, f(&r22), 0.0), (0.4, f(&r22), 0.0), (0.45, f(&r26), 0.0), (0.55, f(&r26), 0.0), (0.58, f(&r22), 0.0), (0.7, f(&r23), 0.0));
    s!(continents, (-1.1, c(0.044), 0.0), (-1.02, c(-0.2222), 0.0), (-0.51, c(-0.2222), 0.0), (-0.44, c(-0.12), 0.0), (-0.18, c(-0.12), 0.0), (-0.16, f(&e1), 0.0), (-0.15, f(&e1), 0.0), (-0.1, f(&e2), 0.0), (0.25, f(&e3), 0.0), (1.0, f(&e4), 0.0))
}

fn overworld_factor(continents: &F, erosion: &F, ridges: &F, rf: &F) -> F {
    let ra = s!(ridges, (-0.2, c(6.3), 0.0), (0.2, c(6.25), 0.0));
    let rb = s!(ridges, (-0.05, c(6.3), 0.0), (0.05, c(2.67), 0.0));
    let rc = s!(ridges, (-0.05, c(2.67), 0.0), (0.05, c(6.3), 0.0));
    let rd = s!(ridges, (-0.2, c(6.3), 0.0), (0.2, c(5.47), 0.0));
    let re = s!(ridges, (-0.2, c(6.3), 0.0), (0.2, c(5.08), 0.0));
    let rfx = s!(ridges, (-0.2, c(6.3), 0.0), (0.2, c(4.69), 0.0));
    let rg = s!(ridges, (0.0, c(6.25), 0.0), (0.1, c(0.625), 0.0));
    let rh = s!(ridges, (0.0, c(5.47), 0.0), (0.1, c(0.625), 0.0));
    let ri = s!(ridges, (0.0, c(5.08), 0.0), (0.1, c(0.625), 0.0));
    let fa = s!(rf, (-0.9, c(6.25), 0.0), (-0.69, f(&rg), 0.0));
    let fb = s!(rf, (-0.9, c(5.47), 0.0), (-0.69, f(&rh), 0.0));
    let fc = s!(rf, (-0.9, c(5.08), 0.0), (-0.69, f(&ri), 0.0));
    let fd = s!(rf, (0.45, f(&rfx), 0.0), (0.7, c(1.56), 0.0));
    let fe = s!(rf, (-0.7, f(&rfx), 0.0), (-0.15, c(1.37), 0.0));
    let e1 = s!(erosion, (-0.6, f(&ra), 0.0), (-0.5, f(&rb), 0.0), (-0.35, f(&ra), 0.0), (-0.25, f(&ra), 0.0), (-0.1, f(&rc), 0.0), (0.03, f(&ra), 0.0), (0.35, c(6.25), 0.0), (0.45, f(&fa), 0.0), (0.55, f(&fa), 0.0), (0.62, c(6.25), 0.0));
    let e2 = s!(erosion, (-0.6, f(&rd), 0.0), (-0.5, f(&rb), 0.0), (-0.35, f(&rd), 0.0), (-0.25, f(&rd), 0.0), (-0.1, f(&rc), 0.0), (0.03, f(&rd), 0.0), (0.35, c(5.47), 0.0), (0.45, f(&fb), 0.0), (0.55, f(&fb), 0.0), (0.62, c(5.47), 0.0));
    let e3 = s!(erosion, (-0.6, f(&re), 0.0), (-0.5, f(&rb), 0.0), (-0.35, f(&re), 0.0), (-0.25, f(&re), 0.0), (-0.1, f(&rc), 0.0), (0.03, f(&re), 0.0), (0.35, c(5.08), 0.0), (0.45, f(&fc), 0.0), (0.55, f(&fc), 0.0), (0.62, c(5.08), 0.0));
    let e4 = s!(erosion, (-0.6, f(&rfx), 0.0), (-0.5, f(&rb), 0.0), (-0.35, f(&rfx), 0.0), (-0.25, f(&rfx), 0.0), (-0.1, f(&rc), 0.0), (0.03, f(&rfx), 0.0), (0.05, f(&fd), 0.0), (0.4, f(&fd), 0.0), (0.45, f(&fe), 0.0), (0.55, f(&fe), 0.0), (0.58, c(4.69), 0.0));
    let sp = s!(continents, (-0.19, c(3.95), 0.0), (-0.15, f(&e1), 0.0), (-0.1, f(&e2), 0.0), (0.03, f(&e3), 0.0), (0.06, f(&e4), 0.0));
    flat(&cache2d(&addc(10.0, &mul(&constant(1.0), &addc(-10.0, &sp)))))
}

fn overworld_jaggedness(continents: &F, erosion: &F, ridges: &F, rf: &F) -> F {
    let hi = || s!(ridges, (-0.01, c(0.63), 0.0), (0.01, c(0.3), 0.0));
    let lo = || s!(ridges, (-0.01, c(0.315), 0.0), (0.01, c(0.15), 0.0));
    let f1 = s!(rf, (0.19999999, c(0.0), 0.0), (0.44999996, c(0.0), 0.0), (1.0, f(&hi()), 0.0));
    let f2 = s!(rf, (0.19999999, c(0.0), 0.0), (0.44999996, c(0.0), 0.0), (1.0, f(&lo()), 0.0));
    let f3 = s!(rf, (0.19999999, c(0.0), 0.0), (0.44999996, c(0.0), 0.0), (1.0, f(&lo()), 0.0));
    let f4 = s!(rf, (0.19999999, c(0.0), 0.0), (0.44999996, f(&hi()), 0.0), (1.0, f(&hi()), 0.0));
    let f5 = s!(rf, (0.19999999, c(0.0), 0.0), (0.44999996, c(0.0), 0.0), (1.0, f(&hi()), 0.0));
    let e1 = s!(erosion, (-1.0, f(&f1), 0.0), (-0.78, f(&f2), 0.0), (-0.5775, f(&f3), 0.0), (-0.375, c(0.0), 0.0));
    let e2 = s!(erosion, (-1.0, f(&f4), 0.0), (-0.78, f(&f5), 0.0), (-0.5775, f(&f5), 0.0), (-0.375, c(0.0), 0.0));
    let sp = s!(continents, (-0.11, c(0.0), 0.0), (0.03, f(&e1), 0.0), (0.65, f(&e2), 0.0));
    flat(&cache2d(&addc(0.0, &mul(&constant(1.0), &addc(-0.0, &sp)))))
}

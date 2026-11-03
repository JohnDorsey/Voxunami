//! An orbit from a state (`docs/orbits-and-thrust-plan.md`, step A4): the place and the velocity of a body against what it goes round, the mass parameter mu = G M of that body
//! and the time, made into the `Orbit` that passes through them. It is what puts a free ship on a rail, puts it on a new one after a burn or a bump, and (the owner's
//! choice (b), 2026-11-02) puts every ship round a body on a new one when the mass of the body changes: the place and the speed do not change, the orbit does.

use std::f64::consts::TAU;

use proptest::prelude::*;
use world_storage::rail::{Centre, ExactOffset, Orbit, Rail, RailError, RailTime};

const NOWHERE: [f64; 3] = [0.0; 3];
const TICKS_A_SECOND: u32 = 20;

fn rail_of(orbit: &Orbit) -> Rail {
    Rail {
        spin: None,
        orbit: Some(orbit.clone()),
    }
}

/// The mass parameter that makes `period` the time of an orbit of semi-major axis `radius`: mu = n^2 a^3.
fn mu_of(radius: f64, period: f64) -> f64 {
    let rate = TAU / period;
    rate * rate * radius * radius * radius
}

fn time(tick: u64) -> RailTime {
    RailTime::of_tick(tick, TICKS_A_SECOND)
}

fn state_at(orbit: &Orbit, tick: u64) -> (ExactOffset, [f64; 3]) {
    let state = rail_of(orbit).state(time(tick), NOWHERE, NOWHERE);
    (
        state.offset.expect("an orbit has an offset"),
        state.velocity,
    )
}

fn fit(offset: ExactOffset, velocity: [f64; 3], mu: f64, tick: u64) -> Result<Orbit, RailError> {
    Orbit::from_state(Centre::Point(NOWHERE), offset, velocity, mu, time(tick))
}

fn fitted(orbit: &Orbit, mu: f64, tick: u64) -> Result<Orbit, RailError> {
    let (offset, velocity) = state_at(orbit, tick);
    fit(offset, velocity, mu, tick)
}

/// How far apart two offsets are, in voxels.
fn apart(first: ExactOffset, second: ExactOffset) -> f64 {
    (0..3)
        .map(|axis| {
            let difference = (first.bricks[axis] - second.bricks[axis]) as f64 * 32.0
                + (first.rest[axis] - second.rest[axis]);
            difference * difference
        })
        .sum::<f64>()
        .sqrt()
}

fn size(vector: [f64; 3]) -> f64 {
    vector.iter().map(|part| part * part).sum::<f64>().sqrt()
}

fn an_orbit(
    normal: [f64; 3],
    radius: f64,
    ratio: f64,
    periapsis: f64,
    period: f64,
    phase: f64,
) -> Orbit {
    Orbit::elliptical(
        Centre::Point(NOWHERE),
        normal,
        radius,
        ratio,
        periapsis,
        period,
        phase,
    )
    .unwrap()
}

/// The two orbits are one: from the tick of the fit and for some ticks after it they are at the same place (to `position` of the semi-major axis) and go at the same speed
/// (to `speed` of the speed).
fn the_same_motion(original: &Orbit, found: &Orbit, tick: u64, position: f64, speed: f64) {
    for later in [0u64, 1, 17, 400, 20_000] {
        let (place, velocity) = state_at(original, tick + later);
        let (found_place, found_velocity) = state_at(found, tick + later);
        let allowed = original.radius() * position;
        assert!(
            apart(place, found_place) <= allowed,
            "{later} ticks on: {} voxels apart, {allowed} allowed (ratio {})",
            apart(place, found_place),
            original.ratio()
        );
        let allowed = speed * size(velocity);
        for axis in 0..3 {
            assert!(
                (velocity[axis] - found_velocity[axis]).abs() <= allowed,
                "{later} ticks on, axis {axis}: {velocity:?} against {found_velocity:?} ({allowed} allowed)"
            );
        }
    }
}

/// `(normal, radius, ratio, periapsis, period, phase, tick)` of an orbit to fit, and to pin.
type FitCase = ([f64; 3], f64, f64, f64, f64, f64, u64);

const FIT_CASES: [FitCase; 6] = [
    // normal, radius, ratio, periapsis, period, phase, tick
    (
        [0.0, 1.0, 0.0],
        1.5e11,
        0.0167,
        1.8,
        3.156e7,
        0.3,
        4_000_000u64,
    ),
    (
        [0.6, 0.8, 0.0],
        3.84e8,
        0.945,
        -2.5,
        2.36e6,
        5.5,
        18_446_744_073_709,
    ),
    ([0.2, -0.5, 0.8], 7.0e6, 0.3, 2.0, 5_400.0, 6.2, 123_456_789),
    ([0.0, 0.0, 1.0], 1.0e12, 0.01, 0.0, 9.0e8, 1.0, 1),
    (
        [1.0, 1.0, 1.0],
        2.5e20,
        0.5,
        3.0,
        7.0e15,
        2.0,
        7_600_000_000_037,
    ),
    (
        [0.0, 1.0, 0.0],
        8.8e26,
        0.2,
        -3.0,
        6.3e12,
        3.0,
        99_999_999_999,
    ),
];

#[test]
fn the_orbit_a_state_is_on_is_the_orbit_it_came_from() {
    let cases = FIT_CASES;
    for (normal, radius, ratio, periapsis, period, phase, tick) in cases {
        let original = an_orbit(normal, radius, ratio, periapsis, period, phase);
        let found = fitted(&original, mu_of(radius, period), tick).unwrap();
        // What a state of `f64`s holds of the size and the shape of the orbit it is on.
        assert!(
            (found.radius() - radius).abs() <= 1.0e-9 * radius,
            "radius {} against {radius}",
            found.radius()
        );
        assert!(
            (found.ratio() - ratio).abs() <= 1.0e-9,
            "ratio {} against {ratio}",
            found.ratio()
        );
        assert!(
            (found.period() - period).abs() <= 1.0e-9 * period,
            "period {} against {period}",
            found.period()
        );
        the_same_motion(&original, &found, tick, 1.0e-10, 1.0e-10);
    }
}

fn any_normal() -> impl Strategy<Value = [f64; 3]> {
    prop::array::uniform3(-1.0f64..1.0).prop_filter("a direction that is not nothing", |normal| {
        normal.iter().map(|part| part * part).sum::<f64>() > 0.04
    })
}

proptest! {
    #[test]
    fn any_orbit_either_way_round_is_found_from_its_state(
        normal in any_normal(),
        log_radius in 6.0f64..20.0,
        ratio in 0.05f64..1.0,
        periapsis in -7.0f64..7.0,
        period in prop_oneof![1.0e3f64..1.0e9, -1.0e9f64..-1.0e3],
        phase in -7.0f64..7.0,
        tick in 0u64..1_000_000_000_000,
    ) {
        let radius = libm::pow(10.0, log_radius);
        let original = an_orbit(normal, radius, ratio, periapsis, period, phase);
        let found = fitted(&original, mu_of(radius, period.abs()), tick);
        let found = found.unwrap();
        // An orbit that goes the other way round is found as the orbit about the opposite normal, gone round the right way: the same motion.
        prop_assert!(found.period() > 0.0);
        // (Measured over 3,000 orbits of ratios 0.05 to 1 and radii 1e3 to 1e24: 4e-13 of a radius in place, 1e-14 of the speed; the most is the grid of 2^-32 voxel on a small radius.)
        the_same_motion(&original, &found, tick, 1.0e-11, 1.0e-11);
    }
}

proptest! {
    #[test]
    fn a_thin_ellipse_is_found_to_what_the_energy_in_its_state_can_say(
        normal in any_normal(),
        log_radius in 6.0f64..20.0,
        log_ratio in -9.0f64..-3.0,
        periapsis in -7.0f64..7.0,
        period in prop_oneof![1.0e3f64..1.0e9, -1.0e9f64..-1.0e3],
        phase in -7.0f64..7.0,
        tick in 0u64..1_000_000_000_000,
    ) {
        // The semi-major axis is the difference of two terms of the size of the state, so the thinner the ellipse the less of it is left. Measured over 6,000 orbits: for ratios of 1e-6
        // to 1e-3, 3.5e-13 of a radius in place and 5e-11 of the speed; for ratios of 1e-9 to 1e-6, 1.2e-11 and 2e-9. These are ten times those.
        let ratio = libm::pow(10.0, log_ratio);
        let radius = libm::pow(10.0, log_radius);
        let original = an_orbit(normal, radius, ratio, periapsis, period, phase);
        let found = fitted(&original, mu_of(radius, period.abs()), tick).unwrap();
        let (position, speed) = if ratio >= 1.0e-6 { (4.0e-12, 5.0e-10) } else { (1.2e-10, 2.0e-8) };
        the_same_motion(&original, &found, tick, position, speed);
    }
}

#[test]
fn a_circle_is_found_a_circle() {
    for (normal, radius, period, phase, tick) in [
        ([0.0, 1.0, 0.0], 1.5e11, 3.156e7, 0.3, 4_000_000u64),
        ([0.6, 0.8, 0.0], 3.84e8, -2.36e6, 5.5, 18_446_744_073_709),
        ([0.2, -0.5, 0.8], 7.0e6, 5_400.0, 6.2, 123_456_789),
        ([0.0, 0.0, 1.0], 4.0e4, 3.7, 1.0, 1),
    ] {
        let original = an_orbit(normal, radius, 1.0, 0.0, period, phase);
        let found = fitted(&original, mu_of(radius, period.abs()), tick).unwrap();
        // (A circle has no nearest point: what a state of `f64`s can say of its shape is nothing below a few parts in 10^15, so it is a circle, and starts toward the first direction.)
        assert_eq!(
            (found.ratio(), found.periapsis()),
            (1.0, 0.0),
            "radius {radius}"
        );
        assert!((found.radius() - radius).abs() <= 1.0e-12 * radius);
        the_same_motion(&original, &found, tick, 1.0e-10, 1.0e-10);
    }
}

#[test]
fn an_orbit_that_is_nearly_a_circle_is_found_with_the_little_eccentricity_it_has() {
    // The eccentricity vector is found to about 1e-16, so an eccentricity of 1e-6 is found to 1e-10 of itself and is not taken for a circle (which would put the body 1e-6 of a
    // radius out); one of 1e-12 is found to a ten-thousandth of itself.
    for (eccentricity, direction) in [(1.0e-6, 1.1), (1.0e-9, -2.0), (1.0e-12, 0.5)] {
        let ratio = 1.0 - eccentricity;
        let original = an_orbit([0.3, 0.9, -0.2], 5.0e8, ratio, direction, 3.0e5, 1.5);
        let found = fitted(&original, mu_of(5.0e8, 3.0e5), 777_777).unwrap();
        assert!(
            found.ratio() < 1.0,
            "e = {eccentricity}: taken for a circle"
        );
        assert!(
            (found.ratio() - ratio).abs() <= 2.0e-16 + 1.0e-4 * eccentricity,
            "e = {eccentricity}: ratio {} against {ratio}",
            found.ratio()
        );
        the_same_motion(&original, &found, 777_777, 1.0e-11, 1.0e-11);
    }
    // At a few parts in 10^15 it is a circle as far as any state can say.
    let original = an_orbit([0.3, 0.9, -0.2], 5.0e8, 1.0 - 1.0e-15, 1.1, 3.0e5, 1.5);
    let found = fitted(&original, mu_of(5.0e8, 3.0e5), 777_777).unwrap();
    assert_eq!((found.ratio(), found.periapsis()), (1.0, 0.0));
}

#[test]
fn the_phase_is_the_mean_anomaly_at_tick_0_worked_out_in_whole_numbers_however_late_the_fit() {
    // A fit at a tick a thousand years in is the same orbit as one at the start of the world: the same elements to what the state holds, and a phase that is a fraction of a turn.
    let original = an_orbit([0.0, 1.0, 0.0], 1.5e11, 0.0167, 1.8, 3.156e7, 0.3);
    let mu = mu_of(1.5e11, 3.156e7);
    let early = fitted(&original, mu, 0).unwrap();
    let late = fitted(&original, mu, 640_000_000_000).unwrap();
    for found in [early.clone(), late] {
        assert!(
            (found.phase() - 0.3).abs() < 1.0e-6
                || (found.phase() - 0.3 + TAU).abs() < 1.0e-6
                || (found.phase() - 0.3 - TAU).abs() < 1.0e-6,
            "{}",
            found.phase()
        );
        assert!(found.phase().abs() <= TAU);
    }
    // A thousand periods on, the orbit made from a state is out of step with the original by what its period is out: the semi-major axis is found to about 2e-16 over the ratio
    // (it is the difference of two terms of the size of the state), and the period goes as the axis to the three halves, so a thousand turns of 2 pi are out by
    // 2 pi x 1,000 x 1.5 x 2e-16 / ratio of a radius. This is why an orbit that is not touched is kept and never made again from its state (the owner's goal): each making costs this.
    let periods = 640_000_000_000.0 / 20.0 / 3.156e7;
    let allowed = original.radius() * TAU * periods * 1.5 * 2.0e-16 / 0.0167 * 4.0;
    let (original_place, _) = state_at(&original, 640_000_000_000);
    let (found_place, _) = state_at(&early, 640_000_000_000);
    assert!(
        apart(original_place, found_place) <= allowed,
        "{} voxels apart, {allowed} allowed",
        apart(original_place, found_place)
    );
}

#[test]
fn a_new_mass_keeps_the_place_and_the_speed_of_the_moment_and_changes_the_orbit() {
    // The owner's choice (b), 2026-11-02: when what a ship goes round weighs more or less, the ship is where it was and goes as fast as it did, and is on another orbit.
    let radius = 4.0e8;
    let period = 2.0e6;
    let mu = mu_of(radius, period);
    let original = an_orbit([0.3, 0.9, -0.2], radius, 0.8, 1.0, period, 2.0);
    let tick = 31_415_926;
    let (place, velocity) = state_at(&original, tick);
    for scale in [1.3, 0.8, 1.0] {
        let found = fit(place, velocity, mu * scale, tick).unwrap();
        let (found_place, found_velocity) = state_at(&found, tick);
        assert!(
            apart(place, found_place) <= 1.0e-9 * radius,
            "scale {scale}: {} voxels",
            apart(place, found_place)
        );
        for axis in 0..3 {
            assert!(
                (velocity[axis] - found_velocity[axis]).abs() <= 1.0e-9 * size(velocity),
                "scale {scale}, axis {axis}"
            );
        }
        // Kepler's third law with the new mass, and the old orbit when the mass is the old one.
        let expected =
            TAU * libm::sqrt(found.radius() * found.radius() * found.radius() / (mu * scale));
        assert!((found.period() - expected).abs() <= 1.0e-12 * expected);
        if scale == 1.0 {
            assert!((found.radius() - radius).abs() <= 1.0e-9 * radius);
        } else {
            assert!(
                (found.radius() - radius).abs() >= 1.0e-3 * radius,
                "scale {scale}"
            );
        }
    }
    // A heavier body holds the same ship in a smaller orbit, and a lighter one in a larger.
    let heavier = fit(place, velocity, mu * 1.3, tick).unwrap();
    let lighter = fit(place, velocity, mu * 0.8, tick).unwrap();
    assert!(heavier.radius() < radius && lighter.radius() > radius);
}

#[test]
fn a_state_that_is_no_ellipse_is_refused() {
    let original = an_orbit([0.0, 1.0, 0.0], 1.0e9, 0.5, 0.0, 1.0e5, 0.0);
    let mu = mu_of(1.0e9, 1.0e5);
    let (place, velocity) = state_at(&original, 100);
    let speed = size(velocity);
    let refused = |result: Result<Orbit, RailError>, what: &str| {
        assert!(
            matches!(result, Err(RailError::Invalid(_))),
            "{what}: {result:?}"
        );
    };
    // Fast enough to get away (the speed of escape is the root of two times the speed of a circle at that distance), or exactly that fast: a parabola.
    let distance = size(place.in_voxels());
    let escape = libm::sqrt(2.0 * mu / distance);
    let along = velocity.map(|part| part / speed);
    refused(
        fit(place, along.map(|part| part * escape * 1.01), mu, 100),
        "faster than escape",
    );
    refused(
        fit(place, along.map(|part| part * escape), mu, 100),
        "exactly escape",
    );
    // A hair below the speed of escape says nothing of the shape (the energy is the difference of two terms that agree to 4e-14 of themselves, which is the noise of the
    // difference): refused, on an orbit small enough that the period it would give is still a period a rail can have. A speed comfortably below it is a very long ellipse.
    let small = an_orbit([0.0, 1.0, 0.0], 1.0e6, 0.5, 0.0, 100.0, 0.0);
    let small_mu = mu_of(1.0e6, 100.0);
    let (small_place, small_velocity) = state_at(&small, 100);
    let small_distance = size(small_place.in_voxels());
    let small_escape = libm::sqrt(2.0 * small_mu / small_distance);
    let small_along = small_velocity.map(|part| part / size(small_velocity));
    refused(
        fit(
            small_place,
            small_along.map(|part| part * small_escape * (1.0 - 2.0e-14)),
            small_mu,
            100,
        ),
        "a hair below escape",
    );
    let long = fit(
        small_place,
        small_along.map(|part| part * small_escape * (1.0 - 1.0e-6)),
        small_mu,
        100,
    )
    .unwrap();
    assert!(long.radius() > 1.0e5 * small_distance, "{}", long.radius());
    // No plane: at rest, or going straight at the centre or straight away from it.
    refused(fit(place, [0.0; 3], mu, 100), "at rest");
    let toward = place.in_voxels().map(|part| -part / distance * speed * 0.1);
    refused(fit(place, toward, mu, 100), "straight at the centre");
    // Numbers that are not numbers, and a mass that weighs nothing.
    refused(
        fit(place, [f64::NAN, 0.0, 0.0], mu, 100),
        "a velocity that is not a number",
    );
    refused(fit(place, velocity, 0.0, 100), "a mass of nothing");
    refused(fit(place, velocity, -1.0, 100), "a mass below nothing");
    refused(
        fit(place, velocity, f64::NAN, 100),
        "a mass that is not a number",
    );
    refused(
        fit(place, velocity, f64::INFINITY, 100),
        "a mass of infinity",
    );
    // On the centre itself.
    let nowhere = ExactOffset {
        bricks: [0; 3],
        rest: [0.0; 3],
    };
    refused(fit(nowhere, velocity, mu, 100), "on the centre");
}

/// The elements the program made from the state of each of `FIT_CASES` once they agreed with the originals above, as bits: `(normal, radius, ratio, periapsis, period, phase)`.
/// The same on every machine (the working is `f64` and `libm`, and whole numbers for the phase), which an orbit that is simulation must be.
const FIT_PINS: [([u64; 3], u64, u64, u64, u64, u64); 6] = [
    (
        [0x0, 0x3ff0000000000000, 0x0],
        0x424176592dfffffe,
        0x3f9119ce075f6fd3,
        0x3ffccccccccccccd,
        0x417e1913fffffffc,
        0x3fd3333333333333,
    ),
    (
        [0x3fe3333333333333, 0x3fe999999999999a, 0x3c767091424eea80],
        0x41b6e36000000000,
        0x3fee3d70a3d70a3d,
        0xc004000000000001,
        0x4142016000000000,
        0x4016000000000000,
    ),
    (
        [0x3fca8bc441a3d031, 0xbfe0975aa906621c, 0x3fea8bc441a3d02d],
        0x415ab3f000000002,
        0x3fd3333333333332,
        0x4000000000000000,
        0x40b5180000000003,
        0x4018ccccccccdcc2,
    ),
    (
        [0x0, 0x0, 0x3ff0000000000000],
        0x426d1a94a1ffffff,
        0x3f847ae147ae147d,
        0x0,
        0x41cad2747fffffff,
        0x3fefffffffffffea,
    ),
    (
        [0x3fe279a74590331c, 0x3fe279a74590331c, 0x3fe279a74590331c],
        0x442b1ae4d6e2ef4f,
        0x3fe0000000000001,
        0x4007ffffffffffff,
        0x4338de76816d7ffe,
        0x4000000000000002,
    ),
    (
        [0x0, 0x3ff0000000000000, 0x0],
        0x4586bf59fcb70388,
        0x3fc999999999999c,
        0xc008000000000000,
        0x4296eb550c600002,
        0x4008000000000001,
    ),
];

#[test]
fn the_orbit_made_from_a_state_is_pinned_to_the_bit() {
    for ((normal, radius, ratio, periapsis, period, phase, tick), pin) in
        FIT_CASES.into_iter().zip(FIT_PINS)
    {
        let original = an_orbit(normal, radius, ratio, periapsis, period, phase);
        let found = fitted(&original, mu_of(radius, period), tick).unwrap();
        assert_eq!(
            (
                found.normal().map(f64::to_bits),
                found.radius().to_bits(),
                found.ratio().to_bits(),
                found.periapsis().to_bits(),
                found.period().to_bits(),
                found.phase().to_bits()
            ),
            pin,
            "radius {radius}, ratio {ratio}, tick {tick}"
        );
    }
}

#[test]
fn an_orbit_from_a_place_given_as_plain_voxels_is_the_orbit_from_the_state_of_that_place() {
    // A ship that is not on a rail has its place as an f64 and no exact offset: `from_place` is what `from_state` does with the plain numbers of an offset, to the bit.
    let orbit = an_orbit([0.3, 0.8, -0.2], 6.8e6, 0.9, 1.1, 5581.0, 0.7);
    let mu = mu_of(6.8e6, 5581.0);
    let (offset, velocity) = state_at(&orbit, 1234);
    let from_state = fit(offset, velocity, mu, 1234).unwrap();
    let from_place = Orbit::from_place(
        Centre::Point(NOWHERE),
        offset.in_voxels(),
        velocity,
        mu,
        time(1234),
    )
    .unwrap();
    assert_eq!(from_place, from_state);
    assert_eq!(
        Orbit::from_place(Centre::Point(NOWHERE), NOWHERE, velocity, mu, time(1234)),
        Err(RailError::Invalid("a body on its centre is on no orbit"))
    );
}

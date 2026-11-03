//! The wide Kepler evaluator (`wide_kepler`) against the rails the game has (`exact_kepler` and `exact_circle`, in 126-bit fixed point): for the same elements the two must
//! put a body in the same place, to what the fixed point can say, and give it the same speed. This is the bridge for widening the rails of the game: the numbers a ship has
//! today are doubles, which any width holds exactly, so whatever a wide rail does with them it must not move a ship that is on a rail now.

use proptest::prelude::*;
use wide_float::{Precision, WideFloat};
use wide_kepler::{Elements, Time};
use world_storage::rail::{Centre, ExactOffset, Orbit, Rail, RailTime};

const NOWHERE: [f64; 3] = [0.0; 3];
const TICKS_A_SECOND: u32 = 20;

fn at(bits: u64) -> Precision {
    Precision::new(bits).unwrap()
}

fn wide(value: f64, precision: Precision) -> WideFloat {
    WideFloat::from_f64(value, precision).unwrap()
}

/// The wide elements of a rail's orbit: the same doubles, as the program keeps them.
fn elements_of(orbit: &Orbit, precision: Precision) -> Elements {
    Elements {
        normal: orbit.normal.map(|component| wide(component, precision)),
        radius: wide(orbit.radius, precision),
        ratio: wide(orbit.ratio, precision),
        periapsis: wide(orbit.periapsis, precision),
        period: wide(orbit.period, precision),
        phase: wide(orbit.phase, precision),
    }
}

/// An offset of the rail as a wide number: whole bricks of 32 voxels and a rest.
fn voxels_of(offset: &ExactOffset, precision: Precision) -> [WideFloat; 3] {
    [0, 1, 2].map(|axis| {
        &(&WideFloat::from_i128(offset.bricks[axis], precision) * &wide(32.0, precision))
            + &wide(offset.rest[axis], precision)
    })
}

/// How far apart the rail and the wide evaluator put a body, in voxels, and how different their speeds are in the fraction of the speed.
fn differences(orbit: Orbit, tick: u64) -> (f64, f64) {
    differences_between_ticks(orbit, tick, tick)
}

/// As [`differences`], with the rail asked at one tick and the wide evaluator at another (the same, but for a negative control).
fn differences_between_ticks(orbit: Orbit, rail_tick: u64, wide_tick: u64) -> (f64, f64) {
    let precision = at(256);
    let rail = Rail {
        spin: None,
        orbit: Some(orbit),
    };
    let state = rail.state(
        RailTime::of_tick(rail_tick, TICKS_A_SECOND),
        NOWHERE,
        NOWHERE,
    );
    let offset = voxels_of(&state.offset.unwrap(), precision);
    let wide_place = elements_of(&orbit, precision)
        .place_at(
            Time {
                units: i128::from(wide_tick),
                units_per_second: TICKS_A_SECOND,
            },
            precision,
        )
        .unwrap();
    let apart = (0..3)
        .map(|axis| (&offset[axis] - &wide_place.position[axis]).abs().to_f64())
        .fold(0.0, f64::max);
    let speed = wide_place
        .velocity
        .iter()
        .map(|component| component.to_f64() * component.to_f64())
        .sum::<f64>()
        .sqrt();
    let speed_apart = (0..3)
        .map(|axis| (state.velocity[axis] - wide_place.velocity[axis].to_f64()).abs())
        .fold(0.0, f64::max);
    (apart, speed_apart / speed)
}

fn orbit(
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// The place of the body, in voxels: the rail's is exact to 2^-32 voxel and its own few units (and a part in 10^20 of a turn for the sharpest of ellipses at their
    /// periapsis, a radius of 1e12 voxels making that 1e-8), so the two agree to a millionth of a voxel; the speed to a part in 10^9.
    #[test]
    fn the_wide_evaluator_puts_a_body_where_the_rail_does(
        normal in proptest::array::uniform3(-1.0f64..1.0),
        radius in 10.0f64..1.0e12,
        ratio in prop_oneof![Just(1.0), 0.001f64..1.0, 1.0e-12f64..1.0e-3],
        periapsis in -10.0f64..10.0,
        period in 20.0f64..1.0e7,
        phase in -100.0f64..100.0,
        tick in 0u64..2_000_000_000,
    ) {
        prop_assume!(normal.iter().any(|component| component.abs() > 1.0e-3));
        let (apart, speed_apart) = differences(orbit(normal, radius, ratio, periapsis, period, phase), tick);
        prop_assert!(apart < 1.0e-6, "{apart} voxels apart");
        prop_assert!(speed_apart < 1.0e-9, "the speeds differ by {speed_apart} of the speed");
    }
}

#[test]
fn the_wide_evaluator_agrees_with_the_rails_that_are_pinned_in_the_game() {
    // The orbits of `rail_ellipse.rs` and `rail_circle_bits.rs`: a low orbit round an Earth-weight planet, a moon's, a planet's year, a comet, and a circle with a periapsis.
    let cases = [
        (
            orbit([0.0, 1.0, 0.0], 6.8e6, 0.9, 0.3, 5581.0, 0.0),
            12345u64,
        ),
        (
            orbit([0.3, 0.5, 0.8], 3.84e8, 0.945, 2.0, 2.36e6, 1.0),
            98_765_432,
        ),
        (
            orbit([0.0, 0.0, 1.0], 1.5e11, 0.983, -1.0, 3.156e7, 3.0),
            1_000_000_000,
        ),
        (
            orbit([1.0, 1.0, 1.0], 5.0e11, 1.0e-6, 0.5, 1.0e9, 0.0),
            1_999_999_999,
        ),
        (orbit([0.95, 0.1, 0.1], 1.0e4, 1.0, 1.1, 600.0, 0.0), 77),
        (orbit([0.0, 1.0, 0.0], 1.0e7, 1.0, 0.0, 600.0, 4.0), 12_000),
    ];
    for (orbit, tick) in cases {
        let (apart, speed_apart) = differences(orbit, tick);
        println!(
            "{:>12.3e} voxels apart, speeds differ by {speed_apart:.1e}: {orbit:?} at tick {tick}",
            apart
        );
        assert!(
            apart < 1.0e-6,
            "{orbit:?} at tick {tick}: {apart} voxels apart"
        );
        assert!(
            speed_apart < 1.0e-9,
            "{orbit:?} at tick {tick}: speeds differ by {speed_apart}"
        );
        // The negative control: a tick later the body is somewhere else, by far more than the tolerance (the test can fail).
        let (later, _) = differences_between_ticks(orbit, tick, tick + 1);
        assert!(
            later > 1.0e-3,
            "{orbit:?}: the body moved {later} voxels in a tick"
        );
    }
}

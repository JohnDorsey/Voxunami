//! The mean anomaly of an orbit as a part of a turn (`Elements::mean_anomaly_in_turns`): the phase and the part of a turn gone since time 0, taken modulo one, exact however
//! long the world has run. The rails use it to tell which end of an ellipse a body passes (`world_storage::rail::Orbit::apsides_passed`), and `place_at` starts from the same
//! number.

use wide_float::{Precision, WideFloat};
use wide_kepler::{Elements, KeplerError, Time};

fn at(bits: u64) -> Precision {
    Precision::new(bits).unwrap()
}

fn whole(value: i128, precision: Precision) -> WideFloat {
    WideFloat::from_i128(value, precision)
}

/// An orbit whose period is a whole number of seconds and whose phase is nothing, with the rest not to the point.
fn elements(period: i64, phase: f64, precision: Precision) -> Elements {
    Elements {
        normal: [
            WideFloat::zero(precision),
            WideFloat::from_i64(1, precision),
            WideFloat::zero(precision),
        ],
        radius: WideFloat::from_i64(1000, precision),
        ratio: WideFloat::from_f64(0.5, precision).unwrap(),
        periapsis: WideFloat::zero(precision),
        period: WideFloat::from_i64(period, precision),
        phase: WideFloat::from_f64(phase, precision).unwrap(),
    }
}

/// `numerator / denominator` as a number of 512 bits: the exact fraction of a turn, to far more than the widths asked.
fn fraction(numerator: i128, denominator: i128) -> WideFloat {
    whole(numerator, at(512))
        .divide(&whole(denominator, at(512)))
        .unwrap()
}

/// Whether `found` is `expected` to within 2^-bits (of a turn).
fn within(found: &WideFloat, expected: &WideFloat, bits: i64) -> bool {
    let difference = (&found.rounded_to(at(512)) - expected).abs();
    difference.is_zero() || difference.floor_log2().unwrap() < -bits
}

#[test]
fn the_part_of_a_turn_gone_is_the_time_over_the_period_modulo_one_to_the_width_asked() {
    // A period of 600 seconds, 20 ticks a second: 12,000 ticks a turn.
    let orbit = elements(600, 0.0, at(128));
    for tick in [
        0u64,
        1,
        5_999,
        6_000,
        11_999,
        12_000,
        12_001,
        987_654_321,
        u64::MAX,
    ] {
        for bits in [128u64, 192, 512] {
            let time = Time {
                units: i128::from(tick),
                units_per_second: 20,
            };
            let found = orbit.mean_anomaly_in_turns(time, at(bits)).unwrap();
            assert_eq!(found.precision(), at(bits));
            let expected = fraction(i128::from(tick) % 12_000, 12_000);
            assert!(
                within(&found, &expected, bits as i64 - 1),
                "tick {tick} at {bits} bits: {found:?} against {expected:?}"
            );
        }
    }
}

#[test]
fn a_whole_number_of_periods_is_exactly_nothing_however_late() {
    let orbit = elements(600, 0.0, at(128));
    for periods in [0i128, 1, 7, 1 << 40, 1 << 60] {
        let time = Time {
            units: periods * 12_000,
            units_per_second: 20,
        };
        let found = orbit.mean_anomaly_in_turns(time, at(128)).unwrap();
        assert!(found.is_zero(), "{periods} periods: {found:?}");
    }
}

#[test]
fn a_negative_period_goes_the_other_way_round() {
    let forward = elements(600, 0.0, at(128));
    let backward = elements(-600, 0.0, at(128));
    for tick in [1u64, 5_999, 7_000, 987_654_321] {
        let time = Time {
            units: i128::from(tick),
            units_per_second: 20,
        };
        let there = forward.mean_anomaly_in_turns(time, at(256)).unwrap();
        let back = backward.mean_anomaly_in_turns(time, at(256)).unwrap();
        // Together they are a whole turn.
        let sum = &there.rounded_to(at(512)) + &back.rounded_to(at(512));
        assert!(
            within(&sum, &whole(1, at(512)), 250),
            "tick {tick}: {there:?} and {back:?}"
        );
    }
}

#[test]
fn the_phase_is_added_in_turns_and_a_turn_is_two_pi_radians() {
    // A phase of one radian is 1 / (2 pi) of a turn on, at any tick.
    let orbit = elements(600, 1.0, at(128));
    let one_turn = WideFloat::pi(at(512)).times_power_of_two(1).unwrap();
    let phase_turns = whole(1, at(512)).divide(&one_turn).unwrap();
    for tick in [0u64, 1, 4_321, 987_654_321] {
        let time = Time {
            units: i128::from(tick),
            units_per_second: 20,
        };
        let found = orbit.mean_anomaly_in_turns(time, at(256)).unwrap();
        let total = &fraction(i128::from(tick) % 12_000, 12_000) + &phase_turns;
        let expected = &total - &whole(total.floor_to_i128().unwrap(), at(512));
        assert!(
            within(&found, &expected, 250),
            "tick {tick}: {found:?} against {expected:?}"
        );
    }
}

#[test]
fn a_clock_with_all_its_bits_and_a_short_period_still_gives_the_fraction_to_the_last_bit() {
    // 2^95 + 12345 units of a clock of 2^20 units a second, and a period of 3 seconds: the turns gone have 74 whole bits, and the fraction after them is
    // `(units mod 3 x 2^20) / (3 x 2^20)`, which is not a number of 64 bits unless the working leaves the room for it.
    let units: i128 = (1 << 95) + 12_345;
    let modulus: i128 = 3 << 20;
    let orbit = elements(3, 0.0, at(128));
    for bits in [128u64, 256] {
        let found = orbit
            .mean_anomaly_in_turns(
                Time {
                    units,
                    units_per_second: 1 << 20,
                },
                at(bits),
            )
            .unwrap();
        let expected = fraction(units % modulus, modulus);
        assert!(
            within(&found, &expected, bits as i64 - 1),
            "{bits} bits: {found:?} against {expected:?}"
        );
    }
}

#[test]
fn the_shortest_period_and_a_clock_with_all_its_bits_still_give_the_fraction_to_the_last_bit() {
    // The worst case for the whole part: 2^96 - 2 units (2^96 - 1 is a multiple of 3, which would leave no fraction) of a clock of one unit a second, and a period of 3 x 2^-20 second, which is 116 whole bits of turns and then a third
    // or two thirds (`units x 2^20 mod 3`, over 3). A working that does not leave the room for the whole part loses the bits of the fraction it needs.
    let units: i128 = (1 << 96) - 2;
    let mut orbit = elements(3, 0.0, at(128));
    orbit.period = orbit.period.times_power_of_two(-20).unwrap();
    for bits in [128u64, 256] {
        let found = orbit
            .mean_anomaly_in_turns(
                Time {
                    units,
                    units_per_second: 1,
                },
                at(bits),
            )
            .unwrap();
        let expected = fraction((units << 20) % 3, 3);
        assert!(
            within(&found, &expected, bits as i64 - 1),
            "{bits} bits: {found:?} against {expected:?}"
        );
    }
}

#[test]
fn a_time_the_clock_cannot_hold_is_refused() {
    let orbit = elements(600, 0.0, at(128));
    for units in [1i128 << 96, -(1i128 << 96), i128::MAX] {
        assert!(matches!(
            orbit.mean_anomaly_in_turns(
                Time {
                    units,
                    units_per_second: 20
                },
                at(128)
            ),
            Err(KeplerError::Invalid(_))
        ));
    }
    assert!(matches!(
        orbit.mean_anomaly_in_turns(
            Time {
                units: 5,
                units_per_second: 0
            },
            at(128)
        ),
        Err(KeplerError::Invalid(_))
    ));
}

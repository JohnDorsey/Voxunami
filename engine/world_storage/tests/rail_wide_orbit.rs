//! A rail whose orbit has wide elements (E9 of `docs/orbits-and-thrust-plan.md`): the elements are numbers of any width from 128 bits, not doubles, so a path that was planned
//! to the millimetre is kept to the last digit by the rail, by its bytes and by the evaluator (the owner's rule of 2026-11-02: lazy exact rails, nothing rounded until a place is asked
//! for).

use voxel_space::GridId;
use wide_float::{Precision, WideFloat};
use wide_kepler::{Elements, Time};
use world_storage::rail::{
    Centre, ExactOffset, Orbit, Rail, RailError, RailTime, Spin, decode_rail, encode_rail,
};

const NOWHERE: [f64; 3] = [0.0; 3];
const TICKS_A_SECOND: u32 = 20;

/// Half a step of the rest, and a hair over for the width the place is worked out in, in voxels.
const HALF_A_STEP: f64 = 1.000_001 / 8_589_934_592.0;

fn at(bits: u64) -> Precision {
    Precision::new(bits).unwrap()
}

/// `digits / 10^decimals` in `bits` bits: a number with digits past what a double holds, which `WideFloat::parse` (hexadecimal) does not write in decimal.
fn decimal(digits: i128, decimals: u32, bits: u64) -> WideFloat {
    let precision = at(bits);
    WideFloat::from_i128(digits, precision)
        .divide(&WideFloat::from_i128(10i128.pow(decimals), precision))
        .unwrap()
}

/// The elements of an ellipse that no double holds: every one has digits past the 53 bits of a double.
fn wide_elements(bits: u64) -> Elements {
    Elements {
        normal: [
            decimal(333_333_333_333_333_333_333_333_333_333_333, 33, bits),
            decimal(666_666_666_666_666_666_666_666_666_666_666, 33, bits),
            decimal(666_666_666_666_666_666_666_666_666_666_667, 33, bits),
        ],
        radius: decimal(123_456_789_012_345_678_901_234_567_890_123_456, 24, bits),
        ratio: decimal(123_456_789_012_345_678_901_234_567_890_123, 34, bits),
        periapsis: decimal(1_234_567_890_123_456_789_012_345_678_901_234, 33, bits),
        period: decimal(31_557_600_000_000_000_000_000_001_234_567, 24, bits),
        phase: decimal(2_718_281_828_459_045_235_360_287_471_352_662_497, 36, bits),
    }
}

fn rail_of(elements: Elements) -> Rail {
    Rail {
        spin: None,
        orbit: Some(Orbit::from_elements(Centre::Grid(GridId::new(3)), elements).unwrap()),
    }
}

fn offset_at(rail: &Rail, tick: u64) -> ExactOffset {
    rail.state(RailTime::of_tick(tick, TICKS_A_SECOND), NOWHERE, NOWHERE)
        .offset
        .expect("an orbit has an offset")
}

fn voxels_of(offset: &ExactOffset, precision: Precision) -> [WideFloat; 3] {
    [0, 1, 2].map(|axis| {
        &(&WideFloat::from_i128(offset.bricks[axis], precision)
            * &WideFloat::from_i64(32, precision))
            + &WideFloat::from_f64(offset.rest[axis], precision).unwrap()
    })
}

#[test]
fn an_orbit_made_of_doubles_has_those_doubles_as_its_exact_elements() {
    let orbit = Orbit::elliptical(
        Centre::Point(NOWHERE),
        [0.0, 0.6, 0.8],
        6.8e6,
        0.9,
        0.3,
        5581.0,
        -1.25,
    )
    .unwrap();
    let exactly = |value: f64| WideFloat::from_f64(value, Precision::MINIMUM).unwrap();
    assert_eq!(orbit.elements.radius, exactly(6.8e6));
    assert_eq!(orbit.elements.ratio, exactly(0.9));
    assert_eq!(orbit.elements.periapsis, exactly(0.3));
    assert_eq!(orbit.elements.period, exactly(5581.0));
    assert_eq!(orbit.elements.phase, exactly(-1.25));
    assert_eq!(
        orbit.elements.normal,
        [exactly(0.0), exactly(0.6), exactly(0.8)]
    );
    // And the views the game's arithmetic wants are those doubles back.
    assert_eq!(
        (
            orbit.radius(),
            orbit.ratio(),
            orbit.periapsis(),
            orbit.period(),
            orbit.phase()
        ),
        (6.8e6, 0.9, 0.3, 5581.0, -1.25)
    );
    assert_eq!(orbit.normal(), [0.0, 0.6, 0.8]);
}

#[test]
fn a_rail_with_wide_elements_comes_back_from_its_bytes_equal_to_the_bit() {
    for bits in [128u64, 192, 256, 1024] {
        let rail = Rail {
            spin: Some(Spin::new([0.0, 1.0, 0.0], 86_400.0, 0.25).unwrap()),
            ..rail_of(wide_elements(bits))
        };
        let bytes = encode_rail(&rail);
        let read = decode_rail(&bytes).unwrap();
        assert_eq!(read, rail, "{bits} bits");
        // Equal includes the width, and the same rail is the same bytes.
        assert_eq!(
            read.orbit.as_ref().unwrap().elements.radius.precision(),
            at(bits)
        );
        assert_eq!(encode_rail(&read), bytes, "{bits} bits");
    }
}

#[test]
fn an_orbit_about_a_point_keeps_the_point_and_a_wide_orbit_about_a_ship_keeps_the_ship() {
    for around in [
        Centre::Point([1.0e6, -2.5, 3.0e9]),
        Centre::Grid(GridId::new(70_000)),
    ] {
        let rail = Rail {
            spin: None,
            orbit: Some(Orbit::from_elements(around, wide_elements(192)).unwrap()),
        };
        assert_eq!(decode_rail(&encode_rail(&rail)).unwrap(), rail);
    }
}

#[test]
fn the_digits_past_a_double_move_the_body() {
    // A circle of 1e30 voxels (a double) and one 2^-60 of itself larger: the nearest double to both is the same one, so a rail that kept doubles could not tell them apart.
    let precision = at(256);
    let circle = |radius: &WideFloat| {
        rail_of(Elements {
            normal: [
                WideFloat::zero(precision),
                WideFloat::from_i64(1, precision),
                WideFloat::zero(precision),
            ],
            radius: radius.clone(),
            ratio: WideFloat::from_i64(1, precision),
            periapsis: WideFloat::zero(precision),
            period: WideFloat::from_i64(600, precision),
            phase: WideFloat::zero(precision),
        })
    };
    let base = WideFloat::from_f64(1.0e30, precision).unwrap();
    let more = &base + &base.times_power_of_two(-60).unwrap();
    assert_eq!(
        more.to_f64(),
        base.to_f64(),
        "a double cannot tell them apart"
    );
    let (near, far) = (offset_at(&circle(&base), 0), offset_at(&circle(&more), 0));
    assert_ne!(near, far);
    // At phase 0 the body is at the radius along +x: the two are 2^-60 of 1e30 voxels apart, which is 8.7e11 voxels, to a part in a million or so.
    let (a, b) = (voxels_of(&near, precision), voxels_of(&far, precision));
    let apart = (&b[0] - &a[0]).to_f64();
    assert!(
        (apart / (1.0e30 / (1u64 << 60) as f64) - 1.0).abs() < 1.0e-6,
        "{apart} voxels apart"
    );
}

#[test]
fn a_rail_places_a_wide_orbit_where_the_wide_evaluator_does_to_the_step() {
    // Orbits whose elements a double cannot hold: a radius of 1.2e11 voxels would be 1.3e-5 voxel out if it were rounded to a double, a hundred thousand steps.
    let precision = at(512);
    let elements = wide_elements(256);
    let rail = rail_of(elements.clone());
    for tick in [0u64, 1, 12_345, 98_765_432, 1_000_000_000, u64::MAX] {
        let state = rail.state(RailTime::of_tick(tick, TICKS_A_SECOND), NOWHERE, NOWHERE);
        let wide_place = elements
            .place_at(
                Time {
                    units: i128::from(tick),
                    units_per_second: TICKS_A_SECOND,
                },
                precision,
            )
            .unwrap();
        let offset = voxels_of(&state.offset.unwrap(), precision);
        for (axis, (found, expected)) in offset.iter().zip(&wide_place.position).enumerate() {
            let apart = (found - expected).abs().to_f64();
            assert!(
                apart <= HALF_A_STEP,
                "tick {tick}, axis {axis}: {apart} voxels apart"
            );
            let speed = wide_place.velocity[axis].to_f64();
            assert_eq!(state.velocity[axis], speed, "tick {tick}, axis {axis}");
        }
    }
}

#[test]
fn a_normal_need_not_be_a_unit_vector() {
    // Twice a normal is the same plane: the places are the same to the bit (a power of two commutes with the correctly rounded working). The scale must not take the normal's
    // x over 0.9: that number as given decides which axis the circle starts from, and not the unit vector's (`wide_kepler::Elements`).
    let unit = wide_elements(256);
    let mut doubled = unit.clone();
    doubled.normal = unit
        .normal
        .each_ref()
        .map(|part| part.times_power_of_two(1).unwrap());
    for tick in [0u64, 777, 123_456_789] {
        assert_eq!(
            offset_at(&rail_of(unit.clone()), tick),
            offset_at(&rail_of(doubled.clone()), tick),
            "tick {tick}"
        );
    }
}

/// The elements of a valid orbit, then each limit exactly (kept) and a hair past it (refused).
#[test]
fn the_limits_of_an_orbit_are_kept_exactly_and_refused_a_hair_beyond() {
    let bits = 256;
    let power = |exponent: i64| {
        WideFloat::from_i64(1, at(bits))
            .times_power_of_two(exponent)
            .unwrap()
    };
    let hair = |value: &WideFloat, more: bool| {
        let step = value.times_power_of_two(-100).unwrap();
        if more { value + &step } else { value - &step }
    };
    type Change = Box<dyn Fn(&mut Elements, WideFloat)>;
    let radius: Change = Box::new(|elements, value| elements.radius = value);
    let period: Change = Box::new(|elements, value| elements.period = value);
    let ratio: Change = Box::new(|elements, value| elements.ratio = value);
    let phase: Change = Box::new(|elements, value| elements.phase = value);
    let periapsis: Change = Box::new(|elements, value| elements.periapsis = value);
    // (what, change, the last value kept, the first value refused)
    let limits: Vec<(&str, &Change, WideFloat, WideFloat)> = vec![
        (
            "the largest radius",
            &radius,
            power(120),
            hair(&power(120), true),
        ),
        (
            "the longest period",
            &period,
            power(80),
            hair(&power(80), true),
        ),
        (
            "the longest period backwards",
            &period,
            -&power(80),
            -&hair(&power(80), true),
        ),
        (
            "the shortest period",
            &period,
            power(-20),
            hair(&power(-20), false),
        ),
        (
            "the shortest period backwards",
            &period,
            -&power(-20),
            -&hair(&power(-20), false),
        ),
        ("the largest ratio", &ratio, power(0), hair(&power(0), true)),
        (
            "the smallest ratio",
            &ratio,
            power(-64),
            hair(&power(-64), false),
        ),
        (
            "the largest phase",
            &phase,
            power(63),
            hair(&power(63), true),
        ),
        (
            "the smallest phase",
            &phase,
            -&power(63),
            -&hair(&power(63), true),
        ),
        (
            "the largest periapsis",
            &periapsis,
            power(63),
            hair(&power(63), true),
        ),
    ];
    for (what, change, kept, refused) in limits {
        let mut elements = wide_elements(bits);
        change(&mut elements, kept);
        assert!(
            Orbit::from_elements(Centre::Point(NOWHERE), elements.clone()).is_ok(),
            "{what} is kept"
        );
        // It also comes back from bytes.
        let rail = Rail {
            spin: None,
            orbit: Some(Orbit {
                around: Centre::Point(NOWHERE),
                elements,
            }),
        };
        assert!(decode_rail(&encode_rail(&rail)).is_ok(), "{what} is read");
        let mut elements = wide_elements(bits);
        change(&mut elements, refused);
        assert!(
            matches!(
                Orbit::from_elements(Centre::Point(NOWHERE), elements.clone()),
                Err(RailError::Invalid(_))
            ),
            "{what}: a hair beyond is refused"
        );
        let rail = Rail {
            spin: None,
            orbit: Some(Orbit {
                around: Centre::Point(NOWHERE),
                elements,
            }),
        };
        assert!(
            decode_rail(&encode_rail(&rail)).is_err(),
            "{what}: a hair beyond is not read"
        );
    }
}

#[test]
fn what_is_no_orbit_is_refused_when_made_and_when_read() {
    let zero = WideFloat::zero(at(256));
    let bad = |change: &dyn Fn(&mut Elements)| {
        let mut elements = wide_elements(256);
        change(&mut elements);
        elements
    };
    let cases = [
        ("no radius", bad(&|e| e.radius = zero.clone())),
        ("a negative radius", bad(&|e| e.radius = -&e.radius.clone())),
        ("no period", bad(&|e| e.period = zero.clone())),
        ("no ratio", bad(&|e| e.ratio = zero.clone())),
        (
            "a ratio over one",
            bad(&|e| e.ratio = WideFloat::from_i64(2, at(256))),
        ),
        (
            "no normal",
            bad(&|e| e.normal = [zero.clone(), zero.clone(), zero.clone()]),
        ),
    ];
    for (what, elements) in cases {
        assert!(
            Orbit::from_elements(Centre::Point(NOWHERE), elements.clone()).is_err(),
            "{what} is no orbit"
        );
        let rail = Rail {
            spin: None,
            orbit: Some(Orbit {
                around: Centre::Point(NOWHERE),
                elements,
            }),
        };
        assert!(
            decode_rail(&encode_rail(&rail)).is_err(),
            "{what} is not read"
        );
    }
    // A centre that is not made of numbers.
    assert!(Orbit::from_elements(Centre::Point([f64::NAN, 0.0, 0.0]), wide_elements(128)).is_err());
}

#[test]
fn bytes_that_are_not_a_wide_rail_are_refused_and_never_a_panic() {
    let rail = Rail {
        spin: Some(Spin::new([0.0, 1.0, 0.0], 3600.0, 0.5).unwrap()),
        ..rail_of(wide_elements(192))
    };
    let bytes = encode_rail(&rail);
    // Every prefix that is not the whole is refused, and so is the whole with a byte after it.
    for length in 0..bytes.len() {
        assert!(
            decode_rail(&bytes[..length]).is_err(),
            "the first {length} bytes"
        );
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(decode_rail(&longer).is_err());
    // Every single byte changed to another: never a panic (it may be another rail, or refused).
    for index in 0..bytes.len() {
        for value in [0u8, 1, 0x7f, 0x80, 0xff] {
            let mut changed = bytes.clone();
            changed[index] = value;
            let _ = decode_rail(&changed);
        }
    }
}

#[test]
fn the_apsides_are_passed_as_often_as_the_mean_anomaly_says_for_an_orbit_no_double_holds() {
    // Walk a wide ellipse a tick at a time for several periods: the periapsis is passed each time the mean anomaly goes through a whole number of turns, and the apoapsis through
    // a half. The number of times is worked out from the phase in 512 bits.
    let elements = wide_elements(256);
    let orbit = Orbit::from_elements(Centre::Point(NOWHERE), elements.clone()).unwrap();
    // A period of 31,557,600 seconds is 631 million ticks: step by a thousandth of a period, so that a few thousand stretches cover three and a half periods.
    let period_ticks = 631_152_000u64;
    let stretch = period_ticks / 1000;
    let mut periapsis = 0u32;
    let mut apoapsis = 0u32;
    for index in 0..3500u64 {
        let (from, to) = (index * stretch, (index + 1) * stretch);
        let passed = orbit.apsides_passed(
            RailTime::of_tick(from, TICKS_A_SECOND),
            RailTime::of_tick(to, TICKS_A_SECOND),
        );
        periapsis += u32::from(passed.periapsis);
        apoapsis += u32::from(passed.apoapsis);
    }
    let high = at(512);
    let turns_in_the_phase = elements
        .phase
        .rounded_to(high)
        .divide(&WideFloat::pi(high).times_power_of_two(1).unwrap())
        .unwrap();
    // After 3,500 stretches the orbit has gone 3.5 periods (the period is a whole number of ticks).
    let end = &turns_in_the_phase + &WideFloat::from_f64(3.5, high).unwrap();
    let crossings = |shift: f64| {
        let shifted_end = &end + &WideFloat::from_f64(shift, high).unwrap();
        let shifted_start = &turns_in_the_phase + &WideFloat::from_f64(shift, high).unwrap();
        shifted_end.floor_to_i128().unwrap() - shifted_start.floor_to_i128().unwrap()
    };
    assert_eq!(i128::from(periapsis), crossings(0.0));
    assert_eq!(i128::from(apoapsis), crossings(0.5));
    assert!(periapsis >= 3 && apoapsis >= 3);
}

/// The bytes of a whole rail whose radius is a third of a voxel at 128 bits: no double holds it, and the saved form of it has the 16 bytes of a 128-bit odd mantissa. **Written by
/// a second program** (`golden_v3.py` in the scratchpad of the session, from the description of the saved form), and the Rust gives the same.
#[test]
fn a_radius_no_double_holds_is_written_as_pinned() {
    let precision = at(128);
    let third = WideFloat::from_i64(1, precision)
        .divide(&WideFloat::from_i64(3, precision))
        .unwrap();
    let rail = Rail {
        spin: None,
        orbit: Some(
            Orbit::from_elements(
                Centre::Point(NOWHERE),
                Elements {
                    normal: [
                        WideFloat::zero(precision),
                        WideFloat::from_i64(1, precision),
                        WideFloat::zero(precision),
                    ],
                    radius: third,
                    ratio: WideFloat::from_i64(1, precision),
                    periapsis: WideFloat::zero(precision),
                    period: WideFloat::from_i64(600, precision),
                    phase: WideFloat::zero(precision),
                },
            )
            .unwrap(),
        ),
    };
    let hex = "0200000000000000000000000000000000000000000000000000800100800101000101800100800101810210abaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa80010106014b800100800101000101800100";
    let expected: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).unwrap())
        .collect();
    assert_eq!(encode_rail(&rail), expected);
    assert_eq!(decode_rail(&expected).unwrap(), rail);
}

//! Elliptical orbits (`docs/orbits-and-thrust-plan.md`, section A, step A3): an orbit with a `ratio` (1 - e, the periapsis over the semi-major axis) and a `periapsis` (the
//! angle of the nearest point in the plane), worked out in whole numbers (`exact_kepler`). With a `ratio` of 1.0 it is the circle of `rail_circle_bits.rs`, to the bit.

use std::f64::consts::{PI, TAU};

use proptest::prelude::*;
use world_storage::rail::{
    Centre, ExactOffset, Orbit, Rail, RailError, RailTime, SMALLEST_RATIO, decode_rail, encode_rail,
};

const NOWHERE: [f64; 3] = [0.0; 3];
const TICKS_A_SECOND: u32 = 20;
const LARGEST_PHASE: f64 = (1u64 << 63) as f64;

fn ellipse(
    normal: [f64; 3],
    radius: f64,
    ratio: f64,
    periapsis: f64,
    period: f64,
    phase: f64,
) -> Rail {
    Rail {
        spin: None,
        orbit: Some(
            Orbit::elliptical(
                Centre::Point(NOWHERE),
                normal,
                radius,
                ratio,
                periapsis,
                period,
                phase,
            )
            .unwrap(),
        ),
    }
}

fn state_at(rail: &Rail, tick: u64) -> world_storage::rail::RailState {
    rail.state(RailTime::of_tick(tick, TICKS_A_SECOND), NOWHERE, NOWHERE)
}

fn offset_at(rail: &Rail, tick: u64) -> ExactOffset {
    state_at(rail, tick).offset.expect("an orbit has an offset")
}

/// Orbits worked out at 120 digits by `engine/world_storage/tools/exact_ellipse_reference.py orbits`, from the numbers as the program keeps them:
/// `(normal, radius, ratio, periapsis, period, phase, tick, ticks a second, bricks, rest, velocity, amplification)` for the offset from the centre and the velocity at that tick.
type PinnedEllipse = (
    [f64; 3],
    f64,
    f64,
    f64,
    f64,
    f64,
    u64,
    u32,
    [i128; 3],
    [f64; 3],
    [f64; 3],
    f64,
);

static ELLIPSES: [PinnedEllipse; 15] = [
    (
        [0.0, 1.0, 0.0],
        6e+26,
        1.0,
        0.0,
        31560000.0,
        0.7,
        12345,
        20,
        [14339306547934188300487885, 0, -12080843833291269939764885],
        [11.966872680635376, 0.0, 9.108164604075284],
        [-7.696444154288144e+19, 0.0, -9.135261872458721e+19],
        1.0,
    ),
    (
        [0.3333333333333333, 0.6666666666666666, 0.6666666666666666],
        2.5e+20,
        1.0,
        0.0,
        7000000000000000.0,
        2.0,
        7600000000037,
        20,
        [
            -3067495228514122961,
            5789295454149192556,
            -4255547839892131076,
        ],
        [1.7770357827473051, 9.13664181291128, 5.974840295715066],
        [-192346.24547974658, -17994.4933145551, 114167.61605442839],
        1.0,
    ),
    (
        [0.6, 0.8, 0.0],
        1e+18,
        1.0,
        0.0,
        5000000000.0,
        -1.25,
        333,
        20,
        [7883059556272012, -5912294667204010, 29655769148690437],
        [23.8304434911537, 31.63107582936384, 3.986664094268062],
        [954023388.0993866, -715517541.0745399, -396245791.8367195],
        1.0,
    ),
    (
        [0.0, 1.0, 0.0],
        150000000000.0,
        1.0,
        0.0,
        31560000.0,
        0.0,
        0,
        20,
        [4687500000, 0, 0],
        [0.0, 0.0, 0.0],
        [0.0, 0.0, -29863.04803792579],
        1.0,
    ),
    (
        [0.2672612419124244, -0.5345224838248488, 0.8017837257372732],
        8.8e+26,
        1.0,
        0.0,
        -6300000000000.0,
        3.0,
        99999999999,
        20,
        [
            -26215493239689928224895128,
            -691212763326508729217752,
            8277689237678970255486541,
        ],
        [11.622089691483545, 10.716784407513616, 13.937159707847895],
        [123522488089954.06, 741422611319178.8, 453107578182801.2],
        1.0,
    ),
    (
        [0.0, 0.0, 1.0],
        384000000.0,
        1.0,
        0.0,
        2360000.0,
        5.5,
        18446744073709551615,
        20,
        [7324252, 9505541, 0],
        [22.22546345251247, 21.484324766636632, 0.0],
        [-809.8315900532943, 623.9950768330881, 0.0],
        1.0,
    ),
    (
        [0.0, 1.0, 0.0],
        150000000000.0,
        0.0167,
        1.8,
        31560000.0,
        0.3,
        4000000,
        20,
        [-52030198, 0, 3376831886],
        [16.434068459813293, 0.0, 29.285737761924498],
        [6940.498538486595, 0.0, 39186.94819496187],
        1.3879706882710974,
    ),
    (
        [0.0, 1.0, 0.0],
        150000000000.0,
        0.0167,
        1.8,
        31560000.0,
        0.3,
        0,
        20,
        [-95254994, 0, 3124385976],
        [29.669873855023383, 0.0, 25.400645106798112],
        [6883.765983909395, 0.0, 41659.333475929634],
        1.4995980179782455,
    ),
    (
        [0.0, 0.0, 1.0],
        384000000.0,
        0.945,
        0.0,
        2360000.0,
        5.5,
        18446744073709551615,
        20,
        [6228897, 9810761, 0],
        [15.972854320859772, 27.718200502015595, 0.0],
        [-864.394617994673, 605.1225131379448, 0.0],
        1.0326035414574912,
    ),
    (
        [0.6, 0.8, 0.0],
        1000000000000.0,
        0.1,
        -2.5,
        -500000000.0,
        -1.25,
        333333333,
        20,
        [24370128600, -18277596451, -36587014628],
        [29.56329611614897, 9.827582025444045, 21.04574190152526],
        [5301.5852878164815, -3976.188965862361, -2340.1735140456162],
        0.6563933546776283,
    ),
    (
        [0.2672612419124244, -0.5345224838248488, 0.8017837257372732],
        8.8e+26,
        1e-06,
        3.0,
        6300000000000.0,
        1.0,
        99999999999,
        20,
        [
            35653637289455033265778755,
            1066308445560441843037350,
            -11173673466111383193234685,
        ],
        [28.69543496719756, 20.490022576965536, 4.094870062244504],
        [574912067244082.1, 17961717721496.582, -179662877267029.66],
        0.7357124274080199,
    ),
    (
        [0.0, 1.0, 0.0],
        1e+22,
        1e-12,
        0.5,
        1e+23,
        0.0,
        1,
        20,
        [274243884, 0, -149821700],
        [30.544964241449932, 0.0, 24.01571838915012],
        [-426008.04139273404, 0.0, -779798.3718574849],
        999999999995.0652,
    ),
    (
        [0.0, 1.0, 0.0],
        1e+22,
        1e-12,
        0.5,
        1e+23,
        0.0,
        987654321012345,
        20,
        [-963750624701026, 0, 525164681772098],
        [4.191198575183913, 0.0, 17.52962716361981],
        [-416.2191843773693, 0.0, 227.09328542589884],
        284725.3995416648,
    ),
    (
        [0.95, 0.1, 0.2],
        7000000.0,
        0.3,
        2.0,
        5400.0,
        6.2,
        123456789,
        20,
        [2910, 158765, -93210],
        [30.105276319938884, 23.10106307791791, 5.449405941381817],
        [-1475.1677706496444, 9116.712756659956, 2448.6905322558323],
        1.1880325888775267,
    ),
    (
        [-0.3, 0.5, -0.8],
        22000000000000.0,
        0.02,
        -3.0,
        -440000000.0,
        -2.5,
        55555555555,
        20,
        [1144840428120, 122949061318, -352471997222],
        [20.01971697047745, 21.437315858075532, 29.892904538916646],
        [-104928.73279123522, 19070.340131751378, 51267.237379057806],
        0.5709351960695117,
    ),
];

/// Units of 2^-126 that everything between the sine and cosine of the anomaly and the offset adds: the sine and cosine (22 at the worst on the circle), the rotation by the
/// periapsis, the plane (16 for each part), and the products. A hundred and sixty is what the circle's tests would allow of the whole circle's chain, and a little more.
const ARITHMETIC_UNITS: f64 = 160.0;

/// Units of 2^-128 turn that the anomaly may be out by for each unit of the amplification 1 / (1 - e cos E): `exact_kepler` allows 24 and measured 8.
const ANOMALY_UNITS: f64 = 24.0;

/// How far, in voxels, an offset may be from the one worked out at 120 digits: a step of 2^-32 voxel (the program rounds to the nearest step, so half a step; over 400 random
/// orbits, from a radius of 1 to 10^26 and a ratio of 1e-18 to 1, the worst error was exactly half a step, and at the large radii and the sharp ellipses it was the same), the arithmetic
/// of the chain times the radius, and the anomaly's error, in radians times the radius (the place moves by at most that), magnified by the amplification at that tick. The last two
/// are bounds worked out beforehand, far above what is found; the first is what is found, twice.
fn position_tolerance(radius: f64, amplification: f64) -> f64 {
    let angle = TAU * ANOMALY_UNITS * amplification.max(1.0) * libm::scalbn(1.0, -128);
    radius * (angle + ARITHMETIC_UNITS * libm::scalbn(1.0, -126)) + libm::scalbn(1.0, -32)
}

fn offset_error(found: ExactOffset, bricks: [i128; 3], rest: [f64; 3]) -> f64 {
    (0..3)
        .map(|axis| {
            ((found.bricks[axis] - bricks[axis]) as f64 * 32.0 + (found.rest[axis] - rest[axis]))
                .abs()
        })
        .fold(0.0, f64::max)
}

#[test]
fn an_ellipse_is_where_it_was_worked_out_at_120_digits_to_what_the_amplification_allows() {
    for (
        normal,
        radius,
        ratio,
        periapsis,
        period,
        phase,
        tick,
        per_second,
        bricks,
        rest,
        velocity,
        amplification,
    ) in ELLIPSES.iter().copied()
    {
        let rail = ellipse(normal, radius, ratio, periapsis, period, phase);
        let state = rail.state(RailTime::of_tick(tick, per_second), NOWHERE, NOWHERE);
        let found = state.offset.expect("an orbit has an offset");
        let error = offset_error(found, bricks, rest);
        let allowed = position_tolerance(radius, amplification);
        let context = format!(
            "ratio {ratio}, radius {radius}, tick {tick}: found {found:?}, expected {bricks:?} {rest:?}"
        );
        assert!(
            error <= allowed,
            "{context} ({error} voxels out, {allowed} allowed)"
        );
        // The velocity is a speed as plain numbers: right to the sine's and cosine's own accuracy, in parts of the speed.
        let speed = velocity.iter().map(|part| part * part).sum::<f64>().sqrt();
        for (axis, expected) in velocity.into_iter().enumerate() {
            assert!(
                (state.velocity[axis] - expected).abs() <= 1.0e-9 * speed,
                "{context}: velocity {:?} against {velocity:?}",
                state.velocity
            );
        }
    }
}

#[test]
fn the_periapsis_and_apoapsis_are_where_the_ratio_says_in_the_direction_of_the_periapsis_angle() {
    // A normal along +y: the plane starts toward +x and goes toward -z. A radius of 640 and a ratio of a half: periapsis at 320 voxels (ten bricks), apoapsis at 960.
    let at = |periapsis: f64, phase: f64| {
        let rail = ellipse([0.0, 1.0, 0.0], 640.0, 0.5, periapsis, 1000.0, phase);
        offset_at(&rail, 0)
    };
    let nearest = at(0.0, 0.0);
    assert_eq!((nearest.bricks, nearest.rest), ([10, 0, 0], [0.0; 3]));
    // A quarter turn on the periapsis, and the nearest point is toward -z.
    let quarter = at(PI / 2.0, 0.0);
    assert_eq!((quarter.bricks, quarter.rest), ([0, 0, -10], [0.0; 3]));
    // Half a turn of the mean anomaly later it is as far as it goes, the other way: 960 voxels, thirty bricks.
    let farthest = at(0.0, PI);
    assert_eq!((farthest.bricks, farthest.rest), ([-30, 0, 0], [0.0; 3]));
    let farthest_turned = at(PI / 2.0, PI);
    assert_eq!(
        (farthest_turned.bricks, farthest_turned.rest),
        ([0, 0, 30], [0.0; 3])
    );
}

#[test]
fn a_ratio_a_hair_under_one_is_the_circle_to_the_hair() {
    for (radius, periapsis) in [(1.0e3, 0.0), (7.5e4, 1.25), (1.0e3, -3.0)] {
        let ratio = 1.0 - libm::scalbn(1.0, -53);
        let nearly = ellipse([0.6, 0.8, 0.0], radius, ratio, periapsis, 5.0e3, 0.3);
        let circle = ellipse([0.6, 0.8, 0.0], radius, 1.0, periapsis, 5.0e3, 0.3);
        for tick in [0, 1, 777, 123_456_789] {
            let error = offset_error(
                offset_at(&nearly, tick),
                offset_at(&circle, tick).bricks,
                offset_at(&circle, tick).rest,
            );
            // e is 1.1e-16, so the place is out by that times the radius, and by the grid.
            assert!(
                error <= radius * 2.3e-16 + 1.0e-9,
                "radius {radius}, tick {tick}: {error}"
            );
        }
    }
}

#[test]
fn an_ellipse_is_exactly_where_it_began_after_any_whole_number_of_periods() {
    // A period of 1000 seconds is 20,000 ticks.
    for ratio in [0.9, 0.3, 1.0e-3, 1.0e-9] {
        let rail = ellipse([0.2, -0.5, 0.8], 3.0e12, ratio, 2.0, 1000.0, 0.4);
        for tick in [0, 7, 19_999, 1_000_003] {
            let began = offset_at(&rail, tick);
            for periods in [1u64, 12_345, 1_000_000_000_000] {
                let later = offset_at(&rail, tick + periods * 20_000);
                assert_eq!(
                    later, began,
                    "ratio {ratio}, tick {tick}, {periods} periods"
                );
            }
        }
    }
}

#[test]
fn an_ellipse_the_other_way_is_the_same_one_run_backwards() {
    // With a negative period the mean anomaly falls as the time goes on: at a time it is where the same ellipse with the period the right way is at the time before the start.
    for ratio in [0.9, 0.2, 1.0e-4] {
        let forward = ellipse([0.1, 0.9, -0.3], 8.0e9, ratio, -1.0, 4.0e5, 2.5);
        let backward = ellipse([0.1, 0.9, -0.3], 8.0e9, ratio, -1.0, -4.0e5, 2.5);
        for seconds in [0.0, 1.5, 12_345.678, 4.0e5, 3.1e9] {
            let there = backward.state(seconds, NOWHERE, NOWHERE).offset.unwrap();
            let here = forward.state(-seconds, NOWHERE, NOWHERE).offset.unwrap();
            assert_eq!(there, here, "ratio {ratio}, {seconds} seconds");
        }
    }
}

fn any_normal() -> impl Strategy<Value = [f64; 3]> {
    prop::array::uniform3(-1.0f64..1.0).prop_filter("a direction that is not nothing", |normal| {
        normal.iter().map(|part| part * part).sum::<f64>() > 0.04
    })
}

fn any_period() -> impl Strategy<Value = f64> {
    prop_oneof![1.0e3f64..1.0e9, -1.0e9f64..-1.0e3]
}

proptest! {
    #[test]
    fn an_ellipse_obeys_vis_viva_and_keeps_its_angular_momentum_and_its_sense(
        normal in any_normal(),
        log_radius in 6.0f64..20.0,
        ratio in 1.0e-3f64..1.0,
        periapsis in -7.0f64..7.0,
        period in any_period(),
        phase in -7.0f64..7.0,
        tick in 0u64..1_000_000_000_000,
    ) {
        let radius = libm::pow(10.0, log_radius);
        let rail = ellipse(normal, radius, ratio, periapsis, period, phase);
        let state = state_at(&rail, tick);
        let place = state.offset.unwrap().in_voxels();
        let velocity = state.velocity;
        let distance = place.iter().map(|part| part * part).sum::<f64>().sqrt();
        let speed_squared = velocity.iter().map(|part| part * part).sum::<f64>();
        // n^2 a^3 is the mass parameter of Kepler's third law; vis-viva: v^2 = mu (2 / r - 1 / a)
        let rate = TAU / period;
        let mu = rate * rate * radius * radius * radius;
        let vis_viva = mu * (2.0 / distance - 1.0 / radius);
        prop_assert!((speed_squared - vis_viva).abs() <= 1.0e-8 * speed_squared, "{speed_squared} against {vis_viva}");
        // h = r x v has the size n a^2 sqrt(1 - e^2), and is along the normal for a period above nothing and against it for one below.
        let momentum = [
            place[1] * velocity[2] - place[2] * velocity[1],
            place[2] * velocity[0] - place[0] * velocity[2],
            place[0] * velocity[1] - place[1] * velocity[0],
        ];
        let size = momentum.iter().map(|part| part * part).sum::<f64>().sqrt();
        let expected = rate.abs() * radius * radius * libm::sqrt(ratio * (2.0 - ratio));
        prop_assert!((size - expected).abs() <= 1.0e-8 * expected, "{size} against {expected}");
        let length = normal.iter().map(|part| part * part).sum::<f64>().sqrt();
        let along = (0..3).map(|axis| momentum[axis] * normal[axis] / length).sum::<f64>() / size;
        prop_assert!((along - period.signum()).abs() <= 1.0e-8, "along the normal: {along} for a period of {period}");
    }

    #[test]
    fn the_velocity_is_how_fast_the_place_changes(
        normal in any_normal(),
        log_radius in 8.0f64..18.0,
        ratio in 0.1f64..1.0,
        periapsis in -7.0f64..7.0,
        period in prop_oneof![1.0e5f64..1.0e9, -1.0e9f64..-1.0e5],
        phase in -7.0f64..7.0,
        seconds in 1.0e3f64..1.0e9,
    ) {
        // A central difference over two seconds: wrong by the square of the second over the time the orbit takes to change (a few hundred seconds at the sharpest here).
        let rail = ellipse(normal, libm::pow(10.0, log_radius), ratio, periapsis, period, phase);
        let before = rail.state(seconds - 1.0, NOWHERE, NOWHERE).offset.unwrap();
        let after = rail.state(seconds + 1.0, NOWHERE, NOWHERE).offset.unwrap();
        let velocity = rail.state(seconds, NOWHERE, NOWHERE).velocity;
        let speed = velocity.iter().map(|part| part * part).sum::<f64>().sqrt();
        for (axis, expected) in velocity.into_iter().enumerate() {
            let difference = ((after.bricks[axis] - before.bricks[axis]) as f64 * 32.0 + (after.rest[axis] - before.rest[axis])) / 2.0;
            prop_assert!((difference - expected).abs() <= 1.0e-4 * speed + 1.0e-9, "axis {axis}: {difference} against {expected}");
        }
    }
}

fn just_above(value: f64) -> f64 {
    f64::from_bits(value.to_bits() + 1)
}

#[test]
fn what_cannot_be_an_ellipse_is_refused_when_made_and_what_can_is_kept() {
    let make = |ratio: f64, periapsis: f64| {
        Orbit::elliptical(
            Centre::Point(NOWHERE),
            [0.0, 1.0, 0.0],
            1000.0,
            ratio,
            periapsis,
            100.0,
            0.0,
        )
    };
    for ratio in [
        0.0,
        -0.5,
        just_above(1.0),
        1.5,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        SMALLEST_RATIO / 2.0,
    ] {
        assert!(
            matches!(make(ratio, 0.0), Err(RailError::Invalid(_))),
            "a ratio of {ratio}"
        );
    }
    for periapsis in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        just_above(LARGEST_PHASE),
        -just_above(LARGEST_PHASE),
    ] {
        assert!(
            matches!(make(0.5, periapsis), Err(RailError::Invalid(_))),
            "a periapsis of {periapsis}"
        );
    }
    for (ratio, periapsis) in [
        (1.0, 0.0),
        (SMALLEST_RATIO, 0.0),
        (0.5, LARGEST_PHASE),
        (0.5, -LARGEST_PHASE),
        (1.0e-18, 3.0),
    ] {
        let orbit = make(ratio, periapsis).unwrap();
        assert_eq!((orbit.ratio, orbit.periapsis), (ratio, periapsis));
    }
    // The old way to make an orbit makes a circle that starts toward the first direction.
    let circle = Orbit::new(Centre::Point(NOWHERE), [0.0, 1.0, 0.0], 1000.0, 100.0, 0.0).unwrap();
    assert_eq!((circle.ratio, circle.periapsis), (1.0, 0.0));
    assert_eq!(circle, make(1.0, 0.0).unwrap());
}

fn rail_of(orbit: Orbit) -> Rail {
    Rail {
        spin: None,
        orbit: Some(orbit),
    }
}

fn an_ellipse() -> Orbit {
    Orbit::elliptical(
        Centre::Point([1.0, 2.0, 3.0]),
        [0.0, 1.0, 0.0],
        1.5e9,
        0.0167,
        1.8,
        31_557_600.0,
        1.0,
    )
    .unwrap()
}

/// Where the ratio and the periapsis are in the bytes of a rail that is only an orbit about a point: the flags, the centre's kind and point, the normal, the radius, the period
/// and the phase come first, and the two numbers after (version 2 of `engine:rail`: every orbit has them, a circle with 1.0 and nothing).
const RATIO_AT: usize = 1 + 1 + 24 + 24 + 8 + 8 + 8;

#[test]
fn an_orbit_is_written_with_its_ratio_and_its_periapsis_after_its_phase() {
    let ellipse_bytes = encode_rail(&rail_of(an_ellipse()));
    let circle = Orbit::new(
        Centre::Point([1.0, 2.0, 3.0]),
        [0.0, 1.0, 0.0],
        1.5e9,
        31_557_600.0,
        1.0,
    )
    .unwrap();
    let circle_bytes = encode_rail(&rail_of(circle));
    // The flags are the same (an orbit); what comes before the ratio is the same; a circle has a ratio of 1.0 and a periapsis of nothing, an ellipse its own.
    assert_eq!(circle_bytes[0], 0b010);
    assert_eq!(ellipse_bytes[0], 0b010);
    assert_eq!(ellipse_bytes.len(), RATIO_AT + 16);
    assert_eq!(ellipse_bytes[1..RATIO_AT], circle_bytes[1..RATIO_AT]);
    assert_eq!(
        circle_bytes[RATIO_AT..RATIO_AT + 8],
        1.0f64.to_bits().to_le_bytes()
    );
    assert_eq!(circle_bytes[RATIO_AT + 8..], 0.0f64.to_bits().to_le_bytes());
    assert_eq!(
        ellipse_bytes[RATIO_AT..RATIO_AT + 8],
        0.0167f64.to_bits().to_le_bytes()
    );
    assert_eq!(
        ellipse_bytes[RATIO_AT + 8..],
        1.8f64.to_bits().to_le_bytes()
    );
    // A rail with no orbit has no such numbers.
    let spinning = Rail {
        spin: Some(world_storage::rail::Spin::new([0.0, 1.0, 0.0], 60.0, 0.0).unwrap()),
        orbit: None,
    };
    assert_eq!(encode_rail(&spinning).len(), 1 + 24 + 8 + 8);
}

#[test]
fn an_ellipse_comes_back_from_its_bytes_the_same_to_the_bit() {
    let orbits = [
        an_ellipse(),
        Orbit::elliptical(
            Centre::Point(NOWHERE),
            [0.6, 0.8, 0.0],
            5.0e17,
            1.0e-18,
            -3.0,
            -9.0e8,
            -1.0,
        )
        .unwrap(),
        Orbit::elliptical(
            Centre::Point(NOWHERE),
            [0.0, 0.0, 1.0],
            3.0e8,
            1.0,
            2.5,
            4.0e5,
            0.0,
        )
        .unwrap(),
        Orbit::elliptical(
            Centre::Point(NOWHERE),
            [0.0, 0.0, 1.0],
            3.0e8,
            SMALLEST_RATIO,
            LARGEST_PHASE,
            4.0e5,
            0.0,
        )
        .unwrap(),
    ];
    for orbit in orbits {
        let rail = rail_of(orbit);
        let bytes = encode_rail(&rail);
        let again = decode_rail(&bytes).unwrap();
        assert_eq!(again, rail);
        assert_eq!(encode_rail(&again), bytes);
    }
}

#[test]
fn bytes_that_are_not_an_ellipse_are_refused() {
    let good = encode_rail(&rail_of(an_ellipse()));
    assert!(decode_rail(&good).is_ok());
    for length in 0..good.len() {
        assert!(decode_rail(&good[..length]).is_err(), "cut at {length}");
    }
    let mut longer = good.clone();
    longer.push(0);
    assert!(decode_rail(&longer).is_err(), "a byte too many");
    let write = |at: usize, value: f64| {
        let mut bytes = good.clone();
        bytes[at..at + 8].copy_from_slice(&value.to_bits().to_le_bytes());
        bytes
    };
    for (what, bytes) in [
        ("a ratio of nothing", write(RATIO_AT, 0.0)),
        ("a negative ratio", write(RATIO_AT, -0.5)),
        ("a ratio above one", write(RATIO_AT, just_above(1.0))),
        (
            "a ratio below the smallest",
            write(RATIO_AT, SMALLEST_RATIO / 2.0),
        ),
        ("a ratio that is not a number", write(RATIO_AT, f64::NAN)),
        ("an infinite ratio", write(RATIO_AT, f64::INFINITY)),
        (
            "a periapsis that is not a number",
            write(RATIO_AT + 8, f64::NAN),
        ),
        ("an infinite periapsis", write(RATIO_AT + 8, f64::INFINITY)),
        (
            "a periapsis too large",
            write(RATIO_AT + 8, just_above(LARGEST_PHASE)),
        ),
    ] {
        assert!(decode_rail(&bytes).is_err(), "{what}");
    }
    // The flag of version 1 of an ellipse (4) is nothing to this program, which has the numbers for every orbit.
    for flags in [0b100u8, 0b110, 0b111] {
        let mut flagged = good.clone();
        flagged[0] = flags;
        assert!(decode_rail(&flagged).is_err(), "flags {flags:#b}");
    }
}

/// The bits the program gave for the rows above (in the same order), once they agreed with the 120-digit values: `(bricks, rest as bits, velocity as bits)`. The same on every machine,
/// which an orbit that is simulation must be (they were checked under Linux too). If one moves, the arithmetic or `libm` did.
static PINS: [([i128; 3], [u64; 3], [u64; 3]); 15] = [
    (
        [14339306547934188300487885, 0, -12080843833291269939764885],
        [4622926368472956928, 0, 4621317058894364672],
        [14127985968149139216, 0, 14128864152791577312],
    ),
    (
        [
            -3067495228514122961,
            5789295454149192556,
            -4255547839892131076,
        ],
        [
            4610681876861943808,
            4621333090137800704,
            4618413090559754240,
        ],
        [
            13909221016363472347,
            13894047539469432373,
            4682581952046031719,
        ],
    ),
    (
        [7883059556272012, -5912294667204010, 29655769148690437],
        [
            4627400891208826880,
            4629596574014504960,
            4616159588264706048,
        ],
        [
            4741286136591988914,
            13962657441390365317,
            13958799546832532287,
        ],
    ),
    (
        [4687500000, 0, 0],
        [0, 0, 0],
        [4120907448745177000, 0, 13897309942950242831],
    ),
    (
        [
            -26215493239689928224895128,
            -691212763326508729217752,
            8277689237678970255486541,
        ],
        [
            4622732272905224192,
            4622222631337721856,
            4624035541463072768,
        ],
        [
            4817749841269446788,
            4829286581804354646,
            4826101322537355538,
        ],
    ),
    (
        [7324252, 9505541, 0],
        [4626949129489809408, 4626740517495439360, 0],
        [13873706606672188576, 4648699934560205047, 0],
    ),
    (
        [-52030198, 0, 3376831886],
        [4625318996719108096, 0, 4628936420036640768],
        [4664353173546721206, 0, 4675618626732881390],
    ),
    (
        [-95254994, 0, 3124385976],
        [4629044544734494720, 0, 4627842863672000512],
        [4664290795443290141, 0, 4675958428778477177],
    ),
    (
        [6228897, 9810761, 0],
        [4625181535650709504, 4628495197522886656, 0],
        [13874186548141534276, 4648533929734375021, 0],
    ),
    (
        [24370128600, -18277596451, -36587014628],
        [
            4629014545767923712,
            4621722054898221056,
            4626617067393712128,
        ],
        [
            4662551169370693259,
            13884334183932898767,
            13880736547907911332,
        ],
    ),
    (
        [
            35653637289455033265778755,
            1066308445560441843037350,
            -11173673466111383193234685,
        ],
        [
            4628770264571183104,
            4626460646309756928,
            4616296432249077760,
        ],
        [
            4827954497451753872,
            4805435402513651856,
            14043469250586380980,
        ],
    ),
    (
        [274243884, 0, -149821700],
        [4629290860780781568, 0, 4627453041456775168],
        [13914434411742550875, 0, 13918317665956666033],
    ),
    (
        [-963750624701026, 0, 525164681772098],
        [4616404888512626688, 0, 4625627369079767040],
        [13869401858373208454, 0, 4642194151217540537],
    ),
    (
        [2910, 158765, -93210],
        [
            4629167099633139712,
            4627195588873945088,
            4617821504069173248,
        ],
        [
            13877574708768126414,
            4666237580169984066,
            4657603142499802947,
        ],
    ),
    (
        [1144840428120, 122949061318, -352471997222],
        [
            4626328267050123264,
            4626727285663989760,
            4629107322291027968,
        ],
        [
            13905319097677335905,
            4670971229135959840,
            4677278929035982657,
        ],
    ),
];

#[test]
fn an_ellipse_is_pinned_to_the_bit() {
    for (row, (bricks, rest, velocity)) in ELLIPSES.iter().zip(PINS.iter()) {
        let (normal, radius, ratio, periapsis, period, phase, tick, per_second, ..) = *row;
        let rail = ellipse(normal, radius, ratio, periapsis, period, phase);
        let state = rail.state(RailTime::of_tick(tick, per_second), NOWHERE, NOWHERE);
        let found = state.offset.expect("an orbit has an offset");
        assert_eq!(
            (
                found.bricks,
                found.rest.map(f64::to_bits),
                state.velocity.map(f64::to_bits)
            ),
            (*bricks, *rest, *velocity),
            "ratio {ratio}, radius {radius}, tick {tick}"
        );
    }
}

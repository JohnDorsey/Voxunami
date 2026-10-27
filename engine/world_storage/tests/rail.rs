//! A rail: where a body is, how it moves and how it is turned, as a function of the time of the world alone. Circular orbits and a steady spin to begin with.

use std::f64::consts::{FRAC_PI_2, TAU};

use voxel_space::GridId;
use world_storage::rail::{
    Centre, Orbit, Rail, RailError, RailTime, Spin, decode_rail, encode_rail,
};

fn close(found: [f64; 3], expected: [f64; 3], tolerance: f64) {
    for axis in 0..3 {
        assert!(
            (found[axis] - expected[axis]).abs() <= tolerance,
            "{found:?} is not {expected:?} (axis {axis})"
        );
    }
}

/// `vector` turned by the unit quaternion `[x, y, z, w]`.
fn turned(quaternion: [f64; 4], vector: [f64; 3]) -> [f64; 3] {
    let [x, y, z, w] = quaternion;
    let (qx, qy, qz) = (x, y, z);
    // v' = v + 2 w (q x v) + 2 q x (q x v)
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let t = cross([qx, qy, qz], vector).map(|component| 2.0 * component);
    let u = cross([qx, qy, qz], t);
    [
        vector[0] + w * t[0] + u[0],
        vector[1] + w * t[1] + u[1],
        vector[2] + w * t[2] + u[2],
    ]
}

fn orbit(normal: [f64; 3], radius: f64, period: f64, phase: f64) -> Rail {
    Rail {
        spin: None,
        orbit: Some(Orbit::new(Centre::Point([0.0; 3]), normal, radius, period, phase).unwrap()),
    }
}

const NOWHERE: [f64; 3] = [0.0; 3];

#[test]
fn an_orbit_is_a_circle_gone_round_once_a_period_counter_clockwise_about_its_normal() {
    let rail = Rail {
        spin: None,
        orbit: Some(
            Orbit::new(
                Centre::Point([100.0, 200.0, 300.0]),
                [0.0, 1.0, 0.0],
                1000.0,
                100.0,
                0.0,
            )
            .unwrap(),
        ),
    };
    let at = |seconds: f64| {
        rail.state(seconds, [100.0, 200.0, 300.0], NOWHERE)
            .position
            .expect("an orbit has a place")
    };
    // Starting toward +x (the +x axis taken into the plane of the orbit), then a quarter of the way to -z, which is counter-clockwise looking down the normal.
    close(at(0.0), [1100.0, 200.0, 300.0], 1e-9);
    close(at(25.0), [100.0, 200.0, -700.0], 1e-9);
    close(at(50.0), [-900.0, 200.0, 300.0], 1e-9);
    close(at(75.0), [100.0, 200.0, 1300.0], 1e-9);
    close(at(100.0), at(0.0), 1e-9);
    // Always a thousand from the centre.
    for step in 0..40 {
        let place = at(f64::from(step) * 2.7);
        let (x, y, z) = (place[0] - 100.0, place[1] - 200.0, place[2] - 300.0);
        let distance = (x * x + y * y + z * z).sqrt();
        assert!((distance - 1000.0).abs() < 1e-9, "{distance}");
    }
}

#[test]
fn the_phase_is_where_in_the_circle_it_starts() {
    let rail = orbit([0.0, 1.0, 0.0], 1000.0, 100.0, FRAC_PI_2);
    close(
        rail.state(0.0, NOWHERE, NOWHERE).position.unwrap(),
        [0.0, 0.0, -1000.0],
        1e-9,
    );
    close(
        rail.state(25.0, NOWHERE, NOWHERE).position.unwrap(),
        [-1000.0, 0.0, 0.0],
        1e-9,
    );
}

#[test]
fn the_plane_of_the_orbit_is_the_one_the_normal_says_whichever_way_it_points() {
    // About +z: from +x toward +y.
    let rail = orbit([0.0, 0.0, 1.0], 10.0, 4.0, 0.0);
    close(
        rail.state(0.0, NOWHERE, NOWHERE).position.unwrap(),
        [10.0, 0.0, 0.0],
        1e-12,
    );
    close(
        rail.state(1.0, NOWHERE, NOWHERE).position.unwrap(),
        [0.0, 10.0, 0.0],
        1e-12,
    );
    // About +x (where +x cannot be taken into the plane): from +z toward -y.
    let rail = orbit([1.0, 0.0, 0.0], 10.0, 4.0, 0.0);
    close(
        rail.state(0.0, NOWHERE, NOWHERE).position.unwrap(),
        [0.0, 0.0, 10.0],
        1e-12,
    );
    close(
        rail.state(1.0, NOWHERE, NOWHERE).position.unwrap(),
        [0.0, -10.0, 0.0],
        1e-12,
    );
    // A normal that is not along an axis: the orbit stays in its plane (nothing out of it) and keeps its distance.
    let normal = [1.0, 2.0, 2.0];
    let rail = orbit(normal, 90.0, 60.0, 0.3);
    let length = 3.0;
    for second in [0.0, 7.0, 19.5, 33.0, 59.0] {
        let place = rail.state(second, NOWHERE, NOWHERE).position.unwrap();
        let out_of_plane =
            (place[0] * normal[0] + place[1] * normal[1] + place[2] * normal[2]) / length;
        assert!(out_of_plane.abs() < 1e-9, "{out_of_plane}");
        let distance = (place[0] * place[0] + place[1] * place[1] + place[2] * place[2]).sqrt();
        assert!((distance - 90.0).abs() < 1e-9);
    }
}

#[test]
fn a_negative_period_goes_the_other_way_round() {
    let forward = orbit([0.0, 1.0, 0.0], 1000.0, 100.0, 0.0);
    let backward = orbit([0.0, 1.0, 0.0], 1000.0, -100.0, 0.0);
    close(
        forward.state(25.0, NOWHERE, NOWHERE).position.unwrap(),
        [0.0, 0.0, -1000.0],
        1e-9,
    );
    close(
        backward.state(25.0, NOWHERE, NOWHERE).position.unwrap(),
        [0.0, 0.0, 1000.0],
        1e-9,
    );
}

#[test]
fn the_velocity_is_how_fast_the_place_changes_and_the_speed_is_the_circumference_over_the_period() {
    let rail = orbit([1.0, 2.0, 2.0], 1.5e6, 31_557_600.0, 1.0);
    let speed = TAU * 1.5e6 / 31_557_600.0;
    for second in [0.0, 1234.5, 8_000_000.0, 31_000_000.0] {
        let state = rail.state(second, NOWHERE, NOWHERE);
        let velocity = state.velocity;
        let magnitude =
            (velocity[0] * velocity[0] + velocity[1] * velocity[1] + velocity[2] * velocity[2])
                .sqrt();
        assert!(
            (magnitude - speed).abs() < 1e-9,
            "{magnitude} against {speed}"
        );
        // A centred difference over two seconds.
        let before = rail.state(second - 1.0, NOWHERE, NOWHERE).position.unwrap();
        let after = rail.state(second + 1.0, NOWHERE, NOWHERE).position.unwrap();
        let slope = [0, 1, 2].map(|axis| (after[axis] - before[axis]) / 2.0);
        close(velocity, slope, 1e-6);
    }
}

#[test]
fn what_it_goes_round_carries_it_along() {
    let rail = orbit([0.0, 1.0, 0.0], 1000.0, 100.0, 0.0);
    let centre = [5.0e6, -2.0e6, 7.0];
    let along = [30.0, -4.0, 0.5];
    let state = rail.state(25.0, centre, along);
    close(state.position.unwrap(), [5.0e6, -2.0e6, 7.0 - 1000.0], 1e-6);
    // Its own speed (a quarter of the way round it is going -x) and the centre's.
    close(state.velocity, [30.0 - TAU * 10.0, -4.0, 0.5], 1e-9);
}

#[test]
fn a_body_that_only_turns_has_no_place_and_one_that_only_goes_round_has_no_turn() {
    let turning = Rail {
        spin: Some(Spin::new([0.0, 1.0, 0.0], 3600.0, 0.0).unwrap()),
        orbit: None,
    };
    let state = turning.state(10.0, [1.0, 2.0, 3.0], [4.0, 5.0, 6.0]);
    assert_eq!(state.position, None);
    assert_eq!(state.velocity, [0.0; 3], "it stays where it is");
    assert!(state.orientation.is_some());

    let going = orbit([0.0, 1.0, 0.0], 1000.0, 100.0, 0.0);
    let state = going.state(10.0, NOWHERE, NOWHERE);
    assert_eq!(state.orientation, None);
    assert_eq!(state.angular_velocity, [0.0; 3]);
    assert!(state.position.is_some());

    let neither = Rail {
        spin: None,
        orbit: None,
    };
    let state = neither.state(10.0, NOWHERE, NOWHERE);
    assert_eq!(
        (
            state.position,
            state.orientation,
            state.velocity,
            state.angular_velocity
        ),
        (None, None, [0.0; 3], [0.0; 3])
    );
}

#[test]
fn a_spin_turns_the_body_about_its_axis_once_a_period() {
    let rail = Rail {
        spin: Some(Spin::new([0.0, 1.0, 0.0], 3600.0, 0.0).unwrap()),
        orbit: None,
    };
    let turn = |seconds: f64, vector: [f64; 3]| {
        let state = rail.state(seconds, NOWHERE, NOWHERE);
        turned(state.orientation.unwrap().components(), vector)
    };
    // At the start nothing is turned; a quarter of the way +x is carried to -z (a right-handed turn about +y), halfway to -x, and after a day it is back.
    close(turn(0.0, [1.0, 0.0, 0.0]), [1.0, 0.0, 0.0], 1e-12);
    close(turn(900.0, [1.0, 0.0, 0.0]), [0.0, 0.0, -1.0], 1e-12);
    close(turn(1800.0, [1.0, 0.0, 0.0]), [-1.0, 0.0, 0.0], 1e-12);
    close(turn(3600.0, [1.0, 0.0, 0.0]), [1.0, 0.0, 0.0], 1e-12);
    // The axis stays where it is.
    close(turn(1234.5, [0.0, 1.0, 0.0]), [0.0, 1.0, 0.0], 1e-12);
    // How fast it turns: an axis scaled by radians a second.
    close(
        rail.state(0.0, NOWHERE, NOWHERE).angular_velocity,
        [0.0, TAU / 3600.0, 0.0],
        1e-15,
    );
}

#[test]
fn the_angle_at_the_start_is_how_far_it_has_turned_by_then_and_a_negative_period_turns_back() {
    let rail = Rail {
        spin: Some(Spin::new([0.0, 1.0, 0.0], 3600.0, FRAC_PI_2).unwrap()),
        orbit: None,
    };
    let state = rail.state(0.0, NOWHERE, NOWHERE);
    close(
        turned(state.orientation.unwrap().components(), [1.0, 0.0, 0.0]),
        [0.0, 0.0, -1.0],
        1e-12,
    );

    let backward = Rail {
        spin: Some(Spin::new([0.0, 1.0, 0.0], -3600.0, 0.0).unwrap()),
        orbit: None,
    };
    let state = backward.state(900.0, NOWHERE, NOWHERE);
    close(
        turned(state.orientation.unwrap().components(), [1.0, 0.0, 0.0]),
        [0.0, 0.0, 1.0],
        1e-12,
    );
    close(state.angular_velocity, [0.0, -TAU / 3600.0, 0.0], 1e-15);
}

#[test]
fn the_axis_of_a_tilted_spin_is_where_the_bodys_own_up_goes_and_it_stays_there() {
    for axis in [
        [0.6, 0.8, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
    ] {
        let rail = Rail {
            spin: Some(Spin::new(axis, 3600.0, 0.4).unwrap()),
            orbit: None,
        };
        for seconds in [0.0, 700.0, 1801.5, 3599.0] {
            let state = rail.state(seconds, NOWHERE, NOWHERE);
            let orientation = state.orientation.unwrap().components();
            close(turned(orientation, [0.0, 1.0, 0.0]), axis, 1e-12);
        }
        // And it turns about that axis.
        let state = rail.state(0.0, NOWHERE, NOWHERE);
        close(
            state.angular_velocity,
            axis.map(|component| component * TAU / 3600.0),
            1e-15,
        );
    }
}

#[test]
fn an_orbit_and_a_spin_are_independent() {
    let both = Rail {
        spin: Some(Spin::new([0.0, 1.0, 0.0], 3600.0, 0.0).unwrap()),
        orbit: Some(
            Orbit::new(Centre::Point(NOWHERE), [0.0, 1.0, 0.0], 1000.0, 100.0, 0.0).unwrap(),
        ),
    };
    let spin = Rail {
        orbit: None,
        ..both
    };
    let orbit_only = Rail { spin: None, ..both };
    let (all, turning, going) = (
        both.state(77.0, NOWHERE, NOWHERE),
        spin.state(77.0, NOWHERE, NOWHERE),
        orbit_only.state(77.0, NOWHERE, NOWHERE),
    );
    assert_eq!(all.position, going.position);
    assert_eq!(all.velocity, going.velocity);
    assert_eq!(all.orientation, turning.orientation);
    assert_eq!(all.angular_velocity, turning.angular_velocity);
}

#[test]
fn a_world_that_has_run_for_years_is_where_one_just_a_period_younger_is() {
    // The whole turns are taken out before the angle is made, so a long time loses nothing: 1,000,000 periods and a quarter.
    let rail = Rail {
        spin: Some(Spin::new([0.0, 1.0, 0.0], 3600.0, 0.0).unwrap()),
        orbit: Some(
            Orbit::new(Centre::Point(NOWHERE), [0.0, 1.0, 0.0], 1000.0, 3600.0, 0.0).unwrap(),
        ),
    };
    let young = rail.state(900.0, NOWHERE, NOWHERE);
    let old = rail.state(1_000_000.0 * 3600.0 + 900.0, NOWHERE, NOWHERE);
    assert_eq!(old.position, young.position);
    assert_eq!(old.orientation, young.orientation);
    assert_eq!(old.velocity, young.velocity);
}

// ---- time as whole ticks ----

const TICKS_A_SECOND: u32 = 20;

/// Where a circle of 10^6 voxels about +y is at `fraction` of a turn, worked out here (the same `libm` angle, from a fraction the test worked out in whole numbers).
fn on_the_circle(fraction: f64) -> [f64; 3] {
    let angle = TAU * fraction;
    [1.0e6 * libm::cos(angle), 0.0, -1.0e6 * libm::sin(angle)]
}

#[test]
fn a_planet_is_where_the_exact_turn_puts_it_however_many_ticks_the_world_has_run() {
    // A hundred seconds is 2,000 ticks, so the part of a turn gone at tick T is (T mod 2,000) / 2,000, in whole numbers. 7.6 x 10^12 ticks is twelve thousand years;
    // 3.8 x 10^18 is six billion; the last is the end of the clock. A turn worked out from the seconds as an f64 is out by whole voxels by the first of these.
    let rail = orbit([0.0, 1.0, 0.0], 1.0e6, 100.0, 0.0);
    for tick in [
        0u64,
        1,
        1_999,
        7_600_000_000_037,
        3_800_000_000_000_000_001,
        u64::MAX,
    ] {
        let found = rail
            .state(RailTime::of_tick(tick, TICKS_A_SECOND), NOWHERE, NOWHERE)
            .position
            .unwrap();
        let expected = on_the_circle((tick % 2_000) as f64 / 2_000.0);
        close(found, expected, 1.0e-9);
    }
}

#[test]
fn a_planet_is_exactly_where_it_began_after_any_whole_number_of_periods() {
    // 52,700 seconds is 1,054,000 ticks. Whole periods later, however many, is the same bits as time 0, for an orbit and a spin.
    let rail = Rail {
        spin: Some(Spin::new([0.0, 1.0, 0.0], 52_700.0, 0.4).unwrap()),
        orbit: Some(
            Orbit::new(
                Centre::Point(NOWHERE),
                [0.0, 1.0, 0.0],
                1.0e6,
                52_700.0,
                0.7,
            )
            .unwrap(),
        ),
    };
    let began = rail.state(RailTime::of_tick(0, TICKS_A_SECOND), NOWHERE, NOWHERE);
    let a_tick_in = rail.state(RailTime::of_tick(1, TICKS_A_SECOND), NOWHERE, NOWHERE);
    assert_ne!(began, a_tick_in);
    for periods in [1u64, 12_345, 3_000_000_000_000, 17_000_000_000_000] {
        let whole = periods * 1_054_000;
        let later = rail.state(RailTime::of_tick(whole, TICKS_A_SECOND), NOWHERE, NOWHERE);
        assert_eq!(later, began, "after {periods} periods");
        // And a tick past them is a tick past the start (a time in seconds as an f64 could not say which tick: 20 ticks to the second, at 10^17 seconds).
        let one_more = rail.state(
            RailTime::of_tick(whole + 1, TICKS_A_SECOND),
            NOWHERE,
            NOWHERE,
        );
        assert_eq!(one_more, a_tick_in, "a tick after {periods} periods");
    }
}

#[test]
fn a_period_of_a_fraction_of_a_second_or_of_a_year_is_kept_exactly_too() {
    // 2,565.5 seconds is 51,310 ticks, 0.25 s is 5 ticks, and 3.156e7 (a year) is 631,200,000.
    for (period, ticks_a_period) in [(2_565.5, 51_310u64), (0.25, 5), (3.156e7, 631_200_000)] {
        let rail = orbit([0.0, 1.0, 0.0], 1.0e6, period, 0.0);
        let began = rail.state(RailTime::of_tick(0, TICKS_A_SECOND), NOWHERE, NOWHERE);
        let later = rail.state(
            RailTime::of_tick(ticks_a_period * 9_000_000_000, TICKS_A_SECOND),
            NOWHERE,
            NOWHERE,
        );
        assert_eq!(later, began, "period {period}");
    }
}

#[test]
fn an_orbit_the_other_way_is_the_mirror_of_one_the_right_way_at_any_tick() {
    let forward = orbit([0.0, 1.0, 0.0], 1.0e6, 100.0, 0.0);
    let backward = orbit([0.0, 1.0, 0.0], 1.0e6, -100.0, 0.0);
    for tick in [1u64, 733, 7_600_000_000_037, u64::MAX] {
        let time = RailTime::of_tick(tick, TICKS_A_SECOND);
        let [x, y, z] = forward.state(time, NOWHERE, NOWHERE).position.unwrap();
        let mirrored = backward.state(time, NOWHERE, NOWHERE).position.unwrap();
        close(mirrored, [x, y, -z], 1.0e-9);
    }
}

#[test]
fn a_time_in_seconds_and_the_same_time_in_ticks_are_the_same_time() {
    let rail = orbit([0.0, 1.0, 0.0], 1.0e6, 100.0, 0.3);
    assert_eq!(
        rail.state(RailTime::of_tick(30, 30), NOWHERE, NOWHERE),
        rail.state(1.0, NOWHERE, NOWHERE)
    );
    assert_eq!(
        rail.state(RailTime::of_tick(30, 20), NOWHERE, NOWHERE),
        rail.state(1.5, NOWHERE, NOWHERE)
    );
    // And before the world began (a time that is not a tick of it), the circle is gone round backward from the start: a quarter of a turn back is a quarter short.
    close(
        rail.state(-25.0, NOWHERE, NOWHERE).position.unwrap(),
        orbit([0.0, 1.0, 0.0], 1.0e6, 100.0, 0.3 - FRAC_PI_2)
            .state(0.0, NOWHERE, NOWHERE)
            .position
            .unwrap(),
        1.0e-9,
    );
}

#[test]
fn a_rail_that_is_no_rail_is_refused_when_it_is_made() {
    let bad_spins = [
        ([0.0; 3], 3600.0, 0.0),
        ([0.0, 1.0, 0.0], 0.0, 0.0),
        ([0.0, 1.0, 0.0], f64::NAN, 0.0),
        ([0.0, f64::INFINITY, 0.0], 3600.0, 0.0),
        ([0.0, 1.0, 0.0], 3600.0, f64::NAN),
    ];
    for (axis, period, angle) in bad_spins {
        assert!(
            Spin::new(axis, period, angle).is_err(),
            "{axis:?} {period} {angle}"
        );
    }
    let bad_orbits = [
        ([0.0; 3], 1000.0, 100.0, 0.0),
        ([0.0, 1.0, 0.0], 0.0, 100.0, 0.0),
        ([0.0, 1.0, 0.0], -5.0, 100.0, 0.0),
        ([0.0, 1.0, 0.0], 1000.0, 0.0, 0.0),
        ([0.0, 1.0, 0.0], 1000.0, f64::INFINITY, 0.0),
        ([0.0, 1.0, 0.0], 1000.0, 100.0, f64::NAN),
        ([f64::NAN, 1.0, 0.0], 1000.0, 100.0, 0.0),
    ];
    for (normal, radius, period, phase) in bad_orbits {
        assert!(
            Orbit::new(Centre::Point(NOWHERE), normal, radius, period, phase).is_err(),
            "{normal:?} {radius} {period} {phase}"
        );
    }
    assert!(
        Orbit::new(
            Centre::Point([f64::NAN, 0.0, 0.0]),
            [0.0, 1.0, 0.0],
            1.0,
            1.0,
            0.0
        )
        .is_err()
    );
    assert_eq!(
        Spin::new([0.0; 3], 3600.0, 0.0).unwrap_err(),
        RailError::NoDirection("the axis of a spin")
    );
}

#[test]
fn a_rail_is_written_as_bytes_and_comes_back_the_same_to_the_bit() {
    let rails = [
        Rail {
            spin: None,
            orbit: None,
        },
        Rail {
            spin: Some(Spin::new([0.0, 1.0, 0.0], 86_400.0, 0.5).unwrap()),
            orbit: None,
        },
        Rail {
            spin: None,
            orbit: Some(
                Orbit::new(
                    Centre::Point([1.5e9, -3.0, 0.25]),
                    [1.0, 2.0, 2.0],
                    7.5e7,
                    -31_557_600.0,
                    2.0,
                )
                .unwrap(),
            ),
        },
        Rail {
            spin: Some(Spin::new([0.6, 0.8, 0.0], -3600.0, -1.0).unwrap()),
            orbit: Some(
                Orbit::new(
                    Centre::Grid(GridId::new(GridId::FIRST_ALLOCATED + 3)),
                    [0.0, 0.0, 1.0],
                    2.0e5,
                    1.2e4,
                    0.0,
                )
                .unwrap(),
            ),
        },
    ];
    for rail in rails {
        let bytes = encode_rail(&rail);
        let again = decode_rail(&bytes).unwrap();
        assert_eq!(again, rail);
        assert_eq!(encode_rail(&again), bytes);
    }
}

#[test]
fn bytes_that_are_not_a_rail_are_refused_and_never_a_panic() {
    let rail = Rail {
        spin: Some(Spin::new([0.0, 1.0, 0.0], 3600.0, 0.0).unwrap()),
        orbit: Some(
            Orbit::new(
                Centre::Grid(GridId::new(7)),
                [0.0, 1.0, 0.0],
                1000.0,
                100.0,
                0.0,
            )
            .unwrap(),
        ),
    };
    let good = encode_rail(&rail);
    assert!(decode_rail(&good).is_ok());
    // Cut short at every length, and with something after it.
    for length in 0..good.len() {
        assert!(decode_rail(&good[..length]).is_err(), "cut at {length}");
    }
    let mut longer = good.clone();
    longer.push(0);
    assert!(decode_rail(&longer).is_err(), "something after");
    // A flag no one has, a kind of centre no one has.
    let mut flagged = good.clone();
    flagged[0] |= 0b100;
    assert!(decode_rail(&flagged).is_err());
    // Numbers that make no rail: a zero period of the spin (the eight bytes after the three of the axis).
    let mut stopped = good.clone();
    stopped[1 + 24..1 + 32].copy_from_slice(&0.0f64.to_bits().to_le_bytes());
    assert!(decode_rail(&stopped).is_err());
    // A kind of centre no one has (the byte after the flags, the spin's forty, and the one of its own).
    let mut odd_centre = good.clone();
    odd_centre[1 + 40] = 9;
    assert!(decode_rail(&odd_centre).is_err());
    // A centre that is not a number (the point's first coordinate, eight bytes after the byte that says it is a point).
    let mut nowhere_at_all = good.clone();
    nowhere_at_all[1 + 40] = 0;
    // (The rail above goes round a ship, so make one that goes round a point to change.)
    let about_a_point = encode_rail(&Rail {
        spin: None,
        orbit: Some(
            Orbit::new(
                Centre::Point([1.0, 2.0, 3.0]),
                [0.0, 1.0, 0.0],
                100.0,
                60.0,
                0.0,
            )
            .unwrap(),
        ),
    });
    assert!(decode_rail(&about_a_point).is_ok());
    let mut not_a_number = about_a_point.clone();
    not_a_number[2..10].copy_from_slice(&f64::NAN.to_bits().to_le_bytes());
    assert!(
        decode_rail(&not_a_number).is_err(),
        "a centre that is not a number"
    );
    let mut infinite = about_a_point.clone();
    infinite[10..18].copy_from_slice(&f64::INFINITY.to_bits().to_le_bytes());
    assert!(decode_rail(&infinite).is_err(), "a centre at infinity");
    // A radius or a period or a phase that is no number, or a radius that is nothing.
    for (what, from, value) in [
        ("a radius of nothing", 1 + 1 + 24 + 24, 0.0f64),
        ("a radius below nothing", 1 + 1 + 24 + 24, -1.0),
        ("a period of nothing", 1 + 1 + 24 + 24 + 8, 0.0),
        ("a phase that is no number", 1 + 1 + 24 + 24 + 16, f64::NAN),
    ] {
        let mut broken = about_a_point.clone();
        broken[from..from + 8].copy_from_slice(&value.to_bits().to_le_bytes());
        assert!(decode_rail(&broken).is_err(), "{what}");
    }
    let _ = nowhere_at_all;
    // An axis that is not a unit vector.
    let mut long_axis = good.clone();
    long_axis[1 + 8..1 + 16].copy_from_slice(&2.0f64.to_bits().to_le_bytes());
    assert!(decode_rail(&long_axis).is_err());
}

#[test]
fn the_place_of_a_planet_on_an_orbit_is_pinned_to_the_bit() {
    // The same on every machine: a year of a planet 1.5e9 voxels out, a day of its spin, 12,345 seconds in. If this moves, the libm or the arithmetic did, and
    // every saved world with a planet on rails is somewhere else.
    let rail = Rail {
        spin: Some(Spin::new([0.0, 1.0, 0.0], 86_400.0, 0.5).unwrap()),
        orbit: Some(
            Orbit::new(
                Centre::Point(NOWHERE),
                [0.0, 1.0, 0.0],
                1.5e9,
                31_557_600.0,
                1.0,
            )
            .unwrap(),
        ),
    };
    let state = rail.state(12_345.0, NOWHERE, NOWHERE);
    let bits = |values: &[f64]| {
        values
            .iter()
            .map(|value| value.to_bits())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        bits(&state.position.unwrap()),
        PINNED_POSITION,
        "{:?}",
        state.position
    );
    assert_eq!(
        bits(&state.velocity),
        PINNED_VELOCITY,
        "{:?}",
        state.velocity
    );
    assert_eq!(
        bits(&state.orientation.unwrap().components()),
        PINNED_ORIENTATION,
        "{:?}",
        state.orientation
    );
}

// (The x moved by one bit when orbits were made exact: 4740055739430465341 is the correctly rounded f64 of the true 807348616.6088635310619857..., which the f64 sine
// before it was a bit short of. Checked at 80 digits.)
const PINNED_POSITION: [u64; 3] = [4740055739430465341, 0, 13966461261688665696];
// (The z moved by one bit for the same reason: the cosine in it is now the correctly rounded one.)
const PINNED_VELOCITY: [u64; 3] = [13866432095420009626, 0, 13863231759238159558];
const PINNED_ORIENTATION: [u64; 4] = [0, 4603970075599681472, 0, 4605070818266441370];

// ---- an orbit exact to a small fraction of a voxel however far out it is

/// Orbits worked out at 120 digits by `engine/world_storage/tools/exact_circle_reference.py`, from the numbers as the program keeps them (the normal made a unit vector
/// in `f64`, as `Orbit::new` does): `(normal, radius, period, phase, tick, ticks a second, bricks, rest)` for the offset from the centre at that tick.
type PinnedOffset = ([f64; 3], f64, f64, f64, u64, u32, [i128; 3], [f64; 3]);

const OFFSETS: [PinnedOffset; 6] = [
    (
        [0.0, 1.0, 0.0],
        6e+26,
        31560000.0,
        0.7,
        12345,
        20,
        [14339306547934188300487885, 0, -12080843833291269939764885],
        [11.966872680635376, 0.0, 9.108164604075284],
    ),
    (
        [0.3333333333333333, 0.6666666666666666, 0.6666666666666666],
        2.5e+20,
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
    ),
    (
        [0.6, 0.8, 0.0],
        1e+18,
        5000000000.0,
        -1.25,
        333,
        20,
        [7883059556272012, -5912294667204010, 29655769148690437],
        [23.8304434911537, 31.63107582936384, 3.986664094268062],
    ),
    (
        [0.0, 1.0, 0.0],
        150000000000.0,
        31560000.0,
        0.0,
        0,
        20,
        [4687500000, 0, 0],
        [0.0, 0.0, 0.0],
    ),
    (
        [0.2672612419124244, -0.5345224838248488, 0.8017837257372732],
        8.8e+26,
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
    ),
    (
        [0.0, 0.0, 1.0],
        384000000.0,
        2360000.0,
        5.5,
        18446744073709551615,
        20,
        [7324252, 9505541, 0],
        [22.22546345251247, 21.484324766636632, 0.0],
    ),
];

/// How far, in voxels, a found offset may be from the one worked out: a step of 2^-32 voxel, to which both are rounded, and the few bits of 126 that the sine, cosine and
/// plane lose, times a radius of up to 2^90. Measured over the cases above: see the test that prints it.
const OFFSET_TOLERANCE: f64 = 1.0 / 2_147_483_648.0;

fn offset_error(found: world_storage::rail::ExactOffset, bricks: [i128; 3], rest: [f64; 3]) -> f64 {
    (0..3)
        .map(|axis| {
            let apart =
                (found.bricks[axis] - bricks[axis]) as f64 * 32.0 + (found.rest[axis] - rest[axis]);
            apart.abs()
        })
        .fold(0.0, f64::max)
}

#[test]
fn an_orbit_a_very_long_way_out_is_where_the_exact_turn_puts_it_to_a_small_fraction_of_a_voxel() {
    let mut worst = 0.0f64;
    for (normal, radius, period, phase, tick, ticks_per_second, bricks, rest) in OFFSETS {
        let rail = orbit(normal, radius, period, phase);
        let state = rail.state(RailTime::of_tick(tick, ticks_per_second), NOWHERE, NOWHERE);
        let found = state.offset.expect("an orbit has an offset");
        let error = offset_error(found, bricks, rest);
        worst = worst.max(error);
        assert!(
            error <= OFFSET_TOLERANCE,
            "radius {radius} at tick {tick}: found {found:?}, expected {bricks:?} {rest:?} ({error} voxels out)"
        );
        // The position as plain numbers is that, to the rounding of an f64.
        let position = state.position.unwrap();
        let in_voxels = found.in_voxels();
        for axis in 0..3 {
            assert_eq!(position[axis], in_voxels[axis]);
        }
    }
    println!("worst offset error: {worst} voxels");
}

#[test]
fn an_orbit_at_the_start_of_its_circle_is_at_exactly_its_radius() {
    // The circle starts toward +x for a normal along +y: a radius of 1.5 x 10^11 voxels is 4,687,500,000 bricks, with nothing over, on the dot.
    let found = orbit([0.0, 1.0, 0.0], 1.5e11, 3.156e7, 0.0)
        .state(0.0, NOWHERE, NOWHERE)
        .offset
        .unwrap();
    assert_eq!(
        (found.bricks, found.rest),
        ([4_687_500_000, 0, 0], [0.0; 3])
    );
}

#[test]
fn an_orbit_in_the_plane_z_equals_0_stays_in_it_however_far_out_it_goes() {
    // 8.8 x 10^26 voxels, the size of the universe in metres: an f64 of the radius is 1.4 x 10^11 apart from the next, and an f64 circle there is that far out of its plane.
    let rail = orbit([0.0, 0.0, 1.0], 8.8e26, 1.0e12, 0.3);
    for step in 0..100 {
        let found = rail
            .state(
                RailTime::of_tick(step * 200_000_000_000, 20),
                NOWHERE,
                NOWHERE,
            )
            .offset
            .unwrap();
        assert_eq!((found.bricks[2], found.rest[2]), (0, 0.0), "step {step}");
    }
}

/// 2^120 voxels, 2^80 seconds and 2^63 radians: the most an orbit has of each.
const LARGEST_RADIUS: f64 = (1u128 << 120) as f64;
const LONGEST_PERIOD: f64 = (1u128 << 80) as f64;
const LARGEST_PHASE: f64 = (1u64 << 63) as f64;

fn just_above(value: f64) -> f64 {
    f64::from_bits(value.to_bits() + 1)
}

#[test]
fn what_the_arithmetic_cannot_hold_is_refused_when_made_and_what_it_can_is_kept_to_the_end() {
    let make = |radius: f64, period: f64, phase: f64| {
        Orbit::new(
            Centre::Point(NOWHERE),
            [0.0, 1.0, 0.0],
            radius,
            period,
            phase,
        )
    };
    assert!(make(LARGEST_RADIUS, LONGEST_PERIOD, LARGEST_PHASE).is_ok());
    assert!(make(LARGEST_RADIUS, -LONGEST_PERIOD, -LARGEST_PHASE).is_ok());
    assert!(make(just_above(LARGEST_RADIUS), 100.0, 0.0).is_err());
    assert!(make(1.0e300, 100.0, 0.0).is_err());
    assert!(make(1.0, just_above(LONGEST_PERIOD), 0.0).is_err());
    assert!(make(1.0, -just_above(LONGEST_PERIOD), 0.0).is_err());
    assert!(make(1.0, 100.0, just_above(LARGEST_PHASE)).is_err());
    assert!(make(1.0, 100.0, -just_above(LARGEST_PHASE)).is_err());
    assert!(Spin::new([0.0, 1.0, 0.0], LONGEST_PERIOD, 0.0).is_ok());
    assert!(Spin::new([0.0, 1.0, 0.0], just_above(LONGEST_PERIOD), 0.0).is_err());
    assert!(Spin::new([0.0, 1.0, 0.0], -just_above(LONGEST_PERIOD), 0.0).is_err());

    // And the most there is of each is worked out, at the last tick of the clock and the first, without a number past what it holds: the offset of the largest radius
    // along +x is 2^115 bricks, to a quarter of a voxel (the sine and cosine are right to 2^-121, and 2^120 of that is 2^-1).
    for (tick, ticks_per_second) in [(0, 1), (u64::MAX, 1), (u64::MAX, u32::MAX)] {
        let found = orbit([0.0, 1.0, 0.0], LARGEST_RADIUS, LONGEST_PERIOD, 0.0)
            .state(RailTime::of_tick(tick, ticks_per_second), NOWHERE, NOWHERE)
            .offset
            .unwrap();
        let length = |bricks: [i128; 3], rest: [f64; 3]| {
            let [x, y, z] = [0, 1, 2].map(|axis| bricks[axis] as f64 * 32.0 + rest[axis]);
            (x * x + y * y + z * z).sqrt()
        };
        // (An f64 of the length is 2^67 voxels from the next, so the bricks are looked at instead: the sum of the squares of the bricks is the radius squared in bricks, to what an f64 shows.)
        let bricks_length = length(found.bricks, [0.0; 3]) / 32.0;
        assert!(
            (bricks_length / (LARGEST_RADIUS / 32.0) - 1.0).abs() < 1.0e-15,
            "{found:?} at tick {tick}"
        );
    }
}

#[test]
fn a_rail_that_is_read_with_a_number_too_large_for_the_arithmetic_is_refused_as_one_that_is_made_is()
 {
    let rail = |radius: f64, period: f64, phase: f64| Rail {
        spin: Some(Spin::new([0.0, 1.0, 0.0], 3600.0, 0.0).unwrap()),
        orbit: Some(
            Orbit::new(Centre::Point(NOWHERE), [0.0, 1.0, 0.0], 1.0, 1.0, 0.0)
                .map(|mut orbit| {
                    orbit.radius = radius;
                    orbit.period = period;
                    orbit.phase = phase;
                    orbit
                })
                .unwrap(),
        ),
    };
    assert!(
        decode_rail(&encode_rail(&rail(
            LARGEST_RADIUS,
            LONGEST_PERIOD,
            LARGEST_PHASE
        )))
        .is_ok()
    );
    for (what, bad) in [
        ("a radius", rail(just_above(LARGEST_RADIUS), 1.0, 0.0)),
        ("a period", rail(1.0, just_above(LONGEST_PERIOD), 0.0)),
        ("a phase", rail(1.0, 1.0, just_above(LARGEST_PHASE))),
        (
            "a period of a spin",
            Rail {
                spin: Some(Spin {
                    axis: [0.0, 1.0, 0.0],
                    period: just_above(LONGEST_PERIOD),
                    angle: 0.0,
                }),
                orbit: None,
            },
        ),
    ] {
        assert!(
            decode_rail(&encode_rail(&bad)).is_err(),
            "{what} too large to be held"
        );
    }
}

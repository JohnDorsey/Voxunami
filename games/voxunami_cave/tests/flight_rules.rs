//! The rules of flight from a command seat that need no server: what the keys of a seated pilot ask, the throttle, the torque the seat gives, the push of an engine and the
//! lines of the readout.

use voxel_space::{FaceDirection, GridId, LatticeRotation, VoxelCoordinate};
use voxunami_cave::flight::{
    ENGINE_THRUST, FlightKeys, MOST_TURN_RATE, OrbitSummary, Readout, SEAT_ANGULAR_ACCELERATION,
    STILL_RATE, SeatFrame, ThrottleChange, distance_speed_and_speed_away, engine_at,
    throttle_after, turning,
};
use world_storage::rail::{Centre, Orbit, RailTime};

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1.0e-12)
}

fn scaled(vector: [f64; 3], by: f64) -> [f64; 3] {
    vector.map(|component| component * by)
}

fn sum(vectors: &[[f64; 3]]) -> [f64; 3] {
    let mut total = [0.0; 3];
    for vector in vectors {
        for axis in 0..3 {
            total[axis] += vector[axis];
        }
    }
    total
}

fn orientation_carrying(from: FaceDirection, to: FaceDirection) -> u32 {
    LatticeRotation::all()
        .into_iter()
        .find(|rotation| rotation.apply_face(from) == to)
        .unwrap()
        .index()
        .into()
}

#[test]
fn the_keys_of_a_seated_pilot_are_pitch_yaw_roll_and_throttle() {
    // W pitches the nose down, S up, D yaws to the right, A to the left, E rolls to the right, Q to the left; shift raises the throttle and control lowers it.
    let keys = FlightKeys::of_movement(1, -1, -1, false, 1);
    assert_eq!(
        keys,
        FlightKeys {
            pitch_down: 1,
            yaw_right: -1,
            roll_right: 1,
            throttle: ThrottleChange::Up,
        }
    );
    let keys = FlightKeys::of_movement(-1, 1, 0, true, -1);
    assert_eq!(
        keys,
        FlightKeys {
            pitch_down: -1,
            yaw_right: 1,
            roll_right: -1,
            throttle: ThrottleChange::Down,
        }
    );
    // Both throttle keys, or neither, leave it; space (up, in a walker's keys) does nothing.
    assert_eq!(
        FlightKeys::of_movement(0, 0, -1, true, 0).throttle,
        ThrottleChange::Hold
    );
    assert_eq!(
        FlightKeys::of_movement(0, 0, 1, false, 0).throttle,
        ThrottleChange::Hold
    );
    assert_eq!(
        FlightKeys::of_movement(0, 0, 0, false, 0),
        FlightKeys::default()
    );
}

#[test]
fn the_throttle_moves_at_a_steady_rate_and_stays_between_nothing_and_full() {
    let seconds = 0.05;
    let mut throttle = 0.0;
    for _ in 0..20 {
        throttle = throttle_after(throttle, ThrottleChange::Up, seconds);
    }
    // (20 ticks of a twentieth of a second at half the throttle a second.)
    assert!((throttle - 0.5).abs() < 1.0e-12, "{throttle}");
    for _ in 0..100 {
        throttle = throttle_after(throttle, ThrottleChange::Up, seconds);
    }
    assert_eq!(throttle, 1.0);
    assert_eq!(throttle_after(1.0, ThrottleChange::Hold, seconds), 1.0);
    for _ in 0..100 {
        throttle = throttle_after(throttle, ThrottleChange::Down, seconds);
    }
    assert_eq!(throttle, 0.0);
}

#[test]
fn a_seat_as_it_is_made_faces_west_with_up_up_and_the_right_hand_to_the_north() {
    let frame = SeatFrame::of_orientation(0).unwrap();
    assert_eq!(frame.forward, [-1.0, 0.0, 0.0]);
    assert_eq!(frame.up, [0.0, 1.0, 0.0]);
    // (Facing west, with up up, the right hand points north, which is -z.)
    assert_eq!(frame.right, [0.0, 0.0, -1.0]);
    // A number that is no turn is no seat.
    assert!(SeatFrame::of_orientation(24).is_none());
    // Turned, all three turn with it, and stay at right angles in the same hand.
    for (orientation, rotation) in LatticeRotation::all().into_iter().enumerate() {
        let turned = SeatFrame::of_orientation(orientation as u32).unwrap();
        let apply = |vector: [i128; 3]| rotation.apply(vector).map(|component| component as f64);
        assert_eq!(turned.forward, apply([-1, 0, 0]));
        assert_eq!(turned.up, apply([0, 1, 0]));
        assert_eq!(turned.right, apply([0, 0, -1]));
    }
}

#[test]
fn an_engine_pushes_the_ship_the_way_opposite_its_exhaust_from_the_middle_of_its_voxel() {
    let voxel = VoxelCoordinate::new(3, -2, 7);
    // Made with the exhaust up: it pushes down.
    let engine = engine_at(voxel, 0).unwrap();
    assert_eq!(engine.at, [3.5, -1.5, 7.5]);
    assert_eq!(engine.push, [0.0, -1.0, 0.0]);
    // Placed against the east face of something, the exhaust is east and it pushes west.
    let east = orientation_carrying(FaceDirection::PositiveY, FaceDirection::PositiveX);
    assert_eq!(engine_at(voxel, east).unwrap().push, [-1.0, 0.0, 0.0]);
    for exhaust in FaceDirection::ALL {
        let orientation = orientation_carrying(FaceDirection::PositiveY, exhaust);
        let push = engine_at(voxel, orientation).unwrap().push;
        assert_eq!(
            push.map(|component| -component),
            exhaust.offset().map(|n| n as f64)
        );
    }
    assert!(engine_at(voxel, 24).is_none());
    // At full throttle one engine pushes with its thrust.
    assert_eq!(ENGINE_THRUST, 200_000.0);
}

/// A seat as it is made, a ship that has the inertia 1,000 about every axis, and the length of a tick.
const INERTIA: f64 = 1_000.0;
const TICK: f64 = 0.05;

fn torque_for(keys: FlightKeys, angular_velocity: [f64; 3]) -> ([f64; 3], bool) {
    let frame = SeatFrame::of_orientation(0).unwrap();
    let turn = turning(&frame, &keys, angular_velocity, [INERTIA; 3], TICK);
    (turn.torque, turn.still)
}

#[test]
fn w_a_s_d_q_and_e_each_give_the_seats_torque_about_their_own_axis() {
    let frame = SeatFrame::of_orientation(0).unwrap();
    let large = INERTIA * SEAT_ANGULAR_ACCELERATION;
    let cases = [
        // (keys, the axis the torque is about)
        (
            FlightKeys {
                pitch_down: 1,
                ..FlightKeys::default()
            },
            scaled(frame.right, -1.0),
        ),
        (
            FlightKeys {
                pitch_down: -1,
                ..FlightKeys::default()
            },
            frame.right,
        ),
        (
            FlightKeys {
                yaw_right: 1,
                ..FlightKeys::default()
            },
            scaled(frame.up, -1.0),
        ),
        (
            FlightKeys {
                yaw_right: -1,
                ..FlightKeys::default()
            },
            frame.up,
        ),
        (
            FlightKeys {
                roll_right: 1,
                ..FlightKeys::default()
            },
            frame.forward,
        ),
        (
            FlightKeys {
                roll_right: -1,
                ..FlightKeys::default()
            },
            scaled(frame.forward, -1.0),
        ),
    ];
    for (keys, axis) in cases {
        let (torque, still) = torque_for(keys, [0.0; 3]);
        assert!(close(torque, scaled(axis, large)), "{keys:?} {torque:?}");
        assert!(!still);
    }
    // Two keys together turn about both.
    let (torque, _) = torque_for(
        FlightKeys {
            pitch_down: 1,
            roll_right: 1,
            ..FlightKeys::default()
        },
        [0.0; 3],
    );
    assert!(close(
        torque,
        scaled(sum(&[scaled(frame.right, -1.0), frame.forward]), large)
    ));
}

#[test]
fn a_key_held_stops_turning_the_ship_faster_at_the_most_turn_rate() {
    let frame = SeatFrame::of_orientation(0).unwrap();
    let pitching_down = FlightKeys {
        pitch_down: 1,
        ..FlightKeys::default()
    };
    // Pitching down is a turn about minus the right hand: at the most rate there is no more torque that way, and below it there is.
    let at_the_most = scaled(frame.right, -MOST_TURN_RATE);
    let (torque, _) = torque_for(pitching_down, at_the_most);
    assert!(close(torque, [0.0; 3]), "{torque:?}");
    let below = scaled(frame.right, -0.9 * MOST_TURN_RATE);
    assert!(torque_for(pitching_down, below).0.iter().any(|n| *n != 0.0));
    // Turning the other way is not limited: it brakes at full torque.
    let (torque, _) = torque_for(
        FlightKeys {
            pitch_down: -1,
            ..FlightKeys::default()
        },
        at_the_most,
    );
    assert!(close(
        torque,
        scaled(frame.right, INERTIA * SEAT_ANGULAR_ACCELERATION)
    ));
}

#[test]
fn with_no_key_down_the_seat_stops_the_turn_and_a_turn_that_is_nearly_nothing_is_nothing() {
    let frame = SeatFrame::of_orientation(0).unwrap();
    // A fast turn is braked at full torque the other way.
    let (torque, still) = torque_for(FlightKeys::default(), scaled(frame.up, 0.8));
    assert!(close(
        torque,
        scaled(frame.up, -INERTIA * SEAT_ANGULAR_ACCELERATION)
    ));
    assert!(!still);
    // A slow one is braked by just what stops it in a tick: its inertia times the rate over the tick.
    let (torque, still) = torque_for(FlightKeys::default(), scaled(frame.forward, 0.01));
    assert!(close(torque, scaled(frame.forward, -INERTIA * 0.01 / TICK)));
    assert!(!still);
    // A key on one axis leaves the others to be braked.
    let (torque, _) = torque_for(
        FlightKeys {
            roll_right: 1,
            ..FlightKeys::default()
        },
        scaled(frame.up, 0.8),
    );
    assert!(close(
        torque,
        sum(&[
            scaled(frame.forward, INERTIA * SEAT_ANGULAR_ACCELERATION),
            scaled(frame.up, -INERTIA * SEAT_ANGULAR_ACCELERATION)
        ])
    ));
    // Below the still rate, with no key, the ship is still (so that it can go on a rail), and no torque is asked.
    let (torque, still) = torque_for(FlightKeys::default(), scaled(frame.up, STILL_RATE * 0.5));
    assert!(still && torque == [0.0; 3]);
    let (torque, still) = torque_for(FlightKeys::default(), [0.0; 3]);
    assert!(still && torque == [0.0; 3]);
    // With a key down it is never still.
    assert!(
        !torque_for(
            FlightKeys {
                yaw_right: 1,
                ..FlightKeys::default()
            },
            [0.0; 3]
        )
        .1
    );
}

#[test]
fn the_readout_says_the_throttle_the_turn_the_distance_the_speed_and_the_orbit() {
    let readout = Readout {
        throttle: 0.45,
        engines: 3,
        acceleration: 38.24,
        turn_rate: 0.123,
        body: Some(3),
        distance: 63_845.4,
        speed: 7_912.6,
        speed_away: -12.34,
        orbit: OrbitSummary::Bound {
            periapsis: 63_710.0,
            apoapsis: 71_203.4,
            period: 5_432.1,
        },
    };
    assert_eq!(
        readout.lines(),
        [
            "throttle 45 %, 3 engines, 38.2 m/s2",
            "turning 0.12 rad/s",
            "planet 3: 63,845 m from its middle, 7,913 m/s, -12.3 m/s away",
            "orbit: periapsis 63,710 m, apoapsis 71,203 m, period 5,432 s",
        ]
    );
    let escaping = Readout {
        orbit: OrbitSummary::Escape {
            periapsis: 1_234_567.8,
        },
        throttle: 0.0,
        engines: 0,
        acceleration: 0.0,
        ..readout.clone()
    };
    let lines = escaping.lines();
    assert_eq!(lines[0], "throttle 0 %, no engines");
    assert_eq!(lines[3], "orbit: escape, periapsis 1,234,568 m");
    let nowhere = Readout {
        body: None,
        orbit: OrbitSummary::None,
        ..readout
    };
    let lines = nowhere.lines();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[2], "no planet pulls here");
}

/// `G M` of a planet of these tests, in voxels cubed a second squared.
const MU: f64 = 6.674e11;

fn orbit_of(place: [f64; 3], velocity: [f64; 3]) -> Result<Orbit, world_storage::rail::RailError> {
    Orbit::from_place(
        Centre::Grid(GridId::new(GridId::FIRST_ALLOCATED)),
        place,
        velocity,
        MU,
        RailTime::of_tick(0, 20),
    )
}

#[test]
fn the_orbit_a_state_is_on_is_summed_up_as_a_closed_orbit_an_escape_or_none() {
    let circular = (MU / 70_000.0).sqrt();
    // A circle: the periapsis and the apoapsis are the distance, and the period is that of Kepler's third law.
    let OrbitSummary::Bound {
        periapsis,
        apoapsis,
        period,
    } = OrbitSummary::of(&orbit_of([70_000.0, 0.0, 0.0], [0.0, 0.0, circular]))
    else {
        panic!("a circle is closed");
    };
    assert!(
        (periapsis / 70_000.0 - 1.0).abs() < 1e-12 && (apoapsis / 70_000.0 - 1.0).abs() < 1e-12
    );
    let kepler = std::f64::consts::TAU * (70_000.0_f64 * 70_000.0 * 70_000.0 / MU).sqrt();
    assert!((period / kepler - 1.0).abs() < 1e-12, "{period} {kepler}");
    // An ellipse with a faster ship: the nearest point is where it is and the farthest is farther.
    let OrbitSummary::Bound {
        periapsis,
        apoapsis,
        ..
    } = OrbitSummary::of(&orbit_of([70_000.0, 0.0, 0.0], [0.0, 0.0, 1.2 * circular]))
    else {
        panic!("closed");
    };
    assert!((periapsis / 70_000.0 - 1.0).abs() < 1e-9 && apoapsis > 100_000.0);
    // Faster than escape: an escape that passes the planet at its nearest distance.
    let OrbitSummary::Escape { periapsis } =
        OrbitSummary::of(&orbit_of([70_000.0, 0.0, 0.0], [0.0, 0.0, 2.0 * circular]))
    else {
        panic!("an escape");
    };
    assert!((periapsis / 70_000.0 - 1.0).abs() < 1e-9, "{periapsis}");
    // Straight at the middle there is no orbit.
    assert_eq!(
        OrbitSummary::of(&orbit_of([70_000.0, 0.0, 0.0], [-100.0, 0.0, 0.0])),
        OrbitSummary::None
    );
}

#[test]
fn how_far_and_how_fast_and_toward_or_away_from_the_planet() {
    assert_eq!(
        distance_speed_and_speed_away([3.0, 4.0, 0.0], [0.0, 0.0, 10.0]),
        (5.0, 10.0, 0.0)
    );
    // Going straight out at 5, and straight in at 5.
    assert_eq!(
        distance_speed_and_speed_away([3.0, 4.0, 0.0], [3.0, 4.0, 0.0]),
        (5.0, 5.0, 5.0)
    );
    assert_eq!(
        distance_speed_and_speed_away([3.0, 4.0, 0.0], [-3.0, -4.0, 0.0]),
        (5.0, 5.0, -5.0)
    );
    // On the middle there is no away.
    assert_eq!(
        distance_speed_and_speed_away([0.0; 3], [1.0, 0.0, 0.0]),
        (0.0, 1.0, 0.0)
    );
}

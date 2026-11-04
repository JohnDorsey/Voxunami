//! The time an orbit starts at (E16 phase B of `docs/orbits-and-thrust-plan.md`): an orbit made from a state starts at the time of the state, a hand-over at the instant of the crossing, and
//! the rail saves it, so that a ship that was parked or a server that was off can find the first sphere crossing **after** it. An orbit made from elements has none, and its bytes are what they were.

use voxel_space::GridId;
use world_storage::rail::{Centre, Orbit, Rail, RailTime, decode_rail, encode_rail};

fn rail_of(orbit: Orbit) -> Rail {
    Rail {
        spin: None,
        orbit: Some(orbit),
    }
}

fn an_ellipse() -> Orbit {
    Orbit::elliptical(
        Centre::Grid(GridId::new(9)),
        [0.0, 0.0, 1.0],
        1.0e6,
        0.5,
        0.25,
        1000.0,
        0.75,
    )
    .unwrap()
}

#[test]
fn an_orbit_made_from_elements_starts_nowhere_and_keeps_the_bytes_it_had() {
    let orbit = an_ellipse();
    assert_eq!(orbit.since, None);
    let bytes = encode_rail(&rail_of(orbit.clone()));
    assert_eq!(bytes[0], 2, "only the flag of an orbit");
    assert_eq!(decode_rail(&bytes).unwrap(), rail_of(orbit));
}

#[test]
fn a_start_is_the_flag_and_a_wide_signed_number_and_the_units_in_a_second_after_the_orbit() {
    let plain = encode_rail(&rail_of(an_ellipse()));
    // The tail by hand: 4,000,000 units at 20 a second. A zigzag of 8,000,000 is 0x7A1200: a length of 3 and the three bytes little-endian; then the units in a second as a varint (20).
    let at_tick = encode_rail(&rail_of(
        an_ellipse().starting_at(RailTime::of_tick(4_000_000, 20)),
    ));
    assert_eq!(at_tick[0], 2 | 4);
    assert_eq!(&at_tick[1..plain.len()], &plain[1..]);
    assert_eq!(&at_tick[plain.len()..], &[3, 0x00, 0x12, 0x7A, 20]);
    // A time before the world began, at 2^20 units a second (1,048,576 is the varint 0x80 0x80 0x40): -1 unit is a zigzag of 1.
    let before = encode_rail(&rail_of(
        an_ellipse().starting_at(RailTime::from(-0.000_000_95)),
    ));
    assert_eq!(&before[plain.len()..], &[1, 1, 0x80, 0x80, 0x40]);
    // The very start of the world: no bytes of units, a length of 0.
    let zero = encode_rail(&rail_of(an_ellipse().starting_at(RailTime::of_tick(0, 20))));
    assert_eq!(&zero[plain.len()..], &[0, 20]);
}

#[test]
fn a_start_comes_back_whatever_it_is() {
    for since in [
        RailTime::of_tick(0, 20),
        RailTime::of_tick(4_000_000, 20),
        RailTime::of_tick(u64::MAX, u32::MAX),
        RailTime::from(-123.456),
        RailTime::from(1.0e15),
        RailTime::of_half_ticks(7, 20),
    ] {
        let rail = rail_of(an_ellipse().starting_at(since));
        assert_eq!(decode_rail(&encode_rail(&rail)).unwrap(), rail, "{since:?}");
    }
}

#[test]
fn a_start_with_no_orbit_or_no_units_in_a_second_or_beyond_the_clock_is_not_read() {
    let with_start = encode_rail(&rail_of(an_ellipse().starting_at(RailTime::of_tick(5, 20))));
    let mut bytes = with_start.clone();
    assert!(decode_rail(&bytes).is_ok());
    // The flag of a start on a rail that has no orbit.
    assert!(decode_rail(&[4]).is_err());
    assert!(decode_rail(&[1 | 4]).is_err());
    // No units in a second: the varint at the end is 0.
    *bytes.last_mut().unwrap() = 0;
    assert!(decode_rail(&bytes).is_err());
    // The units in a second too many for 32 bits: 2^32 as a varint is 0x80 0x80 0x80 0x80 0x10.
    let mut bytes = with_start[..with_start.len() - 1].to_vec();
    bytes.extend_from_slice(&[0x80, 0x80, 0x80, 0x80, 0x10]);
    assert!(decode_rail(&bytes).is_err());
    // A time beyond 2^96 units: a length of 13 bytes of zigzag (2^96 is 13 bytes and a zigzag of twice it has the 13th byte 2).
    let mut bytes = with_start[..with_start.len() - 3].to_vec();
    bytes.push(13);
    bytes.extend_from_slice(&[0; 12]);
    bytes.push(2);
    bytes.push(20);
    assert!(decode_rail(&bytes).is_err());
    // (And one just inside: the zigzag of 2^96 - 1 is 2^97 - 2, which is 13 bytes too, with the 13th byte 1.)
    let mut inside = with_start[..with_start.len() - 3].to_vec();
    inside.push(13);
    inside.extend_from_slice(&[
        0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    ]);
    inside.push(1);
    inside.push(20);
    assert!(decode_rail(&inside).is_ok());
}

#[test]
fn an_orbit_made_from_a_state_starts_at_the_time_of_the_state() {
    let circular = libm::sqrt(1.0e12 / 1.0e6);
    for tick in [0, 1, 123_456_789] {
        let orbit = Orbit::from_place(
            Centre::Grid(GridId::new(9)),
            [1.0e6, 0.0, 0.0],
            [0.0, circular, 0.0],
            1.0e12,
            RailTime::of_tick(tick, 20),
        )
        .unwrap();
        assert_eq!(orbit.since, Some(RailTime::of_tick(tick, 20)));
    }
}

#[test]
fn a_place_in_a_path_is_the_flag_and_two_varints_after_the_start() {
    let plain = encode_rail(&rail_of(an_ellipse()));
    let mut orbit = an_ellipse().starting_at(RailTime::of_tick(5, 20));
    orbit.changes = 3;
    orbit.predicted = 300;
    let bytes = encode_rail(&rail_of(orbit.clone()));
    assert_eq!(bytes[0], 2 | 4 | 8);
    // The start (a length of 1, the zigzag of 5 which is 10, and 20 units a second), then 3 and 300 as varints (300 is 0xAC 0x02).
    assert_eq!(&bytes[plain.len()..], &[1, 10, 20, 3, 0xAC, 0x02]);
    assert_eq!(decode_rail(&bytes).unwrap(), rail_of(orbit));
    // Without a start: the flag of the path, and the two numbers.
    let mut orbit = an_ellipse();
    orbit.predicted = 1;
    let bytes = encode_rail(&rail_of(orbit.clone()));
    assert_eq!(bytes[0], 2 | 8);
    assert_eq!(&bytes[plain.len()..], &[0, 1]);
    assert_eq!(decode_rail(&bytes).unwrap(), rail_of(orbit));
    // The flag is not there for an orbit that is at the start of a path with nothing asked for, and a rail that has it without either is not read.
    assert_eq!(encode_rail(&rail_of(an_ellipse())), plain);
    let mut empty = plain.clone();
    empty[0] |= 8;
    empty.extend_from_slice(&[0, 0]);
    assert!(decode_rail(&empty).is_err());
    // A path with no orbit, or a count of changes too many for 32 bits (2^32 is the varint 0x80 0x80 0x80 0x80 0x10).
    assert!(decode_rail(&[8]).is_err());
    let mut too_many = plain.clone();
    too_many[0] |= 8;
    too_many.extend_from_slice(&[0x80, 0x80, 0x80, 0x80, 0x10, 1]);
    assert!(decode_rail(&too_many).is_err());
    let mut fine = plain;
    fine[0] |= 8;
    fine.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0x0F, 1]);
    assert_eq!(decode_rail(&fine).unwrap().orbit.unwrap().changes, u32::MAX);
}

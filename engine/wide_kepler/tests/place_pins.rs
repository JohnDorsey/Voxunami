//! The place and the speed of a grid of orbits at a grid of times, pinned to the bit: a hash of every number (the exact value and the width, as `Debug` writes them) of
//! every `Place`. The reference rows (`orbits.txt`, `fit.txt`) and the invariants say the answer is right to the width asked for; this says it is **the same answer** after
//! a change that is meant to make no difference to it (what is worked out once for an orbit and what is worked out for a time, a number kept instead of worked out
//! again). A change that is meant to change the bits (a different working width, a different stopping rule) re-pins it, with the reason in the commit.
//!
//! The pin is made from the program as it was before the prepared form (2026-11-03).

use wide_float::{Precision, WideFloat};
use wide_kepler::{Elements, Place, Time};

fn at(bits: u32) -> Precision {
    Precision::of(bits)
}

fn wide(value: f64, precision: Precision) -> WideFloat {
    WideFloat::from_f64(value, precision).unwrap()
}

/// A number of `precision` bits that is not a double: a double and a bit far below it.
fn with_a_low_bit(value: f64, below: i64, precision: Precision) -> WideFloat {
    &wide(value, precision)
        + &WideFloat::from_i64(1, precision)
            .times_power_of_two(below)
            .unwrap()
}

struct Case {
    name: &'static str,
    elements: Elements,
}

fn elements(
    normal: [f64; 3],
    radius: f64,
    ratio: f64,
    periapsis: f64,
    period: f64,
    phase: f64,
    precision: Precision,
) -> Elements {
    Elements {
        normal: normal.map(|component| wide(component, precision)),
        radius: wide(radius, precision),
        ratio: wide(ratio, precision),
        periapsis: wide(periapsis, precision),
        period: wide(period, precision),
        phase: wide(phase, precision),
    }
}

fn cases() -> Vec<Case> {
    let p128 = at(128);
    let p192 = at(192);
    let p256 = at(256);
    let mut cases = vec![
        Case {
            name: "a circle",
            elements: elements([0.0, 0.6, 0.8], 2000.0, 1.0, 0.0, 61.0, 1.0, p128),
        },
        Case {
            name: "an ellipse of e = 0.1",
            elements: elements([0.0, 0.6, 0.8], 2000.0, 0.9, 0.3, 61.0, 1.0, p128),
        },
        Case {
            name: "an ellipse of e = 0.5, the other way round",
            elements: elements([0.3, -0.5, 0.2], 50000.0, 0.5, -2.0, -300.0, 4.0, p128),
        },
        Case {
            name: "a thin ellipse, ratio 2^-20",
            elements: elements(
                [0.0, 0.0, 1.0],
                7.5e4,
                1.0 / 1048576.0,
                1.0,
                3600.0,
                0.0,
                p128,
            ),
        },
        Case {
            name: "the thinnest ellipse, ratio 2^-64",
            elements: elements(
                [0.1, 0.2, 0.3],
                1.0e6,
                1.0 / 18446744073709551616.0,
                3.0,
                86400.0,
                -1.5,
                p128,
            ),
        },
        Case {
            name: "a normal along x, where the plane starts toward z",
            elements: elements([0.95, 0.1, 0.1], 900.0, 0.7, 0.5, 30.0, 0.25, p128),
        },
        Case {
            name: "a short period",
            elements: elements([0.0, 1.0, 0.0], 12.0, 0.8, 0.1, 0.00048828125, 2.0, p128),
        },
        Case {
            name: "a very wide and slow orbit, 2^119 voxels and 2^79 seconds",
            elements: elements(
                [1.0, 1.0, 1.0],
                f64::from_bits((1023 + 119) << 52),
                0.6,
                0.7,
                f64::from_bits((1023 + 79) << 52),
                0.9,
                p128,
            ),
        },
    ];
    // Numbers that are not doubles, at wider widths.
    for (name, precision) in [("192 bits", p192), ("256 bits", p256)] {
        let mut wide_elements = elements([0.2, 0.7, 0.6], 4000.0, 0.8, 0.4, 100.0, 0.3, precision);
        wide_elements.radius = with_a_low_bit(4000.0, -90, precision);
        wide_elements.ratio = with_a_low_bit(0.8, -100, precision);
        wide_elements.period = with_a_low_bit(100.0, -80, precision);
        wide_elements.phase = with_a_low_bit(0.3, -120, precision);
        wide_elements.periapsis = with_a_low_bit(0.4, -110, precision);
        cases.push(Case {
            name,
            elements: wide_elements,
        });
    }
    cases
}

const TIMES: [Time; 8] = [
    Time {
        units: 0,
        units_per_second: 20,
    },
    Time {
        units: 1,
        units_per_second: 20,
    },
    Time {
        units: 7777,
        units_per_second: 20,
    },
    Time {
        units: -123456,
        units_per_second: 20,
    },
    Time {
        units: 1_234_567_891,
        units_per_second: 20,
    },
    Time {
        units: 3_000_000_000_000,
        units_per_second: 1000,
    },
    Time {
        units: (1 << 90) + 12345,
        units_per_second: 20,
    },
    Time {
        units: -(1 << 95) + 1,
        units_per_second: 60,
    },
];

const PRECISIONS: [u32; 3] = [128, 192, 320];

/// FNV-1a, 64 bits.
fn hash_of(text: &str, mut hash: u64) -> u64 {
    for byte in text.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn describe(place: &Place) -> String {
    format!("{:?}|{:?}", place.position, place.velocity)
}

/// The hash of every place of every case at every time and width, and how many places that is.
fn digest(mut place_of: impl FnMut(&Elements, Time, Precision) -> Place) -> (u64, usize) {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut count = 0;
    for case in cases() {
        for time in TIMES {
            for bits in PRECISIONS {
                let place = place_of(&case.elements, time, at(bits));
                hash = hash_of(&format!("{}@{bits}:", case.name), hash);
                hash = hash_of(&describe(&place), hash);
                count += 1;
            }
        }
    }
    (hash, count)
}

/// The hash of the places as the program worked them out before the prepared form.
const PLACES_PIN: u64 = 7857403725755386156;
const PLACES_COUNT: usize = 10 * 8 * 3;

#[test]
fn the_places_of_a_grid_of_orbits_and_times_are_the_same_to_the_bit() {
    let (hash, count) = digest(|elements, time, precision| {
        elements
            .place_at(time, precision)
            .unwrap_or_else(|error| panic!("{elements:?} at {time:?}: {error}"))
    });
    assert_eq!(count, PLACES_COUNT);
    assert_eq!(hash, PLACES_PIN, "the places changed: {hash}");
}

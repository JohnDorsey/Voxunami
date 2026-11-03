use std::f64::consts::TAU;

use proptest::prelude::*;

use super::*;

const PLANES: [([f64; 3], [[i128; 3]; 2]); 8] = [
    (
        [0.0, 1.0, 0.0],
        [
            [85070591730234615865843651857942052864, 0, 0],
            [0, 0, -85070591730234615865843651857942052864],
        ],
    ),
    (
        [1.0, 0.0, 0.0],
        [
            [0, 0, 85070591730234615865843651857942052864],
            [0, -85070591730234615865843651857942052864, 0],
        ],
    ),
    (
        [0.0, 0.0, -1.0],
        [
            [85070591730234615865843651857942052864, 0, 0],
            [0, -85070591730234615865843651857942052864, 0],
        ],
    ),
    (
        [0.3333333333333333, 0.6666666666666666, 0.6666666666666666],
        [
            [
                80205323056001503848344590825875018976,
                -20051330764000375962086147706468754744,
                -20051330764000375962086147706468754744,
            ],
            [
                0,
                60153992292001127886258443119406264232,
                -60153992292001127886258443119406264232,
            ],
        ],
    ),
    (
        [0.6, 0.8, 0.0],
        [
            [
                68056473384187694959410833263783227425,
                -51042355038140766497191642078192311730,
                0,
            ],
            [0, 0, -85070591730234615865843651857942052864],
        ],
    ),
    (
        [0.2672612419124244, -0.5345224838248488, 0.8017837257372732],
        [
            [
                81976073384389514274609147990475057741,
                12611703597598386811478330460073085806,
                -18917555396397580217217495690109628710,
            ],
            [
                0,
                70783010888636242847624593342458952643,
                47188673925757495231749728894972635095,
            ],
        ],
    ),
    (
        [-0.9, 0.1, 0.42],
        [
            [
                35575209629434799276419442306623424980,
                -3952801069937200041504681356492406065,
                77173735174964384200760867535536990214,
            ],
            [
                9394475268860029060589296740631057322,
                84550277419740258937812983725676622006,
                0,
            ],
        ],
    ),
    (
        [0.001, -1.0, 0.003],
        [
            [
                85070549195353465837088323613370214679,
                85069783567301361895685932410072433,
                -255209350701904090999669620646910,
            ],
            [
                0,
                255210626745467494274250285714941054,
                85070208915155829653870633101846269824,
            ],
        ],
    ),
];

const TURNS_OF_RADIANS: [(f64, u128); 14] = [
    (0.0, 0x00000000000000000000000000000000),
    (0.7, 0x1c8543cce74be9982aea8a52ee0b8538),
    (-1.25, 0xcd1206ed878ab962e133b48e635f7ab3),
    (3.0, 0x7a3b2292bab30fdf7d1d81dd77e7a650),
    (5.5, 0xe01714b7ab9d9d19bab618c0b128b0e9),
    (1e-05, 0x00001ab3a71b007403c130e49154583b),
    (-1e-05, 0xffffe54c58e4ff8bfc3ecf1b6eaba7c4),
    (12345.678, 0xe02c8877e382f3710e8867829161e657),
    (-98765.4321, 0xfe473fd2275b2a8d0f78afbcdcc8b699),
    (1.152921504606847e+18, 0xa7f09d5f47d4d377036d8a5664f10e41),
    (-9.223372036854776e+18, 0xc07b1505c1596447e493ad4cd8778df7),
    (1e-300, 0x00000000000000000000000000000000),
    (TAU, 0xfffffffffffffd30ea0a394645e27a99),
    (0.5, 0x145f306dc9c882a53f84eafa3ea69bb8),
];

/// Units of 2^-126 that a coordinate of a direction of a plane may be out by (measured over the vectors above: 4 to 8).
const PLANE_TOLERANCE: i128 = 16;

proptest! {
    #[test]
    fn a_wide_product_is_the_product(first in any::<u128>(), second in any::<u128>()) {
        let wide = Wide::of_product(first, second);
        // The same product made the slow way, of digits of 32 bits.
        let digits = |value: u128| [0, 1, 2, 3].map(|digit| (value >> (32 * digit)) & 0xffff_ffff);
        let mut sums = [0u128; 8];
        for (i, a) in digits(first).iter().enumerate() {
            for (j, b) in digits(second).iter().enumerate() {
                sums[i + j] += a * b;
            }
        }
        let mut carry = 0u128;
        let mut result = [0u128; 8];
        for (slot, sum) in result.iter_mut().zip(sums) {
            let sum = sum + carry;
            *slot = sum & 0xffff_ffff;
            carry = sum >> 32;
        }
        let join = |slice: &[u128]| slice.iter().rev().fold(0u128, |all, digit| (all << 32) | digit);
        prop_assert_eq!(wide.low, join(&result[..4]));
        prop_assert_eq!(wide.high, join(&result[4..]));
    }

    #[test]
    fn shifting_wide_numbers_is_multiplying_and_dividing_by_powers_of_two(value in any::<u64>(), bits in 0u32..190) {
        let wide = Wide { high: 0, low: u128::from(value) };
        prop_assert_eq!(wide.shifted_left(bits).shifted_right(bits), wide);
        // A shift right drops the bits that fall off: the quotient rounded down.
        let shifted = wide.shifted_right(bits.min(127));
        prop_assert_eq!(shifted.high, 0);
        prop_assert_eq!(shifted.low, u128::from(value) >> bits.min(127));
    }

    #[test]
    fn a_product_of_fixed_numbers_is_the_product_rounded_to_the_nearest(
        first in -(1i128 << 63)..=(1i128 << 63),
        second in -(1i128 << 63)..=(1i128 << 63),
    ) {
        // Small enough that the product is exact in an i128 (below 2^126): the same by the plain route, half a unit away from nothing.
        let magnitude = (first * second).unsigned_abs();
        let expected = ((magnitude + (1 << 125)) >> 126) as i128;
        let expected = if (first < 0) != (second < 0) { -expected } else { expected };
        prop_assert_eq!(multiplied(first, second, 126), expected);
    }

    #[test]
    fn one_is_what_a_product_leaves_alone_and_a_half_halves(value in -ONE..=ONE) {
        prop_assert_eq!(product(value, ONE), value);
        prop_assert_eq!(product(value, -ONE), -value);
        let halved = (value.abs() + 1) >> 1;
        prop_assert_eq!(product(value, ONE / 2), if value < 0 { -halved } else { halved });
    }
}

#[test]
fn a_plane_is_the_two_unit_vectors_at_right_angles_worked_out_at_120_digits() {
    for (normal, [first, second]) in PLANES {
        let (found_first, found_second) = plane(normal);
        for axis in 0..3 {
            assert!(
                (found_first[axis] - first[axis]).abs() <= PLANE_TOLERANCE
                    && (found_second[axis] - second[axis]).abs() <= PLANE_TOLERANCE,
                "{normal:?}: found {found_first:?} {found_second:?}, expected {first:?} {second:?}"
            );
        }
    }
}

#[test]
fn the_angle_in_radians_is_a_fraction_of_a_turn_to_the_last_of_128_bits() {
    for (radians, turns) in TURNS_OF_RADIANS {
        let found = turns_of_radians(radians);
        // (A negative angle is made from the whole part rounded down, so it may be one more.)
        assert!(
            found.wrapping_sub(turns) <= 1 || turns.wrapping_sub(found) <= 1,
            "{radians}: found {found:#x}, expected {turns:#x}"
        );
    }
}

#[test]
fn a_fraction_of_a_turn_is_worked_out_to_the_last_bit() {
    assert_eq!(fraction_in_turns(0, 7), 0);
    assert_eq!(fraction_in_turns(1, 2), 1 << 127);
    assert_eq!(fraction_in_turns(3, 4), 3 << 126);
    assert_eq!(
        fraction_in_turns(1, 3),
        0x5555_5555_5555_5555_5555_5555_5555_5555
    );
    assert_eq!(
        fraction_in_turns(2, 3),
        0xaaaa_aaaa_aaaa_aaaa_aaaa_aaaa_aaaa_aaaa
    );
    // The largest denominator there is, about 2^112: (d - 1) / d is 1 - 1/d, so 2^128 less the 2^128 / d rounded up, which is 2^16 + 1.
    let denominator = (1u128 << 112) - 3;
    assert_eq!(
        fraction_in_turns(denominator - 1, denominator),
        0u128.wrapping_sub((1 << 16) + 1)
    );
}

use std::f64::consts::TAU;

use proptest::prelude::*;

use super::*;

const SINE_AND_COSINE: [(u128, i128, i128); 48] = [
    (
        0x00000000000000000000000000000000,
        0,
        85070591730234615865843651857942052864,
    ),
    (
        0x00000000000000000000000000000001,
        2,
        85070591730234615865843651857942052864,
    ),
    (
        0x40000000000000000000000000000000,
        85070591730234615865843651857942052864,
        0,
    ),
    (
        0x80000000000000000000000000000000,
        0,
        -85070591730234615865843651857942052864,
    ),
    (
        0xc0000000000000000000000000000000,
        -85070591730234615865843651857942052864,
        0,
    ),
    (
        0xffffffffffffffffffffffffffffffff,
        -2,
        85070591730234615865843651857942052864,
    ),
    (
        0x20000000000000000000000000003039,
        60153992292001127886258443119406277944,
        60153992292001127886258443119406250520,
    ),
    (
        0x0fffffffffffffffffffffffffffffff,
        32555106036655403880339071844407556997,
        78594978518187694043992111500493281377,
    ),
    (
        0x2120fc8ccee55fe90cc65817272b53a4,
        61797333081453837144630760275254103958,
        58464478115793639666305234784981132867,
    ),
    (
        0x6889c91e3005a70699363c9930cf43ac,
        46324198744989412780861715415636206117,
        -71351763734101090038453664983420287836,
    ),
    (
        0x071674ee0693e0d93a31aadc19e3ecbf,
        14724256898015088343116843187787603665,
        83786644736100498072489977845165530052,
    ),
    (
        0x8de424016b820fe46be1e08a7e114e68,
        -28445377652620341779231563802514429157,
        -80173973754155839949148210007458222642,
    ),
    (
        0x141337acf1e2b4410a727244d2a65227,
        40240162278369225556400999739788436485,
        74951550465235691961215471985299030881,
    ),
    (
        0xc6c4ee2e3b9efad56ede80a3ba89e31d,
        -83899172520569374422778915153270292788,
        14068917076164710187763281896480535088,
    ),
    (
        0xe25c7c4ae65cb5dbbe6fa6127ea696c3,
        -56568768120706466749064929380275378909,
        63537233577155418768705442522353521925,
    ),
    (
        0x62734460417ae7fb3500f48ab489cb10,
        56429858054740846438918952830562258156,
        -63660636952940251179594715425792859194,
    ),
    (
        0x7c3c2f9f335b53e52c442a3ad00f1436,
        7849724080765291866179627979677064077,
        -84707658503751098004781783859874075391,
    ),
    (
        0x213b6da5685f71ae53976270c78e175d,
        61945346131420680077401548919019601993,
        58307629603601167134300135816684888404,
    ),
    (
        0xf428d119e3dff1335e4c955942756822,
        -24375932209820409287153639987243262569,
        81503493828390584595717698620698125397,
    ),
    (
        0xd64a860802f385331674844a519aabec,
        -72653140116955136346316273116344038566,
        44255246112504519783500992442132609981,
    ),
    (
        0x36f2f503a871dff91e0e94dacdca2a59,
        82980184997280804870606458073839722839,
        18742851307880448629500708997385828748,
    ),
    (
        0xeb03a85e443884fcf18d6c4deb6af134,
        -41905181640303281484028730667448652406,
        74033514903896404106874082667823096462,
    ),
    (
        0x8a97bb29a59b03d79063204d4e6752c1,
        -21868676285383082411698864943562277757,
        -82211717989939766263873675279343697075,
    ),
    (
        0x1562fbd6451163b33056dfada05552be,
        42631676843021874231578656548712162261,
        73617563847796639086119601482864106216,
    ),
    (
        0xc0450c5c86166dec2641940b4df62868,
        -85068727697381354675745308654385040182,
        563156533340931800052572835728256907,
    ),
    (
        0x5db1608995ccb6a05298e4bb63fe5590,
        63461994095139251910761446456437701542,
        -56653163043212096870954593409192030323,
    ),
    (
        0x18b2bf46ee11e99295e30255b5d45d27,
        48467863187739807632674192112948874414,
        69913316437906152491690843766309949307,
    ),
    (
        0xb49f8d70989da94a91782a4f625e5555,
        -81775693880111883253494548315199804102,
        -23446566225323786166089912407929418427,
    ),
    (
        0xfd252688627fd848a52dd713b80a40b6,
        -5955961747004100209667705362051850567,
        84861841230322631272704777901375335279,
    ),
    (
        0xc83e8baf538499d9c02aedeba2f6aa9e,
        -83334964258124242599448526421756637247,
        17096470671732725147810898173111193887,
    ),
    (
        0xa087cc556f4be6e85a875486e99ed6ae,
        -60932046115354537620988288116324150877,
        -59365742086902023458689168394005724556,
    ),
    (
        0x24adba25e931a56f9aa80f8a49b4fa90,
        66650181871940301305458480893253960009,
        52865478658284582369122038946276235396,
    ),
    (
        0xe02071366047b6333e60ca3809906d54,
        -59966601022175808115673176332829994989,
        60340801612005831383312362949686028550,
    ),
    (
        0x564b1dd7cdee32a283ff7fe3e1f0859d,
        72650623887640667872220234634255808706,
        -44259376702217998858042468825569812116,
    ),
    (
        0x9cb9557b327bd25bcbd2d4944ca05421,
        -55128106844962374158535085091636618904,
        -64791183142636582375875219289045785714,
    ),
    (
        0xa24590ab86a403fd0f276f7fd5562bfa,
        -63412772333018840866060907333215369543,
        -56708252330441135258345545642769519724,
    ),
    (
        0x3892543b0fdf7bc09e6c6d9e859c2fba,
        83660599455964149628337927249501870213,
        15424320925116711445437928054672599557,
    ),
    (
        0x219b5d8ff50bb9c4792677bc063764fd,
        62479023765655839853956351813529892183,
        57735406525137371471292255681812416133,
    ),
    (
        0x20b98c04b7d04b528b1e1bc8cd7cbf52,
        61214503015081512135713277267875403868,
        59074446234804763288513480556890013183,
    ),
    (
        0x3060d10a946fe4ff4c673ab53e2154ad,
        78893770481225441927058554978865516775,
        31824181946877806827680038588941093101,
    ),
    (
        0x9377d946442173b3f23a8ffaf0634524,
        -39119276548807138042411684534674794959,
        -75542622271339046395931484293814103116,
    ),
    (
        0xafa98c2177f38f493985f9e7ac6c785b,
        -78322447571704803954763322173165594364,
        -33205417987277535132674149575302975520,
    ),
    (
        0x15a7db7df35032d8a27d336dc4fc4fea,
        43116853832252972965678417025256514243,
        73334456382660902172305940315479276491,
    ),
    (
        0x523798b489be84049874971fdca0574d,
        76707938809502882932388253916202544249,
        -36781757719416069921850044829699615876,
    ),
    (
        0x620149105780c65f88784399d488568f,
        57122149238089220106083744717524715411,
        -63040190702072954189964004572572568602,
    ),
    (
        0xfb6c68d5b547b18267611e61fedb3279,
        -9535459043380703466836419514277447356,
        84534493540591294971473951200368932176,
    ),
    (
        0xa704baad5793637eb03c81bc6773b421,
        -69574626543122507894514823308743027162,
        -48952802970997434000501938372524647712,
    ),
    (
        0x483f418a59675f136681d9241dd87b27,
        83333799689461257490855828297043138651,
        -17102146258555065887789492013546615604,
    ),
];

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

/// Units of 2^-126 that a sine or cosine may be out by. Each of the 127 steps rounds twice by at most half a unit, so a hundred units is the most they can add up to;
/// measured over the vectors above: 22.
const TOLERANCE: i128 = 100;

/// Units of 2^-126 that a coordinate of a direction of a plane may be out by (measured over the vectors above: 4 to 8).
const PLANE_TOLERANCE: i128 = 16;

#[test]
fn the_sine_and_cosine_are_those_worked_out_at_120_digits_to_a_few_of_126_bits() {
    for (turns, sine, cosine) in SINE_AND_COSINE {
        let found = sine_and_cosine(turns);
        assert!(
            (found.0 - sine).abs() <= TOLERANCE && (found.1 - cosine).abs() <= TOLERANCE,
            "{turns:#x}: found {found:?}, expected {sine} {cosine}"
        );
    }
}

#[test]
fn the_whole_and_quarter_turns_are_nothing_and_one_to_a_few_bits() {
    let (sine, cosine) = sine_and_cosine(0);
    assert!(sine.abs() <= TOLERANCE && (cosine - ONE).abs() <= TOLERANCE);
    let (sine, cosine) = sine_and_cosine(1 << 126);
    assert!((sine - ONE).abs() <= TOLERANCE && cosine.abs() <= TOLERANCE);
    let (sine, cosine) = sine_and_cosine(1 << 127);
    assert!(sine.abs() <= TOLERANCE && (cosine + ONE).abs() <= TOLERANCE);
}

proptest! {
    #[test]
    fn the_sine_and_cosine_of_any_angle_make_a_unit_vector(turns in any::<u128>()) {
        let (sine, cosine) = sine_and_cosine(turns);
        let size = product(sine, sine) + product(cosine, cosine);
        prop_assert!((size - ONE).abs() <= 2 * TOLERANCE, "{turns:#x}: {}", size - ONE);
    }

    #[test]
    fn a_quarter_turn_more_turns_a_sine_into_a_cosine(turns in any::<u128>()) {
        let (sine, cosine) = sine_and_cosine(turns);
        let (later_sine, later_cosine) = sine_and_cosine(turns.wrapping_add(1 << 126));
        prop_assert!((later_sine - cosine).abs() <= 2 * TOLERANCE);
        prop_assert!((later_cosine + sine).abs() <= 2 * TOLERANCE);
    }

    #[test]
    fn the_sine_and_cosine_of_double_an_angle_follow_from_those_of_the_angle(turns in any::<u128>()) {
        // sin 2x = 2 sin x cos x and cos 2x = cos^2 x - sin^2 x, with x a half of an angle.
        let (sine, cosine) = sine_and_cosine(turns);
        let (half_sine, half_cosine) = sine_and_cosine(turns >> 1);
        prop_assert!((2 * product(half_sine, half_cosine) - sine).abs() <= 4 * TOLERANCE);
        prop_assert!(
            (product(half_cosine, half_cosine) - product(half_sine, half_sine) - cosine).abs()
                <= 4 * TOLERANCE
        );
    }

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
        let negative = wide.negated();
        prop_assert_eq!(negative.shifted_left(bits).shifted_right(bits), negative);
    }

    #[test]
    fn a_negative_wide_number_shifted_right_is_rounded_down(value in 1u64.., bits in 0u32..64) {
        let rounded_up = (u128::from(value) + (1 << bits) - 1) >> bits;
        let expected = Wide { high: u128::MAX, low: 0u128.wrapping_sub(rounded_up) };
        prop_assert_eq!(Wide { high: 0, low: u128::from(value) }.negated().shifted_right(bits), expected);
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

#[test]
fn bricks_and_a_rest_are_the_unit_times_the_radius_rounded_to_a_step_of_2_to_the_minus_32() {
    // Half of a radius of 100 voxels is 50 voxels: a brick and 18.
    assert_eq!(scaled(ONE / 2, 25, 2), (1, 18.0));
    // Minus 50 voxels is two bricks back and 14 forward.
    assert_eq!(scaled(-ONE / 2, 25, 2), (-2, 14.0));
    assert_eq!(scaled(ONE / 4, 1, 0), (0, 0.25));
    // Less than half a step is nothing, and half a step is a step.
    assert_eq!(scaled(ONE >> 40, 1, 0), (0, 0.0));
    assert_eq!(scaled(ONE >> 33, 1, 0), (0, 1.0 / 4_294_967_296.0));
    // A radius of 2^-1000 voxel is nothing, whichever way it points.
    assert_eq!(scaled(-ONE, 1, -1000), (0, 0.0));
    // The largest radius an orbit has, 2^120 voxels, straight along: 2^115 bricks.
    assert_eq!(scaled(ONE, 1, 120), (1 << 115, 0.0));
    assert_eq!(scaled(-ONE, 1, 120), (-(1 << 115), 0.0));
}

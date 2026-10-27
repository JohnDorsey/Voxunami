//! The circle of an orbit in 128-bit fixed point, so that a body a long way from its centre is where its orbit puts it to a small fraction of a voxel.
//!
//! An `f64` has 53 bits: a circle of radius 2.5x10^20 voxels made of `f64` is 32 km out in each coordinate, and one of 10^18 is 128 voxels out. Here the angle is a
//! fraction of a turn in 128 bits, the sine and cosine and the two directions of the plane have 126 bits after the point, and the radius (an `f64`, so a whole number times a
//! power of two) multiplies them in 256 bits; what comes out is whole bricks (an `i128`) and a rest below a brick in steps of 2^-32 voxel. Everything is whole numbers
//! (the `f64` is only a first guess for a square root that is then corrected in whole numbers), so the same bits come out on every machine, which an orbit that is
//! simulation must give; and nothing is a table of an unknown source: the constants below are made, and the values the tests check against are worked out at 120 digits, by
//! `engine/world_storage/tools/exact_circle_reference.py` (it needs `mpmath`).
//!
//! What it costs: about 130 steps of shifts and adds for a sine and cosine, and some dozens of 128-bit products; microseconds for an orbit.

/// One, with 126 bits after the point.
const ONE: i128 = 1 << 126;

/// The sort of number a coordinate of a unit vector is held as: `value x 2^-126`.
pub(crate) type Fixed = i128;

/// The angle whose tangent is `2^-step`, step by step, in turns with 128 bits after the point, rounded to the nearest.
const ANGLES: [u128; 127] = [
    0x20000000000000000000000000000000,
    0x12e4051d9df308665688f6dae35195b0,
    0x09fb385b5ee39e8ddf43f3ca0921e0d2,
    0x051111d41ddd9a1b7f9255cb1f1e296a,
    0x028b0d430e589aecc0cc001229b69dbd,
    0x0145d7e159046278569c94de82daf73a,
    0x00a2f61e5c28262984d6bf58b4b45a49,
    0x00517c5511d442aea2c306cadeaa9b1f,
    0x0028be5346d0c336fc917a6eb1ec3cab,
    0x00145f2ebb30ab37b9341f2d438ee842,
    0x000a2f980091ba7b67f43a922119c803,
    0x000517cc14a80cb70788f0039766abd2,
    0x00028be60cdfec61994b7615dea651dd,
    0x000145f306c172f246af4bf9fd24879d,
    0x0000a2f9836ae91158539db461f393e1,
    0x0000517cc1b6ba7bb2f723fe09adc491,
    0x000028be60db85fc3a56ab54e7901551,
    0x0000145f306dc815e946c44abb5cc791,
    0x00000a2f9836e4adee26d05512fae91c,
    0x00000517cc1b726b5643d5f35d89d4a8,
    0x0000028be60db9383707f8b2e0318cd3,
    0x00000145f306dc9c6d00be1096fdb36d,
    0x000000a2f9836e4e40aff73f3061321d,
    0x000000517cc1b727219deea674cd11f1,
    0x00000028be60db9390f7b5b415fa1984,
    0x000000145f306dc9c880f2a6266f7edf,
    0x0000000a2f9836e4e4411c4c96a60db3,
    0x0000000517cc1b727220a2857bc0d0a2,
    0x000000028be60db9391053cea3ee218a,
    0x0000000145f306dc9c882a38ceb8c7ec,
    0x00000000a2f9836e4e44152696f49adb,
    0x00000000517cc1b727220a94916d544a,
    0x0000000028be60db9391054a71750b01,
    0x00000000145f306dc9c882a53dd2519c,
    0x000000000a2f9836e4e441529f8c2251,
    0x000000000517cc1b727220a94fda7059,
    0x00000000028be60db9391054a7efc413,
    0x000000000145f306dc9c882a53f83386,
    0x0000000000a2f9836e4e441529fc23f3,
    0x0000000000517cc1b727220a94fe133f,
    0x000000000028be60db9391054a7f09c8,
    0x0000000000145f306dc9c882a53f84e9,
    0x00000000000a2f9836e4e441529fc275,
    0x00000000000517cc1b727220a94fe13b,
    0x0000000000028be60db9391054a7f09d,
    0x00000000000145f306dc9c882a53f84f,
    0x000000000000a2f9836e4e441529fc27,
    0x000000000000517cc1b727220a94fe14,
    0x00000000000028be60db9391054a7f0a,
    0x000000000000145f306dc9c882a53f85,
    0x0000000000000a2f9836e4e441529fc2,
    0x0000000000000517cc1b727220a94fe1,
    0x000000000000028be60db9391054a7f1,
    0x0000000000000145f306dc9c882a53f8,
    0x00000000000000a2f9836e4e441529fc,
    0x00000000000000517cc1b727220a94fe,
    0x0000000000000028be60db9391054a7f,
    0x00000000000000145f306dc9c882a540,
    0x000000000000000a2f9836e4e44152a0,
    0x000000000000000517cc1b727220a950,
    0x00000000000000028be60db9391054a8,
    0x000000000000000145f306dc9c882a54,
    0x0000000000000000a2f9836e4e44152a,
    0x0000000000000000517cc1b727220a95,
    0x000000000000000028be60db9391054a,
    0x0000000000000000145f306dc9c882a5,
    0x00000000000000000a2f9836e4e44153,
    0x00000000000000000517cc1b727220a9,
    0x0000000000000000028be60db9391055,
    0x00000000000000000145f306dc9c882a,
    0x000000000000000000a2f9836e4e4415,
    0x000000000000000000517cc1b727220b,
    0x00000000000000000028be60db939105,
    0x000000000000000000145f306dc9c883,
    0x0000000000000000000a2f9836e4e441,
    0x0000000000000000000517cc1b727221,
    0x000000000000000000028be60db93910,
    0x0000000000000000000145f306dc9c88,
    0x00000000000000000000a2f9836e4e44,
    0x00000000000000000000517cc1b72722,
    0x0000000000000000000028be60db9391,
    0x00000000000000000000145f306dc9c9,
    0x000000000000000000000a2f9836e4e4,
    0x000000000000000000000517cc1b7272,
    0x00000000000000000000028be60db939,
    0x000000000000000000000145f306dc9d,
    0x0000000000000000000000a2f9836e4e,
    0x0000000000000000000000517cc1b727,
    0x000000000000000000000028be60db94,
    0x0000000000000000000000145f306dca,
    0x00000000000000000000000a2f9836e5,
    0x00000000000000000000000517cc1b72,
    0x0000000000000000000000028be60db9,
    0x00000000000000000000000145f306dd,
    0x000000000000000000000000a2f9836e,
    0x000000000000000000000000517cc1b7,
    0x00000000000000000000000028be60dc,
    0x000000000000000000000000145f306e,
    0x0000000000000000000000000a2f9837,
    0x0000000000000000000000000517cc1b,
    0x000000000000000000000000028be60e,
    0x0000000000000000000000000145f307,
    0x00000000000000000000000000a2f983,
    0x00000000000000000000000000517cc2,
    0x0000000000000000000000000028be61,
    0x00000000000000000000000000145f30,
    0x000000000000000000000000000a2f98,
    0x000000000000000000000000000517cc,
    0x00000000000000000000000000028be6,
    0x000000000000000000000000000145f3,
    0x0000000000000000000000000000a2fa,
    0x0000000000000000000000000000517d,
    0x000000000000000000000000000028be,
    0x0000000000000000000000000000145f,
    0x00000000000000000000000000000a30,
    0x00000000000000000000000000000518,
    0x0000000000000000000000000000028c,
    0x00000000000000000000000000000146,
    0x000000000000000000000000000000a3,
    0x00000000000000000000000000000051,
    0x00000000000000000000000000000029,
    0x00000000000000000000000000000014,
    0x0000000000000000000000000000000a,
    0x00000000000000000000000000000005,
    0x00000000000000000000000000000003,
    0x00000000000000000000000000000001,
    0x00000000000000000000000000000001,
];
/// What the steps of a rotation make too long, undone: the product of `1 / sqrt(1 + 4^-step)`, 126 bits after the point.
const GAIN: i128 = 0x26dd3b6a10d79699fd7e424af5ff503a;
/// One over two pi times 2^192, rounded down, as three 64-bit parts, the largest first.
const INVERSE_TAU: [u64; 3] = [0x28be60db9391054a, 0x7f09d5f47d4d3770, 0x36d8a5664f10e410];

const MASK_64: u128 = (1 << 64) - 1;

/// What a body that goes round is from its centre, exact: whole bricks of 32 voxels and a rest below a brick, in steps of 2^-32 voxel. (An `f64` of voxels is as
/// far from the next as 2^-52 of itself, which is a hundred voxels at 10^18.)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExactOffset {
    pub bricks: [i128; 3],
    /// In voxels, from 0 up to a brick, a whole number of 2^-32.
    pub rest: [f64; 3],
}

impl ExactOffset {
    /// As plain numbers, in voxels: as near as an `f64` can say it, to a rounding or two.
    pub fn in_voxels(self) -> [f64; 3] {
        [0, 1, 2].map(|axis| self.bricks[axis] as f64 * 32.0 + self.rest[axis])
    }
}

/// 256 bits, in two's complement: what two 128-bit numbers make when multiplied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Wide {
    high: u128,
    low: u128,
}

impl Wide {
    const ZERO: Self = Self { high: 0, low: 0 };
    const ONE: Self = Self { high: 0, low: 1 };

    fn of_product(first: u128, second: u128) -> Self {
        let (first_high, first_low) = (first >> 64, first & MASK_64);
        let (second_high, second_low) = (second >> 64, second & MASK_64);
        let low_low = first_low * second_low;
        let low_high = first_low * second_high;
        let high_low = first_high * second_low;
        let high_high = first_high * second_high;
        // (Three numbers below 2^64 added: it fits.)
        let middle = (low_low >> 64) + (low_high & MASK_64) + (high_low & MASK_64);
        Self {
            high: high_high + (low_high >> 64) + (high_low >> 64) + (middle >> 64),
            low: (low_low & MASK_64) | (middle << 64),
        }
    }

    fn plus(self, other: Self) -> Self {
        let (low, carry) = self.low.overflowing_add(other.low);
        Self {
            high: self
                .high
                .wrapping_add(other.high)
                .wrapping_add(u128::from(carry)),
            low,
        }
    }

    fn negated(self) -> Self {
        Self {
            high: !self.high,
            low: !self.low,
        }
        .plus(Self::ONE)
    }

    fn is_negative(self) -> bool {
        self.high >> 127 == 1
    }

    /// Shifted left by `bits`, the bits that fall off lost.
    fn shifted_left(self, bits: u32) -> Self {
        match bits {
            0 => self,
            1..=127 => Self {
                high: (self.high << bits) | (self.low >> (128 - bits)),
                low: self.low << bits,
            },
            128..=255 => Self {
                high: self.low << (bits - 128),
                low: 0,
            },
            _ => Self::ZERO,
        }
    }

    /// Shifted right by `bits`, the sign copied in (so a negative number is rounded down).
    fn shifted_right(self, bits: u32) -> Self {
        let sign = if self.is_negative() { u128::MAX } else { 0 };
        match bits {
            0 => self,
            1..=127 => Self {
                high: (self.high >> bits) | (sign << (128 - bits)),
                low: (self.low >> bits) | (self.high << (128 - bits)),
            },
            128 => Self {
                high: sign,
                low: self.high,
            },
            129..=255 => Self {
                high: sign,
                low: (self.high >> (bits - 128)) | (sign << (256 - bits)),
            },
            _ => Self {
                high: sign,
                low: sign,
            },
        }
    }
}

/// `value` (an `f64` below `2^(127 - bits)` in size) as a fixed number with `bits` after the point, rounded toward nothing.
fn fixed_of(value: f64, bits: i32) -> Fixed {
    let (mantissa, exponent) = split(value);
    // value = mantissa x 2^exponent, and the fixed number is that x 2^bits.
    let shift = exponent + bits;
    let magnitude = if shift >= 0 {
        debug_assert!(shift < 75, "a number that does not fit");
        i128::from(mantissa) << shift
    } else if shift > -64 {
        i128::from(mantissa >> -shift)
    } else {
        0
    };
    if value < 0.0 { -magnitude } else { magnitude }
}

/// `value.abs()` as a whole number times a power of two, `(mantissa, exponent)`: exact for every `f64` that is a number.
pub(crate) fn split(value: f64) -> (u64, i32) {
    let bits = value.abs().to_bits();
    let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
    let fraction_bits = bits & ((1 << 52) - 1);
    if exponent_bits == 0 {
        (fraction_bits, -1074)
    } else {
        (fraction_bits | (1 << 52), exponent_bits - 1075)
    }
}

/// A fixed number as an `f64`, rounded once.
pub(crate) fn float_of(value: Fixed) -> f64 {
    value as f64 / ONE as f64
}

/// `first x second` with `bits` after the point in the answer, rounded to the nearest: the numbers have the points where they have them, and `bits` is the sum of
/// theirs less the answer's. The answer must fit, which it does for everything here: a unit vector's coordinates are at most one.
fn multiplied(first: Fixed, second: Fixed, bits: u32) -> Fixed {
    let negative = (first < 0) != (second < 0);
    let product = Wide::of_product(first.unsigned_abs(), second.unsigned_abs());
    let half = Wide::ONE.shifted_left(bits - 1);
    let rounded = product.plus(half).shifted_right(bits);
    debug_assert!(rounded.high == 0 && rounded.low >> 127 == 0);
    let magnitude = rounded.low as i128;
    if negative { -magnitude } else { magnitude }
}

/// `first x second`, 126 bits after the point.
fn product(first: Fixed, second: Fixed) -> Fixed {
    multiplied(first, second, 126)
}

/// `vector` made a unit vector. The size is found as one over a square root by Newton's method from the `f64` of it, in whole numbers: two steps take 53 bits to more
/// than the 124 held. (One over a root is as large as 2.3 for the vectors here, which does not fit with 126 bits after the point, so it has 124.)
fn normalised(vector: [Fixed; 3]) -> [Fixed; 3] {
    const ONE_124: i128 = 1 << 124;
    let square: Fixed = vector
        .iter()
        .map(|&component| product(component, component))
        .sum();
    let guess = 1.0 / libm::sqrt(float_of(square));
    let mut inverse = fixed_of(guess, 124);
    for _ in 0..2 {
        let inverse_squared = multiplied(inverse, inverse, 124);
        let error = ONE_124 - multiplied(square, inverse_squared, 126);
        inverse += multiplied(inverse, error, 125);
    }
    vector.map(|component| multiplied(component, inverse, 124))
}

/// The two directions in the plane of an orbit with this normal (a vector of at most one in each coordinate; it is made a unit vector), as unit vectors with 126 bits
/// after the point: the circle starts toward the first and goes toward the second. The first is the +x axis (the +z axis where the normal is close to +x) with its
/// part along the normal taken out; the second is the normal crossed with the first, so that the circle goes counter-clockwise looking down the normal.
pub(crate) fn plane(normal: [f64; 3]) -> ([Fixed; 3], [Fixed; 3]) {
    // (Which axis is taken is decided by the `f64` of the normal, as it always was, so that every saved phase still means the same place.)
    let reference = if normal[0].abs() < 0.9 { 0 } else { 2 };
    let normal = normalised(normal.map(|component| fixed_of(component, 126)));
    let along = normal[reference];
    let inside = [0, 1, 2].map(|axis| {
        let axis_part = if axis == reference { ONE } else { 0 };
        axis_part - product(normal[axis], along)
    });
    let first = normalised(inside);
    let second = [
        product(normal[1], first[2]) - product(normal[2], first[1]),
        product(normal[2], first[0]) - product(normal[0], first[2]),
        product(normal[0], first[1]) - product(normal[1], first[0]),
    ];
    (first, second)
}

/// `value` shifted right by `bits` and rounded to the nearest, as a rotation step does.
fn halved(value: Fixed, bits: usize) -> Fixed {
    if bits == 0 {
        value
    } else {
        (value + (1 << (bits - 1))) >> bits
    }
}

/// The sine and cosine of `turns` (a fraction of a turn, with 128 bits after the point), `(sine, cosine)`, 126 bits after the point. The angle is taken to the first
/// quarter, and then turned out of it a step at a time by the angles whose tangents are one, a half, a quarter...: each step needs only shifts and adds (CORDIC), so it
/// is the same on every machine and needs no multiplication. Within a few of the last bit of 126 of the true value.
pub(crate) fn sine_and_cosine(turns: u128) -> (Fixed, Fixed) {
    let quadrant = turns >> 126;
    let mut angle = (turns & ((1 << 126) - 1)) as i128;
    let (mut cosine, mut sine) = (GAIN, 0);
    for (step, &step_angle) in ANGLES.iter().enumerate() {
        let (from_cosine, from_sine) = (halved(cosine, step), halved(sine, step));
        if angle >= 0 {
            cosine -= from_sine;
            sine += from_cosine;
            angle -= step_angle as i128;
        } else {
            cosine += from_sine;
            sine -= from_cosine;
            angle += step_angle as i128;
        }
    }
    match quadrant {
        0 => (sine, cosine),
        1 => (cosine, -sine),
        2 => (-sine, -cosine),
        _ => (-cosine, sine),
    }
}

/// `numerator / denominator` (below one) as a fraction of a turn with 128 bits after the point, rounded down. The denominator is below 2^127.
pub(crate) fn fraction_in_turns(numerator: u128, denominator: u128) -> u128 {
    debug_assert!(numerator < denominator && denominator < 1 << 127);
    let (mut remainder, mut turns) = (numerator, 0u128);
    for _ in 0..128 {
        remainder <<= 1;
        turns <<= 1;
        if remainder >= denominator {
            remainder -= denominator;
            turns |= 1;
        }
    }
    turns
}

/// An angle in radians (below 2^64 in size) as a fraction of a turn with 128 bits after the point, a negative angle coming round from the end: the radians times
/// one over two pi with 192 bits, so that it is right to the last of the 128.
pub(crate) fn turns_of_radians(radians: f64) -> u128 {
    let (mantissa, exponent) = split(radians);
    let inverse_high = u128::from(INVERSE_TAU[0]);
    let inverse_low = (u128::from(INVERSE_TAU[1]) << 64) | u128::from(INVERSE_TAU[2]);
    let times_mantissa = Wide::of_product(inverse_low, u128::from(mantissa)).plus(Wide {
        high: inverse_high * u128::from(mantissa),
        low: 0,
    });
    // radians = mantissa x 2^exponent, and turns x 2^128 = (mantissa x one over two pi x 2^192) x 2^exponent x 2^-64.
    let shift = 64 - exponent;
    let magnitude = if shift >= 0 {
        times_mantissa.shifted_right(shift as u32).low
    } else {
        times_mantissa.shifted_left((-shift) as u32).low
    };
    if radians < 0.0 {
        magnitude.wrapping_neg()
    } else {
        magnitude
    }
}

/// The point at `radius` voxels (an `f64`) from the centre of a circle, at the angle whose sine and cosine are given, in the plane spanned by `first` and `second`.
pub(crate) fn circle_point(
    radius: f64,
    (sine, cosine): (Fixed, Fixed),
    first: [Fixed; 3],
    second: [Fixed; 3],
) -> ExactOffset {
    let (mantissa, exponent) = split(radius);
    let mut offset = ExactOffset {
        bricks: [0; 3],
        rest: [0.0; 3],
    };
    for axis in 0..3 {
        let unit = product(cosine, first[axis]) + product(sine, second[axis]);
        (offset.bricks[axis], offset.rest[axis]) = scaled(unit, mantissa, exponent);
    }
    offset
}

/// `unit x mantissa x 2^exponent` voxels as whole bricks and a rest: rounded to the nearest 2^-32 voxel.
fn scaled(unit: Fixed, mantissa: u64, exponent: i32) -> (i128, f64) {
    let magnitude = Wide::of_product(unit.unsigned_abs(), u128::from(mantissa));
    let product = if unit < 0 {
        magnitude.negated()
    } else {
        magnitude
    };
    // The product has 126 bits after the point and the answer in steps of 2^-32 voxel has 32.
    let shift = 94 - exponent;
    if shift > 200 {
        // (Less than 2^-70 of a step.)
        return (0, 0.0);
    }
    let steps = if shift > 0 {
        let half = Wide::ONE.shifted_left((shift - 1) as u32);
        product.plus(half).shifted_right(shift as u32)
    } else {
        product.shifted_left((-shift) as u32)
    };
    let bricks = steps.shifted_right(37);
    debug_assert!(
        bricks.high == if bricks.is_negative() { u128::MAX } else { 0 },
        "an offset that is more than an i128 of bricks"
    );
    let rest_steps = steps.low & ((1 << 37) - 1);
    (bricks.low as i128, rest_steps as f64 / 4_294_967_296.0)
}

#[cfg(test)]
mod tests;

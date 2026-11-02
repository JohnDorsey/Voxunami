//! 256-bit whole numbers in two's complement: what products of 128-bit numbers make, and the division and square root of them that the exact orbits need
//! (`exact_circle`, `exact_kepler`). All whole-number arithmetic, so the same on every machine.

const MASK_64: u128 = (1 << 64) - 1;

/// 256 bits, in two's complement: what two 128-bit numbers make when multiplied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Wide {
    pub(crate) high: u128,
    pub(crate) low: u128,
}

impl Wide {
    pub(crate) const ZERO: Self = Self { high: 0, low: 0 };
    pub(crate) const ONE: Self = Self { high: 0, low: 1 };

    pub(crate) fn of_product(first: u128, second: u128) -> Self {
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

    pub(crate) fn plus(self, other: Self) -> Self {
        let (low, carry) = self.low.overflowing_add(other.low);
        Self {
            high: self
                .high
                .wrapping_add(other.high)
                .wrapping_add(u128::from(carry)),
            low,
        }
    }

    pub(crate) fn negated(self) -> Self {
        Self {
            high: !self.high,
            low: !self.low,
        }
        .plus(Self::ONE)
    }

    pub(crate) fn is_negative(self) -> bool {
        self.high >> 127 == 1
    }

    /// Shifted left by `bits`, the bits that fall off lost.
    pub(crate) fn shifted_left(self, bits: u32) -> Self {
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
    pub(crate) fn shifted_right(self, bits: u32) -> Self {
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

    /// `self - other`.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "used by exact_kepler, which Orbit uses from step A3 of docs/orbits-and-thrust-plan.md"
        )
    )]
    pub(crate) fn minus(self, other: Self) -> Self {
        self.plus(other.negated())
    }

    /// Whether `self` is at least `other`, both taken as numbers that are not negative.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "used by exact_kepler, which Orbit uses from step A3 of docs/orbits-and-thrust-plan.md"
        )
    )]
    pub(crate) fn is_at_least(self, other: Self) -> bool {
        self.high > other.high || (self.high == other.high && self.low >= other.low)
    }

    /// `self / divisor` rounded down, for a number that is not negative and a divisor above nothing and below 2^127; a quotient that does not fit in 128 bits is the largest there is.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "used by exact_kepler, which Orbit uses from step A3 of docs/orbits-and-thrust-plan.md"
        )
    )]
    pub(crate) fn divided_by(self, divisor: u128) -> u128 {
        debug_assert!(divisor != 0 && divisor < 1 << 127);
        if self.high >= divisor {
            return u128::MAX;
        }
        // Long division a bit at a time; the remainder is below the divisor, so shifting a bit into it does not overflow.
        let (mut remainder, mut quotient) = (self.high, 0u128);
        for bit in (0..128).rev() {
            remainder = (remainder << 1) | ((self.low >> bit) & 1);
            quotient <<= 1;
            if remainder >= divisor {
                remainder -= divisor;
                quotient |= 1;
            }
        }
        quotient
    }

    /// The square root of `self` rounded down, for a number below 2^254. The root is found a bit at a time from the top (no division, no `f64`), so it is the same on every machine.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "used by exact_kepler, which Orbit uses from step A3 of docs/orbits-and-thrust-plan.md"
        )
    )]
    pub(crate) fn square_root(self) -> u128 {
        debug_assert!(!self.is_negative() && self.high < 1 << 126);
        let mut remainder = self;
        let mut root = Self::ZERO;
        let mut bit = Self::ONE.shifted_left(254);
        while !remainder.is_at_least(bit) {
            bit = bit.shifted_right(2);
        }
        // (Every number here is below 2^255, so the shifts do not copy a sign in.)
        while bit != Self::ZERO {
            let candidate = root.plus(bit);
            if remainder.is_at_least(candidate) {
                remainder = remainder.minus(candidate);
                root = root.shifted_right(1).plus(bit);
            } else {
                root = root.shifted_right(1);
            }
            bit = bit.shifted_right(2);
        }
        root.low
    }
}

#[cfg(test)]
mod tests;

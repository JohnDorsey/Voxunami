//! 256-bit whole numbers that are not negative: what products of 128-bit numbers make, for the angles and directions of `exact_circle`. All whole-number arithmetic, so
//! the same on every machine.

const MASK_64: u128 = (1 << 64) - 1;

/// 256 bits: what two 128-bit numbers make when multiplied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Wide {
    pub(crate) high: u128,
    pub(crate) low: u128,
}

impl Wide {
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
            _ => Self { high: 0, low: 0 },
        }
    }

    /// Shifted right by `bits`, the bits that fall off lost.
    pub(crate) fn shifted_right(self, bits: u32) -> Self {
        match bits {
            0 => self,
            1..=127 => Self {
                high: self.high >> bits,
                low: (self.low >> bits) | (self.high << (128 - bits)),
            },
            128..=255 => Self {
                high: 0,
                low: self.high >> (bits - 128),
            },
            _ => Self { high: 0, low: 0 },
        }
    }
}

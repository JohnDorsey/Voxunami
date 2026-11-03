use std::fmt;

/// How many bits of significand a number has: the width its arithmetic is rounded to.
///
/// The least is 128 bits, the owner's rule for rails (2026-11-02): an `f64` has 53, which fixes
/// the far end of an orbit as light-years across only to about two metres, and a leg of a flight
/// that must be right to a millimetre needs 64 to 80 bits of working alone. The most is 2^24
/// bits (16.7 million, two megabytes a number), a sanity bound: a chain of 300 gravity assists
/// needs about 8,400 bits, and a corrupt file that asks for more is refused before anything is
/// allocated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Precision(u32);

/// The number given is outside what a [`Precision`] can be.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "a precision is {least} to {most} bits, not {bits}",
    least = Precision::MINIMUM.0,
    most = Precision::MAXIMUM.0
)]
pub struct PrecisionOutOfRange {
    pub bits: u64,
}

impl Precision {
    /// 128 bits: the least a rail ever works in.
    pub const MINIMUM: Precision = Precision(128);
    /// 2^24 bits: a bound on what a file or a loop may ask for.
    pub const MAXIMUM: Precision = Precision(1 << 24);

    pub fn new(bits: u64) -> Result<Precision, PrecisionOutOfRange> {
        match u32::try_from(bits) {
            Ok(bits32) if (Self::MINIMUM.0..=Self::MAXIMUM.0).contains(&bits32) => {
                Ok(Precision(bits32))
            }
            _ => Err(PrecisionOutOfRange { bits }),
        }
    }

    /// A precision known when the program is written: out of range it is a compile error in a
    /// constant and a panic anywhere else.
    ///
    /// ```
    /// const WORKING: wide_float::Precision = wide_float::Precision::of(256);
    /// assert_eq!(WORKING.bits(), 256);
    /// ```
    ///
    /// ```compile_fail
    /// const TOO_NARROW: wide_float::Precision = wide_float::Precision::of(127);
    /// ```
    pub const fn of(bits: u32) -> Precision {
        assert!(
            bits >= Self::MINIMUM.0 && bits <= Self::MAXIMUM.0,
            "a precision is 128 to 2^24 bits"
        );
        Precision(bits)
    }

    pub const fn bits(self) -> u64 {
        self.0 as u64
    }

    /// `extra` bits more, or [`Precision::MAXIMUM`] if that is more than a precision can be.
    pub fn widened_by(self, extra: u32) -> Precision {
        Precision(self.0.saturating_add(extra).min(Self::MAXIMUM.0))
    }

    /// Twice the bits, or [`Precision::MAXIMUM`] if that is more than a precision can be.
    pub fn doubled(self) -> Precision {
        self.widened_by(self.0)
    }
}

impl fmt::Display for Precision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} bits", self.0)
    }
}

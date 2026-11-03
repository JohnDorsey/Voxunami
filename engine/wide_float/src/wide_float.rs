use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::ops::{Add, Mul, Neg, Sub};

use malachite_base::num::arithmetic::traits::Abs;
use malachite_base::num::basic::traits::Zero;
use malachite_base::num::conversion::traits::{
    IntegerMantissaAndExponent, PowerOf2Digits, RoundingFrom,
};
use malachite_base::num::logic::traits::SignificantBits;
use malachite_base::rounding_modes::RoundingMode;
use malachite_float::Float;
use malachite_nz::integer::Integer;
use malachite_nz::natural::Natural;

use crate::{NumberError, Precision};

/// A real number held in a stated number of bits ([`Precision`]), never a NaN or an infinity.
///
/// Every operation gives **the correctly rounded value** (the nearest number of the result's
/// precision, a tie going to the one with an even last bit) of the exact result of the operation
/// on its arguments. That is a mathematical fact and not a property of the library or the machine,
/// so the same bits come out on Windows and Linux, from any version of the library underneath that
/// passes the conformance tests (`tests/` of this crate), and no matter how a computation was
/// scheduled. Nothing here uses a floating-point instruction of the machine.
///
/// **The precision of a result** is that of the argument with more bits, so a computation is done
/// at the width of the numbers that go into it; [`WideFloat::rounded_to`] sets the width of a
/// number, and a value read from a file keeps the precision it was written with. Equality
/// compares the precision as well as the value (two numbers are `==` when they are the same to
/// the bit); [`WideFloat::compare`] compares values.
///
/// There is no negative zero. An operation whose result is outside the range of exponents
/// (about 2^±1,000,000,000) is an error for the functions that return a [`Result`], and a panic
/// for the operators `+ - *` and `-`, which nothing the game does can reach.
#[derive(Clone)]
pub struct WideFloat {
    value: Float,
    precision: Precision,
}

/// A number apart: its sign, its odd mantissa and its binary exponent, `± mantissa × 2^exponent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parts {
    pub negative: bool,
    /// The mantissa as little-endian bytes, the fewest that hold it; it is odd, so a number has
    /// one pair of mantissa and exponent.
    pub mantissa: Vec<u8>,
    pub exponent: i64,
}

impl WideFloat {
    // Construction.

    pub fn zero(precision: Precision) -> WideFloat {
        WideFloat {
            value: Float::ZERO,
            precision,
        }
    }

    /// An integer, which is exact (a precision is at least 128 bits and an `i128` needs 128).
    pub fn from_i128(value: i128, precision: Precision) -> WideFloat {
        let (float, _) = Float::from_integer_prec(Integer::from(value), precision.bits());
        Self::finish_exact(float, precision)
    }

    pub fn from_i64(value: i64, precision: Precision) -> WideFloat {
        Self::from_i128(i128::from(value), precision)
    }

    /// A double, which is exact (a precision is at least 128 bits and a double has 53).
    pub fn from_f64(value: f64, precision: Precision) -> Result<WideFloat, NumberError> {
        if !value.is_finite() {
            return Err(NumberError::NotFinite);
        }
        let (float, _) = Float::from_primitive_float_prec(value, precision.bits());
        Ok(Self::finish_exact(float, precision))
    }

    /// π to `precision` bits.
    pub fn pi(precision: Precision) -> WideFloat {
        let (float, _) = Float::pi_prec(precision.bits());
        Self::finish_exact(float, precision)
    }

    /// The number `± mantissa × 2^exponent`, which must fit in `precision` bits exactly (the
    /// mantissa is little-endian bytes and may have trailing zero bits and leading zero bytes).
    pub fn from_parts(
        negative: bool,
        mantissa: &[u8],
        exponent: i64,
        precision: Precision,
    ) -> Result<WideFloat, NumberError> {
        let natural = Natural::from_power_of_2_digits_asc(8, mantissa.iter().copied())
            .ok_or(NumberError::Malformed("a mantissa is a run of bytes"))?;
        if natural == 0u32 {
            return Ok(Self::zero(precision));
        }
        let trailing = natural.trailing_zeros().unwrap_or(0);
        let needed = natural.significant_bits() - trailing;
        if needed > precision.bits() {
            return Err(NumberError::DoesNotFit {
                needed,
                allowed: precision.bits(),
            });
        }
        let mut integer = Integer::from(natural);
        if negative {
            integer = -integer;
        }
        let (float, _) = Float::from_integer_prec(integer, precision.bits());
        // A shift saturates to an infinity or to zero outside the range of exponents, and the mantissa is not zero.
        let scaled = float << exponent;
        if scaled.is_infinite() || scaled.is_zero() {
            return Err(NumberError::ExponentRange);
        }
        Ok(Self::finish_exact(scaled, precision))
    }

    /// Reads `[-]0x<hex digits>p<decimal exponent>`, the form [`std::fmt::Display`] writes
    /// (`-0x1a3p-12` is −0x1a3 × 2^−12, and `0x0p0` is zero), into a number of `precision` bits
    /// that must hold it exactly.
    pub fn parse(text: &str, precision: Precision) -> Result<WideFloat, NumberError> {
        let malformed = NumberError::Malformed("expected [-]0x<hex digits>p<exponent>");
        let (negative, rest) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let rest = rest.strip_prefix("0x").ok_or(malformed.clone())?;
        let (digits, exponent) = rest.split_once('p').ok_or(malformed.clone())?;
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(malformed);
        }
        let exponent: i64 = exponent.parse().map_err(|_| malformed)?;
        let mut bytes = Vec::with_capacity(digits.len().div_ceil(2));
        let digit_values: Vec<u8> = digits
            .bytes()
            .rev()
            .map(|byte| (byte as char).to_digit(16).expect("checked") as u8)
            .collect();
        for pair in digit_values.chunks(2) {
            bytes.push(pair[0] | pair.get(1).copied().unwrap_or(0) << 4);
        }
        Self::from_parts(negative, &bytes, exponent, precision)
    }

    // Looking at a number.

    pub fn precision(&self) -> Precision {
        self.precision
    }

    pub fn is_zero(&self) -> bool {
        self.value.is_zero()
    }

    /// Below zero (zero is not).
    pub fn is_negative(&self) -> bool {
        self.value.is_sign_negative()
    }

    /// Above zero (zero is not).
    pub fn is_positive(&self) -> bool {
        !self.is_zero() && !self.is_negative()
    }

    /// Compares the values; the precisions play no part.
    pub fn compare(&self, other: &WideFloat) -> Ordering {
        self.value
            .partial_cmp(&other.value)
            .expect("a number is never a NaN")
    }

    /// The exponent of the leading bit: `e` such that `2^e <= |x| < 2^(e+1)`; none for zero.
    pub fn floor_log2(&self) -> Option<i64> {
        self.value
            .get_exponent()
            .map(|exponent| i64::from(exponent) - 1)
    }

    /// The nearest double, which is an infinity or a zero for a number beyond what a double can
    /// hold.
    pub fn to_f64(&self) -> f64 {
        f64::rounding_from(&self.value, RoundingMode::Nearest).0
    }

    /// The greatest whole number not above this one, if it fits in an `i128`.
    pub fn floor_to_i128(&self) -> Option<i128> {
        if self.floor_log2().is_some_and(|exponent| exponent > 127) {
            return None;
        }
        let (integer, _) = Integer::rounding_from(&self.value, RoundingMode::Floor);
        i128::try_from(&integer).ok()
    }

    /// The sign, mantissa and exponent; none for zero.
    pub fn to_parts(&self) -> Option<Parts> {
        if self.is_zero() {
            return None;
        }
        let (mantissa, exponent) = (&self.value).integer_mantissa_and_exponent();
        Some(Parts {
            negative: self.is_negative(),
            mantissa: mantissa.to_power_of_2_digits_asc(8),
            exponent,
        })
    }

    // Changing the width.

    /// The number in `precision` bits: exact if that is more than it has, the nearest otherwise.
    pub fn rounded_to(&self, precision: Precision) -> WideFloat {
        let (float, _) = Float::from_float_prec_ref(&self.value, precision.bits());
        Self::finish_exact(float, precision)
    }

    /// The number times 2^`exponent`, exactly.
    pub fn times_power_of_two(&self, exponent: i64) -> Result<WideFloat, NumberError> {
        let scaled = &self.value << exponent;
        if scaled.is_infinite() || (scaled.is_zero() && !self.is_zero()) {
            return Err(NumberError::ExponentRange);
        }
        Ok(Self::finish_exact(scaled, self.precision))
    }

    // Arithmetic that can fail.

    pub fn divide(&self, divisor: &WideFloat) -> Result<WideFloat, NumberError> {
        if divisor.is_zero() {
            return Err(NumberError::DivisionByZero);
        }
        let precision = self.precision.max(divisor.precision);
        let (value, ordering) = self
            .value
            .div_prec_ref_ref(&divisor.value, precision.bits());
        Self::from_rounded(value, ordering, precision)
    }

    pub fn sqrt(&self) -> Result<WideFloat, NumberError> {
        if self.is_negative() {
            return Err(NumberError::OutsideDomain(
                "the square root of a negative number",
            ));
        }
        let (value, ordering) = self.value.sqrt_prec_ref(self.precision.bits());
        Self::from_rounded(value, ordering, self.precision)
    }

    pub fn exp(&self) -> Result<WideFloat, NumberError> {
        let (value, ordering) = self.value.exp_prec_ref(self.precision.bits());
        Self::from_rounded(value, ordering, self.precision)
    }

    pub fn ln(&self) -> Result<WideFloat, NumberError> {
        if !self.is_positive() {
            return Err(NumberError::OutsideDomain(
                "the logarithm of a number that is not above zero",
            ));
        }
        let (value, ordering) = self.value.ln_prec_ref(self.precision.bits());
        Self::from_rounded(value, ordering, self.precision)
    }

    pub fn sinh(&self) -> Result<WideFloat, NumberError> {
        let (value, ordering) = self.value.sinh_prec_ref(self.precision.bits());
        Self::from_rounded(value, ordering, self.precision)
    }

    pub fn cosh(&self) -> Result<WideFloat, NumberError> {
        let (value, ordering) = self.value.cosh_prec_ref(self.precision.bits());
        Self::from_rounded(value, ordering, self.precision)
    }

    /// The inverse of `cosh` for a number from 1 up.
    pub fn acosh(&self) -> Result<WideFloat, NumberError> {
        let one = Float::from(1u32);
        if self.value < one {
            return Err(NumberError::OutsideDomain(
                "the inverse hyperbolic cosine of a number below one",
            ));
        }
        let (value, ordering) = self.value.acosh_prec_ref(self.precision.bits());
        Self::from_rounded(value, ordering, self.precision)
    }

    /// The angle of the point (`x`, `self`) from the x axis, in (−π, π], as `y.atan2(x)` of a
    /// double; the point at the origin has none.
    pub fn atan2(&self, x: &WideFloat) -> Result<WideFloat, NumberError> {
        if self.is_zero() && x.is_zero() {
            return Err(NumberError::OutsideDomain("the angle of the origin"));
        }
        let precision = self.precision.max(x.precision);
        let (value, ordering) = self.value.atan2_prec_ref_ref(&x.value, precision.bits());
        Self::from_rounded(value, ordering, precision)
    }

    // Functions that cannot fail.

    pub fn sin(&self) -> WideFloat {
        let (value, ordering) = self.value.sin_prec_ref(self.precision.bits());
        Self::from_rounded(value, ordering, self.precision).expect(IN_RANGE)
    }

    pub fn cos(&self) -> WideFloat {
        let (value, ordering) = self.value.cos_prec_ref(self.precision.bits());
        Self::from_rounded(value, ordering, self.precision).expect(IN_RANGE)
    }

    /// The sine and the cosine, each correctly rounded (the same bits as `sin` and `cos`).
    pub fn sin_cos(&self) -> (WideFloat, WideFloat) {
        let (sine, cosine, sine_ordering, cosine_ordering) =
            self.value.sin_cos_prec_ref(self.precision.bits());
        (
            Self::from_rounded(sine, sine_ordering, self.precision).expect(IN_RANGE),
            Self::from_rounded(cosine, cosine_ordering, self.precision).expect(IN_RANGE),
        )
    }

    /// The angle in (−π/2, π/2) whose tangent this is.
    pub fn atan(&self) -> WideFloat {
        let (value, ordering) = self.value.atan_prec_ref(self.precision.bits());
        Self::from_rounded(value, ordering, self.precision).expect(IN_RANGE)
    }

    pub fn square(&self) -> WideFloat {
        let (value, ordering) = self.value.square_prec_ref(self.precision.bits());
        Self::from_rounded(value, ordering, self.precision).expect(IN_RANGE)
    }

    pub fn abs(&self) -> WideFloat {
        WideFloat {
            value: (&self.value).abs(),
            precision: self.precision,
        }
    }

    // Making one.

    /// A result of an operation that rounded: a number, unless the operation went outside what
    /// numbers can be. A zero that is not exact is an underflow; a zero is never negative.
    fn from_rounded(
        value: Float,
        ordering: Ordering,
        precision: Precision,
    ) -> Result<WideFloat, NumberError> {
        if value.is_nan() {
            return Err(NumberError::OutsideDomain("the result is not a number"));
        }
        if value.is_infinite() || (value.is_zero() && ordering != Ordering::Equal) {
            return Err(NumberError::ExponentRange);
        }
        Ok(Self::finish_exact(value, precision))
    }

    /// A finite number from a conversion or a shift, which cannot have gone out of range.
    fn finish_exact(value: Float, precision: Precision) -> WideFloat {
        debug_assert!(value.is_finite());
        debug_assert!(value.get_prec().is_none_or(|bits| bits == precision.bits()));
        let value = if value.is_zero() { Float::ZERO } else { value };
        WideFloat { value, precision }
    }

    /// The result of `+`, `-` or `*` at the wider of two precisions.
    fn arithmetic(
        &self,
        other: &WideFloat,
        operation: fn(&Float, &Float, u64) -> (Float, Ordering),
    ) -> WideFloat {
        let precision = self.precision.max(other.precision);
        let (value, ordering) = operation(&self.value, &other.value, precision.bits());
        Self::from_rounded(value, ordering, precision).expect(IN_RANGE)
    }
}

const IN_RANGE: &str = "the result is within the range of exponents";

impl PartialEq for WideFloat {
    fn eq(&self, other: &WideFloat) -> bool {
        self.precision == other.precision && self.value == other.value
    }
}

impl Eq for WideFloat {}

impl Hash for WideFloat {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.precision.hash(state);
        self.to_parts().hash_parts(state);
    }
}

/// Hashing the parts of a number (or zero) without a `Hash` for [`Parts`] to promise.
trait HashParts {
    fn hash_parts<H: Hasher>(&self, state: &mut H);
}

impl HashParts for Option<Parts> {
    fn hash_parts<H: Hasher>(&self, state: &mut H) {
        match self {
            None => 0u8.hash(state),
            Some(parts) => {
                1u8.hash(state);
                parts.negative.hash(state);
                parts.mantissa.hash(state);
                parts.exponent.hash(state);
            }
        }
    }
}

impl Add for &WideFloat {
    type Output = WideFloat;
    fn add(self, other: &WideFloat) -> WideFloat {
        self.arithmetic(other, Float::add_prec_ref_ref)
    }
}

impl Sub for &WideFloat {
    type Output = WideFloat;
    fn sub(self, other: &WideFloat) -> WideFloat {
        self.arithmetic(other, Float::sub_prec_ref_ref)
    }
}

impl Mul for &WideFloat {
    type Output = WideFloat;
    fn mul(self, other: &WideFloat) -> WideFloat {
        self.arithmetic(other, Float::mul_prec_ref_ref)
    }
}

impl Neg for &WideFloat {
    type Output = WideFloat;
    fn neg(self) -> WideFloat {
        WideFloat::finish_exact(-&self.value, self.precision)
    }
}

macro_rules! forward_to_references {
    ($operator:ident, $method:ident) => {
        impl $operator for WideFloat {
            type Output = WideFloat;
            fn $method(self, other: WideFloat) -> WideFloat {
                (&self).$method(&other)
            }
        }
    };
}

forward_to_references!(Add, add);
forward_to_references!(Sub, sub);
forward_to_references!(Mul, mul);

impl Neg for WideFloat {
    type Output = WideFloat;
    fn neg(self) -> WideFloat {
        -&self
    }
}

impl std::fmt::Display for WideFloat {
    /// `[-]0x<hex digits of the odd mantissa>p<decimal exponent>`, exact; zero is `0x0p0`.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.to_parts() {
            None => write!(formatter, "0x0p0"),
            Some(parts) => {
                if parts.negative {
                    write!(formatter, "-")?;
                }
                write!(formatter, "0x")?;
                let mut digits = parts.mantissa.iter().rev();
                if let Some(top) = digits.next() {
                    write!(formatter, "{top:x}")?;
                }
                for byte in digits {
                    write!(formatter, "{byte:02x}")?;
                }
                write!(formatter, "p{}", parts.exponent)
            }
        }
    }
}

impl std::fmt::Debug for WideFloat {
    /// The exact value, then the precision: `0x3p-1@128`.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self}@{}", self.precision.bits())
    }
}

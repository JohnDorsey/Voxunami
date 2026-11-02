//! One small interface over the three libraries, so that the same program runs on each.

use dashu_base::BitTest;
use dashu_int::{IBig, UBig};

use crate::Canon;

pub trait Lib {
    const NAME: &'static str;
    type N: Clone;
    type Ctx;

    fn ctx(precision: usize) -> Self::Ctx;
    fn from_i128(c: &mut Self::Ctx, value: i128) -> Self::N;
    fn from_f64(c: &mut Self::Ctx, value: f64) -> Self::N;
    /// An exact number (its parts), at the context's precision (exact if it fits).
    fn from_canon(c: &mut Self::Ctx, value: &Canon) -> Self::N;
    fn canon(value: &Self::N) -> Canon;
    fn add(c: &mut Self::Ctx, a: &Self::N, b: &Self::N) -> Self::N;
    fn sub(c: &mut Self::Ctx, a: &Self::N, b: &Self::N) -> Self::N;
    fn mul(c: &mut Self::Ctx, a: &Self::N, b: &Self::N) -> Self::N;
    fn div(c: &mut Self::Ctx, a: &Self::N, b: &Self::N) -> Self::N;
    fn sqrt(c: &mut Self::Ctx, a: &Self::N) -> Self::N;
    fn sin(c: &mut Self::Ctx, a: &Self::N) -> Self::N;
    fn cos(c: &mut Self::Ctx, a: &Self::N) -> Self::N;
    fn atan(c: &mut Self::Ctx, a: &Self::N) -> Self::N;
    fn sinh(c: &mut Self::Ctx, a: &Self::N) -> Self::N;
    fn acosh(c: &mut Self::Ctx, a: &Self::N) -> Self::N;
    fn exp(c: &mut Self::Ctx, a: &Self::N) -> Self::N;
    fn ln(c: &mut Self::Ctx, a: &Self::N) -> Self::N;
    fn sin_cos(c: &mut Self::Ctx, a: &Self::N) -> (Self::N, Self::N) {
        (Self::sin(c, a), Self::cos(c, a))
    }
    fn to_f64(a: &Self::N) -> f64 {
        let canon = Self::canon(a);
        let top = canon.mantissa.bit_len() as i64;
        let take = top.min(60);
        let part = (&canon.mantissa >> ((top - take) as usize)).to_f64().value();
        let value = part * 2f64.powi((top - take + canon.exponent) as i32);
        if canon.negative { -value } else { value }
    }
    fn neg(c: &mut Self::Ctx, a: &Self::N) -> Self::N {
        let zero = Self::from_i128(c, 0);
        Self::sub(c, &zero, a)
    }
    fn is_negative(a: &Self::N) -> bool {
        Self::canon(a).negative
    }
}

pub struct Dashu;

mod dashu_impl {
    use super::*;
    use dashu_float::{round::mode::HalfEven, FBig};
    type F = FBig<HalfEven, 2>;

    impl Lib for Dashu {
        const NAME: &'static str = "dashu-float 0.6.2";
        type N = F;
        type Ctx = usize;

        fn ctx(precision: usize) -> usize {
            precision
        }
        fn from_i128(c: &mut usize, value: i128) -> F {
            F::from(IBig::from(value)).with_precision(*c).value()
        }
        fn from_f64(c: &mut usize, value: f64) -> F {
            F::try_from(value).unwrap().with_precision(*c).value()
        }
        fn from_canon(c: &mut usize, value: &Canon) -> F {
            let m = if value.negative { -IBig::from(value.mantissa.clone()) } else { IBig::from(value.mantissa.clone()) };
            F::from_parts(m, value.exponent as isize).with_precision(*c).value()
        }
        fn canon(value: &F) -> Canon {
            let r = value.repr();
            Canon::from_signed(r.significand().clone(), r.exponent() as i64)
        }
        fn add(_: &mut usize, a: &F, b: &F) -> F {
            a + b
        }
        fn sub(_: &mut usize, a: &F, b: &F) -> F {
            a - b
        }
        fn mul(_: &mut usize, a: &F, b: &F) -> F {
            a * b
        }
        fn div(_: &mut usize, a: &F, b: &F) -> F {
            a / b
        }
        fn sqrt(_: &mut usize, a: &F) -> F {
            a.sqrt()
        }
        fn sin(_: &mut usize, a: &F) -> F {
            a.sin()
        }
        fn cos(_: &mut usize, a: &F) -> F {
            a.cos()
        }
        fn sin_cos(_: &mut usize, a: &F) -> (F, F) {
            a.sin_cos()
        }
        fn atan(_: &mut usize, a: &F) -> F {
            a.atan()
        }
        fn sinh(_: &mut usize, a: &F) -> F {
            a.sinh()
        }
        fn acosh(_: &mut usize, a: &F) -> F {
            a.acosh()
        }
        fn exp(_: &mut usize, a: &F) -> F {
            a.exp()
        }
        fn ln(_: &mut usize, a: &F) -> F {
            a.ln()
        }
    }
}

pub struct Malachite;

mod malachite_impl {
    use super::*;
    use malachite_base::num::arithmetic::traits::Sign;
    use malachite_base::num::conversion::traits::FromStringBase;
    use malachite_float::Float;
    use malachite_nz::integer::Integer;
    use malachite_nz::natural::Natural;
    use malachite_q::Rational;

    impl Lib for Malachite {
        const NAME: &'static str = "malachite-float 0.14.0";
        type N = Float;
        type Ctx = u64;

        fn ctx(precision: usize) -> u64 {
            precision as u64
        }
        fn from_i128(c: &mut u64, value: i128) -> Float {
            Float::from_integer_prec(Integer::from(value), *c).0
        }
        fn from_f64(c: &mut u64, value: f64) -> Float {
            Float::from_primitive_float_prec(value, *c).0
        }
        fn from_canon(c: &mut u64, value: &Canon) -> Float {
            let natural = Natural::from_string_base(16, &format!("{:x}", value.mantissa)).unwrap();
            let mantissa = Integer::from(natural);
            let mantissa = if value.negative { -mantissa } else { mantissa };
            let base = Float::from_integer_prec(mantissa, *c).0;
            // value = mantissa x 2^exponent: scale exactly by a power of two.
            base << value.exponent
        }
        fn canon(value: &Float) -> Canon {
            let r = Rational::try_from(value).expect("a finite number");
            let negative = r.sign() == std::cmp::Ordering::Less;
            let numerator = UBig::from_str_radix(&format!("{:x}", r.numerator_ref()), 16).unwrap();
            let denominator = UBig::from_str_radix(&format!("{:x}", r.denominator_ref()), 16).unwrap();
            // The denominator is a power of two.
            let exponent = -(denominator.bit_len() as i64 - 1);
            Canon::new(negative, numerator, exponent)
        }
        fn add(c: &mut u64, a: &Float, b: &Float) -> Float {
            a.clone().add_prec(b.clone(), *c).0
        }
        fn sub(c: &mut u64, a: &Float, b: &Float) -> Float {
            a.clone().sub_prec(b.clone(), *c).0
        }
        fn mul(c: &mut u64, a: &Float, b: &Float) -> Float {
            a.clone().mul_prec(b.clone(), *c).0
        }
        fn div(c: &mut u64, a: &Float, b: &Float) -> Float {
            a.clone().div_prec(b.clone(), *c).0
        }
        fn sqrt(c: &mut u64, a: &Float) -> Float {
            a.clone().sqrt_prec(*c).0
        }
        fn sin(c: &mut u64, a: &Float) -> Float {
            a.clone().sin_prec(*c).0
        }
        fn cos(c: &mut u64, a: &Float) -> Float {
            a.clone().cos_prec(*c).0
        }
        fn sin_cos(c: &mut u64, a: &Float) -> (Float, Float) {
            let (s, co, _, _) = a.clone().sin_cos_prec(*c);
            (s, co)
        }
        fn atan(c: &mut u64, a: &Float) -> Float {
            a.clone().atan_prec(*c).0
        }
        fn sinh(c: &mut u64, a: &Float) -> Float {
            a.clone().sinh_prec(*c).0
        }
        fn acosh(c: &mut u64, a: &Float) -> Float {
            a.clone().acosh_prec(*c).0
        }
        fn exp(c: &mut u64, a: &Float) -> Float {
            a.clone().exp_prec(*c).0
        }
        fn ln(c: &mut u64, a: &Float) -> Float {
            a.clone().ln_prec(*c).0
        }
    }
}

pub struct Astro;

mod astro_impl {
    use super::*;
    use astro_float::{BigFloat, Consts, RoundingMode, Sign};

    const RM: RoundingMode = RoundingMode::ToEven;

    impl Lib for Astro {
        const NAME: &'static str = "astro-float 0.9.6";
        type N = BigFloat;
        type Ctx = (usize, Consts);

        fn ctx(precision: usize) -> (usize, Consts) {
            (precision, Consts::new().expect("constants"))
        }
        fn from_i128(c: &mut (usize, Consts), value: i128) -> BigFloat {
            BigFloat::from_i128(value, c.0)
        }
        fn from_f64(c: &mut (usize, Consts), value: f64) -> BigFloat {
            BigFloat::from_f64(value, c.0)
        }
        fn from_canon(c: &mut (usize, Consts), value: &Canon) -> BigFloat {
            // mantissa x 2^exponent: a BigFloat from words, with the exponent set so that the value is 0.1xxxx x 2^e.
            let words = value.mantissa.as_words();
            let bits = value.mantissa.bit_len();
            let total = words.len() * 64;
            // Shift the mantissa so that its top bit is the top bit of the top word.
            let shifted = &value.mantissa << (total - bits);
            let shifted_words = shifted.as_words().to_vec();
            let exponent = value.exponent + bits as i64;
            let sign = if value.negative { Sign::Neg } else { Sign::Pos };
            let mut number = BigFloat::from_words(&shifted_words, sign, exponent as i32);
            let _ = number.set_precision(c.0.max(total), RM);
            number
        }
        fn canon(value: &BigFloat) -> Canon {
            let (words, _bits, sign, exponent, _inexact) = value.as_raw_parts().expect("a finite number");
            // value = 0.m x 2^exponent, m the words (little end first) with the top bit set: m x 2^(exponent - 64 words).
            let mut mantissa = UBig::ZERO;
            for word in words.iter().rev() {
                mantissa = (mantissa << 64usize) + UBig::from(*word);
            }
            Canon::new(sign == Sign::Neg, mantissa, exponent as i64 - 64 * words.len() as i64)
        }
        fn add(c: &mut (usize, Consts), a: &BigFloat, b: &BigFloat) -> BigFloat {
            a.add(b, c.0, RM)
        }
        fn sub(c: &mut (usize, Consts), a: &BigFloat, b: &BigFloat) -> BigFloat {
            a.sub(b, c.0, RM)
        }
        fn mul(c: &mut (usize, Consts), a: &BigFloat, b: &BigFloat) -> BigFloat {
            a.mul(b, c.0, RM)
        }
        fn div(c: &mut (usize, Consts), a: &BigFloat, b: &BigFloat) -> BigFloat {
            a.div(b, c.0, RM)
        }
        fn sqrt(c: &mut (usize, Consts), a: &BigFloat) -> BigFloat {
            a.sqrt(c.0, RM)
        }
        fn sin(c: &mut (usize, Consts), a: &BigFloat) -> BigFloat {
            a.sin(c.0, RM, &mut c.1)
        }
        fn cos(c: &mut (usize, Consts), a: &BigFloat) -> BigFloat {
            a.cos(c.0, RM, &mut c.1)
        }
        fn atan(c: &mut (usize, Consts), a: &BigFloat) -> BigFloat {
            a.atan(c.0, RM, &mut c.1)
        }
        fn sinh(c: &mut (usize, Consts), a: &BigFloat) -> BigFloat {
            a.sinh(c.0, RM, &mut c.1)
        }
        fn acosh(c: &mut (usize, Consts), a: &BigFloat) -> BigFloat {
            a.acosh(c.0, RM, &mut c.1)
        }
        fn exp(c: &mut (usize, Consts), a: &BigFloat) -> BigFloat {
            a.exp(c.0, RM, &mut c.1)
        }
        fn ln(c: &mut (usize, Consts), a: &BigFloat) -> BigFloat {
            a.ln(c.0, RM, &mut c.1)
        }
    }
}

pub struct Puremp;

mod puremp_impl {
    use super::*;
    use puremp::{Float, Int, Rational, RoundingMode};

    const NEAREST: RoundingMode = RoundingMode::Nearest;

    impl Lib for Puremp {
        const NAME: &'static str = "puremp 0.3.2";
        type N = Float;
        type Ctx = u64;

        fn ctx(precision: usize) -> u64 {
            precision as u64
        }
        fn from_i128(c: &mut u64, value: i128) -> Float {
            Float::from_int(&Int::from_i128(value), *c, NEAREST)
        }
        fn from_f64(c: &mut u64, value: f64) -> Float {
            Float::from_f64(value, *c, NEAREST)
        }
        fn from_canon(c: &mut u64, value: &Canon) -> Float {
            let mantissa = Int::from_str_radix(&format!("{:x}", value.mantissa), 16).unwrap();
            let mantissa = if value.negative { mantissa.neg() } else { mantissa };
            let ratio = if value.exponent >= 0 {
                Rational::from_integer(mantissa.mul_2k(value.exponent as u64))
            } else {
                Rational::new(mantissa, Int::ONE.mul_2k((-value.exponent) as u64))
            };
            Float::from_rational(&ratio, *c, NEAREST)
        }
        fn canon(value: &Float) -> Canon {
            match (value.significand(), value.exponent()) {
                (Some(sig), Some(exponent)) => {
                    let mantissa = UBig::from_str_radix(&format!("{:x}", sig), 16).unwrap();
                    Canon::new(value.is_sign_negative(), mantissa, exponent)
                }
                _ => Canon::new(false, UBig::ZERO, 0),
            }
        }
        fn add(c: &mut u64, a: &Float, b: &Float) -> Float {
            a.add(b, *c, NEAREST)
        }
        fn sub(c: &mut u64, a: &Float, b: &Float) -> Float {
            a.sub(b, *c, NEAREST)
        }
        fn mul(c: &mut u64, a: &Float, b: &Float) -> Float {
            a.mul(b, *c, NEAREST)
        }
        fn div(c: &mut u64, a: &Float, b: &Float) -> Float {
            a.div(b, *c, NEAREST)
        }
        fn sqrt(c: &mut u64, a: &Float) -> Float {
            a.sqrt(*c, NEAREST)
        }
        fn sin(c: &mut u64, a: &Float) -> Float {
            a.sin(*c, NEAREST)
        }
        fn cos(c: &mut u64, a: &Float) -> Float {
            a.cos(*c, NEAREST)
        }
        fn atan(c: &mut u64, a: &Float) -> Float {
            a.atan(*c, NEAREST)
        }
        fn sinh(c: &mut u64, a: &Float) -> Float {
            a.sinh(*c, NEAREST)
        }
        fn acosh(c: &mut u64, a: &Float) -> Float {
            a.acosh(*c, NEAREST)
        }
        fn exp(c: &mut u64, a: &Float) -> Float {
            a.exp(*c, NEAREST)
        }
        fn ln(c: &mut u64, a: &Float) -> Float {
            a.ln(*c, NEAREST)
        }
    }
}

//! Conformance: every operation gives the correctly rounded value of the exact result, on this
//! machine, in the version of the library this build has.
//!
//! This is what makes a number layer safe to share between a server and a client and between
//! Windows and Linux: correct rounding is a mathematical fact, so a result that is wrong here is
//! a bug in the library (or in this crate's use of it), and one that is right on both systems is
//! the same bits on both. The reference is `reference/functions.txt`, made by
//! `reference/generate.py` with other code and other algorithms (exact integers and fractions,
//! mpmath at 256 bits more than the width); the CI of the project runs these tests on Linux and on
//! Windows. The answer to "which library?" is therefore "any that passes".

use std::collections::BTreeMap;

use wide_float::{NumberError, Precision, WideFloat};

const VECTORS: &str = include_str!("reference/functions.txt");

fn number(text: &str, precision: Precision) -> WideFloat {
    WideFloat::parse(text, precision).unwrap_or_else(|error| panic!("{text}: {error}"))
}

#[test]
fn every_operation_gives_the_correctly_rounded_result_of_the_reference_vectors() {
    let mut checked: BTreeMap<&str, usize> = BTreeMap::new();
    for line in VECTORS.lines() {
        let words: Vec<&str> = line.split(' ').collect();
        let name = words[0];
        let precision = Precision::new(words[1].parse().unwrap()).unwrap();
        let arguments: Vec<WideFloat> = words[2..words.len() - 1]
            .iter()
            .map(|word| number(word, precision))
            .collect();
        let expected = number(words.last().unwrap(), precision);
        let result = match name {
            "pi" => WideFloat::pi(precision),
            "add" => &arguments[0] + &arguments[1],
            "sub" => &arguments[0] - &arguments[1],
            "mul" => &arguments[0] * &arguments[1],
            "div" => arguments[0].divide(&arguments[1]).unwrap(),
            "sqrt" => arguments[0].sqrt().unwrap(),
            "sin" => arguments[0].sin(),
            "cos" => arguments[0].cos(),
            "atan" => arguments[0].atan(),
            "sinh" => arguments[0].sinh().unwrap(),
            "cosh" => arguments[0].cosh().unwrap(),
            "exp" => arguments[0].exp().unwrap(),
            "ln" => arguments[0].ln().unwrap(),
            "acosh" => arguments[0].acosh().unwrap(),
            "atan2" => arguments[0].atan2(&arguments[1]).unwrap(),
            other => panic!("the reference file has a function this test does not know: {other}"),
        };
        // `==` compares the precision too: the result is a number of the width asked for.
        assert_eq!(result, expected, "{name} at {precision}:\n{line}");
        *checked.entry(name).or_default() += 1;
    }
    // A reference file that lost its lines would pass the loop above.
    for (name, least) in [
        ("pi", 7),
        ("add", 50),
        ("sub", 50),
        ("mul", 50),
        ("div", 50),
        ("sqrt", 50),
        ("sin", 50),
        ("cos", 50),
        ("atan", 50),
        ("sinh", 50),
        ("cosh", 50),
        ("exp", 50),
        ("ln", 80),
        ("acosh", 80),
        ("atan2", 50),
    ] {
        assert!(
            checked.get(name).copied().unwrap_or(0) >= least,
            "{name}: {checked:?}"
        );
    }
    let widths: std::collections::BTreeSet<&str> = VECTORS
        .lines()
        .map(|line| line.split(' ').nth(1).unwrap())
        .collect();
    assert_eq!(
        widths.into_iter().collect::<Vec<_>>(),
        ["1024", "128", "129", "192", "256", "4096", "512"],
        "the widths the reference covers"
    );
}

fn at(bits: u32) -> Precision {
    Precision::of(bits)
}

fn exact(text: &str, bits: u32) -> WideFloat {
    number(text, at(bits))
}

#[test]
fn results_that_are_exact_are_exact() {
    for bits in [128, 129, 256, 1024] {
        let zero = WideFloat::zero(at(bits));
        let one = WideFloat::from_i64(1, at(bits));
        assert_eq!(zero.sin(), zero);
        assert_eq!(zero.cos(), one);
        assert_eq!(zero.atan(), zero);
        assert_eq!(zero.sinh().unwrap(), zero);
        assert_eq!(zero.cosh().unwrap(), one);
        assert_eq!(zero.exp().unwrap(), one);
        assert_eq!(one.ln().unwrap(), zero);
        assert_eq!(one.acosh().unwrap(), zero);
        assert_eq!(zero.sqrt().unwrap(), zero);
        assert_eq!(exact("0x1p2", bits).sqrt().unwrap(), exact("0x1p1", bits));
        assert_eq!(exact("0x9p-2", bits).sqrt().unwrap(), exact("0x3p-1", bits));
        assert_eq!(
            exact("0x1p200", bits).sqrt().unwrap(),
            exact("0x1p100", bits)
        );
        assert_eq!(
            exact("0x1p-201", bits).times_power_of_two(1).unwrap(),
            exact("0x1p-200", bits)
        );

        // The quarter turns: atan2 of exact axes is a multiple of pi / 2 correctly rounded, and
        // halving is exact, so these are pi scaled.
        let pi = WideFloat::pi(at(bits));
        let half_pi = pi.times_power_of_two(-1).unwrap();
        assert_eq!(one.atan2(&zero).unwrap(), half_pi);
        assert_eq!((-&one).atan2(&zero).unwrap(), -&half_pi);
        assert_eq!(zero.atan2(&(-&one)).unwrap(), pi);
        assert_eq!(zero.atan2(&one).unwrap(), zero);
        assert_eq!(one.atan2(&one).unwrap(), pi.times_power_of_two(-2).unwrap());
    }
}

#[test]
fn what_is_not_defined_is_an_error_and_never_a_nan() {
    let p = at(128);
    let zero = WideFloat::zero(p);
    let one = WideFloat::from_i64(1, p);
    let minus_one = -&one;
    assert_eq!(one.divide(&zero), Err(NumberError::DivisionByZero));
    assert_eq!(zero.divide(&zero), Err(NumberError::DivisionByZero));
    assert!(matches!(
        minus_one.sqrt(),
        Err(NumberError::OutsideDomain(_))
    ));
    assert!(matches!(zero.ln(), Err(NumberError::OutsideDomain(_))));
    assert!(matches!(minus_one.ln(), Err(NumberError::OutsideDomain(_))));
    assert!(matches!(
        exact("0x1p-1", 128).acosh(),
        Err(NumberError::OutsideDomain(_))
    ));
    assert!(matches!(
        minus_one.acosh(),
        Err(NumberError::OutsideDomain(_))
    ));
    assert!(matches!(
        zero.atan2(&zero),
        Err(NumberError::OutsideDomain(_))
    ));
    assert_eq!(
        WideFloat::from_f64(f64::NAN, p),
        Err(NumberError::NotFinite)
    );
    assert_eq!(
        WideFloat::from_f64(f64::INFINITY, p),
        Err(NumberError::NotFinite)
    );
    assert_eq!(
        WideFloat::from_f64(f64::NEG_INFINITY, p),
        Err(NumberError::NotFinite)
    );
}

#[test]
fn a_result_beyond_the_exponents_is_an_error_and_never_an_infinity_or_a_silent_zero() {
    let p = at(128);
    // e^(2^40) is about 2^(1.6e12), far over the limit of about 2^(1.07e9).
    let huge = exact("0x1p40", 128);
    assert_eq!(huge.exp(), Err(NumberError::ExponentRange));
    assert_eq!(huge.sinh(), Err(NumberError::ExponentRange));
    assert_eq!(huge.cosh(), Err(NumberError::ExponentRange));
    // ... and e^(-2^40) is far under the least number: not zero.
    assert_eq!((-&huge).exp(), Err(NumberError::ExponentRange));
    assert_eq!(
        WideFloat::from_i64(1, p).times_power_of_two(i64::MAX),
        Err(NumberError::ExponentRange)
    );
    assert_eq!(
        WideFloat::from_i64(1, p).times_power_of_two(i64::MIN),
        Err(NumberError::ExponentRange)
    );
    assert_eq!(
        WideFloat::from_parts(false, &[1], i64::MAX, p),
        Err(NumberError::ExponentRange)
    );
    // The same at the other end: a number too small to be a number is an error from `from_parts` and `parse`, and not a zero.
    assert_eq!(
        WideFloat::from_parts(false, &[1], i64::MIN, p),
        Err(NumberError::ExponentRange)
    );
    assert_eq!(
        WideFloat::from_parts(true, &[1], -(1 << 31), p),
        Err(NumberError::ExponentRange)
    );
    assert_eq!(
        WideFloat::parse("0x1p-99999999999", p),
        Err(NumberError::ExponentRange)
    );
    // Zero scaled is zero.
    assert_eq!(
        WideFloat::zero(p).times_power_of_two(i64::MAX).unwrap(),
        WideFloat::zero(p)
    );
}

#[test]
fn the_sine_and_the_cosine_together_are_the_two_alone() {
    for text in [
        "0x1p0",
        "0x3p-1",
        "-0x5p3",
        "0x1fffp-20",
        "0x1p100",
        "0x7p-100",
    ] {
        for bits in [128, 300, 1000] {
            let x = exact(text, bits);
            let (sine, cosine) = x.sin_cos();
            assert_eq!(sine, x.sin(), "sin {text} at {bits}");
            assert_eq!(cosine, x.cos(), "cos {text} at {bits}");
        }
    }
}

#[test]
fn there_is_no_negative_zero() {
    let p = at(128);
    let zero = WideFloat::zero(p);
    let three = WideFloat::from_i64(3, p);
    for product in [
        &zero * &(-&three),
        &(-&three) * &zero,
        -&zero,
        &three - &three,
    ] {
        assert_eq!(product, zero);
        assert!(!product.is_negative());
        assert!(product.to_parts().is_none());
        assert_eq!(product.to_f64().to_bits(), 0f64.to_bits());
    }
    assert_eq!(WideFloat::from_f64(-0.0, p).unwrap(), zero);
}

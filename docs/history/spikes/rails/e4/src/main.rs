//! E4 of `docs/orbits-and-thrust-plan.md`: what do big numbers cost, and what do they give? THROWAWAY, kept to reproduce the numbers there.
//!
//! Four pure-Rust libraries that have the elementary functions an orbit needs (`dashu-float` 0.6.2, `malachite-float` 0.14.0, `astro-float` 0.9.6, `puremp` 0.3.2), measured through one
//! small trait (`libs.rs`). Build it where it is (it has a workspace of its own; it is not part of the game's) and run:
//!
//!     cargo run --release -- speed [puremp]        # the time of each operation at 128 to 65,536 bits
//!     cargo run --release -- limits LIB            # the same up to 8 million bits (LIB: dashu, malachite, astro, puremp)
//!     cargo run --release -- agree                 # the elementary functions: how far each library is from the exact value, and whether all four give the same bits
//!     cargo run --release -- binary                # + - x /: how many results each library rounds as the exact result rounded to nearest (checked against whole-number arithmetic)
//!     cargo run --release -- dump > dump.txt       # results for `python e4_check_functions.py dump.txt`, which checks them against mpmath
//!     cargo run --release -- kepler                # the time of a Kepler solve on the owner's orbit (nearest point 7,000 km, farthest one light year) at each width
//!     cargo run --release -- chain LIB FILE P      # a chain of flybys (from `python e4_chain.py export`) at P bits with LIB; prints the time and the last periapsis as an exact number
//!     cargo run --release -- ball FILE P           # the same chain in ball arithmetic (puremp::Ball): the time and the radius of the range of the last periapsis
//!
//! The chains are made by `e4_chain.py` (`design`, `ladder`, `legs`, `adaptive`, `interval`; see its header), which is also the truth that `chain` is compared with (`compare`).
//! `E4_TRACE=1` makes `chain` print every periapsis. The 126-bit fixed-point rail it is compared with is timed by `../e4_baseline_test.rs`.

use std::cmp::Ordering;
use std::time::Instant;

use dashu_base::{BitTest, UnsignedAbs};
use dashu_int::{IBig, UBig};

mod ballchain;
mod chain;
mod libs;

use libs::{Astro, Dashu, Lib, Malachite, Puremp};

/// An exact binary number as its three parts: value = (-1)^negative x mantissa x 2^exponent, the mantissa odd (or zero, with exponent 0).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Canon {
    pub negative: bool,
    pub mantissa: UBig,
    pub exponent: i64,
}

impl Canon {
    pub fn new(negative: bool, mantissa: UBig, exponent: i64) -> Self {
        if mantissa == UBig::ZERO {
            return Self {
                negative: false,
                mantissa,
                exponent: 0,
            };
        }
        let zeros = mantissa.trailing_zeros().unwrap();
        Self {
            negative,
            mantissa: mantissa >> zeros,
            exponent: exponent + zeros as i64,
        }
    }

    pub fn from_signed(m: IBig, exponent: i64) -> Self {
        let negative = m < IBig::ZERO;
        Self::new(negative, m.unsigned_abs(), exponent)
    }

    /// `a - b` in units of the last place of a number with `bits` bits and the size of `reference` (an ulp = 2^(top bit of reference - bits + 1)).
    pub fn difference_in_ulps(&self, other: &Canon, reference: &Canon, bits: usize) -> f64 {
        let low = self.exponent.min(other.exponent);
        let as_int = |c: &Canon| {
            let shifted = IBig::from(c.mantissa.clone()) << ((c.exponent - low) as usize);
            if c.negative {
                -shifted
            } else {
                shifted
            }
        };
        let difference = (as_int(self) - as_int(other)).unsigned_abs();
        if difference == UBig::ZERO {
            return 0.0;
        }
        // The ulp: 2^(exponent of the top bit of the reference - (bits - 1)), relative to 2^low.
        let top = reference.mantissa.bit_len() as i64 + reference.exponent - 1;
        let ulp_exponent = top - (bits as i64 - 1);
        // difference x 2^low / 2^ulp_exponent
        let shift = low - ulp_exponent;
        let bits_of_difference = difference.bit_len() as i64;
        // Work in f64 from the top 53 bits.
        let take = bits_of_difference.min(60);
        let top_part = (&difference >> ((bits_of_difference - take) as usize)).to_f64().value();
        top_part * 2f64.powi((bits_of_difference - take + shift) as i32)
    }
}

/// Time `work` for about `budget_ms` milliseconds (at least once, and at most 3 seconds when a single call is slow); nanoseconds per call.
fn time_per_call(mut work: impl FnMut(), budget_ms: u128) -> f64 {
    let start = Instant::now();
    work();
    let first = start.elapsed();
    if first.as_millis() >= budget_ms {
        return first.as_nanos() as f64;
    }
    let mut calls = 1u64;
    let begin = Instant::now();
    while begin.elapsed().as_millis() < budget_ms {
        work();
        calls += 1;
    }
    begin.elapsed().as_nanos() as f64 / calls as f64
}

fn human(nanoseconds: f64) -> String {
    if nanoseconds < 1e3 {
        format!("{nanoseconds:.0} ns")
    } else if nanoseconds < 1e6 {
        format!("{:.1} us", nanoseconds / 1e3)
    } else if nanoseconds < 1e9 {
        format!("{:.2} ms", nanoseconds / 1e6)
    } else {
        format!("{:.2} s", nanoseconds / 1e9)
    }
}

const WIDTHS: [usize; 10] = [128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536];

fn speed<L: Lib>() {
    println!("speed: {}", L::NAME);
    for &p in &WIDTHS {
        let mut c = L::ctx(p);
        let two = L::from_i128(&mut c, 2);
        let three = L::from_i128(&mut c, 3);
        let x = L::sqrt(&mut c, &two);
        let y = L::sqrt(&mut c, &three);
        let two_again = L::from_i128(&mut c, 2);
        let big = L::add(&mut c, &x, &two_again);
        let budget = if p >= 16384 { 300 } else { 120 };
        let mut row = format!("  {p:>6} bits:");
        let mut entry = |name: &str, nanoseconds: f64| {
            row.push_str(&format!(" {name} {}", human(nanoseconds)));
        };
        entry("mul", time_per_call(|| { std::hint::black_box(L::mul(&mut c, &x, &y)); }, budget));
        entry("div", time_per_call(|| { std::hint::black_box(L::div(&mut c, &x, &y)); }, budget));
        entry("sqrt", time_per_call(|| { std::hint::black_box(L::sqrt(&mut c, &x)); }, budget));
        let slow = p > 16384;
        if !slow {
            entry("sin", time_per_call(|| { std::hint::black_box(L::sin(&mut c, &x)); }, budget));
            entry("cos", time_per_call(|| { std::hint::black_box(L::cos(&mut c, &x)); }, budget));
            entry("atan", time_per_call(|| { std::hint::black_box(L::atan(&mut c, &x)); }, budget));
            entry("sinh", time_per_call(|| { std::hint::black_box(L::sinh(&mut c, &x)); }, budget));
            entry("acosh", time_per_call(|| { std::hint::black_box(L::acosh(&mut c, &big)); }, budget));
            entry("exp", time_per_call(|| { std::hint::black_box(L::exp(&mut c, &x)); }, budget));
            entry("ln", time_per_call(|| { std::hint::black_box(L::ln(&mut c, &x)); }, budget));
        }
        println!("{row}");
    }
}

/// A small deterministic generator of numbers in [1, 2) as fractions of 2^30, so that every library gets the same inputs.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// An input that is not an exact short number: sqrt(k) / m for small whole numbers, at the working precision.
fn input<L: Lib>(c: &mut L::Ctx, rng: &mut Lcg, low: i128, high: i128) -> L::N {
    let k = low + (rng.next() as i128 % (high - low));
    let m = 1 + (rng.next() as i128 % 7);
    let k_n = L::from_i128(c, k);
    let root = L::sqrt(c, &k_n);
    let m_n = L::from_i128(c, m);
    L::div(c, &root, &m_n)
}

fn agree() {
    // For each function and width: the three libraries on the same inputs. An input is made in each library from exact integers by the same operations (a square root and a
    // division), so the inputs are the same number unless a library rounds that differently; the check says how many of the samples had all three inputs the same, and, of
    // those, how many results were the same bit for bit. The error of each result is against dashu at 192 bits more on the same exact input (a result correctly rounded is within
    // half a unit in the last place, 0.5, of the exact value).
    println!("agree: worst error in units of the last place at the width (0.5 is a correctly rounded result; more is not)");
    for &p in &[128usize, 256, 1024, 4096] {
        for name in ["sqrt", "sin", "cos", "atan", "sinh", "acosh", "exp", "ln"] {
            let mut worst = [0f64; 4];
            let samples = if p >= 4096 { 12 } else { 40 };
            let (mut same_inputs, mut same_results) = (0usize, 0usize);
            for sample in 0..samples {
                let (low, high) = match name {
                    "acosh" => (3, 4000),
                    "sinh" | "exp" => (2, 200),
                    "ln" => (2, 100000),
                    _ => (2, 60),
                };
                let results = [
                    one::<Dashu>(name, p, sample, low, high),
                    one::<Malachite>(name, p, sample, low, high),
                    one::<Astro>(name, p, sample, low, high),
                    one::<Puremp>(name, p, sample, low, high),
                ];
                let inputs_agree = results.iter().all(|r| r.1 == results[0].1);
                if inputs_agree {
                    same_inputs += 1;
                    if results.iter().all(|r| r.0 == results[0].0) {
                        same_results += 1;
                    }
                }
                for (index, (result, input_used)) in results.iter().enumerate() {
                    let truth = reference_for(name, p + 192, input_used);
                    let ulps = result.difference_in_ulps(&truth, &truth, p);
                    worst[index] = worst[index].max(ulps);
                }
            }
            println!(
                "  {p:>5} bits {name:>6}: worst error (ulp) dashu {:.2}, malachite {:.2}, astro {:.2}, puremp {:.2};  all four inputs the same in {same_inputs}/{samples}, and then all four results the same bit for bit in {same_results}",
                worst[0], worst[1], worst[2], worst[3]
            );
        }
    }
}

/// One function of one library on a made input: (the result, the input as an exact number).
fn one<L: Lib>(name: &str, p: usize, sample: usize, low: i128, high: i128) -> (Canon, Canon) {
    let mut c = L::ctx(p);
    let mut rng = Lcg(0x9e3779b97f4a7c15 ^ (sample as u64 * 7919));
    let mut x = input::<L>(&mut c, &mut rng, low, high);
    if name == "acosh" {
        let one = L::from_i128(&mut c, 1);
        x = L::add(&mut c, &x, &one);
    }
    let result = match name {
        "sqrt" => L::sqrt(&mut c, &x),
        "sin" => L::sin(&mut c, &x),
        "cos" => L::cos(&mut c, &x),
        "atan" => L::atan(&mut c, &x),
        "sinh" => L::sinh(&mut c, &x),
        "acosh" => L::acosh(&mut c, &x),
        "exp" => L::exp(&mut c, &x),
        "ln" => L::ln(&mut c, &x),
        _ => unreachable!(),
    };
    (L::canon(&result), L::canon(&x))
}

/// The function of an exact input, by dashu at the stated width (a result correctly rounded, meant to be the truth to within half an ulp of that width).
fn reference_for(name: &str, p: usize, input: &Canon) -> Canon {
    let mut c = Dashu::ctx(p);
    let x = Dashu::from_canon(&mut c, input);
    let result = match name {
        "sqrt" => Dashu::sqrt(&mut c, &x),
        "sin" => Dashu::sin(&mut c, &x),
        "cos" => Dashu::cos(&mut c, &x),
        "atan" => Dashu::atan(&mut c, &x),
        "sinh" => Dashu::sinh(&mut c, &x),
        "acosh" => Dashu::acosh(&mut c, &x),
        "exp" => Dashu::exp(&mut c, &x),
        "ln" => Dashu::ln(&mut c, &x),
        _ => unreachable!(),
    };
    Dashu::canon(&result)
}


/// `numerator x 2^exponent` (plus a little more if `sticky`) rounded to `bits` bits, to nearest, ties to even; the numerator must have at least `bits + 2` bits.
fn round_exact(negative: bool, numerator: UBig, exponent: i64, sticky: bool, bits: usize) -> Canon {
    let length = numerator.bit_len();
    assert!(length >= bits + 2, "shift the numerator up first");
    let drop = length - bits;
    let mut quotient = &numerator >> drop;
    let remainder = &numerator - (&quotient << drop);
    let half = UBig::ONE << (drop - 1);
    let up = remainder > half || (remainder == half && (sticky || quotient.bit(0)));
    if up {
        quotient += UBig::ONE;
    }
    Canon::new(negative, quotient, exponent + drop as i64)
}

/// The exact result of a binary operation on two exact numbers, rounded to `bits` bits.
fn exact_binary(operation: &str, x: &Canon, y: &Canon, bits: usize) -> Canon {
    let signed = |c: &Canon| if c.negative { -IBig::from(c.mantissa.clone()) } else { IBig::from(c.mantissa.clone()) };
    match operation {
        "mul" => {
            let product = signed(x) * signed(y);
            let negative = product < IBig::ZERO;
            let magnitude = product.unsigned_abs();
            let pad = (bits + 2).saturating_sub(magnitude.bit_len());
            round_exact(negative, magnitude << pad, x.exponent + y.exponent - pad as i64, false, bits)
        }
        "add" | "sub" => {
            let low = x.exponent.min(y.exponent);
            let a = signed(x) << ((x.exponent - low) as usize);
            let b = signed(y) << ((y.exponent - low) as usize);
            let sum = if operation == "add" { a + b } else { a - b };
            let negative = sum < IBig::ZERO;
            let magnitude = sum.unsigned_abs();
            if magnitude == UBig::ZERO {
                return Canon::new(false, magnitude, 0);
            }
            let pad = (bits + 2).saturating_sub(magnitude.bit_len());
            round_exact(negative, magnitude << pad, low - pad as i64, false, bits)
        }
        "div" => {
            // x / y = (mx << s) / my x 2^(ex - ey - s), with s making the quotient at least bits + 2 long.
            let shift = bits + 4 + y.mantissa.bit_len();
            let numerator = &x.mantissa << shift;
            let quotient = &numerator / &y.mantissa;
            let remainder = &numerator - &quotient * &y.mantissa;
            round_exact(x.negative != y.negative, quotient, x.exponent - y.exponent - shift as i64, remainder != UBig::ZERO, bits)
        }
        _ => unreachable!(),
    }
}

fn binary<L: Lib>(operation: &str, p: usize, x: &Canon, y: &Canon) -> Canon {
    let mut c = L::ctx(p);
    let a = L::from_canon(&mut c, x);
    let b = L::from_canon(&mut c, y);
    assert_eq!(&L::canon(&a), x, "{} does not hold the input {x:?} as it is", L::NAME);
    let r = match operation {
        "add" => L::add(&mut c, &a, &b),
        "sub" => L::sub(&mut c, &a, &b),
        "mul" => L::mul(&mut c, &a, &b),
        "div" => L::div(&mut c, &a, &b),
        _ => unreachable!(),
    };
    L::canon(&r)
}

fn agree_binary() {
    println!("agree_binary: + - x / on inputs of the width: how many of 300 pairs each library rounds as the exact result rounded to nearest, ties to even");
    for &p in &[128usize, 256, 1024, 4096] {
        for operation in ["add", "sub", "mul", "div"] {
            let mut wrong = [0usize; 4];
            let mut worst = [0f64; 4];
            let mut wrong_when_cancelling = [0usize; 4];
            for sample in 0..300usize {
                let mut rng = Lcg(0xdeadbeef ^ (sample as u64 * 104729) ^ p as u64);
                // Two inputs of the width, made by dashu: sqrt(k) over a small number, and for + and - one of a quite different size so that the sum cancels or does not.
                let mut c = Dashu::ctx(p);
                let x = input::<Dashu>(&mut c, &mut rng, 2, 100000);
                let y = if operation == "add" || operation == "sub" {
                    let near = input::<Dashu>(&mut c, &mut rng, 2, 100000);
                    if sample % 3 == 0 {
                        // Nearly the same number as x, so that x - y cancels most of the bits.
                        let tiny = Dashu::from_f64(&mut c, 1.0e-30);
                        let moved = Dashu::mul(&mut c, &x, &tiny);
                        Dashu::add(&mut c, &x, &moved)
                    } else {
                        near
                    }
                } else {
                    input::<Dashu>(&mut c, &mut rng, 2, 100000)
                };
                let (cx, cy) = (Dashu::canon(&x), Dashu::canon(&y));
                let expected = exact_binary(operation, &cx, &cy, p);
                let got = [binary::<Dashu>(operation, p, &cx, &cy), binary::<Malachite>(operation, p, &cx, &cy), binary::<Astro>(operation, p, &cx, &cy), binary::<Puremp>(operation, p, &cx, &cy)];
                for index in 0..4 {
                    if got[index] != expected {
                        if index == 3 && wrong[3] == 0 && std::env::var("E4_DEBUG").is_ok() {
                            let show = |c: &Canon| format!("{}{:x}*2^{}", if c.negative { "-" } else { "" }, c.mantissa, c.exponent);
                            eprintln!("DEBUG {operation} {p}: x={} y={} expected={} puremp={}", show(&cx), show(&cy), show(&expected), show(&got[3]));
                        }
                        wrong[index] += 1;
                        if sample % 3 == 0 && (operation == "add" || operation == "sub") {
                            wrong_when_cancelling[index] += 1;
                        }
                        // Size of the mistake in units of the last place of the exact result.
                        worst[index] = worst[index].max(got[index].difference_in_ulps(&expected, &expected, p));
                    }
                }
            }
            println!(
                "  {p:>5} bits {operation}: not the correctly rounded result: dashu {}, malachite {}, astro {}, puremp {} of 300 (of them in the 100 pairs that nearly cancel: {}, {}, {}, {}); worst mistake in ulps of the result: {:.1}, {:.1}, {:.1}, {:.1}",
                wrong[0], wrong[1], wrong[2], wrong[3], wrong_when_cancelling[0], wrong_when_cancelling[1], wrong_when_cancelling[2], wrong_when_cancelling[3], worst[0], worst[1], worst[2], worst[3]
            );
        }
    }
}


fn limits<L: Lib>() {
    println!("limits: {}", L::NAME);
    let mut slowest: std::collections::BTreeMap<&str, f64> = std::collections::BTreeMap::new();
    for shift in 17..=23 {
        let p = 1usize << shift;
        let mut c = L::ctx(p);
        let two = L::from_i128(&mut c, 2);
        let three = L::from_i128(&mut c, 3);
        let x = L::sqrt(&mut c, &two);
        let y = L::sqrt(&mut c, &three);
        let mut row = format!("  2^{shift:<2} = {p:>8} bits:");
        let mut one_call = |name: &'static str, work: &mut dyn FnMut()| {
            // Skip an operation whose time at the width before, times 4.5 (it costs about p^1.2 to p^1.7), would be over a minute.
            if slowest.get(name).is_some_and(|last| last * 4.5 > 60e9) {
                row.push_str(&format!(" {name} (skipped: over a minute)"));
                return;
            }
            let began = Instant::now();
            work();
            let took = began.elapsed().as_nanos() as f64;
            slowest.insert(name, took);
            row.push_str(&format!(" {name} {}", human(took)));
        };
        one_call("mul", &mut || { std::hint::black_box(L::mul(&mut c, &x, &y)); });
        one_call("div", &mut || { std::hint::black_box(L::div(&mut c, &x, &y)); });
        one_call("sqrt", &mut || { std::hint::black_box(L::sqrt(&mut c, &x)); });
        one_call("exp", &mut || { std::hint::black_box(L::exp(&mut c, &x)); });
        one_call("sin", &mut || { std::hint::black_box(L::sin(&mut c, &x)); });
        println!("{row}");
    }
}

fn kepler<L: Lib>() {
    println!("kepler: {}", L::NAME);
    // The owner's orbit: nearest 7e6 m, farthest 9.4607e15 m; 1 - e = 7e6 / a.
    let axis = (7.0e6 + 9.4607e15) / 2.0;
    let ratio = 7.0e6 / axis;
    for &mean in &[1.0e-3f64, 1.0, 3.0] {
        // The starting guess, in f64.
        let e64 = 1.0 - ratio;
        let mut guess = mean.cbrt().min(3.0).max(1e-3);
        for _ in 0..100 {
            let f = guess - e64 * guess.sin() - mean;
            let df = 1.0 - e64 * guess.cos();
            guess -= f / df;
        }
        for &p in &WIDTHS {
            if p > 16384 {
                continue;
            }
            let mut c = L::ctx(p);
            let one = L::from_i128(&mut c, 1);
            let ratio_n = L::from_f64(&mut c, ratio);
            let e = L::sub(&mut c, &one, &ratio_n);
            let m = L::from_f64(&mut c, mean);
            let mut iterations = 0;
            let mut result = None;
            let nanoseconds = time_per_call(
                || {
                    let mut x = L::from_f64(&mut c, guess);
                    // Newton's method doubles the bits each step; the guess has about 20.
                    let steps = ((p as f64 / 20.0).log2().ceil() as usize) + 2;
                    for _ in 0..steps {
                        let (s, co) = L::sin_cos(&mut c, &x);
                        let es = L::mul(&mut c, &e, &s);
                        let x_less = L::sub(&mut c, &x, &es);
                        let f = L::sub(&mut c, &x_less, &m);
                        let eco = L::mul(&mut c, &e, &co);
                        let df = L::sub(&mut c, &one, &eco);
                        let step = L::div(&mut c, &f, &df);
                        x = L::sub(&mut c, &x, &step);
                    }
                    iterations = steps;
                    result = Some(x);
                },
                if p >= 8192 { 400 } else { 150 },
            );
            // The residual of the answer, in units of 2^-p.
            let x = result.unwrap();
            let (s, _) = L::sin_cos(&mut c, &x);
            let es = L::mul(&mut c, &e, &s);
            let x_less = L::sub(&mut c, &x, &es);
            let residual = L::sub(&mut c, &x_less, &m);
            let residual_bits = L::canon(&residual);
            let size = if residual_bits.mantissa == UBig::ZERO {
                f64::NEG_INFINITY
            } else {
                (residual_bits.mantissa.bit_len() as i64 + residual_bits.exponent) as f64
            };
            println!(
                "  M = {mean:>6} {p:>6} bits: {} for {iterations} Newton steps; the residual is about 2^{size} (E = {:.6})",
                human(nanoseconds),
                L::to_f64(&x)
            );
        }
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    match arguments.get(1).map(String::as_str) {
        Some("speed") if arguments.get(2).map(String::as_str) == Some("puremp") => speed::<Puremp>(),
        Some("speed") => {
            speed::<Dashu>();
            speed::<Malachite>();
            speed::<Astro>();
            speed::<Puremp>();
        }
        Some("agree") => agree(),
        Some("binary") => agree_binary(),
        Some("limits") => match arguments.get(2).map(String::as_str) {
            Some("dashu") => limits::<Dashu>(),
            Some("malachite") => limits::<Malachite>(),
            Some("astro") => limits::<Astro>(),
            Some("puremp") => limits::<Puremp>(),
            _ => eprintln!("limits LIB"),
        },
        Some("dump") => {
            // For the check against mpmath (`e4_check_functions.py`): the input and the result of dashu, as exact numbers.
            for &p in &[256usize, 1024, 4096] {
                for name in ["sqrt", "sin", "cos", "atan", "sinh", "acosh", "exp", "ln"] {
                    for sample in 0..6 {
                        let (low, high) = match name {
                            "acosh" => (3, 4000),
                            "sinh" | "exp" => (2, 200),
                            "ln" => (2, 100000),
                            _ => (2, 60),
                        };
                        let (result, input) = one::<Dashu>(name, p, sample, low, high);
                        let show = |c: &Canon| format!("{}{:x}*2^{}", if c.negative { "-" } else { "" }, c.mantissa, c.exponent);
                        println!("DUMP {name} {p} {} {}", show(&input), show(&result));
                    }
                }
            }
        }
        Some("kepler") => {
            kepler::<Dashu>();
            kepler::<Malachite>();
            kepler::<Astro>();
            kepler::<Puremp>();
        }
        Some("ball") => ballchain::run(&arguments[2], arguments[3].parse().unwrap()),
        Some("chain") => {
            let precision: usize = arguments[4].parse().unwrap();
            match arguments[2].as_str() {
                "dashu" => chain::run::<Dashu>(&arguments[3], precision),
                "malachite" => chain::run::<Malachite>(&arguments[3], precision),
                "astro" => chain::run::<Astro>(&arguments[3], precision),
                "puremp" => chain::run::<Puremp>(&arguments[3], precision),
                other => panic!("no library {other}"),
            }
        }
        _ => eprintln!("speed | agree | kepler | chain LIB FILE P"),
    }
    let _ = Ordering::Equal;
}

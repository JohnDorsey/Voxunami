//! What the tests that read reference rows (`orbits.txt` for ellipses, `hyperbola.txt` for hyperbolas) share: the rows, and how far an answer is from one.

#![allow(dead_code)]

use wide_float::{Precision, WideFloat};
use wide_kepler::{Elements, Time};

pub fn at(bits: u64) -> Precision {
    Precision::new(bits).unwrap()
}

pub fn number(text: &str, bits: u64) -> WideFloat {
    WideFloat::parse(text, at(bits)).unwrap_or_else(|error| panic!("{text}: {error}"))
}

/// A line of a reference file: the elements, a time, and the place and the speed they give, the truth to `bits + 64` bits.
pub struct Row {
    pub bits: u64,
    pub elements: Elements,
    pub time: Time,
    pub place: Vec<WideFloat>,
    pub velocity: Vec<WideFloat>,
}

pub fn rows(source: &str) -> Vec<Row> {
    source
        .lines()
        .map(|line| {
            let words: Vec<&str> = line.split(' ').collect();
            assert_eq!(words[0], "orbit");
            let bits: u64 = words[1].parse().unwrap();
            let elements: Vec<WideFloat> =
                words[2..10].iter().map(|word| number(word, bits)).collect();
            let answer: Vec<WideFloat> = words[12..18]
                .iter()
                .map(|word| number(word, bits + 64))
                .collect();
            Row {
                bits,
                elements: Elements {
                    normal: [
                        elements[0].clone(),
                        elements[1].clone(),
                        elements[2].clone(),
                    ],
                    radius: elements[3].clone(),
                    ratio: elements[4].clone(),
                    periapsis: elements[5].clone(),
                    period: elements[6].clone(),
                    phase: elements[7].clone(),
                },
                time: Time {
                    units: words[10].parse().unwrap(),
                    units_per_second: words[11].parse().unwrap(),
                },
                place: answer[..3].to_vec(),
                velocity: answer[3..].to_vec(),
            }
        })
        .collect()
}

/// How many bits of `bits` the worst component of `got` is short of `expected`, in units of the scale: `bits + log2(error / scale)`; very negative for an exact answer.
pub fn loss(bits: u64, got: &[WideFloat], expected: &[WideFloat], scale: &WideFloat) -> i64 {
    let scale = scale.floor_log2().unwrap_or(i64::MIN / 2);
    got.iter()
        .zip(expected)
        .map(|(got, expected)| {
            let error = (got - expected).abs();
            error
                .floor_log2()
                .map_or(i64::MIN / 2, |exponent| bits as i64 + exponent - scale)
        })
        .max()
        .unwrap()
}

pub fn largest(numbers: &[WideFloat]) -> WideFloat {
    numbers
        .iter()
        .map(WideFloat::abs)
        .max_by(|a, b| a.compare(b))
        .unwrap()
}

/// A line of a fit reference file: elements chosen, a time, the mass parameter, and the state the body is in (worked out forward from the elements, held to `bits + 256` bits).
pub struct FitRow {
    pub bits: u64,
    pub elements: Elements,
    pub time: Time,
    pub mu: WideFloat,
    pub position: [WideFloat; 3],
    pub velocity: [WideFloat; 3],
}

pub fn fit_rows(source: &str) -> Vec<FitRow> {
    source
        .lines()
        .map(|line| {
            let words: Vec<&str> = line.split(' ').collect();
            assert_eq!(words[0], "fit");
            let bits: u64 = words[1].parse().unwrap();
            let element: Vec<WideFloat> =
                words[2..10].iter().map(|word| number(word, bits)).collect();
            let wide = bits + 256;
            let state: Vec<WideFloat> = words[13..19]
                .iter()
                .map(|word| number(word, wide))
                .collect();
            FitRow {
                bits,
                elements: Elements {
                    normal: [element[0].clone(), element[1].clone(), element[2].clone()],
                    radius: element[3].clone(),
                    ratio: element[4].clone(),
                    periapsis: element[5].clone(),
                    period: element[6].clone(),
                    phase: element[7].clone(),
                },
                time: Time {
                    units: words[10].parse().unwrap(),
                    units_per_second: words[11].parse().unwrap(),
                },
                mu: number(words[12], wide),
                position: [state[0].clone(), state[1].clone(), state[2].clone()],
                velocity: [state[3].clone(), state[4].clone(), state[5].clone()],
            }
        })
        .collect()
}

/// `bits + log2(|got - expected| / scale)`: how many bits of the width the difference takes, very negative for an exact answer.
pub fn loss_of(bits: u64, got: &WideFloat, expected: &WideFloat, scale: &WideFloat) -> i64 {
    let high = at(bits + 512);
    let error = (&got.rounded_to(high) - &expected.rounded_to(high)).abs();
    let scale = scale.floor_log2().unwrap_or(i64::MIN / 2);
    error
        .floor_log2()
        .map_or(i64::MIN / 2, |exponent| bits as i64 + exponent - scale)
}

/// An angle taken to (−π, π]: the difference of two angles that are the same one is nothing to this.
pub fn reduced(angle: &WideFloat, bits: u64) -> WideFloat {
    let high = at(bits + 512);
    let turn = WideFloat::pi(high).times_power_of_two(1).unwrap();
    let angle = angle.rounded_to(high);
    let turns = (&angle.divide(&turn).unwrap() + &WideFloat::from_f64(0.5, high).unwrap())
        .floor_to_i128()
        .unwrap();
    &angle - &(&turn * &WideFloat::from_i128(turns, high))
}

pub fn unit(vector: &[WideFloat; 3], bits: u64) -> [WideFloat; 3] {
    let high = at(bits + 512);
    let size = vector
        .iter()
        .map(|part| part.rounded_to(high).square())
        .reduce(|sum, square| &sum + &square)
        .unwrap()
        .sqrt()
        .unwrap();
    vector
        .each_ref()
        .map(|part| part.rounded_to(high).divide(&size).unwrap())
}

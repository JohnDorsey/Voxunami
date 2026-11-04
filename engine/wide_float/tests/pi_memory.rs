//! π is a constant of each width, and the rails ask for it at the same few widths over and over (three times in a place, twice in a fit); `WideFloat::pi` keeps the widths
//! asked for lately. These tests say the memory never changes an answer, whatever was asked before and on whichever thread, and that it is a memory.

use std::time::Instant;

use wide_float::{Precision, WideFloat};

const VECTORS: &str = include_str!("reference/functions.txt");

fn at(bits: u32) -> Precision {
    Precision::of(bits)
}

/// π to `bits` bits from a number with 256 more, rounded: the same as π to `bits` bits unless the 256 bits after the last one are a run of nothing but 0s or 1s, which are not
/// the bits of π. (A second opinion that does not depend on what is kept: the number it rounds is of another width.)
fn pi_from_the_wider(bits: u32) -> WideFloat {
    WideFloat::pi(at(bits + 256)).rounded_to(at(bits))
}

/// `(width, π)` for each line of the reference vectors, which were made by other code (mpmath).
fn reference_pis() -> Vec<(Precision, WideFloat)> {
    VECTORS
        .lines()
        .filter(|line| line.starts_with("pi "))
        .map(|line| {
            let words: Vec<&str> = line.split(' ').collect();
            let precision = Precision::new(words[1].parse().unwrap()).unwrap();
            (precision, WideFloat::parse(words[2], precision).unwrap())
        })
        .collect()
}

#[test]
fn pi_is_the_same_the_first_time_and_every_time_after_in_any_order() {
    let references = reference_pis();
    assert!(references.len() >= 7, "{}", references.len());
    // Other widths first, then the references in the order of the file, backwards, and the file's own again.
    for bits in [200, 4096, 131, 128] {
        assert_eq!(WideFloat::pi(at(bits)).precision(), at(bits));
    }
    for (precision, expected) in references
        .iter()
        .chain(references.iter().rev())
        .chain(&references)
    {
        let pi = WideFloat::pi(*precision);
        assert_eq!(pi.precision(), *precision);
        assert_eq!(&pi, expected, "pi at {precision}");
    }
}

#[test]
fn pi_of_a_width_is_the_rounding_of_pi_of_a_wider_width() {
    for bits in [128, 129, 192, 224, 256, 416, 480, 1000, 2048] {
        for _ in 0..3 {
            assert_eq!(WideFloat::pi(at(bits)), pi_from_the_wider(bits), "{bits}");
        }
    }
}

#[test]
fn more_widths_than_it_keeps_are_all_still_right() {
    // 40 widths asked round and round: each is dropped and worked out again and again, and is right every time.
    let widths: Vec<u32> = (0..40).map(|index| 128 + 37 * index).collect();
    for _ in 0..3 {
        for &bits in &widths {
            let pi = WideFloat::pi(at(bits));
            assert_eq!(pi.precision(), at(bits));
            assert_eq!(pi, pi_from_the_wider(bits), "{bits}");
        }
        // And the other way, so that the one dropped last is asked first.
        for &bits in widths.iter().rev() {
            assert_eq!(WideFloat::pi(at(bits)), pi_from_the_wider(bits), "{bits}");
        }
    }
}

#[test]
fn another_thread_gets_the_same_bits() {
    let widths = [128, 224, 416, 1000];
    let here: Vec<WideFloat> = widths.iter().map(|&bits| WideFloat::pi(at(bits))).collect();
    let there: Vec<WideFloat> =
        std::thread::spawn(move || widths.iter().map(|&bits| WideFloat::pi(at(bits))).collect())
            .join()
            .unwrap();
    assert_eq!(here, there);
}

#[test]
fn a_width_asked_for_again_is_not_worked_out_again() {
    // A width no other test asks for, wide enough that working it out (milliseconds) is far from a copy of it (microseconds). The best of five second askings against the first
    // asking, with a margin of twenty, so that no pause of a busy machine decides it.
    let bits = (1 << 17) + 64;
    let start = Instant::now();
    let first = WideFloat::pi(at(bits));
    let working_out = start.elapsed();
    let mut best_again = std::time::Duration::MAX;
    for _ in 0..5 {
        let start = Instant::now();
        let again = WideFloat::pi(at(bits));
        best_again = best_again.min(start.elapsed());
        assert_eq!(again, first);
    }
    assert!(
        best_again * 20 < working_out,
        "worked out in {working_out:?}, asked again in {best_again:?}"
    );
}

//! THROWAWAY. The cost of the CORDIC sine and cosine of `exact_circle.rs` at 126 bits (native `i128`, copied from the repo), the same algorithm on limbs at 126 bits, and on
//! limbs at 254 bits. The limb version is written once for any width so that the only difference between the last two is the width.
mod tables;

use std::hint::black_box;
use std::time::Instant;
use tables::*;

fn halved(value: i128, bits: usize) -> i128 {
    if bits == 0 { value } else { (value + (1 << (bits - 1))) >> bits }
}

/// As `exact_circle::sine_and_cosine`.
fn native(turns: u128) -> (i128, i128) {
    let quadrant = turns >> 126;
    let mut angle = (turns & ((1 << 126) - 1)) as i128;
    let (mut cosine, mut sine) = (GAIN_128, 0);
    for (step, &step_angle) in ANGLES_128.iter().enumerate() {
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

#[derive(Clone, Copy)]
struct Word<const N: usize>([u64; N]);

impl<const N: usize> Word<N> {
    #[inline(always)]
    fn add(self, other: Self) -> Self {
        let mut result = [0u64; N];
        let mut carry = false;
        for index in 0..N {
            let (first, first_carry) = self.0[index].overflowing_add(other.0[index]);
            let (second, second_carry) = first.overflowing_add(u64::from(carry));
            result[index] = second;
            carry = first_carry || second_carry;
        }
        Word(result)
    }
    #[inline(always)]
    fn sub(self, other: Self) -> Self {
        let mut result = [0u64; N];
        let mut borrow = false;
        for index in 0..N {
            let (first, first_borrow) = self.0[index].overflowing_sub(other.0[index]);
            let (second, second_borrow) = first.overflowing_sub(u64::from(borrow));
            result[index] = second;
            borrow = first_borrow || second_borrow;
        }
        Word(result)
    }
    #[inline(always)]
    fn negative(self) -> bool {
        self.0[N - 1] >> 63 == 1
    }
    #[inline(always)]
    fn bit(position: usize) -> Self {
        let mut result = [0u64; N];
        result[position / 64] = 1 << (position % 64);
        Word(result)
    }
    /// Arithmetic shift right, rounded to the nearest as `halved` does.
    #[inline(always)]
    fn halved(self, bits: usize) -> Self {
        if bits == 0 {
            return self;
        }
        let rounded = self.add(Self::bit(bits - 1));
        let sign = if rounded.negative() { u64::MAX } else { 0 };
        let (limb_shift, bit_shift) = (bits / 64, bits % 64);
        let mut result = [sign; N];
        for index in 0..N {
            let source = index + limb_shift;
            if source >= N {
                break;
            }
            let low = rounded.0[source] >> bit_shift;
            let above = if source + 1 < N { rounded.0[source + 1] } else { sign };
            let high = if bit_shift == 0 { 0 } else { above << (64 - bit_shift) };
            result[index] = low | high;
        }
        Word(result)
    }
    #[inline(always)]
    fn negated(self) -> Self {
        Word([0u64; N]).sub(self)
    }
}

/// The same algorithm for any width: `turns` has `64 N` bits after the point, the results `64 N - 2`.
#[inline(always)]
fn limbs<const N: usize>(turns: [u64; N], angles: &[[u64; N]], gain: [u64; N]) -> (Word<N>, Word<N>) {
    let quadrant = turns[N - 1] >> 62;
    let mut register = turns;
    register[N - 1] &= (1 << 62) - 1;
    let mut angle = Word(register);
    let (mut cosine, mut sine) = (Word(gain), Word([0u64; N]));
    for (step, step_angle) in angles.iter().enumerate() {
        let (from_cosine, from_sine) = (cosine.halved(step), sine.halved(step));
        if !angle.negative() {
            cosine = cosine.sub(from_sine);
            sine = sine.add(from_cosine);
            angle = angle.sub(Word(*step_angle));
        } else {
            cosine = cosine.add(from_sine);
            sine = sine.sub(from_cosine);
            angle = angle.add(Word(*step_angle));
        }
    }
    match quadrant {
        0 => (sine, cosine),
        1 => (cosine, sine.negated()),
        2 => (sine.negated(), cosine.negated()),
        _ => (cosine.negated(), sine),
    }
}

fn mix(index: u64) -> [u64; 4] {
    let mut state = index.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(0x1234_5678_9ABC_DEF1);
    let mut out = [0u64; 4];
    for limb in out.iter_mut() {
        state ^= state >> 30;
        state = state.wrapping_mul(0xBF58476D1CE4E5B9);
        state ^= state >> 27;
        state = state.wrapping_mul(0x94D049BB133111EB);
        state ^= state >> 31;
        *limb = state;
    }
    out
}

fn bench(name: &str, count: u64, function: impl Fn(u64) -> u64) -> f64 {
    let mut accumulator = 0u64;
    for index in 0..count / 10 {
        accumulator ^= function(index);
    }
    let started = Instant::now();
    for index in 0..count {
        accumulator ^= function(index);
    }
    let seconds = started.elapsed().as_secs_f64();
    black_box(accumulator);
    let nanoseconds = seconds * 1e9 / count as f64;
    println!("{name:<44} {nanoseconds:9.0} ns per sine and cosine");
    nanoseconds
}

fn hex(word: &[u64]) -> String {
    word.iter().rev().map(|limb| format!("{limb:016x}")).collect()
}

fn main() {
    // correctness: print results for a few angles, checked against mpmath by check.py
    for index in [1u64, 2, 3, 4, 5, 6] {
        let wide = mix(index);
        let turns_128 = (u128::from(wide[1]) << 64) | u128::from(wide[0]);
        let (sine, cosine) = native(turns_128);
        let (limb_sine, limb_cosine) = limbs::<2>([wide[0], wide[1]], &ANGLES_2, GAIN_2);
        assert_eq!(sine as u128, (u128::from(limb_sine.0[1]) << 64) | u128::from(limb_sine.0[0]), "limb sine differs from native");
        assert_eq!(cosine as u128, (u128::from(limb_cosine.0[1]) << 64) | u128::from(limb_cosine.0[0]), "limb cosine differs from native");
        let (wide_sine, wide_cosine) = limbs::<4>(wide, &ANGLES_4, GAIN_4);
        println!("CHECK4 {} {} {}", hex(&wide), hex(&wide_sine.0), hex(&wide_cosine.0));
        let (three_sine, three_cosine) = limbs::<3>([wide[0], wide[1], wide[2]], &ANGLES_3, GAIN_3);
        println!("CHECK3 {} {} {}", hex(&wide[..3]), hex(&three_sine.0), hex(&three_cosine.0));
    }
    println!("limb version at 126 bits is bit-identical to the native i128 version on the six angles tried");

    let count = 400_000;
    let native_cost = bench("native i128, 126 bits (the repo today)", count, |index| {
        let wide = mix(index);
        let (sine, cosine) = native((u128::from(wide[1]) << 64) | u128::from(wide[0]));
        (sine ^ cosine) as u64
    });
    let two_cost = bench("limbs [u64; 2], 126 bits", count, |index| {
        let wide = mix(index);
        let (sine, cosine) = limbs::<2>([wide[0], wide[1]], &ANGLES_2, GAIN_2);
        sine.0[0] ^ cosine.0[0]
    });
    let three_cost = bench("limbs [u64; 3], 190 bits", count, |index| {
        let wide = mix(index);
        let (sine, cosine) = limbs::<3>([wide[0], wide[1], wide[2]], &ANGLES_3, GAIN_3);
        sine.0[0] ^ cosine.0[0]
    });
    let four_cost = bench("limbs [u64; 4], 254 bits", count, |index| {
        let (sine, cosine) = limbs::<4>(mix(index), &ANGLES_4, GAIN_4);
        sine.0[0] ^ cosine.0[0]
    });
    println!("190 bits costs {:.1} x the native 126-bit sine and cosine", three_cost / native_cost);
    println!("254 bits costs {:.1} x the native 126-bit sine and cosine, {:.1} x the same code at 126 bits", four_cost / native_cost, four_cost / two_cost);
}

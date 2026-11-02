//! Kepler's equation in whole numbers, so that a body on an ellipse is where its orbit puts it to a small fraction of a voxel, however far out and however long the ellipse
//! (`docs/orbits-and-thrust-plan.md`, section A; the definition and the numbers it is checked against are in `engine/world_storage/tools/exact_ellipse_reference.py`).
//!
//! An ellipse is held as its `ratio` = 1 - e (the periapsis over the semi-major axis), not as the eccentricity: an `f64` e near 1 has no digits left to say how near, where a
//! ratio of 1e-18 is exact. The mean anomaly is a fraction of a turn with 128 bits, as the angle of a circle is, and the answer is the eccentric anomaly E in turns with
//! 128 bits and its sine and cosine with 126 (from `exact_circle`), from which the place in the plane is x = a (cos E - e) and y = a sqrt(1 - e^2) sin E.
//!
//! The equation E - e sin E = M is solved by Newton's method in turns, E - (e / 2 pi) sin(2 pi E) = M, from a first guess made in `f64` (a cubic where e is large and
//! the usual M + e sin M where it is small, then a few Newton steps in `f64`, written without the cancellation that E - e sin E has near E = 0), and then
//! [`STEPS`] steps in whole numbers, each a sine and cosine and one division. A guess to 1e-15 is two steps from 1e-38, so three is one to spare, and the count is a
//! count, never a time. A mean anomaly in the second half of the turn is solved as the mirror image in the first, so that E is only looked for between 0 and a half turn,
//! where the equation has a slope (1 - e cos E) between the ratio and 2 and Newton's method from either side is safe.
//!
//! How exact the answer can be is set by that slope: a rounding in the sine of a few units of 2^-126 is a rounding in E of a few units over the slope, and the slope is as
//! small as the ratio. So E is right to about 12 units of 2^-128 turn for each unit of 1 / (1 - e cos E) (measured; see the tests), which is most of the time within the
//! arithmetic's own few units, and a million times more only for the sharpest of ellipses at their periapsis (a ratio of 1e-18), where it is still a part in 10^20 of a turn.
//!
//! What it costs: three Newton steps, each a sine and cosine (about 130 steps of shifts and adds) and a long division (128 steps), and one more sine and cosine for the answer:
//! some microseconds.

use std::f64::consts::{PI, TAU};

use crate::exact_circle::{Fixed, ONE, fixed_of, multiplied, sine_and_cosine};
use crate::wide::Wide;

/// One over two pi with 126 bits after the point, rounded to the nearest (`exact_ellipse_reference.py constants`).
const ONE_OVER_TAU: Fixed = 0x0a2f9836e4e441529fc2757d1f534ddc;

/// A half turn, in turns with 128 bits after the point.
const HALF_TURN: u128 = 1 << 127;

/// 2^128, the turns in a turn, as an `f64`.
const TURN: f64 = (1u128 << 127) as f64 * 2.0;

/// The Newton steps in whole numbers after the first guess.
const STEPS: usize = 3;

/// The smallest ratio, 2^-64: the ratio is kept in 126 bits after the point, and an `f64` of at least this has no bit below 2^-116, so it is kept exactly.
pub const SMALLEST_RATIO: f64 = 1.0 / ((1u128 << 64) as f64);

/// An ellipse, from what is worked out once for it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ellipse {
    /// 1 - e, exactly.
    ratio: Fixed,
    /// The `ratio` as given.
    ratio_float: f64,
    /// e.
    eccentricity: Fixed,
    /// e over two pi: the size of the sine in the equation when it is written in turns.
    eccentricity_in_turns: Fixed,
    /// sqrt(1 - e^2) = sqrt(ratio (2 - ratio)), rounded down.
    minor: Fixed,
}

/// Where a body is on an ellipse, from the semi-major axis a: the eccentric anomaly and what is made of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Anomaly {
    /// The eccentric anomaly E, in turns with 128 bits after the point.
    pub(crate) turns: u128,
    /// sin E and cos E.
    pub(crate) sine: Fixed,
    pub(crate) cosine: Fixed,
    /// The place in the plane in units of a, from the focus: x toward the periapsis is cos E - e, and y a quarter turn on is sqrt(1 - e^2) sin E.
    pub(crate) x: Fixed,
    pub(crate) y: Fixed,
    /// The distance from the focus in units of a: 1 - e cos E, which is also the slope of Kepler's equation.
    pub(crate) distance: Fixed,
    /// The second part of the direction of travel, sqrt(1 - e^2) cos E (the first is -sin E); the speed is that over the distance, times n a.
    pub(crate) velocity_y: Fixed,
}

impl Ellipse {
    /// The ellipse with this `ratio` (1 - e), from [`SMALLEST_RATIO`] up to 1 (a circle).
    pub(crate) fn new(ratio: f64) -> Self {
        debug_assert!((SMALLEST_RATIO..=1.0).contains(&ratio));
        let kept = fixed_of(ratio, 126);
        let eccentricity = ONE - kept;
        // 1 - e^2 = ratio (2 - ratio): both are whole numbers of 2^-126, so their product is a whole number of 2^-252, and the root of that is one of 2^-126.
        let product = Wide::of_product(kept as u128, 2 * ONE as u128 - kept as u128);
        Self {
            ratio: kept,
            ratio_float: ratio,
            eccentricity,
            eccentricity_in_turns: multiplied(eccentricity, ONE_OVER_TAU, 126),
            minor: product.square_root() as Fixed,
        }
    }

    /// Where the body is at this mean anomaly (a fraction of a turn with 128 bits after the point).
    pub(crate) fn anomaly(&self, mean_anomaly: u128) -> Anomaly {
        self.solve(mean_anomaly, STEPS, None)
    }

    /// As [`Self::anomaly`], with the number of Newton steps in whole numbers and the first guess (in the half turn the equation is solved in) as the caller wants them.
    fn solve(&self, mean_anomaly: u128, steps: usize, start: Option<u128>) -> Anomaly {
        let turns = self.eccentric_anomaly(mean_anomaly, steps, start);
        let (sine, cosine) = sine_and_cosine(turns);
        Anomaly {
            turns,
            sine,
            cosine,
            x: cosine - self.eccentricity,
            y: multiplied(self.minor, sine, 126),
            distance: ONE - multiplied(self.eccentricity, cosine, 126),
            velocity_y: multiplied(self.minor, cosine, 126),
        }
    }

    fn eccentric_anomaly(&self, mean_anomaly: u128, steps: usize, start: Option<u128>) -> u128 {
        // A circle, and the periapsis and apoapsis of an ellipse, are where the anomaly is the mean anomaly.
        if self.ratio == ONE || mean_anomaly == 0 || mean_anomaly == HALF_TURN {
            return mean_anomaly;
        }
        let reflected = mean_anomaly > HALF_TURN;
        let mean = if reflected {
            mean_anomaly.wrapping_neg()
        } else {
            mean_anomaly
        };
        // (Below a half turn, so that an anomaly and a mean anomaly are numbers an `i128` holds.)
        let mut eccentric = start
            .unwrap_or_else(|| self.first_guess(mean))
            .min(HALF_TURN - 1);
        for _ in 0..steps {
            let (sine, cosine) = sine_and_cosine(eccentric);
            // f = E - (e / 2 pi) sin E - M, in turns; its slope is 1 - e cos E, which is above the ratio. The step is f over the slope, as turns with 128 bits.
            let sine_part = multiplied(self.eccentricity_in_turns, sine, 126) << 2;
            // (The anomaly and the mean anomaly are both below a half turn, so their difference fits; the sine part is added last and saturates, because near a half turn the
            // sine can come out a few units below nothing, and a guess far out can make the whole too large for an `i128`. A saturated remainder only makes a long step, which is clamped.)
            let remainder = (eccentric as i128 - mean as i128).saturating_sub(sine_part);
            let slope = (ONE - multiplied(self.eccentricity, cosine, 126)) as u128;
            let step = Wide {
                high: 0,
                low: remainder.unsigned_abs(),
            }
            .shifted_left(126)
            .divided_by(slope);
            eccentric = if remainder > 0 {
                eccentric.saturating_sub(step)
            } else {
                eccentric.saturating_add(step).min(HALF_TURN - 1)
            };
        }
        if reflected {
            eccentric.wrapping_neg()
        } else {
            eccentric
        }
    }

    /// The eccentric anomaly for a mean anomaly in the first half turn, in turns, to about 1e-15 of itself: all in `f64`, with `libm` for what is not an operation of the
    /// machine, so the same on every machine.
    fn first_guess(&self, mean: u128) -> u128 {
        let ratio = self.ratio_float;
        let e = 1.0 - ratio;
        let mean_radians = TAU * (mean as f64 / TURN);
        let mut guess = if e < 0.3 {
            mean_radians + e * libm::sin(mean_radians)
        } else {
            // The root of E^3 + p E + q = 0, which is E - e sin E = M with sin E = E - E^3/6 (Cardano); good for the sharp ellipses it is meant for, and a start otherwise.
            let p = 6.0 * ratio / e;
            let q = -6.0 * mean_radians / e;
            let discriminant = libm::sqrt(q * q / 4.0 + p * p * p / 27.0);
            libm::cbrt(-q / 2.0 + discriminant) + libm::cbrt(-q / 2.0 - discriminant)
        };
        guess = guess.clamp(0.0, PI);
        for _ in 0..8 {
            let (value, slope) = Self::equation_in_f64(guess, ratio, e);
            guess = (guess - (value - mean_radians) / slope).clamp(0.0, PI);
        }
        (guess / TAU * TURN) as u128
    }

    /// E - e sin E and its slope 1 - e cos E, without the cancellation near E = 0: E - e sin E = ratio E + e (E - sin E), with E - sin E from its series where E is small, and
    /// 1 - e cos E = ratio + 2 e sin^2(E / 2).
    fn equation_in_f64(eccentric: f64, ratio: f64, e: f64) -> (f64, f64) {
        let value = if eccentric < 0.5 {
            let square = eccentric * eccentric;
            // E - sin E = E^3/6 - E^5/120 + E^7/5040 - ...: each term from the one before.
            let mut term = eccentric * square / 6.0;
            let mut series = 0.0;
            for index in 0..12 {
                series += term;
                term *= -square / f64::from((2 * index + 4) * (2 * index + 5));
            }
            ratio * eccentric + e * series
        } else {
            eccentric - e * libm::sin(eccentric)
        };
        let half = libm::sin(eccentric / 2.0);
        (value, ratio + 2.0 * e * half * half)
    }
}

#[cfg(test)]
mod tests;

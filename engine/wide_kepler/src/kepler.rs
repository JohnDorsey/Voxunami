//! Kepler's equation by Newton's method, in `WideFloat`s: `E - e sin E = M` for an ellipse and `e sinh H - H = M` for a hyperbola.

use std::f64::consts::PI;

use wide_float::WideFloat;

use crate::KeplerError;

/// The most steps Newton's method may take. From a first guess good to 1e-10 or better it takes a handful at any width (the error is squared each step: 4 or 5 for 128
/// bits, 9 for 4,096), so this is a bound that is never met and a way to end if something is wrong.
const MOST_STEPS: usize = 64;

/// `E - e sin E` and its slope `1 - e cos E` in `f64`, without the cancellation near E = 0: `E - e sin E = ratio E + e (E - sin E)`, with `E - sin E` from its series
/// where E is small, and `1 - e cos E = ratio + 2 e sin^2(E / 2)`. `ratio` and `e` are given apart (they add up to one) because `e` is the difference of one and a ratio that
/// may be 2^-64. Here it makes the first guess of the solver, which is not part of the answer, only of how many steps it takes to get there.
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

/// The eccentric anomaly for a mean anomaly in the first half turn, to about 1e-15 of itself: all in `f64`, with `libm` for what is not an operation of the machine, so
/// the same on every machine.
fn first_guess(ratio: f64, mean: f64) -> f64 {
    let e = 1.0 - ratio;
    let mut guess = if e < 0.3 {
        mean + e * libm::sin(mean)
    } else {
        // The root of E^3 + p E + q = 0, which is E - e sin E = M with sin E = E - E^3/6 (Cardano); good for the sharp ellipses it is meant for, and a start otherwise.
        let p = 6.0 * ratio / e;
        let q = -6.0 * mean / e;
        let discriminant = libm::sqrt(q * q / 4.0 + p * p * p / 27.0);
        libm::cbrt(-q / 2.0 + discriminant) + libm::cbrt(-q / 2.0 - discriminant)
    };
    guess = guess.clamp(0.0, PI);
    for _ in 0..8 {
        let (value, slope) = equation_in_f64(guess, ratio, e);
        guess = (guess - (value - mean) / slope).clamp(0.0, PI);
    }
    guess
}

/// The eccentric anomaly `E` (from 0 to pi) of the ellipse with this `ratio` (1 - e, from 2^-64 to 1) at this `mean` anomaly (from 0 to pi), in the width of the wider of
/// the two numbers. Within about 2^(66 - width) of itself (the slope of the equation is as small as the ratio, which is as small as 2^-64).
///
/// On the half turn the function `ratio E + e (E - sin E) - M` is increasing and convex, so Newton's method comes down monotonically to the root from the right, and
/// a step from the left lands to the right of it; the guess is clamped to the half turn, where pi is to the right of the root, whatever the step does.
pub(crate) fn eccentric_anomaly(
    ratio: &WideFloat,
    mean: &WideFloat,
) -> Result<WideFloat, KeplerError> {
    let precision = ratio.precision().max(mean.precision());
    let one = WideFloat::from_i64(1, precision);
    let pi = WideFloat::pi(precision);
    // A circle, and the periapsis and apoapsis of an ellipse, are where the anomaly is the mean anomaly.
    if ratio.compare(&one).is_eq() || mean.is_zero() {
        return Ok(mean.rounded_to(precision));
    }
    if mean.compare(&pi).is_ge() {
        return Ok(pi);
    }
    let e = &one - ratio;
    let mut eccentric = initial_guess(ratio, mean, &pi)?;
    // A step under this fraction of the answer is the end: the error of the next one would be its square. (Not a smaller one: the noise of the steps themselves, at the
    // thinnest ellipse, is about 2^(66 - width) of the answer.)
    let settled = -(precision.bits() as i64 - 70);
    for _ in 0..MOST_STEPS {
        let (sine_half, cosine_half) = eccentric.times_power_of_two(-1)?.sin_cos();
        let sine = (&sine_half * &cosine_half).times_power_of_two(1)?;
        // ratio E + e (E - sin E) - M, and the slope ratio + 2 e sin^2(E / 2).
        let residual = &(&(ratio * &eccentric) + &(&e * &(&eccentric - &sine))) - mean;
        let slope = ratio + &(&e * &(&sine_half * &sine_half)).times_power_of_two(1)?;
        let step = residual.divide(&slope)?;
        let next = &eccentric - &step;
        let done = step
            .abs()
            .compare(&eccentric.times_power_of_two(settled)?)
            .is_le();
        eccentric = if !next.is_positive() {
            WideFloat::zero(precision)
        } else if next.compare(&pi).is_gt() {
            pi.clone()
        } else {
            next
        };
        if done {
            return Ok(eccentric);
        }
    }
    Err(KeplerError::DidNotSettle)
}

fn initial_guess(
    ratio: &WideFloat,
    mean: &WideFloat,
    pi: &WideFloat,
) -> Result<WideFloat, KeplerError> {
    let precision = pi.precision();
    let (ratio_double, mean_double) = (ratio.to_f64(), mean.to_f64());
    // (A mean anomaly too small for a double has the guess it has for small E: E = M / ratio.)
    let guess = if mean_double > 1.0e-200 {
        first_guess(ratio_double, mean_double)
    } else {
        0.0
    };
    if guess > 0.0 {
        return Ok(WideFloat::from_f64(guess, precision)?);
    }
    let by_ratio = mean.divide(ratio)?;
    Ok(if by_ratio.compare(pi).is_gt() {
        pi.clone()
    } else {
        by_ratio
    })
}

/// `(e - 1) sinh H + (sinh H - H)` (that is, `e sinh H - H`) and its slope `(e - 1) cosh H + 2 sinh^2(H / 2)` (that is, `e cosh H - 1`) in `f64`, for H from nothing up, with
/// `size` = e - 1 given apart (it may be 2^-64): both are sums of terms that are not below nothing, so nothing cancels but `sinh H - H`, which is taken from its series where H
/// is small. Here it makes the first guess of the solver, which is not part of the answer, only of how many steps it takes to get there.
fn hyperbolic_equation_in_f64(anomaly: f64, size: f64) -> (f64, f64) {
    let difference = if anomaly < 0.5 {
        let square = anomaly * anomaly;
        // sinh H - H = H^3/6 + H^5/120 + H^7/5040 + ...: each term from the one before.
        let mut term = anomaly * square / 6.0;
        let mut series = 0.0;
        for index in 0..12 {
            series += term;
            term *= square / f64::from((2 * index + 4) * (2 * index + 5));
        }
        series
    } else {
        libm::sinh(anomaly) - anomaly
    };
    let half = libm::sinh(anomaly / 2.0);
    (
        size * libm::sinh(anomaly) + difference,
        size * libm::cosh(anomaly) + 2.0 * half * half,
    )
}

/// The anomaly of the hyperbola of this `size` (e - 1) at this mean anomaly above nothing, to about 1e-15 of itself, in `f64` (with `libm`, so the same on every machine): Newton's
/// method kept inside a bracket (the function is increasing and convex), which bisection takes over from when a step leaves it.
fn first_hyperbolic_guess(size: f64, mean: f64) -> f64 {
    let mut low = 0.0_f64;
    let mut high = bound_above_the_hyperbolic_root(mean);
    let mut guess = high.min(mean / size);
    for _ in 0..100 {
        let (value, slope) = hyperbolic_equation_in_f64(guess, size);
        if value > mean {
            high = guess;
        } else {
            low = guess;
        }
        let mut next = guess - (value - mean) / slope;
        if !(next > low && next < high) {
            next = 0.5 * (low + high);
        }
        let done = (next - guess).abs() <= 1.0e-16 * next;
        guess = next;
        if done {
            break;
        }
    }
    guess
}

/// The widest a number may be for the series of `sinh` and `cosh` to be used, and for the pair to be kept across the steps of the solver: the series takes more terms as the width
/// grows (a term is a multiplication and a division of the width), and the library's exponential grows more slowly, so that at 8,192 bits the pair kept across the steps is only
/// 1.8 times as fast as working it out at every step and at 16,384 it would not be. Over this the pair is worked out by the library, as it was before.
const SERIES_UP_TO_BITS: u64 = 8192;

/// The most terms of the series of `sinh` and `cosh` that are summed: more than a width of 8,192 bits needs for an argument of a half.
const MOST_SERIES_TERMS: i64 = 2048;

/// `sinh d` and `cosh d` from their series, summed from the front, to terms under 2^-(width + 8) of the sums: the terms are all of one sign, so nothing cancels, and how many of them
/// there are is a property of `d` and the width and of nothing else. `d` may be of either sign and is meant to be small (a half or less; the series of one much larger is long).
pub(crate) fn small_hyperbolic_pair(d: &WideFloat) -> Result<(WideFloat, WideFloat), KeplerError> {
    let precision = d.precision();
    let one = WideFloat::from_i64(1, precision);
    if d.is_zero() {
        return Ok((WideFloat::zero(precision), one));
    }
    let square = d.square();
    let (mut sinh, mut cosh) = (d.clone(), one.clone());
    // The terms d^(2k+1) / (2k+1)! and d^(2k) / (2k)!, each from the one before.
    let (mut sinh_term, mut cosh_term) = (d.clone(), one);
    let least = -(precision.bits() as i64 + 8);
    let under = |term: &WideFloat, sum: &WideFloat| {
        term.floor_log2()
            .is_none_or(|bits| bits - sum.floor_log2().expect("a sum that is not nothing") < least)
    };
    for k in 1..=MOST_SERIES_TERMS {
        cosh_term = (&cosh_term * &square)
            .divide(&WideFloat::from_i64((2 * k - 1) * (2 * k), precision))?;
        sinh_term =
            (&sinh_term * &square).divide(&WideFloat::from_i64(2 * k * (2 * k + 1), precision))?;
        cosh = &cosh + &cosh_term;
        sinh = &sinh + &sinh_term;
        if under(&cosh_term, &cosh) && under(&sinh_term, &sinh) {
            return Ok((sinh, cosh));
        }
    }
    Err(KeplerError::DidNotSettle)
}

/// `sinh h` and `cosh h` for `h` from nothing up. Where `h` is under a half they come from their series (14 microseconds each from the library at 224 bits, and about 12 for both
/// from the series); where it is not, both come from one exponential, `t = e^h`, as `(t - 1/t) / 2` and `(t + 1/t) / 2`, which loses less than a bit there and costs a half of the
/// two functions (malachite's `exp`, `sinh` and `cosh` are about 14 microseconds each at 224 bits, a sine and cosine 2.5). (Where `h` is small the difference of `t` and `1/t`
/// would lose the digits `sinh h` has.) Over [`SERIES_UP_TO_BITS`] the two functions are the library's.
pub(crate) fn sinh_and_cosh(h: &WideFloat) -> Result<(WideFloat, WideFloat), KeplerError> {
    let half = WideFloat::from_f64(0.5, h.precision())?;
    if h.compare(&half).is_lt() {
        if h.precision().bits() <= SERIES_UP_TO_BITS {
            return small_hyperbolic_pair(h);
        }
        return Ok((h.sinh()?, h.cosh()?));
    }
    let exponential = h.exp()?;
    let inverse = WideFloat::from_i64(1, h.precision()).divide(&exponential)?;
    Ok((
        (&exponential - &inverse).times_power_of_two(-1)?,
        (&exponential + &inverse).times_power_of_two(-1)?,
    ))
}

/// An anomaly that is above the root of the equation whatever the ratio: `e sinh H - H` is at least `e^H / 4` for H from 3 up (since e is above one), so at `ln(4 M) + 3` it is
/// above `M`; and for a mean anomaly too small for that, 3 is above the root up to a mean anomaly of 7 (`e sinh 3 - 3` is above 7, whatever e), where `ln(4 M) + 3` takes over.
const HYPERBOLIC_BOUND_OFFSET: f64 = 3.0;

fn bound_above_the_hyperbolic_root(mean: f64) -> f64 {
    HYPERBOLIC_BOUND_OFFSET.max(libm::log(4.0 * mean) + HYPERBOLIC_BOUND_OFFSET)
}

/// The anomaly `H` (from nothing up) of the hyperbola whose `size` is e - 1 (from 2^-64 to 2^64) at this `mean` anomaly (from nothing up), in the width of the wider of the two numbers,
/// with `sinh(H / 2)` and `cosh(H / 2)`, which a place is made from and the solver has by its end.
/// Within about 2^(66 - width) of itself, as for an ellipse: the slope of the equation, `e cosh H - 1`, is at least `size`.
///
/// The function `e sinh H - H - M` is increasing and convex from nothing up, so Newton's method comes down monotonically to the root from the right, and a step from the left lands to
/// the right of it, possibly a long way (the slope may be 2^-64): the guess is held to between nothing and a bound above the root, whatever the step does.
pub(crate) fn hyperbolic_anomaly(
    size: &WideFloat,
    mean: &WideFloat,
) -> Result<HyperbolicAnomaly, KeplerError> {
    let precision = size.precision().max(mean.precision());
    // The periapsis is where the anomaly is the mean anomaly.
    if mean.is_zero() {
        return Ok(HyperbolicAnomaly {
            anomaly: WideFloat::zero(precision),
            sinh_half: WideFloat::zero(precision),
            cosh_half: WideFloat::from_i64(1, precision),
        });
    }
    let (size_double, mean_double) = (size.to_f64(), mean.to_f64());
    // (A little more than the bound, for the rounding of the logarithm in a double.)
    let bound = WideFloat::from_f64(
        bound_above_the_hyperbolic_root(mean_double) + 1.0,
        precision,
    )?;
    // (A mean anomaly too small for a double has the guess it has for small H: H = M / size.)
    let guess = if mean_double > 1.0e-200 {
        WideFloat::from_f64(first_hyperbolic_guess(size_double, mean_double), precision)?
    } else {
        let by_size = mean.divide(size)?;
        if by_size.compare(&bound).is_gt() {
            bound.clone()
        } else {
            by_size
        }
    };
    refine_hyperbolic_anomaly(size, mean, guess, &bound)
}

/// The anomaly of a hyperbola and the sine and cosine of half of it.
pub(crate) struct HyperbolicAnomaly {
    /// (A place is made from the pair alone; the anomaly is what the tests of the solver look at.)
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) anomaly: WideFloat,
    pub(crate) sinh_half: WideFloat,
    pub(crate) cosh_half: WideFloat,
}

/// A step of Newton's method is a step of this much of `H` or less (in `h = H / 2`, under 2^-8) for the pair `(sinh h, cosh h)` to be moved by the step and not worked out again.
const NEARLY_NOTHING_BITS: i64 = 8;

/// Newton's method from `guess`, kept between nothing and `bound` (which is above the root): the end of [`hyperbolic_anomaly`], apart so that a guess that is a long way from the
/// root, which the first guess in a double never is, can be tried.
///
/// **The pair `(sinh h, cosh h)` of the half anomaly `h = H / 2` is worked out once and kept across the steps.** A step of `d` in `h` moves it by `sinh(h - d) = sinh h cosh d -
/// cosh h sinh d` and `cosh(h - d) = cosh h cosh d - sinh h sinh d`, with `sinh d` and `cosh d` from their series, which for the steps after the first (2^-45 of `H` and then
/// 2^-95) are two or three terms; the library's exponential, which costs 14 microseconds at 224 bits, is asked once and not at each of the three steps and again for the place.
/// The pair is worked out again, by the library, where a step is not small (under 2^-8 in `h`) or not small against `h` itself (a quarter of it: a step from `h` to something much
/// smaller would lose the digits of the difference), where it was held to the bound or to nothing, and where the width is over [`SERIES_UP_TO_BITS`].
fn refine_hyperbolic_anomaly(
    size: &WideFloat,
    mean: &WideFloat,
    guess: WideFloat,
    bound: &WideFloat,
) -> Result<HyperbolicAnomaly, KeplerError> {
    let precision = size.precision().max(mean.precision());
    let one = WideFloat::from_i64(1, precision);
    let nearly_nothing = one.times_power_of_two(-NEARLY_NOTHING_BITS)?;
    let mut anomaly = guess;
    let mut pair = sinh_and_cosh(&anomaly.times_power_of_two(-1)?)?;
    // A step under this fraction of the answer is the end: the error of the next one would be its square. (Not a smaller one: the noise of the steps themselves, at the thinnest
    // hyperbola, is about 2^(66 - width) of the answer.)
    let settled = -(precision.bits() as i64 - 70);
    for _ in 0..MOST_STEPS {
        let (sinh_half, cosh_half) = &pair;
        // sinh H and cosh H - 1 = 2 sinh^2(H / 2), from the half angle so that nothing cancels.
        let sinh = (sinh_half * cosh_half).times_power_of_two(1)?;
        let cosh_less_one = (sinh_half * sinh_half).times_power_of_two(1)?;
        let cosh = &one + &cosh_less_one;
        // (e - 1) sinh H + (sinh H - H) - M, and the slope (e - 1) cosh H + 2 sinh^2(H / 2).
        let residual = &(&(size * &sinh) + &(&sinh - &anomaly)) - mean;
        let slope = &(size * &cosh) + &cosh_less_one;
        let step = residual.divide(&slope)?;
        let next = &anomaly - &step;
        let done = step
            .abs()
            .compare(&anomaly.times_power_of_two(settled)?)
            .is_le();
        let half_step = step.times_power_of_two(-1)?;
        // (A step under a quarter of `H` leaves `next` over half of it, so that it is above nothing.)
        let moves_the_pair = precision.bits() <= SERIES_UP_TO_BITS
            && next.compare(bound).is_le()
            && half_step.abs().compare(&nearly_nothing).is_lt()
            && half_step
                .abs()
                .compare(&anomaly.times_power_of_two(-2)?)
                .is_lt();
        if moves_the_pair {
            let (sinh_step, cosh_step) = small_hyperbolic_pair(&half_step)?;
            pair = (
                &(&pair.0 * &cosh_step) - &(&pair.1 * &sinh_step),
                &(&pair.1 * &cosh_step) - &(&pair.0 * &sinh_step),
            );
            anomaly = next;
        } else {
            anomaly = if !next.is_positive() {
                WideFloat::zero(precision)
            } else if next.compare(bound).is_gt() {
                bound.clone()
            } else {
                next
            };
            pair = sinh_and_cosh(&anomaly.times_power_of_two(-1)?)?;
        }
        if done {
            let (sinh_half, cosh_half) = pair;
            return Ok(HyperbolicAnomaly {
                anomaly,
                sinh_half,
                cosh_half,
            });
        }
    }
    Err(KeplerError::DidNotSettle)
}

#[cfg(test)]
mod tests;

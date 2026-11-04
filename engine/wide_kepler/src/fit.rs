//! The orbit a body is on, from its place and its speed: the inverse of [`Elements::place_at`], an ellipse or, for a body going faster than it would to escape, a hyperbola.
//!
//! The state is exact (numbers of any width), so the orbit through it is exact, and is found without any rule about what is near enough to be something else: a state that is
//! exactly a circle is a circle (its eccentricity is nothing), and one that is a hair off is an ellipse with the hair, however small. What the state cannot be is refused, and the
//! rest is the business of the limits an orbit has (a ratio from 2^-64, a radius under 2^121, a period under 2^81 seconds): a body that is nearly falling straight is an ellipse
//! with a ratio under 2^-64, and so is refused as an orbit, not as a state.

use wide_float::{Precision, WideFloat};

use crate::KeplerError;
use crate::elements::{
    Elements, MEAN_GUARD, TURN_GUARD, Time, check_time, plane, turns_gone, unit_vector,
};
use crate::kepler::sinh_and_cosh;

/// Bits over the width asked for that the orbit is worked out in. The energy `2 / r - v^2 / mu` loses as many bits as the orbit is long over its periapsis (a ratio of 2^-64 loses 65),
/// the angular momentum as many as the state is nearly along the radius (the ratio is no less than 2^-64 of it), and the two multiply into the ratio: 65 + 32 + 32 are under 128.
const GUARD: u32 = 128;

fn dot(first: &[WideFloat; 3], second: &[WideFloat; 3]) -> WideFloat {
    &(&(&first[0] * &second[0]) + &(&first[1] * &second[1])) + &(&first[2] * &second[2])
}

fn cross(first: &[WideFloat; 3], second: &[WideFloat; 3]) -> [WideFloat; 3] {
    [
        &(&first[1] * &second[2]) - &(&first[2] * &second[1]),
        &(&first[2] * &second[0]) - &(&first[0] * &second[2]),
        &(&first[0] * &second[1]) - &(&first[1] * &second[0]),
    ]
}

impl Elements {
    /// The elements of the orbit that a body is on if it is at `position` from the centre of what it goes round and goes `velocity` against it (voxels, and voxels a second) at
    /// `time`, and what it goes round has the mass parameter `mu` (G M, in voxels cubed a second squared). The elements are numbers of `precision` bits, and put the body at
    /// `position` with that speed at `time` to within the last bit of the width (the radius of the orbit times 2^-width): see [`Elements::place_at`]. The orbit goes counter-clockwise
    /// looking down its normal, which is the direction of the angular momentum, so its period is above nothing.
    ///
    /// The orbit is an ellipse if the body is going slower than it would to escape and **a hyperbola if it is going faster** (a ratio below nothing; its phase is the mean anomaly at
    /// time 0 as it is, and is kept to the bits of its size as well as the width, which a body a long time from time 0 needs).
    ///
    /// Refused: a time the clock cannot hold, a mass that is not above nothing, a body on its centre, one that is at rest or goes straight at the centre or away from it (it has no
    /// plane), one that goes at exactly the speed of escape (a parabola, which is not made), and what is an orbit but not one that [`Elements::validate`] allows.
    pub fn from_state(
        position: &[WideFloat; 3],
        velocity: &[WideFloat; 3],
        mu: &WideFloat,
        time: Time,
        precision: Precision,
    ) -> Result<Elements, KeplerError> {
        check_time(time)?;
        if !mu.is_positive() {
            return Err(KeplerError::Invalid(
                "what a body goes round weighs something",
            ));
        }
        let width = precision.widened_by(GUARD);
        let at = |number: &WideFloat| number.rounded_to(width);
        let place = position.each_ref().map(at);
        let speed = velocity.each_ref().map(at);
        let mu = at(mu);
        let one = WideFloat::from_i64(1, width);
        let two = WideFloat::from_i64(2, width);
        let half = one.times_power_of_two(-1)?;

        let distance = dot(&place, &place).sqrt()?;
        if distance.is_zero() {
            return Err(KeplerError::Invalid("a body on its centre is on no orbit"));
        }
        let momentum = cross(&place, &speed);
        let momentum_squared = dot(&momentum, &momentum);
        if momentum_squared.is_zero() {
            return Err(KeplerError::Invalid(
                "a body at rest, or going straight at its centre or away from it, has no plane to go round in",
            ));
        }
        // The energy: 1 / a = 2 / r - v^2 / mu, above nothing for an ellipse, below nothing for a hyperbola (a body going faster than it would to escape), and nothing for a
        // parabola, which is not made. `axis` is the size of a.
        let inverse_axis = &two.divide(&distance)? - &dot(&speed, &speed).divide(&mu)?;
        if inverse_axis.is_zero() {
            return Err(KeplerError::Invalid(
                "a body that is going at exactly the speed of escape is on a parabola, which is not made",
            ));
        }
        let hyperbola = inverse_axis.is_negative();
        let axis = one.divide(&inverse_axis.abs())?;

        // The shape. The eccentricity vector points at the nearest point and is (v x h) / mu - r / |r|, and the ratio is 1 - e. (The ratio of a thin ellipse is 1 less a number
        // very near 1, but the guard bits are more than the 64 it may lose, so no other form is needed.)
        let across = cross(&speed, &momentum);
        let eccentricity_vector = [0, 1, 2].map(|part| {
            let toward = across[part].divide(&mu).expect("the mass is above nothing");
            let outward = place[part]
                .divide(&distance)
                .expect("the distance is above nothing");
            &toward - &outward
        });
        let eccentricity = dot(&eccentricity_vector, &eccentricity_vector).sqrt()?;
        let ratio = &one - &eccentricity;
        if (hyperbola && !ratio.is_negative()) || (!hyperbola && !ratio.is_positive()) {
            return Err(KeplerError::Invalid(
                "a body that is going at the speed of escape to the width of the working is on no orbit that is made",
            ));
        }

        // The plane. The circle starts toward the axis that the normal **as kept** decides (its x as given), so the normal is rounded to the width it is kept in first, and the
        // plane made from that, as `place_at` will make it.
        let normal = unit_vector(&momentum)?.map(|part| part.rounded_to(precision));
        let (first, second) = plane(&normal.each_ref().map(at))?;
        let (x, y) = (dot(&place, &first), dot(&place, &second));
        let periapsis = {
            let (toward, beyond) = (
                dot(&eccentricity_vector, &first),
                dot(&eccentricity_vector, &second),
            );
            if toward.is_zero() && beyond.is_zero() {
                WideFloat::zero(width)
            } else {
                beyond.atan2(&toward)?
            }
        };

        // Where the body is on the orbit: the true anomaly from the nearest point.
        let (sine_periapsis, cosine_periapsis) = periapsis.sin_cos();
        let along = &(&x * &cosine_periapsis) + &(&y * &sine_periapsis);
        let beyond = &(&y * &cosine_periapsis) - &(&x * &sine_periapsis);
        let true_anomaly = beyond.atan2(&along)?;
        let (sine_half, cosine_half) = true_anomaly.times_power_of_two(-1)?.sin_cos();
        if hyperbola {
            return hyperbola_from_the_true_anomaly(HyperbolaState {
                normal,
                axis,
                ratio,
                periapsis,
                sine_half,
                cosine_half,
                distance,
                outward: dot(&place, &speed).is_positive(),
                mu,
                time,
                precision,
            });
        }
        // The ellipse: tan(E / 2) = sqrt((1 - e) / (1 + e)) tan(v / 2) kept from dividing by nothing as the two parts of an arctangent, then M = E - e sin E written as
        // ratio E + e (E - sin E), whose terms do not cancel.
        let complement = &one - &ratio;
        let eccentric = (&ratio.sqrt()? * &sine_half)
            .atan2(&(&(&two - &ratio).sqrt()? * &cosine_half))?
            .times_power_of_two(1)?;
        let mean = &(&ratio * &eccentric) + &(&complement * &(&eccentric - &eccentric.sin()));

        // The period (kept at the width it is kept in, because it is that number that the turns gone are worked out with), and the phase: the mean anomaly now less the turns gone
        // since time 0, in whole turns exactly.
        let period = (&(&axis.square() * &axis).divide(&mu)?.sqrt()?
            * &WideFloat::pi(width).times_power_of_two(1)?)
            .rounded_to(precision);
        let turns_width = width.widened_by(TURN_GUARD);
        let (two_pi, gone) = turns_gone(&period, &WideFloat::zero(turns_width), time, turns_width)?;
        let mean_turns = mean.rounded_to(turns_width).divide(&two_pi)?;
        // (The phase is an angle from -pi to pi and has more bits than the rest: near the nearest point of a thin ellipse a change of the mean anomaly moves the body by the ratio to
        // the power of a half and its speed by the ratio to the power of one and a half, so that is how much more the phase is kept to, or the body would not be where it was.)
        let turns_away = &mean_turns - &gone;
        let nearest_turn = (&turns_away + &half)
            .floor_to_i128()
            .ok_or(KeplerError::Invalid(
                "an orbit is not gone round that many times",
            ))?;
        let phase_turns = &turns_away - &WideFloat::from_i128(nearest_turn, turns_width);
        let magnified_by = ratio.floor_log2().map_or(0, |bits| (-bits).max(0)) as u32;
        let phase_precision = precision.widened_by((3 * magnified_by).div_ceil(2) + 8);
        let phase = (&two_pi * &phase_turns).rounded_to(phase_precision);

        let elements = Elements {
            normal,
            radius: axis.rounded_to(precision),
            ratio: ratio.rounded_to(precision),
            periapsis: periapsis.rounded_to(precision),
            period,
            phase,
        };
        elements.validate()?;
        Ok(elements)
    }
}

/// What the hyperbola is made from: the shape and the plane of the orbit, the true anomaly of the body as the sine and cosine of half of it, and its distance and whether it is
/// going away.
struct HyperbolaState {
    normal: [WideFloat; 3],
    /// The size of the semi-major axis, `|a|`.
    axis: WideFloat,
    /// 1 - e, below nothing, at the working width.
    ratio: WideFloat,
    periapsis: WideFloat,
    sine_half: WideFloat,
    cosine_half: WideFloat,
    distance: WideFloat,
    outward: bool,
    mu: WideFloat,
    time: Time,
    precision: Precision,
}

/// The half of the hyperbolic anomaly, `h`, with `tanh h = u / v` (from the true anomaly: `tanh(H / 2) = sqrt((e - 1) / (e + 1)) tan(nu / 2)`, which is `u / v` for
/// `u = sqrt(e - 1) sin(nu / 2)` and `v = sqrt(e + 1) cos(nu / 2)`, so that nothing is divided by nothing): the root of `v sinh h - u cosh h`, by Newton's method from a guess in `f64`
/// (`asinh(u / sqrt((v - u)(v + u)))`, with `v - u` worked out in the wide number, so that a body far out where `u` is nearly `v` has its guess too). The logarithm
/// `ln((v + u) / (v - u))` is not used: near the periapsis it is the logarithm of a number near one and loses what the anomaly has.
fn half_hyperbolic_anomaly(u: &WideFloat, v: &WideFloat) -> Result<WideFloat, KeplerError> {
    let precision = u.precision().max(v.precision());
    let (difference, sum) = ((v - u).to_f64(), (v + u).to_f64());
    let guess = libm::asinh(u.to_f64() / libm::sqrt(difference * sum));
    let mut anomaly = if guess.is_finite() {
        WideFloat::from_f64(guess, precision)?
    } else {
        WideFloat::zero(precision)
    };
    // A step under this fraction of the answer is the end: the error of the next one would be its square.
    let settled = -(precision.bits() as i64 - 70);
    for _ in 0..HYPERBOLIC_STEPS {
        let (sinh, cosh) = sinh_and_cosh(&anomaly.abs())?;
        let sinh = if anomaly.is_negative() { -&sinh } else { sinh };
        let residual = &(v * &sinh) - &(u * &cosh);
        let slope = &(v * &cosh) - &(u * &sinh);
        let step = residual.divide(&slope)?;
        let done = step
            .abs()
            .compare(&anomaly.abs().times_power_of_two(settled)?)
            .is_le();
        anomaly = &anomaly - &step;
        if done {
            return Ok(anomaly);
        }
    }
    Err(KeplerError::DidNotSettle)
}

/// The most steps of Newton's method for the half of the anomaly: from a guess good to 1e-15 it takes a handful at any width.
const HYPERBOLIC_STEPS: usize = 64;

/// The elements of the hyperbola of a body whose place in the plane is known as a true anomaly: the anomaly, the mean anomaly `M = e sinh H - H` written as
/// `(e - 1) sinh H + (sinh H - H)` (its terms are not below nothing together), the period (that of a mean motion `sqrt(mu / |a|^3)`), and the phase, which is `M` less
/// the mean motion times the time, **as it is**, in the width its size and the width need.
fn hyperbola_from_the_true_anomaly(state: HyperbolaState) -> Result<Elements, KeplerError> {
    let HyperbolaState {
        normal,
        axis,
        ratio,
        periapsis,
        sine_half,
        cosine_half,
        distance,
        outward,
        mu,
        time,
        precision,
    } = state;
    let width = axis.precision();
    let one = WideFloat::from_i64(1, width);
    let two = WideFloat::from_i64(2, width);
    let size = -&ratio;
    // The anomaly. From the distance where the body is not near its periapsis: `r = |a| (e cosh H - 1)`, so `cosh H = (r / |a| + 1) / e`, a sum of numbers that are not below
    // nothing, and the direction it goes in says the sign. From the true anomaly where it is: far out the direction of the body is nearly that of an asymptote, and says little
    // of H (`tanh(H / 2) = u / v` is magnified by e^H there), while near the periapsis `cosh H - 1` cancels. The two meet at `cosh H` of 2, where both are good.
    let cosh_by_distance = (&distance.divide(&axis)? + &one).divide(&(&one + &size))?;
    let (half_anomaly, anomaly) = if cosh_by_distance.compare(&two).is_ge() {
        let size_of_anomaly = cosh_by_distance.acosh()?;
        let anomaly = if outward {
            size_of_anomaly
        } else {
            -&size_of_anomaly
        };
        (anomaly.times_power_of_two(-1)?, anomaly)
    } else {
        let u = &size.sqrt()? * &sine_half;
        let v = &(&two + &size).sqrt()? * &cosine_half;
        let half_anomaly = half_hyperbolic_anomaly(&u, &v)?;
        let anomaly = half_anomaly.times_power_of_two(1)?;
        (half_anomaly, anomaly)
    };
    // sinh H = 2 sinh(H / 2) cosh(H / 2).
    let (sinh_half, cosh_half) = sinh_and_cosh(&half_anomaly.abs())?;
    let sinh_half = if half_anomaly.is_negative() {
        -&sinh_half
    } else {
        sinh_half
    };
    let sinh = (&sinh_half * &cosh_half).times_power_of_two(1)?;
    let mean = &(&size * &sinh) + &(&sinh - &anomaly);

    // The period (kept at the width it is kept in, because it is that number the mean motion is worked out from), and the phase: the mean anomaly now less what the mean motion has
    // added since time 0. The two numbers are of up to 120 whole bits and may cancel, so they are worked out with the bits that takes.
    let period = (&(&axis.square() * &axis).divide(&mu)?.sqrt()?
        * &WideFloat::pi(width).times_power_of_two(1)?)
        .rounded_to(precision);
    let mean_width = width.widened_by(MEAN_GUARD);
    let rate = WideFloat::pi(mean_width)
        .times_power_of_two(1)?
        .divide(&period.rounded_to(mean_width))?;
    let seconds = WideFloat::from_i128(time.units, mean_width).divide(&WideFloat::from_i64(
        i64::from(time.units_per_second),
        mean_width,
    ))?;
    let phase = &mean.rounded_to(mean_width) - &(&rate * &seconds);
    // (The phase keeps the bits of its size, so that the place is right to the width; and those the equation magnifies it by, near the periapsis of a hyperbola that is barely one,
    // as for an ellipse: a change of the mean anomaly moves the body by the size of the ratio to the power of a half and its speed by it to the power of one and a half.)
    let whole_bits = phase.abs().floor_log2().map_or(0, |bits| (bits + 1).max(0)) as u32;
    let magnified_by = size.floor_log2().map_or(0, |bits| (-bits).max(0)) as u32;
    let phase_precision = precision.widened_by(whole_bits + (3 * magnified_by).div_ceil(2) + 8);

    let elements = Elements {
        normal,
        radius: axis.rounded_to(precision),
        ratio: ratio.rounded_to(precision),
        periapsis: periapsis.rounded_to(precision),
        period,
        phase: phase.rounded_to(phase_precision),
    };
    elements.validate()?;
    Ok(elements)
}

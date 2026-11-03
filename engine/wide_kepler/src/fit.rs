//! The orbit a body is on, from its place and its speed: the inverse of [`Elements::place_at`].
//!
//! The state is exact (numbers of any width), so the orbit through it is exact, and is found without any rule about what is near enough to be something else: a state that is
//! exactly a circle is a circle (its eccentricity is nothing), and one that is a hair off is an ellipse with the hair, however small. What the state cannot be is refused, and the
//! rest is the business of the limits an orbit has (a ratio from 2^-64, a radius under 2^121, a period under 2^81 seconds): a body that is nearly falling straight is an ellipse
//! with a ratio under 2^-64, and so is refused as an orbit, not as a state.

use wide_float::{Precision, WideFloat};

use crate::KeplerError;
use crate::elements::{Elements, TURN_GUARD, Time, check_time, plane, turns_gone, unit_vector};

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
    /// Refused: a time the clock cannot hold, a mass that is not above nothing, a body on its centre, one that is at rest or goes straight at the centre or away from it (it has no
    /// plane), one that goes at the speed of escape or faster (it is on no ellipse), and what is an ellipse but not one that [`Elements::validate`] allows.
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
        // The energy: 1 / a = 2 / r - v^2 / mu, above nothing for an ellipse.
        let inverse_axis = &two.divide(&distance)? - &dot(&speed, &speed).divide(&mu)?;
        if !inverse_axis.is_positive() {
            return Err(KeplerError::Invalid(
                "a body that is going fast enough to get away is on no ellipse",
            ));
        }
        let axis = one.divide(&inverse_axis)?;

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
        if !ratio.is_positive() {
            return Err(KeplerError::Invalid(
                "a body that is going fast enough to get away is on no ellipse",
            ));
        }
        let complement = &one - &ratio;

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

        // Where the body is on the ellipse: the true anomaly from the nearest point, then tan(E / 2) = sqrt((1 - e) / (1 + e)) tan(v / 2) kept from dividing by nothing as the
        // two parts of an arctangent, then M = E - e sin E written as ratio E + e (E - sin E), whose terms do not cancel.
        let (sine_periapsis, cosine_periapsis) = periapsis.sin_cos();
        let along = &(&x * &cosine_periapsis) + &(&y * &sine_periapsis);
        let beyond = &(&y * &cosine_periapsis) - &(&x * &sine_periapsis);
        let true_anomaly = beyond.atan2(&along)?;
        let (sine_half, cosine_half) = true_anomaly.times_power_of_two(-1)?.sin_cos();
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

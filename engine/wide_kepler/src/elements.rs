//! The elements of an ellipse and where they put a body.

use std::cmp::Ordering;

use wide_float::{Precision, WideFloat};

use crate::KeplerError;
use crate::kepler::eccentric_anomaly;

/// The elements of an orbit: the same as `world_storage::rail::Orbit` has, as numbers of any width.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Elements {
    /// A vector that is not nothing: the orbit is counter-clockwise looking down it. It is made a unit vector when the orbit is worked out, so it need not be one.
    pub normal: [WideFloat; 3],
    /// The semi-major axis, in voxels: above nothing and under 2^121.
    pub radius: WideFloat,
    /// The periapsis over the semi-major axis, which is 1 - e: from 2^-64 to 1 (a circle).
    pub ratio: WideFloat,
    /// The angle in the plane, in radians, of the nearest point, from the direction a circle starts toward, counter-clockwise looking down the normal; at most 2^63 either way.
    pub periapsis: WideFloat,
    /// Seconds for one time round, negative for the other way: from 2^-20 to 2^80 seconds either way.
    pub period: WideFloat,
    /// The mean anomaly at time 0, in radians; at most 2^63 either way.
    pub phase: WideFloat,
}

/// A time of the world as a whole number of units, under 2^96 either way, and how many make a second (a tick of the clock and its rate, as `world_storage::rail::RailTime`),
/// so that it is exact however long the world has run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Time {
    pub units: i128,
    pub units_per_second: u32,
}

/// Where a body is from the centre of its orbit, in voxels, and how fast it moves, in voxels a second.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub position: [WideFloat; 3],
    pub velocity: [WideFloat; 3],
}

/// Bits over the width asked for that the working is done in: 64 because Kepler's equation is solved with a slope as small as the ratio (2^-64), and 32 for the roundings
/// of the steps. The tests say what the answer is within.
const GUARD: u32 = 96;

/// The clock the orbit is asked at is under this many units either way (the world's clock is a count of ticks that fits 64 bits).
const MOST_UNITS: u128 = 1 << 96;

/// Bits over the working width for the part of a turn gone: `time / period` has up to 116 whole bits (the clock is under 2^96 units, a period over 2^-20 seconds) that the
/// fraction after them does not have, and the mean anomaly is magnified up to 2^64 by the equation, so 116 + 64 + the roundings are under 192.
const TURN_GUARD: u32 = 192;

impl Elements {
    /// Whether these are the elements of an orbit this program works out: the limits are those of `world_storage`'s rails (a ratio from 2^-64, a radius under 2^121
    /// voxels, a period to 2^80 seconds, an angle to 2^63 radians) and a period of at least 2^-20 seconds.
    pub fn validate(&self) -> Result<(), KeplerError> {
        if self.normal.iter().all(WideFloat::is_zero) {
            return Err(KeplerError::Invalid(
                "the normal of an orbit points somewhere",
            ));
        }
        if !self.radius.is_positive() || self.radius.floor_log2() > Some(120) {
            return Err(KeplerError::Invalid(
                "the radius of an orbit is above nothing and under 2^121 voxels",
            ));
        }
        let smallest_ratio =
            WideFloat::from_i64(1, self.ratio.precision()).times_power_of_two(-64)?;
        let one = WideFloat::from_i64(1, self.ratio.precision());
        if self.ratio.compare(&smallest_ratio) == Ordering::Less
            || self.ratio.compare(&one) == Ordering::Greater
        {
            return Err(KeplerError::Invalid(
                "the ratio of an orbit (1 - e) is from 2^-64 to 1",
            ));
        }
        if self.period.is_zero()
            || self.period.floor_log2() > Some(80)
            || self.period.floor_log2() < Some(-20)
        {
            return Err(KeplerError::Invalid(
                "the period of an orbit is from 2^-20 to 2^80 seconds",
            ));
        }
        for angle in [&self.periapsis, &self.phase] {
            let limit = WideFloat::from_i64(1, angle.precision()).times_power_of_two(63)?;
            if angle.abs().compare(&limit) == Ordering::Greater {
                return Err(KeplerError::Invalid(
                    "an angle of an orbit is at most 2^63 radians",
                ));
            }
        }
        Ok(())
    }

    /// Where the body is at `time`, and its speed, in numbers of `precision` bits.
    pub fn place_at(&self, time: Time, precision: Precision) -> Result<Place, KeplerError> {
        self.validate()?;
        if time.units_per_second == 0 {
            return Err(KeplerError::Invalid("a second is some units of time"));
        }
        if time.units.unsigned_abs() >= MOST_UNITS {
            return Err(KeplerError::Invalid(
                "a time is under 2^96 units of the clock either way",
            ));
        }
        let width = precision.widened_by(GUARD);
        let turns_width = width.widened_by(TURN_GUARD);
        let at = |number: &WideFloat| number.rounded_to(width);

        // The mean anomaly: the phase and the part of a turn gone, which is `units / (units a second x period)` modulo one, worked out in a width that holds its whole
        // part and still leaves the fraction.
        let pi_for_turns = WideFloat::pi(turns_width);
        let two_pi_for_turns = pi_for_turns.times_power_of_two(1)?;
        let units = WideFloat::from_i128(time.units, turns_width);
        let per_second = WideFloat::from_i64(i64::from(time.units_per_second), turns_width);
        let turns = units.divide(&(&per_second * &self.period.rounded_to(turns_width)))?;
        let phase_turns = self
            .phase
            .rounded_to(turns_width)
            .divide(&two_pi_for_turns)?;
        let fraction = fraction_of(&(&fraction_of(&turns)? + &phase_turns))?;
        let mean = at(&(&two_pi_for_turns * &fraction));

        // Kepler's equation is solved on the half turn: a mean anomaly in the second half is the mirror image of one in the first.
        let pi = WideFloat::pi(width);
        let two_pi = pi.times_power_of_two(1)?;
        let reflected = mean.compare(&pi) == Ordering::Greater;
        let mean_on_the_half_turn = if reflected { &two_pi - &mean } else { mean };
        let ratio = at(&self.ratio);
        let one = WideFloat::from_i64(1, width);
        let two = WideFloat::from_i64(2, width);
        let e = &one - &ratio;
        let eccentric = eccentric_anomaly(&ratio, &mean_on_the_half_turn)?;

        // sin E, cos E, and 1 - cos E = 2 sin^2(E / 2), from the half angle so that nothing cancels.
        let (sine_half, cosine_half) = eccentric.times_power_of_two(-1)?.sin_cos();
        let sine = (&sine_half * &cosine_half).times_power_of_two(1)?;
        let half_versine = &sine_half * &sine_half;
        let cosine = &(&cosine_half * &cosine_half) - &half_versine;
        // The place in the plane in units of the semi-major axis from the focus, toward the periapsis and a quarter turn on; the distance from the focus is the slope of the
        // equation; and the part of the speed that is along the second of them has sqrt(1 - e^2) in it.
        let minor = (&ratio * &(&two - &ratio)).sqrt()?;
        let x = &ratio - &half_versine.times_power_of_two(1)?;
        let side = if reflected { -&one } else { one.clone() };
        let y = &(&minor * &sine) * &side;
        let distance = &ratio + &(&e * &half_versine).times_power_of_two(1)?;
        let radius = at(&self.radius);
        let rate = two_pi.divide(&at(&self.period))?;
        let speed = (&rate * &radius).divide(&distance)?;
        let toward = -&(&(&speed * &sine) * &side);
        let along = &(&speed * &minor) * &cosine;

        // The periapsis turns both into the two directions of the plane.
        let (sine_of_periapsis, cosine_of_periapsis) = at(&self.periapsis).sin_cos();
        let turned = |a: &WideFloat, b: &WideFloat| {
            (
                &(a * &cosine_of_periapsis) - &(b * &sine_of_periapsis),
                &(a * &sine_of_periapsis) + &(b * &cosine_of_periapsis),
            )
        };
        let (place_first, place_second) = turned(&(&x * &radius), &(&y * &radius));
        let (speed_first, speed_second) = turned(&toward, &along);

        let (first, second) = plane(&self.normal.each_ref().map(at))?;
        let combine = |a: &WideFloat, b: &WideFloat| -> [WideFloat; 3] {
            [0, 1, 2].map(|axis| (&(a * &first[axis]) + &(b * &second[axis])).rounded_to(precision))
        };
        Ok(Place {
            position: combine(&place_first, &place_second),
            velocity: combine(&speed_first, &speed_second),
        })
    }
}

/// The part of a number after its whole part (from 0 up to, not including, 1; below zero it is the part after the greatest whole number below it).
fn fraction_of(number: &WideFloat) -> Result<WideFloat, KeplerError> {
    let whole = number.floor_to_i128().ok_or(KeplerError::Invalid(
        "an orbit is not gone round that many times",
    ))?;
    Ok(number - &WideFloat::from_i128(whole, number.precision()))
}

/// The two directions in the plane of an orbit with this normal, as unit vectors: the circle starts toward the first and goes toward the second. The first is the +x
/// axis (the +z axis where the normal's x is 0.9 or more, by the number it is given and not by the unit vector made of it, as `world_storage` always decided) with its
/// part along the normal taken out; the second is the normal crossed with the first, so that the circle goes counter-clockwise looking down the normal.
fn plane(normal: &[WideFloat; 3]) -> Result<([WideFloat; 3], [WideFloat; 3]), KeplerError> {
    let precision = normal[0].precision();
    let threshold = WideFloat::from_f64(0.9, precision)?;
    let reference = if normal[0].abs().compare(&threshold) == Ordering::Less {
        0
    } else {
        2
    };
    let unit = unit_vector(normal)?;
    let along = unit[reference].clone();
    let one = WideFloat::from_i64(1, precision);
    let inside = [0, 1, 2].map(|axis| {
        let axis_part = if axis == reference {
            one.clone()
        } else {
            WideFloat::zero(precision)
        };
        &axis_part - &(&unit[axis] * &along)
    });
    let first = unit_vector(&inside)?;
    let second = [
        &(&unit[1] * &first[2]) - &(&unit[2] * &first[1]),
        &(&unit[2] * &first[0]) - &(&unit[0] * &first[2]),
        &(&unit[0] * &first[1]) - &(&unit[1] * &first[0]),
    ];
    Ok((first, second))
}

fn unit_vector(vector: &[WideFloat; 3]) -> Result<[WideFloat; 3], KeplerError> {
    let squares = &(&vector[0].square() + &vector[1].square()) + &vector[2].square();
    let size = squares.sqrt()?;
    Ok([
        vector[0].divide(&size)?,
        vector[1].divide(&size)?,
        vector[2].divide(&size)?,
    ])
}

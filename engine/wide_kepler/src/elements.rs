//! The elements of an ellipse or a hyperbola and where they put a body.

use std::cmp::Ordering;

use wide_float::{Precision, WideFloat};

use crate::KeplerError;
use crate::kepler::{HyperbolicAnomaly, eccentric_anomaly, hyperbolic_anomaly};

/// The elements of an orbit: the same as `world_storage::rail::Orbit` has, as numbers of any width.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Elements {
    /// A vector that is not nothing: the orbit is counter-clockwise looking down it. It is made a unit vector when the orbit is worked out, so it need not be one.
    pub normal: [WideFloat; 3],
    /// The size of the semi-major axis, in voxels: above nothing and under 2^121.
    pub radius: WideFloat,
    /// The periapsis over the size of the semi-major axis, which is 1 - e: **from 2^-64 to 1 for an ellipse (1 a circle), and from -2^-64 down to -2^64 for a hyperbola** (e above 1;
    /// the periapsis is `radius` times the size of the ratio, as it is for an ellipse). Nothing is a parabola, which is not made.
    pub ratio: WideFloat,
    /// The angle in the plane, in radians, of the nearest point, from the direction a circle starts toward, counter-clockwise looking down the normal; at most 2^63 either way.
    pub periapsis: WideFloat,
    /// Seconds for one time round, negative for the other way: from 2^-20 to 2^80 seconds either way. For a hyperbola, which goes round no turn, it is 2 pi over the mean motion
    /// (the mean anomaly is 2 pi over it a second).
    pub period: WideFloat,
    /// The mean anomaly at time 0, in radians: for an ellipse at most 2^63 either way, and a part of a turn as far as the orbit is concerned (a turn more is the same place); for
    /// a hyperbola **as it is, not taken modulo anything** (the mean anomaly at a time is this and 2 pi t over the period, and it is the time of the periapsis that is minus this over the
    /// mean motion), and at most 2^120 either way.
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
pub(crate) const GUARD: u32 = 96;

/// The clock the orbit is asked at is under this many units either way (the world's clock is a count of ticks that fits 64 bits).
pub(crate) const MOST_UNITS: u128 = 1 << 96;

/// Bits over the working width for the part of a turn gone: `time / period` has up to 116 whole bits (the clock is under 2^96 units, a period over 2^-20 seconds) that the
/// fraction after them does not have, and the mean anomaly is magnified up to 2^64 by the equation, so 116 + 64 + the roundings are under 192.
pub(crate) const TURN_GUARD: u32 = 192;

/// Bits over the working width for the mean anomaly of a hyperbola, `phase + 2 pi t / period`: it is the sum of numbers of up to 120 whole bits that may cancel to a tiny one (a body
/// at its periapsis a long time after time 0), which the equation then magnifies up to 2^64, so 120 + 64 and the roundings are under 192.
pub(crate) const MEAN_GUARD: u32 = 192;
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
        let one = WideFloat::from_i64(1, self.ratio.precision());
        let smallest_ratio = one.times_power_of_two(-64)?;
        if self.is_a_hyperbola() {
            let largest_ratio = one.times_power_of_two(64)?;
            let size = self.ratio.abs();
            if size.compare(&smallest_ratio) == Ordering::Less
                || size.compare(&largest_ratio) == Ordering::Greater
            {
                return Err(KeplerError::Invalid(
                    "the ratio of a hyperbola (1 - e, below nothing) is from -2^-64 to -2^64",
                ));
            }
        } else if self.ratio.compare(&smallest_ratio) == Ordering::Less
            || self.ratio.compare(&one) == Ordering::Greater
        {
            return Err(KeplerError::Invalid(
                "the ratio of an orbit (1 - e) is from 2^-64 to 1 for an ellipse and from -2^-64 to -2^64 for a hyperbola",
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
        let phase_limit = if self.is_a_hyperbola() { 120 } else { 63 };
        for (angle, most) in [(&self.periapsis, 63), (&self.phase, phase_limit)] {
            let limit = WideFloat::from_i64(1, angle.precision()).times_power_of_two(most)?;
            if angle.abs().compare(&limit) == Ordering::Greater {
                return Err(KeplerError::Invalid(
                    "an angle of an orbit is at most 2^63 radians, and the phase of a hyperbola 2^120",
                ));
            }
        }
        Ok(())
    }

    /// Whether the ratio is below nothing: the orbit is a hyperbola, which goes round no turn.
    pub fn is_a_hyperbola(&self) -> bool {
        self.ratio.is_negative()
    }

    /// The mean anomaly at `time` as a part of a turn, from 0 up to a whole turn, of `precision` bits: the phase (in radians, over 2 pi) and the part of a turn gone since
    /// time 0, which is `units / (units a second x period)` taken modulo one **exactly**, so that it is right to the last bit however long the world has run. (A part a hair under a
    /// whole turn is rounded to the width and may be the whole turn; the same anomaly.) A negative period goes the other way round.
    pub fn mean_anomaly_in_turns(
        &self,
        time: Time,
        precision: Precision,
    ) -> Result<WideFloat, KeplerError> {
        self.validate()?;
        if self.is_a_hyperbola() {
            return Err(KeplerError::Invalid("a hyperbola goes round no turn"));
        }
        check_time(time)?;
        let turns_width = precision.widened_by(GUARD).widened_by(TURN_GUARD);
        let (_, fraction) = self.mean_anomaly_with_two_pi(time, turns_width)?;
        Ok(fraction.rounded_to(precision))
    }

    /// The mean anomaly of a hyperbola at `time`, `phase + 2 pi t / period` **as it is** (below nothing before the periapsis, above it after; a hyperbola goes round no turn), of
    /// `precision` bits, worked out with the bits its size needs (the phase and the clock term may be 120 bits and cancel). Refused for an ellipse, whose mean anomaly is a part of a turn
    /// ([`Elements::mean_anomaly_in_turns`]).
    pub fn hyperbolic_mean_anomaly(
        &self,
        time: Time,
        precision: Precision,
    ) -> Result<WideFloat, KeplerError> {
        self.validate()?;
        if !self.is_a_hyperbola() {
            return Err(KeplerError::Invalid(
                "the mean anomaly of an ellipse is a part of a turn",
            ));
        }
        check_time(time)?;
        let mean_width = precision.widened_by(GUARD).widened_by(MEAN_GUARD);
        let rate = WideFloat::pi(mean_width)
            .times_power_of_two(1)?
            .divide(&self.period.rounded_to(mean_width))?;
        let seconds = WideFloat::from_i128(time.units, mean_width).divide(&WideFloat::from_i64(
            i64::from(time.units_per_second),
            mean_width,
        ))?;
        Ok((&self.phase.rounded_to(mean_width) + &(&rate * &seconds)).rounded_to(precision))
    }

    /// 2 pi and the mean anomaly in turns (below one), both in `turns_width` bits, for a time that has been checked.
    fn mean_anomaly_with_two_pi(
        &self,
        time: Time,
        turns_width: Precision,
    ) -> Result<(WideFloat, WideFloat), KeplerError> {
        turns_gone(&self.period, &self.phase, time, turns_width)
    }

    /// Where the body is at `time`, and its speed, in numbers of `precision` bits: [`Elements::prepare`] and then [`PreparedElements::place_at`], so that the two ways are the
    /// same to the bit.
    pub fn place_at(&self, time: Time, precision: Precision) -> Result<Place, KeplerError> {
        self.prepare(precision)?.place_at(time)
    }

    /// Everything a place at `precision` bits needs of these elements that is the same whatever the time is (the directions of the plane, the sine and cosine of the
    /// periapsis, the minor axis, the rate, pi), worked out once, for a caller that asks for one orbit at many times. 7 of the 27 microseconds of a place at 128 bits.
    pub fn prepare(&self, precision: Precision) -> Result<PreparedElements, KeplerError> {
        self.validate()?;
        let width = precision.widened_by(GUARD);
        let at = |number: &WideFloat| number.rounded_to(width);
        let pi = WideFloat::pi(width);
        let two_pi = pi.times_power_of_two(1)?;
        let ratio = at(&self.ratio);
        let one = WideFloat::from_i64(1, width);
        let two = WideFloat::from_i64(2, width);
        let radius = at(&self.radius);
        let rate = two_pi.divide(&at(&self.period))?;
        let speed_scale = &rate * &radius;
        // The periapsis turns the place in the plane into its two directions.
        let (sine_of_periapsis, cosine_of_periapsis) = at(&self.periapsis).sin_cos();
        let directions = plane(&self.normal.each_ref().map(at))?;
        let (minor, motion) = if self.is_a_hyperbola() {
            // The part of the speed that is along the second direction of the plane has sqrt(e^2 - 1) in it, which is sqrt(|ratio| (2 + |ratio|)).
            let size = ratio.abs();
            let minor = (&size * &(&two + &size)).sqrt()?;
            let mean_width = width.widened_by(MEAN_GUARD);
            let rate_for_mean = WideFloat::pi(mean_width)
                .times_power_of_two(1)?
                .divide(&self.period.rounded_to(mean_width))?;
            (
                minor,
                Motion::Hyperbola {
                    size,
                    rate: rate_for_mean,
                    phase: self.phase.rounded_to(mean_width),
                },
            )
        } else {
            // The part of the speed that is along the second direction of the plane has sqrt(1 - e^2) in it.
            let minor = (&ratio * &(&two - &ratio)).sqrt()?;
            (
                minor,
                Motion::Ellipse {
                    turns: Turns::new(&self.period, &self.phase, width.widened_by(TURN_GUARD))?,
                    e: &one - &ratio,
                    pi,
                    two_pi,
                },
            )
        };
        Ok(PreparedElements {
            precision,
            width,
            ratio,
            one,
            minor,
            radius,
            speed_scale,
            sine_of_periapsis,
            cosine_of_periapsis,
            plane: directions,
            motion,
        })
    }
}

/// [`Elements`] with what does not depend on the time worked out ([`Elements::prepare`]): a place is then worked out for a time from this without the plane, the periapsis
/// or the rate being worked out again. **It gives the place [`Elements::place_at`] gives, to the bit, at every time and however often it is asked** (`place_at` is
/// `prepare` and this), and it holds about a kilobyte of numbers.
#[derive(Clone, Debug)]
pub struct PreparedElements {
    /// The width of the place that is given, and the width it is worked out in.
    precision: Precision,
    width: Precision,
    /// The ratio as it is (below nothing for a hyperbola).
    ratio: WideFloat,
    one: WideFloat,
    /// sqrt(1 - e^2) for an ellipse, sqrt(e^2 - 1) for a hyperbola: from the ratio.
    minor: WideFloat,
    radius: WideFloat,
    /// The rate of the mean anomaly (2 pi over the period) times the radius: the speed is this over the distance in units of the radius.
    speed_scale: WideFloat,
    sine_of_periapsis: WideFloat,
    cosine_of_periapsis: WideFloat,
    /// The two directions in the plane, as unit vectors.
    plane: ([WideFloat; 3], [WideFloat; 3]),
    motion: Motion,
}

/// What differs between an ellipse and a hyperbola in what is kept.
#[derive(Clone, Debug)]
enum Motion {
    Ellipse {
        turns: Turns,
        /// pi, and 2 pi, at the working width.
        pi: WideFloat,
        two_pi: WideFloat,
        /// 1 - the ratio: the eccentricity, as a number of its own because the ratio may be 2^-64.
        e: WideFloat,
    },
    Hyperbola {
        /// The size of the ratio (e - 1), which is what the formulas of a hyperbola have where an ellipse has the ratio.
        size: WideFloat,
        /// 2 pi over the period, and the phase, at the width the mean anomaly is worked out in (the working width and `MEAN_GUARD`).
        rate: WideFloat,
        phase: WideFloat,
    },
}

/// A place in the plane of the orbit: from the focus, along the direction of the periapsis and a quarter turn on, in voxels; and the speed, the same way, in voxels a second.
struct InPlane {
    place: (WideFloat, WideFloat),
    speed: (WideFloat, WideFloat),
}

impl PreparedElements {
    /// Where the body is at `time`, and its speed, in numbers of the width these were prepared for.
    pub fn place_at(&self, time: Time) -> Result<Place, KeplerError> {
        check_time(time)?;
        let in_plane = match &self.motion {
            Motion::Ellipse {
                turns,
                pi,
                two_pi,
                e,
            } => self.on_the_ellipse(turns, pi, two_pi, e, time)?,
            Motion::Hyperbola { size, rate, phase } => {
                self.on_the_hyperbola(size, rate, phase, time)?
            }
        };

        // The periapsis turns both into the two directions of the plane.
        let turned = |a: &WideFloat, b: &WideFloat| {
            (
                &(a * &self.cosine_of_periapsis) - &(b * &self.sine_of_periapsis),
                &(a * &self.sine_of_periapsis) + &(b * &self.cosine_of_periapsis),
            )
        };
        let (place_first, place_second) = turned(&in_plane.place.0, &in_plane.place.1);
        let (speed_first, speed_second) = turned(&in_plane.speed.0, &in_plane.speed.1);

        let (first, second) = &self.plane;
        let combine = |a: &WideFloat, b: &WideFloat| -> [WideFloat; 3] {
            [0, 1, 2]
                .map(|axis| (&(a * &first[axis]) + &(b * &second[axis])).rounded_to(self.precision))
        };
        Ok(Place {
            position: combine(&place_first, &place_second),
            velocity: combine(&speed_first, &speed_second),
        })
    }

    /// The place and the speed in the plane of an ellipse.
    fn on_the_ellipse(
        &self,
        turns: &Turns,
        pi: &WideFloat,
        two_pi: &WideFloat,
        e: &WideFloat,
        time: Time,
    ) -> Result<InPlane, KeplerError> {
        let width = self.width;
        // The mean anomaly: the phase and the part of a turn gone, worked out in a width that holds its whole part and still leaves the fraction.
        let fraction = turns.fraction_at(time)?;
        let mean = (&turns.two_pi * &fraction).rounded_to(width);

        // Kepler's equation is solved on the half turn: a mean anomaly in the second half is the mirror image of one in the first.
        let reflected = mean.compare(pi) == Ordering::Greater;
        let mean_on_the_half_turn = if reflected { two_pi - &mean } else { mean };
        let eccentric = eccentric_anomaly(&self.ratio, &mean_on_the_half_turn)?;

        // sin E, cos E, and 1 - cos E = 2 sin^2(E / 2), from the half angle so that nothing cancels.
        let (sine_half, cosine_half) = eccentric.times_power_of_two(-1)?.sin_cos();
        let sine = (&sine_half * &cosine_half).times_power_of_two(1)?;
        let half_versine = &sine_half * &sine_half;
        let cosine = &(&cosine_half * &cosine_half) - &half_versine;
        // The place in the plane in units of the semi-major axis from the focus, toward the periapsis and a quarter turn on; the distance from the focus is the slope of the
        // equation.
        let x = &self.ratio - &half_versine.times_power_of_two(1)?;
        let side = if reflected {
            -&self.one
        } else {
            self.one.clone()
        };
        let y = &(&self.minor * &sine) * &side;
        let distance = &self.ratio + &(e * &half_versine).times_power_of_two(1)?;
        let speed = self.speed_scale.divide(&distance)?;
        let toward = -&(&(&speed * &sine) * &side);
        let along = &(&speed * &self.minor) * &cosine;
        Ok(InPlane {
            place: (&x * &self.radius, &y * &self.radius),
            speed: (toward, along),
        })
    }

    /// The place and the speed in the plane of a hyperbola, `M = e sinh H - H`: the same as an ellipse's with the hyperbolic functions in the place of the circular ones and the size
    /// of the ratio (e - 1) in the place of the ratio. The mean anomaly is not reduced (a hyperbola goes round no turn) and is below nothing before the periapsis, where the anomaly
    /// is the mirror image of the one after it.
    fn on_the_hyperbola(
        &self,
        size: &WideFloat,
        rate: &WideFloat,
        phase: &WideFloat,
        time: Time,
    ) -> Result<InPlane, KeplerError> {
        let width = self.width;
        // The mean anomaly: the phase and 2 pi t over the period, the time as a division of whole numbers, in a width that holds the whole of both and leaves the width after them.
        let mean_width = rate.precision();
        let seconds = WideFloat::from_i128(time.units, mean_width).divide(&WideFloat::from_i64(
            i64::from(time.units_per_second),
            mean_width,
        ))?;
        let mean = (phase + &(rate * &seconds)).rounded_to(width);
        let side = if mean.is_negative() {
            -&self.one
        } else {
            self.one.clone()
        };
        // (The solver has the sinh and cosh of half the anomaly by its end: it kept them across its steps.)
        let HyperbolicAnomaly {
            sinh_half,
            cosh_half,
            ..
        } = hyperbolic_anomaly(size, &mean.abs())?;

        // sinh H, cosh H, and cosh H - 1 = 2 sinh^2(H / 2), from the half angle so that nothing cancels.
        let sinh = (&sinh_half * &cosh_half).times_power_of_two(1)?;
        let half_versine = &sinh_half * &sinh_half;
        let cosh = &self.one + &half_versine.times_power_of_two(1)?;
        // In units of the semi-major axis from the focus: toward the periapsis (e - cosh H) and a quarter turn on; the distance from the focus is the slope of the equation.
        let x = size - &half_versine.times_power_of_two(1)?;
        let y = &(&self.minor * &sinh) * &side;
        let distance = &(size * &cosh) + &half_versine.times_power_of_two(1)?;
        let speed = self.speed_scale.divide(&distance)?;
        let toward = -&(&(&speed * &sinh) * &side);
        let along = &(&speed * &self.minor) * &cosh;
        Ok(InPlane {
            place: (&x * &self.radius, &y * &self.radius),
            speed: (toward, along),
        })
    }
}

/// What the part of a turn gone needs of an orbit whatever the time is, in `turns_width` bits: 2 pi, the period, and the phase over 2 pi.
#[derive(Clone, Debug)]
pub(crate) struct Turns {
    two_pi: WideFloat,
    period: WideFloat,
    phase_turns: WideFloat,
}

impl Turns {
    pub(crate) fn new(
        period: &WideFloat,
        phase: &WideFloat,
        turns_width: Precision,
    ) -> Result<Turns, KeplerError> {
        let two_pi = WideFloat::pi(turns_width).times_power_of_two(1)?;
        let phase_turns = phase.rounded_to(turns_width).divide(&two_pi)?;
        Ok(Turns {
            two_pi,
            period: period.rounded_to(turns_width),
            phase_turns,
        })
    }

    /// The part of a turn the body is at, from 0 up to one, at `time` (which has been checked), in the width of the turns: the phase over 2 pi and the time over the
    /// period, taken modulo one **exactly**.
    pub(crate) fn fraction_at(&self, time: Time) -> Result<WideFloat, KeplerError> {
        let turns_width = self.two_pi.precision();
        let units = WideFloat::from_i128(time.units, turns_width);
        let per_second = WideFloat::from_i64(i64::from(time.units_per_second), turns_width);
        let turns = units.divide(&(&per_second * &self.period))?;
        fraction_of(&(&fraction_of(&turns)? + &self.phase_turns))
    }
}

/// 2 pi and the part of a turn a body of this `period` (seconds) and `phase` (radians at time 0) is at, from 0 up to one, at `time` (which has been checked), both in
/// `turns_width` bits: the phase over 2 pi and the time over the period, taken modulo one **exactly**.
pub(crate) fn turns_gone(
    period: &WideFloat,
    phase: &WideFloat,
    time: Time,
    turns_width: Precision,
) -> Result<(WideFloat, WideFloat), KeplerError> {
    let turns = Turns::new(period, phase, turns_width)?;
    let fraction = turns.fraction_at(time)?;
    Ok((turns.two_pi, fraction))
}

/// Whether a time is one the clock can hold.
pub(crate) fn check_time(time: Time) -> Result<(), KeplerError> {
    if time.units_per_second == 0 {
        return Err(KeplerError::Invalid("a second is some units of time"));
    }
    if time.units.unsigned_abs() >= MOST_UNITS {
        return Err(KeplerError::Invalid(
            "a time is under 2^96 units of the clock either way",
        ));
    }
    Ok(())
}

/// The part of a number after its whole part (from 0 up to, not including, 1; below zero it is the part after the greatest whole number below it).
pub(crate) fn fraction_of(number: &WideFloat) -> Result<WideFloat, KeplerError> {
    let whole = number.floor_to_i128().ok_or(KeplerError::Invalid(
        "an orbit is not gone round that many times",
    ))?;
    Ok(number - &WideFloat::from_i128(whole, number.precision()))
}

/// The two directions in the plane of an orbit with this normal, as unit vectors: the circle starts toward the first and goes toward the second. The first is the +x
/// axis (the +z axis where the normal's x is 0.9 or more, by the number it is given and not by the unit vector made of it, as `world_storage` always decided) with its
/// part along the normal taken out; the second is the normal crossed with the first, so that the circle goes counter-clockwise looking down the normal.
pub(crate) fn plane(
    normal: &[WideFloat; 3],
) -> Result<([WideFloat; 3], [WideFloat; 3]), KeplerError> {
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

pub(crate) fn unit_vector(vector: &[WideFloat; 3]) -> Result<[WideFloat; 3], KeplerError> {
    let squares = &(&vector[0].square() + &vector[1].square()) + &vector[2].square();
    let size = squares.sqrt()?;
    Ok([
        vector[0].divide(&size)?,
        vector[1].divide(&size)?,
        vector[2].divide(&size)?,
    ])
}

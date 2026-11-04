//! Rails: how a body moves, as a function of the time of the world and nothing else (`docs/specs/rails-and-recipes-spec.md`, section 3.4).
//!
//! A planet on rails is not integrated: its place and its turn at a time are worked out from the time alone, so they are the same bits on every machine, there is
//! nothing to drift, and a world that is saved and loaded has its planets where the clock says without having saved them. A spin is worked out with `libm` in
//! `f64`; an orbit, which has to be right to a fraction of a voxel however far out it goes, in wide numbers of any width from 128 bits (`wide_kepler`: [`ExactOffset`] is what
//! comes out). The elements of an orbit are wide numbers too and are kept as they are given, to the last bit, in memory and in a file (`engine:rail`); nothing rounds them until a
//! place is asked for. ("Lazy exact rails", the owner, 2026-11-03.)
//! Two motions, each optional and independent:
//!
//! - a **spin**: the body turns steadily about an axis, once in `period` seconds (a negative period turns it the other way), from `angle` at time 0; the body's own
//!   +y axis is carried onto the axis of the spin, so the axis is the body's pole and a tilt is just an axis that is not the level's +y;
//! - an **orbit**: the body goes round a centre on a circle of `radius` voxels in the plane that has `normal` for its normal, once in `period` seconds,
//!   counter-clockwise looking down the normal, from `phase` at time 0. The circle starts toward the +x axis taken into the plane (toward +z when the normal is
//!   nearly along x). The centre is a point of the level, or another body, which may itself be on a rail (a moon goes round a planet that goes round a star).
//!   An orbit may be an **ellipse**: `radius` is then its semi-major axis, `ratio` (1 - e, the periapsis over the semi-major axis, so 1.0 is a circle) says how
//!   long and thin it is, `periapsis` is the angle in the plane of the nearest point from the direction the circle starts toward, and `phase` is the mean anomaly at
//!   time 0 (`wide_kepler`; the definition is in `engine/world_storage/tools/exact_ellipse_reference.py`). The ratio and not the eccentricity is kept because an `f64` e
//!   near 1 has no digits left to say how near it is.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::f64::consts::TAU;

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use voxel_space::{GridId, Orientation};
use wide_float::{Precision, WideFloat};
use wide_kepler::{Elements, KeplerError, PreparedElements, Time};

pub use crate::hand_over::BITS_ADDED_BY_A_HAND_OVER;

pub use crate::exact_offset::ExactOffset;

/// The smallest ratio, 2^-64: an ellipse as thin as this is the sharpest `wide_kepler` solves (Kepler's equation has a slope as small as the ratio, which its guard bits are
/// made for).
pub const SMALLEST_RATIO: f64 = 1.0 / ((1u128 << 64) as f64);

/// Why a spin or an orbit cannot be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RailError {
    #[error("{0} must point somewhere")]
    NoDirection(&'static str),
    #[error("{0}")]
    Invalid(&'static str),
}

impl From<KeplerError> for RailError {
    fn from(error: KeplerError) -> Self {
        match error {
            KeplerError::Invalid(message) => RailError::Invalid(message),
            _ => RailError::Invalid("the elements of an orbit are not those of an orbit"),
        }
    }
}

/// What an orbit goes round.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Centre {
    /// A point of the level, in voxels.
    Point([f64; 3]),
    /// Another body of the world (a ship with a lower number than the body that goes round it, so that its place is worked out first).
    Grid(GridId),
}

/// A steady turn about an axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spin {
    /// A unit vector, in the coordinates of the level: where the body's +y goes.
    pub axis: [f64; 3],
    /// Seconds for one turn; negative for the other way.
    pub period: f64,
    /// How far it has turned at time 0, in radians.
    pub angle: f64,
}

/// A circle or an ellipse gone round at a steady rate. The elements (`wide_kepler::Elements`) are wide numbers, from 128 bits, kept exactly: the radius is the radius of a circle
/// or the semi-major axis of an ellipse, in voxels; the ratio is the periapsis over the semi-major axis, which is 1 - e (1 is a circle and a small number, down to
/// [`SMALLEST_RATIO`], a long thin ellipse); the periapsis is the angle in the plane, in radians, of the nearest point, from the direction a circle starts toward,
/// counter-clockwise looking down the normal; the period is the seconds for one time round, negative for the other way; and the phase is how far round it is at time 0, in radians
/// (for a circle from where it starts and the periapsis, for an ellipse the mean anomaly). The normal need not be a unit vector. Made with [`Orbit::elliptical`] (doubles) or
/// [`Orbit::from_elements`] (any width), both of which refuse what is no orbit; the fields are public for reading, and an orbit built by hand that [`Orbit::from_elements`]
/// would refuse is worked out wrongly, and is refused when read from a file.
#[derive(Clone, Debug, PartialEq)]
pub struct Orbit {
    pub around: Centre,
    pub elements: Elements,
    /// The time the body was put on this orbit, if it is known: an orbit made from a state (`Orbit::from_place` and its kin) starts at the time of the state, and a hand-over (see
    /// [`Orbit::handed_over_to`]) at the instant of the crossing. The orbit says where the body is at every time, but the body is on it only from here, until it leaves the sphere of what it goes
    /// round: so when it leaves is the first crossing **after** this time, which a ship that was parked, or a server that was off, has to be able to find. Saved with the orbit.
    pub since: Option<RailTime>,
    /// How many sphere-of-influence changes the body has been through since it was last put on an orbit from a state (a manoeuvre, a push, a change of the mass of what it goes round):
    /// 0 for an orbit made from a state, and one more for each hand-over (see [`Orbit::handed_over_to`]). The whole path from that state is a function of that first orbit, so this says where on
    /// it the body is. Saved with the orbit.
    pub changes: u32,
    /// How many sphere-of-influence changes after that state the player has asked a prediction of the path to show, counted from the same state (so that the prediction goes on being asked for
    /// as the ship goes through the changes; it is advanced by the player and not by the ship's motion). A hand-over keeps it. See `server::ServerWorld::predicted_path`. Saved with the orbit.
    pub predicted: u32,
}

/// Which ends of an ellipse a body passed in a stretch of time (see [`Orbit::apsides_passed`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Apsides {
    /// The nearest point to the centre, the periapsis.
    pub periapsis: bool,
    /// The farthest, the apoapsis.
    pub apoapsis: bool,
}

/// How a body moves.
#[derive(Clone, Debug, PartialEq)]
pub struct Rail {
    pub spin: Option<Spin>,
    pub orbit: Option<Orbit>,
}

/// Where a rail has a body at a time. What the rail says nothing of is `None`, or still.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RailState {
    /// Where it is, in voxels of the level, if it has an orbit: the centre and the offset from it added as `f64`s, which is exact only as far as an `f64` is.
    pub position: Option<[f64; 3]>,
    /// Where it is from its centre, if it has an orbit: exact to a small fraction of a voxel however far out it is. This is what to add to the centre's place.
    pub offset: Option<ExactOffset>,
    /// How fast it moves, in voxels a second (the centre's speed and its own); still if it has no orbit.
    pub velocity: [f64; 3],
    /// How it is turned, if it has a spin.
    pub orientation: Option<Orientation>,
    /// An axis scaled by radians a second; still if it has no spin.
    pub angular_velocity: [f64; 3],
}

fn finite(values: &[f64]) -> bool {
    values.iter().all(|value| value.is_finite())
}

fn length(vector: [f64; 3]) -> f64 {
    libm::sqrt(vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2])
}

fn unit(vector: [f64; 3], what: &'static str) -> Result<[f64; 3], RailError> {
    if !finite(&vector) {
        return Err(RailError::Invalid("a direction is made of numbers"));
    }
    let size = length(vector);
    if !(size > 0.0 && size.is_finite()) {
        return Err(RailError::NoDirection(what));
    }
    Ok(vector.map(|component| component / size))
}

/// Whether `vector` is a unit vector to within what writing and reading it can lose.
fn is_unit(vector: [f64; 3]) -> bool {
    finite(&vector) && (length(vector) - 1.0).abs() <= 1.0e-9
}

/// The longest period of a spin or an orbit, in seconds: 2^80, a little over 10^24 or 4 x 10^16 years, so that the part of a turn gone is worked out in whole numbers of
/// at most 112 bits.
const LONGEST_PERIOD: f64 = (1u128 << 80) as f64;

/// Whether `period` is one a spin can have: a number that is not nothing and not above [`LONGEST_PERIOD`], either way.
fn usable_period(period: f64) -> bool {
    period.is_finite() && period != 0.0 && period.abs() <= LONGEST_PERIOD
}

/// The largest radius of an orbit, in voxels, as a power of two: 2^120, a million times the distance across the universe in metres, so that its offset in bricks is an `i128`.
const LARGEST_RADIUS_EXPONENT: i64 = 120;

/// The longest period of an orbit, in seconds, as a power of two: 2^80 (the spin's limit too: [`LONGEST_PERIOD`]).
const LONGEST_ORBIT_PERIOD_EXPONENT: i64 = 80;

/// Whether `value`, which is not negative, is at most 2^`exponent`.
fn at_most_a_power_of_two(value: &WideFloat, exponent: i64) -> bool {
    let limit = WideFloat::from_i64(1, value.precision())
        .times_power_of_two(exponent)
        .expect("a power of two within the range of a number");
    value.compare(&limit) != Ordering::Greater
}

/// A time of the world, as a whole number of units (under 2^96 either way) and how many make a second, so that it is exact however long the world has run: a tick of the
/// clock, or a time in seconds that is rounded to a millionth of one (for data and tests; the world always gives ticks). A time before the world began is allowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RailTime {
    units: i128,
    units_per_second: u32,
}

/// A second in the units of a time given in seconds: 2^20, so that every time that is a whole number of those is exact in an `f64`.
const UNITS_IN_A_SECOND_OF_SECONDS: u32 = 1 << 20;

impl RailTime {
    /// The time of tick `tick` of a clock that makes `ticks_per_second` ticks a second. Panics if that is nothing.
    pub fn of_tick(tick: u64, ticks_per_second: u32) -> Self {
        assert!(ticks_per_second > 0, "a clock makes ticks");
        Self {
            units: i128::from(tick),
            units_per_second: ticks_per_second,
        }
    }
}

impl RailTime {
    /// The time `half_ticks` half ticks into a clock that makes `ticks_per_second` ticks a second: an even number is a tick, an odd one halfway between two. For asking where a body is
    /// in its orbit at the middle of the stretch of time that a tick stands for.
    pub fn of_half_ticks(half_ticks: u64, ticks_per_second: u32) -> Self {
        assert!(
            ticks_per_second > 0 && ticks_per_second <= 1 << 30,
            "a clock makes ticks"
        );
        Self {
            units: i128::from(half_ticks),
            units_per_second: 2 * ticks_per_second,
        }
    }
}

/// The most units of a time, either way: just under 2^96 (`wide_kepler`'s limit), which is 2^76 seconds of the units of a time given in seconds, two hundred million times the
/// age of the universe in years.
const MOST_UNITS: i128 = (1 << 96) - 1;

/// A time in seconds (for data and tests): rounded to the nearest millionth of a second. Not a number is the start of the world, and a time further from it than
/// 2^76 seconds (just under 2^96 units) is that far.
impl From<f64> for RailTime {
    fn from(seconds: f64) -> Self {
        Self {
            units: ((seconds * f64::from(UNITS_IN_A_SECOND_OF_SECONDS)).round() as i128)
                .clamp(-MOST_UNITS, MOST_UNITS),
            units_per_second: UNITS_IN_A_SECOND_OF_SECONDS,
        }
    }
}

impl RailTime {
    /// The instant of the grid of `wide_kepler` (see [`wide_kepler::INSTANTS_PER_SECOND`]) that a time is.
    pub(crate) fn of_instant(time: Time) -> Self {
        Self {
            units: time.units,
            units_per_second: time.units_per_second,
        }
    }

    /// The whole number of units of the time (a tick, or a step of the grid of the instants bodies leave spheres at).
    pub fn units(self) -> i128 {
        self.units
    }

    /// How many units make a second.
    pub fn units_per_second(self) -> u32 {
        self.units_per_second
    }

    /// Which of two times is earlier, exactly (a time in units of one rate against one in another: the tick of a server and an instant of the grid). Not `Ord`, because two times
    /// that are the same moment in different units are not equal as values.
    pub fn compare(self, other: Self) -> Ordering {
        // (The units are under 2^96 and a rate under 2^32, so a product fits a `u128`.)
        let (own_sign, other_sign) = (self.units.signum(), other.units.signum());
        if own_sign != other_sign {
            return own_sign.cmp(&other_sign);
        }
        let own = self.units.unsigned_abs() * u128::from(other.units_per_second);
        let theirs = other.units.unsigned_abs() * u128::from(self.units_per_second);
        if own_sign < 0 {
            theirs.cmp(&own)
        } else {
            own.cmp(&theirs)
        }
    }
}

impl RailTime {
    /// As `wide_kepler` has a time.
    pub(crate) fn wide(self) -> Time {
        Time {
            units: self.units,
            units_per_second: self.units_per_second,
        }
    }
}

/// `value.abs()` as a whole number times a power of two, `(mantissa, exponent)`: exact for every `f64` that is a number.
fn split(value: f64) -> (u64, i32) {
    let bits = value.abs().to_bits();
    let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
    let fraction_bits = bits & ((1 << 52) - 1);
    if exponent_bits == 0 {
        (fraction_bits, -1074)
    } else {
        (fraction_bits | (1 << 52), exponent_bits - 1075)
    }
}

/// `period.abs()` as a whole number times a power of two, `(mantissa, exponent)`, with the mantissa odd. Exact for every period that is a number and not nothing.
fn mantissa_and_exponent(period: f64) -> (u64, i32) {
    let (mantissa, exponent) = split(period);
    let even = mantissa.trailing_zeros();
    (mantissa >> even, exponent + even as i32)
}

/// How much of a turn a thing that goes once round in `period` seconds has gone, `time` after it was at the start, as a fraction `(numerator, denominator)` below one;
/// a negative period goes the other way.
///
/// It is worked out in whole numbers, so it is exact however long the world has run: `time` is a count of units and `period` is a whole number times a power of two, so
/// the turns are `units / (units_per_second x mantissa x 2^exponent)` and only the remainder after the whole turns is divided. (An `f64` of seconds divided by an `f64`
/// period loses a digit for every ten turns: after twelve thousand years of a planet's years it is a metre out.) The denominator is below 2^112 for every period a rail
/// can have.
fn turn_fraction(time: RailTime, period: f64) -> (u128, u128) {
    if !usable_period(period) {
        return (0, 1);
    }
    let (mantissa, exponent) = mantissa_and_exponent(period);
    // A thing that goes once round in `mantissa x 2^exponent` seconds has gone `units / (per_second x mantissa x 2^exponent)` turns; `per_second x mantissa` is
    // below 2^85.
    let modulus = i128::from(time.units_per_second) * i128::from(mantissa);
    let (remainder, whole_turn) = if exponent < 0 {
        // The turns are `units x 2^-exponent / modulus`: the part after the whole turns, taking the remainder after each doubling so that nothing grows past 2^125.
        let mut remainder = time.units.rem_euclid(modulus);
        let mut doublings = -exponent;
        while doublings > 0 {
            let step = doublings.min(40);
            remainder = (remainder << step) % modulus;
            doublings -= step;
        }
        (remainder, modulus)
    } else {
        // (Below 2^112: a period is at most 2^80 seconds and a second at most 2^32 units.)
        let whole_turn = modulus << exponent;
        (time.units.rem_euclid(whole_turn), whole_turn)
    };
    let remainder = if period < 0.0 && remainder != 0 {
        whole_turn - remainder
    } else {
        remainder
    };
    (remainder as u128, whole_turn as u128)
}

/// How far round a thing is that goes once round in `period` seconds, in radians (from 0 up to a whole turn), as [`turn_fraction`] puts it, rounded once to an `f64`.
fn fraction_of_a_turn(time: RailTime, period: f64) -> f64 {
    let (remainder, whole_turn) = turn_fraction(time, period);
    TAU * (remainder as f64 / whole_turn as f64)
}

impl Spin {
    /// A spin about `axis` (any vector that is not nothing: it is made a unit vector).
    pub fn new(axis: [f64; 3], period: f64, angle: f64) -> Result<Self, RailError> {
        if !usable_period(period) {
            return Err(RailError::Invalid(
                "a spin has a period that is not nothing and not above 2^80 seconds",
            ));
        }
        if !angle.is_finite() {
            return Err(RailError::Invalid("the angle of a spin is a number"));
        }
        Ok(Self {
            axis: unit(axis, "the axis of a spin")?,
            period,
            angle,
        })
    }

    /// The turn at `time` and how fast it turns.
    fn at(&self, time: RailTime) -> (Orientation, [f64; 3]) {
        let angle = self.angle + fraction_of_a_turn(time, self.period);
        let half = angle * 0.5;
        let (sine, cosine) = (libm::sin(half), libm::cos(half));
        let [x, y, z] = self.axis;
        let spin = Orientation::from_components([x * sine, y * sine, z * sine, cosine])
            .expect("a unit axis and a half angle make a unit quaternion");
        let speed = TAU / self.period;
        (
            spin.compose(self.pole()),
            self.axis.map(|component| component * speed),
        )
    }

    /// The turn that carries the body's +y onto the axis.
    fn pole(&self) -> Orientation {
        let [x, y, z] = self.axis;
        // (+y x axis, 1 + +y . axis), made a unit; and a half turn about x for an axis that is -y, where there is no shortest turn.
        if y < -1.0 + 1.0e-12 {
            return Orientation::from_components([1.0, 0.0, 0.0, 0.0]).expect("a unit quaternion");
        }
        let raw = [z, 0.0, -x, 1.0 + y];
        let size = libm::sqrt(
            raw.iter()
                .map(|component| component * component)
                .sum::<f64>(),
        );
        Orientation::from_components(raw.map(|component| component / size))
            .expect("a normalised quaternion is a unit")
    }
}

impl Orbit {
    /// A circle round `around` in the plane with this normal (any vector that is not nothing).
    pub fn new(
        around: Centre,
        normal: [f64; 3],
        radius: f64,
        period: f64,
        phase: f64,
    ) -> Result<Self, RailError> {
        Self::elliptical(around, normal, radius, 1.0, 0.0, period, phase)
    }

    /// An ellipse round `around` in the plane with this normal (any vector that is not nothing: it is made a unit vector, in `f64`, here): `radius` is the semi-major axis,
    /// `ratio` is 1 - e (1.0 is a circle), and `periapsis` is the angle in radians of the nearest point. The numbers are doubles, which any width holds exactly, so these are
    /// the elements of the orbit as they are given, in 128 bits; [`Orbit::from_elements`] makes an orbit of numbers that are not doubles.
    pub fn elliptical(
        around: Centre,
        normal: [f64; 3],
        radius: f64,
        ratio: f64,
        periapsis: f64,
        period: f64,
        phase: f64,
    ) -> Result<Self, RailError> {
        if !finite(&[radius, ratio, periapsis, period, phase]) {
            return Err(RailError::Invalid("the elements of an orbit are numbers"));
        }
        let exactly = |value: f64| {
            WideFloat::from_f64(value, Precision::MINIMUM).expect("a finite number is a number")
        };
        Self::from_elements(
            around,
            Elements {
                normal: unit(normal, "the normal of an orbit")?.map(exactly),
                radius: exactly(radius),
                ratio: exactly(ratio),
                periapsis: exactly(periapsis),
                period: exactly(period),
                phase: exactly(phase),
            },
        )
    }

    /// A hyperbola round `around` in the plane with this normal: `radius` is the size of the semi-major axis, `excess` is e - 1 (the ratio is minus it), `periapsis` is the angle in
    /// radians of the nearest point, `period` is 2 pi over the mean motion (negative for the other way round) and `phase` is the mean anomaly at time 0 as it is, not taken
    /// modulo a turn (below nothing before the periapsis). The numbers are doubles.
    pub fn hyperbolic(
        around: Centre,
        normal: [f64; 3],
        radius: f64,
        excess: f64,
        periapsis: f64,
        period: f64,
        phase: f64,
    ) -> Result<Self, RailError> {
        if !excess.is_finite() || excess <= 0.0 {
            return Err(RailError::Invalid(
                "a hyperbola has an eccentricity above one (a parabola is not made)",
            ));
        }
        Self::elliptical(around, normal, radius, -excess, periapsis, period, phase)
    }

    /// An orbit round `around` of these elements, of any width from 128 bits, kept exactly as they are. Refused: a centre that is not made of numbers, and what `wide_kepler`
    /// refuses (a normal of nothing, a ratio outside 2^-64 to 1 for an ellipse and outside -2^-64 to -2^64 for a hyperbola, a period outside 2^-20 to 2^80 seconds either way, an
    /// angle over 2^63 radians and a phase over 2^120 for a hyperbola, a radius that is not above nothing), and a radius over 2^120 voxels. The normal need not be a unit vector.
    pub fn from_elements(around: Centre, elements: Elements) -> Result<Self, RailError> {
        if let Centre::Point(point) = around
            && !finite(&point)
        {
            return Err(RailError::Invalid(
                "the centre of an orbit is made of numbers",
            ));
        }
        elements.validate()?;
        if !at_most_a_power_of_two(&elements.radius, LARGEST_RADIUS_EXPONENT)
            || !at_most_a_power_of_two(&elements.period.abs(), LONGEST_ORBIT_PERIOD_EXPONENT)
        {
            return Err(RailError::Invalid(
                "an orbit has a radius of at most 2^120 voxels and a period of at most 2^80 seconds",
            ));
        }
        Ok(Self {
            around,
            elements,
            since: None,
            changes: 0,
            predicted: 0,
        })
    }

    /// The same orbit, the body put on it at `time` (see [`Orbit::since`]).
    #[must_use]
    pub fn starting_at(mut self, time: RailTime) -> Self {
        self.since = Some(time);
        self
    }

    /// The semi-major axis (the radius of a circle) in voxels, as the `f64` nearest to it: for sizes and comparisons, not for placing a body.
    pub fn radius(&self) -> f64 {
        self.elements.radius.to_f64()
    }

    /// The periapsis over the semi-major axis, 1 - e, as the nearest `f64`.
    pub fn ratio(&self) -> f64 {
        self.elements.ratio.to_f64()
    }

    /// The angle of the nearest point in the plane, in radians, as the nearest `f64`.
    pub fn periapsis(&self) -> f64 {
        self.elements.periapsis.to_f64()
    }

    /// The seconds for one time round (negative for the other way), as the nearest `f64`.
    pub fn period(&self) -> f64 {
        self.elements.period.to_f64()
    }

    /// The mean anomaly at time 0, in radians, as the nearest `f64`.
    pub fn phase(&self) -> f64 {
        self.elements.phase.to_f64()
    }

    /// The normal as the nearest `f64`s (a vector in the direction of the normal, not necessarily a unit one).
    pub fn normal(&self) -> [f64; 3] {
        self.elements.normal.each_ref().map(WideFloat::to_f64)
    }

    /// Whether it is a hyperbola: a ratio below nothing (e above 1). A hyperbola goes round no turn, passes its periapsis once and goes out for ever.
    pub fn is_a_hyperbola(&self) -> bool {
        self.elements.is_a_hyperbola()
    }

    /// Whether the periapsis is still to come at `time`: always for an ellipse (it comes round), and for a hyperbola until it has been passed (its mean anomaly is below nothing, or
    /// above it for the other way round). A hyperbola that has passed it goes away for ever, and the nearest it came is behind it.
    pub fn periapsis_ahead(&self, time: RailTime) -> bool {
        if !self.is_a_hyperbola() {
            return true;
        }
        let mean = self
            .elements
            .hyperbolic_mean_anomaly(time.wide(), Precision::of(192))
            .expect("an orbit made by this module has a mean anomaly");
        if self.elements.period.is_negative() {
            mean.is_positive()
        } else {
            mean.is_negative()
        }
    }

    /// Whether it is a circle: a ratio of exactly 1.
    pub fn is_circle(&self) -> bool {
        let one = WideFloat::from_i64(1, self.elements.ratio.precision());
        self.elements.ratio.compare(&one) == Ordering::Equal
    }

    /// The eccentricity (1 less the ratio), as the nearest `f64`: exact however small it is, because the ratio is kept and not the eccentricity (see [`Orbit::ratio`]).
    pub fn eccentricity(&self) -> f64 {
        let one = WideFloat::from_i64(1, self.elements.ratio.precision());
        (&one - &self.elements.ratio).to_f64()
    }

    /// How near to its centre the body comes, in voxels, as an `f64` (the semi-major axis times the size of the ratio).
    pub fn nearest_distance(&self) -> f64 {
        self.radius() * self.ratio().abs()
    }

    /// How far from its centre the body goes, in voxels, as an `f64` (the semi-major axis times 2 less the ratio); **endless for a hyperbola**.
    pub fn farthest_distance(&self) -> f64 {
        if self.is_a_hyperbola() {
            return f64::INFINITY;
        }
        self.radius() * (2.0 - self.ratio())
    }

    /// The mass parameter, `G M` of what the orbit goes round in voxels cubed a second squared, that the period and the semi-major axis say (`mu = n^2 a^3`, Kepler's third law). An
    /// orbit made from a state (`Orbit::from_place`) has the period of the mass it was made with, so this is that mass, to the rounding of two numbers: it is how a ship that is
    /// on a rail knows the mass it was made for.
    pub fn mass_parameter(&self) -> f64 {
        let rate = TAU / self.period();
        rate * rate * self.radius() * self.radius() * self.radius()
    }

    /// The mean anomaly at `time`, as a fraction of a turn of 128 bits (rounded down, the whole turn being nothing): the exact part of a turn gone and the phase, worked out in
    /// 192 bits.
    fn mean_anomaly(&self, time: RailTime) -> u128 {
        let fraction = self
            .elements
            .mean_anomaly_in_turns(time.wide(), Precision::of(192))
            .expect("an orbit made by this module has a mean anomaly");
        turns_with_128_bits(&fraction)
    }

    /// Which ends of the ellipse the body passes after `from` up to and including `to` (a stretch that starts where the one before ended passes each end once, never twice).
    /// Worked out from the mean anomaly in whole numbers: the periapsis is at 0 and the apoapsis at half a turn. A circle has no ends, and passes neither. A stretch of a whole turn
    /// or more passes both.
    pub fn apsides_passed(&self, from: RailTime, to: RailTime) -> Apsides {
        if self.is_a_hyperbola() {
            // The periapsis is where the mean anomaly is nothing, and it is passed once: in the stretch that has it after `from` up to and including `to`. There is no apoapsis.
            let mean = |time: RailTime| {
                self.elements
                    .hyperbolic_mean_anomaly(time.wide(), Precision::of(192))
                    .expect("an orbit made by this module has a mean anomaly")
            };
            let (before, after) = (mean(from), mean(to));
            let passed = if self.elements.period.is_negative() {
                before.is_positive() && !after.is_positive()
            } else {
                before.is_negative() && !after.is_negative()
            };
            return Apsides {
                periapsis: passed,
                apoapsis: false,
            };
        }
        if self.is_circle() {
            return Apsides::default();
        }
        // (The whole numbers know a part of a turn, not how many turns: a stretch as long as the period passes both ends whatever they say.)
        let seconds = |time: RailTime| time.units as f64 / f64::from(time.units_per_second);
        if seconds(to) - seconds(from) >= self.period().abs() {
            return Apsides {
                periapsis: true,
                apoapsis: true,
            };
        }
        let (before, after) = (self.mean_anomaly(from), self.mean_anomaly(to));
        let forward = !self.elements.period.is_negative();
        // How far round, in the way the body goes, a mean anomaly is from where it was before.
        let gone = |anomaly: u128| {
            if forward {
                anomaly.wrapping_sub(before)
            } else {
                before.wrapping_sub(anomaly)
            }
        };
        let reached = gone(after);
        let passed = |end: u128| {
            let distance = gone(end);
            distance != 0 && distance <= reached
        };
        Apsides {
            periapsis: passed(0),
            apoapsis: passed(1 << 127),
        }
    }

    /// Where the body is from the centre at `time`, and how fast it moves round it. The place is worked out in wide numbers (`wide_kepler`: for a circle or an ellipse, the
    /// phase and the part of a turn gone, the mean anomaly, Kepler's equation, the periapsis and the plane) from the elements as they are, in a width that leaves the last bit of
    /// it below 2^-64 voxel (see [`answer_precision`]), and rounded once, to whole bricks and a rest in steps of 2^-32 voxel; the speed is the same working rounded to an `f64`.
    fn at(&self, time: RailTime) -> Option<(ExactOffset, [f64; 3])> {
        Self::at_prepared(&self.prepare(), time)
    }

    /// What the place of this orbit needs whatever the time is, worked out once (see [`RailMemo`], which keeps it while the orbit does not change).
    fn prepare(&self) -> PreparedElements {
        self.elements
            .prepare(answer_precision(&self.elements))
            .expect("an orbit made by this module is one `wide_kepler` works out")
    }

    /// As [`Orbit::at`], from the orbit as [`Orbit::prepare`] made it.
    /// `None` for a place the world cannot say (a body on a hyperbola so far out that its bricks do not fit an `i128`).
    fn at_prepared(prepared: &PreparedElements, time: RailTime) -> Option<(ExactOffset, [f64; 3])> {
        let place = prepared
            .place_at(time.wide())
            .expect("an orbit made by this module is one `wide_kepler` works out");
        Some((
            ExactOffset::of_voxels(&place.position)?,
            place.velocity.map(|component| component.to_f64()),
        ))
    }
}

/// A part of a turn from 0 to a whole turn as a fraction of a turn with 128 bits after the point, rounded down; a whole turn is the same place as nothing.
fn turns_with_128_bits(fraction: &WideFloat) -> u128 {
    let wide_part = fraction
        .times_power_of_two(64)
        .expect("a fraction times 2^64 is a number");
    let high = wide_part
        .floor_to_i128()
        .expect("a part of a turn times 2^64 is a whole number of at most 65 bits");
    if high >> 64 != 0 {
        return 0;
    }
    // (What is left after the whole part is exact: it has fewer bits than the number had.)
    let rest = &wide_part - &WideFloat::from_i128(high, wide_part.precision());
    let low = rest
        .times_power_of_two(64)
        .expect("a rest times 2^64 is a number")
        .floor_to_i128()
        .expect("a rest below one times 2^64 is a whole number of at most 64 bits");
    ((high as u128) << 64) | low as u128
}

/// The width a place is worked out in: the bits above the point of a place as far out as an ellipse of this `radius` goes (twice it), 64 below it (the answer keeps 32, and
/// the place is then the nearest step except where it is within 2^-32 of a step of a tie), to the next multiple of 64, and at least 128. **A hyperbola goes out for ever**, and a place
/// is any distance the world can say: under 2^132 voxels (an `i128` of bricks). With the 64 bits below it that is 196; 192 leaves 60 below the point at the very edge of the world (and the
/// working has 96 bits more than this), and 124 at a place of 2^68.
fn answer_precision(elements: &Elements) -> Precision {
    if elements.is_a_hyperbola() {
        return Precision::of(192);
    }
    let radius = &elements.radius;
    let bits_of_radius = radius.floor_log2().unwrap_or(0) + 1;
    let wanted = (bits_of_radius + 1 + 64).max(0) as u64;
    Precision::new((wanted.div_ceil(64) * 64).max(Precision::MINIMUM.bits()))
        .expect("the radius of an orbit is at most 2^120 voxels, which takes 192 bits")
}

impl Rail {
    /// Where the body is, how fast it moves and how it is turned `time` into the life of the world (a [`RailTime`], or seconds as an `f64`). `centre_position` and
    /// `centre_velocity` are those of what the orbit goes round (the point, and still, for a point), which the caller knows; a rail with no orbit does not look at
    /// them. The position is the centre's and the body's offset from it added in `f64`, which is exact only as far as an `f64` is; the `offset` is exact, and is what to
    /// add to a [`voxel_space::WorldPosition`] with `translated_by_bricks`, which is how the world places a body that goes round a body.
    pub fn state(
        &self,
        time: impl Into<RailTime>,
        centre_position: [f64; 3],
        centre_velocity: [f64; 3],
    ) -> RailState {
        let time = time.into();
        let orbit = self.orbit.as_ref().and_then(|orbit| orbit.at(time));
        self.state_given(time, centre_position, centre_velocity, orbit)
    }

    /// As [`Rail::state`], with what the orbit says at `time` already worked out (see [`RailMemo`]).
    fn state_given(
        &self,
        time: RailTime,
        centre_position: [f64; 3],
        centre_velocity: [f64; 3],
        orbit: Option<(ExactOffset, [f64; 3])>,
    ) -> RailState {
        let mut state = RailState {
            position: None,
            offset: None,
            velocity: [0.0; 3],
            orientation: None,
            angular_velocity: [0.0; 3],
        };
        if let Some((offset, speed)) = orbit {
            let offset_in_voxels = offset.in_voxels();
            state.offset = Some(offset);
            state.position =
                Some([0, 1, 2].map(|axis| centre_position[axis] + offset_in_voxels[axis]));
            state.velocity = [0, 1, 2].map(|axis| centre_velocity[axis] + speed[axis]);
        }
        if let Some(spin) = &self.spin {
            let (orientation, angular_velocity) = spin.at(time);
            state.orientation = Some(orientation);
            state.angular_velocity = angular_velocity;
        }
        state
    }
}

/// What an orbit said of its body at a time, kept so that the same question is not worked out twice, and what an orbit needs whatever the time is, kept so that it is worked
/// out once while the orbit does not change. The server puts the ships on their rails at the start of a tick and again after the logic of it (to put a ship that came into
/// memory, or was put on a rail, where its rail says), and an orbit is a pure function of the time, so the second putting needs no working out; and a ship that goes round
/// for thousands of ticks needs its plane, the sine and cosine of its periapsis and its minor axis (a quarter of the cost of a place) once, not at every tick.
/// **The memory never changes an answer**: it keeps, for each ship, the orbit, what was prepared from it and the time and answer it was last asked, and gives the answer back
/// only for the very same orbit (every element, width and all) at the very same time, and uses what was prepared only for the very same orbit; anything else is worked out and
/// put in its place. A spin is a few `f64` operations and is never kept. What the orbit goes round is not a part of what it says (that is added after), so a moved centre does
/// not spoil an answer.
#[derive(Clone, Debug, Default)]
pub struct RailMemo {
    entries: HashMap<GridId, Remembered>,
    evaluations: u64,
    reused: u64,
    preparations: u64,
}

#[derive(Clone, Debug)]
struct Remembered {
    orbit: Orbit,
    prepared: PreparedElements,
    time: RailTime,
    /// What the orbit said: `None` if it put the body where the world has no place.
    placed: Option<(ExactOffset, [f64; 3])>,
}

impl RailMemo {
    /// As [`Rail::state`] for the ship `grid`: the same answer to the bit, with what the orbit says remembered if it was asked the same just before.
    pub fn state(
        &mut self,
        grid: GridId,
        rail: &Rail,
        time: RailTime,
        centre_position: [f64; 3],
        centre_velocity: [f64; 3],
    ) -> RailState {
        let orbit = match &rail.orbit {
            None => {
                self.entries.remove(&grid);
                None
            }
            Some(orbit) => self.orbit_at(grid, orbit, time),
        };
        rail.state_given(time, centre_position, centre_velocity, orbit)
    }

    fn orbit_at(
        &mut self,
        grid: GridId,
        orbit: &Orbit,
        time: RailTime,
    ) -> Option<(ExactOffset, [f64; 3])> {
        if let Some(kept) = self.entries.get_mut(&grid)
            && kept.orbit == *orbit
        {
            if kept.time == time {
                self.reused += 1;
                return kept.placed;
            }
            // The same orbit at another time: what does not depend on the time is kept.
            let placed = Orbit::at_prepared(&kept.prepared, time);
            self.evaluations += 1;
            (kept.time, kept.placed) = (time, placed);
            return placed;
        }
        let prepared = orbit.prepare();
        let placed = Orbit::at_prepared(&prepared, time);
        self.preparations += 1;
        self.evaluations += 1;
        self.entries.insert(
            grid,
            Remembered {
                orbit: orbit.clone(),
                prepared,
                time,
                placed,
            },
        );
        placed
    }

    /// Forgets the ships for which `keep` is false (those that are not on a rail any more, or not in the world).
    pub fn keep_only(&mut self, mut keep: impl FnMut(GridId) -> bool) {
        self.entries.retain(|grid, _| keep(*grid));
    }

    /// How many ships have an answer kept.
    pub fn remembered(&self) -> usize {
        self.entries.len()
    }

    /// How many times an orbit was worked out for a time (a place and a speed: 8 microseconds for a circle and 20 for an ellipse at the widths of a ship, once the orbit is prepared), since this was made.
    pub fn evaluations(&self) -> u64 {
        self.evaluations
    }

    /// How many times an answer that was kept was given back instead.
    pub fn reused(&self) -> u64 {
        self.reused
    }

    /// How many times what an orbit needs whatever the time is was worked out (about 7 microseconds), which is once for an orbit that stays on a ship and not once for each
    /// time it is asked.
    pub fn preparations(&self) -> u64 {
        self.preparations
    }
}

const HAS_SPIN: u8 = 1;
const HAS_ORBIT: u8 = 2;
/// The orbit has a time it starts at (see [`Orbit::since`]); only with an orbit.
const HAS_START: u8 = 4;
/// The orbit is some changes of sphere of influence into a path, and a prediction has been asked for (see [`Orbit::changes`]); only with an orbit.
const HAS_CHAIN: u8 = 8;
const ABOUT_A_POINT: u8 = 0;
const ABOUT_A_GRID: u8 = 1;

fn write_vector(writer: &mut ByteWriter, vector: [f64; 3]) {
    for component in vector {
        writer.write_u64(component.to_bits());
    }
}

fn read_vector(reader: &mut ByteReader<'_>) -> Result<[f64; 3], DecodeError> {
    Ok([
        f64::from_bits(reader.read_u64()?),
        f64::from_bits(reader.read_u64()?),
        f64::from_bits(reader.read_u64()?),
    ])
}

fn read_number(reader: &mut ByteReader<'_>) -> Result<f64, DecodeError> {
    Ok(f64::from_bits(reader.read_u64()?))
}

/// The bytes of a rail (the payload of a ship file's `engine:rail`, version [`crate::engine_sections::RAIL_VERSION`]): a byte of flags (1 a spin, 2 an orbit); the spin's axis,
/// period and angle; the orbit's centre (a byte, 0 and a point or 1 and the number of a body), normal, radius, period, phase, ratio and periapsis. Every number is the bits of an
/// `f64`, little-endian. A circle has a ratio of 1.0 and a periapsis of nothing.
pub fn encode_rail(rail: &Rail) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    let flags = if rail.spin.is_some() { HAS_SPIN } else { 0 }
        | if rail.orbit.is_some() { HAS_ORBIT } else { 0 }
        | if rail
            .orbit
            .as_ref()
            .is_some_and(|orbit| orbit.since.is_some())
        {
            HAS_START
        } else {
            0
        }
        | if rail
            .orbit
            .as_ref()
            .is_some_and(|orbit| orbit.changes != 0 || orbit.predicted != 0)
        {
            HAS_CHAIN
        } else {
            0
        };
    writer.write_u8(flags);
    if let Some(spin) = &rail.spin {
        write_vector(&mut writer, spin.axis);
        writer.write_u64(spin.period.to_bits());
        writer.write_u64(spin.angle.to_bits());
    }
    if let Some(orbit) = &rail.orbit {
        match orbit.around {
            Centre::Point(point) => {
                writer.write_u8(ABOUT_A_POINT);
                write_vector(&mut writer, point);
            }
            Centre::Grid(grid) => {
                writer.write_u8(ABOUT_A_GRID);
                writer.write_varint(grid.get());
            }
        }
        let elements = &orbit.elements;
        for number in elements.normal.iter().chain([
            &elements.radius,
            &elements.period,
            &elements.phase,
            &elements.ratio,
            &elements.periapsis,
        ]) {
            number.write_to(&mut writer);
        }
        if let Some(since) = orbit.since {
            writer.write_wide_signed(since.units);
            writer.write_varint(u64::from(since.units_per_second));
        }
        if orbit.changes != 0 || orbit.predicted != 0 {
            writer.write_varint(u64::from(orbit.changes));
            writer.write_varint(u64::from(orbit.predicted));
        }
    }
    writer.into_bytes()
}

/// The rail of a payload. Refuses what [`Spin::new`] and [`Orbit::elliptical`] refuse (the directions must be unit vectors already: what was written was read back as it
/// was, to the bit), a flag or a centre this program does not know, bytes that end too soon and bytes after the end.
pub fn decode_rail(payload: &[u8]) -> Result<Rail, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let flags = reader.read_u8()?;
    if flags & !(HAS_SPIN | HAS_ORBIT | HAS_START | HAS_CHAIN) != 0 {
        return Err(DecodeError::Invalid("a rail with a flag nobody knows"));
    }
    if flags & (HAS_START | HAS_CHAIN) != 0 && flags & HAS_ORBIT == 0 {
        return Err(DecodeError::Invalid(
            "a start or a place in a path of an orbit the rail does not have",
        ));
    }
    let spin = if flags & HAS_SPIN != 0 {
        let spin = Spin {
            axis: read_vector(&mut reader)?,
            period: read_number(&mut reader)?,
            angle: read_number(&mut reader)?,
        };
        if !is_unit(spin.axis) || !usable_period(spin.period) || !spin.angle.is_finite() {
            return Err(DecodeError::Invalid("a spin that is no spin"));
        }
        Some(spin)
    } else {
        None
    };
    let orbit = if flags & HAS_ORBIT != 0 {
        let around = match reader.read_u8()? {
            ABOUT_A_POINT => Centre::Point(read_vector(&mut reader)?),
            ABOUT_A_GRID => Centre::Grid(GridId::new(reader.read_varint()?)),
            _ => {
                return Err(DecodeError::Invalid(
                    "an orbit about a kind of centre nobody knows",
                ));
            }
        };
        let mut number = || WideFloat::read_from(&mut reader);
        let normal = [number()?, number()?, number()?];
        let radius = number()?;
        let period = number()?;
        let phase = number()?;
        let ratio = number()?;
        let periapsis = number()?;
        let elements = Elements {
            normal,
            radius,
            ratio,
            periapsis,
            period,
            phase,
        };
        let mut orbit = Orbit::from_elements(around, elements)
            .map_err(|_| DecodeError::Invalid("an orbit that is no orbit"))?;
        if flags & HAS_START != 0 {
            let units = reader.read_wide_signed()?;
            let per_second = u32::try_from(reader.read_varint()?)
                .ok()
                .filter(|per_second| *per_second > 0)
                .ok_or(DecodeError::Invalid("a start with no units in a second"))?;
            if units.unsigned_abs() > MOST_UNITS.unsigned_abs() {
                return Err(DecodeError::Invalid("a start further than the clock goes"));
            }
            orbit = orbit.starting_at(RailTime {
                units,
                units_per_second: per_second,
            });
        }
        if flags & HAS_CHAIN != 0 {
            orbit.changes = u32::try_from(reader.read_varint()?)
                .map_err(|_| DecodeError::Invalid("a count of changes that is too many"))?;
            orbit.predicted = u32::try_from(reader.read_varint()?)
                .map_err(|_| DecodeError::Invalid("a count of changes that is too many"))?;
            if orbit.changes == 0 && orbit.predicted == 0 {
                return Err(DecodeError::Invalid("a place in a path that is none"));
            }
        }
        Some(orbit)
    } else {
        None
    };
    reader.finish()?;
    Ok(Rail { spin, orbit })
}

//! Rails: how a body moves, as a function of the time of the world and nothing else (`docs/rails-and-recipes-spec.md`, section 3.4).
//!
//! A planet on rails is not integrated: its place and its turn at a time are worked out from the time alone, with `libm` in `f64`, so they are the same
//! bits on every machine, there is nothing to drift, and a world that is saved and loaded has its planets where the clock says without having saved them.
//! Two motions, each optional and independent:
//!
//! - a **spin**: the body turns steadily about an axis, once in `period` seconds (a negative period turns it the other way), from `angle` at time 0; the body's own
//!   +y axis is carried onto the axis of the spin, so the axis is the body's pole and a tilt is just an axis that is not the level's +y;
//! - an **orbit**: the body goes round a centre on a circle of `radius` voxels in the plane that has `normal` for its normal, once in `period` seconds,
//!   counter-clockwise looking down the normal, from `phase` at time 0. The circle starts toward the +x axis taken into the plane (toward +z when the normal is
//!   nearly along x). The centre is a point of the level, or another body, which may itself be on a rail (a moon goes round a planet that goes round a star).
//!
//! Circular only to begin with; the orbit is a type so that an ellipse can be added to it without changing a file.

use std::f64::consts::TAU;

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use voxel_space::{GridId, Orientation};

/// Why a spin or an orbit cannot be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RailError {
    #[error("{0} must point somewhere")]
    NoDirection(&'static str),
    #[error("{0}")]
    Invalid(&'static str),
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

/// A circle gone round at a steady rate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orbit {
    pub around: Centre,
    /// A unit vector: the circle is counter-clockwise looking down it.
    pub normal: [f64; 3],
    /// In voxels.
    pub radius: f64,
    /// Seconds for one time round; negative for the other way.
    pub period: f64,
    /// How far round it is at time 0, in radians.
    pub phase: f64,
}

/// How a body moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rail {
    pub spin: Option<Spin>,
    pub orbit: Option<Orbit>,
}

/// Where a rail has a body at a time. What the rail says nothing of is `None`, or still.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RailState {
    /// Where its centre is, in voxels of the level, if it has an orbit.
    pub position: Option<[f64; 3]>,
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

fn nonzero(period: f64) -> bool {
    period.is_finite() && period != 0.0
}

fn cross(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ]
}

/// A time of the world, as a whole number of units and how many make a second, so that it is exact however long the world has run: a tick of the clock, or a time
/// in seconds that is rounded to a millionth of one (for data and tests; the world always gives ticks). A time before the world began is allowed.
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

/// A time in seconds (for data and tests): rounded to the nearest millionth of a second. Not a number is the start of the world.
impl From<f64> for RailTime {
    fn from(seconds: f64) -> Self {
        Self {
            units: (seconds * f64::from(UNITS_IN_A_SECOND_OF_SECONDS)).round() as i128,
            units_per_second: UNITS_IN_A_SECOND_OF_SECONDS,
        }
    }
}

/// `period.abs()` as a whole number times a power of two, `(mantissa, exponent)`, with the mantissa odd. Exact for every period that is a number and not nothing.
fn mantissa_and_exponent(period: f64) -> (u64, i32) {
    let bits = period.abs().to_bits();
    let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
    let fraction_bits = bits & ((1 << 52) - 1);
    let (mantissa, exponent) = if exponent_bits == 0 {
        (fraction_bits, -1074)
    } else {
        (fraction_bits | (1 << 52), exponent_bits - 1075)
    };
    let even = mantissa.trailing_zeros();
    (mantissa >> even, exponent + even as i32)
}

/// How far round a thing is that goes once round in `period` seconds, in radians (from 0 up to a whole turn), `time` after it was at the start; a negative period
/// goes the other way.
///
/// The part of a turn is worked out in whole numbers, so it is exact (to the one rounding of an `f64`) however long the world has run: `time` is a count of units and
/// `period` is a whole number times a power of two, so the turns are `units / (units_per_second x mantissa x 2^exponent)` and only the remainder after the whole turns
/// is divided. (An `f64` of seconds divided by an `f64` period loses a digit for every ten turns: after twelve thousand years of a planet's years it is a metre out.)
fn fraction_of_a_turn(time: RailTime, period: f64) -> f64 {
    if !nonzero(period) {
        return 0.0;
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
    } else if exponent <= 40 {
        let whole_turn = modulus << exponent;
        (time.units.rem_euclid(whole_turn), whole_turn)
    } else {
        // A period of more than 2^93 seconds: no clock of 2^64 ticks has run for a turn of it, so the part is the number of turns, which is below one.
        let turns = time.units as f64 / libm::ldexp(modulus as f64, exponent);
        let part = turns - libm::floor(turns);
        return TAU
            * if period < 0.0 && part != 0.0 {
                1.0 - part
            } else {
                part
            };
    };
    let remainder = if period < 0.0 && remainder != 0 {
        whole_turn - remainder
    } else {
        remainder
    };
    TAU * (remainder as f64 / whole_turn as f64)
}

impl Spin {
    /// A spin about `axis` (any vector that is not nothing: it is made a unit vector).
    pub fn new(axis: [f64; 3], period: f64, angle: f64) -> Result<Self, RailError> {
        if !nonzero(period) {
            return Err(RailError::Invalid(
                "a spin has a period that is not nothing",
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
    /// An orbit round `around` in the plane with this normal (any vector that is not nothing).
    pub fn new(
        around: Centre,
        normal: [f64; 3],
        radius: f64,
        period: f64,
        phase: f64,
    ) -> Result<Self, RailError> {
        if let Centre::Point(point) = around
            && !finite(&point)
        {
            return Err(RailError::Invalid(
                "the centre of an orbit is made of numbers",
            ));
        }
        if !(radius > 0.0 && radius.is_finite()) {
            return Err(RailError::Invalid("an orbit has a radius above nothing"));
        }
        if !nonzero(period) {
            return Err(RailError::Invalid(
                "an orbit has a period that is not nothing",
            ));
        }
        if !phase.is_finite() {
            return Err(RailError::Invalid("the phase of an orbit is a number"));
        }
        Ok(Self {
            around,
            normal: unit(normal, "the normal of an orbit")?,
            radius,
            period,
            phase,
        })
    }

    /// The two directions in the plane the circle starts toward and then goes toward.
    fn plane(&self) -> ([f64; 3], [f64; 3]) {
        let reference = if self.normal[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 0.0, 1.0]
        };
        let along = reference[0] * self.normal[0]
            + reference[1] * self.normal[1]
            + reference[2] * self.normal[2];
        let inside = [0, 1, 2].map(|axis| reference[axis] - self.normal[axis] * along);
        let size = length(inside);
        let first = inside.map(|component| component / size);
        (first, cross(self.normal, first))
    }

    /// Where the body is from the centre at `time`, and how fast it moves round it.
    fn at(&self, time: RailTime) -> ([f64; 3], [f64; 3]) {
        let angle = self.phase + fraction_of_a_turn(time, self.period);
        let (sine, cosine) = (libm::sin(angle), libm::cos(angle));
        let (first, second) = self.plane();
        let speed = self.radius * (TAU / self.period);
        (
            [0, 1, 2].map(|axis| self.radius * (cosine * first[axis] + sine * second[axis])),
            [0, 1, 2].map(|axis| speed * (cosine * second[axis] - sine * first[axis])),
        )
    }
}

impl Rail {
    /// Where the body is, how fast it moves and how it is turned `time` into the life of the world (a [`RailTime`], or seconds as an `f64`). `centre_position` and
    /// `centre_velocity` are those of what the orbit goes round (the point, and still, for a point), which the caller knows; a rail with no orbit does not look at
    /// them. The position is the centre's and the body's offset from it added in `f64`, which is exact only as far as an `f64` is: give the origin as the centre to get
    /// the offset alone, and add it to a [`voxel_space::WorldPosition`] with `translated_by_voxels`, which is how the world places a body that goes round a body.
    pub fn state(
        &self,
        time: impl Into<RailTime>,
        centre_position: [f64; 3],
        centre_velocity: [f64; 3],
    ) -> RailState {
        let time = time.into();
        let mut state = RailState {
            position: None,
            velocity: [0.0; 3],
            orientation: None,
            angular_velocity: [0.0; 3],
        };
        if let Some(orbit) = &self.orbit {
            let (offset, speed) = orbit.at(time);
            state.position = Some([0, 1, 2].map(|axis| centre_position[axis] + offset[axis]));
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

const HAS_SPIN: u8 = 1;
const HAS_ORBIT: u8 = 2;
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

/// The bytes of a rail (the payload of a ship file's `engine:rail`): a byte of flags (1 a spin, 2 an orbit); the spin's axis, period and angle; the orbit's centre
/// (a byte, 0 and a point or 1 and the number of a body), normal, radius, period and phase. Every number is the bits of an `f64`, little-endian.
pub fn encode_rail(rail: &Rail) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    let flags = if rail.spin.is_some() { HAS_SPIN } else { 0 }
        | if rail.orbit.is_some() { HAS_ORBIT } else { 0 };
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
        write_vector(&mut writer, orbit.normal);
        writer.write_u64(orbit.radius.to_bits());
        writer.write_u64(orbit.period.to_bits());
        writer.write_u64(orbit.phase.to_bits());
    }
    writer.into_bytes()
}

/// The rail of a payload. Refuses what [`Spin::new`] and [`Orbit::new`] refuse (the directions must be unit vectors already: what was written was read back as it
/// was, to the bit), a flag or a centre this program does not know, bytes that end too soon and bytes after the end.
pub fn decode_rail(payload: &[u8]) -> Result<Rail, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let flags = reader.read_u8()?;
    if flags & !(HAS_SPIN | HAS_ORBIT) != 0 {
        return Err(DecodeError::Invalid("a rail with a flag nobody knows"));
    }
    let spin = if flags & HAS_SPIN != 0 {
        let spin = Spin {
            axis: read_vector(&mut reader)?,
            period: read_number(&mut reader)?,
            angle: read_number(&mut reader)?,
        };
        if !is_unit(spin.axis) || !nonzero(spin.period) || !spin.angle.is_finite() {
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
        let orbit = Orbit {
            around,
            normal: read_vector(&mut reader)?,
            radius: read_number(&mut reader)?,
            period: read_number(&mut reader)?,
            phase: read_number(&mut reader)?,
        };
        let centre_is_good = match orbit.around {
            Centre::Point(point) => finite(&point),
            Centre::Grid(_) => true,
        };
        if !(centre_is_good
            && is_unit(orbit.normal)
            && orbit.radius > 0.0
            && orbit.radius.is_finite()
            && nonzero(orbit.period)
            && orbit.phase.is_finite())
        {
            return Err(DecodeError::Invalid("an orbit that is no orbit"));
        }
        Some(orbit)
    } else {
        None
    };
    reader.finish()?;
    Ok(Rail { spin, orbit })
}

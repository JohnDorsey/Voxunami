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

/// How far round a thing is that goes once round in `period` seconds, in radians, `seconds` after it was at the start: the whole turns are taken out first, so that a
/// world that has run for years loses nothing to the size of the number.
fn fraction_of_a_turn(seconds: f64, period: f64) -> f64 {
    let turns = seconds / period;
    TAU * (turns - turns.floor())
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

    /// The turn at `seconds` and how fast it turns.
    fn at(&self, seconds: f64) -> (Orientation, [f64; 3]) {
        let angle = self.angle + fraction_of_a_turn(seconds, self.period);
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

    /// Where the body is from the centre at `seconds`, and how fast it moves round it.
    fn at(&self, seconds: f64) -> ([f64; 3], [f64; 3]) {
        let angle = self.phase + fraction_of_a_turn(seconds, self.period);
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
    /// Where the body is, how fast it moves and how it is turned `seconds` into the life of the world. `centre_position` and `centre_velocity` are those of what
    /// the orbit goes round (the point, and still, for a point), which the caller knows; a rail with no orbit does not look at them.
    pub fn state(
        &self,
        seconds: f64,
        centre_position: [f64; 3],
        centre_velocity: [f64; 3],
    ) -> RailState {
        let mut state = RailState {
            position: None,
            velocity: [0.0; 3],
            orientation: None,
            angular_velocity: [0.0; 3],
        };
        if let Some(orbit) = &self.orbit {
            let (offset, speed) = orbit.at(seconds);
            state.position = Some([0, 1, 2].map(|axis| centre_position[axis] + offset[axis]));
            state.velocity = [0, 1, 2].map(|axis| centre_velocity[axis] + speed[axis]);
        }
        if let Some(spin) = &self.spin {
            let (orientation, angular_velocity) = spin.at(seconds);
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

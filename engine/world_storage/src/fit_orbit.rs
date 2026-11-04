//! An orbit from a state (`docs/orbits-and-thrust-plan.md`, step A4 and E10): the place and the velocity of a body against what it goes round, the mass parameter mu = G M of that
//! body, and the time, made into the [`Orbit`] that passes through them. The inverse of [`Orbit::at`] (what a rail says of a body at a time).
//!
//! It is what puts a free ship on a rail, puts it on a new one after a burn or a bump, and, when what it goes round weighs more or less (the owner's choice, 2026-11-02),
//! puts it on the orbit that its place and its speed are on with the new mu: the state does not change, the orbit does.
//!
//! The working is in wide numbers (`wide_kepler::Elements::from_state`), from the numbers of the state as they are (a double is exact in any width), so the orbit is the one the
//! state is on, to the last bit of the width of its elements, and not that orbit less a part in 10^16 of the sizes in the state. **No rule decides that an orbit is near enough
//! something else to be it**: a state that is exactly a circle is a circle, and one that is a hair off is an ellipse with the hair (the owner, 2026-11-03: orbits close enough to
//! circular may be simplified in the map interface, never in a rail or in a file). What a state cannot be is refused: a body on its centre, at rest or going straight at it or away
//! from it (no plane), at the speed of escape or faster, and an ellipse outside the limits of an orbit (a ratio under 2^-64, a radius over 2^120 voxels, a period over 2^80 seconds).
//! The turn of the orbit at the time is exact: the phase is the mean anomaly now less the turns gone since time 0, in whole numbers, as a rail works them out.

use wide_float::{Precision, WideFloat};
use wide_kepler::Elements;

use crate::rail::{Centre, ExactOffset, Orbit, RailError, RailTime};

/// The width of the elements of an orbit made from a state of doubles: the least a rail has (the state is exact in it, and the orbit is the one through the state to 128 bits).
const WIDTH_OF_A_STATE_OF_DOUBLES: Precision = Precision::MINIMUM;

/// A double as the exact number it is, or an error if it is not a number.
fn exactly(value: f64) -> Result<WideFloat, RailError> {
    WideFloat::from_f64(value, WIDTH_OF_A_STATE_OF_DOUBLES).map_err(|_| {
        RailError::Invalid(
            "a state is made of numbers, and what the body goes round weighs something",
        )
    })
}

fn exactly_three(values: [f64; 3]) -> Result<[WideFloat; 3], RailError> {
    Ok([
        exactly(values[0])?,
        exactly(values[1])?,
        exactly(values[2])?,
    ])
}

impl Orbit {
    /// The orbit round `around` that a body is on at `time`, if it is at `offset` from the centre (exact: whole bricks and a rest) and goes `velocity` (voxels a second, against
    /// the centre) and what it goes round has the mass parameter `mu` (G M, in voxels cubed a second squared). The orbit goes counter-clockwise looking down its normal, which
    /// is the direction of the angular momentum, so the period is above nothing: a body that goes the other way round the normal an orbit was made with comes back about the
    /// opposite normal. The elements are numbers of 128 bits (the phase has more: see [`Orbit::from_wide_state`]).
    ///
    /// Refused: a body that is on the centre, at rest or going straight at it or away from it (no plane), one that is bound to nothing (the speed of escape or more), one with a
    /// mass that is not above nothing, numbers that are not numbers, and an ellipse outside the limits of an orbit (see [`Orbit::from_elements`]).
    pub fn from_state(
        around: Centre,
        offset: ExactOffset,
        velocity: [f64; 3],
        mu: f64,
        time: RailTime,
    ) -> Result<Self, RailError> {
        // (A count of bricks has up to 117 bits, which the 32 voxels of a brick and the rest below it, in steps of 2^-32, make 154: 256 bits hold the sum exactly.)
        let precision = Precision::of(256);
        let mut place = Vec::with_capacity(3);
        for axis in 0..3 {
            let in_bricks = WideFloat::from_i128(offset.bricks[axis], precision)
                .times_power_of_two(5)
                .map_err(|_| RailError::Invalid("a count of bricks is a number"))?;
            place.push(&in_bricks + &exactly(offset.rest[axis])?.rounded_to(precision));
        }
        let place: [WideFloat; 3] = place.try_into().expect("three axes");
        Self::from_wide_state(
            around,
            &place,
            &exactly_three(velocity)?,
            &exactly(mu)?,
            time,
            WIDTH_OF_A_STATE_OF_DOUBLES,
        )
    }

    /// The same, for a place given as plain numbers of voxels from the centre: what a ship that is not on a rail has (its place is an `f64` offset in a brick, and the
    /// difference from the centre's is an `f64` of the size of the distance, which is as near as the working can use).
    pub fn from_place(
        around: Centre,
        place: [f64; 3],
        velocity: [f64; 3],
        mu: f64,
        time: RailTime,
    ) -> Result<Self, RailError> {
        Self::from_wide_state(
            around,
            &exactly_three(place)?,
            &exactly_three(velocity)?,
            &exactly(mu)?,
            time,
            WIDTH_OF_A_STATE_OF_DOUBLES,
        )
    }

    /// The same, for a place and a velocity of any width, and elements of `precision` bits (from 128): the orbit through the state, to the last bit of that width. This is the
    /// entry for a state that a rail or a burn worked out in wide numbers, so that a path planned to the millimetre is not rounded to doubles when a ship is put on it again.
    /// Every element has `precision` bits except the phase, which has more: near the nearest point of a thin ellipse the body moves by the ratio to the power of a half of a
    /// change of the phase and its speed by the ratio to the power of one and a half, and the phase is kept to as many more bits as that.
    pub fn from_wide_state(
        around: Centre,
        place: &[WideFloat; 3],
        velocity: &[WideFloat; 3],
        mu: &WideFloat,
        time: RailTime,
        precision: Precision,
    ) -> Result<Self, RailError> {
        let elements = Elements::from_state(place, velocity, mu, time.wide(), precision)?;
        Ok(Self::from_elements(around, elements)?.starting_at(time))
    }
}

//! An orbit from a state (`docs/orbits-and-thrust-plan.md`, step A4): the place and the velocity of a body against what it goes round, the mass parameter mu = G M of that body,
//! and the time, made into the [`Orbit`] that passes through them. The inverse of [`Orbit::at`] (what a rail says of a body at a time).
//!
//! It is what puts a free ship on a rail, puts it on a new one after a burn or a bump, and, when what it goes round weighs more or less (the owner's choice, 2026-11-02),
//! puts it on the orbit that its place and its speed are on with the new mu: the state does not change, the orbit does.
//!
//! The working is in `f64` (with `libm`), relative to what the body goes round, in the units of the state, so what is lost is a part in 10^16 of the sizes in it. The turn of the
//! orbit at the time is made exact again at the end: the phase is the mean anomaly now less the turns gone since time 0, in whole numbers, as a rail works them out.
//! What the state holds limits what comes out: the semi-major axis is the difference of two terms of the size of the state, so it is right to 2e-16 over the ratio, and an
//! orbit that was made from a state and is kept goes round a little out of time with the one it came from (each making costs that: an orbit that is not touched is kept
//! and not made again from its state). A state too near an escape, a straight fall, or the centre itself says too little to make an orbit of, and is refused.

use std::f64::consts::TAU;

use wide_kepler::equation_in_f64;

use crate::exact_circle;
use crate::rail::{Centre, ExactOffset, Orbit, RailError, RailTime, turn_fraction, unit};

/// How near to nothing the part of a state that is not along the line to the centre may be, in parts of the size of the place times the speed, before the state has no plane: the
/// sine of the angle between the place and the velocity, and about the noise of the cross product of two `f64` vectors.
const NO_PLANE: f64 = 1.0e-14;

/// How near an escape the energy may be, in parts of 2 / distance (one minus the square of the speed over the speed of escape), before the orbit is not an ellipse as far as the
/// state can say: the noise of the difference.
const NEAR_ESCAPE: f64 = 1.0e-13;

/// The eccentricity below which a state is on a circle as far as it can say: a few parts in 10^15 is the noise of the eccentricity vector.
const CIRCLE: f64 = 1.0e-14;

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

fn cross(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ]
}

fn length(vector: [f64; 3]) -> f64 {
    libm::sqrt(dot(vector, vector))
}

impl Orbit {
    /// The orbit round `around` that a body is on at `time`, if it is at `offset` from the centre and goes `velocity` (voxels a second, against the centre) and what it goes round
    /// has the mass parameter `mu` (G M, in voxels cubed a second squared). The orbit goes counter-clockwise looking down its normal, which is the direction of the
    /// angular momentum, so the period is above nothing: a body that goes the other way round the normal an orbit was made with comes back about the opposite normal.
    ///
    /// Refused: a body that is on the centre, at rest or going straight at it or away (no plane), one that is bound to nothing (the speed of escape or more, or so near it that
    /// the state cannot say which), one with a mass that is not above nothing, and numbers that are not numbers.
    pub fn from_state(
        around: Centre,
        offset: ExactOffset,
        velocity: [f64; 3],
        mu: f64,
        time: RailTime,
    ) -> Result<Self, RailError> {
        Self::from_place(around, offset.in_voxels(), velocity, mu, time)
    }

    /// The same, for a place given as plain numbers of voxels from the centre: what a ship that is not on a rail has (its place is an `f64` offset in a brick, and the
    /// difference from the centre's is an `f64` of the size of the distance, which is as near as the working below can use).
    pub fn from_place(
        around: Centre,
        place: [f64; 3],
        velocity: [f64; 3],
        mu: f64,
        time: RailTime,
    ) -> Result<Self, RailError> {
        if !(mu > 0.0 && mu.is_finite())
            || !place.iter().chain(&velocity).all(|part| part.is_finite())
        {
            return Err(RailError::Invalid(
                "a state is made of numbers, and what the body goes round weighs something",
            ));
        }
        let distance = length(place);
        let speed = length(velocity);
        if distance == 0.0 {
            return Err(RailError::Invalid("a body on its centre is on no orbit"));
        }
        let momentum = cross(place, velocity);
        let momentum_size = length(momentum);
        if momentum_size <= NO_PLANE * distance * speed || momentum_size == 0.0 {
            return Err(RailError::Invalid(
                "a body at rest, or going straight at its centre or away from it, has no plane to go round in",
            ));
        }
        // The energy: 1 / a = 2 / r - v^2 / mu, above nothing for an ellipse.
        let inverse_axis = 2.0 / distance - speed * speed / mu;
        if inverse_axis <= NEAR_ESCAPE * 2.0 / distance {
            return Err(RailError::Invalid(
                "a body that is going fast enough to get away is on no ellipse",
            ));
        }
        let axis = 1.0 / inverse_axis;
        // The shape. s = 1 - e^2 = p / a, with p = h^2 / mu the semi-latus rectum; the eccentricity vector points at the nearest point and is (v x h) / mu - r / |r|.
        let semi_latus_rectum = momentum_size * momentum_size / mu;
        let s = semi_latus_rectum * inverse_axis;
        let toward = cross(velocity, momentum).map(|part| part / mu);
        let eccentricity_vector = [0, 1, 2].map(|part| toward[part] - place[part] / distance);
        let eccentricity = length(eccentricity_vector);
        let is_circle = eccentricity < CIRCLE;
        // 1 - e from the vector when e is small, and as s / (1 + e) when it is near 1, where 1 - e is the difference of nearly equal numbers.
        let ratio = if is_circle {
            1.0
        } else if eccentricity < 0.5 {
            1.0 - eccentricity
        } else {
            s / (1.0 + eccentricity)
        };
        // The plane, as the orbit will make it from its normal (so the same two directions), and where the body is in it.
        let normal = unit(momentum, "the angular momentum of a body")?;
        let (first, second) = exact_circle::plane(normal);
        let first = first.map(exact_circle::float_of);
        let second = second.map(exact_circle::float_of);
        let (periapsis, mean_anomaly) = if is_circle {
            // A circle starts toward the first direction: the mean anomaly is the angle of the place from it.
            (0.0, libm::atan2(dot(place, second), dot(place, first)))
        } else {
            let periapsis = libm::atan2(
                dot(eccentricity_vector, second),
                dot(eccentricity_vector, first),
            );
            let (sine, cosine) = (libm::sin(periapsis), libm::cos(periapsis));
            // The directions of the orbit: toward the nearest point, and a quarter turn on.
            let toward_periapsis = [0, 1, 2].map(|part| cosine * first[part] + sine * second[part]);
            let beyond = [0, 1, 2].map(|part| cosine * second[part] - sine * first[part]);
            let true_anomaly = libm::atan2(dot(place, beyond), dot(place, toward_periapsis));
            // tan(E / 2) = sqrt((1 - e) / (1 + e)) tan(v / 2), kept from dividing by nothing as the two parts of an arctangent.
            let half = true_anomaly / 2.0;
            let eccentric = 2.0
                * libm::atan2(
                    libm::sqrt(ratio) * libm::sin(half),
                    libm::sqrt(2.0 - ratio) * libm::cos(half),
                );
            // M = E - e sin E, written without the cancellation near E = 0 (the same working as the solver's first guess), for the sign of E.
            let (value, _) = equation_in_f64(eccentric.abs(), ratio, 1.0 - ratio);
            (periapsis, value.copysign(eccentric))
        };
        let period = TAU * libm::sqrt(axis * axis * axis / mu);
        // The phase is the mean anomaly now less the turns gone since time 0, which are worked out in whole numbers for this period.
        let (remainder, whole_turn) = turn_fraction(time, period);
        let gone = exact_circle::fraction_in_turns(remainder, whole_turn);
        let phase_turns = exact_circle::turns_of_radians(mean_anomaly).wrapping_sub(gone);
        let phase = TAU * (phase_turns as f64 / TURN);
        Self::elliptical(around, normal, axis, ratio, periapsis, period, phase)
    }
}

/// 2^128, the turns in a turn, as an `f64`.
const TURN: f64 = (1u128 << 127) as f64 * 2.0;

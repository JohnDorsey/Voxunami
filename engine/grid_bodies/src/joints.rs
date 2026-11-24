//! Joints between ships: when two ships with joint ends (see `engine:joint_face`) pull on each other, how, and when they are coupled
//! (`docs/decisions/joints-and-cars.md`, "Coupling, lock, unlock").
//!
//! This module is the rules, pure and tested alone (`tests/joints.rs`): the geometry is that of [`crate::docking`] (a joint end is a [`BlockFaceView`], as a port is), and
//! the plugin (`plugin.rs`) finds the joint ends of the ships it moves, puts these forces on their bodies and makes the couplings.
//!
//! All of it is in metres, kilograms and seconds, the world's axes and `f64`; the one transcendental function (the cosine of the angle) is `libm`'s, so the result is the same
//! on every machine.

use crate::docking::{Approach, BlockFaceView, cross, dot, length, turn};

/// How ships with joint ends are drawn together and coupled. The numbers of the owner's choice are marked; the pull itself (how it slows the ends as they come) is mine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JointSettings {
    /// How far apart, in metres, the middles of two ends' faces may be and still pull on each other (as far as for docking).
    pub range: f64,
    /// The most force, in newtons, on each of two ends that pull on each other (the owner, 2026-11-01: 10 kN each, as for docking, whatever the ships weigh).
    pub pull_force: f64,
    /// How hard the pull may slow the ends as they come, in metres a second a second: they are asked to come no faster than they can be stopped by this, or by half of what
    /// `pull_force` does to the pair, if that is less (a pull of one force and nothing to slow them would have them past each other at the speed they arrive at, which is
    /// faster than a coupling allows: found while building the coupling, `decisions/joints-and-cars.md`).
    pub approach_acceleration: f64,
    /// How quickly, in seconds, the pull takes the difference between the speed asked of the ends and the speed they have out of it. The speed they have lags the one asked
    /// by `approach_acceleration` times this (0.2 metres a second, at the defaults), which is what they still have when they arrive; half a second made it a metre a second,
    /// and light ends went past each other (found by the tests of the rule, `tests/joints.rs`). A tick is a twentieth of a second, so this must not be less than that
    /// (the lag is steady for as long as the tick is no more than this).
    pub response_seconds: f64,
    /// Ends this near, in metres, are coupled if they are also in line and slow enough (owner, 2026-11-01: 0.3).
    pub couple_distance: f64,
    /// How far from looking exactly at each other the two faces may be, in degrees, for a coupling (owner, 2026-11-01: 45).
    pub couple_degrees: f64,
    /// How fast the two ends may go past each other at a coupling, in metres a second (owner, 2026-11-09: 1), and how fast the two cars may turn against each other, in
    /// radians a second (owner, 2026-11-09: 0.2): a joint made with more begins with a collision, which loses energy and leaves a gap.
    pub max_relative_speed: f64,
    pub max_relative_spin: f64,
    /// Ends this near, in metres, are coupled whatever the spin of the cars against each other (owner, 2026-11-09: a centimetre). A joint is derived from the blocks and
    /// places (only the pair is saved), so a swinging group that is loaded has to be coupled again, and a ball turns however it is: only the speed of the pivots matters then.
    pub spin_waived_within: f64,
    /// The most torque, in newton metres, that slows the cars' spin against each other as they are pulled (the same as docking's twist torque). The pull is at the ends,
    /// as docking's is (owner, 2026-11-09), which swings cars whose ends are away from their centres; this takes away the spin that leaves, which a coupling would not allow.
    /// It never turns the cars toward line, it only slows their turning against each other.
    pub damping_torque_limit: f64,
    /// The extra substeps of the solver for a car that is coupled (owner, 2026-11-09: yes; four is my number, `loop-probe.md`, section 14).
    pub extra_substeps: usize,
}

impl Default for JointSettings {
    fn default() -> Self {
        Self {
            range: 6.0,
            pull_force: 10_000.0,
            approach_acceleration: 2.0,
            response_seconds: 0.1,
            couple_distance: 0.3,
            couple_degrees: 45.0,
            max_relative_speed: 1.0,
            max_relative_spin: 0.2,
            spin_waived_within: 0.01,
            damping_torque_limit: 10_000.0,
            extra_substeps: 4,
        }
    }
}

const DEGREES: f64 = std::f64::consts::PI / 180.0;

impl JointSettings {
    /// Whether two ends can pull on each other: their faces are near enough, each looks toward the other (a little behind it is allowed, by the coupling distance, so that ends
    /// that have come together still do), and they look against each other.
    pub fn pulls(
        &self,
        first: &BlockFaceView,
        second: &BlockFaceView,
        approach: &Approach,
    ) -> bool {
        approach.distance <= self.range
            && dot(first.normal, approach.gap) >= -self.couple_distance
            && dot(second.normal, approach.gap) <= self.couple_distance
            && dot(first.normal, second.normal) < 0.0
    }

    /// Whether two ends are near enough, in line enough and slow enough against each other to be coupled. `relative_velocity` is the velocity of the second end's pivot against
    /// the first's, `relative_spin` the angular velocity of the second car against the first's.
    pub fn can_couple(
        &self,
        first: &BlockFaceView,
        second: &BlockFaceView,
        approach: &Approach,
        relative_velocity: [f64; 3],
        relative_spin: [f64; 3],
    ) -> bool {
        approach.distance <= self.couple_distance
            && dot(first.normal, second.normal) <= -libm::cos(self.couple_degrees * DEGREES)
            && length(relative_velocity) <= self.max_relative_speed
            && (length(relative_spin) <= self.max_relative_spin
                || approach.distance <= self.spin_waived_within)
    }

    /// Whether two ends are at one place, to a centimetre ([`JointSettings::spin_waived_within`]): the ends of a pair that the world has as coupled (a saved world's, or one
    /// whose ships were in files) are coupled again if they are, whatever the line-up of their faces and the speed and spin of the cars against each other. A ball joint lets
    /// the cars fold past the line-up that a new coupling is asked for, and the speed of ends that are together is the speed of the pivot, which the joint keeps.
    pub fn together_again(&self, approach: &Approach) -> bool {
        approach.distance <= self.spin_waived_within
    }

    /// The way the pull on the second end points: toward the first end's speed asked of them, whatever the mass (the mass only scales it). `None` where there is no pull.
    pub fn pull_direction(&self, gap: [f64; 3], relative_velocity: [f64; 3]) -> Option<[f64; 3]> {
        let pull = self.pull_on_second(gap, relative_velocity, 1.0);
        let size = length(pull);
        (size.is_finite() && size > 0.0).then(|| pull.map(|part| part / size))
    }

    /// The force on the second end of a pair that pull on each other (the first has the opposite one): toward the first end, at most `pull_force`, and such that the pair
    /// arrives slowly. `gap` goes from the first end's pivot to the second's, `relative_velocity` is the second's against the first's, and `effective_mass` is that of the pair
    /// (see [`effective_mass`]).
    ///
    /// The speed asked of the pair is the greatest they can be stopped from in the distance that is left, with the deceleration of `approach_acceleration` (less, for a pair
    /// the force would not manage): `sqrt(2 a d)`, toward each other. The force is the mass times the difference from that speed over `response_seconds`, whatever way
    /// the difference points (so it also takes the sideways speed off, and the speed of ends that are at one place), cut to `pull_force`. A force nobody can ask of a mass that is
    /// nothing, or not a number, is none.
    pub fn pull_on_second(
        &self,
        gap: [f64; 3],
        relative_velocity: [f64; 3],
        effective_mass: f64,
    ) -> [f64; 3] {
        if !(effective_mass.is_finite() && effective_mass > 0.0) {
            return [0.0; 3];
        }
        let distance = length(gap);
        let toward_first = if distance > 1e-12 {
            gap.map(|component| -component / distance)
        } else {
            [0.0; 3]
        };
        let braking = (0.5 * self.pull_force / effective_mass).min(self.approach_acceleration);
        let wanted_speed = (2.0 * braking * distance).sqrt();
        let force = [0, 1, 2].map(|axis| {
            let error = toward_first[axis] * wanted_speed - relative_velocity[axis];
            effective_mass * error / self.response_seconds
        });
        let size = length(force);
        if size > self.pull_force {
            force.map(|component| component * self.pull_force / size)
        } else {
            force
        }
    }
}

impl JointSettings {
    /// The torque on the second car that slows its spin against the first (the first has the opposite): the inertia of the pair (see [`effective_inertia`]) times the spin over the
    /// response, against it, cut to `damping_torque_limit`. **A tensor, not a number**: a car that is a long rod turns about its length with a fraction of the inertia it has about the
    /// others, and a torque sized by a number (a third of the trace, as this was) takes the spin about that axis through zero to many times its size every tick (measured 2026-11-19:
    /// the cars' spin against each other alternated 0.5 to 0.8 radians a second in sign every tick on a chain of three heavy cars, and the pair hovered at 0.02 metres for a second
    /// before it was coupled). The tensor gives each axis its own response: a free pair's spin against each other is cut by `tick / response_seconds` every tick, whatever the shape.
    /// None for an inertia that is nothing or not a number.
    pub fn spin_damping_on_second(
        &self,
        relative_spin: [f64; 3],
        effective_inertia: [[f64; 3]; 3],
    ) -> [f64; 3] {
        if !effective_inertia
            .iter()
            .flatten()
            .all(|part| part.is_finite())
        {
            return [0.0; 3];
        }
        let torque = turn(effective_inertia, relative_spin)
            .map(|component| -component / self.response_seconds);
        let size = length(torque);
        if size > self.damping_torque_limit {
            torque.map(|component| component * self.damping_torque_limit / size)
        } else {
            torque
        }
    }
}

impl JointSettings {
    /// The torque on the second car that slows the spin of the two against each other, from the inertia of each in the world's axes (`None` for a car that cannot turn, as for
    /// [`effective_inertia`]): the pair's inertia, and then [`JointSettings::spin_damping_on_second`] against it. No torque where there is no inertia to slow the spin against.
    pub fn spin_damping_between(
        &self,
        relative_spin: [f64; 3],
        first: Option<Inertia>,
        second: Option<Inertia>,
    ) -> [f64; 3] {
        effective_inertia(first, second).map_or([0.0; 3], |inertia| {
            self.spin_damping_on_second(relative_spin, inertia)
        })
    }
}

/// An inertia tensor, in kilograms metres metres, about a centre of mass.
pub type Inertia = [[f64; 3]; 3];

/// The inertia tensor of a body that has `body_inertia` in its own axes, in the axes that `rotation` turns them to (`rotation[row][column]`, as `GridPose::rotation_matrix` has it):
/// `R I R^T`.
pub fn inertia_in_the_world(rotation: [[f64; 3]; 3], body_inertia: Inertia) -> Inertia {
    let turned =
        [0, 1, 2].map(|row| [0, 1, 2].map(|column| dot(rotation[row], body_inertia[column])));
    // (An inertia tensor is symmetric, so the columns of `I` are its rows: `turned` is `R I`.)
    [0, 1, 2].map(|row| [0, 1, 2].map(|column| dot(turned[row], rotation[column])))
}

/// How much faster than its mass alone says a point at `lever` from the centre of mass of a body of `inertia` goes for a force on it: a force `F` there is a torque `lever x F`, which
/// turns the body and adds `(lever x F) I^-1 (lever x F)` to what the point answers (`1/m` is the rest), so `torque_direction` is `lever x F` over `|F|`. `None` for an inertia that
/// is not usable.
pub fn inverse_inertia_along(inertia: Inertia, torque_direction: [f64; 3]) -> Option<f64> {
    let inverse = invert(inertia)?;
    Some(dot(torque_direction, turn(inverse, torque_direction)))
}

/// What the end of a body answers a force along `direction` with, as a mass: the body's own mass and what its lever adds (a force at an end that is away from the centre of mass
/// turns the body, and the end goes faster than the mass alone says: `1/m + (arm x direction) I^-1 (arm x direction)`, `arm` being from the centre of mass to the end and `inertia`
/// in the world's axes). The pull asks for a force of its mass times the error in the speed of the ends over `response_seconds`, and a car whose end is a metre or two from its
/// centre answers it several times harder than its mass says, so the speed of the ends overshoots every tick and does not settle (measured 2026-11-19 on a chain's end car); the
/// end's own mass is the one that makes it settle. `None` if there is no such mass.
pub fn mass_at_an_end(
    mass: f64,
    inertia: Inertia,
    arm: [f64; 3],
    direction: [f64; 3],
) -> Option<f64> {
    let inverse = 1.0 / mass + inverse_inertia_along(inertia, cross(arm, direction))?;
    (inverse.is_finite() && inverse > 0.0).then(|| 1.0 / inverse)
}

/// The inertia a pair is turned as, as [`effective_mass`] is the mass a pair is pulled as: for two bodies that can turn, `(I1^-1 + I2^-1)^-1` (a torque that is the same on both and
/// opposite changes the spin of one against the other as one body of this inertia would), and the tensor of the one that can if the other cannot (a ship that is held). Each is
/// `Some(tensor)` if it can turn and `None` if it cannot; `None` also if neither can, or a tensor is not usable (not a number, or not positive definite).
pub fn effective_inertia(first: Option<Inertia>, second: Option<Inertia>) -> Option<Inertia> {
    let usable = |tensor: Inertia| {
        invert(tensor).is_some()
            && tensor.iter().flatten().all(|part| part.is_finite())
            && (0..3).all(|axis| tensor[axis][axis] > 0.0)
    };
    match (first, second) {
        (Some(first), Some(second)) if usable(first) && usable(second) => {
            let (first_inverse, second_inverse) = (invert(first)?, invert(second)?);
            let sum = [0, 1, 2].map(|row| {
                [0, 1, 2].map(|column| first_inverse[row][column] + second_inverse[row][column])
            });
            invert(sum)
        }
        (Some(tensor), None) | (None, Some(tensor)) if usable(tensor) => Some(tensor),
        _ => None,
    }
}

/// The inverse of a 3 by 3 matrix, by its cofactors; `None` if it has no inverse with a positive determinant (an inertia tensor has one).
fn invert(m: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let cofactor =
        |r0: usize, r1: usize, c0: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
    let c = [
        [
            cofactor(1, 2, 1, 2),
            -cofactor(1, 2, 0, 2),
            cofactor(1, 2, 0, 1),
        ],
        [
            -cofactor(0, 2, 1, 2),
            cofactor(0, 2, 0, 2),
            -cofactor(0, 2, 0, 1),
        ],
        [
            cofactor(0, 1, 1, 2),
            -cofactor(0, 1, 0, 2),
            cofactor(0, 1, 0, 1),
        ],
    ];
    let determinant = m[0][0] * c[0][0] + m[0][1] * c[0][1] + m[0][2] * c[0][2];
    if !(determinant > 0.0 && determinant.is_finite()) {
        return None;
    }
    Some([0, 1, 2].map(|row| [0, 1, 2].map(|column| c[column][row] / determinant)))
}

/// The mass a pair of ends is pulled as: the reduced mass of two ships that can move (the force is the same on both and the one that weighs less moves more, and their
/// distance closes as one body of this mass would), and the mass of the one that can move if the other cannot (a ship that is held). Each is `Some(mass)` if it can move
/// and `None` if it cannot; `None` also if neither can, or a mass is nothing or not a number.
pub fn effective_mass(first: Option<f64>, second: Option<f64>) -> Option<f64> {
    let usable = |mass: f64| mass.is_finite() && mass > 0.0;
    match (first, second) {
        (Some(first), Some(second)) if usable(first) && usable(second) => {
            Some(first * second / (first + second))
        }
        (Some(mass), None) | (None, Some(mass)) if usable(mass) => Some(mass),
        _ => None,
    }
}

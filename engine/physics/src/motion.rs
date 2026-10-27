//! Making one body of several: the mass and the motion the whole has when bodies are joined, with nothing lost.
//!
//! Joining is perfectly inelastic: the parts stop moving relative to each other, and the whole carries exactly the linear momentum and the angular
//! momentum the parts had. Everything is `f64`, summed in the order given, in the world's axes, so the result is the same bit for bit on every machine.

use crate::BodyMass;

/// What a rigid body's motion is, for joining it to others. All in the world's axes, and positions in any one frame the caller picks (the same
/// for every body joined).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyMotion {
    pub mass: f64,
    /// Where the centre of mass is.
    pub centre: [f64; 3],
    /// How fast the centre of mass moves.
    pub linear_velocity: [f64; 3],
    /// How fast the body turns (an axis scaled by radians a second).
    pub angular_velocity: [f64; 3],
    /// The tensor of inertia about the centre of mass, in the world's axes.
    pub inertia: [[f64; 3]; 3],
}

impl BodyMotion {
    /// A body of this mass (see [`BodyMass`], whose centre and tensor are in the body's own axes) that is turned by this matrix (the body's axes
    /// in the world's: `rotation[row][column]`), with its centre of mass at `centre` and moving as given.
    pub fn of_body(
        mass: &BodyMass,
        rotation: [[f64; 3]; 3],
        centre: [f64; 3],
        linear_velocity: [f64; 3],
        angular_velocity: [f64; 3],
    ) -> Self {
        let local = mass.inertia.map(|row| row.map(f64::from));
        // R I R^T
        let mut turned = [[0.0; 3]; 3];
        for (row, turned_row) in turned.iter_mut().enumerate() {
            for (column, entry) in turned_row.iter_mut().enumerate() {
                for first in 0..3 {
                    for second in 0..3 {
                        *entry +=
                            rotation[row][first] * local[first][second] * rotation[column][second];
                    }
                }
            }
        }
        Self {
            mass: f64::from(mass.mass),
            centre,
            linear_velocity,
            angular_velocity,
            inertia: turned,
        }
    }

    /// The linear momentum.
    pub fn linear_momentum(&self) -> [f64; 3] {
        self.linear_velocity.map(|speed| speed * self.mass)
    }

    /// The angular momentum about a point: what the body's own turning carries, and what its centre's motion carries about the point.
    pub fn angular_momentum_about(&self, point: [f64; 3]) -> [f64; 3] {
        let spin = multiply(self.inertia, self.angular_velocity);
        let arm = [0, 1, 2].map(|axis| self.centre[axis] - point[axis]);
        let orbit = cross(arm, self.linear_velocity);
        [0, 1, 2].map(|axis| spin[axis] + self.mass * orbit[axis])
    }

    /// The one body that several joined are: the mass is the sum, the centre is the mass-weighted mean, the linear and the angular momentum
    /// (about the new centre) are the sums, and the inertia is the sum about the new centre (the parallel axis theorem). `None` if there is no
    /// mass to join, or the whole has no inertia to turn with (not a body).
    pub fn join(parts: &[Self]) -> Option<Self> {
        let mass: f64 = parts.iter().map(|part| part.mass).sum();
        if mass <= 0.0 || mass.is_nan() {
            return None;
        }
        let centre = [0, 1, 2].map(|axis| {
            parts
                .iter()
                .map(|part| part.mass * part.centre[axis])
                .sum::<f64>()
                / mass
        });
        let momentum = [0, 1, 2].map(|axis| {
            parts
                .iter()
                .map(|part| part.linear_momentum()[axis])
                .sum::<f64>()
        });
        let angular_momentum = [0, 1, 2].map(|axis| {
            parts
                .iter()
                .map(|part| part.angular_momentum_about(centre)[axis])
                .sum::<f64>()
        });
        let mut inertia = [[0.0; 3]; 3];
        for part in parts {
            let arm = [0, 1, 2].map(|axis| part.centre[axis] - centre[axis]);
            let square: f64 = arm.iter().map(|component| component * component).sum();
            for (row, inertia_row) in inertia.iter_mut().enumerate() {
                for (column, entry) in inertia_row.iter_mut().enumerate() {
                    let identity = if row == column { square } else { 0.0 };
                    *entry +=
                        part.inertia[row][column] + part.mass * (identity - arm[row] * arm[column]);
                }
            }
        }
        let angular_velocity = multiply(invert(inertia)?, angular_momentum);
        Some(Self {
            mass,
            centre,
            linear_velocity: momentum.map(|component| component / mass),
            angular_velocity,
            inertia,
        })
    }
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn multiply(matrix: [[f64; 3]; 3], vector: [f64; 3]) -> [f64; 3] {
    matrix.map(|row| row[0] * vector[0] + row[1] * vector[1] + row[2] * vector[2])
}

/// The inverse of a 3 by 3 matrix, by its cofactors; `None` if it has no inverse.
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
    if determinant <= 0.0 || determinant.is_nan() {
        return None;
    }
    // The inverse is the transpose of the cofactors over the determinant.
    Some([0, 1, 2].map(|row| [0, 1, 2].map(|column| c[column][row] / determinant)))
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)] // the matrix indices are the mathematics
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::MassMoments;

    const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    /// A body of unit cubes at these voxels, each of this mass, upright, with its centre where it is, moving as given.
    fn body(voxels: &[[i128; 3]], mass: f64, linear: [f64; 3], angular: [f64; 3]) -> BodyMotion {
        let properties = MassMoments::sum(
            voxels
                .iter()
                .map(|&voxel| MassMoments::of_voxel(voxel, mass)),
        )
        .properties()
        .unwrap();
        BodyMotion::of_body(
            &properties,
            IDENTITY,
            properties.centre_of_mass.map(f64::from),
            linear,
            angular,
        )
    }

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|axis| (a[axis] - b[axis]).abs() <= 1e-4 * (1.0 + b[axis].abs()))
    }

    #[test]
    fn one_body_joined_with_nothing_is_itself() {
        let alone = body(
            &[[0, 0, 0], [1, 0, 0]],
            3.0,
            [1.0, 2.0, 3.0],
            [0.1, 0.2, 0.3],
        );
        let joined = BodyMotion::join(&[alone]).unwrap();
        assert!(close(joined.centre, alone.centre));
        assert!(close(joined.linear_velocity, alone.linear_velocity));
        assert!(close(joined.angular_velocity, alone.angular_velocity));
        assert!((joined.mass - alone.mass).abs() < 1e-9);
    }

    #[test]
    fn two_equal_bodies_that_meet_head_on_stop_and_a_light_one_that_meets_a_heavy_one_is_carried_with_it()
     {
        let left = body(&[[0, 0, 0]], 5.0, [4.0, 0.0, 0.0], [0.0; 3]);
        let right = body(&[[3, 0, 0]], 5.0, [-4.0, 0.0, 0.0], [0.0; 3]);
        let stopped = BodyMotion::join(&[left, right]).unwrap();
        assert!(close(stopped.linear_velocity, [0.0; 3]), "{stopped:?}");
        assert!(close(stopped.angular_velocity, [0.0; 3]));
        // A body of 1 at 10 meets one of 9 at rest: the whole goes at 1.
        let light = body(&[[0, 0, 0]], 1.0, [10.0, 0.0, 0.0], [0.0; 3]);
        let heavy = body(&[[1, 0, 0]], 9.0, [0.0; 3], [0.0; 3]);
        let whole = BodyMotion::join(&[light, heavy]).unwrap();
        assert!(close(whole.linear_velocity, [1.0, 0.0, 0.0]), "{whole:?}");
        assert!((whole.mass - 10.0).abs() < 1e-9);
        assert!(close(whole.centre, [1.4, 0.5, 0.5]), "{:?}", whole.centre);
    }

    #[test]
    fn bodies_that_move_as_parts_of_one_turning_rigid_body_join_into_that_body_turning_as_it_did() {
        // A rigid body turning at w about a point p and going at v0 there: every part has the turn w and moves at v0 + w x (c - p).
        let (w, v0, p) = ([0.3, -0.7, 0.5], [2.0, -1.0, 0.5], [1.0, 4.0, -2.0]);
        let part = |voxels: &[[i128; 3]], mass: f64| {
            let unmoving = body(voxels, mass, [0.0; 3], w);
            let arm = [0, 1, 2].map(|axis| unmoving.centre[axis] - p[axis]);
            let carried = cross(w, arm);
            BodyMotion {
                linear_velocity: [0, 1, 2].map(|axis| v0[axis] + carried[axis]),
                ..unmoving
            }
        };
        let first = part(&[[0, 0, 0], [1, 0, 0], [2, 0, 0]], 2.0);
        let second = part(&[[3, 0, 0], [3, 1, 0]], 5.0);
        let third = part(&[[0, 3, 2]], 1.0);
        let whole = BodyMotion::join(&[first, second, third]).unwrap();
        assert!(
            close(whole.angular_velocity, w),
            "{:?}",
            whole.angular_velocity
        );
        let arm = [0, 1, 2].map(|axis| whole.centre[axis] - p[axis]);
        let carried = cross(w, arm);
        assert!(close(
            whole.linear_velocity,
            [0, 1, 2].map(|axis| v0[axis] + carried[axis])
        ));
    }

    proptest! {
        #[test]
        fn joining_loses_no_linear_and_no_angular_momentum_about_any_point(
            first in proptest::collection::vec([-4i128..4, -4i128..4, -4i128..4], 1..8),
            second in proptest::collection::vec([6i128..12, -4i128..4, -4i128..4], 1..8),
            speeds in proptest::array::uniform12(-5.0f64..5.0),
            point in proptest::array::uniform3(-20.0f64..20.0),
        ) {
            let one = body(&first, 2.0, [speeds[0], speeds[1], speeds[2]], [speeds[3], speeds[4], speeds[5]]);
            let two = body(&second, 3.0, [speeds[6], speeds[7], speeds[8]], [speeds[9], speeds[10], speeds[11]]);
            let whole = BodyMotion::join(&[one, two]).unwrap();
            let before = [0, 1, 2].map(|axis| one.linear_momentum()[axis] + two.linear_momentum()[axis]);
            prop_assert!(close(whole.linear_momentum(), before));
            let before = [0, 1, 2].map(|axis| {
                one.angular_momentum_about(point)[axis] + two.angular_momentum_about(point)[axis]
            });
            let after = whole.angular_momentum_about(point);
            prop_assert!(close(after, before), "{after:?} against {before:?}");
        }
    }

    #[test]
    fn a_body_turned_by_a_matrix_has_its_inertia_in_the_worlds_axes() {
        // A bar along its own x, turned a quarter about z so that it lies along the world's y: it turns easily about y and not about x or z.
        let bar = MassMoments::sum((0..4).map(|x| MassMoments::of_voxel([x, 0, 0], 2.0)))
            .properties()
            .unwrap();
        let quarter_about_z = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        let motion = BodyMotion::of_body(&bar, quarter_about_z, [0.0; 3], [0.0; 3], [0.0; 3]);
        let local = bar.inertia.map(|row| row.map(f64::from));
        assert!((motion.inertia[1][1] - local[0][0]).abs() < 1e-9);
        assert!((motion.inertia[0][0] - local[1][1]).abs() < 1e-9);
        assert!((motion.inertia[2][2] - local[2][2]).abs() < 1e-9);
    }

    #[test]
    fn nothing_to_join_or_no_mass_is_not_a_body() {
        assert_eq!(BodyMotion::join(&[]), None);
        let mut none = body(&[[0, 0, 0]], 1.0, [0.0; 3], [0.0; 3]);
        none.mass = 0.0;
        assert_eq!(BodyMotion::join(&[none]), None);
    }
}

//! The mass of a body made of voxels: from the mass of each voxel to the total, the centre and the tensor of inertia.
//!
//! Rapier can work out the mass of a collider of one density, but a ship is made of blocks of different densities (`engine:density`), so
//! the mass is summed here and handed to Rapier as it is. Everything is `f64` summed in an order the caller fixes, so the result is the
//! same bit for bit on every machine.

/// What is needed of the voxels of a body to know how it moves: their total mass, the sum of each mass times where it is, and the sum
/// of each mass times the square of where it is (as a matrix, the six places of it that differ). Two of these add, so each brick of a
/// ship can keep its own and the ship has their sum.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MassMoments {
    mass: f64,
    /// The sum of `mass * position`.
    first: [f64; 3],
    /// The sum of `mass * position[i] * position[j]` for `xx, yy, zz, xy, xz, yz`.
    second: [f64; 6],
}

impl MassMoments {
    pub const NOTHING: Self = Self {
        mass: 0.0,
        first: [0.0; 3],
        second: [0.0; 6],
    };

    /// The moments of a block of this mass that fills the voxel with this lowest corner: a unit cube whose mass is spread evenly, so it is
    /// the mass at its middle and the spread of a cube about it.
    pub fn of_voxel(voxel: [i64; 3], mass: f64) -> Self {
        let centre = voxel.map(|corner| corner as f64 + 0.5);
        // A unit cube has a twelfth of its mass as its second moment about its own middle on each axis, and nothing across them.
        let spread = mass / 12.0;
        let mut moments = Self {
            mass,
            first: centre.map(|axis| mass * axis),
            second: [0.0; 6],
        };
        let pairs = [(0, 0), (1, 1), (2, 2), (0, 1), (0, 2), (1, 2)];
        for (slot, (i, j)) in pairs.into_iter().enumerate() {
            moments.second[slot] = mass * centre[i] * centre[j] + if i == j { spread } else { 0.0 };
        }
        moments
    }

    pub fn mass(&self) -> f64 {
        self.mass
    }

    /// The sum of these, in the order given.
    pub fn sum(parts: impl IntoIterator<Item = Self>) -> Self {
        let mut total = Self::NOTHING;
        for part in parts {
            total.mass += part.mass;
            for axis in 0..3 {
                total.first[axis] += part.first[axis];
            }
            for slot in 0..6 {
                total.second[slot] += part.second[slot];
            }
        }
        total
    }

    /// The mass, the centre of mass and the tensor of inertia of everything summed: `None` if there is no mass.
    pub fn properties(&self) -> Option<BodyMass> {
        if self.mass <= 0.0 || self.mass.is_nan() {
            return None;
        }
        let centre = self.first.map(|axis| axis / self.mass);
        // The second moment about the centre of mass: the sum, less the mass times the square of the centre.
        let about =
            |slot: usize, i: usize, j: usize| self.second[slot] - self.mass * centre[i] * centre[j];
        let (xx, yy, zz) = (about(0, 0, 0), about(1, 1, 1), about(2, 2, 2));
        let (xy, xz, yz) = (about(3, 0, 1), about(4, 0, 2), about(5, 1, 2));
        // The tensor of inertia is the trace of the second moment times the identity, less the second moment.
        let trace = xx + yy + zz;
        let tensor = [
            [trace - xx, -xy, -xz],
            [-xy, trace - yy, -yz],
            [-xz, -yz, trace - zz],
        ];
        Some(BodyMass {
            mass: self.mass as f32,
            centre_of_mass: centre.map(|axis| axis as f32),
            inertia: tensor.map(|row| row.map(|entry| entry as f32)),
        })
    }
}

/// What a body's mass is: the total, where the centre is (in the body's own coordinates), and the tensor of inertia about the centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyMass {
    pub mass: f32,
    pub centre_of_mass: [f32; 3],
    /// `inertia[row][column]`, symmetric.
    pub inertia: [[f32; 3]; 3],
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)] // the matrix indices are the mathematics
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn bar(length: i64, mass: f64) -> MassMoments {
        MassMoments::sum((0..length).map(|x| MassMoments::of_voxel([x, 0, 0], mass)))
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1e-4 * (1.0 + b.abs())
    }

    #[test]
    fn nothing_has_no_mass_properties() {
        assert_eq!(MassMoments::NOTHING.properties(), None);
        assert_eq!(MassMoments::sum([]).properties(), None);
        assert_eq!(MassMoments::of_voxel([0, 0, 0], 0.0).properties(), None);
        assert_eq!(
            MassMoments::of_voxel([0, 0, 0], f64::NAN).properties(),
            None
        );
    }

    #[test]
    fn a_single_cube_has_its_mass_at_its_middle_and_the_inertia_of_a_cube() {
        let properties = MassMoments::of_voxel([3, -2, 7], 6.0).properties().unwrap();
        assert_eq!(properties.mass, 6.0);
        assert_eq!(properties.centre_of_mass, [3.5, -1.5, 7.5]);
        // A cube of mass m and edge 1 has m * (1 + 1) / 12 about every axis through its middle, and no products of inertia.
        for row in 0..3 {
            for column in 0..3 {
                let expected = if row == column { 1.0 } else { 0.0 };
                assert!(
                    close(properties.inertia[row][column], expected),
                    "{:?}",
                    properties.inertia
                );
            }
        }
    }

    #[test]
    fn a_bar_of_cubes_turns_more_easily_about_its_length_than_end_over_end() {
        // Four unit cubes of mass 2 in a row along x: 8 in all, the centre at x = 2.
        let properties = bar(4, 2.0).properties().unwrap();
        assert_eq!(properties.mass, 8.0);
        assert_eq!(properties.centre_of_mass, [2.0, 0.5, 0.5]);
        // About its length a solid box of 4 by 1 by 1 and mass 8 has m (1 + 1) / 12; about the other two m (4^2 + 1^2) / 12.
        let tensor = properties.inertia;
        assert!(close(tensor[0][0], 8.0 * 2.0 / 12.0), "{tensor:?}");
        assert!(close(tensor[1][1], 8.0 * 17.0 / 12.0), "{tensor:?}");
        assert!(close(tensor[2][2], 8.0 * 17.0 / 12.0), "{tensor:?}");
        // A symmetric body has no products of inertia.
        for (row, column) in [(0, 1), (0, 2), (1, 2)] {
            assert!(tensor[row][column].abs() < 1e-4 && tensor[column][row].abs() < 1e-4);
        }
    }

    /// The centre and the tensor about it, computed directly from the voxels: for each cube, the trace of its second moment times the
    /// identity less the second moment.
    fn directly(voxels: &[([i64; 3], f64)]) -> ([f64; 3], [[f64; 3]; 3]) {
        let mass: f64 = voxels.iter().map(|(_, mass)| mass).sum();
        let centre = [0, 1, 2].map(|axis| {
            voxels
                .iter()
                .map(|(voxel, m)| m * (voxel[axis] as f64 + 0.5))
                .sum::<f64>()
                / mass
        });
        let mut tensor = [[0.0f64; 3]; 3];
        for (voxel, m) in voxels {
            let at = [0, 1, 2].map(|axis| voxel[axis] as f64 + 0.5 - centre[axis]);
            let square: f64 = at.iter().map(|c| c * c).sum::<f64>() + 3.0 / 12.0;
            for i in 0..3 {
                for j in 0..3 {
                    let own = if i == j { 1.0 / 12.0 } else { 0.0 };
                    let diagonal = if i == j { square } else { 0.0 };
                    tensor[i][j] += m * (diagonal - at[i] * at[j] - own);
                }
            }
        }
        (centre, tensor)
    }

    #[test]
    fn a_lopsided_body_has_the_products_of_inertia_its_voxels_give() {
        // An L: its axes of inertia are not those of its box.
        let voxels: Vec<([i64; 3], f64)> = [
            [0, 0, 0],
            [1, 0, 0],
            [2, 0, 0],
            [3, 0, 0],
            [0, 1, 0],
            [0, 2, 0],
        ]
        .into_iter()
        .map(|voxel| (voxel, 1.0))
        .collect();
        let properties = MassMoments::sum(
            voxels
                .iter()
                .map(|&(voxel, mass)| MassMoments::of_voxel(voxel, mass)),
        )
        .properties()
        .unwrap();
        let (centre, tensor) = directly(&voxels);
        for i in 0..3 {
            assert!((f64::from(properties.centre_of_mass[i]) - centre[i]).abs() < 1e-5);
            for j in 0..3 {
                assert!(
                    (f64::from(properties.inertia[i][j]) - tensor[i][j]).abs() < 1e-4,
                    "{i}{j}: {:?} vs {tensor:?}",
                    properties.inertia
                );
            }
        }
        assert!(tensor[0][1].abs() > 0.5, "the L has a product of inertia");
    }

    #[test]
    fn mass_is_summed_by_brick_or_all_at_once_to_the_same_properties() {
        let voxels: Vec<([i64; 3], f64)> = (0..40)
            .map(|index| {
                (
                    [index % 5, index / 5 % 4, index / 20],
                    1.0 + (index % 3) as f64,
                )
            })
            .collect();
        let whole = MassMoments::sum(
            voxels
                .iter()
                .map(|&(voxel, mass)| MassMoments::of_voxel(voxel, mass)),
        );
        let halves = MassMoments::sum([
            MassMoments::sum(
                voxels[..17]
                    .iter()
                    .map(|&(v, m)| MassMoments::of_voxel(v, m)),
            ),
            MassMoments::sum(
                voxels[17..]
                    .iter()
                    .map(|&(v, m)| MassMoments::of_voxel(v, m)),
            ),
        ]);
        let (a, b) = (whole.properties().unwrap(), halves.properties().unwrap());
        assert!(close(a.mass, b.mass));
        for axis in 0..3 {
            assert!(close(a.centre_of_mass[axis], b.centre_of_mass[axis]));
            for other in 0..3 {
                assert!(close(a.inertia[axis][other], b.inertia[axis][other]));
            }
        }
    }

    proptest! {
        #[test]
        fn the_properties_of_any_body_are_those_its_voxels_give_directly(
            voxels in proptest::collection::vec(([-6i64..6, -6i64..6, -6i64..6], 1u32..9), 1..30),
        ) {
            let voxels: Vec<([i64; 3], f64)> =
                voxels.into_iter().map(|(voxel, mass)| (voxel, f64::from(mass))).collect();
            let properties = MassMoments::sum(
                voxels.iter().map(|&(voxel, mass)| MassMoments::of_voxel(voxel, mass)),
            )
            .properties()
            .unwrap();
            let (centre, tensor) = directly(&voxels);
            for i in 0..3 {
                prop_assert!((f64::from(properties.centre_of_mass[i]) - centre[i]).abs() < 1e-4);
                for j in 0..3 {
                    let scale = 1.0 + tensor[i][j].abs();
                    prop_assert!((f64::from(properties.inertia[i][j]) - tensor[i][j]).abs() <= 1e-4 * scale);
                    prop_assert!(properties.inertia[i][j] == properties.inertia[j][i]);
                }
            }
        }
    }

    #[test]
    fn the_same_voxels_give_the_same_bits() {
        let make = || {
            MassMoments::sum((0..50).map(|index| {
                MassMoments::of_voxel(
                    [index % 7, index / 7, (index * 3) % 5],
                    0.5 + (index % 4) as f64,
                )
            }))
            .properties()
            .unwrap()
        };
        let (a, b) = (make(), make());
        assert_eq!(
            a.inertia.map(|row| row.map(f32::to_bits)),
            b.inertia.map(|row| row.map(f32::to_bits))
        );
    }
}

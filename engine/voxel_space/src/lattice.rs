use crate::{FaceDirection, Orientation, VoxelCoordinate};

/// One of the 24 ways to turn a grid so that its voxels still line up with another grid's voxels: a turn by
/// a multiple of 90 degrees about each axis. All arithmetic on it is on integers, so it is exact.
///
/// It is stored as where each basis vector goes: `images[i]` is `+(j + 1)` or `-(j + 1)` when the `i`th axis is
/// carried onto the `j`th axis, with or without a flip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LatticeRotation {
    images: [i8; 3],
}

impl LatticeRotation {
    /// No turn.
    pub const IDENTITY: Self = Self { images: [1, 2, 3] };

    /// How many rotations there are.
    pub const COUNT: usize = 24;

    /// The number of this rotation in [`LatticeRotation::all`]: 0 for none. This is how a block's orientation is saved in a world, the same for
    /// every block, so the order of `all` is a file format and must never change.
    pub fn index(self) -> u8 {
        Self::all()
            .iter()
            .position(|rotation| *rotation == self)
            .expect("every rotation is in the list of all") as u8
    }

    /// The rotation with this number in [`LatticeRotation::all`]; `None` for a number there is no rotation for.
    pub fn from_index(index: u8) -> Option<Self> {
        Self::all().get(usize::from(index)).copied()
    }

    /// All 24 rotations, in a fixed order (the identity first).
    pub fn all() -> [Self; 24] {
        // Each permutation of the axes and whether it is an even one.
        const PERMUTATIONS: [([usize; 3], bool); 6] = [
            ([0, 1, 2], true),
            ([0, 2, 1], false),
            ([1, 0, 2], false),
            ([1, 2, 0], true),
            ([2, 0, 1], true),
            ([2, 1, 0], false),
        ];
        let mut rotations = Vec::with_capacity(24);
        for (permutation, even) in PERMUTATIONS {
            for flips in 0..8u8 {
                let flipped = flips.count_ones() % 2 == 1;
                // A turn, not a mirror image: an even permutation with an even number of flips, or an odd one with an odd number.
                if even == flipped {
                    continue;
                }
                let mut images = [0i8; 3];
                for (axis, image) in images.iter_mut().enumerate() {
                    let magnitude = permutation[axis] as i8 + 1;
                    *image = if flips >> axis & 1 == 1 {
                        -magnitude
                    } else {
                        magnitude
                    };
                }
                rotations.push(Self { images });
            }
        }
        rotations
            .try_into()
            .expect("there are 24 rotations of the cube")
    }

    /// The axis and sign (`1` or `-1`) the `axis`th basis vector is carried onto.
    fn image(self, axis: usize) -> (usize, i64) {
        let image = self.images[axis];
        (image.unsigned_abs() as usize - 1, i64::from(image.signum()))
    }

    /// The rotation as a matrix: `matrix()[row][column]`, each entry 0, 1 or -1.
    pub fn matrix(self) -> [[i64; 3]; 3] {
        let mut matrix = [[0; 3]; 3];
        for (column, image) in self.images.iter().enumerate() {
            let row = image.unsigned_abs() as usize - 1;
            matrix[row][column] = i64::from(image.signum());
        }
        matrix
    }

    /// The vector turned. Components must be within the voxel coordinate range, so that flipping one cannot overflow.
    pub fn apply(self, vector: [i64; 3]) -> [i64; 3] {
        let mut turned = [0; 3];
        for (axis, component) in vector.into_iter().enumerate() {
            let (target, sign) = self.image(axis);
            turned[target] = sign * component;
        }
        turned
    }

    /// The face this turn carries a face to: the one that looks where the face's step goes.
    pub fn apply_face(self, face: FaceDirection) -> FaceDirection {
        let turned = self.apply(face.offset());
        FaceDirection::ALL
            .into_iter()
            .find(|candidate| candidate.offset() == turned)
            .expect("a turn of a unit step along an axis is a unit step along an axis")
    }

    /// The rotation that does `self` and then `next`.
    pub fn then(self, next: Self) -> Self {
        let mut images = [0i8; 3];
        for (axis, image) in images.iter_mut().enumerate() {
            let (middle, first_sign) = self.image(axis);
            let (last, second_sign) = next.image(middle);
            let sign = first_sign * second_sign;
            *image = (last as i8 + 1) * sign as i8;
        }
        Self { images }
    }

    /// The rotation that undoes this one.
    pub fn inverse(self) -> Self {
        let mut images = [0i8; 3];
        for axis in 0..3 {
            let (target, sign) = self.image(axis);
            images[target] = (axis as i8 + 1) * sign as i8;
        }
        Self { images }
    }

    /// The same turn as an [`Orientation`], with its first component that is not zero positive (a unit quaternion and its negative are
    /// the same turn; this is the one that is written, so that two equal turns are equal bit for bit).
    pub fn orientation(self) -> Orientation {
        let m = self.matrix().map(|row| row.map(|entry| entry as f32));
        let trace = m[0][0] + m[1][1] + m[2][2];
        let [x, y, z, w] = if trace > 0.0 {
            let scale = (trace + 1.0).sqrt() * 2.0;
            [
                (m[2][1] - m[1][2]) / scale,
                (m[0][2] - m[2][0]) / scale,
                (m[1][0] - m[0][1]) / scale,
                0.25 * scale,
            ]
        } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
            let scale = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
            [
                0.25 * scale,
                (m[0][1] + m[1][0]) / scale,
                (m[0][2] + m[2][0]) / scale,
                (m[2][1] - m[1][2]) / scale,
            ]
        } else if m[1][1] > m[2][2] {
            let scale = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
            [
                (m[0][1] + m[1][0]) / scale,
                0.25 * scale,
                (m[1][2] + m[2][1]) / scale,
                (m[0][2] - m[2][0]) / scale,
            ]
        } else {
            let scale = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
            [
                (m[0][2] + m[2][0]) / scale,
                (m[1][2] + m[2][1]) / scale,
                0.25 * scale,
                (m[1][0] - m[0][1]) / scale,
            ]
        };
        let mut components = [x, y, z, w];
        // The first component that is not zero is positive. (Adding zero turns -0.0 into 0.0.)
        let first = components.iter().find(|component| **component != 0.0);
        if first.is_some_and(|component| *component < 0.0) {
            components = components.map(|component| -component);
        }
        Orientation::from_components(components.map(|component| component + 0.0))
            .expect("a lattice rotation is a unit quaternion")
    }

    /// The lattice rotation closest to an orientation, and how close it is: the absolute value of the dot product of the two unit
    /// quaternions, 1 for the same turn and 0.7071 for the furthest possible (45 degrees). The first of [`LatticeRotation::all`] wins a tie.
    pub fn nearest(orientation: Orientation) -> (Self, f32) {
        let wanted = orientation.components();
        let mut best = (Self::IDENTITY, -1.0f32);
        for rotation in Self::all() {
            let own = rotation.orientation().components();
            let dot: f32 = own.iter().zip(wanted).map(|(a, b)| a * b).sum();
            if dot.abs() > best.1 {
                best = (rotation, dot.abs());
            }
        }
        best
    }
}

/// How one grid's voxels are carried onto another's lattice: a [`LatticeRotation`] and then a whole-voxel offset. Exact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LatticeTransform {
    pub rotation: LatticeRotation,
    pub offset: [i64; 3],
}

impl LatticeTransform {
    pub const IDENTITY: Self = Self {
        rotation: LatticeRotation::IDENTITY,
        offset: [0; 3],
    };

    /// Where a voxel goes, or `None` if that is outside the supported coordinate range.
    pub fn apply(self, voxel: VoxelCoordinate) -> Option<VoxelCoordinate> {
        let turned = self.rotation.apply([voxel.x(), voxel.y(), voxel.z()]);
        VoxelCoordinate::try_new(
            turned[0].checked_add(self.offset[0])?,
            turned[1].checked_add(self.offset[1])?,
            turned[2].checked_add(self.offset[2])?,
        )
        .ok()
    }

    /// Where a point goes: the same map as [`LatticeTransform::apply`], for a point anywhere in a voxel (coordinates in voxels, the middle of voxel
    /// `v` being at `v + 0.5`). The middle of a voxel goes to the middle of the voxel it goes to, so what stands on a block stays on it.
    pub fn apply_point(self, point: [f64; 3]) -> [f64; 3] {
        let matrix = self.rotation.matrix();
        [0, 1, 2].map(|row| {
            let turned: f64 = (0..3)
                .map(|column| matrix[row][column] as f64 * point[column])
                .sum();
            let along: f64 = (0..3).map(|column| matrix[row][column] as f64).sum();
            turned + self.offset[row] as f64 + 0.5 * (1.0 - along)
        })
    }

    /// The transform that does `self` and then `next`, or `None` if an offset would overflow.
    pub fn then(self, next: Self) -> Option<Self> {
        let turned = next.rotation.apply(self.offset);
        Some(Self {
            rotation: self.rotation.then(next.rotation),
            offset: [
                turned[0].checked_add(next.offset[0])?,
                turned[1].checked_add(next.offset[1])?,
                turned[2].checked_add(next.offset[2])?,
            ],
        })
    }

    /// The transform that undoes this one, or `None` if an offset would overflow.
    pub fn inverse(self) -> Option<Self> {
        let rotation = self.rotation.inverse();
        let turned = rotation.apply(self.offset);
        Some(Self {
            rotation,
            offset: [
                turned[0].checked_neg()?,
                turned[1].checked_neg()?,
                turned[2].checked_neg()?,
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn rotation() -> impl Strategy<Value = LatticeRotation> {
        (0..24usize).prop_map(|index| LatticeRotation::all()[index])
    }

    fn vector() -> impl Strategy<Value = [i64; 3]> {
        prop::array::uniform3(-1_000_000i64..1_000_000)
    }

    fn transform() -> impl Strategy<Value = LatticeTransform> {
        (rotation(), vector()).prop_map(|(rotation, offset)| LatticeTransform { rotation, offset })
    }

    proptest! {
        #[test]
        fn a_point_goes_where_the_voxel_it_is_in_goes(
            transform in transform(),
            voxel in prop::array::uniform3(-100_000i64..100_000),
            inside in prop::array::uniform3(0.0f64..1.0),
        ) {
            let from = VoxelCoordinate::new(voxel[0], voxel[1], voxel[2]);
            let to = transform.apply(from).unwrap();
            // The middle of the voxel goes to the middle of the voxel it goes to.
            let middle = transform.apply_point([0, 1, 2].map(|axis| voxel[axis] as f64 + 0.5));
            prop_assert_eq!(middle, [to.x() as f64 + 0.5, to.y() as f64 + 0.5, to.z() as f64 + 0.5]);
            // And any point of it goes to a point of the other.
            let point = transform.apply_point([0, 1, 2].map(|axis| voxel[axis] as f64 + inside[axis]));
            for (axis, value) in [to.x(), to.y(), to.z()].into_iter().enumerate() {
                prop_assert!(point[axis] >= value as f64 && point[axis] <= value as f64 + 1.0, "{point:?} not in {to:?}");
            }
        }
    }

    fn determinant(m: [[i64; 3]; 3]) -> i64 {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }

    #[test]
    fn there_are_24_distinct_turns_and_none_is_a_mirror_image() {
        let all = LatticeRotation::all();
        assert_eq!(all[0], LatticeRotation::IDENTITY);
        for (index, rotation) in all.iter().enumerate() {
            assert_eq!(determinant(rotation.matrix()), 1, "{rotation:?}");
            for other in &all[index + 1..] {
                assert_ne!(rotation, other);
            }
        }
    }

    #[test]
    fn a_rotation_has_a_number_for_ever_the_identity_is_zero_and_the_order_is_the_file_format() {
        let all = LatticeRotation::all();
        assert_eq!(LatticeRotation::COUNT, all.len());
        assert_eq!(LatticeRotation::IDENTITY.index(), 0);
        for (index, rotation) in all.iter().enumerate() {
            assert_eq!(rotation.index() as usize, index);
            assert_eq!(LatticeRotation::from_index(index as u8), Some(*rotation));
        }
        assert_eq!(LatticeRotation::from_index(24), None);
        assert_eq!(LatticeRotation::from_index(255), None);
        // The order is written down in saved worlds: where each rotation sends the three axes, for all 24, as one number. Do not change it.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for rotation in all {
            for axis in [[1, 0, 0], [0, 1, 0], [0, 0, 1]] {
                for component in rotation.apply(axis) {
                    hash = (hash ^ (component + 2) as u64).wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
        }
        assert_eq!(hash, PINNED_ORDER);
        // Doing one and then another is one more, whose number the numbers do not need to know to find: the composition is on the rotations.
        for first in all {
            for second in all {
                let both = first.then(second);
                assert_eq!(LatticeRotation::from_index(both.index()), Some(both));
            }
        }
    }

    const PINNED_ORDER: u64 = 989_952_821_047_606_661;

    #[test]
    fn a_face_goes_where_its_step_goes_and_opposite_faces_stay_opposite() {
        for rotation in LatticeRotation::all() {
            for face in FaceDirection::ALL {
                let turned = rotation.apply_face(face);
                assert_eq!(turned.offset(), rotation.apply(face.offset()));
                assert_eq!(
                    rotation.apply_face(face.opposite()),
                    turned.opposite(),
                    "{rotation:?} {face:?}"
                );
                assert_eq!(rotation.inverse().apply_face(turned), face);
            }
        }
        // The turn that carries +z to +x about +y carries the east face to the north face (-z) and leaves the top alone.
        let quarter = LatticeRotation::all()
            .into_iter()
            .find(|rotation| {
                rotation.apply([0, 0, 1]) == [1, 0, 0] && rotation.apply([0, 1, 0]) == [0, 1, 0]
            })
            .unwrap();
        assert_eq!(
            quarter.apply_face(FaceDirection::PositiveX),
            FaceDirection::NegativeZ
        );
        assert_eq!(
            quarter.apply_face(FaceDirection::PositiveY),
            FaceDirection::PositiveY
        );
    }

    #[test]
    fn a_quarter_turn_carries_the_axes_where_it_should() {
        // About +y by 90 degrees (the way the camera's yaw turns): +z goes to +x and +x goes to -z.
        let quarter = LatticeRotation::all()
            .into_iter()
            .find(|rotation| {
                rotation.apply([0, 0, 1]) == [1, 0, 0] && rotation.apply([0, 1, 0]) == [0, 1, 0]
            })
            .unwrap();
        assert_eq!(quarter.apply([1, 0, 0]), [0, 0, -1]);
        assert_eq!(quarter.apply([3, 4, 5]), [5, 4, -3]);
    }

    #[test]
    fn the_orientation_of_a_rotation_turns_vectors_the_same_way() {
        for rotation in LatticeRotation::all() {
            let [x, y, z, w] = rotation.orientation().components();
            for axis in 0..3 {
                let mut basis = [0.0f32; 3];
                basis[axis] = 1.0;
                // v + 2w (q x v) + 2 q x (q x v)
                let cross = |a: [f32; 3], b: [f32; 3]| {
                    [
                        a[1] * b[2] - a[2] * b[1],
                        a[2] * b[0] - a[0] * b[2],
                        a[0] * b[1] - a[1] * b[0],
                    ]
                };
                let q = [x, y, z];
                let first = cross(q, basis);
                let second = cross(q, first);
                let turned: Vec<f32> = (0..3)
                    .map(|i| basis[i] + 2.0 * w * first[i] + 2.0 * second[i])
                    .collect();
                let mut unit = [0i64; 3];
                unit[axis] = 1;
                let expected = rotation.apply(unit);
                for i in 0..3 {
                    assert!(
                        (turned[i] - expected[i] as f32).abs() < 1e-6,
                        "{rotation:?} axis {axis}: {turned:?} vs {expected:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_orientation_of_a_rotation_is_written_one_way() {
        let identity = LatticeRotation::IDENTITY.orientation();
        assert_eq!(identity, Orientation::IDENTITY);
        for rotation in LatticeRotation::all() {
            let components = rotation.orientation().components();
            let first = components
                .iter()
                .find(|component| **component != 0.0)
                .unwrap();
            assert!(*first > 0.0, "{rotation:?}: {components:?}");
            assert!(
                components
                    .iter()
                    .all(|component| !(*component == 0.0 && component.is_sign_negative())),
                "{rotation:?}: {components:?}"
            );
        }
    }

    #[test]
    fn the_nearest_rotation_to_a_rotation_is_itself_and_a_small_tilt_does_not_change_it() {
        for rotation in LatticeRotation::all() {
            let (found, closeness) = LatticeRotation::nearest(rotation.orientation());
            assert_eq!(found, rotation);
            assert!((closeness - 1.0).abs() < 1e-6, "{closeness}");
        }
        let tilted = Orientation::from_yaw_pitch(std::f32::consts::FRAC_PI_2 + 0.05, 0.02);
        let (found, closeness) = LatticeRotation::nearest(tilted);
        assert_eq!(found.apply([0, 0, 1]), [1, 0, 0]);
        assert!(closeness > 0.999 && closeness < 1.0, "{closeness}");
    }

    proptest! {
        #[test]
        fn rotations_keep_lengths_and_inverses_undo_them(rotation in rotation(), v in vector()) {
            let turned = rotation.apply(v);
            let square = |v: [i64; 3]| v.iter().map(|c| c * c).sum::<i64>();
            prop_assert_eq!(square(turned), square(v));
            prop_assert_eq!(rotation.inverse().apply(turned), v);
            prop_assert_eq!(rotation.then(rotation.inverse()), LatticeRotation::IDENTITY);
        }

        #[test]
        fn doing_one_turn_and_then_another_is_applying_them_in_that_order(
            first in rotation(), second in rotation(), v in vector(),
        ) {
            prop_assert_eq!(first.then(second).apply(v), second.apply(first.apply(v)));
        }

        #[test]
        fn a_transform_is_undone_by_its_inverse_and_composes_in_order(
            first in transform(), second in transform(), v in vector(),
        ) {
            let voxel = VoxelCoordinate::new(v[0], v[1], v[2]);
            let there = first.apply(voxel).unwrap();
            prop_assert_eq!(first.inverse().unwrap().apply(there), Some(voxel));
            let both = first.then(second).unwrap();
            prop_assert_eq!(Some(both.apply(voxel).unwrap()), second.apply(there));
        }
    }

    #[test]
    fn a_transform_that_leaves_the_coordinate_range_gives_none() {
        let far = VoxelCoordinate::new(crate::VOXEL_COORDINATE_MAXIMUM, 0, 0);
        let shift = LatticeTransform {
            offset: [1, 0, 0],
            ..LatticeTransform::IDENTITY
        };
        assert_eq!(shift.apply(far), None);
        assert_eq!(
            LatticeTransform {
                offset: [i64::MAX, 0, 0],
                ..LatticeTransform::IDENTITY
            }
            .then(shift),
            None
        );
    }
}

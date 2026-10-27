use crate::{
    BRICK_EDGE_LENGTH, BrickCoordinate, LatticeRotation, LatticeTransform, Orientation,
    VoxelCoordinate, WorldPosition,
};

const EDGE: f64 = BRICK_EDGE_LENGTH as f64;

/// Where a grid is and how it is turned: the world position of its origin (the lowest corner of its voxel `(0, 0, 0)`) and its
/// orientation. A position in the grid is a vector from that origin, turned by the orientation.
///
/// Every conversion here is deterministic: sums and products (in `f64`) and square roots only, never a fused or a
/// platform-specific operation, so the same pose gives the same bits on every machine. Positions far from the origin stay exact to
/// the precision of `f64` (a whole voxel up to 2^53 voxels) because they are carried as a brick and an offset, as
/// [`WorldPosition`] does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridPose {
    pub position: WorldPosition,
    pub orientation: Orientation,
}

/// How close two grids must be to one lattice to count as on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlignmentTolerance {
    /// The least [`LatticeRotation::nearest`] closeness of their relative turn to a lattice rotation (1 is exact).
    pub minimum_closeness: f64,
    /// The most, in voxels, the offset between their origins may be from a whole number on each axis.
    pub maximum_offset: f64,
}

impl Default for AlignmentTolerance {
    /// About half a degree and a hundredth of a voxel.
    fn default() -> Self {
        Self {
            minimum_closeness: 0.99999,
            maximum_offset: 0.01,
        }
    }
}

impl GridPose {
    pub fn new(position: WorldPosition, orientation: Orientation) -> Self {
        Self {
            position,
            orientation,
        }
    }

    /// The grid at the origin of the world, unturned.
    pub fn identity() -> Self {
        Self::new(
            WorldPosition::new(BrickCoordinate::new(0, 0, 0), [0.0; 3]),
            Orientation::IDENTITY,
        )
    }

    /// The pose's turn as a matrix (`rotation_matrix()[row][column]`; a position in the grid, turned, is the matrix times the vector),
    /// normalised, so that a slightly off unit quaternion does not scale lengths.
    pub fn rotation_matrix(&self) -> [[f64; 3]; 3] {
        let [x, y, z, w] = self.orientation.components();
        let length = (x * x + y * y + z * z + w * w).sqrt();
        let (x, y, z, w) = (x / length, y / length, z / length, w / length);
        [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y - z * w),
                2.0 * (x * z + y * w),
            ],
            [
                2.0 * (x * y + z * w),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z - x * w),
            ],
            [
                2.0 * (x * z - y * w),
                2.0 * (y * z + x * w),
                1.0 - 2.0 * (x * x + y * y),
            ],
        ]
    }

    /// A position in the grid, as a vector in voxels from the grid's origin.
    fn local_vector(position: WorldPosition) -> [f64; 3] {
        let brick = position.brick();
        let offset = position.offset();
        [
            brick.x() as f64 * EDGE + f64::from(offset[0]),
            brick.y() as f64 * EDGE + f64::from(offset[1]),
            brick.z() as f64 * EDGE + f64::from(offset[2]),
        ]
    }

    /// The vector from this pose's origin to a world position, in voxels, as `f64` (bricks differenced first, so it is
    /// exact for positions near each other however far both are from the world's origin).
    fn world_vector_from_origin(&self, world: WorldPosition) -> [f64; 3] {
        let (brick, origin_brick) = (world.brick(), self.position.brick());
        let (offset, origin_offset) = (world.offset(), self.position.offset());
        [
            (brick.x() - origin_brick.x()) as f64 * EDGE
                + (f64::from(offset[0]) - f64::from(origin_offset[0])),
            (brick.y() - origin_brick.y()) as f64 * EDGE
                + (f64::from(offset[1]) - f64::from(origin_offset[1])),
            (brick.z() - origin_brick.z()) as f64 * EDGE
                + (f64::from(offset[2]) - f64::from(origin_offset[2])),
        ]
    }

    /// `origin` moved by a vector of voxels, with whole bricks carried exactly.
    fn displaced(origin: WorldPosition, vector: [f64; 3]) -> Option<WorldPosition> {
        let mut bricks = [0i128; 3];
        let mut offset = [0.0f32; 3];
        let origin_offset = origin.offset();
        for axis in 0..3 {
            if !vector[axis].is_finite() {
                return None;
            }
            let whole = (vector[axis] / EDGE).floor();
            // The cast saturates, and an out-of-range brick is refused below.
            bricks[axis] = whole as i128;
            offset[axis] = (f64::from(origin_offset[axis]) + (vector[axis] - whole * EDGE)) as f32;
        }
        let brick = origin
            .brick()
            .checked_offset(bricks[0], bricks[1], bricks[2])?;
        // (The offset may be a whole brick or more past the end of the brick, and is carried into the next: a place past the edge of the
        // world is none, not a panic.)
        WorldPosition::try_new(brick, offset)
    }

    fn turn(matrix: [[f64; 3]; 3], vector: [f64; 3]) -> [f64; 3] {
        matrix.map(|row| row[0] * vector[0] + row[1] * vector[1] + row[2] * vector[2])
    }

    fn turn_back(matrix: [[f64; 3]; 3], vector: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|column| {
            matrix[0][column] * vector[0]
                + matrix[1][column] * vector[1]
                + matrix[2][column] * vector[2]
        })
    }

    /// The vector from a position in the world (a camera) to this grid's origin, in voxels. Bricks are differenced exactly first, so it
    /// is as exact for a camera far from the world's origin as for one near it.
    pub fn origin_relative_to(&self, camera: WorldPosition) -> [f64; 3] {
        self.world_vector_from_origin(camera)
            .map(|component| -component)
    }

    /// Where a position in the grid is in the world, or `None` if that is outside the supported range.
    pub fn local_to_world(&self, local: WorldPosition) -> Option<WorldPosition> {
        let turned = Self::turn(self.rotation_matrix(), Self::local_vector(local));
        Self::displaced(self.position, turned)
    }

    /// Where a world position is in the grid, or `None` if that is outside the supported range.
    pub fn world_to_local(&self, world: WorldPosition) -> Option<WorldPosition> {
        let local = Self::turn_back(self.rotation_matrix(), self.world_vector_from_origin(world));
        Self::displaced(
            WorldPosition::new(BrickCoordinate::new(0, 0, 0), [0.0; 3]),
            local,
        )
    }

    /// A direction (gravity, a velocity) in the grid's axes, in the world's axes.
    pub fn direction_to_world(&self, direction: [f32; 3]) -> [f32; 3] {
        Self::turn(self.rotation_matrix(), direction.map(f64::from))
            .map(|component| component as f32)
    }

    /// A direction in the world's axes, in the grid's axes.
    pub fn direction_to_local(&self, direction: [f32; 3]) -> [f32; 3] {
        Self::turn_back(self.rotation_matrix(), direction.map(f64::from))
            .map(|component| component as f32)
    }

    /// Where a thing placed at `position` in the grid and turned `orientation` there is in the world: its position, and its turn (the
    /// grid's, then the thing's). `None` if the position is outside the supported range.
    pub fn place_in_world(
        &self,
        position: WorldPosition,
        orientation: Orientation,
    ) -> Option<(WorldPosition, Orientation)> {
        Some((
            self.local_to_world(position)?,
            self.orientation.compose(orientation),
        ))
    }

    /// Where a thing placed at a position in the world and turned `orientation` there is in the grid: the inverse of
    /// [`GridPose::place_in_world`].
    pub fn place_in_grid(
        &self,
        position: WorldPosition,
        orientation: Orientation,
    ) -> Option<(WorldPosition, Orientation)> {
        Some((
            self.world_to_local(position)?,
            self.orientation.inverse().compose(orientation),
        ))
    }
}

/// The voxel of the grid `to` that holds the centre of a voxel of the grid `from`, or `None` if that is outside the supported range.
/// A centre exactly on the boundary between two voxels is in the one toward negative infinity (the floor), the same on every machine.
///
/// This is the one conversion between grids that rules are allowed (through the engine, never by reading another grid): it is what a
/// fluid or a current crossing the edge of a ship uses to find where it arrives. It is many to one when the grids are not on one lattice.
pub fn voxel_in_other(
    from: &GridPose,
    voxel: VoxelCoordinate,
    to: &GridPose,
) -> Option<VoxelCoordinate> {
    let centre = WorldPosition::new(BrickCoordinate::new(0, 0, 0), [0.0; 3]);
    let centre = GridPose::displaced(
        centre,
        [
            voxel.x() as f64 + 0.5,
            voxel.y() as f64 + 0.5,
            voxel.z() as f64 + 0.5,
        ],
    )?;
    let world = from.local_to_world(centre)?;
    let local = GridPose::turn_back(to.rotation_matrix(), to.world_vector_from_origin(world));
    VoxelCoordinate::try_new(
        local[0].floor() as i128,
        local[1].floor() as i128,
        local[2].floor() as i128,
    )
    .ok()
}

/// The transform that carries the voxels of `from` onto the lattice of `to`, if the two are on one lattice to within the tolerance:
/// their relative turn is a multiple of 90 degrees and the offset between their origins is a whole number of voxels. Applying it to a
/// voxel of `from` gives the voxel of `to` that [`voxel_in_other`] finds. Docking pulls two grids until this is `Some`.
pub fn lattice_alignment(
    from: &GridPose,
    to: &GridPose,
    tolerance: AlignmentTolerance,
) -> Option<LatticeTransform> {
    // The turn of `from` seen from `to`: the conjugate of `to`'s turn, then `from`'s.
    let relative = to.orientation.inverse().compose(from.orientation);
    let (rotation, closeness) =
        LatticeRotation::nearest(Orientation::from_components(relative.components())?);
    if closeness < tolerance.minimum_closeness {
        return None;
    }
    // Where `from`'s origin is in `to`'s axes.
    let origin = GridPose::turn_back(
        to.rotation_matrix(),
        to.world_vector_from_origin(from.position),
    );
    let mut offset = [0i128; 3];
    for axis in 0..3 {
        let whole = origin[axis].round();
        if (origin[axis] - whole).abs() > tolerance.maximum_offset || whole.abs() > 1e18 {
            return None;
        }
        offset[axis] = whole as i128;
    }
    // A voxel's own centre is half a voxel from its corner, and a turn moves that half voxel: the offset of voxel numbers is the
    // offset of origins plus (R * (1, 1, 1) - (1, 1, 1)) / 2, which is 0 or -1 on each axis.
    let corner = rotation.apply([1, 1, 1]);
    for axis in 0..3 {
        offset[axis] += (corner[axis] - 1) / 2;
    }
    Some(LatticeTransform { rotation, offset })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::{BRICK_COORDINATE_MAXIMUM, BRICK_COORDINATE_MINIMUM};

    fn at(x: i128, y: i128, z: i128, offset: [f32; 3]) -> WorldPosition {
        WorldPosition::new(BrickCoordinate::new(x, y, z), offset)
    }

    fn pose(position: WorldPosition, orientation: Orientation) -> GridPose {
        GridPose::new(position, orientation)
    }

    #[test]
    fn a_place_just_past_the_edge_of_the_world_is_no_place_and_not_a_panic() {
        // The last brick there is, and a ship whose origin is near its far corner: a place half a voxel on carries into the next brick.
        let last = BRICK_COORDINATE_MAXIMUM;
        let near_the_edge = pose(at(last, last, last, [31.9; 3]), Orientation::IDENTITY);
        let past = at(0, 0, 0, [0.5; 3]);
        assert_eq!(near_the_edge.local_to_world(past), None);
        // The same distance from the origin the other way is a place (the low edge has room).
        let inside = pose(at(last, last, last, [31.0; 3]), Orientation::IDENTITY);
        assert!(inside.local_to_world(at(0, 0, 0, [0.5; 3])).is_some());
        // And at the low end of the world too.
        let low = pose(
            at(BRICK_COORDINATE_MINIMUM, 0, 0, [0.0; 3]),
            Orientation::IDENTITY,
        );
        assert_eq!(low.local_to_world(at(-1, 0, 0, [31.5, 0.0, 0.0])), None);
        assert!(low.local_to_world(at(0, 0, 0, [1.5, 0.0, 0.0])).is_some());
    }

    fn close(a: WorldPosition, b: WorldPosition, tolerance: f32) -> bool {
        a.relative_to(b).iter().all(|c| c.abs() <= tolerance)
    }

    /// A pose on the lattice of the world: a lattice turn and a whole-voxel position.
    fn lattice_pose(rotation: LatticeRotation, voxel: [i128; 3]) -> GridPose {
        let position = WorldPosition::new(
            VoxelCoordinate::new(voxel[0], voxel[1], voxel[2]).split().0,
            {
                let local = VoxelCoordinate::new(voxel[0], voxel[1], voxel[2]).split().1;
                [local.x() as f32, local.y() as f32, local.z() as f32]
            },
        );
        pose(position, rotation.orientation())
    }

    fn any_orientation() -> impl Strategy<Value = Orientation> {
        prop::array::uniform4(-1.0f64..1.0)
            .prop_filter("not a tiny vector", |q| {
                q.iter().map(|c| c * c).sum::<f64>() > 0.01
            })
            .prop_map(|q| {
                let length = q.iter().map(|c| c * c).sum::<f64>().sqrt();
                Orientation::from_components(q.map(|c| c / length)).unwrap()
            })
    }

    fn any_lattice_rotation() -> impl Strategy<Value = LatticeRotation> {
        (0..24usize).prop_map(|index| LatticeRotation::all()[index])
    }

    #[test]
    fn the_origin_of_a_grid_is_found_from_a_camera_and_the_matrix_turns_as_the_pose_does() {
        let grid = pose(at(3, -2, 5, [1.0, 2.0, 3.0]), Orientation::IDENTITY);
        let camera = at(1_000_000, 4, -7, [0.5, 0.25, 31.0]);
        // The vector from the camera to the origin: bricks apart, exactly, then the offsets.
        assert_eq!(
            grid.origin_relative_to(camera),
            [
                (3 - 1_000_000) as f64 * 32.0 + 0.5,
                (-2 - 4) as f64 * 32.0 + 1.75,
                (5 + 7) as f64 * 32.0 - 28.0
            ]
        );
        // The matrix of a quarter turn about the vertical axis carries z onto x (first row, third column), and is the identity unturned.
        assert_eq!(
            grid.rotation_matrix(),
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
        );
        let turned = pose(
            at(0, 0, 0, [0.0; 3]),
            Orientation::from_yaw_pitch(std::f32::consts::FRAC_PI_2, 0.0),
        );
        let matrix = turned.rotation_matrix();
        assert!((matrix[0][2] - 1.0).abs() < 1e-6 && (matrix[2][0] + 1.0).abs() < 1e-6);
    }

    #[test]
    fn an_unturned_grid_is_only_moved() {
        let grid = pose(at(2, 0, -1, [4.0, 5.0, 6.0]), Orientation::IDENTITY);
        let world = grid.local_to_world(at(0, 0, 0, [1.0, 2.0, 3.0])).unwrap();
        assert_eq!(world, at(2, 0, -1, [5.0, 7.0, 9.0]));
        assert_eq!(
            grid.world_to_local(world).unwrap(),
            at(0, 0, 0, [1.0, 2.0, 3.0])
        );
    }

    #[test]
    fn an_orientation_a_little_off_unit_length_does_not_stretch_what_it_turns() {}
    #[test]
    fn a_quarter_turn_about_the_vertical_axis_carries_z_onto_x() {
        let grid = pose(
            at(0, 0, 0, [0.0; 3]),
            Orientation::from_yaw_pitch(std::f32::consts::FRAC_PI_2, 0.0),
        );
        let world = grid.local_to_world(at(0, 0, 0, [0.0, 0.0, 10.0])).unwrap();
        assert!(
            close(world, at(0, 0, 0, [10.0, 0.0, 0.0]), 1e-5),
            "{world:?}"
        );
        let direction = grid.direction_to_world([0.0, 0.0, 1.0]);
        assert!((direction[0] - 1.0).abs() < 1e-6 && direction[2].abs() < 1e-6);
    }

    #[test]
    fn gravity_in_a_ship_rolled_upside_down_points_up_in_its_own_axes() {
        // Turned half a turn about x (the quaternion [1, 0, 0, 0]).
        let grid = pose(
            at(0, 0, 0, [0.0; 3]),
            Orientation::from_components([1.0, 0.0, 0.0, 0.0]).unwrap(),
        );
        let local = grid.direction_to_local([0.0, -32.0, 0.0]);
        assert!(
            local[0].abs() < 1e-5 && (local[1] - 32.0).abs() < 1e-4 && local[2].abs() < 1e-4,
            "{local:?}"
        );
        let back = grid.direction_to_world(local);
        assert!((back[1] + 32.0).abs() < 1e-4);
    }

    #[test]
    fn positions_a_long_way_from_the_origin_keep_their_precision() {
        let far = 1i128 << 44;
        let grid = pose(
            at(far, -far, far / 3, [10.25, 20.5, 30.75]),
            Orientation::from_yaw_pitch(0.7, 0.3),
        );
        let local = at(100, -200, 300, [1.5, 2.5, 3.5]);
        let world = grid.local_to_world(local).unwrap();
        assert!(close(grid.world_to_local(world).unwrap(), local, 1e-2));
    }

    #[test]
    fn a_position_outside_the_supported_range_gives_none() {
        let grid = pose(at(0, 0, 0, [0.0; 3]), Orientation::IDENTITY);
        let edge = crate::BRICK_COORDINATE_MAXIMUM;
        let beyond = pose(at(edge, 0, 0, [31.0, 0.0, 0.0]), Orientation::IDENTITY);
        assert_eq!(beyond.local_to_world(at(1, 0, 0, [0.0; 3])), None);
        assert!(grid.local_to_world(at(1, 0, 0, [0.0; 3])).is_some());
    }

    #[test]
    fn a_voxel_centre_on_a_boundary_is_in_the_voxel_toward_negative_infinity() {
        let origin = GridPose::identity();
        let half_right = pose(at(0, 0, 0, [0.5, 0.0, 0.0]), Orientation::IDENTITY);
        let half_left = pose(at(0, 0, 0, [-0.5, 0.0, 0.0]), Orientation::IDENTITY);
        let voxel = VoxelCoordinate::new(3, 4, 5);
        // In a grid whose origin is half a voxel to +x, the centre (3.5) is at 3.0: the boundary, so voxel 3.
        assert_eq!(
            voxel_in_other(&origin, voxel, &half_right),
            Some(VoxelCoordinate::new(3, 4, 5))
        );
        // In one half a voxel to -x it is at 4.0: voxel 4.
        assert_eq!(
            voxel_in_other(&origin, voxel, &half_left),
            Some(VoxelCoordinate::new(4, 4, 5))
        );
    }

    #[test]
    fn two_grids_on_one_lattice_find_the_voxel_the_transform_gives() {
        let from = lattice_pose(LatticeRotation::all()[5], [10, -3, 7]);
        let to = lattice_pose(LatticeRotation::all()[17], [-4, 20, 1]);
        let transform = lattice_alignment(&from, &to, AlignmentTolerance::default()).unwrap();
        for voxel in [
            VoxelCoordinate::new(0, 0, 0),
            VoxelCoordinate::new(5, -2, 9),
            VoxelCoordinate::new(-31, 32, -33),
        ] {
            assert_eq!(
                voxel_in_other(&from, voxel, &to),
                transform.apply(voxel),
                "{voxel:?}"
            );
        }
    }

    #[test]
    fn grids_are_not_aligned_when_turned_or_offset_by_more_than_the_tolerance() {
        let tolerance = AlignmentTolerance::default();
        let base = lattice_pose(LatticeRotation::IDENTITY, [0, 0, 0]);
        let shifted = pose(at(0, 0, 0, [0.3, 0.0, 0.0]), Orientation::IDENTITY);
        assert_eq!(lattice_alignment(&shifted, &base, tolerance), None);
        let slightly_shifted = pose(at(0, 0, 0, [0.004, 0.0, 0.0]), Orientation::IDENTITY);
        assert!(lattice_alignment(&slightly_shifted, &base, tolerance).is_some());
        let turned = pose(at(0, 0, 0, [0.0; 3]), Orientation::from_yaw_pitch(0.2, 0.0));
        assert_eq!(lattice_alignment(&turned, &base, tolerance), None);
        let barely_turned = pose(
            at(0, 0, 0, [0.0; 3]),
            Orientation::from_yaw_pitch(0.005, 0.0),
        );
        assert!(lattice_alignment(&barely_turned, &base, tolerance).is_some());
    }

    #[test]
    fn a_grid_is_aligned_with_itself_and_the_transform_to_its_neighbour_is_the_offset() {
        let base = lattice_pose(LatticeRotation::IDENTITY, [0, 0, 0]);
        assert_eq!(
            lattice_alignment(&base, &base, AlignmentTolerance::default()),
            Some(LatticeTransform::IDENTITY)
        );
        let neighbour = lattice_pose(LatticeRotation::IDENTITY, [5, 6, 7]);
        let transform =
            lattice_alignment(&neighbour, &base, AlignmentTolerance::default()).unwrap();
        assert_eq!(
            transform.apply(VoxelCoordinate::new(1, 1, 1)),
            Some(VoxelCoordinate::new(6, 7, 8))
        );
    }

    proptest! {
        #[test]
        fn a_position_survives_a_trip_into_the_world_and_back(
            orientation in any_orientation(),
            origin in prop::array::uniform3(-1000i128..1000),
            local in prop::array::uniform3(-5000.0f32..5000.0),
        ) {
            let grid = pose(at(origin[0], origin[1], origin[2], [3.0, 9.0, 27.0]), orientation);
            let local = at(0, 0, 0, local);
            let world = grid.local_to_world(local).unwrap();
            prop_assert!(close(grid.world_to_local(world).unwrap(), local, 2e-3));
        }

        #[test]
        fn turning_never_changes_a_distance(
            orientation in any_orientation(),
            a in prop::array::uniform3(-500.0f32..500.0),
            b in prop::array::uniform3(-500.0f32..500.0),
        ) {
            let grid = pose(at(7, 7, 7, [1.0; 3]), orientation);
            let (a, b) = (at(0, 0, 0, a), at(0, 0, 0, b));
            let squared = |v: [f32; 3]| v.iter().map(|c| c * c).sum::<f32>();
            let local = squared(a.relative_to(b));
            let world = squared(
                grid.local_to_world(a).unwrap().relative_to(grid.local_to_world(b).unwrap()),
            );
            prop_assert!((local - world).abs() <= 1e-3 * (1.0 + local), "{local} vs {world}");
        }

        #[test]
        fn a_direction_survives_a_trip_into_the_world_and_back(
            orientation in any_orientation(),
            direction in prop::array::uniform3(-100.0f32..100.0),
        ) {
            let grid = pose(at(0, 0, 0, [0.0; 3]), orientation);
            let back = grid.direction_to_local(grid.direction_to_world(direction));
            for axis in 0..3 {
                prop_assert!((back[axis] - direction[axis]).abs() < 1e-3);
            }
        }

        #[test]
        fn grids_on_a_lattice_agree_on_where_every_voxel_goes_in_both_directions(
            from_rotation in any_lattice_rotation(),
            to_rotation in any_lattice_rotation(),
            from_voxel in prop::array::uniform3(-200i128..200),
            to_voxel in prop::array::uniform3(-200i128..200),
            voxel in prop::array::uniform3(-100i128..100),
        ) {
            let from = lattice_pose(from_rotation, from_voxel);
            let to = lattice_pose(to_rotation, to_voxel);
            let tolerance = AlignmentTolerance::default();
            let forward = lattice_alignment(&from, &to, tolerance).unwrap();
            let backward = lattice_alignment(&to, &from, tolerance).unwrap();
            let voxel = VoxelCoordinate::new(voxel[0], voxel[1], voxel[2]);
            let there = forward.apply(voxel).unwrap();
            prop_assert_eq!(voxel_in_other(&from, voxel, &to), Some(there));
            prop_assert_eq!(backward.apply(there), Some(voxel));
            prop_assert_eq!(voxel_in_other(&to, there, &from), Some(voxel));
            prop_assert_eq!(forward.inverse(), Some(backward));
        }
    }

    #[test]
    fn a_thing_placed_in_a_grid_is_in_the_world_where_the_grid_puts_it_and_turned_by_the_grid_then_itself()
     {
        // A grid a quarter turn about the vertical axis from the world, a thing in it tipped up a quarter turn: the thing faces -z
        // in the grid, so the tip takes it to +y; the grid's turn about y leaves +y alone.
        let quarter = std::f32::consts::FRAC_PI_2;
        let grid = pose(
            at(2, 0, 0, [1.0, 2.0, 3.0]),
            Orientation::from_yaw_pitch(quarter, 0.0),
        );
        let tipped = Orientation::from_yaw_pitch(0.0, quarter);
        let (position, orientation) = grid
            .place_in_world(at(0, 0, 0, [4.0, 5.0, 6.0]), tipped)
            .unwrap();
        assert_eq!(
            position,
            grid.local_to_world(at(0, 0, 0, [4.0, 5.0, 6.0])).unwrap()
        );
        let facing = pose(at(0, 0, 0, [0.0; 3]), orientation).direction_to_world([0.0, 0.0, -1.0]);
        assert!(
            facing[0].abs() < 1e-6 && (facing[1] - 1.0).abs() < 1e-6 && facing[2].abs() < 1e-6,
            "{facing:?}"
        );
        // The other way round (the thing turned in the world first, then the grid) is not the same turn: the yaw takes -z to -x, and
        // the tip about the world's x axis leaves -x alone.
        let other = tipped.compose(grid.orientation);
        let facing = pose(at(0, 0, 0, [0.0; 3]), other).direction_to_world([0.0, 0.0, -1.0]);
        assert!(
            (facing[0] + 1.0).abs() < 1e-6 && facing[1].abs() < 1e-6 && facing[2].abs() < 1e-6,
            "{facing:?}"
        );
    }

    proptest! {
        #[test]
        fn a_thing_survives_a_trip_into_the_world_and_back(
            grid_turn in any_orientation(),
            thing_turn in any_orientation(),
            origin in prop::array::uniform3(-1000i128..1000),
            local in prop::array::uniform3(-5000.0f32..5000.0),
        ) {
            let grid = pose(at(origin[0], origin[1], origin[2], [3.0, 9.0, 27.0]), grid_turn);
            let local = at(0, 0, 0, local);
            let (world, world_turn) = grid.place_in_world(local, thing_turn).unwrap();
            let (back, back_turn) = grid.place_in_grid(world, world_turn).unwrap();
            prop_assert!(close(back, local, 2e-3));
            // The same turn, or its negative (a quaternion and its negative are one rotation).
            let (before, after) = (thing_turn.components(), back_turn.components());
            let same: f64 = (0..4).map(|axis| before[axis] * after[axis]).sum();
            prop_assert!(same.abs() > 1.0 - 1e-5, "{before:?} vs {after:?}");
        }
    }

    #[test]
    fn a_point_fifty_thousand_kilometres_out_on_a_turned_planet_is_placed_to_a_hundredth_of_a_millimetre_where_an_f32_turn_can_miss_by_metres()
     {
        // A planet (a ship) turned about the vertical axis, and a point on it 5 * 10^7 voxels from its origin along x.
        let angle = 0.7f32;
        let radius = 5.0e7f64;
        let at_radius = at(1_562_500, 0, 0, [0.0; 3]);
        let expected = |angle: f64| [radius * libm::cos(angle), 0.0, -radius * libm::sin(angle)];
        let world_vector = |turn: Orientation| {
            let world = pose(at(0, 0, 0, [0.0; 3]), turn)
                .local_to_world(at_radius)
                .unwrap();
            [
                world.brick().x() as f64 * 32.0 + f64::from(world.offset()[0]),
                world.brick().y() as f64 * 32.0 + f64::from(world.offset()[1]),
                world.brick().z() as f64 * 32.0 + f64::from(world.offset()[2]),
            ]
        };
        let exact_turn = Orientation::from_yaw_pitch(angle, 0.0);
        let found = world_vector(exact_turn);
        let wanted = expected(f64::from(angle));
        for axis in 0..3 {
            assert!(
                (found[axis] - wanted[axis]).abs() < 1e-5,
                "axis {axis}: {} against {}",
                found[axis],
                wanted[axis]
            );
        }
        // The same turns rounded to `f32` (what an orientation was) put the point metres away for some of them: over these 199 turns the worst
        // miss is 3.78 metres (measured), and for the turn above 0.05.
        let mut worst = 0.0f64;
        for step in 1..200 {
            let turn = Orientation::from_yaw_pitch(step as f32 * 0.031, step as f32 * 0.017);
            let rounded = Orientation::from_single_components(turn.components_single()).unwrap();
            let (exact, single) = (world_vector(turn), world_vector(rounded));
            let miss = (0..3)
                .map(|axis| (exact[axis] - single[axis]) * (exact[axis] - single[axis]))
                .sum::<f64>()
                .sqrt();
            worst = worst.max(miss);
        }
        assert!(worst > 1.0, "an f32 turn missed by only {worst}");
        // And back: the point is found where it was.
        let back = pose(at(0, 0, 0, [0.0; 3]), exact_turn)
            .world_to_local(
                pose(at(0, 0, 0, [0.0; 3]), exact_turn)
                    .local_to_world(at_radius)
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(back.brick(), at_radius.brick());
        assert!(
            back.offset()
                .iter()
                .all(|component| component.abs() < 1e-3 || *component > 31.99)
        );
    }

    #[test]
    fn conversions_are_the_same_bit_for_bit_on_every_machine() {
        // Values written down once (checked against a separate calculation in Python, which reproduces the bits: the matrix of the `f64` orientation, then
        // the rounding to `f32` that `displaced` does before it carries into the next brick); a change in them is a change in the simulation.
        // (The third offset was 1077386288 when an orientation was `f32`: ten `f32` steps from the exact answer, because the orientation was rounded; and the first component of the direction was 880643037, a speck of about 1e-8, where the exact answer is 0: the
        // pitch and yaw leave the first row of the matrix with no y.)
        let grid = pose(
            at(3, -2, 5, [1.25, 30.5, 7.0]),
            Orientation::from_yaw_pitch(0.9, -0.4),
        );
        let world = grid
            .local_to_world(at(1, 2, -3, [4.5, 6.25, 0.75]))
            .unwrap();
        let direction = grid.direction_to_local([0.0, -32.0, 0.0]);
        let voxel = voxel_in_other(
            &grid,
            VoxelCoordinate::new(40, -17, 3),
            &GridPose::identity(),
        );
        assert_eq!(world.brick(), BrickCoordinate::new(0, -1, 2));
        assert_eq!(
            world.offset().map(f32::to_bits),
            [1106136256, 1104209478, 1077386272]
        );
        assert_eq!(direction.map(f32::to_bits), [0, 3253455527, 3242680791]);
        assert_eq!(voxel, Some(VoxelCoordinate::new(129, -48, 141)));
    }
}

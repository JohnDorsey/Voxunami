//! Property-test strategies for coordinates. Enabled by the `test-support` feature.

use proptest::prelude::*;

use crate::{
    BRICK_COORDINATE_MAXIMUM, BRICK_COORDINATE_MINIMUM, BRICK_EDGE_LENGTH, BRICK_VOXEL_COUNT,
    BrickCoordinate, LocalVoxelIndex, VOXEL_COORDINATE_MAXIMUM, VOXEL_COORDINATE_MINIMUM,
    VoxelCoordinate,
};

/// One voxel coordinate component, biased toward where bugs live: small values around zero, values
/// hugging brick boundaries, and the extremes of the valid range.
pub fn voxel_coordinate_component() -> impl Strategy<Value = i64> {
    let edge_length = BRICK_EDGE_LENGTH as i64;
    prop_oneof![
        4 => -100i64..100,
        3 => (-4i64..4, -1i64..=1).prop_map(move |(brick, delta)| brick * edge_length + delta),
        2 => VOXEL_COORDINATE_MINIMUM..=VOXEL_COORDINATE_MAXIMUM,
        1 => prop_oneof![
            Just(VOXEL_COORDINATE_MINIMUM),
            Just(VOXEL_COORDINATE_MINIMUM + 1),
            Just(VOXEL_COORDINATE_MAXIMUM - 1),
            Just(VOXEL_COORDINATE_MAXIMUM),
        ],
    ]
}

pub fn voxel_coordinate() -> impl Strategy<Value = VoxelCoordinate> {
    (
        voxel_coordinate_component(),
        voxel_coordinate_component(),
        voxel_coordinate_component(),
    )
        .prop_map(|(x, y, z)| VoxelCoordinate::new(x, y, z))
}

/// Coordinates within a few bricks of the origin, so random operations land in the same bricks
/// and the same voxels often enough to exercise overwriting.
pub fn nearby_voxel_coordinate() -> impl Strategy<Value = VoxelCoordinate> {
    (-70i64..70, -70i64..70, -70i64..70).prop_map(|(x, y, z)| VoxelCoordinate::new(x, y, z))
}

pub fn brick_coordinate() -> impl Strategy<Value = BrickCoordinate> {
    let component = || {
        prop_oneof![
            5 => -4i64..4,
            2 => BRICK_COORDINATE_MINIMUM..=BRICK_COORDINATE_MAXIMUM,
            1 => Just(BRICK_COORDINATE_MINIMUM),
            1 => Just(BRICK_COORDINATE_MAXIMUM),
        ]
    };
    (component(), component(), component()).prop_map(|(x, y, z)| BrickCoordinate::new(x, y, z))
}

pub fn local_voxel_index() -> impl Strategy<Value = LocalVoxelIndex> {
    (0..BRICK_VOXEL_COUNT).prop_map(|index| {
        LocalVoxelIndex::from_index(index).expect("strategy only generates indices in range")
    })
}

/// Any orientation a body can have: a unit quaternion made from random turns about the three axes.
pub fn orientation() -> impl Strategy<Value = crate::Orientation> {
    (-6.3f32..6.3, -1.5f32..1.5)
        .prop_map(|(yaw, pitch)| crate::Orientation::from_yaw_pitch(yaw, pitch))
}

/// A position anywhere in the supported range, biased like [`voxel_coordinate`].
pub fn world_position() -> impl Strategy<Value = crate::WorldPosition> {
    (voxel_coordinate(), 0.0f32..32.0, 0.0f32..32.0, 0.0f32..32.0)
        .prop_map(|(voxel, x, y, z)| crate::WorldPosition::new(voxel.split().0, [x, y, z]))
}

/// A pose in a root grid of one of the first few levels (what a saved world holds until ships are saved too).
pub fn entity_pose() -> impl Strategy<Value = crate::EntityPose> {
    (0u32..4, world_position(), orientation()).prop_map(|(level, position, orientation)| {
        crate::EntityPose::new(
            crate::GridId::root(crate::LevelIndex::new(level)),
            position,
            orientation,
        )
    })
}

/// A pose in the frame of a root grid or of one of the first few ships: where an entity can be in a running world.
pub fn entity_pose_in_any_frame() -> impl Strategy<Value = crate::EntityPose> {
    (
        prop_oneof![
            3 => (0u32..4).prop_map(|level| crate::GridId::root(crate::LevelIndex::new(level))),
            2 => (0u64..3).prop_map(|ship| crate::GridId::new(crate::GridId::FIRST_ALLOCATED + ship)),
        ],
        world_position(),
        orientation(),
    )
        .prop_map(|(grid, position, orientation)| {
            crate::EntityPose::new(grid, position, orientation)
        })
}

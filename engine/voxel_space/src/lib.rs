//! Integer coordinate systems for voxel grids.
//!
//! A grid is divided into cubic bricks of [`BRICK_EDGE_LENGTH`] voxels per side. A voxel is
//! addressed either by a [`VoxelCoordinate`] (grid-wide) or by a [`BrickCoordinate`] plus a
//! [`LocalVoxelIndex`] (position inside one brick); [`VoxelCoordinate::split`] converts.
//!
//! Coordinates are `i64` per axis, restricted to a range that leaves headroom so ordinary
//! arithmetic on valid coordinates cannot overflow. There is no practical world edge. A world has
//! several levels, each with its own grid, named by a [`LevelIndex`].

mod brick_box;
mod coordinates;
mod entity_pose;
mod face_direction;
mod grid_id;
mod level_index;
mod level_of_detail;
mod local_voxel_index;
mod orientation;
mod voxel_box;
mod world_position;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use brick_box::BrickBox;
pub use coordinates::{BrickCoordinate, CoordinateOutOfRange, VoxelCoordinate};
pub use entity_pose::EntityPose;
pub use face_direction::FaceDirection;
pub use grid_id::GridId;
pub use level_index::LevelIndex;
pub use level_of_detail::LevelOfDetail;
pub use local_voxel_index::LocalVoxelIndex;
pub use orientation::Orientation;
pub use voxel_box::VoxelBox;
pub use world_position::WorldPosition;

/// Base-2 logarithm of [`BRICK_EDGE_LENGTH`], used for shifts.
pub const BRICK_EDGE_LENGTH_LOG2: u32 = 5;

/// Voxels along each edge of a brick. 32 lets one row of a brick fit a `u32` bit mask.
pub const BRICK_EDGE_LENGTH: usize = 1 << BRICK_EDGE_LENGTH_LOG2;

/// Voxels in one brick.
pub const BRICK_VOXEL_COUNT: usize = BRICK_EDGE_LENGTH * BRICK_EDGE_LENGTH * BRICK_EDGE_LENGTH;

/// Smallest valid voxel coordinate component (inclusive).
pub const VOXEL_COORDINATE_MINIMUM: i64 = -(1 << 60);

/// Largest valid voxel coordinate component (inclusive).
pub const VOXEL_COORDINATE_MAXIMUM: i64 = (1 << 60) - 1;

/// Smallest valid brick coordinate component (inclusive).
pub const BRICK_COORDINATE_MINIMUM: i64 = VOXEL_COORDINATE_MINIMUM >> BRICK_EDGE_LENGTH_LOG2;

/// Largest valid brick coordinate component (inclusive).
pub const BRICK_COORDINATE_MAXIMUM: i64 = VOXEL_COORDINATE_MAXIMUM >> BRICK_EDGE_LENGTH_LOG2;

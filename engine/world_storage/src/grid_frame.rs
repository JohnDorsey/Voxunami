//! Where the voxels of a grid are in the world, whatever the grid is: the terrain of a level or a ship.
//!
//! A rule that asks a question about the world (is a player near this fire? is the sun on this voxel? how high is this block above the sea?)
//! has a voxel of some grid and needs a place in a level. For terrain that is the voxel itself; for a ship it is the voxel turned and placed
//! where the ship is. This is the one place that says so, so that a rule does not have to know which kind of grid it was given.

use voxel_space::{GridId, GridPose, LevelIndex, VoxelCoordinate, WorldPosition};

use crate::WorldData;

/// How a grid sits in the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridFrame {
    /// The terrain of a level: its voxels are the level's.
    Level(LevelIndex),
    /// A ship: its voxels are turned and placed by its pose, in the level it is in.
    Ship { level: LevelIndex, pose: GridPose },
}

impl GridFrame {
    /// The level the grid is in.
    pub fn level(&self) -> LevelIndex {
        match self {
            Self::Level(level) => *level,
            Self::Ship { level, .. } => *level,
        }
    }

    /// Where the middle of a voxel of the grid is in its level, or `None` if that is not a place the world can hold.
    pub fn centre_in_world(&self, voxel: VoxelCoordinate) -> Option<WorldPosition> {
        let (brick, local) = voxel.split();
        let centre = WorldPosition::try_new(
            brick,
            [local.x(), local.y(), local.z()].map(|axis| f32::from(axis) + 0.5),
        )?;
        match self {
            Self::Level(_) => Some(centre),
            Self::Ship { pose, .. } => pose.local_to_world(centre),
        }
    }
}

impl WorldData {
    /// How a grid sits in the world: `None` for a grid that is not there (a level that does not exist, a ship that is gone). A ship that is in a
    /// file is where it was put away.
    pub fn grid_frame(&self, grid: GridId) -> Option<GridFrame> {
        match grid.root_level() {
            Some(level) => self.level(level).map(|_| GridFrame::Level(level)),
            None => self
                .grids()
                .frame_of(grid)
                .map(|(level, pose)| GridFrame::Ship { level, pose }),
        }
    }

    /// The level and the place in it of the middle of a voxel of any grid.
    pub fn voxel_in_world(
        &self,
        grid: GridId,
        voxel: VoxelCoordinate,
    ) -> Option<(LevelIndex, WorldPosition)> {
        let frame = self.grid_frame(grid)?;
        Some((frame.level(), frame.centre_in_world(voxel)?))
    }
}

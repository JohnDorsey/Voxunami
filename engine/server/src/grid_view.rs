use voxel_grid::ChannelIndex;
use voxel_space::{GridId, VoxelCoordinate};

use crate::server_world::{AccessError, ServerWorld};

/// What a rule is given to read and write one grid, in the grid's own coordinates.
///
/// A rule written against this sees one grid and nothing of the world around it, so the same rule
/// works in any grid, however it is placed and whether or not it moves. There is deliberately no
/// call that returns or scans a whole grid: a grid may be huge, so access is by voxel (and, as
/// rules need it, by region or by the voxels still to look at).
pub trait GridView {
    /// Which grid this is.
    fn id(&self) -> GridId;

    /// The value of a voxel in a channel.
    fn get(&mut self, channel: ChannelIndex, at: VoxelCoordinate) -> Result<u32, AccessError>;

    /// Sets a voxel and replicates the change to the clients watching its brick. Returns whether
    /// the voxel changed; setting it to the value it has changes and sends nothing.
    fn set(
        &mut self,
        channel: ChannelIndex,
        at: VoxelCoordinate,
        value: u32,
    ) -> Result<bool, AccessError>;
}

/// One grid of the server's world, as [`GridView`]: the terrain of a level or a ship.
#[derive(Debug)]
pub struct ServerGrid<'a> {
    world: &'a mut ServerWorld,
    grid: GridId,
}

impl ServerWorld {
    /// A grid of this world, or an error if the world has no such grid.
    pub fn grid(&mut self, grid: GridId) -> Result<ServerGrid<'_>, AccessError> {
        let exists = match grid.root_level() {
            Some(level) => self.data().level(level).is_some(),
            None => self.data().grids().get(grid).is_some(),
        };
        if !exists {
            return Err(AccessError::NoSuchGrid(grid));
        }
        Ok(ServerGrid { world: self, grid })
    }
}

impl GridView for ServerGrid<'_> {
    fn id(&self) -> GridId {
        self.grid
    }

    fn get(&mut self, channel: ChannelIndex, at: VoxelCoordinate) -> Result<u32, AccessError> {
        match self.grid.root_level() {
            Some(level) => self.world.voxel(level, channel, at),
            None => self.world.moving_voxel(self.grid, channel, at),
        }
    }

    fn set(
        &mut self,
        channel: ChannelIndex,
        at: VoxelCoordinate,
        value: u32,
    ) -> Result<bool, AccessError> {
        match self.grid.root_level() {
            Some(level) => self.world.set_voxel(level, channel, at, value),
            None => self.world.set_moving_voxel(self.grid, channel, at, value),
        }
    }
}

use voxel_grid::ChannelIndex;
use voxel_space::{BrickCoordinate, GridId, VoxelCoordinate};

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

    /// The value that every voxel of a brick has in a channel, if they all have the same one: a way to know what a brick holds without looking at
    /// its voxels. A brick that nothing has been built in is every voxel at what the grid has there to begin with (a ship's default; the terrain
    /// generator's value).
    fn uniform_value(
        &mut self,
        channel: ChannelIndex,
        brick: BrickCoordinate,
    ) -> Result<Option<u32>, AccessError>;

    /// The value a voxel has to begin with, whatever has been built there since: the terrain generator's for the terrain of a level (a way to tell
    /// what a player built from the ground it stands on), and the channel's default for a ship, which nothing generates.
    fn generated(&mut self, channel: ChannelIndex, at: VoxelCoordinate)
    -> Result<u32, AccessError>;
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
        match grid.root_level() {
            Some(level) if self.data().level(level).is_none() => {
                return Err(AccessError::NoSuchLevel(level));
            }
            None if self.data().grids().get(grid).is_none() => {
                return Err(AccessError::NoSuchGrid(grid));
            }
            _ => {}
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
            Some(level) => self.world.root_voxel(level, channel, at),
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
            Some(level) => self.world.set_root_voxel(level, channel, at, value),
            None => self.world.set_moving_voxel(self.grid, channel, at, value),
        }
    }

    fn uniform_value(
        &mut self,
        channel: ChannelIndex,
        brick: BrickCoordinate,
    ) -> Result<Option<u32>, AccessError> {
        match self.grid.root_level() {
            Some(level) => self.world.root_uniform_value(level, channel, brick),
            None => {
                let ship = self
                    .world
                    .data()
                    .grids()
                    .get(self.grid)
                    .ok_or(AccessError::NoSuchGrid(self.grid))?;
                Ok(match ship.voxels.brick(brick) {
                    Some(contents) => contents.channel(channel).uniform_value(),
                    None => Some(ship.voxels.layout().default_value(channel)),
                })
            }
        }
    }

    fn generated(
        &mut self,
        channel: ChannelIndex,
        at: VoxelCoordinate,
    ) -> Result<u32, AccessError> {
        match self.grid.root_level() {
            Some(level) => self.world.root_generated_voxel(level, channel, at),
            None => {
                let ship = self
                    .world
                    .data()
                    .grids()
                    .get(self.grid)
                    .ok_or(AccessError::NoSuchGrid(self.grid))?;
                Ok(ship.voxels.layout().default_value(channel))
            }
        }
    }
}

//! The world as the rules see it: through the context of the system running them, one grid at a time,
//! so that what a system reads and writes is checked against what it declared (in debug builds).

use plugin_host::Context;
use server::GridView;
use voxel_grid::ChannelIndex;
use voxel_space::{GridId, VoxelCoordinate};

use crate::fire::FireWorld;
use crate::lighting::LightWorld;
use crate::multiscale_light::{self, LightScheme, ScaledLightWorld};

/// The channels of a Cave world that its rules use.
#[derive(Clone, Debug)]
pub(super) struct CaveChannels {
    pub block: ChannelIndex,
    pub state: ChannelIndex,
    pub orientation: ChannelIndex,
    pub light: ChannelIndex,
    pub scales: Vec<ChannelIndex>,
}

impl CaveChannels {
    pub fn of(content: &crate::content::CaveContent) -> Self {
        Self {
            block: content.block_channel(),
            state: content.state_channel(),
            orientation: content.orientation_channel(),
            light: content.light_channel(),
            scales: (0..multiscale_light::MAXIMUM_SCALES)
                .map(|scale| content.scale_channel(scale))
                .collect(),
        }
    }
}

/// The block and state of a voxel of a grid, or `None` where there is nothing to read.
fn block_and_state_of(
    context: &mut Context<'_>,
    channels: &CaveChannels,
    grid: GridId,
    at: VoxelCoordinate,
) -> Option<(u32, u32)> {
    let mut grid = context.grid(grid).ok()?;
    let block = grid.get(channels.block, at).ok()?;
    let state = grid.get(channels.state, at).ok()?;
    Some((block, state))
}

/// The orientation of a voxel of a grid (0 where there is nothing to read).
fn orientation_of(
    context: &mut Context<'_>,
    channels: &CaveChannels,
    grid: GridId,
    at: VoxelCoordinate,
) -> u32 {
    context
        .grid(grid)
        .ok()
        .and_then(|mut grid| grid.get(channels.orientation, at).ok())
        .unwrap_or(0)
}

/// The world as the classic light rule sees it. Terrain this server cannot generate has nothing to
/// read, and lets no light through.
pub(super) struct WorldLight<'a, 'b> {
    pub context: &'a mut Context<'b>,
    pub channels: &'a CaveChannels,
}

impl LightWorld for WorldLight<'_, '_> {
    fn block_and_state(&mut self, grid: GridId, at: VoxelCoordinate) -> Option<(u32, u32)> {
        block_and_state_of(self.context, self.channels, grid, at)
    }

    fn orientation_at(&mut self, grid: GridId, at: VoxelCoordinate) -> u32 {
        orientation_of(self.context, self.channels, grid, at)
    }

    fn light(&mut self, grid: GridId, at: VoxelCoordinate) -> u8 {
        self.context
            .grid(grid)
            .and_then(|mut grid| grid.get(self.channels.light, at))
            .map_or(0, |light| light.min(255) as u8)
    }

    fn set_light(&mut self, grid: GridId, at: VoxelCoordinate, value: u8) {
        // The voxel was just read, so the grid and the terrain there are available.
        let _ = self
            .context
            .grid(grid)
            .and_then(|mut grid| grid.set(self.channels.light, at, u32::from(value)));
    }

    fn is_simulated(&mut self, grid: GridId) -> bool {
        self.context.world().grid_is_simulated(grid)
    }
}

/// The world as the multiscale light sees it.
pub(super) struct WorldScaledLight<'a, 'b> {
    pub context: &'a mut Context<'b>,
    pub channels: &'a CaveChannels,
    pub scheme: LightScheme,
}

impl ScaledLightWorld for WorldScaledLight<'_, '_> {
    fn block_and_state(&mut self, grid: GridId, at: VoxelCoordinate) -> Option<(u32, u32)> {
        block_and_state_of(self.context, self.channels, grid, at)
    }

    fn orientation_at(&mut self, grid: GridId, at: VoxelCoordinate) -> u32 {
        orientation_of(self.context, self.channels, grid, at)
    }

    fn word(&mut self, scale: usize, grid: GridId, anchor: VoxelCoordinate) -> u32 {
        self.context
            .grid(grid)
            .and_then(|mut grid| grid.get(self.channels.scales[scale], anchor))
            .unwrap_or(0)
    }

    fn set_word(&mut self, scale: usize, grid: GridId, anchor: VoxelCoordinate, value: u32) {
        // The cell's anchor was just read, so the grid and the terrain there are available. A
        // cell in more than one brick has its word in each (see `word_homes`); a brick that cannot be
        // written is not drawn from it either.
        let Ok(mut grid) = self.context.grid(grid) else {
            return;
        };
        for home in multiscale_light::word_homes(self.scheme.width(scale), anchor) {
            let _ = grid.set(self.channels.scales[scale], home, value);
        }
    }

    fn is_simulated(&mut self, grid: GridId) -> bool {
        self.context.world().grid_is_simulated(grid)
    }
}

/// The world as the fire rule sees it.
pub(super) struct WorldFire<'a, 'b> {
    pub context: &'a mut Context<'b>,
    pub channels: &'a CaveChannels,
}

impl FireWorld for WorldFire<'_, '_> {
    fn block_and_state(&mut self, grid: GridId, at: VoxelCoordinate) -> Option<(u32, u32)> {
        block_and_state_of(self.context, self.channels, grid, at)
    }

    fn set_block_and_state(&mut self, grid: GridId, at: VoxelCoordinate, block: u32, state: u32) {
        // The voxel was just read, so the grid and the terrain there are available.
        let Ok(mut grid) = self.context.grid(grid) else {
            return;
        };
        let _ = grid.set(self.channels.state, at, state);
        let _ = grid.set(self.channels.block, at, block);
    }
}

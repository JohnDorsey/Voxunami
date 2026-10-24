//! Meshes the bricks of a planet (a ship with terrain) around a viewer: the bricks the ship's generator makes, with the ones the server stored over them, in the
//! ship's own coordinates and under the ship's pose.
//!
//! It is a [`WorldMesher`] reading from a ship instead of a level (see [`TerrainSource`]), and saying what it draws as bricks of the ship. A brick outside the
//! extent of the generator is empty, and the face of a brick against it is drawn, so the edge of a planet that is cut off shows its side.

use std::sync::Arc;

use block_models::BlockTable;
use terrain::TerrainGenerator;
use voxel_grid::{Brick, Grid, GridLayout};
use voxel_space::{BrickBox, BrickCoordinate, GridId, VoxelCoordinate};

use crate::mesher::{MeshUpdate, TerrainSource, ViewChannels, ViewSettings, WorldMesher};

/// The bricks of one ship, as the client has them: the stored ones, and the generator for the rest within the extent.
#[derive(Clone)]
pub struct ShipTerrainSource<'a> {
    voxels: &'a Grid,
    generator: Option<Arc<dyn TerrainGenerator>>,
    extent: BrickBox,
}

impl std::fmt::Debug for ShipTerrainSource<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ShipTerrainSource")
            .field("stored", &self.voxels.brick_count())
            .field("has_generator", &self.generator.is_some())
            .field("extent", &self.extent)
            .finish()
    }
}

impl<'a> ShipTerrainSource<'a> {
    /// `voxels` are the bricks the server sent for the ship; `generator` makes the rest of the bricks in `extent` (`None` if this program lacks it, and then
    /// nothing that needs it can be drawn).
    pub fn new(
        voxels: &'a Grid,
        generator: Option<Arc<dyn TerrainGenerator>>,
        extent: BrickBox,
    ) -> Self {
        Self {
            voxels,
            generator,
            extent,
        }
    }
}

impl TerrainSource for ShipTerrainSource<'_> {
    fn known(&self, coordinate: BrickCoordinate) -> Option<Arc<Brick>> {
        if let Some(stored) = self.voxels.brick(coordinate) {
            return Some(Arc::clone(stored));
        }
        // Beyond what the generator makes there is nothing.
        (!self.extent.contains(coordinate)).then(|| Arc::new(Brick::new(self.voxels.layout())))
    }

    fn generator_for(&self, _: BrickCoordinate) -> Option<Arc<dyn TerrainGenerator>> {
        self.generator.clone()
    }

    fn layout(&self) -> &Arc<GridLayout> {
        self.voxels.layout()
    }
}

/// Keeps the bricks of one planet around the viewer generated and meshed, on background threads.
pub struct ShipTerrainMesher {
    grid: GridId,
    mesher: WorldMesher,
}

impl std::fmt::Debug for ShipTerrainMesher {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ShipTerrainMesher")
            .field("grid", &self.grid)
            .field("mesher", &self.mesher)
            .finish()
    }
}

impl ShipTerrainMesher {
    pub fn new(
        grid: GridId,
        settings: ViewSettings,
        channels: ViewChannels,
        table: Arc<BlockTable>,
    ) -> Self {
        Self {
            grid,
            mesher: WorldMesher::new(settings, channels, table),
        }
    }

    /// What the mesher made for a brick of the ship, as an update of the ship's bricks.
    fn of_the_ship(&self, update: MeshUpdate) -> MeshUpdate {
        match update {
            MeshUpdate::Upload { coordinate, mesh } => MeshUpdate::UploadGridBrick {
                grid: self.grid,
                coordinate,
                mesh,
            },
            MeshUpdate::Remove(coordinate) => MeshUpdate::RemoveGridBrick {
                grid: self.grid,
                coordinate,
            },
            other => unreachable!("a mesher of bricks makes nothing else: {other:?}"),
        }
    }

    fn of_the_ship_all(&self, updates: Vec<MeshUpdate>) -> Vec<MeshUpdate> {
        updates
            .into_iter()
            .map(|update| self.of_the_ship(update))
            .collect()
    }

    /// Collects finished work and schedules more around `center`, a brick of the ship (the viewer's, in the ship's coordinates).
    pub fn update(
        &mut self,
        source: &ShipTerrainSource<'_>,
        center: BrickCoordinate,
    ) -> Vec<MeshUpdate> {
        let updates = self.mesher.update(source, center);
        self.of_the_ship_all(updates)
    }

    /// Updates until no work is pending or schedulable, returning every change. For tests and headless rendering.
    pub fn settle(
        &mut self,
        source: &ShipTerrainSource<'_>,
        center: BrickCoordinate,
    ) -> Vec<MeshUpdate> {
        let updates = self.mesher.settle(source, center);
        self.of_the_ship_all(updates)
    }

    /// Tells the mesher a stored brick of the ship arrived or changed, so it and its neighbours are meshed again.
    pub fn brick_changed(&mut self, brick: BrickCoordinate) {
        self.mesher.brick_changed(brick);
    }

    /// Changes what is drawn from, as [`WorldMesher::set_channels`] does.
    pub fn set_channels(&mut self, channels: ViewChannels) {
        self.mesher.set_channels(channels);
    }

    /// Forgets every brick and all work in progress, and takes down every mesh that was shown.
    pub fn reset(&mut self) -> Vec<MeshUpdate> {
        let updates = self.mesher.reset();
        self.of_the_ship_all(updates)
    }

    /// The block in a voxel of the ship, if its brick is loaded.
    pub fn material_at(&self, voxel: VoxelCoordinate) -> Option<u32> {
        self.mesher.material_at(voxel)
    }

    /// The state of the block in a voxel of the ship (0 where the game has no states), if its brick is loaded.
    pub fn state_at(&self, voxel: VoxelCoordinate) -> Option<u32> {
        self.mesher.state_at(voxel)
    }

    /// Whether a voxel of the ship holds something that fills any part of it, if its brick is loaded.
    pub fn is_solid_at(&self, voxel: VoxelCoordinate) -> Option<bool> {
        self.mesher.is_solid_at(voxel)
    }

    /// Whether there is nothing more to make or to put on the screen.
    pub fn is_settled(&self) -> bool {
        self.mesher.pending_jobs() == 0
    }

    /// Bricks of the ship held in memory (whether or not drawn).
    pub fn loaded_brick_count(&self) -> usize {
        self.mesher.loaded_brick_count()
    }
}

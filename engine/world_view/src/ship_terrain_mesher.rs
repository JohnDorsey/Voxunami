//! Meshes the terrain of a planet (a ship with terrain) around a viewer: the bricks the ship's generator makes, with the ones the server stored over them, in the
//! ship's own coordinates and under the ship's pose; and, if it is to be drawn in detail, the nodes of the generator beyond the bricks.
//!
//! It is a [`WorldMesher`] (or a [`LodView`] over one) reading from a ship instead of a level (see [`TerrainSource`] and [`CoarseTerrainSource`]), and saying what it
//! draws as parts of the ship. A brick outside the extent of the generator is empty, and the face of a brick against it is drawn, so the edge of a planet that is cut
//! off shows its side. The nodes of a planet are not corrected by the server (edits show in the bricks round the viewer, and not in the nodes farther away).

use std::sync::Arc;

use block_models::BlockTable;
use region_octree::BrickCube;
use terrain::{SolidBall, TerrainGenerator};
use voxel_grid::{Brick, Grid, GridLayout};
use voxel_space::{BrickBox, BrickCoordinate, GridId, VoxelCoordinate, WorldPosition};

use crate::lod::{CoarseTerrainSource, LodConfig, LodSettings, LodView};
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

/// Whether any brick of the cube is inside the extent.
fn meets(extent: BrickBox, cube: BrickCube) -> bool {
    let origin = cube.origin();
    let last = i64::try_from(cube.edge_length() - 1).unwrap_or(i64::MAX);
    let far = |coordinate: i64| coordinate.saturating_add(last);
    BrickBox::new(
        origin,
        BrickCoordinate::new(far(origin.x()), far(origin.y()), far(origin.z())),
    )
    .is_some_and(|whole| extent.intersection(whole).is_some())
}

impl CoarseTerrainSource for ShipTerrainSource<'_> {
    /// (The generator and the extent of a planet do not change, so the answers never go stale.)
    fn version(&self) -> u64 {
        0
    }

    /// Nothing outside the extent is drawn, and nothing at all of a planet whose generator this program lacks (the server does not correct the nodes of a planet,
    /// so no edit makes something to see where the generator would not).
    fn may_show(&self, cube: BrickCube) -> bool {
        self.generator
            .as_ref()
            .is_some_and(|generator| meets(self.extent, cube) && generator.may_show(cube))
    }

    fn coarse_generator(&self, cube: BrickCube) -> Option<Arc<dyn TerrainGenerator>> {
        self.generator.clone().filter(|_| meets(self.extent, cube))
    }

    fn solid_core(&self) -> Option<SolidBall> {
        self.generator.as_ref()?.solid_core()
    }
}

/// How a planet is drawn.
enum Drawn {
    /// The bricks around the viewer and nothing beyond.
    Bricks(Box<WorldMesher>),
    /// Bricks near and the generator's nodes beyond, as far and as fine as the settings say.
    Detailed {
        view: Box<LodView>,
        settings: LodSettings,
    },
}

/// Keeps the terrain of one planet around the viewer generated and meshed, on background threads.
pub struct ShipTerrainMesher {
    grid: GridId,
    drawn: Drawn,
}

impl std::fmt::Debug for ShipTerrainMesher {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = formatter.debug_struct("ShipTerrainMesher");
        debug.field("grid", &self.grid);
        match &self.drawn {
            Drawn::Bricks(mesher) => debug.field("mesher", mesher),
            Drawn::Detailed { view, .. } => debug.field("view", view),
        };
        debug.finish()
    }
}

impl ShipTerrainMesher {
    /// A mesher of the bricks round the viewer and nothing beyond them.
    pub fn new(
        grid: GridId,
        settings: ViewSettings,
        channels: ViewChannels,
        table: Arc<BlockTable>,
    ) -> Self {
        Self {
            grid,
            drawn: Drawn::Bricks(Box::new(WorldMesher::new(settings, channels, table))),
        }
    }

    /// A mesher that draws what is beyond the bricks as the nodes of the planet's generator as well, as far and as fine as `detail` says (see
    /// [`ShipTerrainMesher::set_detail`]).
    pub fn with_detail(
        grid: GridId,
        settings: ViewSettings,
        channels: ViewChannels,
        table: Arc<BlockTable>,
        config: LodConfig,
        detail: LodSettings,
    ) -> Self {
        Self {
            grid,
            drawn: Drawn::Detailed {
                view: Box::new(LodView::new(
                    WorldMesher::new(settings, channels, table),
                    config,
                )),
                settings: detail,
            },
        }
    }

    /// Changes how fine and how far the planet is drawn, from the next update. A mesher of the bricks alone has no detail to change.
    pub fn set_detail(&mut self, detail: LodSettings) {
        if let Drawn::Detailed { settings, .. } = &mut self.drawn {
            *settings = detail;
        }
    }

    fn bricks(&self) -> &WorldMesher {
        match &self.drawn {
            Drawn::Bricks(mesher) => mesher,
            Drawn::Detailed { view, .. } => view.mesher(),
        }
    }

    fn bricks_mut(&mut self) -> &mut WorldMesher {
        match &mut self.drawn {
            Drawn::Bricks(mesher) => mesher,
            Drawn::Detailed { view, .. } => view.mesher_mut(),
        }
    }

    /// What the mesher made for a part of the ship, as an update of the ship's.
    fn of_the_ship(&self, update: MeshUpdate) -> MeshUpdate {
        let grid = self.grid;
        match update {
            MeshUpdate::Upload { coordinate, mesh } => MeshUpdate::UploadGridBrick {
                grid,
                coordinate,
                mesh,
            },
            MeshUpdate::Remove(coordinate) => MeshUpdate::RemoveGridBrick { grid, coordinate },
            MeshUpdate::UploadCoarse { cube, mesh } => {
                MeshUpdate::UploadGridCoarse { grid, cube, mesh }
            }
            MeshUpdate::RemoveCoarse(cube) => MeshUpdate::RemoveGridCoarse { grid, cube },
            other => unreachable!("a mesher of terrain makes nothing else: {other:?}"),
        }
    }

    fn of_the_ship_all(&self, updates: Vec<MeshUpdate>) -> Vec<MeshUpdate> {
        updates
            .into_iter()
            .map(|update| self.of_the_ship(update))
            .collect()
    }

    /// Collects finished work and schedules more around `viewer`, a place in the ship (the viewer's, in the ship's coordinates).
    pub fn update(
        &mut self,
        source: &ShipTerrainSource<'_>,
        viewer: WorldPosition,
    ) -> Vec<MeshUpdate> {
        let updates = match &mut self.drawn {
            Drawn::Bricks(mesher) => mesher.update(source, viewer.brick()),
            Drawn::Detailed { view, settings } => view.update(self.grid, source, viewer, settings),
        };
        self.of_the_ship_all(updates)
    }

    /// Updates until no work is pending or schedulable, returning every change. For tests and headless rendering.
    pub fn settle(
        &mut self,
        source: &ShipTerrainSource<'_>,
        viewer: WorldPosition,
    ) -> Vec<MeshUpdate> {
        let updates = match &mut self.drawn {
            Drawn::Bricks(mesher) => mesher.settle(source, viewer.brick()),
            Drawn::Detailed { view, settings } => view.settle(self.grid, source, viewer, settings),
        };
        self.of_the_ship_all(updates)
    }

    /// Tells the mesher a stored brick of the ship arrived or changed, so it and its neighbours are meshed again.
    pub fn brick_changed(&mut self, brick: BrickCoordinate) {
        self.bricks_mut().brick_changed(brick);
    }

    /// Changes what is drawn from, as [`WorldMesher::set_channels`] does.
    pub fn set_channels(&mut self, channels: ViewChannels) {
        self.bricks_mut().set_channels(channels);
    }

    /// Forgets every brick and all work in progress, and takes down every mesh that was shown.
    pub fn reset(&mut self) -> Vec<MeshUpdate> {
        let updates = match &mut self.drawn {
            Drawn::Bricks(mesher) => mesher.reset(),
            Drawn::Detailed { view, .. } => view.reset(),
        };
        self.of_the_ship_all(updates)
    }

    /// The block in a voxel of the ship, if its brick is loaded.
    pub fn material_at(&self, voxel: VoxelCoordinate) -> Option<u32> {
        self.bricks().material_at(voxel)
    }

    /// The state of the block in a voxel of the ship (0 where the game has no states), if its brick is loaded.
    pub fn state_at(&self, voxel: VoxelCoordinate) -> Option<u32> {
        self.bricks().state_at(voxel)
    }

    /// Whether a voxel of the ship holds something that fills any part of it, if its brick is loaded.
    pub fn is_solid_at(&self, voxel: VoxelCoordinate) -> Option<bool> {
        self.bricks().is_solid_at(voxel)
    }

    /// Whether there is nothing more to make or to put on the screen.
    pub fn is_settled(&self) -> bool {
        match &self.drawn {
            Drawn::Bricks(mesher) => mesher.pending_jobs() == 0,
            Drawn::Detailed { view, .. } => view.is_settled(),
        }
    }

    /// Bricks of the ship held in memory (whether or not drawn).
    pub fn loaded_brick_count(&self) -> usize {
        self.bricks().loaded_brick_count()
    }
}

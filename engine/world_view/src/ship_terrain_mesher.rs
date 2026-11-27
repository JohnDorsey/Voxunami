//! Meshes the terrain of a planet (a ship with terrain) around a viewer: the bricks the ship's generator makes, with the ones the server stored over them, in the
//! ship's own coordinates and under the ship's pose; and, if it is to be drawn in detail, the nodes of the generator beyond the bricks.
//!
//! It is a [`WorldMesher`] (or a [`LodView`] over one) reading from a ship instead of a level (see [`TerrainSource`] and [`CoarseTerrainSource`]), and saying what it
//! draws as parts of the ship. A brick outside the extent of the generator is empty, and the face of a brick against it is drawn, so the edge of a planet that is cut
//! off shows its side. The server corrects the nodes of a planet where it was edited (`ShipTerrainMesher::set_override`); the rest are the generator's.

use std::sync::Arc;

use block_models::BlockTable;
use coarse_cells::CoarseNode;
use region_octree::BrickCube;
use terrain::{SolidBall, TerrainGenerator};
use voxel_grid::{Brick, Grid, GridLayout};
use voxel_space::{BrickBox, BrickCoordinate, GridId, VoxelCoordinate, WorldPosition};

use crate::lod::{
    CoarseTerrainSource, LodConfig, LodSettings, LodView, MayShowOracle, Stalled, Viewpoint,
};
use crate::mesher::{MeshUpdate, TerrainSource, ViewChannels, ViewSettings, WorldMesher};
use crate::occupancy::{OccupancyChange, OccupancyStatistics};

/// The bricks of one ship, as the client has them: the stored ones, and the generator for the rest within the extent.
#[derive(Clone)]
pub struct ShipTerrainSource<'a> {
    voxels: &'a Grid,
    generator: Option<Arc<dyn TerrainGenerator>>,
    extent: BrickBox,
    /// Whether the ship is far (see [`ShipTerrainSource::far`]).
    far: bool,
}

impl std::fmt::Debug for ShipTerrainSource<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ShipTerrainSource")
            .field("stored", &self.voxels.brick_count())
            .field("has_generator", &self.generator.is_some())
            .field("extent", &self.extent)
            .field("far", &self.far)
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
            far: false,
        }
    }

    /// A ship that is far away: it has no brick here and no generator, and its picture is the nodes the server sends (see [`ShipTerrainMesher::set_override`]) and
    /// nothing else. `bounds` are the bricks the ship occupies, in its own coordinates; no cube that does not meet them can show anything.
    pub fn far(voxels: &'a Grid, bounds: BrickBox) -> Self {
        Self {
            voxels,
            generator: None,
            extent: bounds,
            far: true,
        }
    }
}

impl TerrainSource for ShipTerrainSource<'_> {
    fn known(&self, coordinate: BrickCoordinate) -> Option<Arc<Brick>> {
        if let Some(stored) = self.voxels.brick(coordinate) {
            return Some(Arc::clone(stored));
        }
        // A far ship has no brick anywhere: what is not drawn as a node is not drawn.
        if self.far {
            return Some(Arc::new(Brick::new(self.voxels.layout())));
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

impl CoarseTerrainSource for ShipTerrainSource<'_> {
    /// (The generator and the extent of a planet do not change, so the answers never go stale. The box of a far ship does: an edit makes the ship bigger, and the
    /// server tells of it again with the new box.)
    fn version(&self) -> u64 {
        if self.far {
            return version_of_box(self.extent);
        }
        0
    }

    fn detached_may_show(&self) -> Option<MayShowOracle> {
        let (generator, extent, far) = (self.generator.clone(), self.extent, self.far);
        Some(Arc::new(move |cube| {
            if far {
                return cube.meets(extent);
            }
            generator
                .as_ref()
                .is_some_and(|generator| cube.meets(extent) && generator.may_show(cube))
        }))
    }

    /// Nothing outside the extent is drawn, and nothing at all of a planet whose generator this program lacks (a node the server corrected is kept by the view whatever the
    /// generator says: an edit can make something to see where the generator would not).
    fn may_show(&self, cube: BrickCube) -> bool {
        if self.far {
            return cube.meets(self.extent);
        }
        self.generator
            .as_ref()
            .is_some_and(|generator| cube.meets(self.extent) && generator.may_show(cube))
    }

    fn coarse_generator(&self, cube: BrickCube) -> Option<Arc<dyn TerrainGenerator>> {
        self.generator.clone().filter(|_| cube.meets(self.extent))
    }

    fn solid_core(&self) -> Option<SolidBall> {
        self.generator.as_ref()?.solid_core()
    }

    /// A far ship has no generator: what it is drawn from is what the server says (see [`ShipTerrainSource::far`]).
    fn waits_for_nodes(&self) -> bool {
        self.far
    }
}

/// A number that is the same for the same box and, but for a chance in 2^64, another for another: the version of the answers of a far ship (FNV-1a over the six coordinates).
fn version_of_box(bounds: BrickBox) -> u64 {
    let (low, high) = (bounds.min(), bounds.max());
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for value in [low.x(), low.y(), low.z(), high.x(), high.y(), high.z()] {
        for byte in value.to_le_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
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

    /// Makes the mesher say which voxels of each brick of the planet block the sun (see [`WorldMesher::enable_occupancy`]); call it before the first update.
    #[must_use]
    pub fn with_occupancy(mut self) -> Self {
        self.bricks_mut().enable_occupancy();
        self
    }

    /// What has changed in the occupancy of the planet's bricks since the last call (see [`WorldMesher::take_occupancy_updates`]); the bricks are in the ship's own
    /// coordinates, and the ship is [`ShipTerrainMesher::grid`].
    pub fn take_occupancy_updates(&mut self) -> Vec<(BrickCoordinate, OccupancyChange)> {
        self.bricks_mut().take_occupancy_updates()
    }

    /// How many of the bricks that were told of are empty, full and mixed.
    pub fn occupancy_statistics(&self) -> OccupancyStatistics {
        self.bricks().occupancy_statistics()
    }

    /// The ship this mesher makes the terrain of.
    pub fn grid(&self) -> GridId {
        self.grid
    }

    /// The node the server says is the node of a cube of the planet instead of the generator's (a `None` takes back what was said), see
    /// [`LodView::set_override`]. A mesher of the bricks alone draws no nodes, so there is nothing to change.
    pub fn set_override(&mut self, cube: BrickCube, node: Option<CoarseNode>) {
        if let Drawn::Detailed { view, .. } = &mut self.drawn {
            view.set_override(self.grid, cube, node);
        }
    }

    /// Keeps the pictures of the nodes that leave the screen, up to `budget_bytes` of them (see [`LodView::hold_hidden_pictures`]). A mesher of the bricks alone has no
    /// nodes to keep.
    pub fn hold_hidden_pictures(&mut self, budget_bytes: usize) {
        if let Drawn::Detailed { view, .. } = &mut self.drawn {
            view.hold_hidden_pictures(budget_bytes);
        }
    }

    /// Keeps the planet ready to be drawn from `viewer`, a place in the ship's coordinates, as well as from where it is updated (see [`LodView::set_warm_viewpoint`]), at the
    /// detail the planet is drawn at. A mesher of the bricks alone has no nodes to keep ready.
    pub fn keep_ready(&mut self, viewer: Option<WorldPosition>) {
        if let Drawn::Detailed { view, settings } = &mut self.drawn {
            view.set_warm_viewpoint(viewer.map(|viewer| Viewpoint {
                viewer,
                settings: *settings,
            }));
        }
    }

    /// Says whether the pictures made for a place kept ready are let go of when it is taken away ([`LodView::set_release_warm_pictures`]). A mesher of the bricks alone keeps
    /// no place ready.
    pub fn set_release_warm_pictures(&mut self, release: bool) {
        if let Drawn::Detailed { view, .. } = &mut self.drawn {
            view.set_release_warm_pictures(release);
        }
    }

    /// Changes how fine and how far the planet is drawn, from the next update. A mesher of the bricks alone has no detail to change.
    /// How far the viewer may go from where the nodes were chosen before they are chosen again ([`LodView::set_reselect_tolerance`]); nothing for a ship drawn in bricks only.
    pub fn set_reselect_tolerance(&mut self, tolerance: f64) {
        if let Drawn::Detailed { view, .. } = &mut self.drawn {
            view.set_reselect_tolerance(tolerance);
        }
    }

    /// The brick, in the ship's own coordinates, that the nodes drawn were chosen for ([`LodView::wanted_centre`]); `None` for a ship drawn in bricks only, or before the first.
    pub fn selected_centre(&self) -> Option<BrickCoordinate> {
        match &self.drawn {
            Drawn::Detailed { view, .. } => view.wanted_centre(),
            Drawn::Bricks(_) => None,
        }
    }

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
            MeshUpdate::HideCoarse(cube) => MeshUpdate::HideGridCoarse { grid, cube },
            MeshUpdate::ShowCoarse(cube) => MeshUpdate::ShowGridCoarse { grid, cube },
            MeshUpdate::UploadHiddenCoarse { cube, mesh } => {
                MeshUpdate::UploadHiddenGridCoarse { grid, cube, mesh }
            }
            MeshUpdate::UploadHiddenBrick { coordinate, mesh } => {
                MeshUpdate::UploadHiddenGridBrick {
                    grid,
                    coordinate,
                    mesh,
                }
            }
            MeshUpdate::HideBrick(coordinate) => MeshUpdate::HideGridBrick { grid, coordinate },
            MeshUpdate::ShowBrick(coordinate) => MeshUpdate::ShowGridBrick { grid, coordinate },
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

    /// What to say, once, when the planet's detailed view has come to rest without drawing what it wants ([`LodView::take_stall_report`]); always `None` for a mesher of
    /// the bricks alone.
    pub fn take_stall_report(&mut self) -> Option<String> {
        match &mut self.drawn {
            Drawn::Bricks(_) => None,
            Drawn::Detailed { view, .. } => view.take_stall_report(),
        }
    }

    /// The cubes that are wanted and not drawn, as of the last time the planet's view came to rest ([`LodView::stalled`]); always empty for a mesher of the bricks
    /// alone.
    pub fn stalled(&self) -> &[Stalled] {
        match &self.drawn {
            Drawn::Bricks(_) => &[],
            Drawn::Detailed { view, .. } => view.stalled(),
        }
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

    /// The orientation of the block in a voxel of the ship (0 where the game has none), if its brick is loaded.
    pub fn orientation_at(&self, voxel: VoxelCoordinate) -> Option<u32> {
        self.bricks().orientation_at(voxel)
    }

    /// Whether a voxel of the ship holds something that fills any part of it, if its brick is loaded.
    pub fn is_solid_at(&self, voxel: VoxelCoordinate) -> Option<bool> {
        self.bricks().is_solid_at(voxel)
    }

    /// Whether there is nothing more to make or to put on the screen.
    pub fn is_settled(&self) -> bool {
        match &self.drawn {
            Drawn::Bricks(mesher) => mesher.is_settled(),
            Drawn::Detailed { view, .. } => view.is_settled(),
        }
    }

    /// Bricks of the ship held in memory (whether or not drawn).
    pub fn loaded_brick_count(&self) -> usize {
        self.bricks().loaded_brick_count()
    }
}

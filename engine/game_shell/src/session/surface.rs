//! What the crosshair points at: the voxel (`Session::target`), and the place on the block's surface (`Session::target_surface`), which a tool that paints on a face needs and a
//! beam that pushes a ship at a point will.

use block_models::{FacePicture, PicturePlace, Shape};
use transport::Connection;
use voxel_grid::Grid;
use voxel_space::{GridId, VoxelCoordinate, WorldPosition};
use world_view::{RayHit, ShipTerrainMesher, SurfaceRayHit, cast_ray, cast_surface};

use super::Session;
use crate::definition::GameDefinition;

/// What is in a voxel: the block, its state and its orientation (0 where the game has none).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoxelContent {
    pub block: u32,
    pub state: u32,
    pub orientation: u32,
}

/// The picture on the face the crosshair meets, and where on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PictureTarget {
    pub picture: FacePicture,
    pub place: PicturePlace,
}

/// The place on a block's surface under the crosshair: not only the voxel, but the side of what the block fills of it that the ray meets, the point on it, and the picture
/// that is drawn there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceTarget {
    pub hit: SurfaceRayHit,
    pub content: VoxelContent,
    /// The picture on the face that was hit, and the place on it; `None` where the face is a flat colour.
    pub picture: Option<PictureTarget>,
}

/// Where the voxels of a grid are read from.
enum VoxelSource<'a> {
    /// The terrain of the level the player is in.
    Terrain,
    /// A planet: what its generator makes where the server has not stored a brick, which the mesher around the camera has.
    Planet(&'a ShipTerrainMesher),
    /// A ship: the voxels the server sent. What was not sent is empty.
    Ship(&'a Grid),
}

/// A grid the crosshair's ray goes through, with the ray in the grid's own coordinates.
struct AimedGrid<'a> {
    grid: GridId,
    start: WorldPosition,
    direction: [f64; 3],
    source: VoxelSource<'a>,
}

impl<C: Connection, G: GameDefinition> Session<C, G> {
    /// The grids the crosshair's ray can meet: the terrain of the level, then each ship of the level (and each planet, which is one), the ray turned into the grid's own
    /// coordinates.
    fn aimed_grids(&self, forward: [f64; 3]) -> impl Iterator<Item = AimedGrid<'_>> {
        let level = self.level();
        let terrain = AimedGrid {
            grid: GridId::root(level),
            start: self.camera.position,
            direction: forward,
            source: VoxelSource::Terrain,
        };
        let ships = self
            .client
            .world()
            .grids()
            .iter()
            .filter(move |(_, ship)| ship.level == level)
            .filter_map(move |(id, ship)| {
                let start = ship.pose.world_to_local(self.camera.position)?;
                Some(AimedGrid {
                    grid: id,
                    start,
                    direction: ship.pose.direction_to_local(forward),
                    source: match self.planets.get(&id) {
                        Some(planet) => VoxelSource::Planet(planet),
                        None => VoxelSource::Ship(&ship.voxels),
                    },
                })
            });
        std::iter::once(terrain).chain(ships)
    }

    /// Whether a voxel of a source holds anything that fills any part of it, or `None` where the world is not loaded.
    fn is_solid_in(&self, source: &VoxelSource<'_>, voxel: VoxelCoordinate) -> Option<bool> {
        match source {
            VoxelSource::Terrain => self.mesher().is_solid_at(voxel),
            VoxelSource::Planet(planet) => planet.is_solid_at(voxel),
            VoxelSource::Ship(voxels) => Some(self.ship_voxel_is_solid(voxels, voxel)),
        }
    }

    /// What is in a voxel of a source, or `None` where the world is not loaded.
    fn content_in(&self, source: &VoxelSource<'_>, voxel: VoxelCoordinate) -> Option<VoxelContent> {
        match source {
            VoxelSource::Terrain => {
                let mesher = self.mesher();
                Some(VoxelContent {
                    block: mesher.material_at(voxel)?,
                    state: mesher.state_at(voxel)?,
                    orientation: mesher.orientation_at(voxel)?,
                })
            }
            VoxelSource::Planet(planet) => Some(VoxelContent {
                block: planet.material_at(voxel)?,
                state: planet.state_at(voxel)?,
                orientation: planet.orientation_at(voxel)?,
            }),
            VoxelSource::Ship(voxels) => Some(VoxelContent {
                block: voxels.get(self.view_channels.blocks, voxel),
                state: self
                    .view_channels
                    .states
                    .map_or(0, |channel| voxels.get(channel, voxel)),
                orientation: self
                    .view_channels
                    .orientations
                    .map_or(0, |channel| voxels.get(channel, voxel)),
            }),
        }
    }

    /// The solid voxel under the crosshair, within the game's reach: of the terrain of the level, or of a ship, whichever the ray reaches first. A ship is aimed at by
    /// turning the ray into the ship's own coordinates.
    pub fn target(&self) -> Option<RayHit> {
        self.aimed_grids(self.camera.forward().to_array().map(f64::from))
            .filter_map(|aimed| {
                cast_ray(
                    aimed.grid,
                    aimed.start,
                    aimed.direction,
                    G::reach(),
                    |voxel| self.is_solid_in(&aimed.source, voxel),
                )
            })
            .min_by(|first, second| first.distance.total_cmp(&second.distance))
    }

    /// The place on a surface under the crosshair, within the game's reach, as [`Session::target`] finds a voxel but for what the block fills of it: a ray goes over a slab
    /// and meets the block behind it, and meets a stair's riser inside the voxel. The point is in the voxel's own coordinates, and the picture is the one the renderer draws
    /// on that face, with the place on it (it is where the true surface is, which with a relief (prisms or parallax) is not always the pixel the eye sees at a slant).
    pub fn target_surface(&self) -> Option<SurfaceTarget> {
        self.target_surface_toward(self.camera.forward().to_array().map(f64::from))
    }

    /// As [`Session::target_surface`], along another ray from the camera: `direction` (in the world, not necessarily a unit vector) is where a pointer that is not the
    /// crosshair looks (see [`renderer::Camera::direction_through`]).
    pub fn target_surface_toward(&self, direction: [f64; 3]) -> Option<SurfaceTarget> {
        let hit = self
            .aimed_grids(direction)
            .filter_map(|aimed| {
                cast_surface(
                    aimed.grid,
                    aimed.start,
                    aimed.direction,
                    G::reach(),
                    |voxel| self.shape_in(&aimed.source, voxel),
                )
                .map(|hit| (hit, aimed))
            })
            .min_by(|(first, _), (second, _)| {
                first.surface.distance.total_cmp(&second.surface.distance)
            })?;
        let (hit, aimed) = hit;
        let content = self.content_in(&aimed.source, hit.voxel)?;
        let picture = self
            .block_table
            .face_picture(
                content.block,
                content.state,
                content.orientation,
                hit.surface.face,
            )
            .map(|picture| PictureTarget {
                picture,
                place: picture.place_of(hit.surface.point),
            });
        Some(SurfaceTarget {
            hit,
            content,
            picture,
        })
    }

    /// The shape of what is in a voxel of a source: `Some(None)` where there is nothing, `None` where the world is not loaded.
    fn shape_in(&self, source: &VoxelSource<'_>, voxel: VoxelCoordinate) -> Option<Option<&Shape>> {
        let content = self.content_in(source, voxel)?;
        let model = self
            .block_table
            .model_of(content.block, content.state, content.orientation);
        Some(
            self.block_table
                .is_solid(model)
                .then(|| self.block_table.shape(model)),
        )
    }
}

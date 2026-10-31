//! Terrain for the tests of what the engine does with a density field (layering, coarse cells, drawing): hills over a flat base with sheet
//! caves. It is a fixture and not content: a game's own generators are the game's. Enabled by the `test-support` feature.

use std::sync::Arc;

use coarse_cells::CoarseNode;
use region_octree::BrickCube;
use voxel_grid::{Brick, ChannelIndex, GridLayout};
use voxel_space::{BrickCoordinate, LevelOfDetail, VoxelBox, VoxelCoordinate};

use crate::density::{DensityField, Heightfield, Minimum, ShellCaves};
use crate::generator::{
    FieldTerrainGenerator, SURFACE_LAYER_DEPTH, SolidBall, SurfaceLayers, TerrainGenerator,
};
use crate::interval::Interval;
use crate::noise::{Dimensionality, FractalNoise};

/// Parameters of the rolling-hills fixture. All lengths are in voxels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RollingHillsSettings {
    pub seed: u64,
    /// Surface height where the hills contribute nothing.
    pub surface_height: f64,
    /// Height contributed by the largest hills (the sum over all octaves can reach about twice this).
    pub hill_height: f64,
    pub largest_hill_wavelength_log2: u8,
    pub hill_octave_count: u8,
    pub cave_wavelength_log2: u8,
    /// Half the thickness of cave sheets, in noise units (the noise ranges over `[-1, 1]`).
    pub cave_half_thickness: f64,
}

impl RollingHillsSettings {
    /// Sensible terrain for 1 metre voxels: hills a few hundred metres wide and tens of metres
    /// tall over a surface near height 64.
    pub fn for_one_metre_voxels(seed: u64) -> Self {
        Self {
            seed,
            surface_height: 64.0,
            hill_height: 40.0,
            largest_hill_wavelength_log2: 7,
            hill_octave_count: 6,
            cave_wavelength_log2: 5,
            cave_half_thickness: 0.08,
        }
    }
}

/// The density field of [`rolling_hills`]: hills over a flat base, riddled with thin sheet-like caves.
pub fn hills_field(settings: RollingHillsSettings) -> Arc<dyn DensityField> {
    let ground = Heightfield::new(
        settings.surface_height,
        FractalNoise::standard(
            settings.seed,
            Dimensionality::Planar,
            settings.largest_hill_wavelength_log2,
            settings.hill_octave_count,
            settings.hill_height,
            0.5,
        ),
    );
    let caves = ShellCaves::new(
        FractalNoise::standard(
            settings.seed ^ 0xCA7E_5CA7_E5CA_7E5C,
            Dimensionality::Volumetric,
            settings.cave_wavelength_log2,
            2,
            1.0,
            0.5,
        ),
        settings.cave_half_thickness,
        16.0,
    );
    Arc::new(Minimum {
        first: ground,
        second: caves,
    })
}

/// Hills over a flat base, riddled with thin sheet-like caves, as a generator.
pub fn rolling_hills(
    settings: RollingHillsSettings,
    material_channel: ChannelIndex,
    layers: SurfaceLayers,
) -> FieldTerrainGenerator {
    FieldTerrainGenerator::new(hills_field(settings), material_channel, layers)
}

/// Ground that is a ball about the origin (the corner where eight voxels meet): the density of a voxel is how far inside the surface its middle is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BallField {
    pub radius: f64,
}

impl BallField {
    fn distance_range(region: &VoxelBox) -> (f64, f64) {
        // The middles of the voxels of the box, per axis; the nearest and the farthest point of that box from the origin.
        let (mut nearest, mut farthest) = (0.0f64, 0.0f64);
        for (low, high) in [
            (region.min().x(), region.max().x()),
            (region.min().y(), region.max().y()),
            (region.min().z(), region.max().z()),
        ] {
            let (low, high) = (low as f64 + 0.5, high as f64 + 0.5);
            let closest = if low > 0.0 {
                low
            } else if high < 0.0 {
                -high
            } else {
                0.0
            };
            nearest += closest * closest;
            let furthest = low.abs().max(high.abs());
            farthest += furthest * furthest;
        }
        (nearest.sqrt(), farthest.sqrt())
    }
}

impl DensityField for BallField {
    fn sample(&self, position: VoxelCoordinate, _footprint: LevelOfDetail) -> f64 {
        let (x, y, z) = (
            position.x() as f64 + 0.5,
            position.y() as f64 + 0.5,
            position.z() as f64 + 0.5,
        );
        self.radius - (x * x + y * y + z * z).sqrt()
    }

    fn bounds(&self, region: &VoxelBox, _footprint: LevelOfDetail) -> Interval {
        let (nearest, farthest) = Self::distance_range(region);
        // A little loose, as bounds may be, for the rounding of the distance.
        Interval::new(self.radius - farthest - 1e-6, self.radius - nearest + 1e-6)
    }

    // The ball has no detail finer than a voxel to ignore: what holds at a level holds at every finer one.
    fn bounds_through_finer(&self, region: &VoxelBox, footprint: LevelOfDetail) -> Interval {
        self.bounds(region, footprint)
    }
}

/// A [`FieldTerrainGenerator`] of a [`BallField`] that says what a planet's generator says: the ball inside it is ground all through.
#[derive(Clone, Debug)]
pub struct BallTerrain {
    inner: FieldTerrainGenerator,
    radius: f64,
}

/// A ball of ground of this radius about the origin, for the tests of how a planet is drawn.
pub fn ball(radius: f64, material_channel: ChannelIndex, layers: SurfaceLayers) -> BallTerrain {
    BallTerrain {
        inner: FieldTerrainGenerator::new(Arc::new(BallField { radius }), material_channel, layers),
        radius,
    }
}

impl TerrainGenerator for BallTerrain {
    fn generate_brick(&self, layout: &GridLayout, coordinate: BrickCoordinate) -> Brick {
        self.inner.generate_brick(layout, coordinate)
    }

    fn coarse_node(&self, cube: BrickCube, unlit: u8) -> Option<CoarseNode> {
        self.inner.coarse_node(cube, unlit)
    }

    fn may_show(&self, cube: BrickCube) -> bool {
        self.inner.may_show(cube)
    }

    fn solid_core(&self) -> Option<SolidBall> {
        // The layers are four voxels deep, and the corner of the box a brick is is one more from the middle of a voxel.
        Some(SolidBall {
            centre: [0.0; 3],
            radius: (self.radius - SURFACE_LAYER_DEPTH as f64 - 2.0).max(0.0),
        })
    }
}

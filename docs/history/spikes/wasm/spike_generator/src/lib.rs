//! Cave's rolling hills (`games/voxunami_cave/src/generators/hills.rs`, copied: that crate cannot be built for wasm, and it only uses the public types of `terrain`),
//! behind one function that fills a buffer with the materials of a brick. Native and wasm run this same code.

use std::sync::Arc;

use terrain::{
    Dimensionality, FieldTerrainGenerator, FractalNoise, Heightfield, Minimum, ShellCaves,
    SurfaceLayers, TerrainGenerator,
};
use voxel_grid::{ChannelDefinition, ChannelIndex, GridLayout};
use voxel_space::{BRICK_VOXEL_COUNT, BrickCoordinate};

/// Cave's `RollingHillsSettings::for_one_metre_voxels`, in the fields that matter.
const SURFACE_HEIGHT: f64 = 64.0;
const HILL_HEIGHT: f64 = 40.0;
const LARGEST_HILL_WAVELENGTH_LOG2: u8 = 7;
const HILL_OCTAVE_COUNT: u8 = 6;
const CAVE_WAVELENGTH_LOG2: u8 = 5;
const CAVE_HALF_THICKNESS: f64 = 0.08;

#[derive(Debug)]
pub struct SpikeGenerator {
    generator: FieldTerrainGenerator,
    layout: GridLayout,
    material: ChannelIndex,
}

impl SpikeGenerator {
    pub fn new(seed: u64) -> Self {
        let layout = GridLayout::new(vec![ChannelDefinition::number(
            "spike:material".parse().expect("valid identifier"),
            0,
        )])
        .expect("one channel");
        let material = layout
            .channel_index(&"spike:material".parse().expect("valid identifier"))
            .expect("channel exists");
        let ground = Heightfield::new(
            SURFACE_HEIGHT,
            FractalNoise::standard(
                seed,
                Dimensionality::Planar,
                LARGEST_HILL_WAVELENGTH_LOG2,
                HILL_OCTAVE_COUNT,
                HILL_HEIGHT,
                0.5,
            ),
        );
        let caves = ShellCaves::new(
            FractalNoise::standard(
                seed ^ 0xCA7E_5CA7_E5CA_7E5C,
                Dimensionality::Volumetric,
                CAVE_WAVELENGTH_LOG2,
                2,
                1.0,
                0.5,
            ),
            CAVE_HALF_THICKNESS,
            16.0,
        );
        let generator = FieldTerrainGenerator::new(
            Arc::new(Minimum {
                first: ground,
                second: caves,
            }),
            material,
            SurfaceLayers {
                air: 0,
                surface: 3,
                subsurface: 2,
                deep: 1,
            },
        );
        Self {
            generator,
            layout,
            material,
        }
    }

    /// The materials of the brick, in voxel index order, into `out` (`BRICK_VOXEL_COUNT` long).
    pub fn materials(&self, x: i32, y: i32, z: i32, out: &mut [u32]) {
        assert_eq!(out.len(), BRICK_VOXEL_COUNT);
        let brick = self.generator.generate_brick(
            &self.layout,
            BrickCoordinate::new(i128::from(x), i128::from(y), i128::from(z)),
        );
        for (slot, value) in out.iter_mut().zip(brick.channel(self.material).iter()) {
            *slot = value;
        }
    }
}

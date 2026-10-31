//! Cave's overworld: hills over a flat base, riddled with thin sheet-like caves.

use std::sync::Arc;

use terrain::{
    Dimensionality, FieldTerrainGenerator, FractalNoise, Heightfield, Minimum, ShellCaves,
    SurfaceLayers,
};
use voxel_grid::ChannelIndex;

/// Parameters of Cave's rolling-hills terrain. All lengths are in voxels.
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

/// Hills over a flat base, riddled with thin sheet-like caves.
///
/// The output for given settings is part of a world's file format and must never change; see
/// [`TerrainGenerator`]. Register a changed design under a new version name instead.
pub fn rolling_hills(
    settings: RollingHillsSettings,
    material_channel: ChannelIndex,
    layers: SurfaceLayers,
) -> FieldTerrainGenerator {
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
    FieldTerrainGenerator::new(
        Arc::new(Minimum {
            first: ground,
            second: caves,
        }),
        material_channel,
        layers,
    )
}

#[cfg(test)]
mod tests {
    use terrain::TerrainGenerator;
    use voxel_grid::test_support::StandardTestLayout;
    use voxel_space::BrickCoordinate;

    use super::*;

    const LAYERS: SurfaceLayers = SurfaceLayers {
        air: 0,
        deep: 1,
        subsurface: 2,
        surface: 3,
    };

    /// Output of the default rolling hills for a fixed seed. These values are the file format:
    /// if one changes, existing worlds change under their players. They were produced by this
    /// code, so they pin behaviour; they are not an independent check of it. The bricks are
    /// chosen to contain real surface or caves, including negative and enormous coordinates.
    #[test]
    fn rolling_hills_output_is_pinned() {
        let fixture = StandardTestLayout::default();
        let generator = rolling_hills(
            RollingHillsSettings::for_one_metre_voxels(2024),
            fixture.material,
            LAYERS,
        );
        let pinned_bricks = [
            (
                BrickCoordinate::new(0, 0, 0),
                PINNED_ROLLING_HILLS_DIGESTS[0],
            ),
            (
                BrickCoordinate::new(0, 3, 0),
                PINNED_ROLLING_HILLS_DIGESTS[1],
            ),
            (
                BrickCoordinate::new(-1, 3, -1),
                PINNED_ROLLING_HILLS_DIGESTS[2],
            ),
            (
                BrickCoordinate::new(7, 1, -12),
                PINNED_ROLLING_HILLS_DIGESTS[3],
            ),
            (
                BrickCoordinate::new(1_000_000_000, 1, 5),
                PINNED_ROLLING_HILLS_DIGESTS[4],
            ),
            (
                BrickCoordinate::new(-33, 2, 21),
                PINNED_ROLLING_HILLS_DIGESTS[5],
            ),
            (
                BrickCoordinate::new(123_456_789_012, 2, -987_654_321),
                PINNED_ROLLING_HILLS_DIGESTS[6],
            ),
        ];
        let mut observed = Vec::new();
        for (coordinate, _) in pinned_bricks {
            let brick = generator.generate_brick(&fixture.layout, coordinate);
            assert_eq!(
                brick.channel(fixture.material).uniform_value(),
                None,
                "{coordinate} should contain terrain detail"
            );
            observed.push(brick.content_digest());
        }
        let expected: Vec<u64> = pinned_bricks.iter().map(|&(_, digest)| digest).collect();
        assert_eq!(observed, expected);
    }

    const PINNED_ROLLING_HILLS_DIGESTS: [u64; 7] = [
        10_435_167_514_397_377_490,
        15_902_999_568_092_474_262,
        9_435_966_543_601_840_145,
        10_910_340_570_707_222_884,
        18_170_788_201_477_264_472,
        17_677_368_896_373_949_775,
        13_609_564_394_996_896_000,
    ];
}

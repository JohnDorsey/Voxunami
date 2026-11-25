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
/// [`terrain::TerrainGenerator`]. Register a changed design under a new version name instead.
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
    // Seen from far away it is the hills without their caves: the sheets of air under them are twelve times the quads of the hills, invisible from above, and what makes a node
    // of them take 94 to 201 ms against 0.3 for a node of the height of the surface alone (docs/specs/lod-spec.md 9a, viewpoints-spec.md 6.6 A).
    FieldTerrainGenerator::new(
        Arc::new(Minimum {
            first: ground.clone(),
            second: caves,
        }),
        material_channel,
        layers,
    )
    .with_far_field(Arc::new(ground))
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
    /// The cubes a view draws at a place on the hills, split by level: nothing under the ground is drawn as a node, because a node is drawn from the hills without
    /// their caves and nobody sees into those (the sheet caves could be anywhere under them, which once made 38,000 of 46,000 cubes buried ground).
    #[test]
    fn a_view_of_the_overworld_selects_cubes_on_the_ground_and_none_deep_under_it() {
        use std::collections::BTreeMap;

        use coarse_cells::{DetailSettings, ViewSelection, select_around_brick};

        let fixture = StandardTestLayout::default();
        let generator = rolling_hills(
            RollingHillsSettings::for_one_metre_voxels(2024),
            fixture.material,
            LAYERS,
        );
        let view = ViewSelection {
            detail: DetailSettings {
                cell_pixels: 3.0,
                pixel_size: 2.0 * 0.7002 / 1080.0,
            },
            zoom: None,
            fine_radius: 8,
            maximum_distance: 10_000.0,
        };
        let cubes = select_around_brick(BrickCoordinate::new(0, 3, 0), &view, &|cube| {
            generator.may_show(cube)
        });
        let mut deep_nodes = 0usize;
        let mut per_level: BTreeMap<u8, usize> = BTreeMap::new();
        for cube in cubes.iter().filter(|cube| cube.level() > 0) {
            *per_level.entry(cube.level()).or_default() += 1;
            let top = cube.origin().origin().y() + (cube.edge_length() * 32) as i128;
            // (The lowest the hills go is 64 - 80 = -16, and a cube is chosen with one beside it.)
            if top <= -(2 * (cube.edge_length() * 32) as i128) {
                deep_nodes += 1;
            }
        }
        assert_eq!(deep_nodes, 0, "{per_level:?}");
        // At most about 3,400 cubes a level (level 1, measured: 13,006 in all), where it was 11,000 with 46,110 in all.
        for (level, count) in &per_level {
            assert!(*count < 4_000, "level {level}: {count}");
        }
        assert!(cubes.len() < 16_000, "{}", cubes.len());
    }

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

    fn overworld_generator(fixture: &StandardTestLayout) -> terrain::FieldTerrainGenerator {
        rolling_hills(
            RollingHillsSettings::for_one_metre_voxels(2024),
            fixture.material,
            LAYERS,
        )
    }

    fn node_of(
        generator: &impl TerrainGenerator,
        brick: [i128; 3],
        level: u8,
    ) -> coarse_cells::CoarseNode {
        let cube = region_octree::BrickCube::containing(
            BrickCoordinate::new(brick[0], brick[1], brick[2]),
            level,
        );
        generator
            .coarse_node(cube, 0)
            .expect("a node of a cube above level 0")
    }

    /// The overworld seen from far away is its hills without their caves (docs/specs/lod-spec.md 9a, viewpoints-spec.md 6.6 A): the sheets of air under the hills are
    /// twelve times the quads of the hills themselves and are not to be seen from above. So the ground of every column of a node is whole: full below the surface, then
    /// the cell the surface goes through, then air, and no air under ground.
    #[test]
    fn the_coarse_nodes_of_the_overworld_have_no_caves_though_its_bricks_do() {
        let fixture = StandardTestLayout::default();
        let generator = overworld_generator(&fixture);
        // The bricks have them: air under the ground in a column of voxels, somewhere among the bricks under the hills.
        let mut caves_in_bricks = false;
        for y in 0..3 {
            for x in 0..4 {
                let brick =
                    generator.generate_brick(&fixture.layout, BrickCoordinate::new(x, y, 0));
                caves_in_bricks |= (0..32u8).any(|column| {
                    let mut seen_air_under_ground = false;
                    let mut ground_above = false;
                    for height in (0..32u8).rev() {
                        let solid = brick.get(
                            fixture.material,
                            voxel_space::LocalVoxelIndex::new(column, height, 3),
                        ) != LAYERS.air;
                        if solid {
                            ground_above = true;
                        } else if ground_above {
                            seen_air_under_ground = true;
                        }
                    }
                    seen_air_under_ground
                });
            }
        }
        assert!(
            caves_in_bricks,
            "the bricks under the hills have no caves: the test looks in the wrong place"
        );

        let mut nodes_with_a_surface = 0;
        for (level, edge) in [(1u8, 2i128), (2, 4), (3, 8)] {
            for x in [-2, 0, 3] {
                for y in 0..=(4 / edge + 1) {
                    let node = node_of(&generator, [x * edge, y * edge, 0], level);
                    if node.uniform_cell().is_some() {
                        continue;
                    }
                    nodes_with_a_surface += 1;
                    for z in 0..32u8 {
                        for column in 0..32u8 {
                            let mut last = u8::MAX;
                            for height in 0..32u8 {
                                let fill = node
                                    .cell(voxel_space::LocalVoxelIndex::new(column, height, z))
                                    .fill;
                                assert!(
                                    fill <= last,
                                    "level {level}, node ({x}, {y}): column ({column}, {z}) is fuller at {height} than under it: {fill} over {last}"
                                );
                                last = fill;
                            }
                        }
                    }
                }
            }
        }
        assert!(
            nodes_with_a_surface > 3,
            "{nodes_with_a_surface} nodes had a surface in them"
        );
    }

    /// Like the bricks of a generator, its coarse nodes are not to change once a recording of them could be played again: these are the fingerprint, from the code that
    /// made them (they pin behaviour; they are not an independent check of it). If this fails, add a new generator version instead of changing the output.
    #[test]
    fn the_coarse_nodes_of_the_overworld_are_pinned() {
        let fixture = StandardTestLayout::default();
        let generator = overworld_generator(&fixture);
        let digests: Vec<u64> = [
            ([0, 0, 0], 1u8),
            ([8, 0, -8], 3),
            ([64, 0, 64], 6),
            ([0, 0, 0], 9),
            ([0, -512, 0], 9),
            ([-512, 0, 512], 9),
            ([1_000_000_000_000, 0, -3], 20),
        ]
        .into_iter()
        .map(|(brick, level)| terrain::node_digest(&node_of(&generator, brick, level)))
        .collect();
        assert_eq!(digests, PINNED_OVERWORLD_NODE_DIGESTS);
    }

    const PINNED_OVERWORLD_NODE_DIGESTS: [u64; 7] = [
        17_679_892_460_646_433_895,
        5_245_355_481_354_334_154,
        1_133_631_287_852_297_237,
        // (The octaves of the hills are 128 voxels across and more: from level 8 up the hills are not in a node, and the same ground is the same node anywhere.)
        9_261_627_657_816_933_157,
        17_360_563_479_208_760_101,
        9_261_627_657_816_933_157,
        157_173_885_870_648_101,
    ];

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

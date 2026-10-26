use std::fmt::Debug;
use std::sync::Arc;

use coarse_cells::CoarseNode;
use region_octree::BrickCube;
use voxel_grid::{Brick, ChannelIndex, ChannelStorage, GridLayout};
use voxel_space::{
    BRICK_EDGE_LENGTH, BRICK_VOXEL_COUNT, BrickCoordinate, LevelOfDetail, LocalVoxelIndex, VoxelBox,
};

use crate::density::{DensityField, Heightfield, Minimum, ShellCaves};
use crate::noise::{Dimensionality, FractalNoise};

/// A ball, in the coordinates of a terrain generator, that is ground all through (caves apart): nothing can be seen through it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolidBall {
    pub centre: [f64; 3],
    pub radius: f64,
}

/// Produces the bricks of a world from nothing but a seed baked into the generator.
///
/// A generator is part of the world's file format: a world regenerates unmodified terrain from it
/// every time it is loaded, on every client. Once a world has used a generator, its output for any
/// brick must never change; improve terrain by adding a new generator, not by editing one.
pub trait TerrainGenerator: Debug + Send + Sync {
    /// The brick at `coordinate`, with every channel of `layout` set.
    fn generate_brick(&self, layout: &GridLayout, coordinate: BrickCoordinate) -> Brick;

    /// The coarse cells of a cube of bricks (see `coarse_cells`): what the terrain of the cube is drawn as from far away, in a node of 32 x 32 x 32
    /// cells of `2^level` voxels, where `level` is the cube's. `unlit` is the brightness of terrain no light reaches, which the caller knows and
    /// the generator does not. `None` if the generator does not say (the default, and always for a cube of level 0, which is a brick).
    ///
    /// Like [`TerrainGenerator::generate_brick`] this is part of a world's file format for as long as a recording can be replayed: a generator that has
    /// said something about its coarse cells never says anything else. It is approximate: it is not the summary of the bricks under it.
    fn coarse_node(&self, cube: BrickCube, unlit: u8) -> Option<CoarseNode> {
        let _ = (cube, unlit);
        None
    }

    /// Whether drawing the cube, or anything inside it at any level of detail, could show something: `false` for a cube that is all air and for one that is ground
    /// all through with ground all round it (nobody can see into ground). A cube that is not drawn at all is neither generated, meshed, sent nor held, which is
    /// most of a view. `true` when unsure, which is the default.
    ///
    /// This is not part of the file format of the world as the bricks are: it only says what need not be drawn. A generator whose answer is wrong shows too little or too much.
    fn may_show(&self, cube: BrickCube) -> bool {
        let _ = cube;
        true
    }

    /// A ball that is ground all through, if the generator has one: a planet has its core. What is behind it is hidden from a viewer outside it, so a view need not
    /// draw it. `None`, the default, when there is none or the generator does not say.
    ///
    /// Like [`TerrainGenerator::may_show`] this is not part of the file format: it only says what need not be drawn, and a ball with air in it hides too much.
    fn solid_core(&self) -> Option<SolidBall> {
        None
    }
}

/// Why a family cannot make a generator from the parameters it was given.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct FamilyError(String);

impl FamilyError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// A kind of terrain generator whose settings are data: code that makes a [`TerrainGenerator`] from parameters, which are bytes whose form the family
/// decides. The family is a file format like a generator is (the same parameters always make the same generator, on every machine, for as long as a world
/// has used it); a world keeps a *recipe* (an instance name, a family and the parameters) and the generator is made from it when the world is loaded.
pub trait GeneratorFamily: Debug + Send + Sync {
    /// The generator these parameters make, or why they make none.
    fn build(&self, parameters: &[u8]) -> Result<Arc<dyn TerrainGenerator>, FamilyError>;
}

/// The material values a terrain generator writes into the material channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainMaterials {
    pub air: u32,
    pub stone: u32,
    pub dirt: u32,
    pub grass: u32,
}

/// Voxels of dirt-or-grass above the stone: grass on top, then dirt, then stone from this depth.
pub(crate) const SURFACE_LAYER_DEPTH: usize = 4;

/// Turns a [`DensityField`] into bricks: solid voxels become grass under open air, dirt within
/// three voxels of the surface above them, and stone deeper down.
///
/// Surface layering looks along +y, so this generator assumes y is "up" in its own world.
#[derive(Clone, Debug)]
pub struct FieldTerrainGenerator {
    field: Arc<dyn DensityField>,
    material_channel: ChannelIndex,
    materials: TerrainMaterials,
}

impl FieldTerrainGenerator {
    pub fn new(
        field: Arc<dyn DensityField>,
        material_channel: ChannelIndex,
        materials: TerrainMaterials,
    ) -> Self {
        Self {
            field,
            material_channel,
            materials,
        }
    }
}

impl TerrainGenerator for FieldTerrainGenerator {
    fn generate_brick(&self, layout: &GridLayout, coordinate: BrickCoordinate) -> Brick {
        let detail = LevelOfDetail::FULL_DETAIL;
        let mut brick = Brick::new(layout);
        let region = VoxelBox::of_brick(coordinate);

        // Bounds classify most bricks (sky and deep underground) without sampling a single voxel.
        if self.field.bounds(&region, detail).upper() <= 0.0 {
            brick
                .channel_mut(self.material_channel)
                .fill(self.materials.air);
            return brick;
        }
        let with_surface_layer = region.extended_upper(0, SURFACE_LAYER_DEPTH as i64, 0);
        if self.field.bounds(&with_surface_layer, detail).lower() > 0.0 {
            brick
                .channel_mut(self.material_channel)
                .fill(self.materials.stone);
            return brick;
        }

        let origin = coordinate.origin();
        let mut materials = vec![self.materials.air; BRICK_VOXEL_COUNT];
        let mut solid = [false; BRICK_EDGE_LENGTH + SURFACE_LAYER_DEPTH];
        for z in 0..BRICK_EDGE_LENGTH {
            for x in 0..BRICK_EDGE_LENGTH {
                // One column, including the voxels above the brick that decide the surface layers.
                for (height, is_solid) in solid.iter_mut().enumerate() {
                    *is_solid = origin
                        .checked_offset(x as i64, height as i64, z as i64)
                        .is_some_and(|position| self.field.sample(position, detail) > 0.0);
                }
                for y in 0..BRICK_EDGE_LENGTH {
                    if !solid[y] {
                        continue;
                    }
                    let solid_voxels_above = solid[y + 1..=y + SURFACE_LAYER_DEPTH]
                        .iter()
                        .take_while(|&&is_solid| is_solid)
                        .count();
                    let local = LocalVoxelIndex::new(x as u8, y as u8, z as u8);
                    materials[local.index()] = match solid_voxels_above {
                        0 => self.materials.grass,
                        depth if depth < SURFACE_LAYER_DEPTH => self.materials.dirt,
                        _ => self.materials.stone,
                    };
                }
            }
        }
        *brick.channel_mut(self.material_channel) = ChannelStorage::from_values(&materials);
        brick
    }

    fn coarse_node(&self, cube: BrickCube, unlit: u8) -> Option<CoarseNode> {
        crate::coarse::coarse_node_of_field(&*self.field, self.materials, cube, unlit)
    }

    fn may_show(&self, cube: BrickCube) -> bool {
        crate::coarse::cube_may_show(&*self.field, cube)
    }
}

/// Parameters of the rolling-hills terrain. All lengths are in voxels.
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
    materials: TerrainMaterials,
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
        materials,
    )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use proptest::prelude::*;
    use voxel_grid::test_support::StandardTestLayout;
    use voxel_space::{VoxelCoordinate, test_support::local_voxel_index};

    use super::*;
    use crate::interval::Interval;

    const MATERIALS: TerrainMaterials = TerrainMaterials {
        air: 0,
        stone: 1,
        dirt: 2,
        grass: 3,
    };

    fn flat_ground_generator(height: f64, fixture: &StandardTestLayout) -> FieldTerrainGenerator {
        FieldTerrainGenerator::new(
            Arc::new(Heightfield::new(height, FractalNoise::none())),
            fixture.material,
            MATERIALS,
        )
    }

    fn material_at(brick: &Brick, fixture: &StandardTestLayout, x: u8, y: u8, z: u8) -> u32 {
        brick.get(fixture.material, LocalVoxelIndex::new(x, y, z))
    }

    #[test]
    fn flat_ground_has_grass_on_top_then_three_dirt_then_stone() {
        let fixture = StandardTestLayout::default();
        let generator = flat_ground_generator(20.0, &fixture);
        let brick = generator.generate_brick(&fixture.layout, BrickCoordinate::new(0, 0, 0));

        // Solid voxels are those with y < 20.
        assert_eq!(material_at(&brick, &fixture, 5, 19, 5), MATERIALS.grass);
        for y in [18, 17, 16] {
            assert_eq!(
                material_at(&brick, &fixture, 5, y, 5),
                MATERIALS.dirt,
                "y = {y}"
            );
        }
        for y in [15, 8, 0] {
            assert_eq!(
                material_at(&brick, &fixture, 5, y, 5),
                MATERIALS.stone,
                "y = {y}"
            );
        }
        for y in [20, 21, 31] {
            assert_eq!(
                material_at(&brick, &fixture, 5, y, 5),
                MATERIALS.air,
                "y = {y}"
            );
        }
    }

    #[test]
    fn the_surface_layers_continue_across_the_brick_boundary_above() {
        // Ground tops out at height 34, so the grass is in the brick above y = 31's brick.
        let fixture = StandardTestLayout::default();
        let generator = flat_ground_generator(34.0, &fixture);
        let lower = generator.generate_brick(&fixture.layout, BrickCoordinate::new(0, 0, 0));
        let upper = generator.generate_brick(&fixture.layout, BrickCoordinate::new(0, 1, 0));

        assert_eq!(material_at(&upper, &fixture, 0, 1, 0), MATERIALS.grass); // y = 33
        assert_eq!(material_at(&upper, &fixture, 0, 0, 0), MATERIALS.dirt); // y = 32
        // Solid voxels are y <= 33, so y = 30 has three solid voxels above it (31..=33) and
        // y = 29 has four (30..=33).
        assert_eq!(material_at(&lower, &fixture, 0, 31, 0), MATERIALS.dirt);
        assert_eq!(material_at(&lower, &fixture, 0, 30, 0), MATERIALS.dirt);
        assert_eq!(material_at(&lower, &fixture, 0, 29, 0), MATERIALS.stone);
    }

    #[derive(Debug)]
    struct CountingField {
        inner: Heightfield,
        samples: AtomicUsize,
    }

    impl DensityField for CountingField {
        fn sample(&self, position: VoxelCoordinate, footprint: LevelOfDetail) -> f64 {
            self.samples.fetch_add(1, Ordering::Relaxed);
            self.inner.sample(position, footprint)
        }

        fn bounds(&self, region: &VoxelBox, footprint: LevelOfDetail) -> Interval {
            self.inner.bounds(region, footprint)
        }
    }

    #[test]
    fn sky_and_deep_underground_bricks_are_classified_without_sampling() {
        let fixture = StandardTestLayout::default();
        let field = Arc::new(CountingField {
            inner: Heightfield::new(20.0, FractalNoise::none()),
            samples: AtomicUsize::new(0),
        });
        let generator = FieldTerrainGenerator::new(field.clone(), fixture.material, MATERIALS);

        let sky = generator.generate_brick(&fixture.layout, BrickCoordinate::new(3, 5, -2));
        let deep = generator.generate_brick(&fixture.layout, BrickCoordinate::new(3, -5, -2));

        assert_eq!(field.samples.load(Ordering::Relaxed), 0);
        assert_eq!(
            sky.channel(fixture.material).uniform_value(),
            Some(MATERIALS.air)
        );
        assert_eq!(
            deep.channel(fixture.material).uniform_value(),
            Some(MATERIALS.stone)
        );
        // The surface brick must sample: one column of 36 voxels per (x, z).
        generator.generate_brick(&fixture.layout, BrickCoordinate::new(0, 0, 0));
        assert_eq!(field.samples.load(Ordering::Relaxed), 32 * 32 * 36);
    }

    #[test]
    fn a_brick_just_below_the_surface_layer_is_not_mistaken_for_all_stone() {
        // The top of this brick is stone, but grass sits 4 voxels above its top layer boundary.
        let fixture = StandardTestLayout::default();
        let generator = flat_ground_generator(35.0, &fixture);
        let brick = generator.generate_brick(&fixture.layout, BrickCoordinate::new(0, 0, 0));
        assert_eq!(brick.channel(fixture.material).uniform_value(), None);
        assert_eq!(material_at(&brick, &fixture, 0, 31, 0), MATERIALS.dirt); // 3 solids above: 32..=34
    }

    #[test]
    fn other_channels_keep_their_defaults() {
        let fixture = StandardTestLayout::default();
        let generator = flat_ground_generator(20.0, &fixture);
        let brick = generator.generate_brick(&fixture.layout, BrickCoordinate::new(0, 0, 0));
        assert_eq!(
            brick.channel(fixture.temperature).uniform_value(),
            Some(voxel_grid::test_support::TEMPERATURE_DEFAULT)
        );
    }

    /// Layering written voxel by voxel, independent of the column-buffer production code.
    fn reference_material(
        field: &dyn DensityField,
        position: VoxelCoordinate,
        materials: TerrainMaterials,
    ) -> u32 {
        let solid_at = |height_above: i64| {
            position
                .checked_offset(0, height_above, 0)
                .is_some_and(|above| field.sample(above, LevelOfDetail::FULL_DETAIL) > 0.0)
        };
        if !solid_at(0) {
            return materials.air;
        }
        let mut solid_above = 0;
        while solid_above < SURFACE_LAYER_DEPTH as i64 && solid_at(solid_above + 1) {
            solid_above += 1;
        }
        match solid_above {
            0 => materials.grass,
            1..=3 => materials.dirt,
            _ => materials.stone,
        }
    }

    /// Bricks whose surfaces the default hills pass through.
    fn surface_brick() -> impl Strategy<Value = BrickCoordinate> {
        (-40i64..40, -3i64..6, -40i64..40).prop_map(|(x, y, z)| BrickCoordinate::new(x, y, z))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(40))]

        #[test]
        fn generated_bricks_match_voxel_by_voxel_reference_layering(
            seed in any::<u64>(),
            coordinate in surface_brick(),
            probes in proptest::collection::vec(local_voxel_index(), 200),
        ) {
            let fixture = StandardTestLayout::default();
            let generator = rolling_hills(
                RollingHillsSettings::for_one_metre_voxels(seed),
                fixture.material,
                MATERIALS,
            );
            let brick = generator.generate_brick(&fixture.layout, coordinate);
            for local in probes {
                let expected = reference_material(
                    generator.field.as_ref(),
                    coordinate.voxel_at(local),
                    MATERIALS,
                );
                prop_assert_eq!(brick.get(fixture.material, local), expected);
            }
        }
    }

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
            MATERIALS,
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

    #[test]
    fn generation_is_independent_of_thread_and_repetition() {
        let fixture = StandardTestLayout::default();
        let generator = Arc::new(rolling_hills(
            RollingHillsSettings::for_one_metre_voxels(5),
            fixture.material,
            MATERIALS,
        ));
        let coordinate = BrickCoordinate::new(2, 1, -3);
        let reference = generator.generate_brick(&fixture.layout, coordinate);
        let from_threads: Vec<Brick> = std::thread::scope(|scope| {
            (0..4)
                .map(|_| scope.spawn(|| generator.generate_brick(&fixture.layout, coordinate)))
                .collect::<Vec<_>>()
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });
        for brick in from_threads {
            assert_eq!(brick, reference);
        }
    }

    #[test]
    fn default_hills_produce_real_terrain_with_all_materials_near_the_surface() {
        let fixture = StandardTestLayout::default();
        let generator = rolling_hills(
            RollingHillsSettings::for_one_metre_voxels(1),
            fixture.material,
            MATERIALS,
        );
        let mut seen = [false; 4];
        for x in -3..3 {
            for y in 0..5 {
                for z in -3..3 {
                    let brick =
                        generator.generate_brick(&fixture.layout, BrickCoordinate::new(x, y, z));
                    for value in brick.channel(fixture.material).iter() {
                        seen[value as usize] = true;
                    }
                }
            }
        }
        assert_eq!(
            seen, [true; 4],
            "air, stone, dirt and grass should all occur"
        );
    }
}

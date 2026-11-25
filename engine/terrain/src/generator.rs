use std::fmt::Debug;
use std::sync::Arc;

use coarse_cells::CoarseNode;
use region_octree::BrickCube;
use voxel_grid::{Brick, ChannelIndex, ChannelStorage, GridLayout};
use voxel_space::{
    BRICK_EDGE_LENGTH, BRICK_VOXEL_COUNT, BrickCoordinate, LevelOfDetail, LocalVoxelIndex, VoxelBox,
};

use crate::density::DensityField;

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

    /// Whether every voxel of every brick of the cube is air, as [`TerrainGenerator::generate_brick`] makes them: a ship that goes through a cube of this and has nothing built in
    /// it needs no colliders of its bricks. `false` when unsure, which is the default (and for a cube that is air in all but a voxel).
    ///
    /// Unlike [`TerrainGenerator::may_show`] this is not only about what is drawn: a generator that says `true` of a cube that has ground in it leaves a ship to go through the
    /// ground, so its answer is as much a part of the generator as its bricks are.
    fn is_air(&self, cube: BrickCube) -> bool {
        let _ = cube;
        false
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

/// The material values a terrain generator writes into the material channel, by where in the ground they go: the top voxel of the ground under open air
/// (`surface`), the ones just below it (`subsurface`), and everything deeper (`deep`). The engine does not know what the game makes of them (Cave's are
/// grass, dirt and stone).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceLayers {
    pub air: u32,
    pub surface: u32,
    pub subsurface: u32,
    pub deep: u32,
}

/// How many voxels of ground from the top count as the surface layers: the surface voxel, then subsurface voxels, and `deep` from this depth.
pub const SURFACE_LAYER_DEPTH: usize = 4;

/// Turns a [`DensityField`] into bricks: solid voxels become the surface layer under open air, the subsurface layer within
/// three voxels of the surface above them, and the deep layer deeper down.
///
/// Surface layering looks along +y, so this generator assumes y is "up" in its own world.
#[derive(Clone, Debug)]
pub struct FieldTerrainGenerator {
    field: Arc<dyn DensityField>,
    /// What the coarse nodes are made from, when it is not the field ([`FieldTerrainGenerator::with_far_field`]).
    far_field: Option<Arc<dyn DensityField>>,
    material_channel: ChannelIndex,
    materials: SurfaceLayers,
}

impl FieldTerrainGenerator {
    pub fn new(
        field: Arc<dyn DensityField>,
        material_channel: ChannelIndex,
        materials: SurfaceLayers,
    ) -> Self {
        Self {
            field,
            far_field: None,
            material_channel,
            materials,
        }
    }

    /// Draws the coarse nodes of this generator from `far_field` and not from its field: the outer skin of the world, what it looks like from far away, without the features
    /// that cannot be seen from there (the caves under hills, which are all but invisible from above and are twelve times the quads, `docs/specs/lod-spec.md` 9a). The bricks are
    /// still the field's. A far field that is the height of a surface ([`DensityField::surface_height`]) has its nodes found a column at a time, which is about a hundred
    /// times faster; one that is not has them found a cell at a time, as a field's are.
    ///
    /// Like the field, it is part of what a world is drawn as: a generator that has had one never has another.
    #[must_use]
    pub fn with_far_field(mut self, far_field: Arc<dyn DensityField>) -> Self {
        self.far_field = Some(far_field);
        self
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
        let with_surface_layer = region.extended_upper(0, SURFACE_LAYER_DEPTH as i128, 0);
        if self.field.bounds(&with_surface_layer, detail).lower() > 0.0 {
            brick
                .channel_mut(self.material_channel)
                .fill(self.materials.deep);
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
                        .checked_offset(x as i128, height as i128, z as i128)
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
                        0 => self.materials.surface,
                        depth if depth < SURFACE_LAYER_DEPTH => self.materials.subsurface,
                        _ => self.materials.deep,
                    };
                }
            }
        }
        *brick.channel_mut(self.material_channel) = ChannelStorage::from_values(&materials);
        brick
    }

    fn coarse_node(&self, cube: BrickCube, unlit: u8) -> Option<CoarseNode> {
        match &self.far_field {
            None => crate::coarse::coarse_node_of_field(&*self.field, self.materials, cube, unlit),
            Some(far_field) => {
                crate::coarse::coarse_node_of_surface(&**far_field, self.materials, cube, unlit)
                    .or_else(|| {
                        crate::coarse::coarse_node_of_field(
                            &**far_field,
                            self.materials,
                            cube,
                            unlit,
                        )
                    })
            }
        }
    }

    fn may_show(&self, cube: BrickCube) -> bool {
        // A cube is drawn as a node from the far field, so it is that field that says whether there is anything to see (the caves of the field are in no node: a cube under the
        // ground is buried though a cave could be in it). The bricks near the viewer do not ask: the selection holds them whatever this says.
        let drawn_from = self.far_field.as_ref().unwrap_or(&self.field);
        crate::coarse::cube_may_show(&**drawn_from, cube)
    }

    fn is_air(&self, cube: BrickCube) -> bool {
        crate::coarse::cube_is_air(&*self.field, cube)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use proptest::prelude::*;
    use voxel_grid::test_support::StandardTestLayout;
    use voxel_space::{VoxelCoordinate, test_support::local_voxel_index};

    use super::*;
    use crate::density::Heightfield;
    use crate::interval::Interval;
    use crate::noise::FractalNoise;
    use crate::test_support::{RollingHillsSettings, hills_field, rolling_hills};

    const MATERIALS: SurfaceLayers = SurfaceLayers {
        air: 0,
        deep: 1,
        subsurface: 2,
        surface: 3,
    };

    fn flat_ground_generator(height: f64, fixture: &StandardTestLayout) -> FieldTerrainGenerator {
        FieldTerrainGenerator::new(
            Arc::new(Heightfield::new(height, FractalNoise::none())),
            fixture.material,
            MATERIALS,
        )
    }

    /// Ground at 20 with sheet caves in it, as the overworld has them: a cube deep in the ground may have a cave in it, so the field cannot say that it is buried.
    fn ground_with_caves(fixture: &StandardTestLayout) -> (FieldTerrainGenerator, Heightfield) {
        use crate::density::{Minimum, ShellCaves};
        use crate::noise::Dimensionality;
        let ground = Heightfield::new(20.0, FractalNoise::none());
        let caves = ShellCaves::new(
            FractalNoise::standard(5, Dimensionality::Volumetric, 5, 2, 1.0, 0.5),
            0.08,
            16.0,
        );
        let generator = FieldTerrainGenerator::new(
            Arc::new(Minimum {
                first: ground.clone(),
                second: caves,
            }),
            fixture.material,
            MATERIALS,
        );
        (generator, ground)
    }

    #[test]
    fn a_cube_deep_under_ground_with_caves_may_show_but_not_when_it_is_drawn_from_the_ground_alone()
    {
        let fixture = StandardTestLayout::default();
        let (with_caves, ground) = ground_with_caves(&fixture);
        let deep = BrickCube::new(BrickCoordinate::new(0, -64, 0), 3).unwrap();
        let surface = BrickCube::new(BrickCoordinate::new(0, 0, 0), 3).unwrap();
        let sky = BrickCube::new(BrickCoordinate::new(0, 8, 0), 3).unwrap();
        // Without a far field the caves may be anywhere in it.
        assert!(
            with_caves.may_show(deep),
            "precondition: caves may be in it"
        );
        assert!(with_caves.may_show(surface));
        assert!(!with_caves.may_show(sky));
        // Drawn from the ground alone (the far field), nobody sees into it, and the surface and the sky are as before.
        let drawn_from_ground = with_caves.with_far_field(Arc::new(ground));
        assert!(!drawn_from_ground.may_show(deep));
        assert!(drawn_from_ground.may_show(surface));
        assert!(!drawn_from_ground.may_show(sky));
    }

    #[test]
    fn a_cube_that_only_the_far_field_has_ground_in_may_show() {
        let fixture = StandardTestLayout::default();
        let plain = flat_ground_generator(20.0, &fixture)
            .with_far_field(Arc::new(Heightfield::new(300.0, FractalNoise::none())));
        // (Air in the field, ground in the far field: it is drawn from the far field.)
        let cube = BrickCube::new(BrickCoordinate::new(0, 4, 0), 2).unwrap();
        assert!(plain.may_show(cube));
        // And what the far field says is air is not shown, whatever the field has.
        let far_air = flat_ground_generator(300.0, &fixture)
            .with_far_field(Arc::new(Heightfield::new(20.0, FractalNoise::none())));
        assert!(!far_air.may_show(cube));
    }

    #[test]
    fn a_generator_with_a_far_field_draws_its_coarse_nodes_from_it_and_makes_its_bricks_from_the_field()
     {
        let fixture = StandardTestLayout::default();
        let plain = flat_ground_generator(20.0, &fixture);
        let with_far = flat_ground_generator(20.0, &fixture)
            .with_far_field(Arc::new(Heightfield::new(100.0, FractalNoise::none())));
        let far_alone = flat_ground_generator(100.0, &fixture);
        // Its bricks are the field's.
        for y in [0, 1, 2, 3] {
            let at = BrickCoordinate::new(0, y, 0);
            assert_eq!(
                with_far
                    .generate_brick(&fixture.layout, at)
                    .content_digest(),
                plain.generate_brick(&fixture.layout, at).content_digest(),
                "brick at {y}"
            );
        }
        // Its coarse nodes are the far field's: the same as those of a generator whose field that is, and not those of its own field.
        let cube = BrickCube::new(BrickCoordinate::new(0, 0, 0), 2).unwrap();
        let far = far_alone.coarse_node(cube, 7).unwrap();
        assert_eq!(with_far.coarse_node(cube, 7), Some(far.clone()));
        assert_ne!(plain.coarse_node(cube, 7), Some(far));
        // A brick has no node, far field or not.
        let brick = BrickCube::new(BrickCoordinate::new(0, 0, 0), 0).unwrap();
        assert_eq!(with_far.coarse_node(brick, 7), None);
    }

    #[test]
    fn a_far_field_that_is_not_the_height_of_a_surface_has_its_nodes_found_a_cell_at_a_time() {
        let fixture = StandardTestLayout::default();
        let settings = RollingHillsSettings::for_one_metre_voxels(7);
        // Hills with caves are not the height of a surface: the nodes of the far field are asked of its cells.
        let far: Arc<dyn DensityField> = hills_field(settings);
        let with_far = flat_ground_generator(20.0, &fixture).with_far_field(Arc::clone(&far));
        let far_alone = FieldTerrainGenerator::new(far, fixture.material, MATERIALS);
        let cube = BrickCube::new(BrickCoordinate::new(0, 0, 0), 3).unwrap();
        let wanted = far_alone.coarse_node(cube, 5);
        assert!(
            wanted
                .as_ref()
                .is_some_and(|node| node.uniform_cell().is_none())
        );
        assert_eq!(with_far.coarse_node(cube, 5), wanted);
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
        assert_eq!(material_at(&brick, &fixture, 5, 19, 5), MATERIALS.surface);
        for y in [18, 17, 16] {
            assert_eq!(
                material_at(&brick, &fixture, 5, y, 5),
                MATERIALS.subsurface,
                "y = {y}"
            );
        }
        for y in [15, 8, 0] {
            assert_eq!(
                material_at(&brick, &fixture, 5, y, 5),
                MATERIALS.deep,
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

        assert_eq!(material_at(&upper, &fixture, 0, 1, 0), MATERIALS.surface); // y = 33
        assert_eq!(material_at(&upper, &fixture, 0, 0, 0), MATERIALS.subsurface); // y = 32
        // Solid voxels are y <= 33, so y = 30 has three solid voxels above it (31..=33) and
        // y = 29 has four (30..=33).
        assert_eq!(
            material_at(&lower, &fixture, 0, 31, 0),
            MATERIALS.subsurface
        );
        assert_eq!(
            material_at(&lower, &fixture, 0, 30, 0),
            MATERIALS.subsurface
        );
        assert_eq!(material_at(&lower, &fixture, 0, 29, 0), MATERIALS.deep);
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
            Some(MATERIALS.deep)
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
        assert_eq!(
            material_at(&brick, &fixture, 0, 31, 0),
            MATERIALS.subsurface
        ); // 3 solids above: 32..=34
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
        materials: SurfaceLayers,
    ) -> u32 {
        let solid_at = |height_above: i128| {
            position
                .checked_offset(0, height_above, 0)
                .is_some_and(|above| field.sample(above, LevelOfDetail::FULL_DETAIL) > 0.0)
        };
        if !solid_at(0) {
            return materials.air;
        }
        let mut solid_above = 0;
        while solid_above < SURFACE_LAYER_DEPTH as i128 && solid_at(solid_above + 1) {
            solid_above += 1;
        }
        match solid_above {
            0 => materials.surface,
            1..=3 => materials.subsurface,
            _ => materials.deep,
        }
    }

    /// Bricks whose surfaces the default hills pass through.
    fn surface_brick() -> impl Strategy<Value = BrickCoordinate> {
        (-40i128..40, -3i128..6, -40i128..40).prop_map(|(x, y, z)| BrickCoordinate::new(x, y, z))
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
            let field = hills_field(RollingHillsSettings::for_one_metre_voxels(seed));
            let generator = FieldTerrainGenerator::new(
                Arc::clone(&field),
                fixture.material,
                MATERIALS,
            );
            let brick = generator.generate_brick(&fixture.layout, coordinate);
            for local in probes {
                let expected = reference_material(
                    field.as_ref(),
                    coordinate.voxel_at(local),
                    MATERIALS,
                );
                prop_assert_eq!(brick.get(fixture.material, local), expected);
            }
        }
    }

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

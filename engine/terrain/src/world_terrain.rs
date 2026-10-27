use std::sync::Arc;

use content_registry::{AlreadyRegistered, Registry, RuntimeId};
use namespaced_id::NamespacedId;
use region_octree::{BrickCube, ROOT_LEVEL, RegionOctree};
use voxel_grid::{Brick, GridLayout};
use voxel_space::BrickCoordinate;

use crate::generator::TerrainGenerator;

pub type GeneratorId = RuntimeId<Arc<dyn TerrainGenerator>>;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WorldTerrainError {
    #[error("{0} is not a registered terrain generator whose plugin is loaded")]
    GeneratorUnavailable(NamespacedId),
    #[error("assignment cubes can be at most level {ROOT_LEVEL}, got {0}")]
    AssignmentLevelTooLarge(u8),
}

/// What generating a brick produced.
#[derive(Debug, PartialEq, Eq)]
pub enum GenerationOutcome {
    Generated(Brick),
    /// No generator version has been assigned to this region yet.
    Unassigned,
    /// The region's generator is registered only as a placeholder: its plugin is not loaded. The
    /// unmodified terrain cannot be reproduced, but nothing is lost from the save.
    MissingGenerator(NamespacedId),
}

/// Which generator version produced each region of a world.
///
/// Regions are assigned lazily, in aligned cubes of `2^assignment_cube_level` bricks, to whatever
/// the default generator is at that moment, and then keep that generator forever. Changing the
/// default therefore affects only regions generated afterwards. The assignment tree collapses
/// uniform areas, so it stays small enough to send to clients.
#[derive(Debug)]
pub struct WorldTerrain {
    generators: Registry<Arc<dyn TerrainGenerator>>,
    assignments: RegionOctree<Option<GeneratorId>>,
    default_generator: GeneratorId,
    assignment_cube_level: u8,
    /// Counts up each time what generator makes a region could have changed (see [`WorldTerrain::version`]).
    version: u64,
}

impl WorldTerrain {
    pub fn new(
        generators: Registry<Arc<dyn TerrainGenerator>>,
        default_generator: GeneratorId,
        assignment_cube_level: u8,
    ) -> Result<Self, WorldTerrainError> {
        if assignment_cube_level > ROOT_LEVEL {
            return Err(WorldTerrainError::AssignmentLevelTooLarge(
                assignment_cube_level,
            ));
        }
        let terrain = Self {
            generators,
            assignments: RegionOctree::new(None),
            default_generator,
            assignment_cube_level,
            version: 0,
        };
        terrain.require_available(default_generator)?;
        Ok(terrain)
    }

    fn require_available(&self, generator: GeneratorId) -> Result<(), WorldTerrainError> {
        match self.generators.entry(generator) {
            Some(entry) if !entry.is_missing() => Ok(()),
            Some(entry) => Err(WorldTerrainError::GeneratorUnavailable(
                entry.identifier().clone(),
            )),
            None => Err(WorldTerrainError::GeneratorUnavailable(
                "unknown:generator"
                    .parse()
                    .expect("constant identifier is valid"),
            )),
        }
    }

    pub fn generators(&self) -> &Registry<Arc<dyn TerrainGenerator>> {
        &self.generators
    }

    /// Adds a generator to the table, after the others, so that the numbers of the others do not move. Nothing is assigned it and the default does not change: a
    /// region has it only when something gives it to the region. The name must be new (a placeholder holds its name too).
    pub fn register_generator(
        &mut self,
        identifier: NamespacedId,
        generator: Arc<dyn TerrainGenerator>,
    ) -> Result<GeneratorId, AlreadyRegistered> {
        self.generators.register(identifier, generator)
    }

    pub fn default_generator(&self) -> GeneratorId {
        self.default_generator
    }

    /// Counts up each time the generator that makes some region could have changed: when an assignment is restored over what was there, and when the default
    /// is chosen. (Giving a region that has none the default generator does not change it: it was made by the default all along, as far as anyone can tell.)
    /// Whoever keeps answers that depend on which generator makes a region, such as [`WorldTerrain::may_show`], keeps them for one version.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn assignment_cube_level(&self) -> u8 {
        self.assignment_cube_level
    }

    /// Restores a saved assignment, replacing whatever the cube held. For loading worlds; during play
    /// regions are assigned with [`WorldTerrain::assign_unassigned`].
    pub fn set_assignment(&mut self, cube: BrickCube, generator: Option<GeneratorId>) {
        self.assignments.set_cube(cube, generator);
        self.version += 1;
    }

    pub fn assignments(&self) -> &RegionOctree<Option<GeneratorId>> {
        &self.assignments
    }

    /// Chooses the generator given to regions generated from now on. Existing assignments stay.
    pub fn set_default_generator(
        &mut self,
        generator: GeneratorId,
    ) -> Result<(), WorldTerrainError> {
        self.require_available(generator)?;
        self.default_generator = generator;
        self.version += 1;
        Ok(())
    }

    pub fn assigned_generator(&self, brick: BrickCoordinate) -> Option<GeneratorId> {
        *self.assignments.get(brick)
    }

    /// Gives the whole assignment cube around `brick` the default generator if it has no
    /// generator yet, and returns the generator now responsible for `brick`. This is a server
    /// operation: clients only learn assignments from the server.
    pub fn assign_unassigned(&mut self, brick: BrickCoordinate) -> GeneratorId {
        if let Some(existing) = self.assigned_generator(brick) {
            return existing;
        }
        let cube = BrickCube::containing(brick, self.assignment_cube_level);
        self.assignments
            .set_cube(cube, Some(self.default_generator));
        self.default_generator
    }

    /// The coarse cells of a cube of bricks (see [`TerrainGenerator::coarse_node`]), from the generator of the brick in the middle of the cube, or the default
    /// generator where nothing is assigned yet (the one the region will have when it is first needed). A cube with more than one generator in it is drawn
    /// as the one in the middle; `None` if that generator is not available or does not give coarse cells.
    pub fn coarse_node(&self, cube: BrickCube, unlit: u8) -> Option<coarse_cells::CoarseNode> {
        self.coarse_generator(cube)?.coarse_node(cube, unlit)
    }

    /// The generator that makes the coarse cells of a cube (see [`WorldTerrain::coarse_node`]), for handing to a thread that makes them.
    pub fn coarse_generator(&self, cube: BrickCube) -> Option<Arc<dyn TerrainGenerator>> {
        let half = (cube.edge_length() / 2) as i128;
        let origin = cube.origin();
        let middle = BrickCoordinate::new(origin.x() + half, origin.y() + half, origin.z() + half);
        let generator = self
            .assigned_generator(middle)
            .unwrap_or(self.default_generator);
        self.generators.entry(generator)?.content().cloned()
    }

    /// Whether drawing the cube could show anything (see [`TerrainGenerator::may_show`]), by the generator that the whole cube is generated by, the default where
    /// nothing is assigned yet. A cube with more than one generator in it, or one whose generator is not here, may show something.
    pub fn may_show(&self, cube: BrickCube) -> bool {
        let mut chosen: Option<GeneratorId> = None;
        let one_generator = self.assignments.all_in_cube(cube, |assigned| {
            let generator = assigned.unwrap_or(self.default_generator);
            *chosen.get_or_insert(generator) == generator
        });
        let Some(generator) = chosen.filter(|_| one_generator) else {
            return true;
        };
        self.generators
            .entry(generator)
            .and_then(|entry| entry.content())
            .is_none_or(|generator| generator.may_show(cube))
    }

    pub fn generate(&self, layout: &GridLayout, brick: BrickCoordinate) -> GenerationOutcome {
        let Some(generator_id) = self.assigned_generator(brick) else {
            return GenerationOutcome::Unassigned;
        };
        let entry = self
            .generators
            .entry(generator_id)
            .expect("assigned generators come from this registry");
        match entry.content() {
            Some(generator) => {
                GenerationOutcome::Generated(generator.generate_brick(layout, brick))
            }
            None => GenerationOutcome::MissingGenerator(entry.identifier().clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use voxel_grid::test_support::StandardTestLayout;

    use content_registry::AlreadyRegistered;

    use super::*;
    use crate::density::Heightfield;
    use crate::generator::{FieldTerrainGenerator, TerrainMaterials};
    use crate::noise::FractalNoise;

    const MATERIALS: TerrainMaterials = TerrainMaterials {
        air: 0,
        stone: 1,
        dirt: 2,
        grass: 3,
    };

    fn id(text: &str) -> NamespacedId {
        text.parse().unwrap()
    }

    fn brick(x: i128, y: i128, z: i128) -> BrickCoordinate {
        BrickCoordinate::new(x, y, z)
    }

    struct Fixture {
        layout: StandardTestLayout,
        terrain: WorldTerrain,
        low: GeneratorId,
        high: GeneratorId,
        ghost: GeneratorId,
    }

    /// Two flat-ground versions (surface at 10 and at 40) and a placeholder for a missing plugin.
    fn fixture(assignment_cube_level: u8) -> Fixture {
        let layout = StandardTestLayout::default();
        let flat = |height: f64| -> Arc<dyn TerrainGenerator> {
            Arc::new(FieldTerrainGenerator::new(
                Arc::new(Heightfield::new(height, FractalNoise::none())),
                layout.material,
                MATERIALS,
            ))
        };
        let mut generators = Registry::new();
        let low = generators
            .register(id("test_terrain:flat_low"), flat(10.0))
            .unwrap();
        let high = generators
            .register(id("test_terrain:flat_high"), flat(40.0))
            .unwrap();
        let ghost = generators.register_missing(id("gone_plugin:ghost_terrain"));
        let terrain = WorldTerrain::new(generators, low, assignment_cube_level).unwrap();
        Fixture {
            layout,
            terrain,
            low,
            high,
            ghost,
        }
    }

    fn generated(outcome: GenerationOutcome) -> Brick {
        match outcome {
            GenerationOutcome::Generated(brick) => brick,
            other => panic!("expected a generated brick, got {other:?}"),
        }
    }

    #[test]
    fn the_version_counts_up_when_the_generator_of_a_region_could_have_changed_and_not_when_the_default_is_given_to_a_region_that_had_none()
     {
        let mut fixture = fixture(3);
        let start = fixture.terrain.version();
        // A region that has none gets the default, which is what it had: the same answers as before.
        fixture.terrain.assign_unassigned(brick(100, 0, 100));
        assert_eq!(fixture.terrain.version(), start);
        // One that is restored over what was there, or a new default, may not be.
        fixture.terrain.set_assignment(
            BrickCube::new(brick(8, 0, 8), 3).unwrap(),
            Some(fixture.high),
        );
        assert_eq!(fixture.terrain.version(), start + 1);
        fixture.terrain.set_default_generator(fixture.high).unwrap();
        assert_eq!(fixture.terrain.version(), start + 2);
        // A default that is not there is refused and changes nothing.
        assert!(
            fixture
                .terrain
                .set_default_generator(fixture.ghost)
                .is_err()
        );
        assert_eq!(fixture.terrain.version(), start + 2);
    }

    #[test]
    fn a_cube_may_show_something_unless_the_one_generator_that_makes_all_of_it_says_it_need_not_be_drawn()
     {
        let mut fixture = fixture(3);
        let cube = |origin: [i128; 3], level: u8| {
            BrickCube::new(brick(origin[0], origin[1], origin[2]), level).unwrap()
        };
        // Nothing is assigned: the default ground, to 10 voxels. A cube of 8 bricks from voxel 256 up is air, and one from 0 holds the surface.
        assert!(!fixture.terrain.may_show(cube([0, 8, 0], 3)));
        assert!(fixture.terrain.may_show(cube([0, 0, 0], 3)));
        assert!(!fixture.terrain.may_show(cube([0, -24, 0], 3)), "buried");
        // Assigned to the generator that is the default anyway, it is the same.
        fixture
            .terrain
            .set_assignment(cube([16, 8, 16], 3), Some(fixture.low));
        assert!(!fixture.terrain.may_show(cube([16, 8, 16], 3)));
        // The high ground (to 40) holds the surface in the cube from voxel 0 to 255, and the cube above it is air: for it too.
        fixture
            .terrain
            .set_assignment(cube([8, 0, 8], 3), Some(fixture.high));
        assert!(fixture.terrain.may_show(cube([8, 0, 8], 3)));
        assert!(!fixture.terrain.may_show(cube([8, 8, 8], 3)));
        // A cube with both in it is drawn, though each would say it is air up there: it is not their say.
        fixture
            .terrain
            .set_assignment(cube([8, 16, 8], 3), Some(fixture.high));
        assert!(!fixture.terrain.may_show(cube([0, 32, 0], 4)));
        assert!(fixture.terrain.may_show(cube([0, 16, 0], 4)));
        // A generator whose plugin is not here cannot say.
        fixture
            .terrain
            .set_assignment(cube([24, 8, 24], 3), Some(fixture.ghost));
        assert!(fixture.terrain.may_show(cube([24, 8, 24], 3)));
    }

    #[test]
    fn the_coarse_cells_of_a_cube_are_those_of_the_generator_in_its_middle_the_default_where_nothing_is_assigned_and_none_where_it_is_missing()
     {
        let mut fixture = fixture(3);
        let cube = |origin: [i128; 3], level: u8| {
            BrickCube::new(brick(origin[0], origin[1], origin[2]), level).unwrap()
        };
        // Flat ground at 10 (the default) and at 40: a cube of level 3 from y = 0 holds voxels 0 to 255, and its cells (8 voxels) are the ground below
        // 10 and below 40 respectively; the cell at height 4 is ground for both and the cell at height 2 (voxels 16 to 23) is air under the first and ground under the second.
        let at = voxel_space::LocalVoxelIndex::new(3, 2, 3);
        let unassigned = fixture.terrain.coarse_node(cube([8, 0, 8], 3), 7).unwrap();
        assert_eq!(unassigned.cell(at).fill, 0, "the default has ground to 10");
        // Assign the cube to the higher ground: now it is ground there.
        fixture
            .terrain
            .set_assignment(cube([8, 0, 8], 3), Some(fixture.high));
        let assigned = fixture.terrain.coarse_node(cube([8, 0, 8], 3), 7).unwrap();
        assert_eq!(assigned.cell(at).fill, 255);
        // A cube that holds both is drawn as the generator in its middle: the cube of level 4 from (0, 0, 0) (cells of 16 voxels, voxels 0 to 511) has
        // its middle brick at (8, 8, 8), in the cube assigned to the high ground (to 40), so its cell at height 1 (voxels 16 to 31) is ground, which
        // the low ground (to 10) would leave air.
        let in_the_middle = voxel_space::LocalVoxelIndex::new(3, 1, 3);
        fixture
            .terrain
            .set_assignment(cube([8, 8, 8], 3), Some(fixture.high));
        let mixed = fixture.terrain.coarse_node(cube([0, 0, 0], 4), 7).unwrap();
        assert_eq!(mixed.cell(in_the_middle).fill, 255);
        fixture
            .terrain
            .set_assignment(cube([8, 8, 8], 3), Some(fixture.low));
        let now_low = fixture.terrain.coarse_node(cube([0, 0, 0], 4), 7).unwrap();
        assert_eq!(now_low.cell(in_the_middle).fill, 0);
        // A generator whose plugin is missing gives nothing, and so does a brick-sized cube (a brick is generated, not summarised).
        fixture
            .terrain
            .set_assignment(cube([16, 0, 16], 3), Some(fixture.ghost));
        assert_eq!(fixture.terrain.coarse_node(cube([16, 0, 16], 3), 7), None);
        assert_eq!(fixture.terrain.coarse_node(cube([8, 0, 8], 0), 7), None);
    }

    #[test]
    fn nothing_is_generated_for_unassigned_regions() {
        let fixture = fixture(3);
        assert_eq!(fixture.terrain.assigned_generator(brick(0, 0, 0)), None);
        assert_eq!(
            fixture
                .terrain
                .generate(&fixture.layout.layout, brick(0, 0, 0)),
            GenerationOutcome::Unassigned
        );
    }

    #[test]
    fn assigning_covers_the_whole_aligned_cube_and_nothing_beyond() {
        let mut fixture = fixture(3);
        assert_eq!(
            fixture.terrain.assign_unassigned(brick(9, 2, -1)),
            fixture.low
        );

        // The level-3 cube around (9, 2, -1) is x 8..16, y 0..8, z -8..0.
        for inside in [brick(8, 0, -8), brick(15, 7, -1), brick(9, 2, -1)] {
            assert_eq!(
                fixture.terrain.assigned_generator(inside),
                Some(fixture.low),
                "{inside}"
            );
        }
        for outside in [
            brick(7, 2, -1),
            brick(16, 2, -1),
            brick(9, 8, -1),
            brick(9, 2, 0),
        ] {
            assert_eq!(
                fixture.terrain.assigned_generator(outside),
                None,
                "{outside}"
            );
        }
    }

    #[test]
    fn a_region_keeps_its_generator_when_the_default_changes() {
        let mut fixture = fixture(3);
        fixture.terrain.assign_unassigned(brick(0, 0, 0));
        fixture.terrain.set_default_generator(fixture.high).unwrap();

        // Assigning again inside the old region changes nothing.
        assert_eq!(
            fixture.terrain.assign_unassigned(brick(1, 1, 1)),
            fixture.low
        );
        // A new region gets the new default.
        assert_eq!(
            fixture.terrain.assign_unassigned(brick(100, 0, 0)),
            fixture.high
        );

        // Each region generates with its own version: surface at 10 puts grass in brick y = 0 only
        // for the low generator; for the high one (surface 40) the brick is solid below the layers.
        let old_region = generated(
            fixture
                .terrain
                .generate(&fixture.layout.layout, brick(0, 0, 0)),
        );
        let new_region = generated(
            fixture
                .terrain
                .generate(&fixture.layout.layout, brick(100, 0, 0)),
        );
        assert_eq!(
            old_region.channel(fixture.layout.material).uniform_value(),
            None
        );
        assert_eq!(
            new_region.channel(fixture.layout.material).uniform_value(),
            Some(MATERIALS.stone)
        );
    }

    #[test]
    fn a_region_assigned_to_a_missing_generator_reports_it_without_generating() {
        let mut fixture = fixture(2);
        // Simulate a save whose region was generated by a plugin that is no longer loaded.
        let cube = BrickCube::containing(brick(0, 0, 0), 2);
        fixture
            .terrain
            .assignments
            .set_cube(cube, Some(fixture.ghost));
        assert_eq!(
            fixture
                .terrain
                .generate(&fixture.layout.layout, brick(1, 1, 1)),
            GenerationOutcome::MissingGenerator(id("gone_plugin:ghost_terrain"))
        );
    }

    #[test]
    fn coarse_assignment_cubes_keep_the_tree_small_however_many_bricks_are_generated() {
        let walk = |terrain: &mut WorldTerrain| {
            for step in 0..1000i128 {
                terrain.assign_unassigned(brick(step % 50 * 4, step % 7, step / 50));
            }
            terrain.assignments().leaves().len()
        };
        let mut coarse = fixture(8);
        let mut fine = fixture(0);

        // The walk stays inside one level-8 cube. Isolating a single cube from the unassigned
        // rest of the world costs seven extra leaves for each of the 108 levels above it.
        assert_eq!(
            walk(&mut coarse.terrain),
            1 + 7 * (usize::from(ROOT_LEVEL) - 8)
        );
        // One cube per brick is far more expensive. Past the spine that isolates the first brick from the rest of the world (which gets longer
        // with the depth of the tree, so it is taken out rather than compared with), each of the 1000 bricks costs more than eight leaves
        // (nine and a half were measured).
        let spine_to_a_brick = 1 + 7 * usize::from(ROOT_LEVEL);
        assert!(walk(&mut fine.terrain) > spine_to_a_brick + 8 * 1000);
    }

    #[test]
    fn saved_assignments_can_be_restored_and_the_configuration_read_back() {
        let mut fixture = fixture(3);
        let cube = BrickCube::containing(brick(40, 0, 0), 3);
        fixture.terrain.set_assignment(cube, Some(fixture.high));
        assert_eq!(
            fixture.terrain.assigned_generator(brick(41, 2, 3)),
            Some(fixture.high)
        );
        assert_eq!(fixture.terrain.assigned_generator(brick(0, 0, 0)), None);
        fixture.terrain.set_assignment(cube, None);
        assert_eq!(fixture.terrain.assigned_generator(brick(41, 2, 3)), None);
        assert_eq!(fixture.terrain.default_generator(), fixture.low);
        assert_eq!(fixture.terrain.assignment_cube_level(), 3);
        assert_eq!(fixture.terrain.generators().len(), 3);
    }

    #[test]
    fn unusable_defaults_and_levels_are_rejected() {
        let mut fixture = fixture(3);
        assert_eq!(
            fixture.terrain.set_default_generator(fixture.ghost),
            Err(WorldTerrainError::GeneratorUnavailable(id(
                "gone_plugin:ghost_terrain"
            )))
        );
        assert_eq!(
            fixture.terrain.assign_unassigned(brick(0, 0, 0)),
            fixture.low
        );

        let mut generators: Registry<Arc<dyn TerrainGenerator>> = Registry::new();
        let ghost = generators.register_missing(id("gone_plugin:ghost_terrain"));
        assert!(WorldTerrain::new(generators, ghost, 3).is_err());

        let mut generators: Registry<Arc<dyn TerrainGenerator>> = Registry::new();
        let only = generators
            .register(id("test_terrain:flat_low"), fixture_generator())
            .unwrap();
        assert_eq!(
            WorldTerrain::new(generators, only, ROOT_LEVEL + 1).err(),
            Some(WorldTerrainError::AssignmentLevelTooLarge(ROOT_LEVEL + 1))
        );
    }

    #[test]
    fn a_generator_added_to_the_table_comes_after_the_others_and_changes_nothing_that_was() {
        let mut fixture = fixture(3);
        let before = fixture.terrain.generators().len();
        let cube = BrickCube::containing(brick(40, 0, 0), 3);
        fixture.terrain.set_assignment(cube, Some(fixture.high));
        let version = fixture.terrain.version();

        let added = fixture
            .terrain
            .register_generator(id("test_terrain:added"), fixture_generator())
            .unwrap();
        assert_eq!(added.raw() as usize, before);
        assert_eq!(fixture.terrain.generators().len(), before + 1);
        assert_eq!(
            fixture
                .terrain
                .generators()
                .runtime_id(&id("test_terrain:added")),
            Some(added)
        );
        // What made any region before makes it still, and nothing says that what makes a region could have changed.
        assert_eq!(
            fixture.terrain.assigned_generator(brick(41, 2, 3)),
            Some(fixture.high)
        );
        assert_eq!(fixture.terrain.default_generator(), fixture.low);
        assert_eq!(fixture.terrain.version(), version);
        // It can be given a region like any other.
        let other = BrickCube::containing(brick(80, 0, 0), 3);
        fixture.terrain.set_assignment(other, Some(added));
        assert!(matches!(
            fixture
                .terrain
                .generate(&fixture.layout.layout, brick(81, 0, 0)),
            GenerationOutcome::Generated(_)
        ));
    }

    #[test]
    fn a_name_that_is_taken_cannot_be_added_again_not_even_as_a_placeholder() {
        let mut fixture = fixture(3);
        let before = fixture.terrain.generators().len();
        for taken in ["test_terrain:flat_low", "gone_plugin:ghost_terrain"] {
            assert_eq!(
                fixture
                    .terrain
                    .register_generator(id(taken), fixture_generator())
                    .err(),
                Some(AlreadyRegistered {
                    identifier: id(taken)
                })
            );
        }
        assert_eq!(fixture.terrain.generators().len(), before);
    }

    fn fixture_generator() -> Arc<dyn TerrainGenerator> {
        let layout = StandardTestLayout::default();
        Arc::new(FieldTerrainGenerator::new(
            Arc::new(Heightfield::new(1.0, FractalNoise::none())),
            layout.material,
            MATERIALS,
        ))
    }
}

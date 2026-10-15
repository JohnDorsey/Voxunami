use std::sync::Arc;

use content_registry::{Registry, RuntimeId};
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

    fn brick(x: i64, y: i64, z: i64) -> BrickCoordinate {
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
            for step in 0..1000i64 {
                terrain.assign_unassigned(brick(step % 50 * 4, step % 7, step / 50));
            }
            terrain.assignments().leaves().len()
        };
        let mut coarse = fixture(8);
        let mut fine = fixture(0);

        // The walk stays inside one level-8 cube. Isolating a single cube from the unassigned
        // rest of the world costs seven extra leaves for each of the 48 levels above it.
        assert_eq!(
            walk(&mut coarse.terrain),
            1 + 7 * (usize::from(ROOT_LEVEL) - 8)
        );
        // One cube per brick is far more expensive.
        assert!(walk(&mut fine.terrain) > 20 * (1 + 7 * (usize::from(ROOT_LEVEL) - 8)));
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

    fn fixture_generator() -> Arc<dyn TerrainGenerator> {
        let layout = StandardTestLayout::default();
        Arc::new(FieldTerrainGenerator::new(
            Arc::new(Heightfield::new(1.0, FractalNoise::none())),
            layout.material,
            MATERIALS,
        ))
    }
}

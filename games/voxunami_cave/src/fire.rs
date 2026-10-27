//! Fire: flames that age and go out, and spread to the blocks next to them that burn.
//!
//! A fire is a block (`base_blocks:fire`) in a voxel, and its state is its age. Every tick each fire
//! that is near a player does three things, by chance, from the world as it stood at the start of the
//! tick (the same tick rules as the light, see [`crate::lighting`]):
//!
//! - it ages by one, with a chance of one in four, and a fire that reaches [`FIRE_LIFETIME`] is
//!   gone, leaving air;
//! - for each of the six blocks around it that burns (its flammability, out of 1024) it may set that
//!   block alight, which turns the block into a new fire of age 0.
//!
//! The chance each voxel gets at each tick is [`voxel_random`] of the world's seed, the tick and the
//! voxel, so that what burns never depends on the order voxels are looked at, and is the same on every
//! machine. Everything is worked out before anything is written, so a block set alight in a tick
//! cannot set its own neighbours alight in the same tick, and two fires that set the same block
//! alight do the same thing to it. Fires that nobody is near are frozen: not looked at, not aged,
//! and still burning when someone comes back.
//!
//! The fires to look at are kept in [`ActiveVoxels`], which is saved with the world, and so is the
//! rule's clock. The chance of a voxel is made from the clock, not from the server's tick, which
//! starts again at 0 whenever a server starts: a world saved and loaded burns exactly as it would
//! have burned if it had never stopped.

use crate::block_properties;
use base_blocks::FIRE_LIFETIME;
use binary_codec::{ByteReader, ByteWriter};
use block_models::{BlockTable, PropertyHandle};
use lattice::{ActiveVoxels, voxel_random};
use namespaced_id::NamespacedId;
use voxel_space::{FaceDirection, GridId, LevelIndex, VoxelCoordinate};
use world_storage::Section;

/// Tells chance of this rule apart from chance made for anything else at the same voxel and tick.
const PURPOSE: u64 = 0x4669_7265; // "Fire"

/// The version of the saved fire this program writes and reads: 2, in which the coordinates of the fires are three wide integers (up to 128 bits each; see
/// `ActiveVoxels`). A section of version 1 would be misread as coordinates, so it is not read.
const SAVED_VERSION: u32 = 2;

pub fn section_id() -> NamespacedId {
    "voxunami_cave:fire"
        .parse()
        .expect("constant identifier is valid")
}

/// A fire ages when the two lowest bits of its number are 0: one tick in four.
const AGING_MASK: u64 = 0b11;

/// Bits of a voxel's number each of the six neighbours uses to decide whether it catches fire,
/// after the two for aging. A neighbour catches when its bits, out of `1 << CHANCE_BITS`, are below its
/// flammability.
const CHANCE_BITS: u32 = 10;

/// The voxels of a world, as far as fire needs to know.
pub trait FireWorld {
    /// The block and state of a voxel, or `None` where there is nothing to read.
    fn block_and_state(&mut self, grid: GridId, at: VoxelCoordinate) -> Option<(u32, u32)>;

    /// Puts a block in a state at a voxel. Only called for voxels that were just read.
    fn set_block_and_state(&mut self, grid: GridId, at: VoxelCoordinate, block: u32, state: u32);
}

/// The fire rule and the fires it has to look at.
#[derive(Debug)]
pub struct Fire {
    table: BlockTable,
    flammability: PropertyHandle<u8>,
    fire_block: u32,
    air_block: u32,
    seed: u64,
    /// How many steps the rule has taken since the world began.
    clock: u64,
    active: ActiveVoxels,
}

impl Fire {
    /// The fire rule for a world with this seed. `clock` and `active` are the rule's clock and the
    /// fires to look at as they were when a world was saved, if it was: 0 and none for a new world.
    pub fn new(
        table: BlockTable,
        fire_block: u32,
        air_block: u32,
        seed: u64,
        clock: u64,
        active: ActiveVoxels,
    ) -> Self {
        Self {
            flammability: block_properties::flammability(&table),
            table,
            fire_block,
            air_block,
            seed,
            clock,
            active,
        }
    }

    /// How many steps the rule has taken since the world began.
    pub fn clock(&self) -> u64 {
        self.clock
    }

    /// The section to save the rule's clock and its fires in; levels are named by `level_ids`, in
    /// level order.
    pub fn to_section(&self, level_ids: &[NamespacedId]) -> Section {
        let mut writer = ByteWriter::new();
        writer.write_varint(self.clock);
        self.active.write_to(&mut writer, level_ids);
        Section {
            identifier: section_id(),
            version: SAVED_VERSION,
            payload: writer.into_bytes(),
        }
    }

    /// The clock and the fires saved in a world's sections, if there is a section this program can
    /// read; `level_index` gives the level a name stands for in this world.
    pub fn read_section(
        sections: &[Section],
        level_index: impl Fn(&NamespacedId) -> Option<LevelIndex>,
    ) -> Option<(u64, ActiveVoxels)> {
        let section = sections.iter().find(|section| {
            section.identifier == section_id() && section.version == SAVED_VERSION
        })?;
        let mut reader = ByteReader::new(&section.payload);
        let clock = reader.read_varint().ok()?;
        let active = ActiveVoxels::read_from(&mut reader, &level_index).ok()?;
        reader.finish().ok()?;
        Some((clock, active))
    }

    /// The fires being looked at, in their fixed order.
    pub fn active(&self) -> &ActiveVoxels {
        &self.active
    }

    /// Tells the rule that a fire block is at a voxel that it did not light itself (a player
    /// placed it), so that it is looked at from the next tick.
    pub fn ignite(&mut self, grid: GridId, at: VoxelCoordinate) {
        self.active.insert(grid, at);
    }

    /// One tick of the rule. Looks at every fire for which `in_range` is true (and keeps the others as they
    /// are), works out what each does from the world as it is now, then writes it all. Returns the
    /// voxels that were written, in the order they were.
    pub fn step(
        &mut self,
        world: &mut impl FireWorld,
        in_range: impl Fn(GridId, VoxelCoordinate) -> bool,
    ) -> Vec<(GridId, VoxelCoordinate)> {
        struct Write {
            grid: GridId,
            at: VoxelCoordinate,
            block: u32,
            state: u32,
        }
        let mut writes = Vec::new();
        let mut burning = Vec::new();
        for (grid, at) in self.active.take() {
            if !in_range(grid, at) {
                burning.push((grid, at));
                continue;
            }
            let Some((block, state)) = world.block_and_state(grid, at) else {
                continue; // nothing there to burn
            };
            if block != self.fire_block {
                continue; // put out, or burnt away, by something else
            }
            let chance = voxel_random(self.seed, self.clock, grid, at, PURPOSE);
            let aged = chance & AGING_MASK == 0;
            let age = state + u32::from(aged);
            if age >= FIRE_LIFETIME {
                writes.push(Write {
                    grid,
                    at,
                    block: self.air_block,
                    state: 0,
                });
            } else {
                burning.push((grid, at));
                if aged {
                    writes.push(Write {
                        grid,
                        at,
                        block: self.fire_block,
                        state: age,
                    });
                }
            }
            for (index, face) in FaceDirection::ALL.into_iter().enumerate() {
                let [dx, dy, dz] = face.offset();
                let Some(neighbour) = at.checked_offset(dx, dy, dz) else {
                    continue;
                };
                let Some((neighbour_block, neighbour_state)) =
                    world.block_and_state(grid, neighbour)
                else {
                    continue;
                };
                let flammability = u64::from(self.table.get_of(
                    self.flammability,
                    neighbour_block,
                    neighbour_state,
                ));
                let roll = (chance >> (2 + CHANCE_BITS * index as u32)) & ((1 << CHANCE_BITS) - 1);
                if roll < flammability {
                    writes.push(Write {
                        grid,
                        at: neighbour,
                        block: self.fire_block,
                        state: 0,
                    });
                }
            }
        }
        // Nothing has been written yet: what each fire did above saw the world as it was.
        let mut changed = Vec::with_capacity(writes.len());
        for write in writes {
            world.set_block_and_state(write.grid, write.at, write.block, write.state);
            if write.block == self.fire_block && write.state == 0 {
                burning.push((write.grid, write.at));
            }
            changed.push((write.grid, write.at));
        }
        self.active = burning.into_iter().collect();
        self.clock += 1;
        changed
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use block_models::{BlockDefinition, Shape};
    use proptest::prelude::*;

    use super::*;

    const AIR: u32 = 0;
    const STONE: u32 = 1;
    const GRASS: u32 = 2;
    const WOOD: u32 = 3;
    const FIRE: u32 = 4;

    const GRASS_FLAMMABILITY: u8 = 100;
    const WOOD_FLAMMABILITY: u8 = 40;

    fn table() -> BlockTable {
        let air = BlockDefinition::air();
        let stone = BlockDefinition::cube([0.5; 3]);
        let grass = BlockDefinition::cube([0.3, 0.6, 0.2])
            .with_property(&base_blocks::FLAMMABILITY, GRASS_FLAMMABILITY);
        let wood = BlockDefinition::cube([0.5, 0.4, 0.2])
            .with_property(&base_blocks::FLAMMABILITY, WOOD_FLAMMABILITY);
        let fire = BlockDefinition::cube([0.9, 0.5, 0.1])
            .with_shapes(vec![Shape::half(FaceDirection::NegativeY)]);
        block_properties::test_table(&[
            Some(&air),
            Some(&stone),
            Some(&grass),
            Some(&wood),
            Some(&fire),
        ])
    }

    const EDGE: i128 = 7;
    const MAIN: GridId = GridId::MAIN;

    fn at(x: i128, y: i128, z: i128) -> VoxelCoordinate {
        VoxelCoordinate::new(x, y, z)
    }

    /// A box of `EDGE` voxels along each side in which everything not set is air; outside it there
    /// is nothing to read.
    #[derive(Clone, Default)]
    struct Box3 {
        blocks: HashMap<(GridId, VoxelCoordinate), (u32, u32)>,
        writes: usize,
    }

    impl FireWorld for Box3 {
        fn block_and_state(&mut self, grid: GridId, at: VoxelCoordinate) -> Option<(u32, u32)> {
            let inside = [at.x(), at.y(), at.z()]
                .iter()
                .all(|component| (0..EDGE).contains(component));
            inside.then(|| self.blocks.get(&(grid, at)).copied().unwrap_or((AIR, 0)))
        }

        fn set_block_and_state(
            &mut self,
            grid: GridId,
            at: VoxelCoordinate,
            block: u32,
            state: u32,
        ) {
            self.writes += 1;
            self.blocks.insert((grid, at), (block, state));
        }
    }

    impl Box3 {
        fn block(&self, x: i128, y: i128, z: i128) -> (u32, u32) {
            self.blocks
                .get(&(MAIN, at(x, y, z)))
                .copied()
                .unwrap_or((AIR, 0))
        }

        fn count(&self, block: u32) -> usize {
            self.blocks
                .values()
                .filter(|(found, _)| *found == block)
                .count()
        }
    }

    fn rule(seed: u64) -> Fire {
        Fire::new(table(), FIRE, AIR, seed, 0, ActiveVoxels::new())
    }

    fn everywhere(_: GridId, _: VoxelCoordinate) -> bool {
        true
    }

    /// Lights a fire at a voxel and puts it in the world.
    fn light(fire: &mut Fire, world: &mut Box3, voxel: VoxelCoordinate) {
        world.blocks.insert((MAIN, voxel), (FIRE, 0));
        fire.ignite(MAIN, voxel);
    }

    /// Ticks until no fire is left, returning how many ticks it took.
    fn burn_out(fire: &mut Fire, world: &mut Box3) -> u64 {
        for ticks in 1..=5000 {
            fire.step(world, everywhere);
            if fire.active().is_empty() {
                return ticks;
            }
        }
        panic!("the fire never went out");
    }

    #[test]
    fn a_fire_with_nothing_to_burn_ages_and_goes_out_leaving_air() {
        for seed in 0..40 {
            let mut fire = rule(seed);
            let mut world = Box3::default();
            world.blocks.insert((MAIN, at(3, 0, 3)), (STONE, 0));
            light(&mut fire, &mut world, at(3, 1, 3));
            let ticks = burn_out(&mut fire, &mut world);
            // Twelve ages, one in four ticks: about 48 ticks, and never fewer than 12.
            assert!((12..400).contains(&ticks), "seed {seed}: {ticks}");
            assert_eq!(world.block(3, 1, 3), (AIR, 0), "seed {seed}");
            assert_eq!(world.block(3, 0, 3), (STONE, 0), "stone does not burn");
            assert_eq!(world.count(FIRE), 0);
        }
    }

    #[test]
    fn a_fire_lasts_about_forty_eight_ticks_on_average() {
        // Twelve ages, each taking four ticks on average.
        let seeds = 400;
        let total: u64 = (0..seeds)
            .map(|seed| {
                let mut fire = rule(seed);
                let mut world = Box3::default();
                light(&mut fire, &mut world, at(3, 3, 3));
                burn_out(&mut fire, &mut world)
            })
            .sum();
        let mean = total as f64 / seeds as f64;
        assert!((43.0..53.0).contains(&mean), "{mean}");
    }

    #[test]
    fn a_fire_ages_one_step_at_a_time_and_is_never_older_than_its_lifetime() {
        let mut fire = rule(7);
        let mut world = Box3::default();
        light(&mut fire, &mut world, at(3, 3, 3));
        let mut last_age = 0;
        for tick in 0..2000 {
            fire.step(&mut world, everywhere);
            match world.block(3, 3, 3) {
                (FIRE, age) => {
                    assert!(age == last_age || age == last_age + 1, "tick {tick}: {age}");
                    assert!(age < FIRE_LIFETIME);
                    last_age = age;
                }
                (AIR, 0) => {
                    assert_eq!(last_age, FIRE_LIFETIME - 1, "it burned through every age");
                    return;
                }
                other => panic!("{other:?}"),
            }
        }
        panic!("never went out");
    }

    #[test]
    fn fire_spreads_to_what_burns_by_chance_and_not_to_what_does_not() {
        let (mut grass_burned, mut wood_burned) = (0, 0);
        let seeds = 300;
        for seed in 0..seeds {
            let mut fire = rule(seed);
            let mut world = Box3::default();
            world.blocks.insert((MAIN, at(2, 3, 3)), (GRASS, 0));
            world.blocks.insert((MAIN, at(4, 3, 3)), (WOOD, 0));
            world.blocks.insert((MAIN, at(3, 4, 3)), (STONE, 0));
            light(&mut fire, &mut world, at(3, 3, 3));
            let mut seen_grass_burn = false;
            let mut seen_wood_burn = false;
            for _ in 0..2000 {
                fire.step(&mut world, everywhere);
                seen_grass_burn |= world.block(2, 3, 3).0 != GRASS;
                seen_wood_burn |= world.block(4, 3, 3).0 != WOOD;
                assert_eq!(world.block(3, 4, 3), (STONE, 0));
                if fire.active().is_empty() {
                    break;
                }
            }
            grass_burned += i32::from(seen_grass_burn);
            wood_burned += i32::from(seen_wood_burn);
        }
        // Grass is much likelier to catch than wood, and each is likely, since a fire lasts long.
        let share = |burned: i32| f64::from(burned) / seeds as f64;
        assert!(share(grass_burned) > 0.95, "grass {}", share(grass_burned));
        assert!(
            (0.6..0.97).contains(&share(wood_burned)),
            "wood {}",
            share(wood_burned)
        );
        assert!(grass_burned > wood_burned);
    }

    #[test]
    fn a_block_set_alight_becomes_a_new_fire_of_age_zero_that_is_looked_at_next_time() {
        // A line of grass with a fire at one end: every ignition is a fire of age 0.
        let mut seeds_with_ignitions = 0;
        for seed in 0..50 {
            let mut fire = rule(seed);
            let mut world = Box3::default();
            for x in 1..EDGE {
                world.blocks.insert((MAIN, at(x, 3, 3)), (GRASS, 0));
            }
            light(&mut fire, &mut world, at(0, 3, 3));
            let mut ignitions = 0;
            for tick in 0..300 {
                let before = world.clone();
                let changed = fire.step(&mut world, everywhere);
                for (grid, voxel) in changed {
                    let was = before
                        .blocks
                        .get(&(grid, voxel))
                        .copied()
                        .unwrap_or((AIR, 0));
                    let now = world.blocks[&(grid, voxel)];
                    if was.0 == GRASS {
                        assert_eq!(now, (FIRE, 0), "seed {seed} tick {tick}");
                        assert!(fire.active().contains(grid, voxel));
                        ignitions += 1;
                    }
                }
                if fire.active().is_empty() {
                    break;
                }
            }
            seeds_with_ignitions += i32::from(ignitions > 0);
        }
        // A fire of that age beside grass almost always spreads, but chance may let one go out.
        assert!(seeds_with_ignitions >= 45, "{seeds_with_ignitions} of 50");
    }

    #[test]
    fn fire_never_travels_more_than_one_voxel_a_tick() {
        // Everything is worked out before anything is written, so a block that catches fire in a
        // tick cannot light the next one in the same tick.
        for seed in 0..300 {
            let mut fire = rule(seed);
            let mut world = Box3::default();
            for x in 1..EDGE {
                world.blocks.insert((MAIN, at(x, 3, 3)), (GRASS, 0));
            }
            light(&mut fire, &mut world, at(0, 3, 3));
            fire.step(&mut world, everywhere);
            assert_ne!(
                world.block(2, 3, 3).0,
                FIRE,
                "seed {seed}: it jumped over a block"
            );
            for x in 3..EDGE {
                assert_eq!(world.block(x, 3, 3).0, GRASS, "seed {seed}");
            }
        }
    }

    #[test]
    fn two_fires_that_light_the_same_block_do_what_one_does() {
        // Both fires, either side of a grass block, may light it in the same tick: it is a fire of
        // age 0 either way, written once for each, and nothing worse.
        let mut lit_by_both = 0;
        for seed in 0..600 {
            let mut fire = rule(seed);
            let mut world = Box3::default();
            world.blocks.insert((MAIN, at(3, 3, 3)), (GRASS, 0));
            light(&mut fire, &mut world, at(2, 3, 3));
            light(&mut fire, &mut world, at(4, 3, 3));
            let changed = fire.step(&mut world, everywhere);
            let writes_to_grass = changed
                .iter()
                .filter(|(_, voxel)| *voxel == at(3, 3, 3))
                .count();
            if writes_to_grass == 2 {
                lit_by_both += 1;
            }
            if writes_to_grass > 0 {
                assert_eq!(world.block(3, 3, 3), (FIRE, 0));
            } else {
                assert_eq!(world.block(3, 3, 3), (GRASS, 0));
            }
            assert!(fire.active().contains(MAIN, at(3, 3, 3)) == (writes_to_grass > 0));
        }
        assert!(lit_by_both > 0, "the case happened");
    }

    #[test]
    fn the_same_world_burns_the_same_way_and_another_seed_burns_differently() {
        let run = |seed| {
            let mut fire = rule(seed);
            let mut world = Box3::default();
            for x in 1..EDGE - 1 {
                for z in 1..EDGE - 1 {
                    world.blocks.insert((MAIN, at(x, 2, z)), (GRASS, 0));
                }
            }
            light(&mut fire, &mut world, at(1, 3, 1));
            let mut history = Vec::new();
            for _ in 0..200 {
                let changed = fire.step(&mut world, everywhere);
                history.push(changed);
            }
            (
                history,
                world
                    .blocks
                    .into_iter()
                    .collect::<std::collections::BTreeMap<_, _>>(),
            )
        };
        assert_eq!(run(11), run(11));
        assert_ne!(run(11), run(12));
    }

    #[test]
    fn fires_nobody_is_near_are_frozen_and_burn_on_when_someone_comes() {
        let mut fire = rule(3);
        let mut world = Box3::default();
        light(&mut fire, &mut world, at(3, 3, 3));
        let writes = world.writes;
        for _ in 0..200 {
            let changed = fire.step(&mut world, |_, _| false);
            assert!(changed.is_empty());
        }
        assert_eq!(world.writes, writes, "nothing was written");
        assert_eq!(world.block(3, 3, 3), (FIRE, 0), "it did not age");
        assert!(
            fire.active().contains(MAIN, at(3, 3, 3)),
            "and is still to be looked at"
        );
        let ticks = burn_out(&mut fire, &mut world);
        assert!(ticks >= 12);
        assert_eq!(world.count(FIRE), 0);
    }

    #[test]
    fn a_fire_that_was_put_out_or_has_nothing_to_read_is_forgotten() {
        let mut fire = rule(3);
        let mut world = Box3::default();
        light(&mut fire, &mut world, at(3, 3, 3));
        // Another block took its place (a player broke it, or built over it).
        world.blocks.insert((MAIN, at(3, 3, 3)), (STONE, 0));
        // And a fire in a voxel the world has nothing to read for.
        fire.ignite(MAIN, at(100, 0, 0));
        assert!(fire.step(&mut world, everywhere).is_empty());
        assert!(fire.active().is_empty());
        assert_eq!(world.block(3, 3, 3), (STONE, 0));
    }

    #[test]
    fn levels_burn_apart() {
        let second = GridId::new(1);
        let mut fire = rule(5);
        let mut world = Box3::default();
        world.blocks.insert((MAIN, at(3, 3, 3)), (FIRE, 0));
        world.blocks.insert((second, at(4, 3, 3)), (GRASS, 0));
        fire.ignite(MAIN, at(3, 3, 3));
        for _ in 0..400 {
            fire.step(&mut world, everywhere);
        }
        assert_eq!(world.blocks[&(second, at(4, 3, 3))], (GRASS, 0));
    }

    #[test]
    fn the_clock_counts_every_step_and_a_saved_rule_carries_on_exactly_as_if_it_never_stopped() {
        let levels = vec!["voxunami_cave:overworld".parse::<NamespacedId>().unwrap()];
        let index_of = |name: &NamespacedId| (*name == levels[0]).then_some(LevelIndex::MAIN);
        let scene = |fire: &mut Fire| {
            let mut world = Box3::default();
            for x in 1..EDGE - 1 {
                for z in 1..EDGE - 1 {
                    world.blocks.insert((MAIN, at(x, 2, z)), (GRASS, 0));
                }
            }
            light(fire, &mut world, at(1, 3, 1));
            world
        };

        let mut control = rule(21);
        let mut control_world = scene(&mut control);
        let mut interrupted = rule(21);
        let mut world = scene(&mut interrupted);
        for _ in 0..25 {
            control.step(&mut control_world, everywhere);
            interrupted.step(&mut world, everywhere);
        }
        assert_eq!(control.clock(), 25);
        assert!(
            !interrupted.active().is_empty(),
            "still burning when it is saved"
        );

        // Saved and loaded: a new rule from the saved clock and fires.
        let section = interrupted.to_section(&levels);
        assert_eq!(section.identifier, section_id());
        let (clock, active) = Fire::read_section(std::slice::from_ref(&section), index_of).unwrap();
        assert_eq!(clock, 25);
        let mut resumed = Fire::new(table(), FIRE, AIR, 21, clock, active);
        for _ in 0..150 {
            control.step(&mut control_world, everywhere);
            resumed.step(&mut world, everywhere);
        }
        let sorted = |world: &Box3| {
            world
                .blocks
                .iter()
                .map(|(k, v)| (*k, *v))
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        assert_eq!(sorted(&world), sorted(&control_world));
        assert_eq!(resumed.clock(), 175);
        assert_eq!(resumed.active(), control.active());

        // A steps with nothing burning still count.
        let mut idle = rule(1);
        idle.step(&mut Box3::default(), everywhere);
        assert_eq!(idle.clock(), 1);
    }

    #[test]
    fn a_saved_fire_that_is_newer_or_damaged_is_not_read() {
        let levels = vec!["voxunami_cave:overworld".parse::<NamespacedId>().unwrap()];
        let index_of = |name: &NamespacedId| (*name == levels[0]).then_some(LevelIndex::MAIN);
        let mut fire = rule(1);
        fire.ignite(MAIN, at(1, 2, 3));
        let good = fire.to_section(&levels);
        assert!(Fire::read_section(std::slice::from_ref(&good), index_of).is_some());
        for version in [0, SAVED_VERSION - 1, SAVED_VERSION + 1] {
            let other = Section {
                version,
                ..good.clone()
            };
            assert!(
                Fire::read_section(&[other], index_of).is_none(),
                "version {version}"
            );
        }
        for length in 0..good.payload.len() {
            let damaged = Section {
                payload: good.payload[..length].to_vec(),
                ..good.clone()
            };
            assert!(
                Fire::read_section(&[damaged], index_of).is_none(),
                "{length}"
            );
        }
        let mut trailing = good.clone();
        trailing.payload.push(7);
        assert!(Fire::read_section(&[trailing], index_of).is_none());
        assert!(Fire::read_section(&[], index_of).is_none());
    }

    fn scene() -> impl Strategy<Value = Vec<(i128, i128, i128, u32)>> {
        proptest::collection::vec((0..EDGE, 0..EDGE, 0..EDGE, 0u32..5), 0..80)
    }

    proptest! {
        /// Whatever the scene: every fire goes out, stone is never touched, the number of blocks
        /// that burn never grows, and no voxel ever holds a fire older than its lifetime.
        #[test]
        fn any_fire_goes_out_and_only_burns_what_burns(scene in scene(), seed in any::<u64>()) {
            let mut fire = rule(seed);
            let mut world = Box3::default();
            for &(x, y, z, block) in &scene {
                world.blocks.insert((MAIN, at(x, y, z)), (block, 0));
                if block == FIRE {
                    fire.ignite(MAIN, at(x, y, z));
                }
            }
            let stone: Vec<_> = world.blocks.iter().filter(|(_, b)| b.0 == STONE).map(|(k, _)| *k).collect();
            let mut burnable = world.count(GRASS) + world.count(WOOD);
            for _ in 0..3000 {
                fire.step(&mut world, everywhere);
                let now = world.count(GRASS) + world.count(WOOD);
                prop_assert!(now <= burnable);
                burnable = now;
                for (block, state) in world.blocks.values() {
                    if *block == FIRE {
                        prop_assert!(*state < FIRE_LIFETIME);
                    }
                }
                if fire.active().is_empty() {
                    break;
                }
            }
            prop_assert!(fire.active().is_empty());
            prop_assert_eq!(world.count(FIRE), 0);
            for key in stone {
                prop_assert_eq!(world.blocks[&key], (STONE, 0));
            }
        }
    }
}

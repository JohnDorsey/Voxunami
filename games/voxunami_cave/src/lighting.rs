//! Block light: the light that lamps give off, spreading through the air and the empty parts of
//! blocks, one voxel a tick, losing one level per voxel.
//!
//! This is a lattice rule in the sense of the design notes: the value of a voxel at the next tick
//! is a function of the values of the voxel and its six neighbours at this tick, and nothing else.
//! Every voxel that may change is worked out from the world as it stood at the start of the tick,
//! and only then are the changes written, so the order and the way the world is divided into bricks
//! cannot matter. Light that spreads therefore travels one voxel a tick (a lamp of 48 takes 48
//! ticks to light its whole reach), and light that has lost its source dies out the same way: a
//! level every tick or two, not at once.
//!
//! What a voxel's light is, once things have settled: it holds the light of its block's emission,
//! or one less than the light of the brightest neighbour that lets light out through the face
//! between them (a neighbour that is not a whole block, and a lamp, which lights every face), if the
//! voxel lets light in through that face. A voxel with no face open, a whole block, holds no light.
//! Whether a face is open is [`BlockTable::connections`]: air and see-through blocks are open on
//! every face, a slab or stair on the sides its empty part reaches. Light that enters a block by one
//! open face is taken to leave by any other: the stored value is one number per voxel, not one for
//! each face it came in by, so a block with a wall inside its cell between two open faces would let
//! light through that it should not. No block of the game has one.
//!
//! The rule only looks at voxels that may have changed: the ones next to a change, and the ones
//! next to a change that was just written. When nothing is pending the light is settled. A step can
//! be given a budget of voxels to look at; the voxels over it wait, in their fixed order, for the next
//! step, so that a great deal of light changing at once (many lamps) takes a few ticks longer
//! instead of one tick too long. The light it settles to is the same.

use block_models::{BlockTable, PropertyHandle};

use crate::block_properties;
use lattice::ActiveVoxels;
use namespaced_id::NamespacedId;
use voxel_space::{FaceDirection, GridId, VoxelCoordinate};

/// The identifier of the section the voxels still to be looked at are saved in.
pub fn section_id() -> NamespacedId {
    "voxunami_cave:light"
        .parse()
        .expect("constant identifier is valid")
}

/// The voxel next to `at` on `face`, if it is within the world's range.
fn step_to(at: VoxelCoordinate, face: FaceDirection) -> Option<VoxelCoordinate> {
    let [dx, dy, dz] = face.offset();
    at.checked_offset(dx, dy, dz)
}

/// The voxels of a world, as far as light needs to know.
pub trait LightWorld {
    /// The block and state of a voxel, or `None` where there is nothing to read (terrain this server
    /// cannot generate, the edge of the world), which lets no light in or out.
    fn block_and_state(&mut self, grid: GridId, at: VoxelCoordinate) -> Option<(u32, u32)>;

    /// The light stored in a voxel, 0 to 255; 0 where there is nothing to read.
    fn light(&mut self, grid: GridId, at: VoxelCoordinate) -> u8;

    /// Stores the light of a voxel. Only called for voxels that were just read.
    fn set_light(&mut self, grid: GridId, at: VoxelCoordinate, value: u8);
}

/// The light rule and the voxels it has yet to look at.
#[derive(Debug)]
pub struct Lighting {
    table: BlockTable,
    emission: PropertyHandle<u8>,
    /// Voxels whose light may be out of date, in a fixed order.
    pending: ActiveVoxels,
}

impl Lighting {
    /// Light for a world of these blocks, with nothing to look at: the world's light is taken to be
    /// settled.
    pub fn new(table: BlockTable) -> Self {
        Self {
            emission: block_properties::emission(&table),
            table,
            pending: ActiveVoxels::new(),
        }
    }

    /// Light that carries on from voxels that were still to be looked at, as when a world is
    /// loaded.
    pub fn with_pending(table: BlockTable, pending: ActiveVoxels) -> Self {
        Self {
            emission: block_properties::emission(&table),
            table,
            pending,
        }
    }

    /// The voxels still to be looked at, in the order they will be.
    pub fn pending(&self) -> &ActiveVoxels {
        &self.pending
    }

    /// Whether every voxel's light is what the rule says it should be.
    pub fn is_settled(&self) -> bool {
        self.pending.is_empty()
    }

    /// Tells the rule that the block or state at a voxel was changed from outside (a block was
    /// placed or broken), so that the voxel and its neighbours are looked at on the next step.
    pub fn voxel_changed(&mut self, grid: GridId, at: VoxelCoordinate) {
        self.pending.insert_with_neighbours(grid, at);
    }

    /// One tick: works out the light of every voxel that is pending from the world as it is now,
    /// then writes the ones that changed, and so has the voxels around them to look at next time.
    /// Returns how many voxels changed.
    pub fn step(&mut self, world: &mut impl LightWorld) -> usize {
        self.step_within(world, usize::MAX)
    }

    /// One tick, looking at no more than `budget` of the pending voxels, in their fixed order; the
    /// rest stay pending for the next step.
    pub fn step_within(&mut self, world: &mut impl LightWorld, budget: usize) -> usize {
        let mut looked_at = self.pending.take();
        if looked_at.len() > budget {
            for (grid, at) in looked_at.split_off(budget) {
                self.pending.insert(grid, at);
            }
        }
        let changes: Vec<_> = looked_at
            .into_iter()
            .filter_map(|(grid, at)| {
                let new = self.light_of(world, grid, at);
                (new != world.light(grid, at)).then_some((grid, at, new))
            })
            .collect();
        for &(grid, at, new) in &changes {
            world.set_light(grid, at, new);
            self.voxel_changed(grid, at);
        }
        changes.len()
    }

    /// The light a voxel should hold, given its neighbours as they are now.
    fn light_of(&self, world: &mut impl LightWorld, grid: GridId, at: VoxelCoordinate) -> u8 {
        let Some((block, state)) = world.block_and_state(grid, at) else {
            return 0;
        };
        let model = self.table.model_of(block, state);
        let connections = self.table.connections(model);
        if !FaceDirection::ALL
            .iter()
            .any(|&face| connections.is_open(face))
        {
            return 0; // a whole block holds no light, though it may give some
        }
        let mut value = self.table.get(self.emission, model);
        for face in FaceDirection::ALL {
            if !connections.is_open(face) {
                continue;
            }
            let Some(neighbour) = step_to(at, face) else {
                continue;
            };
            let Some((neighbour_block, neighbour_state)) = world.block_and_state(grid, neighbour)
            else {
                continue;
            };
            let neighbour_model = self.table.model_of(neighbour_block, neighbour_state);
            let neighbour_connections = self.table.connections(neighbour_model);
            let emission = self.table.get(self.emission, neighbour_model);
            if neighbour_connections.is_open(face.opposite()) || emission > 0 {
                // A lamp lights every face of itself, and holds no light if it is a whole block.
                let given = world.light(grid, neighbour).max(emission);
                value = value.max(given.saturating_sub(1));
            }
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Reverse;
    use std::collections::{BinaryHeap, HashMap};

    use block_models::{BlockDefinition, Shape};
    use proptest::prelude::*;

    use super::*;

    const AIR: u32 = 0;
    const STONE: u32 = 1;
    const GLASS: u32 = 2;
    /// States: 0 whole, 1 bottom slab, 2 top slab, 3 a stair rising toward +x.
    const ROCK: u32 = 3;
    /// A whole lamp of 20.
    const LAMP: u32 = 4;
    /// A lamp of 12 that is see-through, so it holds light itself.
    const GLASS_LAMP: u32 = 5;
    /// A whole lamp of the greatest light there is.
    const BRIGHTEST_LAMP: u32 = 6;

    const EDGE: i64 = 9;

    fn table() -> BlockTable {
        let air = BlockDefinition::air();
        let stone = BlockDefinition::cube([0.5; 3]);
        let glass = BlockDefinition::cube([0.7; 3]).with_see_through(true);
        let rock = BlockDefinition::cube([0.4; 3]).with_shapes(vec![
            Shape::full(),
            Shape::half(FaceDirection::NegativeY),
            Shape::half(FaceDirection::PositiveY),
            Shape::stair(FaceDirection::PositiveX),
        ]);
        let lamp = BlockDefinition::cube([1.0; 3]).with_property(&base_blocks::EMISSION, 20);
        let glass_lamp = BlockDefinition::cube([1.0; 3])
            .with_see_through(true)
            .with_property(&base_blocks::EMISSION, 12);
        let brightest_lamp =
            BlockDefinition::cube([1.0; 3]).with_property(&base_blocks::EMISSION, 255);
        block_properties::test_table(&[
            Some(&air),
            Some(&stone),
            Some(&glass),
            Some(&rock),
            Some(&lamp),
            Some(&glass_lamp),
            Some(&brightest_lamp),
        ])
    }

    /// A box of `EDGE` voxels along each side, from the origin, in which everything not set is air;
    /// outside it there is nothing to read.
    #[derive(Default)]
    struct Box3 {
        blocks: HashMap<(GridId, VoxelCoordinate), (u32, u32)>,
        light: HashMap<(GridId, VoxelCoordinate), u8>,
        reads: usize,
    }

    fn inside(at: VoxelCoordinate) -> bool {
        [at.x(), at.y(), at.z()]
            .iter()
            .all(|component| (0..EDGE).contains(component))
    }

    impl LightWorld for Box3 {
        fn block_and_state(&mut self, grid: GridId, at: VoxelCoordinate) -> Option<(u32, u32)> {
            self.reads += 1;
            inside(at).then(|| self.blocks.get(&(grid, at)).copied().unwrap_or((AIR, 0)))
        }

        fn light(&mut self, grid: GridId, at: VoxelCoordinate) -> u8 {
            self.light.get(&(grid, at)).copied().unwrap_or(0)
        }

        fn set_light(&mut self, grid: GridId, at: VoxelCoordinate, value: u8) {
            self.light.insert((grid, at), value);
        }
    }

    fn at(x: i64, y: i64, z: i64) -> VoxelCoordinate {
        VoxelCoordinate::new(x, y, z)
    }

    const MAIN: GridId = GridId::MAIN;

    impl Box3 {
        fn place(
            &mut self,
            voxel: VoxelCoordinate,
            block: u32,
            state: u32,
            lighting: &mut Lighting,
        ) {
            self.blocks.insert((MAIN, voxel), (block, state));
            lighting.voxel_changed(MAIN, voxel);
        }

        fn light_at(&self, x: i64, y: i64, z: i64) -> u8 {
            self.light.get(&(MAIN, at(x, y, z))).copied().unwrap_or(0)
        }

        /// Every voxel of the box that holds any light, with its light.
        fn lit(&self) -> Vec<(VoxelCoordinate, u8)> {
            let mut lit: Vec<_> = self
                .light
                .iter()
                .filter(|(_, value)| **value > 0)
                .map(|(&(_, voxel), &value)| (voxel, value))
                .collect();
            lit.sort();
            lit
        }
    }

    /// Steps until nothing is pending, returning how many steps it took.
    fn settle(lighting: &mut Lighting, world: &mut Box3) -> usize {
        for steps in 1..=2000 {
            lighting.step(world);
            if lighting.is_settled() {
                return steps;
            }
        }
        panic!("the light never settled");
    }

    /// The light the rule should settle to, worked out the other way round: from each source, a
    /// search that takes the brightest light to each voxel first.
    fn reference(world: &Box3, table: &BlockTable) -> Vec<(VoxelCoordinate, u8)> {
        let emission = block_properties::emission(table);
        let model_at = |voxel: VoxelCoordinate| {
            inside(voxel).then(|| {
                let (block, state) = world
                    .blocks
                    .get(&(MAIN, voxel))
                    .copied()
                    .unwrap_or((AIR, 0));
                (block, table.model_of(block, state))
            })
        };
        let mut best: HashMap<VoxelCoordinate, u8> = HashMap::new();
        let mut queue = BinaryHeap::new();
        // What a voxel gives to the neighbour on `face` of it, and whether it lets light out that way.
        let gives = |voxel: VoxelCoordinate, face: FaceDirection, value: u8| -> Option<u8> {
            let (_, model) = model_at(voxel)?;
            let lets_out = table.connections(model).is_open(face) || table.get(emission, model) > 0;
            lets_out.then_some(value.max(table.get(emission, model)))
        };
        let mut all = Vec::new();
        for x in 0..EDGE {
            for y in 0..EDGE {
                for z in 0..EDGE {
                    all.push(at(x, y, z));
                }
            }
        }
        for &voxel in &all {
            let (_, model) = model_at(voxel).unwrap();
            let passes = FaceDirection::ALL
                .iter()
                .any(|&face| table.connections(model).is_open(face));
            if passes
                && table.get(emission, model) > 0
                && best
                    .get(&voxel)
                    .is_none_or(|&known| table.get(emission, model) > known)
            {
                best.insert(voxel, table.get(emission, model));
                queue.push((table.get(emission, model), Reverse(voxel)));
            }
            // A whole lamp gives light to the voxels around it without holding any.
            if !passes && table.get(emission, model) > 0 {
                for face in FaceDirection::ALL {
                    if let Some(neighbour) = step_to(voxel, face)
                        && let Some((_, neighbour_model)) = model_at(neighbour)
                        && table.connections(neighbour_model).is_open(face.opposite())
                        && table.get(emission, model) > 1
                    {
                        let value = table.get(emission, model) - 1;
                        if best.get(&neighbour).is_none_or(|&known| value > known) {
                            best.insert(neighbour, value);
                            queue.push((value, Reverse(neighbour)));
                        }
                    }
                }
            }
        }
        while let Some((value, Reverse(voxel))) = queue.pop() {
            if best.get(&voxel) != Some(&value) || value <= 1 {
                continue;
            }
            for face in FaceDirection::ALL {
                let Some(neighbour) = step_to(voxel, face) else {
                    continue;
                };
                let Some((_, neighbour_model)) = model_at(neighbour) else {
                    continue;
                };
                if !table.connections(neighbour_model).is_open(face.opposite()) {
                    continue;
                }
                if gives(voxel, face, value).is_none() {
                    continue;
                }
                let candidate = value - 1;
                if best.get(&neighbour).is_none_or(|&known| candidate > known) {
                    best.insert(neighbour, candidate);
                    queue.push((candidate, Reverse(neighbour)));
                }
            }
        }
        let mut lit: Vec<_> = best.into_iter().filter(|&(_, value)| value > 0).collect();
        lit.sort();
        lit
    }

    #[test]
    fn a_lamp_in_open_air_lights_a_diamond_one_level_dimmer_for_every_voxel_away() {
        let mut lighting = Lighting::new(table());
        let mut world = Box3::default();
        world.place(at(4, 4, 4), LAMP, 0, &mut lighting);
        settle(&mut lighting, &mut world);
        // The lamp is a whole block and holds none; its neighbours hold 19, and so on.
        assert_eq!(world.light_at(4, 4, 4), 0);
        assert_eq!(world.light_at(5, 4, 4), 19);
        assert_eq!(world.light_at(4, 4, 3), 19);
        assert_eq!(
            world.light_at(5, 5, 4),
            18,
            "two voxels away along two axes"
        );
        assert_eq!(world.light_at(6, 5, 3), 20 - 4);
        assert_eq!(
            world.light_at(8, 8, 8),
            20 - 12,
            "the far corner of the box"
        );
        assert_eq!(world.lit(), reference(&world, &table()));
    }

    #[test]
    fn the_brightest_lamp_gives_every_level_down_from_254() {
        let mut lighting = Lighting::new(table());
        let mut world = Box3::default();
        world.place(at(0, 0, 0), BRIGHTEST_LAMP, 0, &mut lighting);
        settle(&mut lighting, &mut world);
        assert_eq!(world.light_at(1, 0, 0), 254);
        assert_eq!(world.light_at(8, 8, 8), 255 - 24);
        assert_eq!(world.lit(), reference(&world, &table()));
    }

    #[test]
    fn a_step_with_a_budget_looks_at_no_more_voxels_than_it_and_the_rest_wait_their_turn() {
        let mut lighting = Lighting::new(table());
        let mut world = Box3::default();
        world.place(at(4, 4, 4), LAMP, 0, &mut lighting);
        assert_eq!(lighting.pending().len(), 7);
        // Three of the seven are looked at, and the four others are still waiting.
        lighting.step_within(&mut world, 3);
        assert!(lighting.pending().len() >= 4);
        // The voxels wait in their fixed order: the first ones go first.
        let mut unlimited = Lighting::new(table());
        let mut other = Box3::default();
        other.place(at(4, 4, 4), LAMP, 0, &mut unlimited);
        let order: Vec<_> = unlimited.pending().iter().collect();
        let mut with_budget = Lighting::new(table());
        let mut third = Box3::default();
        third.place(at(4, 4, 4), LAMP, 0, &mut with_budget);
        with_budget.step_within(&mut third, 1);
        // Only the first voxel (the lowest) was looked at: the six others are still pending.
        let waiting: Vec<_> = with_budget.pending().iter().collect();
        assert!(order[1..].iter().all(|voxel| waiting.contains(voxel)));
        // However small the budget, the light settles to what it would have been.
        for budget in [1, 2, 5, 40] {
            let mut lighting = Lighting::new(table());
            let mut world = Box3::default();
            world.place(at(4, 4, 4), LAMP, 0, &mut lighting);
            for _ in 0..20_000 {
                lighting.step_within(&mut world, budget);
                if lighting.is_settled() {
                    break;
                }
            }
            assert!(lighting.is_settled(), "budget {budget}");
            assert_eq!(world.lit(), reference(&world, &table()), "budget {budget}");
        }
    }

    #[test]
    fn light_spreads_one_voxel_a_tick() {
        let mut lighting = Lighting::new(table());
        let mut world = Box3::default();
        world.place(at(0, 0, 0), LAMP, 0, &mut lighting);
        for tick in 1..=6 {
            let changed = lighting.step(&mut world);
            assert!(changed > 0, "tick {tick}");
            // The voxel that is `tick` away along an axis is lit now, and the one beyond it is not.
            assert_eq!(world.light_at(tick, 0, 0), (20 - tick) as u8, "tick {tick}");
            assert_eq!(world.light_at(tick + 1, 0, 0), 0, "tick {tick}");
        }
    }

    #[test]
    fn a_settled_world_has_nothing_pending_and_stepping_changes_nothing() {
        let mut lighting = Lighting::new(table());
        let mut world = Box3::default();
        world.place(at(2, 2, 2), GLASS_LAMP, 0, &mut lighting);
        settle(&mut lighting, &mut world);
        let before = world.lit();
        assert!(lighting.is_settled());
        assert_eq!(lighting.pending().len(), 0);
        let reads = world.reads;
        assert_eq!(lighting.step(&mut world), 0);
        assert_eq!(world.reads, reads, "a settled world is not even looked at");
        assert_eq!(world.lit(), before);
        // Marking a voxel looks at it and its neighbours, finds nothing to change, and settles at once.
        lighting.voxel_changed(MAIN, at(5, 5, 5));
        assert_eq!(lighting.pending().len(), 7);
        assert_eq!(settle(&mut lighting, &mut world), 1);
        assert_eq!(world.lit(), before);
    }

    #[test]
    fn a_lamp_that_is_see_through_holds_its_own_light() {
        let mut lighting = Lighting::new(table());
        let mut world = Box3::default();
        world.place(at(4, 4, 4), GLASS_LAMP, 0, &mut lighting);
        settle(&mut lighting, &mut world);
        assert_eq!(world.light_at(4, 4, 4), 12);
        assert_eq!(world.light_at(5, 4, 4), 11);
        assert_eq!(world.lit(), reference(&world, &table()));
    }

    #[test]
    fn whole_blocks_stop_light_and_hold_none() {
        let mut lighting = Lighting::new(table());
        let mut world = Box3::default();
        // A wall of stone across the box, with the lamp on one side.
        for y in 0..EDGE {
            for z in 0..EDGE {
                world.place(at(6, y, z), STONE, 0, &mut lighting);
            }
        }
        world.place(at(2, 4, 4), LAMP, 0, &mut lighting);
        settle(&mut lighting, &mut world);
        assert_eq!(world.light_at(5, 4, 4), 20 - 3);
        assert_eq!(world.light_at(6, 4, 4), 0, "the wall holds no light");
        assert_eq!(world.light_at(7, 4, 4), 0, "and nothing gets past it");
        assert_eq!(world.light_at(8, 8, 8), 0);
        assert_eq!(world.lit(), reference(&world, &table()));
    }

    #[test]
    fn glass_lets_light_through_and_shapes_let_it_through_only_where_they_are_open() {
        // Glass in the way costs a level per voxel like air.
        let mut lighting = Lighting::new(table());
        let mut world = Box3::default();
        for y in 0..EDGE {
            for z in 0..EDGE {
                world.place(at(6, y, z), GLASS, 0, &mut lighting);
            }
        }
        world.place(at(2, 4, 4), LAMP, 0, &mut lighting);
        settle(&mut lighting, &mut world);
        assert_eq!(world.light_at(6, 4, 4), 20 - 4);
        assert_eq!(world.light_at(7, 4, 4), 20 - 5);
        assert_eq!(world.lit(), reference(&world, &table()));

        // A bottom slab leaves its top open: light crosses above it but not through its bottom.
        let mut lighting = Lighting::new(table());
        let mut world = Box3::default();
        world.place(at(4, 4, 4), ROCK, 1, &mut lighting);
        world.place(at(4, 6, 4), LAMP, 0, &mut lighting);
        settle(&mut lighting, &mut world);
        assert_eq!(world.light_at(4, 5, 4), 19, "above the slab");
        assert_eq!(
            world.light_at(4, 4, 4),
            18,
            "the slab holds light, entering by its open top"
        );
        // Its underside is closed, so the light below it is what went round: five voxels from the lamp.
        assert_eq!(world.light_at(4, 3, 4), 20 - 5);
        assert_eq!(world.lit(), reference(&world, &table()));
    }

    #[test]
    fn a_closed_face_lets_no_light_in_or_out() {
        // A lamp right under a bottom slab, whose underside is closed: the slab gets its light from
        // the side, a voxel farther, and not the 19 it would from below.
        let mut lighting = Lighting::new(table());
        let mut world = Box3::default();
        world.place(at(4, 3, 4), LAMP, 0, &mut lighting);
        world.place(at(4, 4, 4), ROCK, 1, &mut lighting);
        settle(&mut lighting, &mut world);
        assert_eq!(world.light_at(3, 3, 4), 19, "beside the lamp");
        assert_eq!(world.light_at(3, 4, 4), 18, "beside the slab");
        assert_eq!(
            world.light_at(4, 4, 4),
            17,
            "through its side, not its underside"
        );
        assert_eq!(world.lit(), reference(&world, &table()));
    }

    #[test]
    fn taking_the_lamp_away_darkens_the_world_again() {
        let mut lighting = Lighting::new(table());
        let mut world = Box3::default();
        world.place(at(4, 4, 4), LAMP, 0, &mut lighting);
        settle(&mut lighting, &mut world);
        assert!(!world.lit().is_empty());
        world.place(at(4, 4, 4), AIR, 0, &mut lighting);
        let steps = settle(&mut lighting, &mut world);
        assert!(world.lit().is_empty(), "{:?}", world.lit());
        assert!(
            steps < 60,
            "darkness takes about as long as the light did: {steps}"
        );
    }

    #[test]
    fn building_a_wall_and_opening_it_again_changes_the_light_behind_it() {
        let mut lighting = Lighting::new(table());
        let mut world = Box3::default();
        world.place(at(1, 4, 4), LAMP, 0, &mut lighting);
        for y in 0..EDGE {
            for z in 0..EDGE {
                world.place(at(4, y, z), STONE, 0, &mut lighting);
            }
        }
        settle(&mut lighting, &mut world);
        assert_eq!(world.light_at(6, 4, 4), 0);
        world.place(at(4, 4, 4), AIR, 0, &mut lighting);
        settle(&mut lighting, &mut world);
        assert_eq!(
            world.light_at(4, 4, 4),
            20 - 3,
            "light comes through the hole"
        );
        assert_eq!(world.light_at(6, 4, 4), 20 - 5);
        assert_eq!(world.lit(), reference(&world, &table()));
        world.place(at(4, 4, 4), STONE, 0, &mut lighting);
        settle(&mut lighting, &mut world);
        assert_eq!(world.light_at(6, 4, 4), 0);
        assert_eq!(world.lit(), reference(&world, &table()));
    }

    #[test]
    fn levels_do_not_light_each_other() {
        let second = GridId::new(1);
        let mut lighting = Lighting::new(table());
        let mut world = Box3::default();
        world.blocks.insert((second, at(4, 4, 4)), (LAMP, 0));
        lighting.voxel_changed(second, at(4, 4, 4));
        settle(&mut lighting, &mut world);
        assert_eq!(world.light.get(&(second, at(5, 4, 4))), Some(&19));
        assert_eq!(world.light_at(5, 4, 4), 0);
    }

    #[test]
    fn light_that_is_still_spreading_when_the_rule_is_rebuilt_carries_on_the_same_way() {
        let run = |interrupt_after: Option<usize>| {
            let mut lighting = Lighting::new(table());
            let mut world = Box3::default();
            world.place(at(4, 4, 4), LAMP, 0, &mut lighting);
            if let Some(ticks) = interrupt_after {
                for _ in 0..ticks {
                    lighting.step(&mut world);
                }
                let pending = lighting.pending().clone();
                assert!(!pending.is_empty());
                lighting = Lighting::with_pending(table(), pending);
            }
            settle(&mut lighting, &mut world);
            world.lit()
        };
        assert_eq!(run(Some(3)), run(None));
        assert_eq!(run(Some(11)), run(None));
    }

    /// A scene of blocks in the box: (position, block, state).
    fn scene() -> impl Strategy<Value = Vec<(i64, i64, i64, u32, u32)>> {
        proptest::collection::vec((0..EDGE, 0..EDGE, 0..EDGE, 0u32..6, 0u32..4), 0..120)
    }

    fn build(
        scene: &[(i64, i64, i64, u32, u32)],
        shift: [i64; 3],
        lighting: &mut Lighting,
        world: &mut Box3,
    ) {
        for &(x, y, z, block, state) in scene {
            world.place(
                at(x + shift[0], y + shift[1], z + shift[2]),
                block,
                state,
                lighting,
            );
        }
    }

    proptest! {
        /// Whatever the scene, the light settles, to what the search from the sources finds.
        #[test]
        fn any_scene_settles_to_the_light_a_search_from_the_sources_finds(scene in scene()) {
            let mut lighting = Lighting::new(table());
            let mut world = Box3::default();
            build(&scene, [0; 3], &mut lighting, &mut world);
            settle(&mut lighting, &mut world);
            prop_assert_eq!(world.lit(), reference(&world, &table()));
        }

        /// Whatever the scene and however small the budget of voxels looked at in a tick, the light
        /// settles to what the search from the sources finds.
        #[test]
        fn a_budget_only_makes_the_light_slower(scene in scene(), budget in 1usize..60) {
            let mut lighting = Lighting::new(table());
            let mut world = Box3::default();
            build(&scene, [0; 3], &mut lighting, &mut world);
            for _ in 0..50_000 {
                lighting.step_within(&mut world, budget);
                if lighting.is_settled() {
                    break;
                }
            }
            prop_assert!(lighting.is_settled());
            prop_assert_eq!(world.lit(), reference(&world, &table()));
        }

        /// Edits made while light is still spreading, at any moment, settle to the same light as if
        /// the blocks had been there from the start: the light depends on the blocks and nothing else.
        #[test]
        fn edits_made_while_the_light_is_spreading_settle_to_the_same_light(
            scene in scene(),
            split in 0usize..120,
            ticks in 0usize..40,
        ) {
            let split = split.min(scene.len());
            let mut lighting = Lighting::new(table());
            let mut world = Box3::default();
            build(&scene[..split], [0; 3], &mut lighting, &mut world);
            for _ in 0..ticks {
                lighting.step(&mut world);
            }
            build(&scene[split..], [0; 3], &mut lighting, &mut world);
            settle(&mut lighting, &mut world);
            prop_assert_eq!(world.lit(), reference(&world, &table()));
        }

        /// The brick a voxel is in is nothing to the rule: a scene moved by any offset (so that it
        /// lies across brick boundaries differently) settles to the same light, moved the same way.
        /// The offset is a multiple of the size of the box's own world, so the scene is moved inside a
        /// larger world with the same unreadable edge.
        #[test]
        fn light_does_not_depend_on_where_the_bricks_fall(scene in scene(), shift in (0i64..32, 0i64..32, 0i64..32)) {
            let shift = [shift.0, shift.1, shift.2];
            let mut lighting = Lighting::new(table());
            let mut world = Box3::default();
            build(&scene, [0; 3], &mut lighting, &mut world);
            settle(&mut lighting, &mut world);
            let expected = world.lit();

            // The same scene in a world whose readable box is moved by the shift.
            let mut moved_lighting = Lighting::new(table());
            let mut moved = ShiftedBox { inner: Box3::default(), shift };
            for &(x, y, z, block, state) in &scene {
                let voxel = at(x + shift[0], y + shift[1], z + shift[2]);
                moved.inner.blocks.insert((MAIN, at(x, y, z)), (block, state));
                moved_lighting.voxel_changed(MAIN, voxel);
            }
            for _ in 0..2000 {
                moved_lighting.step(&mut moved);
                if moved_lighting.is_settled() {
                    break;
                }
            }
            prop_assert!(moved_lighting.is_settled());
            let mut lit: Vec<_> = moved
                .inner
                .light
                .iter()
                .filter(|(_, value)| **value > 0)
                .map(|(&(_, voxel), &value)| (voxel, value))
                .collect();
            lit.sort();
            prop_assert_eq!(lit, expected);
        }
    }

    /// The same box, with every voxel the rule asks about moved by `shift` before it is looked up, so
    /// that the rule sees the scene at another place in the world.
    struct ShiftedBox {
        inner: Box3,
        shift: [i64; 3],
    }

    impl ShiftedBox {
        fn unshifted(&self, at: VoxelCoordinate) -> VoxelCoordinate {
            VoxelCoordinate::new(
                at.x() - self.shift[0],
                at.y() - self.shift[1],
                at.z() - self.shift[2],
            )
        }
    }

    impl LightWorld for ShiftedBox {
        fn block_and_state(&mut self, grid: GridId, at: VoxelCoordinate) -> Option<(u32, u32)> {
            let at = self.unshifted(at);
            self.inner.block_and_state(grid, at)
        }

        fn light(&mut self, grid: GridId, at: VoxelCoordinate) -> u8 {
            let at = self.unshifted(at);
            self.inner.light(grid, at)
        }

        fn set_light(&mut self, grid: GridId, at: VoxelCoordinate, value: u8) {
            let at = self.unshifted(at);
            self.inner.set_light(grid, at, value);
        }
    }
}

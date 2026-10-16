//! How pushed blocks travel, meet and merge.
//!
//! A push makes a [`Mover`] of a block. Every tick each mover takes one step along its direction:
//!
//! - into an empty voxel: the block moves there;
//! - into a block with an equal number: the two merge into one block of the next number, which
//!   stays where the struck block was, and the motion ends;
//! - into any other block, or into something that is not a numbered block at all (terrain this
//!   program cannot read, the edge of the world): the motion ends, or for a material that bounces,
//!   the block turns around.
//!
//! A mover has a limited number of steps (its material's range), which every tick uses up whether
//! the block moved or bounced, so every motion ends.
//!
//! Movers are entities, not cells of a lattice: they act one at a time, in the order they started
//! moving, each seeing what the ones before it did this tick. That order is part of the rules, since
//! it decides what happens when blocks meet in the same tick, and it is the same on every machine.
//! Blocks never push each other.
//!
//! The rules see the world only through [`BlockField`], so they run the same on a server's world and
//! on the plain maps the tests use.

use voxel_space::{FaceDirection, VoxelCoordinate};

use crate::numbered_block::NumberedBlock;

/// What is at a voxel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Occupant {
    Empty,
    Block(NumberedBlock),
    /// Something blocks can neither enter nor merge with.
    Obstacle,
}

/// The voxels of a world, as far as moving blocks need to know.
pub trait BlockField {
    fn occupant(&mut self, at: VoxelCoordinate) -> Occupant;

    /// Puts a block at a voxel, or empties it. Only called for voxels that were just seen to be
    /// empty or to hold a block.
    fn place(&mut self, at: VoxelCoordinate, block: Option<NumberedBlock>);
}

/// A block that is moving. The block itself is in the world, at `position`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mover {
    pub position: VoxelCoordinate,
    pub direction: FaceDirection,
    /// How many more steps the block takes at most; always at least 1 for a block in motion.
    pub steps_left: u32,
}

/// Every block in motion, in the order they started.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Movers {
    movers: Vec<Mover>,
}

impl Movers {
    pub fn new() -> Self {
        Self::default()
    }

    /// Blocks that were moving when a world was saved. Entries that could not move (no steps left)
    /// are dropped.
    pub fn from_saved(movers: impl IntoIterator<Item = Mover>) -> Self {
        Self {
            movers: movers
                .into_iter()
                .filter(|mover| mover.steps_left > 0)
                .collect(),
        }
    }

    pub fn as_slice(&self) -> &[Mover] {
        &self.movers
    }

    pub fn len(&self) -> usize {
        self.movers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.movers.is_empty()
    }

    /// Pushes the block at `position` toward `direction`, with the full range of its material. A
    /// block that is already moving is pushed anew, keeping its place in the order. Returns whether
    /// there was a block to push.
    pub fn push_block(
        &mut self,
        field: &mut impl BlockField,
        position: VoxelCoordinate,
        direction: FaceDirection,
    ) -> bool {
        let Occupant::Block(block) = field.occupant(position) else {
            return false;
        };
        let pushed = Mover {
            position,
            direction,
            steps_left: block.material.motion().range,
        };
        match self
            .movers
            .iter_mut()
            .find(|mover| mover.position == position)
        {
            Some(moving) => *moving = pushed,
            None => self.movers.push(pushed),
        }
        true
    }

    /// Takes one step with every mover. Returns the blocks the merges of this tick made, in the
    /// order they happened.
    pub fn advance(&mut self, field: &mut impl BlockField) -> Vec<NumberedBlock> {
        let mut merges = Vec::new();
        let mut still_moving = Vec::with_capacity(self.movers.len());
        for mut mover in std::mem::take(&mut self.movers) {
            if step(&mut mover, field, &mut merges) {
                still_moving.push(mover);
            }
        }
        self.movers = still_moving;
        merges
    }
}

/// One tick of one mover. Returns whether it is still moving afterwards.
fn step(mover: &mut Mover, field: &mut impl BlockField, merges: &mut Vec<NumberedBlock>) -> bool {
    // Something other than this program took the block away: nothing is moving any more.
    let Occupant::Block(block) = field.occupant(mover.position) else {
        return false;
    };
    let Some(steps_left) = mover.steps_left.checked_sub(1) else {
        return false;
    };
    mover.steps_left = steps_left;
    let [dx, dy, dz] = mover.direction.offset();
    let ahead = mover
        .position
        .checked_offset(dx, dy, dz)
        .map(|target| (target, field.occupant(target)));
    match ahead {
        Some((target, Occupant::Empty)) => {
            field.place(mover.position, None);
            field.place(target, Some(block));
            mover.position = target;
        }
        Some((target, Occupant::Block(struck))) if block.merges_with(struck) => {
            let merged = block.merged_into(struck);
            field.place(mover.position, None);
            field.place(target, Some(merged));
            merges.push(merged);
            return false;
        }
        _ if block.material.motion().bounces => mover.direction = mover.direction.opposite(),
        _ => return false,
    }
    steps_left > 0
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use proptest::prelude::*;
    use voxel_space::VOXEL_COORDINATE_MAXIMUM;

    use super::*;
    use crate::material::{COASTING_RANGE, Material};

    #[derive(Default)]
    struct TestField {
        blocks: HashMap<VoxelCoordinate, NumberedBlock>,
        obstacles: HashSet<VoxelCoordinate>,
    }

    impl BlockField for TestField {
        fn occupant(&mut self, at: VoxelCoordinate) -> Occupant {
            if self.obstacles.contains(&at) {
                Occupant::Obstacle
            } else {
                self.blocks
                    .get(&at)
                    .map_or(Occupant::Empty, |block| Occupant::Block(*block))
            }
        }

        fn place(&mut self, at: VoxelCoordinate, block: Option<NumberedBlock>) {
            match block {
                Some(block) => self.blocks.insert(at, block),
                None => self.blocks.remove(&at),
            };
        }
    }

    impl TestField {
        fn with(blocks: &[((i64, i64, i64), NumberedBlock)]) -> Self {
            let mut field = Self::default();
            for &((x, y, z), block) in blocks {
                field.blocks.insert(VoxelCoordinate::new(x, y, z), block);
            }
            field
        }

        fn at(&self, x: i64, y: i64, z: i64) -> Option<NumberedBlock> {
            self.blocks.get(&VoxelCoordinate::new(x, y, z)).copied()
        }

        fn total_value(&self) -> u64 {
            self.blocks.values().map(|block| block.value()).sum()
        }
    }

    fn at(x: i64, y: i64, z: i64) -> VoxelCoordinate {
        VoxelCoordinate::new(x, y, z)
    }

    fn stone(exponent: u8) -> NumberedBlock {
        NumberedBlock::new(Material::Stone, exponent)
    }

    fn ice(exponent: u8) -> NumberedBlock {
        NumberedBlock::new(Material::Ice, exponent)
    }

    fn spring_ice(exponent: u8) -> NumberedBlock {
        NumberedBlock::new(Material::SpringIce, exponent)
    }

    /// Advances until nothing moves, returning how many ticks it took and every merge made.
    fn run_to_rest(movers: &mut Movers, field: &mut TestField) -> (u32, Vec<NumberedBlock>) {
        let mut merges = Vec::new();
        for tick in 1..=1000 {
            merges.extend(movers.advance(field));
            if movers.is_empty() {
                return (tick, merges);
            }
        }
        panic!("the blocks never came to rest");
    }

    #[test]
    fn stone_moves_one_step_into_empty_space_and_stops() {
        let mut field = TestField::with(&[((0, 0, 0), stone(3))]);
        let mut movers = Movers::new();
        assert!(movers.push_block(&mut field, at(0, 0, 0), FaceDirection::PositiveZ));
        assert_eq!(movers.len(), 1);
        let (ticks, merges) = run_to_rest(&mut movers, &mut field);
        assert_eq!((ticks, merges.len()), (1, 0));
        assert_eq!(field.at(0, 0, 0), None);
        assert_eq!(field.at(0, 0, 1), Some(stone(3)));
        assert_eq!(field.blocks.len(), 1);
    }

    #[test]
    fn stone_does_not_push_the_block_in_front_of_it() {
        let mut field = TestField::with(&[((0, 0, 0), stone(1)), ((1, 0, 0), stone(2))]);
        let mut movers = Movers::new();
        movers.push_block(&mut field, at(0, 0, 0), FaceDirection::PositiveX);
        let (ticks, merges) = run_to_rest(&mut movers, &mut field);
        assert_eq!((ticks, merges.len()), (1, 0));
        assert_eq!(field.at(0, 0, 0), Some(stone(1)));
        assert_eq!(field.at(1, 0, 0), Some(stone(2)));
    }

    #[test]
    fn stone_merges_with_an_equal_block_whatever_it_is_made_of() {
        let mut field = TestField::with(&[((0, 0, 0), stone(2)), ((1, 0, 0), ice(2))]);
        let mut movers = Movers::new();
        movers.push_block(&mut field, at(0, 0, 0), FaceDirection::PositiveX);
        let (_, merges) = run_to_rest(&mut movers, &mut field);
        // The struck block stays, so the result is ice.
        assert_eq!(merges, [ice(3)]);
        assert_eq!(field.at(0, 0, 0), None);
        assert_eq!(field.at(1, 0, 0), Some(ice(3)));
        assert_eq!(field.blocks.len(), 1);
    }

    #[test]
    fn ice_coasts_until_it_meets_a_different_block_and_stops_beside_it() {
        let mut field = TestField::with(&[((0, 0, 0), ice(1)), ((5, 0, 0), stone(2))]);
        let mut movers = Movers::new();
        movers.push_block(&mut field, at(0, 0, 0), FaceDirection::PositiveX);
        for expected in 1..=4 {
            movers.advance(&mut field);
            assert_eq!(
                field.at(expected, 0, 0),
                Some(ice(1)),
                "after {expected} ticks"
            );
            assert_eq!(movers.len(), 1);
        }
        let (ticks, merges) = run_to_rest(&mut movers, &mut field);
        assert_eq!((ticks, merges.len()), (1, 0));
        assert_eq!(field.at(4, 0, 0), Some(ice(1)));
        assert_eq!(
            field.at(5, 0, 0),
            Some(stone(2)),
            "the struck block is not pushed"
        );
        assert_eq!(field.blocks.len(), 2);
    }

    #[test]
    fn ice_merges_with_an_equal_block_it_meets() {
        let mut field = TestField::with(&[((0, 0, 0), ice(2)), ((3, 0, 0), stone(2))]);
        let mut movers = Movers::new();
        movers.push_block(&mut field, at(0, 0, 0), FaceDirection::PositiveX);
        let (ticks, merges) = run_to_rest(&mut movers, &mut field);
        assert_eq!(ticks, 3);
        assert_eq!(merges, [stone(3)]);
        assert_eq!(field.blocks.len(), 1);
        assert_eq!(field.at(3, 0, 0), Some(stone(3)));
    }

    #[test]
    fn ice_that_meets_nothing_stops_when_its_range_is_used_up() {
        let mut field = TestField::with(&[((0, 0, 0), ice(1))]);
        let mut movers = Movers::new();
        movers.push_block(&mut field, at(0, 0, 0), FaceDirection::NegativeY);
        for _ in 0..COASTING_RANGE - 1 {
            movers.advance(&mut field);
        }
        assert_eq!(movers.len(), 1);
        movers.advance(&mut field);
        assert!(movers.is_empty());
        let end = -i64::from(COASTING_RANGE);
        assert_eq!(field.at(0, end, 0), Some(ice(1)));
        assert_eq!(field.blocks.len(), 1);
    }

    #[test]
    fn spring_ice_bounces_off_a_block_and_comes_back_until_its_range_is_used_up() {
        let mut field = TestField::with(&[((0, 0, 0), spring_ice(1)), ((5, 0, 0), stone(3))]);
        let mut movers = Movers::new();
        movers.push_block(&mut field, at(0, 0, 0), FaceDirection::PositiveX);
        for _ in 0..4 {
            movers.advance(&mut field);
        }
        assert_eq!(field.at(4, 0, 0), Some(spring_ice(1)));
        // The bounce turns it around in place and costs the tick.
        movers.advance(&mut field);
        assert_eq!(field.at(4, 0, 0), Some(spring_ice(1)));
        assert_eq!(movers.as_slice()[0].direction, FaceDirection::NegativeX);
        assert_eq!(movers.as_slice()[0].steps_left, COASTING_RANGE - 5);
        let (ticks, merges) = run_to_rest(&mut movers, &mut field);
        assert_eq!((ticks, merges.len()), (COASTING_RANGE - 5, 0));
        let end = 4 - i64::from(COASTING_RANGE - 5);
        assert_eq!(field.at(end, 0, 0), Some(spring_ice(1)));
        assert_eq!(field.at(5, 0, 0), Some(stone(3)), "the wall is not moved");
    }

    #[test]
    fn spring_ice_bounces_off_what_is_not_a_block_and_merges_with_an_equal_one() {
        let mut field = TestField::with(&[((0, 0, 0), spring_ice(2))]);
        field.obstacles.insert(at(1, 0, 0));
        let mut movers = Movers::new();
        movers.push_block(&mut field, at(0, 0, 0), FaceDirection::PositiveX);
        movers.advance(&mut field);
        assert_eq!(movers.as_slice()[0].direction, FaceDirection::NegativeX);
        assert_eq!(field.at(0, 0, 0), Some(spring_ice(2)));

        let mut field = TestField::with(&[((0, 0, 0), spring_ice(2)), ((3, 0, 0), ice(2))]);
        let mut movers = Movers::new();
        movers.push_block(&mut field, at(0, 0, 0), FaceDirection::PositiveX);
        let (_, merges) = run_to_rest(&mut movers, &mut field);
        assert_eq!(merges, [ice(3)]);
        assert_eq!(field.blocks.len(), 1);
        assert_eq!(field.at(3, 0, 0), Some(ice(3)));
    }

    #[test]
    fn blocks_stop_at_obstacles_and_at_the_edge_of_the_world() {
        let mut field = TestField::with(&[((0, 0, 0), ice(1))]);
        field.obstacles.insert(at(3, 0, 0));
        let mut movers = Movers::new();
        movers.push_block(&mut field, at(0, 0, 0), FaceDirection::PositiveX);
        run_to_rest(&mut movers, &mut field);
        assert_eq!(field.at(2, 0, 0), Some(ice(1)));

        let edge = VOXEL_COORDINATE_MAXIMUM;
        let mut field = TestField::with(&[((edge, 0, 0), ice(1))]);
        let mut movers = Movers::new();
        movers.push_block(&mut field, at(edge, 0, 0), FaceDirection::PositiveX);
        let (ticks, _) = run_to_rest(&mut movers, &mut field);
        assert_eq!(ticks, 1);
        assert_eq!(field.at(edge, 0, 0), Some(ice(1)));
    }

    #[test]
    fn only_a_block_can_be_pushed() {
        let mut field = TestField::default();
        field.obstacles.insert(at(1, 1, 1));
        let mut movers = Movers::new();
        assert!(!movers.push_block(&mut field, at(0, 0, 0), FaceDirection::PositiveX));
        assert!(!movers.push_block(&mut field, at(1, 1, 1), FaceDirection::PositiveX));
        assert!(movers.is_empty());
    }

    #[test]
    fn pushing_a_moving_block_again_gives_it_a_new_direction_and_a_new_range() {
        let mut field = TestField::with(&[((0, 0, 0), ice(1))]);
        let mut movers = Movers::new();
        movers.push_block(&mut field, at(0, 0, 0), FaceDirection::PositiveX);
        movers.advance(&mut field);
        movers.advance(&mut field);
        assert_eq!(movers.as_slice()[0].steps_left, COASTING_RANGE - 2);
        assert!(movers.push_block(&mut field, at(2, 0, 0), FaceDirection::PositiveY));
        assert_eq!(movers.len(), 1);
        assert_eq!(movers.as_slice()[0].steps_left, COASTING_RANGE);
        movers.advance(&mut field);
        assert_eq!(field.at(2, 1, 0), Some(ice(1)));
    }

    #[test]
    fn movers_act_in_the_order_they_started_each_seeing_what_the_earlier_ones_did() {
        // Two ice blocks side by side, pushed the same way in the same tick. Led by the front
        // one, both slide on together.
        let mut field = TestField::with(&[((0, 0, 0), ice(1)), ((1, 0, 0), ice(2))]);
        let mut movers = Movers::new();
        movers.push_block(&mut field, at(1, 0, 0), FaceDirection::PositiveX);
        movers.push_block(&mut field, at(0, 0, 0), FaceDirection::PositiveX);
        for _ in 0..3 {
            movers.advance(&mut field);
        }
        assert_eq!(field.at(4, 0, 0), Some(ice(2)));
        assert_eq!(field.at(3, 0, 0), Some(ice(1)));
        assert_eq!(movers.len(), 2);

        // Led by the back one, it meets the front one, which has not moved yet: it stops at once.
        let mut field = TestField::with(&[((0, 0, 0), ice(1)), ((1, 0, 0), ice(2))]);
        let mut movers = Movers::new();
        movers.push_block(&mut field, at(0, 0, 0), FaceDirection::PositiveX);
        movers.push_block(&mut field, at(1, 0, 0), FaceDirection::PositiveX);
        movers.advance(&mut field);
        assert_eq!(field.at(0, 0, 0), Some(ice(1)));
        assert_eq!(field.at(2, 0, 0), Some(ice(2)));
        assert_eq!(movers.len(), 1);
    }

    #[test]
    fn a_block_that_merges_into_one_in_motion_leaves_it_moving_with_the_new_number() {
        let mut field = TestField::with(&[((0, 0, 0), ice(1)), ((1, 0, 0), ice(1))]);
        let mut movers = Movers::new();
        movers.push_block(&mut field, at(0, 0, 0), FaceDirection::PositiveX);
        movers.push_block(&mut field, at(1, 0, 0), FaceDirection::PositiveY);
        let merges = movers.advance(&mut field);
        assert_eq!(merges, [ice(2)]);
        assert_eq!(field.blocks.len(), 1);
        assert_eq!(field.at(1, 1, 0), Some(ice(2)));
        assert_eq!(
            movers.as_slice(),
            [Mover {
                position: at(1, 1, 0),
                direction: FaceDirection::PositiveY,
                steps_left: COASTING_RANGE - 1,
            }]
        );
    }

    #[test]
    fn a_mover_whose_block_was_taken_away_is_forgotten() {
        let mut field = TestField::with(&[((0, 0, 0), ice(1))]);
        let mut movers = Movers::new();
        movers.push_block(&mut field, at(0, 0, 0), FaceDirection::PositiveX);
        field.place(at(0, 0, 0), None);
        assert!(movers.advance(&mut field).is_empty());
        assert!(movers.is_empty());
        assert!(field.blocks.is_empty());
    }

    #[test]
    fn saved_movers_with_no_steps_left_are_not_restored() {
        let mover = |steps_left| Mover {
            position: at(0, 0, 0),
            direction: FaceDirection::PositiveX,
            steps_left,
        };
        let movers = Movers::from_saved([mover(0), mover(5)]);
        assert_eq!(movers.as_slice(), [mover(5)]);
    }

    fn material_strategy() -> impl Strategy<Value = Material> {
        (0..Material::ALL.len()).prop_map(|index| Material::ALL[index])
    }

    proptest! {
        /// Whatever is pushed, in any arrangement: merging keeps the total of all the numbers,
        /// each merge removes exactly one block, every mover is a block, and all motion ends.
        #[test]
        fn pushes_conserve_value_and_always_come_to_rest(
            blocks in proptest::collection::vec(
                (0i64..5, 0i64..5, 0i64..5, material_strategy(), 1u8..=3), 0..60),
            pushes in proptest::collection::vec((0i64..5, 0i64..5, 0i64..5, 0u8..6), 0..10),
        ) {
            let mut field = TestField::default();
            for (x, y, z, material, exponent) in blocks {
                field.blocks.insert(at(x, y, z), NumberedBlock::new(material, exponent));
            }
            let (starting_value, starting_count) = (field.total_value(), field.blocks.len());
            let mut movers = Movers::new();
            for (x, y, z, direction) in pushes {
                let direction = FaceDirection::from_index(direction).unwrap();
                movers.push_block(&mut field, at(x, y, z), direction);
            }
            let mut merge_count = 0;
            for _ in 0..COASTING_RANGE {
                merge_count += movers.advance(&mut field).len();
                prop_assert_eq!(field.total_value(), starting_value);
                prop_assert_eq!(field.blocks.len(), starting_count - merge_count);
                let positions: HashSet<_> = movers.as_slice().iter().map(|mover| mover.position).collect();
                prop_assert_eq!(positions.len(), movers.len());
                for mover in movers.as_slice() {
                    prop_assert!(field.blocks.contains_key(&mover.position));
                    prop_assert!(mover.steps_left > 0);
                }
            }
            prop_assert!(movers.is_empty());
        }
    }
}

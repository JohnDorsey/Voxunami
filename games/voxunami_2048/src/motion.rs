//! How pushed blocks travel, meet and merge.
//!
//! A push makes a [`Mover`] of a block. Every tick each mover takes one step along its direction:
//!
//! - into an empty voxel: the block moves there;
//! - into a block that the pushing tool combines with (see [`Tool::combine`]): the moving block is
//!   gone and the struck one stays where it was with the number the tool made of the two, or both
//!   are gone, and the motion ends;
//! - into any other block, if the moving block pushes (stone): the block, and the row of blocks behind
//!   it, move one step ahead, provided the row is short enough ([`PUSH_LIMIT`]) and ends in empty
//!   space, and the moving block follows into the place made;
//! - into any other block, or into something that is not a numbered block at all (terrain this
//!   program cannot read, the edge of the world), or into a row that cannot move: the motion ends,
//!   or for a material that bounces, the block turns around.
//!
//! A mover has a limited number of steps (its material's range), which every tick uses up whether
//! the block moved or bounced, so every motion ends.
//!
//! Movers are entities, not cells of a lattice: they act one at a time, in the order they started
//! moving, each seeing what the ones before it did this tick. That order is part of the rules, since
//! it decides what happens when blocks meet in the same tick, and it is the same on every machine.
//! A block that is pushed along by a row moves once and does not become a mover, but if it already
//! was one it stays one, from its new place.
//!
//! A block moves within its level: a world has several, and blocks of different levels never meet.
//!
//! The rules see the world only through [`BlockField`], so they run the same on a server's world and
//! on the plain maps the tests use.

use voxel_space::{FaceDirection, LevelIndex, VoxelCoordinate};

use crate::material::PUSH_LIMIT;
use crate::numbered_block::NumberedBlock;
use crate::tool::{Combination, Tool};

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
    fn occupant(&mut self, level: LevelIndex, at: VoxelCoordinate) -> Occupant;

    /// Puts a block at a voxel, or empties it. Only called for voxels that were just seen to be
    /// empty or to hold a block.
    fn place(&mut self, level: LevelIndex, at: VoxelCoordinate, block: Option<NumberedBlock>);
}

/// A block that is moving. The block itself is in the world, at `position` in `level`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mover {
    pub level: LevelIndex,
    pub position: VoxelCoordinate,
    pub direction: FaceDirection,
    /// The tool that pushed the block, which decides what it does to the blocks it meets.
    pub tool: Tool,
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

    /// Pushes the block at `position` of `level` toward `direction` with `tool`, for the full range
    /// of its material. A block that is already moving is pushed anew (with the new direction and
    /// tool), keeping its place in the order. Returns whether there was a block to push.
    pub fn push_block(
        &mut self,
        field: &mut impl BlockField,
        level: LevelIndex,
        position: VoxelCoordinate,
        direction: FaceDirection,
        tool: Tool,
    ) -> bool {
        let Occupant::Block(block) = field.occupant(level, position) else {
            return false;
        };
        let pushed = Mover {
            level,
            position,
            direction,
            tool,
            steps_left: block.material.motion().range,
        };
        match self
            .movers
            .iter_mut()
            .find(|mover| mover.level == level && mover.position == position)
        {
            Some(moving) => *moving = pushed,
            None => self.movers.push(pushed),
        }
        true
    }

    /// Takes one step with every mover. Returns the blocks the merges of this tick made, in the
    /// order they happened (a merge that made both blocks vanish made none).
    pub fn advance(&mut self, field: &mut impl BlockField) -> Vec<NumberedBlock> {
        let mut merges = Vec::new();
        for index in 0..self.movers.len() {
            if !step(index, &mut self.movers, field, &mut merges) {
                // A mover with no steps left is finished; it is dropped once everyone has acted.
                self.movers[index].steps_left = 0;
            }
        }
        self.movers.retain(|mover| mover.steps_left > 0);
        merges
    }
}

/// The blocks in a row that a push would move, nearest first, and the empty voxel the row ends in;
/// `None` if the row is too long, ends at something that is not empty, or reaches the edge of the
/// world. `first` is the voxel just ahead of the pushing block, and holds a block.
fn row_to_push(
    field: &mut impl BlockField,
    level: LevelIndex,
    first: VoxelCoordinate,
    direction: FaceDirection,
) -> Option<(Vec<(VoxelCoordinate, NumberedBlock)>, VoxelCoordinate)> {
    let [dx, dy, dz] = direction.offset();
    let mut row = Vec::new();
    let mut cursor = first;
    loop {
        match field.occupant(level, cursor) {
            Occupant::Block(block) => {
                // The pushing block is one of the blocks that move.
                if row.len() + 1 >= PUSH_LIMIT as usize {
                    return None;
                }
                row.push((cursor, block));
                cursor = cursor.checked_offset(dx, dy, dz)?;
            }
            Occupant::Empty => return Some((row, cursor)),
            Occupant::Obstacle => return None,
        }
    }
}

/// One tick of the mover at `index`. Returns whether it is still moving afterwards.
fn step(
    index: usize,
    movers: &mut [Mover],
    field: &mut impl BlockField,
    merges: &mut Vec<NumberedBlock>,
) -> bool {
    let mover = movers[index];
    // A mover that finished earlier this tick, or whose block something other than this program
    // took away: nothing is moving any more.
    let Occupant::Block(block) = field.occupant(mover.level, mover.position) else {
        return false;
    };
    let Some(steps_left) = mover.steps_left.checked_sub(1) else {
        return false;
    };
    movers[index].steps_left = steps_left;
    let [dx, dy, dz] = mover.direction.offset();
    let ahead = mover
        .position
        .checked_offset(dx, dy, dz)
        .map(|target| (target, field.occupant(mover.level, target)));
    let mut can_advance = false;
    match ahead {
        Some((_, Occupant::Empty)) => can_advance = true,
        Some((target, Occupant::Block(struck))) => {
            match mover.tool.combine(block.number(), struck.number()) {
                Some(combination) => {
                    field.place(mover.level, mover.position, None);
                    match combination {
                        Combination::Becomes(number) => {
                            let made = NumberedBlock::new(struck.material, number);
                            field.place(mover.level, target, Some(made));
                            merges.push(made);
                        }
                        Combination::Vanish => {
                            field.place(mover.level, target, None);
                            // The struck block is gone, and if it was moving, so is its motion.
                            for other in movers.iter_mut() {
                                if other.level == mover.level && other.position == target {
                                    other.steps_left = 0;
                                }
                            }
                        }
                    }
                    return false;
                }
                None if block.material.motion().pushes => {
                    if let Some((row, end)) =
                        row_to_push(field, mover.level, target, mover.direction)
                    {
                        // Each block of the row moves into the place of the next, the last one
                        // into the empty voxel at the end; the first one's place is the pushing
                        // block's next.
                        let mut into = end;
                        for &(from, pushed) in row.iter().rev() {
                            field.place(mover.level, into, Some(pushed));
                            for other in movers.iter_mut() {
                                if other.level == mover.level && other.position == from {
                                    other.position = into;
                                }
                            }
                            into = from;
                        }
                        can_advance = true;
                    }
                }
                None => {}
            }
        }
        _ => {}
    }
    if can_advance {
        let target = mover
            .position
            .checked_offset(dx, dy, dz)
            .expect("the voxel ahead was just seen");
        field.place(mover.level, mover.position, None);
        field.place(mover.level, target, Some(block));
        movers[index].position = target;
    } else if block.material.motion().bounces {
        movers[index].direction = mover.direction.opposite();
    } else {
        return false;
    }
    steps_left > 0
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use proptest::prelude::*;
    use voxel_space::VOXEL_COORDINATE_MAXIMUM;

    use super::*;
    use crate::material::{COASTING_RANGE, Material, PUSH_LIMIT};

    #[derive(Default)]
    struct TestField {
        blocks: HashMap<(LevelIndex, VoxelCoordinate), NumberedBlock>,
        obstacles: HashSet<(LevelIndex, VoxelCoordinate)>,
    }

    impl BlockField for TestField {
        fn occupant(&mut self, level: LevelIndex, at: VoxelCoordinate) -> Occupant {
            let at = (level, at);
            if self.obstacles.contains(&at) {
                Occupant::Obstacle
            } else {
                self.blocks
                    .get(&at)
                    .map_or(Occupant::Empty, |block| Occupant::Block(*block))
            }
        }

        fn place(&mut self, level: LevelIndex, at: VoxelCoordinate, block: Option<NumberedBlock>) {
            let at = (level, at);
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
                field
                    .blocks
                    .insert((LevelIndex::MAIN, VoxelCoordinate::new(x, y, z)), block);
            }
            field
        }

        fn at(&self, x: i64, y: i64, z: i64) -> Option<NumberedBlock> {
            self.blocks
                .get(&(LevelIndex::MAIN, VoxelCoordinate::new(x, y, z)))
                .copied()
        }

        fn total_value(&self) -> u64 {
            self.blocks
                .values()
                .map(|block| u64::from(block.number()))
                .sum()
        }
    }

    fn at(x: i64, y: i64, z: i64) -> VoxelCoordinate {
        VoxelCoordinate::new(x, y, z)
    }

    /// The blocks of these tests are named by the exponent of the power of two they show.
    fn stone(exponent: u8) -> NumberedBlock {
        NumberedBlock::new(Material::Stone, 1 << exponent)
    }

    fn ice(exponent: u8) -> NumberedBlock {
        NumberedBlock::new(Material::Ice, 1 << exponent)
    }

    fn spring_ice(exponent: u8) -> NumberedBlock {
        NumberedBlock::new(Material::SpringIce, 1 << exponent)
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
        assert!(movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveZ,
            Tool::Double
        ));
        assert_eq!(movers.len(), 1);
        let (ticks, merges) = run_to_rest(&mut movers, &mut field);
        assert_eq!((ticks, merges.len()), (1, 0));
        assert_eq!(field.at(0, 0, 0), None);
        assert_eq!(field.at(0, 0, 1), Some(stone(3)));
        assert_eq!(field.blocks.len(), 1);
    }

    #[test]
    fn stone_pushes_the_row_in_front_of_it_one_step_and_the_pushed_blocks_do_not_coast() {
        // A stone block, a stone block, ice, stone: the whole row moves one step into the empty
        // voxel at its end. The ice in it is pushed along but does not start to coast.
        let mut field = TestField::with(&[
            ((0, 0, 0), stone(1)),
            ((1, 0, 0), stone(2)),
            ((2, 0, 0), ice(3)),
            ((3, 0, 0), stone(4)),
        ]);
        let mut movers = Movers::new();
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
        let (ticks, merges) = run_to_rest(&mut movers, &mut field);
        assert_eq!((ticks, merges.len()), (1, 0));
        assert_eq!(field.at(0, 0, 0), None);
        assert_eq!(field.at(1, 0, 0), Some(stone(1)));
        assert_eq!(field.at(2, 0, 0), Some(stone(2)));
        assert_eq!(field.at(3, 0, 0), Some(ice(3)));
        assert_eq!(field.at(4, 0, 0), Some(stone(4)));
        assert_eq!(field.blocks.len(), 4);
    }

    #[test]
    fn a_row_that_cannot_move_stays_where_it_is() {
        // The row ends at an obstacle.
        let mut field = TestField::with(&[((0, 0, 0), stone(1)), ((1, 0, 0), stone(2))]);
        field.obstacles.insert((LevelIndex::MAIN, at(2, 0, 0)));
        assert_eq!(push_origin_block(&mut field, Tool::Double), []);
        assert_eq!(field.at(0, 0, 0), Some(stone(1)));
        assert_eq!(field.at(1, 0, 0), Some(stone(2)));

        // The row ends at the edge of the world.
        let edge = VOXEL_COORDINATE_MAXIMUM;
        let mut field = TestField::with(&[((edge - 1, 0, 0), stone(1)), ((edge, 0, 0), stone(2))]);
        let mut movers = Movers::new();
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(edge - 1, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
        run_to_rest(&mut movers, &mut field);
        assert_eq!(field.at(edge - 1, 0, 0), Some(stone(1)));
        assert_eq!(field.at(edge, 0, 0), Some(stone(2)));
    }

    #[test]
    fn a_push_moves_at_most_the_push_limit_of_blocks_the_pushing_one_included() {
        let row = |length: u32| {
            // Distinct numbers, so that nothing merges.
            let blocks: Vec<_> = (0..length)
                .map(|position| {
                    (
                        (i64::from(position), 0, 0),
                        block(Material::Stone, position + 1),
                    )
                })
                .collect();
            TestField::with(&blocks)
        };
        let mut field = row(PUSH_LIMIT);
        push_origin_block(&mut field, Tool::Double);
        assert_eq!(field.at(0, 0, 0), None);
        for position in 0..PUSH_LIMIT {
            assert_eq!(
                field.at(i64::from(position) + 1, 0, 0),
                Some(block(Material::Stone, position + 1)),
                "block {position}"
            );
        }

        let mut field = row(PUSH_LIMIT + 1);
        push_origin_block(&mut field, Tool::Double);
        for position in 0..=PUSH_LIMIT {
            assert_eq!(
                field.at(i64::from(position), 0, 0),
                Some(block(Material::Stone, position + 1)),
                "block {position} stayed"
            );
        }
        assert_eq!(field.blocks.len(), PUSH_LIMIT as usize + 1);
    }

    #[test]
    fn a_stone_block_merges_with_what_its_tool_combines_it_with_instead_of_pushing_it() {
        // Equal numbers, with the double tool.
        let mut field = TestField::with(&[
            ((0, 0, 0), stone(2)),
            ((1, 0, 0), stone(2)),
            ((2, 0, 0), stone(5)),
        ]);
        assert_eq!(push_origin_block(&mut field, Tool::Double), [stone(3)]);
        assert_eq!(field.at(1, 0, 0), Some(stone(3)));
        assert_eq!(
            field.at(2, 0, 0),
            Some(stone(5)),
            "the row behind did not move"
        );
        assert_eq!(field.blocks.len(), 2);

        // Any numbers, with the add tool.
        let mut field = TestField::with(&[
            ((0, 0, 0), block(Material::Stone, 3)),
            ((1, 0, 0), block(Material::Stone, 4)),
        ]);
        assert_eq!(
            push_origin_block(&mut field, Tool::Add),
            [block(Material::Stone, 7)]
        );
        assert_eq!(field.blocks.len(), 1);
    }

    #[test]
    fn ice_and_spring_ice_do_not_push_a_row_they_stop_at_it_or_bounce_off_it() {
        let mut field = TestField::with(&[((0, 0, 0), ice(1)), ((1, 0, 0), stone(2))]);
        assert_eq!(push_origin_block(&mut field, Tool::Double), []);
        assert_eq!(field.at(0, 0, 0), Some(ice(1)));
        assert_eq!(field.at(1, 0, 0), Some(stone(2)));

        let mut field = TestField::with(&[((0, 0, 0), spring_ice(1)), ((1, 0, 0), stone(2))]);
        let mut movers = Movers::new();
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
        movers.advance(&mut field);
        assert_eq!(field.at(1, 0, 0), Some(stone(2)));
        assert_eq!(movers.as_slice()[0].direction, FaceDirection::NegativeX);
    }

    #[test]
    fn a_pushed_block_that_was_moving_goes_on_moving_from_the_place_it_was_pushed_to() {
        // The stone block pushes the ice block sideways while the ice block coasts upward.
        let mut field = TestField::with(&[((0, 0, 0), stone(1)), ((1, 0, 0), ice(2))]);
        let mut movers = Movers::new();
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(1, 0, 0),
            FaceDirection::PositiveY,
            Tool::Double,
        );
        movers.advance(&mut field);
        assert_eq!(field.at(1, 0, 0), Some(stone(1)));
        assert_eq!(field.at(2, 1, 0), Some(ice(2)));
        assert_eq!(field.blocks.len(), 2);
        assert_eq!(
            movers.as_slice(),
            [Mover {
                level: LevelIndex::MAIN,
                position: at(2, 1, 0),
                direction: FaceDirection::PositiveY,
                tool: Tool::Double,
                steps_left: COASTING_RANGE - 1,
            }]
        );
    }

    #[test]
    fn rows_are_pushed_within_their_level() {
        let second = LevelIndex::new(1);
        let mut field = TestField::with(&[((0, 0, 0), stone(1)), ((1, 0, 0), stone(2))]);
        field.blocks.insert((second, at(0, 0, 0)), stone(3));
        field.blocks.insert((second, at(1, 0, 0)), stone(4));
        field.blocks.insert((second, at(2, 0, 0)), stone(5));
        let mut movers = Movers::new();
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
        run_to_rest(&mut movers, &mut field);
        // The main level's row moved; the second level's, which has a block at the end of the
        // main level's row, was not in its way and did not move.
        assert_eq!(field.at(2, 0, 0), Some(stone(2)));
        assert_eq!(field.blocks.get(&(second, at(0, 0, 0))), Some(&stone(3)));
        assert_eq!(field.blocks.get(&(second, at(2, 0, 0))), Some(&stone(5)));
    }

    #[test]
    fn stone_merges_with_an_equal_block_whatever_it_is_made_of() {
        let mut field = TestField::with(&[((0, 0, 0), stone(2)), ((1, 0, 0), ice(2))]);
        let mut movers = Movers::new();
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
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
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
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
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
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
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::NegativeY,
            Tool::Double,
        );
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
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
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
        field.obstacles.insert((LevelIndex::MAIN, at(1, 0, 0)));
        let mut movers = Movers::new();
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
        movers.advance(&mut field);
        assert_eq!(movers.as_slice()[0].direction, FaceDirection::NegativeX);
        assert_eq!(field.at(0, 0, 0), Some(spring_ice(2)));

        let mut field = TestField::with(&[((0, 0, 0), spring_ice(2)), ((3, 0, 0), ice(2))]);
        let mut movers = Movers::new();
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
        let (_, merges) = run_to_rest(&mut movers, &mut field);
        assert_eq!(merges, [ice(3)]);
        assert_eq!(field.blocks.len(), 1);
        assert_eq!(field.at(3, 0, 0), Some(ice(3)));
    }

    #[test]
    fn blocks_stop_at_obstacles_and_at_the_edge_of_the_world() {
        let mut field = TestField::with(&[((0, 0, 0), ice(1))]);
        field.obstacles.insert((LevelIndex::MAIN, at(3, 0, 0)));
        let mut movers = Movers::new();
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
        run_to_rest(&mut movers, &mut field);
        assert_eq!(field.at(2, 0, 0), Some(ice(1)));

        let edge = VOXEL_COORDINATE_MAXIMUM;
        let mut field = TestField::with(&[((edge, 0, 0), ice(1))]);
        let mut movers = Movers::new();
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(edge, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
        let (ticks, _) = run_to_rest(&mut movers, &mut field);
        assert_eq!(ticks, 1);
        assert_eq!(field.at(edge, 0, 0), Some(ice(1)));
    }

    #[test]
    fn only_a_block_can_be_pushed() {
        let mut field = TestField::default();
        field.obstacles.insert((LevelIndex::MAIN, at(1, 1, 1)));
        let mut movers = Movers::new();
        assert!(!movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double
        ));
        assert!(!movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(1, 1, 1),
            FaceDirection::PositiveX,
            Tool::Double
        ));
        assert!(movers.is_empty());
    }

    #[test]
    fn pushing_a_moving_block_again_gives_it_a_new_direction_and_a_new_range() {
        let mut field = TestField::with(&[((0, 0, 0), ice(1))]);
        let mut movers = Movers::new();
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
        movers.advance(&mut field);
        movers.advance(&mut field);
        assert_eq!(movers.as_slice()[0].steps_left, COASTING_RANGE - 2);
        assert!(movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(2, 0, 0),
            FaceDirection::PositiveY,
            Tool::Double
        ));
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
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(1, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
        for _ in 0..3 {
            movers.advance(&mut field);
        }
        assert_eq!(field.at(4, 0, 0), Some(ice(2)));
        assert_eq!(field.at(3, 0, 0), Some(ice(1)));
        assert_eq!(movers.len(), 2);

        // Led by the back one, it meets the front one, which has not moved yet: it stops at once.
        let mut field = TestField::with(&[((0, 0, 0), ice(1)), ((1, 0, 0), ice(2))]);
        let mut movers = Movers::new();
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(1, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
        movers.advance(&mut field);
        assert_eq!(field.at(0, 0, 0), Some(ice(1)));
        assert_eq!(field.at(2, 0, 0), Some(ice(2)));
        assert_eq!(movers.len(), 1);
    }

    #[test]
    fn a_block_that_merges_into_one_in_motion_leaves_it_moving_with_the_new_number() {
        let mut field = TestField::with(&[((0, 0, 0), ice(1)), ((1, 0, 0), ice(1))]);
        let mut movers = Movers::new();
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(1, 0, 0),
            FaceDirection::PositiveY,
            Tool::Double,
        );
        let merges = movers.advance(&mut field);
        assert_eq!(merges, [ice(2)]);
        assert_eq!(field.blocks.len(), 1);
        assert_eq!(field.at(1, 1, 0), Some(ice(2)));
        assert_eq!(
            movers.as_slice(),
            [Mover {
                level: LevelIndex::MAIN,
                position: at(1, 1, 0),
                direction: FaceDirection::PositiveY,
                steps_left: COASTING_RANGE - 1,
                tool: Tool::Double,
            }]
        );
    }

    #[test]
    fn a_mover_whose_block_was_taken_away_is_forgotten() {
        let mut field = TestField::with(&[((0, 0, 0), ice(1))]);
        let mut movers = Movers::new();
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
        field.place(LevelIndex::MAIN, at(0, 0, 0), None);
        assert!(movers.advance(&mut field).is_empty());
        assert!(movers.is_empty());
        assert!(field.blocks.is_empty());
    }

    #[test]
    fn saved_movers_with_no_steps_left_are_not_restored() {
        let mover = |steps_left| Mover {
            level: LevelIndex::MAIN,
            position: at(0, 0, 0),
            direction: FaceDirection::PositiveX,
            tool: Tool::Double,
            steps_left,
        };
        let movers = Movers::from_saved([mover(0), mover(5)]);
        assert_eq!(movers.as_slice(), [mover(5)]);
    }

    #[test]
    fn blocks_of_different_levels_never_meet_even_at_the_same_place() {
        let second = LevelIndex::new(1);
        let mut field = TestField::with(&[((0, 0, 0), ice(1)), ((2, 0, 0), stone(2))]);
        // The same voxel positions in the second level hold other blocks, and a wall in the way
        // of the main level's block is not a wall for the second level's.
        field.blocks.insert((second, at(0, 0, 0)), ice(1));
        field.blocks.insert((second, at(3, 0, 0)), stone(1));
        let mut movers = Movers::new();
        assert!(movers.push_block(
            &mut field,
            second,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double
        ));
        let (ticks, merges) = run_to_rest(&mut movers, &mut field);
        assert_eq!(
            (ticks, merges.len()),
            (3, 1),
            "the second level's blocks merged"
        );
        assert_eq!(field.blocks.get(&(second, at(3, 0, 0))), Some(&stone(2)));
        // The main level is as it was: the stone at x = 2 was not in the second level's way.
        assert_eq!(field.at(0, 0, 0), Some(ice(1)));
        assert_eq!(field.at(2, 0, 0), Some(stone(2)));
        // A block pushed in one level is not moving in another, even at the same position.
        field.blocks.insert((second, at(0, 0, 0)), stone(3));
        let mut movers = Movers::new();
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::NegativeY,
            Tool::Double,
        );
        assert!(movers.push_block(
            &mut field,
            second,
            at(0, 0, 0),
            FaceDirection::NegativeY,
            Tool::Double
        ));
        assert_eq!(
            movers.len(),
            2,
            "same position, different levels, two movers"
        );
        movers.advance(&mut field);
        assert_eq!(field.at(0, -1, 0), Some(ice(1)));
        assert_eq!(field.blocks.get(&(second, at(0, -1, 0))), Some(&stone(3)));
        assert_eq!(field.blocks.get(&(second, at(0, 0, 0))), None);
    }

    fn block(material: Material, number: u32) -> NumberedBlock {
        NumberedBlock::new(material, number)
    }

    /// Pushes the block at the origin toward +x with `tool` and runs until everything rests.
    fn push_origin_block(field: &mut TestField, tool: Tool) -> Vec<NumberedBlock> {
        let mut movers = Movers::new();
        assert!(movers.push_block(
            field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            tool
        ));
        run_to_rest(&mut movers, field).1
    }

    #[test]
    fn the_add_tool_merges_blocks_of_any_numbers_into_one_in_the_material_of_the_struck_block() {
        let mut field = TestField::with(&[
            ((0, 0, 0), block(Material::Stone, 3)),
            ((1, 0, 0), block(Material::Ice, 4)),
        ]);
        let merges = push_origin_block(&mut field, Tool::Add);
        assert_eq!(merges, [block(Material::Ice, 7)]);
        assert_eq!(field.blocks.len(), 1);
        assert_eq!(field.at(1, 0, 0), Some(block(Material::Ice, 7)));

        // A coasting block adds itself to the first block it meets.
        let mut field = TestField::with(&[
            ((0, 0, 0), block(Material::Ice, 5)),
            ((4, 0, 0), block(Material::Stone, 64)),
        ]);
        let merges = push_origin_block(&mut field, Tool::Add);
        assert_eq!(merges, [block(Material::Stone, 69)]);
        assert_eq!(field.blocks.len(), 1);
        assert_eq!(field.at(4, 0, 0), Some(block(Material::Stone, 69)));
    }

    #[test]
    fn the_subtract_tool_leaves_the_difference_in_the_struck_block_or_nothing_at_all() {
        let mut field = TestField::with(&[
            ((0, 0, 0), block(Material::Stone, 3)),
            ((1, 0, 0), block(Material::Ice, 10)),
        ]);
        assert_eq!(
            push_origin_block(&mut field, Tool::Subtract),
            [block(Material::Ice, 7)]
        );
        assert_eq!(field.blocks.len(), 1);

        // The other way round the difference is the same.
        let mut field = TestField::with(&[
            ((0, 0, 0), block(Material::Stone, 10)),
            ((1, 0, 0), block(Material::Ice, 3)),
        ]);
        assert_eq!(
            push_origin_block(&mut field, Tool::Subtract),
            [block(Material::Ice, 7)]
        );

        // Equal numbers cancel: both blocks are gone and no block was made.
        let mut field = TestField::with(&[
            ((0, 0, 0), block(Material::Stone, 6)),
            ((1, 0, 0), block(Material::SpringIce, 6)),
            ((9, 9, 9), block(Material::Stone, 1)),
        ]);
        assert_eq!(push_origin_block(&mut field, Tool::Subtract), []);
        assert_eq!(field.blocks.len(), 1);
        assert_eq!(field.at(9, 9, 9), Some(block(Material::Stone, 1)));
    }

    #[test]
    fn a_block_that_cancels_with_one_in_motion_takes_it_out_of_the_motion_too() {
        let mut field = TestField::with(&[
            ((0, 0, 0), block(Material::Ice, 4)),
            ((1, 0, 0), block(Material::Ice, 4)),
        ]);
        let mut movers = Movers::new();
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Subtract,
        );
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(1, 0, 0),
            FaceDirection::PositiveY,
            Tool::Double,
        );
        assert_eq!(movers.advance(&mut field), []);
        assert!(field.blocks.is_empty());
        assert!(
            movers.is_empty(),
            "the second mover's block is gone with it"
        );
    }

    #[test]
    fn a_block_cancelled_by_one_that_already_acted_this_tick_leaves_no_motion_behind() {
        // The ice block acts first and slides next to the stone block, which then slides into it and
        // cancels with it: the ice block's motion must not outlive it.
        let mut field = TestField::with(&[
            ((0, 0, 0), block(Material::Ice, 1)),
            ((2, 0, 0), block(Material::Stone, 1)),
        ]);
        let mut movers = Movers::new();
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Subtract,
        );
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(2, 0, 0),
            FaceDirection::NegativeX,
            Tool::Subtract,
        );
        assert_eq!(movers.advance(&mut field), []);
        assert!(field.blocks.is_empty());
        assert!(movers.is_empty(), "{:?}", movers.as_slice());
    }

    #[test]
    fn what_the_tool_does_not_combine_is_met_like_any_other_block_and_the_tool_stays_with_the_block()
     {
        // The same scene, a 2 coasting into a 3: only the add and subtract tools combine them.
        let scene = [
            ((0, 0, 0), block(Material::Ice, 2)),
            ((3, 0, 0), block(Material::Stone, 3)),
        ];
        let mut field = TestField::with(&scene);
        assert_eq!(push_origin_block(&mut field, Tool::Double), []);
        assert_eq!(field.at(2, 0, 0), Some(block(Material::Ice, 2)));
        assert_eq!(field.at(3, 0, 0), Some(block(Material::Stone, 3)));
        let mut field = TestField::with(&scene);
        assert_eq!(
            push_origin_block(&mut field, Tool::Add),
            [block(Material::Stone, 5)]
        );
        let mut field = TestField::with(&scene);
        assert_eq!(
            push_origin_block(&mut field, Tool::Subtract),
            [block(Material::Stone, 1)]
        );

        // Spring ice that a tool cannot combine bounces, as before.
        let mut field = TestField::with(&[
            ((0, 0, 0), block(Material::SpringIce, 2)),
            ((1, 0, 0), block(Material::Stone, 3)),
        ]);
        let mut movers = Movers::new();
        movers.push_block(
            &mut field,
            LevelIndex::MAIN,
            at(0, 0, 0),
            FaceDirection::PositiveX,
            Tool::Double,
        );
        movers.advance(&mut field);
        assert_eq!(movers.as_slice()[0].direction, FaceDirection::NegativeX);
        assert_eq!(movers.as_slice()[0].tool, Tool::Double);
    }

    #[test]
    fn a_result_that_does_not_fit_is_not_a_merge() {
        // (Ice, which stops at a block it does not combine with; stone would push it.)
        let mut field = TestField::with(&[
            ((0, 0, 0), block(Material::Ice, u32::MAX)),
            ((1, 0, 0), block(Material::Stone, 1)),
        ]);
        assert_eq!(push_origin_block(&mut field, Tool::Add), []);
        assert_eq!(field.at(0, 0, 0), Some(block(Material::Ice, u32::MAX)));
        assert_eq!(field.at(1, 0, 0), Some(block(Material::Stone, 1)));
    }

    #[test]
    fn pushing_a_moving_block_again_gives_it_the_new_tool() {
        let mut field = TestField::with(&[((0, 0, 0), ice(1))]);
        let mut movers = Movers::new();
        for tool in [Tool::Add, Tool::Subtract] {
            movers.push_block(
                &mut field,
                LevelIndex::MAIN,
                at(0, 0, 0),
                FaceDirection::PositiveX,
                tool,
            );
            assert_eq!(movers.len(), 1);
            assert_eq!(movers.as_slice()[0].tool, tool);
        }
    }

    fn tool_strategy() -> impl Strategy<Value = Tool> {
        (0..Tool::ALL.len()).prop_map(|index| Tool::ALL[index])
    }

    fn material_strategy() -> impl Strategy<Value = Material> {
        (0..Material::ALL.len()).prop_map(|index| Material::ALL[index])
    }

    proptest! {
        /// With the subtract tool, merging takes numbers away and removes blocks: the total never
        /// grows, no block is ever numbered zero, every mover is a block, and all motion ends.
        #[test]
        fn subtracting_never_grows_the_total_and_always_comes_to_rest(
            blocks in proptest::collection::vec(
                (0i64..5, 0i64..5, 0i64..5, material_strategy(), 1u32..=9), 0..60),
            pushes in proptest::collection::vec((0i64..5, 0i64..5, 0i64..5, 0u8..6), 0..10),
        ) {
            let mut field = TestField::default();
            for (x, y, z, material, number) in blocks {
                field.blocks.insert((LevelIndex::MAIN, at(x, y, z)), NumberedBlock::new(material, number));
            }
            let mut movers = Movers::new();
            for (x, y, z, direction) in pushes {
                let direction = FaceDirection::from_index(direction).unwrap();
                movers.push_block(&mut field, LevelIndex::MAIN, at(x, y, z), direction, Tool::Subtract);
            }
            let (mut total, mut count) = (field.total_value(), field.blocks.len());
            for _ in 0..COASTING_RANGE {
                movers.advance(&mut field);
                prop_assert!(field.total_value() <= total);
                prop_assert!(field.blocks.len() <= count);
                (total, count) = (field.total_value(), field.blocks.len());
                for mover in movers.as_slice() {
                    prop_assert!(field.blocks.contains_key(&(mover.level, mover.position)));
                    prop_assert_eq!(mover.tool, Tool::Subtract);
                }
            }
            prop_assert!(movers.is_empty());
        }

        /// Whatever is pushed, in any arrangement: merging with the double or add tool keeps the
        /// total of all the numbers, each merge removes exactly one block, every mover is a block,
        /// and all motion ends.
        #[test]
        fn pushes_conserve_value_and_always_come_to_rest(
            blocks in proptest::collection::vec(
                (0i64..5, 0i64..5, 0i64..5, material_strategy(), 1u8..=3), 0..60),
            pushes in proptest::collection::vec((0i64..5, 0i64..5, 0i64..5, 0u8..6), 0..10),
            tool in tool_strategy().prop_filter("the total is kept", |tool| *tool != Tool::Subtract),
        ) {
            let mut field = TestField::default();
            for (x, y, z, material, exponent) in blocks {
                field.blocks.insert((LevelIndex::MAIN, at(x, y, z)), NumberedBlock::new(material, 1 << exponent));
            }
            let (starting_value, starting_count) = (field.total_value(), field.blocks.len());
            let mut movers = Movers::new();
            for (x, y, z, direction) in pushes {
                let direction = FaceDirection::from_index(direction).unwrap();
                movers.push_block(&mut field, LevelIndex::MAIN, at(x, y, z), direction, tool);
            }
            let mut merge_count = 0;
            for _ in 0..COASTING_RANGE {
                merge_count += movers.advance(&mut field).len();
                prop_assert_eq!(field.total_value(), starting_value);
                prop_assert_eq!(field.blocks.len(), starting_count - merge_count);
                let positions: HashSet<_> = movers.as_slice().iter().map(|mover| (mover.level, mover.position)).collect();
                prop_assert_eq!(positions.len(), movers.len());
                for mover in movers.as_slice() {
                    prop_assert!(field.blocks.contains_key(&(mover.level, mover.position)));
                    prop_assert!(mover.steps_left > 0);
                }
            }
            prop_assert!(movers.is_empty());
        }
    }
}

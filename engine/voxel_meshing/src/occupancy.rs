//! What of a brick blocks the sun, one bit for each voxel, for the walk of the sun's ray through the voxels (`docs/specs/contact-trace-spec.md`).
//!
//! It is made from the same channels as the brick's mesh, but it is not a by-product of the mesh: a brick that is all air and one that is solid and buried both have an
//! empty mesh, and the walk must tell them apart (the ray goes through the one and stops in the other).

use block_models::BlockTable;
use voxel_space::{BRICK_VOXEL_COUNT, LocalVoxelIndex};

use crate::mesher::{BrickChannels, uniform_model};

/// The words of a brick's bits: 32 voxels each.
pub const OCCUPANCY_WORDS: usize = BRICK_VOXEL_COUNT / 32;

/// Which voxels of a brick block the sun whole: those whose model is a cube that hides what is behind it ([`BlockTable::is_opaque_cube`]). Every other voxel counts as clear
/// for now: a slab, a stair, leaves and glass are not traced, and the shadow map alone speaks for them.
///
/// A brick is `Empty` or `Full` whenever no voxel or every voxel does, so two bricks that block alike are equal; `Mixed` never holds all zeros or all ones.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BrickOccupancy {
    /// No voxel blocks the sun.
    Empty,
    /// Every voxel does.
    Full,
    /// Bit `i % 32` of word `i / 32` is whether the voxel of [`LocalVoxelIndex::index`] `i` does (so x runs along the bits, then y, then z along the words).
    Mixed(Box<[u32; OCCUPANCY_WORDS]>),
}

impl BrickOccupancy {
    /// What the brick whose channels these are blocks, by the models `table` gives its blocks, states and orientations.
    pub fn of(channels: &BrickChannels<'_>, table: &BlockTable) -> Self {
        if let Some(model) = uniform_model(channels, table) {
            return if table.is_opaque_cube(model) {
                Self::Full
            } else {
                Self::Empty
            };
        }
        let blocks: Vec<u32> = channels.blocks.iter().collect();
        let states: Option<Vec<u32>> = channels.states.map(|states| states.iter().collect());
        let orientations: Option<Vec<u32>> = channels
            .orientations
            .map(|orientations| orientations.iter().collect());
        let mut words = Box::new([0u32; OCCUPANCY_WORDS]);
        for (index, &block) in blocks.iter().enumerate() {
            let state = states.as_ref().map_or(0, |states| states[index]);
            let orientation = orientations
                .as_ref()
                .map_or(0, |orientations| orientations[index]);
            if table.is_opaque_cube(table.model_of(block, state, orientation)) {
                words[index / 32] |= 1 << (index % 32);
            }
        }
        Self::from_words(words)
    }

    /// The occupancy with these bits, `Empty` or `Full` if they are all alike.
    pub fn from_words(words: Box<[u32; OCCUPANCY_WORDS]>) -> Self {
        if words.iter().all(|&word| word == 0) {
            Self::Empty
        } else if words.iter().all(|&word| word == u32::MAX) {
            Self::Full
        } else {
            Self::Mixed(words)
        }
    }

    /// Whether the voxel blocks the sun.
    pub fn blocks(&self, voxel: LocalVoxelIndex) -> bool {
        match self {
            Self::Empty => false,
            Self::Full => true,
            Self::Mixed(words) => {
                let index = voxel.index();
                words[index / 32] >> (index % 32) & 1 == 1
            }
        }
    }

    /// The bits of a `Mixed` brick.
    pub fn words(&self) -> Option<&[u32; OCCUPANCY_WORDS]> {
        match self {
            Self::Mixed(words) => Some(words),
            Self::Empty | Self::Full => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use block_models::{BlockDefinition, Shape, Transparency};
    use proptest::prelude::*;
    use voxel_grid::ChannelStorage;
    use voxel_space::FaceDirection;

    use super::*;
    use crate::{BrickNeighbourhood, mesh_brick};

    const AIR: u32 = 0;
    const STONE: u32 = 1;
    /// A block with states: 0 whole, 1 bottom slab, 2 top slab.
    const ROCK: u32 = 2;
    /// A sight-through whole cube.
    const GLASS: u32 = 3;
    /// A sight-through whole cube in the `leaves` mode.
    const LEAVES: u32 = 4;

    fn table() -> BlockTable {
        let air = BlockDefinition::air();
        let stone = BlockDefinition::cube([0.5; 3]);
        let rock = BlockDefinition::cube([0.4; 3]).with_shapes(vec![
            Shape::full(),
            Shape::half(FaceDirection::NegativeY),
            Shape::half(FaceDirection::PositiveY),
        ]);
        let glass = BlockDefinition::cube([0.7; 3]).with_sight_through(true);
        let leaves = BlockDefinition::cube([0.1, 0.5, 0.2])
            .with_sight_through(true)
            .with_transparency(Transparency::Leaves);
        BlockTable::from_definitions(&[
            Some(&air),
            Some(&stone),
            Some(&rock),
            Some(&glass),
            Some(&leaves),
        ])
    }

    /// What the table says of one voxel, asked for one at a time: the oracle.
    fn blocks_by_the_table(table: &BlockTable, block: u32, state: u32) -> bool {
        table.is_opaque_cube(table.model_of(block, state, 0))
    }

    fn occupancy_of_values(blocks: &[u32], states: Option<&[u32]>) -> BrickOccupancy {
        let blocks = ChannelStorage::from_values(blocks);
        let states = states.map(ChannelStorage::from_values);
        BrickOccupancy::of(
            &BrickChannels {
                blocks: &blocks,
                states: states.as_ref(),
                orientations: None,
                light: None,
            },
            &table(),
        )
    }

    #[test]
    fn a_brick_all_of_one_block_is_empty_if_that_block_does_not_block_the_sun_whole_and_full_if_it_does()
     {
        for (block, state, expected) in [
            (AIR, 0, BrickOccupancy::Empty),
            (STONE, 0, BrickOccupancy::Full),
            (ROCK, 0, BrickOccupancy::Full),
            // A bottom slab, a top slab, glass and leaves are not whole opaque cubes: the shadow map speaks for them.
            (ROCK, 1, BrickOccupancy::Empty),
            (ROCK, 2, BrickOccupancy::Empty),
            (GLASS, 0, BrickOccupancy::Empty),
            (LEAVES, 0, BrickOccupancy::Empty),
        ] {
            let found = occupancy_of_values(
                &vec![block; BRICK_VOXEL_COUNT],
                Some(&vec![state; BRICK_VOXEL_COUNT]),
            );
            assert_eq!(found, expected, "block {block} state {state}");
        }
    }

    #[test]
    fn one_stone_voxel_in_air_sets_exactly_its_bit_in_the_word_for_its_row() {
        // x fastest, then y, then z: the word is `z * 32 + y` and the bit is `x`.
        for (x, y, z) in [
            (0u8, 0u8, 0u8),
            (31, 0, 0),
            (0, 31, 0),
            (0, 0, 31),
            (5, 17, 29),
            (31, 31, 31),
        ] {
            let mut blocks = vec![AIR; BRICK_VOXEL_COUNT];
            blocks[LocalVoxelIndex::new(x, y, z).index()] = STONE;
            let found = occupancy_of_values(&blocks, None);
            let words = found.words().expect("one voxel is a mixed brick");
            for (index, &word) in words.iter().enumerate() {
                let expected = if index == usize::from(z) * 32 + usize::from(y) {
                    1u32 << x
                } else {
                    0
                };
                assert_eq!(word, expected, "({x}, {y}, {z}) word {index}");
            }
            assert!(found.blocks(LocalVoxelIndex::new(x, y, z)));
            assert!(!found.blocks(LocalVoxelIndex::new((x + 1) % 32, y, z)));
        }
    }

    #[test]
    fn a_brick_with_one_voxel_missing_from_a_wall_of_stone_is_mixed_and_never_full() {
        let mut blocks = vec![STONE; BRICK_VOXEL_COUNT];
        blocks[12_345] = AIR;
        let found = occupancy_of_values(&blocks, None);
        assert!(matches!(found, BrickOccupancy::Mixed(_)));
        assert!(!found.blocks(LocalVoxelIndex::from_index(12_345).unwrap()));
        assert!(found.blocks(LocalVoxelIndex::from_index(12_344).unwrap()));
    }

    #[test]
    fn bits_that_are_all_alike_are_empty_or_full_so_that_equal_bricks_are_equal() {
        assert_eq!(
            BrickOccupancy::from_words(Box::new([0; OCCUPANCY_WORDS])),
            BrickOccupancy::Empty
        );
        assert_eq!(
            BrickOccupancy::from_words(Box::new([u32::MAX; OCCUPANCY_WORDS])),
            BrickOccupancy::Full
        );
        let mut almost = Box::new([u32::MAX; OCCUPANCY_WORDS]);
        almost[OCCUPANCY_WORDS - 1] ^= 1 << 31;
        assert!(matches!(
            BrickOccupancy::from_words(almost),
            BrickOccupancy::Mixed(_)
        ));
        // A brick that holds a mix of blocks, none of which blocks, is empty and not mixed.
        let mut blocks = vec![GLASS; BRICK_VOXEL_COUNT];
        for index in (0..BRICK_VOXEL_COUNT).step_by(7) {
            blocks[index] = LEAVES;
        }
        assert_eq!(occupancy_of_values(&blocks, None), BrickOccupancy::Empty);
    }

    #[test]
    fn what_blocks_does_not_depend_on_the_light_or_on_the_neighbours() {
        let mut blocks = vec![AIR; BRICK_VOXEL_COUNT];
        for index in (0..BRICK_VOXEL_COUNT).step_by(3) {
            blocks[index] = STONE;
        }
        let blocks = ChannelStorage::from_values(&blocks);
        let dark = ChannelStorage::uniform(0);
        let mut bright_values = vec![255u32; BRICK_VOXEL_COUNT];
        bright_values[9] = 3;
        let bright = ChannelStorage::from_values(&bright_values);
        let with_light = |light: Option<&ChannelStorage>| {
            BrickOccupancy::of(
                &BrickChannels {
                    blocks: &blocks,
                    states: None,
                    orientations: None,
                    light,
                },
                &table(),
            )
        };
        assert_eq!(with_light(None), with_light(Some(&dark)));
        assert_eq!(with_light(None), with_light(Some(&bright)));
    }

    #[test]
    fn a_buried_brick_has_no_mesh_and_is_full_and_a_brick_of_sky_has_no_mesh_and_is_empty() {
        // The trap of the design: the renderer holds nothing for either, and the walk must still stop in the one and go through the other.
        let table = table();
        let stone = ChannelStorage::uniform(STONE);
        let air = ChannelStorage::uniform(AIR);
        let around_stone = BrickNeighbourhood {
            center: BrickChannels::blocks_only(&stone),
            neighbours: [Some(BrickChannels::blocks_only(&stone)); 6],
        };
        assert!(mesh_brick(&around_stone, &table).is_empty());
        assert_eq!(
            BrickOccupancy::of(&around_stone.center, &table),
            BrickOccupancy::Full
        );
        let around_air = BrickNeighbourhood {
            center: BrickChannels::blocks_only(&air),
            neighbours: [Some(BrickChannels::blocks_only(&air)); 6],
        };
        assert!(mesh_brick(&around_air, &table).is_empty());
        assert_eq!(
            BrickOccupancy::of(&around_air.center, &table),
            BrickOccupancy::Empty
        );
    }

    proptest! {
        /// For random bricks (a base of one block and some edits, with states), voxel by voxel, a bit is set if and only if the table says the voxel's block and
        /// state are a whole opaque cube, and the class is `Empty` or `Full` exactly when no voxel or every voxel is.
        #[test]
        fn every_bit_is_what_the_table_says_of_its_voxel(
            base in prop_oneof![Just(AIR), Just(STONE), Just(ROCK), Just(GLASS), Just(LEAVES)],
            base_state in 0u32..3,
            edits in proptest::collection::vec(
                (0usize..BRICK_VOXEL_COUNT, 0u32..5, 0u32..3),
                0..40,
            ),
        ) {
            let table = table();
            let mut blocks = vec![base; BRICK_VOXEL_COUNT];
            let mut states = vec![if base == ROCK { base_state } else { 0 }; BRICK_VOXEL_COUNT];
            for (index, block, state) in edits {
                blocks[index] = block;
                states[index] = if block == ROCK { state } else { 0 };
            }
            let found = occupancy_of_values(&blocks, Some(&states));
            let expected: Vec<bool> = blocks
                .iter()
                .zip(&states)
                .map(|(&block, &state)| blocks_by_the_table(&table, block, state))
                .collect();
            for (index, &expected) in expected.iter().enumerate() {
                let voxel = LocalVoxelIndex::from_index(index).unwrap();
                prop_assert_eq!(found.blocks(voxel), expected, "voxel {}", index);
            }
            match &found {
                BrickOccupancy::Empty => prop_assert!(expected.iter().all(|&blocks| !blocks)),
                BrickOccupancy::Full => prop_assert!(expected.iter().all(|&blocks| blocks)),
                BrickOccupancy::Mixed(words) => {
                    prop_assert!(expected.iter().any(|&blocks| blocks));
                    prop_assert!(expected.iter().any(|&blocks| !blocks));
                    for (index, &expected) in expected.iter().enumerate() {
                        prop_assert_eq!((words[index / 32] >> (index % 32)) & 1 == 1, expected);
                    }
                }
            }
        }
    }
}

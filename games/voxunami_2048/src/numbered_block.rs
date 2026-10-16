//! The blocks of the world: a material and a number.
//!
//! In the voxels a numbered block is its material's block id (see [`Material::block_id`]) and, in
//! the number channel, its number as the voxel's state. The number is any positive whole number
//! that fits in 32 bits; nothing in the game limits it to powers of two or to a smaller range.

use crate::material::Material;

/// The block id of nothing at all.
pub const AIR: u32 = 0;

/// A block with a number on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NumberedBlock {
    pub material: Material,
    number: u32,
}

impl NumberedBlock {
    /// Panics if `number` is zero, which is not a number a block can show.
    pub fn new(material: Material, number: u32) -> Self {
        assert!(number > 0, "a block cannot be numbered zero");
        Self { material, number }
    }

    /// The number this block shows.
    pub fn number(self) -> u32 {
        self.number
    }

    /// The block id of this block in the world's content.
    pub fn block_id(self) -> u32 {
        self.material.block_id()
    }

    /// The state of the voxel this block is in: its number.
    pub fn state(self) -> u32 {
        self.number
    }

    /// The block a voxel holds, or `None` for air, for a block this game does not know, and for a
    /// numbered block whose number is zero (which no rule of the game makes).
    pub fn from_voxel(block_id: u32, state: u32) -> Option<Self> {
        let material = Material::from_block_id(block_id)?;
        (state > 0).then(|| Self::new(material, state))
    }

    /// Whether this block and `other` merge when they meet: their numbers are equal (whatever they
    /// are made of) and the double of the number still fits.
    pub fn merges_with(self, other: Self) -> bool {
        self.number == other.number && self.number.checked_mul(2).is_some()
    }

    /// What this block and `target`, which it merges with, become: a block of the double number that
    /// is made of whatever `target` is made of, since `target` is the one that stays.
    pub fn merged_into(self, target: Self) -> Self {
        debug_assert!(self.merges_with(target));
        Self::new(target.material, target.number * 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_is_in_the_voxels_as_its_materials_block_and_its_number_as_the_state() {
        for material in Material::ALL {
            for number in [1, 2, 3, 1000, u32::MAX] {
                let block = NumberedBlock::new(material, number);
                assert_eq!(
                    (block.block_id(), block.state()),
                    (material.block_id(), number)
                );
                assert_eq!(
                    NumberedBlock::from_voxel(block.block_id(), block.state()),
                    Some(block)
                );
            }
        }
    }

    #[test]
    fn a_voxel_that_is_not_a_numbered_block_is_not_one() {
        assert_eq!(NumberedBlock::from_voxel(AIR, 0), None);
        assert_eq!(NumberedBlock::from_voxel(AIR, 5), None, "air has no number");
        assert_eq!(NumberedBlock::from_voxel(4, 2), None, "no such material");
        assert_eq!(NumberedBlock::from_voxel(u32::MAX, 2), None);
        assert_eq!(
            NumberedBlock::from_voxel(Material::Stone.block_id(), 0),
            None,
            "a numbered block of zero is not made by any rule"
        );
    }

    #[test]
    fn equal_numbers_merge_into_the_double_in_the_material_of_the_one_that_stays() {
        let moving = NumberedBlock::new(Material::Ice, 12);
        let staying = NumberedBlock::new(Material::Stone, 12);
        assert!(moving.merges_with(staying) && staying.merges_with(moving));
        let merged = moving.merged_into(staying);
        assert_eq!(merged, NumberedBlock::new(Material::Stone, 24));
        assert!(!moving.merges_with(NumberedBlock::new(Material::Ice, 13)));
    }

    #[test]
    fn there_is_no_largest_power_of_two_below_what_the_state_holds() {
        // 2^30 merges into 2^31, and numbers that are not powers of two merge too.
        let big = NumberedBlock::new(Material::Stone, 1 << 30);
        assert_eq!(big.merged_into(big).number(), 1 << 31);
        let odd = NumberedBlock::new(Material::Ice, 3_000_000_001);
        assert!(!odd.merges_with(odd), "the double would not fit in 32 bits");
        let fits = NumberedBlock::new(Material::Ice, 2_147_483_647);
        assert_eq!(fits.merged_into(fits).number(), 4_294_967_294);
    }

    #[test]
    #[should_panic(expected = "numbered zero")]
    fn a_block_cannot_be_numbered_zero() {
        NumberedBlock::new(Material::Stone, 0);
    }
}

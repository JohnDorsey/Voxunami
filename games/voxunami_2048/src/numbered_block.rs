//! The blocks of the world: a material and a power of two.

use crate::material::Material;

/// The largest exponent a block can have; blocks of 2^20 do not merge any further.
pub const MAXIMUM_EXPONENT: u8 = 20;

/// The block id of nothing at all.
pub const AIR: u32 = 0;

/// A block with a number on it. The number is always a power of two, held as its exponent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NumberedBlock {
    pub material: Material,
    exponent: u8,
}

impl NumberedBlock {
    /// Panics unless `exponent` is in `1..=MAXIMUM_EXPONENT`.
    pub fn new(material: Material, exponent: u8) -> Self {
        assert!(
            (1..=MAXIMUM_EXPONENT).contains(&exponent),
            "exponent {exponent} is outside 1..={MAXIMUM_EXPONENT}"
        );
        Self { material, exponent }
    }

    /// The power of two this block shows.
    pub fn exponent(self) -> u8 {
        self.exponent
    }

    /// The number this block shows.
    pub fn value(self) -> u64 {
        1 << self.exponent
    }

    /// Whether this block and `other` merge when they meet: their numbers are equal (whatever they
    /// are made of) and there is a larger block to make.
    pub fn merges_with(self, other: Self) -> bool {
        self.exponent == other.exponent && self.exponent < MAXIMUM_EXPONENT
    }

    /// What this block and `target`, which it merges with, become: a block of the next number that
    /// is made of whatever `target` is made of, since `target` is the one that stays.
    pub fn merged_into(self, target: Self) -> Self {
        debug_assert!(self.merges_with(target));
        Self::new(target.material, target.exponent + 1)
    }

    /// The block id of this block. Blocks are registered in this order: air, then each material's
    /// blocks from exponent 1 up.
    pub fn id(self) -> u32 {
        (self.material.index() as u32) * u32::from(MAXIMUM_EXPONENT) + u32::from(self.exponent)
    }

    /// The block with this id, or `None` for air and for ids that are not numbered blocks.
    pub fn from_id(id: u32) -> Option<Self> {
        let offset = id.checked_sub(1)?;
        let per_material = u32::from(MAXIMUM_EXPONENT);
        let material = Material::from_index((offset / per_material) as usize)?;
        Some(Self::new(material, (offset % per_material) as u8 + 1))
    }

    /// The name that identifies this block in the world's content, after `voxunami_2048:`.
    pub fn name(self) -> String {
        format!("{}_{}", self.material.name(), self.value())
    }

    /// Every block, in id order.
    pub fn all() -> impl Iterator<Item = Self> {
        Material::ALL.into_iter().flat_map(|material| {
            (1..=MAXIMUM_EXPONENT).map(move |exponent| Self::new(material, exponent))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_follow_registration_order_and_map_both_ways() {
        let blocks: Vec<_> = NumberedBlock::all().collect();
        assert_eq!(blocks.len(), 3 * MAXIMUM_EXPONENT as usize);
        for (position, block) in blocks.iter().enumerate() {
            assert_eq!(block.id() as usize, position + 1);
            assert_eq!(NumberedBlock::from_id(block.id()), Some(*block));
        }
        assert_eq!(blocks[0], NumberedBlock::new(Material::Stone, 1));
        assert_eq!(NumberedBlock::from_id(AIR), None);
        assert_eq!(NumberedBlock::from_id(blocks.len() as u32 + 1), None);
        assert_eq!(NumberedBlock::from_id(u32::MAX), None);
    }

    #[test]
    fn names_show_the_material_and_the_number() {
        assert_eq!(NumberedBlock::new(Material::Stone, 1).name(), "stone_2");
        assert_eq!(NumberedBlock::new(Material::Ice, 11).name(), "ice_2048");
        assert_eq!(
            NumberedBlock::new(Material::SpringIce, 3).name(),
            "spring_ice_8"
        );
    }

    #[test]
    fn equal_numbers_merge_into_the_next_number_in_the_material_of_the_one_that_stays() {
        let moving = NumberedBlock::new(Material::Ice, 4);
        let staying = NumberedBlock::new(Material::Stone, 4);
        assert!(moving.merges_with(staying) && staying.merges_with(moving));
        let merged = moving.merged_into(staying);
        assert_eq!(merged, NumberedBlock::new(Material::Stone, 5));
        assert_eq!(merged.value(), 32);
        assert!(!moving.merges_with(NumberedBlock::new(Material::Ice, 5)));
    }

    #[test]
    fn the_largest_blocks_do_not_merge() {
        let largest = NumberedBlock::new(Material::Stone, MAXIMUM_EXPONENT);
        assert!(!largest.merges_with(largest));
        let nearly = NumberedBlock::new(Material::Stone, MAXIMUM_EXPONENT - 1);
        assert!(nearly.merges_with(nearly));
    }

    #[test]
    #[should_panic(expected = "exponent 0")]
    fn a_block_cannot_be_numbered_zero() {
        NumberedBlock::new(Material::Stone, 0);
    }

    #[test]
    #[should_panic(expected = "exponent 21")]
    fn a_block_cannot_be_numbered_beyond_the_largest() {
        NumberedBlock::new(Material::Stone, MAXIMUM_EXPONENT + 1);
    }
}

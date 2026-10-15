use crate::{BRICK_EDGE_LENGTH, BRICK_EDGE_LENGTH_LOG2, BRICK_VOXEL_COUNT};

/// The position of a voxel inside its brick, as one number in `0..BRICK_VOXEL_COUNT`.
///
/// The layout is x-fastest: `index = x + 32 * y + 32 * 32 * z`. Storage code indexes arrays with
/// it, and the order of [`LocalVoxelIndex::all`] is the canonical voxel order of a brick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalVoxelIndex(u16);

impl LocalVoxelIndex {
    /// Panics if any component is `BRICK_EDGE_LENGTH` or more.
    pub fn new(x: u8, y: u8, z: u8) -> Self {
        let edge_length = BRICK_EDGE_LENGTH as u8;
        assert!(
            x < edge_length && y < edge_length && z < edge_length,
            "local voxel ({x}, {y}, {z}) is outside a {edge_length}-voxel brick"
        );
        let (x, y, z) = (u16::from(x), u16::from(y), u16::from(z));
        Self(x | (y << BRICK_EDGE_LENGTH_LOG2) | (z << (2 * BRICK_EDGE_LENGTH_LOG2)))
    }

    /// `None` if `index` is not below `BRICK_VOXEL_COUNT`.
    pub fn from_index(index: usize) -> Option<Self> {
        (index < BRICK_VOXEL_COUNT).then_some(Self(index as u16))
    }

    /// The array index of this voxel within a brick.
    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    pub fn x(self) -> u8 {
        (self.0 & (BRICK_EDGE_LENGTH as u16 - 1)) as u8
    }

    pub fn y(self) -> u8 {
        ((self.0 >> BRICK_EDGE_LENGTH_LOG2) & (BRICK_EDGE_LENGTH as u16 - 1)) as u8
    }

    pub fn z(self) -> u8 {
        (self.0 >> (2 * BRICK_EDGE_LENGTH_LOG2)) as u8
    }

    /// Every voxel of a brick, in index order.
    pub fn all() -> impl ExactSizeIterator<Item = Self> + Clone {
        (0..BRICK_VOXEL_COUNT as u16).map(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_layout_is_x_fastest() {
        assert_eq!(LocalVoxelIndex::new(0, 0, 0).index(), 0);
        assert_eq!(LocalVoxelIndex::new(1, 0, 0).index(), 1);
        assert_eq!(LocalVoxelIndex::new(0, 1, 0).index(), 32);
        assert_eq!(LocalVoxelIndex::new(0, 0, 1).index(), 1024);
        assert_eq!(LocalVoxelIndex::new(31, 31, 31).index(), 32767);
        assert_eq!(
            LocalVoxelIndex::new(5, 7, 11).index(),
            5 + 7 * 32 + 11 * 1024
        );
    }

    #[test]
    fn components_round_trip_for_every_voxel() {
        for index in 0..BRICK_VOXEL_COUNT {
            let local = LocalVoxelIndex::from_index(index).unwrap();
            assert_eq!(local.index(), index);
            assert_eq!(LocalVoxelIndex::new(local.x(), local.y(), local.z()), local);
        }
    }

    #[test]
    fn from_index_rejects_out_of_range_values() {
        assert!(LocalVoxelIndex::from_index(BRICK_VOXEL_COUNT - 1).is_some());
        assert_eq!(LocalVoxelIndex::from_index(BRICK_VOXEL_COUNT), None);
        assert_eq!(LocalVoxelIndex::from_index(usize::MAX), None);
    }

    #[test]
    #[should_panic(expected = "outside a 32-voxel brick")]
    fn new_panics_when_a_component_reaches_the_edge_length() {
        LocalVoxelIndex::new(0, 32, 0);
    }

    #[test]
    fn all_enumerates_each_voxel_once_in_index_order() {
        let all = LocalVoxelIndex::all();
        assert_eq!(all.len(), BRICK_VOXEL_COUNT);
        for (expected_index, local) in all.enumerate() {
            assert_eq!(local.index(), expected_index);
        }
    }
}

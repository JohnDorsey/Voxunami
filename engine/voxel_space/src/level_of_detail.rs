/// How coarsely terrain is sampled: level `n` ignores features smaller than `2^n` voxels.
/// Level 0 is full detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LevelOfDetail(u8);

impl LevelOfDetail {
    pub const FULL_DETAIL: Self = Self(0);

    /// Largest level: a cell as wide as the whole octree's root cube is bricks across, 2^116 voxels (`region_octree::ROOT_LEVEL` is the same number; a test
    /// there checks it). A node of this level has 32 of those cells along an edge, 2^121 voxels, which still fits an `i128`.
    pub const MAXIMUM_LEVEL: u8 = 116;

    /// `None` above [`LevelOfDetail::MAXIMUM_LEVEL`].
    pub fn new(level: u8) -> Option<Self> {
        (level <= Self::MAXIMUM_LEVEL).then_some(Self(level))
    }

    pub fn level(self) -> u8 {
        self.0
    }

    pub fn edge_length_in_voxels(self) -> i128 {
        1 << self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_length_doubles_with_each_level() {
        assert_eq!(LevelOfDetail::FULL_DETAIL.edge_length_in_voxels(), 1);
        assert_eq!(LevelOfDetail::new(5).unwrap().edge_length_in_voxels(), 32);
        assert_eq!(
            LevelOfDetail::new(LevelOfDetail::MAXIMUM_LEVEL)
                .unwrap()
                .edge_length_in_voxels(),
            1 << 116
        );
        // The levels past the old limits of 40 and 60 are levels.
        for level in [40u8, 41, 60, 61, 63, 64, 65, 100] {
            assert_eq!(
                LevelOfDetail::new(level).unwrap().edge_length_in_voxels(),
                1i128 << level,
                "level {level}"
            );
        }
    }

    #[test]
    fn a_node_of_the_coarsest_level_still_fits_the_integers_that_hold_its_corners() {
        let cell = LevelOfDetail::new(LevelOfDetail::MAXIMUM_LEVEL)
            .unwrap()
            .edge_length_in_voxels();
        // 32 cells along an edge, and a margin of 32 cells beyond: the widest region the generators ask about.
        assert!(cell.checked_mul(96).is_some());
    }

    #[test]
    fn levels_beyond_the_maximum_are_rejected() {
        assert!(LevelOfDetail::new(117).is_none());
        assert!(LevelOfDetail::new(u8::MAX).is_none());
    }

    #[test]
    fn coarser_levels_order_after_finer_ones() {
        assert!(LevelOfDetail::FULL_DETAIL < LevelOfDetail::new(1).unwrap());
    }
}

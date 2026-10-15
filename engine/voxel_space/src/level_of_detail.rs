/// How coarsely terrain is sampled: level `n` ignores features smaller than `2^n` voxels.
/// Level 0 is full detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LevelOfDetail(u8);

impl LevelOfDetail {
    pub const FULL_DETAIL: Self = Self(0);

    /// Largest level; keeps `edge_length_in_voxels` far from overflow.
    pub const MAXIMUM_LEVEL: u8 = 60;

    /// `None` above [`LevelOfDetail::MAXIMUM_LEVEL`].
    pub fn new(level: u8) -> Option<Self> {
        (level <= Self::MAXIMUM_LEVEL).then_some(Self(level))
    }

    pub fn level(self) -> u8 {
        self.0
    }

    pub fn edge_length_in_voxels(self) -> i64 {
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
            1 << 60
        );
    }

    #[test]
    fn levels_beyond_the_maximum_are_rejected() {
        assert!(LevelOfDetail::new(61).is_none());
        assert!(LevelOfDetail::new(u8::MAX).is_none());
    }

    #[test]
    fn coarser_levels_order_after_finer_ones() {
        assert!(LevelOfDetail::FULL_DETAIL < LevelOfDetail::new(1).unwrap());
    }
}

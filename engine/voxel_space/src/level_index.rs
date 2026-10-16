use std::fmt;

/// Which level of a world a coordinate belongs to: the position of the level in the world's list of
/// levels. Every level has its own grid, so a coordinate means something only together with a level.
///
/// Levels are never removed or reordered while a world is in use, so an index keeps meaning the same
/// level. In files, levels are named by identifier instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LevelIndex(u32);

impl LevelIndex {
    /// The main level, where players start: the first.
    pub const MAIN: Self = Self(0);

    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for LevelIndex {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "level {}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_main_level_is_the_first_and_indices_order_and_print() {
        assert_eq!(LevelIndex::MAIN, LevelIndex::new(0));
        assert!(LevelIndex::MAIN < LevelIndex::new(1));
        assert_eq!(LevelIndex::new(7).get(), 7);
        assert_eq!(LevelIndex::new(3).to_string(), "level 3");
    }
}

//! The tools players push blocks with. A tool decides what happens when a block it pushed meets
//! another block: whether they combine and into what. What a block does on the way (how far it
//! slides, whether it bounces) is its material's business, not the tool's.

/// What meeting did to the two blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combination {
    /// The moving block is gone, and the block it struck stays, with this number, in its own
    /// material.
    Becomes(u32),
    /// Both blocks are gone.
    Vanish,
}

/// A tool for pushing blocks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tool {
    /// Merges equal numbers into their double: the move of the game 2048.
    #[default]
    Double,
    /// Adds any two numbers together.
    Add,
    /// Takes the smaller number from the larger; equal numbers cancel and both blocks vanish.
    Subtract,
}

impl Tool {
    pub const ALL: [Self; 3] = [Self::Double, Self::Add, Self::Subtract];

    /// The position of the tool in [`Tool::ALL`], which is how messages and saved worlds name it.
    pub fn index(self) -> u8 {
        self as u8
    }

    pub fn from_index(index: u8) -> Option<Self> {
        Self::ALL.get(usize::from(index)).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Double => "double",
            Self::Add => "add",
            Self::Subtract => "subtract",
        }
    }

    /// What happens when a block with number `moving`, pushed with this tool, meets one with number
    /// `struck`; `None` if they do not combine, and the moving block stops (or bounces). A result
    /// that does not fit in a block's 32-bit number is not a result.
    pub fn combine(self, moving: u32, struck: u32) -> Option<Combination> {
        match self {
            Self::Double => (moving == struck)
                .then(|| moving.checked_mul(2))
                .flatten()
                .map(Combination::Becomes),
            Self::Add => moving.checked_add(struck).map(Combination::Becomes),
            Self::Subtract => Some(if moving == struck {
                Combination::Vanish
            } else {
                Combination::Becomes(moving.abs_diff(struck))
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn tools_are_numbered_in_order_and_named_distinctly() {
        for (index, tool) in Tool::ALL.into_iter().enumerate() {
            assert_eq!(usize::from(tool.index()), index);
            assert_eq!(Tool::from_index(index as u8), Some(tool));
        }
        assert_eq!(Tool::from_index(3), None);
        assert_eq!(Tool::from_index(255), None);
        let names: Vec<_> = Tool::ALL.into_iter().map(Tool::name).collect();
        assert_eq!(names, ["double", "add", "subtract"]);
    }

    #[test]
    fn the_double_tool_merges_only_equal_numbers_into_their_double() {
        assert_eq!(Tool::Double.combine(4, 4), Some(Combination::Becomes(8)));
        assert_eq!(Tool::Double.combine(3, 3), Some(Combination::Becomes(6)));
        assert_eq!(Tool::Double.combine(4, 8), None);
        assert_eq!(Tool::Double.combine(8, 4), None);
        // The double must fit.
        assert_eq!(
            Tool::Double.combine(1 << 31, 1 << 31),
            None,
            "2^32 does not fit"
        );
        assert_eq!(
            Tool::Double.combine(1 << 30, 1 << 30),
            Some(Combination::Becomes(1 << 31))
        );
        assert_eq!(Tool::Double.combine(u32::MAX, u32::MAX), None);
    }

    #[test]
    fn the_add_tool_adds_any_two_numbers() {
        assert_eq!(Tool::Add.combine(2, 4), Some(Combination::Becomes(6)));
        assert_eq!(Tool::Add.combine(4, 2), Some(Combination::Becomes(6)));
        assert_eq!(Tool::Add.combine(5, 5), Some(Combination::Becomes(10)));
        assert_eq!(
            Tool::Add.combine(u32::MAX - 1, 1),
            Some(Combination::Becomes(u32::MAX))
        );
        assert_eq!(Tool::Add.combine(u32::MAX, 1), None, "the sum must fit");
        assert_eq!(Tool::Add.combine(1 << 31, 1 << 31), None);
    }

    #[test]
    fn the_subtract_tool_takes_the_smaller_from_the_larger_and_equal_numbers_cancel() {
        assert_eq!(Tool::Subtract.combine(7, 4), Some(Combination::Becomes(3)));
        assert_eq!(Tool::Subtract.combine(4, 7), Some(Combination::Becomes(3)));
        assert_eq!(Tool::Subtract.combine(5, 5), Some(Combination::Vanish));
        assert_eq!(
            Tool::Subtract.combine(u32::MAX, 1),
            Some(Combination::Becomes(u32::MAX - 1))
        );
        assert_eq!(
            Tool::Subtract.combine(1, u32::MAX),
            Some(Combination::Becomes(u32::MAX - 1))
        );
    }

    proptest! {
        /// Combining never makes a number of zero (which no block can be), and the two tools that
        /// keep the total keep it, in either order.
        #[test]
        fn results_are_numbers_and_double_and_add_keep_the_total(moving in 1u32.., struck in 1u32..) {
            for tool in Tool::ALL {
                if let Some(Combination::Becomes(number)) = tool.combine(moving, struck) {
                    prop_assert!(number > 0);
                }
            }
            for tool in [Tool::Double, Tool::Add] {
                if let Some(Combination::Becomes(number)) = tool.combine(moving, struck) {
                    prop_assert_eq!(u64::from(number), u64::from(moving) + u64::from(struck));
                }
            }
            prop_assert_eq!(Tool::Add.combine(moving, struck), Tool::Add.combine(struck, moving));
            prop_assert_eq!(Tool::Subtract.combine(moving, struck), Tool::Subtract.combine(struck, moving));
            if let Some(Combination::Becomes(number)) = Tool::Subtract.combine(moving, struck) {
                prop_assert!(number < moving.max(struck));
            }
        }
    }
}

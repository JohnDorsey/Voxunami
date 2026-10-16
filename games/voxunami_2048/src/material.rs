//! What numbered blocks are made of. The material decides how a block moves when it is pushed.

/// The most steps an ice block coasts after a push. An infinite world has nothing to stop a block
/// that never meets another, so every push has a range.
pub const COASTING_RANGE: u32 = 48;

/// The most blocks a push moves at once, the pushing block included: a block that pushes a row
/// moves it only if the row is no longer than this and has room to move.
pub const PUSH_LIMIT: u32 = 12;

/// How a material answers a push.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Motion {
    /// How many steps, one per tick, the block takes at most. A step into a block that cannot be
    /// merged with ends the motion, unless the material bounces.
    pub range: u32,
    /// Whether hitting a block that cannot be merged with sends the block back the way it came
    /// (using up a step) instead of stopping it.
    pub bounces: bool,
    /// Whether a block that meets a block it cannot merge with pushes it, and the row of blocks
    /// behind it, one step ahead (at most [`PUSH_LIMIT`] blocks, and only into empty space), instead
    /// of stopping. The pushed blocks move that one step and no more.
    pub pushes: bool,
}

/// How a moving block treats the blocks it meets: it merges with one its tool combines it with;
/// otherwise stone pushes it along, ice stops, and spring ice bounces. Materials that pass momentum
/// on will be added later.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Material {
    /// Moves one step per push, never coasts, and pushes a row of blocks.
    Stone,
    /// Coasts until it meets another block.
    Ice,
    /// Coasts like ice, and bounces back off blocks it cannot merge with.
    SpringIce,
}

impl Material {
    pub const ALL: [Self; 3] = [Self::Stone, Self::Ice, Self::SpringIce];

    /// The position of the material in [`Material::ALL`], which is the order its blocks are
    /// registered in.
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    /// The id of this material's block in the world's content: air is block 0, and the materials
    /// follow in order.
    pub fn block_id(self) -> u32 {
        self.index() as u32 + 1
    }

    /// The material whose block has this id, or `None` for air and for ids that are not one.
    pub fn from_block_id(id: u32) -> Option<Self> {
        Self::from_index(id.checked_sub(1)? as usize)
    }

    /// The material's name as it appears in block identifiers.
    pub fn name(self) -> &'static str {
        match self {
            Self::Stone => "stone",
            Self::Ice => "ice",
            Self::SpringIce => "spring_ice",
        }
    }

    pub fn motion(self) -> Motion {
        match self {
            Self::Stone => Motion {
                range: 1,
                bounces: false,
                pushes: true,
            },
            Self::Ice => Motion {
                range: COASTING_RANGE,
                bounces: false,
                pushes: false,
            },
            Self::SpringIce => Motion {
                range: COASTING_RANGE,
                bounces: true,
                pushes: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materials_are_numbered_in_order_and_named_distinctly() {
        for (index, material) in Material::ALL.into_iter().enumerate() {
            assert_eq!(material.index(), index);
            assert_eq!(Material::from_index(index), Some(material));
        }
        assert_eq!(Material::from_index(3), None);
        for material in Material::ALL {
            assert_eq!(Material::from_block_id(material.block_id()), Some(material));
        }
        assert_eq!(Material::Stone.block_id(), 1);
        assert_eq!(Material::from_block_id(0), None);
        assert_eq!(Material::from_block_id(4), None);
        let names: Vec<_> = Material::ALL.into_iter().map(Material::name).collect();
        assert_eq!(names, ["stone", "ice", "spring_ice"]);
    }

    #[test]
    fn stone_takes_one_step_and_pushes_the_ices_coast_and_only_spring_ice_bounces() {
        let stone = Material::Stone.motion();
        assert_eq!((stone.range, stone.bounces, stone.pushes), (1, false, true));
        let ice = Material::Ice.motion();
        assert_eq!(
            (ice.range, ice.bounces, ice.pushes),
            (COASTING_RANGE, false, false)
        );
        let spring = Material::SpringIce.motion();
        assert_eq!(
            (spring.range, spring.bounces, spring.pushes),
            (COASTING_RANGE, true, false)
        );
    }
}

use crate::shape::Shape;

/// What a type of block is like: how it looks, how it fills its cell, and how it treats light.
///
/// A block has one or more *states*, and each state has its own shape, so a block can be a whole
/// cube, a slab, a stair and so on without every shape being a block type of its own (block types
/// share a small palette of colours). A voxel holds a block and a state.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockDefinition {
    /// sRGB colour.
    pub color: [f32; 3],
    /// Whether the block lets what is behind it be seen and light through, so it hides nothing and
    /// holds light itself (like glass, and like air).
    pub see_through: bool,
    /// The light the block gives off, from 0 (none) to 255.
    pub emission: u8,
    shapes: Vec<Shape>,
}

impl BlockDefinition {
    /// An ordinary opaque whole cube, with one state.
    pub fn cube(color: [f32; 3]) -> Self {
        Self {
            color,
            see_through: false,
            emission: 0,
            shapes: vec![Shape::full()],
        }
    }

    /// Nothing: no shape, see-through, one state.
    pub fn air() -> Self {
        Self {
            color: [0.0; 3],
            see_through: true,
            emission: 0,
            shapes: vec![Shape::empty()],
        }
    }

    /// The same block with these shapes, the state being the position in the list.
    ///
    /// Panics if there are none, since a block always has a state 0.
    pub fn with_shapes(mut self, shapes: Vec<Shape>) -> Self {
        assert!(!shapes.is_empty(), "a block has at least one state");
        self.shapes = shapes;
        self
    }

    pub fn with_emission(mut self, emission: u8) -> Self {
        self.emission = emission;
        self
    }

    pub fn with_see_through(mut self, see_through: bool) -> Self {
        self.see_through = see_through;
        self
    }

    /// The shape of each state.
    pub fn shapes(&self) -> &[Shape] {
        &self.shapes
    }

    pub fn state_count(&self) -> usize {
        self.shapes.len()
    }
}

#[cfg(test)]
mod tests {
    use voxel_space::FaceDirection;

    use super::*;

    #[test]
    fn a_cube_is_opaque_whole_and_has_one_state_and_air_is_empty_and_see_through() {
        let cube = BlockDefinition::cube([0.2, 0.3, 0.4]);
        assert!(!cube.see_through && cube.emission == 0);
        assert_eq!(cube.state_count(), 1);
        assert!(cube.shapes()[0].is_full());
        let air = BlockDefinition::air();
        assert!(air.see_through && air.shapes()[0].is_empty());
        assert_eq!(air.state_count(), 1);
    }

    #[test]
    fn states_are_the_positions_of_the_shapes_and_the_rest_is_adjustable() {
        let slabs = BlockDefinition::cube([1.0; 3])
            .with_shapes(vec![
                Shape::full(),
                Shape::half(FaceDirection::NegativeY),
                Shape::half(FaceDirection::PositiveY),
            ])
            .with_emission(200)
            .with_see_through(true);
        assert_eq!(slabs.state_count(), 3);
        assert!(slabs.shapes()[1].covers_face(FaceDirection::NegativeY));
        assert_eq!((slabs.emission, slabs.see_through), (200, true));
    }

    #[test]
    #[should_panic(expected = "at least one state")]
    fn a_block_with_no_states_is_refused() {
        BlockDefinition::cube([0.0; 3]).with_shapes(Vec::new());
    }
}

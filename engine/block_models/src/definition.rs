use crate::property::{PropertyKey, PropertyType, SetProperties, key_id};
use crate::shape::Shape;

/// What a type of block is like: how it looks, how it fills its cell, and how it treats light.
///
/// A block has one or more *states*, and each state has its own shape, so a block can be a whole
/// cube, a slab, a stair and so on without every shape being a block type of its own (block types
/// share a small palette of colours). A voxel holds a block and a state.
///
/// A state can also change a block's colour ([`BlockDefinition::with_state_colors`]), and then it
/// need not choose a shape: a block whose states are numbers, say, shows a colour for each.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockDefinition {
    /// sRGB colour.
    pub color: [f32; 3],
    /// Whether the block lets what is behind it be seen and light through, so it hides nothing and
    /// holds light itself (like glass, and like air).
    pub see_through: bool,
    shapes: Vec<Shape>,
    state_colors: Vec<[f32; 3]>,
    properties: SetProperties,
}

impl BlockDefinition {
    /// An ordinary opaque whole cube, with one state.
    pub fn cube(color: [f32; 3]) -> Self {
        Self {
            color,
            see_through: false,
            shapes: vec![Shape::full()],
            state_colors: Vec::new(),
            properties: SetProperties::new(),
        }
    }

    /// Nothing: no shape, see-through, one state.
    pub fn air() -> Self {
        Self {
            color: [0.0; 3],
            see_through: true,
            shapes: vec![Shape::empty()],
            state_colors: Vec::new(),
            properties: SetProperties::new(),
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

    /// The same block with a colour for each state, state `s` being drawn in `colors[s % colors.len()]`
    /// instead of [`BlockDefinition::color`]. Any state, however large, has a colour (the colours
    /// repeat), so the states of a block can be numbers without a limit. Each colour takes one of the
    /// renderer's limited palette entries, so keep the list short.
    ///
    /// Panics if there are none.
    pub fn with_state_colors(mut self, colors: Vec<[f32; 3]>) -> Self {
        assert!(!colors.is_empty(), "a list of state colours has a colour");
        self.state_colors = colors;
        self
    }

    /// The same block with `value` for the property `key`, in every state (see [`crate::PropertyKey`]). The
    /// key must be declared by the schema of the table the definition goes in, or the table is refused.
    pub fn with_property<T: PropertyType>(mut self, key: &PropertyKey<T>, value: T) -> Self {
        self.properties
            .entry(key_id(key.id()))
            .or_default()
            .every_state = Some(value.to_value());
        self
    }

    /// The same block with `value` for the property `key` in one state, instead of what every state has
    /// (a lit lamp and an unlit one, a door open and shut). A state the block does not have is refused
    /// when the table is built.
    pub fn with_state_property<T: PropertyType>(
        mut self,
        state: u32,
        key: &PropertyKey<T>,
        value: T,
    ) -> Self {
        self.properties
            .entry(key_id(key.id()))
            .or_default()
            .by_state
            .insert(state, value.to_value());
        self
    }

    pub(crate) fn properties(&self) -> &SetProperties {
        &self.properties
    }

    pub fn with_see_through(mut self, see_through: bool) -> Self {
        self.see_through = see_through;
        self
    }

    /// The shape of each state.
    pub fn shapes(&self) -> &[Shape] {
        &self.shapes
    }

    /// The colours that states are drawn in, repeating; empty if every state has [`color`](Self::color).
    pub fn state_colors(&self) -> &[[f32; 3]] {
        &self.state_colors
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
        assert!(!cube.see_through);
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
            .with_see_through(true);
        assert_eq!(slabs.state_count(), 3);
        assert!(slabs.shapes()[1].covers_face(FaceDirection::NegativeY));
        assert!(slabs.see_through);
    }

    #[test]
    fn state_colours_are_optional_and_do_not_add_states() {
        let plain = BlockDefinition::cube([0.1; 3]);
        assert!(plain.state_colors().is_empty());
        let coloured = plain.with_state_colors(vec![[0.2; 3], [0.3; 3]]);
        assert_eq!(coloured.state_colors(), [[0.2; 3], [0.3; 3]]);
        assert_eq!(coloured.state_count(), 1, "colours are not shapes");
    }

    #[test]
    #[should_panic(expected = "has a colour")]
    fn an_empty_list_of_state_colours_is_refused() {
        BlockDefinition::cube([0.0; 3]).with_state_colors(Vec::new());
    }

    #[test]
    #[should_panic(expected = "at least one state")]
    fn a_block_with_no_states_is_refused() {
        BlockDefinition::cube([0.0; 3]).with_shapes(Vec::new());
    }
}

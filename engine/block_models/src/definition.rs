use crate::property::engine::{self, face_code};
use crate::property::{PropertyKey, PropertyType, SetProperties, key_id};
use crate::shape::Shape;
use namespaced_id::NamespacedId;
use voxel_space::FaceDirection;

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

    /// The same block with a face of each state drawn in `color` (sRGB bytes): `faces[state]` is the face marked in that state, if any.
    /// Every other face is the block's own colour. (The marked face takes a place in the renderer's palette, like a colour of a state.)
    pub fn with_marked_faces(self, color: [u8; 3], faces: &[Option<FaceDirection>]) -> Self {
        let packed = u32::from(color[0]) << 16 | u32::from(color[1]) << 8 | u32::from(color[2]);
        let mut marked = self.with_property(&engine::MARK_COLOR, packed);
        for (state, face) in faces.iter().enumerate() {
            marked =
                marked.with_state_property(state as u32, &engine::MARKED_FACE, face_code(*face));
        }
        marked
    }

    /// The same block with a docking face in each state: `faces[state]` is the face that docks in that state, if any.
    pub fn with_docking_faces(mut self, faces: &[Option<FaceDirection>]) -> Self {
        for (state, face) in faces.iter().enumerate() {
            self = self.with_state_property(state as u32, &engine::DOCKING_FACE, face_code(*face));
        }
        self
    }

    /// The same block with a front, as it is made, in every state: a piston-like block, turned to face the player when it is placed (`engine:front_face`).
    pub fn with_front_face(self, face: FaceDirection) -> Self {
        self.with_property(&engine::FRONT_FACE, face_code(Some(face)))
    }

    /// The same block symmetrical about a line, as it is made, in every state, along `face` and the face opposite it: a log-like block, turned to run
    /// along the face it is placed against (`engine:axis_face`).
    pub fn with_axis_face(self, face: FaceDirection) -> Self {
        self.with_property(&engine::AXIS_FACE, face_code(Some(face)))
    }

    /// The same block with the face opposite its marked face marked too, in every state: the two ends of a log.
    pub fn with_both_ends_marked(self) -> Self {
        self.with_property(&engine::MARK_BOTH_ENDS, true)
    }

    /// A definition from its parts, as decoded; the caller has checked them.
    pub(crate) fn from_parts(
        color: [f32; 3],
        see_through: bool,
        shapes: Vec<Shape>,
        state_colors: Vec<[f32; 3]>,
        properties: SetProperties,
    ) -> Self {
        Self {
            color,
            see_through,
            shapes,
            state_colors,
            properties,
        }
    }

    /// The same block with `value` for the property named `key`, in every state, for data that names properties
    /// by text (a pack); the table the definition goes in checks the key and the value.
    pub fn with_property_value(mut self, key: NamespacedId, value: i64) -> Self {
        self.set_property_value(key, None, value);
        self
    }

    /// The same, in one state only.
    pub fn with_state_property_value(mut self, state: u32, key: NamespacedId, value: i64) -> Self {
        self.set_property_value(key, Some(state), value);
        self
    }

    /// Sets a property in one state, or (with `None`) in every state, leaving what other states were given alone.
    pub fn set_property_value(&mut self, key: NamespacedId, state: Option<u32>, value: i64) {
        let set = self.properties.entry(key).or_default();
        match state {
            Some(state) => {
                set.by_state.insert(state, value);
            }
            None => set.every_state = Some(value),
        }
    }

    /// What a property is in a state (or, with `None`, in every state that has no value of its own): what the block
    /// sets, else `default`.
    pub fn property_value(&self, key: &NamespacedId, state: Option<u32>, default: i64) -> i64 {
        let Some(set) = self.properties.get(key) else {
            return default;
        };
        state
            .and_then(|state| set.by_state.get(&state).copied())
            .or(set.every_state)
            .unwrap_or(default)
    }

    /// Takes a property back to what the table says by default: in one state only (that state's own value goes), or,
    /// with `None`, in every state, as if the block had never set it. Returns whether the block had set anything there.
    pub fn remove_property_value(&mut self, key: &NamespacedId, state: Option<u32>) -> bool {
        match state {
            Some(state) => {
                let Some(set) = self.properties.get_mut(key) else {
                    return false;
                };
                let removed = set.by_state.remove(&state).is_some();
                if set.every_state.is_none() && set.by_state.is_empty() {
                    self.properties.remove(key);
                }
                removed
            }
            None => self.properties.remove(key).is_some(),
        }
    }

    /// A new state after the last, with this shape.
    pub fn add_state(&mut self, shape: Shape) {
        self.shapes.push(shape);
    }

    /// Replaces the colours that states are drawn in; none means every state has [`BlockDefinition::color`].
    pub fn set_state_colors(&mut self, colors: Vec<[f32; 3]>) {
        self.state_colors = colors;
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
    fn a_property_is_read_set_and_removed_by_name_and_state() {
        let key: NamespacedId = "test:glow".parse().unwrap();
        let mut block = BlockDefinition::cube([0.0; 3]).with_shapes(vec![
            Shape::full(),
            Shape::empty(),
            Shape::full(),
        ]);
        assert_eq!(
            block.property_value(&key, None, 7),
            7,
            "the default where nothing is set"
        );
        block.set_property_value(key.clone(), None, 3);
        block.set_property_value(key.clone(), Some(1), 9);
        assert_eq!(block.property_value(&key, None, 7), 3);
        assert_eq!(
            block.property_value(&key, Some(0), 7),
            3,
            "a state without its own value has the block's"
        );
        assert_eq!(block.property_value(&key, Some(1), 7), 9);
        // Setting for every state leaves a state's own value alone.
        block.set_property_value(key.clone(), None, 4);
        assert_eq!(block.property_value(&key, Some(1), 7), 9);
        // Removing one state's value gives it back the block's; removing the block's removes everything.
        assert!(block.remove_property_value(&key, Some(1)));
        assert_eq!(block.property_value(&key, Some(1), 7), 4);
        assert!(
            !block.remove_property_value(&key, Some(1)),
            "there was nothing left to remove"
        );
        block.set_property_value(key.clone(), Some(2), 5);
        assert!(block.remove_property_value(&key, None));
        assert_eq!(block.property_value(&key, Some(2), 7), 7);
        assert!(!block.remove_property_value(&key, None));
    }

    #[test]
    fn a_state_can_be_added_and_state_colours_replaced_or_cleared() {
        let mut block = BlockDefinition::cube([0.5; 3]);
        block.add_state(Shape::empty());
        assert_eq!(block.state_count(), 2);
        assert!(block.shapes()[1].is_empty());
        block.set_state_colors(vec![[0.1; 3], [0.2; 3]]);
        assert_eq!(block.state_colors(), [[0.1; 3], [0.2; 3]]);
        block.set_state_colors(Vec::new());
        assert!(block.state_colors().is_empty());
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

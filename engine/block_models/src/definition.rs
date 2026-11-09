use crate::property::engine::{self, face_code};
use crate::property::{PropertyKey, PropertyType, SetProperties, key_id};
use crate::shape::Shape;
use crate::transparency::Transparency;
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
    /// Whether the block lets what is behind it be seen, so it hides nothing (like glass, and like air). Whether light passes is [`BlockDefinition::light_through`].
    pub sight_through: bool,
    /// Whether light passes through the block and the voxel holds light itself (like glass, and like air).
    pub light_through: bool,
    transparency: Transparency,
    shapes: Vec<Shape>,
    state_colors: Vec<[f32; 3]>,
    /// The flat colour of each face of the block as it is made (indexed by [`FaceDirection::index`]), for the faces that differ from `color`.
    face_colors: [Option<[f32; 3]>; 6],
    /// The base name of the block's pictures (such as `base_blocks:log`), if it has any: the files `log_top`, `log_side`, `log_bottom` or `log`
    /// of that namespace say what each face looks like (see `docs/specs/block-textures-spec.md`).
    texture: Option<NamespacedId>,
    /// Whether [`BlockDefinition::color`] is the average of the pictures and not a colour the author chose, so that it follows them if they change.
    /// Only a block with a texture can have it.
    color_from_pictures: bool,
    /// Which pictures those are, once the pictures of the world are known (`None` until then, and for a block with no texture).
    pictures: Option<FacePictures>,
    properties: SetProperties,
}

/// The most different sizes of picture a world uses at once (each size is an atlas of its own on the GPU).
pub const MOST_TILE_SIZES: usize = 4;
/// The most pictures of one size a world uses.
pub const MOST_TILES_OF_A_SIZE: usize = 256;
/// The most bytes of pixels (8-bit RGBA) all the pictures of a world take in the package a client is sent.
pub const MOST_TILE_BYTES: usize = 8 << 20;

/// Where a picture is among the pictures a world uses: the number of its size (the sizes in use, smallest first) and its place among the pictures of that size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileRef {
    class: u8,
    layer: u16,
}

impl TileRef {
    /// The picture at `layer` among those of size number `class`; `None` if a world has no such place for one
    /// ([`MOST_TILE_SIZES`], [`MOST_TILES_OF_A_SIZE`]).
    pub fn new(class: usize, layer: usize) -> Option<Self> {
        (class < MOST_TILE_SIZES && layer < MOST_TILES_OF_A_SIZE).then_some(Self {
            class: class as u8,
            layer: layer as u16,
        })
    }

    /// Which of the sizes in use, smallest first.
    pub fn class(self) -> usize {
        usize::from(self.class)
    }

    /// Which of the pictures of that size.
    pub fn layer(self) -> usize {
        usize::from(self.layer)
    }
}

/// The picture of each face of a block as it is made: the top (+y), the four sides, and the bottom (-y).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FacePictures {
    pub top: TileRef,
    pub side: TileRef,
    pub bottom: TileRef,
}

/// Everything a definition is made of, for the decoder that has read it all.
pub(crate) struct Parts {
    pub color: [f32; 3],
    pub sight_through: bool,
    pub light_through: bool,
    pub transparency: Transparency,
    pub shapes: Vec<Shape>,
    pub state_colors: Vec<[f32; 3]>,
    pub face_colors: [Option<[f32; 3]>; 6],
    pub texture: Option<NamespacedId>,
    pub color_from_pictures: bool,
    pub pictures: Option<FacePictures>,
    pub properties: SetProperties,
}

impl BlockDefinition {
    /// An ordinary opaque whole cube, with one state.
    pub fn cube(color: [f32; 3]) -> Self {
        Self {
            color,
            sight_through: false,
            light_through: false,
            transparency: Transparency::Opaque,
            shapes: vec![Shape::full()],
            state_colors: Vec::new(),
            face_colors: [None; 6],
            texture: None,
            color_from_pictures: false,
            pictures: None,
            properties: SetProperties::new(),
        }
    }

    /// Nothing: no shape, see-through, one state.
    pub fn air() -> Self {
        Self {
            color: [0.0; 3],
            sight_through: true,
            light_through: true,
            transparency: Transparency::Opaque,
            shapes: vec![Shape::empty()],
            state_colors: Vec::new(),
            face_colors: [None; 6],
            texture: None,
            color_from_pictures: false,
            pictures: None,
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

    /// The same block with these faces drawn in their own colours, whatever the state, and every other face in the block's own (or its state's).
    /// The faces are those of the block as it is made, and turn with the voxel. A face given twice has the last colour.
    /// (Each distinct colour takes a place in the renderer's palette, like a colour of a state.)
    pub fn with_face_colors(mut self, colors: &[(FaceDirection, [f32; 3])]) -> Self {
        for (face, color) in colors {
            self.face_colors[usize::from(face.index())] = Some(*color);
        }
        self
    }

    /// The same block with pictures: `texture` is their base name (such as `base_blocks:log`).
    pub fn with_texture(mut self, texture: NamespacedId) -> Self {
        self.texture = Some(texture);
        self
    }

    /// The same block, whose colour is the average of its pictures and follows them if they change (see [`BlockDefinition::color_from_pictures`]).
    ///
    /// Panics if the block has no texture: there would be nothing to follow.
    pub fn with_color_from_pictures(mut self) -> Self {
        assert!(
            self.texture.is_some(),
            "a colour cannot follow pictures that the block does not have"
        );
        self.color_from_pictures = true;
        self
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

    /// A definition from its parts, as decoded; the caller has checked them.
    pub(crate) fn from_parts(parts: Parts) -> Self {
        Self {
            color: parts.color,
            sight_through: parts.sight_through,
            light_through: parts.light_through,
            transparency: parts.transparency,
            shapes: parts.shapes,
            state_colors: parts.state_colors,
            face_colors: parts.face_colors,
            texture: parts.texture,
            color_from_pictures: parts.color_from_pictures,
            pictures: parts.pictures,
            properties: parts.properties,
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

    /// Gives the block pictures with this base name, or none. A block with none has a colour of its own, whatever it was.
    pub fn set_texture(&mut self, texture: Option<NamespacedId>) {
        if texture.is_none() {
            self.color_from_pictures = false;
            self.pictures = None;
        }
        self.texture = texture;
    }

    /// Says which pictures the block's texture is (or, with `None`, that it is not known). A block with no texture has none.
    ///
    /// Panics if the block has no texture.
    pub fn set_pictures(&mut self, pictures: Option<FacePictures>) {
        assert!(
            pictures.is_none() || self.texture.is_some(),
            "a block with no texture has no pictures"
        );
        self.pictures = pictures;
    }

    /// The same block with these pictures for its texture.
    ///
    /// Panics if the block has no texture.
    pub fn with_pictures(mut self, pictures: FacePictures) -> Self {
        self.set_pictures(Some(pictures));
        self
    }

    /// The block's colour is this, chosen by whoever wrote the block: it no longer follows the pictures.
    pub fn set_color(&mut self, color: [f32; 3]) {
        self.color = color;
        self.color_from_pictures = false;
    }

    /// The block's colour is this because it is the average of the pictures: it goes on following them. For whoever works the average out.
    ///
    /// Panics if the colour does not follow pictures.
    pub fn set_color_from_pictures(&mut self, color: [f32; 3]) {
        assert!(
            self.color_from_pictures,
            "the colour of this block is its own and does not follow pictures"
        );
        self.color = color;
    }

    /// Replaces the colours of faces; all `None` means every face has the block's own colour.
    pub fn set_face_colors(&mut self, colors: [Option<[f32; 3]>; 6]) {
        self.face_colors = colors;
    }

    pub(crate) fn properties(&self) -> &SetProperties {
        &self.properties
    }

    pub fn with_sight_through(mut self, sight_through: bool) -> Self {
        self.sight_through = sight_through;
        self
    }

    pub fn with_light_through(mut self, light_through: bool) -> Self {
        self.light_through = light_through;
        self
    }

    /// The same block drawn in this mode (see [`Transparency`]). A mode other than the opaque one is for a block that is `sight_through`: a pack that says otherwise is refused.
    pub fn with_transparency(mut self, transparency: Transparency) -> Self {
        self.transparency = transparency;
        self
    }

    /// Sets how the block shows its faces and its pictures' clear pixels.
    pub fn set_transparency(&mut self, transparency: Transparency) {
        self.transparency = transparency;
    }

    /// How the block shows its faces and its pictures' clear pixels.
    pub fn transparency(&self) -> Transparency {
        self.transparency
    }

    /// The shape of each state.
    pub fn shapes(&self) -> &[Shape] {
        &self.shapes
    }

    /// The colours that states are drawn in, repeating; empty if every state has [`color`](Self::color).
    pub fn state_colors(&self) -> &[[f32; 3]] {
        &self.state_colors
    }

    /// The base name of the block's pictures, if it has any.
    pub fn texture(&self) -> Option<&NamespacedId> {
        self.texture.as_ref()
    }

    /// The pictures of the block's texture, once the pictures of the world are settled.
    pub fn pictures(&self) -> Option<&FacePictures> {
        self.pictures.as_ref()
    }

    /// Whether the block's colour is the average of its pictures (and follows them), and not a colour its author chose.
    pub fn color_from_pictures(&self) -> bool {
        self.color_from_pictures
    }

    /// The colour of each face as the block is made, indexed by [`FaceDirection::index`]; `None` for a face that has the block's own colour.
    pub fn face_colors(&self) -> &[Option<[f32; 3]>; 6] {
        &self.face_colors
    }

    pub fn state_count(&self) -> usize {
        self.shapes.len()
    }
}

/// A colour given in sRGB bytes, as the colours of a definition are (a byte over 255).
pub fn color_from_bytes(bytes: [u8; 3]) -> [f32; 3] {
    bytes.map(|byte| f32::from(byte) / 255.0)
}

#[cfg(test)]
mod tests {
    use voxel_space::FaceDirection;

    use super::*;

    #[test]
    fn a_cube_is_opaque_whole_and_has_one_state_and_air_is_empty_and_sight_through() {
        let cube = BlockDefinition::cube([0.2, 0.3, 0.4]);
        assert!(!cube.sight_through && !cube.light_through);
        assert_eq!(cube.transparency(), Transparency::Opaque);
        assert_eq!(cube.state_count(), 1);
        assert!(cube.shapes()[0].is_full());
        let air = BlockDefinition::air();
        assert!(air.sight_through && air.light_through && air.shapes()[0].is_empty());
        assert_eq!(air.transparency(), Transparency::Opaque);
        assert_eq!(air.state_count(), 1);
    }

    #[test]
    fn seeing_through_a_block_and_light_passing_through_it_are_separate_and_so_is_the_mode() {
        let tinted_glass = BlockDefinition::cube([0.5; 3]).with_sight_through(true);
        assert!(tinted_glass.sight_through && !tinted_glass.light_through);
        let dark_window = BlockDefinition::cube([0.5; 3]).with_light_through(true);
        assert!(!dark_window.sight_through && dark_window.light_through);
        let mut leaves = BlockDefinition::cube([0.1, 0.5, 0.2])
            .with_sight_through(true)
            .with_light_through(true)
            .with_transparency(Transparency::Leaves);
        assert_eq!(leaves.transparency(), Transparency::Leaves);
        leaves.set_transparency(Transparency::Glass);
        assert_eq!(leaves.transparency(), Transparency::Glass);
    }

    #[test]
    fn states_are_the_positions_of_the_shapes_and_the_rest_is_adjustable() {
        let slabs = BlockDefinition::cube([1.0; 3])
            .with_shapes(vec![
                Shape::full(),
                Shape::half(FaceDirection::NegativeY),
                Shape::half(FaceDirection::PositiveY),
            ])
            .with_sight_through(true);
        assert_eq!(slabs.state_count(), 3);
        assert!(slabs.shapes()[1].covers_face(FaceDirection::NegativeY));
        assert!(slabs.sight_through);
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
    fn faces_have_the_blocks_colour_unless_given_their_own_and_the_last_colour_for_a_face_wins() {
        let plain = BlockDefinition::cube([0.1; 3]);
        assert_eq!(plain.face_colors(), &[None; 6]);
        let yellow = color_from_bytes([255, 216, 0]);
        let log = plain.with_face_colors(&[
            (FaceDirection::PositiveY, [0.5; 3]),
            (FaceDirection::NegativeY, yellow),
            (FaceDirection::PositiveY, [0.7; 3]),
        ]);
        let mut expected = [None; 6];
        expected[usize::from(FaceDirection::PositiveY.index())] = Some([0.7; 3]);
        expected[usize::from(FaceDirection::NegativeY.index())] = Some(yellow);
        assert_eq!(log.face_colors(), &expected);
        assert_eq!(log.color, [0.1; 3], "the block's own colour is untouched");
        assert_eq!(yellow, [1.0, 216.0 / 255.0, 0.0]);
    }

    #[test]
    fn a_block_has_pictures_or_not_and_a_colour_that_follows_them_only_if_it_has() {
        let plain = BlockDefinition::cube([0.1; 3]);
        assert_eq!(
            (plain.texture(), plain.color_from_pictures()),
            (None, false)
        );
        let log: NamespacedId = "base_blocks:log".parse().unwrap();
        let textured = plain.clone().with_texture(log.clone());
        assert_eq!(textured.texture(), Some(&log));
        assert!(
            !textured.color_from_pictures(),
            "its colour is its own until it is said to follow"
        );
        let following = textured.clone().with_color_from_pictures();
        assert!(following.color_from_pictures());
        assert_ne!(following, textured, "the two are not the same block");
    }

    fn tile(class: usize, layer: usize) -> TileRef {
        TileRef::new(class, layer).expect("a place for a picture")
    }

    fn pictures() -> FacePictures {
        FacePictures {
            top: tile(0, 3),
            side: tile(1, 0),
            bottom: tile(3, 255),
        }
    }

    #[test]
    fn a_picture_has_a_place_among_four_sizes_and_256_of_each_and_no_other() {
        assert_eq!((tile(0, 0).class(), tile(0, 0).layer()), (0, 0));
        assert_eq!((tile(3, 255).class(), tile(3, 255).layer()), (3, 255));
        assert_eq!(TileRef::new(4, 0), None);
        assert_eq!(TileRef::new(0, 256), None);
        assert_eq!(TileRef::new(usize::MAX, 0), None);
        assert!(tile(0, 1) < tile(0, 2) && tile(0, 255) < tile(1, 0));
    }

    #[test]
    fn a_block_with_a_texture_can_be_told_which_pictures_it_has_and_losing_the_texture_loses_them()
    {
        let log: NamespacedId = "base_blocks:log".parse().unwrap();
        let block = BlockDefinition::cube([0.1; 3]).with_texture(log);
        assert_eq!(
            block.pictures(),
            None,
            "not known until the pictures are settled"
        );
        let settled = block.clone().with_pictures(pictures());
        assert_eq!(settled.pictures(), Some(&pictures()));
        let mut bare = settled;
        bare.set_texture(None);
        assert_eq!(bare.pictures(), None);
        // Telling it that it does not know is allowed with no texture too.
        bare.set_pictures(None);
    }

    #[test]
    #[should_panic(expected = "no texture has no pictures")]
    fn a_block_with_no_texture_has_no_pictures() {
        BlockDefinition::cube([0.1; 3]).with_pictures(pictures());
    }

    #[test]
    #[should_panic(expected = "cannot follow pictures")]
    fn a_colour_cannot_follow_pictures_that_are_not_there() {
        BlockDefinition::cube([0.1; 3]).with_color_from_pictures();
    }

    #[test]
    fn choosing_a_colour_or_taking_the_pictures_away_ends_the_following() {
        let log: NamespacedId = "base_blocks:log".parse().unwrap();
        let following = BlockDefinition::cube([0.1; 3])
            .with_texture(log.clone())
            .with_color_from_pictures();
        let mut chosen = following.clone();
        chosen.set_color([0.2; 3]);
        assert_eq!(
            (chosen.color, chosen.color_from_pictures()),
            ([0.2; 3], false)
        );
        assert_eq!(chosen.texture(), Some(&log), "the pictures stay");
        let mut bare = following.clone();
        bare.set_texture(None);
        assert_eq!((bare.texture(), bare.color_from_pictures()), (None, false));
        assert_eq!(bare.color, [0.1; 3], "the colour it had stays, as its own");
        // Worked out from the pictures, the colour changes and goes on following.
        let mut worked_out = following;
        worked_out.set_color_from_pictures([0.3; 3]);
        assert_eq!(
            (worked_out.color, worked_out.color_from_pictures()),
            ([0.3; 3], true)
        );
    }

    #[test]
    #[should_panic(expected = "does not follow pictures")]
    fn a_colour_that_is_the_authors_is_not_set_as_an_average() {
        BlockDefinition::cube([0.1; 3]).set_color_from_pictures([0.2; 3]);
    }

    #[test]
    fn face_colours_can_be_replaced_and_cleared() {
        let mut block = BlockDefinition::cube([0.5; 3])
            .with_face_colors(&[(FaceDirection::PositiveX, [0.2; 3])]);
        block.set_face_colors([None; 6]);
        assert_eq!(block.face_colors(), &[None; 6]);
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

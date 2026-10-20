//! The byte form of block definitions and of the property schema, for sending them to clients.
//!
//! Canonical: one set of definitions has one byte string. Counts and numbers are varints, identifiers
//! are text, colours are the bits of their `f32`s, and everything that is a map is in key order.
//! Decoding refuses anything that would not have been written (out of order or repeated keys, empty or
//! out-of-range boxes, non-finite colours, a kind that does not exist), so a damaged or hostile package
//! is an error and never a panic or a second spelling of the same package.

use std::collections::BTreeMap;

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;

use crate::definition::BlockDefinition;
use crate::property::{
    PropertyDeclaration, PropertyKind, PropertySchema, SetProperties, SetValues, check_value,
};
use crate::shape::{SUBDIVISIONS, Shape, ShapeBox};

const KIND_BOOL: u8 = 0;
const KIND_INTEGER: u8 = 1;

/// The least bytes a declaration, a box, a colour and a set property take, for `read_count`.
const DECLARATION_BYTES: usize = 5;
const BOX_BYTES: usize = 6;
const COLOR_BYTES: usize = 12;
const SET_PROPERTY_BYTES: usize = 4;

fn write_color(writer: &mut ByteWriter, color: [f32; 3]) {
    for component in color {
        writer.write_u32(component.to_bits());
    }
}

fn read_color(reader: &mut ByteReader<'_>) -> Result<[f32; 3], DecodeError> {
    let mut color = [0.0; 3];
    for component in &mut color {
        *component = f32::from_bits(reader.read_u32()?);
        if !component.is_finite() {
            return Err(DecodeError::Invalid("a colour is not a finite number"));
        }
    }
    Ok(color)
}

/// Reads an identifier that must be greater than the one before it, as the keys of a map are written.
fn read_ascending_identifier(
    reader: &mut ByteReader<'_>,
    previous: &mut Option<NamespacedId>,
) -> Result<NamespacedId, DecodeError> {
    let identifier = reader.read_identifier()?;
    if previous
        .as_ref()
        .is_some_and(|before| *before >= identifier)
    {
        return Err(DecodeError::Invalid(
            "identifiers are repeated or out of order",
        ));
    }
    *previous = Some(identifier.clone());
    Ok(identifier)
}

impl PropertySchema {
    /// Writes every declared property, in the order of their names.
    pub fn encode(&self, writer: &mut ByteWriter) {
        writer.write_varint(self.iter().count() as u64);
        for declaration in self.iter() {
            writer.write_identifier(&declaration.id);
            match declaration.kind {
                PropertyKind::Bool => writer.write_u8(KIND_BOOL),
                PropertyKind::Integer { minimum, maximum } => {
                    writer.write_u8(KIND_INTEGER);
                    writer.write_signed_varint(minimum);
                    writer.write_signed_varint(maximum);
                }
            }
            writer.write_signed_varint(declaration.default);
            writer.write_string(&declaration.doc);
        }
    }

    /// Reads what [`PropertySchema::encode`] wrote. The result declares exactly what was written (the
    /// engine's own properties are in it only if they were sent).
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, DecodeError> {
        let count = reader.read_count(DECLARATION_BYTES)?;
        let mut schema = Self::empty();
        let mut previous = None;
        for _ in 0..count {
            let id = read_ascending_identifier(reader, &mut previous)?;
            let kind = match reader.read_u8()? {
                KIND_BOOL => PropertyKind::Bool,
                KIND_INTEGER => {
                    let minimum = reader.read_signed_varint()?;
                    let maximum = reader.read_signed_varint()?;
                    if minimum > maximum {
                        return Err(DecodeError::Invalid("a property's range is empty"));
                    }
                    PropertyKind::Integer { minimum, maximum }
                }
                _ => {
                    return Err(DecodeError::Invalid(
                        "a property has a kind that is not known",
                    ));
                }
            };
            let declaration = PropertyDeclaration {
                id,
                kind,
                default: reader.read_signed_varint()?,
                doc: reader.read_string()?.to_owned(),
            };
            check_value(&declaration, declaration.default)
                .map_err(|_| DecodeError::Invalid("a property's default is outside its range"))?;
            schema
                .declare(declaration)
                .expect("identifiers were checked to be distinct");
        }
        Ok(schema)
    }
}

impl BlockDefinition {
    /// Writes the definition: colour, see-through, the boxes of every state's shape, the colours of
    /// states, and the properties it sets.
    pub fn encode(&self, writer: &mut ByteWriter) {
        write_color(writer, self.color);
        writer.write_u8(u8::from(self.see_through));
        writer.write_varint(self.shapes().len() as u64);
        for shape in self.shapes() {
            writer.write_varint(shape.boxes().len() as u64);
            for shape_box in shape.boxes() {
                for value in shape_box.min.into_iter().chain(shape_box.max) {
                    writer.write_u8(value);
                }
            }
        }
        writer.write_varint(self.state_colors().len() as u64);
        for color in self.state_colors() {
            write_color(writer, *color);
        }
        writer.write_varint(self.properties().len() as u64);
        for (key, set) in self.properties() {
            writer.write_identifier(key);
            match set.every_state {
                Some(value) => {
                    writer.write_u8(1);
                    writer.write_signed_varint(value);
                }
                None => writer.write_u8(0),
            }
            writer.write_varint(set.by_state.len() as u64);
            for (state, value) in &set.by_state {
                writer.write_varint(u64::from(*state));
                writer.write_signed_varint(*value);
            }
        }
    }

    /// Reads what [`BlockDefinition::encode`] wrote. Whether its properties exist and its values fit is
    /// for the table that is built from it to say.
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, DecodeError> {
        let color = read_color(reader)?;
        let see_through = match reader.read_u8()? {
            0 => false,
            1 => true,
            _ => return Err(DecodeError::Invalid("see-through is neither 0 nor 1")),
        };
        let state_count = reader.read_count(1)?;
        if state_count == 0 {
            return Err(DecodeError::Invalid("a block has no states"));
        }
        let mut shapes = Vec::with_capacity(state_count);
        for _ in 0..state_count {
            let box_count = reader.read_count(BOX_BYTES)?;
            let mut boxes = Vec::with_capacity(box_count);
            for _ in 0..box_count {
                let mut corners = [0u8; 6];
                for corner in &mut corners {
                    *corner = reader.read_u8()?;
                }
                let shape_box = ShapeBox {
                    min: [corners[0], corners[1], corners[2]],
                    max: [corners[3], corners[4], corners[5]],
                };
                if (0..3).any(|axis| {
                    shape_box.min[axis] >= shape_box.max[axis]
                        || usize::from(shape_box.max[axis]) > SUBDIVISIONS
                }) {
                    return Err(DecodeError::Invalid("a box is empty or leaves the voxel"));
                }
                boxes.push(shape_box);
            }
            shapes.push(Shape::from_boxes(&boxes));
        }
        let color_count = reader.read_count(COLOR_BYTES)?;
        let mut state_colors = Vec::with_capacity(color_count);
        for _ in 0..color_count {
            state_colors.push(read_color(reader)?);
        }
        let set_count = reader.read_count(SET_PROPERTY_BYTES)?;
        let mut properties = SetProperties::new();
        let mut previous = None;
        for _ in 0..set_count {
            let key = read_ascending_identifier(reader, &mut previous)?;
            let every_state = match reader.read_u8()? {
                0 => None,
                1 => Some(reader.read_signed_varint()?),
                _ => return Err(DecodeError::Invalid("a property's flag is neither 0 nor 1")),
            };
            let by_state_count = reader.read_count(2)?;
            let mut by_state = BTreeMap::new();
            for _ in 0..by_state_count {
                let state = u32::try_from(reader.read_varint()?)
                    .map_err(|_| DecodeError::Invalid("a state number is too large"))?;
                let value = reader.read_signed_varint()?;
                if by_state
                    .last_key_value()
                    .is_some_and(|(last, _)| *last >= state)
                {
                    return Err(DecodeError::Invalid("states are repeated or out of order"));
                }
                by_state.insert(state, value);
            }
            properties.insert(
                key,
                SetValues {
                    every_state,
                    by_state,
                },
            );
        }
        Ok(Self::from_parts(
            color,
            see_through,
            shapes,
            state_colors,
            properties,
        ))
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use voxel_space::FaceDirection;

    use super::*;
    use crate::property::engine::{DENSITY, PLACEABLE_ON_MOVING_GRIDS};

    fn encoded(definition: &BlockDefinition) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        definition.encode(&mut writer);
        writer.into_bytes()
    }

    fn decoded(bytes: &[u8]) -> Result<BlockDefinition, DecodeError> {
        let mut reader = ByteReader::new(bytes);
        let definition = BlockDefinition::decode(&mut reader)?;
        reader.finish()?;
        Ok(definition)
    }

    fn schema_bytes(schema: &PropertySchema) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        schema.encode(&mut writer);
        writer.into_bytes()
    }

    fn schema_decoded(bytes: &[u8]) -> Result<PropertySchema, DecodeError> {
        let mut reader = ByteReader::new(bytes);
        let schema = PropertySchema::decode(&mut reader)?;
        reader.finish()?;
        Ok(schema)
    }

    fn id(text: &str) -> NamespacedId {
        text.parse().unwrap()
    }

    /// A definition that uses every part of the form.
    fn everything() -> BlockDefinition {
        BlockDefinition::cube([0.25, 0.5, 1.0])
            .with_shapes(vec![
                Shape::full(),
                Shape::half(FaceDirection::NegativeY),
                Shape::stair(FaceDirection::PositiveX),
                Shape::empty(),
            ])
            .with_see_through(true)
            .with_state_colors(vec![[0.1, 0.2, 0.3], [0.4, 0.5, 0.6]])
            .with_property(&DENSITY, 7_000)
            .with_state_property(2, &DENSITY, 12)
            .with_state_property(1, &PLACEABLE_ON_MOVING_GRIDS, false)
    }

    #[test]
    fn definitions_survive_the_trip_exactly() {
        for definition in [
            BlockDefinition::air(),
            BlockDefinition::cube([0.0; 3]),
            BlockDefinition::cube([1.0, 0.0, 0.5]).with_property(&DENSITY, 1),
            everything(),
        ] {
            assert_eq!(decoded(&encoded(&definition)), Ok(definition));
        }
    }

    #[test]
    fn the_engines_schema_and_a_schema_with_more_survive_exactly() {
        let mut schema = PropertySchema::new();
        schema
            .declare(PropertyDeclaration {
                id: id("test:range"),
                kind: PropertyKind::Integer {
                    minimum: -5,
                    maximum: 300,
                },
                default: -2,
                doc: "a number with a range".to_owned(),
            })
            .unwrap();
        for schema in [PropertySchema::new(), PropertySchema::empty(), schema] {
            assert_eq!(schema_decoded(&schema_bytes(&schema)), Ok(schema));
        }
    }

    #[test]
    fn the_same_definition_is_the_same_bytes_however_its_properties_were_set() {
        let one = BlockDefinition::cube([0.5; 3])
            .with_property(&DENSITY, 3)
            .with_property(&PLACEABLE_ON_MOVING_GRIDS, false);
        let other = BlockDefinition::cube([0.5; 3])
            .with_property(&PLACEABLE_ON_MOVING_GRIDS, false)
            .with_property(&DENSITY, 3);
        assert_eq!(encoded(&one), encoded(&other));
    }

    #[test]
    fn a_definition_cut_short_anywhere_or_followed_by_more_is_an_error() {
        let bytes = encoded(&everything());
        for length in 0..bytes.len() {
            assert!(decoded(&bytes[..length]).is_err(), "cut at {length}");
        }
        let mut longer = bytes;
        longer.push(0);
        assert_eq!(decoded(&longer), Err(DecodeError::TrailingBytes(1)));
    }

    #[test]
    fn a_schema_cut_short_anywhere_or_followed_by_more_is_an_error() {
        let bytes = schema_bytes(&PropertySchema::new());
        for length in 0..bytes.len() {
            assert!(schema_decoded(&bytes[..length]).is_err(), "cut at {length}");
        }
        let mut longer = bytes;
        longer.push(0);
        assert_eq!(schema_decoded(&longer), Err(DecodeError::TrailingBytes(1)));
    }

    /// The bytes of a definition written by hand: one state with these boxes and no state colours, then
    /// `properties` as they are written.
    fn handmade(color: [f32; 3], see_through: u8, boxes: &[[u8; 6]], properties: &[u8]) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        for component in color {
            writer.write_u32(component.to_bits());
        }
        writer.write_u8(see_through);
        writer.write_varint(1);
        writer.write_varint(boxes.len() as u64);
        for corners in boxes {
            for corner in corners {
                writer.write_u8(*corner);
            }
        }
        writer.write_varint(0);
        let mut bytes = writer.into_bytes();
        bytes.extend_from_slice(properties);
        bytes
    }

    const NO_PROPERTIES: &[u8] = &[0];
    const WHOLE: [u8; 6] = [0, 0, 0, 8, 8, 8];

    fn refusal(bytes: &[u8]) -> String {
        match decoded(bytes) {
            Err(error) => error.to_string(),
            Ok(definition) => panic!("accepted {definition:?}"),
        }
    }

    #[test]
    fn the_handmade_bytes_with_nothing_wrong_are_accepted() {
        // The control for the refusals below.
        let bytes = handmade([0.5; 3], 0, &[WHOLE], NO_PROPERTIES);
        assert_eq!(decoded(&bytes), Ok(BlockDefinition::cube([0.5; 3])));
    }

    #[test]
    fn a_definition_that_could_not_have_been_written_is_refused_with_the_reason() {
        let colour = "a colour is not a finite number";
        let shape = "a box is empty or leaves the voxel";
        let cases = [
            (
                handmade([f32::NAN, 0.0, 0.0], 0, &[WHOLE], NO_PROPERTIES),
                colour,
            ),
            (
                handmade([0.0, f32::INFINITY, 0.0], 0, &[WHOLE], NO_PROPERTIES),
                colour,
            ),
            (
                handmade([0.0; 3], 2, &[WHOLE], NO_PROPERTIES),
                "see-through is neither 0 nor 1",
            ),
            (
                handmade([0.0; 3], 0, &[[0, 0, 0, 0, 8, 8]], NO_PROPERTIES),
                shape,
            ),
            (
                handmade([0.0; 3], 0, &[[4, 0, 0, 3, 8, 8]], NO_PROPERTIES),
                shape,
            ),
            (
                handmade([0.0; 3], 0, &[[0, 0, 0, 8, 9, 8]], NO_PROPERTIES),
                shape,
            ),
        ];
        for (bytes, reason) in cases {
            assert_eq!(refusal(&bytes), reason);
        }
    }

    #[test]
    fn a_block_with_no_states_is_refused() {
        let mut writer = ByteWriter::new();
        for _ in 0..3 {
            writer.write_u32(0f32.to_bits());
        }
        writer.write_u8(0);
        writer.write_varint(0);
        writer.write_varint(0);
        writer.write_varint(0);
        assert_eq!(refusal(&writer.into_bytes()), "a block has no states");
    }

    /// Two properties set on a block, by the identifiers given, written by hand.
    fn two_properties(first: &str, second: &str) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.write_varint(2);
        for key in [first, second] {
            writer.write_identifier(&id(key));
            writer.write_u8(1);
            writer.write_signed_varint(5);
            writer.write_varint(0);
        }
        handmade([0.0; 3], 0, &[WHOLE], &writer.into_bytes())
    }

    #[test]
    fn properties_must_come_once_each_and_in_order() {
        assert!(decoded(&two_properties("a:one", "a:two")).is_ok());
        for (first, second) in [("a:two", "a:one"), ("a:one", "a:one")] {
            assert_eq!(
                refusal(&two_properties(first, second)),
                "identifiers are repeated or out of order"
            );
        }
    }

    #[test]
    fn a_flag_that_is_neither_0_nor_1_is_refused() {
        let mut writer = ByteWriter::new();
        writer.write_varint(1);
        writer.write_identifier(&id("a:one"));
        writer.write_u8(2);
        let bytes = handmade([0.0; 3], 0, &[WHOLE], &writer.into_bytes());
        assert_eq!(refusal(&bytes), "a property's flag is neither 0 nor 1");
    }

    #[test]
    fn states_of_a_property_must_come_once_each_and_in_order() {
        for (first, second, fine) in [(1, 2, true), (2, 1, false), (2, 2, false)] {
            let mut writer = ByteWriter::new();
            writer.write_varint(1);
            writer.write_identifier(&id("a:one"));
            writer.write_u8(0);
            writer.write_varint(2);
            for state in [first, second] {
                writer.write_varint(state);
                writer.write_signed_varint(1);
            }
            let bytes = handmade([0.0; 3], 0, &[WHOLE], &writer.into_bytes());
            if fine {
                assert!(decoded(&bytes).is_ok());
            } else {
                assert_eq!(refusal(&bytes), "states are repeated or out of order");
            }
        }
    }

    /// One declaration written by hand: the kind's bytes, the default.
    fn declaration_bytes(kind: &[u8], default: i64) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.write_varint(1);
        writer.write_identifier(&id("a:one"));
        writer.write_raw(kind);
        writer.write_signed_varint(default);
        writer.write_string("");
        writer.into_bytes()
    }

    fn integer_kind(minimum: i64, maximum: i64) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.write_u8(KIND_INTEGER);
        writer.write_signed_varint(minimum);
        writer.write_signed_varint(maximum);
        writer.into_bytes()
    }

    #[test]
    fn a_schema_that_could_not_have_been_written_is_refused_with_the_reason() {
        let default = "a property's default is outside its range";
        let cases = [
            (
                declaration_bytes(&[7], 0),
                "a property has a kind that is not known",
            ),
            (
                declaration_bytes(&integer_kind(5, 4), 5),
                "a property's range is empty",
            ),
            (declaration_bytes(&integer_kind(0, 10), 11), default),
            (declaration_bytes(&integer_kind(0, 10), -1), default),
            (declaration_bytes(&[KIND_BOOL], 2), default),
        ];
        for (bytes, reason) in cases {
            assert_eq!(schema_decoded(&bytes).unwrap_err().to_string(), reason);
        }
        // The controls: the same bytes with the default inside the range are accepted.
        assert!(schema_decoded(&declaration_bytes(&integer_kind(0, 10), 10)).is_ok());
        assert!(schema_decoded(&declaration_bytes(&[KIND_BOOL], 1)).is_ok());
    }

    #[test]
    fn declarations_must_come_once_each_and_in_order() {
        for (first, second, fine) in [
            ("a:one", "a:two", true),
            ("a:two", "a:one", false),
            ("a:one", "a:one", false),
        ] {
            let mut writer = ByteWriter::new();
            writer.write_varint(2);
            for name in [first, second] {
                writer.write_identifier(&id(name));
                writer.write_u8(KIND_BOOL);
                writer.write_signed_varint(0);
                writer.write_string("");
            }
            let result = schema_decoded(&writer.into_bytes());
            if fine {
                assert!(result.is_ok());
            } else {
                assert_eq!(
                    result.unwrap_err().to_string(),
                    "identifiers are repeated or out of order"
                );
            }
        }
    }

    fn arbitrary_box() -> impl Strategy<Value = ShapeBox> {
        (0u8..8, 0u8..8, 0u8..8).prop_flat_map(|(x, y, z)| {
            (x + 1..=8, y + 1..=8, z + 1..=8).prop_map(move |(width, height, depth)| ShapeBox {
                min: [x, y, z],
                max: [width, height, depth],
            })
        })
    }

    fn arbitrary_definition() -> impl Strategy<Value = BlockDefinition> {
        let color = || [0.0f32..=1.0, 0.0f32..=1.0, 0.0f32..=1.0];
        (
            color(),
            any::<bool>(),
            prop::collection::vec(prop::collection::vec(arbitrary_box(), 0..4), 1..5),
            prop::collection::vec(color(), 0..4),
            prop::option::of(any::<u32>()),
            prop::collection::vec((0u32..6, any::<bool>()), 0..4),
        )
            .prop_map(
                |(color, see_through, shapes, state_colors, density, by_state)| {
                    let mut definition = BlockDefinition::cube(color)
                        .with_shapes(
                            shapes
                                .iter()
                                .map(|boxes| Shape::from_boxes(boxes))
                                .collect(),
                        )
                        .with_see_through(see_through);
                    if !state_colors.is_empty() {
                        definition = definition.with_state_colors(state_colors);
                    }
                    if let Some(density) = density {
                        definition = definition.with_property(&DENSITY, density);
                    }
                    for (state, value) in by_state {
                        definition = definition.with_state_property(
                            state,
                            &PLACEABLE_ON_MOVING_GRIDS,
                            value,
                        );
                    }
                    definition
                },
            )
    }

    proptest! {
        #[test]
        fn any_definition_survives_the_trip_and_has_one_spelling(definition in arbitrary_definition()) {
            let bytes = encoded(&definition);
            let back = decoded(&bytes).unwrap();
            prop_assert_eq!(&back, &definition);
            prop_assert_eq!(encoded(&back), bytes);
        }

        #[test]
        fn bytes_that_are_not_a_definition_are_an_error_never_a_panic_and_those_that_are_have_one_spelling(
            bytes in prop::collection::vec(any::<u8>(), 0..80)
        ) {
            if let Ok(definition) = decoded(&bytes) {
                prop_assert_eq!(encoded(&definition), bytes);
            }
        }

        #[test]
        fn bytes_that_are_not_a_schema_are_an_error_never_a_panic_and_those_that_are_have_one_spelling(
            bytes in prop::collection::vec(any::<u8>(), 0..80)
        ) {
            if let Ok(schema) = schema_decoded(&bytes) {
                prop_assert_eq!(schema_bytes(&schema), bytes);
            }
        }
    }
}

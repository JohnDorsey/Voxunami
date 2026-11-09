//! A block file: one block of a pack, named by the file.

use block_models::{
    BlockDefinition, PropertyDeclaration, PropertyKind, PropertySchema, Shape, Transparency, engine,
};
use namespaced_id::NamespacedId;
use serde_json::Value;

use crate::error::PackErrors;
use crate::face_colors::read_face_colors;
use crate::shape::read_shape;
use crate::walk::{Path, Walk, nearest};

const KEYS: [&str; 10] = [
    "color",
    "sight_through",
    "light_through",
    "transparency",
    "states",
    "state_colors",
    "face_colors",
    "texture",
    "properties",
    "state_properties",
];

/// Why a block with a transparency mode must be sight-through: the mode is what shows through the holes of its pictures and what is drawn of its faces.
pub(crate) const MODE_NEEDS_SIGHT: &str =
    "a transparency mode needs \"sight_through\": true, or nothing shows through the block";

/// The words of the transparency modes, as an author writes them.
pub(crate) fn transparency_words() -> Vec<&'static str> {
    Transparency::ALL.iter().map(|mode| mode.word()).collect()
}

/// A transparency mode written as a word.
pub(crate) fn read_transparency(
    walk: &mut Walk,
    value: &Value,
    path: &Path,
) -> Option<Transparency> {
    let word = walk.string(value, path)?;
    match Transparency::from_word(word) {
        Some(mode) => Some(mode),
        None => {
            let words = transparency_words();
            let hint = nearest(word, &words)
                .map(|near| format!(" (did you mean \"{near}\"?)"))
                .unwrap_or_default();
            walk.fail(
                path,
                format!(
                    "\"{word}\" is not a transparency mode{hint}; the modes are {}",
                    crate::walk::quoted(&words)
                ),
            );
            None
        }
    }
}

/// The block a file describes: its identifier is the pack's name and the file's, such as `my_pack:marble` for
/// `my_pack/blocks/marble.json`.
///
/// `file` is how the author knows the file (for messages), `schema` says which properties exist.
pub fn read_block(
    file: &str,
    pack: &str,
    name: &str,
    text: &str,
    schema: &PropertySchema,
) -> Result<(NamespacedId, BlockDefinition), PackErrors> {
    let (mut walk, value) = Walk::parse(file, text)?;
    let root = Path::root();
    let identifier = block_identifier(&mut walk, pack, name, &root);
    let definition = read_definition(&mut walk, &value, &root, pack, schema);
    match (identifier, definition) {
        (Some(identifier), Some(definition)) => walk.finish((identifier, definition)),
        _ => Err(walk
            .finish(())
            .expect_err("a problem was found, or both parts would be there")),
    }
}

/// `pack:name`, if both are good names.
pub(crate) fn block_identifier(
    walk: &mut Walk,
    pack: &str,
    name: &str,
    path: &Path,
) -> Option<NamespacedId> {
    match NamespacedId::new(pack, name) {
        Ok(identifier) => Some(identifier),
        Err(error) => {
            walk.fail(
                path,
                format!("the file name \"{name}\" is not a block name: {error}"),
            );
            None
        }
    }
}

fn read_definition(
    walk: &mut Walk,
    value: &Value,
    path: &Path,
    pack: &str,
    schema: &PropertySchema,
) -> Option<BlockDefinition> {
    let object = walk.object(value, path, &KEYS)?;
    let texture = match object.get("texture") {
        Some(texture) => read_texture(walk, texture, &path.key("texture"), Some(pack)).map(Some),
        None => Some(None),
    };
    // A block with a texture may leave its colour out: it is then the average of its pictures, which the loader works out once it knows them.
    let color = match object.get("color") {
        Some(color) => walk
            .color(color, &path.key("color"))
            .map(|color| (color, false)),
        None if object.contains_key("texture") => Some(([0.0; 3], true)),
        None => {
            walk.fail(path, "missing \"color\"");
            None
        }
    };
    let sight_through = match object.get("sight_through") {
        Some(value) => walk.boolean(value, &path.key("sight_through")),
        None => Some(false),
    };
    let light_through = match object.get("light_through") {
        Some(value) => walk.boolean(value, &path.key("light_through")),
        None => Some(false),
    };
    let transparency = match object.get("transparency") {
        Some(value) => read_transparency(walk, value, &path.key("transparency")),
        None => Some(Transparency::Opaque),
    };
    let shapes = match object.get("states") {
        Some(states) => read_states(walk, states, &path.key("states")),
        None => Some(vec![Shape::full()]),
    };
    let state_colors = match object.get("state_colors") {
        Some(colors) => read_state_colors(walk, colors, &path.key("state_colors")),
        None => Some(Vec::new()),
    };
    let face_colors = match object.get("face_colors") {
        Some(colors) => read_face_colors(walk, colors, &path.key("face_colors")),
        None => Some([None; 6]),
    };
    let properties = match object.get("properties") {
        Some(properties) => read_properties(walk, properties, &path.key("properties"), schema),
        None => Some(Vec::new()),
    };
    let state_count = shapes.as_ref().map_or(1, Vec::len);
    let state_properties = match object.get("state_properties") {
        Some(by_state) => read_state_properties(
            walk,
            by_state,
            &path.key("state_properties"),
            schema,
            state_count,
        ),
        None => Some(Vec::new()),
    };
    let (
        (color, color_from_pictures),
        texture,
        sight_through,
        light_through,
        transparency,
        shapes,
        state_colors,
        face_colors,
        properties,
        state_properties,
    ) = (
        color?,
        texture?,
        sight_through?,
        light_through?,
        transparency?,
        shapes?,
        state_colors?,
        face_colors?,
        properties?,
        state_properties?,
    );

    if transparency != Transparency::Opaque && !sight_through {
        walk.fail(&path.key("transparency"), MODE_NEEDS_SIGHT);
        return None;
    }
    let mut definition = BlockDefinition::cube(color)
        .with_shapes(shapes)
        .with_sight_through(sight_through)
        .with_light_through(light_through)
        .with_transparency(transparency);
    if let Some(texture) = texture {
        definition = definition.with_texture(texture);
        if color_from_pictures {
            definition = definition.with_color_from_pictures();
        }
    }
    if !state_colors.is_empty() {
        definition = definition.with_state_colors(state_colors);
    }
    definition.set_face_colors(face_colors);
    for (key, value) in properties {
        definition = definition.with_property_value(key, value);
    }
    for (state, key, value) in state_properties {
        definition = definition.with_state_property_value(state, key, value);
    }
    Some(definition)
}

/// The base name of a block's pictures: `"oak"` is `pack:oak` (a block's file says its own pack's), and `"other_pack:stone"` is a picture of another. In a patch
/// (`pack` is `None`) a name has its namespace, since a patch is not of one pack.
pub(crate) fn read_texture(
    walk: &mut Walk,
    value: &Value,
    path: &Path,
    pack: Option<&str>,
) -> Option<NamespacedId> {
    let text = walk.string(value, path)?;
    let parsed = match (pack, text.contains(':')) {
        (Some(pack), false) => NamespacedId::new(pack, text),
        (None, false) => {
            walk.fail(
                path,
                format!(
                    "a texture in a patch is written with its namespace, such as \"my_pack:oak\", and \"{text}\" has none"
                ),
            );
            return None;
        }
        (_, true) => text.parse(),
    };
    match parsed {
        Ok(texture) => Some(texture),
        Err(error) => {
            walk.fail(
                path,
                format!("\"{text}\" is not the name of a picture: {error}"),
            );
            None
        }
    }
}

fn read_states(walk: &mut Walk, value: &Value, path: &Path) -> Option<Vec<Shape>> {
    let list = walk.array(value, path)?;
    if list.is_empty() {
        walk.fail(path, "a block has at least one state");
        return None;
    }
    let shapes: Vec<Option<Shape>> = list
        .iter()
        .enumerate()
        .map(|(index, entry)| read_shape(walk, entry, &path.index(index)))
        .collect();
    shapes.into_iter().collect()
}

fn read_state_colors(walk: &mut Walk, value: &Value, path: &Path) -> Option<Vec<[f32; 3]>> {
    let list = walk.array(value, path)?;
    if list.is_empty() {
        walk.fail(
            path,
            "give at least one colour, or leave \"state_colors\" out",
        );
        return None;
    }
    let colors: Vec<Option<[f32; 3]>> = list
        .iter()
        .enumerate()
        .map(|(index, entry)| walk.color(entry, &path.index(index)))
        .collect();
    colors.into_iter().collect()
}

/// A property's value, as the number it is stored as, checked against what the declaration allows.
pub(crate) fn read_property_value(
    walk: &mut Walk,
    value: &Value,
    path: &Path,
    declaration: &PropertyDeclaration,
) -> Option<i64> {
    match declaration.kind {
        PropertyKind::Bool => walk.boolean(value, path).map(i64::from),
        PropertyKind::Integer { minimum, maximum } => walk.integer(value, path, minimum, maximum),
        PropertyKind::Face => match value {
            Value::String(word) => face_code(walk, word, path),
            Value::Number(_) => walk.integer(value, path, 0, 6),
            other => {
                walk.fail(
                    path,
                    format!("{}, found {}", expected_face(), crate::walk::kind_of(other)),
                );
                None
            }
        },
    }
}

/// What a face is, for a complaint.
pub(crate) fn expected_face() -> String {
    format!(
        "expected a face ({}) or a number from 0 to 6",
        crate::walk::quoted(&engine::FACE_WORDS)
    )
}

/// The code of a face written as a word, or a complaint that says what a face is.
pub(crate) fn face_code(walk: &mut Walk, word: &str, path: &Path) -> Option<i64> {
    let code = engine::face_code_from_word(word).map(i64::from);
    if code.is_none() {
        walk.fail(path, no_such_face(word));
    }
    code
}

/// The complaint for a word that is no face.
pub(crate) fn no_such_face(word: &str) -> String {
    let hint = nearest(word, &engine::FACE_WORDS)
        .map(|near| format!(" (did you mean \"{near}\"?)"))
        .unwrap_or_default();
    format!("{}, found \"{word}\"{hint}", expected_face())
}

/// The declaration of a property named by text, or a complaint that says what exists.
pub(crate) fn find_declaration<'a>(
    walk: &mut Walk,
    name: &str,
    path: &Path,
    schema: &'a PropertySchema,
) -> Option<&'a PropertyDeclaration> {
    let declared: Vec<&str> = schema
        .iter()
        .map(|declaration| declaration.id.as_str())
        .collect();
    let identifier: NamespacedId = match name.parse() {
        Ok(identifier) => identifier,
        Err(error) => {
            walk.fail(path, format!("\"{name}\" is not a property name: {error}"));
            return None;
        }
    };
    let found = schema.get(&identifier);
    if found.is_none() {
        let hint = nearest(name, &declared)
            .map(|near| format!(" (did you mean \"{near}\"?)"))
            .unwrap_or_default();
        walk.fail(
            path,
            format!(
                "the property \"{name}\" is not declared{hint}; the properties are {}",
                crate::walk::quoted(&declared)
            ),
        );
    }
    found
}

fn read_properties(
    walk: &mut Walk,
    value: &Value,
    path: &Path,
    schema: &PropertySchema,
) -> Option<Vec<(NamespacedId, i64)>> {
    let object = value.as_object().or_else(|| {
        walk.fail(
            path,
            format!(
                "expected an object of properties, found {}",
                crate::walk::kind_of(value)
            ),
        );
        None
    })?;
    let mut properties = Vec::with_capacity(object.len());
    let mut good = true;
    for (name, entry) in object {
        let entry_path = path.key(name);
        let declaration = find_declaration(walk, name, &entry_path, schema);
        let number = declaration.and_then(|declaration| {
            read_property_value(walk, entry, &entry_path, declaration)
                .map(|number| (declaration.id.clone(), number))
        });
        match number {
            Some(pair) => properties.push(pair),
            None => good = false,
        }
    }
    good.then_some(properties)
}

fn read_state_properties(
    walk: &mut Walk,
    value: &Value,
    path: &Path,
    schema: &PropertySchema,
    state_count: usize,
) -> Option<Vec<(u32, NamespacedId, i64)>> {
    let object = value.as_object().or_else(|| {
        walk.fail(
            path,
            format!(
                "expected an object of states, found {}",
                crate::walk::kind_of(value)
            ),
        );
        None
    })?;
    let mut found = Vec::new();
    let mut good = true;
    for (state_text, properties) in object {
        let state_path = path.key(state_text);
        let Some(state) = state_text
            .parse::<u32>()
            .ok()
            .filter(|state| (*state as usize) < state_count)
        else {
            walk.fail(
                &state_path,
                format!(
                    "\"{state_text}\" is not a state of this block, which has {state_count} (0 to {})",
                    state_count - 1
                ),
            );
            good = false;
            continue;
        };
        match read_properties(walk, properties, &state_path, schema) {
            Some(list) => found.extend(list.into_iter().map(|(key, value)| (state, key, value))),
            None => good = false,
        }
    }
    good.then_some(found)
}

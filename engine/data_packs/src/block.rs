//! A block file: one block of a pack, named by the file.

use block_models::{BlockDefinition, PropertyDeclaration, PropertyKind, PropertySchema, Shape};
use namespaced_id::NamespacedId;
use serde_json::Value;

use crate::error::PackErrors;
use crate::shape::read_shape;
use crate::walk::{Path, Walk, nearest};

const KEYS: [&str; 6] = [
    "color",
    "see_through",
    "states",
    "state_colors",
    "properties",
    "state_properties",
];

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
    let definition = read_definition(&mut walk, &value, &root, schema);
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
    schema: &PropertySchema,
) -> Option<BlockDefinition> {
    let object = walk.object(value, path, &KEYS)?;
    let color = walk
        .require(object, "color", path)
        .and_then(|color| walk.color(color, &path.key("color")));
    let see_through = match object.get("see_through") {
        Some(value) => walk.boolean(value, &path.key("see_through")),
        None => Some(false),
    };
    let shapes = match object.get("states") {
        Some(states) => read_states(walk, states, &path.key("states")),
        None => Some(vec![Shape::full()]),
    };
    let state_colors = match object.get("state_colors") {
        Some(colors) => read_state_colors(walk, colors, &path.key("state_colors")),
        None => Some(Vec::new()),
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
    let (color, see_through, shapes, state_colors, properties, state_properties) = (
        color?,
        see_through?,
        shapes?,
        state_colors?,
        properties?,
        state_properties?,
    );

    let mut definition = BlockDefinition::cube(color)
        .with_shapes(shapes)
        .with_see_through(see_through);
    if !state_colors.is_empty() {
        definition = definition.with_state_colors(state_colors);
    }
    for (key, value) in properties {
        definition = definition.with_property_value(key, value);
    }
    for (state, key, value) in state_properties {
        definition = definition.with_state_property_value(state, key, value);
    }
    Some(definition)
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
    }
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

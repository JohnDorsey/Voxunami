//! A patch file: changes to a block that already exists, as a short list of operations.
//!
//! This reads the file's shape. Whether the target exists, whether a property is declared and whether a `test`
//! holds are for the loader, which knows the blocks.

use block_models::Shape;
use namespaced_id::NamespacedId;
use serde_json::{Map, Value};

use crate::error::PackErrors;
use crate::shape::read_shape;
use crate::walk::{Path, Walk, nearest, quoted};

const OPERATIONS: [&str; 4] = ["test", "set", "remove", "add"];
const FIELDS: [&str; 3] = ["color", "see_through", "state_colors"];

/// A property's value as the file wrote it; the property's declaration says whether it is the right kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyValue {
    Bool(bool),
    Integer(i64),
}

/// What an operation looks at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// A property, in every state or in one state.
    Property {
        key: NamespacedId,
        state: Option<u32>,
    },
    Color,
    SeeThrough,
    StateColors,
}

/// What an operation puts or expects.
#[derive(Clone, Debug, PartialEq)]
pub enum PatchValue {
    Property(PropertyValue),
    Color([f32; 3]),
    Flag(bool),
    Colors(Vec<[f32; 3]>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Operation {
    /// The patch is refused unless the place holds this.
    Test {
        place: Place,
        value: PatchValue,
    },
    Set {
        place: Place,
        value: PatchValue,
    },
    /// A property is back to the default, or the state colours are gone.
    Remove {
        place: Place,
    },
    /// A new state, after the last.
    AddState {
        shape: Box<Shape>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    pub target: NamespacedId,
    /// Skip the patch (and say so) if its target or a property it names is not there.
    pub optional: bool,
    pub operations: Vec<Operation>,
}

pub fn read_patch(file: &str, text: &str) -> Result<Patch, PackErrors> {
    let (mut walk, value) = Walk::parse(file, text)?;
    let patch = read(&mut walk, &value, &Path::root());
    match patch {
        Some(patch) => walk.finish(patch),
        None => Err(walk
            .finish(())
            .expect_err("a patch that was not read had a problem")),
    }
}

fn read(walk: &mut Walk, value: &Value, path: &Path) -> Option<Patch> {
    let object = walk.object(value, path, &["target", "optional", "operations"])?;
    let target_path = path.key("target");
    let target = walk
        .require(object, "target", path)
        .and_then(|target| walk.string(target, &target_path))
        .and_then(|text| match text.parse::<NamespacedId>() {
            Ok(target) => Some(target),
            Err(error) => {
                walk.fail(
                    &target_path,
                    format!("\"{text}\" is not a block name: {error}"),
                );
                None
            }
        });
    let optional = match object.get("optional") {
        Some(optional) => walk.boolean(optional, &path.key("optional")),
        None => Some(false),
    };
    let operations_path = path.key("operations");
    let operations = walk
        .require(object, "operations", path)
        .and_then(|list| walk.array(list, &operations_path))
        .and_then(|list| {
            if list.is_empty() {
                walk.fail(&operations_path, "a patch has at least one operation");
                return None;
            }
            let operations: Vec<Option<Operation>> = list
                .iter()
                .enumerate()
                .map(|(index, entry)| read_operation(walk, entry, &operations_path.index(index)))
                .collect();
            operations.into_iter().collect::<Option<Vec<_>>>()
        });
    Some(Patch {
        target: target?,
        optional: optional?,
        operations: operations?,
    })
}

fn read_operation(walk: &mut Walk, value: &Value, path: &Path) -> Option<Operation> {
    let object = value.as_object().or_else(|| {
        walk.fail(
            path,
            format!("expected an object, found {}", crate::walk::kind_of(value)),
        );
        None
    })?;
    let op_path = path.key("op");
    let name = walk
        .require(object, "op", path)
        .and_then(|op| walk.string(op, &op_path))?;
    let allowed: &[&str] = match name {
        "test" | "set" => &["op", "property", "path", "state", "value"],
        "remove" => &["op", "property", "path", "state"],
        "add" => &["op", "shape"],
        _ => {
            let hint = nearest(name, &OPERATIONS)
                .map(|near| format!(" (did you mean \"{near}\"?)"))
                .unwrap_or_default();
            walk.fail(
                &op_path,
                format!(
                    "\"{name}\" is not an operation{hint}; the operations are {}",
                    quoted(&OPERATIONS)
                ),
            );
            return None;
        }
    };
    walk.object(value, path, allowed)?;
    if name == "add" {
        let shape_path = path.key("shape");
        let shape = walk
            .require(object, "shape", path)
            .and_then(|shape| read_shape(walk, shape, &shape_path))?;
        return Some(Operation::AddState {
            shape: Box::new(shape),
        });
    }
    let place = read_place(walk, object, path)?;
    match name {
        "remove" => match place {
            Place::Color | Place::SeeThrough => {
                walk.fail(
                    &path.key("path"),
                    "a block always has its colour and its see-through flag, so they cannot be removed (use \"set\")",
                );
                None
            }
            place => Some(Operation::Remove { place }),
        },
        _ => {
            let value_path = path.key("value");
            let value = walk
                .require(object, "value", path)
                .and_then(|value| read_value(walk, value, &value_path, &place))?;
            Some(if name == "set" {
                Operation::Set { place, value }
            } else {
                Operation::Test { place, value }
            })
        }
    }
}

/// What the operation is about: `"property": "a:b"` (with an optional `"state": n`), or `"path": "color"`.
fn read_place(walk: &mut Walk, object: &Map<String, Value>, path: &Path) -> Option<Place> {
    match (object.get("property"), object.get("path")) {
        (Some(_), Some(_)) => {
            walk.fail(path, "give \"property\" or \"path\", not both");
            None
        }
        (None, None) => {
            walk.fail(path, "say what to change with \"property\" or \"path\"");
            None
        }
        (Some(property), None) => {
            let property_path = path.key("property");
            let text = walk.string(property, &property_path)?;
            let key = match text.parse::<NamespacedId>() {
                Ok(key) => key,
                Err(error) => {
                    walk.fail(
                        &property_path,
                        format!("\"{text}\" is not a property name: {error}"),
                    );
                    return None;
                }
            };
            let state = match object.get("state") {
                None => None,
                Some(state) => {
                    Some(walk.integer(state, &path.key("state"), 0, i64::from(u32::MAX))? as u32)
                }
            };
            Some(Place::Property { key, state })
        }
        (None, Some(field)) => {
            let field_path = path.key("path");
            let text = walk.string(field, &field_path)?;
            if object.contains_key("state") {
                walk.fail(
                    &path.key("state"),
                    "\"state\" goes with \"property\" (a field belongs to the whole block)",
                );
                return None;
            }
            match text {
                "color" => Some(Place::Color),
                "see_through" => Some(Place::SeeThrough),
                "state_colors" => Some(Place::StateColors),
                other => {
                    let hint = nearest(other, &FIELDS)
                        .map(|near| format!(" (did you mean \"{near}\"?)"))
                        .unwrap_or_default();
                    walk.fail(
                        &field_path,
                        format!(
                            "\"{other}\" is not a field of a block{hint}; the fields are {}",
                            quoted(&FIELDS)
                        ),
                    );
                    None
                }
            }
        }
    }
}

fn read_value(walk: &mut Walk, value: &Value, path: &Path, place: &Place) -> Option<PatchValue> {
    match place {
        Place::Property { .. } => match value {
            Value::Bool(flag) => Some(PatchValue::Property(PropertyValue::Bool(*flag))),
            Value::Number(_) => walk
                .integer(value, path, i64::MIN, i64::MAX)
                .map(|number| PatchValue::Property(PropertyValue::Integer(number))),
            other => {
                walk.fail(
                    path,
                    format!(
                        "a property is true or false or a whole number, found {}",
                        crate::walk::kind_of(other)
                    ),
                );
                None
            }
        },
        Place::Color => walk.color(value, path).map(PatchValue::Color),
        Place::SeeThrough => walk.boolean(value, path).map(PatchValue::Flag),
        Place::StateColors => {
            let list = walk.array(value, path)?;
            if list.is_empty() {
                walk.fail(
                    path,
                    "give at least one colour (or \"remove\" the state colours)",
                );
                return None;
            }
            let colors: Vec<Option<[f32; 3]>> = list
                .iter()
                .enumerate()
                .map(|(index, entry)| walk.color(entry, &path.index(index)))
                .collect();
            colors
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .map(PatchValue::Colors)
        }
    }
}

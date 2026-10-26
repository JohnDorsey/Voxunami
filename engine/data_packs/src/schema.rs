//! The JSON Schemas of pack files, generated from what the game declares: the property keys and kinds, the shape
//! names and the sides. Nothing here is written out by hand for a particular game, so a schema cannot drift from
//! the properties a game has; editors that read a schema complete and check pack files as they are typed.
//!
//! A schema can say what shape a file has, not everything the loader checks: that a box is not empty, that a state
//! number is one the block has, that a block's name is free, that a dependency exists and is not a cycle, and that a
//! `test` holds are for the loader (`check-pack` says them all).

use block_models::{PropertyKind, PropertySchema};
use serde_json::{Value, json};

use crate::body::{BodyContext, GREATEST_EXTENT_BRICKS, POSITION_LIMIT};
use crate::shape::{FACE_NAMES, SHAPE_NAMES};

/// The schemas of the four kinds of file a pack has.
#[derive(Clone, Debug, PartialEq)]
pub struct PackSchemas {
    /// For `pack.json`.
    pub manifest: Value,
    /// For the files in `blocks/`.
    pub block: Value,
    /// For the files in `patches/`.
    pub patch: Value,
    /// For the files in `bodies/`.
    pub body: Value,
}

const DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";
/// A part of an identifier: see `namespaced_id`.
const PART: &str = "[a-z][a-z0-9_]{0,63}";

fn part_pattern() -> String {
    format!("^{PART}$")
}

fn identifier_pattern() -> String {
    format!("^{PART}:{PART}$")
}

fn color() -> Value {
    json!({
        "type": "array",
        "minItems": 3,
        "maxItems": 3,
        "items": { "type": "number", "minimum": 0, "maximum": 1 },
        "description": "red, green, blue, each from 0 to 1"
    })
}

fn shape() -> Value {
    let sides: Vec<&str> = FACE_NAMES.iter().map(|(name, _)| *name).collect();
    json!({
        "description": "a named shape, or boxes in eighths of a voxel",
        "oneOf": [
            { "enum": ["full", "empty"] },
            {
                "type": "string",
                "pattern": format!("^(half|stair):({})$", sides.join("|")),
                "description": format!("{} take a side", &SHAPE_NAMES[2..].join(" and "))
            },
            {
                "type": "object",
                "required": ["boxes"],
                "additionalProperties": false,
                "properties": {
                    "boxes": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "array",
                            "minItems": 6,
                            "maxItems": 6,
                            "items": { "type": "integer", "minimum": 0, "maximum": 8 },
                            "description": "min x, min y, min z, max x, max y, max z"
                        }
                    }
                }
            }
        ]
    })
}

/// The schema of one property's value in a block file.
fn property_value(declaration: &block_models::PropertyDeclaration) -> Value {
    let mut value = match declaration.kind {
        PropertyKind::Bool => json!({ "type": "boolean" }),
        PropertyKind::Integer { minimum, maximum } => {
            json!({ "type": "integer", "minimum": minimum, "maximum": maximum })
        }
    };
    let default = match declaration.kind {
        PropertyKind::Bool => (declaration.default != 0).to_string(),
        PropertyKind::Integer { .. } => declaration.default.to_string(),
    };
    value["description"] = json!(format!("{} (default {default})", declaration.doc));
    value
}

fn properties_object(schema: &PropertySchema) -> Value {
    let mut properties = serde_json::Map::new();
    for declaration in schema.iter() {
        properties.insert(declaration.id.to_string(), property_value(declaration));
    }
    json!({ "type": "object", "properties": properties, "additionalProperties": false })
}

fn manifest() -> Value {
    json!({
        "$schema": DIALECT,
        "title": "A Voxunami pack",
        "type": "object",
        "required": ["id"],
        "additionalProperties": false,
        "properties": {
            "$schema": { "type": "string" },
            "id": {
                "type": "string",
                "pattern": part_pattern(),
                "not": { "const": "engine" },
                "description": "the pack's name, also the namespace of the blocks it adds; the folder has the same name"
            },
            "depends_on": {
                "type": "array",
                "uniqueItems": true,
                "items": { "type": "string", "pattern": part_pattern(), "not": { "const": "engine" } },
                "description": "packs, or plugins that have blocks, that load before this one"
            }
        }
    })
}

fn block(schema: &PropertySchema) -> Value {
    json!({
        "$schema": DIALECT,
        "title": "A Voxunami block",
        "description": "one block of a pack; its name is the file's name",
        "type": "object",
        "required": ["color"],
        "additionalProperties": false,
        "$defs": { "color": color(), "shape": shape() },
        "properties": {
            "$schema": { "type": "string" },
            "color": { "$ref": "#/$defs/color" },
            "see_through": { "type": "boolean", "default": false },
            "states": {
                "type": "array",
                "minItems": 1,
                "items": { "$ref": "#/$defs/shape" },
                "description": "the shape of each state, state 0 first; one whole state if left out"
            },
            "state_colors": {
                "type": "array",
                "minItems": 1,
                "items": { "$ref": "#/$defs/color" },
                "description": "the colour each state is drawn in, repeating; every state has `color` if left out"
            },
            "properties": properties_object(schema),
            "state_properties": {
                "type": "object",
                "patternProperties": { "^[0-9]+$": properties_object(schema) },
                "additionalProperties": false,
                "description": "properties that differ in one state, by the state's number"
            }
        }
    })
}

fn body(context: &BodyContext<'_>) -> Value {
    let level = if context.levels.is_empty() {
        json!({ "type": "string", "pattern": identifier_pattern() })
    } else {
        let names: Vec<&str> = context.levels.iter().map(|level| level.as_str()).collect();
        json!({ "enum": names })
    };
    let family = if context.families.is_empty() {
        json!({ "type": "string", "pattern": identifier_pattern() })
    } else {
        let names: Vec<&str> = context
            .families
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        json!({ "enum": names })
    };
    // Each family says how its own parameters are written.
    let parameters: Vec<Value> = context
        .families
        .iter()
        .map(|(name, data)| {
            json!({
                "if": { "properties": { "family": { "const": name.as_str() } }, "required": ["family"] },
                "then": { "properties": { "parameters": data.schema() } }
            })
        })
        .collect();
    json!({
        "$schema": DIALECT,
        "title": "A Voxunami body",
        "description": "one body of a pack, a planet in a level of the world; its name is the file's name",
        "type": "object",
        "required": ["level", "terrain", "extent_bricks"],
        "additionalProperties": false,
        "oneOf": [{ "required": ["mass"] }, { "required": ["surface_gravity"] }],
        "properties": {
            "$schema": { "type": "string" },
            "level": {
                "description": "the level it is in, such as `my_game:space`",
                "$ref": "#/$defs/level"
            },
            "position": {
                "type": "array",
                "minItems": 3,
                "maxItems": 3,
                "items": { "type": "number", "minimum": -POSITION_LIMIT, "maximum": POSITION_LIMIT },
                "description": "where its centre is in the level, in voxels: x, y, z (the origin if left out)"
            },
            "terrain": {
                "type": "object",
                "required": ["family", "parameters"],
                "additionalProperties": false,
                "properties": {
                    "family": { "$ref": "#/$defs/family", "description": "the family of terrain generators that makes its ground" },
                    "parameters": { "type": "object", "description": "what the family takes" }
                },
                "allOf": parameters
            },
            "extent_bricks": {
                "type": "integer",
                "minimum": 1,
                "maximum": GREATEST_EXTENT_BRICKS,
                "description": "how many bricks (32 voxels) either way of its centre its ground reaches"
            },
            "mass": {
                "type": "number",
                "exclusiveMinimum": 0,
                "maximum": f64::from(f32::MAX),
                "description": "how heavy it is, in kilograms (give this or surface_gravity)"
            },
            "surface_gravity": {
                "type": "number",
                "exclusiveMinimum": 0,
                "description": "as heavy as it must be to pull this hard at its surface, in metres a second a second (give this or mass)"
            }
        },
        "$defs": { "level": level, "family": family }
    })
}

fn patch() -> Value {
    let identifier = json!({ "type": "string", "pattern": identifier_pattern() });
    let property_value = json!({ "type": ["boolean", "integer"] });
    let state = json!({ "type": "integer", "minimum": 0 });
    let operation = |properties: Value, required: &[&str]| {
        json!({
            "type": "object",
            "required": required,
            "additionalProperties": false,
            "properties": properties
        })
    };
    json!({
        "$schema": DIALECT,
        "title": "A Voxunami patch",
        "description": "changes to a block that already exists",
        "type": "object",
        "required": ["target", "operations"],
        "additionalProperties": false,
        "$defs": { "color": color(), "shape": shape() },
        "properties": {
            "$schema": { "type": "string" },
            "target": {
                "type": "string",
                "pattern": identifier_pattern(),
                "description": "the block to change, such as `base_blocks:lamp`"
            },
            "optional": {
                "type": "boolean",
                "default": false,
                "description": "skip the patch, and say so, if the block or a property it names is not there"
            },
            "operations": {
                "type": "array",
                "minItems": 1,
                "items": { "oneOf": [
                    operation(json!({
                        "op": { "enum": ["test", "set"] },
                        "property": identifier,
                        "state": state,
                        "value": property_value
                    }), &["op", "property", "value"]),
                    operation(json!({
                        "op": { "const": "remove" },
                        "property": identifier,
                        "state": state
                    }), &["op", "property"]),
                    operation(json!({
                        "op": { "enum": ["test", "set"] },
                        "path": { "const": "color" },
                        "value": { "$ref": "#/$defs/color" }
                    }), &["op", "path", "value"]),
                    operation(json!({
                        "op": { "enum": ["test", "set"] },
                        "path": { "const": "see_through" },
                        "value": { "type": "boolean" }
                    }), &["op", "path", "value"]),
                    operation(json!({
                        "op": { "enum": ["test", "set"] },
                        "path": { "const": "state_colors" },
                        "value": { "type": "array", "minItems": 1, "items": { "$ref": "#/$defs/color" } }
                    }), &["op", "path", "value"]),
                    operation(json!({
                        "op": { "const": "remove" },
                        "path": { "const": "state_colors" }
                    }), &["op", "path"]),
                    operation(json!({
                        "op": { "const": "add" },
                        "shape": { "$ref": "#/$defs/shape" }
                    }), &["op", "shape"]),
                ] }
            }
        }
    })
}

impl PackSchemas {
    /// The files to write for editors to read: each name with the schema's text.
    pub fn files(&self) -> [(&'static str, String); 4] {
        let text = |schema: &Value| {
            let mut text = serde_json::to_string_pretty(schema).expect("a schema is JSON");
            text.push('\n');
            text
        };
        [
            ("pack.schema.json", text(&self.manifest)),
            ("block.schema.json", text(&self.block)),
            ("patch.schema.json", text(&self.patch)),
            ("body.schema.json", text(&self.body)),
        ]
    }
}

/// The schemas for a game that declares these properties and has these families of generators and levels.
pub fn pack_schemas(schema: &PropertySchema, bodies: &BodyContext<'_>) -> PackSchemas {
    PackSchemas {
        manifest: manifest(),
        block: block(schema),
        patch: patch(),
        body: body(bodies),
    }
}

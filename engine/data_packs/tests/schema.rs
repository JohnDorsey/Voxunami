//! The generated schemas against the loader: they must agree about every file that is only a matter of shape, and the
//! files that are beyond what a schema can say are listed, with the reason, and the schema must accept them while the
//! loader refuses.

mod common;

use block_models::{PropertyDeclaration, PropertyKind, PropertySchema};
use data_packs::{
    BodyContext, PackSchemas, pack_schemas, read_block, read_body, read_manifest, read_patch,
};
use jsonschema::Validator;
use namespaced_id::NamespacedId;
use proptest::prelude::*;
use serde_json::{Value, json};

fn id(text: &str) -> NamespacedId {
    text.parse().unwrap()
}

fn properties() -> PropertySchema {
    let mut schema = PropertySchema::new();
    schema
        .declare(PropertyDeclaration {
            id: id("test:glow"),
            kind: PropertyKind::Integer {
                minimum: 0,
                maximum: 255,
            },
            default: 0,
            doc: "how much light".to_owned(),
        })
        .unwrap();
    schema
        .declare(PropertyDeclaration {
            id: id("test:sticky"),
            kind: PropertyKind::Bool,
            default: 0,
            doc: "whether it sticks".to_owned(),
        })
        .unwrap();
    schema
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Manifest,
    Block,
    Patch,
    Body,
}

struct Validators {
    manifest: Validator,
    block: Validator,
    patch: Validator,
    body: Validator,
    schema: PropertySchema,
}

fn validators() -> &'static Validators {
    static VALIDATORS: std::sync::OnceLock<Validators> = std::sync::OnceLock::new();
    VALIDATORS.get_or_init(build_validators)
}

fn build_validators() -> Validators {
    let schema = properties();
    let families = common::families();
    let levels = common::levels();
    let PackSchemas {
        manifest,
        block,
        patch,
        body,
    } = pack_schemas(
        &schema,
        &BodyContext {
            families: &families,
            levels: &levels,
        },
    );
    let build =
        |value: &Value| jsonschema::validator_for(value).expect("the schema is a valid schema");
    Validators {
        manifest: build(&manifest),
        block: build(&block),
        patch: build(&patch),
        body: build(&body),
        schema,
    }
}

impl Validators {
    fn schema_accepts(&self, kind: Kind, instance: &Value) -> bool {
        match kind {
            Kind::Manifest => self.manifest.is_valid(instance),
            Kind::Block => self.block.is_valid(instance),
            Kind::Patch => self.patch.is_valid(instance),
            Kind::Body => self.body.is_valid(instance),
        }
    }

    fn loader_accepts(&self, kind: Kind, text: &str) -> bool {
        match kind {
            Kind::Manifest => read_manifest("p/pack.json", text).is_ok(),
            Kind::Block => read_block("p/blocks/b.json", "p", "b", text, &self.schema).is_ok(),
            Kind::Patch => read_patch("p/patches/x.json", text).is_ok(),
            Kind::Body => {
                let families = common::families();
                let levels = common::levels();
                read_body(
                    "p/bodies/b.json",
                    "p",
                    "b",
                    text,
                    &BodyContext {
                        families: &families,
                        levels: &levels,
                    },
                )
                .is_ok()
            }
        }
    }
}

/// A file, what it is, and what each side should say about it.
struct Case {
    kind: Kind,
    why: &'static str,
    text: &'static str,
    /// `None`: the schema and the loader agree and `accepted` says which. `Some(reason)`: beyond what a schema says.
    beyond_the_schema: Option<&'static str>,
    accepted: bool,
}

const fn ok(kind: Kind, why: &'static str, text: &'static str) -> Case {
    Case {
        kind,
        why,
        text,
        beyond_the_schema: None,
        accepted: true,
    }
}

const fn bad(kind: Kind, why: &'static str, text: &'static str) -> Case {
    Case {
        kind,
        why,
        text,
        beyond_the_schema: None,
        accepted: false,
    }
}

const fn beyond(kind: Kind, why: &'static str, text: &'static str, reason: &'static str) -> Case {
    Case {
        kind,
        why,
        text,
        beyond_the_schema: Some(reason),
        accepted: false,
    }
}

use Kind::{Block, Body, Manifest, Patch};

fn corpus() -> Vec<Case> {
    vec![
        // ---- manifests ----
        ok(Manifest, "a pack with a name", r#"{ "id": "my_pack" }"#),
        ok(
            Manifest,
            "with needs",
            r#"{ "id": "my_pack", "depends_on": ["a", "b_2"] }"#,
        ),
        ok(
            Manifest,
            "naming its schema",
            r#"{ "$schema": "x.json", "id": "p" }"#,
        ),
        bad(Manifest, "no name", "{}"),
        bad(Manifest, "a name with capitals", r#"{ "id": "My_Pack" }"#),
        bad(
            Manifest,
            "a name that starts with a digit",
            r#"{ "id": "2pack" }"#,
        ),
        bad(Manifest, "a name with a colon", r#"{ "id": "a:b" }"#),
        bad(
            Manifest,
            "a name too long",
            r#"{ "id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }"#,
        ),
        bad(Manifest, "the engine's name", r#"{ "id": "engine" }"#),
        bad(
            Manifest,
            "needing the engine",
            r#"{ "id": "p", "depends_on": ["engine"] }"#,
        ),
        bad(
            Manifest,
            "a need twice",
            r#"{ "id": "p", "depends_on": ["a", "a"] }"#,
        ),
        bad(
            Manifest,
            "needs that are not a list",
            r#"{ "id": "p", "depends_on": "a" }"#,
        ),
        bad(Manifest, "an unknown key", r#"{ "id": "p", "version": 1 }"#),
        bad(Manifest, "a number for a name", r#"{ "id": 5 }"#),
        beyond(
            Manifest,
            "needing itself",
            r#"{ "id": "p", "depends_on": ["p"] }"#,
            "a schema cannot compare two values of a file",
        ),
        // ---- blocks ----
        ok(Block, "a colour is enough", r#"{ "color": [0, 0.5, 1] }"#),
        ok(
            Block,
            "whole numbers written with a point",
            r#"{ "color": [0,0,0], "properties": { "test:glow": 7.0 } }"#,
        ),
        ok(
            Block,
            "everything",
            r#"{ "color": [1,1,1], "see_through": true,
            "states": ["full", "empty", "half:positive_z", "stair:negative_x", { "boxes": [[0,0,0,8,4,8]] }],
            "state_colors": [[0,0,0]], "properties": { "test:glow": 255, "test:sticky": false },
            "state_properties": { "0": { "test:glow": 0 }, "4": { "test:sticky": true } } }"#,
        ),
        ok(
            Block,
            "naming its schema",
            r#"{ "$schema": "../../block.schema.json", "color": [0,0,0] }"#,
        ),
        bad(Block, "no colour", "{}"),
        bad(Block, "a colour of two numbers", r#"{ "color": [0, 1] }"#),
        bad(Block, "a colour of four", r#"{ "color": [0, 1, 0, 1] }"#),
        bad(Block, "a component above 1", r#"{ "color": [0, 1.01, 0] }"#),
        bad(
            Block,
            "a component below 0",
            r#"{ "color": [-0.01, 0, 0] }"#,
        ),
        bad(Block, "a colour of text", r#"{ "color": ["a", "b", "c"] }"#),
        bad(
            Block,
            "a misspelt key",
            r#"{ "color": [0,0,0], "colour": [0,0,0] }"#,
        ),
        bad(
            Block,
            "see-through as text",
            r#"{ "color": [0,0,0], "see_through": "yes" }"#,
        ),
        bad(Block, "no states", r#"{ "color": [0,0,0], "states": [] }"#),
        bad(
            Block,
            "a shape that is not one",
            r#"{ "color": [0,0,0], "states": ["slab"] }"#,
        ),
        bad(
            Block,
            "a half without a side",
            r#"{ "color": [0,0,0], "states": ["half"] }"#,
        ),
        bad(
            Block,
            "a whole with a side",
            r#"{ "color": [0,0,0], "states": ["full:positive_y"] }"#,
        ),
        bad(
            Block,
            "a side that is not one",
            r#"{ "color": [0,0,0], "states": ["half:up"] }"#,
        ),
        bad(
            Block,
            "boxes with none",
            r#"{ "color": [0,0,0], "states": [{ "boxes": [] }] }"#,
        ),
        bad(
            Block,
            "a box of five numbers",
            r#"{ "color": [0,0,0], "states": [{ "boxes": [[0,0,0,8,8]] }] }"#,
        ),
        bad(
            Block,
            "a box leaving the voxel",
            r#"{ "color": [0,0,0], "states": [{ "boxes": [[0,0,0,9,8,8]] }] }"#,
        ),
        bad(
            Block,
            "a box with a fraction",
            r#"{ "color": [0,0,0], "states": [{ "boxes": [[0,0,0,8.5,8,8]] }] }"#,
        ),
        bad(
            Block,
            "boxes by another name",
            r#"{ "color": [0,0,0], "states": [{ "box": [[0,0,0,8,8,8]] }] }"#,
        ),
        bad(
            Block,
            "no state colours",
            r#"{ "color": [0,0,0], "state_colors": [] }"#,
        ),
        bad(
            Block,
            "a state colour out of range",
            r#"{ "color": [0,0,0], "state_colors": [[0,0,2]] }"#,
        ),
        bad(
            Block,
            "a property nobody declared",
            r#"{ "color": [0,0,0], "properties": { "test:nothing": 1 } }"#,
        ),
        bad(
            Block,
            "a number above the range",
            r#"{ "color": [0,0,0], "properties": { "test:glow": 256 } }"#,
        ),
        bad(
            Block,
            "a number below the range",
            r#"{ "color": [0,0,0], "properties": { "test:glow": -1 } }"#,
        ),
        bad(
            Block,
            "a number with a fraction",
            r#"{ "color": [0,0,0], "properties": { "test:glow": 1.5 } }"#,
        ),
        bad(
            Block,
            "a flag given a number",
            r#"{ "color": [0,0,0], "properties": { "test:sticky": 1 } }"#,
        ),
        bad(
            Block,
            "a number given a flag",
            r#"{ "color": [0,0,0], "properties": { "test:glow": true } }"#,
        ),
        bad(
            Block,
            "properties as a list",
            r#"{ "color": [0,0,0], "properties": [] }"#,
        ),
        bad(
            Block,
            "state properties for a state called x",
            r#"{ "color": [0,0,0], "state_properties": { "x": {} } }"#,
        ),
        bad(
            Block,
            "state properties that are undeclared",
            r#"{ "color": [0,0,0], "state_properties": { "0": { "test:nothing": 1 } } }"#,
        ),
        beyond(
            Block,
            "a box with no size",
            r#"{ "color": [0,0,0], "states": [{ "boxes": [[4,0,0,4,8,8]] }] }"#,
            "a schema cannot compare two numbers of a list",
        ),
        beyond(
            Block,
            "a box turned inside out",
            r#"{ "color": [0,0,0], "states": [{ "boxes": [[5,0,0,2,8,8]] }] }"#,
            "a schema cannot compare two numbers of a list",
        ),
        beyond(
            Block,
            "properties of a state the block does not have",
            r#"{ "color": [0,0,0], "state_properties": { "1": { "test:glow": 1 } } }"#,
            "a schema cannot count the states of a list",
        ),
        // ---- patches ----
        ok(
            Patch,
            "the smallest",
            r#"{ "target": "a:b", "operations": [{ "op": "add", "shape": "full" }] }"#,
        ),
        ok(
            Patch,
            "naming its schema",
            r#"{ "$schema": "x", "target": "a:b", "operations": [{ "op": "add", "shape": "half:negative_y" }] }"#,
        ),
        ok(
            Patch,
            "every kind of operation",
            r#"{ "target": "a:b", "optional": true, "operations": [
            { "op": "test", "property": "x:y", "value": 3 },
            { "op": "set", "property": "x:y", "state": 2, "value": true },
            { "op": "remove", "property": "x:y" },
            { "op": "remove", "property": "x:y", "state": 0 },
            { "op": "set", "path": "color", "value": [0, 1, 0] },
            { "op": "test", "path": "see_through", "value": false },
            { "op": "set", "path": "state_colors", "value": [[0,0,0]] },
            { "op": "remove", "path": "state_colors" },
            { "op": "add", "shape": { "boxes": [[0,0,0,8,8,8]] } } ] }"#,
        ),
        bad(
            Patch,
            "no target",
            r#"{ "operations": [{ "op": "add", "shape": "full" }] }"#,
        ),
        bad(
            Patch,
            "a target without a namespace",
            r#"{ "target": "lamp", "operations": [{ "op": "add", "shape": "full" }] }"#,
        ),
        bad(Patch, "no operations", r#"{ "target": "a:b" }"#),
        bad(
            Patch,
            "an empty list of them",
            r#"{ "target": "a:b", "operations": [] }"#,
        ),
        bad(
            Patch,
            "optional as text",
            r#"{ "target": "a:b", "optional": "yes", "operations": [{ "op": "add", "shape": "full" }] }"#,
        ),
        bad(
            Patch,
            "an operation that is not one",
            r#"{ "target": "a:b", "operations": [{ "op": "replace", "property": "x:y", "value": 1 }] }"#,
        ),
        bad(
            Patch,
            "a set with no value",
            r#"{ "target": "a:b", "operations": [{ "op": "set", "property": "x:y" }] }"#,
        ),
        bad(
            Patch,
            "a property and a path together",
            r#"{ "target": "a:b", "operations": [{ "op": "set", "property": "x:y", "path": "color", "value": 1 }] }"#,
        ),
        bad(
            Patch,
            "neither a property nor a path",
            r#"{ "target": "a:b", "operations": [{ "op": "set", "value": 1 }] }"#,
        ),
        bad(
            Patch,
            "a field that is not one",
            r#"{ "target": "a:b", "operations": [{ "op": "set", "path": "colour", "value": [0,0,0] }] }"#,
        ),
        bad(
            Patch,
            "removing the colour",
            r#"{ "target": "a:b", "operations": [{ "op": "remove", "path": "color" }] }"#,
        ),
        bad(
            Patch,
            "a state with a field",
            r#"{ "target": "a:b", "operations": [{ "op": "set", "path": "color", "state": 1, "value": [0,0,0] }] }"#,
        ),
        bad(
            Patch,
            "a colour out of range",
            r#"{ "target": "a:b", "operations": [{ "op": "set", "path": "color", "value": [0, 2, 0] }] }"#,
        ),
        bad(
            Patch,
            "see-through given a number",
            r#"{ "target": "a:b", "operations": [{ "op": "set", "path": "see_through", "value": 1 }] }"#,
        ),
        bad(
            Patch,
            "a property value of text",
            r#"{ "target": "a:b", "operations": [{ "op": "set", "property": "x:y", "value": "high" }] }"#,
        ),
        bad(
            Patch,
            "a property value with a fraction",
            r#"{ "target": "a:b", "operations": [{ "op": "set", "property": "x:y", "value": 2.5 }] }"#,
        ),
        bad(
            Patch,
            "a negative state",
            r#"{ "target": "a:b", "operations": [{ "op": "set", "property": "x:y", "state": -1, "value": 1 }] }"#,
        ),
        bad(
            Patch,
            "an add with no shape",
            r#"{ "target": "a:b", "operations": [{ "op": "add" }] }"#,
        ),
        bad(
            Patch,
            "an add with a value",
            r#"{ "target": "a:b", "operations": [{ "op": "add", "shape": "full", "value": 1 }] }"#,
        ),
        bad(
            Patch,
            "state colours given none",
            r#"{ "target": "a:b", "operations": [{ "op": "set", "path": "state_colors", "value": [] }] }"#,
        ),
        bad(
            Patch,
            "a remove with a value",
            r#"{ "target": "a:b", "operations": [{ "op": "remove", "property": "x:y", "value": 1 }] }"#,
        ),
        // ---- bodies ----
        ok(Body, "the smallest", MINIMAL_BODY),
        ok(
            Body,
            "with a mass, a place, a seed that is the one of the world and the greatest numbers",
            r#"{ "$schema": "../../body.schema.json", "level": "test_game:ground", "position": [0, -5.5, 1e9],
            "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 0.5, "seed": "world" } },
            "extent_bricks": 1000000, "mass": 3.4e38 }"#,
        ),
        bad(Body, "nothing", r#"{}"#),
        bad(
            Body,
            "no level",
            r#"{ "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1 }"#,
        ),
        bad(
            Body,
            "a level the game does not have",
            r#"{ "level": "test_game:below", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1 }"#,
        ),
        bad(
            Body,
            "no terrain",
            r#"{ "level": "test_game:space", "extent_bricks": 2, "mass": 1 }"#,
        ),
        bad(
            Body,
            "terrain with no parameters",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1" }, "extent_bricks": 2, "mass": 1 }"#,
        ),
        bad(
            Body,
            "a family nobody has",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:cube_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1 }"#,
        ),
        bad(
            Body,
            "parameters the family refuses",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": -5 } }, "extent_bricks": 2, "mass": 1 }"#,
        ),
        bad(
            Body,
            "parameters with a key the family does not have",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5, "colour": 1 } }, "extent_bricks": 2, "mass": 1 }"#,
        ),
        bad(
            Body,
            "no extent",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "mass": 1 }"#,
        ),
        bad(
            Body,
            "an extent of nothing",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 0, "mass": 1 }"#,
        ),
        bad(
            Body,
            "an extent past the greatest",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 1000001, "mass": 1 }"#,
        ),
        bad(
            Body,
            "an extent with a fraction",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 1.5, "mass": 1 }"#,
        ),
        bad(
            Body,
            "no weight",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2 }"#,
        ),
        bad(
            Body,
            "two weights",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1, "surface_gravity": 1 }"#,
        ),
        bad(
            Body,
            "a mass of nothing",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 0 }"#,
        ),
        bad(
            Body,
            "a mass too great for the ship it is",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 3.5e38 }"#,
        ),
        bad(
            Body,
            "a pull below nothing",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "surface_gravity": -9.8 }"#,
        ),
        bad(
            Body,
            "a pull of nothing",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "surface_gravity": 0 }"#,
        ),
        bad(
            Body,
            "a position of two numbers",
            r#"{ "level": "test_game:space", "position": [0, 0], "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1 }"#,
        ),
        bad(
            Body,
            "a position past the limit",
            r#"{ "level": "test_game:space", "position": [0, 2e15, 0], "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1 }"#,
        ),
        bad(
            Body,
            "a misspelt key",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_brick": 2, "mass": 1 }"#,
        ),
        ok(
            Body,
            "a spin and a place",
            r#"{ "level": "test_game:space", "position": [1, 2, 3], "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1, "rail": { "spin": { "axis": [0, 1, 0], "period": 3600 } } }"#,
        ),
        ok(
            Body,
            "an orbit about a point with every key and no place",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1, "rail": { "orbit": { "around": [0, 0, 0], "normal": [1, 1, 0], "radius": 1e6, "period": -31557600, "phase": 90 } } }"#,
        ),
        ok(
            Body,
            "an orbit about a body and a spin together",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1, "rail": { "spin": { "axis": [0, 1, 0], "period": 3600, "angle": 15 }, "orbit": { "around": "test_game:star", "radius": 100, "period": 60 } } }"#,
        ),
        bad(
            Body,
            "an orbit and a place",
            r#"{ "level": "test_game:space", "position": [1, 2, 3], "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1, "rail": { "orbit": { "around": [0, 0, 0], "radius": 100, "period": 60 } } }"#,
        ),
        bad(
            Body,
            "a rail of nothing",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1, "rail": {} }"#,
        ),
        bad(
            Body,
            "a rail with a key it has not",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1, "rail": { "spinn": {} } }"#,
        ),
        bad(
            Body,
            "a spin with no axis",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1, "rail": { "spin": { "period": 3600 } } }"#,
        ),
        bad(
            Body,
            "a spin about nothing",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1, "rail": { "spin": { "axis": [0, 0, 0], "period": 3600 } } }"#,
        ),
        bad(
            Body,
            "a spin with an axis of two",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1, "rail": { "spin": { "axis": [0, 1], "period": 3600 } } }"#,
        ),
        bad(
            Body,
            "a spin with a period of nothing",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1, "rail": { "spin": { "axis": [0, 1, 0], "period": 0 } } }"#,
        ),
        bad(
            Body,
            "an orbit with no radius",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1, "rail": { "orbit": { "around": [0, 0, 0], "period": 60 } } }"#,
        ),
        bad(
            Body,
            "an orbit with a radius of nothing",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1, "rail": { "orbit": { "around": [0, 0, 0], "radius": 0, "period": 60 } } }"#,
        ),
        bad(
            Body,
            "an orbit of a radius past the greatest place",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1, "rail": { "orbit": { "around": [0, 0, 0], "radius": 2e15, "period": 60 } } }"#,
        ),
        bad(
            Body,
            "an orbit about two numbers",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1, "rail": { "orbit": { "around": [0, 0], "radius": 100, "period": 60 } } }"#,
        ),
        bad(
            Body,
            "an orbit about a name with no pack",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1, "rail": { "orbit": { "around": "star", "radius": 100, "period": 60 } } }"#,
        ),
        bad(
            Body,
            "an orbit about a number",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1, "rail": { "orbit": { "around": 7, "radius": 100, "period": 60 } } }"#,
        ),
        bad(
            Body,
            "an orbit in no plane",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1, "rail": { "orbit": { "around": [0, 0, 0], "normal": [0, 0, 0], "radius": 100, "period": 60 } } }"#,
        ),
        bad(
            Body,
            "an orbit with a phase that is no number",
            r#"{ "level": "test_game:space", "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } }, "extent_bricks": 2, "mass": 1, "rail": { "orbit": { "around": [0, 0, 0], "radius": 100, "period": 60, "phase": null } } }"#,
        ),
    ]
}

const MINIMAL_BODY: &str = r#"{ "level": "test_game:space",
    "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } },
    "extent_bricks": 2, "surface_gravity": 9.8 }"#;

#[test]
fn the_schema_and_the_loader_agree_about_every_file_that_is_only_a_matter_of_shape() {
    let validators = validators();
    let mut disagreements = Vec::new();
    for case in corpus() {
        let instance: Value = serde_json::from_str(case.text)
            .unwrap_or_else(|error| panic!("{}: the corpus file is not JSON: {error}", case.why));
        let schema_says = validators.schema_accepts(case.kind, &instance);
        let loader_says = validators.loader_accepts(case.kind, case.text);
        let expected_loader = case.accepted;
        let expected_schema = case.accepted || case.beyond_the_schema.is_some();
        if (schema_says, loader_says) != (expected_schema, expected_loader) {
            disagreements.push(format!(
                "{:?} {}: schema {schema_says}, loader {loader_says}; expected schema {expected_schema}, loader {expected_loader}",
                case.kind, case.why
            ));
        }
    }
    assert!(disagreements.is_empty(), "{disagreements:#?}");
}

#[test]
fn the_schema_names_every_declared_property_with_its_kind_range_default_and_doc() {
    let PackSchemas { block, .. } = pack_schemas(&properties(), &BodyContext::NONE);
    let glow = &block["properties"]["properties"]["properties"]["test:glow"];
    assert_eq!(glow["type"], "integer");
    assert_eq!(glow["minimum"], 0);
    assert_eq!(glow["maximum"], 255);
    assert_eq!(glow["description"], "how much light (default 0)");
    let sticky = &block["properties"]["properties"]["properties"]["test:sticky"];
    assert_eq!(sticky["type"], "boolean");
    assert_eq!(sticky["description"], "whether it sticks (default false)");
    // The engine's own properties are there too, and the same set is allowed for each state.
    assert!(block["properties"]["properties"]["properties"]["engine:density"].is_object());
    assert_eq!(
        block["properties"]["state_properties"]["patternProperties"]["^[0-9]+$"],
        block["properties"]["properties"]
    );
}

#[test]
fn a_game_with_another_property_gets_a_schema_with_that_property() {
    let mut other = PropertySchema::new();
    other
        .declare(PropertyDeclaration {
            id: id("other:heat"),
            kind: PropertyKind::Integer {
                minimum: -5,
                maximum: 5,
            },
            default: 1,
            doc: "warmth".to_owned(),
        })
        .unwrap();
    let schemas = pack_schemas(&other, &BodyContext::NONE);
    let validator = jsonschema::validator_for(&schemas.block).unwrap();
    assert!(validator.is_valid(&json!({ "color": [0,0,0], "properties": { "other:heat": -5 } })));
    assert!(!validator.is_valid(&json!({ "color": [0,0,0], "properties": { "other:heat": 6 } })));
    assert!(!validator.is_valid(&json!({ "color": [0,0,0], "properties": { "test:glow": 1 } })));
}

// ---- the same, on files made by damaging good ones ----

const GOOD_BLOCK: &str = r#"{ "color": [0.5, 0.5, 0.5], "see_through": false,
    "states": ["full", "half:negative_y", "stair:positive_x", { "boxes": [[0,0,0,8,8,8], [0,0,0,4,4,4]] }],
    "state_colors": [[0,0,0], [1,1,1]], "properties": { "test:glow": 3, "test:sticky": true },
    "state_properties": { "1": { "test:glow": 0 }, "3": { "test:sticky": false } } }"#;
const GOOD_BODY: &str = r#"{ "level": "test_game:ground", "position": [1, 2, 3],
    "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 300, "seed": 99 } },
    "extent_bricks": 12, "mass": 6.07e20,
    "rail": { "spin": { "axis": [0, 1, 0], "period": 3600 } } }"#;
const GOOD_ORBITING_BODY: &str = r#"{ "level": "test_game:ground",
    "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 300, "seed": 99 } },
    "extent_bricks": 12, "surface_gravity": 3,
    "rail": { "spin": { "axis": [0, 1, 0], "period": 3600, "angle": 15 },
              "orbit": { "around": "test_game:star", "normal": [0, 1, 1], "radius": 1.5e6, "period": 31557600, "phase": 90 } } }"#;
const GOOD_MANIFEST: &str = r#"{ "id": "my_pack", "depends_on": ["base_blocks", "other"] }"#;
const GOOD_PATCH: &str = r#"{ "target": "base_blocks:lamp", "optional": true, "operations": [
    { "op": "set", "property": "test:glow", "state": 0, "value": 9 },
    { "op": "set", "path": "color", "value": [1, 0.5, 0.25] },
    { "op": "remove", "path": "state_colors" },
    { "op": "add", "shape": "half:positive_y" },
    { "op": "test", "path": "see_through", "value": false } ] }"#;

/// Every place in a value that can be replaced, as a JSON pointer.
fn places(value: &Value, here: String, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                let path = format!("{here}/{}", key.replace('~', "~0").replace('/', "~1"));
                found.push(path.clone());
                places(inner, path, found);
            }
        }
        Value::Array(list) => {
            for (index, inner) in list.iter().enumerate() {
                let path = format!("{here}/{index}");
                found.push(path.clone());
                places(inner, path, found);
            }
        }
        _ => {}
    }
}

fn replacements() -> Vec<Value> {
    vec![
        json!(null),
        json!(true),
        json!(false),
        json!("x"),
        json!("full"),
        json!("engine"),
        json!(1),
        json!(-1),
        json!(300),
        json!(2.5),
        json!([]),
        json!({}),
        json!([0, 0]),
        json!([0, 0, 0]),
        json!([[0, 0, 0, 8, 8, 8]]),
        json!("half:positive_x"),
    ]
}

fn damaged(text: &str) -> impl Strategy<Value = Value> {
    let good: Value = serde_json::from_str(text).unwrap();
    let mut found = Vec::new();
    places(&good, String::new(), &mut found);
    (
        prop::sample::select(found),
        prop::sample::select(replacements()),
    )
        .prop_map(move |(path, replacement)| {
            let mut damaged = good.clone();
            *damaged.pointer_mut(&path).expect("the place exists") = replacement;
            damaged
        })
}

fn agree(kind: Kind, damaged: &Value) -> Result<(), TestCaseError> {
    let validators = validators();
    let text = damaged.to_string();
    let (schema_says, loader_says) = (
        validators.schema_accepts(kind, damaged),
        validators.loader_accepts(kind, &text),
    );
    prop_assert_eq!(schema_says, loader_says, "{} ({:?})", text, kind);
    Ok(())
}

proptest! {
    #[test]
    fn a_block_damaged_in_one_place_is_judged_alike_by_the_schema_and_the_loader(
        damaged in damaged(GOOD_BLOCK)
    ) {
        agree(Block, &damaged)?;
    }

    #[test]
    fn a_manifest_damaged_in_one_place_is_judged_alike(damaged in damaged(GOOD_MANIFEST)) {
        agree(Manifest, &damaged)?;
    }

    #[test]
    fn a_patch_damaged_in_one_place_is_judged_alike(damaged in damaged(GOOD_PATCH)) {
        agree(Patch, &damaged)?;
    }

    #[test]
    fn a_body_damaged_in_one_place_is_judged_alike(damaged in damaged(GOOD_BODY)) {
        agree(Body, &damaged)?;
    }

    #[test]
    fn a_body_that_goes_round_damaged_in_one_place_is_judged_alike(
        damaged in damaged(GOOD_ORBITING_BODY)
    ) {
        agree(Body, &damaged)?;
    }
}

#[test]
fn the_good_files_the_damage_starts_from_are_good() {
    let validators = validators();
    for (kind, text) in [
        (Block, GOOD_BLOCK),
        (Manifest, GOOD_MANIFEST),
        (Patch, GOOD_PATCH),
        (Body, GOOD_BODY),
        (Body, GOOD_ORBITING_BODY),
    ] {
        let instance: Value = serde_json::from_str(text).unwrap();
        assert!(validators.schema_accepts(kind, &instance), "{kind:?}");
        assert!(validators.loader_accepts(kind, text), "{kind:?}");
    }
}

//! Reading pack files: what good ones become, and exactly what is said about each kind of bad one.

use block_models::{
    BlockDefinition, PropertyDeclaration, PropertyKind, PropertySchema, Shape, ShapeBox,
    Transparency,
};
use data_packs::{
    Operation, Patch, PatchValue, Place, PropertyValue, read_block, read_manifest, read_patch,
};
use namespaced_id::NamespacedId;
use voxel_space::FaceDirection;

fn id(text: &str) -> NamespacedId {
    text.parse().unwrap()
}

/// The properties of the engine plus a number and a flag of the tests'.
fn schema() -> PropertySchema {
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

const FILE: &str = "my_pack/blocks/marble.json";

fn block(text: &str) -> Result<(NamespacedId, BlockDefinition), Vec<String>> {
    read_block(FILE, "my_pack", "marble", text, &schema()).map_err(messages)
}

fn messages(errors: data_packs::PackErrors) -> Vec<String> {
    errors.0.iter().map(ToString::to_string).collect()
}

fn block_errors(text: &str) -> Vec<String> {
    block(text).expect_err("the file should be refused")
}

// ---- manifests ----

#[test]
fn a_manifest_names_the_pack_and_what_it_needs() {
    let manifest = read_manifest(
        "my_pack/pack.json",
        r#"{ "id": "my_pack", "depends_on": ["base_blocks", "other_pack"] }"#,
    )
    .unwrap();
    assert_eq!(manifest.id, "my_pack");
    assert_eq!(manifest.depends_on, ["base_blocks", "other_pack"]);
    let lone = read_manifest("p/pack.json", r#"{ "id": "lone" }"#).unwrap();
    assert!(lone.depends_on.is_empty());
    assert_eq!(lone.version, None);
}

#[test]
fn a_manifest_may_say_which_format_it_is_written_in_and_which_version_of_the_pack_it_is() {
    let manifest = read_manifest(
        "my_pack/pack.json",
        r#"{ "id": "my_pack", "format": 1, "version": "0.3.1" }"#,
    )
    .unwrap();
    assert_eq!(manifest.version.as_deref(), Some("0.3.1"));

    let file = "my_pack/pack.json";
    assert_eq!(
        manifest_errors(r#"{ "id": "my_pack", "format": 2 }"#),
        [format!("{file}: /format: 2 is outside 1 to 1")]
    );
    assert_eq!(
        manifest_errors(r#"{ "id": "my_pack", "format": "1" }"#),
        [format!(
            "{file}: /format: expected a whole number, found text"
        )]
    );
    assert_eq!(
        manifest_errors(r#"{ "id": "my_pack", "version": 3 }"#),
        [format!("{file}: /version: expected text, found a number")]
    );
    assert_eq!(
        manifest_errors(r#"{ "id": "my_pack", "version": "" }"#),
        [format!(
            "{file}: /version: the version is empty; leave it out or say something"
        )]
    );
}

fn manifest_errors(text: &str) -> Vec<String> {
    messages(read_manifest("my_pack/pack.json", text).expect_err("refused"))
}

#[test]
fn a_manifest_that_is_wrong_says_what_and_where() {
    let file = "my_pack/pack.json";
    assert_eq!(
        manifest_errors(r#"{ "id": "my_pack", "depends_on": ["a", "b", "a"] }"#),
        [format!("{file}: /depends_on/2: \"a\" is listed twice")]
    );
    assert_eq!(
        manifest_errors(r#"{ "id": "my_pack", "depends_on": ["my_pack"] }"#),
        [format!(
            "{file}: /depends_on/0: a pack cannot depend on itself"
        )]
    );
    assert_eq!(
        manifest_errors(r#"{ "id": "engine" }"#),
        [format!(
            "{file}: /id: \"engine\" is reserved for the engine"
        )]
    );
    assert_eq!(manifest_errors("{}"), [format!("{file}: missing \"id\"")]);
    assert_eq!(
        manifest_errors(r#"{ "id": 5 }"#),
        [format!("{file}: /id: expected text, found a number")]
    );
    assert_eq!(
        manifest_errors(r#"{ "id": "my_pack", "depends_on": "base" }"#),
        [format!("{file}: /depends_on: expected a list, found text")]
    );
    let bad_name = manifest_errors(r#"{ "id": "My Pack" }"#);
    assert_eq!(bad_name.len(), 1);
    assert!(
        bad_name[0].starts_with(&format!("{file}: /id: \"My Pack\" is not a pack name: ")),
        "{bad_name:?}"
    );
    assert_eq!(
        manifest_errors(r#"{ "idd": "my_pack" }"#),
        [
            format!(
                "{file}: /idd: unknown key \"idd\" (did you mean \"id\"?); the keys here are \"id\", \"depends_on\", \"format\", \"version\""
            ),
            format!("{file}: missing \"id\""),
        ]
    );
}

#[test]
fn a_file_that_is_not_json_is_one_error_with_the_line_and_column() {
    let errors = manifest_errors("{ \"id\": \n  \"my_pack\" ");
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].starts_with("my_pack/pack.json: this is not JSON (line 2, column "),
        "{errors:?}"
    );
}

// ---- blocks ----

#[test]
fn the_smallest_block_is_a_colour_and_is_one_whole_state() {
    let (identifier, definition) = block(r#"{ "color": [0.1, 0.2, 0.3] }"#).unwrap();
    assert_eq!(identifier, id("my_pack:marble"));
    assert_eq!(definition, BlockDefinition::cube([0.1, 0.2, 0.3]));
}

#[test]
fn a_block_with_everything_is_what_the_same_calls_make() {
    let text = r#"{
        "color": [0.9, 0.9, 0.95],
        "sight_through": true,
        "light_through": true,
        "transparency": "leaves",
        "states": [ "full", "half:negative_y", "stair:positive_x", "empty",
                    { "boxes": [[0,0,0, 8,4,8], [2,4,2, 6,8,6]] } ],
        "state_colors": [[0.9, 0.9, 0.95], [0.8, 0.8, 0.9]],
        "face_colors": { "top": [1, 0, 0], "positive_x": [0, 1, 0] },
        "properties": { "test:glow": 3, "test:sticky": true },
        "state_properties": { "1": { "test:glow": 0 }, "4": { "test:sticky": false } }
    }"#;
    let (_, definition) = block(text).unwrap();
    let expected = BlockDefinition::cube([0.9, 0.9, 0.95])
        .with_sight_through(true)
        .with_light_through(true)
        .with_transparency(Transparency::Leaves)
        .with_shapes(vec![
            Shape::full(),
            Shape::half(FaceDirection::NegativeY),
            Shape::stair(FaceDirection::PositiveX),
            Shape::empty(),
            Shape::from_boxes(&[
                ShapeBox {
                    min: [0, 0, 0],
                    max: [8, 4, 8],
                },
                ShapeBox {
                    min: [2, 4, 2],
                    max: [6, 8, 6],
                },
            ]),
        ])
        .with_state_colors(vec![[0.9, 0.9, 0.95], [0.8, 0.8, 0.9]])
        .with_face_colors(&[
            (FaceDirection::PositiveY, [1.0, 0.0, 0.0]),
            (FaceDirection::PositiveX, [0.0, 1.0, 0.0]),
        ])
        .with_property_value(id("test:glow"), 3)
        .with_property_value(id("test:sticky"), 1)
        .with_state_property_value(1, id("test:glow"), 0)
        .with_state_property_value(4, id("test:sticky"), 0);
    assert_eq!(definition, expected);
}

#[test]
fn a_block_file_that_is_not_an_object_or_not_json_is_refused() {
    assert_eq!(
        block_errors("[1, 2]"),
        [format!("{FILE}: expected an object, found a list")]
    );
    let errors = block_errors("{ color: [1,1,1] }");
    assert!(errors[0].starts_with(&format!("{FILE}: this is not JSON (line 1, column ")));
}

#[test]
fn the_file_name_must_make_a_block_name() {
    let error = read_block(
        "my_pack/blocks/Marble Stone.json",
        "my_pack",
        "Marble Stone",
        r#"{ "color": [0, 0, 0] }"#,
        &schema(),
    )
    .unwrap_err();
    let messages = messages(error);
    assert_eq!(messages.len(), 1);
    assert!(
        messages[0].starts_with(
            "my_pack/blocks/Marble Stone.json: the file name \"Marble Stone\" is not a block name: "
        ),
        "{messages:?}"
    );
}

#[test]
fn colours_must_be_three_numbers_from_0_to_1() {
    assert_eq!(block_errors("{}"), [format!("{FILE}: missing \"color\"")]);
    assert_eq!(
        block_errors(r#"{ "color": [0, 1.5, -0.1] }"#),
        [
            format!("{FILE}: /color/1: 1.5 is outside 0 to 1"),
            format!("{FILE}: /color/2: -0.1 is outside 0 to 1"),
        ]
    );
    assert_eq!(
        block_errors(r#"{ "color": [0, 1] }"#),
        [format!(
            "{FILE}: /color: a colour is 3 numbers (red, green, blue), found 2"
        )]
    );
    assert_eq!(
        block_errors(r#"{ "color": [0, "x", 1] }"#),
        [format!("{FILE}: /color/1: expected a number, found text")]
    );
    assert_eq!(
        block_errors(r#"{ "color": "red" }"#),
        [format!("{FILE}: /color: expected a list, found text")]
    );
}

#[test]
fn a_property_that_names_a_face_takes_the_word_or_the_code_and_means_the_same() {
    let face = |text: &str| {
        let (_, definition) = block(text).unwrap();
        definition.property_value(&id("engine:axis_face"), None, -1)
    };
    for (word, code) in [
        ("none", 0),
        ("west", 1),
        ("east", 2),
        ("down", 3),
        ("up", 4),
        ("north", 5),
        ("south", 6),
    ] {
        let by_word =
            format!(r#"{{ "color": [0,0,0], "properties": {{ "engine:axis_face": "{word}" }} }}"#);
        let by_code =
            format!(r#"{{ "color": [0,0,0], "properties": {{ "engine:axis_face": {code} }} }}"#);
        assert_eq!(face(&by_word), code);
        assert_eq!(face(&by_code), code);
    }
    // It is the same in a state of its own.
    let (_, definition) = block(
        r#"{ "color": [0,0,0], "states": ["full", "full"], "state_properties": { "1": { "engine:docking_face": "east" } } }"#,
    )
    .unwrap();
    assert_eq!(
        definition.property_value(&id("engine:docking_face"), Some(1), 0),
        2
    );
}

#[test]
fn a_face_that_is_wrong_says_what_a_face_is() {
    let words = "\"none\", \"west\", \"east\", \"down\", \"up\", \"north\", \"south\"";
    let wrong = |value: &str| {
        block_errors(&format!(
            r#"{{ "color": [0,0,0], "properties": {{ "engine:front_face": {value} }} }}"#
        ))
    };
    assert_eq!(
        wrong(r#""top""#),
        [format!(
            "{FILE}: /properties/engine:front_face: expected a face ({words}) or a number from 0 to 6, found \"top\""
        )]
    );
    assert_eq!(
        wrong(r#""nort""#),
        [format!(
            "{FILE}: /properties/engine:front_face: expected a face ({words}) or a number from 0 to 6, found \"nort\" (did you mean \"north\"?)"
        )]
    );
    assert_eq!(
        wrong("7"),
        [format!(
            "{FILE}: /properties/engine:front_face: 7 is outside 0 to 6"
        )]
    );
    assert_eq!(
        wrong("true"),
        [format!(
            "{FILE}: /properties/engine:front_face: expected a face ({words}) or a number from 0 to 6, found true or false"
        )]
    );
    // A word is not a number for a property that is a number, nor a flag.
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "properties": { "test:glow": "up" } }"#),
        [format!(
            "{FILE}: /properties/test:glow: expected a whole number, found text"
        )]
    );
}

#[test]
fn a_texture_is_a_base_name_in_the_packs_own_namespace_or_a_name_with_the_namespace_of_another_pack()
 {
    let (_, own) = block(r#"{ "color": [0.1, 0.2, 0.3], "texture": "oak" }"#).unwrap();
    assert_eq!(own.texture(), Some(&id("my_pack:oak")));
    assert!(
        !own.color_from_pictures(),
        "a colour that is written is the author's own"
    );
    assert_eq!(own.color, [0.1, 0.2, 0.3]);
    let (_, other) = block(r#"{ "color": [0, 0, 0], "texture": "base_blocks:stone" }"#).unwrap();
    assert_eq!(other.texture(), Some(&id("base_blocks:stone")));
    let (_, none) = block(r#"{ "color": [0, 0, 0] }"#).unwrap();
    assert_eq!(none.texture(), None);
}

#[test]
fn sight_and_light_are_separate_flags_and_the_transparency_mode_is_a_word_for_a_block_that_can_be_seen_through()
 {
    let (_, definition) =
        block(r#"{ "color": [0, 0, 0], "sight_through": true, "transparency": "glass" }"#).unwrap();
    assert!(definition.sight_through && !definition.light_through);
    assert_eq!(definition.transparency(), Transparency::Glass);
    let (_, definition) = block(r#"{ "color": [0, 0, 0], "light_through": true }"#).unwrap();
    assert!(!definition.sight_through && definition.light_through);
    assert_eq!(definition.transparency(), Transparency::Opaque);
    // "opaque" may be written, and is what no word means.
    let (_, written) = block(r#"{ "color": [0, 0, 0], "transparency": "opaque" }"#).unwrap();
    assert_eq!(written, BlockDefinition::cube([0.0; 3]));
    // A word that is no mode is corrected, and every mode is named.
    assert_eq!(
        block_errors(r#"{ "color": [0, 0, 0], "sight_through": true, "transparency": "leeves" }"#),
        [format!(
            "{FILE}: /transparency: \"leeves\" is not a transparency mode (did you mean \"leaves\"?); the modes are \"opaque\", \"leaves\", \"glass\""
        )]
    );
    assert_eq!(
        block_errors(r#"{ "color": [0, 0, 0], "transparency": 3 }"#),
        [format!(
            "{FILE}: /transparency: expected text, found a number"
        )]
    );
    // A mode is for a block that can be seen through: the holes would show nothing.
    for mode in ["leaves", "glass"] {
        assert_eq!(
            block_errors(&format!(
                r#"{{ "color": [0, 0, 0], "light_through": true, "transparency": "{mode}" }}"#
            )),
            [format!(
                "{FILE}: /transparency: a transparency mode needs \"sight_through\": true, or nothing shows through the block"
            )],
            "{mode}"
        );
    }
}

#[test]
fn the_colour_may_be_left_out_of_a_block_with_a_texture_and_then_it_follows_the_pictures() {
    let (_, definition) = block(r#"{ "texture": "oak" }"#).unwrap();
    assert_eq!(definition.texture(), Some(&id("my_pack:oak")));
    assert!(definition.color_from_pictures());
    // It is the loader that works the colour out, once it knows the pictures; until then it is black.
    assert_eq!(definition.color, [0.0; 3]);
    // Without a texture there is nothing to follow, and the colour is still required.
    assert_eq!(
        block_errors(r#"{ "sight_through": true }"#),
        [format!("{FILE}: missing \"color\"")]
    );
}

#[test]
fn a_texture_that_is_wrong_says_what_a_texture_is() {
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "texture": 5 }"#),
        [format!("{FILE}: /texture: expected text, found a number")]
    );
    for wrong in ["Oak Stone", "a:b:c", "", "oak top", ":oak"] {
        let errors = block_errors(&format!(r#"{{ "color": [0,0,0], "texture": "{wrong}" }}"#));
        assert_eq!(errors.len(), 1, "{wrong}: {errors:?}");
        assert!(
            errors[0].starts_with(&format!(
                "{FILE}: /texture: \"{wrong}\" is not the name of a picture: "
            )),
            "{errors:?}"
        );
    }
}

#[test]
fn faces_get_their_colours_by_group_and_by_name_and_a_name_beats_its_group_wherever_it_is_written()
{
    let faces = |text: &str| {
        let (_, definition) = block(text).unwrap();
        *definition.face_colors()
    };
    let (red, green, blue) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    // top is +y, bottom is -y, and side is the other four.
    assert_eq!(
        faces(r#"{ "color": [0,0,0], "face_colors": { "top": [1,0,0], "bottom": [0,0,1] } }"#),
        [None, None, Some(blue), Some(red), None, None]
    );
    assert_eq!(
        faces(r#"{ "color": [0,0,0], "face_colors": { "side": [0,1,0] } }"#),
        [
            Some(green),
            Some(green),
            None,
            None,
            Some(green),
            Some(green)
        ]
    );
    // An exact face beats its group, whichever is written first.
    for text in [
        r#"{ "color": [0,0,0], "face_colors": { "side": [0,1,0], "positive_x": [1,0,0] } }"#,
        r#"{ "color": [0,0,0], "face_colors": { "positive_x": [1,0,0], "side": [0,1,0] } }"#,
    ] {
        assert_eq!(
            faces(text),
            [Some(green), Some(red), None, None, Some(green), Some(green)],
            "{text}"
        );
    }
    // An exact face can also be the top or the bottom.
    assert_eq!(
        faces(r#"{ "color": [0,0,0], "face_colors": { "top": [1,0,0], "positive_y": [0,0,1] } }"#),
        [None, None, None, Some(blue), None, None]
    );
}

#[test]
fn face_colours_that_are_wrong_say_what_and_where() {
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "face_colors": {} }"#),
        [format!(
            "{FILE}: /face_colors: give at least one face, or leave \"face_colors\" out"
        )]
    );
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "face_colors": [[1,0,0]] }"#),
        [format!(
            "{FILE}: /face_colors: expected an object, found a list"
        )]
    );
    let keys = "\"top\", \"side\", \"bottom\", \"negative_x\", \"positive_x\", \"negative_y\", \"positive_y\", \"negative_z\", \"positive_z\"";
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "face_colors": { "tpo": [1,0,0] } }"#),
        [format!(
            "{FILE}: /face_colors/tpo: unknown key \"tpo\" (did you mean \"top\"?); the keys here are {keys}"
        )]
    );
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "face_colors": { "up": [1,0,0] } }"#),
        [format!(
            "{FILE}: /face_colors/up: unknown key \"up\" (did you mean \"top\"?); the keys here are {keys}"
        )]
    );
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "face_colors": { "front": [1,0,0] } }"#),
        [format!(
            "{FILE}: /face_colors/front: unknown key \"front\"; the keys here are {keys}"
        )]
    );
    assert_eq!(
        block_errors(
            r#"{ "color": [0,0,0], "face_colors": { "top": [2,0,0], "side": [0,0], "positive_x": "red" } }"#
        ),
        [
            format!("{FILE}: /face_colors/top/0: 2 is outside 0 to 1"),
            format!("{FILE}: /face_colors/side: a colour is 3 numbers (red, green, blue), found 2"),
            format!("{FILE}: /face_colors/positive_x: expected a list, found text"),
        ]
    );
}

#[test]
fn an_unknown_key_is_refused_and_a_near_miss_is_corrected() {
    let keys = "\"color\", \"sight_through\", \"light_through\", \"transparency\", \"states\", \"state_colors\", \"face_colors\", \"texture\", \"properties\", \"state_properties\"";
    assert_eq!(
        block_errors(r#"{ "colour": [0, 0, 0] }"#),
        [
            format!(
                "{FILE}: /colour: unknown key \"colour\" (did you mean \"color\"?); the keys here are {keys}"
            ),
            format!("{FILE}: missing \"color\""),
        ]
    );
    assert_eq!(
        block_errors(r#"{ "color": [0, 0, 0], "banana": 1 }"#),
        [format!(
            "{FILE}: /banana: unknown key \"banana\"; the keys here are {keys}"
        )]
    );
}

#[test]
fn properties_must_be_declared_and_their_values_must_fit() {
    let declared = "\"engine:axis_face\", \"engine:density\", \"engine:docking_face\", \"engine:front_face\", \"engine:joint_face\", \"engine:placeable_on_moving_grids\", \"test:glow\", \"test:sticky\"";
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "properties": { "test:glo": 3 } }"#),
        [format!(
            "{FILE}: /properties/test:glo: the property \"test:glo\" is not declared (did you mean \"test:glow\"?); the properties are {declared}"
        )]
    );
    assert_eq!(
        block_errors(
            r#"{ "color": [0,0,0], "properties": { "test:glow": 300, "test:sticky": 1 } }"#
        ),
        [
            format!("{FILE}: /properties/test:glow: 300 is outside 0 to 255"),
            format!("{FILE}: /properties/test:sticky: expected true or false, found a number"),
        ]
    );
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "properties": { "test:glow": 1.5 } }"#),
        [format!(
            "{FILE}: /properties/test:glow: expected a whole number, found 1.5"
        )]
    );
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "properties": { "NoColon": 1 } }"#).len(),
        1
    );
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "properties": [] }"#),
        [format!(
            "{FILE}: /properties: expected an object of properties, found a list"
        )]
    );
}

#[test]
fn properties_of_a_state_must_name_a_state_the_block_has() {
    assert_eq!(
        block_errors(
            r#"{ "color": [0,0,0], "states": ["full", "empty"],
                 "state_properties": { "2": { "test:glow": 1 }, "x": {}, "1": { "test:glow": 999 } } }"#
        ),
        [
            format!("{FILE}: /state_properties/1/test:glow: 999 is outside 0 to 255"),
            format!(
                "{FILE}: /state_properties/2: \"2\" is not a state of this block, which has 2 (0 to 1)"
            ),
            format!(
                "{FILE}: /state_properties/x: \"x\" is not a state of this block, which has 2 (0 to 1)"
            ),
        ]
    );
}

#[test]
fn states_must_be_shapes_a_pack_can_name_or_boxes() {
    let names = "\"full\", \"empty\", \"half\", \"stair\"";
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "states": [] }"#),
        [format!("{FILE}: /states: a block has at least one state")]
    );
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "states": ["hal:negative_y"] }"#),
        [format!(
            "{FILE}: /states/0: \"hal\" is not a shape (did you mean \"half\"?); the shapes are {names}, or {{\"boxes\": [...]}}"
        )]
    );
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "states": ["half"] }"#),
        [format!(
            "{FILE}: /states/0: \"half\" needs a side, as in \"half:negative_y\""
        )]
    );
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "states": ["full:positive_y"] }"#),
        [format!("{FILE}: /states/0: \"full\" takes no side")]
    );
    let sides = "\"negative_x\", \"positive_x\", \"negative_y\", \"positive_y\", \"negative_z\", \"positive_z\"";
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "states": ["stair:positive_q"] }"#),
        [format!(
            "{FILE}: /states/0: \"positive_q\" is not a side (did you mean \"positive_x\"?); the sides are {sides}"
        )]
    );
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "states": [7] }"#),
        [format!(
            "{FILE}: /states/0: expected a shape (a name or boxes), found a number"
        )]
    );
}

#[test]
fn boxes_must_be_inside_the_voxel_and_not_empty() {
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "states": [{ "boxes": [] }] }"#),
        [format!(
            "{FILE}: /states/0/boxes: a shape of boxes has at least one box (use \"empty\" for nothing)"
        )]
    );
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "states": [{ "boxes": [[0,0,0,8,8]] }] }"#),
        [format!(
            "{FILE}: /states/0/boxes/0: a box is 6 numbers (min x, y, z then max x, y, z), found 5"
        )]
    );
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "states": [{ "boxes": [[0,0,0,8,8,8,8]] }] }"#),
        [format!(
            "{FILE}: /states/0/boxes/0: a box is 6 numbers (min x, y, z then max x, y, z), found 7"
        )]
    );
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "states": [{ "boxes": [[0,0,0,9,8,8]] }] }"#),
        [format!("{FILE}: /states/0/boxes/0/3: 9 is outside 0 to 8")]
    );
    assert_eq!(
        block_errors(
            r#"{ "color": [0,0,0], "states": [{ "boxes": [[4,0,0,4,8,8], [0,5,0,8,2,8]] }] }"#
        ),
        [
            format!(
                "{FILE}: /states/0/boxes/0: the box is empty on x: it starts at 4 and ends at 4"
            ),
            format!(
                "{FILE}: /states/0/boxes/1: the box is empty on y: it starts at 5 and ends at 2"
            ),
        ]
    );
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "states": [{ "box": [] }] }"#)[0],
        format!(
            "{FILE}: /states/0/box: unknown key \"box\" (did you mean \"boxes\"?); the keys here are \"boxes\""
        )
    );
}

#[test]
fn state_colours_need_at_least_one_colour_each_inside_0_to_1() {
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "state_colors": [] }"#),
        [format!(
            "{FILE}: /state_colors: give at least one colour, or leave \"state_colors\" out"
        )]
    );
    assert_eq!(
        block_errors(r#"{ "color": [0,0,0], "state_colors": [[0,0,0], [2,0,0]] }"#),
        [format!("{FILE}: /state_colors/1/0: 2 is outside 0 to 1")]
    );
}

#[test]
fn every_problem_in_a_file_is_reported_not_just_the_first() {
    let errors = block_errors(
        r#"{ "color": [9, 0, 0], "sight_through": "yes", "states": ["slab"],
            "properties": { "test:glow": -1 } }"#,
    );
    assert_eq!(errors.len(), 4, "{errors:#?}");
    assert!(errors[0].contains("/color/0: 9 is outside 0 to 1"));
    assert!(errors[1].contains("/sight_through: expected true or false, found text"));
    assert!(errors[2].contains("/states/0: \"slab\" is not a shape"));
    assert!(errors[3].contains("/properties/test:glow: -1 is outside 0 to 255"));
}

// ---- patches ----

const PATCH_FILE: &str = "my_pack/patches/dim.json";

fn patch(text: &str) -> Result<Patch, Vec<String>> {
    read_patch(PATCH_FILE, text).map_err(messages)
}

fn patch_errors(text: &str) -> Vec<String> {
    patch(text).expect_err("the patch should be refused")
}

#[test]
fn a_patch_may_set_light_through_and_the_mode_and_test_them_and_take_the_mode_away_but_not_the_flags()
 {
    let patch = patch(
        r#"{ "target": "base_blocks:lamp", "operations": [
            { "op": "set", "path": "light_through", "value": true },
            { "op": "test", "path": "light_through", "value": false },
            { "op": "set", "path": "transparency", "value": "glass" },
            { "op": "test", "path": "transparency", "value": "opaque" },
            { "op": "remove", "path": "transparency" } ] }"#,
    )
    .unwrap();
    let places: Vec<(&str, Place)> = patch
        .operations
        .iter()
        .map(|operation| match operation {
            Operation::Set { place, .. } => ("set", place.clone()),
            Operation::Test { place, .. } => ("test", place.clone()),
            Operation::Remove { place } => ("remove", place.clone()),
            Operation::AddState { .. } => ("add", Place::Color),
        })
        .collect();
    assert_eq!(
        places,
        [
            ("set", Place::LightThrough),
            ("test", Place::LightThrough),
            ("set", Place::Transparency),
            ("test", Place::Transparency),
            ("remove", Place::Transparency),
        ]
    );
    assert!(matches!(
        &patch.operations[2],
        Operation::Set {
            value: PatchValue::Mode(Transparency::Glass),
            ..
        }
    ));
    for flag in ["sight_through", "light_through"] {
        assert_eq!(
            patch_errors(&format!(
                r#"{{ "target": "a:b", "operations": [{{ "op": "remove", "path": "{flag}" }}] }}"#
            )),
            [format!(
                "{PATCH_FILE}: /operations/0/path: a block always has its colour and its sight-through and light-through flags, so they cannot be removed (use \"set\")"
            )]
        );
    }
    assert_eq!(
        patch_errors(
            r#"{ "target": "a:b", "operations": [{ "op": "set", "path": "transparency", "value": "frosted" }] }"#
        ),
        [format!(
            "{PATCH_FILE}: /operations/0/value: \"frosted\" is not a transparency mode; the modes are \"opaque\", \"leaves\", \"glass\""
        )]
    );
}

#[test]
fn a_patch_is_a_target_and_operations_that_say_what_they_change() {
    let patch = patch(
        r#"{ "target": "base_blocks:lamp", "optional": true, "operations": [
            { "op": "test", "property": "base_blocks:emission", "value": 14 },
            { "op": "set",  "property": "base_blocks:emission", "value": 9 },
            { "op": "set",  "property": "base_blocks:passable", "state": 1, "value": true },
            { "op": "set",  "property": "engine:axis_face", "value": "up" },
            { "op": "remove", "property": "base_blocks:flammability" },
            { "op": "set",  "path": "color", "value": [1.0, 0.5, 0.25] },
            { "op": "test", "path": "sight_through", "value": false },
            { "op": "set",  "path": "state_colors", "value": [[0, 0, 0], [1, 1, 1]] },
            { "op": "remove", "path": "state_colors" },
            { "op": "set",  "path": "face_colors", "value": { "side": [0, 1, 0], "top": [1, 0, 0] } },
            { "op": "test", "path": "face_colors", "value": { "negative_z": [0, 0, 1] } },
            { "op": "remove", "path": "face_colors" },
            { "op": "set",  "path": "texture", "value": "my_pack:oak" },
            { "op": "test", "path": "texture", "value": "base_blocks:stone" },
            { "op": "remove", "path": "texture" },
            { "op": "add", "shape": "half:positive_y" } ] }"#,
    )
    .unwrap();
    assert_eq!(patch.target, id("base_blocks:lamp"));
    assert!(patch.optional);
    let property = |name: &str, state: Option<u32>| Place::Property {
        key: id(name),
        state,
    };
    assert_eq!(
        patch.operations,
        [
            Operation::Test {
                place: property("base_blocks:emission", None),
                value: PatchValue::Property(PropertyValue::Integer(14))
            },
            Operation::Set {
                place: property("base_blocks:emission", None),
                value: PatchValue::Property(PropertyValue::Integer(9))
            },
            Operation::Set {
                place: property("base_blocks:passable", Some(1)),
                value: PatchValue::Property(PropertyValue::Bool(true))
            },
            Operation::Set {
                place: property("engine:axis_face", None),
                value: PatchValue::Property(PropertyValue::Word("up".to_owned()))
            },
            Operation::Remove {
                place: property("base_blocks:flammability", None)
            },
            Operation::Set {
                place: Place::Color,
                value: PatchValue::Color([1.0, 0.5, 0.25])
            },
            Operation::Test {
                place: Place::SightThrough,
                value: PatchValue::Flag(false)
            },
            Operation::Set {
                place: Place::StateColors,
                value: PatchValue::Colors(vec![[0.0; 3], [1.0; 3]])
            },
            Operation::Remove {
                place: Place::StateColors
            },
            Operation::Set {
                place: Place::FaceColors,
                value: PatchValue::FaceColors([
                    Some([0.0, 1.0, 0.0]),
                    Some([0.0, 1.0, 0.0]),
                    None,
                    Some([1.0, 0.0, 0.0]),
                    Some([0.0, 1.0, 0.0]),
                    Some([0.0, 1.0, 0.0]),
                ])
            },
            Operation::Test {
                place: Place::FaceColors,
                value: PatchValue::FaceColors([
                    None,
                    None,
                    None,
                    None,
                    Some([0.0, 0.0, 1.0]),
                    None
                ])
            },
            Operation::Remove {
                place: Place::FaceColors
            },
            Operation::Set {
                place: Place::Texture,
                value: PatchValue::Texture(id("my_pack:oak"))
            },
            Operation::Test {
                place: Place::Texture,
                value: PatchValue::Texture(id("base_blocks:stone"))
            },
            Operation::Remove {
                place: Place::Texture
            },
            Operation::AddState {
                shape: Box::new(Shape::half(FaceDirection::PositiveY))
            },
        ]
    );
    let plain = read_patch(
        PATCH_FILE,
        r#"{ "target": "a:b", "operations": [{ "op": "remove", "property": "a:c" }] }"#,
    )
    .unwrap();
    assert!(
        !plain.optional,
        "a patch is required unless it says otherwise"
    );
}

#[test]
fn a_texture_in_a_patch_is_written_with_its_namespace() {
    let f = PATCH_FILE;
    let wrap = |operation: &str| format!(r#"{{ "target": "a:b", "operations": [{operation}] }}"#);
    assert_eq!(
        patch_errors(&wrap(
            r#"{ "op": "set", "path": "texture", "value": "oak" }"#
        )),
        [format!(
            "{f}: /operations/0/value: a texture in a patch is written with its namespace, such as \"my_pack:oak\", and \"oak\" has none"
        )]
    );
    assert_eq!(
        patch_errors(&wrap(r#"{ "op": "set", "path": "texture", "value": 5 }"#)),
        [format!(
            "{f}: /operations/0/value: expected text, found a number"
        )]
    );
    let errors = patch_errors(&wrap(
        r#"{ "op": "set", "path": "texture", "value": "Oak:Stone" }"#,
    ));
    assert!(
        errors[0].starts_with(&format!(
            "{f}: /operations/0/value: \"Oak:Stone\" is not the name of a picture: "
        )),
        "{errors:?}"
    );
}

#[test]
fn a_patch_that_is_wrong_says_what_and_where() {
    let f = PATCH_FILE;
    let wrap = |operation: &str| format!(r#"{{ "target": "a:b", "operations": [{operation}] }}"#);
    assert_eq!(
        patch_errors(&wrap(r#"{ "op": "sett", "property": "a:c", "value": 1 }"#)),
        [format!(
            "{f}: /operations/0/op: \"sett\" is not an operation (did you mean \"set\"?); the operations are \"test\", \"set\", \"remove\", \"add\""
        )]
    );
    assert_eq!(
        patch_errors(&wrap(r#"{ "op": "set", "property": "a:c" }"#)),
        [format!("{f}: /operations/0: missing \"value\"")]
    );
    assert_eq!(
        patch_errors(&wrap(
            r#"{ "op": "set", "property": "a:c", "path": "color", "value": 1 }"#
        )),
        [format!(
            "{f}: /operations/0: give \"property\" or \"path\", not both"
        )]
    );
    assert_eq!(
        patch_errors(&wrap(r#"{ "op": "set", "value": 1 }"#)),
        [format!(
            "{f}: /operations/0: say what to change with \"property\" or \"path\""
        )]
    );
    assert_eq!(
        patch_errors(&wrap(
            r#"{ "op": "set", "path": "colour", "value": [0,0,0] }"#
        )),
        [format!(
            "{f}: /operations/0/path: \"colour\" is not a field of a block (did you mean \"color\"?); the fields are \"color\", \"sight_through\", \"light_through\", \"transparency\", \"state_colors\", \"face_colors\", \"texture\""
        )]
    );
    assert_eq!(
        patch_errors(&wrap(r#"{ "op": "remove", "path": "color" }"#)),
        [format!(
            "{f}: /operations/0/path: a block always has its colour and its sight-through and light-through flags, so they cannot be removed (use \"set\")"
        )]
    );
    assert_eq!(
        patch_errors(&wrap(
            r#"{ "op": "set", "path": "color", "state": 1, "value": [0,0,0] }"#
        )),
        [format!(
            "{f}: /operations/0/state: \"state\" goes with \"property\" (a field belongs to the whole block)"
        )]
    );
    assert_eq!(
        patch_errors(&wrap(
            r#"{ "op": "set", "path": "color", "value": [0, 2, 0] }"#
        )),
        [format!("{f}: /operations/0/value/1: 2 is outside 0 to 1")]
    );
    // (Text is a face's word, so the reader takes it; applying the patch refuses it for a property that is not a face.)
    assert_eq!(
        patch_errors(&wrap(r#"{ "op": "set", "property": "a:c", "value": [1] }"#)),
        [format!(
            "{f}: /operations/0/value: a property is true or false, a whole number or a face's word, found a list"
        )]
    );
    assert_eq!(
        patch_errors(&wrap(r#"{ "op": "add" }"#)),
        [format!("{f}: /operations/0: missing \"shape\"")]
    );
    assert_eq!(
        patch_errors(&wrap(r#"{ "op": "add", "shape": "thin" }"#)).len(),
        1
    );
    assert_eq!(
        patch_errors(&wrap(r#"{ "op": "add", "state": "full" }"#)),
        [
            format!(
                "{f}: /operations/0/state: unknown key \"state\" (did you mean \"shape\"?); the keys here are \"op\", \"shape\""
            ),
            format!("{f}: /operations/0: missing \"shape\""),
        ]
    );
}

#[test]
fn a_patch_needs_a_target_that_is_a_block_name_and_some_operations() {
    let f = PATCH_FILE;
    assert_eq!(
        patch_errors(r#"{ "operations": [{ "op": "add", "shape": "full" }] }"#),
        [format!("{f}: missing \"target\"")]
    );
    let bad = patch_errors(
        r#"{ "target": "NoColon", "operations": [{ "op": "add", "shape": "full" }] }"#,
    );
    assert_eq!(bad.len(), 1);
    assert!(bad[0].starts_with(&format!("{f}: /target: \"NoColon\" is not a block name: ")));
    assert_eq!(
        patch_errors(r#"{ "target": "a:b", "operations": [] }"#),
        [format!(
            "{f}: /operations: a patch has at least one operation"
        )]
    );
    assert_eq!(
        patch_errors(r#"{ "target": "a:b" }"#),
        [format!("{f}: missing \"operations\"")]
    );
    assert_eq!(
        patch_errors(
            r#"{ "target": "a:b", "optional": "yes", "operations": [{ "op": "add", "shape": "full" }] }"#
        ),
        [format!(
            "{f}: /optional: expected true or false, found text"
        )]
    );
}

#[test]
fn a_schema_name_at_the_top_of_any_file_is_allowed_and_means_nothing_to_the_block() {
    let text = r#"{ "$schema": "../../block.schema.json", "color": [0, 0, 0] }"#;
    assert_eq!(block(text).unwrap().1, BlockDefinition::cube([0.0; 3]));
    assert!(
        read_manifest(
            "p/pack.json",
            r#"{ "$schema": "../pack.schema.json", "id": "p" }"#
        )
        .is_ok()
    );
    assert!(
        read_patch(
            PATCH_FILE,
            r#"{ "$schema": "x", "target": "a:b", "operations": [{ "op": "add", "shape": "full" }] }"#
        )
        .is_ok()
    );
    // Only at the top: inside a shape it is an unknown key like any other.
    let inner = block_errors(
        r#"{ "color": [0,0,0], "states": [{ "$schema": "x", "boxes": [[0,0,0,8,8,8]] }] }"#,
    );
    assert_eq!(inner.len(), 1);
    assert!(
        inner[0].contains("/states/0/$schema: unknown key"),
        "{inner:?}"
    );
}

#[test]
fn whole_numbers_may_be_written_with_a_decimal_point_and_others_are_refused() {
    let with = |value: &str| {
        format!(r#"{{ "color": [0,0,0], "properties": {{ "test:glow": {value} }} }}"#)
    };
    let (_, written_plain) = block(&with("7")).unwrap();
    let (_, written_float) = block(&with("7.0")).unwrap();
    assert_eq!(written_plain, written_float);
    assert_eq!(
        block_errors(&with("7.5")),
        [format!(
            "{FILE}: /properties/test:glow: expected a whole number, found 7.5"
        )]
    );
}

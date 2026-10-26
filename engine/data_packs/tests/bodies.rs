//! Bodies: files of a pack that put a planet (or anything with a terrain generator) in a game's world: where it is, what makes its ground, how big it is and how
//! heavy. What good ones become and exactly what is said about each kind of bad one.

mod common;

use std::sync::Arc;

use common::{families, id, levels};
use data_packs::{Body, BodyContext, FamilyData, Weight, read_body, read_family_parameters};
use namespaced_id::NamespacedId;
use serde_json::json;

const FILE: &str = "my_pack/bodies/moon.json";

fn body(text: &str) -> Result<(NamespacedId, Body), Vec<String>> {
    let families = families();
    let levels = levels();
    read_body(
        FILE,
        "my_pack",
        "moon",
        text,
        &BodyContext {
            families: &families,
            levels: &levels,
        },
    )
    .map_err(|errors| errors.0.iter().map(ToString::to_string).collect())
}

fn errors(text: &str) -> Vec<String> {
    body(text).expect_err("the file should be refused")
}

const MOON: &str = r#"{
    "level": "test_game:space",
    "position": [400000, 12.5, -3],
    "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 300, "seed": 99 } },
    "extent_bricks": 12,
    "surface_gravity": 1.6
}"#;

#[test]
fn a_body_has_a_level_a_place_a_terrain_a_size_and_a_weight() {
    let (identifier, moon) = body(MOON).unwrap();
    // The name is the pack's and the file's.
    assert_eq!(identifier, id("my_pack:moon"));
    assert_eq!(moon.level, id("test_game:space"));
    assert_eq!(moon.position, [400_000.0, 12.5, -3.0]);
    assert_eq!(moon.family, id("test_game:ball_v1"));
    assert_eq!(moon.parameters, json!({ "radius": 300, "seed": 99 }));
    assert_eq!(moon.extent_bricks, 12);
    assert_eq!(moon.weight, Weight::SurfaceGravity(1.6));
}

#[test]
fn a_position_may_be_as_far_as_the_limit_and_is_three_numbers_not_more() {
    let with = |from: &str, to: &str| MOON.replace(from, to);
    let (_, far) = body(&with("[400000, 12.5, -3]", "[1e15, -1e15, 0]")).unwrap();
    assert_eq!(far.position, [1.0e15, -1.0e15, 0.0]);
    assert_eq!(
        errors(&with("[400000, 12.5, -3]", "[1, 2, 3, 4]")),
        [format!(
            "{FILE}: /position: a position is 3 numbers (x, y, z), found 4"
        )]
    );
    assert_eq!(
        errors(&with("[400000, 12.5, -3]", "\"here\"")),
        [format!("{FILE}: /position: expected a list, found text")]
    );
}

#[test]
fn a_body_that_leaves_out_its_place_is_at_the_origin_and_may_weigh_what_it_says() {
    let (_, moon) = body(
        r#"{ "$schema": "../../body.schema.json", "level": "test_game:ground",
            "terrain": { "family": "test_game:ball_v1", "parameters": { "radius": 5 } },
            "extent_bricks": 1, "mass": 6.07e20 }"#,
    )
    .unwrap();
    assert_eq!(moon.position, [0.0; 3]);
    assert_eq!(moon.weight, Weight::Mass(6.07e20));
}

#[test]
fn a_body_that_is_wrong_says_what_and_where() {
    let with = |from: &str, to: &str| MOON.replace(from, to);
    let file = FILE;
    // Something missing.
    assert_eq!(
        errors(r#"{}"#),
        [
            format!("{file}: missing \"level\""),
            format!("{file}: missing \"terrain\""),
            format!("{file}: missing \"extent_bricks\""),
            format!(
                "{file}: give how heavy it is as \"mass\" (kilograms) or as \"surface_gravity\" (metres a second a second), one of them"
            ),
        ]
    );
    // A key that is not one, with the nearest that is.
    assert_eq!(
        errors(&with("\"extent_bricks\"", "\"extent_brick\"")),
        [
            format!(
                "{file}: /extent_brick: unknown key \"extent_brick\" (did you mean \"extent_bricks\"?); the keys here are \"level\", \"position\", \"terrain\", \"extent_bricks\", \"mass\", \"surface_gravity\""
            ),
            format!("{file}: missing \"extent_bricks\""),
        ]
    );
    // The level.
    assert_eq!(
        errors(&with("test_game:space", "test_game:nowhere")),
        [format!(
            "{file}: /level: the game has no level \"test_game:nowhere\"; its levels are \"test_game:space\", \"test_game:ground\""
        )]
    );
    let found = errors(&with("test_game:space", "Not A Name"));
    assert_eq!(found.len(), 1);
    assert!(
        found[0].starts_with(&format!(
            "{file}: /level: \"Not A Name\" is not a level name: "
        )),
        "{found:?}"
    );
    // The place.
    assert_eq!(
        errors(&with("[400000, 12.5, -3]", "[1, 2]")),
        [format!(
            "{file}: /position: a position is 3 numbers (x, y, z), found 2"
        )]
    );
    assert_eq!(
        errors(&with("[400000, 12.5, -3]", "[1, \"up\", 3]")),
        [format!(
            "{file}: /position/1: expected a number, found text"
        )]
    );
    assert_eq!(
        errors(&with("400000", "1e30")),
        [format!(
            "{file}: /position/0: 1e+30 is outside -1000000000000000 to 1000000000000000"
        )]
    );
    // The size.
    assert_eq!(
        errors(&with("\"extent_bricks\": 12", "\"extent_bricks\": 0")),
        [format!("{file}: /extent_bricks: 0 is outside 1 to 1000000")]
    );
    assert_eq!(
        errors(&with("\"extent_bricks\": 12", "\"extent_bricks\": 2.5")),
        [format!(
            "{file}: /extent_bricks: expected a whole number, found 2.5"
        )]
    );
    // The weight.
    assert_eq!(
        errors(&with("\"surface_gravity\": 1.6", "\"surface_gravity\": 0")),
        [format!(
            "{file}: /surface_gravity: expected a number above 0, found 0"
        )]
    );
    assert_eq!(
        errors(&with("\"surface_gravity\": 1.6", "\"mass\": -5")),
        [format!(
            "{file}: /mass: expected a number above 0, found -5"
        )]
    );
    assert_eq!(
        errors(&with("\"surface_gravity\": 1.6", "\"mass\": 1e39")),
        [format!(
            "{file}: /mass: 1e+39 is more than a body can weigh (3.4e38 kilograms)"
        )]
    );
    assert_eq!(
        errors(&with(
            "\"surface_gravity\": 1.6",
            "\"surface_gravity\": 1.6, \"mass\": 5"
        )),
        [format!(
            "{file}: give how heavy it is as \"mass\" (kilograms) or as \"surface_gravity\" (metres a second a second), one of them"
        )]
    );
    // The terrain.
    assert_eq!(
        errors(&with("test_game:ball_v1", "test_game:cube_v1")),
        [format!(
            "{file}: /terrain/family: no family \"test_game:cube_v1\" is loaded; the families are \"test_game:ball_v1\""
        )]
    );
    assert_eq!(
        errors(&with("\"terrain\": {", "\"ground\": {")),
        [
            format!(
                "{file}: /ground: unknown key \"ground\"; the keys here are \"level\", \"position\", \"terrain\", \"extent_bricks\", \"mass\", \"surface_gravity\""
            ),
            format!("{file}: missing \"terrain\""),
        ]
    );
    assert_eq!(
        errors(&with("\"family\"", "\"kind\"")),
        [
            format!(
                "{file}: /terrain/kind: unknown key \"kind\"; the keys here are \"family\", \"parameters\""
            ),
            format!("{file}: /terrain: missing \"family\""),
        ]
    );
}

#[test]
fn the_parameters_are_read_by_the_family_and_its_complaints_say_where_in_the_file_they_are() {
    let with = |from: &str, to: &str| MOON.replace(from, to);
    assert_eq!(
        errors(&with("\"radius\": 300", "\"radius\": -4")),
        [format!(
            "{FILE}: /terrain/parameters/radius: a ball has a radius above 0"
        )]
    );
    // Every complaint, not the first.
    assert_eq!(
        errors(&with(
            "{ \"radius\": 300, \"seed\": 99 }",
            "{ \"radius\": 0, \"seed\": \"later\", \"colour\": 1 }"
        )),
        [
            format!("{FILE}: /terrain/parameters/colour: unknown key"),
            format!("{FILE}: /terrain/parameters/radius: a ball has a radius above 0"),
            format!("{FILE}: /terrain/parameters/seed: a seed is a whole number or \"world\""),
        ]
    );
    // A complaint about the object as a whole is at the object.
    assert_eq!(
        errors(&with("{ \"radius\": 300, \"seed\": 99 }", "{}")),
        [format!("{FILE}: /terrain/parameters: missing \"radius\"")]
    );
    assert_eq!(
        errors(&with("{ \"radius\": 300, \"seed\": 99 }", "5")),
        [format!("{FILE}: /terrain/parameters: expected an object")]
    );
    // And a body with no parameters at all is told so (the family may have defaults for every one, but the file says it is giving none).
    assert_eq!(
        errors(&with(
            ", \"parameters\": { \"radius\": 300, \"seed\": 99 }",
            ""
        )),
        [format!("{FILE}: /terrain: missing \"parameters\"")]
    );
}

#[test]
fn the_name_of_the_file_is_the_name_of_the_body_and_must_be_a_name() {
    let families = families();
    let levels = levels();
    let context = BodyContext {
        families: &families,
        levels: &levels,
    };
    let found = read_body(
        "my_pack/bodies/Moon.json",
        "my_pack",
        "Moon",
        MOON,
        &context,
    )
    .unwrap_err()
    .to_string();
    assert!(
        found.starts_with("my_pack/bodies/Moon.json: the file name \"Moon\" is not a body name: "),
        "{found}"
    );
}

#[test]
fn what_not_json_at_all_is_one_complaint() {
    let found = errors("{ \"level\": ");
    assert_eq!(found.len(), 1);
    assert!(
        found[0].starts_with(&format!("{FILE}: this is not JSON (line 1")),
        "{found:?}"
    );
}

#[test]
fn the_family_reads_its_parameters_again_for_the_world_a_body_is_made_in() {
    let (_, moon) = body(&MOON.replace("\"seed\": 99", "\"seed\": \"world\"")).unwrap();
    let families = families();
    // "world" is the seed of the world, and the bytes and the radius are the family's.
    let read = read_family_parameters(&families, &moon, 777).unwrap();
    let mut expected = 300.0f64.to_bits().to_le_bytes().to_vec();
    expected.extend_from_slice(&777u64.to_le_bytes());
    assert_eq!(read.bytes, expected);
    assert_eq!(read.surface_radius, 300.0);
    // Another world, another seed; a number is the number.
    assert_ne!(
        read_family_parameters(&families, &moon, 778).unwrap().bytes,
        expected
    );
    let (_, fixed) = body(MOON).unwrap();
    assert_eq!(
        read_family_parameters(&families, &fixed, 1).unwrap().bytes,
        read_family_parameters(&families, &fixed, 2).unwrap().bytes
    );
    // A family of another name is not the family: the one that is named is looked for.
    let other: Vec<(NamespacedId, Arc<dyn FamilyData>)> =
        vec![(id("test_game:other_v1"), Arc::new(common::Ball))];
    assert_eq!(
        read_family_parameters(&other, &moon, 1).unwrap_err(),
        "no family test_game:ball_v1 is loaded"
    );
    // A family that is not there is said so, not a panic.
    let none: Vec<(NamespacedId, Arc<dyn FamilyData>)> = Vec::new();
    assert!(
        read_family_parameters(&none, &moon, 1)
            .unwrap_err()
            .contains("test_game:ball_v1")
    );
}

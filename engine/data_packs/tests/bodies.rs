//! Bodies: files of a pack that put a planet (or anything with a terrain generator) in a game's world: where it is, what makes its ground, how big it is and how
//! heavy. What good ones become and exactly what is said about each kind of bad one.

mod common;

use std::sync::Arc;

use common::{families, id, levels};
use data_packs::{
    Around, Body, BodyContext, FamilyData, OrbitData, SpinData, Weight, read_body,
    read_family_parameters,
};
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

// ---- rails ----

/// The moon with a rail written after its weight.
fn with_rail(rail: &str) -> String {
    MOON.replace(
        "\"extent_bricks\": 12,",
        &format!("\"extent_bricks\": 12, \"rail\": {rail},"),
    )
    .replace("\"position\": [400000, 12.5, -3],", "")
}

#[test]
fn a_body_may_turn_and_may_go_round_a_point_or_another_body_and_says_how_in_seconds_and_degrees() {
    let (_, moon) = body(&with_rail(
        r#"{
            "spin": { "axis": [0, 1, 0], "period": 86400, "angle": 15 },
            "orbit": { "around": [1000, 0, -50], "normal": [0, 0, 1], "radius": 1.5e6, "period": 31557600, "phase": 90 }
        }"#,
    ))
    .unwrap();
    let rail = moon.rail.expect("the body has a rail");
    assert_eq!(
        rail.spin,
        Some(SpinData {
            axis: [0.0, 1.0, 0.0],
            period: 86_400.0,
            angle_degrees: 15.0
        })
    );
    assert_eq!(
        rail.orbit,
        Some(OrbitData {
            around: Around::Point([1000.0, 0.0, -50.0]),
            normal: [0.0, 0.0, 1.0],
            radius: 1.5e6,
            period: 31_557_600.0,
            phase_degrees: 90.0
        })
    );
    // Another body is named as a body is: by the pack and the file.
    let (_, around_a_body) = body(&with_rail(
        r#"{ "orbit": { "around": "my_pack:star", "radius": 100, "period": -50 } }"#,
    ))
    .unwrap();
    let orbit = around_a_body.rail.unwrap().orbit.unwrap();
    assert_eq!(orbit.around, Around::Body(id("my_pack:star")));
    // What is left out: the normal is up, the phase and the angle nothing.
    assert_eq!(orbit.normal, [0.0, 1.0, 0.0]);
    assert_eq!(orbit.phase_degrees, 0.0);
    assert_eq!(orbit.period, -50.0);
    let (_, turning) = body(&with_rail(
        r#"{ "spin": { "axis": [1, 1, 0], "period": 10 } }"#,
    ))
    .unwrap();
    assert_eq!(turning.rail.unwrap().spin.unwrap().angle_degrees, 0.0);
    // A body with no rail has none.
    assert_eq!(body(MOON).unwrap().1.rail, None);
}

#[test]
fn a_body_that_only_turns_keeps_the_place_it_was_given_and_one_that_goes_round_takes_it_from_its_orbit()
 {
    let with_position = |rail: &str| {
        MOON.replace(
            "\"extent_bricks\": 12,",
            &format!("\"extent_bricks\": 12, \"rail\": {rail},"),
        )
    };
    let (_, turning) = body(&with_position(
        r#"{ "spin": { "axis": [0, 1, 0], "period": 10 } }"#,
    ))
    .unwrap();
    assert_eq!(turning.position, [400_000.0, 12.5, -3.0]);
    assert_eq!(
        errors(&with_position(
            r#"{ "orbit": { "around": [0, 0, 0], "radius": 100, "period": 10 } }"#
        )),
        [format!(
            "{FILE}: /position: a body that goes round has its place from its orbit"
        )]
    );
}

#[test]
fn a_rail_that_is_wrong_says_what_and_where() {
    let file = FILE;
    let one = |text: String, path: &str, message: &str| {
        assert_eq!(errors(&text), [format!("{file}: {path}: {message}")]);
    };
    one(
        with_rail("5"),
        "/rail",
        "expected an object, found a number",
    );
    one(
        with_rail("{}"),
        "/rail",
        "a rail has a spin, an orbit or both",
    );
    one(
        with_rail(r#"{ "spinn": {} }"#),
        "/rail/spinn",
        "unknown key \"spinn\" (did you mean \"spin\"?); the keys here are \"spin\", \"orbit\"",
    );
    // The spin.
    let spin = |inside: &str| with_rail(&format!(r#"{{ "spin": {{ {inside} }} }}"#));
    one(spin(r#""period": 10"#), "/rail/spin", "missing \"axis\"");
    one(
        spin(r#""axis": [0, 1, 0]"#),
        "/rail/spin",
        "missing \"period\"",
    );
    one(
        spin(r#""axis": [0, 1], "period": 10"#),
        "/rail/spin/axis",
        "a direction is 3 numbers (x, y, z), found 2",
    );
    one(
        spin(r#""axis": [0, 0, 0], "period": 10"#),
        "/rail/spin/axis",
        "a direction must point somewhere",
    );
    one(
        spin(r#""axis": [0, "up", 0], "period": 10"#),
        "/rail/spin/axis/1",
        "expected a number, found text",
    );
    one(
        spin(r#""axis": [0, 1, 0], "period": 0"#),
        "/rail/spin/period",
        "a period is a number of seconds that is not 0, found 0",
    );
    one(
        spin(r#""axis": [0, 1, 0], "period": "day""#),
        "/rail/spin/period",
        "expected a number, found text",
    );
    one(
        spin(r#""axis": [0, 1, 0], "period": 10, "angle": true"#),
        "/rail/spin/angle",
        "expected a number, found true or false",
    );
    // The orbit.
    let orbit = |inside: &str| with_rail(&format!(r#"{{ "orbit": {{ {inside} }} }}"#));
    one(
        orbit(r#""radius": 100, "period": 10"#),
        "/rail/orbit",
        "missing \"around\"",
    );
    one(
        orbit(r#""around": [0, 0, 0], "period": 10"#),
        "/rail/orbit",
        "missing \"radius\"",
    );
    one(
        orbit(r#""around": [0, 0, 0], "radius": 100"#),
        "/rail/orbit",
        "missing \"period\"",
    );
    one(
        orbit(r#""around": [0, 0], "radius": 100, "period": 10"#),
        "/rail/orbit/around",
        "a centre is a body (\"my_pack:star\") or a point of the level (3 numbers), found a list of 2",
    );
    one(
        orbit(r#""around": [0, 0, 0, 0], "radius": 100, "period": 10"#),
        "/rail/orbit/around",
        "a centre is a body (\"my_pack:star\") or a point of the level (3 numbers), found a list of 4",
    );
    one(
        spin(r#""axis": [0, 1, 0, 0], "period": 10"#),
        "/rail/spin/axis",
        "a direction is 3 numbers (x, y, z), found 4",
    );
    one(
        orbit(r#""around": "star", "radius": 100, "period": 10"#),
        "/rail/orbit/around",
        "\"star\" is not a body name: \"star\" must contain exactly one ':' between the namespace and the name",
    );
    one(
        orbit(r#""around": 5, "radius": 100, "period": 10"#),
        "/rail/orbit/around",
        "a centre is a body (\"my_pack:star\") or a point of the level (3 numbers), found a number",
    );
    one(
        orbit(r#""around": [0, 0, 0], "radius": 0, "period": 10"#),
        "/rail/orbit/radius",
        "expected a number above 0, found 0",
    );
    one(
        orbit(r#""around": [0, 0, 0], "radius": 2e15, "period": 10"#),
        "/rail/orbit/radius",
        "2000000000000000.0 is more than 1000000000000000 voxels",
    );
    one(
        orbit(r#""around": [0, 0, 0], "radius": 100, "period": 0"#),
        "/rail/orbit/period",
        "a period is a number of seconds that is not 0, found 0",
    );
    one(
        orbit(r#""around": [0, 0, 0], "radius": 100, "period": 10, "normal": [0, 0, 0]"#),
        "/rail/orbit/normal",
        "a direction must point somewhere",
    );
    one(
        orbit(r#""around": [0, 0, 0], "radius": 100, "period": 10, "phase": null"#),
        "/rail/orbit/phase",
        "expected a number, found null",
    );
}

#[test]
fn what_a_rail_names_is_not_judged_until_the_world_has_the_bodies_so_a_name_is_only_a_name_here() {
    // (That the body exists, and was made before this one, is for the world that makes them.)
    let (_, moon) = body(&with_rail(
        r#"{ "orbit": { "around": "nobody_at_all:ever", "radius": 100, "period": 10 } }"#,
    ))
    .unwrap();
    assert_eq!(
        moon.rail.unwrap().orbit.unwrap().around,
        Around::Body(id("nobody_at_all:ever"))
    );
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
                "{file}: /extent_brick: unknown key \"extent_brick\" (did you mean \"extent_bricks\"?); the keys here are \"level\", \"position\", \"terrain\", \"extent_bricks\", \"mass\", \"surface_gravity\", \"rail\""
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
                "{file}: /ground: unknown key \"ground\"; the keys here are \"level\", \"position\", \"terrain\", \"extent_bricks\", \"mass\", \"surface_gravity\", \"rail\""
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

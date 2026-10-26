//! How the parameters of the planet family are written in a body file: what a good one becomes (the bytes of a recipe), what every bad one is told, and that the
//! generated schema and the reader agree.

use base_blocks::{planet_family_data, planet_family_identifier};
use data_packs::{FamilyData, FamilyProblem};
use jsonschema::Validator;
use serde_json::{Value, json};
use terrain::PlanetSettings;

fn data() -> std::sync::Arc<dyn FamilyData> {
    planet_family_data()
}

/// The planet of a new world of Cave's, written as a body file writes it.
fn first_planet() -> Value {
    json!({
        "seed": "world", "radius": 63710, "hill_height": 160,
        "largest_hill_wavelength_log2": 10, "hill_octave_count": 9,
        "cave_wavelength_log2": 5, "cave_half_thickness": 0.08
    })
}

fn problems(parameters: &Value) -> Vec<(String, String)> {
    data()
        .read(parameters, 2024)
        .expect_err("the parameters should be refused")
        .into_iter()
        .map(|FamilyProblem { path, message }| (path, message))
        .collect()
}

#[test]
fn the_family_is_the_one_the_plugin_makes_generators_with() {
    // (The data side of a family has the name of its code side, which a world's recipes are made with.)
    assert_eq!(
        base_blocks::family_data()
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>(),
        [planet_family_identifier()]
    );
}

#[test]
fn the_planet_of_a_new_world_written_as_data_is_the_settings_of_the_small_planet() {
    let read = data().read(&first_planet(), 2024).unwrap();
    assert_eq!(
        read.bytes,
        PlanetSettings::one_percent_of_earth(2024).to_bytes()
    );
    assert_eq!(read.surface_radius, 63_710.0);
    // "world" is the seed of the world it is made in.
    let other = data().read(&first_planet(), 7).unwrap();
    assert_eq!(
        other.bytes,
        PlanetSettings::one_percent_of_earth(7).to_bytes()
    );
}

#[test]
fn a_planet_needs_a_seed_and_a_radius_and_is_a_smooth_ball_without_the_rest() {
    let read = data()
        .read(&json!({ "seed": 5, "radius": 100 }), 2024)
        .unwrap();
    assert_eq!(
        read.bytes,
        PlanetSettings {
            seed: 5,
            radius: 100.0,
            hill_height: 0.0,
            largest_hill_wavelength_log2: 10,
            hill_octave_count: 0,
            cave_wavelength_log2: 5,
            cave_half_thickness: 0.0,
        }
        .to_bytes()
    );
    // A number seed is the number, whatever the world's; the greatest one a file can hold is held exactly.
    let exact = data()
        .read(
            &json!({ "seed": 18_446_744_073_709_551_615u64, "radius": 1 }),
            3,
        )
        .unwrap();
    assert_eq!(&exact.bytes[..8], &u64::MAX.to_le_bytes());
}

#[test]
fn what_is_wrong_is_said_for_every_field_and_where() {
    let with = |key: &str, value: Value| {
        let mut parameters = first_planet();
        parameters[key] = value;
        parameters
    };
    let without = |key: &str| {
        let mut parameters = first_planet();
        parameters.as_object_mut().unwrap().remove(key);
        parameters
    };
    let one = |path: &str, message: &str| vec![(path.to_owned(), message.to_owned())];

    assert_eq!(
        problems(&json!(5)),
        one("", "expected an object, found a number")
    );
    assert_eq!(problems(&without("radius")), one("", "missing \"radius\""));
    assert_eq!(problems(&without("seed")), one("", "missing \"seed\""));
    assert_eq!(
        problems(&with("colour", json!(1))),
        one(
            "/colour",
            "unknown key \"colour\"; the keys here are \"seed\", \"radius\", \"hill_height\", \"largest_hill_wavelength_log2\", \"hill_octave_count\", \"cave_wavelength_log2\", \"cave_half_thickness\""
        )
    );
    assert_eq!(
        problems(&with("radius", json!("big"))),
        one("/radius", "expected a number, found text")
    );
    assert_eq!(
        problems(&with("radius", json!(0))),
        one(
            "/radius",
            "the radius of a planet is above nothing and at most 1000000000 voxels, not 0"
        )
    );
    assert_eq!(
        problems(&with("hill_height", json!(-3))),
        one(
            "/hill_height",
            "the height of the hills of a planet is not negative and finite, not -3"
        )
    );
    assert_eq!(
        problems(&with("hill_octave_count", json!(300))),
        one("/hill_octave_count", "300 is outside 0 to 255")
    );
    assert_eq!(
        problems(&with("hill_octave_count", json!(2.5))),
        one("/hill_octave_count", "expected a whole number, found 2.5")
    );
    assert_eq!(
        problems(&with("cave_wavelength_log2", json!(57))),
        one("/cave_wavelength_log2", "57 is outside 0 to 56")
    );
    assert_eq!(
        problems(&with("seed", json!("later"))),
        one("/seed", "a seed is a whole number from 0 up, or \"world\"")
    );
    assert_eq!(
        problems(&with("seed", json!(-1))),
        one("/seed", "a seed is a whole number from 0 up, or \"world\"")
    );
    assert_eq!(
        problems(&with("seed", json!(2.5))),
        one("/seed", "a seed is a whole number from 0 up, or \"world\"")
    );
    // All of them at once, in the order of the file.
    let mut nonsense = first_planet();
    nonsense["radius"] = json!(-1);
    nonsense["cave_half_thickness"] = json!(-1);
    nonsense["extra"] = json!(true);
    let found: Vec<String> = problems(&nonsense)
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    assert_eq!(found, ["/extra", "/radius", "/cave_half_thickness"]);
}

// ---- the schema ----

fn validator() -> Validator {
    jsonschema::validator_for(&data().schema()).expect("the schema is a valid schema")
}

fn agree(parameters: &Value) -> Result<(), String> {
    let (schema_says, reader_says) = (
        validator().is_valid(parameters),
        data().read(parameters, 1).is_ok(),
    );
    if schema_says == reader_says {
        Ok(())
    } else {
        Err(format!(
            "{parameters}: schema {schema_says}, reader {reader_says}"
        ))
    }
}

#[test]
fn the_schema_and_the_reader_agree_about_good_parameters_and_about_the_same_parameters_damaged_in_one_place()
 {
    // The good ones.
    for good in [
        first_planet(),
        json!({ "seed": 0, "radius": 0.5 }),
        json!({ "seed": "world", "radius": 1000000000 }),
        json!({ "seed": 3, "radius": 5, "largest_hill_wavelength_log2": 56, "cave_wavelength_log2": 0, "hill_octave_count": 255 }),
    ] {
        assert!(validator().is_valid(&good), "{good}");
        assert!(data().read(&good, 1).is_ok(), "{good}");
    }
    // Every field of the first planet, given each of these instead.
    let replacements = [
        json!(null),
        json!(true),
        json!("x"),
        json!("world"),
        json!(-1),
        json!(0),
        json!(2.5),
        json!(56),
        json!(57),
        json!(255),
        json!(256),
        json!(1.0e9),
        json!(1.0e10),
        json!([]),
        json!({}),
    ];
    let mut disagreements = Vec::new();
    for key in first_planet().as_object().unwrap().keys() {
        for replacement in &replacements {
            let mut damaged = first_planet();
            damaged[key] = replacement.clone();
            if let Err(message) = agree(&damaged) {
                disagreements.push(message);
            }
        }
    }
    // And a field that is not one, one left out, and the parameters as a whole something else.
    let mut extra = first_planet();
    extra["colour"] = json!(1);
    for damaged in [
        extra,
        json!({}),
        json!({ "radius": 5 }),
        json!({ "seed": 5 }),
        json!([]),
        json!(5),
        json!(null),
    ] {
        if let Err(message) = agree(&damaged) {
            disagreements.push(message);
        }
    }
    assert!(disagreements.is_empty(), "{disagreements:#?}");
}

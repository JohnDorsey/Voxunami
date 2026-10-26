//! The bodies of packs, loaded with their blocks: in what order they come, and that what is wrong with them is said together with what is wrong with the rest.

mod common;

use block_models::{BlockDefinition, PropertySchema};
use common::{families, id, levels};
use data_packs::{BodyContext, Loaded, RawFile, RawPack, Weight, load_packs};
use namespaced_id::NamespacedId;

fn body_text(radius: u32, level: &str) -> String {
    format!(
        r#"{{ "level": "{level}", "terrain": {{ "family": "test_game:ball_v1", "parameters": {{ "radius": {radius} }} }},
            "extent_bricks": 4, "surface_gravity": 2 }}"#
    )
}

fn pack(
    folder: &str,
    depends_on: &[&str],
    blocks: &[(&str, &str)],
    bodies: &[(&str, String)],
) -> RawPack {
    let dependencies = depends_on
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let file = |name: &str, text: &str| RawFile {
        name: name.to_owned(),
        text: text.to_owned(),
    };
    RawPack {
        folder: folder.to_owned(),
        manifest: format!(r#"{{ "id": "{folder}", "depends_on": [{dependencies}] }}"#),
        blocks: blocks.iter().map(|(name, text)| file(name, text)).collect(),
        patches: Vec::new(),
        bodies: bodies.iter().map(|(name, text)| file(name, text)).collect(),
    }
}

fn load(packs: &[RawPack]) -> Result<Loaded, Vec<String>> {
    let families = families();
    let levels = levels();
    load_packs(
        packs,
        &[(id("base:stone"), BlockDefinition::cube([0.5; 3]))]
            as &[(NamespacedId, BlockDefinition)],
        &PropertySchema::new(),
        &BodyContext {
            families: &families,
            levels: &levels,
        },
    )
    .map_err(|errors| errors.0.iter().map(ToString::to_string).collect())
}

fn body_names(loaded: &Loaded) -> Vec<String> {
    loaded
        .bodies
        .iter()
        .map(|(identifier, _)| identifier.to_string())
        .collect()
}

#[test]
fn the_bodies_of_packs_come_in_the_order_the_packs_are_applied_and_by_file_name_in_each() {
    let first = pack(
        "first",
        &[],
        &[],
        &[
            ("b_moon", body_text(30, "test_game:space")),
            ("a_planet", body_text(300, "test_game:space")),
        ],
    );
    let second = pack(
        "second",
        &["first"],
        &[],
        &[("star", body_text(3000, "test_game:ground"))],
    );
    let expected = ["first:a_planet", "first:b_moon", "second:star"];
    // Whichever way the packs are listed, a pack that needs another comes after it.
    assert_eq!(
        body_names(&load(&[first.clone(), second.clone()]).unwrap()),
        expected
    );
    assert_eq!(body_names(&load(&[second, first]).unwrap()), expected);

    let loaded = load(&[pack(
        "first",
        &[],
        &[],
        &[("a_planet", body_text(300, "test_game:space"))],
    )])
    .unwrap();
    let (_, planet) = &loaded.bodies[0];
    assert_eq!(planet.level, id("test_game:space"));
    assert_eq!(planet.weight, Weight::SurfaceGravity(2.0));
}

#[test]
fn a_pack_with_no_bodies_adds_none_and_blocks_are_as_they_were() {
    let loaded = load(&[pack(
        "plain",
        &[],
        &[("marble", r#"{ "color": [1, 1, 1] }"#)],
        &[],
    )])
    .unwrap();
    assert!(loaded.bodies.is_empty());
    assert_eq!(loaded.blocks.len(), 2);
}

#[test]
fn what_is_wrong_with_a_body_is_said_with_what_is_wrong_with_the_blocks_of_every_pack() {
    let found = load(&[
        pack(
            "one",
            &[],
            &[("bad_block", r#"{ "colour": [1, 1, 1] }"#)],
            &[("bad_body", body_text(300, "test_game:nowhere"))],
        ),
        pack(
            "two",
            &[],
            &[],
            &[("fine", body_text(300, "test_game:space"))],
        ),
    ])
    .unwrap_err();
    assert_eq!(found.len(), 3, "{found:#?}");
    assert!(
        found[0].starts_with("one/blocks/bad_block.json: "),
        "{found:#?}"
    );
    assert!(
        found[1].starts_with("one/blocks/bad_block.json: "),
        "{found:#?}"
    );
    assert_eq!(
        found[2],
        "one/bodies/bad_body.json: /level: the game has no level \"test_game:nowhere\"; its levels are \"test_game:space\", \"test_game:ground\""
    );
}

#[test]
fn nothing_is_loaded_when_a_body_is_wrong_and_the_other_packs_are_fine() {
    let found = load(&[pack(
        "one",
        &[],
        &[],
        &[("bad", r#"{ "level": 4 }"#.to_owned())],
    )])
    .unwrap_err();
    assert!(
        found
            .iter()
            .all(|line| line.starts_with("one/bodies/bad.json: ")),
        "{found:#?}"
    );
}

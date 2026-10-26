//! Loading packs on top of a game's blocks: order, adding, patching, and what is refused.

use block_models::{BlockDefinition, PropertyDeclaration, PropertyKind, PropertySchema, Shape};
use data_packs::{BodyContext, Loaded, RawFile, RawPack, load_packs};
use namespaced_id::NamespacedId;
use proptest::prelude::*;
use voxel_space::FaceDirection;

fn id(text: &str) -> NamespacedId {
    text.parse().unwrap()
}

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
            doc: String::new(),
        })
        .unwrap();
    schema
        .declare(PropertyDeclaration {
            id: id("test:sticky"),
            kind: PropertyKind::Bool,
            default: 0,
            doc: String::new(),
        })
        .unwrap();
    schema
}

/// The game's own blocks: a stone and a lamp that glows.
fn base() -> Vec<(NamespacedId, BlockDefinition)> {
    vec![
        (id("base:stone"), BlockDefinition::cube([0.5; 3])),
        (
            id("base:lamp"),
            BlockDefinition::cube([1.0, 0.9, 0.6]).with_property_value(id("test:glow"), 14),
        ),
    ]
}

fn pack(
    folder: &str,
    depends_on: &[&str],
    blocks: &[(&str, &str)],
    patches: &[(&str, &str)],
) -> RawPack {
    let dependencies = depends_on
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let files = |list: &[(&str, &str)]| {
        list.iter()
            .map(|(name, text)| RawFile {
                name: (*name).to_owned(),
                text: (*text).to_owned(),
            })
            .collect()
    };
    RawPack {
        folder: folder.to_owned(),
        manifest: format!(r#"{{ "id": "{folder}", "depends_on": [{dependencies}] }}"#),
        blocks: files(blocks),
        patches: files(patches),
        bodies: Vec::new(),
    }
}

fn load(packs: &[RawPack]) -> Result<Loaded, Vec<String>> {
    load_packs(packs, &base(), &schema(), &BodyContext::NONE)
        .map_err(|errors| errors.0.iter().map(ToString::to_string).collect())
}

fn names(loaded: &Loaded) -> Vec<String> {
    loaded
        .blocks
        .iter()
        .map(|(identifier, _)| identifier.to_string())
        .collect()
}

fn definition<'a>(loaded: &'a Loaded, name: &str) -> &'a BlockDefinition {
    &loaded
        .blocks
        .iter()
        .find(|(identifier, _)| identifier.as_str() == name)
        .unwrap_or_else(|| panic!("no block {name}"))
        .1
}

const RED: &str = r#"{ "color": [1, 0, 0] }"#;
const BLUE: &str = r#"{ "color": [0, 0, 1] }"#;

fn one_patch(depends_on: &[&str], patch: &str) -> Vec<RawPack> {
    vec![pack("my_pack", depends_on, &[], &[("change", patch)])]
}

fn patch_of(target: &str, optional: bool, operations: &str) -> String {
    format!(r#"{{ "target": "{target}", "optional": {optional}, "operations": [{operations}] }}"#)
}

fn refusal(packs: &[RawPack]) -> Vec<String> {
    load(packs).expect_err("the packs should be refused")
}

// ---- adding blocks ----

#[test]
fn no_packs_leave_the_games_blocks_as_they_were() {
    let loaded = load(&[]).unwrap();
    assert_eq!(loaded.blocks, base());
    assert!(loaded.warnings.is_empty());
}

#[test]
fn a_packs_blocks_follow_the_games_in_the_order_of_their_file_names() {
    let loaded = load(&[pack("my_pack", &[], &[("zinc", BLUE), ("amber", RED)], &[])]).unwrap();
    assert_eq!(
        names(&loaded),
        ["base:stone", "base:lamp", "my_pack:amber", "my_pack:zinc"]
    );
    assert_eq!(definition(&loaded, "my_pack:amber").color, [1.0, 0.0, 0.0]);
}

#[test]
fn a_block_that_already_exists_cannot_be_added() {
    let errors = refusal(&[
        pack("first", &[], &[("clash", RED)], &[]),
        // The same name in one pack twice cannot happen (files have unique names); two packs cannot share a
        // namespace either, so the clash is with a block of the game.
        pack("base", &[], &[("stone", RED)], &[]),
    ]);
    assert_eq!(
        errors,
        ["base/blocks/stone.json: the block base:stone already exists"]
    );
}

// ---- order ----

#[test]
fn packs_load_after_what_they_depend_on_and_otherwise_by_id() {
    let independent = [
        pack("zeta", &[], &[("one", RED)], &[]),
        pack("alpha", &[], &[("one", RED)], &[]),
    ];
    assert_eq!(
        names(&load(&independent).unwrap())[2..],
        ["alpha:one", "zeta:one"]
    );
    // `alpha` waits for `zeta`, so `zeta` goes first although its id is later.
    let dependent = [
        pack("alpha", &["zeta"], &[("one", RED)], &[]),
        pack("zeta", &[], &[("one", RED)], &[]),
    ];
    assert_eq!(
        names(&load(&dependent).unwrap())[2..],
        ["zeta:one", "alpha:one"]
    );
}

fn three_packs() -> Vec<RawPack> {
    vec![
        pack(
            "mod_a",
            &["base"],
            &[("one", RED), ("two", BLUE)],
            &[(
                "a_patch",
                &patch_of(
                    "base:lamp",
                    false,
                    r#"{ "op": "set", "property": "test:glow", "value": 20 }"#,
                ),
            )],
        ),
        pack(
            "mod_b",
            &["mod_a"],
            &[("one", BLUE)],
            &[
                (
                    "b_patch",
                    &patch_of(
                        "base:lamp",
                        false,
                        r#"{ "op": "test", "property": "test:glow", "value": 20 },
                           { "op": "set", "property": "test:glow", "value": 30 }"#,
                    ),
                ),
                (
                    "b_other",
                    &patch_of(
                        "mod_a:one",
                        false,
                        r#"{ "op": "set", "path": "color", "value": [0.2, 0.2, 0.2] }"#,
                    ),
                ),
            ],
        ),
        pack("mod_c", &[], &[("one", RED)], &[]),
    ]
}

proptest! {
    #[test]
    fn the_order_packs_and_their_files_are_listed_in_changes_nothing(
        packs in Just(three_packs()).prop_shuffle(),
        file_order in any::<[bool; 3]>(),
    ) {
        let mut packs = packs;
        for (pack, reverse) in packs.iter_mut().zip(file_order) {
            if reverse {
                pack.blocks.reverse();
                pack.patches.reverse();
            }
        }
        let expected = load(&three_packs()).unwrap();
        prop_assert_eq!(load(&packs).unwrap(), expected);
    }
}

#[test]
fn a_patch_sees_what_the_packs_before_it_did() {
    // `three_packs`: mod_b's test only passes after mod_a's patch.
    let loaded = load(&three_packs()).unwrap();
    assert_eq!(
        definition(&loaded, "base:lamp").property_value(&id("test:glow"), None, 0),
        30
    );
    assert_eq!(definition(&loaded, "mod_a:one").color, [0.2, 0.2, 0.2]);
}

#[test]
fn a_dependency_that_is_missing_or_a_cycle_is_refused() {
    assert_eq!(
        refusal(&[pack("my_pack", &["nowhere"], &[], &[])]),
        [
            "my_pack/pack.json: /depends_on/0: \"nowhere\" is not a pack here and the game has no blocks of that name"
        ]
    );
    let cycle = refusal(&[
        pack("one", &["two"], &[], &[]),
        pack("two", &["three"], &[], &[]),
        pack("three", &["two"], &[], &[]),
        pack("free", &[], &[], &[]),
    ]);
    let message =
        "these packs wait for each other, so none can load first: \"one\", \"three\", \"two\"";
    assert_eq!(
        cycle,
        [
            format!("one/pack.json: /depends_on: {message}"),
            format!("three/pack.json: /depends_on: {message}"),
            format!("two/pack.json: /depends_on: {message}"),
        ]
    );
}

#[test]
fn the_folder_must_be_called_what_the_pack_says() {
    let mut wrong = pack("folder_name", &[], &[], &[]);
    wrong.manifest = r#"{ "id": "other_name" }"#.to_owned();
    assert_eq!(
        refusal(&[wrong]),
        [
            "folder_name/pack.json: /id: the folder is called \"folder_name\" but pack.json says \"other_name\"; they must be the same"
        ]
    );
}

// ---- patching ----

#[test]
fn patches_change_properties_fields_and_states_and_nothing_else() {
    let operations = r#"
        { "op": "test", "property": "test:glow", "value": 14 },
        { "op": "set", "property": "test:glow", "value": 9 },
        { "op": "set", "property": "test:sticky", "value": true },
        { "op": "set", "path": "color", "value": [0.1, 0.2, 0.3] },
        { "op": "set", "path": "see_through", "value": true },
        { "op": "set", "path": "state_colors", "value": [[0, 0, 0], [1, 1, 1]] },
        { "op": "add", "shape": "half:negative_y" },
        { "op": "set", "property": "test:glow", "state": 1, "value": 77 }"#;
    let loaded = load(&one_patch(
        &["base"],
        &patch_of("base:lamp", false, operations),
    ))
    .unwrap();
    let expected = BlockDefinition::cube([0.1, 0.2, 0.3])
        .with_see_through(true)
        .with_shapes(vec![Shape::full(), Shape::half(FaceDirection::NegativeY)])
        .with_state_colors(vec![[0.0; 3], [1.0; 3]])
        .with_property_value(id("test:glow"), 9)
        .with_property_value(id("test:sticky"), 1)
        .with_state_property_value(1, id("test:glow"), 77);
    assert_eq!(definition(&loaded, "base:lamp"), &expected);
    assert_eq!(definition(&loaded, "base:stone"), &base()[0].1);
    assert!(loaded.warnings.is_empty());
}

#[test]
fn removing_takes_a_property_or_the_state_colours_away() {
    let loaded = load(&[pack(
        "my_pack",
        &["base"],
        &[("tinted", r#"{ "color": [0,0,0], "state_colors": [[1,0,0]], "properties": { "test:glow": 5 } }"#)],
        &[(
            "strip",
            &patch_of(
                "my_pack:tinted",
                false,
                r#"{ "op": "remove", "property": "test:glow" }, { "op": "remove", "path": "state_colors" }"#,
            ),
        )],
    )])
    .unwrap();
    assert_eq!(
        definition(&loaded, "my_pack:tinted"),
        &BlockDefinition::cube([0.0; 3])
    );
}

#[test]
fn tags_that_two_packs_both_set_agree() {
    let set_sticky = |folder: &str| {
        pack(
            folder,
            &["base"],
            &[],
            &[(
                "stick",
                &patch_of(
                    "base:stone",
                    false,
                    r#"{ "op": "set", "property": "test:sticky", "value": true }"#,
                ),
            )],
        )
    };
    let loaded = load(&[set_sticky("one"), set_sticky("two")]).unwrap();
    assert_eq!(
        definition(&loaded, "base:stone").property_value(&id("test:sticky"), None, 0),
        1
    );
}

#[test]
fn a_patch_can_only_touch_blocks_of_a_namespace_the_pack_depends_on() {
    let patch = patch_of(
        "base:lamp",
        false,
        r#"{ "op": "set", "property": "test:glow", "value": 1 }"#,
    );
    assert_eq!(
        refusal(&one_patch(&[], &patch)),
        [
            "my_pack/patches/change.json: /target: base:lamp is in \"base\", which this pack does not depend on (add it to \"depends_on\" in pack.json)"
        ]
    );
    // Depending on a pack that depends on `base` is enough: it names what it needs, and those needs carry over.
    let packs = [
        pack("lower", &["base"], &[], &[]),
        pack("upper", &["lower"], &[], &[("change", &patch)]),
    ];
    assert!(load(&packs).is_ok());
}

#[test]
fn a_patch_to_a_block_that_does_not_exist_is_refused_unless_it_is_optional() {
    let operations = r#"{ "op": "set", "property": "test:glow", "value": 1 }"#;
    assert_eq!(
        refusal(&one_patch(
            &["base"],
            &patch_of("base:ghost", false, operations)
        )),
        [
            "my_pack/patches/change.json: /target: there is no block base:ghost (is its pack or plugin loaded?)"
        ]
    );
    let loaded = load(&one_patch(
        &["base"],
        &patch_of("base:ghost", true, operations),
    ))
    .unwrap();
    assert_eq!(loaded.blocks, base());
    assert_eq!(
        loaded.warnings,
        [
            "my_pack/patches/change.json: skipped: there is no block base:ghost (is its pack or plugin loaded?)"
        ]
    );
}

#[test]
fn a_property_nobody_declared_is_refused_with_a_hint_unless_the_patch_is_optional() {
    let operations = r#"{ "op": "set", "property": "test:glo", "value": 1 }"#;
    let declared = "\"engine:axis_face\", \"engine:density\", \"engine:docking_face\", \"engine:front_face\", \"engine:mark_both_ends\", \"engine:mark_color\", \"engine:marked_face\", \"engine:placeable_on_moving_grids\", \"test:glow\", \"test:sticky\"";
    assert_eq!(
        refusal(&one_patch(
            &["base"],
            &patch_of("base:lamp", false, operations)
        )),
        [format!(
            "my_pack/patches/change.json: /operations/0/property: the property \"test:glo\" is not declared (did you mean \"test:glow\"?); the properties are {declared}"
        )]
    );
    let loaded = load(&one_patch(
        &["base"],
        &patch_of("base:lamp", true, operations),
    ))
    .unwrap();
    assert_eq!(loaded.blocks, base());
    assert_eq!(
        loaded.warnings,
        [
            "my_pack/patches/change.json: skipped: the property test:glo is not declared here (an optional patch)"
        ]
    );
}

#[test]
fn a_skipped_patch_changes_nothing_not_even_what_came_before_the_property_it_skipped_on() {
    let operations = r#"{ "op": "set", "property": "test:glow", "value": 99 },
                        { "op": "set", "property": "other:thing", "value": 1 }"#;
    let loaded = load(&one_patch(
        &["base"],
        &patch_of("base:lamp", true, operations),
    ))
    .unwrap();
    assert_eq!(loaded.blocks, base(), "the first operation was not kept");
    assert_eq!(loaded.warnings.len(), 1);
}

#[test]
fn a_test_that_fails_refuses_the_patch_even_if_it_is_optional() {
    for optional in [false, true] {
        let errors = refusal(&one_patch(
            &["base"],
            &patch_of(
                "base:lamp",
                optional,
                r#"{ "op": "test", "property": "test:glow", "value": 15 }"#,
            ),
        ));
        assert_eq!(
            errors,
            [
                "my_pack/patches/change.json: /operations/0: the test failed: test:glow is 14, not 15"
            ]
        );
    }
    let errors = refusal(&one_patch(
        &["base"],
        &patch_of(
            "base:lamp",
            false,
            r#"{ "op": "test", "property": "test:sticky", "value": true }"#,
        ),
    ));
    assert_eq!(
        errors,
        [
            "my_pack/patches/change.json: /operations/0: the test failed: test:sticky is false, not true"
        ]
    );
    let errors = refusal(&one_patch(
        &["base"],
        &patch_of(
            "base:lamp",
            false,
            r#"{ "op": "test", "path": "see_through", "value": true },
               { "op": "test", "path": "color", "value": [0, 0, 0] },
               { "op": "test", "path": "state_colors", "value": [[0, 0, 0]] }"#,
        ),
    ));
    assert_eq!(errors.len(), 3, "{errors:#?}");
    assert!(errors[0].ends_with("the test failed: see_through is false, not true"));
    assert!(errors[1].contains("the test failed: the colour is"));
    assert!(errors[2].contains("the test failed: the state colours are []"));
}

#[test]
fn values_and_states_that_do_not_fit_are_refused_where_they_are_written() {
    let f = "my_pack/patches/change.json";
    let one = |operation: &str| {
        refusal(&one_patch(
            &["base"],
            &patch_of("base:lamp", false, operation),
        ))
    };
    assert_eq!(
        one(r#"{ "op": "set", "property": "test:glow", "value": 300 }"#),
        [format!("{f}: /operations/0/value: 300 is outside 0 to 255")]
    );
    assert_eq!(
        one(r#"{ "op": "set", "property": "test:glow", "value": true }"#),
        [format!(
            "{f}: /operations/0/value: expected a whole number, found true or false"
        )]
    );
    assert_eq!(
        one(r#"{ "op": "set", "property": "test:sticky", "value": 1 }"#),
        [format!(
            "{f}: /operations/0/value: expected true or false, found a number"
        )]
    );
    assert_eq!(
        one(r#"{ "op": "set", "property": "test:glow", "state": 3, "value": 1 }"#),
        [format!(
            "{f}: /operations/0/state: this block has 1 states (0 to 0), not a state 3"
        )]
    );
    assert_eq!(
        one(r#"{ "op": "remove", "property": "test:sticky" }"#),
        [format!(
            "{f}: /operations/0: test:sticky is not set on this block, so there is nothing to remove"
        )]
    );
    assert_eq!(
        one(r#"{ "op": "remove", "path": "state_colors" }"#),
        [format!(
            "{f}: /operations/0: this block has no state colours, so there is nothing to remove"
        )]
    );
}

#[test]
fn a_state_added_earlier_in_a_patch_can_be_written_to_later_in_it() {
    let loaded = load(&one_patch(
        &["base"],
        &patch_of(
            "base:lamp",
            false,
            r#"{ "op": "add", "shape": "empty" },
               { "op": "set", "property": "test:glow", "state": 1, "value": 0 }"#,
        ),
    ))
    .unwrap();
    let lamp = definition(&loaded, "base:lamp");
    assert_eq!(lamp.state_count(), 2);
    assert_eq!(lamp.property_value(&id("test:glow"), Some(1), 14), 0);
}

#[test]
fn every_failing_patch_is_reported_and_a_failing_patch_does_not_hide_the_ones_after_it() {
    let bad = |name: &str, target: &str| {
        (
            name.to_owned(),
            patch_of(
                target,
                false,
                r#"{ "op": "set", "property": "test:glow", "value": 999 }"#,
            ),
        )
    };
    let (first, first_text) = bad("a_first", "base:lamp");
    let (second, second_text) = bad("b_second", "base:stone");
    let errors = refusal(&[pack(
        "my_pack",
        &["base"],
        &[],
        &[(&first, &first_text), (&second, &second_text)],
    )]);
    assert_eq!(
        errors,
        [
            "my_pack/patches/a_first.json: /operations/0/value: 999 is outside 0 to 255",
            "my_pack/patches/b_second.json: /operations/0/value: 999 is outside 0 to 255",
        ]
    );
}

// ---- reading problems across packs ----

#[test]
fn problems_in_every_file_of_every_pack_are_reported_together() {
    let errors = refusal(&[
        pack("one", &[], &[("bad", r#"{ "colour": [0,0,0] }"#)], &[]),
        pack(
            "two",
            &[],
            &[("worse", "not json")],
            &[("patch", r#"{ "target": "a:b" }"#)],
        ),
    ]);
    assert_eq!(errors.len(), 4, "{errors:#?}");
    assert!(errors[0].starts_with("one/blocks/bad.json: /colour: unknown key"));
    assert!(errors[1].starts_with("one/blocks/bad.json: missing \"color\""));
    assert!(errors[2].starts_with("two/blocks/worse.json: this is not JSON"));
    assert_eq!(errors[3], "two/patches/patch.json: missing \"operations\"");
}

#[test]
fn patches_of_one_pack_apply_in_the_order_of_their_file_names() {
    let set = |value: u32| {
        patch_of(
            "base:lamp",
            false,
            &format!(r#"{{ "op": "set", "property": "test:glow", "value": {value} }}"#),
        )
    };
    let (first, second) = (set(1), set(2));
    let (first, second) = (first.as_str(), second.as_str());
    for listing in [
        [("a_first", first), ("b_second", second)],
        [("b_second", second), ("a_first", first)],
    ] {
        let loaded = load(&[pack("my_pack", &["base"], &[], &listing)]).unwrap();
        assert_eq!(
            definition(&loaded, "base:lamp").property_value(&id("test:glow"), None, 0),
            2,
            "the later name wins whatever the listing order"
        );
    }
}

#[test]
fn a_test_of_a_property_a_block_never_set_reads_the_declared_default() {
    // The engine declares a default of 1000 for density; the stone never sets it.
    let loaded = load(&one_patch(
        &["base"],
        &patch_of(
            "base:stone",
            false,
            r#"{ "op": "test", "property": "engine:density", "value": 1000 }"#,
        ),
    ));
    assert!(loaded.is_ok(), "{loaded:?}");
}

#[test]
fn a_patch_that_fails_leaves_no_trace_for_the_patches_after_it() {
    // The first patch changes the lamp and then fails; the second tests that the lamp is as it was.
    let failing = patch_of(
        "base:lamp",
        false,
        r#"{ "op": "set", "property": "test:glow", "value": 50 },
           { "op": "set", "property": "test:glow", "value": 999 }"#,
    );
    let checking = patch_of(
        "base:lamp",
        false,
        r#"{ "op": "test", "property": "test:glow", "value": 14 }"#,
    );
    let errors = refusal(&[pack(
        "my_pack",
        &["base"],
        &[],
        &[("a_fails", &failing), ("b_checks", &checking)],
    )]);
    assert_eq!(
        errors,
        ["my_pack/patches/a_fails.json: /operations/1/value: 999 is outside 0 to 255"]
    );
}

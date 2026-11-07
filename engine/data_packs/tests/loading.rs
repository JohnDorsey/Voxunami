//! Loading packs on top of a game's blocks: order, adding, patching, and what is refused.

use block_models::{BlockDefinition, PropertyDeclaration, PropertyKind, PropertySchema, Shape};
mod common;

use data_packs::{BodyContext, GameTexture, Loaded, RawFile, RawPack, RawTexture, load_packs};
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
        textures: Vec::new(),
        folder: folder.to_owned(),
        manifest: format!(r#"{{ "id": "{folder}", "depends_on": [{dependencies}] }}"#),
        blocks: files(blocks),
        patches: files(patches),
        bodies: Vec::new(),
    }
}

fn load(packs: &[RawPack]) -> Result<Loaded, Vec<String>> {
    load_packs(packs, &base(), &[], &schema(), &BodyContext::NONE)
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

// ---- pictures ----

fn picture(name: &str, size: u32, rgba: [u8; 4]) -> RawTexture {
    RawTexture {
        name: name.to_owned(),
        bytes: common::png_bytes(size, rgba),
    }
}

fn with_pictures(mut pack: RawPack, pictures: Vec<RawTexture>) -> RawPack {
    pack.textures = pictures;
    pack
}

fn load_with_game_pictures(packs: &[RawPack], game: &[GameTexture]) -> Result<Loaded, Vec<String>> {
    load_packs(packs, &base(), game, &schema(), &BodyContext::NONE)
        .map_err(|errors| errors.0.iter().map(ToString::to_string).collect())
}

fn close(found: [f32; 3], wanted: [f32; 3]) {
    for (found, wanted) in found.into_iter().zip(wanted) {
        assert!((found - wanted).abs() < 1e-4, "{found:?} is not {wanted:?}");
    }
}

#[test]
fn a_block_whose_colour_follows_its_pictures_gets_the_average_of_the_top_once_and_the_sides_four_times()
 {
    let pack = with_pictures(
        pack(
            "my_pack",
            &["base"],
            &[("oak", r#"{ "texture": "oak" }"#)],
            &[],
        ),
        vec![
            picture("oak_top", 16, [255, 255, 255, 255]),
            picture("oak_side", 16, [0, 0, 0, 255]),
        ],
    );
    let loaded = load(&[pack]).unwrap();
    let oak = definition(&loaded, "my_pack:oak");
    close(oak.color, [0.4845; 3]);
    assert!(oak.color_from_pictures());
    let pictures = &loaded.textures.pictures[&id("my_pack:oak")];
    assert_eq!(pictures.top, id("my_pack:oak_top"));
    assert_eq!(
        pictures.bottom,
        id("my_pack:oak_top"),
        "no bottom picture: the top's"
    );
    assert_eq!(loaded.textures.tiles.len(), 2);
    // (A white top and black sides on a block that nothing turns: the spec says to say so.)
    assert_eq!(
        loaded.warnings,
        [
            "my_pack/blocks/oak.json: its top picture differs from its other faces, but nothing turns the block (no engine:axis_face or engine:front_face), so its top is always up"
        ]
    );
}

#[test]
fn a_later_pack_replaces_a_picture_by_file_name_and_the_colour_that_follows_it_follows_the_new_picture()
 {
    let first = with_pictures(
        pack(
            "first",
            &["base"],
            &[("oak", r#"{ "texture": "oak" }"#)],
            &[],
        ),
        vec![picture("oak", 16, [255, 255, 255, 255])],
    );
    let retexture = with_pictures(
        pack("retexture", &["first"], &[], &[]),
        vec![picture("first/oak", 16, [0, 0, 0, 255])],
    );
    let loaded = load(&[retexture.clone(), first.clone()]).unwrap();
    close(definition(&loaded, "first:oak").color, [0.0; 3]);
    assert_eq!(
        loaded.warnings,
        ["retexture/textures/first/oak.png: replaces first:oak from first/textures/oak.png"]
    );
    // Without the retexture the block is white; the order the packs are listed in does not matter.
    close(
        definition(&load(&[first]).unwrap(), "first:oak").color,
        [1.0; 3],
    );
    let other_order = load(&[first_clone_for_order(), retexture]).unwrap();
    assert_eq!(other_order.blocks, loaded.blocks);
}

fn first_clone_for_order() -> RawPack {
    with_pictures(
        pack(
            "first",
            &["base"],
            &[("oak", r#"{ "texture": "oak" }"#)],
            &[],
        ),
        vec![picture("oak", 16, [255, 255, 255, 255])],
    )
}

#[test]
fn a_pack_that_does_not_depend_on_another_may_neither_use_nor_replace_its_pictures() {
    let first = with_pictures(
        pack("first", &["base"], &[], &[]),
        vec![picture("oak", 16, [255, 255, 255, 255])],
    );
    let user = pack(
        "user",
        &["base"],
        &[("slab", r#"{ "texture": "first:oak" }"#)],
        &[],
    );
    let thief = with_pictures(
        pack("thief", &["base"], &[], &[]),
        vec![picture("first/oak", 16, [0, 0, 0, 255])],
    );
    assert_eq!(
        refusal(&[first.clone(), user]),
        [
            "user/blocks/slab.json: /texture: \"first:oak\" is a picture of \"first\", which this pack does not depend on (add it to \"depends_on\" in pack.json)"
        ]
    );
    assert_eq!(
        refusal(&[first, thief]),
        [
            "thief/textures/first/oak.png: pictures of \"first\" can be put in by a pack that depends on it (add it to \"depends_on\" in pack.json)"
        ]
    );
}

#[test]
fn a_block_whose_texture_has_no_picture_and_a_picture_that_is_not_one_are_refused_with_the_file() {
    let broken = with_pictures(
        pack(
            "my_pack",
            &["base"],
            &[("oak", r#"{ "texture": "oak" }"#)],
            &[],
        ),
        vec![RawTexture {
            name: "stone".to_owned(),
            bytes: b"not a picture".to_vec(),
        }],
    );
    let errors = refusal(&[broken]);
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors[0].starts_with("my_pack/textures/stone.png: this is not a PNG picture: "));
    assert_eq!(
        errors[1],
        "my_pack/blocks/oak.json: /texture: \"my_pack:oak\" names no picture: there is no my_pack:oak_top and no my_pack:oak"
    );
}

#[test]
fn the_games_own_pictures_are_there_for_a_pack_to_use_and_to_replace_when_it_depends_on_them() {
    let game = [GameTexture {
        id: id("base:stone"),
        file: "plugins/base/textures/stone.png".to_owned(),
        bytes: common::png_bytes(16, [100, 100, 100, 255]),
    }];
    let uses = pack(
        "uses",
        &["base"],
        &[("slab", r#"{ "texture": "base:stone" }"#)],
        &[],
    );
    let loaded = load_with_game_pictures(std::slice::from_ref(&uses), &game).unwrap();
    close(definition(&loaded, "uses:slab").color, [100.0 / 255.0; 3]);
    let repaint = with_pictures(
        pack("repaint", &["base"], &[], &[]),
        vec![picture("base/stone", 16, [200, 200, 200, 255])],
    );
    let loaded = load_with_game_pictures(&[uses, repaint], &game).unwrap();
    close(definition(&loaded, "uses:slab").color, [200.0 / 255.0; 3]);
    assert_eq!(
        loaded.warnings,
        [
            "repaint/textures/base/stone.png: replaces base:stone from plugins/base/textures/stone.png"
        ]
    );
}

#[test]
fn patches_give_a_block_other_pictures_and_the_colour_follows_unless_it_was_chosen() {
    let pack = with_pictures(
        pack(
            "my_pack",
            &["base"],
            &[
                ("followed", r#"{ "texture": "white" }"#),
                (
                    "chosen",
                    r#"{ "texture": "white", "color": [0.5, 0.5, 0.5] }"#,
                ),
            ],
            &[
                (
                    "change",
                    &patch_of(
                        "my_pack:followed",
                        false,
                        r#"{ "op": "test", "path": "texture", "value": "my_pack:white" },
                       { "op": "set", "path": "texture", "value": "my_pack:black" }"#,
                    ),
                ),
                (
                    "change_chosen",
                    &patch_of(
                        "my_pack:chosen",
                        false,
                        r#"{ "op": "set", "path": "texture", "value": "my_pack:black" }"#,
                    ),
                ),
            ],
        ),
        vec![
            picture("white", 16, [255, 255, 255, 255]),
            picture("black", 16, [0, 0, 0, 255]),
        ],
    );
    let loaded = load(&[pack]).unwrap();
    close(definition(&loaded, "my_pack:followed").color, [0.0; 3]);
    close(definition(&loaded, "my_pack:chosen").color, [0.5; 3]);
    assert_eq!(
        loaded.textures.pictures[&id("my_pack:chosen")].top,
        id("my_pack:black")
    );
}

#[test]
fn choosing_a_colour_ends_the_following_and_a_texture_cannot_be_taken_from_a_colour_that_follows_it()
 {
    let make = |operations: &str| {
        with_pictures(
            pack(
                "my_pack",
                &["base"],
                &[("oak", r#"{ "texture": "white" }"#)],
                &[("change", &patch_of("my_pack:oak", false, operations))],
            ),
            vec![
                picture("white", 16, [255, 255, 255, 255]),
                picture("black", 16, [0, 0, 0, 255]),
            ],
        )
    };
    // The colour is chosen, so a later change of the pictures leaves it alone; the texture can then go.
    let loaded = load(&[make(
        r#"{ "op": "set", "path": "color", "value": [0.25, 0.25, 0.25] },
           { "op": "set", "path": "texture", "value": "my_pack:black" },
           { "op": "remove", "path": "texture" }"#,
    )])
    .unwrap();
    let oak = definition(&loaded, "my_pack:oak");
    assert_eq!(
        (oak.color, oak.color_from_pictures(), oak.texture()),
        ([0.25; 3], false, None)
    );
    assert!(!loaded.textures.pictures.contains_key(&id("my_pack:oak")));
    // Taking the texture from a block whose colour is its pictures' leaves it with no colour.
    assert_eq!(
        refusal(&[make(r#"{ "op": "remove", "path": "texture" }"#)]),
        [
            "my_pack/patches/change.json: /operations/0: the colour of this block is the average of its pictures, so give it a color (set \"color\") before taking its texture away"
        ]
    );
    // A block that has none has none to take.
    assert_eq!(
        refusal(&one_patch(
            &["base"],
            &patch_of(
                "base:lamp",
                false,
                r#"{ "op": "remove", "path": "texture" }"#
            )
        )),
        [
            "my_pack/patches/change.json: /operations/0: this block has no texture, so there is nothing to remove"
        ]
    );
}

#[test]
fn a_test_of_a_texture_says_what_the_texture_is() {
    let errors = refusal(&one_patch(
        &["base"],
        &patch_of(
            "base:lamp",
            false,
            r#"{ "op": "test", "path": "texture", "value": "base:stone" }"#,
        ),
    ));
    assert_eq!(
        errors,
        [
            "my_pack/patches/change.json: /operations/0: the test failed: the texture is none, not base:stone"
        ]
    );
}

#[test]
fn pictures_that_nothing_uses_are_said_for_a_pack() {
    let pack = with_pictures(
        pack("my_pack", &["base"], &[], &[]),
        vec![picture("spare", 16, [1, 2, 3, 255])],
    );
    assert_eq!(
        load(&[pack]).unwrap().warnings,
        ["my_pack/textures/spare.png: no block uses this picture"]
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
fn a_patch_may_name_a_face_by_its_word_and_a_failed_test_says_the_words() {
    let set = r#"{ "op": "set", "property": "engine:axis_face", "value": "up" },
                 { "op": "set", "property": "engine:front_face", "value": 2 },
                 { "op": "test", "property": "engine:axis_face", "value": 4 },
                 { "op": "test", "property": "engine:front_face", "value": "east" }"#;
    let loaded = load(&one_patch(&["base"], &patch_of("base:lamp", false, set))).unwrap();
    let lamp = definition(&loaded, "base:lamp");
    assert_eq!(lamp.property_value(&id("engine:axis_face"), None, 0), 4);
    assert_eq!(lamp.property_value(&id("engine:front_face"), None, 0), 2);
    let f = "my_pack/patches/change.json";
    let one = |operation: &str| {
        refusal(&one_patch(
            &["base"],
            &patch_of("base:lamp", false, operation),
        ))
    };
    assert_eq!(
        one(r#"{ "op": "test", "property": "engine:axis_face", "value": "down" }"#),
        [format!(
            "{f}: /operations/0: the test failed: engine:axis_face is none, not down"
        )]
    );
    assert_eq!(
        one(r#"{ "op": "set", "property": "engine:axis_face", "value": "top" }"#),
        [format!(
            "{f}: /operations/0/value: expected a face (\"none\", \"west\", \"east\", \"down\", \"up\", \"north\", \"south\") or a number from 0 to 6, found \"top\""
        )]
    );
    assert_eq!(
        one(r#"{ "op": "set", "property": "engine:axis_face", "value": 9 }"#),
        [format!("{f}: /operations/0/value: 9 is outside 0 to 6")]
    );
    assert_eq!(
        one(r#"{ "op": "set", "property": "test:glow", "value": "up" }"#),
        [format!(
            "{f}: /operations/0/value: expected a whole number, found text"
        )]
    );
    assert_eq!(
        one(r#"{ "op": "set", "property": "test:sticky", "value": "up" }"#),
        [format!(
            "{f}: /operations/0/value: expected true or false, found text"
        )]
    );
}

#[test]
fn a_patch_sets_tests_and_removes_the_colours_of_faces() {
    let set = r#"{ "op": "set", "path": "face_colors", "value": { "top": [1, 0, 0], "positive_x": [0, 0, 1] } }"#;
    let loaded = load(&one_patch(&["base"], &patch_of("base:lamp", false, set))).unwrap();
    assert_eq!(
        definition(&loaded, "base:lamp"),
        &base()[1].1.clone().with_face_colors(&[
            (FaceDirection::PositiveY, [1.0, 0.0, 0.0]),
            (FaceDirection::PositiveX, [0.0, 0.0, 1.0]),
        ])
    );
    // Setting replaces all of them, as for the state colours; testing reads them back; removing takes them all away.
    let loaded = load(&[pack(
        "my_pack",
        &["base"],
        &[("log", r#"{ "color": [0,0,0], "face_colors": { "top": [1,0,0], "bottom": [1,0,0] } }"#)],
        &[(
            "change",
            &patch_of(
                "my_pack:log",
                false,
                r#"{ "op": "test", "path": "face_colors", "value": { "top": [1,0,0], "bottom": [1,0,0] } },
                   { "op": "set", "path": "face_colors", "value": { "side": [0,1,0] } },
                   { "op": "test", "path": "face_colors", "value": { "side": [0,1,0] } },
                   { "op": "remove", "path": "face_colors" }"#,
            ),
        )],
    )])
    .unwrap();
    assert_eq!(
        definition(&loaded, "my_pack:log"),
        &BlockDefinition::cube([0.0; 3])
    );
}

#[test]
fn a_test_of_face_colours_that_fails_and_a_remove_with_none_to_remove_are_refused() {
    let f = "my_pack/patches/change.json";
    let one = |operation: &str| {
        refusal(&one_patch(
            &["base"],
            &patch_of("base:lamp", false, operation),
        ))
    };
    let errors = one(r#"{ "op": "test", "path": "face_colors", "value": { "top": [1, 0, 0] } }"#);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(errors[0].starts_with(&format!(
        "{f}: /operations/0: the test failed: the face colours are [None, None, None, None, None, None], not "
    )));
    assert_eq!(
        one(r#"{ "op": "remove", "path": "face_colors" }"#),
        [format!(
            "{f}: /operations/0: this block has no face colours, so there is nothing to remove"
        )]
    );
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
    let declared = "\"engine:axis_face\", \"engine:density\", \"engine:docking_face\", \"engine:front_face\", \"engine:placeable_on_moving_grids\", \"test:glow\", \"test:sticky\"";
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

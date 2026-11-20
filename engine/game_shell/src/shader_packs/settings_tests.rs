//! The settings a `shaderpack.json` declares (`"settings"`) and the defaults it sets for the engine's (`"engine_settings"`).

use renderer::{SettingKind, SettingValue};

use super::tests::Folder;
use super::*;

const STEPPED: &str = "fn pack_shade(s: Surface) -> vec3<f32> { return s.albedo * floor(s.sun_visible * f32(SETTING_STEPS) + 0.5) / f32(SETTING_STEPS); }";

/// What `read_pack` says of a folder whose `shaderpack.json` is this text after the id and the name.
fn read(extra: &str, lighting: &str) -> Result<ShaderPack, Vec<String>> {
    let folder = Folder::new("settings");
    folder.write(
        "shaderpack.json",
        &format!(r#"{{ "id": "tester:stepped", "name": "Stepped"{extra} }}"#),
    );
    folder.write("lighting.wgsl", lighting);
    read_pack(folder.path())
}

#[test]
fn a_pack_declares_settings_of_each_kind_with_their_ranges_and_defaults() {
    let pack = read(
        r#", "settings": {
            "steps": { "type": "integer", "description": "How many steps.", "default": 3, "minimum": 2, "maximum": 8 },
            "softness": { "type": "float", "default": 0.5, "minimum": 0, "maximum": 2 },
            "outline": { "type": "switch", "default": true }
        }"#,
        STEPPED,
    )
    .unwrap_or_else(|problems| panic!("{problems:?}"));
    let own = pack.own_settings();
    assert_eq!(own.len(), 3);
    let by_name = |name: &str| {
        own.iter()
            .find(|setting| setting.name() == name)
            .unwrap_or_else(|| panic!("{name}"))
    };
    assert_eq!(
        by_name("steps").kind(),
        &SettingKind::Integer {
            default: 3,
            minimum: 2,
            maximum: 8
        }
    );
    assert_eq!(by_name("steps").description(), "How many steps.");
    assert_eq!(
        by_name("softness").kind(),
        &SettingKind::Float {
            default: 0.5,
            minimum: 0.0,
            maximum: 2.0
        }
    );
    assert_eq!(
        by_name("outline").default_value(),
        SettingValue::Switch(true)
    );
}

#[test]
fn a_pack_declares_a_choice_with_its_words_and_one_of_them_as_the_default() {
    let pack = read(
        r#", "settings": {
            "quality": { "type": "choice", "description": "How fine.", "options": ["low", "medium", "high"], "default": "medium" }
        }"#,
        "fn pack_shade(s: Surface) -> vec3<f32> { return s.albedo * select(0.5, 1.0, SETTING_QUALITY == SETTING_QUALITY_HIGH); }",
    )
    .unwrap_or_else(|problems| panic!("{problems:?}"));
    let quality = &pack.own_settings()[0];
    assert_eq!(
        quality.kind(),
        &SettingKind::Choice {
            default: "medium".to_owned(),
            options: vec!["low".to_owned(), "medium".to_owned(), "high".to_owned()],
        }
    );
    assert_eq!(
        quality.default_value(),
        SettingValue::Choice("medium".into())
    );
}

#[test]
fn a_choice_that_is_wrong_is_a_problem_that_names_the_pack_json_and_the_setting() {
    let lighting = renderer::default_lighting();
    for (extra, says) in [
        (
            r#", "settings": { "a": { "type": "choice", "default": "x" } }"#,
            "`options`",
        ),
        (
            r#", "settings": { "a": { "type": "choice", "options": "low", "default": "low" } }"#,
            "list of texts",
        ),
        (
            r#", "settings": { "a": { "type": "choice", "options": ["low", 2], "default": "low" } }"#,
            "list of texts",
        ),
        (
            r#", "settings": { "a": { "type": "choice", "options": ["low", "high"] } }"#,
            "`default`",
        ),
        (
            r#", "settings": { "a": { "type": "choice", "options": ["low", "high"], "default": 1 } }"#,
            "`default`",
        ),
        (
            r#", "settings": { "a": { "type": "choice", "options": ["low", "high"], "default": "ultra" } }"#,
            "one of low, high",
        ),
        (
            r#", "settings": { "a": { "type": "choice", "options": ["low"], "default": "low" } }"#,
            "at least two",
        ),
        (
            r#", "settings": { "a": { "type": "choice", "options": ["low", "low"], "default": "low" } }"#,
            "twice",
        ),
        (
            r#", "settings": { "a": { "type": "choice", "options": ["low", "High"], "default": "low" } }"#,
            "not a word of a choice",
        ),
        (
            r#", "settings": { "a": { "type": "choice", "options": ["low", "high"], "default": "low", "minimum": 0 } }"#,
            "`minimum` of the setting `a` is not for a choice",
        ),
        (
            r#", "settings": { "a": { "type": "integer", "default": 1, "minimum": 0, "maximum": 3, "options": ["x", "y"] } }"#,
            "`options` of the setting `a` is not for a number",
        ),
        (
            r#", "settings": { "a": { "type": "switch", "default": true, "options": ["x", "y"] } }"#,
            "`options` of the setting `a` is not for a switch",
        ),
    ] {
        let problems = read(extra, lighting).expect_err(extra);
        assert_eq!(problems.len(), 1, "{extra}: {problems:?}");
        assert!(
            problems[0].contains("shaderpack.json") && problems[0].contains(says),
            "{extra}: {problems:?}"
        );
    }
}

#[test]
fn a_pack_sets_the_default_of_an_engine_choice_by_its_word() {
    let pack = read(
        r#", "engine_settings": { "relief": "flat", "scene_lift": 1.0 }"#,
        renderer::default_lighting(),
    )
    .unwrap_or_else(|problems| panic!("{problems:?}"));
    assert_eq!(
        pack.engine_defaults().get("relief"),
        Some(&SettingValue::Choice("flat".to_owned()))
    );
    let settings = pack.settings();
    assert_eq!(settings[0].name(), "relief");
    assert_eq!(
        settings[0].default_value(),
        SettingValue::Choice("flat".into())
    );
}

#[test]
fn a_pack_sets_the_defaults_of_the_engines_settings() {
    let pack = read(
        r#", "engine_settings": { "scene_lift": 1.0, "wall_darkness": 0 }"#,
        renderer::default_lighting(),
    )
    .unwrap_or_else(|problems| panic!("{problems:?}"));
    assert_eq!(
        pack.engine_defaults(),
        &renderer::SettingChoices::from([
            ("scene_lift".to_owned(), SettingValue::Float(1.0)),
            ("wall_darkness".to_owned(), SettingValue::Float(0.0)),
        ])
    );
}

#[test]
fn a_setting_or_a_default_that_is_wrong_is_a_problem_that_names_the_pack_json_and_the_setting() {
    let lighting = renderer::default_lighting();
    for (extra, says) in [
        (r#", "settings": [1]"#, "`settings` is an object"),
        (r#", "settings": { "a": 3 }"#, "`a`"),
        (r#", "settings": { "a": { "default": 1 } }"#, "`type`"),
        (
            r#", "settings": { "a": { "type": "color", "default": 1 } }"#,
            "\"color\"",
        ),
        (
            r#", "settings": { "a": { "type": "integer", "default": 1.5, "minimum": 0, "maximum": 2 } }"#,
            "whole number",
        ),
        (
            r#", "settings": { "a": { "type": "integer", "default": 9, "minimum": 0, "maximum": 2 } }"#,
            "`a`",
        ),
        (
            r#", "settings": { "a": { "type": "float", "default": 1, "maximum": 2 } }"#,
            "`minimum`",
        ),
        (
            r#", "settings": { "a": { "type": "float", "default": 1, "minimum": 3, "maximum": 2 } }"#,
            "`a`",
        ),
        (
            r#", "settings": { "a": { "type": "switch", "default": 1 } }"#,
            "`default`",
        ),
        (
            r#", "settings": { "a": { "type": "switch", "default": true, "minimum": 0 } }"#,
            "`minimum`",
        ),
        (
            r#", "settings": { "a": { "type": "switch", "default": true, "colour": 1 } }"#,
            "`colour`",
        ),
        (
            r#", "settings": { "A": { "type": "switch", "default": true } }"#,
            "not the name of a setting",
        ),
        (
            r#", "settings": { "scene_lift": { "type": "switch", "default": true } }"#,
            "scene_lift",
        ),
        (
            r#", "settings": { "a": { "type": "switch", "default": true, "description": 3 } }"#,
            "`description`",
        ),
        (
            r#", "engine_settings": [1]"#,
            "`engine_settings` is an object",
        ),
        (r#", "engine_settings": { "brightness": 1 }"#, "brightness"),
        (r#", "engine_settings": { "scene_lift": 9 }"#, "scene_lift"),
        (
            r#", "engine_settings": { "scene_lift": "high" }"#,
            "scene_lift",
        ),
    ] {
        let problems = read(extra, lighting).expect_err(extra);
        assert_eq!(problems.len(), 1, "{extra}: {problems:?}");
        assert!(
            problems[0].contains("shaderpack.json") && problems[0].contains(says),
            "{extra}: {problems:?}"
        );
    }
}

#[test]
fn the_cel_example_declares_the_steps_the_renderers_tests_assume() {
    let pack = read_pack(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../renderer/example_shader_packs/cel"),
    )
    .unwrap_or_else(|problems| panic!("{problems:?}"));
    assert_eq!(
        pack.own_settings().len(),
        1,
        "the renderer's test helper `example_settings` writes the same one"
    );
    let steps = &pack.own_settings()[0];
    assert_eq!(steps.name(), "steps");
    assert_eq!(
        steps.kind(),
        &SettingKind::Integer {
            default: 3,
            minimum: 2,
            maximum: 8
        }
    );
}

#[test]
fn a_pack_that_reads_a_setting_it_did_not_declare_is_told_where() {
    let problems = read("", STEPPED).expect_err("SETTING_STEPS is not declared");
    assert!(
        problems[0].contains("lighting.wgsl, line 1"),
        "{problems:?}"
    );
}

//! A pack's settings: the ones it declares (read in its lighting as `SETTING_NAME`) and the defaults it sets of the engine's.

use super::*;
use crate::shader_settings::{
    SettingChoices, SettingDeclaration, SettingKind, SettingValue, engine_settings,
};

fn steps() -> SettingDeclaration {
    SettingDeclaration::new(
        "steps",
        "how many steps the sun's light has",
        SettingKind::Integer {
            default: 3,
            minimum: 2,
            maximum: 8,
        },
    )
    .expect("a good declaration")
}

const STEPPED_SUN: &str = "
fn pack_shade(s: Surface) -> vec3<f32> {
    let cosine = max(dot(s.normal, frame.sun_direction.xyz), 0.0) * s.sun_visible;
    let stepped = floor(cosine * f32(SETTING_STEPS) + 0.5) / f32(SETTING_STEPS);
    return s.albedo * (s.side * s.lamp + frame.sun_light.x * stepped);
}
";

#[test]
fn a_pack_reads_its_own_settings_as_constants_and_is_refused_without_declaring_them() {
    let undeclared = ShaderPack::new("test:stepped", "Stepped", STEPPED_SUN)
        .expect_err("SETTING_STEPS is nothing without a declaration");
    assert_eq!(undeclared.problems.len(), 1, "{undeclared}");
    let pack = ShaderPack::new_with_settings("test:stepped", "Stepped", STEPPED_SUN, vec![steps()])
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(pack.own_settings(), [steps()]);
    // A mistake is still found at its line of the pack, with the pack's constants before it.
    let text =
        "fn pack_shade(s: Surface) -> vec3<f32> {\n    return s.albedo * SETTING_STEPS +;\n}\n";
    let error = ShaderPack::new_with_settings("test:x", "X", text, vec![steps()])
        .expect_err("a syntax error");
    assert_eq!(error.problems[0].line, Some(2), "{error}");
}

#[test]
fn a_setting_of_a_pack_may_not_have_the_name_of_an_engine_setting_or_of_another_of_the_pack() {
    let engine_named =
        SettingDeclaration::new("scene_lift", "", SettingKind::Switch { default: false })
            .expect("a good declaration");
    let error = ShaderPack::new_with_settings("test:x", "X", DEFAULT_LIGHTING, vec![engine_named])
        .expect_err("the engine has this name");
    assert!(error.to_string().contains("scene_lift"), "{error}");
    let error =
        ShaderPack::new_with_settings("test:x", "X", DEFAULT_LIGHTING, vec![steps(), steps()])
            .expect_err("two of one name");
    assert!(error.to_string().contains("steps"), "{error}");
}

#[test]
fn a_pack_sets_the_defaults_of_the_engines_settings_within_their_ranges() {
    let pack = ShaderPack::new("test:x", "X", DEFAULT_LIGHTING).expect("a valid pack");
    let defaults = SettingChoices::from([("scene_lift".to_owned(), SettingValue::Float(1.0))]);
    let pack = pack.with_engine_defaults(defaults).expect("in range");
    let lift = pack
        .settings()
        .into_iter()
        .find(|declaration| declaration.name() == "scene_lift")
        .expect("the engine's setting is among the pack's");
    assert_eq!(lift.default_value(), SettingValue::Float(1.0));
    // The range is still the engine's.
    assert!(lift.kind().check(SettingValue::Float(4.0)).is_err());
    for (name, value) in [
        ("scene_lift", SettingValue::Float(9.0)),
        ("brightness", SettingValue::Float(1.0)),
        ("scene_lift", SettingValue::Switch(true)),
    ] {
        let pack = ShaderPack::new("test:x", "X", DEFAULT_LIGHTING).expect("a valid pack");
        let error = pack
            .with_engine_defaults(SettingChoices::from([(name.to_owned(), value)]))
            .expect_err("a bad default");
        assert!(error.contains(name), "{error}");
    }
}

#[test]
fn the_settings_of_a_pack_are_the_engines_with_the_packs_defaults_and_then_its_own() {
    let pack = ShaderPack::new_with_settings("test:stepped", "Stepped", STEPPED_SUN, vec![steps()])
        .expect("a valid pack");
    let names: Vec<String> = pack
        .settings()
        .iter()
        .map(|declaration| declaration.name().to_owned())
        .collect();
    assert_eq!(
        names,
        ["relief_depth", "wall_darkness", "scene_lift", "steps"]
    );
    let plain = ShaderPack::new("test:x", "X", DEFAULT_LIGHTING).expect("a valid pack");
    assert_eq!(plain.settings(), engine_settings());
}

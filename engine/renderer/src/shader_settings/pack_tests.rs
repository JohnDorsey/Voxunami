//! The settings of a pack that has some of its own and sets defaults of the engine's.

use super::*;

fn steps() -> SettingDeclaration {
    SettingDeclaration::new(
        "steps",
        "",
        SettingKind::Integer {
            default: 3,
            minimum: 2,
            maximum: 8,
        },
    )
    .expect("a good declaration")
}

#[test]
fn a_pack_with_settings_of_its_own_has_a_constant_for_each_with_the_prefix_and_its_own_default() {
    let engine_defaults =
        SettingChoices::from([("scene_lift".to_owned(), SettingValue::Float(1.0))]);
    let own = [steps()];
    let none = SettingChoices::new();
    let settings =
        ShaderSettings::resolve_for(&engine_defaults, &own, &none).expect("the defaults");
    assert_eq!(
        settings.prelude(),
        "const RELIEF: i32 = 1;\nconst RELIEF_FLAT: i32 = 0;\nconst RELIEF_PRISMS: i32 = 1;\nconst RELIEF_PARALLAX_OCCLUSION: i32 = 2;\nconst RELIEF_DEPTH: f32 = 0.1;\nconst WALL_DARKNESS: f32 = 0.2;\nconst SCENE_LIFT: f32 = 1.0;\nconst SETTING_STEPS: i32 = 3;\n"
    );
    assert_eq!(settings.scene_lift(), 1.0);
    // What the player chooses is over the pack's defaults, and a choice of the engine's own default is still a choice over the pack's.
    let chosen = SettingChoices::from([
        ("steps".to_owned(), SettingValue::Integer(5)),
        ("scene_lift".to_owned(), SettingValue::Float(1.2)),
    ]);
    let settings =
        ShaderSettings::resolve_for(&engine_defaults, &own, &chosen).expect("choices in range");
    assert!(settings.prelude().contains("const SETTING_STEPS: i32 = 5;"));
    assert!(settings.prelude().contains("const SCENE_LIFT: f32 = 1.2;"));
    let too_many = SettingChoices::from([("steps".to_owned(), SettingValue::Integer(9))]);
    let error =
        ShaderSettings::resolve_for(&engine_defaults, &own, &too_many).expect_err("out of range");
    assert!(
        error.contains("steps") && error.contains("a whole number from 2 to 8"),
        "{error}"
    );
    let unknown = SettingChoices::from([("steps".to_owned(), SettingValue::Integer(4))]);
    let error =
        ShaderSettings::resolve_for(&engine_defaults, &[], &unknown).expect_err("not a setting");
    assert!(error.contains("\"steps\" is not a setting"), "{error}");
}

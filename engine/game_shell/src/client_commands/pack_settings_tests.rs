//! `.shadersetting` with the settings of a shader pack: the pack's own, and the engine's with the pack's defaults.

use renderer::{SettingChoices, SettingDeclaration, SettingKind, SettingValue};

use super::tests::run;
use super::*;

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

/// Settings with two packs: `tester:stepped` has a setting of its own and sets the lift's default to 1; `tester:plain` has none.
fn with_packs() -> ClientSettings {
    let mut settings = ClientSettings::new(0);
    let mut engine = renderer::engine_settings();
    engine[2] = engine[2]
        .with_default(SettingValue::Float(1.0))
        .expect("in range");
    let mut stepped = engine;
    stepped.push(steps());
    settings.shader_packs = vec![
        ShaderPackName {
            id: "tester:stepped".to_owned(),
            name: "Stepped".to_owned(),
            settings: stepped,
        },
        ShaderPackName {
            id: "tester:plain".to_owned(),
            name: "Plain".to_owned(),
            settings: renderer::engine_settings(),
        },
    ];
    settings
}

#[test]
fn the_settings_of_the_pack_in_use_are_listed_with_the_packs_own_and_its_defaults() {
    let mut settings = with_packs();
    run(".shader stepped", &mut settings);
    assert_eq!(
        run(".shadersetting", &mut settings),
        "shader settings: relief_depth 0.1, wall_darkness 0.2, scene_lift 1, steps 3; \
         .shadersetting NAME says what one is for, .shadersetting NAME VALUE changes it"
    );
    let reply = run(".shadersetting steps", &mut settings);
    assert!(
        reply.starts_with("steps is 3 (the default)")
            && reply.contains("a whole number from 2 to 8"),
        "{reply}"
    );
    // With another pack, a setting of the first is not a setting.
    run(".shader plain", &mut settings);
    let reply = run(".shadersetting steps 5", &mut settings);
    assert!(
        reply.contains("there is no shader setting called \"steps\""),
        "{reply}"
    );
    assert!(settings.pack_settings.is_empty());
}

#[test]
fn a_value_of_a_packs_own_setting_is_the_packs_and_is_kept_when_another_pack_is_used() {
    let mut settings = with_packs();
    run(".shader stepped", &mut settings);
    assert_eq!(
        run(".shadersetting steps 5", &mut settings),
        "steps is now 5 (the default is 3); .shadersetting steps default goes back"
    );
    assert_eq!(
        settings.pack_settings["tester:stepped"].get("steps"),
        Some(&SettingValue::Integer(5))
    );
    assert!(
        settings.shader_settings.is_empty(),
        "a pack's own setting is not the engine's"
    );
    assert_eq!(
        settings.chosen_settings(),
        SettingChoices::from([("steps".to_owned(), SettingValue::Integer(5))])
    );
    // With the other pack, the choice is not given to the renderer (which would refuse it), and it is there again when the pack is.
    run(".shader plain", &mut settings);
    assert_eq!(settings.chosen_settings(), SettingChoices::new());
    run(".shader stepped", &mut settings);
    assert_eq!(
        settings.chosen_settings(),
        SettingChoices::from([("steps".to_owned(), SettingValue::Integer(5))])
    );
    // A whole number it is, in its range.
    for bad in ["1", "9", "2.5", "many"] {
        let reply = run(&format!(".shadersetting steps {bad}"), &mut settings);
        assert!(
            reply.contains("steps") && reply.contains("a whole number from 2 to 8"),
            "{bad}: {reply}"
        );
    }
    run(".shadersetting steps default", &mut settings);
    assert_eq!(settings.chosen_settings(), SettingChoices::new());
}

#[test]
fn the_default_of_an_engine_setting_is_the_packs_and_a_choice_of_the_players_is_theirs_whatever_the_pack()
 {
    let mut settings = with_packs();
    run(".shader stepped", &mut settings);
    // The pack makes the lift 1: that is the default, and saying so is no choice.
    assert_eq!(
        run(".shadersetting scene_lift 1", &mut settings),
        "scene_lift is 1, the default"
    );
    assert!(settings.shader_settings.is_empty());
    // The engine's own 1.2 is a choice, over the pack's default.
    assert_eq!(
        run(".shadersetting scene_lift 1.2", &mut settings),
        "scene_lift is now 1.2 (the default is 1); .shadersetting scene_lift default goes back"
    );
    assert_eq!(
        settings.shader_settings.get("scene_lift"),
        Some(&SettingValue::Float(1.2))
    );
    // It is the player's: with the other pack, it is the same value (and is the other pack's default, so it is no more than a choice that agrees).
    run(".shader plain", &mut settings);
    assert_eq!(
        settings.chosen_settings(),
        SettingChoices::from([("scene_lift".to_owned(), SettingValue::Float(1.2))])
    );
}

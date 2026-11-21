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
    let lift = engine
        .iter()
        .position(|declaration| declaration.name() == "scene_lift")
        .expect("the engine has a lift");
    engine[lift] = engine[lift]
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
        "shader settings: relief prisms, relief_depth 0.1, wall_darkness 0.2, scene_lift 1, steps 3; \
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
        settings.pack_settings[&Some("tester:stepped".to_owned())].get("steps"),
        Some(&SettingValue::Integer(5))
    );
    assert!(
        !settings.pack_settings.contains_key(&None),
        "a pack's choices are not the game's own shader's"
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
fn the_default_of_an_engine_setting_is_the_packs_and_a_choice_of_the_players_is_for_that_shader_alone()
 {
    let mut settings = with_packs();
    run(".shader stepped", &mut settings);
    // The pack makes the lift 1: that is the default, and saying so is no choice.
    assert_eq!(
        run(".shadersetting scene_lift 1", &mut settings),
        "scene_lift is 1, the default"
    );
    assert!(settings.pack_settings.is_empty());
    // The engine's own 1.2 is a choice, over the pack's default.
    assert_eq!(
        run(".shadersetting scene_lift 1.2", &mut settings),
        "scene_lift is now 1.2 (the default is 1); .shadersetting scene_lift default goes back"
    );
    assert_eq!(
        settings.pack_settings[&Some("tester:stepped".to_owned())].get("scene_lift"),
        Some(&SettingValue::Float(1.2))
    );
    // It is that pack's: with the other pack, and with the game's own shader, it is not chosen (each shader has its own file).
    run(".shader plain", &mut settings);
    assert_eq!(settings.chosen_settings(), SettingChoices::new());
    run(".shader default", &mut settings);
    assert_eq!(settings.chosen_settings(), SettingChoices::new());
    // And what is chosen for the game's own shader is the game's own alone.
    run(".shadersetting relief flat", &mut settings);
    run(".shader stepped", &mut settings);
    assert_eq!(
        settings.chosen_settings(),
        SettingChoices::from([("scene_lift".to_owned(), SettingValue::Float(1.2))])
    );
    run(".shader default", &mut settings);
    assert_eq!(
        settings.chosen_settings(),
        SettingChoices::from([("relief".to_owned(), SettingValue::Choice("flat".to_owned()))])
    );
}

#[test]
fn a_pack_that_draws_the_relief_flat_says_so_in_its_defaults_and_the_player_may_choose_prisms_over_it()
 {
    let mut settings = ClientSettings::new(0);
    let mut engine = renderer::engine_settings();
    engine[0] = engine[0]
        .with_default(SettingValue::Choice("flat".to_owned()))
        .expect("flat is a relief");
    settings.shader_packs = vec![ShaderPackName {
        id: "tester:flat".to_owned(),
        name: "Flat".to_owned(),
        settings: engine,
    }];
    run(".shader flat", &mut settings);
    assert!(
        run(".shadersetting", &mut settings).starts_with("shader settings: relief flat, "),
        "the pack's default is shown as the default"
    );
    assert_eq!(
        run(".shadersetting relief prisms", &mut settings),
        "relief is now prisms (the default is flat); .shadersetting relief default goes back"
    );
    assert_eq!(
        settings.chosen_settings(),
        SettingChoices::from([(
            "relief".to_owned(),
            SettingValue::Choice("prisms".to_owned())
        )])
    );
}

#[test]
fn what_the_commands_choose_is_kept_in_the_files_and_comes_back_in_the_next_session() {
    use crate::shader_pack_settings::SettingsFiles;
    use crate::shader_packs::tests::Folder;
    let folder = Folder::new("commands_to_files");
    let files = || {
        std::collections::BTreeMap::from([
            (None, folder.path().join("default.settings.json")),
            (
                Some("tester:stepped".to_owned()),
                folder.path().join("stepped.settings.json"),
            ),
        ])
    };
    let mut settings = with_packs();
    let declared = std::collections::BTreeMap::from([
        (None, renderer::engine_settings()),
        (
            Some("tester:stepped".to_owned()),
            settings.shader_packs[0].settings.clone(),
        ),
    ]);
    let (mut store, choices, warnings) = SettingsFiles::load(files(), &declared);
    assert!(choices.is_empty() && warnings.is_empty());
    // Nothing is chosen, and nothing is written.
    assert!(store.save_changes(&settings.pack_settings).is_empty());
    assert!(std::fs::read_dir(folder.path()).unwrap().next().is_none());
    // The game's own shader: the relief flat, which is what brings the normal pictures' tilts back.
    run(".shadersetting relief flat", &mut settings);
    assert!(store.save_changes(&settings.pack_settings).is_empty());
    assert_eq!(
        std::fs::read_to_string(folder.path().join("default.settings.json")).unwrap(),
        "{\n  \"relief\": \"flat\"\n}\n"
    );
    assert!(!folder.path().join("stepped.settings.json").exists());
    // A pack has a file of its own, with the engine's settings and its own in it.
    run(".shader stepped", &mut settings);
    run(".shadersetting steps 5", &mut settings);
    run(".shadersetting scene_lift 1.4", &mut settings);
    assert!(store.save_changes(&settings.pack_settings).is_empty());
    assert_eq!(
        std::fs::read_to_string(folder.path().join("stepped.settings.json")).unwrap(),
        "{\n  \"scene_lift\": 1.4,\n  \"steps\": 5\n}\n"
    );
    // The next session reads them back, shader by shader.
    let (mut store, choices, warnings) = SettingsFiles::load(files(), &declared);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(choices, settings.pack_settings);
    let mut next = with_packs();
    next.pack_settings = choices;
    // (The game's own shader is in use to start with: what the renderer is told is its choices alone.)
    assert_eq!(
        next.chosen_settings(),
        SettingChoices::from([("relief".to_owned(), SettingValue::Choice("flat".to_owned()))])
    );
    run(".shader stepped", &mut next);
    assert_eq!(
        next.chosen_settings(),
        SettingChoices::from([
            ("scene_lift".to_owned(), SettingValue::Float(1.4)),
            ("steps".to_owned(), SettingValue::Integer(5)),
        ])
    );
    run(".shader default", &mut next);
    // Going back to the defaults removes the files.
    run(".shadersetting relief default", &mut next);
    run(".shader stepped", &mut next);
    run(".shadersetting steps default", &mut next);
    run(".shadersetting scene_lift default", &mut next);
    assert!(next.pack_settings.is_empty());
    assert!(store.save_changes(&next.pack_settings).is_empty());
    assert!(std::fs::read_dir(folder.path()).unwrap().next().is_none());
}

#[test]
fn a_shader_the_graphics_card_refuses_loses_its_choices_and_nothing_else_does() {
    let mut settings = with_packs();
    run(".shadersetting relief flat", &mut settings);
    run(".shader stepped", &mut settings);
    run(".shadersetting steps 5", &mut settings);
    run(".shader plain", &mut settings);
    run(".shadersetting wall_darkness 0.5", &mut settings);
    settings.forget_the_refused_shader();
    assert_eq!(settings.shader_pack, None);
    assert!(
        settings
            .pack_settings
            .contains_key(&Some("tester:stepped".to_owned())),
        "another pack's choices are kept"
    );
    assert!(
        !settings
            .pack_settings
            .contains_key(&Some("tester:plain".to_owned())),
        "the refused pack's are not"
    );
    assert_eq!(
        settings.chosen_settings(),
        SettingChoices::from([("relief".to_owned(), SettingValue::Choice("flat".to_owned()))])
    );
}

use super::*;
use crate::shader_packs::tests::Folder;

fn declared() -> BTreeMap<PackKey, Vec<SettingDeclaration>> {
    let mut own = renderer::engine_settings();
    own.push(
        SettingDeclaration::new(
            "steps",
            "",
            SettingKind::Integer {
                default: 3,
                minimum: 2,
                maximum: 8,
            },
        )
        .expect("a good declaration"),
    );
    own.push(
        SettingDeclaration::new("outline", "", SettingKind::Switch { default: false })
            .expect("a good declaration"),
    );
    BTreeMap::from([
        (None, renderer::engine_settings()),
        (Some("tester:cel".to_owned()), own),
    ])
}

fn files_in(folder: &Folder) -> BTreeMap<PackKey, PathBuf> {
    BTreeMap::from([
        (None, folder.path().join("default.settings.json")),
        (
            Some("tester:cel".to_owned()),
            folder.path().join("cel.settings.json"),
        ),
    ])
}

fn cel() -> PackKey {
    Some("tester:cel".to_owned())
}

#[test]
fn a_file_is_read_by_the_kind_of_each_setting_it_names() {
    let folder = Folder::new("read");
    folder.write(
        "cel.settings.json",
        r#"{ "relief": "flat", "scene_lift": 1.4, "steps": 5, "outline": true }"#,
    );
    let (_, choices, warnings) = SettingsFiles::load(files_in(&folder), &declared());
    assert_eq!(warnings, Vec::<String>::new());
    assert_eq!(
        choices,
        PackChoices::from([(
            cel(),
            SettingChoices::from([
                ("relief".to_owned(), SettingValue::Choice("flat".to_owned())),
                ("scene_lift".to_owned(), SettingValue::Float(1.4)),
                ("steps".to_owned(), SettingValue::Integer(5)),
                ("outline".to_owned(), SettingValue::Switch(true)),
            ])
        )])
    );
}

#[test]
fn a_file_that_is_not_there_is_no_choices_and_no_warning() {
    let folder = Folder::new("absent");
    let (_, choices, warnings) = SettingsFiles::load(files_in(&folder), &declared());
    assert!(choices.is_empty());
    assert!(warnings.is_empty());
}

#[test]
fn a_name_that_is_no_setting_and_a_value_that_is_not_one_are_warnings_and_left_out_and_the_rest_is_kept()
 {
    let folder = Folder::new("bad_values");
    folder.write(
        "cel.settings.json",
        r#"{ "brightness": 1, "scene_lift": 9, "relief": "bumpy", "steps": 2.5, "outline": "yes", "wall_darkness": 0.5 }"#,
    );
    let (_, choices, warnings) = SettingsFiles::load(files_in(&folder), &declared());
    assert_eq!(
        choices,
        PackChoices::from([(
            cel(),
            SettingChoices::from([("wall_darkness".to_owned(), SettingValue::Float(0.5))])
        )])
    );
    assert_eq!(warnings.len(), 5, "{warnings:#?}");
    for name in ["brightness", "scene_lift", "relief", "steps", "outline"] {
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains(&format!("`{name}`"))
                    && warning.contains("cel.settings.json")),
            "{name}: {warnings:#?}"
        );
    }
}

#[test]
fn a_shader_that_is_not_among_those_declared_has_its_file_left_alone() {
    let folder = Folder::new("not_declared");
    folder.write("cel.settings.json", r#"{ "steps": 5 }"#);
    let only_the_games_own = BTreeMap::from([(None, renderer::engine_settings())]);
    let (_, choices, warnings) = SettingsFiles::load(files_in(&folder), &only_the_games_own);
    assert!(choices.is_empty() && warnings.is_empty());
}

#[test]
fn what_is_chosen_is_written_and_read_back_the_same_and_nothing_chosen_is_no_file() {
    let folder = Folder::new("round_trip");
    let (mut files, _, _) = SettingsFiles::load(files_in(&folder), &declared());
    let chosen = PackChoices::from([
        (
            None,
            SettingChoices::from([("relief".to_owned(), SettingValue::Choice("flat".to_owned()))]),
        ),
        (
            cel(),
            SettingChoices::from([
                ("scene_lift".to_owned(), SettingValue::Float(1.3)),
                ("steps".to_owned(), SettingValue::Integer(5)),
                ("outline".to_owned(), SettingValue::Switch(true)),
                ("relief_depth".to_owned(), SettingValue::Float(0.123_456_79)),
            ]),
        ),
    ]);
    assert_eq!(files.save_changes(&chosen), Vec::<String>::new());
    let text = std::fs::read_to_string(folder.path().join("cel.settings.json")).expect("written");
    // (A number is written as the shortest text it reads back from, not as the `f64` an `f32` widens to: 1.3 and not 1.2999999523162842.)
    assert!(text.contains("\"scene_lift\": 1.3"), "{text}");
    assert!(text.contains("\"relief_depth\": 0.12345679"), "{text}");
    assert!(text.ends_with("}\n"), "{text}");
    let default_file =
        std::fs::read_to_string(folder.path().join("default.settings.json")).expect("written");
    assert!(
        default_file.contains("\"relief\": \"flat\""),
        "{default_file}"
    );
    let (_, read_back, warnings) = SettingsFiles::load(files_in(&folder), &declared());
    assert_eq!(warnings, Vec::<String>::new());
    assert_eq!(read_back, chosen);
    // Everything chosen back to the defaults removes the files.
    assert_eq!(
        files.save_changes(&PackChoices::from([(cel(), SettingChoices::new())])),
        Vec::<String>::new()
    );
    assert!(!folder.path().join("cel.settings.json").exists());
    assert!(!folder.path().join("default.settings.json").exists());
}

#[test]
fn only_a_change_is_written_and_no_file_is_made_for_choices_that_were_there() {
    let folder = Folder::new("only_changes");
    folder.write("cel.settings.json", r#"{ "steps": 5 }"#);
    let (mut files, choices, _) = SettingsFiles::load(files_in(&folder), &declared());
    // The file is as the player has it: if it were written it would be as the text of the game's, and a hand-written one would change. It is not written, for nothing changed.
    folder.write("cel.settings.json", "{ \"steps\":   5 }");
    assert!(files.save_changes(&choices).is_empty());
    assert_eq!(
        std::fs::read_to_string(folder.path().join("cel.settings.json")).unwrap(),
        "{ \"steps\":   5 }"
    );
    assert!(!folder.path().join("default.settings.json").exists());
    // A change is written, and then not again.
    let mut changed = choices.clone();
    changed
        .get_mut(&cel())
        .expect("the choices of cel")
        .insert("steps".to_owned(), SettingValue::Integer(6));
    assert!(files.save_changes(&changed).is_empty());
    let text = std::fs::read_to_string(folder.path().join("cel.settings.json")).unwrap();
    assert!(text.contains("\"steps\": 6"), "{text}");
    folder.write("cel.settings.json", "{ \"steps\": 7 }");
    assert!(files.save_changes(&changed).is_empty());
    assert_eq!(
        std::fs::read_to_string(folder.path().join("cel.settings.json")).unwrap(),
        "{ \"steps\": 7 }",
        "what has been saved is not saved again"
    );
}

#[test]
fn a_file_that_cannot_be_read_is_not_written_over_and_is_said_once() {
    let folder = Folder::new("unreadable");
    folder.write("cel.settings.json", "{ \"steps\": 5, ");
    let (mut files, choices, warnings) = SettingsFiles::load(files_in(&folder), &declared());
    assert!(choices.is_empty());
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("cel.settings.json"), "{warnings:?}");
    let chosen = PackChoices::from([(
        cel(),
        SettingChoices::from([("steps".to_owned(), SettingValue::Integer(4))]),
    )]);
    let said = files.save_changes(&chosen);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("not saved"), "{said:?}");
    assert_eq!(
        std::fs::read_to_string(folder.path().join("cel.settings.json")).unwrap(),
        "{ \"steps\": 5, ",
        "the mistake is still the player's to mend"
    );
    assert!(files.save_changes(&chosen).is_empty(), "said once");
    // A file that is JSON and not an object is the same.
    folder.write("default.settings.json", "[1, 2]");
    let (_, _, warnings) = SettingsFiles::load(files_in(&folder), &declared());
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("default.settings.json")
                && warning.contains("not a JSON object")),
        "{warnings:?}"
    );
}

#[test]
fn a_folder_that_is_not_there_is_made_for_the_file() {
    let folder = Folder::new("made");
    let file = folder
        .path()
        .join("shaderpacks")
        .join("default.settings.json");
    let (mut files, _, _) =
        SettingsFiles::load(BTreeMap::from([(None, file.clone())]), &declared());
    let chosen = PackChoices::from([(
        None,
        SettingChoices::from([("scene_lift".to_owned(), SettingValue::Float(2.0))]),
    )]);
    assert!(files.save_changes(&chosen).is_empty());
    assert!(file.is_file());
    // (No file is left beside it by the move.)
    let names: Vec<_> = std::fs::read_dir(file.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(
        names,
        vec![std::ffi::OsString::from("default.settings.json")]
    );
}

#[test]
fn a_write_that_fails_is_said_and_not_tried_again_in_every_frame() {
    let folder = Folder::new("fails");
    // A folder where the file should be: it cannot be moved over.
    std::fs::create_dir_all(folder.path().join("default.settings.json")).unwrap();
    let (mut files, _, _) = SettingsFiles::load(
        BTreeMap::from([(None, folder.path().join("default.settings.json"))]),
        &declared(),
    );
    let chosen = PackChoices::from([(
        None,
        SettingChoices::from([("scene_lift".to_owned(), SettingValue::Float(2.0))]),
    )]);
    // (Reading a folder as a file is an error on one system and on another: it is a warning when loaded, then.)
    let said = files.save_changes(&chosen);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("default.settings.json"), "{said:?}");
    assert!(files.save_changes(&chosen).is_empty(), "said once");
}

#[test]
fn with_no_files_nothing_is_read_or_written() {
    let mut files = SettingsFiles::none();
    let chosen = PackChoices::from([(
        None,
        SettingChoices::from([("scene_lift".to_owned(), SettingValue::Float(2.0))]),
    )]);
    assert!(files.save_changes(&chosen).is_empty());
}

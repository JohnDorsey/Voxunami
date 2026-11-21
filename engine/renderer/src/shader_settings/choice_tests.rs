//! A setting that is a choice of words (`relief` is `flat` or `prisms`).

use super::*;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|word| (*word).to_owned()).collect()
}

fn quality() -> SettingKind {
    SettingKind::Choice {
        default: "medium".to_owned(),
        options: words(&["low", "medium", "high"]),
    }
}

#[test]
fn a_choice_is_one_of_its_words_and_nothing_else() {
    let kind = quality();
    assert_eq!(kind.default_value(), SettingValue::Choice("medium".into()));
    assert_eq!(kind.describe(), "one of low, medium, high");
    for word in ["low", "medium", "high"] {
        let value = SettingValue::Choice(word.to_owned());
        assert_eq!(kind.check(value.clone()), Ok(value), "{word}");
    }
    for wrong in [
        SettingValue::Choice("ultra".into()),
        SettingValue::Choice("High".into()),
        SettingValue::Choice(String::new()),
        SettingValue::Integer(1),
        SettingValue::Switch(true),
        SettingValue::Float(1.0),
    ] {
        let error = kind.check(wrong.clone()).expect_err(&format!("{wrong:?}"));
        assert!(
            error.contains("one of low, medium, high"),
            "{wrong:?}: {error}"
        );
    }
    // The other kinds do not take a word.
    for other in [
        SettingKind::Switch { default: false },
        SettingKind::Integer {
            default: 1,
            minimum: 0,
            maximum: 3,
        },
    ] {
        assert!(other.check(SettingValue::Choice("low".into())).is_err());
    }
}

#[test]
fn a_players_word_is_read_in_any_case_and_a_word_that_is_not_there_is_refused() {
    let kind = quality();
    assert_eq!(kind.parse("low"), Ok(SettingValue::Choice("low".into())));
    assert_eq!(
        kind.parse(" HIGH "),
        Ok(SettingValue::Choice("high".into()))
    );
    for text in ["", "ultra", "hig", "1", "low high"] {
        let error = kind.parse(text).expect_err(text);
        assert!(
            error.contains("one of low, medium, high"),
            "{text:?}: {error}"
        );
    }
}

#[test]
fn a_choice_is_declared_with_two_words_or_more_each_a_name_each_once_and_its_default_among_them() {
    let declare = |default: &str, list: &[&str]| {
        SettingDeclaration::new(
            "quality",
            "",
            SettingKind::Choice {
                default: default.to_owned(),
                options: words(list),
            },
        )
    };
    assert!(declare("low", &["low", "high"]).is_ok());
    assert!(
        declare("low", &["low"])
            .expect_err("one word")
            .contains("at least two")
    );
    assert!(
        declare("low", &[])
            .expect_err("no word")
            .contains("at least two")
    );
    assert!(
        declare("low", &["low", "Hi gh"])
            .expect_err("a bad word")
            .contains("is not a word of a choice")
    );
    assert!(
        declare("low", &["low", "2high"])
            .expect_err("a word that begins with a digit")
            .contains("is not a word of a choice")
    );
    assert!(
        declare("low", &["low", "high", "low"])
            .expect_err("twice")
            .contains("twice in the choice")
    );
    let error = declare("ultra", &["low", "high"]).expect_err("a default that is not a word");
    assert!(
        error.starts_with("quality: the default ultra") && error.contains("one of low, high"),
        "{error}"
    );
}

#[test]
fn a_choice_is_written_into_a_shader_as_its_place_and_a_constant_for_each_word() {
    let declaration = SettingDeclaration::new("quality", "", quality()).expect("good");
    let none = SettingChoices::new();
    let settings = ShaderSettings::resolve_for(&none, std::slice::from_ref(&declaration), &none)
        .expect("the default");
    let prelude = settings.prelude();
    assert!(
        prelude.ends_with(
            "const SETTING_QUALITY: i32 = 1;\nconst SETTING_QUALITY_LOW: i32 = 0;\nconst SETTING_QUALITY_MEDIUM: i32 = 1;\nconst SETTING_QUALITY_HIGH: i32 = 2;\n"
        ),
        "{prelude}"
    );
    let high = SettingChoices::from([("quality".to_owned(), SettingValue::Choice("high".into()))]);
    let settings = ShaderSettings::resolve_for(&none, std::slice::from_ref(&declaration), &high)
        .expect("a word");
    assert!(
        settings
            .prelude()
            .contains("const SETTING_QUALITY: i32 = 2;"),
        "{}",
        settings.prelude()
    );
    assert_ne!(settings.prelude(), ShaderSettings::defaults().prelude());
    // A word that is not there is refused by name.
    let ultra =
        SettingChoices::from([("quality".to_owned(), SettingValue::Choice("ultra".into()))]);
    let error = ShaderSettings::resolve_for(&none, std::slice::from_ref(&declaration), &ultra)
        .expect_err("not a word");
    assert!(
        error.starts_with("quality: ") && error.contains("one of low, medium, high"),
        "{error}"
    );
    // The names of the constants a setting makes, for a pack to keep apart.
    assert_eq!(
        constants_of(OWN_PREFIX, &declaration),
        vec![
            "SETTING_QUALITY",
            "SETTING_QUALITY_LOW",
            "SETTING_QUALITY_MEDIUM",
            "SETTING_QUALITY_HIGH"
        ]
    );
}

#[test]
fn the_relief_is_an_engine_setting_with_a_word_for_each_relief_and_its_default_is_the_engines() {
    let relief = engine_settings()
        .into_iter()
        .find(|declaration| declaration.name() == "relief")
        .expect("the engine has a relief setting");
    assert_eq!(
        relief.default_value(),
        SettingValue::Choice(DEFAULT_RELIEF.name().to_owned())
    );
    assert_eq!(
        relief.kind().options(),
        Relief::ALL.map(|relief| relief.name().to_owned())
    );
    assert_eq!(ShaderSettings::defaults().relief(), DEFAULT_RELIEF);
    for wanted in Relief::ALL {
        let choice = SettingChoices::from([(
            "relief".to_owned(),
            SettingValue::Choice(wanted.name().to_owned()),
        )]);
        assert_eq!(
            ShaderSettings::resolve(&choice).expect("a relief").relief(),
            wanted
        );
    }
}

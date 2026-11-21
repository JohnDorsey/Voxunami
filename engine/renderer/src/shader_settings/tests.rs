use super::*;

fn float_kind() -> SettingKind {
    SettingKind::Float {
        default: 0.5,
        minimum: 0.0,
        maximum: 2.0,
    }
}

#[test]
fn the_engines_settings_are_the_numbers_the_shaders_had_before_they_were_settings() {
    // These were constants written in the shaders (`RELIEF_DEPTH`, `WALL_DARKNESS`, `SCENE_LIFT`) and a pack's `relief`: a picture drawn with the defaults must be the picture drawn then.
    let defaults: Vec<(String, SettingValue)> = engine_settings()
        .iter()
        .map(|declaration| (declaration.name().to_owned(), declaration.default_value()))
        .collect();
    assert_eq!(
        defaults,
        vec![
            (
                "relief".to_owned(),
                SettingValue::Choice(DEFAULT_RELIEF.name().to_owned())
            ),
            ("relief_depth".to_owned(), SettingValue::Float(0.1)),
            ("wall_darkness".to_owned(), SettingValue::Float(0.2)),
            ("scene_lift".to_owned(), SettingValue::Float(1.2)),
        ]
    );
    assert_eq!(DEFAULT_SCENE_LIFT, 1.2);
    assert_eq!(ShaderSettings::defaults().scene_lift(), 1.2);
}

#[test]
fn the_default_prelude_is_one_constant_for_each_setting() {
    assert_eq!(
        ShaderSettings::defaults().prelude(),
        "const RELIEF: i32 = 1;\nconst RELIEF_FLAT: i32 = 0;\nconst RELIEF_PRISMS: i32 = 1;\nconst RELIEF_PARALLAX_OCCLUSION: i32 = 2;\nconst RELIEF_DEPTH: f32 = 0.1;\nconst WALL_DARKNESS: f32 = 0.2;\nconst SCENE_LIFT: f32 = 1.2;\n"
    );
}

#[test]
fn every_setting_has_its_default_in_its_range_and_says_what_it_is_for() {
    for declaration in engine_settings() {
        assert_eq!(
            declaration.kind().check(declaration.default_value()),
            Ok(declaration.default_value()),
            "{}",
            declaration.name()
        );
        assert!(
            declaration.description().len() > 20,
            "{} says what it is for",
            declaration.name()
        );
    }
}

#[test]
fn a_declaration_is_refused_for_a_bad_name_a_range_that_is_empty_and_a_default_outside_it() {
    for name in [
        "",
        "Depth",
        "2deep",
        "_x",
        "wall-darkness",
        "wall darkness",
        "größe",
    ] {
        let error = SettingDeclaration::new(name, "", float_kind()).expect_err(name);
        assert!(
            error.contains("not the name of a setting"),
            "{name}: {error}"
        );
    }
    for name in ["a", "relief_depth", "x2", "a_b_c"] {
        assert!(
            SettingDeclaration::new(name, "", float_kind()).is_ok(),
            "{name}"
        );
    }
    let backwards = SettingKind::Float {
        default: 1.0,
        minimum: 2.0,
        maximum: 0.0,
    };
    assert!(
        SettingDeclaration::new("x", "", backwards)
            .expect_err("a backwards range")
            .contains("not a range")
    );
    let not_a_number = SettingKind::Float {
        default: 1.0,
        minimum: f32::NAN,
        maximum: 2.0,
    };
    assert!(SettingDeclaration::new("x", "", not_a_number).is_err());
    let outside = SettingKind::Float {
        default: 3.0,
        minimum: 0.0,
        maximum: 2.0,
    };
    let error = SettingDeclaration::new("x", "", outside).expect_err("a default outside");
    assert!(error.starts_with("x: the default 3"), "{error}");
    let empty = SettingKind::Integer {
        default: 1,
        minimum: 5,
        maximum: 4,
    };
    assert!(
        SettingDeclaration::new("x", "", empty)
            .expect_err("an empty range")
            .contains("is empty")
    );
}

#[test]
fn a_value_is_checked_against_the_kind_of_its_setting_and_its_range() {
    let float = float_kind();
    let integer = SettingKind::Integer {
        default: 3,
        minimum: 1,
        maximum: 8,
    };
    let switch = SettingKind::Switch { default: false };
    // The ends are in.
    assert_eq!(
        float.check(SettingValue::Float(0.0)),
        Ok(SettingValue::Float(0.0))
    );
    assert_eq!(
        float.check(SettingValue::Float(2.0)),
        Ok(SettingValue::Float(2.0))
    );
    assert_eq!(
        integer.check(SettingValue::Integer(1)),
        Ok(SettingValue::Integer(1))
    );
    assert_eq!(
        integer.check(SettingValue::Integer(8)),
        Ok(SettingValue::Integer(8))
    );
    assert_eq!(
        switch.check(SettingValue::Switch(true)),
        Ok(SettingValue::Switch(true))
    );
    // Past them, not a number, and the wrong type are out, and the error says what the setting takes.
    for (kind, value, takes) in [
        (&float, SettingValue::Float(2.01), "a number from 0 to 2"),
        (&float, SettingValue::Float(-0.01), "a number from 0 to 2"),
        (
            &float,
            SettingValue::Float(f32::NAN),
            "a number from 0 to 2",
        ),
        (
            &float,
            SettingValue::Float(f32::INFINITY),
            "a number from 0 to 2",
        ),
        (&float, SettingValue::Integer(1), "a number from 0 to 2"),
        (
            &integer,
            SettingValue::Integer(0),
            "a whole number from 1 to 8",
        ),
        (
            &integer,
            SettingValue::Integer(9),
            "a whole number from 1 to 8",
        ),
        (
            &integer,
            SettingValue::Float(3.0),
            "a whole number from 1 to 8",
        ),
        (&switch, SettingValue::Float(1.0), "on or off"),
    ] {
        let error = kind.check(value.clone()).expect_err(&format!("{value:?}"));
        assert!(error.contains(takes), "{value:?}: {error}");
    }
}

#[test]
fn a_players_text_is_read_as_the_kind_says_and_nothing_else_is() {
    let float = float_kind();
    let integer = SettingKind::Integer {
        default: 3,
        minimum: 1,
        maximum: 8,
    };
    let switch = SettingKind::Switch { default: false };
    assert_eq!(float.parse("0.25"), Ok(SettingValue::Float(0.25)));
    assert_eq!(float.parse(" 2 "), Ok(SettingValue::Float(2.0)));
    assert_eq!(float.parse("1e-1"), Ok(SettingValue::Float(0.1)));
    assert_eq!(integer.parse("4"), Ok(SettingValue::Integer(4)));
    assert_eq!(switch.parse("on"), Ok(SettingValue::Switch(true)));
    assert_eq!(switch.parse("true"), Ok(SettingValue::Switch(true)));
    assert_eq!(switch.parse("off"), Ok(SettingValue::Switch(false)));
    assert_eq!(switch.parse("false"), Ok(SettingValue::Switch(false)));
    for (kind, text) in [
        (&float, ""),
        (&float, "abc"),
        (&float, "nan"),
        (&float, "inf"),
        (&float, "3"),
        (&float, "-1"),
        (&integer, "1.5"),
        (&integer, "0"),
        (&integer, "9"),
        (&integer, "on"),
        (&switch, "1"),
        (&switch, "maybe"),
        (&switch, "ON"),
    ] {
        assert!(
            kind.parse(text).is_err(),
            "{text:?} is no value of {kind:?}"
        );
    }
}

#[test]
fn a_value_is_written_into_a_shader_as_a_constant_that_reads_back_as_the_same_value() {
    let written = |value: SettingValue| value.wgsl(&[]);
    assert_eq!(written(SettingValue::Float(1.0)), ("f32", "1.0".to_owned()));
    assert_eq!(written(SettingValue::Float(0.0)), ("f32", "0.0".to_owned()));
    assert_eq!(written(SettingValue::Integer(-3)), ("i32", "-3".to_owned()));
    assert_eq!(
        written(SettingValue::Switch(true)),
        ("bool", "true".to_owned())
    );
    for number in [
        0.1f32,
        1.2,
        0.123_456_79,
        1.0e-7,
        123_456.0,
        3.4e38,
        1.0 / 3.0,
    ] {
        let (_, text) = written(SettingValue::Float(number));
        assert_eq!(text.parse::<f32>(), Ok(number), "{text}");
        assert!(
            text.contains('.') || text.contains('e'),
            "{text} is a float literal of the shader language, not an integer"
        );
    }
}

#[test]
fn what_is_chosen_is_used_the_rest_are_defaults_and_a_bad_choice_is_refused_by_name() {
    let mut choices = SettingChoices::new();
    choices.insert("wall_darkness".to_owned(), SettingValue::Float(0.75));
    let settings = ShaderSettings::resolve(&choices).expect("a good choice");
    assert!(
        settings
            .prelude()
            .contains("const WALL_DARKNESS: f32 = 0.75;")
    );
    assert!(
        settings
            .prelude()
            .contains("const RELIEF_DEPTH: f32 = 0.1;")
    );
    assert_eq!(settings.scene_lift(), 1.2);
    assert_ne!(settings, ShaderSettings::defaults());
    // Choosing the default is the default.
    choices.insert("wall_darkness".to_owned(), SettingValue::Float(0.2));
    assert_eq!(
        ShaderSettings::resolve(&choices),
        Ok(ShaderSettings::defaults())
    );
    // The lift is read back from what was chosen.
    choices.insert("scene_lift".to_owned(), SettingValue::Float(2.0));
    assert_eq!(
        ShaderSettings::resolve(&choices)
            .expect("lift")
            .scene_lift(),
        2.0
    );
    // A name that is not a setting, and a value that is not one of its setting.
    let unknown = SettingChoices::from([("brightness".to_owned(), SettingValue::Float(1.0))]);
    let error = ShaderSettings::resolve(&unknown).expect_err("not a setting");
    assert!(
        error.contains("\"brightness\" is not a setting")
            && error.contains("relief, relief_depth, wall_darkness, scene_lift"),
        "{error}"
    );
    let too_deep = SettingChoices::from([("relief_depth".to_owned(), SettingValue::Float(0.5))]);
    let error = ShaderSettings::resolve(&too_deep).expect_err("out of range");
    assert!(
        error.starts_with("relief_depth: ") && error.contains("a number from 0 to 0.2"),
        "{error}"
    );
    let wrong_type = SettingChoices::from([("scene_lift".to_owned(), SettingValue::Switch(true))]);
    assert!(ShaderSettings::resolve(&wrong_type).is_err());
}

#[test]
fn the_shaders_are_accepted_with_every_setting_at_either_end_of_its_range() {
    use crate::shader_pack::{DEFAULT_LIGHTING, Relief, Shading};
    use crate::voxel_renderer::{SunWalk, voxel_source};
    let heaviest = SunWalk::Voxels {
        shapes: true,
        other_windows: true,
        entities: true,
    };
    // Each end of the numbers' ranges, with each word of the relief.
    for end in 0..2 {
        for word in 0..Relief::ALL.len() {
            let choices: SettingChoices = engine_settings()
                .iter()
                .map(|declaration| match declaration.kind() {
                    SettingKind::Float {
                        minimum, maximum, ..
                    } => (
                        declaration.name().to_owned(),
                        SettingValue::Float(if end == 0 { *minimum } else { *maximum }),
                    ),
                    SettingKind::Choice { options, .. } => (
                        declaration.name().to_owned(),
                        SettingValue::Choice(options[word].clone()),
                    ),
                    other => panic!("the engine has only numbers of taste and words: {other:?}"),
                })
                .collect();
            let settings = ShaderSettings::resolve(&choices).expect("the ends are in their ranges");
            assert_eq!(settings.relief(), Relief::ALL[word]);
            for (with_pictures, walk) in [
                (false, SunWalk::None),
                (true, SunWalk::None),
                (true, heaviest),
            ] {
                let source = voxel_source(
                    with_pictures,
                    walk,
                    &Shading {
                        lighting: DEFAULT_LIGHTING.to_owned(),
                        settings: settings.clone(),
                    },
                );
                let module = naga::front::wgsl::parse_str(&source)
                    .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
                naga::valid::Validator::new(
                    naga::valid::ValidationFlags::all(),
                    naga::valid::Capabilities::all(),
                )
                .validate(&module)
                .unwrap_or_else(|error| panic!("{choices:?}: {error:?}"));
            }
        }
    }
}

//! The settings of how the engine draws (`docs/specs/shader-pack-spec.md`, decision 8): numbers of taste that a player changes by eye, each with a name, a type, a range and a
//! default, and that reach the shaders as constants written into their text. A setting that is a constant costs nothing in a frame (the shader's compiler folds it into the code
//! round it); one that is changed makes the shaders again, which is a pause of a moment and not something done while a player holds a key down.
//!
//! The engine's own are [`engine_settings`]: how the relief of a block's picture is drawn and how deep it is, how dark the foot of a prism's wall is, how much the whole scene is
//! lifted. A name of a setting is lower case with underscores, and the constant is the same in upper case (`scene_lift` is `SCENE_LIFT`).
//!
//! A setting is a number, a whole number, a switch or a **choice** of words (`relief` is `flat` or `prisms`). A choice's constant is the number of the word in its list (from 0) and
//! each word has a constant of its own, so that a shader says what it means: `RELIEF == RELIEF_FLAT`.

use std::collections::BTreeMap;
use std::fmt;

use crate::shader_pack::{DEFAULT_RELIEF, Relief};

/// The value of a setting.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingValue {
    Float(f32),
    Integer(i32),
    Switch(bool),
    /// One of the words of a choice.
    Choice(String),
}

impl fmt::Display for SettingValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingValue::Float(value) => write!(formatter, "{value}"),
            SettingValue::Integer(value) => write!(formatter, "{value}"),
            SettingValue::Switch(true) => write!(formatter, "on"),
            SettingValue::Switch(false) => write!(formatter, "off"),
            SettingValue::Choice(word) => write!(formatter, "{word}"),
        }
    }
}

impl SettingValue {
    /// The shader language's text of the value, as the right of `const NAME: type = ...;` (and the type). A choice is the place of its word in `options`, from 0.
    fn wgsl(&self, options: &[String]) -> (&'static str, String) {
        match self {
            // (`{:?}` of a float always has a point or an exponent: it is a float literal, and it reads back as the same number.)
            SettingValue::Float(value) => ("f32", format!("{value:?}")),
            SettingValue::Integer(value) => ("i32", value.to_string()),
            SettingValue::Switch(value) => ("bool", value.to_string()),
            SettingValue::Choice(word) => (
                "i32",
                options
                    .iter()
                    .position(|option| option == word)
                    .expect("a choice is one of its words (it was checked)")
                    .to_string(),
            ),
        }
    }
}

/// What a setting holds, and what values it may have.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingKind {
    Float {
        default: f32,
        minimum: f32,
        maximum: f32,
    },
    Integer {
        default: i32,
        minimum: i32,
        maximum: i32,
    },
    Switch {
        default: bool,
    },
    /// One of a list of words, each a name like a setting's (lower case letters, digits and underscores, beginning with a letter).
    Choice {
        default: String,
        options: Vec<String>,
    },
}

/// Whether this is a name for a setting or for a word of a choice: lower case letters, digits and underscores, beginning with a letter.
fn is_well_named(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}

impl SettingKind {
    pub fn default_value(&self) -> SettingValue {
        match self {
            SettingKind::Float { default, .. } => SettingValue::Float(*default),
            SettingKind::Integer { default, .. } => SettingValue::Integer(*default),
            SettingKind::Switch { default } => SettingValue::Switch(*default),
            SettingKind::Choice { default, .. } => SettingValue::Choice(default.clone()),
        }
    }

    /// What a player is told the setting takes: `a number from 0 to 1`.
    pub fn describe(&self) -> String {
        match self {
            SettingKind::Float {
                minimum, maximum, ..
            } => format!("a number from {minimum} to {maximum}"),
            SettingKind::Integer {
                minimum, maximum, ..
            } => format!("a whole number from {minimum} to {maximum}"),
            SettingKind::Switch { .. } => "on or off".to_owned(),
            SettingKind::Choice { options, .. } => format!("one of {}", options.join(", ")),
        }
    }

    /// The value, if it is of this kind and in its range (or one of its words); an error says what the setting takes.
    pub fn check(&self, value: SettingValue) -> Result<SettingValue, String> {
        let wrong = || {
            Err(format!(
                "{value} is not a value of this setting, which takes {}",
                self.describe()
            ))
        };
        match (self, &value) {
            (
                SettingKind::Float {
                    minimum, maximum, ..
                },
                SettingValue::Float(number),
            ) if number.is_finite() && (*minimum..=*maximum).contains(number) => Ok(value),
            (
                SettingKind::Integer {
                    minimum, maximum, ..
                },
                SettingValue::Integer(number),
            ) if (*minimum..=*maximum).contains(number) => Ok(value),
            (SettingKind::Switch { .. }, SettingValue::Switch(_)) => Ok(value),
            (SettingKind::Choice { options, .. }, SettingValue::Choice(word))
                if options.contains(word) =>
            {
                Ok(value)
            }
            _ => wrong(),
        }
    }

    /// The value a player's text means for this kind, in its range: a number, a whole number, `on` and `off` (also `true` and `false`), or one of the words of a choice (in
    /// lower case or not).
    pub fn parse(&self, text: &str) -> Result<SettingValue, String> {
        let text = text.trim();
        let value = match self {
            SettingKind::Float { .. } => text.parse::<f32>().ok().map(SettingValue::Float),
            SettingKind::Integer { .. } => text.parse::<i32>().ok().map(SettingValue::Integer),
            SettingKind::Switch { .. } => match text {
                "on" | "true" => Some(SettingValue::Switch(true)),
                "off" | "false" => Some(SettingValue::Switch(false)),
                _ => None,
            },
            SettingKind::Choice { .. } => Some(SettingValue::Choice(text.to_lowercase())),
        };
        match value {
            Some(value) => self.check(value),
            None => Err(format!(
                "{text:?} is not a value of this setting, which takes {}",
                self.describe()
            )),
        }
    }

    /// The words of a choice, and none for the other kinds.
    fn options(&self) -> &[String] {
        match self {
            SettingKind::Choice { options, .. } => options,
            _ => &[],
        }
    }
}

/// A setting: its name, what it is for, and its kind (which has its default and its range).
#[derive(Clone, Debug, PartialEq)]
pub struct SettingDeclaration {
    name: String,
    description: String,
    kind: SettingKind,
}

impl SettingDeclaration {
    /// A declaration; an error says what is wrong with it: a name that is not lower case letters, digits and underscores that begins with a letter, a range that is empty or not
    /// a number, a default outside it.
    pub fn new(name: &str, description: &str, kind: SettingKind) -> Result<Self, String> {
        if !is_well_named(name) {
            return Err(format!(
                "{name:?} is not the name of a setting: lower case letters, digits and underscores, beginning with a letter"
            ));
        }
        match &kind {
            SettingKind::Float {
                minimum, maximum, ..
            } if !(minimum.is_finite() && maximum.is_finite() && minimum <= maximum) => {
                return Err(format!(
                    "{name}: the range {minimum} to {maximum} is not a range of numbers"
                ));
            }
            SettingKind::Integer {
                minimum, maximum, ..
            } if minimum > maximum => {
                return Err(format!("{name}: the range {minimum} to {maximum} is empty"));
            }
            SettingKind::Choice { options, .. } => {
                if options.len() < 2 {
                    return Err(format!(
                        "{name}: a choice has at least two words (it has {})",
                        options.len()
                    ));
                }
                if let Some(word) = options.iter().find(|word| !is_well_named(word)) {
                    return Err(format!(
                        "{name}: {word:?} is not a word of a choice: lower case letters, digits and underscores, beginning with a letter"
                    ));
                }
                if let Some(index) =
                    (1..options.len()).find(|&index| options[..index].contains(&options[index]))
                {
                    return Err(format!(
                        "{name}: the word {:?} is twice in the choice",
                        options[index]
                    ));
                }
            }
            _ => {}
        }
        kind.check(kind.default_value())
            .map_err(|error| format!("{name}: the default {error}"))?;
        Ok(Self {
            name: name.to_owned(),
            description: description.to_owned(),
            kind,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn kind(&self) -> &SettingKind {
        &self.kind
    }

    pub fn default_value(&self) -> SettingValue {
        self.kind.default_value()
    }

    /// The same setting with another default, which must be a value of it.
    pub fn with_default(&self, value: SettingValue) -> Result<Self, String> {
        let kind = match (&self.kind, self.kind.check(value)?) {
            (
                SettingKind::Float {
                    minimum, maximum, ..
                },
                SettingValue::Float(default),
            ) => SettingKind::Float {
                default,
                minimum: *minimum,
                maximum: *maximum,
            },
            (
                SettingKind::Integer {
                    minimum, maximum, ..
                },
                SettingValue::Integer(default),
            ) => SettingKind::Integer {
                default,
                minimum: *minimum,
                maximum: *maximum,
            },
            (SettingKind::Switch { .. }, SettingValue::Switch(default)) => {
                SettingKind::Switch { default }
            }
            (SettingKind::Choice { options, .. }, SettingValue::Choice(default)) => {
                SettingKind::Choice {
                    default,
                    options: options.clone(),
                }
            }
            _ => unreachable!("check accepts only a value of the kind"),
        };
        Ok(Self {
            kind,
            ..self.clone()
        })
    }
}

/// The engine's settings with the defaults a pack sets for them. An error names the one that is not an engine setting, or whose default is not a value of it.
pub(crate) fn engine_settings_with(
    defaults: &SettingChoices,
) -> Result<Vec<SettingDeclaration>, String> {
    let declarations = engine_settings();
    if let Some(unknown) = defaults.keys().find(|name| {
        !declarations
            .iter()
            .any(|declaration| declaration.name() == *name)
    }) {
        return Err(format!(
            "{unknown:?} is not an engine setting; there are {}",
            declarations
                .iter()
                .map(SettingDeclaration::name)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    declarations
        .into_iter()
        .map(|declaration| match defaults.get(declaration.name()) {
            Some(value) => declaration
                .with_default(value.clone())
                .map_err(|error| format!("{}: {error}", declaration.name())),
            None => Ok(declaration),
        })
        .collect()
}

/// What a player (or a pack) has chosen, by the names of settings: the rest are as their declarations say.
pub type SettingChoices = BTreeMap<String, SettingValue>;

/// What a pack's own setting's constant begins with: the pack's `steps` is `SETTING_STEPS`, which no name of the engine's is.
pub(crate) const OWN_PREFIX: &str = "SETTING_";

/// The default of the number of the lift of the whole scene: the colour raised to the power of one over it (1 is no lift).
pub const DEFAULT_SCENE_LIFT: f32 = 1.2;

/// The settings the engine has, whatever the pack: how the relief of a block's picture is drawn (`flat`, where the normal pictures' tilts bump the face, `prisms`, or the smooth
/// `parallax_occlusion`), how deep, in voxels, the relief is (the lowest of a height field whose top is the face; prisms and parallax occlusion are that deep), how much darker the ambient light is at the foot of the wall of the deepest
/// prism than at its top, and how much the whole scene is lifted.
pub fn engine_settings() -> Vec<SettingDeclaration> {
    let float = |name, description, default, minimum, maximum| {
        SettingDeclaration::new(
            name,
            description,
            SettingKind::Float {
                default,
                minimum,
                maximum,
            },
        )
        .expect("the engine's settings are well made")
    };
    vec![
        SettingDeclaration::new(
            "relief",
            "how the relief of block pictures is drawn: flat (the face is flat, and the tilts of a normal picture bump it, so that the sun's light is the cosine of the tilted normal) prisms (each pixel of the picture is a square column as deep as its height, with shaded walls, and its top is flat: the tilts are left out where prisms are drawn) or parallax_occlusion (the heights are a smooth surface the eye's ray is marched through: the usual parallax, lumpy, with no walls, and the tilts still light it)",
            SettingKind::Choice {
                default: DEFAULT_RELIEF.name().to_owned(),
                options: Relief::ALL
                    .iter()
                    .map(|relief| relief.name().to_owned())
                    .collect(),
            },
        )
        .expect("the engine's settings are well made"),
        float(
            "relief_depth",
            "how deep, in voxels, the relief of a block's picture is (the lowest pixel of its height field below the top); more than about 0.15 shows the shader's limit of steps",
            0.1,
            0.0,
            0.2,
        ),
        float(
            "wall_darkness",
            "how much darker the ambient light is at the foot of the wall of the deepest prism than at its top, 0 for none and 1 for black",
            0.2,
            0.0,
            1.0,
        ),
        float(
            "scene_lift",
            "how much brighter the darks of the whole scene are made: the light is raised to the power of 1 over this, so 1 is no lift, black stays black and full brightness stays full",
            DEFAULT_SCENE_LIFT,
            0.5,
            3.0,
        ),
    ]
}

/// The names of the constants a setting makes in the shader, given the text its name begins with (`SETTING_` for a pack's own, nothing for the engine's): the name in upper case,
/// and for a choice the name and each of its words.
pub(crate) fn constants_of(prefix: &str, declaration: &SettingDeclaration) -> Vec<String> {
    let name = format!("{prefix}{}", declaration.name().to_uppercase());
    let words = declaration
        .kind()
        .options()
        .iter()
        .map(|word| format!("{name}_{}", word.to_uppercase()))
        .collect::<Vec<_>>();
    std::iter::once(name).chain(words).collect()
}

/// One setting as it is in a shader: the constant, the value, and the words if it is a choice (a choice is the place of its word).
#[derive(Clone, Debug, PartialEq)]
struct ShaderSetting {
    constant: String,
    value: SettingValue,
    options: Vec<String>,
}

/// The values the shaders are made with: each setting's constant and what it is. Equal values make the same shaders.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShaderSettings {
    /// In the order of the declarations.
    values: Vec<ShaderSetting>,
}

impl ShaderSettings {
    /// The engine's settings with what is chosen of them (the rest are their defaults). An error says which name is not a setting (and what there are) or which value is not
    /// one of its setting.
    pub(crate) fn resolve(choices: &SettingChoices) -> Result<Self, String> {
        Self::resolve_for(&SettingChoices::new(), &[], choices)
    }

    /// The settings of a pack: the engine's with the defaults the pack sets (`engine_defaults`), then the pack's own (`own`, whose constants are `SETTING_` and the name in
    /// upper case), with what is chosen of any of them over the defaults.
    pub(crate) fn resolve_for(
        engine_defaults: &SettingChoices,
        own: &[SettingDeclaration],
        choices: &SettingChoices,
    ) -> Result<Self, String> {
        let engine = engine_settings_with(engine_defaults)?;
        let declared: Vec<(String, &SettingDeclaration)> = engine
            .iter()
            .map(|declaration| (String::new(), declaration))
            .chain(
                own.iter()
                    .map(|declaration| (OWN_PREFIX.to_owned(), declaration)),
            )
            .collect();
        if let Some(unknown) = choices.keys().find(|name| {
            !declared
                .iter()
                .any(|(_, declaration)| declaration.name() == *name)
        }) {
            return Err(format!(
                "{unknown:?} is not a setting; there are {}",
                declared
                    .iter()
                    .map(|(_, declaration)| declaration.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let mut values = Vec::new();
        for (prefix, declaration) in declared {
            let value = match choices.get(declaration.name()) {
                Some(chosen) => declaration
                    .kind()
                    .check(chosen.clone())
                    .map_err(|error| format!("{}: {error}", declaration.name()))?,
                None => declaration.default_value(),
            };
            values.push(ShaderSetting {
                constant: constants_of(&prefix, declaration).remove(0),
                value,
                options: declaration.kind().options().to_vec(),
            });
        }
        Ok(Self { values })
    }

    /// The defaults of the engine's settings.
    pub(crate) fn defaults() -> Self {
        Self::resolve(&SettingChoices::new()).expect("the defaults are in their ranges")
    }

    /// The constants of the shader language: one for each setting, and for a choice one more for each of its words (the place of the word), so that a shader can say
    /// `RELIEF == RELIEF_FLAT`.
    pub(crate) fn prelude(&self) -> String {
        let mut text = String::new();
        for setting in &self.values {
            let (kind, value) = setting.value.wgsl(&setting.options);
            text.push_str(&format!("const {}: {kind} = {value};\n", setting.constant));
            for (place, word) in setting.options.iter().enumerate() {
                text.push_str(&format!(
                    "const {}_{}: i32 = {place};\n",
                    setting.constant,
                    word.to_uppercase()
                ));
            }
        }
        text
    }

    /// The value of the setting with this constant, if the settings have one.
    fn value_of(&self, constant: &str) -> Option<&SettingValue> {
        self.values
            .iter()
            .find(|setting| setting.constant == constant)
            .map(|setting| &setting.value)
    }

    /// The lift of the whole scene (`scene_lift`), for the colour the picture is cleared to (the shader lifts what it draws).
    pub(crate) fn scene_lift(&self) -> f32 {
        match self.value_of("SCENE_LIFT") {
            Some(SettingValue::Float(lift)) => *lift,
            other => panic!("the engine has a scene lift: {other:?}"),
        }
    }

    /// How the relief of pictures is drawn (`relief`): the shader of each is a different text.
    pub(crate) fn relief(&self) -> Relief {
        match self.value_of("RELIEF") {
            Some(SettingValue::Choice(word)) => {
                Relief::from_name(word).expect("the engine's relief is one of its words")
            }
            other => panic!("the engine has a relief: {other:?}"),
        }
    }
}

#[cfg(test)]
mod choice_tests;
#[cfg(test)]
mod pack_tests;
#[cfg(test)]
mod tests;

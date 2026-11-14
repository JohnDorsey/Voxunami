//! The settings of how the engine draws (`docs/specs/shader-pack-spec.md`, decision 8): numbers of taste that a player changes by eye, each with a name, a type, a range and a
//! default, and that reach the shaders as constants written into their text. A setting that is a constant costs nothing in a frame (the shader's compiler folds it into the code
//! round it); one that is changed makes the shaders again, which is a pause of a moment and not something done while a player holds a key down.
//!
//! The engine's own are [`engine_settings`]: how deep the relief of a block's picture is, how dark the foot of a prism's wall is, how much the whole scene is lifted. A name of a
//! setting is lower case with underscores, and the constant is the same in upper case (`scene_lift` is `SCENE_LIFT`).

use std::collections::BTreeMap;
use std::fmt;

/// The value of a setting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SettingValue {
    Float(f32),
    Integer(i32),
    Switch(bool),
}

impl fmt::Display for SettingValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingValue::Float(value) => write!(formatter, "{value}"),
            SettingValue::Integer(value) => write!(formatter, "{value}"),
            SettingValue::Switch(true) => write!(formatter, "on"),
            SettingValue::Switch(false) => write!(formatter, "off"),
        }
    }
}

impl SettingValue {
    /// The shader language's text of the value, as the right of `const NAME: type = ...;` (and the type).
    fn wgsl(self) -> (&'static str, String) {
        match self {
            // (`{:?}` of a float always has a point or an exponent: it is a float literal, and it reads back as the same number.)
            SettingValue::Float(value) => ("f32", format!("{value:?}")),
            SettingValue::Integer(value) => ("i32", value.to_string()),
            SettingValue::Switch(value) => ("bool", value.to_string()),
        }
    }
}

/// What a setting holds, and what values it may have.
#[derive(Clone, Copy, Debug, PartialEq)]
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
}

impl SettingKind {
    pub fn default_value(self) -> SettingValue {
        match self {
            SettingKind::Float { default, .. } => SettingValue::Float(default),
            SettingKind::Integer { default, .. } => SettingValue::Integer(default),
            SettingKind::Switch { default } => SettingValue::Switch(default),
        }
    }

    /// What a player is told the setting takes: `a number from 0 to 1`.
    pub fn describe(self) -> String {
        match self {
            SettingKind::Float {
                minimum, maximum, ..
            } => format!("a number from {minimum} to {maximum}"),
            SettingKind::Integer {
                minimum, maximum, ..
            } => format!("a whole number from {minimum} to {maximum}"),
            SettingKind::Switch { .. } => "on or off".to_owned(),
        }
    }

    /// The value, if it is of this kind and in its range; an error says what the setting takes.
    pub fn check(self, value: SettingValue) -> Result<SettingValue, String> {
        let wrong = || {
            Err(format!(
                "{value} is not a value of this setting, which takes {}",
                self.describe()
            ))
        };
        match (self, value) {
            (
                SettingKind::Float {
                    minimum, maximum, ..
                },
                SettingValue::Float(number),
            ) if number.is_finite() && (minimum..=maximum).contains(&number) => Ok(value),
            (
                SettingKind::Integer {
                    minimum, maximum, ..
                },
                SettingValue::Integer(number),
            ) if (minimum..=maximum).contains(&number) => Ok(value),
            (SettingKind::Switch { .. }, SettingValue::Switch(_)) => Ok(value),
            _ => wrong(),
        }
    }

    /// The value a player's text means for this kind, in its range: a number, a whole number, or `on` and `off` (also `true` and `false`).
    pub fn parse(self, text: &str) -> Result<SettingValue, String> {
        let text = text.trim();
        let value = match self {
            SettingKind::Float { .. } => text.parse::<f32>().ok().map(SettingValue::Float),
            SettingKind::Integer { .. } => text.parse::<i32>().ok().map(SettingValue::Integer),
            SettingKind::Switch { .. } => match text {
                "on" | "true" => Some(SettingValue::Switch(true)),
                "off" | "false" => Some(SettingValue::Switch(false)),
                _ => None,
            },
        };
        match value {
            Some(value) => self.check(value),
            None => Err(format!(
                "{text:?} is not a value of this setting, which takes {}",
                self.describe()
            )),
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
        let mut characters = name.chars();
        let well_named = characters
            .next()
            .is_some_and(|first| first.is_ascii_lowercase())
            && characters.all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
            });
        if !well_named {
            return Err(format!(
                "{name:?} is not the name of a setting: lower case letters, digits and underscores, beginning with a letter"
            ));
        }
        match kind {
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

    pub fn kind(&self) -> SettingKind {
        self.kind
    }

    pub fn default_value(&self) -> SettingValue {
        self.kind.default_value()
    }
}

/// What a player (or a pack) has chosen, by the names of settings: the rest are as their declarations say.
pub type SettingChoices = BTreeMap<String, SettingValue>;

/// The default of the number of the lift of the whole scene: the colour raised to the power of one over it (1 is no lift).
pub const DEFAULT_SCENE_LIFT: f32 = 1.2;

/// The settings the engine has, whatever the pack: the first is how deep, in voxels, the relief of a block's picture is (the lowest of a height field whose top is the face; prisms
/// are that deep), the second how much darker the ambient light is at the foot of the wall of the deepest prism than at its top, the third how much the whole scene is lifted.
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

/// The values the shaders are made with: each setting's constant and what it is. Equal values make the same shaders.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShaderSettings {
    /// The name of the constant (upper case) and its value, in the order of the declarations.
    values: Vec<(String, SettingValue)>,
}

impl ShaderSettings {
    /// The engine's settings with what is chosen of them (the rest are their defaults). An error says which name is not a setting (and what there are) or which value is not
    /// one of its setting.
    pub(crate) fn resolve(choices: &SettingChoices) -> Result<Self, String> {
        let declarations = engine_settings();
        if let Some(unknown) = choices.keys().find(|name| {
            !declarations
                .iter()
                .any(|declaration| declaration.name() == *name)
        }) {
            return Err(format!(
                "{unknown:?} is not a setting; there are {}",
                declarations
                    .iter()
                    .map(SettingDeclaration::name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let mut values = Vec::new();
        for declaration in &declarations {
            let value = match choices.get(declaration.name()) {
                Some(&chosen) => declaration
                    .kind()
                    .check(chosen)
                    .map_err(|error| format!("{}: {error}", declaration.name()))?,
                None => declaration.default_value(),
            };
            values.push((declaration.name().to_uppercase(), value));
        }
        Ok(Self { values })
    }

    /// The defaults of the engine's settings.
    pub(crate) fn defaults() -> Self {
        Self::resolve(&SettingChoices::new()).expect("the defaults are in their ranges")
    }

    /// One constant of the shader language for each setting.
    pub(crate) fn prelude(&self) -> String {
        self.values
            .iter()
            .map(|(name, value)| {
                let (kind, text) = value.wgsl();
                format!("const {name}: {kind} = {text};\n")
            })
            .collect()
    }

    /// The lift of the whole scene (`scene_lift`), for the colour the picture is cleared to (the shader lifts what it draws).
    pub(crate) fn scene_lift(&self) -> f32 {
        self.values
            .iter()
            .find_map(|(name, value)| match (name.as_str(), value) {
                ("SCENE_LIFT", SettingValue::Float(lift)) => Some(*lift),
                _ => None,
            })
            .expect("the engine has a scene lift")
    }
}

#[cfg(test)]
mod tests;

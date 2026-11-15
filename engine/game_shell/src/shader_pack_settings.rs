//! What a player chose of the settings of shader packs, kept in files next to the packs (`docs/specs/shader-pack-spec.md`, decision 8).
//!
//! Each pack has a file in the folder of packs it was found in, called after its folder and `.settings.json`: the pack in the folder `cel` keeps its choices in `cel.settings.json`
//! beside it. The game's own shader, which has no folder, keeps them in `default.settings.json` in the `shaderpacks` folder of the game data folder. A file is one JSON object, a
//! name of a setting (the engine's or the pack's own) and the value chosen:
//!
//! ```json
//! { "relief": "flat", "scene_lift": 1.4, "steps": 5 }
//! ```
//!
//! A number is a number, a switch is `true` or `false`, a choice is the text of one of its words. Only what differs from the pack's defaults is kept in the file the game writes (a
//! pack with nothing chosen has no file), and one that is written by hand with a value that is a default is kept as it is read. A name that is not a setting of the pack, or a
//! value that is not one of its setting, is a warning and is left out; a file that is not a JSON object is a warning and is left alone: it is not written over until it is mended or
//! removed, so that a mistake in a hand-written file does not cost what else was in it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use renderer::{SettingChoices, SettingDeclaration, SettingKind, SettingValue};

/// Which shader the choices are for: a pack by its id, or `None` for the game's own.
pub type PackKey = Option<String>;

/// The choices for each shader. A shader with none has no entry (or an empty one: they are the same).
pub type PackChoices = BTreeMap<PackKey, SettingChoices>;

/// The value a JSON value means for a setting of this kind, in its range.
fn value_from_json(kind: &SettingKind, json: &serde_json::Value) -> Result<SettingValue, String> {
    let value = match (kind, json) {
        (SettingKind::Float { .. }, serde_json::Value::Number(number)) => number
            .as_f64()
            .map(|number| SettingValue::Float(number as f32)),
        (SettingKind::Integer { .. }, serde_json::Value::Number(number)) => number
            .as_i64()
            .and_then(|number| i32::try_from(number).ok())
            .map(SettingValue::Integer),
        (SettingKind::Switch { .. }, serde_json::Value::Bool(on)) => {
            Some(SettingValue::Switch(*on))
        }
        (SettingKind::Choice { .. }, serde_json::Value::String(word)) => {
            Some(SettingValue::Choice(word.clone()))
        }
        _ => None,
    };
    match value {
        Some(value) => kind.check(value),
        None => Err(format!(
            "{json} is not a value of this setting, which takes {}",
            kind.describe()
        )),
    }
}

/// The JSON value of a setting's value (a number reads back as the same number: the shortest text of the `f32` is the text of the `f64` it is read as).
fn value_to_json(value: &SettingValue) -> serde_json::Value {
    match value {
        SettingValue::Float(number) => number
            .to_string()
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        SettingValue::Integer(number) => serde_json::Value::from(*number),
        SettingValue::Switch(on) => serde_json::Value::Bool(*on),
        SettingValue::Choice(word) => serde_json::Value::String(word.clone()),
    }
}

/// The files that keep the choices, and what each held when it was read or last written.
#[derive(Debug)]
pub struct SettingsFiles {
    files: BTreeMap<PackKey, PathBuf>,
    saved: PackChoices,
    /// The files that were there and could not be read as a JSON object: not written over.
    unreadable: BTreeSet<PackKey>,
}

impl SettingsFiles {
    /// No files: nothing is read and nothing is written.
    pub fn none() -> Self {
        Self {
            files: BTreeMap::new(),
            saved: PackChoices::new(),
            unreadable: BTreeSet::new(),
        }
    }

    /// Reads the files that are there. `files` says where each shader's is, and `declared` what its settings are (the engine's with its defaults, and its own): the choices of a
    /// shader that is not in `declared` are not read. Returns the files, the choices, and a warning for each thing that was wrong.
    pub fn load(
        files: BTreeMap<PackKey, PathBuf>,
        declared: &BTreeMap<PackKey, Vec<SettingDeclaration>>,
    ) -> (Self, PackChoices, Vec<String>) {
        let mut loaded = Self {
            files,
            saved: PackChoices::new(),
            unreadable: BTreeSet::new(),
        };
        let mut warnings = Vec::new();
        for (key, file) in &loaded.files {
            let Some(declarations) = declared.get(key) else {
                continue;
            };
            let text = match std::fs::read_to_string(file) {
                Ok(text) => text,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    warnings.push(format!("{}: {error}", file.display()));
                    loaded.unreadable.insert(key.clone());
                    continue;
                }
            };
            let object = match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(serde_json::Value::Object(object)) => object,
                Ok(_) => {
                    warnings.push(format!("{}: the file is not a JSON object", file.display()));
                    loaded.unreadable.insert(key.clone());
                    continue;
                }
                Err(error) => {
                    warnings.push(format!("{}: {error}", file.display()));
                    loaded.unreadable.insert(key.clone());
                    continue;
                }
            };
            let mut choices = SettingChoices::new();
            for (name, json) in &object {
                let Some(declaration) = declarations
                    .iter()
                    .find(|declaration| declaration.name() == name)
                else {
                    warnings.push(format!(
                        "{}: `{name}` is not a setting of this shader (they are {}): left out",
                        file.display(),
                        declarations
                            .iter()
                            .map(SettingDeclaration::name)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                    continue;
                };
                match value_from_json(declaration.kind(), json) {
                    Ok(value) => {
                        choices.insert(name.clone(), value);
                    }
                    Err(error) => {
                        warnings.push(format!("{}: `{name}`: {error}: left out", file.display()))
                    }
                }
            }
            loaded.saved.insert(key.clone(), choices);
        }
        let choices = loaded.saved.clone();
        (loaded, choices, warnings)
    }

    /// Writes the files of the shaders whose choices are not what the file holds, and says what could not be written: a shader with nothing chosen has no file, and one whose file
    /// was unreadable when it was read is not written over (it is said once).
    pub fn save_changes(&mut self, current: &PackChoices) -> Vec<String> {
        let mut warnings = Vec::new();
        let empty = SettingChoices::new();
        for (key, file) in &self.files {
            let now = current.get(key).unwrap_or(&empty);
            let before = self.saved.get(key).unwrap_or(&empty);
            if now == before {
                continue;
            }
            if self.unreadable.contains(key) {
                warnings.push(format!(
                    "{}: what you chose is not saved, for the file could not be read: mend it or remove it",
                    file.display()
                ));
            } else if let Err(error) = write_choices(file, now) {
                warnings.push(error);
            }
            // (Said once, and not tried again in every frame until something else changes.)
            self.saved.insert(key.clone(), now.clone());
        }
        warnings
    }
}

/// Writes the choices to the file, whole and at once (a file beside it is written and moved over it); no choices is no file.
fn write_choices(file: &Path, choices: &SettingChoices) -> Result<(), String> {
    let failed = |error: std::io::Error| format!("{}: {error}", file.display());
    if choices.is_empty() {
        return match std::fs::remove_file(file) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(failed(error)),
            _ => Ok(()),
        };
    }
    let object: serde_json::Map<String, serde_json::Value> = choices
        .iter()
        .map(|(name, value)| (name.clone(), value_to_json(value)))
        .collect();
    let mut text = serde_json::to_string_pretty(&object).map_err(|error| error.to_string())?;
    text.push('\n');
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder).map_err(failed)?;
    }
    let mut beside = file.as_os_str().to_owned();
    beside.push(".new");
    let beside = PathBuf::from(beside);
    std::fs::write(&beside, text).map_err(failed)?;
    std::fs::rename(&beside, file).map_err(failed)
}

#[cfg(test)]
mod tests;

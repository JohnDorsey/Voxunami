//! Shader packs at the shell (`docs/specs/shader-pack-spec.md`, step 2): reading the folders a player has, and choosing one by name.
//!
//! A pack is a folder with a `pack.json` and a `lighting.wgsl`:
//!
//! ```json
//! { "id": "someone:cel", "name": "Cel shading", "interface": 1, "description": "Three steps of sun." }
//! ```
//!
//! `id` (a namespaced id) and `name` are required; `interface` is the version of the pack interface the pack is written for (the renderer's
//! [`renderer::SHADER_PACK_INTERFACE`]), and a pack for another is refused; `format` may be 1; `description` is free text; `relief` is how the relief of block pictures is drawn
//! (`"flat"` or `"prisms"`, [`renderer::Relief`]; left out, the engine's own choice, [`renderer::DEFAULT_RELIEF`]); `settings` declares settings of the pack's own (each a name with a
//! `type`, a `default` and for a number a range; the lighting reads `steps` as `SETTING_STEPS`) and `engine_settings` the defaults it sets for the engine's. `lighting.wgsl` defines `pack_shade`: the renderer's
//! [`renderer::ShaderPack::new`] checks it. Packs are the client's own: a folder of them lives in the game data folder (`shaderpacks`), and `--shader-packs` names another.

use std::path::{Path, PathBuf};

use identity::Identity;
use namespaced_id::NamespacedId;
use renderer::{Relief, SHADER_PACK_INTERFACE, ShaderPack};

/// The keys a `pack.json` may have.
const KEYS: [&str; 8] = [
    "id",
    "name",
    "format",
    "interface",
    "description",
    "relief",
    "settings",
    "engine_settings",
];

/// A pack the player can choose: its id and its name.
#[derive(Clone, Debug, PartialEq)]
pub struct ShaderPackName {
    pub id: String,
    pub name: String,
    /// The settings in play with the pack (`renderer::ShaderPack::settings`): the engine's with the pack's defaults, then its own.
    pub settings: Vec<renderer::SettingDeclaration>,
}

/// The packs found in a folder, and what was wrong with the folders that are not packs.
#[derive(Clone, Debug, Default)]
pub struct ShaderPackShelf {
    /// By id.
    pub packs: Vec<ShaderPack>,
    /// One text for each thing wrong, each naming the folder and file.
    pub problems: Vec<String>,
}

impl ShaderPackShelf {
    /// The names of the packs, in their order.
    pub fn names(&self) -> Vec<ShaderPackName> {
        self.packs
            .iter()
            .map(|pack| ShaderPackName {
                id: pack.id().to_owned(),
                name: pack.name().to_owned(),
                settings: pack.settings(),
            })
            .collect()
    }
}

impl ShaderPackShelf {
    /// Adds what another shelf holds: a pack whose id is one already here is not added, and is a problem.
    fn absorb(&mut self, other: ShaderPackShelf) {
        self.problems.extend(other.problems);
        for pack in other.packs {
            if let Some(first) = self.packs.iter().find(|known| known.id() == pack.id()) {
                self.problems.push(format!(
                    "the id {} of {} is the id of {} too, which is used",
                    pack.id(),
                    pack.name(),
                    first.name()
                ));
            } else {
                self.packs.push(pack);
            }
        }
        self.packs.sort_by(|a, b| a.id().cmp(b.id()));
    }
}

/// The packs a player has: those in the game data folder's (`default`, if it is there: it need not be) and in the folder they asked for (`asked`, which must be). Where two
/// have an id the first is kept.
pub fn shelf_of(default: Option<&Path>, asked: Option<&Path>) -> Result<ShaderPackShelf, String> {
    let mut shelf = ShaderPackShelf::default();
    if let Some(default) = default.filter(|directory| directory.is_dir()) {
        shelf.absorb(read_directory(default)?);
    }
    if let Some(asked) = asked {
        shelf.absorb(read_directory(asked)?);
    }
    Ok(shelf)
}

/// The folder of the player's shader packs in the game data folder (next to the identity), if the data folder is known. It need not exist.
pub fn default_directory() -> Option<PathBuf> {
    Identity::default_path()?
        .parent()
        .map(|data| data.join("shaderpacks"))
}

/// The pack in this folder, or every problem with it.
pub fn read_pack(folder: &Path) -> Result<ShaderPack, Vec<String>> {
    let shown = folder.display();
    let read = |file: &str| {
        std::fs::read_to_string(folder.join(file)).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                format!("{shown}: there is no {file}")
            } else {
                format!("{shown}{}{file}: {error}", std::path::MAIN_SEPARATOR)
            }
        })
    };
    let description = read("pack.json").map_err(|error| vec![error])?;
    let value: serde_json::Value = serde_json::from_str(&description).map_err(|error| {
        vec![format!(
            "{shown}{}pack.json: {error}",
            std::path::MAIN_SEPARATOR
        )]
    })?;
    let mut problems = Vec::new();
    let in_file =
        |message: String| format!("{shown}{}pack.json: {message}", std::path::MAIN_SEPARATOR);
    let Some(object) = value.as_object() else {
        return Err(vec![in_file("the file is not a JSON object".to_owned())]);
    };
    for key in object.keys() {
        if !KEYS.contains(&key.as_str()) {
            problems.push(in_file(format!(
                "`{key}` is not a key of a pack (they are {})",
                KEYS.join(", ")
            )));
        }
    }
    let text = |key: &str, problems: &mut Vec<String>| match object.get(key) {
        Some(serde_json::Value::String(text)) if !text.is_empty() => Some(text.clone()),
        Some(_) => {
            problems.push(in_file(format!("`{key}` is a text that is not empty")));
            None
        }
        None => {
            problems.push(in_file(format!("`{key}` is missing")));
            None
        }
    };
    let id = text("id", &mut problems);
    let name = text("name", &mut problems);
    if let Some(id) = &id
        && id.parse::<NamespacedId>().is_err()
    {
        problems.push(in_file(format!(
            "`id` {id:?} is not a namespaced id like `author:name`"
        )));
    }
    if let Some(format) = object.get("format")
        && format != 1
    {
        problems.push(in_file(format!("`format` is {format}: only 1 is known")));
    }
    if let Some(interface) = object.get("interface")
        && *interface != SHADER_PACK_INTERFACE
    {
        problems.push(in_file(format!(
            "`interface` is {interface}: the pack is written for another version of the interface than this engine's, {SHADER_PACK_INTERFACE}"
        )));
    }
    if let Some(description) = object.get("description")
        && !description.is_string()
    {
        problems.push(in_file("`description` is a text".to_owned()));
    }
    let relief = match object.get("relief") {
        None => None,
        Some(serde_json::Value::String(word)) => match Relief::from_name(word) {
            Some(relief) => Some(relief),
            None => {
                problems.push(in_file(format!(
                    "`relief` is {word:?}: it is \"flat\" or \"prisms\""
                )));
                None
            }
        },
        Some(_) => {
            problems.push(in_file(
                "`relief` is a text: \"flat\" or \"prisms\"".to_owned(),
            ));
            None
        }
    };
    let own_settings = match object.get("settings") {
        None => Vec::new(),
        Some(value) => read_own_settings(value, &in_file, &mut problems),
    };
    let engine_defaults = match object.get("engine_settings") {
        None => renderer::SettingChoices::new(),
        Some(value) => read_engine_defaults(value, &in_file, &mut problems),
    };
    let lighting = match read("lighting.wgsl") {
        Ok(lighting) => Some(lighting),
        Err(problem) => {
            problems.push(problem);
            None
        }
    };
    if !problems.is_empty() {
        return Err(problems);
    }
    let (Some(id), Some(name), Some(lighting)) = (id, name, lighting) else {
        return Err(problems);
    };
    ShaderPack::new_with_settings(&id, &name, &lighting, own_settings)
        .map(|pack| {
            pack.with_engine_defaults(engine_defaults)
                .expect("the defaults were checked, one by one, as they were read")
        })
        .map(|pack| match relief {
            Some(relief) => pack.with_relief(relief),
            None => pack,
        })
        .map_err(|error| {
            error
                .problems
                .iter()
                .map(|problem| {
                    format!(
                        "{shown}{}lighting.wgsl{}{}",
                        std::path::MAIN_SEPARATOR,
                        if problem.line.is_some() { ", " } else { ": " },
                        problem
                    )
                })
                .collect()
        })
}

/// The keys of a setting a pack declares.
const SETTING_KEYS: [&str; 5] = ["type", "description", "default", "minimum", "maximum"];

/// The settings of the `"settings"` object of a `pack.json`: each key is a name, each value an object with the type (`float`, `integer` or `switch`), a default and, for a number, a
/// range. Every problem is added to `problems`, and a setting that has any is left out.
fn read_own_settings(
    value: &serde_json::Value,
    in_file: &dyn Fn(String) -> String,
    problems: &mut Vec<String>,
) -> Vec<renderer::SettingDeclaration> {
    let Some(settings) = value.as_object() else {
        problems.push(in_file(
            "`settings` is an object: each key a name, each value the setting".to_owned(),
        ));
        return Vec::new();
    };
    let engine = renderer::engine_settings();
    let mut declared = Vec::new();
    for (name, setting) in settings {
        match read_setting(name, setting, &engine) {
            Ok(declaration) => declared.push(declaration),
            Err(problem) => problems.push(in_file(problem)),
        }
    }
    declared
}

fn read_setting(
    name: &str,
    value: &serde_json::Value,
    engine: &[renderer::SettingDeclaration],
) -> Result<renderer::SettingDeclaration, String> {
    use renderer::SettingKind;
    let Some(setting) = value.as_object() else {
        return Err(format!(
            "the setting `{name}` is an object with a `type` and a `default`"
        ));
    };
    if let Some(key) = setting
        .keys()
        .find(|key| !SETTING_KEYS.contains(&key.as_str()))
    {
        return Err(format!(
            "`{key}` is not a key of the setting `{name}` (they are {})",
            SETTING_KEYS.join(", ")
        ));
    }
    if engine.iter().any(|declaration| declaration.name() == name) {
        return Err(format!(
            "the setting `{name}` is an engine setting's name: rename it (the engine's are set in `engine_settings`)"
        ));
    }
    let description = match setting.get("description") {
        None => "",
        Some(serde_json::Value::String(text)) => text,
        Some(_) => {
            return Err(format!("`description` of the setting `{name}` is a text"));
        }
    };
    let Some(kind_word) = setting.get("type").and_then(serde_json::Value::as_str) else {
        return Err(format!(
            "`type` of the setting `{name}` is missing, a text: float, integer or switch"
        ));
    };
    let need = |key: &str| {
        setting
            .get(key)
            .ok_or_else(|| format!("`{key}` of the setting `{name}` is missing"))
    };
    let whole = |key: &str| -> Result<i32, String> {
        need(key)?
            .as_i64()
            .and_then(|number| i32::try_from(number).ok())
            .ok_or_else(|| format!("`{key}` of the setting `{name}` is a whole number"))
    };
    let number = |key: &str| -> Result<f32, String> {
        need(key)?
            .as_f64()
            .map(|number| number as f32)
            .ok_or_else(|| format!("`{key}` of the setting `{name}` is a number"))
    };
    let kind = match kind_word {
        "float" => SettingKind::Float {
            default: number("default")?,
            minimum: number("minimum")?,
            maximum: number("maximum")?,
        },
        "integer" => SettingKind::Integer {
            default: whole("default")?,
            minimum: whole("minimum")?,
            maximum: whole("maximum")?,
        },
        "switch" => {
            if let Some(key) = ["minimum", "maximum"]
                .into_iter()
                .find(|key| setting.contains_key(*key))
            {
                return Err(format!(
                    "`{key}` of the setting `{name}` is for numbers, and it is a switch"
                ));
            }
            SettingKind::Switch {
                default: need("default")?
                    .as_bool()
                    .ok_or_else(|| format!("`default` of the setting `{name}` is true or false"))?,
            }
        }
        other => {
            return Err(format!(
                "`type` of the setting `{name}` is {other:?}: float, integer or switch"
            ));
        }
    };
    renderer::SettingDeclaration::new(name, description, kind)
        .map_err(|error| format!("the setting `{name}`: {error}"))
}

/// The `"engine_settings"` object of a `pack.json`: the defaults the pack sets for the engine's settings, by name, each a number or true or false.
fn read_engine_defaults(
    value: &serde_json::Value,
    in_file: &dyn Fn(String) -> String,
    problems: &mut Vec<String>,
) -> renderer::SettingChoices {
    let Some(defaults) = value.as_object() else {
        problems.push(in_file(
            "`engine_settings` is an object: each key the name of an engine setting, each value what the pack makes its default".to_owned(),
        ));
        return renderer::SettingChoices::new();
    };
    let engine = renderer::engine_settings();
    let mut chosen = renderer::SettingChoices::new();
    for (name, value) in defaults {
        let Some(declaration) = engine.iter().find(|declaration| declaration.name() == name) else {
            problems.push(in_file(format!(
                "`engine_settings`: `{name}` is not an engine setting (they are {})",
                engine
                    .iter()
                    .map(renderer::SettingDeclaration::name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
            continue;
        };
        let parsed = match value {
            serde_json::Value::Number(_) | serde_json::Value::Bool(_) => {
                declaration.kind().parse(&value.to_string())
            }
            _ => Err(format!(
                "{value} is not a value of this setting, which takes {}",
                declaration.kind().describe()
            )),
        };
        match parsed {
            Ok(parsed) => {
                chosen.insert(name.clone(), parsed);
            }
            Err(error) => problems.push(in_file(format!("`engine_settings`: `{name}`: {error}"))),
        }
    }
    chosen
}

/// The packs in the folders of this folder (a file in it is not a pack and is not looked at). The packs that are good are kept whatever the others are; two with one id are one
/// (the first, in the order of the folders' names) and a problem.
pub fn read_directory(directory: &Path) -> Result<ShaderPackShelf, String> {
    let mut folders: Vec<PathBuf> = std::fs::read_dir(directory)
        .map_err(|error| format!("the shader packs folder {}: {error}", directory.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect();
    folders.sort();
    let mut shelf = ShaderPackShelf::default();
    for folder in folders {
        match read_pack(&folder) {
            Ok(pack) => {
                if let Some(first) = shelf.packs.iter().find(|known| known.id() == pack.id()) {
                    shelf.problems.push(format!(
                        "{}: the id {} is the id of {} too, which is used",
                        folder.display(),
                        pack.id(),
                        first.name()
                    ));
                } else {
                    shelf.packs.push(pack);
                }
            }
            Err(problems) => shelf.problems.extend(problems),
        }
    }
    shelf.packs.sort_by(|a, b| a.id().cmp(b.id()));
    Ok(shelf)
}

/// The part of an id after the colon: what a player types.
fn local_name(id: &str) -> &str {
    id.split_once(':').map_or(id, |(_, local)| local)
}

/// Which of the packs a player's text names: the id as it is, its part after the colon or the name in any case, or the beginning of any of them if only one pack begins so. An
/// error says what was wrong and what there is.
pub fn pick(names: &[ShaderPackName], text: &str) -> Result<usize, String> {
    let lower = text.to_lowercase();
    if let Some(found) = names.iter().position(|pack| {
        pack.id.to_lowercase() == lower
            || local_name(&pack.id).to_lowercase() == lower
            || pack.name.to_lowercase() == lower
    }) {
        return Ok(found);
    }
    let beginnings: Vec<usize> = names
        .iter()
        .enumerate()
        .filter(|(_, pack)| {
            pack.id.to_lowercase().starts_with(&lower)
                || local_name(&pack.id).to_lowercase().starts_with(&lower)
                || pack.name.to_lowercase().starts_with(&lower)
        })
        .map(|(index, _)| index)
        .collect();
    let listed = |indexes: &mut dyn Iterator<Item = usize>| {
        indexes
            .map(|index| format!("{} ({})", names[index].name, names[index].id))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match beginnings.as_slice() {
        [one] => Ok(*one),
        [] if names.is_empty() => Err(format!(
            "there is no shader pack called {text:?}: there are none (see `shader-pack-format.md` for how to make one)"
        )),
        [] => Err(format!(
            "there is no shader pack called {text:?}; there are {}",
            listed(&mut (0..names.len()))
        )),
        several => Err(format!(
            "{text:?} could be any of {}",
            listed(&mut several.iter().copied())
        )),
    }
}

/// The command `check-shader-pack DIRECTORY`, for the people who write packs: what the game would say of a pack's folder, or of every pack in a folder of them. Says `ok` for each
/// good pack, and is an error that lists every problem if there is any.
pub fn run_check_shader_packs(arguments: Vec<String>) -> Result<(), String> {
    let [directory] = arguments.as_slice() else {
        return Err("usage: voxunami check-shader-pack DIRECTORY (the folder of a pack, or a folder of packs)".to_owned());
    };
    let path = Path::new(directory);
    let shelf = if path.join("pack.json").exists() {
        match read_pack(path) {
            Ok(pack) => ShaderPackShelf {
                packs: vec![pack],
                problems: Vec::new(),
            },
            Err(problems) => ShaderPackShelf {
                packs: Vec::new(),
                problems,
            },
        }
    } else {
        read_directory(path)?
    };
    for pack in &shelf.packs {
        println!("{} ({}): ok", pack.name(), pack.id());
    }
    if !shelf.problems.is_empty() {
        return Err(shelf.problems.join("\n"));
    }
    if shelf.packs.is_empty() {
        return Err(format!("there are no shader packs in {directory}"));
    }
    Ok(())
}

#[cfg(test)]
mod settings_tests;
#[cfg(test)]
mod tests;

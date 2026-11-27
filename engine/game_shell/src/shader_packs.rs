//! Shader packs at the shell (`docs/specs/shader-pack-spec.md`, step 2): reading the folders a player has, and choosing one by name.
//!
//! A pack is a folder with a `shaderpack.json` and a `lighting.wgsl`:
//!
//! ```json
//! { "id": "someone:cel", "name": "Cel shading", "interface": 1, "description": "Three steps of sun." }
//! ```
//!
//! `id` (a namespaced id) and `name` are required; `interface` is the version of the pack interface the pack is written for (the renderer's
//! [`renderer::SHADER_PACK_INTERFACE`]), and a pack for another is refused; `format` may be 1; `description` is free text; `settings` declares settings of the pack's own (each a
//! name with a `type` (`float`, `integer`, `switch` or `choice`), a `default`, for a number a range and for a choice its `options`; the lighting reads `steps` as `SETTING_STEPS`)
//! and `engine_settings` the defaults it sets for the engine's (`{ "relief": "flat" }` draws the relief of block pictures flat unless the player says otherwise). `lighting.wgsl`
//! defines `pack_shade`: the renderer's [`renderer::ShaderPack::new`] checks it. Packs are the client's own: a folder of them lives in the game data folder (`shaderpacks`), and
//! `--shader-packs` names another. What a player chooses of a pack's settings is kept in a file next to its folder (`cel.settings.json` for `cel`; see [`crate::shader_pack_settings`]).

use std::path::{Path, PathBuf};

use identity::Identity;
use namespaced_id::NamespacedId;
use renderer::{SHADER_PACK_INTERFACE, ShaderPack};

/// The file that makes a folder a shader pack. (A data pack's is `pack.json`; the two kinds are told apart by the name, and each reader says so when it meets the other's.)
pub const MANIFEST_FILE: &str = "shaderpack.json";

/// The manifest of a data pack, which this reader never reads and only names when a folder has it and not its own.
const DATA_PACK_MANIFEST_FILE: &str = "pack.json";

/// The keys a `shaderpack.json` may have.
const KEYS: [&str; 7] = [
    "id",
    "name",
    "format",
    "interface",
    "description",
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
    /// Where what the player chose of each pack's settings is kept, by the pack's id: next to the pack's folder, in the folder of packs it was found in.
    pub settings_files: std::collections::BTreeMap<String, PathBuf>,
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
        let mut files = other.settings_files;
        for pack in other.packs {
            if let Some(first) = self.packs.iter().find(|known| known.id() == pack.id()) {
                self.problems.push(format!(
                    "the id {} of {} is the id of {} too, which is used",
                    pack.id(),
                    pack.name(),
                    first.name()
                ));
            } else {
                if let Some(file) = files.remove(pack.id()) {
                    self.settings_files.insert(pack.id().to_owned(), file);
                }
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

/// What a pack's file of chosen settings is called after the name of its folder (`cel.settings.json` for `cel`).
pub const SETTINGS_FILE_ENDING: &str = ".settings.json";

/// The name that stands for the game's own shader in a file of chosen settings, in the player's folder of packs: `default.settings.json`.
pub const OWN_SETTINGS_NAME: &str = "default";

/// The file that keeps what the player chose of the game's own shader's settings, if the data folder is known.
pub fn own_settings_file(default_directory: Option<&Path>) -> Option<PathBuf> {
    default_directory
        .map(|directory| directory.join(format!("{OWN_SETTINGS_NAME}{SETTINGS_FILE_ENDING}")))
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
    let description = read(MANIFEST_FILE).map_err(|mut error| {
        if folder.join(DATA_PACK_MANIFEST_FILE).is_file() {
            error.push_str(&format!(
                " (this folder has a {DATA_PACK_MANIFEST_FILE}, which is the manifest of a data pack: those are the server's, and `--packs` names a folder of them)"
            ));
        }
        vec![error]
    })?;
    let value: serde_json::Value = serde_json::from_str(&description).map_err(|error| {
        vec![format!(
            "{shown}{}{MANIFEST_FILE}: {error}",
            std::path::MAIN_SEPARATOR
        )]
    })?;
    let mut problems = Vec::new();
    let in_file = |message: String| {
        format!(
            "{shown}{}{MANIFEST_FILE}: {message}",
            std::path::MAIN_SEPARATOR
        )
    };
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
const SETTING_KEYS: [&str; 6] = [
    "type",
    "description",
    "default",
    "minimum",
    "maximum",
    "options",
];

/// The settings of the `"settings"` object of a `shaderpack.json`: each key is a name, each value an object with the type (`float`, `integer`, `switch` or `choice`), a default and, for a
/// number, a range, and for a choice its words (`options`). Every problem is added to `problems`, and a setting that has any is left out.
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
            "`type` of the setting `{name}` is missing, a text: float, integer, switch or choice"
        ));
    };
    // (`options` are the words of a choice and nothing else has them; a number has a range and a switch and a choice have none.)
    let only_for = |keys: &[&str], kind: &str| -> Result<(), String> {
        match setting.keys().find(|key| {
            ["minimum", "maximum", "options"].contains(&key.as_str())
                && !keys.contains(&key.as_str())
        }) {
            Some(key) => Err(format!("`{key}` of the setting `{name}` is not for {kind}")),
            None => Ok(()),
        }
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
        "float" => {
            only_for(&["minimum", "maximum"], "a number")?;
            SettingKind::Float {
                default: number("default")?,
                minimum: number("minimum")?,
                maximum: number("maximum")?,
            }
        }
        "integer" => {
            only_for(&["minimum", "maximum"], "a number")?;
            SettingKind::Integer {
                default: whole("default")?,
                minimum: whole("minimum")?,
                maximum: whole("maximum")?,
            }
        }
        "switch" => {
            only_for(&[], "a switch")?;
            SettingKind::Switch {
                default: need("default")?
                    .as_bool()
                    .ok_or_else(|| format!("`default` of the setting `{name}` is true or false"))?,
            }
        }
        "choice" => {
            only_for(&["options"], "a choice")?;
            let words = need("options")?
                .as_array()
                .and_then(|words| {
                    words
                        .iter()
                        .map(|word| word.as_str().map(str::to_owned))
                        .collect::<Option<Vec<String>>>()
                })
                .ok_or_else(|| {
                    format!("`options` of the setting `{name}` is a list of texts, its words")
                })?;
            SettingKind::Choice {
                default: need("default")?
                    .as_str()
                    .ok_or_else(|| {
                        format!("`default` of the setting `{name}` is one of its `options`, a text")
                    })?
                    .to_owned(),
                options: words,
            }
        }
        other => {
            return Err(format!(
                "`type` of the setting `{name}` is {other:?}: float, integer, switch or choice"
            ));
        }
    };
    renderer::SettingDeclaration::new(name, description, kind)
        .map_err(|error| format!("the setting `{name}`: {error}"))
}

/// The `"engine_settings"` object of a `shaderpack.json`: the defaults the pack sets for the engine's settings, by name, each a number, true or false, or a word of a choice.
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
            serde_json::Value::String(word)
                if matches!(declaration.kind(), renderer::SettingKind::Choice { .. }) =>
            {
                declaration.kind().parse(word)
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
        let folder_name = folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if folder_name == OWN_SETTINGS_NAME {
            shelf.problems.push(format!(
                "{}: a pack's folder is not called `{OWN_SETTINGS_NAME}`, which is the name of the file that keeps what the player chose of the game's own shader settings (`{OWN_SETTINGS_NAME}.settings.json`): rename the folder",
                folder.display()
            ));
            continue;
        }
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
                    shelf.settings_files.insert(
                        pack.id().to_owned(),
                        directory.join(format!("{folder_name}{SETTINGS_FILE_ENDING}")),
                    );
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
    let shelf = if path.join(MANIFEST_FILE).exists() {
        match read_pack(path) {
            Ok(pack) => ShaderPackShelf {
                packs: vec![pack],
                ..ShaderPackShelf::default()
            },
            Err(problems) => ShaderPackShelf {
                problems,
                ..ShaderPackShelf::default()
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
pub(crate) mod tests;

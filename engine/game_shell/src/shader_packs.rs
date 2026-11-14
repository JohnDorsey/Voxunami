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
//! (`"flat"` or `"prisms"`, [`renderer::Relief`]; left out, the engine's own choice, [`renderer::DEFAULT_RELIEF`]). `lighting.wgsl` defines `pack_shade`: the renderer's
//! [`renderer::ShaderPack::new`] checks it. Packs are the client's own: a folder of them lives in the game data folder (`shaderpacks`), and `--shader-packs` names another.

use std::path::{Path, PathBuf};

use identity::Identity;
use namespaced_id::NamespacedId;
use renderer::{Relief, SHADER_PACK_INTERFACE, ShaderPack};

/// The keys a `pack.json` may have.
const KEYS: [&str; 6] = ["id", "name", "format", "interface", "description", "relief"];

/// A pack the player can choose: its id and its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderPackName {
    pub id: String,
    pub name: String,
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
    ShaderPack::new(&id, &name, &lighting)
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
mod tests;

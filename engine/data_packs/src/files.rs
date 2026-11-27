//! A pack's files, as a map from path to bytes, understood: the one place that knows which files of a pack are read. A folder and an archive both become
//! such a map, so the same pack gives the same [`RawPack`] whichever way it was stored.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::PackError;
use crate::load::{RawFile, RawPack, RawTexture};
use crate::walk::nearest;

/// The files of one pack by path (`pack.json`, `blocks/lava.json`), in the order of the paths.
pub(crate) type Files = BTreeMap<String, Vec<u8>>;

/// The folders of a pack that are read. Anything else is an error: a pack with a `recipes` folder that nothing reads would be reported as fine and then do
/// nothing, which is the worst thing a checker can say.
const READ_FOLDERS: [&str; 4] = ["blocks", "patches", "bodies", "textures"];

pub(crate) fn problem(file: String, message: impl Into<String>) -> PackError {
    PackError {
        file,
        path: String::new(),
        message: message.into(),
    }
}

fn text_of(bytes: &[u8], label: &str) -> Result<String, PackError> {
    String::from_utf8(bytes.to_vec()).map_err(|_| {
        problem(
            label.to_owned(),
            "cannot read this file: it is not text (UTF-8)",
        )
    })
}

/// The `.json` files directly in `folder`, by name without the extension, in the order of the names.
fn json_files(
    files: &Files,
    pack: &str,
    folder: &str,
    errors: &mut Vec<PackError>,
) -> Vec<RawFile> {
    let prefix = format!("{folder}/");
    let mut found = Vec::new();
    for (path, bytes) in files.range(prefix.clone()..) {
        let Some(rest) = path.strip_prefix(&prefix) else {
            break;
        };
        let Some(name) = rest.strip_suffix(".json") else {
            continue;
        };
        if name.is_empty() || name.contains('/') {
            continue;
        }
        let label = format!("{pack}/{folder}/{rest}");
        match text_of(bytes, &label) {
            Ok(text) => found.push(RawFile {
                name: name.to_owned(),
                text,
            }),
            Err(error) => errors.push(error),
        }
    }
    found
}

/// The pictures of a pack: its `.png` files in `textures`, named by the file's name, and the `.png` files in the folders of `textures` (named
/// `namespace/name`), which put pictures into the namespace of the folder. A folder inside those is an error, as an unread folder anywhere is.
fn textures(files: &Files, pack: &str, errors: &mut Vec<PackError>) -> Vec<RawTexture> {
    let mut textures = Vec::new();
    let mut too_deep = BTreeSet::new();
    for (path, bytes) in files.range("textures/".to_owned()..) {
        let Some(rest) = path.strip_prefix("textures/") else {
            break;
        };
        let parts: Vec<&str> = rest.split('/').collect();
        match parts.as_slice() {
            [file] => {
                if let Some(name) = file.strip_suffix(".png").filter(|name| !name.is_empty()) {
                    textures.push(RawTexture {
                        name: name.to_owned(),
                        bytes: bytes.clone(),
                    });
                }
            }
            [namespace, file] => {
                if let Some(name) = file.strip_suffix(".png").filter(|name| !name.is_empty()) {
                    textures.push(RawTexture {
                        name: format!("{namespace}/{name}"),
                        bytes: bytes.clone(),
                    });
                }
            }
            [namespace, deeper, ..] => {
                too_deep.insert(format!("{pack}/textures/{namespace}/{deeper}"));
            }
            [] => {}
        }
    }
    for folder in too_deep {
        errors.push(problem(
            folder,
            "nothing reads this folder; pictures are in `textures` or in a folder of it named for a namespace",
        ));
    }
    textures.sort_by(|first, second| first.name.cmp(&second.name));
    textures
}

fn unread_folders(files: &Files, pack: &str, errors: &mut Vec<PackError>) {
    let names: BTreeSet<&str> = files
        .keys()
        .filter_map(|path| path.split_once('/').map(|(folder, _)| folder))
        // (the folder a server keeps to itself is not read by design, and is not a mistake)
        .filter(|folder| {
            !READ_FOLDERS.contains(folder) && *folder != crate::directory::SERVER_FOLDER
        })
        .collect();
    for name in names {
        let hint = nearest(name, &READ_FOLDERS)
            .map(|near| format!(" (did you mean \"{near}\"?)"))
            .unwrap_or_default();
        errors.push(problem(
            format!("{pack}/{name}"),
            format!(
                "nothing reads this folder{hint}; a pack's folders are \"blocks\", \"patches\", \"bodies\" and \"textures\""
            ),
        ));
    }
}

/// Understands the files of the pack called `pack` (its folder's name, or its archive's). Files whose path has a part that starts with a dot are left alone,
/// as are files beside `pack.json`. Every problem found is pushed to `errors`; `None` if there is no `pack.json`.
pub(crate) fn raw_pack(pack: &str, files: &Files, errors: &mut Vec<PackError>) -> Option<RawPack> {
    let files: Files = files
        .iter()
        .filter(|(path, _)| !path.split('/').any(|part| part.starts_with('.')))
        .map(|(path, bytes)| (path.clone(), bytes.clone()))
        .collect();
    let manifest_label = format!("{pack}/pack.json");
    let Some(manifest_bytes) = files.get("pack.json") else {
        let mut message =
            "this file is missing: every folder here is a pack and has a pack.json".to_owned();
        if files.contains_key("shaderpack.json") {
            message.push_str(" (this folder has a shaderpack.json, which is the manifest of a shader pack: those are the client's and go in the game data folder's shaderpacks, or `--shader-packs`)");
        }
        errors.push(problem(manifest_label, message));
        return None;
    };
    let manifest = match text_of(manifest_bytes, &manifest_label) {
        Ok(text) => text,
        Err(error) => {
            errors.push(error);
            return None;
        }
    };
    unread_folders(&files, pack, errors);
    let blocks = json_files(&files, pack, "blocks", errors);
    let patches = json_files(&files, pack, "patches", errors);
    let bodies = json_files(&files, pack, "bodies", errors);
    let textures = textures(&files, pack, errors);
    Some(RawPack {
        folder: pack.to_owned(),
        manifest,
        blocks,
        patches,
        bodies,
        textures,
    })
}

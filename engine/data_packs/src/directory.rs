//! Finding packs on disk: a directory of folders, each with a `pack.json`, a `blocks` folder and a `patches`
//! folder.

use std::fs;
use std::path::Path;

use crate::error::{PackError, PackErrors};
use crate::load::{RawFile, RawPack};

fn problem(file: String, message: impl Into<String>) -> PackError {
    PackError {
        file,
        path: String::new(),
        message: message.into(),
    }
}

/// The names of what is in a directory, sorted: only folders if `folders`, only `.json` files (without the
/// extension) otherwise. Names that start with a dot are left alone.
fn names_in(directory: &Path, label: &str, folders: bool) -> Result<Vec<String>, PackError> {
    let entries = fs::read_dir(directory).map_err(|error| {
        problem(
            label.to_owned(),
            format!("cannot read this folder: {error}"),
        )
    })?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            problem(
                label.to_owned(),
                format!("cannot read this folder: {error}"),
            )
        })?;
        let Ok(name) = entry.file_name().into_string() else {
            continue; // a name that is not text is not a pack's name
        };
        if name.starts_with('.') {
            continue;
        }
        let is_folder = entry.path().is_dir();
        if folders {
            if is_folder {
                names.push(name);
            }
        } else if !is_folder && let Some(stem) = name.strip_suffix(".json") {
            names.push(stem.to_owned());
        }
    }
    names.sort();
    Ok(names)
}

fn read_text(path: &Path, label: &str) -> Result<String, PackError> {
    fs::read_to_string(path)
        .map_err(|error| problem(label.to_owned(), format!("cannot read this file: {error}")))
}

fn files_in(
    pack: &Path,
    folder_name: &str,
    folder: &str,
    errors: &mut Vec<PackError>,
) -> Vec<RawFile> {
    let directory = pack.join(folder);
    if !directory.is_dir() {
        return Vec::new();
    }
    let label = format!("{folder_name}/{folder}");
    let names = match names_in(&directory, &label, false) {
        Ok(names) => names,
        Err(error) => {
            errors.push(error);
            return Vec::new();
        }
    };
    names
        .into_iter()
        .filter_map(|name| {
            let label = format!("{label}/{name}.json");
            match read_text(&directory.join(format!("{name}.json")), &label) {
                Ok(text) => Some(RawFile { name, text }),
                Err(error) => {
                    errors.push(error);
                    None
                }
            }
        })
        .collect()
}

/// Reads every pack in the folders of `root`. A folder without a `pack.json` is an error (it is probably not meant to
/// be there, and quietly skipping it would hide a misplaced pack). A `root` that does not exist is an error.
pub fn read_directory(root: &Path) -> Result<Vec<RawPack>, PackErrors> {
    let root_label = root.display().to_string();
    let folders = names_in(root, &root_label, true).map_err(|error| PackErrors(vec![error]))?;
    let mut errors = Vec::new();
    let mut packs = Vec::new();
    for folder in folders {
        let pack = root.join(&folder);
        let manifest_path = pack.join("pack.json");
        let manifest_label = format!("{folder}/pack.json");
        if !manifest_path.is_file() {
            errors.push(problem(
                manifest_label,
                "this file is missing: every folder here is a pack and has a pack.json",
            ));
            continue;
        }
        let manifest = match read_text(&manifest_path, &manifest_label) {
            Ok(text) => text,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        let blocks = files_in(&pack, &folder, "blocks", &mut errors);
        let patches = files_in(&pack, &folder, "patches", &mut errors);
        packs.push(RawPack {
            folder,
            manifest,
            blocks,
            patches,
        });
    }
    if errors.is_empty() {
        Ok(packs)
    } else {
        Err(PackErrors(errors))
    }
}

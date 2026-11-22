//! Finding packs on disk: a directory of folders and of canonical `.zip` archives, each with a `pack.json` and the folders `blocks`, `patches`, `bodies` and
//! `textures`. A folder and an archive of the same files give the same [`RawPack`], because both become one map of files first (`files.rs`).

use std::fs;
use std::path::Path;

use pack_archive::{Archive, FileIndex, Hash, Limits};

use crate::error::{PackError, PackErrors};
use crate::files::{Files, problem, raw_pack};
use crate::load::RawPack;

/// The names of what is in a directory, sorted: only folders if `folders`, only `.json` files (without the
/// extension) otherwise. Names that start with a dot are left alone.
fn names_in(directory: &Path, label: &str, folders: bool) -> Result<Vec<String>, PackError> {
    names_with(directory, label, if folders { None } else { Some(".json") })
}

/// The names of the folders in a directory if `extension` is `None`, else of the files that end in it (without it), sorted. Names that start with a dot are left
/// alone.
fn names_with(
    directory: &Path,
    label: &str,
    extension: Option<&str>,
) -> Result<Vec<String>, PackError> {
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
        match extension {
            None if is_folder => names.push(name),
            Some(extension) if !is_folder => {
                if let Some(stem) = name.strip_suffix(extension) {
                    names.push(stem.to_owned());
                }
            }
            _ => {}
        }
    }
    names.sort();
    Ok(names)
}

/// How deep a pack's folders may nest; a folder that goes deeper than anything a pack has is a loop or a mistake.
const MOST_FOLDER_DEPTH: usize = 16;

/// Every file under `folder` by its path from the pack's folder, leaving alone names that start with a dot (a `.git` folder, a `.hidden.png`).
fn read_folder(
    folder: &Path,
    relative: &str,
    depth: usize,
    label: &str,
    files: &mut Files,
    errors: &mut Vec<PackError>,
) {
    let named = |relative: &str| {
        if relative.is_empty() {
            label.to_owned()
        } else {
            format!("{label}/{relative}")
        }
    };
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) => {
            errors.push(problem(
                named(relative),
                format!("cannot read this folder: {error}"),
            ));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                errors.push(problem(
                    named(relative),
                    format!("cannot read this folder: {error}"),
                ));
                continue;
            }
        };
        let Ok(name) = entry.file_name().into_string() else {
            continue; // a name that is not text is not a pack's name
        };
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let here = if relative.is_empty() {
            name
        } else {
            format!("{relative}/{name}")
        };
        if path.is_dir() {
            if depth < MOST_FOLDER_DEPTH {
                read_folder(&path, &here, depth + 1, label, files, errors);
            } else {
                errors.push(problem(
                    named(&here),
                    "this folder is nested deeper than any pack goes",
                ));
            }
        } else {
            match fs::read(&path) {
                Ok(bytes) => {
                    files.insert(here, bytes);
                }
                Err(error) => errors.push(problem(
                    named(&here),
                    format!("cannot read this file: {error}"),
                )),
            }
        }
    }
}

/// The pack a zip file holds is named by its file name without `.zip`, and without the `-` and 32 hexadecimal digits of a cached pack's name
/// (`my_pack-0123456789abcdef0123456789abcdef.zip` is the pack `my_pack`). `None` if the name does not end in `.zip`.
pub fn pack_name_of_zip(file_name: &str) -> Option<String> {
    let stem = file_name.strip_suffix(".zip")?;
    if let Some((name, key)) = stem.rsplit_once('-')
        && key.len() == 32
        && key
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Some(name.to_owned());
    }
    Some(stem.to_owned())
}

fn files_of(archive: &Archive) -> Files {
    archive
        .files()
        .map(|(path, bytes)| (path.to_owned(), bytes.to_vec()))
        .collect()
}

/// Reads one pack from a canonical archive, naming it `pack` in messages. The archive's files are understood by the same code as a folder's.
pub fn read_archive(pack: &str, archive: &Archive) -> Result<RawPack, PackErrors> {
    let mut errors = Vec::new();
    let raw = raw_pack(pack, &files_of(archive), &mut errors);
    match raw {
        Some(raw) if errors.is_empty() => Ok(raw),
        _ => Err(PackErrors(errors)),
    }
}

/// Reads a `.zip` file, which must be in the canonical form (`pack_archive`), and tells whoever wrote it if it is not.
fn read_zip(path: &Path, errors: &mut Vec<PackError>) -> Option<Files> {
    let label = path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            errors.push(problem(label, format!("cannot read this file: {error}")));
            return None;
        }
    };
    match Archive::from_bytes(&bytes, &Limits::default()) {
        Ok(archive) => Some(files_of(&archive)),
        Err(error) => {
            errors.push(problem(label, error.to_string()));
            None
        }
    }
}

/// The files of every pack in `root` by the pack's name, sorted by name: each folder, and each `.zip` file. A folder and a zip of one name are an error.
fn collect_packs(
    root: &Path,
    errors: &mut Vec<PackError>,
) -> Result<Vec<(String, Files)>, PackError> {
    let root_label = root.display().to_string();
    let folders = names_in(root, &root_label, true)?;
    let zips = names_with(root, &root_label, Some(".zip"))?;
    // every pack: its name, and the zip's file name if it is a zip
    let mut entries: Vec<(String, Option<String>)> =
        folders.into_iter().map(|name| (name, None)).collect();
    for stem in zips {
        let file_name = format!("{stem}.zip");
        if let Some(name) = pack_name_of_zip(&file_name) {
            entries.push((name, Some(file_name)));
        }
    }
    entries.sort();
    for pair in entries.windows(2) {
        if pair[0].0 == pair[1].0 {
            errors.push(problem(
                pair[1].0.clone(),
                "two packs have this name (a folder and a zip, or two zips)",
            ));
        }
    }
    let mut packs = Vec::new();
    for (name, zip) in entries {
        let files = match zip {
            Some(file_name) => read_zip(&root.join(file_name), errors),
            None => {
                let mut files = Files::new();
                read_folder(&root.join(&name), "", 0, &name, &mut files, errors);
                Some(files)
            }
        };
        if let Some(files) = files {
            packs.push((name, files));
        }
    }
    Ok(packs)
}

/// Reads every pack in `root`: each folder, and each `.zip` file (a canonical archive). A folder without a `pack.json` is an error (it is probably not meant to
/// be there, and quietly skipping it would hide a misplaced pack). A `root` that does not exist is an error. Packs come back sorted by name; a folder and a
/// zip of one name are an error.
pub fn read_directory(root: &Path) -> Result<Vec<RawPack>, PackErrors> {
    let mut errors = Vec::new();
    let collected = collect_packs(root, &mut errors).map_err(|error| PackErrors(vec![error]))?;
    let mut packs = Vec::new();
    for (name, files) in collected {
        if let Some(pack) = raw_pack(&name, &files, &mut errors) {
            packs.push(pack);
        }
    }
    if errors.is_empty() {
        Ok(packs)
    } else {
        Err(PackErrors(errors))
    }
}

/// A pack's name and its identity: the root hash of its file index, or why it cannot be archived.
pub type PackIdentity = (String, Result<Hash, String>);

/// The identity of each pack in `root`: its name and the root hash of its file index (all files sent), or why it cannot be archived (a file name that is not
/// lower case, for one). A zip is canonical already; a folder's files are written into the canonical form to be hashed. Read with the same rules as
/// [`read_directory`], which says what is wrong with the content; this says only what the identity is.
pub fn pack_identities(root: &Path) -> Result<Vec<PackIdentity>, PackErrors> {
    let mut errors = Vec::new();
    let collected = collect_packs(root, &mut errors).map_err(|error| PackErrors(vec![error]))?;
    if !errors.is_empty() {
        return Err(PackErrors(errors));
    }
    Ok(collected
        .into_iter()
        .map(|(name, files)| {
            let identity = Archive::from_files(files)
                .map(|archive| FileIndex::of(&archive, |_| true).root_hash())
                .map_err(|error| error.to_string());
            (name, identity)
        })
        .collect())
}

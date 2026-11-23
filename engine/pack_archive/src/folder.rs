//! The files of a folder on disk, for a build script that makes the archive of a program's own pack. (A pack a server reads from a folder is read by
//! `data_packs`, which reports each problem with the file it is in; a build script wants the files and a plain error, and stops.)

use std::fs;
use std::io;
use std::path::Path;

/// Every file under `folder`, at any depth, as (path, bytes) in the order of the paths. A path is `prefix` and then the file's path from `folder` with `/`
/// for a separator (`files_of_folder(textures, "textures/", ...)` gives `textures/dirt.png`). Names that start with a dot (`.git`, `.DS_Store`) are left
/// alone, folders of them too, as in a pack read from a folder; `keep` is asked about the path from `folder` (without the prefix) of every other file, and
/// a file it refuses is left out. A name that is not text is an error, since no archive can hold it.
///
/// The paths are not checked here: [`Archive::from_files`](crate::Archive::from_files) refuses one that is not allowed, naming it.
pub fn files_of_folder(
    folder: &Path,
    prefix: &str,
    keep: &dyn Fn(&str) -> bool,
) -> io::Result<Vec<(String, Vec<u8>)>> {
    let mut files = Vec::new();
    collect(folder, "", prefix, keep, &mut files)?;
    files.sort_by(|first, second| first.0.cmp(&second.0));
    Ok(files)
}

fn collect(
    folder: &Path,
    relative: &str,
    prefix: &str,
    keep: &dyn Fn(&str) -> bool,
    files: &mut Vec<(String, Vec<u8>)>,
) -> io::Result<()> {
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let name = entry.file_name().into_string().map_err(|name| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{name:?} in {} is not a name an archive can hold: it is not text",
                    folder.display()
                ),
            )
        })?;
        if name.starts_with('.') {
            continue;
        }
        let path = format!("{relative}{name}");
        if entry.file_type()?.is_dir() {
            collect(&entry.path(), &format!("{path}/"), prefix, keep, files)?;
        } else if keep(&path) {
            files.push((format!("{prefix}{path}"), fs::read(entry.path())?));
        }
    }
    Ok(())
}

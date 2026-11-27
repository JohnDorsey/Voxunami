//! Reading an author's own zip, which can be anything (deflated, with folder entries, inside a wrapper folder, with a Mac's leftovers), and giving the
//! canonical [`Archive`] of its files. The canonical form is what the server writes and what a client reads; this is how the server takes an author's zip
//! into it, as the owner decided on 2026-11-21. Needs the `import` feature.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use crate::archive::{Archive, Limits};
use crate::error::ArchiveError;

fn refuse(reason: impl Into<String>) -> ArchiveError {
    ArchiveError::Import {
        reason: reason.into(),
    }
}

/// A path as an archive spells it: `/` for a separator and no leading `./`.
fn normalized(name: &str) -> String {
    let mut path = name.replace('\\', "/");
    while let Some(rest) = path.strip_prefix("./") {
        path = rest.to_owned();
    }
    path
}

/// Whether a path is a tool's leftover and not part of the pack: a name that starts with a dot (`.git`, `.DS_Store`) or the `__MACOSX` folder of a Mac.
fn is_leftover(path: &str) -> bool {
    path.split('/')
        .any(|part| part == "__MACOSX" || (part.starts_with('.') && part != "." && part != ".."))
}

/// The canonical archive of the files in an author's zip.
///
/// What is done: folder entries are dropped; a folder the pack was zipped inside (one top-level folder that holds the `pack.json`) is taken off; `\` is
/// read as `/`; leftovers of tools are dropped (names that start with a dot, `__MACOSX`). What is **refused**, naming the file: a name that is not
/// lower case or otherwise not allowed (it is never changed for the author, because the pack's own JSON names its files), two entries of one path, a
/// symbolic link, an encrypted entry, a damaged one, and anything beyond `limits`, which are checked against the sizes the zip declares before an entry
/// is inflated and again on what comes out.
pub fn import_zip(bytes: &[u8], limits: &Limits) -> Result<Archive, ArchiveError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| refuse(format!("not a zip file: {error}")))?;
    // the `zip` crate keeps one of two entries that have exactly one name and says nothing, which would let a later file shadow an earlier one unseen
    if let Some(count) = declared_entry_count(bytes)
        && count != zip.len()
    {
        return Err(refuse(
            "two entries have the same name (the zip says it has more entries than it has names)",
        ));
    }
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut total = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|error| refuse(format!("entry {index} cannot be read: {error}")))?;
        let path = normalized(entry.name());
        if path.ends_with('/') {
            continue; // a folder entry says nothing a file's path does not
        }
        if path.split('/').any(|part| part == "..") {
            return Err(ArchiveError::InvalidPath {
                path,
                reason: "it has a part that is . or ..",
            });
        }
        if is_leftover(&path) {
            continue;
        }
        if entry.is_symlink() {
            return Err(refuse(format!("{path:?} is a symbolic link")));
        }
        if entry.encrypted() {
            return Err(refuse(format!("{path:?} is encrypted")));
        }
        if files.len() >= limits.max_files {
            return Err(ArchiveError::TooManyFiles {
                count: files.len() + 1,
                max: limits.max_files,
            });
        }
        let size = entry.size();
        if size > limits.max_file_bytes {
            return Err(ArchiveError::FileTooLarge {
                path,
                size,
                max: limits.max_file_bytes,
            });
        }
        total += size;
        if total > limits.max_total_bytes {
            return Err(ArchiveError::TooLarge {
                max: limits.max_total_bytes,
            });
        }
        if files.contains_key(&path) {
            return Err(ArchiveError::Duplicate { path });
        }
        // never more than the size it declared (and the limit checked) is inflated, whatever the stream holds
        let mut content = Vec::new();
        (&mut entry)
            .take(size + 1)
            .read_to_end(&mut content)
            .map_err(|error| refuse(format!("{path:?} cannot be read: {error}")))?;
        if content.len() as u64 != size {
            return Err(refuse(format!("{path:?} is not the size the zip says")));
        }
        files.insert(path, content);
    }
    take_off_the_wrapper_folder(&mut files);
    Archive::from_files(files)
}

/// A pack zipped as its folder (`my_pack/pack.json`, `my_pack/blocks/...`) is the pack: if there is no `pack.json` at the top but the one top-level folder
/// has one, the folder is taken off.
fn take_off_the_wrapper_folder(files: &mut BTreeMap<String, Vec<u8>>) {
    if files.contains_key("pack.json") {
        return;
    }
    let mut tops = files
        .keys()
        .map(|path| path.split('/').next().unwrap_or(""));
    let Some(top) = tops.next() else { return };
    if tops.any(|other| other != top) || !files.contains_key(&format!("{top}/pack.json")) {
        return;
    }
    let prefix = format!("{top}/");
    *files = std::mem::take(files)
        .into_iter()
        .map(|(path, bytes)| (path[prefix.len()..].to_owned(), bytes))
        .collect();
}

/// The number of entries the end record of a zip says it has, found by looking for the last end record in the final 65,557 bytes (22 and a comment of at
/// most 65,535). `None` if there is none, or the count is the zip64 marker.
fn declared_entry_count(bytes: &[u8]) -> Option<usize> {
    const SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
    let start = bytes.len().saturating_sub(22 + 65_535);
    let at = bytes[start..]
        .windows(4)
        .rposition(|window| window == SIGNATURE)?
        + start;
    let count = u16::from_le_bytes(bytes.get(at + 10..at + 12)?.try_into().ok()?);
    (count != u16::MAX).then_some(usize::from(count))
}

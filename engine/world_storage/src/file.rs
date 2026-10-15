//! Writing world files to disk without ever leaving a half-written world behind.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

/// Replaces the file at `path` with `bytes`, all or nothing: the data is written to a temporary file
/// next to it, flushed to disk, and only then renamed over the old file. If anything fails the old
/// file is untouched.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path has no file name"))?;
    let mut temporary_name = file_name.to_os_string();
    temporary_name.push(".saving");
    let temporary = path.with_file_name(temporary_name);

    let written = (|| {
        let mut file = File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    let result = written.and_then(|()| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    fn scratch_directory() -> std::path::PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let directory = std::env::temp_dir().join(format!(
            "voxunami_world_storage_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn writes_a_new_file_and_replaces_an_existing_one_without_leaving_scraps() {
        let directory = scratch_directory();
        let path = directory.join("world.vxw");
        write_atomically(&path, b"first").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"first");
        write_atomically(&path, b"second, longer").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second, longer");
        let names: Vec<_> = fs::read_dir(&directory)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["world.vxw"]);
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_failed_write_leaves_the_existing_file_intact() {
        let directory = scratch_directory();
        let path = directory.join("world.vxw");
        write_atomically(&path, b"precious").unwrap();
        // Make the rename fail: the destination is replaced by a directory of the same name.
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(write_atomically(&path, b"new").is_err());
        assert!(path.is_dir(), "the destination was not disturbed");
        let leftovers: Vec<_> = fs::read_dir(&directory)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(
            leftovers,
            ["world.vxw"],
            "the temporary file was cleaned up"
        );
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_missing_directory_or_file_name_is_an_error() {
        let directory = scratch_directory();
        assert!(
            write_atomically(&directory.join("no_such_folder").join("world.vxw"), b"x").is_err()
        );
        assert!(write_atomically(Path::new(""), b"x").is_err());
        fs::remove_dir_all(&directory).unwrap();
    }
}

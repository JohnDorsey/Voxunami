//! Where the files of ships are kept: one file for each ship, named for the ship and the number of the write, in a folder beside the
//! world file (or in memory, for a world that is not kept anywhere).
//!
//! A world is saved as a set: the files of its ships first, the world file last (see [`crate::WorldSave`]). The world file names every ship
//! file it needs, so the set that was written last and completely is the one that is read, however a save ended.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};

use voxel_space::GridId;

use crate::file::write_atomically;

/// What a ship file is called: the ship's number, a dash and the number of the write, then `.voxship` (`4294967296-12.voxship`). The writes
/// of a world are numbered from a counter that is saved with it, so no file of a world is given a name that an earlier one had, in this run
/// of the server or in another (a tick would be: the ticks begin again when the server does, and the world file on the disk may still name
/// the file that had that name).
///
/// A name can only be made from a ship and a number, or read in exactly that form, so a world file cannot send a program to a file outside
/// the folder of ships, whatever it says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShipFileName {
    pub grid: GridId,
    pub serial: u64,
}

const EXTENSION: &str = ".voxship";

impl ShipFileName {
    pub fn new(grid: GridId, serial: u64) -> Self {
        Self { grid, serial }
    }
}

impl fmt::Display for ShipFileName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}-{}{EXTENSION}", self.grid.get(), self.serial)
    }
}

/// Why some text is not the name of a ship file.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not the name of a ship file")]
pub struct NotAShipFileName(pub String);

impl FromStr for ShipFileName {
    type Err = NotAShipFileName;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let refuse = || NotAShipFileName(text.to_owned());
        let stem = text.strip_suffix(EXTENSION).ok_or_else(refuse)?;
        let (grid, serial) = stem.split_once('-').ok_or_else(refuse)?;
        // Digits and nothing else, written the way `Display` writes them: no sign, no spaces, no leading zeros, so each name has one spelling.
        let plain = |digits: &str| {
            !digits.is_empty()
                && digits.bytes().all(|byte| byte.is_ascii_digit())
                && (digits == "0" || !digits.starts_with('0'))
        };
        if !plain(grid) || !plain(serial) {
            return Err(refuse());
        }
        let grid = GridId::new(grid.parse().map_err(|_| refuse())?);
        // Only the ids of ships are named by files; a root grid is the terrain of a level.
        if grid.root_level().is_some() {
            return Err(refuse());
        }
        Ok(Self {
            grid,
            serial: serial.parse().map_err(|_| refuse())?,
        })
    }
}

/// A place ship files are kept. The calls are the whole of what the server and a save ask of the disk, so a test can put anything in
/// its place: a folder, memory, or a disk that fails.
pub trait ShipFiles: fmt::Debug + Send {
    /// The bytes of a file.
    fn read(&self, name: &ShipFileName) -> io::Result<Vec<u8>>;

    /// Writes a file, all or nothing: if it fails, a file of that name is as it was.
    fn write(&mut self, name: &ShipFileName, bytes: &[u8]) -> io::Result<()>;

    /// Removes a file. A file that is not there is not an error.
    fn remove(&mut self, name: &ShipFileName) -> io::Result<()>;

    /// The names of the ship files there are, in order. Files of other names are not listed (and are left alone).
    fn names(&self) -> io::Result<Vec<ShipFileName>>;
}

/// The files of ships in a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryShipFiles {
    directory: PathBuf,
}

impl DirectoryShipFiles {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    /// The folder for the ships of the world file at `world`: its whole name and then `.ships` (`saves/cave.vxw` has them in
    /// `saves/cave.vxw.ships`). The extension is not replaced, so that two world files that differ only in it do not share a folder, one of
    /// them cleaning away the ship files of the other.
    pub fn beside(world: &Path) -> Self {
        let mut name = world.file_name().unwrap_or_default().to_os_string();
        name.push(".ships");
        Self::new(world.with_file_name(name))
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    fn path(&self, name: &ShipFileName) -> PathBuf {
        self.directory.join(name.to_string())
    }
}

impl ShipFiles for DirectoryShipFiles {
    fn read(&self, name: &ShipFileName) -> io::Result<Vec<u8>> {
        fs::read(self.path(name))
    }

    fn write(&mut self, name: &ShipFileName, bytes: &[u8]) -> io::Result<()> {
        fs::create_dir_all(&self.directory)?;
        write_atomically(&self.path(name), bytes)
    }

    fn remove(&mut self, name: &ShipFileName) -> io::Result<()> {
        match fs::remove_file(self.path(name)) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    fn names(&self) -> io::Result<Vec<ShipFileName>> {
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut names = Vec::new();
        for entry in entries {
            if let Some(name) = entry?.file_name().to_str()
                && let Ok(name) = name.parse::<ShipFileName>()
            {
                names.push(name);
            }
        }
        names.sort();
        Ok(names)
    }
}

/// The files of ships in memory: for a world that is not kept anywhere, and for tests. Clones share the files, so a test can keep one and give
/// another to the server and look at what the server did.
#[derive(Clone, Debug, Default)]
pub struct MemoryShipFiles {
    files: Arc<Mutex<BTreeMap<ShipFileName, Vec<u8>>>>,
}

impl MemoryShipFiles {
    pub fn new() -> Self {
        Self::default()
    }

    fn files(&self) -> MutexGuard<'_, BTreeMap<ShipFileName, Vec<u8>>> {
        self.files
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A copy that does not share with this one: what a disk looks like at a moment, for a test that lets a save go wrong in many ways from
    /// the same beginning.
    pub fn duplicate(&self) -> Self {
        Self {
            files: Arc::new(Mutex::new(self.files().clone())),
        }
    }

    /// How many files there are.
    pub fn len(&self) -> usize {
        self.files().len()
    }

    pub fn is_empty(&self) -> bool {
        self.files().is_empty()
    }
}

impl ShipFiles for MemoryShipFiles {
    fn read(&self, name: &ShipFileName) -> io::Result<Vec<u8>> {
        self.files()
            .get(name)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("no file {name}")))
    }

    fn write(&mut self, name: &ShipFileName, bytes: &[u8]) -> io::Result<()> {
        self.files().insert(*name, bytes.to_vec());
        Ok(())
    }

    fn remove(&mut self, name: &ShipFileName) -> io::Result<()> {
        self.files().remove(name);
        Ok(())
    }

    fn names(&self) -> io::Result<Vec<ShipFileName>> {
        Ok(self.files().keys().copied().collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    fn name(grid: u64, serial: u64) -> ShipFileName {
        ShipFileName::new(GridId::new(grid), serial)
    }

    const FIRST: u64 = GridId::FIRST_ALLOCATED;

    #[test]
    fn a_name_is_the_ship_and_the_number_of_the_write_and_reads_back_as_them() {
        let made = name(FIRST + 3, 1520);
        assert_eq!(made.to_string(), "4294967299-1520.voxship");
        assert_eq!("4294967299-1520.voxship".parse(), Ok(made));
        assert_eq!(name(FIRST, 0).to_string(), "4294967296-0.voxship");
        assert_eq!("4294967296-0.voxship".parse(), Ok(name(FIRST, 0)));
        // The largest ship and write there are.
        let largest = name(u64::MAX, u64::MAX);
        assert_eq!(largest.to_string().parse(), Ok(largest));
    }

    #[test]
    fn only_that_spelling_is_a_name_so_no_name_leads_anywhere_else() {
        for text in [
            "",
            ".voxship",
            "4294967296.voxship",
            "4294967296-.voxship",
            "-5.voxship",
            "4294967296-5",
            "4294967296-5.vxw",
            "4294967296-5.voxship.saving",
            "04294967296-5.voxship",
            "4294967296-05.voxship",
            "+4294967296-5.voxship",
            "4294967296-5 .voxship",
            " 4294967296-5.voxship",
            "4294967296-5-6.voxship",
            "../4294967296-5.voxship",
            "sub/4294967296-5.voxship",
            "sub\\4294967296-5.voxship",
            "4294967296-18446744073709551616.voxship",
            "18446744073709551616-5.voxship",
            "٣-٤.voxship",
            // The root grids are the terrain of levels, not ships.
            "0-5.voxship",
            "4294967295-5.voxship",
        ] {
            assert!(text.parse::<ShipFileName>().is_err(), "{text:?}");
        }
    }

    fn scratch_directory() -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let directory = std::env::temp_dir().join(format!(
            "voxunami_ship_files_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn the_folder_of_a_world_is_beside_it_and_made_when_the_first_ship_is_written() {
        let world = Path::new("saves").join("cave.vxw");
        assert_eq!(
            DirectoryShipFiles::beside(&world).directory(),
            Path::new("saves").join("cave.vxw.ships")
        );
        // A world file of the same name and another extension has a folder of its own.
        assert_ne!(
            DirectoryShipFiles::beside(&world).directory(),
            DirectoryShipFiles::beside(&Path::new("saves").join("cave.save")).directory()
        );
        let base = scratch_directory();
        let mut files = DirectoryShipFiles::new(base.join("cave.ships"));
        assert_eq!(files.names().unwrap(), []);
        files.write(&name(FIRST, 7), b"first").unwrap();
        assert_eq!(files.read(&name(FIRST, 7)).unwrap(), b"first");
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_folder_lists_ship_files_in_order_and_leaves_everything_else_alone() {
        let base = scratch_directory();
        let mut files = DirectoryShipFiles::new(&base);
        files.write(&name(FIRST + 1, 3), b"b").unwrap();
        files.write(&name(FIRST, 9), b"a").unwrap();
        files.write(&name(FIRST, 10), b"c").unwrap();
        fs::write(base.join("notes.txt"), b"mine").unwrap();
        fs::write(base.join("4294967296-5.voxship.saving"), b"half").unwrap();
        assert_eq!(
            files.names().unwrap(),
            [name(FIRST, 9), name(FIRST, 10), name(FIRST + 1, 3)]
        );
        files.remove(&name(FIRST, 9)).unwrap();
        // Removing a file that is not there is not an error.
        files.remove(&name(FIRST, 9)).unwrap();
        assert_eq!(
            files.read(&name(FIRST, 9)).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(fs::read(base.join("notes.txt")).unwrap(), b"mine");
        assert!(base.join("4294967296-5.voxship.saving").exists());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn writing_a_file_again_replaces_it_whole() {
        let base = scratch_directory();
        let mut files = DirectoryShipFiles::new(&base);
        files
            .write(&name(FIRST, 1), b"a long first version")
            .unwrap();
        files.write(&name(FIRST, 1), b"short").unwrap();
        assert_eq!(files.read(&name(FIRST, 1)).unwrap(), b"short");
        assert_eq!(files.names().unwrap(), [name(FIRST, 1)]);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_folder_that_is_not_there_has_no_ships() {
        let base = scratch_directory();
        let files = DirectoryShipFiles::new(base.join("nothing_here"));
        assert_eq!(files.names().unwrap(), []);
        assert_eq!(
            files.read(&name(FIRST, 1)).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn memory_files_behave_as_a_folder_does_and_clones_share_them() {
        let mut files = MemoryShipFiles::new();
        let other = files.clone();
        assert!(files.is_empty());
        files.write(&name(FIRST + 2, 5), b"x").unwrap();
        files.write(&name(FIRST, 5), b"y").unwrap();
        assert_eq!(other.len(), 2);
        assert_eq!(other.names().unwrap(), [name(FIRST, 5), name(FIRST + 2, 5)]);
        assert_eq!(other.read(&name(FIRST, 5)).unwrap(), b"y");
        let mut other = other;
        other.remove(&name(FIRST, 5)).unwrap();
        other.remove(&name(FIRST, 5)).unwrap();
        assert_eq!(files.names().unwrap(), [name(FIRST + 2, 5)]);
        assert_eq!(
            files.read(&name(FIRST, 5)).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}

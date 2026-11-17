//! The coarse nodes of a ship, kept in a file of their own beside the ship's (`docs/specs/far-nodes-spec.md`, section 8).
//!
//! Terrain's nodes are never saved: a generator can make any of them again. A ship that nothing generates has no such thing, and making its nodes takes all of
//! its bricks, which for a very large ship is more than anyone wants to read each time the game starts. So they are kept, and the file that holds them is
//! **named for the ship file it was made from** (`4294967296-12.voxfar` beside `4294967296-12.voxship`) and says that file's checksum, so that nodes of another
//! state of the ship are never taken for these (the world file knows the checksum of every ship file, and so a ship in a file is not read to check it).
//!
//! A damaged or missing file, one made from another ship file, or one of blocks this program does not have, costs the far view of that ship and nothing else:
//! [`read_far_nodes`] says which and the ship is as it was. Nothing here is the ship's data.
//!
//! The file is a [container](crate::container) of three sections, at version 1:
//! - `engine:far_header`: the checksum of the ship file the nodes were made from (`u32`).
//! - `engine:far_blocks`: the identifiers of the blocks the cells hold, in the order they are first used. A file holds names, never the numbers a program gave
//!   the blocks in one run (the rule of every saved file).
//! - `engine:far_nodes`: a count, then for each node, coarsest first: the origin of its cube (three wide integers), its detail level (1 or more), and its cells
//!   as [`coarse_cells::node_codec`] writes them: a block (an index into the blocks above), how much is filled, how bright, and the exposure.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use coarse_cells::{CoarseCell, CoarseNode, MAXIMUM_DETAIL_LEVEL, node_codec};
use namespaced_id::NamespacedId;
use protocol::{read_brick_coordinate, write_brick_coordinate};
use region_octree::BrickCube;
use voxel_space::{BrickCoordinate, GridId};

use crate::container::{ContainerError, Section, read_container, write_container};
use crate::file::write_atomically;

const VERSION: u32 = 1;

fn section_id(name: &str) -> NamespacedId {
    NamespacedId::new("engine", name).expect("engine section names are valid identifiers")
}

/// Why far nodes are not there to use. None of these is a loss of the ship.
#[derive(Debug, thiserror::Error)]
pub enum FarNodesError {
    #[error("the far nodes cannot be read: {0}")]
    Damaged(#[from] ContainerError),
    #[error("the far nodes are not well formed: {0}")]
    Malformed(#[from] DecodeError),
    #[error("the file has no section {0}")]
    MissingSection(NamespacedId),
    #[error(
        "the nodes were made from another state of the ship (checksum {found:#010x}, the ship's is {expected:#010x})"
    )]
    OfAnotherShipFile { expected: u32, found: u32 },
    #[error("the nodes hold the block {0}, which this program does not have")]
    UnknownBlock(NamespacedId),
    #[error("a cell holds block number {0}, which has no name")]
    BlockWithoutName(u32),
    #[error("the file cannot be read: {0}")]
    Io(#[from] io::Error),
}

/// The coarse nodes of one ship: the cubes (detail level 1 or more, in the ship's own coordinates) that have one, and what is in them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FarNodes {
    nodes: BTreeMap<FarKey, (BrickCube, CoarseNode)>,
}

/// Coarsest first, then by where: the order the file is written in.
type FarKey = (std::cmp::Reverse<u8>, [i128; 3]);

fn key_of(cube: BrickCube) -> FarKey {
    let origin = cube.origin();
    (
        std::cmp::Reverse(cube.level()),
        [origin.x(), origin.y(), origin.z()],
    )
}

impl FarNodes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keeps the node of a cube, returning the one it replaces. Cubes of detail level 0 are bricks and have no node.
    pub fn insert(&mut self, cube: BrickCube, node: CoarseNode) -> Option<CoarseNode> {
        assert!(
            (1..=MAXIMUM_DETAIL_LEVEL).contains(&cube.level()),
            "a node is of detail level 1 or more"
        );
        self.nodes
            .insert(key_of(cube), (cube, node))
            .map(|(_, replaced)| replaced)
    }

    pub fn remove(&mut self, cube: BrickCube) -> Option<CoarseNode> {
        self.nodes.remove(&key_of(cube)).map(|(_, node)| node)
    }

    pub fn get(&self, cube: BrickCube) -> Option<&CoarseNode> {
        self.nodes.get(&key_of(cube)).map(|(_, node)| node)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The nodes, coarsest first.
    pub fn iter(&self) -> impl Iterator<Item = (BrickCube, &CoarseNode)> {
        self.nodes.values().map(|(cube, node)| (*cube, node))
    }

    /// The nodes of detail level `level` or coarser, which is what a ship that is not in memory is drawn from.
    pub fn from_level(&self, level: u8) -> Self {
        Self {
            nodes: self
                .nodes
                .iter()
                .filter(|(_, (cube, _))| cube.level() >= level)
                .map(|(key, entry)| (*key, entry.clone()))
                .collect(),
        }
    }

    /// The bytes of the file for the ship file with checksum `ship_checksum`. `name_of` says what a block number is called; the same nodes and names always
    /// make the same bytes.
    pub fn encode(
        &self,
        ship_checksum: u32,
        name_of: impl Fn(u32) -> Option<NamespacedId>,
    ) -> Result<Vec<u8>, FarNodesError> {
        // The blocks, in the order they are first met, each written once.
        let mut table: Vec<NamespacedId> = Vec::new();
        let mut index_of: BTreeMap<u32, u64> = BTreeMap::new();
        let mut nodes = ByteWriter::new();
        nodes.write_varint(self.nodes.len() as u64);
        for (cube, node) in self.iter() {
            write_brick_coordinate(&mut nodes, cube.origin());
            nodes.write_u8(cube.level());
            let mut failure = None;
            node_codec::write_node(
                &mut nodes,
                node,
                |cell| cell,
                |writer, cell| {
                    let index = match index_of.get(&cell.block) {
                        Some(&index) => index,
                        None => match name_of(cell.block) {
                            Some(name) => {
                                let index = table.len() as u64;
                                table.push(name);
                                index_of.insert(cell.block, index);
                                index
                            }
                            None => {
                                failure = Some(cell.block);
                                0
                            }
                        },
                    };
                    writer.write_varint(index);
                    writer.write_u8(cell.fill);
                    writer.write_u8(cell.light);
                    writer.write_varint(u64::from(cell.exposure));
                },
            );
            if let Some(block) = failure {
                return Err(FarNodesError::BlockWithoutName(block));
            }
        }
        let mut header = ByteWriter::new();
        header.write_u32(ship_checksum);
        let mut blocks = ByteWriter::new();
        blocks.write_varint(table.len() as u64);
        for name in &table {
            blocks.write_identifier(name);
        }
        let section = |name: &str, writer: ByteWriter| Section {
            identifier: section_id(name),
            version: VERSION,
            payload: writer.into_bytes(),
        };
        Ok(write_container(&[
            section("far_header", header),
            section("far_blocks", blocks),
            section("far_nodes", nodes),
        ]))
    }

    /// The nodes in a file, if it is whole, was made from the ship file with checksum `ship_checksum`, and holds only blocks `number_of` knows.
    pub fn decode(
        bytes: &[u8],
        ship_checksum: u32,
        number_of: impl Fn(&NamespacedId) -> Option<u32>,
    ) -> Result<Self, FarNodesError> {
        let sections = read_container(bytes)?;
        let payload = |name: &str| -> Result<&[u8], FarNodesError> {
            let identifier = section_id(name);
            sections
                .iter()
                .find(|section| section.identifier == identifier)
                .filter(|section| section.version == VERSION)
                .map(|section| section.payload.as_slice())
                .ok_or(FarNodesError::MissingSection(identifier))
        };
        let mut header = ByteReader::new(payload("far_header")?);
        let found = header.read_u32()?;
        header.finish()?;
        if found != ship_checksum {
            return Err(FarNodesError::OfAnotherShipFile {
                expected: ship_checksum,
                found,
            });
        }
        let mut blocks = ByteReader::new(payload("far_blocks")?);
        let count = blocks.read_count(2)?;
        let mut numbers = Vec::with_capacity(count);
        for _ in 0..count {
            let name = blocks.read_identifier()?;
            numbers.push(number_of(&name).ok_or(FarNodesError::UnknownBlock(name))?);
        }
        blocks.finish()?;

        let mut reader = ByteReader::new(payload("far_nodes")?);
        // A cube (three coordinates of a byte, a level) and a node (a form and a cell of three bytes).
        let count = reader.read_count(9)?;
        let mut nodes = Self::new();
        for _ in 0..count {
            let origin: BrickCoordinate = read_brick_coordinate(&mut reader)?;
            let level = reader.read_u8()?;
            if level == 0 || level > MAXIMUM_DETAIL_LEVEL {
                return Err(
                    DecodeError::Invalid("a node has a detail level that is not one").into(),
                );
            }
            let cube = BrickCube::new(origin, level)
                .map_err(|_| DecodeError::Invalid("a node is not aligned to its size"))?;
            let tag = reader.read_u8()?;
            let node = node_codec::read_node(&mut reader, tag, |reader| {
                let index = reader.read_varint()?;
                let block = usize::try_from(index)
                    .ok()
                    .and_then(|index| numbers.get(index))
                    .copied()
                    .ok_or(DecodeError::Invalid(
                        "a cell has a block that is not listed",
                    ))?;
                Ok(CoarseCell {
                    block,
                    fill: reader.read_u8()?,
                    light: reader.read_u8()?,
                    exposure: u32::try_from(reader.read_varint()?)
                        .map_err(|_| DecodeError::Invalid("an exposure is out of range"))?,
                })
            })?;
            if nodes.insert(cube, node).is_some() {
                return Err(DecodeError::Invalid("a node is in the file twice").into());
            }
        }
        reader.finish()?;
        Ok(nodes)
    }
}

/// What reading the far nodes of a ship came to. Only the first has anything to use; the others cost the ship its far view, and say why.
#[derive(Debug)]
pub enum FarNodesRead {
    Found(FarNodes),
    /// No file: the ship's nodes were never kept.
    Absent,
    Unusable(FarNodesError),
}

/// Reads the far nodes kept for the ship file with checksum `ship_checksum`, whatever state the file is in.
pub fn read_far_nodes(
    files: &dyn FarFiles,
    name: &FarFileName,
    ship_checksum: u32,
    number_of: impl Fn(&NamespacedId) -> Option<u32>,
) -> FarNodesRead {
    let bytes = match files.read(name) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return FarNodesRead::Absent,
        Err(error) => return FarNodesRead::Unusable(error.into()),
    };
    match FarNodes::decode(&bytes, ship_checksum, number_of) {
        Ok(nodes) => FarNodesRead::Found(nodes),
        Err(error) => FarNodesRead::Unusable(error),
    }
}

/// Removes the far nodes files that are not made from a ship file in `named` (the ship files a world file names): the ship was put away again, or lost, and
/// its nodes are of a state it is no longer in. Returns how many were removed. A file that cannot be removed stays for the next time.
pub fn remove_far_files_without_ship_file(
    files: &mut dyn FarFiles,
    named: &std::collections::BTreeSet<crate::ShipFileName>,
) -> io::Result<usize> {
    let mut removed = 0;
    for name in files.names()? {
        let ship = crate::ShipFileName::new(name.grid, name.serial);
        if !named.contains(&ship) && files.remove(&name).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// What a far nodes file is called: the name of the ship file it was made from, with `.voxfar` in place of `.voxship`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FarFileName {
    pub grid: GridId,
    pub serial: u64,
}

const EXTENSION: &str = ".voxfar";

impl FarFileName {
    pub fn new(grid: GridId, serial: u64) -> Self {
        Self { grid, serial }
    }

    /// The name of the far nodes made from the ship file `ship`.
    pub fn of(ship: &crate::ShipFileName) -> Self {
        Self::new(ship.grid, ship.serial)
    }
}

impl fmt::Display for FarFileName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}-{}{EXTENSION}", self.grid.get(), self.serial)
    }
}

/// Why some text is not the name of a far nodes file.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not the name of a far nodes file")]
pub struct NotAFarFileName(pub String);

impl FromStr for FarFileName {
    type Err = NotAFarFileName;

    /// Only the spelling `Display` writes, so that a name cannot lead out of the folder and each file has one name: it is the name of a ship file, read by the
    /// same rules, with another extension.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let stem = text
            .strip_suffix(EXTENSION)
            .ok_or_else(|| NotAFarFileName(text.to_owned()))?;
        let ship: crate::ShipFileName = format!("{stem}.voxship")
            .parse()
            .map_err(|_| NotAFarFileName(text.to_owned()))?;
        Ok(Self::of(&ship))
    }
}

/// A place far nodes files are kept (the folder of the ship files, or memory, or a disk that fails in a test).
pub trait FarFiles: fmt::Debug + Send {
    fn read(&self, name: &FarFileName) -> io::Result<Vec<u8>>;

    /// Writes a file, all or nothing.
    fn write(&mut self, name: &FarFileName, bytes: &[u8]) -> io::Result<()>;

    /// Removes a file. A file that is not there is not an error.
    fn remove(&mut self, name: &FarFileName) -> io::Result<()>;

    /// The names of the far nodes files there are, in order. Files of other names are not listed (and are left alone).
    fn names(&self) -> io::Result<Vec<FarFileName>>;
}

/// The far nodes files in a folder (the one the ship files of the world are in: they have different extensions).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryFarFiles {
    directory: PathBuf,
}

impl DirectoryFarFiles {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    fn path(&self, name: &FarFileName) -> PathBuf {
        self.directory.join(name.to_string())
    }
}

impl FarFiles for DirectoryFarFiles {
    fn read(&self, name: &FarFileName) -> io::Result<Vec<u8>> {
        fs::read(self.path(name))
    }

    fn write(&mut self, name: &FarFileName, bytes: &[u8]) -> io::Result<()> {
        fs::create_dir_all(&self.directory)?;
        write_atomically(&self.path(name), bytes)
    }

    fn remove(&mut self, name: &FarFileName) -> io::Result<()> {
        match fs::remove_file(self.path(name)) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    fn names(&self) -> io::Result<Vec<FarFileName>> {
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut names = Vec::new();
        for entry in entries {
            if let Some(name) = entry?.file_name().to_str()
                && let Ok(name) = name.parse::<FarFileName>()
            {
                names.push(name);
            }
        }
        names.sort();
        Ok(names)
    }
}

/// Far nodes files in memory (clones share them), for tests and for a world that is kept nowhere.
#[derive(Clone, Debug, Default)]
pub struct MemoryFarFiles {
    files: Arc<Mutex<BTreeMap<FarFileName, Vec<u8>>>>,
}

impl MemoryFarFiles {
    pub fn new() -> Self {
        Self::default()
    }

    fn files(&self) -> MutexGuard<'_, BTreeMap<FarFileName, Vec<u8>>> {
        self.files
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl FarFiles for MemoryFarFiles {
    fn read(&self, name: &FarFileName) -> io::Result<Vec<u8>> {
        self.files()
            .get(name)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("no file {name}")))
    }

    fn write(&mut self, name: &FarFileName, bytes: &[u8]) -> io::Result<()> {
        self.files().insert(*name, bytes.to_vec());
        Ok(())
    }

    fn remove(&mut self, name: &FarFileName) -> io::Result<()> {
        self.files().remove(name);
        Ok(())
    }

    fn names(&self) -> io::Result<Vec<FarFileName>> {
        Ok(self.files().keys().copied().collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use proptest::prelude::*;

    use super::*;

    const SHIP_FILE_CHECKSUM: u32 = 0xDEAD_BEEF;

    fn name(number: u32) -> Option<NamespacedId> {
        let names = ["base_blocks:air", "base_blocks:stone", "base_blocks:grass"];
        names.get(number as usize).map(|name| name.parse().unwrap())
    }

    fn number(id: &NamespacedId) -> Option<u32> {
        (0..3).find(|&number| name(number).as_ref() == Some(id))
    }

    fn cube(x: i128, level: u8) -> BrickCube {
        BrickCube::new(BrickCoordinate::new(x, 0, -(1_i128 << level)), level)
            .expect("an aligned cube")
    }

    fn cell(block: u32, fill: u8, exposure: u32) -> CoarseCell {
        CoarseCell {
            block,
            fill,
            light: 200,
            exposure,
        }
    }

    /// A few nodes of different levels and forms: a uniform one, and one of two kinds of cell with exposure.
    fn some_nodes() -> FarNodes {
        let mut nodes = FarNodes::new();
        nodes.insert(cube(0, 6), CoarseNode::uniform(cell(0, 0, 0)));
        nodes.insert(
            cube(64, 6),
            CoarseNode::from_cells(
                (0..coarse_cells::CELLS_PER_NODE)
                    .map(|index| {
                        if index % 5 < 2 {
                            cell(1, 255, 300 + index as u32)
                        } else {
                            cell(2, 40, 1)
                        }
                    })
                    .collect(),
            ),
        );
        nodes.insert(cube(8, 3), CoarseNode::uniform(cell(2, 128, 77)));
        nodes
    }

    #[test]
    fn nodes_keep_everything_through_the_file_and_the_file_is_the_same_bytes_for_the_same_nodes() {
        let nodes = some_nodes();
        let bytes = nodes.encode(SHIP_FILE_CHECKSUM, name).unwrap();
        assert_eq!(bytes, nodes.encode(SHIP_FILE_CHECKSUM, name).unwrap());
        let read = FarNodes::decode(&bytes, SHIP_FILE_CHECKSUM, number).unwrap();
        assert_eq!(read, nodes);
        // Coarsest first, as written.
        let levels: Vec<u8> = read.iter().map(|(cube, _)| cube.level()).collect();
        assert_eq!(levels, [6, 6, 3]);
        // An empty set of nodes is a file too.
        let empty = FarNodes::new().encode(SHIP_FILE_CHECKSUM, name).unwrap();
        assert!(
            FarNodes::decode(&empty, SHIP_FILE_CHECKSUM, number)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_file_holds_the_names_of_blocks_and_not_the_numbers_a_run_gave_them() {
        let nodes = some_nodes();
        let bytes = nodes.encode(SHIP_FILE_CHECKSUM, name).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        for id in ["base_blocks:air", "base_blocks:stone", "base_blocks:grass"] {
            assert!(text.contains(id), "{id}");
        }
        // A program that numbers the blocks differently reads them as its own numbers.
        let renumbered = |id: &NamespacedId| number(id).map(|number| number + 10);
        let read = FarNodes::decode(&bytes, SHIP_FILE_CHECKSUM, renumbered).unwrap();
        let blocks: std::collections::BTreeSet<u32> = read
            .iter()
            .flat_map(|(_, node)| node.cells().map(|cell| cell.block).collect::<Vec<_>>())
            .collect();
        assert_eq!(blocks, [10, 11, 12].into());
        // Only the blocks the nodes use are named.
        let mut uniform = FarNodes::new();
        uniform.insert(cube(0, 4), CoarseNode::uniform(cell(2, 1, 0)));
        let bytes = uniform.encode(SHIP_FILE_CHECKSUM, name).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("base_blocks:grass") && !text.contains("base_blocks:stone"));
    }

    #[test]
    fn nodes_made_from_another_state_of_the_ship_or_of_blocks_that_are_not_here_are_not_used() {
        let bytes = some_nodes().encode(SHIP_FILE_CHECKSUM, name).unwrap();
        assert!(matches!(
            FarNodes::decode(&bytes, SHIP_FILE_CHECKSUM + 1, number),
            Err(FarNodesError::OfAnotherShipFile {
                expected,
                found: SHIP_FILE_CHECKSUM
            }) if expected == SHIP_FILE_CHECKSUM + 1
        ));
        let without_grass = |id: &NamespacedId| number(id).filter(|&number| number != 2);
        assert!(matches!(
            FarNodes::decode(&bytes, SHIP_FILE_CHECKSUM, without_grass),
            Err(FarNodesError::UnknownBlock(id)) if id.as_str() == "base_blocks:grass"
        ));
        // And a block that has no name is not written.
        let mut nodes = FarNodes::new();
        nodes.insert(cube(0, 4), CoarseNode::uniform(cell(9, 1, 0)));
        assert!(matches!(
            nodes.encode(SHIP_FILE_CHECKSUM, name),
            Err(FarNodesError::BlockWithoutName(9))
        ));
    }

    #[test]
    fn a_file_that_is_cut_short_or_changed_anywhere_is_an_error_or_what_it_was_and_never_other_nodes()
     {
        let nodes = some_nodes();
        let bytes = nodes.encode(SHIP_FILE_CHECKSUM, name).unwrap();
        for length in 0..bytes.len() {
            assert!(
                FarNodes::decode(&bytes[..length], SHIP_FILE_CHECKSUM, number).is_err(),
                "cut to {length}"
            );
        }
        let mut longer = bytes.clone();
        longer.push(0);
        // (What follows the last section is not read by the container, and is not part of the nodes.)
        let _ = FarNodes::decode(&longer, SHIP_FILE_CHECKSUM, number);
        // A change in any byte, in each of eight ways.
        for index in 0..bytes.len() {
            for flip in [1_u8, 2, 4, 8, 16, 32, 64, 128] {
                let mut damaged = bytes.clone();
                damaged[index] ^= flip;
                match FarNodes::decode(&damaged, SHIP_FILE_CHECKSUM, number) {
                    Err(_) => {}
                    Ok(read) => assert_eq!(read, nodes, "byte {index} xor {flip}"),
                }
            }
        }
    }

    #[test]
    fn nodes_that_are_out_of_place_are_refused() {
        let bytes = some_nodes().encode(SHIP_FILE_CHECKSUM, name).unwrap();
        let sections = read_container(&bytes).unwrap();
        let nodes_section = |payload: Vec<u8>| {
            let sections: Vec<Section> = sections
                .iter()
                .map(|section| {
                    let mut section = section.clone();
                    if section.identifier == section_id("far_nodes") {
                        section.payload = payload.clone();
                    }
                    section
                })
                .collect();
            write_container(&sections)
        };
        let decode = |payload: Vec<u8>| {
            FarNodes::decode(&nodes_section(payload), SHIP_FILE_CHECKSUM, number)
        };
        let uniform = |level: u8, x: i128| {
            let mut writer = ByteWriter::new();
            writer.write_varint(1);
            write_brick_coordinate(&mut writer, BrickCoordinate::new(x, 0, 0));
            writer.write_u8(level);
            node_codec::write_node(
                &mut writer,
                &CoarseNode::uniform(cell(1, 1, 0)),
                |c| c,
                |w, c| {
                    w.write_varint(1);
                    w.write_u8(c.fill);
                    w.write_u8(c.light);
                    w.write_varint(u64::from(c.exposure));
                },
            );
            writer.into_bytes()
        };
        assert!(decode(uniform(4, 0)).is_ok());
        let mut followed = uniform(4, 0);
        followed.push(0);
        assert!(decode(followed).is_err(), "bytes after the last node");
        assert!(decode(uniform(0, 0)).is_err(), "a brick has no node");
        assert!(decode(uniform(MAXIMUM_DETAIL_LEVEL + 1, 0)).is_err());
        assert!(decode(uniform(4, 8)).is_err(), "not aligned to its size");
        // The same cube twice.
        let mut twice = uniform(4, 0);
        twice[0] = 2;
        twice.extend_from_slice(&uniform(4, 0)[1..]);
        assert!(decode(twice).is_err());
    }

    #[test]
    fn a_file_with_a_section_of_a_version_this_program_does_not_read_is_not_read() {
        let bytes = some_nodes().encode(SHIP_FILE_CHECKSUM, name).unwrap();
        let sections: Vec<Section> = read_container(&bytes)
            .unwrap()
            .into_iter()
            .map(|mut section| {
                if section.identifier == section_id("far_nodes") {
                    section.version = VERSION + 1;
                }
                section
            })
            .collect();
        assert!(matches!(
            FarNodes::decode(&write_container(&sections), SHIP_FILE_CHECKSUM, number),
            Err(FarNodesError::MissingSection(_))
        ));
    }

    proptest! {
        #[test]
        fn reading_arbitrary_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
            let _ = FarNodes::decode(&bytes, SHIP_FILE_CHECKSUM, number);
        }
    }

    #[test]
    fn the_nodes_from_a_level_up_are_those_of_that_level_and_coarser() {
        let nodes = some_nodes();
        assert_eq!(nodes.from_level(1).len(), 3);
        assert_eq!(nodes.from_level(4).len(), 2);
        assert_eq!(nodes.from_level(6).len(), 2, "the level itself is in");
        assert_eq!(nodes.from_level(7).len(), 0);
        assert!(nodes.from_level(4).get(cube(8, 3)).is_none());
        assert!(nodes.from_level(4).get(cube(0, 6)).is_some());
    }

    fn ship_file(grid: u64, serial: u64) -> crate::ShipFileName {
        crate::ShipFileName::new(GridId::new(grid), serial)
    }

    #[test]
    fn a_far_file_is_named_for_the_ship_file_it_was_made_from_and_only_that_spelling_is_a_name() {
        let made = FarFileName::of(&ship_file(GridId::FIRST_ALLOCATED + 3, 1520));
        assert_eq!(made.to_string(), "4294967299-1520.voxfar");
        assert_eq!("4294967299-1520.voxfar".parse(), Ok(made));
        for text in [
            "",
            ".voxfar",
            "4294967299-1520.voxship",
            "4294967299-1520",
            "4294967299-1520.voxfar.saving",
            "04294967299-1520.voxfar",
            "../4294967299-1520.voxfar",
            "sub/4294967299-1520.voxfar",
            "0-5.voxfar",
            "4294967299-.voxfar",
        ] {
            assert!(text.parse::<FarFileName>().is_err(), "{text:?}");
        }
    }

    fn scratch_directory() -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let directory = std::env::temp_dir().join(format!(
            "voxunami_far_files_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    /// Both kinds of store do the same.
    fn exercise(files: &mut dyn FarFiles) {
        let first = FarFileName::new(GridId::new(GridId::FIRST_ALLOCATED), 1);
        let second = FarFileName::new(GridId::new(GridId::FIRST_ALLOCATED), 2);
        assert!(matches!(
            read_far_nodes(files, &first, SHIP_FILE_CHECKSUM, number),
            FarNodesRead::Absent
        ));
        let nodes = some_nodes();
        files
            .write(&first, &nodes.encode(SHIP_FILE_CHECKSUM, name).unwrap())
            .unwrap();
        files.write(&second, b"not a far nodes file").unwrap();
        assert_eq!(files.names().unwrap(), [first, second]);
        match read_far_nodes(files, &first, SHIP_FILE_CHECKSUM, number) {
            FarNodesRead::Found(read) => assert_eq!(read, nodes),
            other => panic!("{other:?}"),
        }
        // A file that is damaged, and one made from another state of the ship, cost the nodes and nothing else.
        assert!(matches!(
            read_far_nodes(files, &second, SHIP_FILE_CHECKSUM, number),
            FarNodesRead::Unusable(FarNodesError::Damaged(_))
        ));
        assert!(matches!(
            read_far_nodes(files, &first, 5, number),
            FarNodesRead::Unusable(FarNodesError::OfAnotherShipFile { .. })
        ));
        // Written again, a file is the new one; removed, it is gone, and removing it again is not an error.
        files
            .write(&first, &FarNodes::new().encode(5, name).unwrap())
            .unwrap();
        assert!(matches!(
            read_far_nodes(files, &first, 5, number),
            FarNodesRead::Found(read) if read.is_empty()
        ));
        files.remove(&first).unwrap();
        files.remove(&first).unwrap();
        assert_eq!(files.names().unwrap(), [second]);
    }

    #[test]
    fn far_files_in_memory_are_written_read_listed_and_removed() {
        exercise(&mut MemoryFarFiles::new());
    }

    #[test]
    fn far_files_in_a_folder_are_written_read_listed_and_removed_and_files_of_other_names_are_left_alone()
     {
        let directory = scratch_directory();
        // The ship files of a world are in the same folder: they are not far files, and a far file is not a ship file.
        fs::write(directory.join("4294967296-1.voxship"), b"a ship").unwrap();
        fs::write(directory.join("notes.txt"), b"notes").unwrap();
        let mut files = DirectoryFarFiles::new(&directory);
        exercise(&mut files);
        assert!(directory.join("4294967296-1.voxship").exists());
        assert!(directory.join("notes.txt").exists());
        // A folder that is not there has no files, and the first write makes it.
        let mut later = DirectoryFarFiles::new(directory.join("later"));
        assert!(later.names().unwrap().is_empty());
        later
            .write(
                &FarFileName::new(GridId::new(GridId::FIRST_ALLOCATED), 0),
                b"x",
            )
            .unwrap();
        assert_eq!(later.names().unwrap().len(), 1);
        fs::remove_dir_all(&directory).unwrap();
    }
}

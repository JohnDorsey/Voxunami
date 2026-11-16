//! A folder of loose pictures that override the pictures of the world's blocks, on this client alone (`docs/hot-reload-plan.md` step 3; the first piece of the texture packs of
//! `docs/specs/block-textures-spec.md`, D9).
//!
//! The folder is `NAMESPACE/textures/NAME.png` for each picture, named as a pack's pictures are: a block whose base name is `test:stone` has `test/textures/stone_top.png`,
//! `stone_side.png` and `stone_bottom.png` (or `stone.png` for a face that has none of its own), and each picture may have a specular companion `stone_top_s.png` and a normal
//! companion `stone_top_n.png` (LabPBR). A face the folder has no picture for keeps the world's. Nothing here is sent anywhere or known to the server: it is how this client
//! draws, so a recording is unaffected.
//!
//! The pictures are the world's own places (a size and a layer among those of the size): a picture of the folder is written into the place of the face it is for, so it must be
//! as big as the world's picture there. Two faces that the world draws with one picture have one place, so one picture of the folder wins there (the first, top before side
//! before bottom) and the other is told as a problem.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use data_packs::{GREATEST_FILE_BYTES, Tile};
use renderer::{PictureWrite, VoxelRenderer};

use crate::package_data::PackageData;

/// The place of a picture in the world: its size's number and its layer among the pictures of the size.
pub type PicturePlace = (usize, usize);

/// What a folder says of the pictures: for each place it has a picture for, the picture (with its companions) that is to be there, and what was wrong with the files.
#[derive(Debug, Default)]
pub struct ReadPictures {
    pub overrides: BTreeMap<PicturePlace, Tile>,
    /// The places a file is for that is there and is no good: they are kept as they are, neither written nor given back to the world's.
    pub kept: BTreeSet<PicturePlace>,
    pub problems: Vec<String>,
}

/// What a face of a block is named in the folder, and which of the block's pictures it is.
const FACES: [&str; 3] = ["_top", "_side", "_bottom"];

/// Reads the pictures of this folder that are for the faces of the blocks of `package`. An error only if the folder cannot be read; a picture that is no good is a problem and
/// the others are read.
pub fn read_pictures(folder: &Path, package: &PackageData) -> Result<ReadPictures, String> {
    std::fs::read_dir(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let mut read = ReadPictures::default();
    let mut files = FileCache::new(folder);
    // (Each place with the face and block that asked for it, for the message of two that want one place.)
    let mut wanted_by: BTreeMap<PicturePlace, String> = BTreeMap::new();
    for (block, definition) in &package.blocks {
        let Some(definition) = definition else {
            continue;
        };
        let (Some(base), Some(pictures)) = (definition.texture(), definition.pictures()) else {
            continue;
        };
        for (suffix, tile) in FACES
            .iter()
            .zip([pictures.top, pictures.side, pictures.bottom])
        {
            let place = (tile.class(), tile.layer());
            let Some(world) = package
                .tile_classes
                .get(place.0)
                .and_then(|class| class.get(place.1))
            else {
                continue;
            };
            let picture = match files.find_picture(base, suffix, world, &mut read.problems) {
                Found::Picture(picture) => picture,
                Found::Nothing => continue,
                Found::Damaged => {
                    read.kept.insert(place);
                    continue;
                }
            };
            let face = format!("the {} of {block}", &suffix[1..]);
            match read.overrides.get(&place) {
                None => {
                    wanted_by.insert(place, face);
                    read.overrides.insert(place, picture);
                }
                Some(first) if *first == picture => {}
                Some(_) => read.problems.push(format!(
                    "{face} and {} share one picture in the world (size {}, picture {}), and the folder has two different pictures for them: the first is used",
                    wanted_by[&place], place.0, place.1
                )),
            }
        }
    }
    Ok(read)
}

/// What the folder has for a face.
enum Found {
    Picture(Tile),
    /// No file for the face.
    Nothing,
    /// A file for the face that is no good.
    Damaged,
}

/// What a file is.
enum Read {
    Missing,
    Damaged,
    Tile(Tile),
}

/// The pictures of a folder as they are read, each file once: a file that is no good is a problem once.
struct FileCache {
    folder: PathBuf,
    tiles: BTreeMap<PathBuf, Result<Tile, bool>>,
}

impl FileCache {
    fn new(folder: &Path) -> Self {
        Self {
            folder: folder.to_path_buf(),
            tiles: BTreeMap::new(),
        }
    }

    /// The picture for a face of the block whose base name is `base`, if the folder has a picture or a companion for it: the file for the face, and then the one with no ending.
    fn find_picture(
        &mut self,
        base: &namespaced_id::NamespacedId,
        face: &str,
        world: &Tile,
        problems: &mut Vec<String>,
    ) -> Found {
        let name = base.name();
        if name.contains("..") {
            return Found::Nothing;
        }
        let directory = self.folder.join(base.namespace()).join("textures");
        // (The face's own name first, then the base's: the first for which the folder has a picture or a companion of one is the face's.)
        for stem in [format!("{name}{face}"), name.to_owned()] {
            let mut file = |ending: &str| {
                self.read(
                    &directory.join(format!("{stem}{ending}.png")),
                    world.size(),
                    problems,
                )
            };
            let (picture, specular, normal) = (file(""), file("_s"), file("_n"));
            let files = [&picture, &specular, &normal];
            if files.iter().any(|read| matches!(read, Read::Damaged)) {
                return Found::Damaged;
            }
            let [picture, specular, normal] = files.map(|read| match read {
                Read::Tile(tile) => Some(tile.clone()),
                _ => None,
            });
            if picture.is_none() && specular.is_none() && normal.is_none() {
                continue;
            }
            let replaced = picture.is_some();
            let rgba = picture.as_ref().map_or(world.rgba(), Tile::rgba);
            let mut tile = Tile::from_rgba(world.size(), rgba.to_vec());
            // (A picture that is replaced takes its companions with it: the world's belonged to the picture that is gone.)
            let companion = |found: &Option<Tile>, worlds: Option<&[u8]>| match found {
                Some(found) => Some(found.rgba().to_vec()),
                None if replaced => None,
                None => worlds.map(<[u8]>::to_vec),
            };
            if let Some(pixels) = companion(&specular, world.specular()) {
                tile = tile.with_specular(pixels);
            }
            if let Some(pixels) = companion(&normal, world.normal()) {
                tile = tile.with_normal(pixels);
            }
            return Found::Picture(tile);
        }
        Found::Nothing
    }

    /// The tile in this file, if there is a file and it is a tile of this size; a file that is there and no good is a problem.
    fn read(&mut self, path: &Path, size: u32, problems: &mut Vec<String>) -> Read {
        let found = self
            .tiles
            .entry(path.to_path_buf())
            .or_insert_with(|| read_tile(path, problems))
            .clone();
        match found {
            Err(false) => Read::Missing,
            Err(true) => Read::Damaged,
            Ok(tile) if tile.size() != size => {
                problems.push(format!(
                    "{}: this picture is {} pixels across and the world's picture it is for is {size}",
                    path.display(),
                    tile.size()
                ));
                Read::Damaged
            }
            Ok(tile) => Read::Tile(tile),
        }
    }
}

/// The tile in a file; `Err(false)` if there is no such file, `Err(true)` if it is no good (which is a problem).
fn read_tile(path: &Path, problems: &mut Vec<String>) -> Result<Tile, bool> {
    let Some(metadata) = std::fs::metadata(path)
        .ok()
        .filter(|metadata| metadata.is_file())
    else {
        return Err(false);
    };
    if metadata.len() > GREATEST_FILE_BYTES as u64 {
        problems.push(format!(
            "{}: this file is {} bytes and a picture is at most {GREATEST_FILE_BYTES}",
            path.display(),
            metadata.len()
        ));
        return Err(true);
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            problems.push(format!("{}: {error}", path.display()));
            return Err(true);
        }
    };
    match Tile::from_png(&path.display().to_string(), &bytes) {
        Ok(tile) => Ok(tile),
        Err(error) => {
            problems.push(error.to_string());
            Err(true)
        }
    }
}

/// What a reload of pictures writes into: the renderer, or a stand-in in a test.
pub trait PictureApplier {
    /// As [`renderer::VoxelRenderer::set_picture`], with the reason as text.
    fn write(
        &mut self,
        class: usize,
        layer: usize,
        picture: &PictureWrite<'_>,
    ) -> Result<(), String>;
}

impl PictureApplier for VoxelRenderer {
    fn write(
        &mut self,
        class: usize,
        layer: usize,
        picture: &PictureWrite<'_>,
    ) -> Result<(), String> {
        self.set_picture(class, layer, picture)
            .map_err(|error| error.to_string())
    }
}

/// The places the renderer draws with a picture of the folder and not the world's, and what it was given for each.
#[derive(Debug, Default)]
pub struct PicturesInUse {
    written: BTreeMap<PicturePlace, Tile>,
}

fn write_of(tile: &Tile) -> PictureWrite<'_> {
    PictureWrite {
        rgba: tile.rgba(),
        specular: tile.specular(),
        normal: tile.normal(),
    }
}

/// Puts pictures that were read in use (the swap of a reload): what changed is written, and a place the folder no longer has a picture for gets the world's again. A write the
/// renderer refuses stops it with the reason; what was written before it is known, and the rest is tried again at the next. `Ok(None)` says nothing changed and nothing was wrong.
pub fn swap_pictures(
    applier: &mut impl PictureApplier,
    package: &PackageData,
    in_use: &mut PicturesInUse,
    folder: &Path,
    read: ReadPictures,
) -> Result<Option<String>, String> {
    let (mut written, mut restored) = (0, 0);
    for (&place, tile) in &read.overrides {
        if in_use.written.get(&place) == Some(tile) {
            continue;
        }
        applier
            .write(place.0, place.1, &write_of(tile))
            .map_err(|error| refused(place, &error))?;
        in_use.written.insert(place, tile.clone());
        written += 1;
    }
    let gone: Vec<PicturePlace> = in_use
        .written
        .keys()
        .filter(|place| !read.overrides.contains_key(place) && !read.kept.contains(place))
        .copied()
        .collect();
    for place in gone {
        let world = package
            .tile_classes
            .get(place.0)
            .and_then(|class| class.get(place.1))
            .ok_or_else(|| format!("the world has no picture at {place:?} to give back"))?;
        applier
            .write(place.0, place.1, &write_of(world))
            .map_err(|error| refused(place, &error))?;
        in_use.written.remove(&place);
        restored += 1;
    }
    let problems = read.problems.len();
    for problem in &read.problems {
        eprintln!("warning: {problem}");
    }
    if written == 0 && restored == 0 && problems == 0 {
        return Ok(None);
    }
    let mut detail = format!(
        "{written} picture{} written from {}",
        if written == 1 { "" } else { "s" },
        folder.display()
    );
    if restored > 0 {
        detail.push_str(&format!(", {restored} back to the world's"));
    }
    if problems > 0 {
        detail.push_str(&format!(
            "; {problems} problem{} (on the error stream)",
            if problems == 1 { "" } else { "s" }
        ));
    }
    Ok(Some(detail))
}

fn refused(place: PicturePlace, reason: &str) -> String {
    format!(
        "the picture for size {}, picture {} was refused: {reason}",
        place.0, place.1
    )
}

/// Reads the pictures of this folder and puts them in use: for the start and for a reload.
pub fn use_pictures_from(
    applier: &mut impl PictureApplier,
    package: &PackageData,
    in_use: &mut PicturesInUse,
    folder: &Path,
) -> Result<Option<String>, String> {
    let read = read_pictures(folder, package)?;
    swap_pictures(applier, package, in_use, folder, read)
}

#[cfg(test)]
mod tests;

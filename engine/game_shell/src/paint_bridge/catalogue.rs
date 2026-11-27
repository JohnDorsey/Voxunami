//! Which pictures the world has, named for a tool that has them as files.
//!
//! A picture of the world is a place (its size's number and its layer among the pictures of that size). The tool is told each place as an id, with the names of the files it
//! may be: for a face of a block whose base name is `test:stone`, `stone_top` (or `_side`, `_bottom`) and then `stone`, which are the names a texture pack gives the files
//! (`crate::texture_pack`). One place may be the picture of several faces and blocks; it has all their names, in the order they were found, and the tool takes the first
//! it has a file for.

use std::collections::BTreeMap;

use block_models::TileRef;

use super::protocol::CatalogueTile;
use crate::package_data::PackageData;
use crate::texture_pack::PicturePlace;

/// What a face of a block is named: the ending of its file's name.
const FACES: [&str; 3] = ["_top", "_side", "_bottom"];

/// The pictures of a package, as the tool is told of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalogue {
    pub tiles: Vec<CatalogueTile>,
    ids: BTreeMap<PicturePlace, String>,
}

impl Catalogue {
    /// The id the tool knows a picture of the world by.
    pub fn id_of(&self, tile: TileRef) -> Option<&str> {
        self.ids
            .get(&(tile.class(), tile.layer()))
            .map(String::as_str)
    }

    /// The place of the world a tool's id is.
    pub fn place_of(&self, id: &str) -> Option<PicturePlace> {
        self.ids
            .iter()
            .find_map(|(place, known)| (known == id).then_some(*place))
    }
}

/// Names the pictures of the blocks of `package` (those that have a texture and pictures).
pub fn catalogue_of(package: &PackageData) -> Catalogue {
    let mut tiles: BTreeMap<PicturePlace, CatalogueTile> = BTreeMap::new();
    for (_, definition) in &package.blocks {
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
            let entry = tiles.entry(place).or_insert_with(|| CatalogueTile {
                id: format!("{}:{}", place.0, place.1),
                group: Some(base.namespace().to_owned()),
                files: Vec::new(),
                size: world.size(),
            });
            for name in [format!("{}{suffix}", base.name()), base.name().to_owned()] {
                if !entry.files.contains(&name) {
                    entry.files.push(name);
                }
            }
        }
    }
    Catalogue {
        ids: tiles
            .iter()
            .map(|(place, tile)| (*place, tile.id.clone()))
            .collect(),
        tiles: tiles.into_values().collect(),
    }
}

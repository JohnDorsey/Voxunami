//! Paint mode: what the game says to the tool each frame, and what it does with what the tool says back.

use std::collections::BTreeMap;
use std::net::SocketAddr;

use data_packs::Tile;

use super::catalogue::{Catalogue, catalogue_of};
use super::connection::ToolConnection;
use super::path::PaintPath;
use super::protocol::{FromTool, Pointer, ToTool};
use crate::package_data::PackageData;
use crate::session::SurfaceTarget;
use crate::texture_pack::{PictureApplier, PicturePlace, write_of};

/// Which buttons of the mouse are down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Buttons {
    pub left: bool,
    pub right: bool,
}

/// A game in paint mode, connected to a tool.
#[derive(Debug)]
pub struct Painter {
    connection: ToolConnection,
    catalogue: Catalogue,
    path: PaintPath,
    last_pointer: Option<Pointer>,
    /// What each place the tool has changed was last written as: a picture the tool sends has no companions, so the picture's own are kept.
    written: BTreeMap<PicturePlace, Tile>,
    told_gone: bool,
}

impl Painter {
    /// Connects to the tool at `address` and tells it who this is and which pictures the world has.
    pub fn connect(
        address: SocketAddr,
        host: &str,
        package: &PackageData,
    ) -> std::io::Result<Self> {
        let mut connection = ToolConnection::connect(address)?;
        let catalogue = catalogue_of(package);
        connection.send(&ToTool::Hello {
            host: host.to_owned(),
        });
        connection.send(&ToTool::Tiles(catalogue.tiles.clone()));
        Ok(Self {
            connection,
            catalogue,
            path: PaintPath::default(),
            last_pointer: None,
            written: BTreeMap::new(),
            told_gone: false,
        })
    }

    pub fn is_connected(&self) -> bool {
        self.connection.is_open()
    }

    /// Once a frame: takes in what the tool sent (writing its pictures with `applier`) and tells it where the pointer is (`target` is what the pointer is over, if
    /// anything; `buttons` are the mouse's). Returns what is to be told to the player.
    pub fn step(
        &mut self,
        package: &PackageData,
        target: Option<&SurfaceTarget>,
        buttons: Buttons,
        applier: &mut impl PictureApplier,
    ) -> Vec<String> {
        let mut notes = Vec::new();
        for message in self.connection.poll() {
            match message {
                Err(problem) => notes.push(format!(
                    "the tool sent a line that is not a message: {problem}"
                )),
                Ok(FromTool::Welcome { tool, .. }) => notes.push(format!("{tool} is connected")),
                Ok(FromTool::Known { ids, missing }) => notes.push(format!(
                    "the tool has files for {} of the world's {} pictures{}",
                    ids.len(),
                    self.catalogue.tiles.len(),
                    if missing.is_empty() {
                        String::new()
                    } else {
                        format!(" (not for {} of them)", missing.len())
                    }
                )),
                Ok(FromTool::Tile { id, size, rgba }) => {
                    if let Some(note) = self.write_picture(package, &id, size, rgba, applier) {
                        notes.push(note);
                    }
                }
                Ok(FromTool::Error { text }) => notes.push(format!("the tool: {text}")),
                Ok(FromTool::Unknown) => {}
            }
        }
        if !self.connection.is_open() {
            if !self.told_gone {
                self.told_gone = true;
                notes.push("the tool went away".to_owned());
            }
            return notes;
        }
        let pointer = self.pointer_over(package, target, buttons);
        if self.last_pointer.as_ref() != Some(&pointer) {
            self.connection.send(&ToTool::Pointer(pointer.clone()));
            self.last_pointer = Some(pointer);
        }
        // (Sent now, not at the next frame.)
        for message in self.connection.poll() {
            if let Err(problem) = message {
                notes.push(format!(
                    "the tool sent a line that is not a message: {problem}"
                ));
            }
        }
        notes
    }

    fn pointer_over(
        &mut self,
        package: &PackageData,
        target: Option<&SurfaceTarget>,
        buttons: Buttons,
    ) -> Pointer {
        let over = target.and_then(|target| {
            let painted = target.picture?;
            let id = self.catalogue.id_of(painted.picture.tile)?;
            let tile = painted.picture.tile;
            let size = package
                .tile_classes
                .get(tile.class())?
                .get(tile.layer())?
                .size();
            Some((target, painted, id.to_owned(), size))
        });
        match over {
            Some((target, painted, id, size)) => {
                let at = self.path.place(
                    target.hit.grid,
                    target.hit.voxel,
                    target.hit.surface.face,
                    target.hit.surface.point,
                    painted.picture,
                    size,
                );
                Pointer {
                    tile: Some(id),
                    x: at.x,
                    y: at.y,
                    left: buttons.left,
                    right: buttons.right,
                    segment: at.segment,
                }
            }
            None => {
                self.path.lift();
                Pointer {
                    tile: None,
                    x: 0,
                    y: 0,
                    left: buttons.left,
                    right: buttons.right,
                    segment: 0,
                }
            }
        }
    }

    /// Writes a picture the tool sent into the renderer, keeping the companions the picture had. Returns what is wrong, if anything.
    fn write_picture(
        &mut self,
        package: &PackageData,
        id: &str,
        size: u32,
        rgba: Vec<u8>,
        applier: &mut impl PictureApplier,
    ) -> Option<String> {
        let Some(place) = self.catalogue.place_of(id) else {
            return Some(format!(
                "the tool sent a picture the world does not have: {id}"
            ));
        };
        let world = package
            .tile_classes
            .get(place.0)
            .and_then(|class| class.get(place.1))?;
        if world.size() != size {
            return Some(format!(
                "the tool's picture {id} is {size} pixels across and the world's is {}",
                world.size()
            ));
        }
        let before = self.written.get(&place).unwrap_or(world);
        let mut tile = Tile::from_rgba(size, rgba);
        if let Some(specular) = before.specular() {
            tile = tile.with_specular(specular.to_vec());
        }
        if let Some(normal) = before.normal() {
            tile = tile.with_normal(normal.to_vec());
        }
        if let Err(reason) = applier.write(place.0, place.1, &write_of(&tile)) {
            return Some(format!("the picture {id} was refused: {reason}"));
        }
        self.written.insert(place, tile);
        None
    }

    /// The server replaced the package: the renderer's pictures were made again, so what the tool changed is written again over them, and the tool is told the new
    /// pictures.
    pub fn package_replaced(
        &mut self,
        package: &PackageData,
        applier: &mut impl PictureApplier,
    ) -> Vec<String> {
        self.catalogue = catalogue_of(package);
        self.connection
            .send(&ToTool::Tiles(self.catalogue.tiles.clone()));
        let mut notes = Vec::new();
        let places: Vec<PicturePlace> = self.written.keys().copied().collect();
        for place in places {
            let Some(tile) = self.written.get(&place) else {
                continue;
            };
            let same_size = package
                .tile_classes
                .get(place.0)
                .and_then(|class| class.get(place.1))
                .is_some_and(|world| world.size() == tile.size());
            if !same_size {
                self.written.remove(&place);
                continue;
            }
            if let Err(reason) = applier.write(place.0, place.1, &write_of(tile)) {
                notes.push(format!(
                    "a picture of the tool was not written again: {reason}"
                ));
            }
        }
        notes
    }
}

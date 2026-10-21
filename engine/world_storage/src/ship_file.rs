//! The file of one ship: its voxels, and where it was and how it was moving, in the same container as a world file, readable by itself.
//!
//! ```text
//! engine:header        the seed of the world it was made in, and the engine version
//! engine:registries    the tables of the registries its channels use (content ids are numbers in the bricks; these say what they are)
//! engine:channels      the layout of its voxels
//! engine:ship          the name of the level it was in, and its edited bricks as a level's are written
//! engine:grid_flight   optional: its pose, how fast it moved and turned, whether it was held, what its pilot's logout does
//! (any other)          sections of plugins, kept as they are
//! ```
//!
//! A ship file carries its own tables and channels so that it can be read in any world: its content is named, its numbers are the writing
//! world's, and loading translates them into the reading world's. `engine:grid_flight` is how a ship is put back where it was; a ship that is
//! loaded into another world, or a second time into the same one, ignores it and is put where its loader says, as a new ship.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use protocol::{read_grid_pose, write_grid_pose};
use voxel_grid::{Brick, ChannelValueKind, Grid, GridLayout};
use voxel_space::{BrickCoordinate, GridPose};

use crate::container::{ContainerError, Section, read_container, write_container};
use crate::engine_sections::{
    self as sections, ChannelRecord, RegistryTable, decode_channels, decode_header,
    decode_override_bricks, decode_registries, encode_channels, encode_header,
    encode_override_bricks, encode_registries,
};
use crate::moving_grids::{MovingGrid, PilotLogout};
use crate::translation::{SourceTranslator, TranslationError};
use crate::world::SaveError;

/// The section with the ship's voxels.
pub fn ship_id() -> NamespacedId {
    NamespacedId::new("engine", "ship").expect("engine section names are valid identifiers")
}

/// The section that says where the ship was and how it moved.
pub fn flight_id() -> NamespacedId {
    NamespacedId::new("engine", "grid_flight").expect("engine section names are valid identifiers")
}

/// Where a ship was, and how it was moving, when it was saved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShipFlight {
    pub pose: GridPose,
    pub linear_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub held: bool,
    pub on_pilot_logout: PilotLogout,
}

impl ShipFlight {
    /// What a ship was doing now, to save.
    pub fn of(ship: &MovingGrid) -> Self {
        Self {
            pose: ship.pose,
            linear_velocity: ship.linear_velocity,
            angular_velocity: ship.angular_velocity,
            held: ship.held,
            on_pilot_logout: ship.on_pilot_logout,
        }
    }
}

fn encode_flight(flight: &ShipFlight) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    write_grid_pose(&mut writer, &flight.pose);
    for component in flight
        .linear_velocity
        .iter()
        .chain(&flight.angular_velocity)
    {
        writer.write_u32(component.to_bits());
    }
    writer.write_u8(u8::from(flight.held));
    writer.write_u8(match flight.on_pilot_logout {
        PilotLogout::Stay => 0,
        PilotLogout::Vanish => 1,
    });
    writer.into_bytes()
}

fn decode_flight(payload: &[u8]) -> Result<ShipFlight, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let pose = read_grid_pose(&mut reader)?;
    let mut components = [0f32; 6];
    for component in &mut components {
        *component = f32::from_bits(reader.read_u32()?);
        if !component.is_finite() {
            return Err(DecodeError::Invalid("a speed is not a number"));
        }
    }
    let held = match reader.read_u8()? {
        0 => false,
        1 => true,
        _ => return Err(DecodeError::Invalid("a held flag is neither 0 nor 1")),
    };
    let on_pilot_logout = match reader.read_u8()? {
        0 => PilotLogout::Stay,
        1 => PilotLogout::Vanish,
        _ => return Err(DecodeError::Invalid("unknown pilot logout")),
    };
    reader.finish()?;
    Ok(ShipFlight {
        pose,
        linear_velocity: [components[0], components[1], components[2]],
        angular_velocity: [components[3], components[4], components[5]],
        held,
        on_pilot_logout,
    })
}

fn encode_ship(level: &NamespacedId, bricks: &[(BrickCoordinate, &Brick)]) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    writer.write_identifier(level);
    writer.write_bytes(&encode_override_bricks(bricks));
    writer.into_bytes()
}

fn decode_ship(
    payload: &[u8],
) -> Result<(NamespacedId, Vec<(BrickCoordinate, Brick)>), DecodeError> {
    let mut reader = ByteReader::new(payload);
    let level = reader.read_identifier()?;
    let bricks = decode_override_bricks(reader.read_bytes()?)?;
    reader.finish()?;
    Ok((level, bricks))
}

/// Why a ship file could not be made into a ship.
#[derive(Debug, thiserror::Error)]
pub enum ShipFileError {
    #[error(transparent)]
    Container(#[from] ContainerError),
    #[error("section {section} is malformed: {source}")]
    Section {
        section: NamespacedId,
        source: DecodeError,
    },
    #[error("section {section} is version {version}, newer than this program understands")]
    UnsupportedSection { section: NamespacedId, version: u32 },
    #[error("the file has no {0} section, so it is not the file of a ship")]
    MissingSection(NamespacedId),
    #[error("the file has channel {0}, which this world has no place for")]
    UnknownChannel(NamespacedId),
    #[error("channel {0} means something else in this world than in the file")]
    ChannelKindMismatch(NamespacedId),
    #[error("the file uses {identifier} of {registry}, which this world does not have")]
    UnknownContent {
        registry: NamespacedId,
        identifier: NamespacedId,
    },
    #[error("a registry is listed twice, or an entry is: {0}")]
    BadRegistry(NamespacedId),
    #[error("the file's tables do not account for its contents: {0}")]
    Translation(#[from] TranslationError),
    #[error("the ship's voxels are not laid out as the world's are")]
    LayoutMismatch,
}

/// A ship file read and checked, not yet put into a world's numbering.
#[derive(Debug)]
pub struct ParsedShip {
    seed: u64,
    registries: Vec<RegistryTable>,
    channels: Vec<ChannelRecord>,
    level: NamespacedId,
    bricks: Vec<(BrickCoordinate, Brick)>,
    flight: Option<ShipFlight>,
    sections: Vec<Section>,
}

impl ParsedShip {
    pub fn parse(bytes: &[u8]) -> Result<Self, ShipFileError> {
        let mut header = None;
        let mut registries = None;
        let mut channels = None;
        let mut ship = None;
        let mut flight = None;
        let mut opaque = Vec::new();
        for section in read_container(bytes)? {
            let known = [
                sections::header_id(),
                sections::registries_id(),
                sections::channels_id(),
                ship_id(),
                flight_id(),
            ]
            .contains(&section.identifier);
            if !known {
                opaque.push(section);
                continue;
            }
            if section.version != sections::SUPPORTED_VERSION {
                return Err(ShipFileError::UnsupportedSection {
                    section: section.identifier,
                    version: section.version,
                });
            }
            let wrap = |source| ShipFileError::Section {
                section: section.identifier.clone(),
                source,
            };
            let payload = &section.payload[..];
            if section.identifier == sections::header_id() {
                header = Some(decode_header(payload).map_err(wrap)?);
            } else if section.identifier == sections::registries_id() {
                registries = Some(decode_registries(payload).map_err(wrap)?);
            } else if section.identifier == sections::channels_id() {
                channels = Some(decode_channels(payload).map_err(wrap)?);
            } else if section.identifier == ship_id() {
                ship = Some(decode_ship(payload).map_err(wrap)?);
            } else {
                flight = Some(decode_flight(payload).map_err(wrap)?);
            }
        }
        let (level, bricks) = ship.ok_or_else(|| ShipFileError::MissingSection(ship_id()))?;
        Ok(Self {
            seed: header
                .ok_or_else(|| ShipFileError::MissingSection(sections::header_id()))?
                .world_seed,
            registries: registries
                .ok_or_else(|| ShipFileError::MissingSection(sections::registries_id()))?,
            channels: channels
                .ok_or_else(|| ShipFileError::MissingSection(sections::channels_id()))?,
            level,
            bricks,
            flight,
            sections: opaque,
        })
    }

    /// The seed of the world the ship was made in.
    pub fn world_seed(&self) -> u64 {
        self.seed
    }

    /// The name of the level it was in.
    pub fn level(&self) -> &NamespacedId {
        &self.level
    }

    /// Where it was and how it moved, if the file says (a file that was made to be shared may not).
    pub fn flight(&self) -> Option<ShipFlight> {
        self.flight
    }

    /// The content its voxels can have, as (registry, identifier) in order: what a world must know before the ship can be read into it.
    pub fn referenced_content(&self) -> Vec<(NamespacedId, NamespacedId)> {
        let mut used: Vec<&NamespacedId> = Vec::new();
        for channel in &self.channels {
            if let ChannelValueKind::Content { registry } = &channel.value_kind
                && !used.contains(&registry)
            {
                used.push(registry);
            }
        }
        self.registries
            .iter()
            .filter(|table| used.contains(&&table.kind))
            .flat_map(|table| {
                table
                    .entries
                    .iter()
                    .map(|entry| (table.kind.clone(), entry.clone()))
            })
            .collect()
    }

    /// Reads the ship into a world with this `layout`. `resolve` says what number a world has for a named piece of content (`None` for
    /// content it cannot give a number to, which refuses the file). Every channel of the file must be one of the layout's, meaning what it
    /// means in the file; a channel of the layout that the file lacks is at its default.
    pub fn into_contents(
        self,
        layout: &Arc<GridLayout>,
        mut resolve: impl FnMut(&NamespacedId, &NamespacedId) -> Option<u32>,
    ) -> Result<ShipContents, ShipFileError> {
        for record in &self.channels {
            let definition = layout
                .channel_index(&record.identifier)
                .map(|index| layout.definition(index))
                .ok_or_else(|| ShipFileError::UnknownChannel(record.identifier.clone()))?;
            if definition.value_kind != record.value_kind {
                return Err(ShipFileError::ChannelKindMismatch(
                    record.identifier.clone(),
                ));
            }
        }
        let mut seen: Vec<&NamespacedId> = Vec::new();
        let mut lengths = HashMap::new();
        let mut mappings: HashMap<NamespacedId, Vec<u32>> = HashMap::new();
        for table in &self.registries {
            if seen.contains(&&table.kind) {
                return Err(ShipFileError::BadRegistry(table.kind.clone()));
            }
            seen.push(&table.kind);
            // (A table that no channel's values are numbers of is not the reader's concern.)
            let used = self.channels.iter().any(|channel| {
                matches!(&channel.value_kind, ChannelValueKind::Content { registry } if *registry == table.kind)
            });
            if !used {
                continue;
            }
            lengths.insert(&table.kind, table.entries.len());
            let mapping = table
                .entries
                .iter()
                .map(|entry| {
                    resolve(&table.kind, entry).ok_or_else(|| ShipFileError::UnknownContent {
                        registry: table.kind.clone(),
                        identifier: entry.clone(),
                    })
                })
                .collect::<Result<Vec<u32>, _>>()?;
            mappings.insert(table.kind.clone(), mapping);
        }
        let translator = SourceTranslator::new(
            Arc::clone(layout),
            &self.channels,
            &lengths,
            mappings,
            Vec::new(),
        )?;
        let mut voxels = Grid::new(Arc::clone(layout));
        for (coordinate, brick) in self.bricks {
            voxels
                .insert_brick(coordinate, Arc::new(translator.translate_brick(brick)?))
                .expect("the translated brick has one storage per layout channel");
        }
        Ok(ShipContents {
            level: self.level,
            voxels,
            flight: self.flight,
            sections: self.sections,
        })
    }
}

/// A ship as a world can have it.
#[derive(Debug)]
pub struct ShipContents {
    /// The name of the level the ship was in.
    pub level: NamespacedId,
    pub voxels: Grid,
    pub flight: Option<ShipFlight>,
    /// The sections of plugins that the file had, as they were.
    pub sections: Vec<Section>,
}

/// What it takes to write the file of a ship and to read one of this world's: the world's numbering of its content, and its layout.
#[derive(Clone, Debug)]
pub struct ShipFormat {
    world_seed: u64,
    /// For each registry the layout's channels use, the identifiers of its entries in order of their numbers.
    tables: BTreeMap<NamespacedId, Vec<NamespacedId>>,
    numbers: HashMap<(NamespacedId, NamespacedId), u32>,
    layout: Arc<GridLayout>,
}

impl ShipFormat {
    /// `tables` has an entry for every registry a channel of `layout` uses (more are fine, and ignored).
    pub fn new(
        world_seed: u64,
        tables: &BTreeMap<NamespacedId, Vec<NamespacedId>>,
        layout: Arc<GridLayout>,
    ) -> Result<Self, SaveError> {
        let mut used = BTreeMap::new();
        let mut numbers = HashMap::new();
        for channel in layout.channels() {
            if let ChannelValueKind::Content { registry } = &channel.value_kind {
                let table = tables
                    .get(registry)
                    .ok_or_else(|| SaveError::MissingRegistry {
                        channel: channel.identifier.clone(),
                        registry: registry.clone(),
                    })?;
                if used.insert(registry.clone(), table.clone()).is_none() {
                    for (number, entry) in table.iter().enumerate() {
                        numbers.insert(
                            (registry.clone(), entry.clone()),
                            u32::try_from(number).expect("a registry has fewer than 2^32 entries"),
                        );
                    }
                }
            }
        }
        Ok(Self {
            world_seed,
            tables: used,
            numbers,
            layout,
        })
    }

    pub fn layout(&self) -> &Arc<GridLayout> {
        &self.layout
    }

    /// The file of a ship that is in the level called `level`. Its bytes are the same for the same ship, whatever the order things were
    /// done in.
    pub fn encode(
        &self,
        ship: &MovingGrid,
        level: &NamespacedId,
    ) -> Result<Vec<u8>, ShipFileError> {
        if **ship.voxels.layout() != *self.layout {
            return Err(ShipFileError::LayoutMismatch);
        }
        let channels: Vec<ChannelRecord> = self
            .layout
            .channels()
            .iter()
            .map(|channel| ChannelRecord {
                identifier: channel.identifier.clone(),
                default_value: channel.default_value,
                value_kind: channel.value_kind.clone(),
            })
            .collect();
        let tables: Vec<RegistryTable> = self
            .tables
            .iter()
            .map(|(kind, entries)| RegistryTable {
                kind: kind.clone(),
                entries: entries.clone(),
            })
            .collect();
        let coordinates = ship.voxels.sorted_brick_coordinates();
        let bricks: Vec<(BrickCoordinate, &Brick)> = coordinates
            .iter()
            .map(|&coordinate| {
                (
                    coordinate,
                    &**ship
                        .voxels
                        .brick(coordinate)
                        .expect("a listed brick exists"),
                )
            })
            .collect();
        let section = |identifier, payload| Section {
            identifier,
            version: sections::SUPPORTED_VERSION,
            payload,
        };
        let mut written = vec![
            section(
                sections::header_id(),
                encode_header(&sections::Header {
                    world_seed: self.world_seed,
                    engine_version: env!("CARGO_PKG_VERSION").to_owned(),
                }),
            ),
            section(sections::registries_id(), encode_registries(&tables)),
            section(sections::channels_id(), encode_channels(&channels)),
            section(ship_id(), encode_ship(level, &bricks)),
            section(flight_id(), encode_flight(&ShipFlight::of(ship))),
        ];
        written.extend(ship.sections.iter().cloned());
        Ok(write_container(&written))
    }

    /// Reads a file that this world wrote, or one whose content it has all of.
    pub fn decode(&self, bytes: &[u8]) -> Result<ShipContents, ShipFileError> {
        ParsedShip::parse(bytes)?.into_contents(&self.layout, |registry, identifier| {
            self.numbers
                .get(&(registry.clone(), identifier.clone()))
                .copied()
        })
    }
}

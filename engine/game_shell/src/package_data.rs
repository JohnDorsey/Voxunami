//! What a client needs to show a world, as the server sends it in `Welcome`: how every block looks
//! and which channels the view reads. Nothing here comes from the client's own copy of the game, so a
//! client draws what the server says, and a recording of the stream plays the same with any content
//! installed.
//!
//! The byte form is canonical (see `block_models`' codec): the same data is the same bytes.

use std::collections::{HashMap, HashSet};

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use block_models::{BlockDefinition, BlockTable, PropertyError, PropertySchema};
use content_registry::IdentifierRegistry;
use namespaced_id::NamespacedId;
use voxel_grid::GridLayout;
use world_view::ViewChannels;

/// Why the data a server sent cannot be used.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PackageError {
    #[error("the package is damaged: {0}")]
    Damaged(#[from] DecodeError),
    #[error("the package's blocks cannot be used: {0}")]
    Blocks(#[from] PropertyError),
    #[error("the package names the channel {0}, which the world does not have")]
    NoChannel(NamespacedId),
    #[error("the package is for the block registry {0}, which this game does not have")]
    NoBlockRegistry(NamespacedId),
}

/// The least bytes a block (an identifier and a presence flag) takes, for `read_count`.
const BLOCK_BYTES: usize = 3;

/// The data a client builds its view of a world from.
#[derive(Clone, Debug, PartialEq)]
pub struct PackageData {
    /// The properties blocks can set, the engine's and the game's.
    pub properties: PropertySchema,
    /// The kind of the block registry in world files (such as `my_game:block`).
    pub block_registry: NamespacedId,
    /// Every block of the world, in the order of their numbers, with its definition; `None` for a block
    /// whose plugin the server does not have (drawn as unknown).
    pub blocks: Vec<(NamespacedId, Option<BlockDefinition>)>,
    /// The channel holding each voxel's block, and where the game has them, its state and its light.
    pub block_channel: NamespacedId,
    pub state_channel: Option<NamespacedId>,
    pub light_channel: Option<NamespacedId>,
    /// How bright a face is that no light reaches, 0 to 255.
    pub ambient_light: u8,
}

impl PackageData {
    /// What every block and state looks like and how it fills its cell, by block number. An error if
    /// a definition sets a property the schema lacks, a value it does not take, or a state its block
    /// does not have.
    pub fn block_table(&self) -> Result<BlockTable, PropertyError> {
        let definitions: Vec<Option<&BlockDefinition>> = self
            .blocks
            .iter()
            .map(|(_, definition)| definition.as_ref())
            .collect();
        BlockTable::try_from_definitions(&self.properties, &definitions)
    }

    /// The same table, with the blocks numbered as `numbering` has them (a client numbers content its own
    /// way and learns the server's numbering only to translate it). A block the package has no definition
    /// for, because the server lacks its plugin or does not know it, is unknown.
    pub fn block_table_numbered_by(
        &self,
        numbering: &dyn IdentifierRegistry,
    ) -> Result<BlockTable, PropertyError> {
        let definitions: HashMap<&NamespacedId, &BlockDefinition> = self
            .blocks
            .iter()
            .filter_map(|(identifier, definition)| Some((identifier, definition.as_ref()?)))
            .collect();
        let numbered: Vec<Option<&BlockDefinition>> = (0..numbering.entry_count() as u32)
            .map(|raw| {
                let identifier = numbering.identifier_of_raw(raw)?;
                definitions.get(identifier).copied()
            })
            .collect();
        BlockTable::try_from_definitions(&self.properties, &numbered)
    }

    /// The channels the view reads, found by name in `layout`.
    pub fn view_channels(&self, layout: &GridLayout) -> Result<ViewChannels, PackageError> {
        let find = |identifier: &NamespacedId| {
            layout
                .channel_index(identifier)
                .ok_or_else(|| PackageError::NoChannel(identifier.clone()))
        };
        Ok(ViewChannels::new(
            find(&self.block_channel)?,
            self.state_channel.as_ref().map(find).transpose()?,
            self.light_channel.as_ref().map(find).transpose()?,
        ))
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        self.properties.encode(&mut writer);
        writer.write_identifier(&self.block_registry);
        writer.write_varint(self.blocks.len() as u64);
        for (identifier, definition) in &self.blocks {
            writer.write_identifier(identifier);
            match definition {
                Some(definition) => {
                    writer.write_u8(1);
                    definition.encode(&mut writer);
                }
                None => writer.write_u8(0),
            }
        }
        writer.write_identifier(&self.block_channel);
        for channel in [&self.state_channel, &self.light_channel] {
            match channel {
                Some(identifier) => {
                    writer.write_u8(1);
                    writer.write_identifier(identifier);
                }
                None => writer.write_u8(0),
            }
        }
        writer.write_u8(self.ambient_light);
        writer.into_bytes()
    }

    /// Reads what [`PackageData::encode`] wrote. An error for anything that would not have been written,
    /// including two blocks with one identifier.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(bytes);
        let properties = PropertySchema::decode(&mut reader)?;
        let block_registry = reader.read_identifier()?;
        let count = reader.read_count(BLOCK_BYTES)?;
        let mut blocks = Vec::with_capacity(count);
        let mut seen = HashSet::new();
        for _ in 0..count {
            let identifier = reader.read_identifier()?;
            if !seen.insert(identifier.clone()) {
                return Err(DecodeError::Invalid("two blocks have one identifier"));
            }
            let definition = match reader.read_u8()? {
                0 => None,
                1 => Some(BlockDefinition::decode(&mut reader)?),
                _ => return Err(DecodeError::Invalid("a block's flag is neither 0 nor 1")),
            };
            blocks.push((identifier, definition));
        }
        let block_channel = reader.read_identifier()?;
        let optional_channel = |reader: &mut ByteReader<'_>| match reader.read_u8()? {
            0 => Ok(None),
            1 => reader.read_identifier().map(Some),
            _ => Err(DecodeError::Invalid("a channel's flag is neither 0 nor 1")),
        };
        let state_channel = optional_channel(&mut reader)?;
        let light_channel = optional_channel(&mut reader)?;
        let ambient_light = reader.read_u8()?;
        reader.finish()?;
        Ok(Self {
            properties,
            block_registry,
            blocks,
            block_channel,
            state_channel,
            light_channel,
            ambient_light,
        })
    }
}

#[cfg(test)]
mod tests {
    use block_models::{Shape, engine::DENSITY};
    use proptest::prelude::*;

    use super::*;

    fn id(text: &str) -> NamespacedId {
        text.parse().unwrap()
    }

    fn sample() -> PackageData {
        PackageData {
            properties: PropertySchema::new(),
            block_registry: id("test_game:block"),
            blocks: vec![
                (id("test_game:air"), Some(BlockDefinition::air())),
                (
                    id("test_game:rock"),
                    Some(BlockDefinition::cube([0.5; 3]).with_property(&DENSITY, 2500)),
                ),
                (
                    id("test_game:slab"),
                    Some(
                        BlockDefinition::cube([0.1, 0.2, 0.3])
                            .with_shapes(vec![Shape::full(), Shape::empty()]),
                    ),
                ),
                (id("gone_plugin:thing"), None),
            ],
            block_channel: id("test_game:block"),
            state_channel: Some(id("test_game:state")),
            light_channel: Some(id("test_game:light")),
            ambient_light: 90,
        }
    }

    #[test]
    fn a_package_survives_the_trip_exactly_with_and_without_its_optional_channels() {
        let mut bare = sample();
        bare.state_channel = None;
        bare.light_channel = None;
        for package in [sample(), bare] {
            assert_eq!(PackageData::decode(&package.encode()), Ok(package));
        }
    }

    #[test]
    fn the_table_has_a_block_for_each_entry_and_a_block_without_a_definition_is_unknown() {
        let package = sample();
        let table = package.block_table().unwrap();
        assert_eq!(table.block_count(), 4);
        assert!(table.shape(table.model_of(0, 0)).is_empty());
        assert!(table.is_opaque_cube(table.model_of(1, 0)));
        assert_eq!(table.color(1), [0.5; 3]);
        assert_eq!(table.state_count(2), 2);
        assert_eq!(table.color(3), block_models::MISSING_BLOCK_COLOR);
        let density = table.handle::<u32>(&DENSITY).unwrap();
        assert_eq!(table.get_of(density, 1, 0), 2500);
    }

    #[test]
    fn a_definition_that_sets_a_property_the_schema_lacks_is_an_error_when_the_table_is_built() {
        let mut package = sample();
        package.properties = PropertySchema::decode(&mut ByteReader::new(&[0])).unwrap();
        assert_eq!(
            package.block_table().unwrap_err(),
            PropertyError::Undeclared(id("engine:density"))
        );
    }

    #[test]
    fn the_view_channels_are_found_by_name_in_the_layout_whatever_order_it_has_them_in() {
        use voxel_grid::{ChannelDefinition, GridLayout};
        let layout = GridLayout::new(vec![
            ChannelDefinition::number(id("test_game:light"), 0),
            ChannelDefinition::number(id("test_game:state"), 0),
            ChannelDefinition::content(id("test_game:block"), 0, id("test_game:block")),
        ])
        .unwrap();
        let channels = sample().view_channels(&layout).unwrap();
        assert_eq!(
            (
                channels.blocks.get(),
                channels.states.map(|c| c.get()),
                channels.light.map(|c| c.get())
            ),
            (2, Some(1), Some(0))
        );
        let mut bare = sample();
        bare.state_channel = None;
        bare.light_channel = None;
        let channels = bare.view_channels(&layout).unwrap();
        assert_eq!((channels.states, channels.light), (None, None));
        let mut missing = sample();
        missing.state_channel = Some(id("test_game:nothing"));
        assert_eq!(
            missing.view_channels(&layout),
            Err(PackageError::NoChannel(id("test_game:nothing")))
        );
    }

    #[test]
    fn a_package_cut_short_anywhere_or_followed_by_more_is_an_error() {
        let bytes = sample().encode();
        for length in 0..bytes.len() {
            assert!(
                PackageData::decode(&bytes[..length]).is_err(),
                "cut at {length}"
            );
        }
        let mut longer = bytes;
        longer.push(0);
        assert_eq!(
            PackageData::decode(&longer),
            Err(DecodeError::TrailingBytes(1))
        );
    }

    #[test]
    fn two_blocks_with_one_identifier_are_refused() {
        let mut package = sample();
        package.blocks[2].0 = id("test_game:rock");
        assert_eq!(
            PackageData::decode(&package.encode()),
            Err(DecodeError::Invalid("two blocks have one identifier"))
        );
    }

    #[test]
    fn a_flag_that_is_not_0_or_1_is_refused() {
        let mut package = sample();
        package.blocks.truncate(1);
        package.state_channel = None;
        package.light_channel = None;
        let bytes = package.encode();
        // The block's flag is right after the schema, the registry kind, the count and the identifier.
        let mut prefix = ByteWriter::new();
        package.properties.encode(&mut prefix);
        prefix.write_identifier(&package.block_registry);
        prefix.write_varint(1);
        prefix.write_identifier(&package.blocks[0].0);
        let flag_at = prefix.into_bytes().len();
        assert_eq!(bytes[flag_at], 1, "the control: this is the flag");
        let mut damaged = bytes;
        damaged[flag_at] = 2;
        assert_eq!(
            PackageData::decode(&damaged),
            Err(DecodeError::Invalid("a block's flag is neither 0 nor 1"))
        );
    }

    proptest! {
        #[test]
        fn bytes_that_are_not_a_package_are_an_error_never_a_panic_and_those_that_are_have_one_spelling(
            bytes in prop::collection::vec(any::<u8>(), 0..120)
        ) {
            if let Ok(package) = PackageData::decode(&bytes) {
                prop_assert_eq!(package.encode(), bytes);
            }
        }
    }
}

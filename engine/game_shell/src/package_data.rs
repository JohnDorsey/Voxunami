//! What a client needs to show a world, as the server sends it in `Welcome`: how every block looks
//! and which channels the view reads. Nothing here comes from the client's own copy of the game, so a
//! client draws what the server says, and a recording of the stream plays the same with any content
//! installed.
//!
//! The byte form is canonical (see `block_models`' codec): the same data is the same bytes.

use std::collections::{HashMap, HashSet};

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use block_models::{
    BlockDefinition, BlockTable, MOST_TILE_BYTES, MOST_TILE_SIZES, MOST_TILES_OF_A_SIZE,
    PropertyError, PropertySchema,
};
use content_registry::IdentifierRegistry;
use data_packs::{Tile, is_tile_size};
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
    #[error("the blocks need {materials} colours and at most {limit} can be drawn")]
    TooManyMaterials { materials: usize, limit: usize },
}

/// Whether the renderer can draw a table's colours (a block each, and the colours of states): it draws at most
/// [`renderer::MAXIMUM_GAME_MATERIALS`], and a table of more would be a panic there.
pub fn check_materials(table: &BlockTable) -> Result<(), PackageError> {
    let materials = table.palette().len();
    if materials > renderer::MAXIMUM_GAME_MATERIALS {
        return Err(PackageError::TooManyMaterials {
            materials,
            limit: renderer::MAXIMUM_GAME_MATERIALS,
        });
    }
    Ok(())
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
    pub orientation_channel: Option<NamespacedId>,
    pub light_channel: Option<NamespacedId>,
    /// How bright a face is that no light reaches, 0 to 255.
    pub ambient_light: u8,
    /// The pictures of the blocks, by size, smallest first: each size's pictures in the order of their places (the pictures of a block name them by size number and
    /// place), the sizes all different, each picture as big as its size says. Raw pixels: a client never decodes an image it was sent.
    pub tile_classes: Vec<Vec<Tile>>,
}

impl PackageData {
    /// A package with one block and nothing else to say, for tests of what only needs one to exist.
    #[cfg(test)]
    pub(crate) fn for_tests() -> Self {
        let id = |text: &str| text.parse().expect("a valid identifier");
        Self {
            properties: PropertySchema::new(),
            block_registry: id("test:block"),
            blocks: vec![(id("test:air"), Some(BlockDefinition::air()))],
            block_channel: id("test:block"),
            state_channel: None,
            orientation_channel: None,
            light_channel: None,
            ambient_light: 0,
            tile_classes: Vec::new(),
        }
    }

    /// The blocks whose definitions give a true value for the property `key` (a flag a game reads, such as which blocks a player
    /// can place), in the order of the blocks. A block with no definition is not among them.
    pub fn blocks_with(&self, key: &block_models::PropertyKey<bool>) -> Vec<NamespacedId> {
        let key: NamespacedId = key
            .id()
            .parse()
            .expect("a property key is a valid identifier");
        self.blocks
            .iter()
            .filter(|(_, definition)| {
                definition
                    .as_ref()
                    .is_some_and(|definition| definition.property_value(&key, None, 0) != 0)
            })
            .map(|(identifier, _)| identifier.clone())
            .collect()
    }

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
        )
        .with_orientations(self.orientation_channel.as_ref().map(find).transpose()?))
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
        for channel in [
            &self.state_channel,
            &self.light_channel,
            &self.orientation_channel,
        ] {
            match channel {
                Some(identifier) => {
                    writer.write_u8(1);
                    writer.write_identifier(identifier);
                }
                None => writer.write_u8(0),
            }
        }
        writer.write_u8(self.ambient_light);
        writer.write_varint(self.tile_classes.len() as u64);
        for class in &self.tile_classes {
            let first = class.first().expect("a size of pictures has a picture");
            assert!(
                class.iter().all(|tile| tile.size() == first.size()),
                "the pictures of a size are all of that size"
            );
            writer.write_varint(u64::from(first.size()));
            writer.write_varint(class.len() as u64);
            for tile in class {
                writer.write_raw(tile.rgba());
            }
        }
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
        let orientation_channel = optional_channel(&mut reader)?;
        let ambient_light = reader.read_u8()?;
        let class_count = reader.read_count(2)?;
        if class_count > MOST_TILE_SIZES {
            return Err(DecodeError::Invalid(
                "there are more sizes of picture than a world has",
            ));
        }
        let mut tile_classes: Vec<Vec<Tile>> = Vec::with_capacity(class_count);
        let mut bytes_in_all = 0usize;
        for _ in 0..class_count {
            let size = u32::try_from(reader.read_varint()?).unwrap_or(0);
            if !is_tile_size(size) {
                return Err(DecodeError::Invalid(
                    "a size of picture is not one a picture has",
                ));
            }
            if tile_classes
                .last()
                .is_some_and(|before| before[0].size() >= size)
            {
                return Err(DecodeError::Invalid(
                    "the sizes of picture are repeated or not smallest first",
                ));
            }
            let tile_bytes = size as usize * size as usize * 4;
            let count = reader.read_count(tile_bytes)?;
            if count == 0 || count > MOST_TILES_OF_A_SIZE {
                return Err(DecodeError::Invalid(
                    "a size of picture has no pictures or more than a world has",
                ));
            }
            bytes_in_all += count * tile_bytes;
            if bytes_in_all > MOST_TILE_BYTES {
                return Err(DecodeError::Invalid(
                    "the pictures take more bytes than a client is sent",
                ));
            }
            let mut tiles = Vec::with_capacity(count);
            for _ in 0..count {
                tiles.push(Tile::from_rgba(size, reader.read_raw(tile_bytes)?.to_vec()));
            }
            tile_classes.push(tiles);
        }
        reader.finish()?;
        for (_, definition) in &blocks {
            let Some(pictures) = definition.as_ref().and_then(BlockDefinition::pictures) else {
                continue;
            };
            for place in [pictures.top, pictures.side, pictures.bottom] {
                if tile_classes
                    .get(place.class())
                    .is_none_or(|class| place.layer() >= class.len())
                {
                    return Err(DecodeError::Invalid(
                        "a block has a picture that the package does not hold",
                    ));
                }
            }
        }
        Ok(Self {
            properties,
            block_registry,
            blocks,
            block_channel,
            state_channel,
            orientation_channel,
            light_channel,
            ambient_light,
            tile_classes,
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
            orientation_channel: None,
            light_channel: Some(id("test_game:light")),
            ambient_light: 90,
            tile_classes: Vec::new(),
        }
    }

    fn tile(size: u32, first_pixel: u8) -> Tile {
        let mut rgba = vec![0u8; (size * size * 4) as usize];
        rgba[0] = first_pixel;
        Tile::from_rgba(size, rgba)
    }

    /// The sample with two sizes of picture, and a block that has pictures among them.
    fn pictured() -> PackageData {
        use block_models::{FacePictures, TileRef};
        let mut package = sample();
        package.tile_classes = vec![vec![tile(8, 1), tile(8, 2)], vec![tile(16, 3)]];
        let place = |class, layer| TileRef::new(class, layer).unwrap();
        package.blocks[1].1 = Some(
            BlockDefinition::cube([0.5; 3])
                .with_texture(id("test_game:rock"))
                .with_pictures(FacePictures {
                    top: place(0, 1),
                    side: place(1, 0),
                    bottom: place(0, 0),
                }),
        );
        package
    }

    /// The bytes of the sample up to its pictures (`tile_classes` empty), with these bytes for the pictures instead of the empty list.
    fn with_picture_section(section: &[u8]) -> Vec<u8> {
        let mut bytes = sample().encode();
        assert_eq!(
            bytes.pop(),
            Some(0),
            "the control: the empty list of sizes is one byte"
        );
        bytes.extend_from_slice(section);
        bytes
    }

    #[test]
    fn pictures_survive_the_trip_exactly_smallest_size_first_and_are_the_raw_pixels() {
        let package = pictured();
        let bytes = package.encode();
        assert_eq!(PackageData::decode(&bytes), Ok(package.clone()));
        // The section is the count of sizes, then for each the size, the count and the pixels, after the ambient light.
        let tail = bytes.len() - (1 + (1 + 1 + 2 * 8 * 8 * 4) + (1 + 1 + 16 * 16 * 4));
        assert_eq!(bytes[tail], 2);
        assert_eq!(&bytes[tail + 1..tail + 3], [8, 2]);
        assert_eq!(
            bytes[tail + 3],
            1,
            "the first pixel of the first picture of 8"
        );
    }

    #[test]
    fn a_package_with_no_pictures_ends_with_one_byte_that_says_so() {
        let bytes = sample().encode();
        assert_eq!(bytes.last(), Some(&0));
        assert_eq!(
            PackageData::decode(&bytes).unwrap().tile_classes,
            Vec::<Vec<Tile>>::new()
        );
    }

    #[test]
    fn sizes_that_are_not_sizes_or_not_in_order_or_empty_or_too_many_are_refused_with_the_reason() {
        let section = |classes: &[(u64, u64)]| {
            let mut writer = ByteWriter::new();
            writer.write_varint(classes.len() as u64);
            for (size, count) in classes {
                writer.write_varint(*size);
                writer.write_varint(*count);
                writer.write_raw(&vec![0; (*size * *size * 4 * *count) as usize]);
            }
            writer.into_bytes()
        };
        let refused = |classes: &[(u64, u64)]| {
            PackageData::decode(&with_picture_section(&section(classes))).unwrap_err()
        };
        assert!(PackageData::decode(&with_picture_section(&section(&[(8, 1), (16, 1)]))).is_ok());
        let not_a_size = DecodeError::Invalid("a size of picture is not one a picture has");
        for size in [0, 4, 12, 512, 1 << 33] {
            // (A size of 4 billion could not be made room for: it is refused before the pixels are asked for.)
            let mut writer = ByteWriter::new();
            writer.write_varint(1);
            writer.write_varint(size);
            writer.write_varint(1);
            assert_eq!(
                PackageData::decode(&with_picture_section(&writer.into_bytes())),
                Err(not_a_size.clone()),
                "{size}"
            );
        }
        let unordered =
            DecodeError::Invalid("the sizes of picture are repeated or not smallest first");
        assert_eq!(refused(&[(16, 1), (8, 1)]), unordered);
        assert_eq!(refused(&[(8, 1), (8, 1)]), unordered);
        assert_eq!(
            refused(&[(8, 1), (16, 1), (32, 1), (64, 1), (128, 1)]),
            DecodeError::Invalid("there are more sizes of picture than a world has")
        );
        let count =
            DecodeError::Invalid("a size of picture has no pictures or more than a world has");
        let mut none = ByteWriter::new();
        none.write_varint(1);
        none.write_varint(8);
        none.write_varint(0);
        assert_eq!(
            PackageData::decode(&with_picture_section(&none.into_bytes())),
            Err(count.clone())
        );
        assert_eq!(refused(&[(8, 257)]), count);
        assert!(PackageData::decode(&with_picture_section(&section(&[(8, 256)]))).is_ok());
    }

    #[test]
    fn pictures_that_take_more_than_8_mib_are_refused_and_exactly_8_mib_is_not() {
        let section = |count: u64| {
            let mut writer = ByteWriter::new();
            writer.write_varint(1);
            writer.write_varint(256);
            writer.write_varint(count);
            writer.write_raw(&vec![0; (256 * 256 * 4 * count) as usize]);
            writer.into_bytes()
        };
        assert!(PackageData::decode(&with_picture_section(&section(32))).is_ok());
        assert_eq!(
            PackageData::decode(&with_picture_section(&section(33))),
            Err(DecodeError::Invalid(
                "the pictures take more bytes than a client is sent"
            ))
        );
    }

    #[test]
    fn pictures_that_are_cut_short_are_an_error_everywhere_and_a_block_cannot_point_at_a_picture_that_is_not_there()
     {
        let bytes = pictured().encode();
        for length in (0..bytes.len()).step_by(7) {
            assert!(
                PackageData::decode(&bytes[..length]).is_err(),
                "cut at {length}"
            );
        }
        let mut lacking = pictured();
        lacking.tile_classes[0].pop();
        assert_eq!(
            PackageData::decode(&lacking.encode()),
            Err(DecodeError::Invalid(
                "a block has a picture that the package does not hold"
            ))
        );
        let mut fewer_sizes = pictured();
        fewer_sizes.tile_classes.pop();
        assert_eq!(
            PackageData::decode(&fewer_sizes.encode()),
            Err(DecodeError::Invalid(
                "a block has a picture that the package does not hold"
            ))
        );
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
        assert!(table.shape(table.model_of(0, 0, 0)).is_empty());
        assert!(table.is_opaque_cube(table.model_of(1, 0, 0)));
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

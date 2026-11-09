//! The first blocks (air, stone, dirt, grass, lamp, wood, fire, great river water, leaves, glass) and the properties they carry.
//!
//! The blocks are data: one JSON file each in `data/blocks`, in the format of a data pack (`docs/reference/pack-format.md`), read by the reader packs use and
//! registered in a fixed order, because their numbers are fixed. A modder can copy any of them.
//!
//! The solid blocks come in several shapes, chosen by the voxel's *state* (the channel
//! `base_blocks:block_state`): a whole cube, slabs, and stairs, so shapes do not take block types
//! of their own.

use block_models::{BlockDefinition, BlockTable, PropertyDeclaration, PropertyKey, PropertySchema};
use content_registry::{Registry, RuntimeId};
use data_packs::{GameTexture, Textures};
use namespaced_id::NamespacedId;
use voxel_grid::ChannelDefinition;

fn identifier(text: &str) -> NamespacedId {
    text.parse().expect("constant identifiers are valid")
}

/// The registry kind block ids belong to.
pub fn block_registry_kind() -> NamespacedId {
    identifier("base_blocks:block")
}

/// The grid channel holding each voxel's block.
pub fn block_channel_identifier() -> NamespacedId {
    identifier("base_blocks:block")
}

/// The grid channel holding each voxel's state: which shape its block has.
pub fn state_channel_identifier() -> NamespacedId {
    identifier("base_blocks:block_state")
}

/// The grid channel holding each voxel's orientation: the number of a turn of the lattice (see `voxel_space::LatticeRotation::index`), 0 for none. It is
/// the same for every block, whatever the block looks like turned, so a ship can be turned by turning these numbers, and the blocks of a plugin that is not
/// loaded are turned too.
pub fn orientation_channel_identifier() -> NamespacedId {
    identifier("base_blocks:block_orientation")
}

/// The grid channel holding the block light of each voxel: 0 to 255, in the voxels light can be in
/// (air, and the empty parts of blocks) and 0 in the others.
pub fn light_channel_identifier() -> NamespacedId {
    identifier("base_blocks:block_light")
}

/// The light a block gives off, 0 (none) to 255. The light rules scale it to their own levels.
pub const EMISSION: PropertyKey<u8> = PropertyKey::new(
    "base_blocks:emission",
    0,
    "the light a block gives off, 0 (none) to 255",
);

/// How readily a block catches fire next to a fire, 0 (it does not) to 255. The fire rule decides what the
/// number means.
pub const FLAMMABILITY: PropertyKey<u8> = PropertyKey::new(
    "base_blocks:flammability",
    0,
    "how readily a block catches fire next to a fire, 0 (it does not) to 255",
);

/// Whether bodies move through a block as if it were not there (flames), though it can still be seen and
/// aimed at.
pub const PASSABLE: PropertyKey<bool> = PropertyKey::new(
    "base_blocks:passable",
    false,
    "whether bodies move through the block as if it were not there",
);

/// The properties this plugin declares, for a game to add to its package.
pub fn property_declarations() -> Vec<PropertyDeclaration> {
    vec![
        EMISSION.declaration(),
        FLAMMABILITY.declaration(),
        PASSABLE.declaration(),
    ]
}

/// The engine's properties and this plugin's.
pub fn property_schema() -> PropertySchema {
    let mut schema = PropertySchema::new();
    for declaration in property_declarations() {
        schema
            .declare(declaration)
            .expect("the plugin's properties are distinct from the engine's");
    }
    schema
}

/// How much light a lamp gives off (`data/blocks/lamp.json` says so too, and a test keeps them equal). Light loses one level for every voxel it
/// spreads, so this is also how many voxels away a lamp lights things.
pub const LAMP_EMISSION: u8 = 64;

/// How many steps a fire burns through before it goes out: its state is its age, counting up from
/// 0, and a fire of this age is gone. (The rule that ages it is the game's; `data/blocks/fire.json` has a colour for each age, and a test keeps the
/// numbers equal.)
pub const FIRE_LIFETIME: u32 = 12;

/// How much light a fire gives off (`data/blocks/fire.json` says so too, and a test keeps them equal).
pub const FIRE_EMISSION: u8 = 40;

/// What each state of a solid block of this plugin looks like, by name; the position is the state.
pub const SHAPE_NAMES: [&str; 7] = [
    "whole",
    "bottom slab",
    "top slab",
    "stair rising east",
    "stair rising west",
    "stair rising north",
    "stair rising south",
];

/// The name of a state of a solid block, as for [`SHAPE_NAMES`].
pub fn shape_name(state: u32) -> &'static str {
    SHAPE_NAMES.get(state as usize).copied().unwrap_or("whole")
}

/// The blocks in the order of their numbers, each with the text of the file in `data/blocks` that defines it. The numbers are fixed (air is 0, so a
/// world whose voxels are all zero is sky), so the order is written here and not left to the order of the files' names.
const BLOCK_FILES: [(&str, &str); 10] = [
    ("air", include_str!("../data/blocks/air.json")),
    ("stone", include_str!("../data/blocks/stone.json")),
    ("dirt", include_str!("../data/blocks/dirt.json")),
    ("grass", include_str!("../data/blocks/grass.json")),
    ("lamp", include_str!("../data/blocks/lamp.json")),
    ("wood", include_str!("../data/blocks/wood.json")),
    ("fire", include_str!("../data/blocks/fire.json")),
    (
        "great_river_water",
        include_str!("../data/blocks/great_river_water.json"),
    ),
    ("leaves", include_str!("../data/blocks/leaves.json")),
    ("glass", include_str!("../data/blocks/glass.json")),
];

/// The pictures of the blocks, each with the name its file has in `textures` (`log_top.png` is `base_blocks:log_top`). The folder holds more pictures than
/// these, which no block uses yet; a test keeps this list equal to the pictures the blocks name.
const TEXTURE_FILES: [(&str, &[u8]); 11] = [
    ("grass", include_bytes!("../textures/grass.png")),
    ("dirt", include_bytes!("../textures/dirt.png")),
    ("stone", include_bytes!("../textures/stone.png")),
    ("still_water", include_bytes!("../textures/still_water.png")),
    ("log_top", include_bytes!("../textures/log_top.png")),
    ("log_side", include_bytes!("../textures/log_side.png")),
    ("leaves", include_bytes!("../textures/leaves.png")),
    ("glass", include_bytes!("../textures/glass.png")),
    (
        "dev_docking_port_top",
        include_bytes!("../textures/dev_docking_port_top.png"),
    ),
    (
        "dev_docking_port_side",
        include_bytes!("../textures/dev_docking_port_side.png"),
    ),
    (
        "dev_docking_port_bottom",
        include_bytes!("../textures/dev_docking_port_bottom.png"),
    ),
];

/// The pictures this plugin has, for a game to give its packs (which may replace them by file name) and to settle the colours of the blocks that follow them.
/// They belong to the blocks of this plugin and to the dev docking port of Cave, which names `base_blocks:dev_docking_port`.
pub fn textures() -> Vec<GameTexture> {
    TEXTURE_FILES
        .iter()
        .map(|(name, bytes)| GameTexture {
            id: identifier(&format!("base_blocks:{name}")),
            file: format!("plugins/base_blocks/textures/{name}.png"),
            bytes: bytes.to_vec(),
        })
        .collect()
}

/// The ids of the blocks, which are fixed: air first, so a world whose voxels are all zero is sky.
/// Everything a game needs to use the blocks and their channels and terrain once the registry is
/// the game's.
#[derive(Clone, Copy, Debug)]
pub struct BaseBlockIds {
    pub air: u32,
    pub stone: u32,
    pub dirt: u32,
    pub grass: u32,
    /// A solid block that gives off light. Not part of the terrain.
    pub lamp: u32,
    /// A solid block that burns.
    pub wood: u32,
    /// Flames: a low, lit, non-solid-looking block that spreads to what burns and goes out. Its state
    /// is its age, which any number can be (see [`FIRE_LIFETIME`]).
    pub fire: u32,
    /// The water of the great rivers: a block that never flows, which bodies move through (Cave's great rivers, `voxunami_cave::generators::great_rivers`).
    pub great_river_water: u32,
    /// A cube you see and light pass through, whose picture has holes in it (`transparency` `leaves`): it shows the faces between its voxels and its faces from behind.
    pub leaves: u32,
    /// A cube you see and light pass through, with a frame and clear panes (`transparency` `glass`): it shows its faces from behind, and not those between its voxels.
    pub glass: u32,
}

impl BaseBlockIds {
    /// The channel definition for the block of each voxel; new voxels are air.
    pub fn block_channel(&self) -> ChannelDefinition {
        ChannelDefinition::content(block_channel_identifier(), self.air, block_registry_kind())
    }

    /// The channel definition for the state of each voxel's block; new voxels are in state 0, the
    /// whole cube.
    pub fn state_channel(&self) -> ChannelDefinition {
        ChannelDefinition::number(state_channel_identifier(), 0)
    }

    /// The channel definition for the orientation of each voxel; new voxels are not turned.
    pub fn orientation_channel(&self) -> ChannelDefinition {
        ChannelDefinition::number(orientation_channel_identifier(), 0)
    }

    /// The channel definition for the light of each voxel; new voxels have none.
    pub fn light_channel(&self) -> ChannelDefinition {
        ChannelDefinition::number(light_channel_identifier(), 0)
    }
}

/// The blocks, registered in a fixed order (see [`BaseBlockIds`]), with their ids. A game takes
/// the registry for its world and keeps the ids.
#[derive(Debug)]
pub struct BaseBlocks {
    pub blocks: Registry<BlockDefinition>,
    ids: BaseBlockIds,
    pictures: Textures,
}

impl std::ops::Deref for BaseBlocks {
    type Target = BaseBlockIds;

    fn deref(&self) -> &BaseBlockIds {
        &self.ids
    }
}

impl Default for BaseBlocks {
    fn default() -> Self {
        Self::new()
    }
}

impl BaseBlocks {
    pub fn new() -> Self {
        let schema = property_schema();
        let mut definitions: Vec<(NamespacedId, BlockDefinition)> = BLOCK_FILES
            .iter()
            .map(|(name, text)| {
                let file = format!("plugins/base_blocks/data/blocks/{name}.json");
                data_packs::read_block(&file, "base_blocks", name, text, &schema)
                    .unwrap_or_else(|errors| panic!("a file of the base blocks is wrong: {errors}"))
            })
            .collect();
        // The colours that follow the pictures are worked out from the pictures of the plugin.
        let pictures = data_packs::settle_game_pictures(&mut definitions, &textures())
            .unwrap_or_else(|errors| panic!("the pictures of the base blocks are wrong: {errors}"));
        let mut blocks = Registry::new();
        let ids: Vec<u32> = definitions
            .into_iter()
            .map(|(identifier, definition)| {
                blocks
                    .register(identifier, definition)
                    .expect("the block names are distinct")
                    .raw()
            })
            .collect();
        // (The order of `BLOCK_FILES`, which a test keeps equal to the order of the fields.)
        let [
            air,
            stone,
            dirt,
            grass,
            lamp,
            wood,
            fire,
            great_river_water,
            leaves,
            glass,
        ] = ids[..]
        else {
            unreachable!("there is a file for each of the ten blocks");
        };
        Self {
            blocks,
            ids: BaseBlockIds {
                air,
                stone,
                dirt,
                grass,
                lamp,
                wood,
                fire,
                great_river_water,
                leaves,
                glass,
            },
            pictures,
        }
    }

    /// The pictures of the blocks: each block's, and the tiles they are.
    pub fn pictures(&self) -> &Textures {
        &self.pictures
    }

    /// The ids of the blocks, to keep when the registry goes to a game.
    pub fn ids(&self) -> BaseBlockIds {
        self.ids
    }

    /// The registry of the blocks, to give to a game's content.
    pub fn into_registry(self) -> Registry<BlockDefinition> {
        self.blocks
    }

    /// What every block looks like and how it fills its cell, by block id. Blocks from plugins that
    /// are not loaded are opaque magenta cubes.
    pub fn block_table(&self) -> BlockTable {
        let definitions: Vec<Option<&BlockDefinition>> = self
            .blocks
            .iter()
            .map(|(_, entry)| entry.content())
            .collect();
        BlockTable::try_from_definitions(&property_schema(), &definitions)
            .expect("the base blocks' properties are declared")
    }

    pub fn block_id(&self, identifier: &NamespacedId) -> Option<RuntimeId<BlockDefinition>> {
        self.blocks.runtime_id(identifier)
    }
}

#[cfg(test)]
mod tests {
    use block_models::{MISSING_BLOCK_COLOR, Shape};
    use content_registry::IdentifierRegistry;
    use voxel_space::FaceDirection;

    use super::*;

    #[test]
    fn every_file_in_the_data_folder_is_a_block_and_every_block_is_a_file_and_the_order_is_that_of_the_ids()
     {
        // A file added to `data/blocks` and not to the list would be a block nobody gets; a name in the list without a file does not build.
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/blocks");
        let mut on_disk: Vec<String> = std::fs::read_dir(folder)
            .expect("the data folder is there")
            .map(|entry| {
                let name = entry.unwrap().file_name().into_string().unwrap();
                name.strip_suffix(".json")
                    .unwrap_or_else(|| panic!("{name} is not a .json file"))
                    .to_owned()
            })
            .collect();
        on_disk.sort();
        let mut listed: Vec<String> = BLOCK_FILES
            .iter()
            .map(|(name, _)| (*name).to_owned())
            .collect();
        listed.sort();
        assert_eq!(on_disk, listed);
        // The list is in the order of the fields of `BaseBlockIds`, which are the numbers.
        let blocks = BaseBlocks::new();
        let numbers = [
            blocks.air,
            blocks.stone,
            blocks.dirt,
            blocks.grass,
            blocks.lamp,
            blocks.wood,
            blocks.fire,
            blocks.great_river_water,
            blocks.leaves,
            blocks.glass,
        ];
        assert_eq!(numbers, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        for ((name, _), number) in BLOCK_FILES.iter().zip(numbers) {
            let wanted = identifier(&format!("base_blocks:{name}"));
            assert_eq!(
                blocks.block_id(&wanted).map(|id| id.raw()),
                Some(number),
                "{name}"
            );
        }
    }

    #[test]
    fn the_solid_blocks_have_the_states_that_shape_names_names_in_that_order() {
        // `SHAPE_NAMES` names the states by position and the files list them by position; the two must say the same thing.
        let expected = [
            Shape::full(),
            Shape::half(FaceDirection::NegativeY),
            Shape::half(FaceDirection::PositiveY),
            Shape::stair(FaceDirection::PositiveX),
            Shape::stair(FaceDirection::NegativeX),
            Shape::stair(FaceDirection::NegativeZ),
            Shape::stair(FaceDirection::PositiveZ),
        ];
        assert_eq!(SHAPE_NAMES.len(), expected.len());
        let blocks = BaseBlocks::new();
        for solid in [blocks.stone, blocks.dirt, blocks.grass, blocks.wood] {
            let (_, entry) = blocks.blocks.iter().nth(solid as usize).unwrap();
            let definition = entry.content().unwrap();
            assert_eq!(definition.shapes(), expected, "block {solid}");
        }
        for single in [
            blocks.air,
            blocks.lamp,
            blocks.fire,
            blocks.great_river_water,
            blocks.leaves,
            blocks.glass,
        ] {
            let (_, entry) = blocks.blocks.iter().nth(single as usize).unwrap();
            assert_eq!(entry.content().unwrap().state_count(), 1, "block {single}");
        }
    }

    #[test]
    fn the_blocks_with_pictures_are_the_colour_of_their_pictures_and_the_pictures_are_the_ones_the_plugin_has()
     {
        let blocks = BaseBlocks::new();
        let textured = [
            ("stone", blocks.stone),
            ("dirt", blocks.dirt),
            ("grass", blocks.grass),
            ("wood", blocks.wood),
            ("great_river_water", blocks.great_river_water),
            ("leaves", blocks.leaves),
            ("glass", blocks.glass),
        ];
        let tiles = &blocks.pictures().tiles;
        for (name, number) in textured {
            let (_, entry) = blocks.blocks.iter().nth(number as usize).unwrap();
            let definition = entry.content().unwrap();
            assert!(definition.color_from_pictures(), "{name}");
            let pictures = &blocks.pictures().pictures[entry.identifier()];
            let average = data_packs::average_color(&[
                (&tiles[&pictures.top], 1),
                (&tiles[&pictures.side], 4),
            ])
            .expect("the pictures are not clear");
            assert_eq!(definition.color, average, "{name}");
        }
        // The air, the lamp and the fire have none.
        for number in [blocks.air, blocks.lamp, blocks.fire] {
            let (_, entry) = blocks.blocks.iter().nth(number as usize).unwrap();
            assert_eq!(entry.content().unwrap().texture(), None);
            assert!(!blocks.pictures().pictures.contains_key(entry.identifier()));
        }
        // The pictures the plugin embeds are the ones its blocks and Cave's port use, no more and no fewer: the log's two, the port's three, and one each for the rest.
        let mut used: Vec<String> = blocks
            .pictures()
            .tiles
            .keys()
            .map(ToString::to_string)
            .collect();
        let port =
            data_packs::resolve_pictures(&identifier("base_blocks:dev_docking_port"), |id| {
                textures().iter().any(|texture| texture.id == *id)
            })
            .expect("the port has pictures");
        used.extend(
            [port.top, port.side, port.bottom]
                .iter()
                .map(ToString::to_string),
        );
        used.sort();
        used.dedup();
        let mut embedded: Vec<String> = textures()
            .iter()
            .map(|texture| texture.id.to_string())
            .collect();
        embedded.sort();
        assert_eq!(used, embedded);
    }

    #[test]
    fn each_embedded_picture_is_the_file_of_its_name() {
        // `TEXTURE_FILES` pairs a name with `include_bytes!` of a path written next to it; the two must say the same file.
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("textures");
        for texture in textures() {
            let name = texture.id.name();
            let on_disk = std::fs::read(folder.join(format!("{name}.png")))
                .unwrap_or_else(|error| panic!("{name}.png: {error}"));
            assert_eq!(texture.bytes, on_disk, "{name}");
            assert_eq!(
                texture.file,
                format!("plugins/base_blocks/textures/{name}.png")
            );
        }
    }

    #[test]
    fn the_numbers_the_games_rules_use_are_the_numbers_in_the_files() {
        let blocks = BaseBlocks::new();
        let table = blocks.block_table();
        let emission = table.handle(&EMISSION).unwrap();
        assert_eq!(table.get_of(emission, blocks.lamp, 0), LAMP_EMISSION);
        assert_eq!(table.get_of(emission, blocks.fire, 0), FIRE_EMISSION);
        // A fire has a colour for each age it burns through (and the state is the age, which any number can be).
        let (_, fire) = blocks.blocks.iter().nth(blocks.fire as usize).unwrap();
        assert_eq!(
            fire.content().unwrap().state_colors().len(),
            FIRE_LIFETIME as usize
        );
    }

    #[test]
    fn air_is_block_zero_and_the_blocks_have_their_names() {
        let blocks = BaseBlocks::new();
        assert_eq!(
            (
                blocks.air,
                blocks.stone,
                blocks.dirt,
                blocks.grass,
                blocks.lamp,
                blocks.wood,
                blocks.fire,
                blocks.great_river_water,
                blocks.leaves,
                blocks.glass
            ),
            (0, 1, 2, 3, 4, 5, 6, 7, 8, 9)
        );
        let names: Vec<String> = (0..10)
            .map(|raw| blocks.blocks.identifier_of_raw(raw).unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            [
                "base_blocks:air",
                "base_blocks:stone",
                "base_blocks:dirt",
                "base_blocks:grass",
                "base_blocks:lamp",
                "base_blocks:wood",
                "base_blocks:fire",
                "base_blocks:great_river_water",
                "base_blocks:leaves",
                "base_blocks:glass"
            ]
        );
        assert_eq!(blocks.light_channel().default_value, 0);
        assert_eq!(
            blocks.light_channel().identifier.to_string(),
            "base_blocks:block_light"
        );
        assert_eq!(blocks.block_channel().default_value, 0);
        assert_eq!(blocks.state_channel().default_value, 0);
        assert_eq!(
            blocks.state_channel().identifier.to_string(),
            "base_blocks:block_state"
        );
    }

    #[test]
    fn solid_blocks_have_the_seven_shapes_and_air_the_lamp_and_fire_have_one() {
        let table = BaseBlocks::new().block_table();
        assert_eq!(table.state_count(0), 1);
        assert_eq!(table.state_count(4), 1);
        assert_eq!(
            table.state_count(6),
            1,
            "a fire has one shape, and ages are not shapes"
        );
        // The whole cube, and the slabs and stairs: how they stand is their orientation, which every block has, not a state.
        for block in [1, 2, 3, 5] {
            assert_eq!(table.state_count(block), SHAPE_NAMES.len() as u32);
        }
        // State 0 is the whole cube and the others are not; the names follow the shapes.
        let stone = |state| table.model_of(1, state, 0);
        assert!(table.is_opaque_cube(stone(0)));
        for state in 1..7 {
            assert!(!table.is_opaque_cube(stone(state)), "{}", shape_name(state));
            assert!(table.is_solid(stone(state)));
        }
        assert!(table.shape(stone(1)).covers_face(FaceDirection::NegativeY));
        assert!(table.shape(stone(2)).covers_face(FaceDirection::PositiveY));
        assert!(table.shape(stone(3)).covers_face(FaceDirection::PositiveX));
        assert!(table.shape(stone(4)).covers_face(FaceDirection::NegativeX));
        assert!(table.shape(stone(5)).covers_face(FaceDirection::NegativeZ));
        assert!(table.shape(stone(6)).covers_face(FaceDirection::PositiveZ));
        assert_eq!(shape_name(0), "whole");
        assert_eq!(shape_name(2), "top slab");
        assert_eq!(shape_name(99), "whole");
    }

    #[test]
    fn wood_is_a_log_with_its_line_up_and_down_and_both_ends_coloured_and_nothing_else_is_turned_by_the_player()
     {
        let table = BaseBlocks::new().block_table();
        let wood = BaseBlocks::new().wood;
        assert_eq!(
            table.placement(wood, 0),
            block_models::Placement::AlongPlacedFace(FaceDirection::PositiveY)
        );
        // Coloured at the top and at the bottom as it is made (the ends of the trunk), and the other faces are the bark.
        let ends: Vec<FaceDirection> = FaceDirection::ALL
            .into_iter()
            .filter(|face| {
                table.palette()[table.material_of_face(wood, 0, 0, *face) as usize]
                    != table.color(wood)
            })
            .collect();
        assert_eq!(ends, [FaceDirection::NegativeY, FaceDirection::PositiveY]);
        for block in [0, 1, 2, 3, 4, 6] {
            assert_eq!(
                table.placement(block, 0),
                block_models::Placement::Fixed,
                "{block}"
            );
        }
    }

    #[test]
    fn the_table_has_the_blocks_colours_air_is_sight_through_and_missing_blocks_are_magenta_cubes()
    {
        let mut blocks = BaseBlocks::new();
        let table = blocks.block_table();
        // The flat colours are the blocks, then the colours a fire is drawn in as it ages, then the colour of the ends of a log; after them come the colours with a
        // picture on them, which are not flat.
        let with_pictures = table
            .palette_pictures()
            .iter()
            .filter(|word| **word != 0)
            .count();
        assert_eq!(
            table.palette().len() - with_pictures,
            10 + FIRE_LIFETIME as usize + 1
        );
        assert!(with_pictures > 0);
        assert_eq!(
            table.color(blocks.lamp),
            [1.0, 0.92, 0.65],
            "a block without pictures is the colour its file says"
        );
        // Only the lamp and the fire give off light, only grass and wood burn, and bodies pass through flames and
        // river water and nothing else of the blocks.
        let emission = table.handle(&EMISSION).unwrap();
        let flammability = table.handle(&FLAMMABILITY).unwrap();
        let passable = table.handle(&PASSABLE).unwrap();
        assert_eq!(
            (0..10)
                .map(|block| table.get_of(emission, block, 0))
                .collect::<Vec<_>>(),
            [0, 0, 0, 0, LAMP_EMISSION, 0, FIRE_EMISSION, 0, 0, 0]
        );
        assert_eq!(
            (0..10)
                .map(|block| table.get_of(flammability, block, 0))
                .collect::<Vec<_>>(),
            [0, 0, 0, 100, 0, 40, 0, 0, 0, 0],
            "grass and wood burn, and fire and water do not burn"
        );
        // The lamp is a whole opaque block.
        assert!(table.is_opaque_cube(table.model_of(4, 0, 0)));
        assert_eq!(
            (0..10)
                .map(|block| table.get_of(passable, block, 0))
                .collect::<Vec<_>>(),
            [
                false, false, false, false, false, false, true, true, false, false
            ]
        );
        assert!(!table.is_solid(table.model_of(0, 0, 0)));
        assert!(table.is_sight_through(table.model_of(0, 0, 0)));
        let missing = blocks
            .blocks
            .register_missing(identifier("elsewhere:marble"));
        assert_eq!(missing.raw(), 10);
        let table = blocks.block_table();
        assert_eq!(table.palette()[10], MISSING_BLOCK_COLOR);
        assert!(table.is_opaque_cube(table.model_of(10, 0, 0)));
    }

    #[test]
    fn leaves_and_glass_are_solid_cubes_that_are_seen_and_lit_through_in_their_own_modes_and_no_other_block_has_a_mode()
     {
        use block_models::Transparency;
        let blocks = BaseBlocks::new();
        let table = blocks.block_table();
        // (block, mode, faces between its voxels drawn, faces from behind drawn)
        let wanted = [
            (blocks.leaves, Transparency::Leaves, true, true),
            (blocks.glass, Transparency::Glass, false, true),
        ];
        for (block, mode, inner, back) in wanted {
            let model = table.model_of(block, 0, 0);
            assert!(table.is_solid(model), "{block}");
            assert!(table.shape(model).is_full(), "{block}");
            assert!(table.is_sight_through(model), "{block}");
            assert!(table.is_light_through(model), "{block}");
            assert!(!table.is_opaque_cube(model), "{block}");
            assert_eq!(table.transparency(model), mode, "{block}");
            assert_eq!(table.shows_inner_faces(model), inner, "{block}");
            assert_eq!(table.shows_back_faces(model), back, "{block}");
            // Beside a voxel of itself a block hides nothing of it only if it shows inner faces.
            assert_eq!(
                table.hidden_by_neighbour(model, FaceDirection::PositiveX, model) == 0,
                inner,
                "{block}"
            );
            // Bodies cannot walk through them, and they are placed the same whichever way the player faces.
            assert!(!table.get_of(table.handle(&PASSABLE).unwrap(), block, 0));
            assert_eq!(table.placement(block, 0), block_models::Placement::Fixed);
            // Their pictures cut holes, and only theirs.
            let picture = table
                .face_materials(block, 0, 0)
                .map(|materials| table.palette_pictures()[materials[0] as usize])
                .and_then(block_models::FacePicture::from_word)
                .expect("the block has a picture");
            assert!(picture.cut_out, "{block}");
        }
        for block in [
            blocks.air,
            blocks.stone,
            blocks.dirt,
            blocks.grass,
            blocks.lamp,
            blocks.wood,
            blocks.fire,
            blocks.great_river_water,
        ] {
            let model = table.model_of(block, 0, 0);
            assert_eq!(table.transparency(model), Transparency::Opaque, "{block}");
            if let Some(materials) = table.face_materials(block, 0, 0) {
                for material in materials {
                    if let Some(picture) = block_models::FacePicture::from_word(
                        table.palette_pictures()[material as usize],
                    ) {
                        assert!(
                            !picture.cut_out,
                            "{block}: an opaque block draws its picture whole"
                        );
                    }
                }
            }
        }
        // Light passes through air, leaves and glass, and nothing else of the blocks.
        let passes: Vec<bool> = (0..10)
            .map(|block| table.is_light_through(table.model_of(block, 0, 0)))
            .collect();
        assert_eq!(
            passes,
            [
                true, false, false, false, false, false, false, false, true, true
            ]
        );
    }

    #[test]
    fn the_plugins_properties_are_declared_once_beside_the_engines_and_an_unknown_block_has_none_of_them()
     {
        let declared: Vec<String> = property_schema().iter().map(|d| d.id.to_string()).collect();
        assert_eq!(
            declared,
            [
                "base_blocks:emission",
                "base_blocks:flammability",
                "base_blocks:passable",
                "engine:axis_face",
                "engine:density",
                "engine:docking_face",
                "engine:front_face",
                "engine:joint_face",
                "engine:placeable_on_moving_grids"
            ]
        );
        let table = BaseBlocks::new().block_table();
        let emission = table.handle(&EMISSION).unwrap();
        let passable = table.handle(&PASSABLE).unwrap();
        // A block the table has no definition for: no light, and a wall.
        assert_eq!(table.get_of(emission, 9999, 0), 0);
        assert!(!table.get_of(passable, 9999, 0));
    }
}

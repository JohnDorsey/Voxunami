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
use data_packs::{GameTexture, RawPack, Textures};
use namespaced_id::NamespacedId;
use std::sync::OnceLock;
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

/// The plugin's own pack, as the canonical archive `build.rs` makes of it: every file of the `data` folder (`pack.json`, `blocks/*.json`) and every `.png` of the
/// `textures` folder, which is the painter's working folder and holds tools and notes beside the pictures (`docs/specs/pack-archive-spec.md`, step 3). A block or a
/// picture put in those folders is in the archive with no list to forget it in. The archive is also what a server will hand to a client that asks for this pack.
const PACK_ARCHIVE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/base_blocks.pack.zip"));

/// The id of the plugin's pack, which is its namespace.
const PACK_ID: &str = "base_blocks";

/// The bytes of the plugin's pack, the canonical archive of its files: the same on every machine, so its root hash (`pack_archive::FileIndex`) names the plugin's
/// content.
pub fn pack_archive_bytes() -> &'static [u8] {
    PACK_ARCHIVE
}

/// The plugin's pack, read once from [`PACK_ARCHIVE`].
fn pack() -> &'static RawPack {
    static PACK: OnceLock<RawPack> = OnceLock::new();
    PACK.get_or_init(|| data_packs::read_builtin(PACK_ID, PACK_ARCHIVE))
}

/// The blocks in the order of their numbers. The numbers are fixed (air is 0, so a world whose voxels are all zero is sky), so the order is written here and not
/// left to the order of the files' names; a test keeps this list equal to the blocks the pack has, and the order of the fields of [`BaseBlockIds`].
const BLOCK_ORDER: [&str; 10] = [
    "air",
    "stone",
    "dirt",
    "grass",
    "lamp",
    "wood",
    "fire",
    "great_river_water",
    "leaves",
    "glass",
];

/// The pictures of the plugin, each with the name its file has in `textures` (`log_top.png` is `base_blocks:log_top`): every picture in the folder, those no
/// block uses yet as well (a picture is put in a world only if a block uses it). The companions of a picture (`dirt_n` is the normal picture of `dirt`: a file
/// ending `_s` is a specular picture and one ending `_n` a normal one, `docs/specs/block-textures-spec.md` section 4.4) come with the picture they go with.
/// For a game to give its packs (which may replace them by file name) and to settle the colours of the blocks that follow them. They belong to the blocks of
/// this plugin and to the dev docking port of Cave, which names `base_blocks:dev_docking_port`.
pub fn textures() -> Vec<GameTexture> {
    pack()
        .textures
        .iter()
        .map(|texture| GameTexture {
            id: identifier(&format!("{PACK_ID}:{}", texture.name)),
            file: format!("plugins/base_blocks/textures/{}.png", texture.name),
            bytes: texture.bytes.clone(),
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
        let pack = pack();
        let mut definitions: Vec<(NamespacedId, BlockDefinition)> = BLOCK_ORDER
            .iter()
            .map(|name| {
                let file = pack
                    .blocks
                    .iter()
                    .find(|file| file.name == *name)
                    .unwrap_or_else(|| {
                        panic!("the pack of the base blocks has no file for {name}")
                    });
                let path = format!("plugins/base_blocks/data/blocks/{name}.json");
                data_packs::read_block(&path, PACK_ID, name, &file.text, &schema)
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
        // (The order of `BLOCK_ORDER`, which a test keeps equal to the order of the fields.)
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
        // A file added to `data/blocks` and not to the list would be a block nobody gets; a name in the list without a file does not build. The archive is made of
        // the folder by `build.rs`, so the folder and the archive are compared with the list, each on its own.
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
        let mut in_the_pack: Vec<String> =
            pack().blocks.iter().map(|file| file.name.clone()).collect();
        in_the_pack.sort();
        let mut listed: Vec<String> = BLOCK_ORDER.iter().map(|name| (*name).to_owned()).collect();
        listed.sort();
        assert_eq!(on_disk, listed);
        assert_eq!(in_the_pack, listed);
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
        for (name, number) in BLOCK_ORDER.iter().zip(numbers) {
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
        // The lamp has a picture and keeps the colour its file gives it (which the far levels show and the light it gives is not made of): it is not the average of its picture.
        let (_, lamp) = blocks.blocks.iter().nth(blocks.lamp as usize).unwrap();
        let lamp_definition = lamp.content().unwrap();
        assert_eq!(
            lamp_definition.texture().map(ToString::to_string),
            Some("base_blocks:lamp".to_owned())
        );
        assert!(!lamp_definition.color_from_pictures());
        assert!(blocks.pictures().pictures.contains_key(lamp.identifier()));
        // The air and the fire have none.
        for number in [blocks.air, blocks.fire] {
            let (_, entry) = blocks.blocks.iter().nth(number as usize).unwrap();
            assert_eq!(entry.content().unwrap().texture(), None);
            assert!(!blocks.pictures().pictures.contains_key(entry.identifier()));
        }
        // Every picture the plugin's blocks and Cave's port use is one the pack has (the log's two, the port's three, and one each for the rest), and nothing else is
        // in a world: the pack also has pictures no block uses yet, which are not put in one.
        let mut used: Vec<String> = blocks
            .pictures()
            .tiles
            .keys()
            .map(ToString::to_string)
            .collect();
        let has = |id: &NamespacedId| textures().iter().any(|texture| texture.id == *id);
        let port = data_packs::resolve_pictures(&identifier("base_blocks:dev_docking_port"), has)
            .expect("the port has pictures");
        used.extend(
            [port.top, port.side, port.bottom]
                .iter()
                .map(ToString::to_string),
        );
        used.sort();
        used.dedup();
        assert_eq!(
            used,
            [
                "base_blocks:dev_docking_port_bottom",
                "base_blocks:dev_docking_port_side",
                "base_blocks:dev_docking_port_top",
                "base_blocks:dirt",
                "base_blocks:glass",
                "base_blocks:grass",
                "base_blocks:lamp",
                "base_blocks:leaves",
                "base_blocks:log_side",
                "base_blocks:log_top",
                "base_blocks:still_water",
                "base_blocks:stone",
            ]
        );
        for id in &used {
            assert!(
                has(&identifier(id)),
                "{id} is used and the pack has no such picture"
            );
        }
        // (A companion is not a picture of a block: it goes with the picture whose name is the rest of its own, which the pack must have.)
        let ids: Vec<String> = textures()
            .iter()
            .map(|texture| texture.id.to_string())
            .collect();
        for companion in ids
            .iter()
            .filter(|id| id.ends_with("_s") || id.ends_with("_n"))
        {
            let picture = &companion[..companion.len() - 2];
            assert!(
                ids.iter().any(|id| id == picture),
                "{companion} goes with {picture}, which the pack does not have"
            );
        }
    }

    /// The placeholder specular pictures (`textures/make_placeholder_specular.py`): the lamp shines where its picture is bright and the leaves let the sun through. They set no
    /// smoothness and no reflectance (no glint), the lamp has no scatter and the leaves no emission, so that each says one thing.
    #[test]
    fn the_lamp_shines_where_its_picture_is_bright_and_the_leaves_let_the_sun_through() {
        let tile = |name: &str| {
            let texture = textures()
                .into_iter()
                .find(|texture| texture.id.name() == name)
                .unwrap_or_else(|| panic!("no picture {name}"));
            data_packs::Tile::from_png(name, &texture.bytes).expect("the picture reads")
        };
        let brightness = |pixel: &[u8]| {
            0.2126 * f64::from(pixel[0])
                + 0.7152 * f64::from(pixel[1])
                + 0.0722 * f64::from(pixel[2])
        };
        let (lamp, glow) = (tile("lamp"), tile("lamp_s"));
        assert_eq!(glow.size(), lamp.size());
        let pairs: Vec<(f64, u8)> = lamp
            .rgba()
            .chunks(4)
            .zip(glow.rgba().chunks(4))
            .map(|(colour, specular)| {
                assert_eq!(
                    specular[..3],
                    [0, 0, 0],
                    "the lamp's glow has no glint and no scatter"
                );
                (brightness(colour), specular[3])
            })
            .collect();
        // The dim rim does not shine at all, the bright middle shines wholly (254 is all of it; 255 would be none), and a brighter pixel never shines less than a dimmer one.
        assert!(
            pairs
                .iter()
                .any(|&(bright, glow)| bright < 190.0 && glow == 0)
        );
        assert!(
            pairs
                .iter()
                .all(|&(bright, glow)| bright >= 190.0 || glow == 0)
        );
        assert!(
            pairs
                .iter()
                .any(|&(bright, glow)| bright >= 240.0 && glow == 254)
        );
        assert!(
            pairs
                .iter()
                .all(|&(bright, glow)| bright < 240.0 || glow == 254)
        );
        assert!(pairs.iter().all(|&(_, glow)| glow != 255));
        for &(bright, glow) in &pairs {
            for &(other_bright, other_glow) in &pairs {
                assert!(bright > other_bright || glow <= other_glow || bright == other_bright);
            }
        }
        // The leaves: every pixel, the holes too (so that the smaller levels keep it), has a blue byte of scatter (from 65: below it is porosity); a leaf lets through at least 0.55;
        // there is no emission or smoothness, and the reflectance (the green byte) is a level of a dielectric, below the metals' bytes from 230 (the owner painted some pixels at 30 on 2026-11-18;
        // before that it was 0 everywhere).
        let (leaves, scatter) = (tile("leaves"), tile("leaves_s"));
        assert_eq!(scatter.size(), leaves.size());
        for (colour, specular) in leaves.rgba().chunks(4).zip(scatter.rgba().chunks(4)) {
            assert_eq!([specular[0], specular[3]], [0, 0]);
            assert!(specular[1] < 230, "a leaf pixel is a metal: {specular:?}");
            let share = (f64::from(specular[2]) - 64.0) / 191.0;
            assert!(share > 0.0, "a pixel lets nothing through: {specular:?}");
            if colour[3] > 0 {
                // (0.55 of the 191 steps above 64 is 105.05, which the script rounds to 105: a byte of 169.)
                assert!(specular[2] >= 169, "a leaf pixel scatters {share:.3}");
            }
        }
    }

    #[test]
    fn the_pack_holds_every_picture_of_the_textures_folder_as_the_file_has_it_and_nothing_else() {
        // `build.rs` takes every `.png` of the folder (the folder also holds the painter's tools and notes, which are not in the pack): none is forgotten, none is
        // changed, and a file of the pack is a file of the folder.
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("textures");
        let mut on_disk: Vec<String> = std::fs::read_dir(&folder)
            .expect("the textures folder is there")
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter_map(|name| name.strip_suffix(".png").map(str::to_owned))
            .collect();
        on_disk.sort();
        let mut in_the_pack: Vec<String> = textures()
            .iter()
            .map(|texture| texture.id.name().to_owned())
            .collect();
        in_the_pack.sort();
        assert_eq!(in_the_pack, on_disk);
        assert!(on_disk.len() >= 20, "{on_disk:?}");
        for texture in textures() {
            let name = texture.id.name();
            let from_disk = std::fs::read(folder.join(format!("{name}.png")))
                .unwrap_or_else(|error| panic!("{name}.png: {error}"));
            assert_eq!(texture.bytes, from_disk, "{name}");
            assert_eq!(
                texture.file,
                format!("plugins/base_blocks/textures/{name}.png")
            );
        }
    }

    #[test]
    fn the_pack_is_the_canonical_archive_of_the_data_folder_and_the_pictures_and_has_its_own_name()
    {
        use pack_archive::{Archive, FileIndex, Limits};
        let archive = Archive::from_bytes(pack_archive_bytes(), &Limits::default())
            .expect("the plugin's pack is a canonical archive");
        assert_eq!(archive.to_bytes(), pack_archive_bytes());
        let paths: Vec<&str> = archive.files().map(|(path, _)| path).collect();
        assert!(paths.contains(&"pack.json"));
        assert!(paths.contains(&"blocks/stone.json"));
        assert!(paths.contains(&"textures/stone.png"));
        // nothing of the painter's tools: a script or a note is not a picture
        assert!(
            paths.iter().all(|path| path.starts_with("blocks/")
                || path.starts_with("textures/") && path.ends_with(".png")
                || *path == "pack.json"),
            "{paths:?}"
        );
        // the same bytes give the same identity, however often it is asked
        let index = FileIndex::of(&archive, |_| true);
        assert_eq!(
            index.root_hash(),
            FileIndex::of(&archive, |_| true).root_hash()
        );
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
                "engine:joint_line_face",
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

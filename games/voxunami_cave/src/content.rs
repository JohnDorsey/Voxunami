//! What the game is made of: its plugins' content, and how worlds are made and loaded from it.

use std::sync::Arc;

use base_blocks::{
    BaseBlockIds, BaseBlocks, block_channel_identifier, block_registry_kind,
    light_channel_identifier, orientation_channel_identifier, state_channel_identifier,
};
use block_models::{BlockDefinition, Shape, ShapeBox};
use content_registry::{IdentifierRegistry, Registry};
use game_shell::{
    BodySupport, ContentPackage, GameContent, GameDefinition, PackErrors, PackageData, PackageSpec,
    RawPack,
};
use namespaced_id::NamespacedId;
use terrain::GeneratorId;
use voxel_grid::{ChannelDefinition, ChannelIndex};
use voxel_space::FaceDirection;
use world_storage::{GeneratorRecipe, LoadContext, RecipeError, WorldData};
use world_view::ViewChannels;

use crate::generators::{
    self, great_rivers_generator_identifier, rolling_hills_generator_identifier,
};
use crate::items::{CREATIVE_ONLY, CREATIVE_ONLY_BY_DEFAULT, PLACEABLE, PLACEABLE_BY_DEFAULT};
use crate::multiscale_light;

/// How bright, from 0 to 255, a face is that no light reaches. There is no sky light yet (it was
/// stood in for by 160, a lit day), so the world is as dark as its lamps leave it, until sky light
/// is calculated. A player can change how it is drawn with the window's `.ambient` command.
pub const AMBIENT_LIGHT: u8 = 0;

/// Terrain generators are assigned to cubes of 8 bricks (256 voxels) per side.
pub const ASSIGNMENT_CUBE_LEVEL: u8 = 3;

/// The level players start in, and the only one a new world has.
pub fn overworld_id() -> NamespacedId {
    "voxunami_cave:overworld"
        .parse()
        .expect("constant identifier is valid")
}

/// The level of space with a planet in it (Phase 39), where players start in a new world. It has no pull of its own: what is there falls toward the planet.
pub fn mini_cosmos_id() -> NamespacedId {
    "voxunami_cave:mini_cosmos"
        .parse()
        .expect("constant identifier is valid")
}

pub fn game_id() -> NamespacedId {
    "voxunami_cave:game"
        .parse()
        .expect("constant identifier is valid")
}

/// The plugins this game uses, declared as a package. Both the server and every client have one.
#[derive(Debug)]
pub struct CaveContent {
    /// The ids of the base blocks (the registry itself is in the package).
    pub blocks: BaseBlockIds,
    package: ContentPackage,
}

/// The identifier of the dev docking port.
pub fn dev_docking_port_id() -> NamespacedId {
    "voxunami_cave:dev_docking_port"
        .parse()
        .expect("constant identifier is valid")
}

/// A block of Cave that is a data file (in `data/blocks/`), read against the properties of the base blocks and of Cave. `path` is the file's, to name it in an error; `name` is the
/// block's, after `voxunami_cave:`.
fn block_from_data(path: &str, name: &str, text: &str) -> (NamespacedId, BlockDefinition) {
    let mut properties = base_blocks::property_schema();
    for declaration in [PLACEABLE.declaration(), CREATIVE_ONLY.declaration()] {
        properties
            .declare(declaration)
            .expect("the properties of Cave are distinct from those of the base blocks");
    }
    data_packs::read_block(path, "voxunami_cave", name, text, &properties)
        .unwrap_or_else(|errors| panic!("the file of {name} is wrong: {errors}"))
}

/// The dev docking port (a block for building and testing ships, creative only): a cube with one yellow face, the one that docks with
/// the yellow face of another. It has one state, made with the yellow face to the east, which is also its front, and its top up: it is placed with its top away from the face that was clicked, as a log is, and
/// its front turned to the player's side as far as that allows (see `placement.rs`); the orientation of the voxel says which way the yellow is. It is data: `data/blocks/dev_docking_port.json`, and its
/// colour is the average of its pictures, which are in the pictures of `base_blocks`.
pub fn dev_docking_port() -> BlockDefinition {
    let (identifier, definition) = block_from_data(
        "games/voxunami_cave/data/blocks/dev_docking_port.json",
        "dev_docking_port",
        include_str!("../data/blocks/dev_docking_port.json"),
    );
    assert_eq!(identifier, dev_docking_port_id());
    let mut port = [(identifier, definition)];
    data_packs::settle_game_pictures(&mut port, &base_blocks::textures()).unwrap_or_else(
        |errors| panic!("the pictures of the dev docking port are wrong: {errors}"),
    );
    let [(_, definition)] = port;
    definition
}

/// The identifier of the dev ball joint.
pub fn dev_ball_joint_id() -> NamespacedId {
    "voxunami_cave:dev_ball_joint"
        .parse()
        .expect("constant identifier is valid")
}

/// The dev ball joint (a block for building and testing ships with parts that move against each other, creative only): a gunmetal gray bar of 0.75 by 0.5 by 0.5 metres whose end is a
/// blue face, the end of the joint (`engine:joint_face`); two of them whose blue ends look at each other across a cell boundary, half a metre apart, hold a ball between them that
/// the two cars turn on (`docs/decisions/joints-and-cars.md`). It has one state, made with the blue end up, its flat end flush with the bottom of the cell and the blue end a quarter of a metre
/// short of the top, centred across. It is placed like a log, with the blue end away from the face that was clicked (the bar is the same all round, so it has no front). It is data:
/// `data/blocks/dev_ball_joint.json`, in flat colours: it has no pictures yet.
pub fn dev_ball_joint() -> BlockDefinition {
    let (identifier, definition) = block_from_data(
        "games/voxunami_cave/data/blocks/dev_ball_joint.json",
        "dev_ball_joint",
        include_str!("../data/blocks/dev_ball_joint.json"),
    );
    assert_eq!(identifier, dev_ball_joint_id());
    definition
}

/// The identifier of the dev command seat.
pub fn dev_command_seat_id() -> NamespacedId {
    "voxunami_cave:dev_command_seat"
        .parse()
        .expect("constant identifier is valid")
}

/// The identifier of the dev rocket engine.
pub fn dev_rocket_engine_id() -> NamespacedId {
    "voxunami_cave:dev_rocket_engine"
        .parse()
        .expect("constant identifier is valid")
}

/// The colour of the dev blocks of flight, gunmetal gray (that of the dev docking port).
const GUNMETAL: [f32; 3] = [0.17, 0.21, 0.22];

/// The brown of the surfaces a sitter sits against, and the red of the exhaust face (the bytes the faces were given in before a face had a colour of its own).
const SEAT_BROWN: [f32; 3] = [110.0 / 255.0, 70.0 / 255.0, 35.0 / 255.0];
const EXHAUST_RED: [f32; 3] = [200.0 / 255.0, 30.0 / 255.0, 30.0 / 255.0];

/// The dev command seat (a block for building and testing ships, creative only): a stair in gunmetal gray whose bottom and back, the surfaces a sitter sits against, are brown.
/// It is made with its back to the east, so a sitter faces west; its back is its front face, so it is placed with its back to the player and whoever sits in it faces the way
/// the player looked. Its orientation says which way the sitter faces and which way is up for them.
pub fn dev_command_seat() -> BlockDefinition {
    BlockDefinition::cube(GUNMETAL)
        .with_shapes(vec![Shape::stair(FaceDirection::PositiveX)])
        .with_face_colors(&[
            (FaceDirection::PositiveX, SEAT_BROWN),
            (FaceDirection::NegativeY, SEAT_BROWN),
        ])
        .with_front_face(FaceDirection::PositiveX)
}

/// The dev rocket engine (a block for building and testing ships, creative only): a pyramid of three tiers in gunmetal gray, wide at the exhaust end, which is a red face. It
/// is made with the exhaust up, and is placed like a log, along the face it is put against with the exhaust end away from it, so it pushes the ship toward that surface.
pub fn dev_rocket_engine() -> BlockDefinition {
    // (In eighths of a voxel: the wide tier at the top, the narrowest at the bottom.)
    let tiers = Shape::from_boxes(&[
        ShapeBox {
            min: [0, 5, 0],
            max: [8, 8, 8],
        },
        ShapeBox {
            min: [1, 3, 1],
            max: [7, 5, 7],
        },
        ShapeBox {
            min: [2, 0, 2],
            max: [6, 3, 6],
        },
    ]);
    BlockDefinition::cube(GUNMETAL)
        .with_shapes(vec![tiers])
        .with_face_colors(&[(FaceDirection::PositiveY, EXHAUST_RED)])
        .with_axis_face(FaceDirection::PositiveY)
}

/// The blocks of a registry, with the flags that say a player can place a block, and that a block is for creative only, set on the ones Cave says
/// they are.
fn mark_placeable(blocks: Registry<BlockDefinition>) -> Registry<BlockDefinition> {
    let key = |property: &str| -> NamespacedId {
        property.parse().expect("a property key is an identifier")
    };
    let (placeable, creative_only) = (key(PLACEABLE.id()), key(CREATIVE_ONLY.id()));
    let mut marked = Registry::new();
    for (_, entry) in blocks.iter() {
        let identifier = entry.identifier().clone();
        match entry.content() {
            Some(definition) => {
                let mut definition = definition.clone();
                if PLACEABLE_BY_DEFAULT.contains(&identifier.as_str()) {
                    definition.set_property_value(placeable.clone(), None, 1);
                }
                if CREATIVE_ONLY_BY_DEFAULT.contains(&identifier.as_str()) {
                    definition.set_property_value(creative_only.clone(), None, 1);
                }
                marked
                    .register(identifier, definition)
                    .expect("the blocks of a registry are distinct");
            }
            None => {
                marked.register_missing(identifier);
            }
        }
    }
    marked
}

impl Default for CaveContent {
    fn default() -> Self {
        Self::new()
    }
}

impl CaveContent {
    /// A new world with the overworld alone in it, which is the world Cave began with (rolling hills everywhere, nothing edited): the world the tests of the
    /// overworld start in. A new game starts in [`GameContent::new_world`].
    pub fn new_overworld_world(&self, seed: u64) -> WorldData {
        self.package.new_world(seed)
    }

    pub fn new() -> Self {
        let base = BaseBlocks::new();
        let ids = base.ids();
        // The base blocks have the ids they always had; Cave's own come after them.
        let mut blocks = base.into_registry();
        blocks
            .register(dev_docking_port_id(), dev_docking_port())
            .expect("the name of the dev docking port is not a base block's");
        blocks
            .register(dev_command_seat_id(), dev_command_seat())
            .expect("the name of the dev command seat is not another block's");
        blocks
            .register(dev_rocket_engine_id(), dev_rocket_engine())
            .expect("the name of the dev rocket engine is not another block's");
        blocks
            .register(dev_ball_joint_id(), dev_ball_joint())
            .expect("the name of the dev ball joint is not another block's");
        let spec = PackageSpec {
            textures: base_blocks::textures(),
            level: overworld_id(),
            block_registry_kind: block_registry_kind(),
            // The channels of every brick: the block of each voxel, its state (which shape it
            // has), and the block light in it.
            channels: vec![
                ids.block_channel(),
                ids.state_channel(),
                ids.light_channel(),
                // The levels of the light of each scale, kept at the first voxel of each cell (a
                // world has either these or the light channel lit, see `LightModel`).
                ChannelDefinition::number(multiscale_light::channel_identifier(0), 0),
                ChannelDefinition::number(multiscale_light::channel_identifier(1), 0),
                ChannelDefinition::number(multiscale_light::channel_identifier(2), 0),
                ChannelDefinition::number(multiscale_light::channel_identifier(3), 0),
                ChannelDefinition::number(multiscale_light::channel_identifier(4), 0),
                ChannelDefinition::number(multiscale_light::channel_identifier(5), 0),
                ChannelDefinition::number(multiscale_light::channel_identifier(6), 0),
                // (After all the others, so that the numbers of the channels of worlds saved before it do not move.)
                ids.orientation_channel(),
            ],
            block_channel: block_channel_identifier(),
            state_channel: Some(state_channel_identifier()),
            orientation_channel: Some(orientation_channel_identifier()),
            light_channel: Some(light_channel_identifier()),
            properties: {
                let mut properties = base_blocks::property_declarations();
                properties.push(PLACEABLE.declaration());
                properties.push(CREATIVE_ONLY.declaration());
                properties
            },
            generators: Arc::new(move |seed, layout| {
                let block_channel = layout
                    .channel_index(&block_channel_identifier())
                    .expect("the layout has the block channel");
                generators::terrain_generators(&ids, seed, block_channel)
            }),
            families: Arc::new(move |layout| {
                let block_channel = layout
                    .channel_index(&block_channel_identifier())
                    .expect("the layout has the block channel");
                generators::generator_families(&ids, block_channel)
            }),
            // The planet of a new world is a body of the game's own data (`data/bodies/planet.json`); packs may add more.
            body_support: BodySupport {
                families: generators::family_data(),
                levels: vec![mini_cosmos_id(), overworld_id()],
            },
            builtin_bodies: vec![(
                "voxunami_cave:planet"
                    .parse()
                    .expect("a constant identifier is valid"),
                include_str!("../data/bodies/planet.json"),
            )],
            pull_strength: crate::game::Cave::gravity().pull_strength,
            default_generator: rolling_hills_generator_identifier(),
            assignment_cube_level: ASSIGNMENT_CUBE_LEVEL,
            ambient_light: AMBIENT_LIGHT,
        };
        Self {
            blocks: ids,
            package: ContentPackage::new(mark_placeable(blocks), spec),
        }
    }

    /// Adds a terrain generator that is data to a world of this game (a planet of another size or seed, for instance): `recipe` names the instance, the family
    /// (`voxunami_cave:planet_v1` is the planets) and the parameters. The world keeps the recipe, and a client that joins makes the same generator from it.
    pub fn add_generator_recipe(
        &self,
        world: &mut WorldData,
        recipe: GeneratorRecipe,
    ) -> Result<GeneratorId, RecipeError> {
        self.package.add_generator_recipe(world, recipe)
    }

    /// The channel holding each voxel's block.
    pub fn block_channel(&self) -> ChannelIndex {
        self.package.block_channel()
    }

    /// The channel holding each voxel's block state.
    /// The channel holding each voxel's orientation.
    pub fn orientation_channel(&self) -> ChannelIndex {
        self.package.orientation_channel()
    }

    pub fn state_channel(&self) -> ChannelIndex {
        self.package.state_channel()
    }

    /// The channel holding the block light of each voxel.
    pub fn light_channel(&self) -> ChannelIndex {
        self.package.channel(&light_channel_identifier())
    }

    /// The channel holding the levels of the light of a scale (see [`crate::multiscale_light`]).
    pub fn scale_channel(&self, scale: usize) -> ChannelIndex {
        self.package
            .channel(&multiscale_light::channel_identifier(scale))
    }
}

impl GameContent for CaveContent {
    fn view_channels(&self) -> ViewChannels {
        self.package.view_channels()
    }

    fn package_data(&self) -> PackageData {
        self.package.package_data()
    }

    fn apply_packs(&mut self, packs: &[RawPack]) -> Result<Vec<String>, PackErrors> {
        self.package.apply_packs(packs)
    }

    fn body_support(&self) -> BodySupport {
        self.package.body_support()
    }

    fn body_names(&self) -> Vec<NamespacedId> {
        self.package.body_names()
    }

    /// A new world with this seed: a level of space first, with the planet in it and the great rivers across it, and the overworld (rolling hills everywhere) second. Nothing is edited. A world
    /// that was saved before there was a planet is loaded as it was: with the overworld as its only level.
    fn new_world(&self, seed: u64) -> WorldData {
        let mut world = self.package.new_world_of(
            seed,
            &[
                (mini_cosmos_id(), great_rivers_generator_identifier()),
                (overworld_id(), rolling_hills_generator_identifier()),
            ],
        );
        // The planet, and the bodies of the packs: the files were checked when the packs were loaded.
        self.package
            .add_bodies(&mut world)
            .unwrap_or_else(|why| panic!("the bodies of a new world cannot be made: {why}"));
        world
    }

    /// What loading a world file, or a server's description of a world, needs. `seed` is the
    /// world's: terrain generators are built from it.
    fn load_context(&mut self, seed: u64) -> LoadContext<'_> {
        self.package.load_context(seed)
    }

    /// The content tables a save or a description needs.
    fn registries(&self) -> Vec<(NamespacedId, &dyn IdentifierRegistry)> {
        self.package.registries()
    }
}

#[cfg(test)]
mod tests {
    use terrain::WorldTerrain;
    use voxel_grid::{Grid, GridLayout};
    use world_storage::{LevelData, ParsedWorld};

    use super::*;

    #[test]
    fn a_new_overworld_world_saves_and_loads_back_as_the_same_world() {
        let content = CaveContent::new();
        let world = content.new_overworld_world(77);
        let bytes = world.save(&content.registries()).unwrap();

        let mut later = CaveContent::new();
        let parsed = ParsedWorld::parse(&bytes).unwrap();
        let seed = parsed.world_seed();
        let (loaded, report) = parsed.load(later.load_context(seed)).unwrap();
        assert_eq!(seed, 77);
        assert!(report.missing_content.is_empty());
        assert!(report.extra_channels.is_empty());
        assert_eq!(loaded.world_seed, 77);
        assert_eq!(
            loaded.main().preferred_default_generator(),
            &rolling_hills_generator_identifier()
        );
        assert_eq!(loaded.save(&later.registries()).unwrap(), bytes);
    }

    #[test]
    fn a_world_saved_before_blocks_had_states_loads_with_every_block_in_the_first_state() {
        // The old layout had the block channel alone.
        let content = CaveContent::new();
        let old_layout = Arc::new(
            GridLayout::new(vec![content.blocks.block_channel()])
                .expect("one channel is a valid layout"),
        );
        let generators =
            generators::terrain_generators(&content.blocks, 5, content.block_channel());
        let default = generators
            .runtime_id(&rolling_hills_generator_identifier())
            .unwrap();
        let mut old_world = WorldData::new(
            5,
            overworld_id(),
            LevelData::new(
                WorldTerrain::new(generators, default, ASSIGNMENT_CUBE_LEVEL).unwrap(),
                Grid::new(old_layout),
            ),
        );
        let edited = voxel_space::VoxelCoordinate::new(3, 4, 5);
        old_world
            .main_mut()
            .overrides
            .set(content.block_channel(), edited, content.blocks.stone);
        let bytes = old_world.save(&content.registries()).unwrap();

        let mut later = CaveContent::new();
        let parsed = ParsedWorld::parse(&bytes).unwrap();
        let seed = parsed.world_seed();
        let (loaded, report) = parsed.load(later.load_context(seed)).unwrap();
        assert!(report.missing_content.is_empty());
        assert!(report.extra_channels.is_empty());
        let overrides = &loaded.main().overrides;
        assert_eq!(
            overrides.get(later.block_channel(), edited),
            later.blocks.stone
        );
        assert_eq!(overrides.get(later.state_channel(), edited), 0);
        // Saving it again writes the channel it now has.
        assert_ne!(loaded.save(&later.registries()).unwrap(), bytes);
    }

    #[test]
    fn faces_no_light_reaches_are_black_until_there_is_sky_light() {
        assert_eq!(AMBIENT_LIGHT, 0);
        assert_eq!(CaveContent::new().ambient_light(), 0);
    }

    #[test]
    fn the_block_channel_is_the_first_and_defaults_to_air() {
        let content = CaveContent::new();
        assert_eq!(content.block_channel().get(), 0);
        assert_eq!(content.state_channel().get(), 1);
        assert_eq!(content.light_channel().get(), 2);
        assert_eq!(content.blocks.light_channel().default_value, 0);
        assert_eq!(content.blocks.state_channel().default_value, 0);
        assert_eq!(
            content.blocks.block_channel().default_value,
            content.blocks.air
        );
    }
}

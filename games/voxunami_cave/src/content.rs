//! What the game is made of: its plugins' content, and how worlds are made and loaded from it.

use std::sync::Arc;

use base_blocks::{
    BaseBlockIds, BaseBlocks, block_channel_identifier, block_registry_kind,
    empty_generator_identifier, light_channel_identifier, orientation_channel_identifier,
    rolling_hills_generator_identifier, small_planet_generator_identifier,
    state_channel_identifier,
};
use block_models::BlockDefinition;
use content_registry::{IdentifierRegistry, Registry};
use game_shell::{
    ContentPackage, GameContent, GameDefinition, PackErrors, PackageData, PackageSpec, RawPack,
};
use namespaced_id::NamespacedId;
use terrain::GeneratorId;
use terrain::PlanetSettings;
use voxel_grid::{ChannelDefinition, ChannelIndex, Grid};
use voxel_space::{BrickBox, BrickCoordinate, FaceDirection, GridPose, LevelIndex};
use world_storage::{
    GeneratorRecipe, GridTerrain, LoadContext, MovingGrid, PilotLogout, RecipeError, WorldData,
};
use world_view::ViewChannels;

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

/// The dev docking port (a block for building and testing ships, creative only): a gunmetal gray cube with one yellow face, the one that docks with
/// the yellow face of another. It has one state, made with the yellow face to the east, which is also its front: it is placed turned to face the
/// player, as a piston is, and the orientation of the voxel says which way the yellow is.
pub fn dev_docking_port() -> BlockDefinition {
    BlockDefinition::cube([0.17, 0.21, 0.22])
        .with_marked_faces([255, 216, 0], &[Some(FaceDirection::PositiveX)])
        .with_docking_faces(&[Some(FaceDirection::PositiveX)])
        .with_front_face(FaceDirection::PositiveX)
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

/// How hard the planet of a new world pulls at its surface, in voxels a second a second (the owner, 2026-10-24: ten; the overworld's pull is five).
pub const PLANET_SURFACE_GRAVITY: f64 = 10.0;

/// How many bricks from its centre, each way along each axis, the planet is made: enough to hold its ground with the highest hills it can have (those of a face
/// reach 320 voxels, and the blend of the faces up to 1.8 times that, over a radius of 63,710).
const PLANET_REACH_BRICKS: u32 = 2_100;

/// The planet of a new world: a ship in the first level, centred on its origin and held there, made by the planet's generator, as heavy as it must be to pull
/// [`PLANET_SURFACE_GRAVITY`] at its surface with the pull of Cave's gravity. Nothing is stored for it.
fn planet(world: &WorldData, seed: u64) -> MovingGrid {
    let radius = PlanetSettings::one_percent_of_earth(seed).radius;
    let pull = f64::from(crate::game::Cave::gravity().pull_strength);
    let mass = (PLANET_SURFACE_GRAVITY * radius * radius / pull) as f32;
    let first = world.main();
    let generator = first
        .terrain
        .generators()
        .runtime_id(&small_planet_generator_identifier())
        .expect("the package has the planet's generator");
    MovingGrid {
        level: LevelIndex::MAIN,
        pose: GridPose::identity(),
        linear_velocity: [0.0; 3],
        angular_velocity: [0.0; 3],
        mass,
        centre_of_mass: [0.0; 3],
        held: true,
        frozen: false,
        on_pilot_logout: PilotLogout::Stay,
        voxels: Grid::new(std::sync::Arc::clone(first.overrides.layout())),
        terrain: Some(GridTerrain {
            generator,
            extent: BrickBox::around(BrickCoordinate::ORIGIN, PLANET_REACH_BRICKS),
            mass,
        }),
        sections: Vec::new(),
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
        let spec = PackageSpec {
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
                ids.terrain_generators(seed, block_channel)
            }),
            families: Arc::new(move |layout| {
                let block_channel = layout
                    .channel_index(&block_channel_identifier())
                    .expect("the layout has the block channel");
                ids.generator_families(block_channel)
            }),
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
    /// (`base_blocks:planet_v1` is the planets) and the parameters. The world keeps the recipe, and a client that joins makes the same generator from it.
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

    /// A new world with this seed: a level of space first, with the planet in it, and the overworld (rolling hills everywhere) second. Nothing is edited. A world
    /// that was saved before there was a planet is loaded as it was: with the overworld as its only level.
    fn new_world(&self, seed: u64) -> WorldData {
        let mut world = self.package.new_world_of(
            seed,
            &[
                (mini_cosmos_id(), empty_generator_identifier()),
                (overworld_id(), rolling_hills_generator_identifier()),
            ],
        );
        let planet = planet(&world, seed);
        world.grids_mut().create(planet);
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
        let generators = content
            .blocks
            .terrain_generators(5, content.block_channel());
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

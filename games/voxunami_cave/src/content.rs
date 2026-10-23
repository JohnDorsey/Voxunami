//! What the game is made of: its plugins' content, and how worlds are made and loaded from it.

use std::sync::Arc;

use base_blocks::{
    BaseBlockIds, BaseBlocks, block_channel_identifier, block_registry_kind,
    light_channel_identifier, orientation_channel_identifier, rolling_hills_generator_identifier,
    state_channel_identifier,
};
use block_models::BlockDefinition;
use content_registry::{IdentifierRegistry, Registry};
use game_shell::{ContentPackage, GameContent, PackErrors, PackageData, PackageSpec, RawPack};
use namespaced_id::NamespacedId;
use voxel_grid::{ChannelDefinition, ChannelIndex};
use voxel_space::FaceDirection;
use world_storage::{LoadContext, WorldData};
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

impl CaveContent {
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
            default_generator: rolling_hills_generator_identifier(),
            assignment_cube_level: ASSIGNMENT_CUBE_LEVEL,
            ambient_light: AMBIENT_LIGHT,
        };
        Self {
            blocks: ids,
            package: ContentPackage::new(mark_placeable(blocks), spec),
        }
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

    /// A new world with this seed: rolling hills everywhere, nothing edited.
    fn new_world(&self, seed: u64) -> WorldData {
        self.package.new_world(seed)
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
    fn a_new_world_saves_and_loads_back_as_the_same_world() {
        let content = CaveContent::new();
        let world = content.new_world(77);
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

//! What the game is made of: its plugins' content, and how worlds are made and loaded from it.

use std::sync::Arc;

use base_blocks::{
    BaseBlocks, block_channel_identifier, block_registry_kind, rolling_hills_generator_identifier,
    state_channel_identifier,
};
use block_models::BlockTable;
use content_registry::IdentifierRegistry;
use game_shell::GameContent;
use namespaced_id::NamespacedId;
use terrain::WorldTerrain;
use voxel_grid::{ChannelDefinition, ChannelIndex, Grid, GridLayout};
use world_storage::{LevelData, LoadContext, WorldData};
use world_view::ViewChannels;

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

/// The plugins this game uses. Both the server and every client have one.
#[derive(Debug)]
pub struct CaveContent {
    pub blocks: BaseBlocks,
}

impl Default for CaveContent {
    fn default() -> Self {
        Self::new()
    }
}

impl CaveContent {
    pub fn new() -> Self {
        Self {
            blocks: BaseBlocks::new(),
        }
    }

    fn layout(&self) -> Arc<GridLayout> {
        Arc::new(GridLayout::new(self.channels()).expect("the channels are distinct"))
    }

    /// The channels of every brick: the block of each voxel, and its state (which shape it has).
    fn channels(&self) -> Vec<ChannelDefinition> {
        vec![self.blocks.block_channel(), self.blocks.state_channel()]
    }

    /// The channel holding each voxel's block.
    pub fn block_channel(&self) -> ChannelIndex {
        self.layout()
            .channel_index(&block_channel_identifier())
            .expect("the layout has the block channel")
    }

    /// The channel holding each voxel's block state.
    pub fn state_channel(&self) -> ChannelIndex {
        self.layout()
            .channel_index(&state_channel_identifier())
            .expect("the layout has the state channel")
    }
}

impl GameContent for CaveContent {
    fn view_channels(&self) -> ViewChannels {
        ViewChannels {
            blocks: self.block_channel(),
            states: Some(self.state_channel()),
            light: None,
        }
    }

    fn block_table(&self) -> BlockTable {
        self.blocks.block_table()
    }

    /// A new world with this seed: rolling hills everywhere, nothing edited.
    fn new_world(&self, seed: u64) -> WorldData {
        let layout = self.layout();
        let generators = self.blocks.terrain_generators(seed, self.block_channel());
        let default = generators
            .runtime_id(&rolling_hills_generator_identifier())
            .expect("the plugin registers its generator");
        let terrain = WorldTerrain::new(generators, default, ASSIGNMENT_CUBE_LEVEL)
            .expect("the generator is available");
        WorldData::new(
            seed,
            overworld_id(),
            LevelData::new(terrain, Grid::new(layout)),
        )
    }

    /// What loading a world file, or a server's description of a world, needs. `seed` is the
    /// world's: terrain generators are built from it.
    fn load_context(&mut self, seed: u64) -> LoadContext<'_> {
        let block_channel = self.block_channel();
        let generators = self.blocks.terrain_generators(seed, block_channel);
        LoadContext {
            channels: self.channels(),
            registries: vec![(block_registry_kind(), &mut self.blocks.blocks)],
            generators,
            fallback_default_generator: rolling_hills_generator_identifier(),
            assignment_cube_level: ASSIGNMENT_CUBE_LEVEL,
            legacy_level: overworld_id(),
        }
    }

    /// The content tables a save or a description needs.
    fn registries(&self) -> Vec<(NamespacedId, &dyn IdentifierRegistry)> {
        vec![(block_registry_kind(), &self.blocks.blocks)]
    }
}

#[cfg(test)]
mod tests {
    use world_storage::ParsedWorld;

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
    fn the_block_channel_is_the_first_and_defaults_to_air() {
        let content = CaveContent::new();
        assert_eq!(content.block_channel().get(), 0);
        assert_eq!(content.state_channel().get(), 1);
        assert_eq!(content.blocks.state_channel().default_value, 0);
        assert_eq!(
            content.blocks.block_channel().default_value,
            content.blocks.air
        );
    }
}

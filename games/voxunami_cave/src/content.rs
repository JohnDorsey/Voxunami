//! What the game is made of: its plugins' content, and how worlds are made and loaded from it.

use std::sync::Arc;

use base_blocks::{
    BaseBlocks, block_channel_identifier, block_registry_kind, rolling_hills_generator_identifier,
};
use content_registry::IdentifierRegistry;
use game_shell::GameContent;
use namespaced_id::NamespacedId;
use terrain::WorldTerrain;
use voxel_grid::{ChannelIndex, Grid, GridLayout};
use world_storage::{LoadContext, WorldData};

/// Terrain generators are assigned to cubes of 8 bricks (256 voxels) per side.
pub const ASSIGNMENT_CUBE_LEVEL: u8 = 3;

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
        Arc::new(
            GridLayout::new(vec![self.blocks.block_channel()])
                .expect("one channel is a valid layout"),
        )
    }

    /// The channel holding each voxel's block.
    pub fn block_channel(&self) -> ChannelIndex {
        self.layout()
            .channel_index(&block_channel_identifier())
            .expect("the layout has the block channel")
    }
}

impl GameContent for CaveContent {
    fn material_channel(&self) -> ChannelIndex {
        self.block_channel()
    }

    fn palette(&self) -> Vec<[f32; 3]> {
        self.blocks.palette()
    }

    fn opacity_table(&self) -> Vec<bool> {
        self.blocks.opacity_table()
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
        WorldData::new(seed, terrain, Grid::new(layout))
    }

    /// What loading a world file, or a server's description of a world, needs. `seed` is the
    /// world's: terrain generators are built from it.
    fn load_context(&mut self, seed: u64) -> LoadContext<'_> {
        let block_channel = self.block_channel();
        let generators = self.blocks.terrain_generators(seed, block_channel);
        LoadContext {
            channels: vec![self.blocks.block_channel()],
            registries: vec![(block_registry_kind(), &mut self.blocks.blocks)],
            generators,
            fallback_default_generator: rolling_hills_generator_identifier(),
            assignment_cube_level: ASSIGNMENT_CUBE_LEVEL,
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
            loaded.preferred_default_generator(),
            &rolling_hills_generator_identifier()
        );
        assert_eq!(loaded.save(&later.registries()).unwrap(), bytes);
    }

    #[test]
    fn the_block_channel_is_the_first_and_defaults_to_air() {
        let content = CaveContent::new();
        assert_eq!(content.block_channel().get(), 0);
        assert_eq!(
            content.blocks.block_channel().default_value,
            content.blocks.air
        );
    }
}

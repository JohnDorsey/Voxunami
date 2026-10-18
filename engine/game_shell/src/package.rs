//! A game's content as one declared package: what a game that has one block registry, one set of
//! channels and one terrain generator says to the shell, instead of writing the same
//! [`GameContent`] by hand.
//!
//! A [`PackageSpec`] is everything a game declares about its world: the level players start in,
//! the channels of every voxel, which of them the view reads, the terrain generators, and how
//! the blocks look. [`ContentPackage`] builds worlds and loads them from it. This is the shape a
//! data-driven mod will fill in later, so what a spec holds is what a mod's manifest will need.

use std::fmt;
use std::sync::Arc;

use block_models::{BlockDefinition, BlockTable, PropertyDeclaration, PropertySchema};
use content_registry::{IdentifierRegistry, Registry};
use namespaced_id::NamespacedId;
use terrain::{TerrainGenerator, WorldTerrain};
use voxel_grid::{ChannelDefinition, ChannelIndex, Grid, GridLayout};
use world_storage::{LevelData, LoadContext, WorldData};
use world_view::ViewChannels;

use crate::definition::GameContent;

/// Makes a game's terrain generators for a world seed. The layout is the world's, for finding the
/// channels a generator writes.
pub type GeneratorFactory =
    Arc<dyn Fn(u64, &GridLayout) -> Registry<Arc<dyn TerrainGenerator>> + Send + Sync>;

/// What a game declares about its world.
#[derive(Clone)]
pub struct PackageSpec {
    /// The level players start in, and the only one a new world has.
    pub level: NamespacedId,
    /// The kind of the block registry in world files (such as `my_game:block`).
    pub block_registry_kind: NamespacedId,
    /// The channels of every brick, in order. The first is conventionally the block channel.
    pub channels: Vec<ChannelDefinition>,
    /// Which channel holds each voxel's block, and where the game has them, its state and light.
    pub block_channel: NamespacedId,
    pub state_channel: Option<NamespacedId>,
    pub light_channel: Option<NamespacedId>,
    /// The properties of blocks that the game's plugins declare, besides the engine's.
    pub properties: Vec<PropertyDeclaration>,
    pub generators: GeneratorFactory,
    /// The generator new regions are given, and the one used if a world's own is missing.
    pub default_generator: NamespacedId,
    /// Generators are assigned to cubes of 8 to the power of this many bricks per side (see
    /// `WorldTerrain`).
    pub assignment_cube_level: u8,
    /// How bright a face is that no light reaches, 0 to 255.
    pub ambient_light: u8,
}

impl fmt::Debug for PackageSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PackageSpec")
            .field("level", &self.level)
            .field("channels", &self.channels.len())
            .field("default_generator", &self.default_generator)
            .finish_non_exhaustive()
    }
}

/// A game's blocks and its declaration, as the shell's [`GameContent`].
#[derive(Debug)]
pub struct ContentPackage {
    blocks: Registry<BlockDefinition>,
    spec: PackageSpec,
}

impl ContentPackage {
    pub fn new(blocks: Registry<BlockDefinition>, spec: PackageSpec) -> Self {
        Self { blocks, spec }
    }

    fn layout(&self) -> Arc<GridLayout> {
        Arc::new(GridLayout::new(self.spec.channels.clone()).expect("the channels are distinct"))
    }

    /// The channel with an identifier. Panics if the package has no such channel, which is a
    /// mistake in the game's declaration.
    pub fn channel(&self, identifier: &NamespacedId) -> ChannelIndex {
        self.layout()
            .channel_index(identifier)
            .unwrap_or_else(|| panic!("the package has no channel {identifier}"))
    }

    /// The channel holding each voxel's block.
    pub fn block_channel(&self) -> ChannelIndex {
        self.channel(&self.spec.block_channel)
    }

    /// The channel holding each voxel's state. Panics if the game has none.
    pub fn state_channel(&self) -> ChannelIndex {
        self.channel(
            self.spec
                .state_channel
                .as_ref()
                .expect("the package declares a state channel"),
        )
    }

    fn generators(&self, seed: u64) -> Registry<Arc<dyn TerrainGenerator>> {
        (self.spec.generators)(seed, &self.layout())
    }
}

impl GameContent for ContentPackage {
    fn view_channels(&self) -> ViewChannels {
        ViewChannels::new(
            self.block_channel(),
            self.spec.state_channel.as_ref().map(|id| self.channel(id)),
            self.spec.light_channel.as_ref().map(|id| self.channel(id)),
        )
    }

    fn ambient_light(&self) -> u8 {
        self.spec.ambient_light
    }

    fn block_table(&self) -> BlockTable {
        let definitions: Vec<Option<&BlockDefinition>> = self
            .blocks
            .iter()
            .map(|(_, entry)| entry.content())
            .collect();
        let mut schema = PropertySchema::new();
        for declaration in &self.spec.properties {
            schema.declare(declaration.clone()).unwrap_or_else(|error| {
                panic!("the package's properties are inconsistent: {error}")
            });
        }
        BlockTable::try_from_definitions(&schema, &definitions)
            .unwrap_or_else(|error| panic!("the package's blocks cannot be used: {error}"))
    }

    fn new_world(&self, seed: u64) -> WorldData {
        let generators = self.generators(seed);
        let default = generators
            .runtime_id(&self.spec.default_generator)
            .expect("the package's generators include its default");
        let terrain = WorldTerrain::new(generators, default, self.spec.assignment_cube_level)
            .expect("the default generator is available");
        WorldData::new(
            seed,
            self.spec.level.clone(),
            LevelData::new(terrain, Grid::new(self.layout())),
        )
    }

    fn load_context(&mut self, seed: u64) -> LoadContext<'_> {
        let generators = self.generators(seed);
        LoadContext {
            channels: self.spec.channels.clone(),
            registries: vec![(self.spec.block_registry_kind.clone(), &mut self.blocks)],
            generators,
            fallback_default_generator: self.spec.default_generator.clone(),
            assignment_cube_level: self.spec.assignment_cube_level,
            legacy_level: self.spec.level.clone(),
        }
    }

    fn registries(&self) -> Vec<(NamespacedId, &dyn IdentifierRegistry)> {
        vec![(self.spec.block_registry_kind.clone(), &self.blocks)]
    }
}

#[cfg(test)]
mod tests {
    use voxel_grid::Brick;
    use voxel_space::BrickCoordinate;
    use world_storage::ParsedWorld;

    use super::*;

    fn id(text: &str) -> NamespacedId {
        text.parse().unwrap()
    }

    #[derive(Debug)]
    struct Empty;

    impl TerrainGenerator for Empty {
        fn generate_brick(&self, layout: &GridLayout, _coordinate: BrickCoordinate) -> Brick {
            Brick::new(layout)
        }
    }

    fn package() -> ContentPackage {
        let mut blocks = Registry::new();
        blocks
            .register(id("test_game:air"), BlockDefinition::air())
            .unwrap();
        blocks
            .register(id("test_game:rock"), BlockDefinition::cube([0.5; 3]))
            .unwrap();
        ContentPackage::new(
            blocks,
            PackageSpec {
                level: id("test_game:main"),
                block_registry_kind: id("test_game:block"),
                channels: vec![
                    ChannelDefinition::content(id("test_game:block"), 0, id("test_game:block")),
                    ChannelDefinition::number(id("test_game:state"), 0),
                ],
                block_channel: id("test_game:block"),
                state_channel: Some(id("test_game:state")),
                light_channel: None,
                properties: Vec::new(),
                generators: Arc::new(|_seed, _layout| {
                    let mut generators: Registry<Arc<dyn TerrainGenerator>> = Registry::new();
                    generators
                        .register(id("test_game:empty_v1"), Arc::new(Empty))
                        .unwrap();
                    generators
                }),
                default_generator: id("test_game:empty_v1"),
                assignment_cube_level: 3,
                ambient_light: 90,
            },
        )
    }

    #[test]
    fn the_package_says_what_its_spec_declares() {
        let content = package();
        assert_eq!(content.block_channel().get(), 0);
        assert_eq!(content.state_channel().get(), 1);
        assert_eq!(content.ambient_light(), 90);
        let view = content.view_channels();
        assert_eq!(
            (view.blocks.get(), view.states.map(|s| s.get()), view.light),
            (0, Some(1), None)
        );
        assert_eq!(content.block_table().block_count(), 2);
        assert_eq!(content.registries().len(), 1);
    }

    #[test]
    #[should_panic(expected = "the package has no channel test_game:nothing")]
    fn asking_for_a_channel_that_was_not_declared_is_a_loud_mistake() {
        package().channel(&id("test_game:nothing"));
    }

    #[test]
    fn a_new_world_is_in_the_declared_level_unedited_and_saves_and_loads_back_the_same() {
        let content = package();
        let world = content.new_world(42);
        assert_eq!(world.world_seed, 42);
        assert_eq!(world.main().overrides.brick_count(), 0);
        let bytes = world.save(&content.registries()).unwrap();

        let mut later = package();
        let parsed = ParsedWorld::parse(&bytes).unwrap();
        let seed = parsed.world_seed();
        let (loaded, report) = parsed.load(later.load_context(seed)).unwrap();
        assert_eq!(seed, 42);
        assert!(report.missing_content.is_empty() && report.extra_channels.is_empty());
        assert_eq!(
            loaded.main().preferred_default_generator(),
            &id("test_game:empty_v1")
        );
        assert_eq!(loaded.save(&later.registries()).unwrap(), bytes);
    }

    #[test]
    fn the_generators_are_made_for_the_seed_of_the_world() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut content = package();
        let remembered = Arc::clone(&seen);
        content.spec.generators = Arc::new(move |seed, layout| {
            remembered
                .lock()
                .unwrap()
                .push((seed, layout.channel_count()));
            let mut generators: Registry<Arc<dyn TerrainGenerator>> = Registry::new();
            generators
                .register(id("test_game:empty_v1"), Arc::new(Empty))
                .unwrap();
            generators
        });
        let world = content.new_world(7);
        let bytes = world.save(&content.registries()).unwrap();
        let parsed = ParsedWorld::parse(&bytes).unwrap();
        let _ = parsed.load(content.load_context(7)).unwrap();
        assert_eq!(*seen.lock().unwrap(), [(7, 2), (7, 2)]);
    }
}

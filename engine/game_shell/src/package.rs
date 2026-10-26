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

use block_models::{BlockDefinition, PropertyDeclaration, PropertySchema};
use content_registry::{IdentifierRegistry, Registry};
use namespaced_id::NamespacedId;
use terrain::{GeneratorFamily, GeneratorId, TerrainGenerator, WorldTerrain};
use voxel_grid::{ChannelDefinition, ChannelIndex, Grid, GridLayout};
use world_storage::{GeneratorRecipe, LevelData, LoadContext, RecipeError, WorldData};
use world_view::ViewChannels;

use crate::definition::GameContent;
use crate::package_data::{PackageData, check_materials};
use data_packs::{PackErrors, RawPack, load_packs};

/// Makes a game's terrain generators for a world seed. The layout is the world's, for finding the
/// channels a generator writes.
pub type GeneratorFactory =
    Arc<dyn Fn(u64, &GridLayout) -> Registry<Arc<dyn TerrainGenerator>> + Send + Sync>;

/// Makes a game's families of terrain generators, which a world's recipes are made with. The layout is the world's, for finding the channels a generator writes.
pub type FamilyFactory =
    Arc<dyn Fn(&GridLayout) -> Vec<(NamespacedId, Arc<dyn GeneratorFamily>)> + Send + Sync>;

/// For a game that has no families: its generators are all code.
pub fn no_families() -> FamilyFactory {
    Arc::new(|_layout| Vec::new())
}

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
    /// The channel holding each voxel's orientation, where the game's blocks can be turned.
    pub orientation_channel: Option<NamespacedId>,
    pub light_channel: Option<NamespacedId>,
    /// The properties of blocks that the game's plugins declare, besides the engine's.
    pub properties: Vec<PropertyDeclaration>,
    pub generators: GeneratorFactory,
    /// The families of terrain generators whose settings a world keeps as data (see [`WorldData::add_generator_recipe`]).
    pub families: FamilyFactory,
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

    /// The channel holding each voxel's orientation. Panics if the game has none.
    pub fn orientation_channel(&self) -> ChannelIndex {
        self.channel(
            self.spec
                .orientation_channel
                .as_ref()
                .expect("the package declares an orientation channel"),
        )
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

    fn families(&self) -> Vec<(NamespacedId, Arc<dyn GeneratorFamily>)> {
        (self.spec.families)(&self.layout())
    }

    /// Adds a terrain generator made from a recipe to a world of this package (see [`WorldData::add_generator_recipe`]), with the package's families.
    pub fn add_generator_recipe(
        &self,
        world: &mut WorldData,
        recipe: GeneratorRecipe,
    ) -> Result<GeneratorId, RecipeError> {
        world.add_generator_recipe(recipe, &self.families())
    }

    /// A level that starts with nothing edited and `default` for the generator of its regions.
    fn new_level(&self, seed: u64, default: &NamespacedId) -> LevelData {
        let generators = self.generators(seed);
        let default = generators
            .runtime_id(default)
            .expect("the package's generators include the default of every level");
        let terrain = WorldTerrain::new(generators, default, self.spec.assignment_cube_level)
            .expect("the default generator is available");
        LevelData::new(terrain, Grid::new(self.layout()))
    }

    /// A new world with these levels, each with its identifier and the generator that is the default of its regions, in this order: the first is the main level,
    /// where players start. Nothing is edited. (The world of [`GameContent::new_world`] is the one level of the spec, with the spec's default generator.)
    ///
    /// Panics with no levels, or with a generator the package does not have, which are mistakes in the game's declaration.
    pub fn new_world_of(&self, seed: u64, levels: &[(NamespacedId, NamespacedId)]) -> WorldData {
        let [(first_id, first_default), rest @ ..] = levels else {
            panic!("a world has at least one level");
        };
        let mut world = WorldData::new(seed, first_id.clone(), self.new_level(seed, first_default));
        for (level_id, default) in rest {
            world
                .add_level(level_id.clone(), self.new_level(seed, default))
                .expect("the levels of a package have distinct identifiers and the same channels");
        }
        world
    }

    /// Adds the blocks of data packs and applies their patches (see the `data_packs` crate). The game's own blocks
    /// keep their numbers and the packs' blocks follow them. Returns what was skipped on purpose, one line each.
    /// Nothing changes if there is an error: the packs are not loaded at all. Call it before a world is loaded,
    /// so that its blocks are not taken for blocks of a plugin that is missing.
    pub fn apply_packs(&mut self, packs: &[RawPack]) -> Result<Vec<String>, PackErrors> {
        let package = self.package_data();
        let base: Vec<(NamespacedId, BlockDefinition)> = package
            .blocks
            .iter()
            .filter_map(|(identifier, definition)| Some((identifier.clone(), definition.clone()?)))
            .collect();
        let loaded = load_packs(packs, &base, &package.properties)?;

        let mut blocks = Registry::new();
        let changed: std::collections::HashMap<&NamespacedId, &BlockDefinition> = loaded
            .blocks
            .iter()
            .map(|(identifier, definition)| (identifier, definition))
            .collect();
        for (_, entry) in self.blocks.iter() {
            let identifier = entry.identifier().clone();
            match (entry.content(), changed.get(&identifier)) {
                (Some(_), Some(definition)) => {
                    blocks
                        .register(identifier, (*definition).clone())
                        .expect("the blocks of a registry are distinct");
                }
                (Some(definition), None) => {
                    blocks
                        .register(identifier, definition.clone())
                        .expect("the blocks of a registry are distinct");
                }
                (None, _) => {
                    blocks.register_missing(identifier);
                }
            }
        }
        for (identifier, definition) in &loaded.blocks {
            if blocks.runtime_id(identifier).is_none() {
                blocks
                    .register(identifier.clone(), definition.clone())
                    .expect("the identifier was just found to be free");
            }
        }
        let candidate = Self {
            blocks,
            spec: self.spec.clone(),
        };
        // The renderer cannot draw more colours than it has: say so now, not when a window opens.
        let table = candidate
            .package_data()
            .block_table()
            .map_err(|error| pack_problem(error.to_string()))?;
        check_materials(&table).map_err(|error| pack_problem(error.to_string()))?;
        *self = candidate;
        Ok(loaded.warnings)
    }
}

fn pack_problem(message: String) -> PackErrors {
    PackErrors(vec![data_packs::PackError {
        file: "the packs together".to_owned(),
        path: String::new(),
        message,
    }])
}

impl GameContent for ContentPackage {
    fn view_channels(&self) -> ViewChannels {
        ViewChannels::new(
            self.block_channel(),
            self.spec.state_channel.as_ref().map(|id| self.channel(id)),
            self.spec.light_channel.as_ref().map(|id| self.channel(id)),
        )
        .with_orientations(
            self.spec
                .orientation_channel
                .as_ref()
                .map(|id| self.channel(id)),
        )
    }

    fn package_data(&self) -> PackageData {
        let mut properties = PropertySchema::new();
        for declaration in &self.spec.properties {
            properties
                .declare(declaration.clone())
                .unwrap_or_else(|error| {
                    panic!("the package's properties are inconsistent: {error}")
                });
        }
        PackageData {
            properties,
            block_registry: self.spec.block_registry_kind.clone(),
            blocks: self
                .blocks
                .iter()
                .map(|(_, entry)| (entry.identifier().clone(), entry.content().cloned()))
                .collect(),
            block_channel: self.spec.block_channel.clone(),
            state_channel: self.spec.state_channel.clone(),
            orientation_channel: self.spec.orientation_channel.clone(),
            light_channel: self.spec.light_channel.clone(),
            ambient_light: self.spec.ambient_light,
        }
    }

    fn new_world(&self, seed: u64) -> WorldData {
        self.new_world_of(
            seed,
            &[(self.spec.level.clone(), self.spec.default_generator.clone())],
        )
    }

    fn load_context(&mut self, seed: u64) -> LoadContext<'_> {
        let generators = self.generators(seed);
        let families = self.families();
        LoadContext {
            channels: self.spec.channels.clone(),
            registries: vec![(self.spec.block_registry_kind.clone(), &mut self.blocks)],
            generators,
            families,
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
                orientation_channel: None,
                light_channel: None,
                properties: Vec::new(),
                generators: Arc::new(|_seed, _layout| {
                    let mut generators: Registry<Arc<dyn TerrainGenerator>> = Registry::new();
                    generators
                        .register(id("test_game:empty_v1"), Arc::new(Empty))
                        .unwrap();
                    generators
                }),
                families: Arc::new(|_layout| {
                    let family: Arc<dyn GeneratorFamily> = Arc::new(NothingFamily);
                    vec![(id("test_game:nothing_family_v1"), family)]
                }),
                default_generator: id("test_game:empty_v1"),
                assignment_cube_level: 3,
                ambient_light: 90,
            },
        )
    }

    /// A family whose generators make nothing, however they are set.
    #[derive(Debug)]
    struct NothingFamily;

    impl GeneratorFamily for NothingFamily {
        fn build(
            &self,
            _parameters: &[u8],
        ) -> Result<Arc<dyn TerrainGenerator>, terrain::FamilyError> {
            Ok(Arc::new(Empty))
        }
    }

    #[test]
    fn a_recipe_made_with_the_packages_families_is_in_the_world_and_is_made_again_when_it_is_loaded()
     {
        let content = package();
        let mut world = content.new_world(42);
        let recipe = GeneratorRecipe {
            instance: id("test_game:the_moon"),
            family: id("test_game:nothing_family_v1"),
            parameters: vec![1, 2, 3],
        };
        let made = content
            .add_generator_recipe(&mut world, recipe.clone())
            .unwrap();
        assert_eq!(
            world
                .main()
                .terrain
                .generators()
                .runtime_id(&id("test_game:the_moon")),
            Some(made)
        );
        // A family the package does not have is refused.
        let mut stranger = recipe.clone();
        stranger.instance = id("test_game:the_sun");
        stranger.family = id("somebody_else:star_family_v1");
        assert_eq!(
            content.add_generator_recipe(&mut world, stranger),
            Err(RecipeError::UnknownFamily(id(
                "somebody_else:star_family_v1"
            )))
        );

        let bytes = world.save(&content.registries()).unwrap();
        let mut later = package();
        let parsed = ParsedWorld::parse(&bytes).unwrap();
        let (loaded, report) = parsed.load(later.load_context(42)).unwrap();
        assert!(report.missing_content.is_empty() && report.refused_recipes.is_empty());
        assert_eq!(loaded.generator_recipes(), &[recipe]);
        let generators = loaded.main().terrain.generators();
        let number = generators.runtime_id(&id("test_game:the_moon")).unwrap();
        assert!(!generators.entry(number).unwrap().is_missing());
        assert_eq!(loaded.save(&later.registries()).unwrap(), bytes);
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
    fn a_new_world_of_several_levels_has_them_in_that_order_each_with_the_default_generator_it_was_given()
     {
        let mut content = package();
        content.spec.generators = Arc::new(|_seed, _layout| {
            let mut generators: Registry<Arc<dyn TerrainGenerator>> = Registry::new();
            generators
                .register(id("test_game:empty_v1"), Arc::new(Empty))
                .unwrap();
            generators
                .register(id("test_game:other_v1"), Arc::new(Empty))
                .unwrap();
            generators
        });
        let world = content.new_world_of(
            7,
            &[
                (id("test_game:space"), id("test_game:other_v1")),
                (id("test_game:main"), id("test_game:empty_v1")),
            ],
        );
        let levels: Vec<(String, String)> = world
            .levels()
            .map(|(_, level_id, level)| {
                (
                    level_id.to_string(),
                    level.preferred_default_generator().to_string(),
                )
            })
            .collect();
        assert_eq!(
            levels,
            [
                (
                    "test_game:space".to_owned(),
                    "test_game:other_v1".to_owned()
                ),
                ("test_game:main".to_owned(), "test_game:empty_v1".to_owned()),
            ]
        );
        // The first is where players start: the main level.
        assert_eq!(
            world.level_id(voxel_space::LevelIndex::MAIN),
            Some(&id("test_game:space"))
        );
        assert_eq!(world.world_seed, 7);
        // And it saves and loads back the same, with both levels.
        let bytes = world.save(&content.registries()).unwrap();
        let parsed = ParsedWorld::parse(&bytes).unwrap();
        let (loaded, report) = parsed.load(content.load_context(7)).unwrap();
        assert!(report.missing_content.is_empty());
        assert_eq!(loaded.levels().count(), 2);
        assert_eq!(loaded.save(&content.registries()).unwrap(), bytes);
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

    // ---- data packs ----

    use data_packs::RawFile;

    fn pack(
        folder: &str,
        depends_on: &str,
        blocks: &[(&str, &str)],
        patches: &[(&str, &str)],
    ) -> RawPack {
        let files = |list: &[(&str, &str)]| {
            list.iter()
                .map(|(name, text)| RawFile {
                    name: (*name).to_owned(),
                    text: (*text).to_owned(),
                })
                .collect()
        };
        RawPack {
            folder: folder.to_owned(),
            manifest: format!(r#"{{ "id": "{folder}", "depends_on": [{depends_on}] }}"#),
            blocks: files(blocks),
            patches: files(patches),
        }
    }

    fn block_names(content: &ContentPackage) -> Vec<String> {
        content
            .package_data()
            .blocks
            .iter()
            .map(|(identifier, _)| identifier.to_string())
            .collect()
    }

    #[test]
    fn packs_add_blocks_after_the_games_own_and_leave_their_numbers_alone() {
        let mut content = package();
        let before = content.package_data();
        let warnings = content
            .apply_packs(&[pack(
                "my_pack",
                "",
                &[("marble", r#"{ "color": [0.9, 0.9, 1.0] }"#)],
                &[],
            )])
            .unwrap();
        assert!(warnings.is_empty());
        assert_eq!(
            block_names(&content),
            ["test_game:air", "test_game:rock", "my_pack:marble"]
        );
        let after = content.package_data();
        assert_eq!(after.blocks[..2], before.blocks[..]);
        assert_eq!(
            after.blocks[2].1,
            Some(BlockDefinition::cube([0.9, 0.9, 1.0]))
        );
        // The table has the new block too (it is what a server's plugins and its clients see).
        assert_eq!(content.block_table().block_count(), 3);
    }

    #[test]
    fn a_pack_can_patch_the_games_blocks_and_what_it_skipped_on_purpose_is_returned() {
        let mut content = package();
        let patch = r#"{ "target": "test_game:rock", "operations": [
            { "op": "set", "path": "color", "value": [1, 0, 0] } ] }"#;
        let skipped = r#"{ "target": "test_game:ghost", "optional": true, "operations": [
            { "op": "set", "path": "color", "value": [1, 0, 0] } ] }"#;
        let warnings = content
            .apply_packs(&[pack(
                "my_pack",
                r#""test_game""#,
                &[],
                &[("a_red", patch), ("b_skipped", skipped)],
            )])
            .unwrap();
        assert_eq!(
            warnings,
            [
                "my_pack/patches/b_skipped.json: skipped: there is no block test_game:ghost (is its pack or plugin loaded?)"
            ]
        );
        assert_eq!(
            content.package_data().blocks[1].1,
            Some(BlockDefinition::cube([1.0, 0.0, 0.0]))
        );
    }

    #[test]
    fn packs_that_cannot_be_loaded_change_nothing() {
        let mut content = package();
        let before = content.package_data();
        let errors = content
            .apply_packs(&[pack(
                "my_pack",
                "",
                &[("bad", r#"{ "colour": [0,0,0] }"#)],
                &[],
            )])
            .unwrap_err();
        assert!(
            errors.to_string().contains("my_pack/blocks/bad.json"),
            "{errors}"
        );
        assert_eq!(content.package_data(), before);
    }

    #[test]
    fn packs_whose_blocks_need_more_colours_than_can_be_drawn_are_refused_and_change_nothing() {
        let mut content = package();
        let before = content.package_data();
        // Two blocks of the game, one of the pack, and as many colours for the states of one as can be drawn.
        let most = renderer::MAXIMUM_GAME_MATERIALS;
        let colors = vec!["[0, 0, 0]"; most].join(", ");
        let heavy = format!(r#"{{ "color": [0,0,0], "state_colors": [{colors}] }}"#);
        let errors = content
            .apply_packs(&[pack("my_pack", "", &[("heavy", &heavy)], &[])])
            .unwrap_err();
        assert_eq!(
            errors.to_string(),
            format!(
                "the packs together: the blocks need {} colours and at most {most} can be drawn",
                most + 3
            )
        );
        assert_eq!(content.package_data(), before);
    }

    #[test]
    fn a_block_of_a_missing_plugin_keeps_its_place_when_packs_are_added() {
        let mut content = package();
        content.blocks.register_missing(id("gone_plugin:thing"));
        content
            .apply_packs(&[pack(
                "my_pack",
                "",
                &[("marble", r#"{ "color": [0,0,0] }"#)],
                &[],
            )])
            .unwrap();
        assert_eq!(
            block_names(&content),
            [
                "test_game:air",
                "test_game:rock",
                "gone_plugin:thing",
                "my_pack:marble"
            ]
        );
        assert_eq!(content.package_data().blocks[2].1, None);
    }
}

//! A small pretend game for tests of code that loads, saves, serves or replays worlds. Its plugins
//! can be switched on and off, so tests can load a world with different content than wrote it.
//! Enabled by the `test-support` feature.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use content_registry::{IdentifierRegistry, Registry};
use namespaced_id::NamespacedId;
use terrain::{
    FamilyError, FieldTerrainGenerator, FractalNoise, GeneratorFamily, GeneratorId, Heightfield,
    TerrainGenerator, TerrainMaterials, WorldTerrain,
};
use voxel_grid::{Brick, ChannelDefinition, ChannelValueKind, Grid, GridLayout};
use voxel_space::{BrickBox, BrickCoordinate, VoxelCoordinate};

use voxel_space::{GridPose, Orientation, WorldPosition};

use crate::{
    GeneratorRecipe, GridTerrain, LevelData, LevelIndex, LoadContext, LoadError, LoadReport,
    MovingGrid, ParsedWorld, PilotLogout, RecipeError, Section, ShipFiles, ShipFormat, WorldData,
    WorldSave,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plugin {
    /// Blocks registered before everything else, so enabling it shifts every other block's runtime id.
    Newcomer,
    BaseBlocks,
    FancyBlocks,
    /// Adds a content channel with its own registry, and a plain number channel.
    Magic,
    FancyTerrain,
}

use Plugin::*;

pub const ALL_PLUGINS: [Plugin; 5] = [Newcomer, BaseBlocks, FancyBlocks, Magic, FancyTerrain];

pub fn id(text: &str) -> NamespacedId {
    text.parse().unwrap()
}

/// The name of the fixture world's one level.
pub fn main_level_id() -> NamespacedId {
    id("test_content:main")
}

pub fn block_kind() -> NamespacedId {
    id("voxel_content:block")
}

pub fn spell_kind() -> NamespacedId {
    id("magic:spell")
}

/// The family of the fancy terrain plugin: a flat world, whose height in voxels is its parameters (the bits of an `f64`, little-endian).
pub fn flat_family_id() -> NamespacedId {
    id("fancy_terrain:flat_family_v1")
}

#[derive(Debug)]
struct FlatFamily {
    material: voxel_grid::ChannelIndex,
    materials: TerrainMaterials,
}

impl GeneratorFamily for FlatFamily {
    fn build(&self, parameters: &[u8]) -> Result<Arc<dyn TerrainGenerator>, FamilyError> {
        let bytes: [u8; 8] = parameters.try_into().map_err(|_| {
            FamilyError::new(format!("a flat world is 8 bytes, not {}", parameters.len()))
        })?;
        let height = f64::from_bits(u64::from_le_bytes(bytes));
        if !height.is_finite() {
            return Err(FamilyError::new("the height of a flat world is a number"));
        }
        Ok(Arc::new(FieldTerrainGenerator::new(
            Arc::new(Heightfield::new(height, FractalNoise::none())),
            self.material,
            self.materials,
        )))
    }
}

pub fn brick(x: i64, y: i64, z: i64) -> BrickCoordinate {
    BrickCoordinate::new(x, y, z)
}

#[derive(Debug)]
pub struct Game {
    pub plugins: Vec<Plugin>,
    pub blocks: Registry<()>,
    pub spells: Option<Registry<()>>,
}

impl Game {
    pub fn new(plugins: &[Plugin]) -> Self {
        let has = |plugin| plugins.contains(&plugin);
        let mut blocks = Registry::new();
        let mut register = |names: &[&str]| {
            for name in names {
                blocks.register(id(name), ()).unwrap();
            }
        };
        if has(Newcomer) {
            register(&["newcomer:obsidian", "newcomer:slime"]);
        }
        if has(BaseBlocks) {
            register(&[
                "base_blocks:air",
                "base_blocks:stone",
                "base_blocks:dirt",
                "base_blocks:grass",
            ]);
        }
        if has(FancyBlocks) {
            register(&["fancy_blocks:marble", "fancy_blocks:glass"]);
        }
        let spells = has(Magic).then(|| {
            let mut spells = Registry::new();
            for name in ["magic:no_spell", "magic:fireball", "magic:frost"] {
                spells.register(id(name), ()).unwrap();
            }
            spells
        });
        Self {
            plugins: plugins.to_vec(),
            blocks,
            spells,
        }
    }

    pub fn has(&self, plugin: Plugin) -> bool {
        self.plugins.contains(&plugin)
    }

    pub fn raw_block(&self, name: &str) -> u32 {
        self.blocks
            .raw_id_of(&id(name))
            .unwrap_or_else(|| panic!("{name} is not registered"))
    }

    pub fn channel_definitions(&self) -> Vec<ChannelDefinition> {
        let mut channels = vec![
            ChannelDefinition::content(
                id("core:block"),
                self.raw_block("base_blocks:air"),
                block_kind(),
            ),
            ChannelDefinition::number(id("core:temperature"), 20),
        ];
        if let Some(spells) = &self.spells {
            channels.push(ChannelDefinition::number(id("magic:mana"), 0));
            channels.push(ChannelDefinition::content(
                id("magic:spell"),
                spells.raw_id_of(&id("magic:no_spell")).unwrap(),
                spell_kind(),
            ));
        }
        channels
    }

    /// The families of terrain generators this game has: the fancy terrain plugin's flat world, if the plugin is loaded.
    pub fn families(&self) -> Vec<(NamespacedId, Arc<dyn GeneratorFamily>)> {
        if !self.has(FancyTerrain) {
            return Vec::new();
        }
        let layout = GridLayout::new(self.channel_definitions()).unwrap();
        let family: Arc<dyn GeneratorFamily> = Arc::new(FlatFamily {
            material: layout.channel_index(&id("core:block")).unwrap(),
            materials: TerrainMaterials {
                air: self.raw_block("base_blocks:air"),
                stone: self.raw_block("base_blocks:stone"),
                dirt: self.raw_block("base_blocks:dirt"),
                grass: self.raw_block("base_blocks:grass"),
            },
        });
        vec![(flat_family_id(), family)]
    }

    /// A recipe for a flat world `height` voxels high, called `instance`.
    pub fn flat_recipe(&self, instance: &str, height: f64) -> GeneratorRecipe {
        GeneratorRecipe {
            instance: id(instance),
            family: flat_family_id(),
            parameters: height.to_bits().to_le_bytes().to_vec(),
        }
    }

    /// Adds the recipe to the world, with this game's families.
    pub fn add_recipe(
        &self,
        world: &mut WorldData,
        recipe: GeneratorRecipe,
    ) -> Result<GeneratorId, RecipeError> {
        world.add_generator_recipe(recipe, &self.families())
    }

    pub fn generator_registry(&self) -> Registry<Arc<dyn TerrainGenerator>> {
        let layout = GridLayout::new(self.channel_definitions()).unwrap();
        let material = layout.channel_index(&id("core:block")).unwrap();
        let materials = TerrainMaterials {
            air: self.raw_block("base_blocks:air"),
            stone: self.raw_block("base_blocks:stone"),
            dirt: self.raw_block("base_blocks:dirt"),
            grass: self.raw_block("base_blocks:grass"),
        };
        let flat = |height: f64| -> Arc<dyn TerrainGenerator> {
            Arc::new(FieldTerrainGenerator::new(
                Arc::new(Heightfield::new(height, FractalNoise::none())),
                material,
                materials,
            ))
        };
        let mut generators = Registry::new();
        generators
            .register(id("base_terrain:flat_v1"), flat(10.0))
            .unwrap();
        if self.has(FancyTerrain) {
            generators
                .register(id("fancy_terrain:cliffs_v1"), flat(40.0))
                .unwrap();
        }
        generators
    }

    pub fn new_world(&self) -> WorldData {
        let generators = self.generator_registry();
        let default = generators.runtime_id(&id("base_terrain:flat_v1")).unwrap();
        let terrain = WorldTerrain::new(generators, default, 3).unwrap();
        let layout = Arc::new(GridLayout::new(self.channel_definitions()).unwrap());
        WorldData::new(
            0xFEED_FACE,
            main_level_id(),
            LevelData::new(terrain, Grid::new(layout)),
        )
    }

    /// What loading a world file or a server's description into this game needs.
    pub fn load_context(&mut self) -> LoadContext<'_> {
        let channels = self.channel_definitions();
        let generators = self.generator_registry();
        let families = self.families();
        let mut registries: Vec<(NamespacedId, &mut dyn IdentifierRegistry)> =
            vec![(block_kind(), &mut self.blocks)];
        if let Some(spells) = self.spells.as_mut() {
            registries.push((spell_kind(), spells));
        }
        LoadContext {
            channels,
            registries,
            generators,
            families,
            fallback_default_generator: id("base_terrain:flat_v1"),
            assignment_cube_level: 3,
            legacy_level: main_level_id(),
        }
    }

    /// The registries a save or description needs, by kind.
    pub fn registries(&self) -> Vec<(NamespacedId, &dyn IdentifierRegistry)> {
        let mut registries: Vec<(NamespacedId, &dyn IdentifierRegistry)> =
            vec![(block_kind(), &self.blocks)];
        if let Some(spells) = &self.spells {
            registries.push((spell_kind(), spells));
        }
        registries
    }

    pub fn load(&mut self, bytes: &[u8]) -> Result<(WorldData, LoadReport), LoadError> {
        ParsedWorld::parse(bytes)?.load(self.load_context())
    }

    pub fn save(&self, world: &WorldData) -> Vec<u8> {
        world.save(&self.registries()).unwrap()
    }

    /// The world file and the files of the ships that are in memory.
    pub fn save_with_ships(&self, world: &mut WorldData) -> WorldSave {
        world.save_with_ships(&self.registries()).unwrap()
    }

    /// A ship of two bricks of this game's blocks (a deck of stone, and a marble block a brick away that is warm and, with the magic plugin,
    /// holds a spell), in flight and held, that vanishes with its pilot, with a section of a plugin nobody has. Its pose is `shift` voxels
    /// along x of where the first is, and so are its voxels, so that ships made with different shifts are different files.
    pub fn sample_ship(&self, world: &WorldData, shift: i64) -> MovingGrid {
        let layout = Arc::clone(world.main().overrides.layout());
        let mut voxels = Grid::new(Arc::clone(&layout));
        let block = layout.channel_index(&id("core:block")).unwrap();
        let temperature = layout.channel_index(&id("core:temperature")).unwrap();
        for x in 0..4 {
            for z in 0..3 {
                voxels.set(
                    block,
                    VoxelCoordinate::new(x + shift, 1, z),
                    self.raw_block("base_blocks:stone"),
                );
            }
        }
        voxels.set(
            block,
            VoxelCoordinate::new(40 + shift, 33, 2),
            self.raw_block("fancy_blocks:marble"),
        );
        voxels.set(temperature, VoxelCoordinate::new(40 + shift, 33, 2), 77);
        if let Some(spells) = &self.spells {
            let spell = layout.channel_index(&id("magic:spell")).unwrap();
            voxels.set(
                spell,
                VoxelCoordinate::new(1 + shift, 1, 1),
                spells.raw_id_of(&id("magic:fireball")).unwrap(),
            );
        }
        MovingGrid {
            level: LevelIndex::MAIN,
            pose: GridPose::new(
                WorldPosition::new(brick(-3 + shift, 2, 7), [1.5, 20.25, 31.0]),
                Orientation::from_yaw_pitch(0.7, -0.2),
            ),
            linear_velocity: [1.0, -2.5, 0.125],
            angular_velocity: [0.0, 0.5, -0.25],
            mass: 12.0,
            centre_of_mass: [1.0, 1.0, 1.0],
            held: true,
            frozen: false,
            on_pilot_logout: PilotLogout::Vanish,
            voxels,
            terrain: None,
            rail: None,
            sections: vec![Section {
                identifier: id("unheard_of:ship_notes"),
                version: 4,
                payload: b"kept as they are".to_vec(),
            }],
        }
    }

    /// A planet: the ship of [`Game::sample_ship`] (its two bricks are the edits) with terrain, the game's `fancy_terrain:cliffs_v1` (`base_terrain:flat_v1` in a game
    /// without that plugin) over 41 x 41 x 41 bricks about its origin, weighing as much as a planet of 63,710 metres does.
    pub fn sample_planet(&self, world: &WorldData, shift: i64) -> MovingGrid {
        let mut ship = self.sample_ship(world, shift);
        let generator = if self.has(FancyTerrain) {
            "fancy_terrain:cliffs_v1"
        } else {
            "base_terrain:flat_v1"
        };
        let terrain = GridTerrain {
            generator: world
                .main()
                .terrain
                .generators()
                .runtime_id(&id(generator))
                .unwrap(),
            extent: BrickBox::around(BrickCoordinate::ORIGIN, 20),
            mass: 6.07e20,
        };
        ship.mass = terrain.mass;
        ship.centre_of_mass = [0.0; 3];
        ship.terrain = Some(terrain);
        ship
    }

    /// What it takes to make the files of the ships of this game's worlds and to read them.
    pub fn ship_format(&self, world: &WorldData) -> ShipFormat {
        world.ship_format(&self.registries()).unwrap()
    }

    /// Brings every ship that is in a file into memory, as a server that wants them all does.
    pub fn load_all_ships(&self, world: &mut WorldData, files: &dyn ShipFiles) {
        let format = self.ship_format(world);
        let parked: Vec<_> = world.grids().parked().map(|(id, _)| id).collect();
        for id in parked {
            world.load_ship(id, files, &format).unwrap();
        }
    }

    pub fn set(&self, world: &mut WorldData, at: [i64; 3], channel: &str, value: u32) {
        self.set_in_level(world, LevelIndex::MAIN, at, channel, value);
    }

    pub fn set_in_level(
        &self,
        world: &mut WorldData,
        level: LevelIndex,
        at: [i64; 3],
        channel: &str,
        value: u32,
    ) {
        let level = world.level_mut(level).expect("the level exists");
        let index = level
            .overrides
            .layout()
            .channel_index(&id(channel))
            .unwrap();
        level
            .overrides
            .set(index, VoxelCoordinate::new(at[0], at[1], at[2]), value);
    }

    pub fn set_block(&self, world: &mut WorldData, at: [i64; 3], name: &str) {
        self.set(world, at, "core:block", self.raw_block(name));
    }

    /// What a raw value of a channel means, as text: its identifier for content, its decimal
    /// number otherwise. Raw ids this game cannot name are shown as `?` and the number.
    pub fn value_name(&self, definition: &ChannelDefinition, raw: u32) -> String {
        match &definition.value_kind {
            ChannelValueKind::Number => raw.to_string(),
            ChannelValueKind::Content { registry } if *registry == block_kind() => self
                .blocks
                .identifier_of_raw(raw)
                .map_or_else(|| format!("?{raw}"), |i| i.to_string()),
            ChannelValueKind::Content { registry } if *registry == spell_kind() => self
                .spells
                .as_ref()
                .and_then(|spells| spells.identifier_of_raw(raw))
                .map_or_else(|| format!("?{raw}"), |i| i.to_string()),
            ChannelValueKind::Content { .. } => format!("raw{raw}"),
        }
    }

    /// A brick as meaning, whatever this game's numbering: for each channel by identifier, its
    /// voxels in storage order as runs of equal values named by [`Game::value_name`].
    pub fn brick_meaning(&self, layout: &GridLayout, brick: &Brick) -> BrickMeaning {
        layout
            .channels()
            .iter()
            .map(|definition| {
                let index = layout.channel_index(&definition.identifier).unwrap();
                let mut names: HashMap<u32, String> = HashMap::new();
                let mut runs: Vec<(String, u32)> = Vec::new();
                for raw in brick.channel(index).iter() {
                    let name = names
                        .entry(raw)
                        .or_insert_with(|| self.value_name(definition, raw));
                    match runs.last_mut() {
                        Some((last, length)) if last == name => *length += 1,
                        _ => runs.push((name.clone(), 1)),
                    }
                }
                (definition.identifier.to_string(), runs)
            })
            .collect()
    }

    /// The world as meaning, with every runtime id replaced by its identifier.
    pub fn describe(&self, world: &WorldData) -> Description {
        Description {
            seed: world.world_seed,
            levels: world
                .levels()
                .map(|(_, id, level)| self.describe_level(id, level))
                .collect(),
            opaque: world
                .opaque_sections()
                .iter()
                .map(|s| (s.identifier.to_string(), s.version, s.payload.clone()))
                .collect(),
        }
    }

    fn describe_level(&self, id: &NamespacedId, level: &LevelData) -> LevelDescription {
        let layout = level.overrides.layout();
        let mut bricks = BTreeMap::new();
        for coordinate in level.overrides.sorted_brick_coordinates() {
            let stored = level.overrides.brick(coordinate).unwrap();
            for (channel, definition) in layout.channels().iter().enumerate() {
                let index = layout.channel_index(&definition.identifier).unwrap();
                let name_of = |raw: u32| self.value_name(definition, raw);
                let default = name_of(definition.default_value);
                // Names and raw ids correspond one to one, so compare raw ids and name only the changes.
                let changed: Vec<(usize, String)> = stored
                    .channel(index)
                    .iter()
                    .enumerate()
                    .filter(|(_, raw)| *raw != definition.default_value)
                    .map(|(voxel, raw)| (voxel, name_of(raw)))
                    .collect();
                bricks.insert(
                    (coordinate, definition.identifier.to_string(), channel),
                    (default, changed),
                );
            }
        }
        let assignments = level
            .terrain
            .assignments()
            .leaves()
            .into_iter()
            .map(|(cube, generator)| {
                let origin = cube.origin();
                let name = generator.map(|g| {
                    level
                        .terrain
                        .generators()
                        .entry(g)
                        .unwrap()
                        .identifier()
                        .to_string()
                });
                ((origin.x(), origin.y(), origin.z()), cube.level(), name)
            })
            .collect();
        LevelDescription {
            id: id.to_string(),
            preferred_default: level.preferred_default_generator().to_string(),
            assignments,
            bricks: bricks.into_iter().collect(),
        }
    }

    /// A new level that uses this game's channels and terrain generators and starts with `default`
    /// as its default generator, for adding to a world of this game.
    pub fn new_level(&self, default: &str) -> LevelData {
        let generators = self.generator_registry();
        let default = generators.runtime_id(&id(default)).unwrap();
        let terrain = WorldTerrain::new(generators, default, 3).unwrap();
        let layout = Arc::new(GridLayout::new(self.channel_definitions()).unwrap());
        LevelData::new(terrain, Grid::new(layout))
    }
}

/// A brick as meaning: for each channel by identifier, runs of equal values and their lengths.
pub type BrickMeaning = BTreeMap<String, Vec<(String, u32)>>;
pub type Assignment = ((i64, i64, i64), u8, Option<String>);
/// A channel's default value (as a name) and the voxels that differ from it.
pub type ChannelChanges = (String, Vec<(usize, String)>);
/// A plugin section: identifier, version, payload.
pub type OpaqueRecord = (String, u32, Vec<u8>);

/// One level as meaning.
#[derive(Debug, PartialEq)]
pub struct LevelDescription {
    pub id: String,
    pub preferred_default: String,
    pub assignments: Vec<Assignment>,
    /// (brick, channel name, channel position) -> changes.
    pub bricks: BTreeMap<(BrickCoordinate, String, usize), ChannelChanges>,
}

#[derive(Debug, PartialEq)]
pub struct Description {
    pub seed: u64,
    pub levels: Vec<LevelDescription>,
    pub opaque: Vec<OpaqueRecord>,
}

/// Like `LevelDescription` but without channel positions, which legitimately differ between plugin
/// sets.
#[derive(Debug, PartialEq)]
pub struct ComparableLevel {
    pub id: String,
    pub preferred_default: String,
    pub assignments: Vec<Assignment>,
    pub bricks: BTreeMap<(BrickCoordinate, String), ChannelChanges>,
}

/// Like `Description` but without channel positions.
#[derive(Debug, PartialEq)]
pub struct Comparable {
    pub seed: u64,
    pub levels: Vec<ComparableLevel>,
    pub opaque: Vec<OpaqueRecord>,
}

pub fn comparable(description: Description) -> Comparable {
    Comparable {
        seed: description.seed,
        levels: description
            .levels
            .into_iter()
            .map(|level| ComparableLevel {
                id: level.id,
                preferred_default: level.preferred_default,
                assignments: level.assignments,
                bricks: level
                    .bricks
                    .into_iter()
                    .map(|((brick, name, _), value)| ((brick, name), value))
                    .collect(),
            })
            .collect(),
        opaque: description.opaque,
    }
}

/// A world touching both plugins' content, with edits in several bricks and two terrain generators.
pub fn sample_world(game: &Game) -> WorldData {
    let mut world = game.new_world();
    game.set_block(&mut world, [1, 2, 3], "base_blocks:stone");
    game.set_block(&mut world, [5, 5, 5], "base_blocks:dirt");
    game.set(&mut world, [1, 2, 3], "core:temperature", 99);
    if game.has(Newcomer) {
        game.set_block(&mut world, [-5, -5, -5], "newcomer:obsidian");
    }
    if game.has(FancyBlocks) {
        game.set_block(&mut world, [40, 2, 3], "fancy_blocks:marble");
        game.set_block(&mut world, [100, 70, -33], "fancy_blocks:glass");
    }
    if let Some(spells) = &game.spells {
        game.set(&mut world, [40, 2, 3], "magic:mana", 7);
        game.set(
            &mut world,
            [2, 2, 2],
            "magic:spell",
            spells.raw_id_of(&id("magic:fireball")).unwrap(),
        );
    }
    world.main_mut().terrain.assign_unassigned(brick(0, 0, 0));
    if game.has(FancyTerrain) {
        let cliffs = world
            .main()
            .terrain
            .generators()
            .runtime_id(&id("fancy_terrain:cliffs_v1"))
            .unwrap();
        world.main_mut().set_default_generator(cliffs).unwrap();
        world.main_mut().terrain.assign_unassigned(brick(100, 0, 0));
    }
    world.set_opaque_section(Section {
        identifier: id("magic:spellbook"),
        version: 3,
        payload: vec![1, 2, 3, 4],
    });
    world.set_opaque_section(Section {
        identifier: id("unheard_of:data"),
        version: 1,
        payload: b"mystery".to_vec(),
    });
    world
}

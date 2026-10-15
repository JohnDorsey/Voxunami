//! A small pretend game for tests of code that loads, saves, serves or replays worlds. Its plugins
//! can be switched on and off, so tests can load a world with different content than wrote it.
//! Enabled by the `test-support` feature.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use content_registry::{IdentifierRegistry, Registry};
use namespaced_id::NamespacedId;
use terrain::{
    FieldTerrainGenerator, FractalNoise, Heightfield, TerrainGenerator, TerrainMaterials,
    WorldTerrain,
};
use voxel_grid::{Brick, ChannelDefinition, ChannelValueKind, Grid, GridLayout};
use voxel_space::{BrickCoordinate, VoxelCoordinate};

use crate::{LoadContext, LoadError, LoadReport, ParsedWorld, Section, WorldData};

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

pub fn block_kind() -> NamespacedId {
    id("voxel_content:block")
}

pub fn spell_kind() -> NamespacedId {
    id("magic:spell")
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
        WorldData::new(0xFEED_FACE, terrain, Grid::new(layout))
    }

    /// What loading a world file or a server's description into this game needs.
    pub fn load_context(&mut self) -> LoadContext<'_> {
        let channels = self.channel_definitions();
        let generators = self.generator_registry();
        let mut registries: Vec<(NamespacedId, &mut dyn IdentifierRegistry)> =
            vec![(block_kind(), &mut self.blocks)];
        if let Some(spells) = self.spells.as_mut() {
            registries.push((spell_kind(), spells));
        }
        LoadContext {
            channels,
            registries,
            generators,
            fallback_default_generator: id("base_terrain:flat_v1"),
            assignment_cube_level: 3,
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

    pub fn set(&self, world: &mut WorldData, at: [i64; 3], channel: &str, value: u32) {
        let index = world
            .overrides
            .layout()
            .channel_index(&id(channel))
            .unwrap();
        world
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
        let layout = world.overrides.layout();
        let mut bricks = BTreeMap::new();
        for coordinate in world.overrides.sorted_brick_coordinates() {
            let stored = world.overrides.brick(coordinate).unwrap();
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
        let assignments = world
            .terrain
            .assignments()
            .leaves()
            .into_iter()
            .map(|(cube, generator)| {
                let origin = cube.origin();
                let name = generator.map(|g| {
                    world
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
        Description {
            seed: world.world_seed,
            preferred_default: world.preferred_default_generator().to_string(),
            assignments,
            bricks: bricks.into_iter().collect(),
            opaque: world
                .opaque_sections()
                .iter()
                .map(|s| (s.identifier.to_string(), s.version, s.payload.clone()))
                .collect(),
        }
    }
}

/// A brick as meaning: for each channel by identifier, runs of equal values and their lengths.
pub type BrickMeaning = BTreeMap<String, Vec<(String, u32)>>;
pub type Assignment = ((i64, i64, i64), u8, Option<String>);
/// A channel's default value (as a name) and the voxels that differ from it.
pub type ChannelChanges = (String, Vec<(usize, String)>);
/// A plugin section: identifier, version, payload.
pub type OpaqueRecord = (String, u32, Vec<u8>);

#[derive(Debug, PartialEq)]
pub struct Description {
    pub seed: u64,
    pub preferred_default: String,
    pub assignments: Vec<Assignment>,
    /// (brick, channel name, channel position) -> changes.
    pub bricks: BTreeMap<(BrickCoordinate, String, usize), ChannelChanges>,
    pub opaque: Vec<OpaqueRecord>,
}

/// Like `Description` but without channel positions, which legitimately differ between plugin sets.
#[derive(Debug, PartialEq)]
pub struct Comparable {
    pub seed: u64,
    pub preferred_default: String,
    pub assignments: Vec<Assignment>,
    pub bricks: BTreeMap<(BrickCoordinate, String), ChannelChanges>,
    pub opaque: Vec<OpaqueRecord>,
}

pub fn comparable(description: Description) -> Comparable {
    Comparable {
        seed: description.seed,
        preferred_default: description.preferred_default,
        assignments: description.assignments,
        bricks: description
            .bricks
            .into_iter()
            .map(|((brick, name, _), value)| ((brick, name), value))
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
    world.terrain.assign_unassigned(brick(0, 0, 0));
    if game.has(FancyTerrain) {
        let cliffs = world
            .terrain
            .generators()
            .runtime_id(&id("fancy_terrain:cliffs_v1"))
            .unwrap();
        world.set_default_generator(cliffs).unwrap();
        world.terrain.assign_unassigned(brick(100, 0, 0));
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

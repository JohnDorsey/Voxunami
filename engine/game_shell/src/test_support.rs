//! A minimal game for the shell's own tests, so they do not depend on any real game.

use block_models::{BlockDefinition, PropertySchema};
use content_registry::IdentifierRegistry;
use namespaced_id::NamespacedId;
use plugin_host::Plugin as ServerPlugin;
use world_storage::test_support::{Game, Plugin, id};
use world_storage::{LoadContext, WorldData};
use world_view::ViewChannels;

use crate::definition::{CameraStart, GameContent, GameDefinition};
use crate::package_data::PackageData;

/// Flat ground of stone, dirt and grass, from the fixture game of `world_storage`.
#[derive(Debug)]
pub struct ExampleContent {
    pub game: Game,
}

impl GameContent for ExampleContent {
    fn view_channels(&self) -> ViewChannels {
        ViewChannels {
            blocks: self
                .game
                .new_world()
                .main()
                .overrides
                .layout()
                .channel_index(&id("core:block"))
                .expect("the fixture has a block channel"),
            states: None,
            orientations: None,
            light: None,
            bulk_light: None,
        }
    }

    fn package_data(&self) -> PackageData {
        let registries = self.game.registries();
        let (block_registry, blocks) = &registries[0];
        // Air first, then whole opaque cubes.
        let blocks = (0..blocks.entry_count() as u32)
            .map(|raw| {
                let identifier = blocks.identifier_of_raw(raw).expect("the id is in range");
                let definition = if raw == 0 {
                    BlockDefinition::air()
                } else {
                    BlockDefinition::cube([0.5; 3])
                };
                (identifier.clone(), Some(definition))
            })
            .collect();
        PackageData {
            properties: PropertySchema::new(),
            block_registry: block_registry.clone(),
            blocks,
            block_channel: id("core:block"),
            state_channel: None,
            orientation_channel: None,
            light_channel: None,
            ambient_light: 0,
        }
    }

    fn new_world(&self, _seed: u64) -> WorldData {
        self.game.new_world()
    }

    fn load_context(&mut self, _seed: u64) -> LoadContext<'_> {
        self.game.load_context()
    }

    fn registries(&self) -> Vec<(NamespacedId, &dyn IdentifierRegistry)> {
        self.game.registries()
    }
}

/// A game that only has the defaults.
#[derive(Debug)]
pub struct ExampleGame;

impl GameDefinition for ExampleGame {
    type Content = ExampleContent;

    fn id() -> NamespacedId {
        id("example:game")
    }

    fn title() -> &'static str {
        "Example"
    }

    fn command() -> &'static str {
        "example"
    }

    fn summary() -> &'static str {
        "a game for tests"
    }

    fn controls() -> &'static str {
        "Nothing else."
    }

    fn default_seed() -> u64 {
        2024
    }

    fn default_camera() -> CameraStart {
        CameraStart {
            position: [16.0, 110.0, 16.0],
            yaw_degrees: 0.0,
            pitch_degrees: -15.0,
        }
    }

    fn reach() -> f32 {
        10.0
    }

    fn new_content() -> ExampleContent {
        ExampleContent {
            game: Game::new(&[Plugin::BaseBlocks]),
        }
    }

    fn plugins(_content: &ExampleContent) -> Vec<Box<dyn ServerPlugin>> {
        Vec::new()
    }
}

/// A game whose levels pull differently from the default: a moon of 10^20 kilograms pulls, and a level in space has no pull of its own.
#[derive(Debug)]
pub struct ExampleSpaceGame;

impl GameDefinition for ExampleSpaceGame {
    type Content = ExampleContent;

    fn id() -> NamespacedId {
        id("example:space_game")
    }

    fn title() -> &'static str {
        ExampleGame::title()
    }

    fn command() -> &'static str {
        ExampleGame::command()
    }

    fn summary() -> &'static str {
        ExampleGame::summary()
    }

    fn controls() -> &'static str {
        ExampleGame::controls()
    }

    fn default_seed() -> u64 {
        ExampleGame::default_seed()
    }

    fn default_camera() -> CameraStart {
        ExampleGame::default_camera()
    }

    fn reach() -> f32 {
        ExampleGame::reach()
    }

    fn new_content() -> ExampleContent {
        ExampleGame::new_content()
    }

    fn plugins(content: &ExampleContent) -> Vec<Box<dyn ServerPlugin>> {
        ExampleGame::plugins(content)
    }

    fn gravity() -> voxel_space::GravitySettings {
        voxel_space::GravitySettings {
            base: [0.0, -9.0, 0.0],
            pull_mass_threshold: Some(1.0e20),
            ..voxel_space::GravitySettings::default()
        }
    }

    fn level_gravity() -> Vec<(NamespacedId, [f32; 3])> {
        vec![(id("example:space"), [0.0; 3])]
    }
}

/// A game with a sun: a day of 1,200 seconds that begins at 6 in the morning, over flat ground (every level of it).
#[derive(Debug)]
pub struct ExampleSunGame;

impl ExampleSunGame {
    pub fn the_sun() -> crate::sun::Sun {
        crate::sun::Sun {
            day_seconds: 1_200,
            start_hour: 6.0,
            noon: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            light: 255,
            sky_light: 100,
            flat_ground: true,
            sky: crate::sun::Sky {
                color: [0.4, 0.6, 0.9],
                thickness: 30_000.0,
            },
        }
    }
}

impl GameDefinition for ExampleSunGame {
    type Content = ExampleContent;

    fn id() -> NamespacedId {
        id("example:sun_game")
    }

    fn title() -> &'static str {
        ExampleGame::title()
    }

    fn command() -> &'static str {
        ExampleGame::command()
    }

    fn summary() -> &'static str {
        ExampleGame::summary()
    }

    fn controls() -> &'static str {
        ExampleGame::controls()
    }

    fn default_seed() -> u64 {
        ExampleGame::default_seed()
    }

    fn default_camera() -> CameraStart {
        ExampleGame::default_camera()
    }

    fn reach() -> f32 {
        ExampleGame::reach()
    }

    fn new_content() -> ExampleContent {
        ExampleGame::new_content()
    }

    fn plugins(content: &ExampleContent) -> Vec<Box<dyn ServerPlugin>> {
        ExampleGame::plugins(content)
    }

    fn sun(level: &NamespacedId) -> Option<crate::sun::Sun> {
        // (No sun in the level called "example:dark".)
        (level.as_str() != "example:dark").then(Self::the_sun)
    }
}

/// A game that says how far its terrain is drawn, and nothing about levels: so every level is drawn so (`GameDefinition::level_detail`).
#[derive(Debug)]
pub struct ExampleDetailGame;

impl GameDefinition for ExampleDetailGame {
    type Content = ExampleContent;

    fn id() -> NamespacedId {
        id("example:detail_game")
    }

    fn title() -> &'static str {
        ExampleGame::title()
    }

    fn command() -> &'static str {
        ExampleGame::command()
    }

    fn summary() -> &'static str {
        ExampleGame::summary()
    }

    fn controls() -> &'static str {
        ExampleGame::controls()
    }

    fn default_seed() -> u64 {
        ExampleGame::default_seed()
    }

    fn default_camera() -> CameraStart {
        ExampleGame::default_camera()
    }

    fn reach() -> f32 {
        ExampleGame::reach()
    }

    fn new_content() -> ExampleContent {
        ExampleGame::new_content()
    }

    fn plugins(content: &ExampleContent) -> Vec<Box<dyn ServerPlugin>> {
        ExampleGame::plugins(content)
    }

    fn coarse_detail() -> Option<crate::definition::CoarseDetail> {
        Some(crate::definition::CoarseDetail {
            maximum_distance: 700.0,
            unlit: 0,
            cell_pixels: 3.0,
            bytes_per_second: 0,
        })
    }
}

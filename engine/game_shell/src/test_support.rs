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

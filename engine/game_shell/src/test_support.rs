//! A minimal game for the shell's own tests, so they do not depend on any real game.

use block_models::BlockTable;
use content_registry::IdentifierRegistry;
use namespaced_id::NamespacedId;
use server::GameLogic;
use server::test_support::ScriptedLogic;
use world_storage::test_support::{Game, Plugin, id};
use world_storage::{LoadContext, WorldData};
use world_view::ViewChannels;

use crate::definition::{CameraStart, GameContent, GameDefinition};

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
        }
    }

    fn block_table(&self) -> BlockTable {
        BlockTable::of_cubes(&[false, true, true, true])
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
    type ClientState = ();

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

    fn new_logic(_content: &ExampleContent, _world: &WorldData) -> Box<dyn GameLogic> {
        Box::new(ScriptedLogic::new())
    }
}

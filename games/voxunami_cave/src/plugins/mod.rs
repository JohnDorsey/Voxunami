//! Voxunami Cave's rules, as plugins of the server: who may change which blocks, how bodies move, how
//! fire burns, how light spreads. They know nothing of each other's state: they hear of one another's
//! work as events (a plugin that writes blocks is heard by the others through the engine's
//! `ChannelsChanged`), and share only the record of where players are, which the game gives them.

mod adapters;
mod block_editing;
mod fire;
mod items;
mod light;
mod movement;
mod ships;

use game_shell::GameContent;
use grid_bodies::{GridBodiesConfig, GridBodiesPlugin};
use physics::BodySettings;
use player_presence::PlayersPlugin;
use plugin_host::Plugin;

use crate::content::CaveContent;

pub use block_editing::{BlockBroken, BlockEditRequested, BlockEditingPlugin, BlockPlaced};
pub use fire::FirePlugin;
pub use items::ItemsPlugin;
pub use light::LightPlugin;
pub use movement::{MovementPlugin, SharedMovement};
pub use ships::ShipCommandsPlugin;

/// The plugins of the Cave server.
pub fn cave_plugins(content: &CaveContent) -> Vec<Box<dyn Plugin>> {
    // The server moves the players, so what a client says about where it is is not believed.
    let players = PlayersPlugin::new(false);
    let presence = players.presence();
    let movement = MovementPlugin::new(content, presence.clone());
    // The one that flies ships by their pilots' keys shares the players' bodies with the one that moves them.
    let commands = ShipCommandsPlugin::new(content, presence.clone(), movement.movement());
    vec![
        Box::new(players),
        Box::new(BlockEditingPlugin::new(content, presence.clone())),
        Box::new(ItemsPlugin::new(content, presence.clone())),
        Box::new(movement),
        Box::new(FirePlugin::new(content, presence)),
        Box::new(LightPlugin::new(content)),
        Box::new(ships(content)),
        Box::new(commands),
    ]
}

/// The plugin that moves ships: they fall under the gravity players do, rest on the terrain, and are as heavy as the density of their blocks.
fn ships(content: &CaveContent) -> GridBodiesPlugin {
    let table = content.block_table();
    GridBodiesPlugin::new(GridBodiesConfig {
        block_channel: content.block_channel(),
        state_channel: content.state_channel(),
        air: content.blocks.air,
        passable: Some(crate::block_properties::passable(&table)),
        density: table
            .handle(&block_models::engine::DENSITY)
            .expect("the engine declares engine:density in every game"),
        settings: BodySettings::default(),
        table,
    })
}

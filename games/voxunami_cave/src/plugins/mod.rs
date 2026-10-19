//! Voxunami Cave's rules, as plugins of the server: who may change which blocks, how bodies move, how
//! fire burns, how light spreads. They know nothing of each other's state: they hear of one another's
//! work as events (a plugin that writes blocks is heard by the others through the engine's
//! `ChannelsChanged`), and share only the record of where players are, which the game gives them.

mod adapters;
mod block_editing;
mod fire;
mod light;
mod movement;

use player_presence::PlayersPlugin;
use plugin_host::Plugin;

use crate::content::CaveContent;

pub use block_editing::{BlockEditRequested, BlockEditingPlugin};
pub use fire::FirePlugin;
pub use light::LightPlugin;
pub use movement::MovementPlugin;

/// The plugins of the Cave server.
pub fn cave_plugins(content: &CaveContent) -> Vec<Box<dyn Plugin>> {
    // The server moves the players, so what a client says about where it is is not believed.
    let players = PlayersPlugin::new(false);
    let presence = players.presence();
    vec![
        Box::new(players),
        Box::new(BlockEditingPlugin::new(content, presence.clone())),
        Box::new(MovementPlugin::new(content, presence.clone())),
        Box::new(FirePlugin::new(content, presence)),
        Box::new(LightPlugin::new(content)),
    ]
}

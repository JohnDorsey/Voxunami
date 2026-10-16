//! What a game tells the shell about itself.

use std::fmt::Debug;

use block_models::BlockTable;
use content_registry::IdentifierRegistry;
use namespaced_id::NamespacedId;
use server::GameLogic;
use transport::Connection;
use winit::keyboard::KeyCode;
use world_storage::{LoadContext, WorldData};
use world_view::ViewChannels;

use crate::session::Session;

/// A game's plugins, as the shell needs them to show and build worlds. Both the server and every
/// client have one.
pub trait GameContent: Debug + Send {
    /// The channels of a world's bricks that say what each voxel is drawn as: its block, and where
    /// the game has them, the state of the block (its shape) and the voxel's light.
    fn view_channels(&self) -> ViewChannels;

    /// How bright a face is that no light reaches, from 0 (black) to 255; see
    /// `renderer::Atmosphere::ambient_light`. Games with no light channel have light everywhere
    /// and are not affected. The default is 0.
    fn ambient_light(&self) -> u8 {
        0
    }

    /// What every block and state looks like and how it fills its cell, by block id. Built after
    /// the world is loaded, so that blocks from plugins that are not loaded are in it as unknown.
    fn block_table(&self) -> BlockTable;

    /// A new world with this seed.
    fn new_world(&self, seed: u64) -> WorldData;

    /// What loading a world file, or a server's description of a world, needs. `seed` is the
    /// world's: terrain generators are built from it.
    fn load_context(&mut self, seed: u64) -> LoadContext<'_>;

    /// The content tables a save or a description needs.
    fn registries(&self) -> Vec<(NamespacedId, &dyn IdentifierRegistry)>;
}

/// Where a player starts, in voxels, and which way they face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraStart {
    pub position: [f64; 3],
    pub yaw_degrees: f32,
    pub pitch_degrees: f32,
}

/// A game, as the shell runs it. Implemented by a unit struct in each game's crate.
///
/// The shell does everything that does not depend on the game: the window and camera, the
/// singleplayer server, joining it, streaming the world to the screen, screenshots, recording and
/// replay. The game says what it is, how its server behaves, and what the player's inputs do.
pub trait GameDefinition: Sized + 'static {
    type Content: GameContent + 'static;
    /// What the client remembers between messages and clicks: a selected block, a score.
    type ClientState: Default + Debug;

    /// The game's id, such as `voxunami_cave:game`. Servers and recordings carry it.
    fn id() -> NamespacedId;
    /// The window title.
    fn title() -> &'static str;
    /// What to type after `voxunami` to start the game.
    fn command() -> &'static str;
    /// One line saying what the game is.
    fn summary() -> &'static str;
    /// The controls beyond moving and looking, as lines of help text.
    fn controls() -> &'static str;
    fn default_seed() -> u64;
    fn default_camera() -> CameraStart;
    /// How far, in voxels, the crosshair reaches.
    fn reach() -> f32;

    fn new_content() -> Self::Content;
    /// The server's rules, built from the world it will serve (which holds any saved game state).
    fn new_logic(content: &Self::Content, world: &WorldData) -> Box<dyn GameLogic>;

    /// A game message from the server that is not about where players are.
    fn handle_game_message(_state: &mut Self::ClientState, _kind: &NamespacedId, _payload: &[u8]) {}

    /// The left mouse button, while the mouse is captured.
    fn primary_action<C: Connection>(_session: &mut Session<C, Self>) {}

    /// The right mouse button, while the mouse is captured.
    fn secondary_action<C: Connection>(_session: &mut Session<C, Self>) {}

    /// A key other than the movement keys went down.
    fn key_pressed<C: Connection>(_session: &mut Session<C, Self>, _key: KeyCode) {}

    /// Shown in the window title while playing.
    fn status_text<C: Connection>(_session: &Session<C, Self>) -> String {
        String::new()
    }
}

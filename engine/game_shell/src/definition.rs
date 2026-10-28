//! What a game tells the shell about itself.

use std::collections::HashSet;
use std::fmt::Debug;

use block_models::BlockTable;
use content_registry::IdentifierRegistry;
use namespaced_id::NamespacedId;
use plugin_host::Plugin;
use renderer::Camera;
use voxel_space::WorldPosition;
use winit::keyboard::KeyCode;
use world_storage::{LoadContext, WorldData};
use world_view::ViewChannels;

use crate::client_plugin::ClientPlugin;
use crate::package_data::PackageData;
use data_packs::{BodySupport, PackErrors, RawPack};

/// A game's plugins, as the shell needs them to show and build worlds. Both the server and every
/// client have one.
pub trait GameContent: Debug + Send {
    /// The channels of a world's bricks that say what each voxel is drawn as: its block, and where
    /// the game has them, the state of the block (its shape) and the voxel's light.
    fn view_channels(&self) -> ViewChannels;

    /// What a client needs to show this game's world, which the server sends in its welcome: how
    /// every block looks, which channels the view reads, how bright unlit faces are. Made after the
    /// world is loaded, so that blocks from plugins that are not loaded are in it as unknown.
    fn package_data(&self) -> PackageData;

    /// How bright a face is that no light reaches, from 0 (black) to 255; see
    /// `renderer::Atmosphere::ambient_light`. Games with no light channel have light everywhere
    /// and are not affected.
    fn ambient_light(&self) -> u8 {
        self.package_data().ambient_light
    }

    /// What every block and state looks like and how it fills its cell, by block id. Built after
    /// the world is loaded, so that blocks from plugins that are not loaded are in it as unknown.
    /// Panics if the game's own definitions cannot make a table, which is a mistake the game's tests
    /// find.
    fn block_table(&self) -> BlockTable {
        self.package_data()
            .block_table()
            .unwrap_or_else(|error| panic!("the game's blocks cannot be used: {error}"))
    }

    /// Adds the blocks of data packs and applies their patches, before a world is loaded. Returns what was skipped on
    /// purpose, one line each. A game that does not take packs says so.
    fn apply_packs(&mut self, _packs: &[RawPack]) -> Result<Vec<String>, PackErrors> {
        Err(PackErrors(vec![data_packs::PackError {
            file: "--packs".to_owned(),
            path: String::new(),
            message: "this game does not take packs".to_owned(),
        }]))
    }

    /// What packs may put in the bodies of this game's worlds: the families of terrain generators they may name, and the levels they may put bodies in. A
    /// game with no bodies says nothing.
    fn body_support(&self) -> BodySupport {
        BodySupport::default()
    }

    /// The bodies a new world of this game has: its own and those of the packs loaded, by name.
    fn body_names(&self) -> Vec<NamespacedId> {
        Vec::new()
    }

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

/// How a game draws terrain that is far away: as coarse nodes (`docs/lod-spec.md`), which the client makes from the terrain generators and the server corrects where
/// the world has been edited. A game that does not say draws bricks around the camera and nothing beyond.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoarseDetail {
    /// How far, in voxels, terrain is drawn.
    pub maximum_distance: f64,
    /// How bright terrain that the generators made is: they know nothing of the light, so this is the light of everything far away that nobody has edited (the
    /// server is configured with the same).
    pub unlit: u8,
    /// How many pixels across a cell of a node may be before it is split: the lower, the finer the drawing and the more nodes.
    pub cell_pixels: f32,
    /// The most bytes a second of corrected nodes the client asks the server for.
    pub bytes_per_second: u32,
}

/// A game, as the shell runs it. Implemented by a unit struct in each game's crate.
///
/// The shell does everything that does not depend on the game: the window and camera, the
/// singleplayer server, joining it, streaming the world to the screen, screenshots, recording and
/// replay. The game says what it is, how its server behaves, and what the player's inputs do.
pub trait GameDefinition: Sized + 'static {
    type Content: GameContent + 'static;

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

    /// How many ticks a second the game's server runs. Most games want the default, 20.
    fn ticks_per_second() -> u32 {
        20
    }

    /// What gravity is in the game's worlds: the pull of a level and which bodies pull besides. The default is down at 5 metres a second a second and nothing pulls.
    fn gravity() -> voxel_space::GravitySettings {
        voxel_space::GravitySettings::default()
    }

    /// The levels whose own pull is not the base of [`GameDefinition::gravity`], by identifier (a level in space has none). A level a world does not have is ignored.
    fn level_gravity() -> Vec<(NamespacedId, [f32; 3])> {
        Vec::new()
    }

    /// Whether the server also decides which way the player looks (a game that steers the player's
    /// avatar, such as a worm): the camera then turns to the angles the server says, smoothly, and
    /// the mouse does not turn it. Only meaningful with [`GameDefinition::server_moves_players`].
    /// The default is no.
    fn server_sets_view() -> bool {
        false
    }

    /// Whether a crosshair is drawn in the middle of the view. A game that does not aim at anything
    /// has none. The default is yes.
    fn shows_crosshair() -> bool {
        true
    }

    /// Whether the other players are drawn as pillars where they are. A game whose players are
    /// visible in the world itself does not want them. The default is yes.
    fn shows_other_players() -> bool {
        true
    }

    /// Whether the server moves the players' bodies (gravity, collision) and says where each one is,
    /// instead of each player's program moving its own camera and reporting it. If so the shell
    /// does not report the camera: it sends [`GameDefinition::join_message`] when joining and
    /// [`GameDefinition::movement_message`] as keys are held, and shows the camera where the server
    /// says the player is, though the way the player faces stays their own. The default is no.
    fn server_moves_players() -> bool {
        false
    }

    /// What to tell a server that moves players, on joining, to be put at `position` (the start of the
    /// camera).
    fn join_message(_position: WorldPosition) -> Option<(NamespacedId, Vec<u8>)> {
        None
    }

    /// What the keys held and the way the camera faces ask of the player's body, as a message for a
    /// server that moves players. Sent when it changes, and now and then.
    fn movement_message(
        _keys: &HashSet<KeyCode>,
        _camera: &Camera,
    ) -> Option<(NamespacedId, Vec<u8>)> {
        None
    }
    /// How far terrain is drawn, if beyond the bricks round the camera it is. The default is not at all.
    fn coarse_detail() -> Option<CoarseDetail> {
        None
    }

    /// How far the terrain of the level with this identifier is drawn, if beyond the bricks round the camera it is: what a window follows as the player goes from level to
    /// level (`Session::enable_level_detail`). The default is [`GameDefinition::coarse_detail`] for every level, so a game that says only that is drawn as it was.
    fn level_detail(level: &NamespacedId) -> Option<CoarseDetail> {
        let _ = level;
        Self::coarse_detail()
    }

    /// How far the terrain of a planet (a ship with terrain) is drawn, if beyond the bricks round the camera it is. The default is not at all. (The server corrects the
    /// nodes of a level where it was edited and the nodes of a planet it does not, so `bytes_per_second` means nothing here.)
    fn grid_detail() -> Option<CoarseDetail> {
        None
    }

    /// The plugins that are the server's rules. Each is given the world the server starts from when it
    /// is registered, and so finds any state it saved. The order of the list does not matter.
    fn plugins(content: &Self::Content) -> Vec<Box<dyn Plugin>>;

    /// The client halves of the game's plugins: what the client does with the server's messages, what a
    /// key, a mouse button or a typed `.`-command does, and what the HUD says. Made once for each
    /// session. The order of the list does not matter.
    fn client_plugins(_content: &Self::Content) -> Vec<Box<dyn ClientPlugin>> {
        Vec::new()
    }
}

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

use std::sync::Arc;

use block_models::{BlockTable, SharedBlockTable};
use game_shell::{GameContent, GameDefinition};
use grid_bodies::collision::BlockCollisions;
use grid_bodies::{BodiesReport, Docking, DockingSettings, GridBodiesConfig, GridBodiesPlugin};
use grid_lifecycle::{ArrivalConfig, GridLifecyclePlugin};
use physics::BodySettings;
use player_presence::PlayersPlugin;
use plugin_host::Plugin;
use voxel_space::{BrickCoordinate, WorldPosition};

use crate::content::CaveContent;
use crate::game::Cave;

pub use block_editing::{BlockBroken, BlockEditRequested, BlockEditingPlugin, BlockPlaced};
pub use fire::FirePlugin;
pub use items::ItemsPlugin;
pub use light::LightPlugin;
pub use movement::{MovementPlugin, SharedMovement};
pub use ships::ShipCommandsPlugin;

/// The plugins of the Cave server.
pub fn cave_plugins(content: &CaveContent) -> Vec<Box<dyn Plugin>> {
    cave_plugins_reporting(content).0
}

/// The plugins of the Cave server, and a way to ask what the one that moves ships has done with the colliders of terrain (a test counts them).
pub fn cave_plugins_reporting(
    content: &CaveContent,
) -> (
    Vec<Box<dyn Plugin>>,
    impl Fn() -> BodiesReport + Send + Sync + 'static + use<>,
) {
    cave_plugins_sharing(content, &SharedBlockTable::new(content.block_table()))
}

/// The same, with every rule that reads the table of blocks reading `table`: the one table of the server, which is held by every part of it that reads one, so
/// that replacing it through the handle is seen by all of them (`SharedBlockTable`). None of them makes a table of its own.
pub fn cave_plugins_sharing(
    content: &CaveContent,
    table: &SharedBlockTable,
) -> (
    Vec<Box<dyn Plugin>>,
    impl Fn() -> BodiesReport + Send + Sync + 'static + use<>,
) {
    // The server moves the players, so what a client says about where it is is not believed. A player sent to a level arrives on the ground at the top of the
    // planet that is in it; a player sent from a level with a planet in it to one that has none arrives where players start (the places of the one are nowhere in
    // the other); and otherwise they are at the same place.
    let (block_channel, air) = (content.block_channel(), content.blocks.air);
    let players = PlayersPlugin::new(false).with_level_arrival(move |world, from, to| {
        crate::landing::on_a_planet(world, to, block_channel, air).or_else(|| {
            crate::landing::on_a_planet(world, from, block_channel, air).map(|_| world_spawn())
        })
    });
    let presence = players.presence();
    let movement = MovementPlugin::new(content, table, presence.clone());
    // Ships with docking ports are drawn together and joined by the one that moves them, and undocked by the command; they share what docking is.
    let docking = docking_of(content, table.current());
    // The one that flies ships by their pilots' keys shares the players' bodies with the one that moves them.
    let commands = ShipCommandsPlugin::new(
        content,
        table,
        presence.clone(),
        movement.movement(),
        docking.clone(),
    );
    // Ships move while a player is near, wait farther off, and are put in files farther still. A player whose ship is lost can go to where
    // players start, and one whose ship went away with them and finds something in its place is asked what to do.
    let snapshot = table.current();
    let lifecycle = GridLifecyclePlugin::new(presence.clone(), move |world, level| {
        crate::landing::spawn_in(world, level, block_channel, air)
    })
    .with_arrivals(ArrivalConfig {
        collisions: BlockCollisions::new(
            Arc::clone(&snapshot),
            Some(crate::block_properties::passable(&snapshot)),
        ),
        block_channel: content.block_channel(),
        state_channel: content.state_channel(),
        orientation_channel: Some(content.orientation_channel()),
        air: content.blocks.air,
    });
    let bodies = ships(content, table, docking);
    let report = bodies.report_handle();
    let plugins: Vec<Box<dyn Plugin>> = vec![
        Box::new(players),
        Box::new(BlockEditingPlugin::new(content, table, presence.clone())),
        Box::new(ItemsPlugin::new(content, table, presence.clone())),
        Box::new(movement),
        Box::new(FirePlugin::new(content, table, presence)),
        Box::new(LightPlugin::new(content, table)),
        Box::new(bodies),
        Box::new(lifecycle),
        Box::new(commands),
    ];
    (plugins, report)
}

/// Where players start: the world's spawn.
pub fn world_spawn() -> WorldPosition {
    let [x, y, z] = Cave::default_camera().position;
    WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z])
}

/// How ships dock in Cave: the dev docking port, with the distances, forces and angles the owner chose (see [`DockingSettings`]).
pub fn docking(content: &CaveContent) -> Docking {
    docking_of(content, Arc::new(content.block_table()))
}

/// The same, for the blocks of `table`.
fn docking_of(content: &CaveContent, table: Arc<BlockTable>) -> Docking {
    Docking::new(
        DockingSettings::default(),
        table,
        content.block_channel(),
        content.state_channel(),
        Some(content.orientation_channel()),
        content.blocks.air,
    )
}

/// The plugin that moves ships: they fall under the gravity players do, rest on the terrain, are as heavy as the density of their blocks, and
/// dock.
fn ships(content: &CaveContent, table: &SharedBlockTable, docking: Docking) -> GridBodiesPlugin {
    let table = table.current();
    GridBodiesPlugin::new(GridBodiesConfig {
        block_channel: content.block_channel(),
        state_channel: content.state_channel(),
        orientation_channel: Some(content.orientation_channel()),
        air: content.blocks.air,
        passable: Some(crate::block_properties::passable(&table)),
        density: table
            .handle(&block_models::engine::DENSITY)
            .expect("the engine declares engine:density in every game"),
        docking: Some(docking),
        joints: Some(grid_bodies::JointSettings::default()),
        settings: BodySettings::default(),
        table,
    })
}

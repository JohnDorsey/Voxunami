//! The players' bodies: spawning, walking and flying, `/fly` and `/tp`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use block_models::BlockTable;
use game_shell::GameContent;
use namespaced_id::NamespacedId;
use player_presence::{SharedPresence, lock};
use plugin_host::{
    Argument, Arguments, CommandSpec, HandlerSpec, Phase, PlayerLeft, Plugin, Registrar, SystemSpec,
};
use protocol::ClientId;
use voxel_grid::ChannelIndex;
use voxel_space::{LevelIndex, VoxelCoordinate, WorldPosition};

use crate::content::CaveContent;
use crate::messages::{MoveRequest, Spawn};
use crate::movement::Movement;

/// Moves the players' bodies, on the server: a client says which keys are held and which way it
/// faces, never where it is. The terrain bodies stand on is made again where blocks change.
#[derive(Debug)]
pub struct MovementPlugin {
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    table: BlockTable,
    air: u32,
    players: SharedPresence,
}

impl MovementPlugin {
    pub fn new(content: &CaveContent, players: SharedPresence) -> Self {
        Self {
            block_channel: content.block_channel(),
            state_channel: content.state_channel(),
            table: content.block_table(),
            air: content.blocks.air,
            players,
        }
    }
}

fn locked(movement: &Mutex<Movement>) -> MutexGuard<'_, Movement> {
    movement
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `/tp X Y Z`: puts the player's eyes at a position, in voxels. Returns what to tell them.
fn teleport(movement: &mut Movement, client: ClientId, arguments: &Arguments<'_>) -> String {
    let [x, y, z] = [0, 1, 2].map(|position| arguments.number(position));
    // The position must be somewhere in the world.
    let Ok(voxel) = VoxelCoordinate::try_new(x.floor() as i64, y.floor() as i64, z.floor() as i64)
    else {
        return "that is outside the world".to_owned();
    };
    let (brick, local) = voxel.split();
    let fraction = |value: f64| (value - value.floor()) as f32;
    let position = WorldPosition::new(
        brick,
        [
            f32::from(local.x()) + fraction(x),
            f32::from(local.y()) + fraction(y),
            f32::from(local.z()) + fraction(z),
        ],
    );
    if movement.teleport(client, position) {
        format!("teleported to {x} {y} {z}")
    } else {
        "you have not joined yet".to_owned()
    }
}

impl Plugin for MovementPlugin {
    fn id(&self) -> NamespacedId {
        "voxunami_cave:movement"
            .parse()
            .expect("constant identifier is valid")
    }

    fn depends_on(&self) -> Vec<NamespacedId> {
        vec![
            "player_presence:players"
                .parse()
                .expect("constant identifier is valid"),
        ]
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        let movement = Arc::new(Mutex::new(Movement::new(
            self.table.clone(),
            self.block_channel,
            self.state_channel,
            self.air,
        )));

        let (bodies, players) = (Arc::clone(&movement), Arc::clone(&self.players));
        registrar.message(move |context, client, Spawn(pose)| {
            let mut movement = locked(&bodies);
            if !movement.has_body(client) {
                let mut presence = lock(&players);
                let world = context.world();
                // A player who took over the entity their key had before resumes where they were.
                let pose = presence.pose_of(world.data(), client).unwrap_or(pose);
                movement.spawn(client, pose, presence.level_of(world.data(), client));
                presence.set_pose(world, client, pose);
            }
        });
        let bodies = Arc::clone(&movement);
        registrar.message(move |_, client, request: MoveRequest| {
            locked(&bodies).ask(client, request);
        });

        let bodies = Arc::clone(&movement);
        registrar.command(
            CommandSpec::new("fly", "toggles flying and walking"),
            move |context, client, _| {
                let flying = locked(&bodies).toggle_flying(client);
                context.reply(
                    client,
                    match flying {
                        Some(true) => "you are flying",
                        Some(false) => "you are walking",
                        None => "you have not joined yet",
                    },
                );
            },
        );
        let bodies = Arc::clone(&movement);
        registrar.command(
            CommandSpec::new("tp", "puts your eyes at a position, in voxels")
                .argument(Argument::number("x"))
                .argument(Argument::number("y"))
                .argument(Argument::number("z")),
            move |context, client, arguments| {
                let reply = teleport(&mut locked(&bodies), client, arguments);
                context.reply(client, &reply);
            },
        );

        let bodies = Arc::clone(&movement);
        registrar.on::<PlayerLeft>(
            HandlerSpec::new("voxunami_cave:remove_the_body_of_a_player_who_leaves"),
            move |event, _| locked(&bodies).remove(event.client),
        );
        // Where blocks changed, the terrain bodies stand on is made again.
        let bodies = Arc::clone(&movement);
        registrar.on_channels(
            HandlerSpec::new("voxunami_cave:remake_the_terrain_bodies_stand_on"),
            &[self.block_channel, self.state_channel],
            move |event, _| {
                let Some(level) = event.grid.root_level() else {
                    return;
                };
                let mut movement = locked(&bodies);
                for voxel in &event.voxels {
                    movement.brick_changed(level, voxel.split().0);
                }
            },
        );

        // The bodies move first, so that what the players do this tick is from where they are; the
        // entities that are the players move with them, and clients are told at the end of the tick.
        let (bodies, players) = (movement, Arc::clone(&self.players));
        registrar.system(
            SystemSpec::new("voxunami_cave:move_the_bodies", Phase::Physics),
            move |context| {
                let mut presence = lock(&players);
                let levels: BTreeMap<ClientId, LevelIndex> = presence
                    .players(context.world().data())
                    .into_iter()
                    .map(|(client, _, level)| (client, level))
                    .collect();
                let moved = locked(&bodies).step(context.world(), |client| {
                    levels.get(&client).copied().unwrap_or(LevelIndex::MAIN)
                });
                for (client, pose) in moved {
                    presence.set_pose(context.world(), client, pose);
                }
            },
        );
    }
}

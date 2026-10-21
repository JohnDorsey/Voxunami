//! The players' bodies: spawning, walking and flying, `/fly` and `/tp`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use block_models::BlockTable;
use game_shell::GameContent;
use namespaced_id::NamespacedId;
use player_presence::{PlayerPose, Presence, SharedPresence, lock};
use plugin_host::{
    Argument, Arguments, CommandSpec, HandlerSpec, Phase, PlayerLeft, Plugin, Registrar, SystemSpec,
};
use protocol::ClientId;
use server::ServerWorld;
use voxel_grid::ChannelIndex;
use voxel_space::{GridId, LevelIndex, Orientation, VoxelCoordinate, WorldPosition};

use crate::content::CaveContent;
use crate::messages::{MoveRequest, Spawn};
use crate::movement::{Movement, Placement};

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

/// A position as plain numbers, in voxels.
fn voxels(position: WorldPosition) -> [f64; 3] {
    let brick = position.brick();
    let offset = position.offset();
    [
        brick.x() as f64 * 32.0 + f64::from(offset[0]),
        brick.y() as f64 * 32.0 + f64::from(offset[1]),
        brick.z() as f64 * 32.0 + f64::from(offset[2]),
    ]
}

/// `/tp X Y Z`: puts the player's eyes at a position, in voxels. Returns what to tell them: where they are, which
/// is not the place asked for if that is inside the terrain (they are moved out of it, the shortest way).
fn teleport(
    movement: &mut Movement,
    presence: &mut Presence,
    world: &mut ServerWorld,
    level: LevelIndex,
    client: ClientId,
    arguments: &Arguments<'_>,
) -> String {
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
    let Some(now) = movement.teleport(world, level, client, position) else {
        return "you have not joined yet".to_owned();
    };
    // The record of the player is where they are for everyone, and the body is made to agree with it each tick: so the record goes
    // there now, in the level and not aboard whatever ship they were on, looking the way they were.
    if let Some((_, saved, _)) = presence.local_pose_of(world.data(), client) {
        let pose = PlayerPose {
            position: now,
            ..saved
        };
        let _ = presence.set_pose_in(
            world,
            client,
            GridId::root(level),
            pose,
            pose.view_orientation(Orientation::IDENTITY),
        );
    }
    // Their client watches around its camera, which may be far from here: it is told where they are.
    presence.tell_placed(world, client);
    let [now_x, now_y, now_z] = voxels(now);
    // (The position is made of an `f32` offset in a brick, so a place that was not moved is the one asked for,
    // to within what an `f32` holds.)
    let moved = [now_x - x, now_y - y, now_z - z]
        .iter()
        .any(|difference| difference.abs() > 1.0e-3);
    if moved {
        format!(
            "teleported to {now_x} {now_y} {now_z} (moved from {x} {y} {z}, which is inside the terrain)"
        )
    } else {
        format!("teleported to {x} {y} {z}")
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
            // The ships move first, so that the players meet them where they are this tick.
            "engine:grid_bodies"
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
                // A player who took over the entity their key had before resumes where they were, in the frame they were in: aboard
                // the ship they logged out on, wherever it is now.
                let placement = match presence.local_pose_of(world.data(), client) {
                    Some((frame, saved, orientation)) => Placement {
                        frame,
                        level: presence.level_of(world.data(), client),
                        position: saved.position,
                        orientation,
                        yaw: saved.yaw,
                        pitch: saved.pitch,
                    },
                    None => {
                        let level = presence.level_of(world.data(), client);
                        Placement {
                            frame: GridId::root(level),
                            level,
                            position: pose.position,
                            orientation: Orientation::from_yaw_pitch(pose.yaw, pose.pitch),
                            yaw: pose.yaw,
                            pitch: pose.pitch,
                        }
                    }
                };
                let is_new = presence.entity_of(client).is_none();
                movement.spawn(client, placement);
                if is_new {
                    presence.set_pose(world, client, pose);
                }
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
        let (bodies, players) = (Arc::clone(&movement), Arc::clone(&self.players));
        registrar.command(
            CommandSpec::new("tp", "puts your eyes at a position, in voxels")
                .argument(Argument::number("x"))
                .argument(Argument::number("y"))
                .argument(Argument::number("z")),
            move |context, client, arguments| {
                let mut presence = lock(&players);
                let level = presence.level_of(context.world().data(), client);
                let reply = teleport(
                    &mut locked(&bodies),
                    &mut presence,
                    context.world(),
                    level,
                    client,
                    arguments,
                );
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
                let mut movement = locked(&bodies);
                for voxel in &event.voxels {
                    movement.brick_changed(event.grid, voxel.split().0);
                }
            },
        );

        // The bodies move after the ships do, so that what the players do this tick is from where they and the ships are; the
        // entities that are the players move with them, and clients are told at the end of the tick.
        let (bodies, players) = (movement, Arc::clone(&self.players));
        registrar.system(
            SystemSpec::new("voxunami_cave:move_the_bodies", Phase::Physics)
                .after("engine:grid_bodies_move"),
            move |context| {
                let mut presence = lock(&players);
                // Where each player is, by their record: the frame and the place in it, and the level.
                let placements: BTreeMap<ClientId, Placement> = presence
                    .players(context.world().data())
                    .into_iter()
                    .filter_map(|(client, _, level)| {
                        let (frame, pose, orientation) =
                            presence.local_pose_of(context.world().data(), client)?;
                        Some((
                            client,
                            Placement {
                                frame,
                                level,
                                position: pose.position,
                                orientation,
                                yaw: pose.yaw,
                                pitch: pose.pitch,
                            },
                        ))
                    })
                    .collect();
                let moved = locked(&bodies)
                    .step(context.world(), |client| placements.get(&client).copied());
                for update in moved {
                    // (A ship that is gone since the body was made: the record says where the player is, and the body is made to agree
                    // next tick.)
                    let _ = presence.set_pose_in(
                        context.world(),
                        update.client,
                        update.frame,
                        update.pose,
                        update.orientation,
                    );
                }
            },
        );
    }
}

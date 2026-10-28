//! The players' bodies: spawning, walking and flying, `/fly` and `/tp` (to coordinates, and to the places of `places.rs`).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use game_shell::GameContent;
use namespaced_id::NamespacedId;
use player_presence::{PlayerPose, Presence, SharedPresence, lock};
use plugin_host::{
    Argument, Arguments, CommandSpec, HandlerSpec, Phase, PlayerLeft, Plugin, Registrar, SystemSpec,
};
use protocol::ClientId;
use server::ServerWorld;
use terrain::GreatRivers;
use voxel_grid::ChannelIndex;
use voxel_space::{Displacement, GridId, LevelIndex, Orientation, VoxelCoordinate, WorldPosition};

use crate::content::CaveContent;
use crate::messages::{MoveRequest, Spawn};
use crate::movement::{Movement, Placement};
use crate::places::{self, BodyArrival, PlaceKind};
use crate::rivers;

/// Moves the players' bodies, on the server: a client says which keys are held and which way it
/// faces, never where it is. The terrain bodies stand on is made again where blocks change.
#[derive(Debug)]
pub struct MovementPlugin {
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    air: u32,
    players: SharedPresence,
    movement: SharedMovement,
}

/// The players' bodies, which the plugin that flies ships shares with the one that moves players.
pub type SharedMovement = Arc<Mutex<Movement>>;

impl MovementPlugin {
    pub fn new(content: &CaveContent, players: SharedPresence) -> Self {
        let movement = Movement::new(
            content.block_table(),
            content.block_channel(),
            content.state_channel(),
            content.orientation_channel(),
            content.blocks.air,
        );
        Self {
            block_channel: content.block_channel(),
            state_channel: content.state_channel(),
            air: content.blocks.air,
            players,
            movement: Arc::new(Mutex::new(movement)),
        }
    }

    /// The bodies of the players, to share.
    pub fn movement(&self) -> SharedMovement {
        Arc::clone(&self.movement)
    }
}

pub(super) fn locked(movement: &Mutex<Movement>) -> MutexGuard<'_, Movement> {
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

/// Puts the player's eyes at `position` in `level` (moved out of the terrain if that is inside it), in the level and not aboard whatever ship they were on, and tells
/// them and everyone where they are. Returns where their eyes are now, or `None` if they have not joined.
fn put_player_at(
    movement: &mut Movement,
    presence: &mut Presence,
    world: &mut ServerWorld,
    level: LevelIndex,
    client: ClientId,
    position: WorldPosition,
) -> Option<WorldPosition> {
    let now = movement.teleport(world, level, client, position)?;
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
    Some(now)
}

/// What `/tp` is told when it is not understood.
const TP_USAGE: &str = "usage: /tp, /tp X Y Z, /tp PLACE [LATITUDE LONGITUDE | X Y Z]";

/// What `/tp` with no words answers: the places of the world, and the ways to say where.
fn list_places(world: &ServerWorld) -> String {
    let names: Vec<String> = places::places(world.data())
        .iter()
        .map(|place| place.name.to_string())
        .collect();
    format!(
        "places: {}. /tp PLACE goes to the ground of a body nearest to you, or in view of a landmark; /tp PLACE LATITUDE LONGITUDE and /tp PLACE X Y Z (in the body's own axes) say where on a body; /tp X Y Z goes to coordinates of the level you are in",
        if names.is_empty() {
            "none".to_owned()
        } else {
            names.join(", ")
        }
    )
}

fn finite_number(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|number| number.is_finite())
}

/// `/tp`: with no words, lists the places; with a place (its name has a colon), goes to it, and with numbers after it says where on it; with three numbers, goes to
/// those coordinates of the level the player is in. Returns what to tell them.
fn tp(
    movement: &mut Movement,
    presence: &mut Presence,
    world: &mut ServerWorld,
    client: ClientId,
    arguments: &Arguments<'_>,
    ground: (ChannelIndex, u32),
) -> String {
    let mut words: Vec<&str> = Vec::new();
    if arguments.present(0) {
        words.push(arguments.word(0));
        words.extend_from_slice(arguments.rest(1));
    }
    let Some((&first, rest)) = words.split_first() else {
        return list_places(world);
    };
    if first.contains(':') {
        go_to_a_place(movement, presence, world, client, first, rest, ground)
    } else {
        go_to_coordinates(movement, presence, world, client, &words)
    }
}

/// `/tp X Y Z`: puts the player's eyes at a position, in voxels, in the level they are in. Returns what to tell them: where they are, which is not the place asked for if
/// that is inside the terrain (they are moved out of it, the shortest way).
fn go_to_coordinates(
    movement: &mut Movement,
    presence: &mut Presence,
    world: &mut ServerWorld,
    client: ClientId,
    words: &[&str],
) -> String {
    if words.len() != 3 {
        return TP_USAGE.to_owned();
    }
    let mut coordinates = [(0, 0.0); 3];
    for (index, (axis, word)) in ["X", "Y", "Z"].into_iter().zip(words).enumerate() {
        match places::parse_coordinate(word) {
            Some(parsed) => coordinates[index] = parsed,
            None => return format!("usage: /tp X Y Z; {axis} must be a number"),
        }
    }
    // The position must be somewhere in the world.
    let Some(position) = places::position_of(coordinates) else {
        return "that is outside the world".to_owned();
    };
    let level = presence.level_of(world.data(), client);
    let Some(now) = put_player_at(movement, presence, world, level, client, position) else {
        return "you have not joined yet".to_owned();
    };
    let [x, y, z] = [0, 1, 2].map(|axis| {
        words[axis]
            .parse::<f64>()
            .expect("a coordinate that was read is a number")
    });
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

/// `/tp PLACE [LATITUDE LONGITUDE | X Y Z]`: see [`places`]. A place in another level takes the player there.
fn go_to_a_place(
    movement: &mut Movement,
    presence: &mut Presence,
    world: &mut ServerWorld,
    client: ClientId,
    name: &str,
    rest: &[&str],
    (block_channel, air): (ChannelIndex, u32),
) -> String {
    let unknown = || format!("there is no place {name}; /tp alone lists the places");
    let Some(place) = name
        .parse::<NamespacedId>()
        .ok()
        .and_then(|name| places::find(world.data(), &name))
    else {
        return unknown();
    };
    let here = presence.level_of(world.data(), client);
    // Where the player is, if it means something to the place: in its level.
    let player = if place.level == here {
        presence
            .pose_of(world.data(), client)
            .map(|pose| pose.position)
    } else {
        None
    };
    // Where they are to be put, and what to tell them of it.
    let (eyes, told): (WorldPosition, String) = match &place.kind {
        PlaceKind::Body(grid) => {
            let arrival = match *rest {
                [] => BodyArrival::Nearest,
                [latitude, longitude] => {
                    let Some(latitude) = finite_number(latitude) else {
                        return "LATITUDE must be a number".to_owned();
                    };
                    let Some(longitude) = finite_number(longitude) else {
                        return "LONGITUDE must be a number".to_owned();
                    };
                    if !(-90.0..=90.0).contains(&latitude) {
                        return "a latitude is from -90 to 90".to_owned();
                    }
                    BodyArrival::Direction {
                        latitude,
                        longitude,
                    }
                }
                [x, y, z] => {
                    let mut coordinates = [(0, 0.0); 3];
                    for (index, (axis, word)) in
                        ["X", "Y", "Z"].into_iter().zip([x, y, z]).enumerate()
                    {
                        match places::parse_coordinate(word) {
                            Some(parsed) => coordinates[index] = parsed,
                            None => return format!("{axis} must be a number"),
                        }
                    }
                    BodyArrival::Point(coordinates)
                }
                _ => return TP_USAGE.to_owned(),
            };
            let landed =
                match places::land_on_body(world, *grid, arrival, player, block_channel, air) {
                    Ok(landed) => landed,
                    Err(why) => return why,
                };
            // (To a tenth of a degree, and no minus zero for a latitude that is a hair under the equator.)
            let tenth = |degrees: f64| (degrees * 10.0).round() / 10.0 + 0.0;
            let told = match landed.direction {
                Some((latitude, longitude)) => format!(
                    "you are on {} at latitude {:.1}, longitude {:.1}",
                    place.name,
                    tenth(latitude),
                    tenth(longitude)
                ),
                None => format!(
                    "you are at {} {} {} of {}",
                    rest[0], rest[1], rest[2], place.name
                ),
            };
            (landed.position, told)
        }
        PlaceKind::RiverKnot { index, knot } => {
            if !rest.is_empty() {
                return format!("{} has no coordinates; /tp it alone", place.name);
            }
            let rivers = GreatRivers::new(world.data().world_seed);
            // The way they look: the angles their program last sent.
            let forward = movement
                .request_of(client)
                .map_or([0.0, 0.0, -1.0], |request| {
                    rivers::forward(request.input.yaw, request.pitch)
                });
            let eyes = rivers::eyes_in_view_of(&rivers, knot, forward);
            let Ok(voxel) = VoxelCoordinate::try_new(eyes[0], eyes[1], eyes[2]) else {
                return "that is outside the world".to_owned();
            };
            let away = [0, 1, 2].map(|axis| (eyes[axis] - knot.position[axis]) as f64);
            let distance = (away[0] * away[0] + away[1] * away[1] + away[2] * away[2]).sqrt();
            let (brick, local) = voxel.split();
            let position = WorldPosition::new(
                brick,
                [
                    f32::from(local.x()) + 0.5,
                    f32::from(local.y()) + 0.5,
                    f32::from(local.z()) + 0.5,
                ],
            );
            (
                position,
                format!(
                    "you are {} from {} (knot {index}, {} across)",
                    rivers::length_in_words(distance),
                    place.name,
                    rivers::length_in_words(knot.radius() * 2.0)
                ),
            )
        }
    };
    // A place in another level takes the player there first.
    let mut note = String::new();
    if place.level != here {
        presence.move_to_level(client, place.level, world);
        if let Some(level) = world.data().level_id(place.level) {
            note = format!(" (in {level})");
        }
    }
    let Some(now) = put_player_at(movement, presence, world, place.level, client, eyes) else {
        return "you have not joined yet".to_owned();
    };
    // A place that was in the terrain is not where they are: they are moved out of it, which is said (it is a fault of the arrival if it is a body's ground).
    let [now_x, now_y, now_z] = voxels(now);
    let [x, y, z] = voxels(eyes);
    if [now_x - x, now_y - y, now_z - z]
        .iter()
        .any(|difference| difference.abs() > 0.05)
    {
        note.push_str(" (moved out of the terrain)");
    }
    format!("{told}{note}")
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
        let movement = Arc::clone(&self.movement);

        let (bodies, players) = (Arc::clone(&movement), Arc::clone(&self.players));
        let (block_channel, air) = (self.block_channel, self.air);
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
                        // A new player in a level that has a planet in it stands on the ground at the top of it if their client began where the game says players
                        // start (which is not a place on a planet), and is put where it asked to be otherwise (`--camera`).
                        let landing = if pose.position == super::world_spawn() {
                            crate::landing::on_a_planet(world, level, block_channel, air)
                        } else {
                            None
                        };
                        let position = landing.unwrap_or(pose.position);
                        Placement {
                            frame: GridId::root(level),
                            level,
                            position,
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
        let ground = (self.block_channel, self.air);
        registrar.command(
            CommandSpec::new(
                "tp",
                "goes to a place or to coordinates; /tp alone lists the places",
            )
            .argument(Argument::word("where").optional())
            .argument(Argument::rest("numbers").optional()),
            move |context, client, arguments| {
                let mut presence = lock(&players);
                let reply = tp(
                    &mut locked(&bodies),
                    &mut presence,
                    context.world(),
                    client,
                    arguments,
                    ground,
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
        let warmed = std::sync::atomic::AtomicBool::new(false);
        registrar.system(
            SystemSpec::new("voxunami_cave:move_the_bodies", Phase::Physics)
                .after("engine:grid_bodies_move"),
            move |context| {
                // The first time, the bricks where players arrive on a planet are asked for, to be made meanwhile.
                if !warmed.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    crate::landing::warm_up_the_poles(context.world(), block_channel, air);
                }
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
                // What the world carried each player by this tick (a planet that goes round), for their bodies to go with.
                let carried: BTreeMap<ClientId, Displacement> = placements
                    .keys()
                    .filter_map(|&client| {
                        let entity = presence.entity_of(client)?;
                        Some((client, context.world().carried_by_rails(entity)?))
                    })
                    .collect();
                let moved = locked(&bodies).step(
                    context.world(),
                    |client| placements.get(&client).copied(),
                    |client| carried.get(&client).copied(),
                );
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

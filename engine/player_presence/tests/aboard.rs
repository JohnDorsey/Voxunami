//! A player aboard a ship: the entity is in the frame of the ship, and everything that asks where a player is, in which level,
//! gets the answer in the world. Checked through a real server with the pretend game of `world_storage::test_support`.

use std::sync::{Arc, Mutex};

use entities::{EntityCatalog, EntityError};
use player_presence::{PlayerPlaced, PlayerPose, PoseReport, Presence, player_kind_spec};
use protocol::{ClientId, ClientMessage, Message, ServerMessage};
use server::test_support::{ScriptedLogic, log_in};
use server::{Server, ServerConfig, ServerWorld};
use transport::Connection;
use transport::loopback::{LoopbackConnection, LoopbackConnector, loopback_listener};
use voxel_space::{BrickCoordinate, GridId, GridPose, LevelIndex, Orientation, WorldPosition};
use world_storage::test_support::{ALL_PLUGINS, Game, id};

type Action = Box<dyn FnOnce(&mut ServerWorld, &mut Presence) + Send>;

struct Rig {
    server: Server,
    connector: LoopbackConnector,
    connection: LoopbackConnection,
    presence: Arc<Mutex<Presence>>,
    actions: Arc<Mutex<Vec<Action>>>,
}

const PLAYER: ClientId = ClientId::new(1);

impl Rig {
    /// A server of a world with a second level, whose logic is the players' presence and whatever the test asks to be done in
    /// a tick; one player has joined and reported a pose at the origin of the main level, so they have an entity.
    fn new() -> Self {
        let game = Game::new(&ALL_PLUGINS);
        let mut world = game.new_world();
        world
            .add_level(
                id("test_content:second"),
                game.new_level("base_terrain:flat_v1"),
            )
            .unwrap();
        let mut catalog = EntityCatalog::default();
        catalog.kinds.register(player_kind_spec()).unwrap();
        let presence = Arc::new(Mutex::new(Presence::new()));
        let actions: Arc<Mutex<Vec<Action>>> = Arc::default();
        let logic = ScriptedLogic::new()
            .with_entity_catalog(catalog)
            .on_join({
                let presence = Arc::clone(&presence);
                move |client, world| presence.lock().unwrap().client_joined(client, world)
            })
            .on_leave({
                let presence = Arc::clone(&presence);
                move |client, world| presence.lock().unwrap().client_left(client, world)
            })
            .on_message({
                let presence = Arc::clone(&presence);
                move |client, kind, payload, world| {
                    presence
                        .lock()
                        .unwrap()
                        .handle_message(world, client, kind, payload);
                }
            })
            .on_tick({
                let presence = Arc::clone(&presence);
                let actions = Arc::clone(&actions);
                move |_, world| {
                    let queued: Vec<Action> = std::mem::take(&mut *actions.lock().unwrap());
                    for action in queued {
                        action(world, &mut presence.lock().unwrap());
                    }
                }
            });
        let (listener, connector) = loopback_listener();
        let mut server = Server::new(
            ServerConfig::new(id("test_game:game")),
            world,
            &game.registries(),
            Box::new(logic),
            Box::new(listener),
        )
        .unwrap();
        let mut connection = connector.connect().unwrap();
        log_in(&mut server, &mut connection, &id("test_game:game"), 1);
        let mut rig = Self {
            server,
            connector,
            connection,
            presence,
            actions,
        };
        rig.connection
            .send(
                ClientMessage::GameMessage {
                    kind: PoseReport::kind(),
                    payload: PoseReport(PlayerPose {
                        position: WorldPosition::new(BrickCoordinate::ORIGIN, [1.0; 3]),
                        yaw: 0.0,
                        pitch: 0.0,
                    })
                    .encode(),
                }
                .encode(),
            )
            .unwrap();
        rig.server.step();
        rig
    }

    /// Runs `action` in the next tick.
    fn in_a_tick(&mut self, action: impl FnOnce(&mut ServerWorld, &mut Presence) + Send + 'static) {
        self.actions.lock().unwrap().push(Box::new(action));
        self.server.step();
    }

    fn presence(&self) -> std::sync::MutexGuard<'_, Presence> {
        self.presence.lock().unwrap()
    }
}

fn second() -> LevelIndex {
    LevelIndex::new(1)
}

fn at(brick: [i64; 3], offset: [f32; 3]) -> WorldPosition {
    WorldPosition::new(BrickCoordinate::new(brick[0], brick[1], brick[2]), offset)
}

/// A ship in the second level, a quarter turn about the vertical axis from the world, with its origin at brick (2, 0, 0) plus (1, 2, 3).
fn quarter_turned_ship() -> GridPose {
    GridPose::new(
        at([2, 0, 0], [1.0, 2.0, 3.0]),
        Orientation::from_yaw_pitch(std::f32::consts::FRAC_PI_2, 0.0),
    )
}

fn looking(position: WorldPosition) -> PlayerPose {
    PlayerPose {
        position,
        yaw: 0.5,
        pitch: -0.25,
    }
}

/// The ship's id, once a tick has made it.
fn make_ship(rig: &mut Rig, pose: GridPose) -> GridId {
    let made = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&made);
    rig.in_a_tick(move |world, _| {
        *slot.lock().unwrap() = Some(world.create_grid(second(), pose).unwrap());
    });
    made.lock().unwrap().expect("the tick made the ship")
}

#[test]
fn a_player_aboard_is_in_the_world_where_the_ship_puts_them_and_in_the_level_of_the_ship() {
    let mut rig = Rig::new();
    let ship = make_ship(&mut rig, quarter_turned_ship());
    assert_eq!(
        rig.presence().frame_of(rig.server.world(), PLAYER),
        Some(GridId::MAIN)
    );

    let local = at([0, 0, 0], [10.0, 0.0, 0.0]);
    let pose = looking(local);
    rig.in_a_tick(move |world, presence| {
        presence
            .set_pose_in(
                world,
                PLAYER,
                ship,
                pose,
                Orientation::from_yaw_pitch(0.5, -0.25),
            )
            .unwrap();
    });

    let presence = rig.presence();
    let world = rig.server.world();
    // In the world: ten along the ship's x is ten along the world's -z from its origin, which is at (2 bricks, 1, 2, 3).
    let seen = presence.pose_of(world, PLAYER).unwrap();
    assert_eq!(seen.position.brick(), BrickCoordinate::new(2, 0, -1));
    let offset = seen.position.offset();
    assert!(
        (offset[0] - 1.0).abs() < 1e-4
            && (offset[1] - 2.0).abs() < 1e-4
            && (offset[2] - 25.0).abs() < 1e-4,
        "{offset:?}"
    );
    // The look angles are the player's own, not turned.
    assert_eq!((seen.yaw, seen.pitch), (0.5, -0.25));
    // In the level of the ship, not the main level.
    assert_eq!(presence.level_of(world, PLAYER), second());
    assert_eq!(presence.players(world), [(PLAYER, seen, second())]);
    // In the frame of the ship, the pose is what was set.
    let (frame, local_pose, orientation) = presence.local_pose_of(world, PLAYER).unwrap();
    assert_eq!(frame, ship);
    assert_eq!(local_pose, pose);
    assert_eq!(orientation, Orientation::from_yaw_pitch(0.5, -0.25));
}

#[test]
fn a_ship_that_moves_carries_the_player_and_a_ship_that_is_gone_leaves_them_in_the_world() {
    let mut rig = Rig::new();
    let ship = make_ship(&mut rig, quarter_turned_ship());
    let pose = looking(at([0, 0, 0], [10.0, 0.0, 0.0]));
    rig.in_a_tick(move |world, presence| {
        presence
            .set_pose_in(world, PLAYER, ship, pose, Orientation::IDENTITY)
            .unwrap();
    });
    let before = rig.presence().pose_of(rig.server.world(), PLAYER).unwrap();
    // The ship moves three bricks along the world's x: the player does, and their pose in the ship does not change.
    rig.in_a_tick(move |world, _| {
        let moved = GridPose::new(
            at([5, 0, 0], [1.0, 2.0, 3.0]),
            quarter_turned_ship().orientation,
        );
        world.set_grid_pose(ship, moved).unwrap();
    });
    let presence = rig.presence();
    let carried = presence.pose_of(rig.server.world(), PLAYER).unwrap();
    assert_eq!(
        carried.position.relative_to(before.position),
        [96.0, 0.0, 0.0]
    );
    assert_eq!(
        presence
            .local_pose_of(rig.server.world(), PLAYER)
            .unwrap()
            .1,
        pose
    );
    drop(presence);
    // The ship is destroyed: the player stays where they were in the world, in the level, and their entity is in the level's frame.
    rig.in_a_tick(move |world, _| {
        world.remove_grid(ship).unwrap();
    });
    let presence = rig.presence();
    let world = rig.server.world();
    assert_eq!(
        presence.frame_of(world, PLAYER),
        Some(GridId::root(second()))
    );
    assert_eq!(presence.level_of(world, PLAYER), second());
    let left = presence.pose_of(world, PLAYER).unwrap();
    assert_eq!(left.position, carried.position);
    assert_eq!((left.yaw, left.pitch), (carried.yaw, carried.pitch));
}

#[test]
fn a_player_sent_to_another_level_while_aboard_leaves_the_ship_where_it_is_in_the_world() {
    let mut rig = Rig::new();
    let ship = make_ship(&mut rig, quarter_turned_ship());
    let pose = looking(at([0, 0, 0], [10.0, 0.0, 0.0]));
    rig.in_a_tick(move |world, presence| {
        presence
            .set_pose_in(world, PLAYER, ship, pose, Orientation::IDENTITY)
            .unwrap();
    });
    let aboard_at = rig.presence().pose_of(rig.server.world(), PLAYER).unwrap();
    rig.in_a_tick(|world, presence| presence.move_to_level(PLAYER, LevelIndex::MAIN, world));
    let presence = rig.presence();
    let world = rig.server.world();
    assert_eq!(presence.frame_of(world, PLAYER), Some(GridId::MAIN));
    assert_eq!(presence.level_of(world, PLAYER), LevelIndex::MAIN);
    // The same place in the world, not the same coordinates.
    assert_eq!(
        presence.pose_of(world, PLAYER).unwrap().position,
        aboard_at.position
    );
}

#[test]
fn a_pose_in_the_frame_of_a_ship_that_is_not_there_is_refused_and_changes_nothing() {
    let mut rig = Rig::new();
    let missing = GridId::new(GridId::FIRST_ALLOCATED + 40);
    let before = rig.presence().pose_of(rig.server.world(), PLAYER).unwrap();
    let outcome = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&outcome);
    rig.in_a_tick(move |world, presence| {
        *slot.lock().unwrap() = Some(presence.set_pose_in(
            world,
            PLAYER,
            missing,
            looking(at([0, 0, 0], [3.0; 3])),
            Orientation::IDENTITY,
        ));
    });
    assert_eq!(
        outcome.lock().unwrap().take(),
        Some(Err(EntityError::NoSuchGrid(missing)))
    );
    assert_eq!(
        rig.presence().pose_of(rig.server.world(), PLAYER).unwrap(),
        before
    );
}

#[test]
fn a_player_who_is_away_aboard_stays_in_the_frame_of_the_ship_and_comes_back_aboard() {
    let mut rig = Rig::new();
    let ship = make_ship(&mut rig, quarter_turned_ship());
    let pose = looking(at([0, 0, 0], [10.0, 0.0, 0.0]));
    rig.in_a_tick(move |world, presence| {
        presence
            .set_pose_in(world, PLAYER, ship, pose, Orientation::IDENTITY)
            .unwrap();
    });
    // The player leaves: the entity stays in the world, hidden, in the frame of the ship.
    rig.connection
        .send(ClientMessage::Goodbye.encode())
        .unwrap();
    rig.server.step();
    let entity = {
        let world = rig.server.world();
        world
            .entities()
            .iter()
            .find(|entity| entity.pose.grid == ship)
            .map(|entity| entity.id)
            .expect("the entity of the player who left is still aboard")
    };
    // The ship moves while they are away.
    rig.in_a_tick(move |world, _| {
        world
            .set_grid_pose(ship, quarter_turned_ship_moved())
            .unwrap();
    });
    assert_eq!(
        rig.server
            .world()
            .entities()
            .get(entity)
            .unwrap()
            .pose
            .position,
        pose.position,
        "their place in the ship did not change"
    );
    // The same player comes back, as a new connection: they take over the entity, still aboard, now wherever the ship is.
    let mut again = rig.connector.connect().unwrap();
    log_in(&mut rig.server, &mut again, &id("test_game:game"), 1);
    let back = ClientId::new(2);
    let presence = rig.presence();
    let world = rig.server.world();
    assert_eq!(presence.entity_of(back), Some(entity));
    assert_eq!(presence.frame_of(world, back), Some(ship));
    assert_eq!(presence.local_pose_of(world, back).unwrap().1, pose);
    let placed = quarter_turned_ship_moved()
        .place_in_world(pose.position, Orientation::IDENTITY)
        .unwrap()
        .0;
    assert_eq!(presence.pose_of(world, back).unwrap().position, placed);
    drop(presence);
    // They were told where they are in the world, whatever the ship has done since, so that their view goes there.
    rig.server.step();
    let mut told = Vec::new();
    while let Some(frame) = again.receive().unwrap_or(None) {
        if let ServerMessage::GameMessage { kind, payload } = ServerMessage::decode(&frame).unwrap()
            && kind == PlayerPlaced::kind()
        {
            told.push(PlayerPlaced::decode(&payload).unwrap().0);
        }
    }
    assert_eq!(told, [placed]);
}

/// Where the ship is after it moved while the player was away.
fn quarter_turned_ship_moved() -> GridPose {
    GridPose::new(at([9, 1, 0], [0.0; 3]), quarter_turned_ship().orientation)
}

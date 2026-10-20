//! The players plugin, run by the plugin host on a real server with the pretend game of
//! `world_storage::test_support`.

use entities::Inventory;
use namespaced_id::NamespacedId;
use player_presence::{
    PLAYER_INVENTORY_SLOTS, PlayerClient, PlayerKeyComponent, PlayerLook, PlayerPose,
    PlayersPlugin, PoseReport, SharedPresence, lock, player_kind,
};
use plugin_host::{Phase, Plugin, PluginHost, Registrar, SystemSpec};
use protocol::{ClientId, ClientMessage, EntityId, Message, ServerMessage};
use server::test_support::{log_in, test_identity};
use server::{Server, ServerConfig};
use transport::Connection;
use transport::loopback::{LoopbackConnection, LoopbackConnector, loopback_listener};
use voxel_space::{BrickBox, BrickCoordinate, GridId, LevelIndex, Orientation, WorldPosition};
use world_storage::test_support::{Game, Plugin as FixturePlugin, id, sample_world};

struct Client {
    connection: LoopbackConnection,
}

impl Client {
    fn send(&mut self, message: &ClientMessage) {
        self.connection.send(message.encode()).unwrap();
    }

    fn received(&mut self) -> Vec<ServerMessage> {
        let mut messages = Vec::new();
        while let Some(frame) = self.connection.receive().unwrap_or(None) {
            messages.push(ServerMessage::decode(&frame).unwrap());
        }
        messages
    }

    /// Asks to be told of what is in the bricks around the origin of the main level.
    fn watch_origin(&mut self) {
        self.send(&ClientMessage::SetInterest {
            level: LevelIndex::MAIN,
            region: BrickBox::around(BrickCoordinate::ORIGIN, 1),
        });
    }
}

struct Rig {
    server: Server,
    connector: LoopbackConnector,
    client: Client,
    presence: SharedPresence,
}

fn pose(x: f32) -> PlayerPose {
    PlayerPose {
        position: WorldPosition::new(BrickCoordinate::new(0, 0, 0), [x, 5.0, 5.0]),
        yaw: 0.5,
        pitch: 0.0,
    }
}

/// A server with the players plugin and these others, and one client who has joined (as test identity 1).
fn rig(
    accept_reported_poses: bool,
    others: impl FnOnce(&SharedPresence) -> Vec<Box<dyn Plugin>>,
) -> Rig {
    let game = Game::new(&[FixturePlugin::BaseBlocks]);
    let world = sample_world(&game);
    let players = PlayersPlugin::new(accept_reported_poses);
    let presence = players.presence();
    let mut plugins: Vec<Box<dyn Plugin>> = vec![Box::new(players)];
    plugins.extend(others(&presence));
    let host = PluginHost::new(plugins, &world).unwrap();
    let (listener, connector) = loopback_listener();
    let mut server = Server::new(
        ServerConfig::new(id("test_game:game")).with_counted_challenges(1),
        world,
        &game.registries(),
        Box::new(host),
        Box::new(listener),
    )
    .unwrap();
    let mut client = Client {
        connection: connector.connect().unwrap(),
    };
    log_in(
        &mut server,
        &mut client.connection,
        &id("test_game:game"),
        1,
    );
    Rig {
        server,
        connector,
        client,
        presence,
    }
}

fn the_client(rig: &Rig) -> ClientId {
    rig.server
        .connected_clients()
        .next()
        .expect("a client has joined")
}

fn report(rig: &mut Rig, pose: &PlayerPose) {
    rig.client.send(&ClientMessage::GameMessage {
        kind: PoseReport::kind(),
        payload: PoseReport(*pose).encode(),
    });
    rig.server.step();
}

/// The entity messages among these, in the order they came.
fn entity_messages(messages: &[ServerMessage]) -> Vec<&ServerMessage> {
    messages
        .iter()
        .filter(|message| {
            matches!(
                message,
                ServerMessage::EntitiesSpawned(_)
                    | ServerMessage::EntitiesChanged(_)
                    | ServerMessage::EntitiesDespawned(_)
            )
        })
        .collect()
}

#[test]
fn a_reported_pose_is_taken_in_only_when_the_game_believes_what_players_say() {
    for believes in [true, false] {
        let mut rig = rig(believes, |_| Vec::new());
        report(&mut rig, &pose(3.0));
        let client = the_client(&rig);
        assert_eq!(
            lock(&rig.presence).pose_of(rig.server.world(), client),
            believes.then(|| pose(3.0)),
            "believes: {believes}"
        );
    }
}

#[test]
fn a_player_is_an_entity_with_the_pose_the_look_and_the_connection_and_the_key_stays_home() {
    let mut rig = rig(true, |_| Vec::new());
    rig.client.watch_origin();
    rig.server.step();
    let _ = rig.client.received();
    report(&mut rig, &pose(3.0));
    let told = rig.client.received();
    let [ServerMessage::EntitiesSpawned(spawned)] = entity_messages(&told)[..] else {
        panic!("{told:?}");
    };
    let [player] = &spawned[..] else {
        panic!("{spawned:?}");
    };
    assert_eq!(player.kind, player_kind());
    assert_eq!(player.pose.grid, GridId::MAIN);
    assert_eq!(player.pose.position, pose(3.0).position);
    assert_eq!(
        player.pose.orientation,
        Orientation::from_yaw_pitch(0.5, 0.0)
    );
    // What clients are told: who the player is on this server and where they look, not the key.
    let told_of: Vec<_> = player
        .components
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    // (In order of identifier.)
    assert_eq!(
        told_of,
        [Inventory::kind(), PlayerClient::kind(), PlayerLook::kind()]
    );
    let told = |name: NamespacedId| {
        player
            .components
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, bytes)| bytes.clone())
            .unwrap()
    };
    assert_eq!(
        told(PlayerClient::kind()),
        PlayerClient(the_client(&rig)).encode()
    );
    assert_eq!(
        told(PlayerLook::kind()),
        PlayerLook {
            yaw: 0.5,
            pitch: 0.0
        }
        .encode()
    );
    // An empty inventory with the slots a player has.
    assert_eq!(
        told(Inventory::kind()),
        Inventory::new(PLAYER_INVENTORY_SLOTS).encode()
    );
    // The server keeps the key with the entity.
    let entity = rig
        .server
        .world()
        .entities()
        .get(player.id)
        .expect("the entity is in the world");
    assert_eq!(
        entity.components.get::<PlayerKeyComponent>(),
        Ok(Some(PlayerKeyComponent(test_identity(1).player_key())))
    );
}

#[test]
fn a_pose_that_changed_is_announced_once_and_not_again_while_it_holds() {
    let mut rig = rig(true, |_| Vec::new());
    rig.client.watch_origin();
    report(&mut rig, &pose(3.0));
    let _ = rig.client.received();
    let announced = |messages: Vec<ServerMessage>| entity_messages(&messages).len();
    rig.server.step();
    rig.server.step();
    assert_eq!(announced(rig.client.received()), 0);
    report(&mut rig, &pose(4.0));
    assert_eq!(announced(rig.client.received()), 1);
    // Saying the same thing again changes nothing, and so tells nobody.
    report(&mut rig, &pose(4.0));
    assert_eq!(announced(rig.client.received()), 0);
}

#[test]
fn what_an_earlier_phase_moved_is_announced_in_the_same_tick() {
    // A system that puts every player at x = 9 in the `Physics` phase, as a game that moves players does.
    let mut rig = rig(false, |presence| {
        struct Mover(SharedPresence);
        impl Plugin for Mover {
            fn id(&self) -> namespaced_id::NamespacedId {
                id("test:mover")
            }
            fn depends_on(&self) -> Vec<namespaced_id::NamespacedId> {
                vec![id("player_presence:players")]
            }
            fn register(&self, registrar: &mut Registrar<'_>) {
                let presence = std::sync::Arc::clone(&self.0);
                registrar.system(
                    SystemSpec::new("test:move", Phase::Physics),
                    move |context| {
                        let clients: Vec<ClientId> = context.world().clients().collect();
                        for client in clients {
                            lock(&presence).set_pose(context.world(), client, pose(9.0));
                        }
                    },
                );
            }
        }
        vec![Box::new(Mover(std::sync::Arc::clone(presence)))]
    });
    // (The tick in which the client joined has run the mover; it watches from the next.)
    rig.client.watch_origin();
    rig.server.step();
    let told = rig.client.received();
    let [ServerMessage::EntitiesSpawned(spawned)] = entity_messages(&told)[..] else {
        panic!("{told:?}");
    };
    assert_eq!(spawned.len(), 1);
    assert_eq!(spawned[0].pose.position, pose(9.0).position);
}

#[test]
fn the_level_command_answers_and_a_player_who_leaves_is_hidden_and_forgotten_by_the_table() {
    let mut rig = rig(true, |_| Vec::new());
    rig.client.watch_origin();
    report(&mut rig, &pose(3.0));
    let client = the_client(&rig);
    let entity = lock(&rig.presence)
        .entity_of(client)
        .expect("the player has an entity");
    let _ = rig.client.received();
    rig.client.send(&ClientMessage::Command {
        text: "/level".to_owned(),
    });
    rig.server.step();
    let replies: Vec<String> = rig
        .client
        .received()
        .into_iter()
        .filter_map(|message| match message {
            ServerMessage::CommandReply { text } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(replies.len(), 1);
    assert!(replies[0].starts_with("you are in "), "{replies:?}");
    // An unknown command lists what the plugin offers.
    rig.client.send(&ClientMessage::Command {
        text: "/nonsense".to_owned(),
    });
    rig.server.step();
    let replies: Vec<String> = rig
        .client
        .received()
        .into_iter()
        .filter_map(|message| match message {
            ServerMessage::CommandReply { text } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(
        replies,
        ["unknown command; try /help [COMMAND], /level [SUBCOMMAND...]"]
    );
    // Leaving: the table forgets the connection; the entity stays in the world, hidden.
    rig.client.send(&ClientMessage::Goodbye);
    rig.server.step();
    assert_eq!(
        lock(&rig.presence).pose_of(rig.server.world(), client),
        None
    );
    let kept = rig
        .server
        .world()
        .entities()
        .get(entity)
        .expect("the player's entity is kept");
    assert!(kept.is_hidden());
}

#[test]
fn others_are_told_when_a_player_leaves_and_when_they_come_back() {
    let mut rig = rig(true, |_| Vec::new());
    let mut watcher = Client {
        connection: rig.connector.connect().unwrap(),
    };
    log_in(
        &mut rig.server,
        &mut watcher.connection,
        &id("test_game:game"),
        2,
    );
    watcher.watch_origin();
    report(&mut rig, &pose(3.0));
    let told = watcher.received();
    let [ServerMessage::EntitiesSpawned(spawned)] = entity_messages(&told)[..] else {
        panic!("{told:?}");
    };
    let player: EntityId = spawned[0].id;

    rig.client.send(&ClientMessage::Goodbye);
    rig.server.step();
    let told = watcher.received();
    assert!(
        matches!(
            entity_messages(&told)[..],
            [ServerMessage::EntitiesDespawned(ids)] if ids == &[player]
        ),
        "{told:?}"
    );

    // The same install comes back and, before saying anything, is where it was.
    let mut again = Client {
        connection: rig.connector.connect().unwrap(),
    };
    log_in(
        &mut rig.server,
        &mut again.connection,
        &id("test_game:game"),
        1,
    );
    rig.server.step();
    let told = watcher.received();
    let [ServerMessage::EntitiesSpawned(spawned)] = entity_messages(&told)[..] else {
        panic!("{told:?}");
    };
    assert_eq!(spawned[0].id, player, "the same entity, not a new one");
    assert_eq!(spawned[0].pose.position, pose(3.0).position);
    let new_client = rig
        .server
        .connected_clients()
        .find(|client| lock(&rig.presence).entity_of(*client) == Some(player))
        .expect("the returning player has the saved entity");
    assert_eq!(
        spawned[0]
            .components
            .iter()
            .find(|(name, _)| *name == PlayerClient::kind())
            .map(|(_, bytes)| bytes.clone())
            .unwrap(),
        PlayerClient(new_client).encode(),
        "told the connection it has now"
    );
}

#[test]
fn another_key_is_another_player_even_in_the_same_place() {
    let mut rig = rig(true, |_| Vec::new());
    let mut other = Client {
        connection: rig.connector.connect().unwrap(),
    };
    log_in(
        &mut rig.server,
        &mut other.connection,
        &id("test_game:game"),
        2,
    );
    report(&mut rig, &pose(3.0));
    other.send(&ClientMessage::GameMessage {
        kind: PoseReport::kind(),
        payload: PoseReport(pose(3.0)).encode(),
    });
    rig.server.step();
    let players: Vec<EntityId> = rig
        .server
        .world()
        .entities()
        .iter()
        .filter(|entity| entity.kind == player_kind())
        .map(|entity| entity.id)
        .collect();
    assert_eq!(players.len(), 2);
}

#[test]
fn a_different_key_does_not_take_over_the_player_a_key_left_behind() {
    let mut rig = rig(true, |_| Vec::new());
    report(&mut rig, &pose(3.0));
    let first = lock(&rig.presence)
        .entity_of(the_client(&rig))
        .expect("the first player has an entity");
    rig.client.send(&ClientMessage::Goodbye);
    rig.server.step();

    // Another install joins: it is nobody's entity until its own first pose, and the saved one stays hidden.
    let mut other = Client {
        connection: rig.connector.connect().unwrap(),
    };
    log_in(
        &mut rig.server,
        &mut other.connection,
        &id("test_game:game"),
        2,
    );
    let newcomer = the_client(&rig);
    assert_eq!(lock(&rig.presence).entity_of(newcomer), None);
    assert!(
        rig.server
            .world()
            .entities()
            .get(first)
            .unwrap()
            .is_hidden()
    );
    other.send(&ClientMessage::GameMessage {
        kind: PoseReport::kind(),
        payload: PoseReport(pose(8.0)).encode(),
    });
    rig.server.step();
    let theirs = lock(&rig.presence)
        .entity_of(newcomer)
        .expect("their first pose made their entity");
    assert_ne!(theirs, first);
    assert!(
        rig.server
            .world()
            .entities()
            .get(first)
            .unwrap()
            .is_hidden()
    );
}

#[test]
fn a_later_pose_changes_the_look_and_the_orientation_with_it() {
    let mut rig = rig(true, |_| Vec::new());
    report(&mut rig, &pose(3.0));
    let turned = PlayerPose {
        yaw: 1.25,
        pitch: -0.5,
        ..pose(3.0)
    };
    report(&mut rig, &turned);
    let client = the_client(&rig);
    assert_eq!(
        lock(&rig.presence).pose_of(rig.server.world(), client),
        Some(turned),
        "the angles the player reported, exactly"
    );
    let entity = lock(&rig.presence).entity_of(client).unwrap();
    assert_eq!(
        rig.server
            .world()
            .entities()
            .get(entity)
            .unwrap()
            .pose
            .orientation,
        Orientation::from_yaw_pitch(1.25, -0.5)
    );
}

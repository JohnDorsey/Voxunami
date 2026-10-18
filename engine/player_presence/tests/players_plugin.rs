//! The players plugin, run by the plugin host on a real server with the pretend game of
//! `world_storage::test_support`.

use player_presence::{PlayerPose, Players, PlayersPlugin, PoseReport, SharedPoseBook, lock};
use plugin_host::{Phase, Plugin, PluginHost, Registrar, SystemSpec};
use protocol::{ClientId, ClientMessage, Message, PROTOCOL_VERSION, ServerMessage};
use server::{Server, ServerConfig};
use transport::Connection;
use transport::loopback::{LoopbackConnection, loopback_listener};
use voxel_space::{BrickCoordinate, WorldPosition};
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
}

struct Rig {
    server: Server,
    client: Client,
    book: SharedPoseBook,
}

fn pose(x: f32) -> PlayerPose {
    PlayerPose {
        position: WorldPosition::new(BrickCoordinate::new(0, 0, 0), [x, 5.0, 5.0]),
        yaw: 0.5,
        pitch: 0.0,
    }
}

/// A server with the players plugin and these others, and one client who has joined.
fn rig(
    accept_reported_poses: bool,
    others: impl FnOnce(&SharedPoseBook) -> Vec<Box<dyn Plugin>>,
) -> Rig {
    let game = Game::new(&[FixturePlugin::BaseBlocks]);
    let world = sample_world(&game);
    let players = PlayersPlugin::new(accept_reported_poses);
    let book = players.book();
    let mut plugins: Vec<Box<dyn Plugin>> = vec![Box::new(players)];
    plugins.extend(others(&book));
    let host = PluginHost::new(plugins, &world).unwrap();
    let (listener, connector) = loopback_listener();
    let mut server = Server::new(
        ServerConfig::new(id("test_game:game")),
        world,
        &game.registries(),
        Box::new(host),
        Box::new(listener),
    )
    .unwrap();
    let mut client = Client {
        connection: connector.connect().unwrap(),
    };
    client.send(&ClientMessage::Hello {
        protocol_version: PROTOCOL_VERSION,
        game: id("test_game:game"),
    });
    server.step();
    Rig {
        server,
        client,
        book,
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

#[test]
fn a_reported_pose_is_taken_in_only_when_the_game_believes_what_players_say() {
    for believes in [true, false] {
        let mut rig = rig(believes, |_| Vec::new());
        report(&mut rig, &pose(3.0));
        let client = the_client(&rig);
        assert_eq!(
            lock(&rig.book).pose_of(client).copied(),
            believes.then(|| pose(3.0)),
            "believes: {believes}"
        );
    }
}

#[test]
fn a_pose_that_changed_is_announced_to_everyone_once_and_not_again_while_it_holds() {
    let mut rig = rig(true, |_| Vec::new());
    let _ = rig.client.received();
    report(&mut rig, &pose(3.0));
    let announced = |messages: Vec<ServerMessage>| -> usize {
        messages
            .into_iter()
            .filter(|message| matches!(message, ServerMessage::GameMessage { kind, .. } if *kind == Players::kind()))
            .count()
    };
    assert_eq!(announced(rig.client.received()), 1);
    rig.server.step();
    rig.server.step();
    assert_eq!(announced(rig.client.received()), 0);
    report(&mut rig, &pose(4.0));
    assert_eq!(announced(rig.client.received()), 1);
}

#[test]
fn what_an_earlier_phase_moved_is_announced_in_the_same_tick() {
    // A system that puts every player at x = 9 in the `Physics` phase, as a game that moves players does.
    let mut rig = rig(false, |book| {
        let book = std::sync::Arc::clone(book);
        struct Mover(SharedPoseBook);
        impl Plugin for Mover {
            fn id(&self) -> namespaced_id::NamespacedId {
                id("test:mover")
            }
            fn depends_on(&self) -> Vec<namespaced_id::NamespacedId> {
                vec![id("player_presence:players")]
            }
            fn register(&self, registrar: &mut Registrar<'_>) {
                let book = std::sync::Arc::clone(&self.0);
                registrar.system(
                    SystemSpec::new("test:move", Phase::Physics),
                    move |context| {
                        let clients: Vec<ClientId> = context.world().clients().collect();
                        for client in clients {
                            lock(&book).set_pose(client, pose(9.0));
                        }
                    },
                );
            }
        }
        vec![Box::new(Mover(book))]
    });
    // (The tick in which the client joined has run the mover and the announcement already.)
    let announced: Vec<_> = rig
        .client
        .received()
        .into_iter()
        .filter_map(|message| match message {
            ServerMessage::GameMessage { kind, payload } if kind == Players::kind() => {
                Some(Players::decode(&payload).unwrap().0)
            }
            _ => None,
        })
        .collect();
    assert_eq!(announced.len(), 1);
    assert_eq!(announced[0].len(), 1);
    assert_eq!(announced[0][0].1.pose, pose(9.0));
}

#[test]
fn the_level_command_answers_and_a_player_who_leaves_is_forgotten() {
    let mut rig = rig(true, |_| Vec::new());
    report(&mut rig, &pose(3.0));
    let client = the_client(&rig);
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
    // Leaving.
    rig.client.send(&ClientMessage::Goodbye);
    rig.server.step();
    assert_eq!(lock(&rig.book).pose_of(client), None);
}

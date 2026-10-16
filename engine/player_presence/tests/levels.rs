//! Which level each player is in, and the `/level` command that moves them, checked through a real
//! server with the pretend game of `world_storage::test_support`.

use std::sync::{Arc, Mutex};

use namespaced_id::NamespacedId;
use player_presence::{
    PlayerPose, PoseBook, decode_level_change, decode_players, encode_pose, level_kind,
    players_kind, pose_kind,
};
use protocol::{ClientId, ClientMessage, PROTOCOL_VERSION, ServerMessage};
use server::test_support::ScriptedLogic;
use server::{Server, ServerConfig};
use transport::Connection;
use transport::loopback::{LoopbackConnection, LoopbackConnector, loopback_listener};
use voxel_space::{BrickCoordinate, LevelIndex, WorldPosition};
use world_storage::test_support::{ALL_PLUGINS, Game, id};

fn game_id() -> NamespacedId {
    id("test_game:game")
}

struct Client {
    connection: LoopbackConnection,
}

impl Client {
    fn send(&mut self, message: &ClientMessage) {
        self.connection.send(message.encode()).unwrap();
    }

    fn command(&mut self, text: &str) {
        self.send(&ClientMessage::Command {
            text: text.to_owned(),
        });
    }

    fn pose_at(&mut self, x: f32) {
        let pose = PlayerPose {
            position: WorldPosition::new(BrickCoordinate::ORIGIN, [x, 1.0, 1.0]),
            yaw: 0.0,
            pitch: 0.0,
        };
        self.send(&ClientMessage::GameMessage {
            kind: pose_kind(),
            payload: encode_pose(&pose),
        });
    }

    fn received(&mut self) -> Vec<ServerMessage> {
        let mut messages = Vec::new();
        while let Some(frame) = self.connection.receive().unwrap_or(None) {
            messages.push(ServerMessage::decode(&frame).unwrap());
        }
        messages
    }

    /// The text of every command reply received.
    fn replies(&mut self) -> Vec<String> {
        self.received()
            .into_iter()
            .filter_map(|message| match message {
                ServerMessage::CommandReply { text } => Some(text),
                _ => None,
            })
            .collect()
    }
}

struct Rig {
    server: Server,
    connector: LoopbackConnector,
    book: Arc<Mutex<PoseBook>>,
}

impl Rig {
    /// A server of a world with the levels `test_content:main`, `test_content:second` and
    /// `other_game:second`, whose logic is a pose book and nothing else but the command `/ping`.
    fn new() -> Self {
        let game = Game::new(&ALL_PLUGINS);
        let mut world = game.new_world();
        for name in ["test_content:second", "other_game:second"] {
            world
                .add_level(id(name), game.new_level("base_terrain:flat_v1"))
                .unwrap();
        }
        let book = Arc::new(Mutex::new(PoseBook::new()));
        let logic = ScriptedLogic::new()
            .on_message({
                let book = Arc::clone(&book);
                move |client, kind, payload, world| {
                    let mut book = book.lock().unwrap();
                    if *kind == id("test_game:teleport") {
                        book.move_to_level(client, LevelIndex::new(u32::from(payload[0])), world);
                    } else {
                        book.handle_message(client, kind, payload);
                    }
                }
            })
            .on_command({
                let book = Arc::clone(&book);
                move |client, text, world| {
                    if !book.lock().unwrap().handle_command(client, text, world) {
                        world.reply(client, "not a level command");
                    }
                }
            })
            .on_tick({
                let book = Arc::clone(&book);
                move |_, world| book.lock().unwrap().tell_everyone(world)
            });
        let (listener, connector) = loopback_listener();
        let server = Server::new(
            ServerConfig::new(game_id()),
            world,
            &game.registries(),
            Box::new(logic),
            Box::new(listener),
        )
        .unwrap();
        Self {
            server,
            connector,
            book,
        }
    }

    fn join(&mut self) -> Client {
        let mut client = Client {
            connection: self.connector.connect().unwrap(),
        };
        client.send(&ClientMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            game: game_id(),
        });
        self.server.step();
        client.received();
        client
    }

    fn level_of(&self, client: u32) -> LevelIndex {
        self.book.lock().unwrap().level_of(ClientId::new(client))
    }
}

/// The level of each player in the last snapshot of where everyone is.
fn levels_told(messages: &[ServerMessage]) -> Option<Vec<(u32, u32)>> {
    messages.iter().rev().find_map(|message| match message {
        ServerMessage::GameMessage { kind, payload } if *kind == players_kind() => Some(
            decode_players(payload)
                .unwrap()
                .into_iter()
                .map(|(client, state)| (client.get(), state.level.get()))
                .collect(),
        ),
        _ => None,
    })
}

fn level_changes(messages: &[ServerMessage]) -> Vec<NamespacedId> {
    messages
        .iter()
        .filter_map(|message| match message {
            ServerMessage::GameMessage { kind, payload } if *kind == level_kind() => {
                Some(decode_level_change(payload).unwrap())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn players_start_in_the_main_level_and_the_snapshot_says_which_level_each_is_in() {
    let mut rig = Rig::new();
    let (mut first, mut second) = (rig.join(), rig.join());
    first.pose_at(1.0);
    second.pose_at(2.0);
    rig.server.step();
    assert_eq!(rig.level_of(1), LevelIndex::MAIN);
    assert_eq!(levels_told(&first.received()), Some(vec![(1, 0), (2, 0)]));

    second.command("/level go test_content:second");
    rig.server.step();
    assert_eq!(rig.level_of(2), LevelIndex::new(1));
    assert_eq!(rig.level_of(1), LevelIndex::MAIN);
    assert_eq!(levels_told(&first.received()), Some(vec![(1, 0), (2, 1)]));
}

#[test]
fn level_with_no_argument_and_list_tell_where_you_are_and_what_there_is() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    player.command("/level");
    player.command("level list");
    rig.server.step();
    assert_eq!(
        player.replies(),
        [
            "you are in test_content:main",
            "levels: test_content:main (you are here), test_content:second, other_game:second",
        ]
    );
}

#[test]
fn going_to_a_level_moves_the_player_tells_them_by_name_and_replies() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    // By whole identifier.
    player.command("/level go other_game:second");
    rig.server.step();
    let messages = player.received();
    assert_eq!(level_changes(&messages), [id("other_game:second")]);
    assert!(messages.iter().any(|message| matches!(
        message,
        ServerMessage::CommandReply { text } if text == "you are now in other_game:second"
    )));
    assert_eq!(rig.level_of(1), LevelIndex::new(2));

    // By the part after the colon, when only one level has it.
    player.command("/level go main");
    rig.server.step();
    assert_eq!(level_changes(&player.received()), [id("test_content:main")]);
    assert_eq!(rig.level_of(1), LevelIndex::MAIN);

    // Already there: nothing moves, nothing is sent.
    player.command("/level go main");
    rig.server.step();
    let messages = player.received();
    assert_eq!(level_changes(&messages), []);
    assert!(messages.iter().any(|message| matches!(
        message,
        ServerMessage::CommandReply { text } if text == "you are already in test_content:main"
    )));
}

#[test]
fn a_level_that_does_not_exist_or_a_name_that_means_two_is_explained_and_nothing_moves() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    player.command("/level go nowhere");
    player.command("/level go second");
    player.command("/level go test_content:nowhere");
    player.command("/level fly");
    player.command("/level go");
    rig.server.step();
    assert_eq!(
        player.replies(),
        [
            "there is no level nowhere; /level list shows them",
            "second could mean several levels; use the whole name",
            "there is no level test_content:nowhere; /level list shows them",
            "usage: /level, /level list, /level go NAME",
            "usage: /level, /level list, /level go NAME",
        ]
    );
    assert_eq!(rig.level_of(1), LevelIndex::MAIN);
}

#[test]
fn other_commands_are_left_for_the_game() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    player.command("/ping");
    player.command("");
    player.command("/");
    rig.server.step();
    assert_eq!(player.replies(), ["not a level command"; 3]);
}

#[test]
fn a_game_can_move_players_itself_but_only_to_levels_the_world_has() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    let teleport = |player: &mut Client, level: u8| {
        player.send(&ClientMessage::GameMessage {
            kind: id("test_game:teleport"),
            payload: vec![level],
        });
    };
    teleport(&mut player, 1);
    rig.server.step();
    assert_eq!(rig.level_of(1), LevelIndex::new(1));
    assert_eq!(
        level_changes(&player.received()),
        [id("test_content:second")]
    );

    teleport(&mut player, 9);
    rig.server.step();
    assert_eq!(rig.level_of(1), LevelIndex::new(1), "there is no level 9");
    assert_eq!(level_changes(&player.received()), []);
}

#[test]
fn a_player_who_leaves_is_forgotten_by_the_book() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    player.pose_at(1.0);
    player.command("/level go test_content:second");
    rig.server.step();
    assert_eq!(rig.level_of(1), LevelIndex::new(1));
    assert_eq!(rig.book.lock().unwrap().len(), 1);
    rig.book.lock().unwrap().client_left(ClientId::new(1));
    assert_eq!(rig.level_of(1), LevelIndex::MAIN);
    assert!(rig.book.lock().unwrap().is_empty());
}

//! Which level each player is in, and the `/level` command that moves them, checked through a real
//! server with the pretend game of `world_storage::test_support`.

use std::sync::{Arc, Mutex};

use entities::EntityCatalog;
use namespaced_id::NamespacedId;
use player_presence::{
    LevelChange, PlayerClient, PlayerPose, PoseReport, Presence, player_kind_spec,
};
use protocol::{ClientId, ClientMessage, EntityId, Message, ServerMessage};
use server::test_support::{ScriptedLogic, log_in};
use server::{Server, ServerConfig};
use transport::Connection;
use transport::loopback::{LoopbackConnection, LoopbackConnector, loopback_listener};
use voxel_space::{BrickBox, BrickCoordinate, GridId, LevelIndex, WorldPosition};
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

    /// Asks to be told of what is in the bricks around the origin of this level.
    fn watch(&mut self, level: LevelIndex) {
        self.send(&ClientMessage::SetInterest {
            level,
            region: BrickBox::around(BrickCoordinate::ORIGIN, 1),
        });
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
            kind: PoseReport::kind(),
            payload: PoseReport(pose).encode(),
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
    presence: Arc<Mutex<Presence>>,
    /// How many clients have joined: each logs in as an identity of its own.
    joined: u8,
}

impl Rig {
    /// A server of a world with the levels `test_content:main`, `test_content:second` and
    /// `other_game:second`, whose logic is the players' presence and nothing else but the command `/ping`.
    fn new() -> Self {
        let game = Game::new(&ALL_PLUGINS);
        let mut world = game.new_world();
        for name in ["test_content:second", "other_game:second"] {
            world
                .add_level(id(name), game.new_level("base_terrain:flat_v1"))
                .unwrap();
        }
        let mut catalog = EntityCatalog::default();
        catalog
            .kinds
            .register(player_kind_spec())
            .expect("the player kind is new");
        let presence = Arc::new(Mutex::new(Presence::new()));
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
                    let mut presence = presence.lock().unwrap();
                    if *kind == id("test_game:teleport") {
                        presence.move_to_level(
                            client,
                            LevelIndex::new(u32::from(payload[0])),
                            world,
                        );
                    } else {
                        presence.handle_message(world, client, kind, payload);
                    }
                }
            })
            .on_command({
                let presence = Arc::clone(&presence);
                move |client, text, world| {
                    if !presence.lock().unwrap().handle_command(client, text, world) {
                        world.reply(client, "not a level command");
                    }
                }
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
            presence,
            joined: 0,
        }
    }

    fn join(&mut self) -> Client {
        let mut client = Client {
            connection: self.connector.connect().unwrap(),
        };
        self.joined += 1;
        log_in(
            &mut self.server,
            &mut client.connection,
            &game_id(),
            self.joined,
        );
        client.received();
        client
    }

    fn level_of(&self, client: u32) -> LevelIndex {
        self.presence
            .lock()
            .unwrap()
            .level_of(self.server.world(), ClientId::new(client))
    }
}

/// The players among the entities these messages tell of as new: their connection and their grid.
fn players_told(messages: &[ServerMessage]) -> Vec<(u32, GridId)> {
    messages
        .iter()
        .filter_map(|message| match message {
            ServerMessage::EntitiesSpawned(spawned) => Some(spawned),
            _ => None,
        })
        .flatten()
        .map(|entity| {
            let (_, bytes) = entity
                .components
                .iter()
                .find(|(name, _)| *name == PlayerClient::kind())
                .expect("a player is told whose it is");
            let client = PlayerClient::decode(bytes).unwrap();
            (client.0.get(), entity.pose.grid)
        })
        .collect()
}

/// The entities these messages tell of as gone.
fn despawned(messages: &[ServerMessage]) -> Vec<EntityId> {
    messages
        .iter()
        .filter_map(|message| match message {
            ServerMessage::EntitiesDespawned(ids) => Some(ids.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

fn level_changes(messages: &[ServerMessage]) -> Vec<NamespacedId> {
    messages
        .iter()
        .filter_map(|message| match message {
            ServerMessage::GameMessage { kind, payload } if *kind == LevelChange::kind() => {
                Some(LevelChange::decode(payload).unwrap().0)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn players_start_in_the_main_level_and_are_told_of_by_the_entities_of_the_level_they_are_in() {
    let mut rig = Rig::new();
    let (mut first, mut second) = (rig.join(), rig.join());
    first.pose_at(1.0);
    second.pose_at(2.0);
    rig.server.step();
    assert_eq!(rig.level_of(1), LevelIndex::MAIN);
    first.watch(LevelIndex::MAIN);
    rig.server.step();
    assert_eq!(
        players_told(&first.received()),
        [(1, GridId::MAIN), (2, GridId::MAIN)]
    );

    // The second moves to another level: the first, who watches the main level, is told they are gone, and
    // the entity's grid is the other level's root.
    let second_entity = rig
        .presence
        .lock()
        .unwrap()
        .entity_of(ClientId::new(2))
        .unwrap();
    second.command("/level go test_content:second");
    rig.server.step();
    assert_eq!(rig.level_of(2), LevelIndex::new(1));
    assert_eq!(rig.level_of(1), LevelIndex::MAIN);
    assert_eq!(despawned(&first.received()), [second_entity]);
    assert_eq!(
        rig.server
            .world()
            .entities()
            .get(second_entity)
            .unwrap()
            .pose
            .grid,
        GridId::root(LevelIndex::new(1))
    );

    // The presence lists the players who have said where they are, with their levels, in id order.
    let listed: Vec<_> = rig
        .presence
        .lock()
        .unwrap()
        .players(rig.server.world())
        .into_iter()
        .map(|(client, _, level)| (client.get(), level))
        .collect();
    assert_eq!(listed, [(1, LevelIndex::MAIN), (2, LevelIndex::new(1))]);

    // A player who arrives in that level is told of the one who is there.
    let mut third = rig.join();
    third.pose_at(3.0);
    third.command("/level go test_content:second");
    rig.server.step();
    third.watch(LevelIndex::new(1));
    rig.server.step();
    assert_eq!(
        players_told(&third.received()),
        [
            (2, GridId::root(LevelIndex::new(1))),
            (3, GridId::root(LevelIndex::new(1)))
        ]
    );
}

#[test]
fn a_level_asked_for_before_the_first_pose_is_where_the_first_pose_puts_the_player() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    player.command("/level go test_content:second");
    rig.server.step();
    assert_eq!(rig.level_of(1), LevelIndex::new(1));
    assert!(
        rig.presence
            .lock()
            .unwrap()
            .entity_of(ClientId::new(1))
            .is_none()
    );
    player.pose_at(5.0);
    rig.server.step();
    assert_eq!(rig.level_of(1), LevelIndex::new(1));
    let entity = rig
        .presence
        .lock()
        .unwrap()
        .entity_of(ClientId::new(1))
        .unwrap();
    assert_eq!(
        rig.server.world().entities().get(entity).unwrap().pose.grid,
        GridId::root(LevelIndex::new(1))
    );
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
fn a_player_who_leaves_is_forgotten_by_the_table_and_their_entity_stays_hidden_in_its_level() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    player.pose_at(1.0);
    player.command("/level go test_content:second");
    rig.server.step();
    assert_eq!(rig.level_of(1), LevelIndex::new(1));
    assert_eq!(rig.presence.lock().unwrap().len(), 1);
    let entity = rig
        .presence
        .lock()
        .unwrap()
        .entity_of(ClientId::new(1))
        .unwrap();
    player.send(&ClientMessage::Goodbye);
    rig.server.step();
    assert_eq!(rig.level_of(1), LevelIndex::MAIN);
    assert!(rig.presence.lock().unwrap().is_empty());
    let kept = rig.server.world().entities().get(entity).unwrap();
    assert!(kept.is_hidden());
    assert_eq!(kept.pose.grid, GridId::root(LevelIndex::new(1)));
}

#[test]
fn a_player_who_comes_back_in_the_level_they_left_is_told_which_level_it_is() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    player.pose_at(1.0);
    player.command("/level go test_content:second");
    rig.server.step();
    player.send(&ClientMessage::Goodbye);
    rig.server.step();

    // The same install (the same identity, the first this rig made) joins again.
    let mut again = Client {
        connection: rig.connector.connect().unwrap(),
    };
    log_in(&mut rig.server, &mut again.connection, &game_id(), 1);
    assert_eq!(
        level_changes(&again.received()),
        [id("test_content:second")],
        "told at once, before saying anything"
    );
    let new_client = rig.server.connected_clients().next().unwrap();
    assert_eq!(
        rig.presence
            .lock()
            .unwrap()
            .level_of(rig.server.world(), new_client),
        LevelIndex::new(1)
    );
}

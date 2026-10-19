//! What clients see of a server, checked through the loopback transport with the pretend game of
//! `world_storage::test_support`.

use std::sync::{Arc, Mutex};

use binary_codec::{ByteReader, DecodeError};
use entities::{ComponentBag, EntityCatalog, EntityKindSpec};
use namespaced_id::NamespacedId;
use protocol::{
    ClientId, ClientMessage, EntityChange, EntityId, Message, PROTOCOL_VERSION, ServerMessage, Tick,
};
use server::GridView;
use server::test_support::{LogicEvent, ScriptedLogic};
use server::{Audience, LeaveReason, Server, ServerConfig, StepReport};
use transport::loopback::{LoopbackConnection, LoopbackConnector, loopback_listener};
use transport::{Connection, ConnectionClosed};
use voxel_space::{
    BrickBox, BrickCoordinate, EntityPose, GridId, LevelIndex, Orientation, VoxelCoordinate,
    WorldPosition,
};
use world_storage::ParsedWorld;
use world_storage::WorldData;
use world_storage::test_support::{ALL_PLUGINS, Game, Plugin, brick, id, sample_world};

fn game_id() -> NamespacedId {
    id("test_game:game")
}

/// A client of the server under test: it speaks the protocol and remembers nothing else.
struct TestClient {
    connection: LoopbackConnection,
}

impl TestClient {
    fn send(&mut self, message: &ClientMessage) {
        self.connection.send(message.encode()).unwrap();
    }

    fn hello(&mut self) {
        self.send(&ClientMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            game: game_id(),
        });
    }

    fn watch(&mut self, center: BrickCoordinate, radius: u32) {
        self.send(&ClientMessage::SetInterest {
            level: LevelIndex::MAIN,
            region: BrickBox::around(center, radius),
        });
    }

    fn place(&mut self, at: [i8; 3], block: u8) {
        self.send(&ClientMessage::GameMessage {
            kind: id("test_game:set_block"),
            payload: vec![at[0] as u8, at[1] as u8, at[2] as u8, block],
        });
    }

    /// Everything the server has sent since the last call.
    fn received(&mut self) -> Vec<ServerMessage> {
        let mut messages = Vec::new();
        while let Some(frame) = self.connection.receive().unwrap_or(None) {
            messages.push(ServerMessage::decode(&frame).expect("the server sends valid frames"));
        }
        messages
    }

    fn raw_frames(&mut self) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        while let Some(frame) = self.connection.receive().unwrap_or(None) {
            frames.push(frame);
        }
        frames
    }
}

struct Harness {
    server: Server,
    connector: LoopbackConnector,
    events: Arc<Mutex<Vec<LogicEvent>>>,
}

impl Harness {
    /// A server whose game lets clients place blocks with `test_game:set_block` messages: three
    /// signed voxel coordinates and a block number (a raw id in the fixture's block registry).
    fn new(game: &Game, world: WorldData, config: ServerConfig) -> Self {
        let logic = ScriptedLogic::new().on_message(|_, kind, payload, world| {
            if *kind == id("test_game:set_block") {
                let channel = world
                    .data()
                    .main()
                    .overrides
                    .layout()
                    .channel_index(&id("core:block"))
                    .unwrap();
                let at = VoxelCoordinate::new(
                    i64::from(payload[0] as i8),
                    i64::from(payload[1] as i8),
                    i64::from(payload[2] as i8),
                );
                world
                    .set_voxel(LevelIndex::MAIN, channel, at, u32::from(payload[3]))
                    .unwrap();
            }
        });
        Self::with_logic(game, world, config, logic)
    }

    fn with_logic(
        game: &Game,
        world: WorldData,
        config: ServerConfig,
        logic: ScriptedLogic,
    ) -> Self {
        let (listener, connector) = loopback_listener();
        let events = logic.events();
        let server = Server::new(
            config,
            world,
            &game.registries(),
            Box::new(logic),
            Box::new(listener),
        )
        .unwrap();
        Self {
            server,
            connector,
            events,
        }
    }

    fn connect(&self) -> TestClient {
        TestClient {
            connection: self.connector.connect().unwrap(),
        }
    }

    fn logic_events(&self) -> Vec<LogicEvent> {
        self.events.lock().unwrap().clone()
    }
}

fn default_harness() -> (Game, Harness) {
    let game = Game::new(&ALL_PLUGINS);
    let harness = Harness::new(&game, game.new_world(), ServerConfig::new(game_id()));
    (game, harness)
}

fn block_of(game: &Game, name: &str) -> u8 {
    game.raw_block(name).try_into().unwrap()
}

#[test]
fn a_joining_client_is_welcomed_with_a_description_it_can_load() {
    let (_, mut harness) = default_harness();
    let mut client = harness.connect();
    client.hello();
    let report = harness.server.step();

    assert_eq!(report.tick, Tick::new(1));
    assert_eq!(report.joined, [ClientId::new(1)]);
    assert!(report.left.is_empty());
    let messages = client.received();
    let [
        ServerMessage::Welcome(welcome),
        ServerMessage::TickCompleted { tick },
    ] = &messages[..]
    else {
        panic!("expected a welcome and a tick, got {messages:?}");
    };
    assert_eq!(*tick, Tick::new(1));
    assert_eq!(welcome.protocol_version, PROTOCOL_VERSION);
    assert_eq!(welcome.game, game_id());
    assert_eq!(welcome.client, ClientId::new(1));
    assert_eq!(welcome.ticks_per_second, 20);
    assert_eq!(
        welcome.tick,
        Tick::ZERO,
        "the last tick completed before joining"
    );

    let mut receiver = Game::new(&ALL_PLUGINS);
    let parsed = ParsedWorld::parse(&welcome.world_description).unwrap();
    assert_eq!(parsed.world_seed(), harness.server.world().world_seed);
    let (described, report) = parsed.load(receiver.load_context()).unwrap();
    assert!(report.missing_content.is_empty());
    assert_eq!(described.main().overrides.brick_count(), 0);
    assert_eq!(
        harness.logic_events(),
        [
            LogicEvent::Joined(ClientId::new(1)),
            LogicEvent::Tick(Tick::new(1))
        ]
    );
}

#[test]
fn clients_get_ids_in_the_order_they_connected_and_each_gets_its_own_stream() {
    let (_, mut harness) = default_harness();
    let mut first = harness.connect();
    let mut second = harness.connect();
    second.hello();
    first.hello();
    harness.server.step();
    let welcome_of = |messages: Vec<ServerMessage>| match &messages[0] {
        ServerMessage::Welcome(welcome) => welcome.client,
        other => panic!("{other:?}"),
    };
    assert_eq!(welcome_of(first.received()), ClientId::new(1));
    assert_eq!(welcome_of(second.received()), ClientId::new(2));
    assert_eq!(
        harness.server.connected_clients().collect::<Vec<_>>(),
        [ClientId::new(1), ClientId::new(2)]
    );
}

fn refusal_cases() -> Vec<(&'static str, Vec<Vec<u8>>, &'static str)> {
    let hello = |protocol_version, game: &str| {
        ClientMessage::Hello {
            protocol_version,
            game: id(game),
        }
        .encode()
    };
    vec![
        (
            "wrong version",
            vec![hello(PROTOCOL_VERSION + 1, "test_game:game")],
            "protocol version",
        ),
        (
            "wrong game",
            vec![hello(PROTOCOL_VERSION, "other_game:game")],
            "other_game:game",
        ),
        ("garbage", vec![vec![200, 1, 2]], "malformed"),
        ("empty frame", vec![vec![]], "malformed"),
        (
            "not hello first",
            vec![
                ClientMessage::SetInterest {
                    level: LevelIndex::MAIN,
                    region: BrickBox::around(BrickCoordinate::ORIGIN, 1),
                }
                .encode(),
            ],
            "hello",
        ),
        (
            "hello twice",
            vec![
                hello(PROTOCOL_VERSION, "test_game:game"),
                hello(PROTOCOL_VERSION, "test_game:game"),
            ],
            "already",
        ),
    ]
}

#[test]
fn clients_that_break_the_protocol_are_told_why_and_dropped() {
    for (name, frames, expected_reason) in refusal_cases() {
        let (_, mut harness) = default_harness();
        let mut client = harness.connect();
        for frame in frames {
            client.connection.send(frame).unwrap();
        }
        let report = harness.server.step();
        let [(id_, LeaveReason::Refused(reason))] = &report.left[..] else {
            panic!("{name}: {report:?}");
        };
        assert_eq!(*id_, ClientId::new(1), "{name}");
        assert!(reason.contains(expected_reason), "{name}: {reason}");
        assert_eq!(harness.server.connected_clients().count(), 0, "{name}");
        let messages = client.received();
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, ServerMessage::Disconnected { reason } if reason.contains(expected_reason))),
            "{name}: {messages:?}"
        );
        assert_eq!(client.connection.receive(), Err(ConnectionClosed), "{name}");
        // A client that was refused before joining never reached the game.
        if name != "hello twice" {
            assert!(
                !harness
                    .logic_events()
                    .iter()
                    .any(|event| matches!(event, LogicEvent::Joined(_))),
                "{name}"
            );
        }
    }
}

#[test]
fn a_client_that_never_says_hello_is_dropped_after_the_timeout() {
    let game = Game::new(&ALL_PLUGINS);
    let mut config = ServerConfig::new(game_id());
    config.handshake_timeout_ticks = 3;
    let mut harness = Harness::new(&game, game.new_world(), config);
    let mut silent = harness.connect();
    for _ in 0..3 {
        assert!(harness.server.step().left.is_empty());
    }
    let report = harness.server.step();
    assert!(
        matches!(&report.left[..], [(_, LeaveReason::Refused(reason))] if reason.contains("hello"))
    );
    assert!(matches!(
        &silent.received()[..],
        [ServerMessage::Disconnected { .. }]
    ));
}

#[test]
fn watching_a_region_tells_the_client_which_generator_made_each_part() {
    let (_, mut harness) = default_harness();
    let mut client = harness.connect();
    client.hello();
    harness.server.step();
    client.received();

    client.watch(BrickCoordinate::ORIGIN, 1);
    harness.server.step();
    let messages = client.received();
    let [
        ServerMessage::InterestChanged {
            level: LevelIndex::MAIN,
            region,
            assignments,
        },
        ServerMessage::TickCompleted { tick },
    ] = &messages[..]
    else {
        panic!("{messages:?}");
    };
    assert_eq!(*region, BrickBox::around(BrickCoordinate::ORIGIN, 1));
    assert_eq!(*tick, Tick::new(2));
    // Bricks -1..=1 straddle the origin, so with cubes of 8 bricks the region touches 2 x 2 x 2 cubes.
    assert_eq!(assignments.len(), 8);
    let flat = harness
        .server
        .world()
        .main()
        .terrain
        .generators()
        .runtime_id(&id("base_terrain:flat_v1"))
        .unwrap();
    for assignment in assignments {
        assert_eq!(assignment.cube.level(), 3);
        assert_eq!(assignment.generator, Some(flat.raw()));
        assert!(
            harness
                .server
                .world()
                .main()
                .terrain
                .assigned_generator(assignment.cube.origin())
                .is_some(),
            "the server assigned it too"
        );
    }
}

#[test]
fn unedited_terrain_is_never_sent_and_edited_bricks_are_sent_when_they_come_into_view() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = game.new_world();
    game.set_block(&mut world, [1, 2, 3], "base_blocks:stone"); // brick (0, 0, 0)
    game.set_block(&mut world, [100, 0, 0], "fancy_blocks:marble"); // brick (3, 0, 0)
    game.set_block(&mut world, [3000, 0, 0], "fancy_blocks:glass"); // brick (93, 0, 0), far away
    let mut harness = Harness::new(&game, world, ServerConfig::new(game_id()));
    let mut client = harness.connect();
    client.hello();
    harness.server.step();
    client.received();

    client.watch(BrickCoordinate::ORIGIN, 1);
    harness.server.step();
    let messages = client.received();
    let sent_bricks: Vec<_> = messages
        .iter()
        .filter_map(|m| match m {
            ServerMessage::BrickContents {
                level: LevelIndex::MAIN,
                brick,
                ..
            } => Some(*brick),
            _ => None,
        })
        .collect();
    assert_eq!(sent_bricks, [brick(0, 0, 0)]);
    assert!(
        matches!(
            messages[0],
            ServerMessage::InterestChanged {
                level: LevelIndex::MAIN,
                ..
            }
        ),
        "the region change comes first"
    );

    // Moving the region right gains a slab; only the edited brick in the gained part is sent, and
    // the brick left behind is not mentioned again.
    client.watch(brick(3, 0, 0), 1);
    harness.server.step();
    let messages = client.received();
    let sent_bricks: Vec<_> = messages
        .iter()
        .filter_map(|m| match m {
            ServerMessage::BrickContents {
                level: LevelIndex::MAIN,
                brick,
                ..
            } => Some(*brick),
            _ => None,
        })
        .collect();
    assert_eq!(sent_bricks, [brick(3, 0, 0)]);
    let ServerMessage::InterestChanged {
        level: LevelIndex::MAIN,
        region,
        assignments,
    } = &messages[0]
    else {
        panic!("{messages:?}");
    };
    // Every newly watched brick has an assignment cube telling the client who generates it.
    for watched in region.bricks() {
        assert!(
            assignments.iter().any(|a| a.cube.contains(watched)),
            "{watched} has no assignment"
        );
    }

    // Asking for the same region again changes nothing.
    client.watch(brick(3, 0, 0), 1);
    harness.server.step();
    assert!(matches!(
        &client.received()[..],
        [ServerMessage::TickCompleted { .. }]
    ));
}

#[test]
fn the_contents_sent_are_exactly_the_servers_brick() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = game.new_world();
    game.set_block(&mut world, [1, 2, 3], "base_blocks:stone");
    game.set(&mut world, [1, 2, 3], "core:temperature", 77);
    let expected = Arc::clone(world.main().overrides.brick(brick(0, 0, 0)).unwrap());
    let mut harness = Harness::new(&game, world, ServerConfig::new(game_id()));
    let mut client = harness.connect();
    client.hello();
    client.watch(BrickCoordinate::ORIGIN, 0);
    harness.server.step();
    let contents = client
        .received()
        .into_iter()
        .find_map(|m| match m {
            ServerMessage::BrickContents {
                level: LevelIndex::MAIN,
                contents,
                ..
            } => Some(contents),
            _ => None,
        })
        .expect("the edited brick was sent");
    assert_eq!(*contents, *expected);
}

#[test]
fn a_first_edit_sends_the_whole_brick_and_later_edits_send_only_changes() {
    // The fixture's terrain is stone at this depth, so marble and dirt are real changes.
    let (game, mut harness) = default_harness();
    let marble = block_of(&game, "fancy_blocks:marble");
    let dirt = block_of(&game, "base_blocks:dirt");
    let mut watcher = harness.connect();
    let mut elsewhere = harness.connect();
    let mut builder = harness.connect();
    for client in [&mut watcher, &mut elsewhere, &mut builder] {
        client.hello();
    }
    watcher.watch(BrickCoordinate::ORIGIN, 1);
    elsewhere.watch(brick(50, 0, 0), 1);
    harness.server.step();
    for client in [&mut watcher, &mut elsewhere, &mut builder] {
        client.received();
    }

    // The brick has never been edited, so the watcher cannot apply a change to what it generated
    // itself... it is sent the brick.
    builder.place([1, 2, 3], marble);
    harness.server.step();
    let messages = watcher.received();
    let [
        ServerMessage::BrickContents {
            level: LevelIndex::MAIN,
            brick: edited,
            contents,
        },
        ServerMessage::TickCompleted { .. },
    ] = &messages[..]
    else {
        panic!("{messages:?}");
    };
    assert_eq!(*edited, brick(0, 0, 0));
    assert_eq!(
        *contents,
        *harness
            .server
            .world()
            .main()
            .overrides
            .brick(brick(0, 0, 0))
            .unwrap()
    );
    assert!(matches!(
        &elsewhere.received()[..],
        [ServerMessage::TickCompleted { .. }]
    ));
    assert!(matches!(
        &builder.received()[..],
        [ServerMessage::TickCompleted { .. }]
    ));

    // Now the brick is edited, so further edits go as changes, several in one message, with the
    // digest of the result.
    builder.place([1, 2, 4], dirt);
    builder.place([1, 2, 5], dirt);
    builder.place([1, 2, 3], marble); // already marble: not a change
    harness.server.step();
    let messages = watcher.received();
    let [
        ServerMessage::VoxelEdits {
            level: LevelIndex::MAIN,
            brick: edited,
            changes,
            resulting_digest,
        },
        ServerMessage::TickCompleted { .. },
    ] = &messages[..]
    else {
        panic!("{messages:?}");
    };
    assert_eq!(*edited, brick(0, 0, 0));
    assert_eq!(changes.len(), 2);
    assert_eq!(
        changes[0].voxel.index(),
        VoxelCoordinate::new(1, 2, 4).split().1.index()
    );
    assert_eq!(
        (changes[0].value, changes[1].value),
        (u32::from(dirt), u32::from(dirt))
    );
    assert_eq!(
        *resulting_digest,
        harness
            .server
            .world()
            .main()
            .overrides
            .brick(brick(0, 0, 0))
            .unwrap()
            .content_digest()
    );
}

#[test]
fn a_brick_that_comes_into_view_in_the_tick_it_is_edited_is_sent_once() {
    let (game, mut harness) = default_harness();
    let marble = block_of(&game, "fancy_blocks:marble");
    let mut client = harness.connect();
    client.hello();
    client.watch(brick(10, 0, 0), 0);
    harness.server.step();
    client.received();

    // In the same tick: the client asks to watch brick (0, 0, 0), and someone edits it.
    let mut builder = harness.connect();
    builder.hello();
    harness.server.step();
    client.received();
    client.watch(BrickCoordinate::ORIGIN, 0);
    builder.place([1, 2, 3], marble);
    harness.server.step();
    let messages = client.received();
    assert_eq!(
        messages
            .iter()
            .filter(|m| matches!(
                m,
                ServerMessage::BrickContents {
                    level: LevelIndex::MAIN,
                    ..
                } | ServerMessage::VoxelEdits {
                    level: LevelIndex::MAIN,
                    ..
                }
            ))
            .count(),
        1,
        "{messages:?}"
    );
    assert!(matches!(
        messages[1],
        ServerMessage::BrickContents {
            level: LevelIndex::MAIN,
            ..
        }
    ));
}

#[test]
fn game_messages_reach_exactly_their_audience() {
    let game = Game::new(&ALL_PLUGINS);
    let logic = ScriptedLogic::new().on_tick(|tick, world| {
        if tick == Tick::new(3) {
            world.send(Audience::Everyone, id("test_game:all"), vec![1]);
            world.send(
                Audience::Client(ClientId::new(2)),
                id("test_game:second"),
                vec![2],
            );
            world.send(
                Audience::Watching(LevelIndex::MAIN, BrickCoordinate::ORIGIN),
                id("test_game:near_origin"),
                vec![3],
            );
            world.send(
                Audience::Client(ClientId::new(77)),
                id("test_game:nobody"),
                vec![4],
            );
        }
    });
    let mut harness =
        Harness::with_logic(&game, game.new_world(), ServerConfig::new(game_id()), logic);
    let mut first = harness.connect();
    let mut second = harness.connect();
    first.hello();
    second.hello();
    first.watch(BrickCoordinate::ORIGIN, 0);
    second.watch(brick(9, 9, 9), 0);
    harness.server.step();
    harness.server.step();
    first.received();
    second.received();

    harness.server.step();
    let kinds = |messages: Vec<ServerMessage>| -> Vec<String> {
        messages
            .into_iter()
            .filter_map(|m| match m {
                ServerMessage::GameMessage { kind, .. } => Some(kind.to_string()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        kinds(first.received()),
        ["test_game:all", "test_game:near_origin"]
    );
    assert_eq!(
        kinds(second.received()),
        ["test_game:all", "test_game:second"]
    );
}

#[test]
fn a_game_message_from_a_client_reaches_the_game_in_order() {
    let (_, mut harness) = default_harness();
    let mut client = harness.connect();
    client.hello();
    for payload in [1u8, 2, 3] {
        client.send(&ClientMessage::GameMessage {
            kind: id("test_game:note"),
            payload: vec![payload],
        });
    }
    harness.server.step();
    let received: Vec<_> = harness
        .logic_events()
        .into_iter()
        .filter_map(|event| match event {
            LogicEvent::Message { payload, .. } => Some(payload[0]),
            _ => None,
        })
        .collect();
    assert_eq!(received, [1, 2, 3]);
}

#[test]
fn leaving_by_goodbye_or_by_closing_is_reported_to_the_game() {
    let (_, mut harness) = default_harness();
    let mut polite = harness.connect();
    let mut abrupt = harness.connect();
    let mut staying = harness.connect();
    for client in [&mut polite, &mut abrupt, &mut staying] {
        client.hello();
    }
    harness.server.step();
    polite.send(&ClientMessage::Goodbye);
    drop(abrupt);
    let report: StepReport = harness.server.step();
    assert_eq!(
        report.left,
        [
            (ClientId::new(1), LeaveReason::Goodbye),
            (ClientId::new(2), LeaveReason::ConnectionClosed)
        ]
    );
    assert_eq!(
        harness.server.connected_clients().collect::<Vec<_>>(),
        [ClientId::new(3)]
    );
    let events = harness.logic_events();
    assert!(events.contains(&LogicEvent::Left(ClientId::new(1))));
    assert!(events.contains(&LogicEvent::Left(ClientId::new(2))));
    assert!(!events.contains(&LogicEvent::Left(ClientId::new(3))));
    // The server keeps ticking and the remaining client keeps receiving.
    harness.server.step();
    let last = staying.received().pop().unwrap();
    assert_eq!(last, ServerMessage::TickCompleted { tick: Tick::new(3) });
}

#[test]
fn watching_more_than_the_limit_is_refused_without_the_server_doing_the_work() {
    let game = Game::new(&ALL_PLUGINS);
    let mut config = ServerConfig::new(game_id());
    config.maximum_watched_bricks = 27;
    let mut harness = Harness::new(&game, game.new_world(), config);
    let mut client = harness.connect();
    client.hello();
    client.watch(BrickCoordinate::ORIGIN, 1); // exactly 27 bricks: allowed
    assert!(harness.server.step().left.is_empty());
    client.watch(BrickCoordinate::ORIGIN, 2); // 125 bricks
    let report = harness.server.step();
    assert!(
        matches!(&report.left[..], [(_, LeaveReason::Refused(reason))] if reason.contains("limit"))
    );
    // A request for everything must not hang or exhaust memory either.
    let mut greedy = harness.connect();
    greedy.hello();
    greedy.send(&ClientMessage::SetInterest {
        level: LevelIndex::MAIN,
        region: BrickBox::around(BrickCoordinate::ORIGIN, u32::MAX),
    });
    let report = harness.server.step();
    assert!(matches!(&report.left[..], [(_, LeaveReason::Refused(_))]));
}

#[test]
fn every_client_sees_each_tick_exactly_once_in_order_whether_or_not_it_watches_anything() {
    let (_, mut harness) = default_harness();
    let mut early = harness.connect();
    early.hello();
    for _ in 0..3 {
        harness.server.step();
    }
    let mut late = harness.connect();
    late.hello();
    for _ in 0..3 {
        harness.server.step();
    }
    let ticks = |messages: Vec<ServerMessage>| -> Vec<u64> {
        messages
            .into_iter()
            .filter_map(|m| match m {
                ServerMessage::TickCompleted { tick } => Some(tick.get()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(ticks(early.received()), [1, 2, 3, 4, 5, 6]);
    assert_eq!(ticks(late.received()), [4, 5, 6]);
}

#[test]
fn editing_terrain_whose_generator_is_not_loaded_is_an_error_not_a_crash() {
    // A world with terrain from a generator the server does not have.
    let everything = Game::new(&ALL_PLUGINS);
    let bytes = everything.save(&sample_world(&everything));
    let mut reduced = Game::new(&[Plugin::BaseBlocks]);
    let (world, _) = reduced.load(&bytes).unwrap();

    let grass = reduced.raw_block("base_blocks:grass");
    let outcome = Arc::new(Mutex::new(None));
    let recorded = Arc::clone(&outcome);
    let logic = ScriptedLogic::new().on_tick(move |_, world| {
        let channel = world
            .data()
            .main()
            .overrides
            .layout()
            .channel_index(&id("core:block"))
            .unwrap();
        // Brick (100, 0, 0) was generated by fancy_terrain:cliffs_v1; brick (-50, 0, 0) is fine.
        let unavailable = world.set_voxel(
            LevelIndex::MAIN,
            channel,
            VoxelCoordinate::new(100 * 32, 5, 5),
            grass,
        );
        let available = world.set_voxel(
            LevelIndex::MAIN,
            channel,
            VoxelCoordinate::new(-50 * 32, 5, 5),
            grass,
        );
        *recorded.lock().unwrap() = Some((unavailable, available));
    });
    let mut harness = Harness::with_logic(&reduced, world, ServerConfig::new(game_id()), logic);
    harness.server.step();
    let (unavailable, available) = outcome.lock().unwrap().take().unwrap();
    assert_eq!(
        unavailable.unwrap_err(),
        server::AccessError::TerrainUnavailable {
            generator: id("fancy_terrain:cliffs_v1")
        }
    );
    assert_eq!(available, Ok(true));
}

#[test]
fn a_brick_that_holds_one_value_everywhere_says_so_whether_generated_or_edited() {
    let outcome = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&outcome);
    let logic = ScriptedLogic::new().on_tick(move |tick, world| {
        let channel = world
            .data()
            .main()
            .overrides
            .layout()
            .channel_index(&id("core:block"))
            .unwrap();
        let sky = BrickCoordinate::new(0, 40, 0);
        let mut found = Vec::new();
        found.push(world.uniform_value(LevelIndex::MAIN, channel, sky));
        // Editing one voxel of the brick makes it not uniform any more; changing it back does not
        // need to make it uniform again.
        if tick.get() == 1 {
            let _ = world.set_voxel(
                LevelIndex::MAIN,
                channel,
                VoxelCoordinate::new(3, 40 * 32, 3),
                7,
            );
            found.push(world.uniform_value(LevelIndex::MAIN, channel, sky));
        }
        // A level that does not exist is an error.
        found.push(world.uniform_value(LevelIndex::new(9), channel, sky));
        recorded.lock().unwrap().push((tick.get(), found));
    });
    let game = Game::new(&[Plugin::BaseBlocks]);
    let world = game.new_world();
    let mut harness = Harness::with_logic(&game, world, ServerConfig::new(game_id()), logic);
    harness.server.step();
    harness.server.step();
    let results = outcome.lock().unwrap().clone();
    let air = game.raw_block("base_blocks:air");
    // First tick: the sky is all air; after the edit it is not all anything.
    assert_eq!(results[0].1[0], Ok(Some(air)));
    assert_eq!(results[0].1[1], Ok(None));
    assert_eq!(
        results[0].1[2],
        Err(server::AccessError::NoSuchLevel(LevelIndex::new(9)))
    );
    // Second tick: it stays edited.
    assert_eq!(results[1].1[0], Ok(None));
}

/// Runs a fixed scenario and returns every frame each client received.
fn scenario_frames() -> Vec<Vec<Vec<u8>>> {
    let (game, mut harness) = default_harness();
    let blocks = [
        "fancy_blocks:glass",
        "fancy_blocks:marble",
        "base_blocks:dirt",
    ]
    .map(|name| block_of(&game, name));
    let mut clients = [harness.connect(), harness.connect(), harness.connect()];
    for client in &mut clients {
        client.hello();
    }
    clients[0].watch(BrickCoordinate::ORIGIN, 2);
    clients[1].watch(brick(1, 0, 0), 1);
    for step in 0..12i8 {
        clients[(step % 3) as usize].place([step, step % 5, -step], blocks[(step % 3) as usize]);
        if step == 6 {
            clients[0].watch(brick(1, 0, 1), 2);
        }
        harness.server.step();
    }
    let mut frames: Vec<Vec<Vec<u8>>> = clients.iter_mut().map(TestClient::raw_frames).collect();
    // The frames are only comparable if there was something to compare.
    assert!(frames.iter().all(|f| f.len() >= 12));
    frames.shrink_to_fit();
    frames
}

#[test]
fn the_same_inputs_give_byte_identical_streams() {
    assert_eq!(scenario_frames(), scenario_frames());
}

#[test]
fn a_region_that_moves_partly_sends_only_the_bricks_it_gains_and_shrinking_sends_nothing_new() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = game.new_world();
    game.set_block(&mut world, [1, 2, 3], "base_blocks:dirt"); // brick (0, 0, 0)
    game.set_block(&mut world, [70, 2, 3], "base_blocks:dirt"); // brick (2, 0, 0)
    game.set_block(&mut world, [-40, 2, 3], "base_blocks:dirt"); // brick (-2, 0, 0)
    let mut harness = Harness::new(&game, world, ServerConfig::new(game_id()));
    let mut client = harness.connect();
    client.hello();
    client.watch(BrickCoordinate::ORIGIN, 1); // x from -1 to 1
    harness.server.step();
    client.received();

    // One brick to the right: x from 0 to 2. Brick (2, 0, 0) is gained, (0, 0, 0) is kept.
    client.watch(brick(1, 0, 0), 1);
    harness.server.step();
    let messages = client.received();
    let sent: Vec<_> = messages
        .iter()
        .filter_map(|m| match m {
            ServerMessage::BrickContents {
                level: LevelIndex::MAIN,
                brick,
                ..
            } => Some(*brick),
            _ => None,
        })
        .collect();
    assert_eq!(sent, [brick(2, 0, 0)], "{messages:?}");

    // Shrinking to a part of what was watched: the client is told, and nothing is sent again.
    client.watch(brick(1, 0, 0), 0); // x = 1 only
    harness.server.step();
    let messages = client.received();
    let [
        ServerMessage::InterestChanged {
            level: LevelIndex::MAIN,
            region,
            assignments,
        },
        ServerMessage::TickCompleted { .. },
    ] = &messages[..]
    else {
        panic!("{messages:?}");
    };
    assert_eq!(*region, BrickBox::around(brick(1, 0, 0), 0));
    assert!(assignments.is_empty());
}

#[test]
fn setting_a_voxel_to_the_value_it_already_has_edits_nothing() {
    let (game, mut harness) = default_harness();
    let stone = block_of(&game, "base_blocks:stone"); // the terrain at this depth
    let mut client = harness.connect();
    let mut builder = harness.connect();
    client.hello();
    builder.hello();
    client.watch(BrickCoordinate::ORIGIN, 1);
    harness.server.step();
    client.received();

    builder.place([1, 2, 3], stone);
    harness.server.step();
    assert!(matches!(
        &client.received()[..],
        [ServerMessage::TickCompleted { .. }]
    ));
    assert_eq!(harness.server.world().main().overrides.brick_count(), 0);
}

#[test]
fn the_games_own_state_is_saved_with_the_world_and_survives_a_load() {
    use world_storage::Section;

    let game = Game::new(&ALL_PLUGINS);
    let state = Section {
        identifier: id("test_game:score"),
        version: 2,
        payload: vec![9, 9, 9],
    };
    let logic = ScriptedLogic::new().saving(state.clone());
    let harness = Harness::with_logic(&game, game.new_world(), ServerConfig::new(game_id()), logic);
    let world = harness.server.into_world();
    assert_eq!(world.opaque_sections(), std::slice::from_ref(&state));

    // The state rides in the saved file like any plugin section, even for a program without the game.
    let bytes = game.save(&world);
    let mut other = Game::new(&[Plugin::BaseBlocks]);
    let (loaded, report) = other.load(&bytes).unwrap();
    assert_eq!(report.opaque_sections, [id("test_game:score")]);
    assert_eq!(loaded.opaque_sections(), [state]);
}

#[test]
fn a_running_server_can_be_saved_with_its_games_state_and_keeps_running() {
    use world_storage::Section;

    let game = Game::new(&ALL_PLUGINS);
    let state = Section {
        identifier: id("test_game:score"),
        version: 1,
        payload: vec![4, 2],
    };
    let logic = ScriptedLogic::new().saving(state.clone());
    let mut harness =
        Harness::with_logic(&game, game.new_world(), ServerConfig::new(game_id()), logic);
    let mut client = harness.connect();
    client.hello();
    harness.server.step();
    let bytes = harness.server.save(&game.registries()).unwrap();

    let mut other = Game::new(&ALL_PLUGINS);
    let (loaded, _) = other.load(&bytes).unwrap();
    assert_eq!(loaded.opaque_sections(), [state]);
    // Saving twice does not duplicate the section, and the server goes on serving.
    let again = harness.server.save(&game.registries()).unwrap();
    assert_eq!(again, bytes);
    assert_eq!(harness.server.step().tick, Tick::new(2));
    assert_eq!(harness.server.connected_clients().count(), 1);
}

// ---------------------------------------------------------------------------------------------
// Levels and commands
// ---------------------------------------------------------------------------------------------

impl TestClient {
    fn watch_level(&mut self, level: LevelIndex, center: BrickCoordinate, radius: u32) {
        self.send(&ClientMessage::SetInterest {
            level,
            region: BrickBox::around(center, radius),
        });
    }

    fn command(&mut self, text: &str) {
        self.send(&ClientMessage::Command {
            text: text.to_owned(),
        });
    }
}

/// A world of two levels: the main level (flat ground) with an edited brick at (1, 0, 0), and a
/// second one (cliffs) with edited bricks at (0, 0, 0) and (1, 1, 1).
fn two_level_world(game: &Game) -> (WorldData, LevelIndex) {
    let mut world = game.new_world();
    let grass = game.raw_block("base_blocks:grass");
    game.set(&mut world, [32, 1, 1], "core:block", grass);
    let second = world
        .add_level(
            id("test_content:second"),
            game.new_level("fancy_terrain:cliffs_v1"),
        )
        .unwrap();
    game.set_in_level(&mut world, second, [1, 1, 1], "core:block", grass);
    game.set_in_level(&mut world, second, [40, 40, 40], "core:block", grass);
    (world, second)
}

/// A game whose clients place blocks in a level, with `test_game:set_block_in`: the level number,
/// three signed voxel coordinates and a block number.
fn level_placing_logic() -> ScriptedLogic {
    ScriptedLogic::new().on_message(|_, kind, payload, world| {
        if *kind == id("test_game:set_block_in") {
            let channel = world
                .data()
                .main()
                .overrides
                .layout()
                .channel_index(&id("core:block"))
                .unwrap();
            let at = VoxelCoordinate::new(
                i64::from(payload[1] as i8),
                i64::from(payload[2] as i8),
                i64::from(payload[3] as i8),
            );
            world
                .set_voxel(
                    LevelIndex::new(u32::from(payload[0])),
                    channel,
                    at,
                    u32::from(payload[4]),
                )
                .unwrap();
        }
    })
}

fn place_in(client: &mut TestClient, level: LevelIndex, at: [i8; 3], block: u8) {
    client.send(&ClientMessage::GameMessage {
        kind: id("test_game:set_block_in"),
        payload: vec![
            level.get() as u8,
            at[0] as u8,
            at[1] as u8,
            at[2] as u8,
            block,
        ],
    });
}

/// The bricks a client was sent contents of, as (level, brick).
fn contents_sent(messages: &[ServerMessage]) -> Vec<(LevelIndex, BrickCoordinate)> {
    messages
        .iter()
        .filter_map(|message| match message {
            ServerMessage::BrickContents { level, brick, .. } => Some((*level, *brick)),
            _ => None,
        })
        .collect()
}

fn edits_sent(messages: &[ServerMessage]) -> Vec<(LevelIndex, BrickCoordinate)> {
    messages
        .iter()
        .filter_map(|message| match message {
            ServerMessage::VoxelEdits { level, brick, .. } => Some((*level, *brick)),
            _ => None,
        })
        .collect()
}

fn has_command_reply(messages: &[ServerMessage]) -> bool {
    messages
        .iter()
        .any(|message| matches!(message, ServerMessage::CommandReply { .. }))
}

#[test]
fn the_welcome_describes_every_level_by_name() {
    let game = Game::new(&ALL_PLUGINS);
    let (world, _) = two_level_world(&game);
    let mut harness = Harness::new(&game, world, ServerConfig::new(game_id()));
    let mut client = harness.connect();
    client.hello();
    harness.server.step();
    let Some(ServerMessage::Welcome(welcome)) = client.received().into_iter().next() else {
        panic!("a welcome comes first");
    };
    let mut receiver = Game::new(&ALL_PLUGINS);
    let (described, _) = ParsedWorld::parse(&welcome.world_description)
        .unwrap()
        .load(receiver.load_context())
        .unwrap();
    assert_eq!(described.level_count(), 2);
    let second = LevelIndex::new(1);
    assert_eq!(described.level_id(second), Some(&id("test_content:second")));
    // Shape only: no contents, and the second level's generator setup is told.
    let level = described.level(second).unwrap();
    assert_eq!(level.overrides.brick_count(), 0);
    assert_eq!(
        level.preferred_default_generator(),
        &id("fancy_terrain:cliffs_v1")
    );
}

#[test]
fn clients_watching_different_levels_are_sent_only_that_levels_bricks_generators_and_edits() {
    let game = Game::new(&ALL_PLUGINS);
    let (world, second) = two_level_world(&game);
    let mut harness = Harness::with_logic(
        &game,
        world,
        ServerConfig::new(game_id()),
        level_placing_logic(),
    );
    let (mut main_watcher, mut second_watcher) = (harness.connect(), harness.connect());
    main_watcher.hello();
    second_watcher.hello();
    harness.server.step();
    main_watcher.received();
    second_watcher.received();
    main_watcher.watch_level(LevelIndex::MAIN, brick(0, 0, 0), 1);
    second_watcher.watch_level(second, brick(0, 0, 0), 1);
    harness.server.step();

    let (for_main, for_second) = (main_watcher.received(), second_watcher.received());
    let generator_of = |messages: &[ServerMessage]| {
        messages.iter().find_map(|message| match message {
            ServerMessage::InterestChanged {
                level, assignments, ..
            } => Some((*level, assignments[0].generator)),
            _ => None,
        })
    };
    // The flat generator is entry 0 of the shared table and the cliffs entry 1.
    assert_eq!(generator_of(&for_main), Some((LevelIndex::MAIN, Some(0))));
    assert_eq!(generator_of(&for_second), Some((second, Some(1))));
    assert_eq!(
        contents_sent(&for_main),
        [(LevelIndex::MAIN, brick(1, 0, 0))]
    );
    assert_eq!(
        contents_sent(&for_second),
        [(second, brick(0, 0, 0)), (second, brick(1, 1, 1))]
    );

    // A change in each level reaches only those watching it, as the kind of message the brick's
    // history calls for: the main level's brick (0, 0, 0) is edited for the first time, the second
    // level's already was.
    let dirt = block_of(&game, "base_blocks:dirt");
    place_in(&mut main_watcher, LevelIndex::MAIN, [5, 5, 5], dirt);
    place_in(&mut main_watcher, second, [6, 6, 6], dirt);
    harness.server.step();
    let (for_main, for_second) = (main_watcher.received(), second_watcher.received());
    assert_eq!(
        contents_sent(&for_main),
        [(LevelIndex::MAIN, brick(0, 0, 0))]
    );
    assert_eq!(edits_sent(&for_main), []);
    assert_eq!(contents_sent(&for_second), []);
    assert_eq!(edits_sent(&for_second), [(second, brick(0, 0, 0))]);
}

#[test]
fn moving_to_another_level_sends_the_whole_region_again_and_back_again() {
    let game = Game::new(&ALL_PLUGINS);
    let (world, second) = two_level_world(&game);
    let mut harness = Harness::new(&game, world, ServerConfig::new(game_id()));
    let mut client = harness.connect();
    client.hello();
    harness.server.step();
    client.watch_level(LevelIndex::MAIN, brick(0, 0, 0), 1);
    harness.server.step();
    assert_eq!(
        contents_sent(&client.received()),
        [(LevelIndex::MAIN, brick(1, 0, 0))]
    );

    // The same region of another level is all new, not a change to what was held.
    client.watch_level(second, brick(0, 0, 0), 1);
    harness.server.step();
    let messages = client.received();
    assert!(matches!(
        messages
            .iter()
            .find(|message| matches!(message, ServerMessage::InterestChanged { .. })),
        Some(ServerMessage::InterestChanged { level, .. }) if *level == second
    ));
    assert_eq!(
        contents_sent(&messages),
        [(second, brick(0, 0, 0)), (second, brick(1, 1, 1))]
    );

    client.watch_level(LevelIndex::MAIN, brick(0, 0, 0), 1);
    harness.server.step();
    assert_eq!(
        contents_sent(&client.received()),
        [(LevelIndex::MAIN, brick(1, 0, 0))]
    );

    // Moving within a level still sends only the newly watched part: nothing here, since the
    // newly watched bricks hold no edits.
    client.watch_level(LevelIndex::MAIN, brick(-1, 0, 0), 1);
    harness.server.step();
    assert_eq!(contents_sent(&client.received()), []);
}

#[test]
fn watching_a_level_the_world_does_not_have_is_refused_with_a_reason() {
    let (_, mut harness) = default_harness();
    let mut client = harness.connect();
    client.hello();
    harness.server.step();
    client.received();
    client.watch_level(LevelIndex::new(5), brick(0, 0, 0), 1);
    let report = harness.server.step();
    assert_eq!(
        report.left,
        [(
            ClientId::new(1),
            LeaveReason::Refused("the world has no level 5".to_owned())
        )]
    );
    let messages = client.received();
    assert!(matches!(
        &messages[..],
        [ServerMessage::Disconnected { reason }] if reason == "the world has no level 5"
    ));
}

#[test]
fn a_game_message_for_those_watching_a_brick_reaches_only_those_watching_that_level() {
    let game = Game::new(&ALL_PLUGINS);
    let (world, second) = two_level_world(&game);
    let logic = ScriptedLogic::new().on_message(move |_, kind, _, world| {
        if *kind == id("test_game:ping") {
            world.send(
                Audience::Watching(second, BrickCoordinate::ORIGIN),
                id("test_game:pong"),
                vec![],
            );
        }
    });
    let mut harness = Harness::with_logic(&game, world, ServerConfig::new(game_id()), logic);
    let (mut main_watcher, mut second_watcher) = (harness.connect(), harness.connect());
    for client in [&mut main_watcher, &mut second_watcher] {
        client.hello();
    }
    harness.server.step();
    main_watcher.watch_level(LevelIndex::MAIN, brick(0, 0, 0), 1);
    second_watcher.watch_level(second, brick(0, 0, 0), 1);
    harness.server.step();
    main_watcher.received();
    second_watcher.received();
    main_watcher.send(&ClientMessage::GameMessage {
        kind: id("test_game:ping"),
        payload: vec![],
    });
    harness.server.step();
    let pongs = |messages: Vec<ServerMessage>| {
        messages
            .iter()
            .filter(|message| {
                matches!(message, ServerMessage::GameMessage { kind, .. } if *kind == id("test_game:pong"))
            })
            .count()
    };
    assert_eq!(pongs(main_watcher.received()), 0);
    assert_eq!(pongs(second_watcher.received()), 1);
}

#[test]
fn a_command_reaches_the_game_in_order_and_its_reply_goes_to_the_sender_after_the_game_messages() {
    let game = Game::new(&ALL_PLUGINS);
    let logic = ScriptedLogic::new().on_command(|client, text, world| {
        world.send(Audience::Everyone, id("test_game:heard"), vec![]);
        world.reply(client, &format!("you said {text}"));
    });
    let mut harness =
        Harness::with_logic(&game, game.new_world(), ServerConfig::new(game_id()), logic);
    let (mut speaker, mut listener) = (harness.connect(), harness.connect());
    speaker.hello();
    listener.hello();
    harness.server.step();
    speaker.received();
    listener.received();
    speaker.command("/echo hi");
    speaker.command("/echo again");
    harness.server.step();

    let heard = speaker.received();
    let kinds: Vec<&str> = heard
        .iter()
        .map(|message| match message {
            ServerMessage::GameMessage { .. } => "game message",
            ServerMessage::CommandReply { .. } => "reply",
            ServerMessage::TickCompleted { .. } => "tick",
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        ["game message", "game message", "reply", "reply", "tick"]
    );
    let replies: Vec<&str> = heard
        .iter()
        .filter_map(|message| match message {
            ServerMessage::CommandReply { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(replies, ["you said /echo hi", "you said /echo again"]);
    // The other client heard the game's broadcast but not the reply meant for the speaker.
    assert!(!has_command_reply(&listener.received()));
    let commands: Vec<LogicEvent> = harness
        .logic_events()
        .into_iter()
        .filter(|event| matches!(event, LogicEvent::Command { .. }))
        .collect();
    assert_eq!(
        commands,
        [
            LogicEvent::Command {
                client: ClientId::new(1),
                text: "/echo hi".to_owned()
            },
            LogicEvent::Command {
                client: ClientId::new(1),
                text: "/echo again".to_owned()
            }
        ]
    );
    // Replies are not repeated on later ticks.
    harness.server.step();
    assert!(!has_command_reply(&speaker.received()));
}

#[test]
fn a_game_with_no_commands_says_so_and_bad_commands_are_protocol_violations() {
    let (_, mut harness) = default_harness();
    let mut client = harness.connect();
    client.hello();
    harness.server.step();
    client.received();
    client.command("/anything");
    harness.server.step();
    assert!(matches!(
        client
            .received()
            .iter()
            .find(|message| matches!(message, ServerMessage::CommandReply { .. })),
        Some(ServerMessage::CommandReply { text }) if text == "this game has no commands"
    ));

    // Before hello, a command is the wrong first message.
    let mut early = harness.connect();
    early.command("/level list");
    let report = harness.server.step();
    assert!(
        report.left.iter().any(
            |(_, reason)| matches!(reason, LeaveReason::Refused(text) if text.contains("hello"))
        )
    );

    // A command longer than allowed does not decode.
    let mut rude = harness.connect();
    rude.hello();
    harness.server.step();
    rude.received();
    rude.command(&"a".repeat(protocol::MAXIMUM_COMMAND_LENGTH + 1));
    let report = harness.server.step();
    assert!(report.left.iter().any(
        |(_, reason)| matches!(reason, LeaveReason::Refused(text) if text.contains("malformed"))
    ));
}

#[test]
fn switching_levels_in_the_tick_of_an_edit_sends_each_brick_once_in_its_final_state() {
    let game = Game::new(&ALL_PLUGINS);
    let (world, second) = two_level_world(&game);
    let mut harness = Harness::with_logic(
        &game,
        world,
        ServerConfig::new(game_id()),
        level_placing_logic(),
    );
    let (mut switcher, mut editor) = (harness.connect(), harness.connect());
    switcher.hello();
    editor.hello();
    harness.server.step();
    switcher.watch_level(LevelIndex::MAIN, brick(0, 0, 0), 1);
    harness.server.step();
    switcher.received();

    // In one tick the switcher moves to the second level and the second level's brick (0, 0, 0),
    // which was already edited, is edited again inside the region both levels share.
    switcher.watch_level(second, brick(0, 0, 0), 1);
    let dirt = block_of(&game, "base_blocks:dirt");
    place_in(&mut editor, second, [6, 6, 6], dirt);
    harness.server.step();
    let messages = switcher.received();
    assert_eq!(
        contents_sent(&messages),
        [(second, brick(0, 0, 0)), (second, brick(1, 1, 1))]
    );
    assert_eq!(
        edits_sent(&messages),
        [],
        "the bricks already carry the edit; sending it again would apply it twice"
    );
}

#[test]
fn a_grid_reads_and_writes_the_voxels_of_its_level_as_the_world_does_and_unknown_grids_are_errors()
{
    let outcome = Arc::new(Mutex::new(None));
    let recorded = Arc::clone(&outcome);
    let logic = ScriptedLogic::new().on_tick(move |_, world| {
        let channel = world
            .data()
            .main()
            .overrides
            .layout()
            .channel_index(&id("core:block"))
            .unwrap();
        let at = VoxelCoordinate::new(3, 40 * 32, 3);
        let mut grid = world.grid(GridId::MAIN).unwrap();
        let identity = grid.id();
        let before = grid.get(channel, at);
        let changed = grid.set(channel, at, 7);
        let unchanged = grid.set(channel, at, 7);
        let through_the_grid = grid.get(channel, at);
        // The same voxel, asked of the world by level.
        let through_the_world = world.voxel(LevelIndex::MAIN, channel, at);
        let no_such_level = world.grid(GridId::root(LevelIndex::new(9))).err();
        let not_a_root = world.grid(GridId::new(GridId::FIRST_ALLOCATED)).err();
        *recorded.lock().unwrap() = Some((
            identity,
            before,
            changed,
            unchanged,
            through_the_grid,
            through_the_world,
            no_such_level,
            not_a_root,
        ));
    });
    let game = Game::new(&[Plugin::BaseBlocks]);
    let world = sample_world(&game);
    let mut harness = Harness::with_logic(&game, world, ServerConfig::new(game_id()), logic);
    harness.server.step();
    let found = outcome.lock().unwrap().take().unwrap();
    assert_eq!(found.0, GridId::MAIN);
    assert_eq!(found.1, Ok(0));
    assert_eq!(found.2, Ok(true));
    assert_eq!(found.3, Ok(false), "setting what is there changes nothing");
    assert_eq!(found.4, Ok(7));
    assert_eq!(found.5, Ok(7));
    assert_eq!(
        found.6,
        Some(server::AccessError::NoSuchGrid(GridId::root(
            LevelIndex::new(9)
        )))
    );
    assert_eq!(
        found.7,
        Some(server::AccessError::NoSuchGrid(GridId::new(
            GridId::FIRST_ALLOCATED
        )))
    );
}

// ---------------------------------------------------------------- entities

/// A component clients are told of.
#[derive(Debug, PartialEq)]
struct Health(u32);

impl Message for Health {
    fn kind() -> NamespacedId {
        id("test:health")
    }
    fn encode(&self) -> Vec<u8> {
        self.0.to_le_bytes().to_vec()
    }
    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let value = reader.read_u32()?;
        reader.finish()?;
        Ok(Self(value))
    }
    fn samples() -> Vec<Self> {
        vec![Self(0)]
    }
}

/// A component only the server knows of.
#[derive(Debug, PartialEq)]
struct Mood(u8);

impl Message for Mood {
    fn kind() -> NamespacedId {
        id("test:mood")
    }
    fn encode(&self) -> Vec<u8> {
        vec![self.0]
    }
    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        match payload {
            [value] => Ok(Self(*value)),
            _ => Err(DecodeError::Invalid("a mood is one byte")),
        }
    }
    fn samples() -> Vec<Self> {
        vec![Self(0)]
    }
}

fn cow_catalog() -> EntityCatalog {
    let mut catalog = EntityCatalog::default();
    catalog
        .kinds
        .register(
            EntityKindSpec::new("test:cow")
                .component(Health(10), true)
                .component(Mood(0), false),
        )
        .unwrap();
    catalog
}

fn pose_in(brick_x: i64, offset_x: f32) -> EntityPose {
    EntityPose::new(
        GridId::MAIN,
        WorldPosition::new(BrickCoordinate::new(brick_x, 0, 0), [offset_x, 5.0, 5.0]),
        Orientation::IDENTITY,
    )
}

/// A server with the cow kind whose game does `act` at each tick.
fn herd_harness(
    act: impl FnMut(Tick, &mut server::ServerWorld) + Send + 'static,
) -> (Game, Harness) {
    let game = Game::new(&ALL_PLUGINS);
    let logic = ScriptedLogic::new()
        .with_entity_catalog(cow_catalog())
        .on_tick(act);
    let harness = Harness::with_logic(&game, game.new_world(), ServerConfig::new(game_id()), logic);
    (game, harness)
}

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

fn health_bytes(value: u32) -> (NamespacedId, Vec<u8>) {
    (id("test:health"), Health(value).encode())
}

#[test]
fn a_client_is_told_of_the_entities_in_the_region_it_watches_and_only_what_its_kind_replicates() {
    let (_game, mut harness) = herd_harness(|tick, world| {
        if tick.get() == 1 {
            world
                .spawn_entity(&id("test:cow"), pose_in(0, 3.0), ComponentBag::new())
                .unwrap();
            // Far from anyone's region.
            world
                .spawn_entity(&id("test:cow"), pose_in(40, 3.0), ComponentBag::new())
                .unwrap();
        }
    });
    let mut client = harness.connect();
    client.hello();
    harness.server.step();
    client.received();
    client.watch(BrickCoordinate::ORIGIN, 2);
    harness.server.step();
    let messages = client.received();
    let told = entity_messages(&messages);
    assert_eq!(told.len(), 1, "{told:?}");
    let ServerMessage::EntitiesSpawned(spawned) = told[0] else {
        panic!("{told:?}");
    };
    assert_eq!(spawned.len(), 1);
    assert_eq!(spawned[0].id, EntityId::new(1));
    assert_eq!(spawned[0].kind, id("test:cow"));
    assert_eq!(spawned[0].pose, pose_in(0, 3.0));
    // The health is told, the mood is not.
    assert_eq!(spawned[0].components, [health_bytes(10)]);
    // Quiet ticks say nothing more of it.
    harness.server.step();
    harness.server.step();
    assert!(entity_messages(&client.received()).is_empty());
}

#[test]
fn what_changed_is_what_is_sent_and_a_change_to_what_is_not_replicated_is_not() {
    let (_game, mut harness) = herd_harness(|tick, world| match tick.get() {
        1 => {
            world
                .spawn_entity(&id("test:cow"), pose_in(0, 3.0), ComponentBag::new())
                .unwrap();
        }
        3 => world
            .set_entity_pose(EntityId::new(1), pose_in(0, 4.0))
            .unwrap(),
        4 => world
            .set_entity_component(EntityId::new(1), &Health(7))
            .unwrap(),
        5 => world
            .set_entity_component(EntityId::new(1), &Mood(3))
            .unwrap(),
        6 => world
            .set_entity_component(EntityId::new(1), &Health(7))
            .unwrap(),
        _ => {}
    });
    let mut client = harness.connect();
    client.hello();
    harness.server.step();
    client.watch(BrickCoordinate::ORIGIN, 1);
    harness.server.step();
    client.received();
    let mut per_tick = Vec::new();
    for _ in 3..=6 {
        harness.server.step();
        per_tick.push(client.received());
    }
    let changes = |index: usize| -> Vec<EntityChange> {
        entity_messages(&per_tick[index])
            .into_iter()
            .flat_map(|message| match message {
                ServerMessage::EntitiesChanged(changes) => changes.clone(),
                other => panic!("{other:?}"),
            })
            .collect()
    };
    // Tick 3: only the pose.
    assert_eq!(
        changes(0),
        [EntityChange {
            id: EntityId::new(1),
            pose: Some(pose_in(0, 4.0)),
            components: Vec::new()
        }]
    );
    // Tick 4: only the health.
    assert_eq!(
        changes(1),
        [EntityChange {
            id: EntityId::new(1),
            pose: None,
            components: vec![health_bytes(7)]
        }]
    );
    // Tick 5: a mood nobody is told of; tick 6: the health set to what it already is. Nothing is sent.
    assert!(entity_messages(&per_tick[2]).is_empty());
    assert!(entity_messages(&per_tick[3]).is_empty());
}

#[test]
fn an_entity_that_leaves_the_region_is_despawned_and_one_that_comes_in_is_spawned() {
    let (_game, mut harness) = herd_harness(|tick, world| match tick.get() {
        1 => {
            world
                .spawn_entity(&id("test:cow"), pose_in(0, 3.0), ComponentBag::new())
                .unwrap();
        }
        3 => world
            .set_entity_pose(EntityId::new(1), pose_in(40, 3.0))
            .unwrap(),
        5 => world
            .set_entity_pose(EntityId::new(1), pose_in(1, 3.0))
            .unwrap(),
        7 => {
            world.despawn_entity(EntityId::new(1));
        }
        _ => {}
    });
    let mut client = harness.connect();
    client.hello();
    harness.server.step();
    client.watch(BrickCoordinate::ORIGIN, 1);
    harness.server.step();
    client.received();
    let mut kinds = Vec::new();
    for _ in 3..=7 {
        harness.server.step();
        for message in client.received() {
            kinds.push(match message {
                ServerMessage::EntitiesSpawned(entities) => {
                    format!(
                        "spawned {:?}",
                        entities.iter().map(|e| e.id.get()).collect::<Vec<_>>()
                    )
                }
                ServerMessage::EntitiesChanged(_) => "changed".to_owned(),
                ServerMessage::EntitiesDespawned(ids) => {
                    format!(
                        "despawned {:?}",
                        ids.iter().map(|i| i.get()).collect::<Vec<_>>()
                    )
                }
                _ => continue,
            });
        }
    }
    assert_eq!(
        kinds,
        [
            "despawned [1]", // tick 3: it walked away
            "spawned [1]",   // tick 5: and came back
            "despawned [1]", // tick 7: and was removed
        ]
    );
}

#[test]
fn every_client_is_told_what_it_watches_and_a_late_one_is_told_everything() {
    let (_game, mut harness) = herd_harness(|tick, world| {
        if tick.get() == 1 {
            for brick_x in [0, 0, 6] {
                world
                    .spawn_entity(&id("test:cow"), pose_in(brick_x, 3.0), ComponentBag::new())
                    .unwrap();
            }
        }
    });
    let mut near = harness.connect();
    let mut far = harness.connect();
    near.hello();
    far.hello();
    harness.server.step();
    near.watch(BrickCoordinate::ORIGIN, 1);
    far.watch(BrickCoordinate::new(6, 0, 0), 1);
    harness.server.step();
    let ids = |messages: Vec<ServerMessage>| -> Vec<u64> {
        messages
            .iter()
            .filter_map(|message| match message {
                ServerMessage::EntitiesSpawned(entities) => {
                    Some(entities.iter().map(|e| e.id.get()).collect::<Vec<_>>())
                }
                _ => None,
            })
            .flatten()
            .collect()
    };
    assert_eq!(ids(near.received()), [1, 2]);
    assert_eq!(ids(far.received()), [3]);
    // Someone who joins later, watching both, is told of all three, in one message.
    let mut late = harness.connect();
    late.hello();
    harness.server.step();
    late.send(&ClientMessage::SetInterest {
        level: LevelIndex::MAIN,
        region: BrickBox::new(
            BrickCoordinate::new(-1, -1, -1),
            BrickCoordinate::new(7, 1, 1),
        )
        .unwrap(),
    });
    harness.server.step();
    assert_eq!(ids(late.received()), [1, 2, 3]);
}

#[test]
fn two_clients_watching_the_same_region_are_sent_the_same_bytes() {
    let (_game, mut harness) = herd_harness(|tick, world| {
        if tick.get() == 1 {
            world
                .spawn_entity(&id("test:cow"), pose_in(0, 3.0), ComponentBag::new())
                .unwrap();
        }
    });
    let mut first = harness.connect();
    let mut second = harness.connect();
    first.hello();
    second.hello();
    harness.server.step();
    first.watch(BrickCoordinate::ORIGIN, 1);
    second.watch(BrickCoordinate::ORIGIN, 1);
    harness.server.step();
    let entity_frames = |client: &mut TestClient| -> Vec<Vec<u8>> {
        client
            .raw_frames()
            .into_iter()
            .filter(|frame| matches!(frame.first(), Some(9..=11)))
            .collect()
    };
    let (a, b) = (entity_frames(&mut first), entity_frames(&mut second));
    assert_eq!(a.len(), 1);
    assert_eq!(a, b);
}

#[test]
fn entities_follow_the_voxel_edits_and_come_before_the_messages_of_the_game() {
    let (_game, mut harness) = herd_harness(|tick, world| {
        if tick.get() == 3 {
            world
                .spawn_entity(&id("test:cow"), pose_in(0, 3.0), ComponentBag::new())
                .unwrap();
            let channel = world
                .data()
                .main()
                .overrides
                .layout()
                .channel_index(&id("core:block"))
                .unwrap();
            world
                .set_voxel(LevelIndex::MAIN, channel, VoxelCoordinate::new(1, 1, 1), 1)
                .unwrap();
            world.send(Audience::Everyone, id("test_game:news"), vec![1]);
        }
    });
    let mut client = harness.connect();
    client.hello();
    harness.server.step();
    client.watch(BrickCoordinate::ORIGIN, 1);
    harness.server.step();
    client.received();
    harness.server.step();
    let order: Vec<&str> = client
        .received()
        .iter()
        .map(|message| match message {
            ServerMessage::BrickContents { .. } | ServerMessage::VoxelEdits { .. } => "voxels",
            ServerMessage::EntitiesSpawned(_) => "entities",
            ServerMessage::GameMessage { .. } => "game",
            ServerMessage::TickCompleted { .. } => "end",
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(order, ["voxels", "entities", "game", "end"]);
}

#[test]
fn an_entity_of_a_kind_the_game_does_not_have_is_kept_and_not_shown() {
    // The world holds a cow, but the game that serves it knows no cows.
    let game = Game::new(&ALL_PLUGINS);
    let mut world = game.new_world();
    {
        let mut catalog = EntityCatalog::default();
        catalog
            .kinds
            .register(EntityKindSpec::new("test:cow"))
            .unwrap();
        world
            .entities_mut()
            .spawn(
                &catalog.kinds,
                &id("test:cow"),
                pose_in(0, 3.0),
                ComponentBag::new(),
            )
            .unwrap();
    }
    let mut harness = Harness::new(&game, world, ServerConfig::new(game_id()));
    let mut client = harness.connect();
    client.hello();
    harness.server.step();
    client.watch(BrickCoordinate::ORIGIN, 1);
    harness.server.step();
    assert!(entity_messages(&client.received()).is_empty());
    assert_eq!(harness.server.world().entities().len(), 1);
}

/// How much replicating entities costs: run with `--ignored --nocapture`.
#[test]
#[ignore = "a benchmark: run with --ignored --nocapture"]
fn replication_benchmark() {
    const ENTITIES: u64 = 1000;
    const CLIENTS: usize = 4;
    println!(
        "\n| entities moving each tick | server ms per tick | bytes per tick per client | frames per tick per client |"
    );
    println!("|---|---|---|---|");
    for moving_every in [0u64, 10, 1] {
        let (_game, mut harness) = herd_harness(move |tick, world| {
            if tick.get() == 1 {
                for index in 0..ENTITIES {
                    let brick_x = (index % 8) as i64 - 4;
                    world
                        .spawn_entity(
                            &id("test:cow"),
                            pose_in(brick_x, (index % 30) as f32),
                            ComponentBag::new(),
                        )
                        .unwrap();
                }
            } else if moving_every > 0 {
                for index in 0..ENTITIES {
                    if (index + tick.get()) % moving_every == 0 {
                        let brick_x = (index % 8) as i64 - 4;
                        let offset = ((index + tick.get()) % 30) as f32;
                        world
                            .set_entity_pose(EntityId::new(index + 1), pose_in(brick_x, offset))
                            .unwrap();
                    }
                }
            }
        });
        let mut clients: Vec<TestClient> = (0..CLIENTS).map(|_| harness.connect()).collect();
        for client in &mut clients {
            client.hello();
        }
        harness.server.step();
        for client in &mut clients {
            client.watch(BrickCoordinate::ORIGIN, 4);
        }
        harness.server.step();
        harness.server.step();
        for client in &mut clients {
            client.raw_frames();
        }
        let ticks = 100u32;
        let start = std::time::Instant::now();
        let mut bytes = 0usize;
        let mut frames = 0usize;
        for _ in 0..ticks {
            harness.server.step();
            for client in &mut clients {
                let received = client.raw_frames();
                frames += received.len();
                bytes += received.iter().map(Vec::len).sum::<usize>();
            }
        }
        let per_tick = start.elapsed().as_secs_f64() * 1000.0 / f64::from(ticks);
        println!(
            "| {} | {per_tick:.2} | {} | {} |",
            100u64
                .checked_div(moving_every)
                .map_or_else(|| "none".to_owned(), |percent| format!("{percent}%")),
            bytes / (ticks as usize * CLIENTS),
            frames / (ticks as usize * CLIENTS)
        );
    }
}

#[test]
fn entities_made_in_play_are_in_the_saved_world_and_a_server_started_from_it_tells_clients_of_them()
{
    let (game, mut harness) = herd_harness(|tick, world| {
        if tick.get() == 1 {
            world
                .spawn_entity(&id("test:cow"), pose_in(0, 3.0), ComponentBag::new())
                .unwrap();
            let second = world
                .spawn_entity(&id("test:cow"), pose_in(1, 4.0), ComponentBag::new())
                .unwrap();
            world.despawn_entity(second);
        }
    });
    harness.server.step();
    let saved = harness.server.save(&game.registries()).unwrap();

    // A new server, started from the file, with the same kinds, knows the entity and the counter.
    let mut reloaded = Game::new(&ALL_PLUGINS);
    let (world, _) = reloaded.load(&saved).unwrap();
    assert_eq!(world.entities().len(), 1);
    assert_eq!(world.entities().next_id(), 3);
    let logic = ScriptedLogic::new().with_entity_catalog(cow_catalog());
    let mut harness = Harness::with_logic(&reloaded, world, ServerConfig::new(game_id()), logic);
    let mut client = harness.connect();
    client.hello();
    harness.server.step();
    client.watch(BrickCoordinate::ORIGIN, 1);
    harness.server.step();
    let told = client.received();
    let told = entity_messages(&told);
    assert_eq!(told.len(), 1);
    let ServerMessage::EntitiesSpawned(spawned) = told[0] else {
        panic!("{told:?}");
    };
    assert_eq!(spawned.len(), 1);
    assert_eq!(spawned[0].id, EntityId::new(1));
    assert_eq!(spawned[0].pose, pose_in(0, 3.0));
    assert_eq!(spawned[0].components, [health_bytes(10)]);
}

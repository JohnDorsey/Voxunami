//! What clients see of a server, checked through the loopback transport with the pretend game of
//! `world_storage::test_support`.

use std::sync::{Arc, Mutex};

use namespaced_id::NamespacedId;
use protocol::{ClientId, ClientMessage, PROTOCOL_VERSION, ServerMessage, Tick};
use server::test_support::{LogicEvent, ScriptedLogic};
use server::{Audience, LeaveReason, Server, ServerConfig, StepReport};
use transport::loopback::{LoopbackConnection, LoopbackConnector, loopback_listener};
use transport::{Connection, ConnectionClosed};
use voxel_space::{BrickBox, BrickCoordinate, VoxelCoordinate};
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
                    .overrides
                    .layout()
                    .channel_index(&id("core:block"))
                    .unwrap();
                let at = VoxelCoordinate::new(
                    i64::from(payload[0] as i8),
                    i64::from(payload[1] as i8),
                    i64::from(payload[2] as i8),
                );
                world.set_voxel(channel, at, u32::from(payload[3])).unwrap();
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
    assert_eq!(described.overrides.brick_count(), 0);
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
            ServerMessage::BrickContents { brick, .. } => Some(*brick),
            _ => None,
        })
        .collect();
    assert_eq!(sent_bricks, [brick(0, 0, 0)]);
    assert!(
        matches!(messages[0], ServerMessage::InterestChanged { .. }),
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
            ServerMessage::BrickContents { brick, .. } => Some(*brick),
            _ => None,
        })
        .collect();
    assert_eq!(sent_bricks, [brick(3, 0, 0)]);
    let ServerMessage::InterestChanged {
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
    let expected = Arc::clone(world.overrides.brick(brick(0, 0, 0)).unwrap());
    let mut harness = Harness::new(&game, world, ServerConfig::new(game_id()));
    let mut client = harness.connect();
    client.hello();
    client.watch(BrickCoordinate::ORIGIN, 0);
    harness.server.step();
    let contents = client
        .received()
        .into_iter()
        .find_map(|m| match m {
            ServerMessage::BrickContents { contents, .. } => Some(contents),
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
                ServerMessage::BrickContents { .. } | ServerMessage::VoxelEdits { .. }
            ))
            .count(),
        1,
        "{messages:?}"
    );
    assert!(matches!(messages[1], ServerMessage::BrickContents { .. }));
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
                Audience::Watching(BrickCoordinate::ORIGIN),
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
            .overrides
            .layout()
            .channel_index(&id("core:block"))
            .unwrap();
        // Brick (100, 0, 0) was generated by fancy_terrain:cliffs_v1; brick (-50, 0, 0) is fine.
        let unavailable = world.set_voxel(channel, VoxelCoordinate::new(100 * 32, 5, 5), grass);
        let available = world.set_voxel(channel, VoxelCoordinate::new(-50 * 32, 5, 5), grass);
        *recorded.lock().unwrap() = Some((unavailable, available));
    });
    let mut harness = Harness::with_logic(&reduced, world, ServerConfig::new(game_id()), logic);
    harness.server.step();
    let (unavailable, available) = outcome.lock().unwrap().take().unwrap();
    assert_eq!(
        unavailable.unwrap_err().generator,
        id("fancy_terrain:cliffs_v1")
    );
    assert_eq!(available, Ok(true));
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
            ServerMessage::BrickContents { brick, .. } => Some(*brick),
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
    assert_eq!(harness.server.world().overrides.brick_count(), 0);
}

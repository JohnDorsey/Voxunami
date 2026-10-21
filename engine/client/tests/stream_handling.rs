//! How a client follows a stream of server messages, checked against a hand-made server so each
//! rule and each failure can be provoked exactly.

use std::sync::Arc;

use client::{Client, ClientError, ClientEvent, Handshake, HandshakeProgress};
use identity::Identity;
use protocol::{
    ClientId, ClientMessage, GeneratorAssignment, PROTOCOL_VERSION, ServerMessage, Tick,
    VoxelChange, Welcome,
};
use region_octree::BrickCube;
use transport::Connection;
use transport::loopback::{LoopbackConnection, loopback_pair};
use voxel_grid::Brick;
use voxel_space::{
    BrickBox, BrickCoordinate, GridId, GridPose, LevelIndex, LocalVoxelIndex, Orientation,
    VoxelCoordinate, WorldPosition,
};
use world_storage::TranslationError;
use world_storage::test_support::{ALL_PLUGINS, Game, Plugin, brick, id};

fn game_id() -> namespaced_id::NamespacedId {
    id("test_game:game")
}

fn identity() -> Identity {
    Identity::from_secret([3; 32])
}

fn welcome(game: &Game, tick: u64) -> ServerMessage {
    ServerMessage::Welcome(Welcome {
        protocol_version: PROTOCOL_VERSION,
        game: game_id(),
        client: ClientId::new(7),
        ticks_per_second: 20,
        tick: Tick::new(tick),
        world_description: game
            .new_world()
            .describe(&game.registries())
            .expect("the fixture world can be described"),
        package: Vec::new(),
    })
}

/// A client connected to a server the test plays by hand.
struct Rig {
    /// The game the pretend server runs: all plugins.
    game: Game,
    /// The game the client runs, whose registries show how the client names what it was sent.
    client_game: Game,
    server: LoopbackConnection,
    client: Client<LoopbackConnection>,
}

impl Rig {
    fn new() -> Self {
        Self::with_plugins(&ALL_PLUGINS)
    }

    fn with_plugins(plugins: &[Plugin]) -> Self {
        // The server side always runs the full game; the client may have fewer plugins.
        let server_game = Game::new(&ALL_PLUGINS);
        let (client_end, mut server) = loopback_pair();
        let handshake = Handshake::start(client_end, game_id(), &identity()).unwrap();
        let hello = ClientMessage::decode(&server.receive().unwrap().unwrap()).unwrap();
        assert_eq!(
            hello,
            ClientMessage::Hello {
                protocol_version: PROTOCOL_VERSION,
                game: game_id(),
                player_key: identity().player_key(),
            }
        );
        server.send(welcome(&server_game, 4).encode()).unwrap();
        let HandshakeProgress::Welcomed(welcomed) = handshake.poll().unwrap() else {
            panic!("the welcome was sent");
        };
        let mut client_game = Game::new(plugins);
        let (client, _) = welcomed.finish(client_game.load_context()).unwrap();
        Self {
            game: server_game,
            client_game,
            server,
            client,
        }
    }

    fn send(&mut self, message: ServerMessage) {
        self.server.send(message.encode()).unwrap();
    }

    fn tick(&mut self, number: u64) {
        self.send(ServerMessage::TickCompleted {
            tick: Tick::new(number),
        });
    }

    /// Watches the region around the origin, as the first tick after the welcome (tick 5).
    fn watch_origin(&mut self, radius: u32) {
        self.send(ServerMessage::InterestChanged {
            level: LevelIndex::MAIN,
            region: BrickBox::around(BrickCoordinate::ORIGIN, radius),
            assignments: vec![assignment_everywhere(0)],
        });
        self.tick(5);
        self.client.poll_tick().unwrap().unwrap();
    }

    fn edited_brick(&self, at: [i64; 3], block: &str) -> Arc<Brick> {
        let mut world = self.game.new_world();
        self.game.set_block(&mut world, at, block);
        let coordinate = VoxelCoordinate::new(at[0], at[1], at[2]).split().0;
        Arc::clone(world.main().overrides.brick(coordinate).unwrap())
    }
}

fn assignment_everywhere(generator: u32) -> GeneratorAssignment {
    GeneratorAssignment {
        cube: BrickCube::new(BrickCoordinate::new(0, 0, 0), 3).unwrap(),
        generator: Some(generator),
    }
}

#[test]
fn connecting_sends_hello_waits_for_the_welcome_and_loads_the_world() {
    let game = Game::new(&ALL_PLUGINS);
    let (client_end, mut server) = loopback_pair();
    let handshake = Handshake::start(client_end, game_id(), &identity()).unwrap();
    // Nothing from the server yet: still waiting.
    let handshake = match handshake.poll().unwrap() {
        HandshakeProgress::Waiting(handshake) => handshake,
        HandshakeProgress::Welcomed(_) => panic!("no welcome was sent"),
    };
    server.send(welcome(&game, 41).encode()).unwrap();
    let HandshakeProgress::Welcomed(welcomed) = handshake.poll().unwrap() else {
        panic!("the welcome arrived");
    };
    assert_eq!(welcomed.world_seed().unwrap(), 0xFEED_FACE);
    assert_eq!(welcomed.welcome().client, ClientId::new(7));

    let mut client_game = Game::new(&ALL_PLUGINS);
    let (client, report) = welcomed.finish(client_game.load_context()).unwrap();
    assert!(report.missing_content.is_empty());
    let world = client.world();
    assert_eq!(world.tick(), Tick::new(41));
    assert_eq!(world.watched(), None);
    assert_eq!(world.server_info().game, game_id());
    assert_eq!(world.server_info().client, ClientId::new(7));
    assert_eq!(world.server_info().ticks_per_second, 20);
    assert!(world.verifies_edit_digests());
    assert_eq!(world.world().world_seed, 0xFEED_FACE);
}

#[test]
fn a_refusal_a_wrong_first_message_and_a_wrong_version_fail_the_handshake() {
    let game = Game::new(&ALL_PLUGINS);
    let outcome = |frame: Vec<u8>| {
        let (client_end, mut server) = loopback_pair();
        let handshake = Handshake::start(client_end, game_id(), &identity()).unwrap();
        server.send(frame).unwrap();
        handshake.poll().expect_err("the handshake fails")
    };
    assert!(matches!(
        outcome(ServerMessage::Disconnected { reason: "full".to_owned() }.encode()),
        ClientError::DisconnectedByServer(reason) if reason == "full"
    ));
    assert!(matches!(
        outcome(ServerMessage::TickCompleted { tick: Tick::new(1) }.encode()),
        ClientError::UnexpectedMessage(_)
    ));
    assert!(matches!(outcome(vec![250]), ClientError::Protocol(_)));

    // A welcome in a protocol version this program does not speak is refused at `finish`.
    let ServerMessage::Welcome(mut stale) = welcome(&game, 0) else {
        unreachable!()
    };
    stale.protocol_version = PROTOCOL_VERSION + 1;
    let (client_end, mut server) = loopback_pair();
    let handshake = Handshake::start(client_end, game_id(), &identity()).unwrap();
    server.send(ServerMessage::Welcome(stale).encode()).unwrap();
    let HandshakeProgress::Welcomed(welcomed) = handshake.poll().unwrap() else {
        panic!("welcomed")
    };
    let error = welcomed
        .finish(Game::new(&ALL_PLUGINS).load_context())
        .err()
        .unwrap();
    assert!(
        matches!(error, ClientError::UnsupportedProtocolVersion(v) if v == PROTOCOL_VERSION + 1)
    );

    // A garbage description is a failure to load, not a panic.
    let ServerMessage::Welcome(mut broken) = welcome(&game, 0) else {
        unreachable!()
    };
    broken.world_description = vec![1, 2, 3];
    let (client_end, mut server) = loopback_pair();
    let handshake = Handshake::start(client_end, game_id(), &identity()).unwrap();
    server
        .send(ServerMessage::Welcome(broken).encode())
        .unwrap();
    let HandshakeProgress::Welcomed(welcomed) = handshake.poll().unwrap() else {
        panic!("welcomed")
    };
    assert!(welcomed.world_seed().is_err());

    // The server vanishing mid-handshake.
    let (client_end, server) = loopback_pair();
    let handshake = Handshake::start(client_end, game_id(), &identity()).unwrap();
    drop(server);
    assert!(matches!(
        handshake.poll().err().unwrap(),
        ClientError::ConnectionClosed
    ));
}

#[test]
fn a_tick_is_applied_only_when_all_of_it_has_arrived() {
    let mut rig = Rig::new();
    rig.send(ServerMessage::InterestChanged {
        level: LevelIndex::MAIN,
        region: BrickBox::around(BrickCoordinate::ORIGIN, 1),
        assignments: vec![assignment_everywhere(0)],
    });
    // Half a tick: the message is held back, the view unchanged.
    assert_eq!(rig.client.poll_tick().unwrap(), None);
    assert_eq!(rig.client.world().watched(), None);
    rig.tick(5);
    let update = rig.client.poll_tick().unwrap().expect("the tick completed");
    assert_eq!(update.tick, Tick::new(5));
    assert_eq!(
        update.events,
        [ClientEvent::WatchedRegionChanged {
            level: LevelIndex::MAIN,
            region: BrickBox::around(BrickCoordinate::ORIGIN, 1)
        }]
    );
    assert_eq!(rig.client.world().tick(), Tick::new(5));
    assert_eq!(rig.client.poll_tick().unwrap(), None);
    // Several ticks arrive together and are applied one poll at a time.
    rig.tick(6);
    rig.tick(7);
    assert_eq!(rig.client.poll_tick().unwrap().unwrap().tick, Tick::new(6));
    assert_eq!(rig.client.poll_tick().unwrap().unwrap().tick, Tick::new(7));
}

#[test]
fn the_client_generates_terrain_where_the_server_named_a_generator() {
    let mut rig = Rig::new();
    let before = rig
        .client
        .world()
        .world()
        .main()
        .effective_brick(brick(0, 0, 0));
    assert!(matches!(before, world_storage::BrickSource::Unassigned));
    rig.watch_origin(1);
    let after = rig
        .client
        .world()
        .world()
        .main()
        .effective_brick(brick(0, 0, 0));
    let world_storage::BrickSource::Generated(generated) = after else {
        panic!("{after:?}");
    };
    // Flat terrain at height 10: stone at the bottom of brick (0, 0, 0), air above the surface.
    let block = rig
        .client
        .world()
        .world()
        .main()
        .overrides
        .layout()
        .channel_index(&id("core:block"))
        .unwrap();
    assert_eq!(
        generated.get(block, LocalVoxelIndex::new(3, 0, 3)),
        rig.game.raw_block("base_blocks:stone")
    );
    assert_eq!(
        generated.get(block, LocalVoxelIndex::new(3, 20, 3)),
        rig.game.raw_block("base_blocks:air")
    );
}

#[test]
fn edited_bricks_replace_generated_terrain_and_edits_change_them() {
    let mut rig = Rig::new();
    rig.watch_origin(1);
    let marble = rig.game.raw_block("fancy_blocks:marble");
    let contents = rig.edited_brick([1, 2, 3], "fancy_blocks:marble");
    rig.send(ServerMessage::BrickContents {
        grid: GridId::MAIN,
        brick: brick(0, 0, 0),
        contents: Arc::clone(&contents),
    });
    rig.tick(6);
    let update = rig.client.poll_tick().unwrap().unwrap();
    assert_eq!(
        update.events,
        [ClientEvent::BrickChanged {
            grid: GridId::MAIN,
            brick: brick(0, 0, 0)
        }]
    );
    let layout = Arc::clone(rig.client.world().world().main().overrides.layout());
    let block_channel = layout.channel_index(&id("core:block")).unwrap();
    let held = rig
        .client
        .world()
        .world()
        .main()
        .overrides
        .brick(brick(0, 0, 0))
        .unwrap();
    assert_eq!(**held, *contents);
    assert_eq!(
        held.get(block_channel, VoxelCoordinate::new(1, 2, 3).split().1),
        marble
    );

    // An edit on top, with the server's digest of the result.
    let mut edited = (*contents).clone();
    let dirt = rig.game.raw_block("base_blocks:dirt");
    let voxel = VoxelCoordinate::new(9, 9, 9).split().1;
    edited.set(block_channel, voxel, dirt);
    rig.send(ServerMessage::VoxelEdits {
        grid: GridId::MAIN,
        brick: brick(0, 0, 0),
        changes: vec![VoxelChange {
            voxel,
            channel: block_channel.get() as u16,
            value: dirt,
        }],
        resulting_digest: edited.content_digest(),
    });
    rig.tick(7);
    rig.client.poll_tick().unwrap().unwrap();
    assert_eq!(
        **rig
            .client
            .world()
            .world()
            .main()
            .overrides
            .brick(brick(0, 0, 0))
            .unwrap(),
        edited
    );
}

#[test]
fn moving_the_watched_region_forgets_bricks_left_behind_and_keeps_the_rest() {
    let mut rig = Rig::new();
    rig.watch_origin(2);
    for at in [[1, 2, 3], [40, 2, 3], [-60, 2, 3]] {
        let coordinate = VoxelCoordinate::new(at[0], at[1], at[2]).split().0;
        let contents = rig.edited_brick(at, "fancy_blocks:marble");
        rig.send(ServerMessage::BrickContents {
            grid: GridId::MAIN,
            brick: coordinate,
            contents,
        });
    }
    rig.tick(6);
    rig.client.poll_tick().unwrap().unwrap();
    assert_eq!(rig.client.world().world().main().overrides.brick_count(), 3);

    // Bricks (0), (1) and (-2) were edited; the new region spans x = 0..=4, so (-2) is left behind.
    rig.send(ServerMessage::InterestChanged {
        level: LevelIndex::MAIN,
        region: BrickBox::new(brick(0, -2, -2), brick(4, 2, 2)).unwrap(),
        assignments: vec![],
    });
    rig.tick(7);
    rig.client.poll_tick().unwrap().unwrap();
    assert_eq!(
        rig.client
            .world()
            .world()
            .main()
            .overrides
            .sorted_brick_coordinates(),
        [brick(0, 0, 0), brick(1, 0, 0)]
    );
}

#[test]
fn a_stream_that_breaks_the_rules_is_an_error_not_a_wrong_picture() {
    // Each case sends one bad message after watching a small region, then completes the tick.
    let contents_of = |rig: &Rig| rig.edited_brick([1, 2, 3], "fancy_blocks:marble");
    type Case = (&'static str, Box<dyn Fn(&Rig) -> Vec<ServerMessage>>);
    let cases: Vec<Case> = vec![
        (
            "a brick outside the watched region",
            Box::new(move |rig| {
                vec![ServerMessage::BrickContents {
                    grid: GridId::MAIN,
                    brick: brick(30, 0, 0),
                    contents: contents_of(rig),
                }]
            }),
        ),
        (
            "an edit to a brick never sent",
            Box::new(|_| {
                vec![ServerMessage::VoxelEdits {
                    grid: GridId::MAIN,
                    brick: brick(0, 0, 0),
                    changes: vec![],
                    resulting_digest: 0,
                }]
            }),
        ),
        (
            "a second welcome",
            Box::new(|rig| vec![welcome(&rig.game, 9)]),
        ),
        (
            "the server disconnecting",
            Box::new(|_| {
                vec![ServerMessage::Disconnected {
                    reason: "bye".to_owned(),
                }]
            }),
        ),
        (
            "a generator the description never listed",
            Box::new(|_| {
                vec![ServerMessage::InterestChanged {
                    level: LevelIndex::MAIN,
                    region: BrickBox::around(BrickCoordinate::ORIGIN, 1),
                    assignments: vec![assignment_everywhere(99)],
                }]
            }),
        ),
        (
            "a brick with the wrong number of channels",
            Box::new(|_| {
                vec![ServerMessage::BrickContents {
                    grid: GridId::MAIN,
                    brick: brick(0, 0, 0),
                    contents: Arc::new(Brick::from_channels(vec![
                        voxel_grid::ChannelStorage::uniform(0),
                    ])),
                }]
            }),
        ),
    ];
    for (name, make) in cases {
        let mut rig = Rig::new();
        rig.watch_origin(1);
        for message in make(&rig) {
            rig.send(message);
        }
        rig.tick(6);
        let error = rig.client.poll_tick().expect_err(name);
        let recognised = match (name, &error) {
            (
                "a brick outside the watched region",
                ClientError::BrickOutsideWatchedRegion(_, _),
            ) => true,
            ("an edit to a brick never sent", ClientError::EditToUnknownBrick(_, _)) => true,
            ("a second welcome", ClientError::UnexpectedMessage(_)) => true,
            ("the server disconnecting", ClientError::DisconnectedByServer(reason)) => {
                reason == "bye"
            }
            (
                "a generator the description never listed",
                ClientError::Translation(TranslationError::GeneratorOutsideTable(99)),
            ) => true,
            (
                "a brick with the wrong number of channels",
                ClientError::Translation(TranslationError::BrickChannelCountMismatch { .. }),
            ) => true,
            _ => false,
        };
        assert!(recognised, "{name}: {error}");
    }
}

#[test]
fn ticks_must_be_consecutive_and_edits_must_match_the_servers_digest() {
    let mut rig = Rig::new();
    rig.watch_origin(1);
    rig.tick(7); // 6 was due
    assert!(matches!(
        rig.client.poll_tick().unwrap_err(),
        ClientError::TickOutOfOrder { expected, found } if expected == Tick::new(6) && found == Tick::new(7)
    ));

    let mut rig = Rig::new();
    rig.watch_origin(1);
    let contents = rig.edited_brick([1, 2, 3], "fancy_blocks:marble");
    rig.send(ServerMessage::BrickContents {
        grid: GridId::MAIN,
        brick: brick(0, 0, 0),
        contents: Arc::clone(&contents),
    });
    rig.send(ServerMessage::VoxelEdits {
        grid: GridId::MAIN,
        brick: brick(0, 0, 0),
        changes: vec![],
        resulting_digest: contents.content_digest() ^ 1,
    });
    rig.tick(6);
    assert!(matches!(
        rig.client.poll_tick().unwrap_err(),
        ClientError::OutOfSync(GridId::MAIN, coordinate) if coordinate == BrickCoordinate::ORIGIN
    ));
}

#[test]
fn edits_to_channels_or_values_the_server_has_no_table_for_are_refused() {
    let voxel = LocalVoxelIndex::new(0, 0, 0);
    let bad_edits = [
        // No such channel in the server's table.
        (50, 0),
        // Channel 0 is the block channel; this block id is not in the server's block table.
        (0, 5000),
    ];
    for (channel, value) in bad_edits {
        let mut rig = Rig::new();
        rig.watch_origin(1);
        let contents = rig.edited_brick([1, 2, 3], "fancy_blocks:marble");
        rig.send(ServerMessage::BrickContents {
            grid: GridId::MAIN,
            brick: brick(0, 0, 0),
            contents,
        });
        rig.send(ServerMessage::VoxelEdits {
            grid: GridId::MAIN,
            brick: brick(0, 0, 0),
            changes: vec![VoxelChange {
                voxel,
                channel,
                value,
            }],
            resulting_digest: 0,
        });
        rig.tick(6);
        assert!(matches!(
            rig.client.poll_tick().unwrap_err(),
            ClientError::Translation(_)
        ));
    }
}

#[test]
fn game_messages_arrive_as_events_in_order_and_requests_reach_the_server() {
    let mut rig = Rig::new();
    for payload in [1u8, 2] {
        rig.send(ServerMessage::GameMessage {
            kind: id("test_game:note"),
            payload: vec![payload],
        });
    }
    rig.tick(5);
    let update = rig.client.poll_tick().unwrap().unwrap();
    assert_eq!(
        update.events,
        [
            ClientEvent::GameMessage {
                kind: id("test_game:note"),
                payload: vec![1]
            },
            ClientEvent::GameMessage {
                kind: id("test_game:note"),
                payload: vec![2]
            }
        ]
    );

    let region = BrickBox::around(brick(3, 3, 3), 1);
    rig.client.watch(LevelIndex::MAIN, region).unwrap();
    rig.client
        .send_game_message(id("test_game:chat"), vec![9])
        .unwrap();
    rig.client.say_goodbye().unwrap();
    let mut sent = Vec::new();
    while let Some(frame) = rig.server.receive().unwrap() {
        sent.push(ClientMessage::decode(&frame).unwrap());
    }
    assert_eq!(
        sent,
        [
            ClientMessage::SetInterest {
                level: LevelIndex::MAIN,
                region
            },
            ClientMessage::GameMessage {
                kind: id("test_game:chat"),
                payload: vec![9]
            },
            ClientMessage::Goodbye
        ]
    );
    drop(rig.server);
    assert!(matches!(
        rig.client.say_goodbye().unwrap_err(),
        ClientError::ConnectionClosed
    ));
    assert!(matches!(
        rig.client.poll_tick().unwrap_err(),
        ClientError::ConnectionClosed
    ));
}

#[test]
fn a_garbage_frame_is_a_protocol_error() {
    let mut rig = Rig::new();
    rig.server.send(vec![0xEE, 1, 2, 3]).unwrap();
    assert!(matches!(
        rig.client.poll_tick().unwrap_err(),
        ClientError::Protocol(_)
    ));
}

#[test]
fn views_built_from_the_same_messages_are_equal_and_different_messages_differ() {
    let play = |extra_edit: bool| {
        let mut rig = Rig::new();
        rig.watch_origin(1);
        let contents = rig.edited_brick([1, 2, 3], "fancy_blocks:marble");
        rig.send(ServerMessage::BrickContents {
            grid: GridId::MAIN,
            brick: brick(0, 0, 0),
            contents,
        });
        if extra_edit {
            let contents = rig.edited_brick([40, 2, 3], "fancy_blocks:glass");
            rig.send(ServerMessage::BrickContents {
                grid: GridId::MAIN,
                brick: brick(1, 0, 0),
                contents,
            });
        }
        rig.tick(6);
        rig.client.poll_tick().unwrap().unwrap();
        rig.client.world().summary()
    };
    assert_eq!(play(false), play(false));
    assert_ne!(play(false), play(true));
    let summary = play(false);
    assert_eq!(summary.tick, Tick::new(6));
    assert_eq!(
        summary.watched,
        Some((
            LevelIndex::MAIN,
            BrickBox::around(BrickCoordinate::ORIGIN, 1)
        ))
    );
    // The cube at the origin has the flat generator; the rest of space is unassigned.
    assert!(
        summary.levels[0]
            .generator_assignments
            .iter()
            .any(|(cube, name)| cube.origin() == BrickCoordinate::ORIGIN
                && *name == Some(id("base_terrain:flat_v1")))
    );
}

#[test]
fn a_client_with_different_plugins_follows_the_stream_and_sees_the_same_meaning() {
    // The server has every plugin; this client has only base blocks, so its numbering of content
    // differs and the content it lacks is held as placeholders that keep their names.
    let mut rig = Rig::with_plugins(&[Plugin::BaseBlocks]);
    assert!(
        !rig.client.world().verifies_edit_digests(),
        "numbering differs, so the server's digests cannot be compared"
    );
    rig.watch_origin(1);
    let contents = rig.edited_brick([1, 2, 3], "fancy_blocks:marble");
    rig.send(ServerMessage::BrickContents {
        grid: GridId::MAIN,
        brick: brick(0, 0, 0),
        contents: Arc::clone(&contents),
    });
    // An edit, with a digest that could not match in the client's numbering.
    let server_layout = Arc::clone(rig.game.new_world().main().overrides.layout());
    let block_channel = server_layout.channel_index(&id("core:block")).unwrap();
    let voxel = VoxelCoordinate::new(9, 9, 9).split().1;
    let dirt = rig.game.raw_block("base_blocks:dirt");
    rig.send(ServerMessage::VoxelEdits {
        grid: GridId::MAIN,
        brick: brick(0, 0, 0),
        changes: vec![VoxelChange {
            voxel,
            channel: block_channel.get() as u16,
            value: dirt,
        }],
        resulting_digest: 0,
    });
    rig.tick(6);
    rig.client.poll_tick().unwrap().unwrap();

    let mut expected = (*contents).clone();
    expected.set(block_channel, voxel, dirt);
    let client_layout = Arc::clone(rig.client.world().world().main().overrides.layout());
    let held = rig
        .client
        .world()
        .world()
        .main()
        .overrides
        .brick(brick(0, 0, 0))
        .unwrap();
    let client_meaning = rig.client_game.brick_meaning(&client_layout, held);
    let server_meaning = rig.game.brick_meaning(&server_layout, &expected);
    for channel in ["core:block", "core:temperature"] {
        assert_eq!(
            client_meaning[channel], server_meaning[channel],
            "{channel}"
        );
    }
    // The placeholder for marble is the client's own entry, named as the server named it.
    let client_block = client_layout.channel_index(&id("core:block")).unwrap();
    let raw = held.get(client_block, VoxelCoordinate::new(1, 2, 3).split().1);
    assert_eq!(
        rig.client_game
            .value_name(client_layout.definition(client_block), raw),
        "fancy_blocks:marble"
    );
}

// ---------------------------------------------------------------------------------------------
// Levels and commands
// ---------------------------------------------------------------------------------------------

impl Rig {
    /// A client of a server whose world has a second level, `test_content:second`.
    fn with_two_levels() -> Self {
        let server_game = Game::new(&ALL_PLUGINS);
        let mut world = server_game.new_world();
        world
            .add_level(
                id("test_content:second"),
                server_game.new_level("fancy_terrain:cliffs_v1"),
            )
            .unwrap();
        let (client_end, mut server) = loopback_pair();
        let handshake = Handshake::start(client_end, game_id(), &identity()).unwrap();
        server.receive().unwrap().unwrap();
        server
            .send(
                ServerMessage::Welcome(Welcome {
                    protocol_version: PROTOCOL_VERSION,
                    game: game_id(),
                    client: ClientId::new(7),
                    ticks_per_second: 20,
                    tick: Tick::new(4),
                    world_description: world.describe(&server_game.registries()).unwrap(),
                    package: Vec::new(),
                })
                .encode(),
            )
            .unwrap();
        let HandshakeProgress::Welcomed(welcomed) = handshake.poll().unwrap() else {
            panic!("the welcome was sent");
        };
        let mut client_game = Game::new(&ALL_PLUGINS);
        let (client, _) = welcomed.finish(client_game.load_context()).unwrap();
        Self {
            game: server_game,
            client_game,
            server,
            client,
        }
    }

    /// Watches the region around the origin of a level, as the next tick after `after`.
    fn watch_level_origin(&mut self, level: LevelIndex, radius: u32, tick: u64) {
        self.send(ServerMessage::InterestChanged {
            level,
            region: BrickBox::around(BrickCoordinate::ORIGIN, radius),
            assignments: vec![assignment_everywhere(0)],
        });
        self.tick(tick);
        self.client.poll_tick().unwrap().unwrap();
    }
}

const SECOND: LevelIndex = LevelIndex::new(1);

#[test]
fn the_client_knows_every_level_the_server_described() {
    let rig = Rig::with_two_levels();
    let world = rig.client.world().world();
    assert_eq!(world.level_count(), 2);
    assert_eq!(world.level_id(SECOND), Some(&id("test_content:second")));
    assert_eq!(rig.client.world().watched(), None);
    assert!(rig.client.world().watched_level().is_none());
}

#[test]
fn following_the_server_into_another_level_forgets_what_was_held_of_the_first() {
    let mut rig = Rig::with_two_levels();
    rig.watch_level_origin(LevelIndex::MAIN, 1, 5);
    rig.send(ServerMessage::BrickContents {
        grid: GridId::MAIN,
        brick: brick(0, 0, 0),
        contents: rig.edited_brick([1, 2, 3], "fancy_blocks:marble"),
    });
    rig.tick(6);
    rig.client.poll_tick().unwrap().unwrap();
    assert_eq!(rig.client.world().world().main().overrides.brick_count(), 1);

    // The same region of the second level: the first level's bricks are not the second's.
    rig.send(ServerMessage::InterestChanged {
        level: SECOND,
        region: BrickBox::around(BrickCoordinate::ORIGIN, 1),
        assignments: vec![assignment_everywhere(1)],
    });
    rig.send(ServerMessage::BrickContents {
        grid: GridId::root(SECOND),
        brick: brick(1, 0, 0),
        contents: rig.edited_brick([33, 2, 3], "base_blocks:dirt"),
    });
    rig.tick(7);
    let update = rig.client.poll_tick().unwrap().unwrap();
    assert_eq!(
        update.events,
        [
            ClientEvent::WatchedRegionChanged {
                level: SECOND,
                region: BrickBox::around(BrickCoordinate::ORIGIN, 1)
            },
            ClientEvent::BrickChanged {
                grid: GridId::root(SECOND),
                brick: brick(1, 0, 0)
            }
        ]
    );
    let world = rig.client.world().world();
    assert_eq!(
        world.main().overrides.brick_count(),
        0,
        "the main level was forgotten"
    );
    assert_eq!(world.level(SECOND).unwrap().overrides.brick_count(), 1);
    assert_eq!(
        rig.client.world().watched(),
        Some((SECOND, BrickBox::around(BrickCoordinate::ORIGIN, 1)))
    );
    let (level, data) = rig.client.world().watched_level().unwrap();
    assert_eq!(level, SECOND);
    assert!(std::ptr::eq(data, world.level(SECOND).unwrap()));
    // The second level uses the generator the server assigned it: entry 1 of the shared table.
    let second = world.level(SECOND).unwrap();
    let assigned = second.terrain.assigned_generator(brick(0, 0, 0)).unwrap();
    assert_eq!(
        second
            .terrain
            .generators()
            .entry(assigned)
            .unwrap()
            .identifier(),
        &id("fancy_terrain:cliffs_v1")
    );
    assert!(
        world
            .main()
            .terrain
            .assigned_generator(brick(0, 0, 0))
            .is_some()
    );
}

#[test]
fn the_summary_tells_the_levels_apart() {
    let mut rig = Rig::with_two_levels();
    rig.watch_level_origin(SECOND, 1, 5);
    let before = rig.client.world().summary();
    assert_eq!(before.levels.len(), 2);
    assert_eq!(
        before.watched,
        Some((SECOND, BrickBox::around(BrickCoordinate::ORIGIN, 1)))
    );
    rig.send(ServerMessage::BrickContents {
        grid: GridId::root(SECOND),
        brick: brick(0, 0, 0),
        contents: rig.edited_brick([1, 2, 3], "fancy_blocks:marble"),
    });
    rig.tick(6);
    rig.client.poll_tick().unwrap().unwrap();
    let after = rig.client.world().summary();
    assert_eq!(
        after.levels[0], before.levels[0],
        "the main level did not change"
    );
    assert_ne!(
        after.levels[1].edited_bricks,
        before.levels[1].edited_bricks
    );
}

#[test]
fn what_names_a_level_the_world_does_not_have_or_one_that_is_not_watched_is_an_error() {
    type Case = (&'static str, ServerMessage, fn(&ClientError) -> bool);
    let cases: Vec<Case> = vec![
        (
            "interest in a level that does not exist",
            ServerMessage::InterestChanged {
                level: LevelIndex::new(5),
                region: BrickBox::around(BrickCoordinate::ORIGIN, 1),
                assignments: vec![],
            },
            |error| matches!(error, ClientError::UnknownLevel(level) if level.get() == 5),
        ),
        (
            "an edit in a level that does not exist",
            ServerMessage::VoxelEdits {
                grid: GridId::root(LevelIndex::new(5)),
                brick: brick(0, 0, 0),
                changes: vec![],
                resulting_digest: 0,
            },
            |error| matches!(error, ClientError::UnknownLevel(level) if level.get() == 5),
        ),
        (
            "contents of a level that is not the watched one",
            ServerMessage::BrickContents {
                grid: GridId::root(SECOND),
                brick: brick(0, 0, 0),
                contents: Arc::new(Brick::from_channels(vec![])),
            },
            |error| matches!(error, ClientError::BrickOutsideWatchedRegion(grid, _) if *grid == GridId::root(SECOND)),
        ),
        (
            "an edit to a brick held in another level",
            ServerMessage::VoxelEdits {
                grid: GridId::root(SECOND),
                brick: brick(0, 0, 0),
                changes: vec![],
                resulting_digest: 0,
            },
            |error| matches!(error, ClientError::EditToUnknownBrick(grid, _) if *grid == GridId::root(SECOND)),
        ),
    ];
    for (name, message, recognised) in cases {
        let mut rig = Rig::with_two_levels();
        rig.watch_level_origin(LevelIndex::MAIN, 1, 5);
        // A brick held in the main level, so an edit to the same brick of the second is the mistake.
        rig.send(ServerMessage::BrickContents {
            grid: GridId::MAIN,
            brick: brick(0, 0, 0),
            contents: rig.edited_brick([1, 2, 3], "fancy_blocks:marble"),
        });
        rig.tick(6);
        rig.client.poll_tick().unwrap().unwrap();
        rig.send(message);
        rig.tick(7);
        let error = rig.client.poll_tick().expect_err(name);
        assert!(recognised(&error), "{name}: {error}");
    }
}

#[test]
fn watching_names_the_level_and_commands_go_out_as_text_and_replies_come_in_as_events() {
    let mut rig = Rig::with_two_levels();
    rig.client
        .watch(SECOND, BrickBox::around(brick(2, 2, 2), 1))
        .unwrap();
    rig.client.send_command("/level go second").unwrap();
    let sent: Vec<ClientMessage> = std::iter::from_fn(|| rig.server.receive().unwrap())
        .map(|frame| ClientMessage::decode(&frame).unwrap())
        .collect();
    assert_eq!(
        sent,
        [
            ClientMessage::SetInterest {
                level: SECOND,
                region: BrickBox::around(brick(2, 2, 2), 1)
            },
            ClientMessage::Command {
                text: "/level go second".to_owned()
            }
        ]
    );

    rig.send(ServerMessage::CommandReply {
        text: "you are now in the second level".to_owned(),
    });
    rig.send(ServerMessage::CommandReply {
        text: "have fun".to_owned(),
    });
    rig.tick(5);
    let update = rig.client.poll_tick().unwrap().unwrap();
    assert_eq!(
        update.events,
        [
            ClientEvent::CommandReply {
                text: "you are now in the second level".to_owned()
            },
            ClientEvent::CommandReply {
                text: "have fun".to_owned()
            }
        ]
    );
}

// ---- ships ----

fn a_ship() -> GridId {
    GridId::new(GridId::FIRST_ALLOCATED + 3)
}

fn pose_at(brick_x: i64) -> GridPose {
    GridPose::new(
        WorldPosition::new(BrickCoordinate::new(brick_x, 0, 0), [1.0, 2.0, 3.0]),
        Orientation::IDENTITY,
    )
}

fn spawn_a_ship(rig: &mut Rig) {
    rig.send(ServerMessage::GridsSpawned(vec![protocol::GridState {
        id: a_ship(),
        level: LevelIndex::MAIN,
        pose: pose_at(0),
    }]));
}

#[test]
fn a_ship_is_built_from_its_bricks_moved_by_changes_checked_by_digests_and_forgotten_when_gone() {
    let mut rig = Rig::new();
    rig.watch_origin(1);
    spawn_a_ship(&mut rig);
    // A brick far from the watched region is fine for a ship: its bricks are in its own coordinates.
    let contents = rig.edited_brick([1, 2, 3], "fancy_blocks:marble");
    rig.send(ServerMessage::BrickContents {
        grid: a_ship(),
        brick: brick(30, -4, 7),
        contents: Arc::clone(&contents),
    });
    rig.tick(6);
    let update = rig.client.poll_tick().unwrap().unwrap();
    assert_eq!(
        update.events,
        [
            ClientEvent::GridsChanged,
            ClientEvent::BrickChanged {
                grid: a_ship(),
                brick: brick(30, -4, 7)
            }
        ]
    );
    let world = rig.client.world();
    let ship = world.grids().get(a_ship()).unwrap();
    assert_eq!((ship.level, ship.pose), (LevelIndex::MAIN, pose_at(0)));
    assert_eq!(ship.voxels.brick_count(), 1);
    // The terrain did not get the brick.
    assert_eq!(world.world().main().overrides.brick_count(), 0);

    // It moves.
    rig.send(ServerMessage::GridsChanged(vec![protocol::GridChange {
        id: a_ship(),
        pose: pose_at(5),
    }]));
    rig.tick(7);
    rig.client.poll_tick().unwrap().unwrap();
    assert_eq!(
        rig.client.world().grids().get(a_ship()).unwrap().pose,
        pose_at(5)
    );

    // An edit that the client reaches the same result from is applied; one that it does not is a mistake.
    let layout = Arc::clone(rig.client.world().world().main().overrides.layout());
    let block = layout.channel_index(&id("core:block")).unwrap();
    let marble = rig.game.raw_block("fancy_blocks:marble");
    let mut expected = Brick::clone(&contents);
    expected.set(block, LocalVoxelIndex::new(4, 5, 6), marble);
    let edit = |digest: u64| ServerMessage::VoxelEdits {
        grid: a_ship(),
        brick: brick(30, -4, 7),
        changes: vec![VoxelChange {
            voxel: LocalVoxelIndex::new(4, 5, 6),
            channel: block.get() as u16,
            value: marble,
        }],
        resulting_digest: digest,
    };
    rig.send(edit(expected.content_digest()));
    rig.tick(8);
    rig.client.poll_tick().unwrap().unwrap();
    let held = rig
        .client
        .world()
        .grids()
        .get(a_ship())
        .unwrap()
        .voxels
        .brick(brick(30, -4, 7))
        .unwrap();
    assert_eq!(held.content_digest(), expected.content_digest());
    // Its summary says where it is and what it holds, so that a replay can be compared with the live view.
    let summary = rig.client.world().summary();
    assert_eq!(summary.grids.len(), 1);
    assert_eq!(
        (
            summary.grids[0].id,
            summary.grids[0].pose,
            summary.grids[0].bricks
        ),
        (
            a_ship(),
            pose_at(5),
            rig.client
                .world()
                .grids()
                .get(a_ship())
                .unwrap()
                .voxels
                .content_digest()
        )
    );
    rig.send(edit(expected.content_digest() ^ 1));
    rig.tick(9);
    assert!(matches!(
        rig.client.poll_tick().unwrap_err(),
        ClientError::OutOfSync(grid, _) if grid == a_ship()
    ));

    // Gone, it is forgotten with its bricks.
    let mut rig = Rig::new();
    rig.watch_origin(1);
    spawn_a_ship(&mut rig);
    rig.tick(6);
    rig.client.poll_tick().unwrap().unwrap();
    rig.send(ServerMessage::GridsDespawned(vec![a_ship()]));
    rig.tick(7);
    let update = rig.client.poll_tick().unwrap().unwrap();
    assert_eq!(update.events, [ClientEvent::GridsChanged]);
    assert!(rig.client.world().grids().is_empty());
    assert!(rig.client.world().summary().grids.is_empty());
}

#[test]
fn what_the_server_says_of_a_ship_it_never_described_or_described_twice_is_a_mistake() {
    type Recognise = fn(&ClientError) -> bool;
    let cases: Vec<(&str, bool, ServerMessage, Recognise)> = vec![
        (
            "a ship told of twice",
            true,
            ServerMessage::GridsSpawned(vec![protocol::GridState {
                id: a_ship(),
                level: LevelIndex::MAIN,
                pose: pose_at(0),
            }]),
            |error| matches!(error, ClientError::GridAlreadyKnown(grid) if *grid == a_ship()),
        ),
        (
            "a move of a ship never told of",
            false,
            ServerMessage::GridsChanged(vec![protocol::GridChange {
                id: a_ship(),
                pose: pose_at(1),
            }]),
            |error| matches!(error, ClientError::UnknownGrid(grid) if *grid == a_ship()),
        ),
        (
            "the removal of a ship never told of",
            false,
            ServerMessage::GridsDespawned(vec![a_ship()]),
            |error| matches!(error, ClientError::UnknownGrid(grid) if *grid == a_ship()),
        ),
        (
            "a brick of a ship never told of",
            false,
            ServerMessage::BrickContents {
                grid: a_ship(),
                brick: brick(0, 0, 0),
                contents: Arc::new(Brick::from_channels(vec![])),
            },
            |error| matches!(error, ClientError::UnknownGrid(grid) if *grid == a_ship()),
        ),
        (
            "an edit to a ship never told of",
            false,
            ServerMessage::VoxelEdits {
                grid: a_ship(),
                brick: brick(0, 0, 0),
                changes: vec![],
                resulting_digest: 0,
            },
            |error| matches!(error, ClientError::UnknownGrid(grid) if *grid == a_ship()),
        ),
        (
            "an edit to a brick of a ship that was never sent",
            true,
            ServerMessage::VoxelEdits {
                grid: a_ship(),
                brick: brick(0, 0, 0),
                changes: vec![],
                resulting_digest: 0,
            },
            |error| matches!(error, ClientError::EditToUnknownBrick(grid, _) if *grid == a_ship()),
        ),
    ];
    for (name, ship_is_known, message, recognised) in cases {
        let mut rig = Rig::new();
        rig.watch_origin(1);
        if ship_is_known {
            spawn_a_ship(&mut rig);
        }
        rig.send(message);
        rig.tick(6);
        let error = rig.client.poll_tick().expect_err(name);
        assert!(recognised(&error), "{name}: {error}");
    }
}

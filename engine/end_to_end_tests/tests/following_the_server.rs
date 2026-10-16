//! A client's picture of the world matches the server's, however play goes.

use std::thread;
use std::time::{Duration, Instant};

use client::ClientEvent;
use end_to_end_tests::{Fixture, differences_from_server, quick_differences};
use proptest::prelude::*;
use voxel_space::{BrickBox, LevelIndex};
use world_storage::test_support::{ALL_PLUGINS, Game, Plugin, brick, id};

#[derive(Clone, Debug)]
enum Action {
    Place {
        player: usize,
        at: [i64; 3],
        block: &'static str,
    },
    Watch {
        player: usize,
        center: [i64; 3],
        radius: u32,
    },
    Wait,
}

const BLOCKS: [&str; 4] = [
    "base_blocks:dirt",
    "fancy_blocks:marble",
    "fancy_blocks:glass",
    "base_blocks:grass",
];

fn action() -> impl Strategy<Value = Action> {
    prop_oneof![
        6 => (0usize..3, [-70i64..70, 0i64..14, -70i64..70], 0usize..4)
            .prop_map(|(player, at, block)| Action::Place { player, at, block: BLOCKS[block] }),
        2 => (0usize..3, [-3i64..3, -1i64..2, -3i64..3], 0u32..3)
            .prop_map(|(player, center, radius)| Action::Watch { player, center, radius }),
        1 => Just(Action::Wait),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]

    /// After every tick, for every player, every brick in the region it watches is what the server
    /// has: edits from any player and from the game, regions that grow, shrink and jump, players of
    /// different plugin sets.
    #[test]
    fn every_player_sees_what_the_server_has_in_the_region_it_watches(
        actions in proptest::collection::vec(action(), 1..45),
    ) {
        let mut fixture = Fixture::new();
        let mut players = [
            fixture.join(&ALL_PLUGINS),
            fixture.join(&ALL_PLUGINS),
            // A player whose content is numbered differently and who lacks some of it.
            fixture.join(&[Plugin::Newcomer, Plugin::BaseBlocks, Plugin::FancyBlocks]),
        ];
        for action in actions {
            match action {
                Action::Place { player, at, block } => players[player].place(at, block),
                Action::Watch { player, center, radius } => players[player].watch(
                    BrickBox::around(brick(center[0], center[1], center[2]), radius),
                ),
                Action::Wait => {}
            }
            fixture.step();
            for (index, player) in players.iter_mut().enumerate() {
                let already_seen = player.updates.len();
                player.catch_up().unwrap();
                let changed: Vec<_> = player.updates[already_seen..]
                    .iter()
                    .flat_map(|update| &update.events)
                    .filter_map(|event| match event {
                        ClientEvent::BrickChanged { level: LevelIndex::MAIN, brick } => Some(*brick),
                        _ => None,
                    })
                    .collect();
                let differences =
                    quick_differences(player, fixture.server.world(), &fixture.game, &changed);
                prop_assert!(differences.is_empty(), "player {} differs: {:?}", index, differences);
            }
        }
        // At the end, everything the players watch, generated terrain included.
        for (index, player) in players.iter().enumerate() {
            let differences =
                differences_from_server(player, fixture.server.world(), &fixture.game);
            prop_assert!(differences.is_empty(), "player {} differs at the end: {:?}", index, differences);
        }
    }
}

#[test]
fn a_player_who_joins_late_sees_the_edits_made_before_it_arrived() {
    let mut fixture = Fixture::new();
    let mut early = fixture.join(&ALL_PLUGINS);
    early.watch(BrickBox::around(brick(0, 0, 0), 2));
    for step in 0..30 {
        early.place([step * 3 - 40, 2, step % 9], "fancy_blocks:marble");
        fixture.step();
        early.catch_up().unwrap();
    }
    let mut late = fixture.join(&ALL_PLUGINS);
    late.watch(BrickBox::around(brick(0, 0, 0), 2));
    fixture.step();
    late.catch_up().unwrap();
    early.catch_up().unwrap();

    assert!(
        late.client.world().world().main().overrides.brick_count() >= 3,
        "the earlier edits arrive as brick contents"
    );
    assert_eq!(
        differences_from_server(&late, fixture.server.world(), &fixture.game),
        Vec::<String>::new()
    );
    assert_eq!(
        late.client
            .world()
            .world()
            .main()
            .overrides
            .content_digest(),
        early
            .client
            .world()
            .world()
            .main()
            .overrides
            .content_digest()
    );
}

#[test]
fn the_server_can_run_on_its_own_thread_as_an_integrated_server_does() {
    let mut fixture = Fixture::new();
    let mut player = fixture.join(&ALL_PLUGINS);
    player.watch(BrickBox::around(brick(0, 0, 0), 1));
    let first_tick = fixture.server.tick().get();

    let mut server = fixture.server;
    let runner = thread::spawn(move || {
        for _ in 0..40 {
            server.step();
            thread::sleep(Duration::from_millis(1));
        }
        server
    });
    // The client follows from this thread, only ever looking at its own end of the connection.
    let deadline = Instant::now() + Duration::from_secs(20);
    while player.tick().get() < first_tick + 40 {
        player.catch_up().unwrap();
        assert!(Instant::now() < deadline, "the stream stalled");
        thread::sleep(Duration::from_millis(1));
    }
    let server = runner.join().unwrap();
    assert_eq!(server.tick().get(), first_tick + 40);
    assert_eq!(
        differences_from_server(&player, server.world(), &fixture.game),
        Vec::<String>::new()
    );
}

const SECOND: LevelIndex = LevelIndex::new(1);

/// A server whose world has two levels: the main one and `test_content:second` (cliffs).
fn two_level_fixture() -> Fixture {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = game.new_world();
    world
        .add_level(
            id("test_content:second"),
            game.new_level("fancy_terrain:cliffs_v1"),
        )
        .unwrap();
    Fixture::with_world(game, world)
}

#[derive(Clone, Debug)]
enum LevelAction {
    Place {
        player: usize,
        level: u32,
        at: [i64; 3],
        block: &'static str,
    },
    Watch {
        player: usize,
        level: u32,
        center: [i64; 3],
        radius: u32,
    },
    Wait,
}

fn level_action() -> impl Strategy<Value = LevelAction> {
    prop_oneof![
        6 => (0usize..3, 0u32..2, [-70i64..70, 0i64..14, -70i64..70], 0usize..4)
            .prop_map(|(player, level, at, block)| LevelAction::Place { player, level, at, block: BLOCKS[block] }),
        3 => (0usize..3, 0u32..2, [-3i64..3, -1i64..2, -3i64..3], 0u32..3)
            .prop_map(|(player, level, center, radius)| LevelAction::Watch { player, level, center, radius }),
        1 => Just(LevelAction::Wait),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]

    /// Players who watch different levels, and move between them, always see what the server has in
    /// the level and region they watch; each level's edits stay in that level.
    #[test]
    fn every_player_sees_what_the_server_has_in_the_level_it_watches(
        actions in proptest::collection::vec(level_action(), 1..45),
    ) {
        let mut fixture = two_level_fixture();
        let mut players = [
            fixture.join(&ALL_PLUGINS),
            fixture.join(&ALL_PLUGINS),
            fixture.join(&[Plugin::Newcomer, Plugin::BaseBlocks, Plugin::FancyBlocks]),
        ];
        for action in actions {
            match action {
                LevelAction::Place { player, level, at, block } => {
                    players[player].place_in(LevelIndex::new(level), at, block);
                }
                LevelAction::Watch { player, level, center, radius } => {
                    players[player].watch_level(
                        LevelIndex::new(level),
                        BrickBox::around(brick(center[0], center[1], center[2]), radius),
                    );
                }
                LevelAction::Wait => {}
            }
            fixture.step();
            for (index, player) in players.iter_mut().enumerate() {
                let already_seen = player.updates.len();
                player.catch_up().unwrap();
                let watched_level = player.client.world().watched().map(|(level, _)| level);
                let changed: Vec<_> = player.updates[already_seen..]
                    .iter()
                    .flat_map(|update| &update.events)
                    .filter_map(|event| match event {
                        ClientEvent::BrickChanged { level, brick } if Some(*level) == watched_level => Some(*brick),
                        _ => None,
                    })
                    .collect();
                let differences =
                    quick_differences(player, fixture.server.world(), &fixture.game, &changed);
                prop_assert!(differences.is_empty(), "player {} differs: {:?}", index, differences);
            }
        }
        for (index, player) in players.iter().enumerate() {
            let differences =
                differences_from_server(player, fixture.server.world(), &fixture.game);
            prop_assert!(differences.is_empty(), "player {} differs at the end: {:?}", index, differences);
        }
    }
}

#[test]
fn players_in_different_levels_see_only_their_own_levels_edits_and_can_move_between_them() {
    let mut fixture = two_level_fixture();
    let mut here = fixture.join(&ALL_PLUGINS);
    let mut there = fixture.join(&ALL_PLUGINS);
    let region = BrickBox::around(brick(0, 0, 0), 2);
    here.watch(region);
    there.watch_level(SECOND, region);
    fixture.step();
    here.catch_up().unwrap();
    there.catch_up().unwrap();

    here.place([3, 2, 3], "fancy_blocks:marble");
    there.place_in(SECOND, [40, 2, 3], "fancy_blocks:glass");
    fixture.step();
    here.catch_up().unwrap();
    there.catch_up().unwrap();
    let held = |player: &end_to_end_tests::Player<_>, level: LevelIndex| {
        player
            .client
            .world()
            .world()
            .level(level)
            .unwrap()
            .overrides
            .sorted_brick_coordinates()
    };
    assert_eq!(held(&here, LevelIndex::MAIN), [brick(0, 0, 0)]);
    assert_eq!(held(&here, SECOND), []);
    assert_eq!(held(&there, SECOND), [brick(1, 0, 0)]);
    assert_eq!(held(&there, LevelIndex::MAIN), []);
    for player in [&here, &there] {
        assert_eq!(
            differences_from_server(player, fixture.server.world(), &fixture.game),
            Vec::<String>::new()
        );
    }

    // Moving to the other level shows what is there, and forgets what was here.
    here.watch_level(SECOND, region);
    fixture.step();
    here.catch_up().unwrap();
    there.catch_up().unwrap();
    assert_eq!(held(&here, SECOND), [brick(1, 0, 0)]);
    assert_eq!(held(&here, LevelIndex::MAIN), []);
    assert_eq!(
        differences_from_server(&here, fixture.server.world(), &fixture.game),
        Vec::<String>::new()
    );
    assert_eq!(here.client.world().watched(), Some((SECOND, region)));
}

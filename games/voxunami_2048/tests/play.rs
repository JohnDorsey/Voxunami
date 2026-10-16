//! Voxunami 2048, played through its server and sessions, without a window.
//!
//! The scenes are built in the sky, above every hill, so the blocks in them are the only ones near.

use std::collections::BTreeMap;

use content_registry::{IdentifierRegistry, Registry};
use game_shell::GameContent;
use game_shell::files::{open_recording, record_connection};
use game_shell::host::build_server;
use game_shell::session::{Mode, Session};
use game_shell::view::camera_at;
use proptest::prelude::*;
use server::Server;
use transport::Connection;
use transport::loopback::LoopbackConnector;
use voxel_grid::Grid;
use voxel_space::{FaceDirection, LocalVoxelIndex, VoxelCoordinate};
use voxunami_2048::content::{BlockAppearance, Content2048, block_registry_kind};
use voxunami_2048::game::{Game2048, pull_with_tool, push_with_tool};
use voxunami_2048::logic::REACH;
use voxunami_2048::material::{COASTING_RANGE, Material};
use voxunami_2048::messages::{
    Progress, PushRequest, encode_push, push_kind, read_saved_state, saved_state_section_id,
};
use voxunami_2048::numbered_block::NumberedBlock;
use world_storage::WorldData;

const SEED: u64 = 2048;
const VIEW_RADIUS: i64 = 3;
/// Above the tallest hill the landscape makes.
const SKY: i64 = 100;

type Player = Session<Box<dyn Connection>, Game2048>;
type Blocks = BTreeMap<VoxelCoordinate, NumberedBlock>;

fn at(x: i64, y: i64, z: i64) -> VoxelCoordinate {
    VoxelCoordinate::new(x, y, z)
}

fn stone(exponent: u8) -> NumberedBlock {
    NumberedBlock::new(Material::Stone, exponent)
}

fn ice(exponent: u8) -> NumberedBlock {
    NumberedBlock::new(Material::Ice, exponent)
}

fn spring_ice(exponent: u8) -> NumberedBlock {
    NumberedBlock::new(Material::SpringIce, exponent)
}

/// Every numbered block in a world's edited bricks.
fn blocks_in(grid: &Grid, content: &Content2048) -> Blocks {
    let channel = content.block_channel();
    let mut blocks = Blocks::new();
    for (coordinate, brick) in grid.bricks() {
        for local in LocalVoxelIndex::all() {
            if let Some(block) = NumberedBlock::from_id(brick.get(channel, local)) {
                blocks.insert(coordinate.voxel_at(local), block);
            }
        }
    }
    blocks
}

fn blocks_of(list: &[(VoxelCoordinate, NumberedBlock)]) -> Blocks {
    list.iter().copied().collect()
}

fn total_value(blocks: &Blocks) -> u64 {
    blocks.values().map(|block| block.value()).sum()
}

fn blocks_seen_by(player: &Player, content: &Content2048) -> Blocks {
    blocks_in(&player.client().world().world().overrides, content)
}

/// Which way a player faces, in degrees of yaw, to look along a horizontal direction.
fn yaw_toward(direction: FaceDirection) -> f32 {
    match direction {
        FaceDirection::NegativeZ => 0.0,
        FaceDirection::NegativeX => 90.0,
        FaceDirection::PositiveZ => 180.0,
        FaceDirection::PositiveX => -90.0,
        FaceDirection::NegativeY | FaceDirection::PositiveY => 0.0,
    }
}

struct Rig {
    content: Content2048,
    server: Server,
    connector: LoopbackConnector,
}

impl Rig {
    /// A world of numbered hills with these blocks added in the sky.
    fn with_blocks(blocks: &[(VoxelCoordinate, NumberedBlock)]) -> Self {
        let content = Content2048::new();
        let mut world = content.new_world(SEED);
        let channel = content.block_channel();
        for &(voxel, block) in blocks {
            world.overrides.set(channel, voxel, block.id());
        }
        Self::with_world(content, world)
    }

    fn with_world(content: Content2048, world: WorldData) -> Self {
        let (server, connector) = build_server::<Game2048>(&content, world, VIEW_RADIUS).unwrap();
        Self {
            content,
            server,
            connector,
        }
    }

    fn join(&mut self, position: [f64; 3], yaw_degrees: f32, pitch_degrees: f32) -> Player {
        let connection = self.connector.connect().unwrap();
        self.join_with(Box::new(connection), position, yaw_degrees, pitch_degrees)
    }

    fn join_with(
        &mut self,
        connection: Box<dyn Connection>,
        position: [f64; 3],
        yaw_degrees: f32,
        pitch_degrees: f32,
    ) -> Player {
        let start = camera_at(position, yaw_degrees, pitch_degrees, VIEW_RADIUS);
        let server = &mut self.server;
        Session::<_, Game2048>::connect(connection, Mode::Playing, VIEW_RADIUS, start, || {
            server.step();
        })
        .unwrap()
    }

    /// A player four voxels from the voxel's centre, looking along `direction` at it.
    fn join_looking(&mut self, voxel: VoxelCoordinate, direction: FaceDirection) -> Player {
        let [dx, dy, dz] = direction.offset();
        let centre = [
            voxel.x() as f64 + 0.5,
            voxel.y() as f64 + 0.5,
            voxel.z() as f64 + 0.5,
        ];
        let position = [
            centre[0] - 4.0 * dx as f64,
            centre[1] - 4.0 * dy as f64,
            centre[2] - 4.0 * dz as f64,
        ];
        let pitch = match direction {
            FaceDirection::PositiveY => 89.0,
            FaceDirection::NegativeY => -89.0,
            _ => 0.0,
        };
        self.join(position, yaw_toward(direction), pitch)
    }

    fn settle(&mut self, players: &mut [&mut Player]) {
        for player in players.iter_mut() {
            player.send_pose();
        }
        for _ in 0..2 {
            for player in players.iter_mut() {
                let server = &mut self.server;
                player
                    .settle(|| {
                        server.step();
                    })
                    .unwrap();
            }
        }
    }

    fn step(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.server.step();
        }
    }

    fn blocks(&self) -> Blocks {
        blocks_in(&self.server.world().overrides, &self.content)
    }
}

fn send_push(player: &mut Player, voxel: VoxelCoordinate, direction: FaceDirection) {
    player.send_game_message(push_kind(), encode_push(&PushRequest { voxel, direction }));
}

#[test]
fn the_tools_push_a_stone_block_away_and_pull_it_toward_the_player_from_every_side() {
    let voxel = at(10, SKY, 10);
    for direction in FaceDirection::ALL {
        let [dx, dy, dz] = direction.offset();
        for pulling in [false, true] {
            let mut rig = Rig::with_blocks(&[(voxel, stone(2))]);
            let mut player = rig.join_looking(voxel, direction);
            rig.settle(&mut [&mut player]);
            if pulling {
                pull_with_tool(&mut player);
            } else {
                push_with_tool(&mut player);
            }
            rig.step(3);
            rig.settle(&mut [&mut player]);

            let sign = if pulling { -1 } else { 1 };
            let moved = at(
                voxel.x() + sign * dx,
                voxel.y() + sign * dy,
                voxel.z() + sign * dz,
            );
            let expected = blocks_of(&[(moved, stone(2))]);
            assert_eq!(rig.blocks(), expected, "{direction:?}, pulling: {pulling}");
            assert_eq!(blocks_seen_by(&player, &rig.content), expected);
            assert_eq!(
                player.state.progress,
                Progress {
                    pushes: 1,
                    ..Progress::default()
                }
            );
        }
    }
}

#[test]
fn a_tool_click_on_nothing_does_nothing() {
    let mut rig = Rig::with_blocks(&[(at(10, SKY, 10), stone(2))]);
    let mut player = rig.join(
        [10.5, SKY as f64 + 0.5, 14.5],
        0.0,
        60.0, // looking up into empty sky
    );
    rig.settle(&mut [&mut player]);
    push_with_tool(&mut player);
    pull_with_tool(&mut player);
    rig.step(3);
    rig.settle(&mut [&mut player]);
    assert_eq!(rig.blocks(), blocks_of(&[(at(10, SKY, 10), stone(2))]));
    assert_eq!(player.state.progress, Progress::default());
}

#[test]
fn ice_coasts_one_step_a_tick_and_stops_beside_the_block_it_meets() {
    let wall = at(10, SKY, 3);
    let mut rig = Rig::with_blocks(&[(at(10, SKY, 10), ice(1)), (wall, stone(2))]);
    let mut player = rig.join_looking(at(10, SKY, 10), FaceDirection::NegativeZ);
    rig.settle(&mut [&mut player]);
    push_with_tool(&mut player);
    for step in 1..=6 {
        rig.step(1);
        assert_eq!(
            rig.blocks(),
            blocks_of(&[(at(10, SKY, 10 - step), ice(1)), (wall, stone(2))]),
            "after {step} ticks"
        );
    }
    // It has met the wall: it stays beside it, and the wall has not moved.
    rig.step(20);
    rig.settle(&mut [&mut player]);
    let expected = blocks_of(&[(at(10, SKY, 4), ice(1)), (wall, stone(2))]);
    assert_eq!(rig.blocks(), expected);
    assert_eq!(blocks_seen_by(&player, &rig.content), expected);
    assert_eq!(player.state.progress.score, 0);
}

#[test]
fn equal_blocks_that_meet_merge_and_everyone_hears_the_score() {
    let mut rig = Rig::with_blocks(&[(at(10, SKY, 10), ice(2)), (at(10, SKY, 6), stone(2))]);
    let mut pusher = rig.join_looking(at(10, SKY, 10), FaceDirection::NegativeZ);
    let mut watcher = rig.join([30.5, SKY as f64 + 0.5, 30.5], 0.0, 0.0);
    rig.settle(&mut [&mut pusher, &mut watcher]);
    push_with_tool(&mut pusher);
    rig.step(10);
    rig.settle(&mut [&mut pusher, &mut watcher]);

    // The block that stayed is the one that grew, in its own material.
    let expected = blocks_of(&[(at(10, SKY, 6), stone(3))]);
    assert_eq!(rig.blocks(), expected);
    assert_eq!(blocks_seen_by(&pusher, &rig.content), expected);
    assert_eq!(blocks_seen_by(&watcher, &rig.content), expected);
    let progress = Progress {
        score: 8,
        pushes: 1,
        largest_merge: 3,
    };
    assert_eq!(pusher.state.progress, progress);
    assert_eq!(watcher.state.progress, progress);

    // Someone who joins later hears how the game stands.
    let mut late = rig.join([10.5, SKY as f64 + 0.5, 20.5], 0.0, 0.0);
    rig.settle(&mut [&mut late]);
    assert_eq!(late.state.progress, progress);
}

#[test]
fn the_score_adds_up_every_merge_and_remembers_the_largest_block_made() {
    // A big merge first, a small one later.
    let mut rig = Rig::with_blocks(&[
        (at(10, SKY, 10), ice(3)),
        (at(10, SKY, 8), stone(3)),
        (at(14, SKY, 10), ice(1)),
        (at(14, SKY, 5), stone(1)),
    ]);
    let mut player = rig.join([12.5, SKY as f64 + 0.5, 14.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    send_push(&mut player, at(10, SKY, 10), FaceDirection::NegativeZ);
    send_push(&mut player, at(14, SKY, 10), FaceDirection::NegativeZ);
    rig.step(10);
    rig.settle(&mut [&mut player]);
    assert_eq!(
        rig.blocks(),
        blocks_of(&[(at(10, SKY, 8), stone(4)), (at(14, SKY, 5), stone(2))])
    );
    assert_eq!(
        player.state.progress,
        Progress {
            score: 16 + 4,
            pushes: 2,
            largest_merge: 4
        }
    );
}

#[test]
fn spring_ice_bounces_back_across_brick_boundaries_until_its_range_is_used_up() {
    let wall = at(10, SKY, 6);
    let mut rig = Rig::with_blocks(&[(at(10, SKY, 10), spring_ice(1)), (wall, stone(3))]);
    let mut player = rig.join_looking(at(10, SKY, 10), FaceDirection::NegativeZ);
    rig.settle(&mut [&mut player]);
    push_with_tool(&mut player);
    // Three steps to z = 7, a bounce, then the rest of its range back the way it came.
    rig.step(3);
    assert_eq!(rig.blocks().get(&at(10, SKY, 7)), Some(&spring_ice(1)));
    rig.step(1);
    assert_eq!(rig.blocks().get(&at(10, SKY, 7)), Some(&spring_ice(1)));
    rig.step(COASTING_RANGE - 4 + 10);
    rig.settle(&mut [&mut player]);
    let end = 7 + i64::from(COASTING_RANGE) - 4;
    assert!(end > 32, "the journey crosses into the next brick");
    let expected = blocks_of(&[(at(10, SKY, end), spring_ice(1)), (wall, stone(3))]);
    assert_eq!(rig.blocks(), expected);
    assert_eq!(blocks_seen_by(&player, &rig.content), expected);
}

#[test]
fn pushes_that_are_out_of_reach_or_not_of_a_block_or_malformed_are_ignored() {
    let near = at(10, SKY, 10);
    let far = at(10, SKY, 10 - REACH as i64 - 8);
    let mut rig = Rig::with_blocks(&[(near, stone(2)), (far, stone(2))]);
    let mut player = rig.join([10.5, SKY as f64 + 0.5, 14.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    send_push(&mut player, far, FaceDirection::PositiveX); // too far to reach
    send_push(&mut player, at(10, SKY, 12), FaceDirection::PositiveX); // empty air
    player.send_game_message(push_kind(), vec![]);
    player.send_game_message(push_kind(), vec![1, 2, 3, 4, 5, 6, 7]);
    player.send_game_message("other_game:hello".parse().unwrap(), vec![1, 2, 3]);
    rig.step(3);
    rig.settle(&mut [&mut player]);
    assert_eq!(
        rig.blocks(),
        blocks_of(&[(near, stone(2)), (far, stone(2))])
    );
    assert_eq!(player.state.progress, Progress::default());

    // The same push, from within reach, works.
    send_push(&mut player, near, FaceDirection::PositiveX);
    rig.step(3);
    assert_eq!(
        rig.blocks(),
        blocks_of(&[(at(11, SKY, 10), stone(2)), (far, stone(2))])
    );
}

/// A world, as another program with one more block than this game has might have saved it: the
/// voxel `unknown` holds that block, and an ice block of 2 is at `ice_at`.
fn world_with_a_block_from_elsewhere(
    unknown: VoxelCoordinate,
    ice_at: VoxelCoordinate,
) -> (Content2048, WorldData) {
    let content = Content2048::new();
    let channel = content.block_channel();
    let mut world = content.new_world(SEED);
    let mut blocks = Registry::new();
    let appearance = BlockAppearance {
        color: [0.5; 3],
        opaque: true,
    };
    let names = std::iter::once("voxunami_2048:air".to_owned())
        .chain(NumberedBlock::all().map(|block| format!("voxunami_2048:{}", block.name())))
        .chain(["elsewhere:marble".to_owned()]);
    let mut marble = 0;
    for name in names {
        marble = blocks
            .register(name.parse().unwrap(), appearance)
            .unwrap()
            .raw();
    }
    world.overrides.set(channel, unknown, marble);
    world.overrides.set(channel, ice_at, ice(1).id());
    let bytes = world
        .save(&[(block_registry_kind(), &blocks as &dyn IdentifierRegistry)])
        .unwrap();

    let mut content = Content2048::new();
    let parsed = world_storage::ParsedWorld::parse(&bytes).unwrap();
    let seed = parsed.world_seed();
    let (loaded, report) = parsed.load(content.load_context(seed)).unwrap();
    assert_eq!(report.missing_content.len(), 1);
    (content, loaded)
}

#[test]
fn blocks_stop_at_content_the_game_does_not_know_and_never_overwrite_it() {
    let unknown = at(10, SKY, 5);
    let (content, world) = world_with_a_block_from_elsewhere(unknown, at(10, SKY, 10));
    let channel = content.block_channel();
    let marble = world.overrides.get(channel, unknown);
    let mut rig = Rig::with_world(content, world);
    let mut player = rig.join_looking(at(10, SKY, 10), FaceDirection::NegativeZ);
    rig.settle(&mut [&mut player]);
    push_with_tool(&mut player);
    rig.step(20);
    assert_eq!(rig.blocks(), blocks_of(&[(at(10, SKY, 6), ice(1))]));
    assert_eq!(rig.server.world().overrides.get(channel, unknown), marble);

    // Pushing the unknown block itself does nothing either: this game cannot move what it
    // does not know.
    send_push(&mut player, unknown, FaceDirection::PositiveX);
    rig.step(3);
    rig.settle(&mut [&mut player]);
    assert_eq!(rig.server.world().overrides.get(channel, unknown), marble);
    assert_eq!(player.state.progress.pushes, 1);
}

#[test]
fn the_ground_is_made_of_blocks_that_can_be_pushed_like_any_other() {
    let content = Content2048::new();
    let mut rig = Rig::with_world(Content2048::new(), content.new_world(SEED));
    let mut player = rig.join([8.5, 60.5, 8.5], 0.0, -89.0);
    rig.settle(&mut [&mut player]);

    // Push the top block of a column straight up into the empty air above it.
    let top = (-60..60)
        .rev()
        .map(|y| at(8, y, 8))
        .find(|&voxel| player.mesher().is_opaque_at(voxel) == Some(true))
        .expect("the column has ground");
    let block = player
        .mesher()
        .material_at(top)
        .and_then(NumberedBlock::from_id)
        .expect("the ground is numbered blocks");
    player.camera = camera_at([8.5, top.y() as f64 + 5.5, 8.5], 0.0, -89.0, VIEW_RADIUS);
    rig.settle(&mut [&mut player]);
    send_push(&mut player, top, FaceDirection::PositiveY);
    rig.step(COASTING_RANGE + 2);
    rig.settle(&mut [&mut player]);

    // Stone moved one step, and ice of either kind coasted its whole range through the air.
    let landed = at(8, top.y() + i64::from(block.material.motion().range), 8);
    for (what, material_at) in [
        (
            "the server",
            rig.server
                .world()
                .overrides
                .get(rig.content.block_channel(), landed),
        ),
        ("the player", player.mesher().material_at(landed).unwrap()),
    ] {
        assert_eq!(NumberedBlock::from_id(material_at), Some(block), "{what}");
    }
    assert_eq!(player.mesher().is_opaque_at(top), Some(false));
}

fn direction_strategy() -> impl Strategy<Value = FaceDirection> {
    (0u8..6).prop_map(|index| FaceDirection::from_index(index).unwrap())
}

fn block_strategy() -> impl Strategy<Value = NumberedBlock> {
    (0..Material::ALL.len(), 1u8..=3)
        .prop_map(|(material, exponent)| NumberedBlock::new(Material::ALL[material], exponent))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10))]

    /// Whatever is in a scene and however it is pushed, what players see is what the server has,
    /// and merging never loses or creates value.
    #[test]
    fn the_players_view_always_matches_the_server_and_value_is_conserved(
        scene in proptest::collection::vec((0i64..5, 0i64..5, 0i64..5, block_strategy()), 0..50),
        pushes in proptest::collection::vec((0i64..5, 0i64..5, 0i64..5, direction_strategy()), 0..8),
    ) {
        let list: Vec<_> = scene
            .into_iter()
            .map(|(x, y, z, block)| (at(10 + x, SKY + y, 10 + z), block))
            .collect();
        let mut rig = Rig::with_blocks(&list);
        let mut player = rig.join([12.5, SKY as f64 + 2.5, 12.5], 0.0, 0.0);
        let mut watcher = rig.join([20.5, SKY as f64 + 2.5, 20.5], 0.0, 0.0);
        rig.settle(&mut [&mut player, &mut watcher]);
        let starting_value = total_value(&rig.blocks());
        for (x, y, z, direction) in pushes {
            send_push(&mut player, at(10 + x, SKY + y, 10 + z), direction);
            rig.step(2);
            prop_assert_eq!(total_value(&rig.blocks()), starting_value);
        }
        rig.step(COASTING_RANGE + 2);
        rig.settle(&mut [&mut player, &mut watcher]);
        let blocks = rig.blocks();
        prop_assert_eq!(total_value(&blocks), starting_value);
        prop_assert_eq!(blocks_seen_by(&player, &rig.content), blocks.clone());
        prop_assert_eq!(blocks_seen_by(&watcher, &rig.content), blocks);
        prop_assert_eq!(player.state.progress, watcher.state.progress);
    }
}

#[test]
fn a_world_saved_while_blocks_are_moving_carries_on_exactly_as_if_it_never_stopped() {
    let scene = [
        (at(10, SKY, 10), ice(1)),
        (at(12, SKY, 10), spring_ice(2)),
        (at(12, SKY, 3), stone(3)),
    ];
    let push_both = |rig: &mut Rig| {
        let mut player = rig.join([11.5, SKY as f64 + 0.5, 12.5], 0.0, 0.0);
        rig.settle(&mut [&mut player]);
        send_push(&mut player, at(10, SKY, 10), FaceDirection::NegativeZ);
        send_push(&mut player, at(12, SKY, 10), FaceDirection::NegativeZ);
        rig.step(1);
        player.say_goodbye();
        rig.step(1);
    };

    // A world that never stops.
    let mut control = Rig::with_blocks(&scene);
    push_both(&mut control);
    control.step(80);

    // The same world, saved after five ticks and loaded as if by a new program run.
    let mut interrupted = Rig::with_blocks(&scene);
    push_both(&mut interrupted);
    interrupted.step(3);
    let moving_at_save = interrupted.blocks();
    let Rig {
        content, server, ..
    } = interrupted;
    let world = server.into_world();
    let saved = read_saved_state(world.opaque_sections()).expect("the game saved its state");
    assert_eq!(saved.movers.len(), 2, "both blocks were in motion");
    assert_eq!(saved.progress.pushes, 2);
    let bytes = world.save(&content.registries()).unwrap();

    let mut content = Content2048::new();
    let parsed = world_storage::ParsedWorld::parse(&bytes).unwrap();
    let seed = parsed.world_seed();
    let (loaded, report) = parsed.load(content.load_context(seed)).unwrap();
    assert_eq!(report.opaque_sections, [saved_state_section_id()]);
    let mut resumed = Rig::with_world(content, loaded);
    assert_eq!(resumed.blocks(), moving_at_save);
    resumed.step(75);
    assert_ne!(resumed.blocks(), moving_at_save, "they went on moving");
    assert_eq!(resumed.blocks(), control.blocks());
    assert_eq!(
        resumed.server.world().overrides.content_digest(),
        control.server.world().overrides.content_digest()
    );
}

#[test]
fn a_recording_of_a_game_replays_the_blocks_and_the_score() {
    let directory =
        std::env::temp_dir().join(format!("voxunami_2048_replay_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("game.voxreplay");

    let mut rig = Rig::with_blocks(&[(at(10, SKY, 10), ice(2)), (at(10, SKY, 6), stone(2))]);
    let connection = rig.connector.connect().unwrap();
    let recorded = record_connection(connection, &path).unwrap();
    let mut player = rig.join_with(recorded, [10.5, SKY as f64 + 0.5, 14.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    push_with_tool(&mut player);
    rig.step(10);
    rig.settle(&mut [&mut player]);
    let live_summary = player.client().world().summary();
    drop(player);

    let connection: Box<dyn Connection> = Box::new(open_recording(&path).unwrap());
    let mut replay = Session::<_, Game2048>::connect(
        connection,
        Mode::Replaying,
        VIEW_RADIUS,
        camera_at([0.0; 3], 0.0, 0.0, VIEW_RADIUS),
        || {},
    )
    .unwrap();
    while replay.advance_one_tick().is_ok() {}
    assert_eq!(replay.client().world().summary(), live_summary);
    assert_eq!(
        blocks_seen_by(&replay, &rig.content),
        blocks_of(&[(at(10, SKY, 6), stone(3))])
    );
    assert_eq!(replay.state.progress.score, 8);
    std::fs::remove_dir_all(&directory).unwrap();
}

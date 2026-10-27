//! Voxunami 2048, played through its server and sessions, without a window.
//!
//! The scenes are built in the sky, above every hill, so the blocks in them are the only ones near.

use std::collections::BTreeMap;

use block_models::BlockDefinition;
use content_registry::{IdentifierRegistry, Registry};
use game_shell::Identity;
use game_shell::files::{open_recording, record_connection};
use game_shell::host::build_server;
use game_shell::session::{Mode, Session};
use game_shell::view::camera_at;
use game_shell::{GameContent, KeyCode};
use proptest::prelude::*;
use protocol::Message;
use server::Server;
use transport::Connection;
use transport::loopback::LoopbackConnector;
use voxel_grid::Grid;
use voxel_space::{FaceDirection, LocalVoxelIndex, VoxelCoordinate};
use voxunami_2048::content::{Content2048, block_registry_kind, landscape_id};
use voxunami_2048::game::{Game2048, progress as progress_of, pull_with_tool, push_with_tool};
use voxunami_2048::logic::REACH;
use voxunami_2048::material::{COASTING_RANGE, Material};
use voxunami_2048::messages::{Progress, PushRequest, read_saved_state, saved_state_section_id};
use voxunami_2048::numbered_block::NumberedBlock;
use voxunami_2048::tool::Tool;
use world_storage::WorldData;

const SEED: u64 = 2048;
const VIEW_RADIUS: i128 = 3;
/// Above the tallest hill the landscape makes.
const SKY: i128 = 100;

type Player = Session<Box<dyn Connection>, Game2048>;
type Blocks = BTreeMap<VoxelCoordinate, NumberedBlock>;

fn at(x: i128, y: i128, z: i128) -> VoxelCoordinate {
    VoxelCoordinate::new(x, y, z)
}

/// The blocks of these tests are named by the exponent of the power of two they show.
fn stone(exponent: u8) -> NumberedBlock {
    NumberedBlock::new(Material::Stone, 1 << exponent)
}

fn ice(exponent: u8) -> NumberedBlock {
    NumberedBlock::new(Material::Ice, 1 << exponent)
}

fn spring_ice(exponent: u8) -> NumberedBlock {
    NumberedBlock::new(Material::SpringIce, 1 << exponent)
}

/// Every numbered block in a world's edited bricks.
fn blocks_in(grid: &Grid, content: &Content2048) -> Blocks {
    let (block_channel, number_channel) = (content.block_channel(), content.number_channel());
    let mut blocks = Blocks::new();
    for (coordinate, brick) in grid.bricks() {
        for local in LocalVoxelIndex::all() {
            if let Some(block) = NumberedBlock::from_voxel(
                brick.get(block_channel, local),
                brick.get(number_channel, local),
            ) {
                blocks.insert(coordinate.voxel_at(local), block);
            }
        }
    }
    blocks
}

/// Puts a block in the voxels of a grid, as the game would.
fn put_block(grid: &mut Grid, content: &Content2048, voxel: VoxelCoordinate, block: NumberedBlock) {
    grid.set(content.block_channel(), voxel, block.block_id());
    grid.set(content.number_channel(), voxel, block.state());
}

/// Air has no number: wherever blocks have been, the voxels they left hold the default again, so
/// that edited bricks hold nothing the blocks do not show.
fn assert_air_has_no_number(grid: &Grid, content: &Content2048) {
    let (block_channel, number_channel) = (content.block_channel(), content.number_channel());
    for (_, brick) in grid.bricks() {
        for local in LocalVoxelIndex::all() {
            if brick.get(block_channel, local) == 0 {
                assert_eq!(brick.get(number_channel, local), 0);
            }
        }
    }
}

/// The block a world holds at a voxel of its main level, if it is a numbered block.
fn block_in_world(
    world: &WorldData,
    content: &Content2048,
    voxel: VoxelCoordinate,
) -> Option<NumberedBlock> {
    let grid = &world.main().overrides;
    NumberedBlock::from_voxel(
        grid.get(content.block_channel(), voxel),
        grid.get(content.number_channel(), voxel),
    )
}

fn blocks_of(list: &[(VoxelCoordinate, NumberedBlock)]) -> Blocks {
    list.iter().copied().collect()
}

fn total_value(blocks: &Blocks) -> u64 {
    blocks.values().map(|block| u64::from(block.number())).sum()
}

fn blocks_seen_by(player: &Player, content: &Content2048) -> Blocks {
    blocks_in(&player.client().world().world().main().overrides, content)
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
    /// How many players have joined: each is an identity of its own.
    joined: u8,
}

impl Rig {
    /// A world of numbered hills with these blocks added in the sky.
    fn with_blocks(blocks: &[(VoxelCoordinate, NumberedBlock)]) -> Self {
        let content = Content2048::new();
        let mut world = content.new_world(SEED);
        for &(voxel, block) in blocks {
            put_block(&mut world.main_mut().overrides, &content, voxel, block);
        }
        Self::with_world(content, world)
    }

    fn with_world(content: Content2048, world: WorldData) -> Self {
        let (server, connector) = build_server::<Game2048>(&content, world, VIEW_RADIUS).unwrap();
        Self {
            content,
            server,
            connector,
            joined: 0,
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
        let start = camera_at(position, yaw_degrees, pitch_degrees);
        self.joined += 1;
        let identity = Identity::from_secret([self.joined; 32]);
        let server = &mut self.server;
        Session::<_, Game2048>::connect(
            connection,
            &identity,
            Mode::Playing,
            VIEW_RADIUS,
            start,
            || {
                server.step();
            },
        )
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
        blocks_in(&self.server.world().main().overrides, &self.content)
    }
}

fn send_push(player: &mut Player, voxel: VoxelCoordinate, direction: FaceDirection) {
    send_push_with(player, voxel, direction, Tool::Double);
}

fn send_push_with(
    player: &mut Player,
    voxel: VoxelCoordinate,
    direction: FaceDirection,
    tool: Tool,
) {
    player.send_game_message(
        PushRequest::kind(),
        PushRequest {
            voxel,
            direction,
            tool,
        }
        .encode(),
    );
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
            assert_air_has_no_number(&rig.server.world().main().overrides, &rig.content);
            assert_eq!(blocks_seen_by(&player, &rig.content), expected);
            assert_eq!(
                progress_of(&player),
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
    assert_eq!(progress_of(&player), Progress::default());
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
    assert_eq!(progress_of(&player).score, 0);
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
        largest_merge: 8,
    };
    assert_eq!(progress_of(&pusher), progress);
    assert_eq!(progress_of(&watcher), progress);

    // Someone who joins later hears how the game stands.
    let mut late = rig.join([10.5, SKY as f64 + 0.5, 20.5], 0.0, 0.0);
    rig.settle(&mut [&mut late]);
    assert_eq!(progress_of(&late), progress);
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
        progress_of(&player),
        Progress {
            score: 16 + 4,
            pushes: 2,
            largest_merge: 16
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
    let end = 7 + i128::from(COASTING_RANGE) - 4;
    assert!(end > 32, "the journey crosses into the next brick");
    let expected = blocks_of(&[(at(10, SKY, end), spring_ice(1)), (wall, stone(3))]);
    assert_eq!(rig.blocks(), expected);
    assert_eq!(blocks_seen_by(&player, &rig.content), expected);
}

#[test]
fn pushes_that_are_out_of_reach_or_not_of_a_block_or_malformed_are_ignored() {
    let near = at(10, SKY, 10);
    let far = at(10, SKY, 10 - REACH as i128 - 8);
    let mut rig = Rig::with_blocks(&[(near, stone(2)), (far, stone(2))]);
    let mut player = rig.join([10.5, SKY as f64 + 0.5, 14.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    send_push(&mut player, far, FaceDirection::PositiveX); // too far to reach
    send_push(&mut player, at(10, SKY, 12), FaceDirection::PositiveX); // empty air
    player.send_game_message(PushRequest::kind(), vec![]);
    player.send_game_message(PushRequest::kind(), vec![1, 2, 3, 4, 5, 6, 7]);
    player.send_game_message("other_game:hello".parse().unwrap(), vec![1, 2, 3]);
    rig.step(3);
    rig.settle(&mut [&mut player]);
    assert_eq!(
        rig.blocks(),
        blocks_of(&[(near, stone(2)), (far, stone(2))])
    );
    assert_eq!(progress_of(&player), Progress::default());

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
    let names = std::iter::once("voxunami_2048:air".to_owned())
        .chain(Material::ALL.map(|material| format!("voxunami_2048:{}", material.name())))
        .chain(["elsewhere:marble".to_owned()]);
    let mut marble = 0;
    for name in names {
        marble = blocks
            .register(name.parse().unwrap(), BlockDefinition::cube([0.5; 3]))
            .unwrap()
            .raw();
    }
    world.main_mut().overrides.set(channel, unknown, marble);
    // The marble has a number of its own, which the game must not read as a numbered block.
    world
        .main_mut()
        .overrides
        .set(content.number_channel(), unknown, 2);
    put_block(&mut world.main_mut().overrides, &content, ice_at, ice(1));
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
    let marble = world.main().overrides.get(channel, unknown);
    let mut rig = Rig::with_world(content, world);
    let mut player = rig.join_looking(at(10, SKY, 10), FaceDirection::NegativeZ);
    rig.settle(&mut [&mut player]);
    push_with_tool(&mut player);
    rig.step(20);
    assert_eq!(rig.blocks(), blocks_of(&[(at(10, SKY, 6), ice(1))]));
    assert_eq!(
        rig.server.world().main().overrides.get(channel, unknown),
        marble
    );

    // Pushing the unknown block itself does nothing either: this game cannot move what it
    // does not know.
    send_push(&mut player, unknown, FaceDirection::PositiveX);
    rig.step(3);
    rig.settle(&mut [&mut player]);
    assert_eq!(
        rig.server.world().main().overrides.get(channel, unknown),
        marble
    );
    assert_eq!(progress_of(&player).pushes, 1);
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
    let block = NumberedBlock::from_voxel(
        player.mesher().material_at(top).unwrap(),
        player.mesher().state_at(top).unwrap(),
    )
    .expect("the ground is numbered blocks");
    player.camera = camera_at([8.5, top.y() as f64 + 5.5, 8.5], 0.0, -89.0);
    rig.settle(&mut [&mut player]);
    send_push(&mut player, top, FaceDirection::PositiveY);
    rig.step(COASTING_RANGE + 2);
    rig.settle(&mut [&mut player]);

    // Stone moved one step, and ice of either kind coasted its whole range through the air.
    let landed = at(8, top.y() + i128::from(block.material.motion().range), 8);
    for (what, found) in [
        (
            "the server",
            block_in_world(rig.server.world(), &rig.content, landed),
        ),
        (
            "the player",
            NumberedBlock::from_voxel(
                player.mesher().material_at(landed).unwrap(),
                player.mesher().state_at(landed).unwrap(),
            ),
        ),
    ] {
        assert_eq!(found, Some(block), "{what}");
    }
    assert_eq!(player.mesher().is_opaque_at(top), Some(false));
}

fn direction_strategy() -> impl Strategy<Value = FaceDirection> {
    (0u8..6).prop_map(|index| FaceDirection::from_index(index).unwrap())
}

fn block_strategy() -> impl Strategy<Value = NumberedBlock> {
    (0..Material::ALL.len(), 1u32..=3)
        .prop_map(|(material, exponent)| NumberedBlock::new(Material::ALL[material], 1 << exponent))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10))]

    /// Whatever is in a scene and however it is pushed, what players see is what the server has,
    /// and merging never loses or creates value.
    #[test]
    fn the_players_view_always_matches_the_server_and_value_is_conserved(
        scene in proptest::collection::vec((0i128..5, 0i128..5, 0i128..5, block_strategy()), 0..50),
        pushes in proptest::collection::vec((0i128..5, 0i128..5, 0i128..5, direction_strategy()), 0..8),
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
        assert_air_has_no_number(&rig.server.world().main().overrides, &rig.content);
        let blocks = rig.blocks();
        prop_assert_eq!(total_value(&blocks), starting_value);
        prop_assert_eq!(blocks_seen_by(&player, &rig.content), blocks.clone());
        prop_assert_eq!(blocks_seen_by(&watcher, &rig.content), blocks);
        prop_assert_eq!(progress_of(&player), progress_of(&watcher));
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
        resumed.server.world().main().overrides.content_digest(),
        control.server.world().main().overrides.content_digest()
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
        &Identity::generate(),
        Mode::Replaying,
        VIEW_RADIUS,
        camera_at([0.0; 3], 0.0, 0.0),
        || {},
    )
    .unwrap();
    while replay.advance_one_tick().is_ok() {}
    assert_eq!(replay.client().world().summary(), live_summary);
    assert_eq!(
        blocks_seen_by(&replay, &rig.content),
        blocks_of(&[(at(10, SKY, 6), stone(3))])
    );
    assert_eq!(progress_of(&replay).score, 8);
    std::fs::remove_dir_all(&directory).unwrap();
}

/// The blocks in one level of a world.
fn blocks_in_level(
    world: &WorldData,
    level: voxel_space::LevelIndex,
    content: &Content2048,
) -> Blocks {
    blocks_in(
        &world.level(level).expect("the level exists").overrides,
        content,
    )
}

/// A world with a second level, `voxunami_2048:underside`, holding these blocks in its sky.
fn world_with_a_second_level(
    blocks: &[(VoxelCoordinate, NumberedBlock)],
) -> (Content2048, WorldData, voxel_space::LevelIndex) {
    let content = Content2048::new();
    let mut world = content.new_world(SEED);
    let mut level = content.new_world(SEED).into_main();
    for &(voxel, block) in blocks {
        put_block(&mut level.overrides, &content, voxel, block);
    }
    let second = world
        .add_level("voxunami_2048:underside".parse().unwrap(), level)
        .unwrap();
    (content, world, second)
}

#[test]
fn blocks_pushed_in_another_level_move_and_merge_there_and_the_score_is_shared() {
    let (content, mut world, second) =
        world_with_a_second_level(&[(at(10, SKY, 10), ice(2)), (at(10, SKY, 6), stone(2))]);
    // The main level has a block in the way at the same place, which must not matter.
    put_block(
        &mut world.main_mut().overrides,
        &content,
        at(10, SKY, 8),
        stone(5),
    );
    let mut rig = Rig::with_world(content, world);
    let mut player = rig.join_looking(at(10, SKY, 10), FaceDirection::NegativeZ);
    rig.settle(&mut [&mut player]);
    player.send_command("/level go underside");
    rig.settle(&mut [&mut player]);
    assert_eq!(player.level(), second);
    assert_eq!(
        player.take_command_replies(),
        ["you are now in voxunami_2048:underside"]
    );

    push_with_tool(&mut player);
    rig.step(10);
    rig.settle(&mut [&mut player]);

    let merged = blocks_of(&[(at(10, SKY, 6), stone(3))]);
    assert_eq!(
        blocks_in_level(rig.server.world(), second, &rig.content),
        merged
    );
    assert_eq!(
        blocks_in_level(
            rig.server.world(),
            voxel_space::LevelIndex::MAIN,
            &rig.content
        ),
        blocks_of(&[(at(10, SKY, 8), stone(5))]),
        "nothing happened in the main level"
    );
    assert_eq!(
        blocks_in(
            &player
                .client()
                .world()
                .world()
                .level(second)
                .unwrap()
                .overrides,
            &rig.content
        ),
        merged
    );
    assert_eq!(
        progress_of(&player),
        Progress {
            score: 8,
            pushes: 1,
            largest_merge: 8
        }
    );
}

#[test]
fn a_block_moving_in_another_level_when_the_world_is_saved_is_still_moving_there_when_it_is_loaded()
{
    let scene = [(at(10, SKY, 10), ice(1))];
    let play = |save_after: Option<u32>| {
        let (content, world, second) = world_with_a_second_level(&scene);
        let mut rig = Rig::with_world(content, world);
        let mut player = rig.join_looking(at(10, SKY, 10), FaceDirection::NegativeZ);
        rig.settle(&mut [&mut player]);
        player.send_command("/level go underside");
        rig.settle(&mut [&mut player]);
        push_with_tool(&mut player);
        rig.step(1);
        player.say_goodbye();
        rig.step(1);
        let Some(ticks_before_saving) = save_after else {
            rig.step(80);
            return (
                blocks_in_level(rig.server.world(), second, &rig.content),
                None,
            );
        };
        rig.step(ticks_before_saving);
        let Rig {
            content, server, ..
        } = rig;
        let world = server.into_world();
        let saved = read_saved_state(world.opaque_sections()).unwrap();
        assert_eq!(saved.movers.len(), 1);
        assert_eq!(saved.movers[0].level.to_string(), "voxunami_2048:underside");
        let bytes = world.save(&content.registries()).unwrap();

        let mut content = Content2048::new();
        let parsed = world_storage::ParsedWorld::parse(&bytes).unwrap();
        let seed = parsed.world_seed();
        let (loaded, _) = parsed.load(content.load_context(seed)).unwrap();
        let mut resumed = Rig::with_world(content, loaded);
        resumed.step(80);
        (
            blocks_in_level(resumed.server.world(), second, &resumed.content),
            Some(blocks_in_level(
                resumed.server.world(),
                voxel_space::LevelIndex::MAIN,
                &resumed.content,
            )),
        )
    };
    let (uninterrupted, _) = play(None);
    let (resumed, main_level) = play(Some(5));
    assert_eq!(resumed, uninterrupted);
    assert_eq!(resumed.len(), 1);
    assert_eq!(
        main_level,
        Some(Blocks::new()),
        "nothing leaked into the main level"
    );
    // The block went all of its range, so it really was moving after the world was loaded.
    assert!(resumed.keys().any(|voxel| voxel.z() < 10 - 5));
}

#[test]
fn a_saved_state_of_an_older_version_is_not_read_and_the_blocks_it_named_are_not_moving() {
    use voxunami_2048::messages::saved_state_section_id;
    // Version 4 of the section, which had a position as three varints of 64 bits: progress, then movers as level, position, direction, tool and steps.
    let mut payload = binary_codec::ByteWriter::new();
    payload.write_varint(40); // score
    payload.write_varint(3); // pushes
    payload.write_varint(4); // largest merge
    payload.write_varint(1); // one mover
    payload.write_identifier(&landscape_id());
    for component in [10i128, SKY, 10] {
        payload.write_varint(((component << 1) ^ (component >> 127)) as u64);
    }
    payload.write_u8(FaceDirection::NegativeZ.index());
    payload.write_u8(0);
    payload.write_varint(30);
    let content = Content2048::new();
    let mut world = content.new_world(SEED);
    put_block(
        &mut world.main_mut().overrides,
        &content,
        at(10, SKY, 10),
        ice(1),
    );
    world.set_opaque_section(world_storage::Section {
        identifier: saved_state_section_id(),
        version: 4,
        payload: payload.into_bytes(),
    });
    let mut rig = Rig::with_world(content, world);
    rig.step(40);
    // Not read: the block stays where it was put, as it would for a world with no state.
    assert_eq!(rig.blocks(), blocks_of(&[(at(10, SKY, 10), ice(1))]));
}

fn numbered(material: Material, number: u32) -> NumberedBlock {
    NumberedBlock::new(material, number)
}

/// A coasting ice block of 3 meets a stone block of 5, after the player has pressed `key` to choose
/// a tool and pushed. Returns the blocks left and how the game stands.
fn ice_meets_stone_with_the_tool_of(key: KeyCode) -> (Blocks, Progress) {
    let wall = at(10, SKY, 3);
    let mut rig = Rig::with_blocks(&[
        (at(10, SKY, 10), numbered(Material::Ice, 3)),
        (wall, numbered(Material::Stone, 5)),
    ]);
    let mut player = rig.join_looking(at(10, SKY, 10), FaceDirection::NegativeZ);
    rig.settle(&mut [&mut player]);
    player.key_pressed(key);
    push_with_tool(&mut player);
    rig.step(12);
    rig.settle(&mut [&mut player]);
    assert_air_has_no_number(&rig.server.world().main().overrides, &rig.content);
    assert_eq!(blocks_seen_by(&player, &rig.content), rig.blocks());
    (rig.blocks(), progress_of(&player))
}

#[test]
fn the_tool_the_player_chose_decides_how_the_blocks_they_push_combine() {
    let wall = at(10, SKY, 3);
    // The double tool, which is the one a player starts with, does not combine a 3 and a 5.
    let (blocks, progress) = ice_meets_stone_with_the_tool_of(KeyCode::Digit1);
    assert_eq!(
        blocks,
        blocks_of(&[
            (at(10, SKY, 4), numbered(Material::Ice, 3)),
            (wall, numbered(Material::Stone, 5))
        ])
    );
    assert_eq!(
        progress,
        Progress {
            pushes: 1,
            ..Progress::default()
        }
    );

    // The add tool makes 8 of them, in the stone that stayed.
    let (blocks, progress) = ice_meets_stone_with_the_tool_of(KeyCode::Digit2);
    assert_eq!(blocks, blocks_of(&[(wall, numbered(Material::Stone, 8))]));
    assert_eq!(
        progress,
        Progress {
            score: 8,
            pushes: 1,
            largest_merge: 8
        }
    );

    // The subtract tool leaves the difference.
    let (blocks, progress) = ice_meets_stone_with_the_tool_of(KeyCode::Digit3);
    assert_eq!(blocks, blocks_of(&[(wall, numbered(Material::Stone, 2))]));
    assert_eq!(
        progress,
        Progress {
            score: 2,
            pushes: 1,
            largest_merge: 2
        }
    );
}

#[test]
fn equal_numbers_cancel_with_the_subtract_tool_and_score_nothing() {
    let mut rig = Rig::with_blocks(&[
        (at(10, SKY, 10), numbered(Material::Ice, 7)),
        (at(10, SKY, 6), numbered(Material::Stone, 7)),
        (at(30, SKY, 30), stone(1)),
    ]);
    let mut player = rig.join_looking(at(10, SKY, 10), FaceDirection::NegativeZ);
    rig.settle(&mut [&mut player]);
    player.key_pressed(KeyCode::Digit3);
    push_with_tool(&mut player);
    rig.step(10);
    rig.settle(&mut [&mut player]);
    let expected = blocks_of(&[(at(30, SKY, 30), stone(1))]);
    assert_eq!(rig.blocks(), expected);
    assert_eq!(blocks_seen_by(&player, &rig.content), expected);
    assert_air_has_no_number(&rig.server.world().main().overrides, &rig.content);
    assert_eq!(
        progress_of(&player),
        Progress {
            pushes: 1,
            ..Progress::default()
        }
    );
}

#[test]
fn the_chosen_tool_is_shown_and_keys_that_are_not_tools_leave_it_alone() {
    let mut rig = Rig::with_blocks(&[(at(10, SKY, 10), stone(1))]);
    let mut player = rig.join_looking(at(10, SKY, 10), FaceDirection::NegativeZ);
    rig.settle(&mut [&mut player]);
    assert!(player.hud_text().starts_with("tool double | "));
    player.key_pressed(KeyCode::Digit2);
    assert!(player.hud_text().starts_with("tool add | "));
    player.key_pressed(KeyCode::KeyR);
    player.key_pressed(KeyCode::Digit9);
    assert!(player.hud_text().starts_with("tool add | "));
    player.key_pressed(KeyCode::Digit3);
    assert!(player.hud_text().starts_with("tool subtract | "));
    player.key_pressed(KeyCode::Digit1);
    assert!(player.hud_text().starts_with("tool double | "));
    assert!(player.hud_text().contains("looking at stone 2"));
}

#[test]
fn a_block_moving_with_a_tool_when_the_world_is_saved_still_has_that_tool_when_it_is_loaded() {
    let scene = [
        (at(10, SKY, 10), numbered(Material::Ice, 9)),
        (at(10, SKY, 3), numbered(Material::Stone, 4)),
    ];
    let start = |rig: &mut Rig| {
        let mut player = rig.join_looking(at(10, SKY, 10), FaceDirection::NegativeZ);
        rig.settle(&mut [&mut player]);
        send_push_with(
            &mut player,
            at(10, SKY, 10),
            FaceDirection::NegativeZ,
            Tool::Subtract,
        );
        rig.step(1);
        player.say_goodbye();
        rig.step(1);
    };
    let mut control = Rig::with_blocks(&scene);
    start(&mut control);
    control.step(20);
    // 9 coasting into 4 leaves 5 in the stone that stayed.
    assert_eq!(
        control.blocks(),
        blocks_of(&[(at(10, SKY, 3), numbered(Material::Stone, 5))])
    );

    let mut interrupted = Rig::with_blocks(&scene);
    start(&mut interrupted);
    interrupted.step(2);
    let Rig {
        content, server, ..
    } = interrupted;
    let world = server.into_world();
    let saved = read_saved_state(world.opaque_sections()).unwrap();
    assert_eq!(saved.movers.len(), 1);
    assert_eq!(saved.movers[0].tool, Tool::Subtract);
    let bytes = world.save(&content.registries()).unwrap();
    let mut content = Content2048::new();
    let parsed = world_storage::ParsedWorld::parse(&bytes).unwrap();
    let seed = parsed.world_seed();
    let (loaded, _) = parsed.load(content.load_context(seed)).unwrap();
    let mut resumed = Rig::with_world(content, loaded);
    resumed.step(18);
    assert_eq!(resumed.blocks(), control.blocks());
}

#[test]
fn a_stone_block_pushed_by_a_player_pushes_the_row_in_front_of_it_for_everyone_to_see() {
    // Three stone blocks in a row toward -z, with room beyond them; the player pushes the nearest.
    let row = [
        (at(10, SKY, 10), numbered(Material::Stone, 3)),
        (at(10, SKY, 9), numbered(Material::Stone, 5)),
        (at(10, SKY, 8), numbered(Material::Ice, 7)),
    ];
    let mut rig = Rig::with_blocks(&row);
    let mut player = rig.join_looking(at(10, SKY, 10), FaceDirection::NegativeZ);
    let mut watcher = rig.join([30.5, SKY as f64 + 0.5, 30.5], 0.0, 0.0);
    rig.settle(&mut [&mut player, &mut watcher]);
    push_with_tool(&mut player);
    rig.step(30);
    rig.settle(&mut [&mut player, &mut watcher]);

    let moved = blocks_of(&[
        (at(10, SKY, 9), numbered(Material::Stone, 3)),
        (at(10, SKY, 8), numbered(Material::Stone, 5)),
        (at(10, SKY, 7), numbered(Material::Ice, 7)),
    ]);
    assert_eq!(rig.blocks(), moved, "one step, and the ice did not coast");
    assert_eq!(blocks_seen_by(&player, &rig.content), moved);
    assert_eq!(blocks_seen_by(&watcher, &rig.content), moved);
    assert_air_has_no_number(&rig.server.world().main().overrides, &rig.content);
    assert_eq!(
        progress_of(&player),
        Progress {
            pushes: 1,
            ..Progress::default()
        }
    );

    // Pulling the nearest block back toward the player moves it alone: the row is behind it.
    pull_with_tool(&mut player);
    rig.step(3);
    rig.settle(&mut [&mut player, &mut watcher]);
    assert_eq!(
        blocks_seen_by(&watcher, &rig.content),
        blocks_of(&[
            (at(10, SKY, 10), numbered(Material::Stone, 3)),
            (at(10, SKY, 8), numbered(Material::Stone, 5)),
            (at(10, SKY, 7), numbered(Material::Ice, 7)),
        ]),
    );
}

//! Which ships a client is told of as far away (`grid_replication`, `FarShipSettings`): the ones that are not near enough for their bricks and look big enough to be
//! seen, how a ship is moved from one tier to the other, and what a far ship is and is not sent.

use protocol::{GridContent, GridTerrainState};
use voxel_space::{Orientation, WorldPosition};

use world_storage::{ParkedGrid, ShipFileName};

use super::*;
use crate::far_ship_settings::FarShipSettings;
use crate::grid_replication::{EditedBrick, KnownGrids, ShipLimits, grid_messages, ship_views};

/// A ship has to look 0.01 of a radian long to be seen, and no more than three are told of.
fn settings() -> FarShipSettings {
    FarShipSettings {
        minimum_apparent_size: 0.01,
        maximum_ships: 3,
    }
}

/// A ship of two bricks along x (64 voxels), whose first brick is brick `x` of the level.
fn ship_at_brick(world: &mut ServerWorld, block: ChannelIndex, x: i128) -> GridId {
    let ship = world
        .create_grid(
            LevelIndex::MAIN,
            GridPose::new(
                WorldPosition::new(BrickCoordinate::new(x, 0, 0), [0.0; 3]),
                Orientation::IDENTITY,
            ),
        )
        .unwrap();
    for voxel in [
        VoxelCoordinate::new(0, 0, 0),
        VoxelCoordinate::new(40, 0, 0),
    ] {
        world.grid(ship).unwrap().set(block, voxel, 2).unwrap();
    }
    ship
}

/// What a client that watches five bricks around the origin is told this tick.
fn told(
    world: &ServerWorld,
    known: &mut KnownGrids,
    aboard: Option<GridId>,
    budget: u64,
    far: Option<FarShipSettings>,
    edited: &[EditedBrick],
) -> Vec<ServerMessage> {
    told_from(
        world,
        known,
        BrickBox::around(BrickCoordinate::ORIGIN, 5),
        aboard,
        budget,
        far,
        edited,
    )
}

fn told_from(
    world: &ServerWorld,
    known: &mut KnownGrids,
    region: BrickBox,
    aboard: Option<GridId>,
    budget: u64,
    far: Option<FarShipSettings>,
    edited: &[EditedBrick],
) -> Vec<ServerMessage> {
    grid_messages(
        world,
        &ship_views(world),
        known,
        Some((LevelIndex::MAIN, region)),
        aboard,
        ShipLimits {
            maximum_bricks: budget,
            far,
        },
        edited,
    )
}

fn contents_of(messages: &[ServerMessage]) -> Vec<(GridId, GridContent)> {
    messages
        .iter()
        .filter_map(|message| match message {
            ServerMessage::GridsSpawned(states) => Some(states),
            _ => None,
        })
        .flatten()
        .map(|state| (state.id, state.content))
        .collect()
}

fn bricks_sent_for(messages: &[ServerMessage], grid: GridId) -> usize {
    messages
        .iter()
        .filter(|message| matches!(message, ServerMessage::BrickContents { grid: sent, .. } if *sent == grid))
        .count()
}

fn despawned(messages: &[ServerMessage]) -> Vec<GridId> {
    messages
        .iter()
        .filter_map(|message| match message {
            ServerMessage::GridsDespawned(ids) => Some(ids.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

#[test]
fn a_ship_out_of_the_region_is_not_told_of_unless_far_ships_are_on_and_it_looks_big_enough() {
    let (mut world, block, _) = counting_world();
    // 64 voxels long: from brick 100 (3,200 voxels) it is 0.02 of a radian, from brick 300 (9,600 voxels) 0.0067.
    let near_enough = ship_at_brick(&mut world, block, 100);
    let too_far = ship_at_brick(&mut world, block, 300);

    let mut known = KnownGrids::new();
    assert!(told(&world, &mut known, None, 1000, None, &[]).is_empty());
    assert!(known.is_empty());

    let messages = told(&world, &mut known, None, 1000, Some(settings()), &[]);
    let spawned = contents_of(&messages);
    assert_eq!(spawned.len(), 1);
    assert_eq!(spawned[0].0, near_enough);
    // The box it is told of with is the ship's own, in its own coordinates: two bricks along x.
    assert_eq!(
        spawned[0].1,
        GridContent::Far {
            bounds: BrickBox::new(BrickCoordinate::new(0, 0, 0), BrickCoordinate::new(1, 0, 0))
                .unwrap()
        }
    );
    // Nothing of a far ship is sent but its being there.
    assert_eq!(bricks_sent_for(&messages, near_enough), 0);
    assert_eq!(known.keys().copied().collect::<Vec<_>>(), [near_enough]);
    assert!(!known.contains_key(&too_far));
}

#[test]
fn a_ship_that_is_far_stays_far_a_little_past_the_size_that_made_it_far_and_one_that_was_never_far_does_not_start_there()
 {
    let (mut world, block, _) = counting_world();
    // From brick 100 it is 0.02. At brick 210 (6,736 voxels from the middle of the region) it is 0.0095: under 0.01, over 0.009, the size at which it is let go.
    let ship = ship_at_brick(&mut world, block, 100);
    let mut known = KnownGrids::new();
    told(&world, &mut known, None, 1000, Some(settings()), &[]);
    assert!(known.contains_key(&ship));
    let place = |world: &mut ServerWorld, x: i128| {
        world
            .set_grid_pose(
                ship,
                GridPose::new(
                    WorldPosition::new(BrickCoordinate::new(x, 0, 0), [0.0; 3]),
                    Orientation::IDENTITY,
                ),
            )
            .unwrap();
    };
    place(&mut world, 210);
    let messages = told(&world, &mut known, None, 1000, Some(settings()), &[]);
    assert!(
        known.contains_key(&ship),
        "a ship that is far is kept while it looks 90 percent of the size"
    );
    assert!(contents_of(&messages).is_empty() && despawned(&messages).is_empty());
    // At 0.0067 it is let go.
    place(&mut world, 300);
    let messages = told(&world, &mut known, None, 1000, Some(settings()), &[]);
    assert_eq!(despawned(&messages), [ship]);
    assert!(known.is_empty());
    // And at the 0.0095 distance it is not taken up again: one that is not known needs the full size.
    place(&mut world, 210);
    let messages = told(&world, &mut known, None, 1000, Some(settings()), &[]);
    assert!(contents_of(&messages).is_empty());
    assert!(known.is_empty());
}

#[test]
fn a_far_ship_that_comes_into_the_region_is_taken_back_and_told_of_again_with_its_bricks_and_the_other_way_round()
 {
    let (mut world, block, _) = counting_world();
    let ship = ship_at_brick(&mut world, block, 100);
    let mut known = KnownGrids::new();
    told(&world, &mut known, None, 1000, Some(settings()), &[]);

    // The client moves to where it watches the ship: near, so its two bricks follow, after the old one is gone.
    let near_region = BrickBox::around(BrickCoordinate::new(100, 0, 0), 5);
    let messages = told_from(
        &world,
        &mut known,
        near_region,
        None,
        1000,
        Some(settings()),
        &[],
    );
    assert_eq!(despawned(&messages), [ship]);
    assert_eq!(contents_of(&messages), [(ship, GridContent::Bricks)]);
    assert_eq!(bricks_sent_for(&messages, ship), 2);
    let kinds: Vec<&str> = messages
        .iter()
        .map(|message| match message {
            ServerMessage::GridsDespawned(_) => "despawned",
            ServerMessage::GridsSpawned(_) => "spawned",
            ServerMessage::BrickContents { .. } => "brick",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["despawned", "spawned", "brick", "brick"]);

    // And away again: taken back and told of as far, with no brick.
    let messages = told(&world, &mut known, None, 1000, Some(settings()), &[]);
    assert_eq!(despawned(&messages), [ship]);
    assert!(matches!(
        contents_of(&messages)[..],
        [(grid, GridContent::Far { .. })] if grid == ship
    ));
    assert_eq!(bricks_sent_for(&messages, ship), 0);
}

#[test]
fn a_far_ship_is_moved_with_changes_and_its_edits_are_not_sent_and_a_near_ships_are() {
    let (mut world, block, _) = counting_world();
    let far_ship = ship_at_brick(&mut world, block, 100);
    let near_ship = ship_at_brick(&mut world, block, 0);
    let mut known = KnownGrids::new();
    told(&world, &mut known, None, 1000, Some(settings()), &[]);
    assert_eq!(known.len(), 2);

    world
        .set_grid_pose(
            far_ship,
            GridPose::new(
                WorldPosition::new(BrickCoordinate::new(101, 0, 0), [0.0; 3]),
                Orientation::IDENTITY,
            ),
        )
        .unwrap();
    let edit = |grid| EditedBrick {
        grid,
        brick: BrickCoordinate::new(0, 0, 0),
        update: ServerMessage::CommandReply {
            text: format!("{grid:?}"),
        },
    };
    let messages = told(
        &world,
        &mut known,
        None,
        1000,
        Some(settings()),
        &[edit(far_ship), edit(near_ship)],
    );
    let moved: Vec<GridId> = messages
        .iter()
        .filter_map(|message| match message {
            ServerMessage::GridsChanged(changes) => Some(changes.iter().map(|change| change.id)),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(moved, [far_ship]);
    let edits: Vec<&ServerMessage> = messages
        .iter()
        .filter(|message| matches!(message, ServerMessage::CommandReply { .. }))
        .collect();
    assert_eq!(
        edits,
        [&ServerMessage::CommandReply {
            text: format!("{near_ship:?}")
        }]
    );
}

#[test]
fn no_more_far_ships_are_told_of_than_the_setting_allows_and_the_nearest_are_kept() {
    let (mut world, block, _) = counting_world();
    // Five ships, all big enough to be seen, at different distances: ids go up as they go away, so the order of ids is not what is kept.
    let mut ships = Vec::new();
    for x in [140, 120, 160, 100, 180] {
        ships.push((x, ship_at_brick(&mut world, block, x)));
    }
    let mut known = KnownGrids::new();
    told(&world, &mut known, None, 1000, Some(settings()), &[]);
    let mut kept: Vec<i128> = ships
        .iter()
        .filter(|(_, id)| known.contains_key(id))
        .map(|(x, _)| *x)
        .collect();
    kept.sort_unstable();
    assert_eq!(kept, [100, 120, 140]);
}

#[test]
fn a_ship_that_did_not_fit_the_budget_of_bricks_is_told_of_as_far_if_it_is_big_enough_to_see() {
    let (mut world, block, _) = counting_world();
    let first = ship_at_brick(&mut world, block, 0);
    let second = ship_at_brick(&mut world, block, 1);
    let mut known = KnownGrids::new();
    // The budget of three bricks holds the first ship (2) and not the second.
    let messages = told(&world, &mut known, None, 3, Some(settings()), &[]);
    let spawned = contents_of(&messages);
    assert_eq!(spawned[0], (first, GridContent::Bricks));
    // (A ship next to the viewer looks as big as a ship can, so it is seen as far.)
    assert!(matches!(spawned[1], (grid, GridContent::Far { .. }) if grid == second));
    assert_eq!(bricks_sent_for(&messages, second), 0);
}

#[test]
fn the_ship_the_client_is_aboard_and_a_planet_are_never_told_of_as_far() {
    let (mut world, planet, block, _) = world_with_a_planet();
    let aboard = ship_at_brick(&mut world, block, 400);
    let mut known = KnownGrids::new();
    // The viewer is a long way from both: the planet is known whole and the ship it rides is near, whatever the distance.
    let region = BrickBox::around(BrickCoordinate::new(100_000, 0, 0), 5);
    let messages = told_from(
        &world,
        &mut known,
        region,
        Some(aboard),
        1000,
        Some(settings()),
        &[],
    );
    let spawned = contents_of(&messages);
    assert_eq!(spawned.len(), 2);
    assert!(matches!(
        spawned[..],
        [
            (first, GridContent::Generated(GridTerrainState { .. })),
            (second, GridContent::Bricks)
        ] if first == planet && second == aboard
    ));
}

#[test]
fn a_ship_of_another_level_is_not_told_of_as_far() {
    let (mut world, block, _) = counting_world();
    let ship = ship_at_brick(&mut world, block, 100);
    let mut known = KnownGrids::new();
    let views = ship_views(&world);
    let messages = grid_messages(
        &world,
        &views,
        &mut known,
        Some((
            LevelIndex::new(1),
            BrickBox::around(BrickCoordinate::ORIGIN, 5),
        )),
        None,
        ShipLimits {
            maximum_bricks: 1000,
            far: Some(settings()),
        },
        &[],
    );
    assert!(messages.is_empty());
    assert!(!known.contains_key(&ship));
}

#[test]
fn a_planet_that_did_not_fit_the_budget_is_left_out_not_told_of_as_far() {
    let (mut world, planet, block, _) = world_with_a_planet();
    // A stored brick, and no room for it: a planet is drawn from its generator, which a far ship has no use of.
    world
        .grid(planet)
        .unwrap()
        .set(block, VoxelCoordinate::new(1, 1, 1), 2)
        .unwrap();
    let mut known = KnownGrids::new();
    let messages = told(&world, &mut known, None, 0, Some(settings()), &[]);
    assert!(contents_of(&messages).is_empty());
    assert!(known.is_empty());
}

#[test]
fn how_far_a_ship_is_is_measured_from_the_middle_of_the_region_to_the_middle_of_the_ship() {
    let (mut world, block, _) = counting_world();
    // The ship is 64 voxels long and the middle of the region around brick 0 is 16 voxels from its origin: a ship from brick x has its middle at 32 x + 32 voxels, so
    // at x = 199 it is 6,384 voxels away (0.01003 of a radian, seen) and at x = 200 6,416 (0.00998, not).
    let seen = ship_at_brick(&mut world, block, 199);
    let not_seen = ship_at_brick(&mut world, block, 200);
    let mut known = KnownGrids::new();
    told(&world, &mut known, None, 1000, Some(settings()), &[]);
    assert_eq!(known.keys().copied().collect::<Vec<_>>(), [seen]);
    assert!(!known.contains_key(&not_seen));

    // From another region the distance is another: the ship at 450 is 4,800 voxels from the middle of the region around brick 600, and the one at 100 is 16,000.
    let (mut world, block, _) = counting_world();
    let behind = ship_at_brick(&mut world, block, 100);
    let ahead = ship_at_brick(&mut world, block, 450);
    let mut known = KnownGrids::new();
    told_from(
        &world,
        &mut known,
        BrickBox::around(BrickCoordinate::new(600, 0, 0), 5),
        None,
        1000,
        Some(settings()),
        &[],
    );
    assert_eq!(known.keys().copied().collect::<Vec<_>>(), [ahead]);
    assert!(!known.contains_key(&behind));
}

#[test]
fn the_ships_a_client_was_told_of_as_far_are_asked_for_by_client_and_forgotten_when_it_leaves() {
    let (mut world, _, _) = counting_world();
    let (first, second) = (protocol::ClientId::new(1), protocol::ClientId::new(2));
    for client in [first, second] {
        world.add_client(client, protocol::PlayerKey::new([client.get() as u8; 32]));
    }
    let ships = |ids: &[u64]| -> std::collections::BTreeSet<GridId> {
        ids.iter()
            .map(|id| GridId::new(GridId::FIRST_ALLOCATED + id))
            .collect()
    };
    assert_eq!(world.far_ships_of(first).count(), 0);
    world.set_far_ships(first, ships(&[3, 1]));
    world.set_far_ships(second, ships(&[2]));
    assert_eq!(
        world.far_ships_of(first).collect::<Vec<_>>(),
        ships(&[1, 3]).into_iter().collect::<Vec<_>>()
    );
    assert_eq!(world.far_ships_of(second).count(), 1);
    // A later telling replaces the earlier.
    world.set_far_ships(first, ships(&[]));
    assert_eq!(world.far_ships_of(first).count(), 0);
    world.set_far_ships(first, ships(&[5]));
    world.remove_client(first);
    assert_eq!(world.far_ships_of(first).count(), 0);
    assert_eq!(world.far_ships_of(second).count(), 1);
}

// ---- ships in files ----

/// The ship put in a file: the world knows where it is and what it touched, and nothing of its bricks.
fn put_in_a_file(world: &mut ServerWorld, ship: GridId) {
    let moving = world.data().grids().get(ship).unwrap();
    let record = ParkedGrid {
        level: moving.level,
        pose: moving.pose,
        bounds: moving.world_brick_bounds(),
        extent: moving.own_brick_box(),
        file: ShipFileName::new(ship, 0),
        checksum: 0,
        waits_for_those_aboard: false,
        has_terrain: false,
        on_a_rail: false,
    };
    world.data_mut().grids_mut().park(ship, record).unwrap();
}

#[test]
fn a_ship_in_a_file_is_told_of_as_far_with_the_box_of_its_own_bricks_as_it_is_in_memory_and_never_with_bricks()
 {
    let (mut world, block, _) = counting_world();
    let ship = ship_at_brick(&mut world, block, 100);
    let in_memory = {
        let mut known = KnownGrids::new();
        contents_of(&told(&world, &mut known, None, 1000, Some(settings()), &[]))
    };
    assert_eq!(in_memory.len(), 1);
    put_in_a_file(&mut world, ship);
    assert!(world.data().grids().get(ship).is_none());

    let mut known = KnownGrids::new();
    let messages = told(&world, &mut known, None, 1000, Some(settings()), &[]);
    assert_eq!(contents_of(&messages), in_memory);
    assert!(matches!(in_memory[0].1, GridContent::Far { .. }));
    assert_eq!(bricks_sent_for(&messages, ship), 0);
    assert!(known.contains_key(&ship));
    // Nothing is said again while nothing changes.
    assert!(told(&world, &mut known, None, 1000, Some(settings()), &[]).is_empty());
}

#[test]
fn a_far_ship_that_is_put_in_a_file_is_not_told_of_again_and_one_brought_back_is_not_either() {
    let (mut world, block, _) = counting_world();
    let ship = ship_at_brick(&mut world, block, 100);
    let mut known = KnownGrids::new();
    told(&world, &mut known, None, 1000, Some(settings()), &[]);
    let moving = world.data().grids().get(ship).unwrap().clone();
    put_in_a_file(&mut world, ship);
    let messages = told(&world, &mut known, None, 1000, Some(settings()), &[]);
    assert!(messages.is_empty(), "{messages:?}");
    // And back in memory, where it was.
    world.data_mut().grids_mut().unpark(ship, moving).unwrap();
    let messages = told(&world, &mut known, None, 1000, Some(settings()), &[]);
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn a_ship_in_a_file_is_told_of_only_when_far_ships_are_on_and_is_never_given_bricks_even_in_the_region()
 {
    let (mut world, block, _) = counting_world();
    let ship = ship_at_brick(&mut world, block, 2);
    put_in_a_file(&mut world, ship);
    let mut known = KnownGrids::new();
    // (It is in the region the client watches.)
    assert!(told(&world, &mut known, None, 1000, None, &[]).is_empty());
    assert!(known.is_empty());
    let messages = told(&world, &mut known, None, 1000, Some(settings()), &[]);
    assert!(matches!(
        contents_of(&messages)[..],
        [(_, GridContent::Far { .. })]
    ));
    assert_eq!(bricks_sent_for(&messages, ship), 0);
}

#[test]
fn a_ship_in_a_file_that_is_a_planet_or_was_on_a_rail_or_has_no_bricks_is_not_told_of() {
    let (mut world, block, _) = counting_world();
    let ships: Vec<GridId> = (0..3)
        .map(|number| ship_at_brick(&mut world, block, 100 + 4 * number))
        .collect();
    for (index, ship) in ships.iter().enumerate() {
        let moving = world.data().grids().get(*ship).unwrap();
        let record = ParkedGrid {
            level: moving.level,
            pose: moving.pose,
            bounds: moving.world_brick_bounds(),
            extent: if index == 2 {
                None
            } else {
                moving.own_brick_box()
            },
            file: ShipFileName::new(*ship, 0),
            checksum: 0,
            waits_for_those_aboard: false,
            // (A planet is drawn from its generator, and a ship that was on a rail has no place that is known without its rail.)
            has_terrain: index == 0,
            on_a_rail: index == 1,
        };
        world.data_mut().grids_mut().park(*ship, record).unwrap();
    }
    let mut known = KnownGrids::new();
    let messages = told(&world, &mut known, None, 1000, Some(settings()), &[]);
    assert!(contents_of(&messages).is_empty(), "{messages:?}");
    assert!(known.is_empty());
}

#[test]
fn ships_in_files_and_in_memory_are_far_nearest_first_together_up_to_the_most_there_are() {
    let (mut world, block, _) = counting_world();
    // Four ships at 100, 104, 108 and 112 bricks; the two in the middle are in files; three are told of, the nearest, and so not the one at 112.
    let ships: Vec<GridId> = (0..4)
        .map(|number| ship_at_brick(&mut world, block, 100 + 4 * number))
        .collect();
    put_in_a_file(&mut world, ships[1]);
    put_in_a_file(&mut world, ships[2]);
    let mut known = KnownGrids::new();
    told(&world, &mut known, None, 1000, Some(settings()), &[]);
    assert_eq!(known.keys().copied().collect::<Vec<_>>(), ships[..3]);
}

#[test]
fn a_far_ship_whose_box_changes_is_told_of_again_with_the_new_one_in_the_same_tick_and_one_that_only_moves_is_not()
 {
    let (mut world, block, _) = counting_world();
    let ship = ship_at_brick(&mut world, block, 100);
    let mut known = KnownGrids::new();
    let first = told(&world, &mut known, None, 1000, Some(settings()), &[]);
    let old_box =
        BrickBox::new(BrickCoordinate::new(0, 0, 0), BrickCoordinate::new(1, 0, 0)).unwrap();
    assert_eq!(
        contents_of(&first),
        [(ship, GridContent::Far { bounds: old_box })]
    );

    // It grows by a brick along x: told of again, the old one forgotten first, in one tick.
    world
        .grid(ship)
        .unwrap()
        .set(block, VoxelCoordinate::new(70, 0, 0), 2)
        .unwrap();
    let new_box =
        BrickBox::new(BrickCoordinate::new(0, 0, 0), BrickCoordinate::new(2, 0, 0)).unwrap();
    let messages = told(&world, &mut known, None, 1000, Some(settings()), &[]);
    assert_eq!(despawned(&messages), [ship]);
    assert_eq!(
        contents_of(&messages),
        [(ship, GridContent::Far { bounds: new_box })]
    );
    assert_eq!(bricks_sent_for(&messages, ship), 0);
    // And nothing more while it stays as it is.
    assert!(told(&world, &mut known, None, 1000, Some(settings()), &[]).is_empty());

    // Digging the block away leaves the brick (an empty one is still the ship's), and so the box: nothing is said.
    world
        .grid(ship)
        .unwrap()
        .set(block, VoxelCoordinate::new(70, 0, 0), 0)
        .unwrap();
    let messages = told(&world, &mut known, None, 1000, Some(settings()), &[]);
    assert!(despawned(&messages).is_empty() && contents_of(&messages).is_empty());

    // A brick that is taken out of the ship, which is what makes the box smaller: told of again.
    world
        .data_mut()
        .grids_mut()
        .get_mut(ship)
        .unwrap()
        .voxels
        .remove_brick(BrickCoordinate::new(2, 0, 0))
        .expect("the brick was there");
    let messages = told(&world, &mut known, None, 1000, Some(settings()), &[]);
    assert_eq!(despawned(&messages), [ship]);
    assert_eq!(
        contents_of(&messages),
        [(ship, GridContent::Far { bounds: old_box })]
    );

    // A ship that only moves is not told of again, and its box is the same.
    world
        .set_grid_pose(
            ship,
            GridPose::new(
                WorldPosition::new(BrickCoordinate::new(101, 0, 0), [0.0; 3]),
                Orientation::IDENTITY,
            ),
        )
        .unwrap();
    let messages = told(&world, &mut known, None, 1000, Some(settings()), &[]);
    assert!(despawned(&messages).is_empty() && contents_of(&messages).is_empty());
}

#[test]
fn a_ship_in_the_picture_that_grows_is_not_told_of_again_for_its_box_which_only_a_far_ship_is_told_of()
 {
    let (mut world, block, _) = counting_world();
    // A ship at the origin, in the region the client watches: told of with its bricks.
    let ship = ship_at_brick(&mut world, block, 1);
    let mut known = KnownGrids::new();
    let first = told(&world, &mut known, None, 1000, Some(settings()), &[]);
    assert_eq!(contents_of(&first), [(ship, GridContent::Bricks)]);
    // It grows by a brick: the client is not told of the ship again (its new brick is an edit, and comes as one).
    world
        .grid(ship)
        .unwrap()
        .set(block, VoxelCoordinate::new(70, 0, 0), 2)
        .unwrap();
    let messages = told(&world, &mut known, None, 1000, Some(settings()), &[]);
    assert!(despawned(&messages).is_empty(), "{messages:?}");
    assert!(contents_of(&messages).is_empty());
}

//! Which ships a client is told of as far away (`grid_replication`, `FarShipSettings`): the ones that are not near enough for their bricks and look big enough to be
//! seen, how a ship is moved from one tier to the other, and what a far ship is and is not sent.

use protocol::{GridContent, GridTerrainState};
use voxel_space::{Orientation, WorldPosition};

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

//! Which ships are near a group of ships coupled by their joint ends (`ServerWorld::ships_near_group`, step (a) of 4c-5-2 in `docs/decisions/joints-and-cars.md`): the ships that
//! the contact ghost of the group is made for. Every one of them is given, not only the lowest; the measure is the hull of each ship (the box of its blocks, step 4c-5-1) and not the
//! sphere through the far corner of its bricks; and the room is the caller's: a margin in voxels and the way the two would close in some seconds.

use super::*;
use crate::server_world::ContactRoom;

/// A box of 4 voxels a side (the hull of a ship of 64 blocks): half its diagonal is `2 * sqrt(3)`.
const SIDE: i128 = 4;
const HULL_RADIUS: f64 = 3.464_101_615_137_754_6;

fn hull_at(lowest: [i128; 3]) -> HullBox {
    HullBox::of_voxels([
        VoxelCoordinate::new(lowest[0], lowest[1], lowest[2]),
        VoxelCoordinate::new(
            lowest[0] + SIDE - 1,
            lowest[1] + SIDE - 1,
            lowest[2] + SIDE - 1,
        ),
    ])
    .unwrap()
}

/// A ship with a hull of one box at its origin, put at this place, going at this speed.
fn ship_with_hull(world: &mut ServerWorld, place: [f64; 3], velocity: [f64; 3]) -> GridId {
    let ship = world
        .create_grid(
            LevelIndex::MAIN,
            GridPose::new(WorldPosition::from_voxels(place), Orientation::IDENTITY),
        )
        .unwrap();
    world.set_grid_velocity(ship, velocity, [0.0; 3]).unwrap();
    world.set_grid_hull(ship, Some(hull_at([0, 0, 0]))).unwrap();
    ship
}

/// Three ships in a row along z, 20 voxels apart from the origin, coupled one to the next, going nowhere. The world and the ships.
fn a_group() -> (ServerWorld, [GridId; 3]) {
    let mut world = world_for_rails();
    let ships =
        [0.0, 20.0, 40.0].map(|along| ship_with_hull(&mut world, [0.0, 0.0, along], [0.0; 3]));
    world.set_ships_coupled(ships[0], ships[1], true);
    world.set_ships_coupled(ships[1], ships[2], true);
    (world, ships)
}

fn near(world: &ServerWorld, anchor: GridId, room: ContactRoom) -> Vec<GridId> {
    world.ships_near_group(anchor, room)
}

#[test]
fn a_group_with_nothing_near_has_no_ships_near_it() {
    let (mut world, ships) = a_group();
    ship_with_hull(&mut world, [10_000.0, 0.0, 0.0], [0.0; 3]);
    let room = ContactRoom {
        margin: 16.0,
        seconds: 0.5,
    };
    assert_eq!(near(&world, ships[0], room), []);
}

#[test]
fn every_ship_that_is_near_is_given_in_the_order_of_their_numbers_and_not_only_the_lowest() {
    let (mut world, ships) = a_group();
    let first = ship_with_hull(&mut world, [10.0, 0.0, 0.0], [0.0; 3]);
    let far = ship_with_hull(&mut world, [5_000.0, 0.0, 0.0], [0.0; 3]);
    let second = ship_with_hull(&mut world, [0.0, 10.0, 40.0], [0.0; 3]);
    let third = ship_with_hull(&mut world, [-9.0, 0.0, 20.0], [0.0; 3]);
    let room = ContactRoom {
        margin: 16.0,
        seconds: 0.0,
    };
    let found = near(&world, ships[0], room);
    assert_eq!(found, [first, second, third]);
    assert!(!found.contains(&far));
}

#[test]
fn the_members_of_the_group_are_not_near_it_though_they_are_next_to_each_other() {
    let (world, ships) = a_group();
    let room = ContactRoom {
        margin: 1_000.0,
        seconds: 1.0,
    };
    assert_eq!(near(&world, ships[0], room), []);
}

#[test]
fn a_ship_is_near_when_the_gap_between_the_hulls_is_less_than_the_margin_and_not_when_it_is_not() {
    // The middles of the hulls are the origins plus (2, 2, 2) for both, so along x the gap is the difference of the places less the two radii.
    for (margin, along, near_it) in [
        (10.0, 2.0 * HULL_RADIUS + 10.0 - 0.01, true),
        (10.0, 2.0 * HULL_RADIUS + 10.0 + 0.01, false),
        (0.5, 2.0 * HULL_RADIUS + 0.5 - 0.01, true),
        (0.5, 2.0 * HULL_RADIUS + 0.5 + 0.01, false),
    ] {
        let (mut world, ships) = a_group();
        let stranger = ship_with_hull(&mut world, [along, 0.0, 0.0], [0.0; 3]);
        let room = ContactRoom {
            margin,
            seconds: 0.0,
        };
        let expected: &[GridId] = if near_it { &[stranger] } else { &[] };
        assert_eq!(
            near(&world, ships[0], room),
            expected,
            "margin {margin}, {along} apart"
        );
    }
}

#[test]
fn the_hull_is_what_is_measured_and_not_the_sphere_through_the_far_corner_of_the_bricks() {
    // The old rule makes a ship of one brick reach 55 voxels from its origin whatever is in it, so these two are "in the way" of each other (64 voxels of margin) at a hundred
    // voxels; the hulls of 4 voxels are 93 voxels apart.
    let (mut world, ships) = a_group();
    let stranger = ship_with_hull(&mut world, [100.0, 0.0, 0.0], [0.0; 3]);
    let room = ContactRoom {
        margin: 16.0,
        seconds: 0.0,
    };
    assert_eq!(near(&world, ships[0], room), []);
    // The same ship nearer is near it.
    world
        .set_grid_pose(
            stranger,
            GridPose::new(
                WorldPosition::from_voxels([20.0, 0.0, 0.0]),
                Orientation::IDENTITY,
            ),
        )
        .unwrap();
    assert_eq!(near(&world, ships[0], room), [stranger]);
}

#[test]
fn the_speed_two_ships_close_at_counts_and_the_speed_they_go_at_together_does_not() {
    // 50 voxels between the hulls, 10 of margin.
    let along = 2.0 * HULL_RADIUS + 50.0;
    let room = ContactRoom {
        margin: 10.0,
        seconds: 1.0,
    };
    // Both at rest: not near. The group at 100 voxels a second and the stranger at rest: near. Both going at 100: not near.
    for (group_speed, stranger_speed, near_it) in [
        (0.0, 0.0, false),
        (100.0, 0.0, true),
        (0.0, -100.0, true),
        (100.0, 100.0, false),
        (0.0, 30.0, false),
        (0.0, 41.0, true),
    ] {
        let mut world = world_for_rails();
        let ships = [0.0, 20.0, 40.0]
            .map(|z| ship_with_hull(&mut world, [0.0, 0.0, z], [group_speed, 0.0, 0.0]));
        world.set_ships_coupled(ships[0], ships[1], true);
        world.set_ships_coupled(ships[1], ships[2], true);
        let stranger = ship_with_hull(&mut world, [along, 0.0, 0.0], [stranger_speed, 0.0, 0.0]);
        let expected: &[GridId] = if near_it { &[stranger] } else { &[] };
        assert_eq!(
            near(&world, ships[0], room),
            expected,
            "group at {group_speed}, stranger at {stranger_speed}"
        );
    }
}

#[test]
fn a_ship_that_turns_reaches_by_its_turn_over_the_seconds_of_the_room() {
    // 50 voxels between the hulls and no margin: a turn of 20 radians a second moves a corner 20 * 3.46 = 69 voxels a second, so within a second it reaches; at 5 it does not.
    let along = 2.0 * HULL_RADIUS + 50.0;
    let room = ContactRoom {
        margin: 0.0,
        seconds: 1.0,
    };
    for (spin, near_it) in [(0.0, false), (5.0, false), (20.0, true)] {
        // A ship of the group that turns, and one of the stranger.
        for turning_is_the_stranger in [false, true] {
            let (mut world, ships) = a_group();
            let stranger = ship_with_hull(&mut world, [along, 0.0, 0.0], [0.0; 3]);
            let turning = if turning_is_the_stranger {
                stranger
            } else {
                ships[0]
            };
            world
                .set_grid_velocity(turning, [0.0; 3], [0.0, spin, 0.0])
                .unwrap();
            let expected: &[GridId] = if near_it { &[stranger] } else { &[] };
            assert_eq!(
                near(&world, ships[0], room),
                expected,
                "spin {spin}, the stranger turns: {turning_is_the_stranger}"
            );
        }
    }
}

#[test]
fn a_hull_that_is_far_from_the_origin_of_its_ship_is_where_the_blocks_are() {
    let (mut world, ships) = a_group();
    let room = ContactRoom {
        margin: 5.0,
        seconds: 0.0,
    };
    // A ship whose origin is 3 voxels from the group (the old rule's "in the way") and whose blocks are a thousand voxels along x: nothing of it is near.
    let remote = ship_with_hull(&mut world, [3.0, 0.0, 0.0], [0.0; 3]);
    world
        .set_grid_hull(remote, Some(hull_at([1_000, 0, 0])))
        .unwrap();
    assert_eq!(near(&world, ships[0], room), []);
    // The blocks brought to the group (4 voxels along x of its origin, 7 from the group's, a gap of 0.07 voxels between the hulls): near.
    world
        .set_grid_hull(remote, Some(hull_at([4, 0, 0])))
        .unwrap();
    assert_eq!(near(&world, ships[0], room), [remote]);
}

#[test]
fn a_hull_is_turned_with_its_ship() {
    // The group is 100 voxels along -x. Two ships have their blocks 100 voxels along +x of their origins at the origin of the world; one is turned half way about y, which
    // puts its blocks 100 voxels along -x, where the group is.
    let mut world = world_for_rails();
    let ships = [0.0, 20.0, 40.0].map(|z| ship_with_hull(&mut world, [-100.0, 0.0, z], [0.0; 3]));
    world.set_ships_coupled(ships[0], ships[1], true);
    world.set_ships_coupled(ships[1], ships[2], true);
    let mut at_the_origin = |orientation| {
        let ship = world
            .create_grid(
                LevelIndex::MAIN,
                GridPose::new(WorldPosition::from_voxels([0.0, 0.0, 0.0]), orientation),
            )
            .unwrap();
        world
            .set_grid_hull(ship, Some(hull_at([100, 0, 0])))
            .unwrap();
        ship
    };
    let unturned = at_the_origin(Orientation::IDENTITY);
    let turned = at_the_origin(Orientation::from_yaw_pitch(std::f64::consts::PI, 0.0));
    let room = ContactRoom {
        margin: 5.0,
        seconds: 0.0,
    };
    let found = near(&world, ships[0], room);
    assert_eq!(found, [turned]);
    assert!(!found.contains(&unturned));
}

#[test]
fn what_is_not_a_ship_with_blocks_is_not_near_planets_ships_without_a_hull_and_other_levels() {
    let (mut world, ships) = a_group();
    let room = ContactRoom {
        margin: 1_000.0,
        seconds: 1.0,
    };
    // A ship that has not been weighed (no hull) is nothing that can touch.
    world
        .create_grid(
            LevelIndex::MAIN,
            GridPose::new(
                WorldPosition::from_voxels([5.0, 0.0, 0.0]),
                Orientation::IDENTITY,
            ),
        )
        .unwrap();
    // A planet is the ground, which is 4c-5-3's.
    let planet = planet_on(&mut world, 3, 1.0e22, None);
    assert_eq!(near(&world, ships[0], room), []);
    // (Nor with a hull given it: a planet's blocks are mostly not stored, and the box is not the ground.)
    world
        .set_grid_hull(planet, Some(hull_at([0, 0, 0])))
        .unwrap();
    assert_eq!(near(&world, ships[0], room), []);
    // A weighed ship is near, with the same room.
    let weighed = ship_with_hull(&mut world, [5.0, 5.0, 0.0], [0.0; 3]);
    assert_eq!(near(&world, ships[0], room), [weighed]);
}

#[test]
fn a_ship_of_another_level_is_not_near() {
    let game = Game::new(&[Plugin::BaseBlocks]);
    let calls = Arc::new(AtomicUsize::new(0));
    let mut data = WorldData::new(1, main_level_id(), counting_level(&game, &calls));
    let other = data
        .add_level(id("test_content:nether"), counting_level(&game, &calls))
        .unwrap();
    let mut world = ServerWorld::new(data, Arc::default());
    let ships = [0.0, 20.0].map(|z| ship_with_hull(&mut world, [0.0, 0.0, z], [0.0; 3]));
    world.set_ships_coupled(ships[0], ships[1], true);
    let there = world
        .create_grid(
            other,
            GridPose::new(
                WorldPosition::from_voxels([5.0, 0.0, 0.0]),
                Orientation::IDENTITY,
            ),
        )
        .unwrap();
    world
        .set_grid_hull(there, Some(hull_at([0, 0, 0])))
        .unwrap();
    let room = ContactRoom {
        margin: 1_000.0,
        seconds: 1.0,
    };
    assert_eq!(near(&world, ships[0], room), []);
}

#[test]
fn only_the_anchor_of_a_group_speaks_for_it_and_a_single_ship_or_an_unknown_one_is_no_group() {
    let (mut world, ships) = a_group();
    let stranger = ship_with_hull(&mut world, [10.0, 0.0, 0.0], [0.0; 3]);
    let room = ContactRoom {
        margin: 16.0,
        seconds: 0.0,
    };
    assert_eq!(near(&world, ships[0], room), [stranger]);
    assert_eq!(near(&world, ships[1], room), []);
    assert_eq!(near(&world, ships[2], room), []);
    // (A ship that is no group has no ghost: it is not a mechanism.)
    assert_eq!(near(&world, stranger, room), []);
    assert_eq!(near(&world, GridId::new(9_999), room), []);
}

#[test]
fn a_member_without_a_hull_is_left_out_and_a_group_with_no_hull_has_nothing_near() {
    let (mut world, ships) = a_group();
    let stranger = ship_with_hull(&mut world, [0.0, 0.0, 70.0], [0.0; 3]);
    let room = ContactRoom {
        margin: 16.0,
        seconds: 0.0,
    };
    // The third ship of the group is 40 along, its hull to 44: the stranger at 70 is 26 from it, not near. The second (at 20) is farther. Without the third's hull it is as it was.
    assert_eq!(near(&world, ships[0], room), []);
    world.set_grid_hull(ships[2], None).unwrap();
    assert_eq!(near(&world, ships[0], room), []);
    // Nearer than the margin to the third: near while its hull is there, not when it is gone.
    world
        .set_grid_pose(
            stranger,
            GridPose::new(
                WorldPosition::from_voxels([0.0, 0.0, 52.0]),
                Orientation::IDENTITY,
            ),
        )
        .unwrap();
    assert_eq!(near(&world, ships[0], room), []);
    world
        .set_grid_hull(ships[2], Some(hull_at([0, 0, 0])))
        .unwrap();
    assert_eq!(near(&world, ships[0], room), [stranger]);
    // No hull anywhere in the group: nothing.
    for ship in ships {
        world.set_grid_hull(ship, None).unwrap();
    }
    assert_eq!(near(&world, ships[0], room), []);
}

#[test]
fn a_held_ship_or_another_group_is_a_stranger_like_any_other() {
    let (mut world, ships) = a_group();
    let held = ship_with_hull(&mut world, [10.0, 0.0, 0.0], [0.0; 3]);
    world.set_grid_held(held, true).unwrap();
    let other_a = ship_with_hull(&mut world, [-10.0, 0.0, 0.0], [0.0; 3]);
    let other_b = ship_with_hull(&mut world, [-10.0, 15.0, 0.0], [0.0; 3]);
    world.set_ships_coupled(other_a, other_b, true);
    let room = ContactRoom {
        margin: 20.0,
        seconds: 0.0,
    };
    assert_eq!(near(&world, ships[0], room), [held, other_a, other_b]);
    // (And the other group has the first as its stranger: two of its ships, the third is 40 voxels along z, out of the room.)
    assert_eq!(near(&world, other_a, room), [ships[0], ships[1], held]);
}

#[test]
fn the_answers_are_those_of_the_formula_on_a_crowd_of_random_ships() {
    // A small deterministic generator: the same crowd every run.
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    let (mut world, ships) = a_group();
    let mut crowd = Vec::new();
    for _ in 0..300 {
        let place = [0, 1, 2].map(|_| (random() - 0.5) * 200.0);
        let velocity = [0, 1, 2].map(|_| (random() - 0.5) * 200.0);
        let spin = [0, 1, 2].map(|_| (random() - 0.5) * 6.0);
        let ship = ship_with_hull(&mut world, place, velocity);
        world.set_grid_velocity(ship, velocity, spin).unwrap();
        crowd.push((ship, place, velocity, spin));
    }
    let room = ContactRoom {
        margin: 12.0,
        seconds: 0.25,
    };
    let found = near(&world, ships[0], room);
    let norm = |vector: [f64; 3]| libm::sqrt(vector.iter().map(|part| part * part).sum());
    let mut expected = Vec::new();
    let mut closest_to_the_edge = f64::INFINITY;
    for (ship, place, velocity, spin) in &crowd {
        // The group is at rest and does not turn; its members are at z = 0, 20, 40, hulls of the same size at the same offset.
        let mut near_it = false;
        for along in [0.0, 20.0, 40.0] {
            let apart = norm([place[0], place[1], place[2] - along]);
            let gap = apart - 2.0 * HULL_RADIUS;
            let closing = norm(*velocity) + norm(*spin) * HULL_RADIUS;
            let limit = room.margin + closing * room.seconds;
            closest_to_the_edge = closest_to_the_edge.min((gap - limit).abs());
            near_it |= gap < limit;
        }
        if near_it {
            expected.push(*ship);
        }
    }
    assert!(
        closest_to_the_edge > 1.0e-6,
        "a ship at the edge of the room, which the sum cannot tell by the rounding: {closest_to_the_edge}"
    );
    assert!(
        expected.len() > 10 && expected.len() < 250,
        "{} of the crowd are near: the crowd tests both sides",
        expected.len()
    );
    assert_eq!(found, expected);
}

/// The ships near the mechanism that `anchor` speaks for: the one ship or the group (`ServerWorld::ships_near_mechanism_lagging`, 4c-5-4c: a single ship on a rail has a frame of its
/// own while a stranger is near it, and is a side of a ghost as a group is).
fn near_mechanism(world: &ServerWorld, anchor: GridId, room: ContactRoom) -> Vec<GridId> {
    world.ships_near_mechanism_lagging(anchor, room, 0.0)
}

#[test]
fn a_ship_that_is_alone_has_the_ships_near_it_as_a_group_has() {
    let mut world = world_for_rails();
    let alone = ship_with_hull(&mut world, [0.0, 0.0, 0.0], [0.0; 3]);
    let stranger = ship_with_hull(&mut world, [10.0, 0.0, 0.0], [0.0; 3]);
    let far = ship_with_hull(&mut world, [0.0, 5_000.0, 0.0], [0.0; 3]);
    let room = ContactRoom {
        margin: 16.0,
        seconds: 0.0,
    };
    // (Both are near each other, and neither is a group: the mechanism of each is itself.)
    assert_eq!(near_mechanism(&world, alone, room), [stranger]);
    assert_eq!(near_mechanism(&world, stranger, room), [alone]);
    assert_eq!(near_mechanism(&world, far, room), []);
    // (A ship that is not there is no mechanism, and `ships_near_group` still gives nothing for a ship that is no group.)
    assert_eq!(near_mechanism(&world, GridId::new(9_999), room), []);
    assert_eq!(near(&world, alone, room), []);
}

#[test]
fn a_group_has_the_same_ships_near_it_as_a_mechanism_and_as_a_group_and_only_its_anchor_speaks_for_it()
 {
    let (mut world, ships) = a_group();
    let stranger = ship_with_hull(&mut world, [10.0, 0.0, 0.0], [0.0; 3]);
    let room = ContactRoom {
        margin: 16.0,
        seconds: 0.0,
    };
    assert_eq!(
        near_mechanism(&world, ships[0], room),
        near(&world, ships[0], room)
    );
    assert_eq!(near_mechanism(&world, ships[0], room), [stranger]);
    // (A ship of a group that is not its anchor does not speak for it, as for `ships_near_group`.)
    assert_eq!(near_mechanism(&world, ships[1], room), []);
    assert_eq!(near_mechanism(&world, ships[2], room), []);
}

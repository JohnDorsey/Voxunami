//! A group of ships coupled by their joint ends that is **alone** may have a frame of its own off a rail (step 4c-4 of `docs/decisions/joints-and-cars.md`): the bodies make a frame
//! centred on its centre of mass and in free fall, in which nothing but the group is, so the question the world answers is whether anything could touch the group. These are the
//! rules (`ServerWorld::group_is_alone`); what the bodies do with a group that is alone is tested with them (`games/voxunami_cave/tests/group_off_a_rail.rs`).
//!
//! A group is alone when it is in a level with no pull of its own, every ship of it is free (not held, not frozen, not on a rail of its own) and weighed; and no planet on a rail carries it
//! (it is then in the frame of the planet). **The ground of a planet is asked apart** (step 4c-5-3: `ground_is_in_the_way_of_group`; the frame holds it until the group touches it). **It takes the room of `factor` 2 to be alone
//! and 1 to stay so** where a room counts (a planet that carries it), so that a group does not go in and out of its frame at the edge. **Other ships are not part of the rule** (step 4c-5-2): what is near a group is the
//! contact ghost's, and the group keeps its frame and its bits whatever is near. (The world does not know the joint ends and the docking ports that would make a group leave it for
//! a coupling or a docking: the bodies add that rule, `grid_bodies`.)

use super::coasting_ships::{PULL, TICKS, ship_at, space};
use super::*;
use world_storage::rail::Centre;

/// What the planet of `space` weighs, and how far from its middle the group is (the ground reaches 3,200).
const DISTANCE: f64 = 10_000.0;

/// Whether the group is alone with the ground out of the way too, as the world said before step 4c-5-3 (the bodies now keep a group in its frame over the ground until it touches
/// it: [`ServerWorld::ground_is_in_the_way_of_group`] is theirs to ask).
fn alone(world: &ServerWorld, anchor: GridId, room: f64) -> bool {
    world.group_is_alone_but_for_the_ground(anchor, room)
        && !world.ground_is_in_the_way_of_group(anchor, room)
}

fn circular_speed(distance: f64) -> f64 {
    libm::sqrt(PULL * 1.0e22 / distance)
}

/// Three ships in a row along z, 20 voxels apart, weighed, going round the planet of `space` in a circle at this distance, coupled one to the next. The world and the ships.
fn a_group(distance: f64) -> (ServerWorld, [GridId; 3]) {
    let (mut world, _planet) = space();
    let speed = circular_speed(distance);
    let ships = [0.0, 20.0, 40.0]
        .map(|along| ship_at(&mut world, [distance, 0.0, along], [0.0, 0.0, speed]));
    for ship in ships {
        world.set_grid_mass(ship, 1000.0, [0.0; 3]).unwrap();
    }
    world.set_ships_coupled(ships[0], ships[1], true);
    world.set_ships_coupled(ships[1], ships[2], true);
    (world, ships)
}

#[test]
fn a_group_in_space_with_nothing_near_is_alone_whichever_ship_of_it_is_named_by_the_anchor() {
    let (world, ships) = a_group(DISTANCE);
    assert!(alone(&world, ships[0], 2.0));
    assert!(alone(&world, ships[0], 1.0));
    // Only the anchor, the ship with the lowest number, speaks for the group.
    assert!(!alone(&world, ships[1], 1.0));
    assert!(!alone(&world, ships[2], 1.0));
}

#[test]
fn a_ship_that_is_no_group_is_not_a_group_that_is_alone() {
    let (mut world, _) = space();
    let ship = ship_at(&mut world, [DISTANCE, 0.0, 0.0], [0.0, 0.0, 8_000.0]);
    world.set_grid_mass(ship, 1000.0, [0.0; 3]).unwrap();
    assert!(!alone(&world, ship, 1.0));
    // (Nor a ship the world does not have.)
    assert!(!alone(&world, GridId::new(9_999), 1.0));
}

#[test]
fn the_ground_of_a_planet_in_the_terrain_lookahead_is_in_the_way_and_the_room_to_stay_alone_is_half_the_room_to_become_so()
 {
    // At 7,000 from the middle the ground (3,200) is 3,800 below the group, which goes round at 9,764 voxels a second: the lookahead of the terrain is 64 + a quarter of a second of
    // that, 2,505 voxels, and twice that is 5,010. So the ground is in the way at the room to become alone and not at the room to stay.
    let (world, ships) = a_group(7_000.0);
    assert!(
        world.ground_is_in_the_way_of_group(ships[0], 2.0),
        "in the way at twice the room"
    );
    assert!(
        !world.ground_is_in_the_way_of_group(ships[0], 1.0),
        "not at the room to stay"
    );
    // Lower: in the lookahead even of the room to stay.
    let (world, ships) = a_group(5_500.0);
    assert!(world.ground_is_in_the_way_of_group(ships[0], 1.0));
    // Higher: out of the way at twice the room.
    let (world, ships) = a_group(12_000.0);
    assert!(!world.ground_is_in_the_way_of_group(ships[0], 2.0));
}

#[test]
fn the_ground_of_a_planet_is_nothing_to_whether_a_group_could_be_alone_however_near_it_is() {
    // (Step 4c-5-3: a group that has its frame keeps it over the ground, with the ground in it as a body that comes at the group, until it touches the ground: the bodies ask the
    // two questions apart. The world's other rules do not look at the ground at all: a group is alone, apart from the ground, at 12,000, 7,000, 5,500 and 3,300 from the middle
    // of a planet whose ground reaches 3,200.)
    for distance in [12_000.0, 7_000.0, 5_500.0, 3_300.0] {
        let (world, ships) = a_group(distance);
        for room in [1.0, 2.0] {
            assert!(
                world.group_is_alone_but_for_the_ground(ships[0], room),
                "a group {distance} voxels from the middle of the planet is not alone but for the ground, at room {room}"
            );
        }
    }
}

#[test]
fn a_ship_that_is_near_the_group_is_nothing_to_whether_it_is_alone_whatever_it_is_and_wherever() {
    // (Step 4c-5-2: a stranger near is the ghost's, and the group keeps its frame. Before, a ship in the way by the room the rails use, 64 voxels and a second of the speed they close
    // at, took the group out of it: these strangers did.)
    let speed = circular_speed(DISTANCE);
    for (place, velocity) in [
        // A hundred and forty voxels along, going the same way: in the way of the room to go (128) and not of the room to stay (64) in the old rule.
        ([DISTANCE, 0.0, 140.0], [0.0, 0.0, speed]),
        // Closer than that, and on the other side of the group.
        ([DISTANCE, 0.0, 100.0], [0.0, 0.0, speed]),
        ([DISTANCE, 0.0, -40.0], [0.0, 0.0, speed]),
        // Touching the first ship's place.
        ([DISTANCE, 0.0, 1.0], [0.0, 0.0, speed]),
        // Three thousand voxels away and coming at five thousand a second: on the group in a second.
        ([DISTANCE, 3_000.0, 0.0], [0.0, -5_000.0, speed]),
        // Far away.
        ([DISTANCE, 5_000.0, 0.0], [0.0, 0.0, speed]),
    ] {
        let (mut world, ships) = a_group(DISTANCE);
        ship_at(&mut world, place, velocity);
        for room in [1.0, 2.0] {
            assert!(
                alone(&world, ships[0], room),
                "a ship at {place:?} going {velocity:?} takes the group out of being alone at room {room}"
            );
        }
    }
}

#[test]
fn a_group_with_a_ship_that_is_held_or_frozen_or_unweighed_or_on_a_rail_is_not_alone() {
    let (mut world, ships) = a_group(DISTANCE);
    world.set_grid_held(ships[1], true).unwrap();
    assert!(!alone(&world, ships[0], 1.0), "held");
    let (mut world, ships) = a_group(DISTANCE);
    world.set_grid_frozen(ships[2], true).unwrap();
    assert!(!alone(&world, ships[0], 1.0), "frozen");
    let (mut world, ships) = a_group(DISTANCE);
    world.set_grid_mass(ships[1], 0.0, [0.0; 3]).unwrap();
    assert!(!alone(&world, ships[0], 1.0), "not weighed");
    // (A ship with a rail of its own is the rail's: a group on a rail has its own frame.)
    let (mut world, ships) = a_group(DISTANCE);
    world
        .set_grid_rail(
            ships[0],
            Some(world_storage::rail::Rail {
                spin: Some(world_storage::rail::Spin {
                    axis: [0.0, 1.0, 0.0],
                    period: 100.0,
                    angle: 0.0,
                }),
                orbit: None,
            }),
        )
        .unwrap();
    assert!(!alone(&world, ships[0], 1.0), "on a rail");
}

#[test]
fn a_group_in_a_level_that_pulls_is_not_alone() {
    let (mut world, ships) = a_group(DISTANCE);
    assert!(alone(&world, ships[0], 1.0));
    world.set_level_gravity(LevelIndex::MAIN, [0.0, -5.0, 0.0]);
    assert!(!alone(&world, ships[0], 1.0));
}

#[test]
fn a_group_that_a_planet_on_a_rail_carries_is_not_alone() {
    // A moon on a rail carries what is within its sphere (960 voxels: 1.5 times the 640 its ground reaches); the group has a frame that goes with the moon (the frame of a planet),
    // and not one of its own. It is 800 from the moon, clear of the ground, so that it is the carrying and nothing else that makes it not alone.
    let (mut world, planet) = space();
    let moon = planet_on(
        &mut world,
        20,
        1.0e19,
        Some(orbit_about(Centre::Grid(planet), 60_000.0, 5_000.0)),
    );
    world.data.set_clock(0);
    world.apply_rails(TICKS);
    let moon_at = world
        .data()
        .grids()
        .get(moon)
        .unwrap()
        .pose
        .position
        .in_voxels();
    let ships = [0.0, 20.0, 40.0].map(|along| {
        ship_at(
            &mut world,
            [moon_at[0] + 800.0, moon_at[1], moon_at[2] + along],
            [0.0, 0.0, 100.0],
        )
    });
    for ship in ships {
        world.set_grid_mass(ship, 1000.0, [0.0; 3]).unwrap();
    }
    world.set_ships_coupled(ships[0], ships[1], true);
    world.set_ships_coupled(ships[1], ships[2], true);
    world.apply_rails(TICKS);
    assert_eq!(
        world.carrier_of(ships[0]),
        Some(moon),
        "the premise: the moon carries them"
    );
    assert!(!alone(&world, ships[0], 1.0));
}

//! What a ship that goes at the speed of a rocket costs the server.
//!
//! Found by the owner, who flew the dev rocket up from a planet and, when they got off it, saw it and themselves "float apart at 2 jumps a second": the server's ticks took
//! 700 milliseconds. A ship is given colliders of the terrain it will meet before it meets it, the bricks within a margin of its bounds that grows with its speed; the margin was a cube
//! all round the ship, so the bricks were the cube of its speed: 36,000 at 2 kilometres a second, 800,000 at 6, in every tick. They are the cube only up to a cap now, and past it a tube
//! the way the ship goes (`physics::SweptBricks`). Counted in bricks, not in time, as `docs/decisions` has it: what is simulated must not depend on how fast a machine is.

mod common;

use server::GridView;
use voxel_space::{GridId, LevelIndex, VoxelCoordinate};
use voxunami_cave::content::CaveContent;

use common::Scene;
use common::rocket::{
    a_ship_with_a_seat_and_four_engines_over_the_planet, sit_and_let_go_going_up, state_of,
};
use common::tick_and_follow;

#[test]
fn a_ship_going_six_kilometres_a_second_is_given_colliders_for_the_bricks_ahead_and_not_for_a_cube_of_them()
 {
    let (scene, report) = Scene::reporting_in_a_new_world();
    let (mut scene, mut player, grid) = a_ship_with_a_seat_and_four_engines_over_the_planet(scene);
    sit_and_let_go_going_up(&mut scene, &mut player, grid, 6000.0);
    // The first ticks make the bricks of the whole way ahead; after that the bricks that come into it as the ship goes.
    tick_and_follow(&mut scene, &mut player, 10);
    let (begun_at, _, _) = state_of(&scene, grid);
    let mut most_in_a_tick = 0;
    for _ in 0..60 {
        tick_and_follow(&mut scene, &mut player, 1);
        most_in_a_tick = most_in_a_tick.max(report().terrain_bricks_made_last_tick);
    }
    let (ended_at, _, _) = state_of(&scene, grid);
    let travelled = common::length([0, 1, 2].map(|axis| ended_at[axis] - begun_at[axis]));
    assert!(
        travelled > 15_000.0,
        "the ship went {travelled} voxels in three seconds, not at 6,000 a second"
    );
    // A tick takes the ship 300 voxels, 9 or 10 bricks, through a tube 9 or 10 bricks across: the most of any tick measured 2,000 bricks that come into it. In a cube of the margin it measured 30,422.
    assert!(
        most_in_a_tick < 3_000,
        "{most_in_a_tick} bricks were given colliders in one tick"
    );
}

#[test]
fn a_ship_going_four_hundred_and_twenty_kilometres_a_second_through_empty_space_is_given_no_colliders_of_terrain_at_all()
 {
    // The owner left a rocket at full throttle for a day and came back at 444 Mm and 420 km/s, and the server took 9 to 20 seconds for a tick: 7,900 rings of tube, and 65,600 bricks of air
    // made in each tick. A cube that the generator says is air all through, with nothing built in it, has no ground to meet, so none of it is needed, wanted or kept.
    let (scene, report) = Scene::reporting_in_a_new_world();
    let (mut scene, mut player, grid) = a_ship_with_a_seat_and_four_engines_over_the_planet(scene);
    sit_and_let_go_going_up(&mut scene, &mut player, grid, 420_000.0);
    // The first ticks are the ship leaving the planet, whose ground is asked for while the ship is within its reach and is not left out yet; after them there is only space.
    tick_and_follow(&mut scene, &mut player, 12);
    let (begun_at, _, _) = state_of(&scene, grid);
    let mut most_in_a_tick = 0;
    for _ in 0..30 {
        tick_and_follow(&mut scene, &mut player, 1);
        most_in_a_tick = most_in_a_tick.max(report().terrain_bricks_made_last_tick);
    }
    let (ended_at, _, _) = state_of(&scene, grid);
    let travelled = common::length([0, 1, 2].map(|axis| ended_at[axis] - begun_at[axis]));
    assert!(
        travelled > 500_000.0,
        "the ship went {travelled} voxels in a second and a half, not at 420,000 a second"
    );
    assert_eq!(
        most_in_a_tick, 0,
        "{most_in_a_tick} bricks were given colliders in one tick"
    );
}

/// Where a ship that goes up at 800 voxels a second from over the planet is after 7 seconds, with a slab of stone built across its way 5,000 voxels up or not.
fn where_the_ship_is_after_seven_seconds(slab_in_its_way: bool) -> [f64; 3] {
    let (mut scene, _) = Scene::reporting_in_a_new_world();
    if slab_in_its_way {
        // (Over the planet, in the space of the level: air all through by the generator's bounds, with a floor built in it, in one go, four thick: a ship that goes this fast is kept
        // from going through a floor by solver steps, and a floor of one voxel is not thick enough for it, as for any speed over a hundred.)
        let content = CaveContent::new();
        scene.run(move |world| {
            let mut level = world.grid(GridId::root(LevelIndex::MAIN)).unwrap();
            for x in -4..12 {
                for z in -4..12 {
                    for y in 5_000 + 70_000..5_004 + 70_000 {
                        level
                            .set(
                                content.block_channel(),
                                VoxelCoordinate::new(x, y, z),
                                content.blocks.stone,
                            )
                            .unwrap();
                    }
                }
            }
        });
    }
    let (mut scene, mut player, grid) = a_ship_with_a_seat_and_four_engines_over_the_planet(scene);
    sit_and_let_go_going_up(&mut scene, &mut player, grid, 800.0);
    tick_and_follow(&mut scene, &mut player, 140);
    state_of(&scene, grid).0
}

#[test]
fn a_floor_built_in_empty_space_is_still_met_by_a_fast_ship_though_the_generator_says_the_space_is_air()
 {
    // At 800 voxels a second (a tube of bricks ahead of the ship, not the cube that a slower ship has) the ship is 5,000 voxels up in 6 seconds and a quarter. Without the floor it goes on up
    // for the rest of the 7 seconds; with it, it has colliders for the bricks of the floor, though the generator says that everything round is air, and the floor is what stops it.
    let free = where_the_ship_is_after_seven_seconds(false);
    assert!(free[1] > 75_500.0, "the free ship went to {free:?}");
    let stopped = where_the_ship_is_after_seven_seconds(true);
    assert!(
        stopped[1] < 75_000.0,
        "the ship went through the floor: it is at {stopped:?}, and at {free:?} with none"
    );
}

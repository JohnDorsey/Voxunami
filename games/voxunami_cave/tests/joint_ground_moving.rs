//! A car that is joined to the ground of a planet that moves (it goes round the home planet at 125 metres a second, and turns): the joint keeps the car at the ground end however the
//! planet goes (`docs/decisions/joints-and-cars.md`, "A structure on the ground becomes a car with `/joint unlock`").

mod common;

use common::docking::{PLAIN, Placed, ships, stone};
use common::joints::{
    Report, ball_end_toward, joint_block, minus, pairs_the_world_has_as_coupled, world_of,
};
use common::moving_planet::{
    WANDERER, orbit_of_the_home_planet, reporting_scene_with, wanderer_pack,
};
use common::{Player, Scene, length, say, ship_id, ship_pose, tick_and_follow, top_of_the_ground};
use server::GridView;
use voxel_space::{FaceDirection, GridId, VoxelCoordinate};
use voxunami_cave::content::CaveContent;

/// The wanderer, with two joint blocks facing each other on its ground at the column (0, 0) and three stones on the upper one, and a player flying over them. Returns the scene, the
/// player, what the bodies say, and the height of the top of the ground there.
fn a_structure_on_the_wanderer(spin: Option<f64>) -> (Scene, Player, Report, i128) {
    let pack = wanderer_pack(&orbit_of_the_home_planet(spin), 340, 8.0);
    let (mut scene, report) = reporting_scene_with(&pack);
    let top = top_of_the_ground(&mut scene, WANDERER, 0, 0, 9_900..10_100);
    let planet = ship_id(WANDERER);
    let blocks: Vec<Placed> = vec![
        (
            [0, top + 1, 0],
            joint_block(),
            ball_end_toward(FaceDirection::PositiveY),
        ),
        (
            [0, top + 2, 0],
            joint_block(),
            ball_end_toward(FaceDirection::NegativeY),
        ),
        ([0, top + 3, 0], stone(), PLAIN),
        ([1, top + 3, 0], stone(), PLAIN),
        ([0, top + 3, 1], stone(), PLAIN),
    ];
    let content = CaveContent::new();
    let (block_channel, state_channel, orientation_channel) = (
        content.block_channel(),
        content.state_channel(),
        content.orientation_channel(),
    );
    scene.run(move |world| {
        let mut view = world.grid(planet).unwrap();
        for ([x, y, z], block, look) in blocks {
            let at = VoxelCoordinate::new(x, y, z);
            view.set(state_channel, at, look.state).unwrap();
            view.set(orientation_channel, at, look.orientation).unwrap();
            view.set(block_channel, at, block).unwrap();
        }
    });
    let centre = ship_pose(&scene, WANDERER).position.in_voxels();
    let mut player = scene.rig.join(
        [centre[0], centre[1] + top as f64 + 8.0, centre[2]],
        0.0,
        0.0,
    );
    tick_and_follow(&mut scene, &mut player, 3);
    (scene, player, Box::new(report), top)
}

/// Where the middle of a pivot is in the world, for the car and for the planet: the pivot is at the middle of the face where the two blocks meet.
fn pivots_apart(scene: &Scene, car: GridId, top: i128) -> f64 {
    let in_the_car = world_of(scene, car, [0.5, 0.0, 0.5]);
    let in_the_planet = world_of(scene, ship_id(WANDERER), [0.5, top as f64 + 2.0, 0.5]);
    length(minus(in_the_car, in_the_planet))
}

fn the_car(scene: &Scene) -> GridId {
    ships(scene)
        .into_iter()
        .find(|id| *id != ship_id(0) && *id != ship_id(WANDERER))
        .expect("a car")
}

fn a_car_cut_out_of_the_ground_stays_joined_to_its_end_as_the_planet_goes_on(spin: Option<f64>) {
    let (mut scene, mut player, report, top) = a_structure_on_the_wanderer(spin);
    // (The planet goes six voxels a tick: the player is put over the structure where it is now and the command is heard in the tick after.)
    let centre = ship_pose(&scene, WANDERER).position.in_voxels();
    player.send_command(&format!(
        "/tp {} {} {}",
        centre[0] + 0.5,
        centre[1] + top as f64 + 6.0,
        centre[2] + 0.5
    ));
    tick_and_follow(&mut scene, &mut player, 1);
    let reply = say(&mut scene, &mut player, "/joint unlock");
    assert!(reply.contains("unlocked"), "{reply}");
    let car = the_car(&scene);
    for _ in 0..300 {
        tick_and_follow(&mut scene, &mut player, 1);
        if report().couplings == 1 {
            break;
        }
    }
    assert_eq!(
        report().couplings,
        1,
        "the car is coupled to the planet's end"
    );
    assert_eq!(
        pairs_the_world_has_as_coupled(&mut scene),
        [(ship_id(WANDERER), car)].into_iter().collect()
    );
    let before = ship_pose(&scene, WANDERER).position.in_voxels();
    tick_and_follow(&mut scene, &mut player, 300);
    let travelled = length(minus(
        ship_pose(&scene, WANDERER).position.in_voxels(),
        before,
    ));
    assert!(travelled > 1_000.0, "the planet went {travelled} voxels");
    let apart = pivots_apart(&scene, car, top);
    assert!(
        apart < 0.01,
        "the pivots are {apart} voxels apart after the planet has gone {travelled}"
    );
    assert_eq!(report().couplings, 1);
}

#[test]
fn a_car_cut_out_of_the_ground_of_a_planet_on_a_fast_orbit_stays_joined_to_its_end() {
    a_car_cut_out_of_the_ground_stays_joined_to_its_end_as_the_planet_goes_on(None);
}

#[test]
fn a_car_cut_out_of_the_ground_of_a_planet_that_also_turns_stays_joined_to_its_end() {
    // (The column is on the planet's axis: it turns under the car once in 200 seconds.)
    a_car_cut_out_of_the_ground_stays_joined_to_its_end_as_the_planet_goes_on(Some(200.0));
}

//! A group of ships coupled by their joint ends is put on a rail by itself (step 4c-3 of `docs/decisions/joints-and-cars.md`), the way a ship is: left alone for a second, with
//! nothing near it, in a world with a planet that pulls it. The rail is the orbit of the centre of mass of the group, on the ship of it with the lowest number; the bodies hold the
//! group in a frame that goes with the rail (`group_on_a_rail.rs`, where the rails are made by hand).
//!
//! The chain of three cars of `common/chains.rs` swings on its joints the whole time: what a mechanism does is its own, and the rail is not touched by it.

mod common;

use common::chains::{
    Vec3, a_chain, a_chain_in_orbit, a_swing, centre_of, close, pivot_gap, rail_point, set_going,
    spin_of,
};
use common::docking::{make_all_going, stones};
use common::{Scene, circular_speed, length, pose_at, ship_id};
use server::GridView;
use voxel_space::{GridId, GridPose, Orientation, VoxelCoordinate, WorldPosition};
use voxunami_cave::content::CaveContent;
use world_storage::rail::Rail;

const DISTANCE: f64 = 70_016.0;

/// The pivots of the chain of `a_chain`: the east pivot of each car, with the car it is joined to, from the number of the first car.
fn gaps(scene: &Scene, ships: &[GridId]) -> f64 {
    pivot_gap(scene, (ships[0], 7.0), ships[1]).max(pivot_gap(scene, (ships[1], 6.0), ships[2]))
}

/// The rail on the ship, if it has one.
fn rail_of(scene: &Scene, ship: GridId) -> Option<Rail> {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship)
        .unwrap()
        .rail
        .clone()
}

/// A chain in orbit that swings and is left alone, and the scene, its report and its speed. It has been set going in the last tick, which is the last time anything touched it.
fn a_chain_left_alone() -> (
    Scene,
    impl Fn() -> grid_bodies::BodiesReport,
    Vec<GridId>,
    f64,
) {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    let ships = a_chain_in_orbit(&mut scene, a_chain(), DISTANCE, speed);
    assert_eq!(report().couplings, 2, "the bodies coupled the chain");
    set_going(&mut scene, &ships, speed, a_swing());
    (scene, report, ships, speed)
}

#[test]
fn a_chain_left_alone_is_put_on_a_rail_after_a_second_and_rides_it_while_its_joints_swing() {
    let (mut scene, report, ships, _speed) = a_chain_left_alone();
    // Nothing has a rail for the first second, and then the anchor has one (and the others have none).
    let mut first = None;
    for tick in 1..=30 {
        scene.ticks(1);
        if first.is_none() && rail_of(&scene, ships[0]).is_some() {
            first = Some(tick);
        }
    }
    let first = first.expect("the chain was put on a rail");
    assert!((20..=22).contains(&first), "after {first} ticks");
    for ship in &ships[1..] {
        assert_eq!(rail_of(&scene, *ship), None, "{ship}");
    }
    // It rides it: the centre of mass of the group is the rail's, and goes as the rail does, and the joints hold and swing.
    let rail = rail_of(&scene, ships[0]).unwrap();
    let mut worst_gap: f64 = 0.0;
    let mut most_spin: f64 = 0.0;
    for tick in 1..=300 {
        scene.ticks(1);
        let (place, going) = centre_of(&scene, &ships);
        let (on_the_rail, rail_speed) = rail_point(&scene, &rail);
        close(
            place,
            on_the_rail,
            1e-6,
            &format!("tick {tick}: the centre of mass is the rail's"),
        );
        close(
            going,
            rail_speed,
            1e-6,
            &format!("tick {tick}: and goes as the rail does"),
        );
        worst_gap = worst_gap.max(gaps(&scene, &ships));
        most_spin = ships
            .iter()
            .map(|ship| length(spin_of(&scene, *ship)))
            .fold(most_spin, f64::max);
    }
    assert!(worst_gap < 0.05, "the joints hold: {worst_gap} m");
    assert!(most_spin > 0.05, "and the chain swings: {most_spin} rad/s");
    assert_eq!(report().couplings, 2);
    assert_eq!(
        rail_of(&scene, ships[0]),
        Some(rail),
        "and the rail is what it was"
    );
}

#[test]
fn a_chain_swings_on_its_rail_for_a_simulated_hour_and_its_centre_of_mass_stays_on_the_rail() {
    // An hour of the world is 72,000 ticks, 6.2 times round the planet at this distance. The rail is data and does not change; what could change is where the group is, which is
    // the bodies' work and the world's: this is the test that "joint motion changes nothing of the rail".
    let (mut scene, report, ships, _speed) = a_chain_left_alone();
    scene.ticks(25);
    let rail = rail_of(&scene, ships[0]).expect("on a rail");
    let mut worst_off: f64 = 0.0;
    let mut worst_gap: f64 = 0.0;
    let mut most_spin: f64 = 0.0;
    for _ in 0..720 {
        scene.ticks(100);
        let (place, going) = centre_of(&scene, &ships);
        let (on_the_rail, rail_speed) = rail_point(&scene, &rail);
        let off = (0..3)
            .map(|axis| {
                (place[axis] - on_the_rail[axis])
                    .abs()
                    .max((going[axis] - rail_speed[axis]).abs())
            })
            .fold(0.0, f64::max);
        worst_off = worst_off.max(off);
        worst_gap = worst_gap.max(gaps(&scene, &ships));
        most_spin = ships
            .iter()
            .map(|ship| length(spin_of(&scene, *ship)))
            .fold(most_spin, f64::max);
    }
    // (Measured, 2026-11-11: 2.3e-13 off the rail at the worst, joints within 1.1e-6, the chain still turning at 0.38 radians a second.)
    assert!(
        worst_off < 1e-9,
        "the centre of mass was {worst_off} off the rail at the worst"
    );
    assert!(worst_gap < 0.05, "the joints held: {worst_gap} m");
    assert!(
        most_spin > 0.05,
        "and the chain was still swinging: {most_spin} rad/s"
    );
    assert_eq!(report().couplings, 2);
    assert_eq!(rail_of(&scene, ships[0]), Some(rail));
}

#[test]
fn a_push_takes_a_riding_chain_off_its_rail_and_left_alone_again_it_is_put_on_another() {
    let (mut scene, report, ships, _speed) = a_chain_left_alone();
    scene.ticks(30);
    let first = rail_of(&scene, ships[0]).expect("on a rail");
    // A force on the middle car for a tick: the rail comes off the anchor in that tick.
    let middle = ships[1];
    scene.run(move |world| {
        world
            .apply_grid_force(middle, [0.0, 60_000.0, 0.0], None)
            .unwrap()
    });
    assert_eq!(rail_of(&scene, ships[0]), None);
    // It is a chain still, and in the second that it is left alone it is on a rail again: another, the orbit of what it is now.
    let mut worst_gap: f64 = 0.0;
    let mut second = None;
    for tick in 1..=40 {
        scene.ticks(1);
        worst_gap = worst_gap.max(gaps(&scene, &ships));
        if second.is_none() && rail_of(&scene, ships[0]).is_some() {
            second = Some(tick);
        }
    }
    let second = second.expect("it was put on a rail again");
    assert!((19..=22).contains(&second), "after {second} ticks");
    assert!(worst_gap < 0.05, "the joints held: {worst_gap} m");
    let rail = rail_of(&scene, ships[0]).unwrap();
    assert_ne!(
        rail, first,
        "and the orbit is not the one it had: the push changed it"
    );
    for _ in 0..100 {
        scene.ticks(1);
        close(
            centre_of(&scene, &ships).0,
            rail_point(&scene, &rail).0,
            1e-6,
            "it rides the new rail",
        );
    }
    assert_eq!(report().couplings, 2);
}

#[test]
fn a_ship_that_comes_near_a_riding_chain_takes_it_off_its_rail_and_it_is_put_on_one_when_the_ship_has_gone()
 {
    let (mut scene, report, ships, speed) = a_chain_left_alone();
    // A stranger of three blocks in the same orbit, 300 voxels along x from the chain: not near it (the room to go on a rail is 128).
    make_all_going(
        &mut scene,
        vec![(pose_at(300.0, DISTANCE, 0.0), stones(0, 3))],
        [0.0, 0.0, speed],
    );
    let stranger = ship_id(4);
    scene.ticks(30);
    assert!(
        rail_of(&scene, ships[0]).is_some(),
        "the chain is on a rail, the stranger far off"
    );
    // Brought to 20 voxels from the middle of the middle car (and going as it goes): the chain comes off its rail.
    let (place, going) = centre_of(&scene, &ships);
    scene.run(move |world| {
        let near = WorldPosition::from_voxels([place[0], place[1] + 20.0, place[2]]);
        world
            .set_grid_pose(stranger, GridPose::new(near, Orientation::IDENTITY))
            .unwrap();
        world.set_grid_velocity(stranger, going, [0.0; 3]).unwrap();
    });
    scene.ticks(2);
    assert_eq!(rail_of(&scene, ships[0]), None, "the chain is off its rail");
    assert_eq!(report().couplings, 2, "and it is coupled still");
    // The stranger goes away; the chain, left alone, is put on a rail again.
    let (place, going) = centre_of(&scene, &ships);
    scene.run(move |world| {
        let far = WorldPosition::from_voxels([place[0] + 5_000.0, place[1], place[2]]);
        world
            .set_grid_pose(stranger, GridPose::new(far, Orientation::IDENTITY))
            .unwrap();
        world.set_grid_velocity(stranger, going, [0.0; 3]).unwrap();
    });
    scene.ticks(30);
    assert!(
        rail_of(&scene, ships[0]).is_some(),
        "the chain is on a rail again"
    );
}

#[test]
fn a_chain_on_a_rail_that_is_saved_and_loaded_is_where_the_rail_says_and_a_chain_still() {
    let (mut scene, _report, ships, _speed) = a_chain_left_alone();
    scene.ticks(60);
    let rail = rail_of(&scene, ships[0]).expect("on a rail");
    assert!(
        gaps(&scene, &ships) < 0.005,
        "the joints are together when it is saved"
    );
    let spins: Vec<Vec3> = ships.iter().map(|ship| spin_of(&scene, *ship)).collect();
    assert!(spins.iter().any(|spin| length(*spin) > 0.05));

    let (mut scene, report) = scene.restarted_reporting();
    // The pairs came back with the world, and the rail with the anchor's file; every ship was brought in with the anchor, and the chain rides.
    assert_eq!(scene.rig.server.world().grids().coupled().len(), 2);
    scene.ticks(10);
    assert_eq!(
        rail_of(&scene, ships[0]),
        Some(rail.clone()),
        "the rail is what it was"
    );
    assert_eq!(report().couplings, 2, "and the bodies coupled it again");
    for tick in 1..=100 {
        scene.ticks(1);
        close(
            centre_of(&scene, &ships).0,
            rail_point(&scene, &rail).0,
            1e-6,
            &format!("tick {tick} after the load: the centre of mass is the rail's"),
        );
    }
    assert!(gaps(&scene, &ships) < 0.05);
    assert_eq!(report().couplings, 2);
}

#[test]
fn a_car_cut_in_two_while_the_chain_rides_takes_the_rail_from_it_and_nothing_breaks() {
    let (mut scene, _report, ships, _speed) = a_chain_left_alone();
    scene.ticks(30);
    assert!(rail_of(&scene, ships[0]).is_some());
    // A stone out of the middle of the middle car: it is two ships, and the chain, which was three, is not what the rail was made for.
    let content = CaveContent::new();
    let middle = ships[1];
    scene.run(move |world| {
        world
            .grid(middle)
            .unwrap()
            .set(
                content.block_channel(),
                VoxelCoordinate::new(3, 0, 0),
                content.blocks.air,
            )
            .unwrap();
    });
    scene.ticks(10);
    assert_eq!(rail_of(&scene, ships[0]), None, "the rail is gone");
    let count = scene
        .rig
        .server
        .world()
        .grids()
        .iter()
        .filter(|(_, grid)| grid.terrain.is_none())
        .count();
    assert_eq!(count, 4, "and the car is two cars");
    // Whatever the bodies make of the pieces, they go on: a hundred ticks of the world with no panic, nothing lost.
    scene.ticks(100);
    assert_eq!(
        scene
            .rig
            .server
            .world()
            .grids()
            .iter()
            .filter(|(_, grid)| grid.terrain.is_none())
            .count(),
        4
    );
}

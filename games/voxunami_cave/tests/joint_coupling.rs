//! Coupling in Cave (step 3 of `docs/decisions/joints-and-cars.md`): ships with dev ball joints are drawn together by their joint ends, coupled when the ends are together, in
//! line and slow, held together by a ball joint, and let go when an end is gone. What is not here: the group moving between frames, rails and freezing, saving (step 4).

mod common;

use common::docking::{PLAIN, Placed, make, stones};
use common::joints::{ball_end_toward, joint_block};
use common::{Scene, ship_id};
use server::GridView;
use voxel_space::{
    BrickCoordinate, FaceDirection, GravitySettings, GridId, GridPose, LevelIndex, Orientation,
    VoxelCoordinate, WorldPosition,
};
use voxunami_cave::content::CaveContent;

type Vec3 = [f64; 3];

fn pose(x: f64, y: f64, z: f64) -> GridPose {
    GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z]),
        Orientation::IDENTITY,
    )
}

/// No gravity: cars hang in the air where they are put, and the only forces are the joint ends'.
fn weightless(scene: &mut Scene) {
    scene.run(|world| {
        world.set_gravity_settings(GravitySettings {
            base: [0.0; 3],
            ..GravitySettings::default()
        });
    });
}

/// A car of `stones` stones in a row along x, with a dev ball joint after them whose ball end looks east. Its pivot is at x = `stones + 1`, in the middle of the face.
fn east_car(stones_count: i128) -> Vec<Placed> {
    let mut blocks = stones(0, stones_count);
    blocks.push((
        [stones_count, 0, 0],
        joint_block(),
        ball_end_toward(FaceDirection::PositiveX),
    ));
    blocks
}

/// A car with a dev ball joint before `stones` stones in a row along x, whose ball end looks west. Its pivot is at x = 0.
fn west_car(stones_count: i128) -> Vec<Placed> {
    let mut blocks = vec![(
        [0, 0, 0],
        joint_block(),
        ball_end_toward(FaceDirection::NegativeX),
    )];
    blocks.extend(stones(1, stones_count + 1));
    blocks
}

/// The place of the middle of a face of a car's voxel, in voxels from the world's origin, with the car where the world has it.
fn world_of(scene: &Scene, id: GridId, local: Vec3) -> Vec3 {
    let ship = scene.rig.server.world().grids().get(id).unwrap();
    let rotation = ship.pose.rotation_matrix();
    let origin = ship
        .pose
        .position
        .relative_to(WorldPosition::from_voxels([0.0; 3]));
    [0, 1, 2].map(|axis| {
        origin[axis]
            + (0..3)
                .map(|column| rotation[axis][column] * local[column])
                .sum::<f64>()
    })
}

fn minus(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn length(a: Vec3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// How far apart the two pivots (a local place in each car) are, in metres.
fn pivot_gap(scene: &Scene, first: (GridId, Vec3), second: (GridId, Vec3)) -> f64 {
    length(minus(
        world_of(scene, first.0, first.1),
        world_of(scene, second.0, second.1),
    ))
}

/// The velocity of a place in a car (its centre of mass' velocity and what its turning adds), in metres a second.
fn velocity_at(scene: &Scene, id: GridId, local: Vec3) -> Vec3 {
    let ship = scene.rig.server.world().grids().get(id).unwrap();
    let centre = world_of(scene, id, ship.centre_of_mass);
    let arm = minus(world_of(scene, id, local), centre);
    let spun = cross(ship.angular_velocity, arm);
    [0, 1, 2].map(|axis| ship.linear_velocity[axis] + spun[axis])
}

/// The momentum of some cars, in kilogram metres a second.
fn momentum_of(scene: &Scene, ids: &[GridId]) -> Vec3 {
    let mut total = [0.0; 3];
    for id in ids {
        let ship = scene.rig.server.world().grids().get(*id).unwrap();
        for (sum, speed) in total.iter_mut().zip(ship.linear_velocity) {
            *sum += ship.mass * speed;
        }
    }
    total
}

/// Makes a car that goes at `velocity` from the tick it is made in (it is set going in the same tick that makes it, or the ships it is near would act on it at rest first).
fn make_going(scene: &mut Scene, at: GridPose, blocks: Vec<Placed>, velocity: Vec3) {
    let content = CaveContent::new();
    scene.run(move |world| {
        let grid = world.create_grid(LevelIndex::MAIN, at).unwrap();
        {
            let mut view = world.grid(grid).unwrap();
            for ([x, y, z], block, look) in blocks {
                let voxel = VoxelCoordinate::new(x, y, z);
                view.set(content.state_channel(), voxel, look.state)
                    .unwrap();
                view.set(content.orientation_channel(), voxel, look.orientation)
                    .unwrap();
                view.set(content.block_channel(), voxel, block).unwrap();
            }
        }
        world.set_grid_velocity(grid, velocity, [0.0; 3]).unwrap();
    });
}

/// Two cars on a line (an east car and a west car) with their pivots `distance` metres apart, the first at rest and the second going at `velocity`, in a world without weight.
/// Returns the scene, the report, and the two cars' pivots (each a local place) for measuring.
fn two_cars_going(
    distance: f64,
    east_stones: i128,
    west_stones: i128,
    velocity: Vec3,
) -> (
    Scene,
    impl Fn() -> grid_bodies::BodiesReport,
    (GridId, Vec3),
    (GridId, Vec3),
) {
    let (mut scene, report) = Scene::reporting();
    weightless(&mut scene);
    make(
        &mut scene,
        pose(20.0, 230.0, 20.0),
        false,
        east_car(east_stones),
    );
    make_going(
        &mut scene,
        pose(20.0 + east_stones as f64 + 1.0 + distance, 230.0, 20.0),
        west_car(west_stones),
        velocity,
    );
    (
        scene,
        report,
        (ship_id(0), [east_stones as f64 + 1.0, 0.5, 0.5]),
        (ship_id(1), [0.0, 0.5, 0.5]),
    )
}

/// Two cars at rest (see [`two_cars_going`]).
fn two_cars(
    distance: f64,
    east_stones: i128,
    west_stones: i128,
) -> (
    Scene,
    impl Fn() -> grid_bodies::BodiesReport,
    (GridId, Vec3),
    (GridId, Vec3),
) {
    two_cars_going(distance, east_stones, west_stones, [0.0; 3])
}

/// Ticks until two cars are coupled (the report says so) or `limit` ticks have gone. How many ticks.
fn until_coupled(
    scene: &mut Scene,
    report: &impl Fn() -> grid_bodies::BodiesReport,
    limit: usize,
) -> usize {
    let mut ticks = 0;
    while report().couplings == 0 && ticks < limit {
        scene.ticks(1);
        ticks += 1;
    }
    ticks
}

#[test]
fn two_cars_with_joint_ends_facing_each_other_are_pulled_together_and_coupled_and_stay_so() {
    let (mut scene, report, first, second) = two_cars(3.0, 3, 3);
    let masses: Vec<f64> = [first.0, second.0]
        .iter()
        .map(|id| scene.rig.server.world().grids().get(*id).unwrap().mass)
        .collect();
    let ticks = until_coupled(&mut scene, &report, 400);
    assert_eq!(report().couplings, 1, "after {ticks} ticks");
    assert_eq!(report().couplings_made, 1);
    // Coupled slowly: the ends were this fast against each other when the joint was made (the speeds are those of the tick before, which the rule was given).
    let speed = length(minus(
        velocity_at(&scene, second.0, second.1),
        velocity_at(&scene, first.0, first.1),
    ));
    assert!(speed < 1.0, "{speed} m/s after {ticks} ticks");
    // The ends are together, and stay so: the joint is the thing holding them now.
    scene.ticks(200);
    assert_eq!(report().couplings, 1);
    assert_eq!(report().couplings_made, 1, "it was made once and kept");
    let gap = pivot_gap(&scene, first, second);
    assert!(gap < 0.005, "{gap} m");
    // The pull gave each car the same force the other way: the cars had no momentum, and have none, whatever they weigh.
    let momentum = momentum_of(&scene, &[first.0, second.0]);
    let scale = masses.iter().sum::<f64>();
    assert!(
        length(momentum) / scale < 1e-6,
        "{momentum:?} for {masses:?}"
    );
}

#[test]
fn a_car_ten_times_heavier_than_the_other_and_one_that_is_held_are_coupled_just_the_same() {
    for (east_stones, west_stones, held) in [(3, 30, false), (30, 3, false), (3, 3, true)] {
        let (mut scene, report) = Scene::reporting();
        weightless(&mut scene);
        make(
            &mut scene,
            pose(20.0, 230.0, 20.0),
            held,
            east_car(east_stones),
        );
        make(
            &mut scene,
            pose(20.0 + east_stones as f64 + 1.0 + 4.0, 230.0, 20.0),
            false,
            west_car(west_stones),
        );
        let first = (ship_id(0), [east_stones as f64 + 1.0, 0.5, 0.5]);
        let second = (ship_id(1), [0.0, 0.5, 0.5]);
        let before = world_of(&scene, first.0, first.1);
        let ticks = until_coupled(&mut scene, &report, 600);
        assert_eq!(
            report().couplings,
            1,
            "{east_stones} and {west_stones} stones, held {held}: {ticks} ticks"
        );
        scene.ticks(100);
        let gap = pivot_gap(&scene, first, second);
        assert!(gap < 0.005, "{east_stones} {west_stones}: {gap} m");
        if held {
            // A car that is held does not move: the other came to it.
            assert_eq!(world_of(&scene, first.0, first.1), before);
        }
    }
}

#[test]
fn ends_that_face_the_same_way_or_are_out_of_range_neither_pull_nor_couple() {
    // Out of range: 7 metres between the pivots, the range is 6.
    let (mut scene, report, first, second) = two_cars(7.0, 3, 3);
    scene.ticks(100);
    assert_eq!(report().couplings, 0);
    assert_eq!(pivot_gap(&scene, first, second), 7.0, "not pulled at all");

    // Facing the same way: two east cars in a row, the second's end looking away from the first's.
    let (mut scene, report) = Scene::reporting();
    weightless(&mut scene);
    make(&mut scene, pose(20.0, 230.0, 20.0), false, east_car(3));
    make(&mut scene, pose(28.0, 230.0, 20.0), false, east_car(3));
    scene.ticks(100);
    assert_eq!(report().couplings, 0);
    for id in [ship_id(0), ship_id(1)] {
        let speed = length(
            scene
                .rig
                .server
                .world()
                .grids()
                .get(id)
                .unwrap()
                .linear_velocity,
        );
        assert_eq!(speed, 0.0, "nothing pulled {id:?}");
    }
}

#[test]
fn ends_are_never_coupled_while_they_go_past_each_other_faster_than_a_metre_a_second() {
    // At one place, and the second car going sideways at 3 metres a second against the first (too fast; the limit is one).
    let (mut scene, report, first, second) = two_cars_going(0.0, 3, 3, [0.0, 3.0, 0.0]);
    assert_eq!(
        report().couplings,
        0,
        "not in the tick they are made in, at 3 m/s"
    );
    // The pull takes the speed off (it slows the ends against each other as well as drawing them in), and they are coupled once they are slow enough: the speed they had
    // when the coupling was made, which is what the rule was given, is the one the world had at the start of that tick.
    let mut ticks = 0;
    loop {
        let before = length(minus(
            velocity_at(&scene, second.0, second.1),
            velocity_at(&scene, first.0, first.1),
        ));
        scene.ticks(1);
        ticks += 1;
        if report().couplings == 1 {
            assert!(
                before <= 1.0,
                "coupled going {before} m/s after {ticks} ticks"
            );
            break;
        }
        assert!(ticks < 300, "never coupled; going {before} m/s");
    }
    let gap = pivot_gap(&scene, first, second);
    assert!(gap < 0.3, "{gap} m");
}

#[test]
fn a_coupled_pair_swings_about_its_ball_and_keeps_its_momentum_and_its_ends_together() {
    let (mut scene, report, first, second) = two_cars(3.0, 3, 3);
    until_coupled(&mut scene, &report, 400);
    assert_eq!(report().couplings, 1);
    scene.ticks(60);
    let before = momentum_of(&scene, &[first.0, second.0]);
    // Push the second car sideways (a command: a velocity it did not have) and watch the pair swing for ten seconds.
    scene.run(|world| {
        world
            .set_grid_velocity(ship_id(1), [0.0, 2.0, 0.0], [0.0; 3])
            .unwrap();
    });
    let mass_second = scene.rig.server.world().grids().get(second.0).unwrap().mass;
    let pushed = momentum_of(&scene, &[first.0, second.0]);
    let mut worst: f64 = 0.0;
    for _ in 0..200 {
        scene.ticks(1);
        worst = worst.max(pivot_gap(&scene, first, second));
    }
    assert_eq!(report().couplings, 1, "it is not broken by the swing");
    assert!(worst < 0.02, "the ends came {worst} m apart");
    // The command added mass x 2 to the momentum, and then nothing did: the joint conserves it to rounding.
    let after = momentum_of(&scene, &[first.0, second.0]);
    let change = minus(pushed, before);
    assert!((change[1] - mass_second * 2.0).abs() < 1e-3, "{change:?}");
    assert!(
        length(minus(after, pushed)) / mass_second < 1e-4,
        "{after:?} from {pushed:?}"
    );
}

#[test]
fn a_coupled_pair_drifting_slowly_does_not_fall_asleep_and_stop() {
    // A mechanism that stops because its cars fell asleep works differently from one that does not (`decisions/joints-and-cars.md`, "Promises", answer 10). A pair that is
    // not coupled and goes this slowly (5 centimetres a second) is put to sleep by the bodies after a couple of seconds, and stops dead; a coupled pair may not sleep.
    let (mut scene, report, first, second) = two_cars(3.0, 3, 3);
    until_coupled(&mut scene, &report, 400);
    assert_eq!(report().couplings, 1);
    scene.ticks(20);
    scene.run(|world| {
        for id in [ship_id(0), ship_id(1)] {
            world
                .set_grid_velocity(id, [0.0, 0.05, 0.0], [0.0; 3])
                .unwrap();
        }
    });
    let started = world_of(&scene, first.0, first.1)[1];
    scene.ticks(200);
    let travelled = world_of(&scene, first.0, first.1)[1] - started;
    assert!(
        (travelled - 0.05 * 10.0).abs() < 0.02,
        "went {travelled} m in ten seconds at 5 cm/s"
    );
    for id in [first.0, second.0] {
        let speed = length(
            scene
                .rig
                .server
                .world()
                .grids()
                .get(id)
                .unwrap()
                .linear_velocity,
        );
        assert!((speed - 0.05).abs() < 1e-3, "{id:?} went on at {speed} m/s");
    }
    // The same two cars, not coupled (the ends facing away), stop: that is what this rules out.
    let (mut loose, _) = Scene::reporting();
    weightless(&mut loose);
    make(&mut loose, pose(20.0, 230.0, 20.0), false, stones(0, 3));
    loose.run(|world| {
        world
            .set_grid_velocity(ship_id(0), [0.0, 0.05, 0.0], [0.0; 3])
            .unwrap();
    });
    loose.ticks(200);
    let speed = length(
        loose
            .rig
            .server
            .world()
            .grids()
            .get(ship_id(0))
            .unwrap()
            .linear_velocity,
    );
    assert_eq!(
        speed, 0.0,
        "a car of stone on its own goes to sleep and stops"
    );
}

#[test]
fn taking_away_an_end_lets_the_pair_go_and_they_go_on_at_the_speeds_they_had() {
    let (mut scene, report, first, second) = two_cars(3.0, 3, 3);
    until_coupled(&mut scene, &report, 400);
    assert_eq!(report().couplings, 1);
    scene.run(|world| {
        world
            .set_grid_velocity(ship_id(1), [0.0, 2.0, 0.0], [0.0; 3])
            .unwrap();
    });
    scene.ticks(10);
    // The joint block of the second car is broken.
    let content = CaveContent::new();
    scene.run(move |world| {
        let mut view = world.grid(ship_id(1)).unwrap();
        view.set(
            content.block_channel(),
            VoxelCoordinate::new(0, 0, 0),
            content.blocks.air,
        )
        .unwrap();
    });
    scene.ticks(2);
    assert_eq!(report().couplings, 0);
    // They are apart for good: no pull (the end that pulled is gone), so the gap grows as they part.
    let early = pivot_gap(&scene, first, second);
    scene.ticks(60);
    assert_eq!(report().couplings, 0);
    assert_eq!(report().couplings_made, 1, "not made again");
    assert!(pivot_gap(&scene, first, second) > early);
}

#[test]
fn a_chain_of_three_cars_is_two_couplings_and_cars_that_do_not_face_are_not_coupled() {
    let (mut scene, report) = Scene::reporting();
    weightless(&mut scene);
    // A, B, C along x: A's east end faces B's west end; B's east end faces C's west end. B has a joint at both ends.
    let mut middle = vec![(
        [0, 0, 0],
        joint_block(),
        ball_end_toward(FaceDirection::NegativeX),
    )];
    middle.extend(stones(1, 4));
    middle.push((
        [4, 0, 0],
        joint_block(),
        ball_end_toward(FaceDirection::PositiveX),
    ));
    make(&mut scene, pose(20.0, 230.0, 20.0), false, east_car(3)); // pivot at x = 24
    make(&mut scene, pose(27.0, 230.0, 20.0), false, middle); // pivots at 27 and 32
    make(&mut scene, pose(35.0, 230.0, 20.0), false, west_car(3)); // pivot at 35
    for _ in 0..600 {
        scene.ticks(1);
        if report().couplings == 2 {
            break;
        }
    }
    assert_eq!(report().couplings, 2);
    scene.ticks(100);
    assert_eq!(report().couplings, 2);
    assert_eq!(report().couplings_made, 2);
    // A and C never faced each other's ends: only the two couplings of the chain.
    let gaps = [
        pivot_gap(
            &scene,
            (ship_id(0), [4.0, 0.5, 0.5]),
            (ship_id(1), [0.0, 0.5, 0.5]),
        ),
        pivot_gap(
            &scene,
            (ship_id(1), [5.0, 0.5, 0.5]),
            (ship_id(2), [0.0, 0.5, 0.5]),
        ),
    ];
    for gap in gaps {
        assert!(gap < 0.01, "{gaps:?}");
    }
}

#[test]
fn two_cars_with_two_pairs_of_ends_between_them_are_coupled_at_one_pair() {
    // The first car has a joint end on its east and one on its north; the second has two that face them, side by side: each pair is close. One ball is made between two cars
    // for now (a second ball on the same two cars is for the integer rule of the decision file), at the closest pair.
    let (mut scene, report) = Scene::reporting();
    weightless(&mut scene);
    let mut first = stones(0, 3);
    first.push((
        [3, 0, 0],
        joint_block(),
        ball_end_toward(FaceDirection::PositiveX),
    ));
    first.push((
        [3, 0, 1],
        joint_block(),
        ball_end_toward(FaceDirection::PositiveX),
    ));
    first.push(([0, 0, 1], common::docking::stone(), PLAIN));
    first.push(([1, 0, 1], common::docking::stone(), PLAIN));
    first.push(([2, 0, 1], common::docking::stone(), PLAIN));
    let mut second = vec![
        (
            [0, 0, 0],
            joint_block(),
            ball_end_toward(FaceDirection::NegativeX),
        ),
        (
            [0, 0, 1],
            joint_block(),
            ball_end_toward(FaceDirection::NegativeX),
        ),
    ];
    second.extend(stones(1, 4));
    second.extend((1..4).map(|x| ([x, 0, 1], common::docking::stone(), PLAIN)));
    make(&mut scene, pose(20.0, 230.0, 20.0), false, first);
    make(&mut scene, pose(27.0, 230.0, 20.4), false, second);
    until_coupled(&mut scene, &report, 600);
    scene.ticks(100);
    assert_eq!(report().couplings, 1);
    assert_eq!(report().couplings_made, 1);
}

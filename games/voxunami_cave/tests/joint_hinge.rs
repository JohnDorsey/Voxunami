//! The dev hinge joint (`docs/decisions/joints-and-cars.md`, "The dev hinge joint"): two hinge ends whose lines are one line are coupled as a hinge, which lets the two cars bend about that line and
//! about nothing else; any other pair of ends (a ball end, or hinges whose lines are not one line) is a ball. Everything else is the ball joint's: the pull, the commands, the lock.

mod common;

use common::docking::{make, ships, stones};
use common::joints::{
    LEVEL, Report, a_player_and_cars, a_player_at_and_cars_posed, east_car, east_hinge_car,
    pairs_the_world_has_as_coupled, pose, spin_against_in_the_first,
    turn_of_the_second_in_the_first, two_cars, two_hinge_cars, until_coupled, weightless, west_car,
    west_hinge_car, world_of,
};
use common::{FLOOR, Player, Scene, say, ship_id, tick_and_follow};
use voxel_space::{BrickCoordinate, FaceDirection, GridId, GridPose, Orientation, WorldPosition};

const Y: usize = 1;
const Z: usize = 2;

/// Sets a car turning on all three axes at 0.4 radians a second.
fn start_turning(scene: &mut Scene, car: GridId) {
    scene.run(move |world| {
        world
            .set_grid_velocity(car, [0.0; 3], [0.4, 0.4, 0.4])
            .unwrap();
    });
}

/// Sets the second car turning (`start_turning`) and lets it turn for a second.
fn set_turning(scene: &mut Scene, player: &mut Player) {
    start_turning(scene, ship_id(1));
    tick_and_follow(scene, player, 20);
}

fn coupled_and_settled(mut cars: (Scene, Report, Player)) -> (Scene, Report, Player) {
    until_coupled(&mut cars.0, &mut cars.2, &cars.1, 1, 600);
    tick_and_follow(&mut cars.0, &mut cars.2, 10);
    cars
}

/// How much the second car's spin against the first is across the line of the first's hinge that is along this axis of the first (what the hinge forbids), and along it (what it lets).
/// In the first's own axes: the hinge turns the first about the world's axes as well, and takes its line with it.
fn spin_across_and_along(scene: &Scene, line: usize) -> (f64, f64) {
    spin_across_and_along_of(scene, ship_id(0), ship_id(1), line)
}

fn spin_across_and_along_of(
    scene: &Scene,
    first: GridId,
    second: GridId,
    line: usize,
) -> (f64, f64) {
    let spin = spin_against_in_the_first(scene, first, second);
    let (one, other) = ((line + 1) % 3, (line + 2) % 3);
    (libm::hypot(spin[one], spin[other]), spin[line])
}

#[test]
fn two_hinge_ends_with_their_lines_on_one_line_are_coupled_and_the_pair_bends_about_that_line_and_nothing_else()
 {
    let (mut scene, report, mut player) = coupled_and_settled(two_hinge_cars(
        FaceDirection::PositiveZ,
        FaceDirection::PositiveZ,
    ));
    assert_eq!((report().couplings, report().hinges), (1, 1));
    assert_eq!(
        pairs_the_world_has_as_coupled(&mut scene),
        [(ship_id(0), ship_id(1))].into_iter().collect()
    );
    // Coupled, the cars lie as they were put: the pivots are paired line end to line end, so the second is not twisted half round about the bars to meet crosswise pivots.
    let at_rest = turn_of_the_second_in_the_first(&scene, ship_id(0), ship_id(1));
    for row in 0..3 {
        for column in 0..3 {
            let identity = if row == column { 1.0 } else { 0.0 };
            assert!(
                (at_rest[row][column] - identity).abs() < 0.01,
                "the second car is turned against the first: {at_rest:?}"
            );
        }
    }
    set_turning(&mut scene, &mut player);
    let (across, along) = spin_across_and_along(&scene, Z);
    assert!(
        across < 0.03,
        "the cars turn against each other by {across} across the line"
    );
    assert!(
        along.abs() > 0.05,
        "the hinge turns about its line ({along})"
    );
    // The turn of the second car in the first's axes is a turn about z: nothing joins z to x or to y, and the pair has bent.
    let turn = turn_of_the_second_in_the_first(&scene, ship_id(0), ship_id(1));
    for (row, column) in [(0, 2), (1, 2), (2, 0), (2, 1)] {
        assert!(
            turn[row][column].abs() < 0.01,
            "the second is turned off its line: {turn:?}"
        );
    }
    assert!(turn[1][0].abs() > 0.05, "the pair has not bent: {turn:?}");
}

#[test]
fn the_same_cars_with_ball_ends_turn_against_each_other_about_every_axis() {
    // The control of the test above: the cars, the push and the time are the same, and only the ends are balls.
    let (mut scene, report, mut player) = {
        let mut cars = two_cars();
        until_coupled(&mut cars.0, &mut cars.2, &cars.1, 1, 600);
        tick_and_follow(&mut cars.0, &mut cars.2, 10);
        cars
    };
    assert_eq!((report().couplings, report().hinges), (1, 0));
    set_turning(&mut scene, &mut player);
    let (across, _) = spin_across_and_along(&scene, Z);
    assert!(across > 0.1, "a ball turns every way: {across}");
}

#[test]
fn a_hinge_whose_line_is_along_another_axis_bends_about_that_axis() {
    // (The line is along y: the cars bend about y, as `two_cars_swinging` of the ball swings them.)
    let (mut scene, _report, mut player) = coupled_and_settled(two_hinge_cars(
        FaceDirection::PositiveY,
        FaceDirection::NegativeY,
    ));
    set_turning(&mut scene, &mut player);
    let (across, along) = spin_across_and_along(&scene, Y);
    assert!(across < 0.03, "across the line: {across}");
    assert!(along.abs() > 0.05, "{along}");
}

#[test]
fn two_hinges_whose_lines_cross_are_a_ball_and_so_is_a_hinge_with_a_ball() {
    for (name, mut cars) in [
        (
            "lines along z and along y",
            two_hinge_cars(FaceDirection::PositiveZ, FaceDirection::PositiveY),
        ),
        (
            "a hinge and a ball",
            a_player_and_cars(
                22.0,
                vec![
                    (20.0, east_hinge_car(3, FaceDirection::PositiveZ)),
                    (27.0, west_car(3)),
                ],
            ),
        ),
        (
            "a ball and a hinge",
            a_player_and_cars(
                22.0,
                vec![
                    (20.0, east_car(3)),
                    (27.0, west_hinge_car(3, FaceDirection::PositiveZ)),
                ],
            ),
        ),
    ] {
        until_coupled(&mut cars.0, &mut cars.2, &cars.1, 1, 600);
        tick_and_follow(&mut cars.0, &mut cars.2, 10);
        assert_eq!((cars.1)().hinges, 0, "{name}");
        set_turning(&mut cars.0, &mut cars.2);
        let (across, _) = spin_across_and_along(&cars.0, Z);
        assert!(
            across > 0.1,
            "{name}: coupled as a ball, and so free to turn about z: {across}"
        );
    }
}

/// A turn about x (the way the bars run) by this many degrees.
fn about_the_bars(degrees: f64) -> Orientation {
    let half = (degrees / 2.0).to_radians();
    Orientation::from_components([libm::sin(half), 0.0, 0.0, libm::cos(half)])
        .expect("a turn about x is an orientation")
}

fn two_hinge_cars_one_turned_about_the_bars(degrees: f64) -> (Scene, Report, Player) {
    let at = |x: f64, orientation: Orientation| {
        GridPose::new(
            WorldPosition::new(BrickCoordinate::ORIGIN, [x, LEVEL, 20.0]),
            orientation,
        )
    };
    a_player_at_and_cars_posed(
        [22.0, FLOOR + 5.0, 30.0],
        vec![
            (
                at(20.0, Orientation::IDENTITY),
                east_hinge_car(3, FaceDirection::PositiveZ),
            ),
            (
                at(27.0, about_the_bars(degrees)),
                west_hinge_car(3, FaceDirection::PositiveZ),
            ),
        ],
    )
}

#[test]
fn hinges_whose_lines_are_ten_degrees_apart_or_less_are_a_hinge_and_farther_apart_they_are_a_ball()
{
    // (A turn about the bars turns the second's line off the first's by that angle.)
    let (mut near, report, mut player) =
        coupled_and_settled(two_hinge_cars_one_turned_about_the_bars(6.0));
    assert_eq!((report().couplings, report().hinges), (1, 1));
    set_turning(&mut near, &mut player);
    let (across, _) = spin_across_and_along(&near, Z);
    assert!(across < 0.05, "6 degrees off: a hinge ({across})");

    let (mut far, report, mut player) =
        coupled_and_settled(two_hinge_cars_one_turned_about_the_bars(14.0));
    assert_eq!((report().couplings, report().hinges), (1, 0));
    set_turning(&mut far, &mut player);
    let (across, _) = spin_across_and_along(&far, Z);
    assert!(across > 0.1, "14 degrees off: a ball ({across})");
}

#[test]
fn a_hinge_that_is_made_rigid_and_then_mobile_is_a_hinge_again() {
    let (mut scene, _report, mut player) = coupled_and_settled(two_hinge_cars(
        FaceDirection::PositiveZ,
        FaceDirection::PositiveZ,
    ));
    let reply = say(&mut scene, &mut player, "/joint immobilize 0");
    assert!(reply.contains("immobilized"), "{reply}");
    tick_and_follow(&mut scene, &mut player, 5);
    let reply = say(&mut scene, &mut player, "/joint mobilize");
    assert!(reply.contains("mobilized"), "{reply}");
    tick_and_follow(&mut scene, &mut player, 5);
    set_turning(&mut scene, &mut player);
    let (across, along) = spin_across_and_along(&scene, Z);
    assert!(across < 0.03, "a hinge again, not a ball ({across})");
    assert!(along.abs() > 0.05, "{along}");
}

/// Ticks (with the player following) until `done` or `limit` ticks have gone; whether it was done.
fn until(
    scene: &mut Scene,
    player: &mut Player,
    limit: usize,
    done: impl Fn(&Scene) -> bool,
) -> bool {
    for _ in 0..limit {
        if done(scene) {
            return true;
        }
        tick_and_follow(scene, player, 1);
    }
    done(scene)
}

#[test]
fn a_bent_hinge_is_locked_straight_into_one_ship_and_unlocked_it_is_two_cars_coupled_as_a_hinge_again()
 {
    let (mut scene, report, mut player) = coupled_and_settled(two_hinge_cars(
        FaceDirection::PositiveZ,
        FaceDirection::PositiveZ,
    ));
    // Bend it a little about its line, and ask for a lock: the cars are turned into line (about the line, which is all the hinge lets them do) and made one ship.
    scene.run(|world| {
        world
            .set_grid_velocity(ship_id(1), [0.0; 3], [0.0, 0.0, 0.4])
            .unwrap();
    });
    tick_and_follow(&mut scene, &mut player, 10);
    let turn = turn_of_the_second_in_the_first(&scene, ship_id(0), ship_id(1));
    assert!(turn[1][0].abs() > 0.1, "it is bent: {turn:?}");
    let reply = say(&mut scene, &mut player, "/joint lock");
    assert!(reply.contains("locking ships 0 and 1"), "{reply}");
    assert!(
        until(&mut scene, &mut player, 600, |scene| ships(scene).len()
            == 1),
        "the cars were not made one ship"
    );
    // (A few ticks, for the desk to forget the lock it was asked for: unlock takes back a lock that is asked before it cuts one that is made.)
    tick_and_follow(&mut scene, &mut player, 5);
    let reply = say(&mut scene, &mut player, "/joint unlock");
    assert!(reply.contains("unlocked"), "{reply}");
    assert!(
        until(&mut scene, &mut player, 300, |scene| ships(scene).len()
            == 2),
        "the ship was not cut in two"
    );
    until_coupled(&mut scene, &mut player, &report, 1, 300);
    tick_and_follow(&mut scene, &mut player, 10);
    let cars = ships(&scene);
    let (first, second) = (cars[0], cars[1]);
    start_turning(&mut scene, second);
    tick_and_follow(&mut scene, &mut player, 20);
    let (across, along) = spin_across_and_along_of(&scene, first, second, Z);
    assert!(across < 0.03, "coupled as a hinge again: {across}");
    assert!(along.abs() > 0.05, "{along}");
}

#[test]
fn a_hinge_is_made_again_as_a_hinge_by_the_bodies_when_the_world_is_saved_and_loaded() {
    let (scene, report, player) = coupled_and_settled(two_hinge_cars(
        FaceDirection::PositiveZ,
        FaceDirection::PositiveZ,
    ));
    assert_eq!((report().couplings, report().hinges), (1, 1));
    drop(player);
    let (mut scene, report) = scene.restarted_reporting();
    // (The cars are saved as a pair, and the bodies couple the ends that are together, whatever the cars' spin against each other: the joint is made again from the ends, and the
    // ends are hinges whose lines are one line.)
    scene.ticks(10);
    assert_eq!((report().couplings, report().hinges), (1, 1));
}

/// Moves cars by `by` voxels along x in one tick, as a command does (they are put there; nothing moves them there).
fn move_cars(scene: &mut Scene, ids: [GridId; 2], by: f64) {
    scene.run(move |world| {
        for id in ids {
            let mut moved = world.data().grids().get(id).unwrap().pose;
            moved.position = moved.position.translated([by, 0.0, 0.0]);
            world.set_grid_pose(id, moved).unwrap();
        }
    });
}

#[test]
fn a_hinge_that_the_edge_of_a_frame_runs_through_is_made_again_in_the_other_frame_as_a_hinge() {
    // As `joint_coupling.rs` has it for a ball: the edge of a frame between the two cars of a pair takes the pair whole to another frame, and the joint is made again as it was (not a coupling made).
    let (mut scene, report) = Scene::reporting();
    weightless(&mut scene);
    make(&mut scene, pose(20.0, 1000.0, 20.0), true, stones(0, 1));
    make(
        &mut scene,
        pose(100.0, 1000.0, 20.0),
        false,
        east_hinge_car(3, FaceDirection::PositiveZ),
    );
    make(
        &mut scene,
        pose(107.0, 1000.0, 20.0),
        false,
        west_hinge_car(3, FaceDirection::PositiveZ),
    );
    for _ in 0..400 {
        if report().couplings == 1 {
            break;
        }
        scene.ticks(1);
    }
    scene.ticks(20);
    assert_eq!(report().couplings, 1);
    let east_now = world_of(&scene, ship_id(1), [0.0; 3])[0];
    move_cars(&mut scene, [ship_id(1), ship_id(2)], 32_125.0 - east_now);
    scene.ticks(3);
    assert_eq!(
        (report().couplings, report().hinges),
        (1, 1),
        "still coupled"
    );
    assert_eq!(
        report().couplings_made,
        1,
        "the joint was made again, not the coupling"
    );
    let moved = report().couplings_moved;
    assert!(moved >= 2, "{moved}");
    scene.ticks(60);
    // The joint that was made again is a hinge about the line it was.
    start_turning(&mut scene, ship_id(2));
    scene.ticks(20);
    let (across, along) = spin_across_and_along_of(&scene, ship_id(1), ship_id(2), Z);
    assert!(across < 0.03, "{across}");
    assert!(along.abs() > 0.05, "{along}");
}

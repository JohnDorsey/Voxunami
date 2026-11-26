//! The dev axle joint (`docs/decisions/joints-and-cars.md`, "The dev axle joint"): two axle ends whose lines (the length of their bars) are one line are coupled as an axle, about which the two cars
//! spin without limit and about nothing else. An axle couples only with an axle. Everything else is the ball joint's: the pull, the commands, the lock.

mod common;

use common::docking::ships;
use common::joints::{
    LEVEL, Report, a_player_and_cars, a_player_at_and_cars_posed, east_axle_car, east_car,
    east_hinge_car, minus, spin_against_in_the_first, turn_of_the_second_in_the_first,
    two_axle_cars, until_coupled, west_axle_car, west_car, west_hinge_car, world_of,
};
use common::{FLOOR, Player, Scene, length, say, ship_id, tick_and_follow};
use voxel_space::{BrickCoordinate, FaceDirection, GridId, GridPose, Orientation, WorldPosition};

/// The axle runs along x in both cars.
const ALONG: usize = 0;

fn coupled_and_settled(mut cars: (Scene, Report, Player)) -> (Scene, Report, Player) {
    until_coupled(&mut cars.0, &mut cars.2, &cars.1, 1, 600);
    tick_and_follow(&mut cars.0, &mut cars.2, 10);
    cars
}

fn set_spin(scene: &mut Scene, car: GridId, spin: [f64; 3]) {
    scene.run(move |world| {
        world.set_grid_velocity(car, [0.0; 3], spin).unwrap();
    });
}

/// How much the second car's spin against the first is across the axle (what it forbids), and along it (what it lets), in the first's own axes.
fn spin_across_and_along(scene: &Scene, first: GridId, second: GridId) -> (f64, f64) {
    let spin = spin_against_in_the_first(scene, first, second);
    (libm::hypot(spin[1], spin[2]), spin[ALONG])
}

/// The angle the second car has turned about the axle against the first, in radians, in `-π..π`, from the turn between them.
fn roll(scene: &Scene) -> f64 {
    let turn = turn_of_the_second_in_the_first(scene, ship_id(0), ship_id(1));
    libm::atan2(turn[2][1], turn[1][1])
}

/// How far the turn between the cars is from a turn about the axle: the greatest of the entries that join the axle's axis to the others.
fn off_the_axle(scene: &Scene, first: GridId, second: GridId) -> f64 {
    let turn = turn_of_the_second_in_the_first(scene, first, second);
    [(0, 1), (0, 2), (1, 0), (2, 0)]
        .into_iter()
        .map(|(row, column)| turn[row][column].abs())
        .fold(0.0, f64::max)
}

#[test]
fn two_axle_ends_are_coupled_as_an_axle_and_the_pair_turns_about_the_length_of_the_bars_and_nothing_else()
 {
    let (mut scene, report, mut player) = coupled_and_settled(two_axle_cars());
    assert_eq!(
        (report().couplings, report().hinges, report().axles),
        (1, 0, 1)
    );
    // Coupled, the cars lie as they were put.
    assert!(
        off_the_axle(&scene, ship_id(0), ship_id(1)) < 0.01 && roll(&scene).abs() < 0.01,
        "the second car is turned against the first"
    );
    set_spin(&mut scene, ship_id(1), [0.4, 0.4, 0.4]);
    tick_and_follow(&mut scene, &mut player, 20);
    let (across, along) = spin_across_and_along(&scene, ship_id(0), ship_id(1));
    assert!(
        across < 0.03,
        "the cars turn against each other across the axle by {across}"
    );
    assert!(along > 0.3, "they spin about it: {along}");
    let off = off_the_axle(&scene, ship_id(0), ship_id(1));
    assert!(off < 0.01, "the second is turned off the axle by {off}");
    assert!(roll(&scene).abs() > 0.2, "{}", roll(&scene));
}

#[test]
fn an_axle_turns_round_and_round_with_no_limit_and_loses_nothing_of_its_spin() {
    let (mut scene, report, mut player) = coupled_and_settled(two_axle_cars());
    set_spin(&mut scene, ship_id(1), [1.0, 0.0, 0.0]);
    tick_and_follow(&mut scene, &mut player, 1);
    let (_, at_first) = spin_across_and_along(&scene, ship_id(0), ship_id(1));
    let (mut turned, mut last) = (0.0, roll(&scene));
    for _ in 0..400 {
        tick_and_follow(&mut scene, &mut player, 1);
        let now = roll(&scene);
        let mut step = now - last;
        if step < -std::f64::consts::PI {
            step += std::f64::consts::TAU;
        }
        assert!(step > 0.0, "it turns one way all the time: {step}");
        turned += step;
        last = now;
    }
    assert!(
        turned > 3.0 * std::f64::consts::TAU,
        "turned {turned} radians in twenty seconds"
    );
    let (across, along) = spin_across_and_along(&scene, ship_id(0), ship_id(1));
    assert!(
        (along - at_first).abs() < 0.01,
        "the spin was {at_first} and is {along}"
    );
    assert!(across < 0.01, "{across}");
    assert_eq!((report().couplings, report().axles), (1, 1));
    // And its ends have stayed together.
    let apart = length(minus(
        world_of(&scene, ship_id(0), [4.0, 0.5, 0.5]),
        world_of(&scene, ship_id(1), [0.0, 0.5, 0.5]),
    ));
    assert!(apart < 0.01, "{apart}");
}

/// Two axle cars whose second is turned by `turn` as it is made.
fn two_axle_cars_the_second_turned(turn: Orientation) -> (Scene, Report, Player) {
    let at = |x: f64, orientation: Orientation| {
        GridPose::new(
            WorldPosition::new(BrickCoordinate::ORIGIN, [x, LEVEL, 20.0]),
            orientation,
        )
    };
    a_player_at_and_cars_posed(
        [22.0, FLOOR + 5.0, 30.0],
        vec![
            (at(20.0, Orientation::IDENTITY), east_axle_car(3)),
            (at(27.0, turn), west_axle_car(3)),
        ],
    )
}

#[test]
fn axles_couple_however_the_cars_are_rolled_about_them_and_not_when_they_are_tilted_off_each_others_line()
 {
    let half = |degrees: f64| (degrees / 2.0).to_radians();
    // Rolled about the bars by 45 degrees (no hinge or ball cares; a quarter turn is not asked of it).
    let rolled =
        Orientation::from_components([libm::sin(half(45.0)), 0.0, 0.0, libm::cos(half(45.0))])
            .unwrap();
    let (mut scene, report, mut player) = two_axle_cars_the_second_turned(rolled);
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    assert_eq!(report().axles, 1);
    // Tilted about y by 14 degrees, the lines of the two are not one line.
    let tilted =
        Orientation::from_components([0.0, libm::sin(half(14.0)), 0.0, libm::cos(half(14.0))])
            .unwrap();
    let (mut scene, report, mut player) = two_axle_cars_the_second_turned(tilted);
    tick_and_follow(&mut scene, &mut player, 300);
    assert_eq!((report().couplings, report().couplings_made), (0, 0));
}

#[test]
fn an_axle_does_not_pull_or_couple_with_a_ball_or_a_hinge() {
    for (name, east, west) in [
        ("an axle and a ball", east_axle_car(3), west_car(3)),
        ("a ball and an axle", east_car(3), west_axle_car(3)),
        (
            "an axle and a hinge",
            east_axle_car(3),
            west_hinge_car(3, FaceDirection::PositiveZ),
        ),
        (
            "a hinge and an axle",
            east_hinge_car(3, FaceDirection::PositiveZ),
            west_axle_car(3),
        ),
    ] {
        let (mut scene, report, mut player) =
            a_player_and_cars(22.0, vec![(20.0, east), (27.0, west)]);
        let apart = |scene: &Scene| {
            length(minus(
                world_of(scene, ship_id(0), [4.0, 0.5, 0.5]),
                world_of(scene, ship_id(1), [0.0, 0.5, 0.5]),
            ))
        };
        let before = apart(&scene);
        tick_and_follow(&mut scene, &mut player, 200);
        assert_eq!(report().couplings_made, 0, "{name}");
        assert!((apart(&scene) - before).abs() < 1e-6, "{name}");
    }
}

#[test]
fn an_axle_that_is_made_rigid_holds_its_roll_and_made_mobile_spins_again() {
    let (mut scene, report, mut player) = coupled_and_settled(two_axle_cars());
    let reply = say(&mut scene, &mut player, "/joint immobilize 0");
    assert!(reply.contains("immobilized"), "{reply}");
    tick_and_follow(&mut scene, &mut player, 3);
    let held = roll(&scene);
    set_spin(&mut scene, ship_id(1), [1.0, 0.0, 0.0]);
    tick_and_follow(&mut scene, &mut player, 40);
    let (_, along) = spin_across_and_along(&scene, ship_id(0), ship_id(1));
    assert!(along.abs() < 0.01, "a rigid axle does not turn: {along}");
    assert!((roll(&scene) - held).abs() < 0.01);
    let reply = say(&mut scene, &mut player, "/joint mobilize");
    assert!(reply.contains("mobilized"), "{reply}");
    tick_and_follow(&mut scene, &mut player, 5);
    // (The spin the second was given while the pair was rigid is both cars' now: the first is stopped, so that the spin given next is the one between them.)
    set_spin(&mut scene, ship_id(0), [0.0; 3]);
    set_spin(&mut scene, ship_id(1), [0.4, 0.4, 0.4]);
    tick_and_follow(&mut scene, &mut player, 20);
    let (across, along) = spin_across_and_along(&scene, ship_id(0), ship_id(1));
    assert!(across < 0.03, "an axle again, not a ball: {across}");
    assert!(along.abs() > 0.1, "{along}");
    assert_eq!(report().axles, 1);
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
fn a_rolled_axle_is_locked_at_the_nearest_quarter_turn_into_one_ship_and_unlocked_it_is_an_axle_again()
 {
    let (mut scene, report, mut player) = coupled_and_settled(two_axle_cars());
    set_spin(&mut scene, ship_id(1), [1.0, 0.0, 0.0]);
    tick_and_follow(&mut scene, &mut player, 22);
    let rolled = roll(&scene);
    assert!(
        rolled > 0.9 && rolled < 1.4,
        "rolled past an eighth of a turn: {rolled}"
    );
    set_spin(&mut scene, ship_id(1), [0.0; 3]);
    set_spin(&mut scene, ship_id(0), [0.0; 3]);
    tick_and_follow(&mut scene, &mut player, 2);
    let reply = say(&mut scene, &mut player, "/joint lock");
    assert!(reply.contains("locking ships 0 and 1"), "{reply}");
    assert!(
        until(&mut scene, &mut player, 600, |scene| ships(scene).len()
            == 1),
        "the cars were not made one ship"
    );
    // (A few ticks, for the desk to forget the lock it was asked for.)
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
    assert_eq!(report().axles, 1);
    let cars = ships(&scene);
    set_spin(&mut scene, cars[1], [0.4, 0.4, 0.4]);
    tick_and_follow(&mut scene, &mut player, 20);
    let (across, along) = spin_across_and_along(&scene, cars[0], cars[1]);
    assert!(across < 0.03, "coupled as an axle again: {across}");
    assert!(along.abs() > 0.1, "{along}");
}

#[test]
fn an_axle_is_made_again_as_an_axle_by_the_bodies_when_the_world_is_saved_and_loaded() {
    let (scene, report, player) = coupled_and_settled(two_axle_cars());
    assert_eq!((report().couplings, report().axles), (1, 1));
    drop(player);
    let (mut scene, report) = scene.restarted_reporting();
    scene.ticks(10);
    assert_eq!((report().couplings, report().axles), (1, 1));
}

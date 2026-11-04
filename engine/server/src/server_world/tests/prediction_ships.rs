//! The prediction of a ship's path through the spheres it will cross (`server_world/coasting/prediction.rs`, E16 phase B of `docs/orbits-and-thrust-plan.md`): how much is shown (what was
//! asked for, no less than the fewest rails and no more than the most changes), that it is the path the ship takes, that it is not advanced by the ship's motion, and that it comes back
//! the same from the ship's file.

use super::coasting_ships::{PULL, orbit_of, rail_of};
use super::exit_ships::{
    MOON_MASS, PLANET_MASS, STAR_MASS, a_ship_round, a_ship_that_goes_out_of_the_sphere,
    a_star_a_planet_and_a_moon, a_star_and_a_planet, a_star_with_ground_and_a_planet, run_from_to,
    start, the_sphere_of,
};
use super::*;
use crate::{PredictedRail, PredictionEnd, RailSettings};
use world_storage::rail::{decode_rail, encode_rail};

const NOW: fn() -> RailTime = || RailTime::of_tick(0, 20);

/// A world of a star, a planet and a moon, and a ship round the moon that goes out of the moon's sphere, then the planet's: three rails (the star has no sphere).
fn a_ship_through_two_spheres() -> (ServerWorld, GridId, GridId, GridId, GridId) {
    let (mut world, star, planet, moon) = a_star_a_planet_and_a_moon();
    let ship = a_ship_round(
        &mut world,
        moon,
        MOON_MASS,
        [0.0, 600.0, 0.0],
        [5.0e4, 0.0, 0.0],
    );
    (world, star, planet, moon, ship)
}

fn settings(most: u32, fewest: u32) -> RailSettings {
    RailSettings::default()
        .with_most_predicted_changes(most)
        .with_fewest_rails_shown(fewest)
        .unwrap()
}

#[test]
fn a_prediction_shows_the_rails_the_ship_goes_through_and_the_instant_it_leaves_each() {
    let (world, star, planet, moon, ship) = a_ship_through_two_spheres();
    let prediction = world.predicted_path(ship, NOW()).expect("on a rail");
    // The default shows three rails (the one it is on and two ahead), and the path has three: it ends in an orbit that never leaves its sphere.
    assert_eq!(prediction.rails.len(), 3);
    assert_eq!(prediction.end, PredictionEnd::ForEver);
    let [first, second, third] = &prediction.rails[..] else {
        panic!()
    };
    assert_eq!(first.orbit.around, Centre::Grid(moon));
    assert_eq!(second.orbit.around, Centre::Grid(planet));
    assert_eq!(third.orbit.around, Centre::Grid(star));
    assert_eq!(first.from, start());
    assert_eq!(second.from, first.until.unwrap());
    assert_eq!(third.from, second.until.unwrap());
    assert_eq!(third.until, None);
    // The ship goes out of the moon's sphere before the planet's.
    assert!(first.until.unwrap().compare(second.until.unwrap()).is_lt());
    // Each is one change further on the path, and as wide as a hand-over makes it.
    assert_eq!(
        [first, second, third].map(|rail| rail.orbit.changes),
        [0, 1, 2]
    );
    assert_eq!(
        [first, second, third].map(|rail| rail.orbit.elements.radius.precision().bits()),
        [128, 160, 192]
    );
    // The instants are those of the closed form, for the spheres the world has.
    assert_eq!(
        first.until,
        first
            .orbit
            .leaves_at(the_sphere_of(&world, moon), first.from)
            .unwrap()
    );
    assert_eq!(
        second.until,
        second
            .orbit
            .leaves_at(the_sphere_of(&world, planet), second.from)
            .unwrap()
    );
}

#[test]
fn a_prediction_is_the_path_the_ship_takes() {
    let (mut world, _, _, _, ship) = a_ship_through_two_spheres();
    let prediction = world.predicted_path(ship, NOW()).unwrap();
    // Nothing was done to the world by asking.
    assert_eq!(world.rail_work().hand_overs, 0);
    assert_eq!(
        rail_of(&world, ship).unwrap().orbit.unwrap(),
        prediction.rails[0].orbit
    );
    // Then the ship goes through the two spheres, in two ticks of a second: it is on the rails that were shown, to the bit.
    run_from_to(&mut world, 1, 1, 1);
    assert_eq!(world.rail_work().hand_overs, 2);
    let after = orbit_of(&world, ship);
    assert_eq!(after, prediction.rails[2].orbit);
}

#[test]
fn how_many_rails_are_shown_follows_the_fewest_and_the_most() {
    let (mut world, _, _, _, ship) = a_ship_through_two_spheres();
    // (Changes ahead: the fewest rails less the one it is on, or what was asked for if that is more; and no more than the most.)
    for (most, fewest, rails, end) in [
        (100, 3, 3, PredictionEnd::ForEver),
        (100, 2, 2, PredictionEnd::AsFarAsShown),
        (100, 1, 1, PredictionEnd::AsFarAsShown),
        // More is wanted than the path has: all of it is shown.
        (100, 9, 3, PredictionEnd::ForEver),
        // The most is a limit on what the fewest wants too.
        (1, 3, 2, PredictionEnd::AsFarAsShown),
        (0, 3, 1, PredictionEnd::AsFarAsShown),
    ] {
        world.set_rail_settings(settings(most, fewest));
        let prediction = world.predicted_path(ship, NOW()).unwrap();
        assert_eq!(
            prediction.rails.len(),
            rails,
            "most {most}, fewest {fewest}"
        );
        assert_eq!(prediction.end, end, "most {most}, fewest {fewest}");
    }
}

#[test]
fn continue_prediction_shows_one_more_change_until_there_are_no_more_or_the_most_is_reached() {
    let (mut world, _, _, _, ship) = a_ship_through_two_spheres();
    world.set_rail_settings(settings(100, 1));
    assert_eq!(world.predicted_path(ship, NOW()).unwrap().rails.len(), 1);
    assert_eq!(world.continue_prediction(ship, NOW()), Some(1));
    assert_eq!(world.predicted_path(ship, NOW()).unwrap().rails.len(), 2);
    assert_eq!(world.continue_prediction(ship, NOW()), Some(2));
    assert_eq!(world.predicted_path(ship, NOW()).unwrap().rails.len(), 3);
    // The path has no more changes: the button stops, and nothing is asked for beyond.
    let asked = orbit_of(&world, ship).predicted;
    assert_eq!(world.continue_prediction(ship, NOW()), Some(2));
    assert_eq!(orbit_of(&world, ship).predicted, asked);

    // A host that allows one change shown at once: the second press does nothing.
    let (mut world, _, _, _, ship) = a_ship_through_two_spheres();
    world.set_rail_settings(settings(1, 1));
    assert_eq!(world.continue_prediction(ship, NOW()), Some(1));
    assert_eq!(world.continue_prediction(ship, NOW()), Some(1));
    assert_eq!(world.predicted_path(ship, NOW()).unwrap().rails.len(), 2);
    assert_eq!(orbit_of(&world, ship).predicted, 1);
}

#[test]
fn the_motion_of_the_ship_through_the_spheres_does_not_advance_what_was_asked_for() {
    // Asked for two changes from the state; the ship goes through one: the end of what is shown is still where it was (two changes after the state, one ahead of the ship), and the
    // fewest rails (here one) does not move it.
    let (mut world, star, planet, moon) = a_star_a_planet_and_a_moon();
    let ship = a_ship_round(
        &mut world,
        moon,
        MOON_MASS,
        [600.0, 0.0, 0.0],
        [0.0, 1.5e3, 0.0],
    );
    // (Along the moon's own way at 1,500 voxels a second: out of the moon's sphere in a second, and out of the planet's some seconds after.)
    world.set_rail_settings(settings(100, 1));
    world.continue_prediction(ship, NOW());
    world.continue_prediction(ship, NOW());
    let before = world.predicted_path(ship, NOW()).unwrap();
    assert_eq!(orbit_of(&world, ship).predicted, 2);
    // Go on until the first hand-over has happened, and not the second.
    let first = before.rails[0].until.unwrap();
    let seconds = first.units() as f64 / f64::from(first.units_per_second());
    run_from_to(&mut world, 1, (seconds * 20.0).ceil() as u64 + 1, 20);
    assert_eq!(world.rail_work().hand_overs, 1);
    let after = world.predicted_path(ship, NOW()).unwrap();
    assert_eq!(orbit_of(&world, ship).around, Centre::Grid(planet));
    assert_eq!(orbit_of(&world, ship).predicted, 2);
    assert_eq!(orbit_of(&world, ship).changes, 1);
    // The same rails, the first gone: the one it is on and the one after.
    assert_eq!(after.rails, before.rails[1..]);
    assert_eq!(after.rails.last().unwrap().orbit.around, Centre::Grid(star));
}

/// A moonlet of a thousandth of the moon's weight round the moon, and a ship that goes out of its sphere, the moon's, the planet's: four rails and three changes.
const MOONLET_MASS: f64 = 6.0e15;

fn a_ship_through_three_spheres() -> (ServerWorld, GridId) {
    let (mut world, _, _, moon) = a_star_a_planet_and_a_moon();
    let mu = PULL * MOON_MASS;
    let orbit = Orbit::from_place(
        Centre::Grid(moon),
        [1_000.0, 0.0, 0.0],
        [0.0, libm::sqrt(mu / 1_000.0), 0.0],
        mu,
        start(),
    )
    .unwrap();
    let moonlet = planet_on(
        &mut world,
        1,
        MOONLET_MASS,
        Some(Rail {
            spin: None,
            orbit: Some(orbit),
        }),
    );
    world.apply_rails(20);
    let ship = a_ship_round(
        &mut world,
        moonlet,
        MOONLET_MASS,
        [20.0, 0.0, 0.0],
        [0.0, 3.0e3, 0.0],
    );
    (world, ship)
}

#[test]
fn what_was_asked_for_is_counted_from_the_state_and_goes_on_being_asked_as_the_ship_goes_through_the_spheres()
 {
    let (mut world, ship) = a_ship_through_three_spheres();
    world.set_rail_settings(settings(100, 1));
    assert_eq!(world.predicted_path(ship, NOW()).unwrap().rails.len(), 1);
    // Asked for two changes: three rails, of the four.
    assert_eq!(world.continue_prediction(ship, NOW()), Some(1));
    assert_eq!(world.continue_prediction(ship, NOW()), Some(2));
    let before = world.predicted_path(ship, NOW()).unwrap();
    assert_eq!(before.rails.len(), 3);
    assert_eq!(before.end, PredictionEnd::AsFarAsShown);
    // The ship goes through one sphere, and no more.
    let first = before.rails[0].until.unwrap();
    let seconds = first.units() as f64 / f64::from(first.units_per_second());
    let tick = (seconds * 1000.0).ceil() as u64 + 1;
    run_from_to(&mut world, 1, tick, 1000);
    assert_eq!(orbit_of(&world, ship).changes, 1);
    assert_eq!(world.rail_work().hand_overs, 1);
    // What is shown ends where it did: the rail it is on and the one after.
    let during = world.predicted_path(ship, NOW()).unwrap();
    assert_eq!(during.rails, before.rails[1..]);
    assert_eq!(during.end, PredictionEnd::AsFarAsShown);
    // One more asked for is one more after the end that was there: the star's rail.
    assert_eq!(world.continue_prediction(ship, NOW()), Some(2));
    assert_eq!(orbit_of(&world, ship).predicted, 3);
    let more = world.predicted_path(ship, NOW()).unwrap();
    assert_eq!(more.rails.len(), 3);
    assert_eq!(more.end, PredictionEnd::ForEver);
    // (The same rails, but for the number asked for that each carries.)
    let without_the_asking = |rails: &[PredictedRail]| -> Vec<PredictedRail> {
        rails
            .iter()
            .map(|rail| PredictedRail {
                orbit: Orbit {
                    predicted: 0,
                    ..rail.orbit.clone()
                },
                ..rail.clone()
            })
            .collect()
    };
    assert_eq!(
        without_the_asking(&more.rails[..2]),
        without_the_asking(&before.rails[1..])
    );
}

#[test]
fn when_the_end_of_what_was_asked_for_comes_down_to_the_fewest_rails_it_moves_on() {
    // Fewest 2 (one change ahead), nothing asked for: one ahead whatever the ship has been through.
    let (mut world, _, planet, moon) = a_star_a_planet_and_a_moon();
    let ship = a_ship_round(
        &mut world,
        moon,
        MOON_MASS,
        [600.0, 0.0, 0.0],
        [0.0, 1.5e3, 0.0],
    );
    world.set_rail_settings(settings(100, 2));
    assert_eq!(world.predicted_path(ship, NOW()).unwrap().rails.len(), 2);
    let first = world.predicted_path(ship, NOW()).unwrap().rails[0]
        .until
        .unwrap();
    let seconds = first.units() as f64 / f64::from(first.units_per_second());
    run_from_to(&mut world, 1, (seconds * 20.0).ceil() as u64 + 1, 20);
    assert_eq!(orbit_of(&world, ship).around, Centre::Grid(planet));
    // It is round the planet and the next rail (the star's) is shown: one ahead, as before, though nothing was asked for.
    let now = world.predicted_path(ship, NOW()).unwrap();
    assert_eq!(now.rails.len(), 2);
    assert_eq!(orbit_of(&world, ship).predicted, 0);
}

#[test]
fn a_prediction_comes_back_the_same_from_the_ships_file_whatever_has_happened_to_the_ship() {
    let (mut world, _, _, _, ship) = a_ship_through_two_spheres();
    world.set_rail_settings(settings(100, 1));
    world.continue_prediction(ship, NOW());
    let before = world.predicted_path(ship, NOW()).unwrap();
    // Saved and read again (the rail is what a ship's file keeps).
    let rail = rail_of(&world, ship).unwrap();
    let read = decode_rail(&encode_rail(&rail)).unwrap();
    assert_eq!(read, rail);
    world.set_grid_rail(ship, Some(read)).unwrap();
    assert_eq!(world.predicted_path(ship, NOW()).unwrap(), before);
}

#[test]
fn a_ship_that_is_not_on_a_rail_of_this_kind_has_no_prediction() {
    let (mut world, _, planet) = a_star_and_a_planet();
    assert!(world.predicted_path(planet, NOW()).is_none());
    assert!(world.continue_prediction(planet, NOW()).is_none());
    let ship = a_ship_that_goes_out_of_the_sphere(&mut world, planet);
    world.set_grid_rail(ship, None).unwrap();
    assert!(world.predicted_path(ship, NOW()).is_none());
    assert!(world.continue_prediction(ship, NOW()).is_none());
}

#[test]
fn a_prediction_says_where_the_ship_would_be_taken_off_its_rail() {
    // A star whose ground reaches farther than the planet is: the orbit round it is too low.
    let (mut world, _, planet) = a_star_with_ground_and_a_planet(400_000);
    let ship = a_ship_that_goes_out_of_the_sphere(&mut world, planet);
    let prediction = world.predicted_path(ship, NOW()).unwrap();
    assert_eq!(prediction.rails.len(), 1);
    assert!(matches!(
        prediction.end,
        PredictionEnd::Lost(NotCoasting::TooLow { .. })
    ));
    let _ = (PULL, STAR_MASS, PLANET_MASS);
}

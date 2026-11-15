//! A group of ships coupled by their joint ends that is **off a rail and alone** has a frame of its own (step 4c-4 of `docs/decisions/joints-and-cars.md`): the bodies make a frame
//! centred on its centre of mass and in free fall, with nothing in it but the group. What the joints do is the bodies' in it, as on a rail, and promise (a) of the joint plan holds
//! off a rail: **a mechanism does not change because of where the group is or how fast it goes**. The group is alone when nothing could touch it (`ServerWorld::group_is_alone`:
//! a level with no pull of its own, free ships, no planet's ground in the lookahead of the terrain, no other ship with joint ends or docking ports that pulls on its own), and **a
//! stranger near does not take it out of its frame** (step 4c-5-2: the contact is the ghost's, `contact_frame.rs`). (The terrain of a level is not in a frame of its own: the same
//! limit as a frame that goes with a planet.)

mod common;

use common::chains::{
    Setting, TICKS_PER_SECOND, Vec3, a_chain, a_chain_in_orbit, a_swing, centre_of, has_swung,
    how_the_chain_turns, how_the_chain_turns_after, mass_of, minus, most_apart, origins_of,
    pivot_gap, set_going,
};
use common::docking::{make_all_going, stones};
use common::{Scene, circular_speed, length, pose_at, ship_id, ship_voxels};

#[test]
fn what_a_mechanism_does_does_not_depend_on_where_an_alone_group_is_or_how_fast_it_goes_off_a_rail_either()
 {
    // The same chain, swung the same, at 70 and at 140 kilometres, free: alone, it is in a frame of its own with nothing but it in it, so the facing and the turn of each car are
    // those of the chain on a rail, to the ticks before it was alone. (In the frame everything is in they differ by millions of times as much: `group_on_a_rail.rs`.)
    let near = how_the_chain_turns(70_016.0, Setting::Alone);
    let far = how_the_chain_turns(140_000.0, Setting::Alone);
    let apart = most_apart(&near, &far);
    assert!(apart < 1e-7, "alone, the two differ by {apart}");
    // (They did swing.)
    assert!(has_swung(&near));
}

#[test]
fn an_alone_chain_off_a_rail_swings_as_the_same_chain_does_on_one() {
    // For the first hundred ticks: the alone chain is put on a rail by the world after a second of quiet (tick 19), and the hand-over from its frame to the rail's changes nothing of
    // the swing (eight digits, through it). Later the swing turns back, which makes the rounding of the ticks before it large, and the two part: see `how_the_chain_turns_after`.
    let off = how_the_chain_turns_after(70_016.0, Setting::Alone, 100);
    let on = how_the_chain_turns_after(70_016.0, Setting::OnARail, 100);
    let apart = most_apart(&off, &on);
    assert!(apart < 1e-7, "off a rail and on one they differ by {apart}");
}

#[test]
fn an_alone_chain_goes_round_the_planet_on_its_orbit_in_its_frame_of_its_own() {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let distance = 70_016.0;
    let speed = circular_speed(&scene, 0, distance);
    let ships = a_chain_in_orbit(&mut scene, a_chain(), distance, speed);
    let planet = ship_voxels(&scene, 0);
    let radius = |scene: &Scene| length(minus(centre_of(scene, &ships).0, ship_voxels(scene, 0)));
    let begin = radius(&scene);
    let mut most = 0.0f64;
    for _ in 0..400 {
        scene.ticks(1);
        most = most.max((radius(&scene) - begin).abs());
    }
    // A circle: the centre of mass keeps its distance from the planet (the steps of the bodies make a circle of a few centimetres' wobble, not a drift), twenty seconds on.
    assert!(
        most < 1.0,
        "the centre of mass was {most} voxels off its circle at the worst"
    );
    // It has gone on round: twenty seconds at 761 a second is 15 kilometres of its path.
    let now = minus(centre_of(&scene, &ships).0, planet);
    assert!(now[2].abs() > 10_000.0, "it went round: {now:?}");
    assert_eq!(report().couplings, 2, "and it is coupled still");
    assert_eq!(report().frames, 1, "in one frame: its own");
}

#[test]
fn a_stranger_that_comes_near_does_not_take_the_chain_out_of_its_frame_and_it_stays_coupled() {
    // (Step 4c-5-2: a stranger near is the ghost's, and the chain keeps its frame and its bits: `contact_frame.rs`.)
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let distance = 70_016.0;
    let speed = circular_speed(&scene, 0, distance);
    let ships = a_chain_in_orbit(&mut scene, a_chain(), distance, speed);
    set_going(&mut scene, &ships, speed, a_swing());
    scene.ticks(5);
    assert_eq!(
        report().frames,
        1,
        "alone: the frame of its own, and nobody else"
    );
    // A stranger far off, going the same way: another frame, and the chain still has its own.
    make_all_going(
        &mut scene,
        vec![(pose_at(0.0, distance + 5_000.0, 0.0), stones(0, 1))],
        [0.0, 0.0, speed],
    );
    scene.ticks(5);
    assert_eq!(
        report().frames,
        2,
        "a stranger far away is in a frame of its own"
    );
    assert_eq!(report().couplings, 2);
    // It comes near, forty voxels from the end of the chain: the chain keeps its frame and the stranger has its own.
    let stranger = ship_id(4);
    // (Where the end of the chain is now: it goes at 761 a second, and the stranger is put by it.)
    let end = {
        let origin = origins_of(&scene, &ships)[2];
        [origin[0] + 5.0, origin[1], origin[2]]
    };
    let near = pose_at(end[0] + 40.0, end[1], end[2]);
    scene.run(move |world| {
        world.set_grid_pose(stranger, near).unwrap();
    });
    let mut worst_gap: f64 = 0.0;
    for _ in 0..40 {
        scene.ticks(1);
        worst_gap = worst_gap
            .max(pivot_gap(&scene, (ships[0], 7.0), ships[1]))
            .max(pivot_gap(&scene, (ships[1], 6.0), ships[2]));
    }
    assert_eq!(
        report().frames,
        2,
        "with a stranger near the chain keeps its frame, and the stranger has the other"
    );
    assert_eq!(report().couplings, 2, "and the chain is coupled still");
    assert!(worst_gap < 0.05, "and its joints held: {worst_gap} m");
    // It goes away again: nothing changed for the chain.
    let away = {
        let origin = origins_of(&scene, &ships)[2];
        pose_at(origin[0], origin[1] + 5_000.0, origin[2])
    };
    scene.run(move |world| {
        world.set_grid_pose(stranger, away).unwrap();
    });
    let mut worst_gap: f64 = 0.0;
    for _ in 0..40 {
        scene.ticks(1);
        worst_gap = worst_gap
            .max(pivot_gap(&scene, (ships[0], 7.0), ships[1]))
            .max(pivot_gap(&scene, (ships[1], 6.0), ships[2]));
    }
    assert_eq!(report().frames, 2);
    assert_eq!(report().couplings, 2);
    assert!(worst_gap < 0.05, "the joints held: {worst_gap} m");
}

/// The speed of the centre of mass of the chain `ticks` ticks after it is set swinging, with a force of `force` voxels a second a second on the middle of its first car in each of
/// them, if there is one.
fn speed_of_the_chain_after(ticks: usize, push: Option<Vec3>) -> (Vec3, f64) {
    let mut scene = Scene::in_a_new_world();
    let distance = 70_016.0;
    let speed = circular_speed(&scene, 0, distance);
    let ships = a_chain_in_orbit(&mut scene, a_chain(), distance, speed);
    set_going(&mut scene, &ships, speed, a_swing());
    let mass = mass_of(&scene, &ships);
    let first = ships[0];
    for _ in 0..ticks {
        if let Some(acceleration) = push {
            scene.run(move |world| {
                world
                    .apply_grid_force(first, acceleration.map(|part| part * mass), None)
                    .unwrap();
            });
        } else {
            scene.ticks(1);
        }
    }
    (centre_of(&scene, &ships).1, mass)
}

#[test]
fn a_force_on_a_car_of_an_alone_chain_gives_the_group_the_push_over_its_mass_whatever_the_joints_do()
 {
    // Half a metre a second a second on the whole chain's mass, for ten ticks (half a second), on one car of a swinging chain: the centre of mass goes 0.25 faster than it would
    // have, along the push, and only that: the joints between the cars are the bodies' and push nothing on the whole.
    let ticks = 10;
    let acceleration = [0.0, 0.5, 0.0];
    let (without, _) = speed_of_the_chain_after(ticks, None);
    let (with, _) = speed_of_the_chain_after(ticks, Some(acceleration));
    let gained = minus(with, without);
    let expected = acceleration.map(|part| part * ticks as f64 / f64::from(TICKS_PER_SECOND));
    for axis in 0..3 {
        assert!(
            (gained[axis] - expected[axis]).abs() < 2e-4,
            "the centre of mass gained {gained:?} and the push says {expected:?}"
        );
    }
}

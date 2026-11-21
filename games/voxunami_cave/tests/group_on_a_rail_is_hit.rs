//! A group of ships coupled by their joint ends that rides a rail **stays on it while a stranger passes near, and is given the exact kick of a hit when one touches it** (the rail's
//! kick, step 4c-5-4 of `docs/decisions/joints-and-cars.md`; the owner's answers of 2026-11-09: "two railed ships that pass close both stay on their rails; their collision is worked
//! out in a temporary shared frame in which the rails are the authority, and only a real hit changes either", and "a collision with the outside of a ship gives its rail a kick").
//!
//! Before this, anything near a group took it off its rail (`NEAR_MARGIN`): a group beside a station never kept an exact path, and the hit of a stranger on a group that was on a
//! rail would have been lost as a push on the rail. Now what is near is the contact ghost's, as for a group that is alone (`contact_frame.rs`), and only a touch takes the rail:
//! the change of the group's momentum that the ghost's contacts gave it is the kick, the group goes on from where it was with the speed it had and the kick, off the rail, and is
//! put on one again when it is quiet.
//!
//! What stays as it was (and is pinned here): ends that pull on each other (a joint end that is to couple, a docking port) need the shared frame, so a group goes off its rail for them.

mod common;

use common::chains::{
    Vec3, a_chain, a_chain_in_orbit, a_swing, a_wall, car, centre_of, mass_of, most_apart,
    orientation_of, origins_of, pivot_gap, put_on_its_rail, spin_of,
};
use common::docking::make_all_going;
use common::{Scene, circular_speed, length, pose_at, ship_id};
use voxel_space::{GridId, Orientation};
use world_storage::rail::Rail;

const DISTANCE: f64 = 70_016.0;

/// How far from the east end of the chain along x a stranger begins, in voxels.
const FIRST_OFFSET: f64 = 150.0;

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

/// The facings and turns of the cars of a chain, as `most_apart` compares them.
fn turns(scene: &Scene, ships: &[GridId]) -> Vec<(Orientation, Vec3)> {
    ships
        .iter()
        .map(|ship| (orientation_of(scene, *ship), spin_of(scene, *ship)))
        .collect()
}

/// A chain of three cars in orbit at `DISTANCE`, swinging, on the rail of its centre of mass (made by hand, as `group_on_a_rail.rs` makes them), and what it needs to be watched.
fn a_chain_on_its_rail() -> (
    Scene,
    impl Fn() -> grid_bodies::BodiesReport,
    Vec<GridId>,
    f64,
    Rail,
) {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    let ships = a_chain_in_orbit(&mut scene, a_chain(), DISTANCE, speed);
    let rail = put_on_its_rail(&mut scene, &ships, speed, a_swing());
    scene.ticks(3);
    assert_eq!(rail_of(&scene, ships[0]), Some(rail.clone()), "it rides it");
    (scene, report, ships, speed, rail)
}

/// A wall (ship number 4) that begins `FIRST_OFFSET` voxels past the east end of the chain, `lateral` voxels off its axis along y, going at the chain's speed and `closing` metres a
/// second toward it along x.
fn send_a_wall(scene: &mut Scene, ships: &[GridId], speed: f64, closing: f64, lateral: f64) {
    let origin = origins_of(scene, ships)[2];
    make_all_going(
        scene,
        vec![(
            pose_at(
                origin[0] + 5.0 + FIRST_OFFSET,
                origin[1] + lateral,
                origin[2],
            ),
            a_wall(),
        )],
        [-closing, 0.0, speed],
    );
}

#[test]
fn a_stranger_that_passes_a_chain_on_a_rail_without_touching_leaves_it_on_the_rail_and_changes_nothing()
 {
    // The wall goes by 24 voxels off the chain's axis (its edge is 18 off), inside the room of the ghost and of the old rule, closing at 50 metres a second.
    let ticks = 120;
    let (mut alone, _, ships, _, _) = a_chain_on_its_rail();
    // (Making the wall takes a tick, so the chain alone has one too: the two are compared tick for tick.)
    alone.ticks(1);
    let mut baseline = Vec::new();
    for _ in 0..ticks {
        alone.ticks(1);
        baseline.extend(turns(&alone, &ships));
    }
    let (mut scene, report, ships, speed, rail) = a_chain_on_its_rail();
    send_a_wall(&mut scene, &ships, speed, 50.0, 24.0);
    let mut watched = Vec::new();
    let mut nearest = f64::INFINITY;
    for tick in 0..ticks {
        scene.ticks(1);
        assert_eq!(
            rail_of(&scene, ships[0]),
            Some(rail.clone()),
            "tick {tick}: the chain is on its rail, the wall going by"
        );
        watched.extend(turns(&scene, &ships));
        let wall = origins_of(&scene, &[ship_id(4)])[0];
        let end = origins_of(&scene, &ships)[2];
        nearest = nearest.min(length([0, 1, 2].map(|axis| wall[axis] - end[axis])));
    }
    // (The wall did go by near it, inside what the old rule called near, and the ghost had it.)
    assert!(
        nearest < 40.0,
        "the wall came to within {nearest} voxels of the last car's origin"
    );
    assert!(
        report().ghosts_made > 0,
        "the wall was near enough to be in a ghost"
    );
    let apart = most_apart(&watched, &baseline);
    assert!(
        apart < 1e-7,
        "with the wall going by the swing differs from the swing alone by {apart}"
    );
}

/// What a wall that hits a chain on its rail did.
struct Hit {
    /// The tick (from the first after the wall was sent) in which the rail came off, if it did.
    rail_off_at: Option<usize>,
    /// How far the wall's origin was from the east end of the chain, along x, in that tick.
    gap_then: f64,
    /// The momentum, along x, the chain and the wall gained, together, and the chain alone.
    momentum_gained: f64,
    momentum_of_the_chain: f64,
    /// How the chain's centre of mass went along x before and after.
    chain_change: f64,
    worst_gap: f64,
    coupled: bool,
    still_on_its_rail_before: bool,
}

fn a_wall_that_hits_a_chain_on_its_rail(closing: f64, ticks: usize) -> Hit {
    let (mut scene, report, ships, speed, rail) = a_chain_on_its_rail();
    send_a_wall(&mut scene, &ships, speed, closing, 0.0);
    scene.ticks(2);
    let wall = ship_id(4);
    let before = (centre_of(&scene, &ships).1, centre_of(&scene, &[wall]).1);
    let (chain_mass, wall_mass) = (mass_of(&scene, &ships), mass_of(&scene, &[wall]));
    let (mut rail_off_at, mut gap_then, mut worst_gap) = (None, f64::NAN, 0.0f64);
    let mut still_on_its_rail_before = true;
    for tick in 0..ticks {
        scene.ticks(1);
        let rail_now = rail_of(&scene, ships[0]);
        if rail_off_at.is_none() {
            if rail_now.is_none() {
                rail_off_at = Some(tick);
                let end = origins_of(&scene, &ships)[2][0] + 5.0;
                gap_then = origins_of(&scene, &[wall])[0][0] - end;
            } else {
                still_on_its_rail_before &= rail_now == Some(rail.clone());
            }
        }
        worst_gap = worst_gap
            .max(pivot_gap(&scene, (ships[0], 7.0), ships[1]))
            .max(pivot_gap(&scene, (ships[1], 6.0), ships[2]));
    }
    let after = (centre_of(&scene, &ships).1, centre_of(&scene, &[wall]).1);
    let (chain_change, wall_change) = (after.0[0] - before.0[0], after.1[0] - before.1[0]);
    Hit {
        rail_off_at,
        gap_then,
        momentum_gained: chain_mass * chain_change + wall_mass * wall_change,
        momentum_of_the_chain: chain_mass * chain_change,
        chain_change,
        worst_gap,
        coupled: report().couplings == 2,
        still_on_its_rail_before,
    }
}

#[test]
fn a_wall_that_hits_a_chain_on_its_rail_takes_it_off_the_rail_at_the_touch_with_the_momentum_of_the_hit()
 {
    for (closing, ticks) in [(50.0, 100), (150.0, 50)] {
        let hit = a_wall_that_hits_a_chain_on_its_rail(closing, ticks);
        let off = hit
            .rail_off_at
            .unwrap_or_else(|| panic!("closing at {closing}: the rail never came off"));
        assert!(
            hit.still_on_its_rail_before,
            "closing at {closing}: before the touch the chain was on the rail it had"
        );
        // The rail came off at the touch and not before it (the old rule took it off 64 voxels and a second of the closing speed before): the wall was at the chain's end.
        assert!(
            hit.gap_then.abs() < 3.0,
            "closing at {closing}: the rail came off in tick {off} with the wall {} voxels from the chain's end",
            hit.gap_then
        );
        // The chain went the way the wall went, and the two kept the momentum: what the chain gained is what the wall lost, to a hundredth.
        assert!(
            hit.chain_change < -0.1 * closing,
            "closing at {closing}: the chain's speed along it changed by {}",
            hit.chain_change
        );
        assert!(
            hit.momentum_gained.abs() < 0.01 * hit.momentum_of_the_chain.abs(),
            "closing at {closing}: together they gained {} of momentum and the chain {}",
            hit.momentum_gained,
            hit.momentum_of_the_chain
        );
        assert!(hit.coupled, "closing at {closing}: it is coupled still");
        assert!(
            hit.worst_gap < 0.5,
            "closing at {closing}: the joints' worst gap was {} voxels",
            hit.worst_gap
        );
    }
}

#[test]
fn a_chain_that_was_hit_is_put_on_a_new_rail_when_the_wall_has_gone_and_the_rail_is_the_chains() {
    let (mut scene, report, ships, speed, rail) = a_chain_on_its_rail();
    send_a_wall(&mut scene, &ships, speed, 100.0, 0.0);
    let mut off = None;
    for tick in 0..80 {
        scene.ticks(1);
        if rail_of(&scene, ships[0]).is_none() {
            off = Some(tick);
            break;
        }
    }
    assert!(
        off.is_some(),
        "the wall hit the chain and took it off its rail"
    );
    assert_eq!(report().rail_kicks, 1, "one kick");
    // The wall is taken far off (a program that puts a ship somewhere); the chain, left alone for a second, is put on a rail again.
    let wall = ship_id(4);
    scene.run(move |world| {
        let at = world.data().grids().get(wall).unwrap().pose.position;
        world
            .set_grid_pose(
                wall,
                voxel_space::GridPose::new(
                    at.translated([5_000.0, 0.0, 0.0]),
                    Orientation::IDENTITY,
                ),
            )
            .unwrap();
    });
    let mut on_again = None;
    for tick in 0..120 {
        scene.ticks(1);
        if let Some(new) = rail_of(&scene, ships[0]) {
            on_again = Some((tick, new));
            break;
        }
    }
    let (tick, new) = on_again.expect("the chain was put on a rail again");
    assert!(
        tick >= 19,
        "after a second of quiet and not before: tick {tick}"
    );
    assert_ne!(
        new, rail,
        "it is another rail than the one the hit took it from"
    );
    // It is the chain's own: the centre of mass is on it, and goes as it does.
    let (place, going) = centre_of(&scene, &ships);
    let (on_the_rail, rail_going) = common::chains::rail_point(&scene, &new);
    common::chains::close(place, on_the_rail, 1e-6, "the chain is on its new rail");
    common::chains::close(going, rail_going, 1e-6, "and goes as it does");
}

#[test]
fn a_hit_on_another_chain_far_off_does_not_kick_a_chain_on_its_rail() {
    // Chain B, 600 voxels along from chain A (which is on its rail), is hit by a wall: A is not near anything, and what the ghost of B does to B is none of A's.
    let ticks = 120;
    let (mut scene, report, ships, speed, rail) = a_chain_on_its_rail();
    let clock = |scene: &Scene| scene.rig.server.world().clock();
    let before_making = clock(&scene);
    let mut made = Vec::new();
    let mut at = 600.0;
    for (blocks, length) in a_chain() {
        made.push((pose_at(at, DISTANCE, 0.0), blocks));
        at += length;
    }
    made.push((pose_at(at + FIRST_OFFSET, DISTANCE, 0.0), a_wall()));
    make_all_going(&mut scene, made, [0.0, 0.0, speed]);
    // (The wall is the last of the four ships made: number 7; it goes at the speed of the chains, and 50 metres a second along x toward them.)
    scene.run(|world| {
        let wall = ship_id(7);
        let going = world.data().grids().get(wall).unwrap().linear_velocity;
        world
            .set_grid_velocity(wall, [going[0] - 50.0, going[1], going[2]], [0.0; 3])
            .unwrap();
    });
    // (Making chain B and the wall takes as many ticks as it takes (the ships are made a few bricks a tick) and setting the wall going one more: the chain alone is given the
    // same number, to have swung as far when the wall is let go.)
    let setting_up = clock(&scene) - before_making;
    let (mut alone, _, alone_ships, _, _) = a_chain_on_its_rail();
    alone.ticks(setting_up as usize);
    let mut baseline = Vec::new();
    for _ in 0..ticks {
        alone.ticks(1);
        baseline.extend(turns(&alone, &alone_ships));
    }
    let mut watched = Vec::new();
    for tick in 0..ticks {
        scene.ticks(1);
        assert_eq!(
            rail_of(&scene, ships[0]),
            Some(rail.clone()),
            "tick {tick}: chain A is on its rail while chain B is hit"
        );
        watched.extend(turns(&scene, &ships));
    }
    assert!(
        report().ghost_corrections > 0,
        "the wall did hit chain B (a ghost gave something a correction)"
    );
    assert_eq!(report().rail_kicks, 0, "and no group on a rail was kicked");
    let apart = most_apart(&watched, &baseline);
    assert!(apart < 1e-7, "chain A swings as it did alone to {apart}");
}

/// A plate of stone across the orbit, 7 voxels along x and 13 across y (up and down), one thick along z: a stranger that closes on the chain along the way it goes. (A small
/// one: the room of a ghost counts the reach of the hulls, and a big plate is near early enough for a ghost that is measured a tick wrong; it is tall enough for the few
/// voxels that a plate going a little faster than the chain rises over it in the seconds it takes.)
fn a_plate_across_the_orbit() -> Vec<common::docking::Placed> {
    (-3..=3)
        .flat_map(|x| {
            (-6..=6).map(move |y| ([x, y, 0], common::docking::stone(), common::docking::PLAIN))
        })
        .collect()
}

#[test]
fn a_stranger_that_closes_slowly_along_the_way_the_chain_goes_is_in_the_ghost_before_it_touches() {
    // A plate that comes up from behind a chain on its rail, 5 metres a second faster, the thin way: a ghost has to be made while it is still 40 voxels off, and the hit is the
    // ghost's. (A rail puts a group at its place of the end of the tick and a free ship is at its place of the start, 38 voxels apart at 761 voxels a second: that the near
    // search puts them back is pinned by `group_ships.rs` (`the_ships_near_a_group_on_a_rail_are_measured_with_the_group_a_tick_back`); **here it is not**: ran, a ghost made
    // from the places as the world has them is made when the plate is 5 voxels off, and the plate and the chain, a voxel thick each, are still a step apart. The mutant that
    // does not put the group back in the bodies' own call survived this test, and the only speeds Cave's world has are too slow to show it.)
    let (mut scene, report, ships, speed, _) = a_chain_on_its_rail();
    let origin = origins_of(&scene, &ships)[1];
    make_all_going(
        &mut scene,
        vec![(
            // (95 voxels behind, which is 57 from where the chain was at the start of the tick, and 22 below: a plate that goes faster than the chain rises by about that much, in
            // the 365 ticks it takes to come up with it.)
            pose_at(origin[0] + 3.0, origin[1] - 22.0, origin[2] - 95.0),
            a_plate_across_the_orbit(),
        )],
        [0.0, 0.0, speed + 5.0],
    );
    let mut off = None;
    for tick in 0..500 {
        scene.ticks(1);
        if rail_of(&scene, ships[0]).is_none() {
            off = Some(tick);
            break;
        }
    }
    assert!(
        off.is_some(),
        "the plate hit the chain and took it off its rail ({} ghosts made)",
        report().ghosts_made
    );
    assert_eq!(report().rail_kicks, 1);
}

#[test]
fn two_chains_on_rails_that_meet_both_stay_on_them_until_they_touch_and_both_are_kicked() {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    let closing = 50.0;
    let mut made = Vec::new();
    let mut at = 0.0;
    for (blocks, length) in a_chain() {
        made.push((pose_at(at, DISTANCE, 0.0), blocks));
        at += length;
    }
    for (blocks, length) in a_chain() {
        made.push((pose_at(at + FIRST_OFFSET, DISTANCE, 0.0), blocks));
        at += length;
    }
    make_all_going(&mut scene, made, [0.0, 0.0, speed]);
    scene.ticks(2);
    let (first, second): (Vec<GridId>, Vec<GridId>) = (
        (1..=3).map(ship_id).collect(),
        (4..=6).map(ship_id).collect(),
    );
    assert_eq!(report().couplings, 4, "both chains are coupled");
    let first_rail = put_on_its_rail(&mut scene, &first, speed, a_swing());
    let toward = |added: f64| -> Vec<(Vec3, Vec3)> {
        a_swing()
            .into_iter()
            .map(|(_, spin)| ([added, 0.0, 0.0], spin))
            .collect()
    };
    let second_rail = put_on_its_rail(&mut scene, &second, speed, toward(-closing));
    scene.ticks(2);
    assert_eq!(rail_of(&scene, first[0]), Some(first_rail.clone()));
    assert_eq!(rail_of(&scene, second[0]), Some(second_rail.clone()));
    let before = (centre_of(&scene, &first).1, centre_of(&scene, &second).1);
    let masses = (mass_of(&scene, &first), mass_of(&scene, &second));
    let mut off: (Option<usize>, Option<usize>) = (None, None);
    let mut both_on_until_the_first_is_off = true;
    for tick in 0..120 {
        scene.ticks(1);
        let (a, b) = (rail_of(&scene, first[0]), rail_of(&scene, second[0]));
        if off.0.is_none() && a.is_none() {
            off.0 = Some(tick);
        }
        if off.1.is_none() && b.is_none() {
            off.1 = Some(tick);
        }
        if off.0.is_none() && off.1.is_none() {
            both_on_until_the_first_is_off &=
                a == Some(first_rail.clone()) && b == Some(second_rail.clone());
        }
    }
    let (Some(first_off), Some(second_off)) = off else {
        panic!("the two chains met and the rails came off at {off:?}");
    };
    assert!(
        both_on_until_the_first_is_off,
        "both were on the rails they had until one came off (in tick {first_off} and {second_off})"
    );
    assert!(
        first_off.abs_diff(second_off) <= 1,
        "they came off in ticks {first_off} and {second_off}: a touch is a touch of both"
    );
    let after = (centre_of(&scene, &first).1, centre_of(&scene, &second).1);
    let (gain_first, gain_second) = (
        masses.0 * (after.0[0] - before.0[0]),
        masses.1 * (after.1[0] - before.1[0]),
    );
    assert!(
        gain_first < 0.0 && gain_second > 0.0,
        "the first chain gained {gain_first} of momentum along x, the second {gain_second}"
    );
    assert!(
        (gain_first + gain_second).abs() < 0.01 * gain_first.abs(),
        "together they gained {} of momentum, one of them {gain_first}",
        gain_first + gain_second
    );
    assert_eq!(report().couplings, 4, "and both are coupled still");
}

/// A chain whose last car has a joint end that looks east, free; and a car (the last number) whose west end looks at it, `gap` voxels beyond, at rest against it.
fn a_chain_and_a_car_that_could_couple(
    gap: f64,
) -> (
    Scene,
    impl Fn() -> grid_bodies::BodiesReport,
    Vec<GridId>,
    Rail,
) {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    let cars = vec![car(false, 6, true), car(true, 4, true), car(true, 5, true)];
    let mut at = 0.0;
    let mut made = Vec::new();
    for (blocks, length) in cars {
        made.push((pose_at(at, DISTANCE, 0.0), blocks));
        at += length;
    }
    let (blocks, _) = car(true, 3, false);
    made.push((pose_at(at + gap, DISTANCE, 0.0), blocks));
    make_all_going(&mut scene, made, [0.0, 0.0, speed]);
    scene.ticks(2);
    let ships: Vec<GridId> = (1..=3).map(ship_id).collect();
    assert_eq!(
        report().couplings,
        2,
        "the chain of three is coupled, and the car is not"
    );
    let rail = put_on_its_rail(&mut scene, &ships, speed, a_swing());
    scene.ticks(2);
    (scene, report, ships, rail)
}

#[test]
fn a_car_whose_joint_end_is_to_couple_to_the_chains_takes_the_chain_off_its_rail_and_is_coupled() {
    // Ends that pull on each other need the one frame (`alone.rs`): not a touch, and not the ghost's, so the rail goes for them, as it did for anything near.
    let (mut scene, report, ships, rail) = a_chain_and_a_car_that_could_couple(200.0);
    assert_eq!(
        rail_of(&scene, ships[0]),
        Some(rail),
        "it rides the rail, the car 200 voxels off"
    );
    // The car is brought to three voxels off the end of the chain, going as the chain does.
    let (place, going) = centre_of(&scene, &ships);
    let car_id = ship_id(4);
    let end = origins_of(&scene, &ships)[2][0] + 7.0;
    scene.run(move |world| {
        let at = voxel_space::WorldPosition::from_voxels([end + 3.0, place[1], place[2]]);
        world
            .set_grid_pose(
                car_id,
                voxel_space::GridPose::new(at, Orientation::IDENTITY),
            )
            .unwrap();
        world.set_grid_velocity(car_id, going, [0.0; 3]).unwrap();
    });
    let mut coupled_at = None;
    for tick in 0..200 {
        scene.ticks(1);
        if report().couplings == 3 {
            coupled_at = Some(tick);
            break;
        }
    }
    assert!(coupled_at.is_some(), "the car was coupled to the chain");
    assert_eq!(
        rail_of(&scene, ships[0]),
        None,
        "and the chain is off its rail"
    );
}

//! A group on a rail that is loaded with big cars and met by a stranger while the cars are still being made (the rail's kick of step 4c-5-4a and the ships made over ticks of
//! `docs/decisions/joints-and-cars.md`). Two rules about a group in a frame of its own are asked in `hold_groups_at_their_points`: a group that a stranger hit is kicked off its rail,
//! and a group that has a car being made is held where it is until it is made; which of the two came first was a choice made when `main` was merged in (2026-11-21).
//!
//! The scene is three cars of four bricks each (100 stones), made over four ticks when the world is loaded again (a tick makes four bricks of ships, three here: one for each car in
//! each round), a chain on a rail when the world was saved, and a slab of stone coming at the chain's west end at 200 voxels a second from `gap` voxels.
//!
//! **Measured 2026-11-22: a hit and a car being made were never in one tick.** A print in `hold_groups_at_their_points` (taken out again) saw a group with a car being made in 22
//! ticks of two series of runs, six scenes, and the ghost had corrected nothing in any of them: a ship that is being made "does not collide as a whole" (`ship_first_build.rs`), and the
//! window is the first ticks after the group is loaded, before a stranger that is not touching it can. A slab of another size (20 thick, 300 voxels a second, cars of six bricks,
//! made over four ticks) that came at it from 30, 45 and 60 voxels went through it in those ticks with no touch and no kick, the chain keeping its rail (a probe, not kept); the slabs
//! here, 25 and 60 voxels away at 200 voxels a second, hit it once the cars are made (tick 3) and kick it, once, with the pivots together to 1.2e-5 voxels. So **the order of the two
//! rules, which a merge settled, is not reached by this route**, and what is tested is what must hold in any order: the chain is whole and coupled after it all, and a hit after the
//! cars are made is a kick, once.

mod common;

use common::chains::{a_chain_in_orbit, car, origins_of, pivot_gap, put_on_its_rail};
use common::docking::{PLAIN, Placed, make_all_going, stone};
use common::{Scene, circular_speed, pose_at};

const DISTANCE: f64 = 70_016.0;

/// A slab 12 thick along x and 8 by 8, all in one brick: a stranger that is made whole in the first tick (thicker than the 10 voxels that it goes in a tick).
fn a_slab() -> Vec<Placed> {
    (0..12)
        .flat_map(|x| (0..8).flat_map(move |y| (0..8).map(move |z| ([x, y, z], stone(), PLAIN))))
        .collect()
}

/// What a run gave.
struct Run {
    /// The tick (after the world is loaded) in which the bodies had made every car.
    made_at: usize,
    kicks: u64,
    couplings: u64,
    /// Whether the chain rode a rail at the end.
    rides_at_the_end: bool,
    /// The most that two pivots of the chain were apart from the tick it was made.
    worst_pivot_gap: f64,
}

/// The chain on a rail and the slab `gap` voxels west of its end, saved and loaded again, and 14 ticks.
fn run(gap: f64) -> Run {
    let (mut scene, _) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    let cars = vec![
        car(false, 100, true),
        car(true, 100, true),
        car(true, 100, false),
    ];
    let ships = a_chain_in_orbit(&mut scene, cars, DISTANCE, speed);
    put_on_its_rail(&mut scene, &ships, speed, vec![([0.0; 3], [0.0; 3]); 3]);
    scene.ticks(5);
    let start = origins_of(&scene, &ships)[0];
    make_all_going(
        &mut scene,
        vec![(
            pose_at(start[0] - gap - 12.0, start[1] - 4.0, start[2] - 4.0),
            a_slab(),
        )],
        [200.0, 0.0, speed],
    );
    let (mut scene, report) = scene.restarted_reporting();
    let (mut made_at, mut worst_pivot_gap) = (None, 0.0f64);
    for tick in 0..14 {
        scene.ticks(1);
        if made_at.is_none() && report().ships_being_made == 0 {
            made_at = Some(tick);
        }
        if made_at.is_some() {
            worst_pivot_gap = worst_pivot_gap
                .max(pivot_gap(&scene, (ships[0], 101.0), ships[1]))
                .max(pivot_gap(&scene, (ships[1], 102.0), ships[2]));
        }
    }
    Run {
        made_at: made_at.expect("the cars were made"),
        kicks: report().rail_kicks,
        couplings: report().couplings,
        rides_at_the_end: scene
            .rig
            .server
            .world()
            .grids()
            .get(ships[0])
            .unwrap()
            .rail
            .is_some(),
        worst_pivot_gap,
    }
}

#[test]
fn a_chain_on_a_rail_that_is_loaded_with_cars_to_make_is_whole_and_coupled_when_a_stranger_comes_in_the_ticks_they_are_made_and_after_them()
 {
    // The cars take 4 ticks to make; a slab 25 voxels away comes while they are made, and one 60 voxels away just after.
    for gap in [25.0, 60.0] {
        let run = run(gap);
        assert!(
            (3..=5).contains(&run.made_at),
            "gap {gap}: the cars were made in tick {} (measured 4)",
            run.made_at
        );
        assert_eq!(
            run.couplings, 2,
            "gap {gap}: the chain is coupled when it is made, and still"
        );
        assert!(
            run.worst_pivot_gap < 1.0e-3,
            "gap {gap}: two pivots of the chain were {} voxels apart",
            run.worst_pivot_gap
        );
        if gap > 40.0 {
            // A hit after the cars are made is the kick, once: the chain is off its rail and the rail's momentum is the hit's (`group_on_a_rail_is_hit.rs`).
            assert_eq!(run.kicks, 1, "gap {gap}: one kick");
            assert!(
                !run.rides_at_the_end,
                "gap {gap}: the chain was hit and is off its rail"
            );
        }
    }
}

//! A chain of cars coupled by joints, alone and in free fall over a planet that goes round on its rail (the planet carries it), **is hit by a stranger that the planet carries in its own
//! frame as it would be if the planet stood still** (the contact ghost, 4c-5-2 and the rail's kick 4c-5-4 of `docs/decisions/joints-and-cars.md`).
//!
//! The world puts what a planet carries at the planet's place of the end of the tick when the tick begins (`apply_rails`), the chain's frame with it, the stranger's too; the bodies then
//! step the tick. The ghost is the pair stepped alone, from what both are at the start of it: a planet that goes round at 125.7 or at 1,257 voxels a second has gone 6 or 63 voxels
//! in a tick, and if the stranger's frame is put back that far and the chain's is not (the first version of the rail's kick did that: `Follow::at_the_start_of_the_tick` knew the
//! planet's own frame and not the frame of a group that the planet carries), the stranger goes through the chain and nothing is hit (ran, 2026-11-17: the chain moved 0.16 voxels in
//! 7 seconds, where the wall's momentum sends it 95 voxels; and not at all in the world with the planet at rest, which is why nothing else caught it).
//!
//! The scenes are the wanderer (`common::moving_planet`) standing still, going round at 125.7, and at 1,257 voxels a second, 1,200 voxels over its ground (so that the chain is alone
//! and has a frame of its own: `ground_cost_on_a_planet_on_a_rail.rs`), with a wall of stone 60 voxels from the chain's east end, sent at it along the chain at 30 voxels a second
//! against the ground.

mod common;

use common::chains::chain_over;
use common::moving_planet::{WANDERER, orbit_with_period, reporting_scene_with, wanderer_pack};
use common::{Scene, length, minus, ship_id, ship_over, ship_voxels, top_of_the_ground};
use voxel_space::GridId;

/// How high over the ground the chain is made (`ground_cost_on_a_planet_on_a_rail.rs`: 1,200 is in free fall, alone).
const HEIGHT: f64 = 1_200.0;

/// The wall begins this far past the chain's east end, in voxels, and closes at this many voxels a second.
const GAP: f64 = 60.0;
const CLOSING: f64 = 30.0;

/// The ticks watched: the wall has gone through the chain's length and well beyond.
const TICKS: usize = 150;

/// The periods of the wanderer's rail, in seconds, and what each is.
const PERIODS: [(&str, f64); 3] = [
    ("standing still", 1.0e15),
    ("going round at 125.7 voxels a second", 20_000.0),
    ("going round at 1,257 voxels a second", 2_000.0),
];

/// How far a world's paths may be from the world that stands still, in voxels, at any tick (ran: 4e-5 at the most, the rounding of a rail that is a curve and a tick that is a chord).
const ABOUT_THE_SAME: f64 = 1e-3;

/// What a scene did: where each car of the chain and the wall were in each tick, from the middle of the wanderer, and what the contact ghost did.
struct Hit {
    cars: Vec<Vec<[f64; 3]>>,
    wall: Vec<[f64; 3]>,
    ghost_corrections: u64,
}

fn place_of(scene: &Scene, ship: GridId) -> [f64; 3] {
    minus(
        ship_voxels(scene, ship.get() - GridId::FIRST_ALLOCATED),
        ship_voxels(scene, WANDERER),
    )
}

fn a_wall_that_hits_a_chain_over_the_wanderer_going_round_in(period: f64) -> Hit {
    let (mut scene, report) =
        reporting_scene_with(&wanderer_pack(&orbit_with_period(period), 340, 8.0));
    scene.ticks(1);
    let surface = top_of_the_ground(&mut scene, WANDERER, 3, 3, 9_900..10_100) as f64;
    let cars = chain_over(&mut scene, WANDERER, [0.0, surface + HEIGHT, 0.0], [0.0; 3]);
    let end = place_of(&scene, *cars.last().unwrap());
    // (2 voxels lower than the last car's origin and 3 to the side: the wall, 6 by 7, covers the cars' height and their width.)
    let wall = ship_id(ship_over(
        &mut scene,
        WANDERER,
        [end[0] + GAP, end[1] - 2.0, end[2] - 3.0],
        false,
        [-CLOSING, 0.0, 0.0],
        [1, 6, 7],
    ));
    let mut hit = Hit {
        cars: vec![Vec::new(); cars.len()],
        wall: Vec::new(),
        ghost_corrections: 0,
    };
    for _ in 0..TICKS {
        scene.ticks(1);
        for (path, car) in hit.cars.iter_mut().zip(&cars) {
            path.push(place_of(&scene, *car));
        }
        hit.wall.push(place_of(&scene, wall));
    }
    hit.ghost_corrections = report().ghost_corrections;
    hit
}

/// The greatest distance, in voxels, between where two paths put something at the same tick.
fn most_apart(first: &[[f64; 3]], second: &[[f64; 3]]) -> f64 {
    first
        .iter()
        .zip(second)
        .map(|(one, other)| length(minus(*one, *other)))
        .fold(0.0, f64::max)
}

#[test]
fn a_wall_that_a_planet_carries_hits_a_chain_in_free_fall_whatever_the_planet_does() {
    let hits: Vec<_> = PERIODS
        .iter()
        .map(|(name, period)| {
            (
                *name,
                a_wall_that_hits_a_chain_over_the_wanderer_going_round_in(*period),
            )
        })
        .collect();
    let (still_name, still) = &hits[0];
    // The hit is made (in the world that stands still, where nothing is in doubt): the ghost corrected the bodies, and the wall's momentum sent the first car along the chain, west,
    // by tens of voxels (95 in the 7.5 seconds), and not by the 0.16 voxels of a chain that the wall went through.
    assert!(
        still.ghost_corrections > 0,
        "the ghost did nothing {still_name}"
    );
    let sent = still.cars[0][0][0] - still.cars[0].last().unwrap()[0];
    assert!(
        sent > 50.0,
        "the wall sent the first car {sent} voxels west, {still_name}"
    );
    for (name, hit) in &hits[1..] {
        assert!(hit.ghost_corrections > 0, "the ghost did nothing {name}");
        for (index, (car, still_car)) in hit.cars.iter().zip(&still.cars).enumerate() {
            let apart = most_apart(car, still_car);
            assert!(
                apart < ABOUT_THE_SAME,
                "car {index} goes {apart} voxels from where it goes {still_name}, {name}"
            );
        }
        let apart = most_apart(&hit.wall, &still.wall);
        assert!(
            apart < ABOUT_THE_SAME,
            "the wall goes {apart} voxels from where it goes {still_name}, {name}"
        );
    }
}

//! A group of ships coupled by their joint ends beside a planet on a rail (step 4c-5-3 of `docs/decisions/joints-and-cars.md`, and the owner's word of 2026-11-16 that planets on
//! rails are the planets of the finished game): **what the world's carrying decides is the only thing the planet decides about the group's frame.** A planet on a rail carries what
//! is within 1.5 times the reach of its ground (the wanderer's 10,880 voxels: 16,320) and what it carried a quarter farther out; a group that it carries is in the frame of the
//! planet, and any other group that is off a rail and alone has a frame of its own, in free fall, where what a mechanism does is its own: the same bits at any distance.
//!
//! Before, the room to become alone counted the quarter beyond as carried, so a group made, or come to it from far, in the ring between 16,320 and 20,400 voxels from the wanderer
//! had no frame of its own, and a mechanism swung there differently (4.5e-4 against 4e-10 outside). The sphere of influence of the planet (83,000 voxels for the wanderer: where it
//! pulls more than the home planet) has never been part of the rule; a note of 2026-11-16 that said so was wrong (it took the ring for the sphere), and the first test here pins it.
//!
//! Every scene has the wanderer going round the home planet at 125.7 voxels a second (`common::moving_planet`), and the chain is made over its pole at rest against it.

mod common;

use common::chains::{a_chain, a_swing, has_swung, most_apart, orientation_of, spin_of};
use common::docking::make_all_going;
use common::moving_planet::{
    WANDERER, orbit_of_the_home_planet, reporting_scene_with, wanderer_pack,
};
use common::{Scene, pose_at, ship_id, ship_voxels};
use grid_bodies::BodiesReport;
use std::sync::{Arc, Mutex};
use voxel_space::{GridId, Orientation};

/// The reach of the ground of the wanderer is 340 bricks, 10,880 voxels, and it carries what is within 1.5 times that.
const CARRIED_TO: f64 = 16_320.0;

/// What it keeps carrying of what it carried: a quarter farther out.
const KEPT_TO: f64 = 20_400.0;

struct Chain {
    scene: Scene,
    report: Box<dyn Fn() -> BodiesReport>,
    ships: Vec<GridId>,
}

/// A chain of three cars made `distance` voxels over the pole of the wanderer (+y), going as the wanderer goes, with the ticks that couple it.
fn a_chain_beside_the_wanderer(distance: f64) -> Chain {
    let (mut scene, report) =
        reporting_scene_with(&wanderer_pack(&orbit_of_the_home_planet(None), 340, 8.0));
    scene.ticks(1);
    let wanderer = ship_voxels(&scene, WANDERER);
    let mut made = Vec::new();
    let mut along = 0.0;
    for (blocks, length) in a_chain() {
        made.push((
            pose_at(wanderer[0] + along, wanderer[1] + distance, wanderer[2]),
            blocks,
        ));
        along += length;
    }
    let speed = speed_of_the_wanderer(&scene);
    make_all_going(&mut scene, made, speed);
    scene.ticks(2);
    Chain {
        scene,
        report: Box::new(report),
        ships: (2..=4).map(ship_id).collect(),
    }
}

fn speed_of_the_wanderer(scene: &Scene) -> [f64; 3] {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(WANDERER))
        .expect("the wanderer is there")
        .linear_velocity
}

impl Chain {
    fn group_frames(&self) -> u64 {
        (self.report)().group_frames
    }

    /// Sets the cars swinging as `a_swing` says, on top of the speed of the wanderer, in the tick it runs.
    fn swing(&mut self) {
        let speed = speed_of_the_wanderer(&self.scene);
        let ships = self.ships.clone();
        self.scene.run(move |world| {
            for (ship, (added, spin)) in ships.iter().zip(a_swing()) {
                let velocity = [0, 1, 2].map(|axis| speed[axis] + added[axis]);
                world.set_grid_velocity(*ship, velocity, spin).unwrap();
            }
        });
    }

    /// The facing and the turn of each car.
    fn how_it_turns(&self) -> Vec<(Orientation, [f64; 3])> {
        self.ships
            .iter()
            .map(|ship| {
                (
                    orientation_of(&self.scene, *ship),
                    spin_of(&self.scene, *ship),
                )
            })
            .collect()
    }
}

/// How the chain turns `ticks` ticks after it is set swinging, `distance` from the wanderer.
fn how_the_chain_turns_beside_the_wanderer(
    distance: f64,
    ticks: usize,
) -> Vec<(Orientation, [f64; 3])> {
    let mut chain = a_chain_beside_the_wanderer(distance);
    chain.swing();
    chain.scene.ticks(ticks);
    chain.how_it_turns()
}

#[test]
fn a_chain_beside_a_planet_on_a_rail_has_a_frame_of_its_own_out_to_what_the_planet_carries_and_none_inside_it()
 {
    // (The premise: the wanderer carries what is within 16,320 voxels, found from the world, and its sphere of influence, where it pulls more than the home planet, is far
    // beyond the farthest of these distances: 83,000.) The chain is alone, with a frame of its own, at every distance past what the planet carries, the ring in which the world
    // keeps what it carried (16,320 to 20,400) and the 22,000 to 60,000 voxels that are in the planet's sphere of influence included. Inside what the planet carries it is in the
    // frame of the planet, which is no frame of its own.
    let mut asked = a_chain_beside_the_wanderer(30_000.0);
    let carried_to = Arc::new(Mutex::new(0.0));
    let slot = Arc::clone(&carried_to);
    asked.scene.run(move |world| {
        *slot.lock().unwrap() = world.influence_radius(ship_id(WANDERER)).unwrap_or(-1.0);
    });
    assert_eq!(*carried_to.lock().unwrap(), CARRIED_TO, "the premise");
    for distance in [
        60_000.0, 40_000.0, 30_000.0, 25_000.0, 22_000.0, 21_000.0, 20_500.0, 20_000.0, 18_000.0,
        16_500.0,
    ] {
        let mut chain = a_chain_beside_the_wanderer(distance);
        chain.scene.ticks(10);
        assert_eq!(
            chain.group_frames(),
            1,
            "a chain {distance} voxels from the wanderer, which carries to {CARRIED_TO} and keeps what it carried to {KEPT_TO}, has no frame of its own"
        );
    }
    for distance in [16_000.0, 14_000.0, 11_500.0] {
        let mut chain = a_chain_beside_the_wanderer(distance);
        chain.scene.ticks(10);
        assert_eq!(
            chain.group_frames(),
            0,
            "a chain {distance} voxels from the wanderer, inside what it carries, has a frame of its own: the planet moves it every tick and the frame would move it again"
        );
    }
}

#[test]
fn what_a_mechanism_does_is_the_same_in_the_ring_where_the_world_keeps_what_it_carried_as_far_out()
{
    // The same chain, swung the same way, 60,000 voxels from the wanderer (in its sphere of influence and far from what it carries) and at 18,000, 16,500 and 20,000: ran 4e-10
    // apart, and the whole of the difference is the rounding of the steps of the frames. Before, at 18,000 the chain was in the frame everything is in, 4.5e-4 apart.
    let far = how_the_chain_turns_beside_the_wanderer(60_000.0, 120);
    assert!(has_swung(&far), "(they did swing)");
    for distance in [30_000.0, 20_000.0, 18_000.0, 16_500.0] {
        let there = how_the_chain_turns_beside_the_wanderer(distance, 120);
        let apart = most_apart(&far, &there);
        assert!(
            apart < 1e-7,
            "the chain swung at 60,000 and at {distance} voxels from the wanderer differs by {apart}"
        );
    }
}

//! A group of ships coupled by their joint ends that is off a rail and alone **keeps its frame of its own over the ground of a planet, until it touches it** (step 4c-5-3 of
//! `docs/decisions/joints-and-cars.md`): the frame holds the planet as a kinematic body with the bricks of the ground that the group is about to reach, the ground comes at the
//! group at the speed they close at, the tick is cut into the steps that speed asks for, and the hit is the group's own world's. Before, the ground in the lookahead of the terrain
//! took a group out of its frame and into the one everything is in, and what a mechanism did changed at that moment, by rounding, though nothing had touched it. After the first
//! touch the group is in the frame everything is in as before: a group at rest on the ground is as exact there as a ship (in a frame that falls it creeps: the ground accelerates
//! against the frame, see `grid_bodies/src/plugin/alone.rs`).
//!
//! The scenes are a chain of three cars (`common::chains`) over the home planet at its pole, where the ground is 63,681 voxels from the middle (and slopes by half a voxel a voxel),
//! with nothing near it. The ground is in the way of a chain at rest from 67,375 voxels on (`ServerWorld::ground_is_in_the_way_of_group`), 3,700 voxels above the ground itself:
//! the planet's own reach is a sphere round its bricks.

mod common;

use common::chains::{
    Setting, a_chain, a_chain_in_orbit, centre_of, has_swung, how_the_chain_turns_going,
    most_apart, origins_of,
};
use common::docking::make_all_going;
use common::{Scene, length, pose_at, ship_id, top_of_the_ground};
use game_shell::GameDefinition;
use grid_bodies::BodiesReport;
use voxel_space::{GravitySettings, GridId};
use voxunami_cave::game::Cave;

/// Where the chain is made: 25 voxels outside the room in which the ground is in the way, at rest.
const START: f64 = 67_400.0;

/// How fast the chain goes down in the test of a hard hit, in voxels a second: the ground comes at the chain 20 voxels in a tick. (Ran: even at 1,500 the chain does not go through the
/// ground when the steps and the lookahead leave out the speed of the ground, which is thick; the steps are tested in `physics`, and the lookahead by when the bricks are made.)
const HARD: f64 = 400.0;

/// Where it is made: the room of the ground at that speed is twice 64 voxels and a quarter of a second of it, 328 voxels, past the sphere at 67,300 (a chain made nearer than that has
/// no frame of its own, and is in the frame everything is in, as the first version of this test was).
const START_HARD: f64 = 67_800.0;

/// The highest voxel of the ground under the chain (ran, `the_ground_under_the_chain_is_where_the_other_tests_say`).
const GROUND: f64 = 63_681.0;

/// A chain at rest over the home planet, with the pull of the planet made weaker if asked (a hundredth: the chain comes down in 6,000 ticks and meets the ground at 25 voxels a
/// second, where at the pull of the game it would meet it at 250).
struct Chain {
    scene: Scene,
    report: Box<dyn Fn() -> BodiesReport>,
    ships: Vec<GridId>,
}

/// Sets the pull of the planets to this many times what the game has.
fn set_the_pull_to(scene: &mut Scene, times: f64) {
    let game = Cave::gravity();
    scene.run(move |world| {
        world.set_gravity_settings(GravitySettings {
            pull_strength: game.pull_strength * times,
            ..game
        });
    });
}

fn a_chain_over_the_planet(weaker_pull: bool) -> Chain {
    a_chain_going_over_the_planet(weaker_pull, START, [0.0; 3])
}

/// The chain made `start` voxels from the middle of the planet going at `velocity` (all in one tick, and coupled by the bodies two ticks after).
fn a_chain_going_over_the_planet(weaker_pull: bool, start: f64, velocity: [f64; 3]) -> Chain {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    if weaker_pull {
        set_the_pull_to(&mut scene, 0.01);
    }
    let mut made = Vec::new();
    let mut at = 0.0;
    for (blocks, length) in a_chain() {
        made.push((pose_at(at, start, 0.0), blocks));
        at += length;
    }
    make_all_going(&mut scene, made, velocity);
    scene.ticks(2);
    Chain {
        scene,
        report: Box::new(report),
        ships: (1..=3).map(ship_id).collect(),
    }
}

impl Chain {
    /// How high the centre of mass of the chain is over the highest voxel of the ground under it, and how fast it goes.
    fn height_and_speed(&self) -> (f64, f64) {
        let (place, velocity) = centre_of(&self.scene, &self.ships);
        (length(place) - GROUND, length(velocity))
    }

    fn group_frames(&self) -> u64 {
        (self.report)().group_frames
    }
}

#[test]
fn a_chain_that_comes_down_to_the_ground_keeps_its_frame_until_it_touches_it_and_then_lies_on_it() {
    let mut chain = a_chain_over_the_planet(true);
    assert_eq!(chain.group_frames(), 1, "alone, far above the ground");
    // It falls 3,700 voxels in 6,000 ticks. Over the ground (the highest voxel under the chain is the GROUND, and the cars are a few voxels high) it has its frame of its own
    // every tick, though the ground has been in the way of it for the last 3,600 voxels. (Before this step it was in the frame everything is in from the first tick of that.)
    let mut ticks = 0;
    loop {
        let (height, _) = chain.height_and_speed();
        if height < 8.0 {
            break;
        }
        assert_eq!(
            chain.group_frames(),
            1,
            "at tick {ticks}, {height} voxels over the ground, the chain has no frame of its own"
        );
        chain.scene.ticks(1);
        ticks += 1;
        assert!(ticks < 9_000, "it did not come down: {height}");
    }
    // It lands, bounces a little and slides on the slope, and comes to rest, in the frame everything is in once it has touched. (The pull is the game's again from just before it
    // touches: at a hundredth of it the chain would bounce for thousands of ticks.)
    set_the_pull_to(&mut chain.scene, 1.0);
    chain.scene.ticks(1_200);
    let (height, speed) = chain.height_and_speed();
    assert!(
        (-6.0..1.0).contains(&height),
        "it lies {height} voxels over the highest voxel of the ground under it (the ground slopes by half a voxel a voxel under the chain, and the cars lie as they land: in the frame everything is in a chain that is put just over the ground lies at -2.99 or -3.04)"
    );
    assert!(speed < 0.05, "and it goes at {speed} voxels a second");
    let report = (chain.report)();
    assert_eq!(report.couplings, 2, "it is coupled still");
    assert_eq!(
        report.group_frames, 0,
        "it is in the frame everything is in"
    );
    assert!(
        report.terrain_bricks_made > 0,
        "the ground that it met was made for it"
    );
}

#[test]
fn a_chain_that_hits_the_ground_hard_does_not_go_through_it() {
    // It is made at 400 voxels a second down, at the pull that is a hundredth of the game's (so that it goes on at that speed), and meets the ground 4,100 voxels on, in 10 seconds:
    // alone, in its frame, from far above. The ground comes at it at that speed and cannot be seen from the chain's frame as anything but a body that is coming: the bricks are
    // made for it from the speed against the ground (the chain is at rest in its frame), and the tick is cut into the steps that it takes. It is stopped by the ground (at the
    // lowest by a few voxels: the cars hit it one after another), and does not go through. (Ran: it does not go through thick ground even with the steps and the lookahead as they
    // were, at 400 and at 1,500: the steps are tested in `physics` and the lookahead in the next test, and what this one tells is the place of the ground in the frame, which a mutant
    // that put it by the end of the tick of the world has go through.)
    let mut chain = a_chain_going_over_the_planet(true, START_HARD, [0.0, -HARD, 0.0]);
    assert_eq!(
        chain.group_frames(),
        1,
        "alone: made outside the room of the ground at that speed"
    );
    let mut lowest = f64::INFINITY;
    for _ in 0..900 {
        chain.scene.ticks(1);
        for origin in origins_of(&chain.scene, &chain.ships) {
            lowest = lowest.min(length(origin) - GROUND);
        }
    }
    assert!(
        lowest > -25.0,
        "a car of the chain went {lowest} voxels under the highest voxel of the ground under it"
    );
    // (It came down to the ground: it was not left far above it.)
    assert!(
        lowest < 10.0,
        "the chain came down to {lowest} voxels over the ground and no nearer"
    );
}

/// How many bricks of the ground the chain holds when it has come down to this many voxels over the highest voxel of the ground under it, falling at `speed` (the pull a hundredth of
/// the game's, so that it goes on at that speed), made 4,100 voxels over the ground. The count is read at the first tick that it is that low and still has its frame of its own.
fn ground_held_at(speed: f64, height: f64) -> u64 {
    let mut chain = a_chain_going_over_the_planet(true, START_HARD, [0.0, -speed, 0.0]);
    for _ in 0..5_000 {
        if chain.height_and_speed().0 < height {
            assert_eq!(chain.group_frames(), 1, "it has its frame of its own");
            return (chain.report)().group_ground_bricks;
        }
        chain.scene.ticks(1);
    }
    panic!("the chain did not come down to {height} voxels over the ground going at {speed}");
}

#[test]
fn the_ground_that_a_group_holds_grows_with_the_speed_it_closes_on_it() {
    // The lookahead of the terrain is 8 voxels and a quarter of a second of the speed against the ground, and a group is at rest in its frame: it is the speed of the ground
    // against it that counts and not its own. **Bricks of air are not made or held** (2026-11-20, `docs/ship-terrain-cost.md`: a cube that the generator says is air all through is
    // not looked into), so a chain 3,700 voxels over the ground holds nothing of it at any speed, and this test used to count bricks of sky (8, 22 and 117 at 0, 100 and 400 voxels a
    // second). What it counts now is the ground that it holds when it has come down to 40 voxels over it, the same height at each speed: the farther ahead it looks, the more of the
    // ground is in what it holds. (Ran: 10, 17, 17 and 27 at 50, 100, 200 and 400; with the speed in the frame, which is none, it would hold the same at all four.)
    let (slowly, fast) = (ground_held_at(50.0, 40.0), ground_held_at(400.0, 40.0));
    assert!(
        slowly > 0,
        "the ground was looked at, from 40 voxels over it, though slowly"
    );
    assert!(
        fast >= 2 * slowly,
        "the group held {slowly} bricks of the ground at 50 voxels a second and {fast} at 400"
    );
}

#[test]
fn what_a_mechanism_does_does_not_change_because_the_ground_comes_near() {
    // The same chain, swung the same way, at rest in the world 25 voxels outside the room in which the ground is in the way and falling 150 voxels in the 120 ticks that it is
    // watched (it is in the way of the ground for the last 125 of them, at the room to stay alone for the last 50, and touches nothing), and the same chain going round at
    // 70 kilometres: both are alone, with a frame of their own and nothing in it but them, so what each car does is the same to the digits of the rounding of the steps. (Before
    // this step the chain over the ground left its frame for the one everything is in at the room to stay alone, and the two differed by 5e-4, millions of times as much.)
    let near = how_the_chain_turns_going(START, 0.0, Setting::Alone, 120);
    let far = how_the_chain_turns_going(70_016.0, 0.0, Setting::Alone, 120);
    let apart = most_apart(&near, &far);
    assert!(
        apart < 1e-7,
        "over the ground and far from it, the two differ by {apart}"
    );
    assert!(has_swung(&near), "(they did swing)");
}

#[test]
fn a_chain_made_outside_the_room_of_the_ground_has_a_frame_and_one_made_inside_it_has_not() {
    // (The other side of the rule: a group that is made where the ground is in the way, with no frame, is in the frame everything is in, as before; one that has its frame is not
    // asked to leave it for the ground's being near.)
    let mut chain = a_chain_over_the_planet(false);
    chain.scene.ticks(10);
    assert_eq!(chain.group_frames(), 1, "made 25 voxels outside the room");
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let _ = a_chain_in_orbit(&mut scene, a_chain(), 67_330.0, 0.0);
    scene.ticks(10);
    assert_eq!(
        report().group_frames,
        0,
        "made 45 voxels inside the room of the ground: in the frame everything is in"
    );
}

#[test]
fn the_ground_under_the_chain_is_where_the_other_tests_say() {
    let (mut scene, _) = Scene::reporting_in_a_new_world();
    scene.ticks(2);
    let highest = [0, 4, 8, 12, 16]
        .into_iter()
        .flat_map(|x| [0, 3].map(move |z| (x, z)))
        .map(|(x, z)| top_of_the_ground(&mut scene, 0, x, z, 63_000..67_000))
        .max()
        .expect("columns");
    assert_eq!(highest as f64, GROUND);
}

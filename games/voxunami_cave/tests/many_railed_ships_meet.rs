//! Many ships on rails that meet other ships on rails at once (step 4c-5-4c of `docs/decisions/joints-and-cars.md`): each has a frame of its own while the other is near, each two
//! that are near each other are the two sides of **one** ghost, and what that costs a tick does not grow with the square of the ships (the ghost's search for what is near, and its
//! clusters, once asked of each mechanism by itself and of each mechanism about every other: 55 ms a tick of 400 ships, measured 2026-11-18).
//!
//! The ships are in pairs along x, 12,000 voxels from the next pair (a ship on a rail has a rail only if nothing is near it, and what is near is a second of speed: 1,500 voxels of
//! two ships at orbital speed). Both ships of a pair go round the planet together; the second is made east of the first and closes on it along x at 1,200 voxels a second, 24 voxels
//! off its axis, so that they pass and do not touch.

mod common;

use common::docking::{PLAIN, make_all_going, stone};
use common::rails::put_on_its_rail_in;
use common::{Scene, circular_speed, pose_at, ship_id};

const DISTANCE: f64 = 70_016.0;

/// What a run of `pairs` pairs gave, over 100 ticks.
struct Run {
    /// Most frames the bodies had at once.
    most_frames: u64,
    ghosts_made: u64,
    /// The average of the milliseconds a tick took over the last 40 ticks (when the pairs are near).
    milliseconds_near: f64,
}

/// `pairs` pairs of ships on rails; in each the second starts `gap` voxels east of the first (a gap that is negative puts it west, going away from it: the ships never meet).
fn run_pairs(pairs: usize, gap: f64) -> Run {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    let (mut firsts, mut seconds) = (Vec::new(), Vec::new());
    for pair in 0..pairs {
        let x = pair as f64 * 12_000.0;
        firsts.push((pose_at(x, DISTANCE, 0.0), vec![([0, 0, 0], stone(), PLAIN)]));
        seconds.push((
            pose_at(x + gap, DISTANCE + 24.0, 0.0),
            vec![([0, 0, 0], stone(), PLAIN)],
        ));
    }
    make_all_going(&mut scene, firsts, [0.0, 0.0, speed]);
    make_all_going(&mut scene, seconds, [-1_200.0, 0.0, speed]);
    scene.ticks(2);
    let count = 2 * pairs as u64;
    scene.run(move |world| {
        for ship in 1..=count {
            put_on_its_rail_in(world, ship_id(ship));
        }
    });
    let mut most_frames = 0;
    let mut milliseconds = Vec::new();
    for _ in 0..100 {
        let started = std::time::Instant::now();
        scene.ticks(1);
        milliseconds.push(started.elapsed().as_secs_f64() * 1_000.0);
        most_frames = most_frames.max(report().frames);
    }
    Run {
        most_frames,
        ghosts_made: report().ghosts_made,
        milliseconds_near: milliseconds[60..].iter().sum::<f64>() / 40.0,
    }
}

#[test]
fn every_ship_on_a_rail_that_meets_another_has_a_frame_and_each_pair_has_one_ghost() {
    let pairs = 20;
    let meeting = run_pairs(pairs, 6_000.0);
    // (Both ships of every pair have a frame while the other is near, and the two are the two sides of one ghost: not two ghosts for a pair and not one for them all.)
    assert_eq!(
        meeting.most_frames,
        2 * pairs as u64,
        "a frame for each ship"
    );
    assert_eq!(meeting.ghosts_made, pairs as u64, "one ghost for each pair");
    // The pairs that go apart are never near: no frame, no ghost. (The frames the bodies have, the one for the planet's ground and those of the first ticks, are what they were.)
    let apart = run_pairs(pairs, -2_500.0);
    assert!(
        apart.most_frames < meeting.most_frames,
        "{} frames with the ships going apart, {} with them meeting",
        apart.most_frames,
        meeting.most_frames
    );
    assert_eq!(apart.ghosts_made, 0);
}

/// What the frames and the ghosts of 400 ships that all meet cost a tick, against 400 that do not: run with `--ignored --nocapture`. Measured on the owner's machine 2026-11-18
/// before the ghost's near search was asked for every mechanism together: 88 ms a tick against 33; after: about 56 against 40 (a machine that was busy: the cost of the search is
/// about 8 ms of what is left, 400 mechanisms times 400 ships, and the next lever is a spatial index for it).
#[test]
#[ignore = "a benchmark: run with --ignored --nocapture"]
fn what_a_tick_costs_when_four_hundred_ships_on_rails_all_meet() {
    for pairs in [25, 50, 100, 200] {
        let apart = run_pairs(pairs, -2_500.0);
        let meeting = run_pairs(pairs, 6_000.0);
        eprintln!(
            "{:4} ships: {:7.2} ms a tick going apart, {:7.2} ms meeting ({} frames at most, {} ghosts)",
            2 * pairs,
            apart.milliseconds_near,
            meeting.milliseconds_near,
            meeting.most_frames,
            meeting.ghosts_made
        );
    }
}

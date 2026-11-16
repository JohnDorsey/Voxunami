//! **What the ground of a planet costs a ship near it or on it does not depend on how fast the planet goes round on its rail** (the owner, 2026-11-16: planets on rails are the
//! planets of the finished game, and a bug where the speed that the lookahead of the terrain or the steps of the solver count is not the speed against the ground would show here).
//!
//! The ground of a planet is in a frame that goes with it, where a ship it carries is at rest against the ground when it rests on it, whatever the planet does. What the plugin
//! asks of the ground is **the bricks of it that get colliders** (the lookahead of the terrain: 8 voxels and a quarter of a second of the speed against the ground) and **the
//! steps the solver takes** (a tick is cut so that nothing crosses more than 0.8 voxels in one of them, by the speed bodies close at); both count work and not time
//! (`BodiesReport`). A speed that is the world's, with the speed of the planet in it, would ask for the bricks of a quarter of a second at 125 or at 1,257 voxels a second and the
//! steps of a tick at 6 or at 63 voxels, though the ship rests.
//!
//! Each scene has the wanderer (`common::moving_planet`: ground 10,000 voxels from its middle, hills 20 high, a pull of 8 at its surface) on a rail round the home planet that
//! stands still (a period of 10^15 seconds), goes round at 125.7 voxels a second (the usual), or at 1,257. The same ship is made in each, at the same place from the planet and
//! going the same way against the ground, and what each tick cost is asked of all three: **the same bricks and the same steps, tick for tick.** (The ship is a free ship that
//! the planet carries; a group with a frame of its own over the ground of such a planet needs the world not to carry it, and has a scene here when it can be there.)
//!
//! **What is not compared, and why.** The two ticks in which a scene makes its ship (`raft_over` makes it in one tick, at the place the planet was, and puts it in the next, where
//! the planet is: a planet that goes 63 voxels a tick has gone) are not counted: the plugin sees the ship in the wrong place for a tick, and makes bricks there, in each world a
//! different number. **Where the ship ends is only asked to be about the same** (half a voxel): a thin raft that lands on a slope amplifies the rounding of its place (ran,
//! 2026-11-16: the paths of a raft landing at four places differ from a planet that stands still by 1e-14 to 1e-5 at every speed from 126 to 2,513 voxels a second, with no trend
//! in the speed; at one place the same alternative outcome of the first touch, 0.15 voxels, at 1,257 and at 2,513 and none at 126 to 838: a flip of a discrete choice of the
//! contact by the noise that a planet that moves has and one that stands still has none of). What the cost is, is not touched by that: the steps and the bricks are asked tick
//! for tick.

mod common;

use common::moving_planet::{WANDERER, orbit_with_period, reporting_scene_with, wanderer_pack};
use common::{Scene, minus, raft_over, ship_over, ship_voxels, top_of_the_ground};
use grid_bodies::BodiesReport;

/// The periods of the wanderer's rail, in seconds, and what each is: standing still, 125.7 voxels a second, 1,257.
const PERIODS: [(&str, f64); 3] = [
    ("standing still", 1.0e15),
    ("going round at 125.7 voxels a second", 20_000.0),
    ("going round at 1,257 voxels a second", 2_000.0),
];

/// How far two worlds may put the ship apart at the end (see the head of the file): half a voxel.
const ABOUT_THE_SAME: f64 = 0.5;

/// What a tick cost: the bricks of terrain that got colliders in it, and the solver steps it was cut into.
type Cost = (u64, u64);

struct Wanderer {
    scene: Scene,
    report: Box<dyn Fn() -> BodiesReport>,
    /// The highest voxel of the ground in the column the ships are made over, in voxels from the middle of the wanderer.
    surface: f64,
}

/// What a scene did with a ship: what each tick cost, where the ship was at the end (along x, and its height over the ground), and the lowest it was.
struct Outcome {
    costs: Vec<Cost>,
    along: f64,
    height: f64,
    lowest: f64,
}

/// A scene with the wanderer going round the home planet once in `period` seconds, a tick on.
fn the_wanderer_going_round_in(period: f64) -> Wanderer {
    let (mut scene, report) =
        reporting_scene_with(&wanderer_pack(&orbit_with_period(period), 340, 8.0));
    scene.ticks(1);
    let surface = top_of_the_ground(&mut scene, WANDERER, 3, 3, 9_900..10_100) as f64;
    Wanderer {
        scene,
        report: Box::new(report),
        surface,
    }
}

impl Wanderer {
    /// What each of the next `ticks` ticks cost.
    fn costs_over(&mut self, ticks: usize) -> Vec<Cost> {
        (0..ticks)
            .map(|_| {
                self.scene.ticks(1);
                let report = (self.report)();
                (
                    report.terrain_bricks_made_last_tick,
                    report.solver_steps_last_tick,
                )
            })
            .collect()
    }

    /// Where a ship is from the middle of the wanderer, in the axes of the world: how far along x it is and how high over the ground.
    fn place_of(&self, ship: u64) -> (f64, f64) {
        let from_the_middle = minus(
            ship_voxels(&self.scene, ship),
            ship_voxels(&self.scene, WANDERER),
        );
        (from_the_middle[0], from_the_middle[1] - self.surface)
    }

    /// Watches a ship for `ticks` ticks: what each cost, where it ends, and the lowest it was over the ground.
    fn watch(&mut self, ship: u64, ticks: usize) -> Outcome {
        let mut costs = Vec::new();
        let mut lowest = f64::INFINITY;
        for _ in 0..ticks {
            costs.extend(self.costs_over(1));
            lowest = lowest.min(self.place_of(ship).1);
        }
        let (along, height) = self.place_of(ship);
        Outcome {
            costs,
            along,
            height,
            lowest,
        }
    }
}

/// Makes the same scene in the three worlds, and returns what each did.
fn in_the_three_worlds(scene: impl Fn(&mut Wanderer) -> Outcome) -> Vec<(&'static str, Outcome)> {
    PERIODS
        .iter()
        .map(|(name, period)| (*name, scene(&mut the_wanderer_going_round_in(*period))))
        .collect()
}

/// Asks that every world cost what the first did, tick for tick, and ended about where it did; and returns what the first did.
fn the_same_in_all<'a>(outcomes: &'a [(&'static str, Outcome)]) -> &'a Outcome {
    let (still, reference) = &outcomes[0];
    for (name, outcome) in &outcomes[1..] {
        let first_apart = outcome
            .costs
            .iter()
            .zip(&reference.costs)
            .position(|(found, expected)| found != expected);
        assert_eq!(
            outcome.costs,
            reference.costs,
            "the cost of each tick (bricks of the ground made, solver steps), {name}, is not what it was {still}, from tick {first_apart:?} on: {} bricks and {} steps in all, against {} and {}",
            bricks_in_all(outcome),
            steps_in_all(outcome),
            bricks_in_all(reference),
            steps_in_all(reference),
        );
        assert!(
            (outcome.along - reference.along).abs() < ABOUT_THE_SAME
                && (outcome.height - reference.height).abs() < ABOUT_THE_SAME
                && (outcome.lowest - reference.lowest).abs() < ABOUT_THE_SAME,
            "the ship, {name}, ends {} along and {} over the ground (lowest {}), and {} and {} (lowest {}) {still}",
            outcome.along,
            outcome.height,
            outcome.lowest,
            reference.along,
            reference.height,
            reference.lowest,
        );
    }
    reference
}

/// A raft made `up` voxels over the ground, going `against` it as said, watched for `ticks` ticks.
fn a_raft(up: f64, against: [f64; 3], ticks: usize) -> impl Fn(&mut Wanderer) -> Outcome {
    move |world| {
        let raft = raft_over(
            &mut world.scene,
            WANDERER,
            [0.0, world.surface + up, 0.0],
            false,
            against,
        );
        world.watch(raft, ticks)
    }
}

fn bricks_in_all(outcome: &Outcome) -> u64 {
    outcome.costs.iter().map(|cost| cost.0).sum()
}

fn steps_in_all(outcome: &Outcome) -> u64 {
    outcome.costs.iter().map(|cost| cost.1).sum()
}

#[test]
fn a_ship_that_rests_on_the_ground_of_a_planet_costs_the_same_whatever_the_planet_does() {
    let outcomes = in_the_three_worlds(a_raft(4.0, [0.0; 3], 60));
    let first = the_same_in_all(&outcomes);
    // (It did something: it fell the 4 voxels and lies on the ground, its bottom a voxel over the highest voxel of the column it was made over.)
    assert!(
        first.height > 0.0 && first.height < 2.0,
        "the raft is {} voxels over the ground",
        first.height
    );
}

#[test]
fn a_ship_that_comes_down_to_the_ground_fast_costs_the_same_whatever_the_planet_does() {
    // 400 voxels over the ground going down at 300 against it: the lookahead is 83 voxels and the tick is cut into 19 steps. The planet's speed (125.7 or 1,257, in any direction)
    // would have it asked for more.
    let outcomes = in_the_three_worlds(a_raft(400.0, [0.0, -300.0, 0.0], 60));
    let first = the_same_in_all(&outcomes);
    let at_rest = in_the_three_worlds(a_raft(4.0, [0.0; 3], 60));
    assert!(
        steps_in_all(first) > steps_in_all(&at_rest[0].1),
        "(a ship that comes down at 300 voxels a second cost {} steps, and one that rests {}: the closing speed was counted)",
        steps_in_all(first),
        steps_in_all(&at_rest[0].1)
    );
    // (It came down to the ground and did not go through it: it was within 5 voxels of it, over or under, at the lowest. It is thrown up from the hit and tumbles after it, which is
    // what a thin plate does that hits a slope at 300 voxels a second, in all three worlds alike; where it ends is not asked.)
    assert!(
        first.lowest > -5.0 && first.lowest < 5.0,
        "the raft was {} voxels over the ground at the lowest: it came down to it and did not go through",
        first.lowest
    );
}

#[test]
fn a_ship_that_slides_over_the_ground_fast_costs_the_same_whatever_the_planet_does() {
    // Three voxels over the ground going 250 voxels a second along it: the bricks of the ground it is about to reach are made as it goes.
    let outcomes = in_the_three_worlds(a_raft(3.0, [250.0, 0.0, 0.0], 60));
    let first = the_same_in_all(&outcomes);
    assert!(
        first.along > 100.0,
        "the raft went {} voxels along the ground",
        first.along
    );
    assert!(bricks_in_all(first) > 0, "no brick of the ground was made");
}

#[test]
fn a_big_ship_at_rest_on_the_ground_costs_the_same_whatever_the_planet_does() {
    // (A ship of 16 by 4 by 16 has bricks of its own and more of the ground under it.)
    let outcomes = in_the_three_worlds(|world| {
        let ship = ship_over(
            &mut world.scene,
            WANDERER,
            [0.0, world.surface + 3.0, 0.0],
            false,
            [0.0; 3],
            [16, 4, 16],
        );
        world.watch(ship, 60)
    });
    let first = the_same_in_all(&outcomes);
    // (It fell 3 voxels and lies on the ground: its bottom is a voxel over the highest voxel of the column, or a little more where the ground is higher under it.)
    assert!(
        first.height > 0.0 && first.height < 12.0,
        "the ship is {} voxels over the ground",
        first.height
    );
}

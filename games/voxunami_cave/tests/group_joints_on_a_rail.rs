//! A joint end of a car of a chain that rides a rail pulls on the joint end of a car (step 4c-5-4b of `docs/decisions/joints-and-cars.md`, for groups): **the pull is a force on the
//! group's rail and a torque on the bodies, so it never takes the chain off its rail; only the coupling, which is a touch, does** (the owner, 2026-11-19). This is `joints_on_rails.rs`
//! for a group.
//!
//! The chain is three cars with a joint end on the east end of the last, and the car is the one with a joint end on its west face that `joints_on_rails.rs` has, made `GAP` voxels beyond
//! it and closing on it. Each case is run with the chain on a rail and, as its oracle, free (the bodies have always pulled and coupled a group that is free), each with joints and
//! as plain stone, so that what a pull does is a difference of two runs: what a rail does to a car in a tick that a body does not is in both and is taken out.
//!
//! What a pull does to a group is its **centre of mass**: the impulse on the group is its mass times the change of the speed of its centre of mass, which a rail has exact.

mod common;

use common::chains::{
    Lying, Vec3, a_chain_in_orbit_lying, car, centre_of, mass_of, origins_of, pivot_gap,
    put_on_its_rail, set_going, world_of,
};
use common::docking::{PLAIN, make_going, stone, stones};
use common::joints::{ball_end_toward, joint_block};
use common::rails::put_on_a_rail_near_another_ship;
use common::{Scene, circular_speed, length, ship_id};
use voxel_space::{FaceDirection, GridId};

const DISTANCE: f64 = 70_016.0;

/// How far the car's west pivot is from the chain's east pivot when it is made, in voxels, and how fast it closes on it, in voxels a second: it comes into the range of the pull (6) as
/// ships do, from outside it, with its rail already if it is to have one.
const GAP: f64 = 7.0;
const CLOSING: f64 = 2.0;

/// What the chain is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Weight {
    /// Three cars 3, 4 and 3 voxels long (the last has the joint end on its east end): the cars turn into line fast.
    Light,
    /// The cars of `a_chain`: 7, 6 and 7 voxels long, long rods that turn easily about their length and not about the rest.
    Heavy,
}

impl Weight {
    /// The stones in each car, and the length of the chain along x.
    fn stones(self) -> [i128; 3] {
        match self {
            Weight::Light => [2, 2, 1],
            Weight::Heavy => [6, 4, 5],
        }
    }

    /// Where the last car's east joint end is (its x), which is also where its pivot is, less one.
    fn east_end(self) -> i128 {
        match self {
            Weight::Light => 2,
            Weight::Heavy => 6,
        }
    }

    fn length(self) -> f64 {
        self.length_of(3)
    }

    /// The stones in each car of a chain of `cars` cars (at least two): the first, the middle ones as the second of three, the last.
    fn stones_of(self, cars: usize) -> Vec<i128> {
        let [first, middle, last] = self.stones();
        let mut stones = vec![first];
        stones.extend(std::iter::repeat_n(middle, cars - 2));
        stones.push(last);
        stones
    }

    /// The length of a chain of `cars` cars along x: the first car's `stones + 1` (its east joint), each middle car's `stones + 2`, and the last's `stones + 2` (a west joint and the
    /// east end, one more).
    fn length_of(self, cars: usize) -> f64 {
        let [first, middle, last] = self.stones();
        (first + 1 + (cars as i128 - 2) * (middle + 2) + last + 2) as f64
    }

    /// The x of each car's east pivot in a chain of `cars` cars, except the last, which has the chain's (`east_pivot`): the car's length, which is also where the next car's west pivot
    /// is, at its origin.
    fn pivots_between(self, cars: usize) -> Vec<f64> {
        let [first, middle, _] = self.stones();
        (0..cars - 1)
            .map(|index| if index == 0 { first + 1 } else { middle + 2 } as f64)
            .collect()
    }

    /// The x of the chain's east pivot in the last car.
    fn east_pivot(self) -> f64 {
        (self.east_end() + 1) as f64
    }
}

/// Whether the chain rides a rail while the joint ends pull.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Chain {
    /// Free, as a group always was (the oracle).
    Free,
    /// On the rail of its centre of mass, put on it before the car came.
    OnARail,
    /// On its rail, and the car is put on one in the tick after it is made, as near the chain as it is.
    AndTheCarOnOne,
}

/// The chain (three cars, numbers 1 to 3, coupled by their joints, standing still against each other) in orbit at `DISTANCE` going along z at the speed of a circle, with a joint end
/// on the east end of the last car (or a stone of the same size, if `with_joints` is false), and the car (number 4: a joint end and three stones, or four stones) made `GAP` voxels
/// beyond the chain's end, going as it goes and `CLOSING` voxels a second toward it. The scene, what the bodies report, the cars and the car. This one is for a chain of this weight, with the car closing at `closing` voxels a second, the chain and the car lying along x as they are made or turned a quarter so that
/// they lie along z (the car then comes along z, from beyond the chain's end), and the car made `to_the_side` voxels off the line of the chain along y (the way up from the planet), so
/// that the pull on the chain's end has a part across the chain, which turns it.
fn a_weighted_chain_and_a_car(
    chain: Chain,
    weight: Weight,
    closing: f64,
    lying: Lying,
    to_the_side: f64,
    with_joints: bool,
) -> (
    Scene,
    impl Fn() -> grid_bodies::BodiesReport,
    Vec<GridId>,
    GridId,
) {
    a_long_chain_and_a_car(chain, weight, 3, closing, lying, to_the_side, with_joints)
}

/// `a_weighted_chain_and_a_car` for a chain of `cars` cars (numbers 1 to `cars`, the car is the next).
fn a_long_chain_and_a_car(
    chain: Chain,
    weight: Weight,
    cars: usize,
    closing: f64,
    lying: Lying,
    to_the_side: f64,
    with_joints: bool,
) -> (
    Scene,
    impl Fn() -> grid_bodies::BodiesReport,
    Vec<GridId>,
    GridId,
) {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    // Three light cars (3, 4 and 3 voxels long): the heavy ones of `a_chain` turn too slowly into line for a coupling before they touch (the free chain does not couple either).
    let in_each = weight.stones_of(cars);
    let mut made = vec![car(false, in_each[0], true)];
    made.extend(
        in_each[1..cars - 1]
            .iter()
            .map(|count| car(true, *count, true)),
    );
    made.push(car(true, in_each[cars - 1], false));
    if with_joints {
        made[cars - 1].0.push((
            [weight.east_end(), 0, 0],
            joint_block(),
            ball_end_toward(FaceDirection::PositiveX),
        ));
    } else {
        made[cars - 1]
            .0
            .push(([weight.east_end(), 0, 0], stone(), PLAIN));
    }
    made[cars - 1].1 += 1.0;
    let ships = a_chain_in_orbit_lying(&mut scene, made, DISTANCE, speed, lying);
    let still = vec![([0.0; 3], [0.0; 3]); cars];
    match chain {
        Chain::OnARail | Chain::AndTheCarOnOne => {
            put_on_its_rail(&mut scene, &ships, speed, still);
        }
        Chain::Free => set_going(&mut scene, &ships, speed, still),
    }
    scene.ticks(3);
    let second = if with_joints {
        car(true, 3, false).0
    } else {
        stones(0, 4)
    };
    let mut origin = origins_of(&scene, &ships[..1])[0];
    origin[lying.axis()] += weight.length_of(cars) + GAP;
    origin[1] += to_the_side;
    let mut velocity = [0.0, 0.0, speed];
    velocity[lying.axis()] -= closing;
    make_going(&mut scene, lying.pose_at(origin), second, velocity);
    // (The same number of ticks for every case: the car is given its rail in this one, or nothing is done.)
    let ship = ship_id(cars as u64 + 1);
    scene.run(move |world| {
        if chain == Chain::AndTheCarOnOne {
            put_on_a_rail_near_another_ship(world, ship);
        }
    });
    (scene, report, ships, ship)
}

/// What is read of the chain and the car after a tick.
#[derive(Clone, Copy, Debug)]
struct Reading {
    chain_speed: Vec3,
    car_speed: Vec3,
    /// Whether the chain's anchor and the car ride a rail.
    on_a_rail: [bool; 2],
    /// How far the chain's east pivot and the car's west pivot are apart, in voxels.
    gap: f64,
}

fn read(scene: &Scene, ships: &[GridId], car: GridId, weight: Weight) -> Reading {
    let grids = scene.rig.server.world().grids();
    Reading {
        chain_speed: centre_of(scene, ships).1,
        car_speed: grids.get(car).expect("the car").linear_velocity,
        on_a_rail: [
            grids.get(ships[0]).expect("the anchor").rail.is_some(),
            grids.get(car).expect("the car").rail.is_some(),
        ],
        gap: pivot_gap(
            scene,
            (*ships.last().expect("a chain"), weight.east_pivot()),
            car,
        ),
    }
}

/// What is read after each of `ticks` ticks that the cars are followed for.
fn followed_for(chain: Chain, with_joints: bool, ticks: usize) -> Vec<Reading> {
    followed_for_a_chain_of(chain, 3, with_joints, ticks)
}

/// `followed_for` for a chain of `cars` light cars.
fn followed_for_a_chain_of(
    chain: Chain,
    cars: usize,
    with_joints: bool,
    ticks: usize,
) -> Vec<Reading> {
    let (mut scene, _, ships, car) = a_long_chain_and_a_car(
        chain,
        Weight::Light,
        cars,
        CLOSING,
        Lying::AlongX,
        0.0,
        with_joints,
    );
    (0..ticks)
        .map(|_| {
            scene.ticks(1);
            read(&scene, &ships, car, Weight::Light)
        })
        .collect()
}

/// The impulse (mass times the change of speed) that the pull had given the chain and the car by the end of each tick, from the first in which either has a newton second: what the pull
/// did to the speed of the chain's centre of mass and to the car's (less the same in the run of plain stone), times the mass.
fn impulses_given_by_the_pull(chain: Chain, ticks: usize) -> Vec<[Vec3; 2]> {
    impulses_given_to_a_chain_of(chain, 3, ticks)
}

/// `impulses_given_by_the_pull` for a chain of `cars` light cars.
fn impulses_given_to_a_chain_of(chain: Chain, cars: usize, ticks: usize) -> Vec<[Vec3; 2]> {
    let (scene, _, ships, car) = a_long_chain_and_a_car(
        chain,
        Weight::Light,
        cars,
        CLOSING,
        Lying::AlongX,
        0.0,
        true,
    );
    let masses = [mass_of(&scene, &ships), mass_of(&scene, &[car])];
    drop(scene);
    let (with, without) = (
        followed_for_a_chain_of(chain, cars, true, ticks),
        followed_for_a_chain_of(chain, cars, false, ticks),
    );
    let mut impulses = Vec::new();
    for (with, without) in with.iter().zip(&without) {
        let given = [
            [0, 1, 2].map(|axis| (with.chain_speed[axis] - without.chain_speed[axis]) * masses[0]),
            [0, 1, 2].map(|axis| (with.car_speed[axis] - without.car_speed[axis]) * masses[1]),
        ];
        if given.iter().flatten().any(|part| part.abs() > 1.0) || !impulses.is_empty() {
            impulses.push(given);
        }
    }
    impulses
}

#[test]
fn each_tick_the_pull_of_a_joint_end_of_a_chain_on_a_rail_gives_the_chain_and_the_car_the_same_impulse_the_opposite_way()
 {
    for chain in [Chain::OnARail, Chain::AndTheCarOnOne] {
        let given = impulses_given_by_the_pull(chain, 30);
        assert!(
            given.len() >= 15,
            "{chain:?}: the chain and the car were pulled for a while: {} ticks",
            given.len()
        );
        // **A tick apart, for a chain on a rail and a car that is free** (see `joints_on_rails.rs`): the rail is made whole in the tick that the pull is asked in, and the bodies give
        // the force to a car that is free in the next.
        let skew = usize::from(chain == Chain::OnARail);
        for tick in 0..given.len() - skew {
            let (chain_has, car_has) = (given[tick][0], given[tick + skew][1]);
            assert!(
                length(chain_has) > 1.0,
                "{chain:?}, tick {tick}: the chain was pulled: {chain_has:?}"
            );
            let sum = [0, 1, 2].map(|axis| chain_has[axis] + car_has[axis]);
            assert!(
                length(sum) <= 1.0e-3 * length(chain_has),
                "{chain:?}, tick {tick}: the impulses {chain_has:?} and {car_has:?} are not opposite"
            );
        }
    }
}

/// How the run went until the car was coupled to the chain.
struct Run {
    /// The tick (from the first after the car was made) in which the chain was found off its rail, if it was.
    left_at: Option<usize>,
    /// The tick the bodies reported the car coupled in (the chain's two couplings and one more), if they did.
    coupled_at: Option<usize>,
    /// The tick (from the first after the car was made) in which the pivots were first within 0.3 voxels of each other, the coupling distance, if they were.
    near_at: Option<usize>,
    gap_at_the_touch: f64,
    car_rode_until_the_touch: bool,
    /// The widest that any two pivots of the chain's own cars were apart in any tick of the run, in voxels.
    worst_gap_inside: f64,
    scene: Scene,
    report: Box<dyn Fn() -> grid_bodies::BodiesReport>,
    ships: Vec<GridId>,
    car: GridId,
}

fn until_the_car_is_coupled(chain: Chain) -> Run {
    until_a_car_is_coupled(chain, Weight::Light, CLOSING, Lying::AlongX, 0.0)
}

/// The run of a chain of this weight and a car that closes on it at `closing` voxels a second, the two lying as asked, the car `to_the_side` voxels off the line of the chain.
fn until_a_car_is_coupled(
    chain: Chain,
    weight: Weight,
    closing: f64,
    lying: Lying,
    to_the_side: f64,
) -> Run {
    until_a_car_is_coupled_by_a_chain_of(chain, weight, 3, closing, lying, to_the_side)
}

/// `until_a_car_is_coupled` for a chain of `cars` cars (the car is coupled when the bodies report one coupling more than the chain's `cars - 1`).
fn until_a_car_is_coupled_by_a_chain_of(
    chain: Chain,
    weight: Weight,
    cars: usize,
    closing: f64,
    lying: Lying,
    to_the_side: f64,
) -> Run {
    let (mut scene, report, ships, car) =
        a_long_chain_and_a_car(chain, weight, cars, closing, lying, to_the_side, true);
    let (mut left_at, mut coupled_at, mut near_at) = (None, None, None);
    let mut gap_at_the_touch = f64::NAN;
    let mut car_rode_until_the_touch = true;
    let mut worst_gap_inside = 0.0f64;
    let pivots = weight.pivots_between(cars);
    for tick in 0..400 {
        scene.ticks(1);
        for (index, pivot) in pivots.iter().enumerate() {
            worst_gap_inside =
                worst_gap_inside.max(pivot_gap(&scene, (ships[index], *pivot), ships[index + 1]));
        }
        let reading = read(&scene, &ships, car, weight);
        if near_at.is_none() && reading.gap < 0.3 {
            near_at = Some(tick);
        }
        if left_at.is_none() {
            if reading.on_a_rail[0] {
                car_rode_until_the_touch &=
                    reading.on_a_rail[1] == (chain == Chain::AndTheCarOnOne);
            } else {
                left_at = Some(tick);
                gap_at_the_touch = reading.gap;
            }
        }
        if report().couplings > cars as u64 - 1 {
            coupled_at = Some(tick);
            break;
        }
    }
    Run {
        left_at,
        coupled_at,
        near_at,
        gap_at_the_touch,
        car_rode_until_the_touch,
        worst_gap_inside,
        scene,
        report: Box::new(report),
        ships,
        car,
    }
}

#[test]
fn a_chain_on_a_rail_stays_on_it_while_its_joint_end_pulls_and_comes_off_it_at_the_coupling_and_so_does_the_car_that_is_on_one()
 {
    for chain in [Chain::OnARail, Chain::AndTheCarOnOne] {
        let run = until_the_car_is_coupled(chain);
        let left_at = run.left_at.unwrap_or_else(|| {
            panic!(
                "{chain:?}: the chain never left its rail; the car was coupled at {:?}",
                run.coupled_at
            )
        });
        // The old rule took the chain off its rail in the first tick of the pull, 6 voxels from the car.
        assert!(
            left_at >= 15,
            "{chain:?}: the chain rode its rail until tick {left_at} (the pull went on for a while)"
        );
        assert!(
            run.gap_at_the_touch <= 0.3,
            "{chain:?}: the pivots were {} voxels apart when the rail came off",
            run.gap_at_the_touch
        );
        assert!(
            run.car_rode_until_the_touch,
            "{chain:?}: the car was on a rail exactly when it was meant to be"
        );
        let coupled_at = run
            .coupled_at
            .unwrap_or_else(|| panic!("{chain:?}: the car was never coupled"));
        // (Measured 2026-11-19: the rail came off in tick 49 with the pivots 0.15 voxels apart, and the bodies coupled the car in the next; the free chain was coupled in 54.)
        assert_eq!(
            coupled_at,
            left_at + 1,
            "{chain:?}: the rail came off in tick {left_at} and the car was coupled in {coupled_at}"
        );
        let report = (run.report)();
        assert_eq!(
            report.rails_left_for_ends,
            if chain == Chain::AndTheCarOnOne { 2 } else { 1 },
            "{chain:?}: a rail was left for the coupling by each of the chain and the car that rode one, and not before"
        );
        assert_eq!(report.rail_kicks, 0, "{chain:?}: no hit was made");
        // And they are coupled for good: the chain and the car are one group, and the joint holds.
        let mut scene = run.scene;
        scene.ticks(20);
        let gap = pivot_gap(&scene, (run.ships[2], Weight::Light.east_pivot()), run.car);
        assert!(gap < 0.05, "{chain:?}: the joint holds: {gap} voxels");
        assert_eq!((run.report)().couplings, 3, "{chain:?}: three couplings");
    }
}

#[test]
fn a_chain_on_a_rail_is_coupled_to_a_car_in_about_the_time_the_free_chain_is() {
    let free = until_the_car_is_coupled(Chain::Free)
        .coupled_at
        .expect("the free chain was coupled to the car");
    for chain in [Chain::OnARail, Chain::AndTheCarOnOne] {
        let on_a_rail = until_the_car_is_coupled(chain)
            .coupled_at
            .unwrap_or_else(|| panic!("{chain:?}: the car was never coupled"));
        assert!(
            on_a_rail.abs_diff(free) <= 4,
            "{chain:?}: coupled in tick {on_a_rail}, the free chain in {free}"
        );
    }
}

#[test]
fn the_pull_of_a_free_chain_closes_the_ends_smoothly_and_slowly_at_the_end() {
    // The pull of the bodies (sized by the mass of the group) was thought to flip-flop on a chain's end car, its ends swinging between 1.9 and 3.1 metres a second apart. It does not in
    // this scene (measured 2026-11-19: the gap shrinks every tick and the ends meet at about half a metre a second); the flip-flop was the rails' pull when it was sized the same way.
    let readings = followed_for(Chain::Free, true, 70);
    let gaps: Vec<f64> = readings.iter().map(|reading| reading.gap).collect();
    let touch = gaps
        .iter()
        .position(|gap| *gap < 0.01)
        .expect("the ends were brought together");
    assert!(touch > 30, "the pull took a while: {touch} ticks");
    for tick in 1..touch {
        assert!(
            gaps[tick] < gaps[tick - 1],
            "the gap grew in tick {tick}: {} after {}",
            gaps[tick],
            gaps[tick - 1]
        );
    }
    // And they are coupled soon after they come within the coupling distance (they take about 8 ticks, arriving slowly; a pull that hovers there takes 30 or more).
    let near = gaps
        .iter()
        .position(|gap| *gap < 0.3)
        .expect("the ends came within the coupling distance");
    assert!(
        touch - near <= 12,
        "the ends were within the coupling distance from tick {near} and together in {touch}"
    );
}

#[test]
fn a_chain_of_heavy_cars_is_coupled_soon_after_its_end_is_within_the_coupling_distance_of_the_car_free_or_on_a_rail_at_every_speed_of_the_car()
 {
    // The cars of `a_chain` are long rods: they turn easily about their length. Measured 2026-11-19 against the old pull (a force sized by the mass of the group and a torque
    // against the spin sized by one number for the inertia): the spin of the cars against each other alternated in sign every tick and the free chain hovered 0.02 voxels from the car
    // for 25 ticks and more (coupled 34 ticks after it was within 0.3 voxels, at a closing speed of 2; and not at all at 1 in 400 ticks with a torque by axis and the old force).
    for closing in [1.0, 2.0, 3.0, 4.0] {
        let free = until_a_car_is_coupled(Chain::Free, Weight::Heavy, closing, Lying::AlongX, 0.0);
        let (near, coupled) = (
            free.near_at
                .unwrap_or_else(|| panic!("closing {closing}: never within 0.3 voxels")),
            free.coupled_at
                .unwrap_or_else(|| panic!("closing {closing}: the free chain was never coupled")),
        );
        assert!(
            coupled - near <= 12,
            "closing {closing}: within 0.3 voxels in tick {near}, coupled in {coupled}"
        );
        for chain in [Chain::OnARail, Chain::AndTheCarOnOne] {
            let run = until_a_car_is_coupled(chain, Weight::Heavy, closing, Lying::AlongX, 0.0);
            assert!(
                run.coupled_at.is_some(),
                "closing {closing}, {chain:?}: never coupled"
            );
        }
    }
}

#[test]
fn a_chain_and_a_car_turned_a_quarter_go_as_they_do_not_turned_in_line_or_to_one_side_free_or_on_a_rail()
 {
    // Nothing in the pull is about which way the world's axes point: the same chain and car, turned a quarter about y so that they lie along z and not along x, come together in the
    // same ticks (measured 2026-11-20 for the free chain and the one on a rail, light and heavy, the car in line and 0.5 to 1.5 voxels to one side: the same
    // to the tick in every case but one, a tick apart for the free chain with the car 1.5 voxels to one side). A pull that took a car's own axes for the world's, for the spin it slows or for the mass that the end answers with, would come together later: 3 to 15 ticks (measured with each of the four places of the turn taken for the identity).
    // Unlike a rule of "coupled soon after near" this tells a pull that is only slower, and the car to one side gives the pull a part across the chain, which turns it, so that the
    // end's mass is not the mass of the chain.
    for weight in [Weight::Light, Weight::Heavy] {
        let (scene, _, ships, _) =
            a_weighted_chain_and_a_car(Chain::Free, weight, CLOSING, Lying::AlongZ, 0.0, true);
        // (The turn is the one meant: the chain's length lies along the world's z, from the first car's origin, and its east pivot is beyond it.)
        let first = world_of(&scene, ships[0], [0.0, 0.5, 0.5]);
        let last = world_of(&scene, ships[2], [weight.east_pivot(), 0.5, 0.5]);
        let along = [0, 1, 2].map(|axis| last[axis] - first[axis]);
        assert!(
            (along[2] - weight.length()).abs() < 1.0e-3
                && along[0].abs() < 1.0e-3
                && along[1].abs() < 1.0e-3,
            "{weight:?}: the chain lies along z, {} voxels long: {along:?}",
            weight.length()
        );
        drop(scene);
        for to_the_side in [0.0, 1.0, 1.5] {
            for chain in [Chain::Free, Chain::OnARail, Chain::AndTheCarOnOne] {
                let run = |lying| {
                    let run = until_a_car_is_coupled(chain, weight, CLOSING, lying, to_the_side);
                    (
                        run.near_at,
                        run.coupled_at.unwrap_or_else(|| {
                            panic!("{weight:?}, {chain:?}, {lying:?}, {to_the_side} to the side: never coupled")
                        }),
                    )
                };
                let (along_x, along_z) = (run(Lying::AlongX), run(Lying::AlongZ));
                let (near_x, near_z) = (
                    along_x.0.expect("within 0.3 voxels along x"),
                    along_z.0.expect("within 0.3 voxels along z"),
                );
                assert!(
                    near_x.abs_diff(near_z) <= 1 && along_x.1.abs_diff(along_z.1) <= 1,
                    "{weight:?}, {chain:?}, {to_the_side} to the side: within 0.3 voxels in tick {near_x} and coupled in {} along x, in {near_z} and {} along z",
                    along_x.1,
                    along_z.1
                );
            }
        }
    }
}

#[test]
fn a_chain_of_more_cars_is_coupled_to_a_car_in_the_tick_a_chain_of_three_is_free_or_on_a_rail_light_or_heavy_and_its_joints_are_not_strained()
 {
    // Nothing in the pull is about how many cars there are: the force is the end's own response, the rail's is the mass of the whole group over the force, and the chain's own joints
    // hold it together. Measured 2026-11-22: 3 to 6 cars, light and heavy, free, on a rail and with the car on one, are coupled in ticks 50 to 52 (the ends within 0.3 voxels in 46
    // to 48), and no two pivots of the chain's own cars were more than 6.2e-5 voxels apart in 120 ticks.
    for weight in [Weight::Light, Weight::Heavy] {
        for chain in [Chain::Free, Chain::OnARail, Chain::AndTheCarOnOne] {
            let of_three =
                until_a_car_is_coupled_by_a_chain_of(chain, weight, 3, CLOSING, Lying::AlongX, 0.0);
            let three = of_three.coupled_at.expect("three cars: coupled");
            for cars in [4, 6] {
                let run = until_a_car_is_coupled_by_a_chain_of(
                    chain,
                    weight,
                    cars,
                    CLOSING,
                    Lying::AlongX,
                    0.0,
                );
                let coupled = run.coupled_at.unwrap_or_else(|| {
                    panic!("{weight:?}, {chain:?}, {cars} cars: never coupled to the car")
                });
                assert!(
                    coupled.abs_diff(three) <= 2,
                    "{weight:?}, {chain:?}: {cars} cars were coupled in tick {coupled}, three in {three}"
                );
                assert_eq!(
                    (run.report)().couplings,
                    cars as u64,
                    "{weight:?}, {chain:?}, {cars} cars: one coupling for each pair"
                );
                assert!(
                    run.worst_gap_inside < 1.0e-3,
                    "{weight:?}, {chain:?}, {cars} cars: two pivots of the chain's own cars were {} voxels apart",
                    run.worst_gap_inside
                );
                if chain != Chain::Free {
                    assert!(
                        run.left_at.is_some_and(|left| left + 3 >= coupled),
                        "{weight:?}, {chain:?}, {cars} cars: the chain left its rail at {:?}, and the car was coupled at {coupled}",
                        run.left_at
                    );
                }
            }
        }
    }
}

#[test]
fn each_tick_the_pull_on_a_chain_of_six_cars_gives_the_chain_and_the_car_the_same_impulse_the_opposite_way()
 {
    // (The rail is pushed by the force over the mass of all the ships of the group: with a chain of three that is every ship there is, and a count of ships that was wrong would not
    // show: one that counted the first three of six would push the rail twice as hard, an asymmetry of a half.)
    //
    // **The tolerance is 4e-4 for each car, not the 1e-3 of three cars, and that is the oracle's and not the pull's** (measured 2026-11-22: the asymmetry of the first tick is 4.3e-4,
    // 7.0e-4, 9.7e-4, 1.24e-3 and 1.77e-3 for 3, 4, 5, 6 and 8 cars, a straight line in the cars, and in no later tick more): the run of plain stone that is taken away has the joint
    // block at the chain's end made a stone, which weighs 812.5 more (a chain of three 5,937.5 against 6,750), so its centre of mass is 0.6 voxels east (4.38 against 5.00) and its rail
    // is another orbit, and what the two runs differ by is not only the pull; the lever of that grows with the length of the chain.
    for chain in [Chain::OnARail, Chain::AndTheCarOnOne] {
        let cars = 6;
        let given = impulses_given_to_a_chain_of(chain, cars, 30);
        assert!(given.len() >= 15, "{chain:?}: pulled for a while");
        let skew = usize::from(chain == Chain::OnARail);
        for tick in 0..given.len() - skew {
            let (chain_has, car_has) = (given[tick][0], given[tick + skew][1]);
            assert!(
                length(chain_has) > 1.0,
                "{chain:?}, tick {tick}: the chain was pulled: {chain_has:?}"
            );
            let sum = [0, 1, 2].map(|axis| chain_has[axis] + car_has[axis]);
            assert!(
                length(sum) <= 4.0e-4 * cars as f64 * length(chain_has),
                "{chain:?}, tick {tick}: the impulses {chain_has:?} and {car_has:?} are not opposite"
            );
        }
    }
}

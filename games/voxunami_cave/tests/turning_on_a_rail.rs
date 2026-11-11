//! A ship that turns rides a rail (step 4c-2 of `docs/decisions/joints-and-cars.md`; the owner's answer 6 of the ship audit: rotation never keeps a ship off a rail).
//!
//! A rail holds the centre of mass of a ship and nothing else; **the turn is the bodies' work**, in a frame that goes with the rail's point: the ship is a free body there, at
//! rest at the point, with no pull on it, and what it does is turn as a free body turns (Rapier's own gyroscopic step). Each tick the rail puts the centre of mass where the
//! clock says and the bodies write back how the ship faces and how fast it turns, so a ship on a rail goes on turning, and its path is the one it would have if it did not.
//!
//! The ships here are put on their rails directly (`install_a_rail`: set turning first, the rail last, since moving or speeding a ship takes a coasting rail from it), so
//! that what they do is what a ship does on a rail from its first tick, and ships at different places and speeds can be compared bit for bit.

mod common;

use common::{Scene, circular_speed, make_ship, ship_id};
use voxel_space::{GridId, GridPose, Orientation, WorldPosition};
use world_storage::rail::RailTime;

/// The distance from the middle of the planet at which a ship is clear of its ground and may go on a rail (see `orbit.rs`).
const DISTANCE: f64 = 70_000.0;
const TICKS_PER_SECOND: u32 = 20;
const SECONDS_PER_TICK: f64 = 1.0 / TICKS_PER_SECOND as f64;

/// A ship of stone of this size, whose origin is a little off the place it will have its middle at (a ship's origin is not its middle), that goes round the planet at
/// `distance` from its middle at `speed_factor` times the speed of a circle, facing `facing` and turning `spin`, **on a rail from the tick after it is made**. Its number is 1.
fn a_ship_on_a_rail(
    distance: f64,
    speed_factor: f64,
    size: [i128; 3],
    facing: Orientation,
    spin: [f64; 3],
) -> (Scene, GridId) {
    let mut scene = Scene::in_a_new_world();
    let speed = circular_speed(&scene, 0, distance) * speed_factor;
    let middle = size.map(|edge| edge as f64 / 2.0);
    make_ship(
        &mut scene,
        GridPose::new(
            WorldPosition::from_voxels([-middle[0], distance - middle[1], -middle[2]]),
            Orientation::IDENTITY,
        ),
        false,
        size,
        |_| true,
    );
    let ship = ship_id(1);
    install_a_rail(&mut scene, ship, speed, facing, spin);
    (scene, ship)
}

/// Faces a ship, sets it going at `speed` along x and turning, and puts it on the rail that is the orbit its middle and speed are on, at the world's clock.
fn install_a_rail(
    scene: &mut Scene,
    ship: GridId,
    speed: f64,
    facing: Orientation,
    spin: [f64; 3],
) {
    scene.run(move |world| {
        let position = world.data().grids().get(ship).unwrap().pose.position;
        world
            .set_grid_pose(ship, GridPose::new(position, facing))
            .unwrap();
        world
            .set_grid_velocity(ship, [speed, 0.0, 0.0], spin)
            .unwrap();
        let time = RailTime::of_tick(world.data().clock(), TICKS_PER_SECOND);
        let rail = world
            .coasting_rail(ship, time)
            .expect("the ship is clear of the ground and alone, so a rail may have it");
        world.set_grid_rail(ship, Some(rail)).unwrap();
    });
}

struct Turn {
    facing: Orientation,
    spin: [f64; 3],
}

fn turn_of(scene: &Scene, ship: GridId) -> Turn {
    let ship = scene.rig.server.world().grids().get(ship).unwrap();
    Turn {
        facing: ship.pose.orientation,
        spin: ship.angular_velocity,
    }
}

fn bits_of(turn: &Turn) -> Vec<u64> {
    turn.facing
        .components()
        .into_iter()
        .chain(turn.spin)
        .map(f64::to_bits)
        .collect()
}

fn on_a_rail(scene: &Scene, ship: GridId) -> bool {
    on_a_rail_is(scene, ship)
}

fn on_a_rail_is(scene: &Scene, ship: GridId) -> bool {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship)
        .unwrap()
        .rail
        .is_some()
}

/// Where the centre of mass of a ship is in the world, in voxels, given where it is in the ship (the middle of a block of stone).
fn middle_of(scene: &Scene, ship: GridId, middle: [f64; 3]) -> [f64; 3] {
    let ship = scene.rig.server.world().grids().get(ship).unwrap();
    ship.pose
        .local_to_world(WorldPosition::from_voxels(middle))
        .unwrap()
        .in_voxels()
}

fn length(vector: [f64; 3]) -> f64 {
    common::length(vector)
}

/// The turn from one facing to another, in radians (whichever way round).
fn angle_between(first: Orientation, second: Orientation) -> f64 {
    let turn = second.compose(first.inverse()).components();
    2.0 * libm::atan2(length([turn[0], turn[1], turn[2]]), turn[3].abs())
}

/// A vector turned by an orientation (the vector part `q` of a unit quaternion `(q, w)`: `v + 2 w (q x v) + 2 q x (q x v)`).
fn turned(by: Orientation, vector: [f64; 3]) -> [f64; 3] {
    let [x, y, z, w] = by.components();
    let q = [x, y, z];
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let first = cross(q, vector);
    let second = cross(q, first);
    [0, 1, 2].map(|axis| vector[axis] + 2.0 * w * first[axis] + 2.0 * second[axis])
}

/// The angular momentum (in the world's axes) and the energy of a box of stone of unit voxel mass that turns as `turn` says: the tensor of a box is diagonal in the box's own axes.
fn momentum_and_energy(size: [i128; 3], turn: &Turn) -> ([f64; 3], f64) {
    let [a, b, c] = size.map(|edge| edge as f64);
    let tensor = [
        (b * b + c * c) / 12.0,
        (a * a + c * c) / 12.0,
        (a * a + b * b) / 12.0,
    ];
    let own_spin = turned(turn.facing.inverse(), turn.spin);
    let own_momentum = [0, 1, 2].map(|axis| tensor[axis] * own_spin[axis]);
    let energy = 0.5
        * (0..3)
            .map(|axis| tensor[axis] * own_spin[axis] * own_spin[axis])
            .sum::<f64>();
    (turned(turn.facing, own_momentum), energy)
}

/// A ship that turns and coasts, left alone, is put on a rail after its second, and goes on turning: a cube (every axis is a principal axis, so its spin does not change) turns
/// at the speed it had, and has turned by that speed times the time.
#[test]
fn a_ship_that_turns_and_coasts_is_put_on_a_rail_and_goes_on_turning() {
    let mut scene = Scene::in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    make_ship(
        &mut scene,
        GridPose::new(
            WorldPosition::from_voxels([-1.5, DISTANCE - 1.5, -1.5]),
            Orientation::IDENTITY,
        ),
        false,
        [3, 3, 3],
        |_| true,
    );
    let ship = ship_id(1);
    let spin = [0.0, 0.3, 0.1];
    scene.run(move |world| {
        world
            .set_grid_velocity(ship, [speed, 0.0, 0.0], spin)
            .unwrap();
    });
    scene.ticks(30);
    assert!(
        on_a_rail(&scene, ship),
        "the ship coasts clear of the ground, so it is on a rail, though it turns"
    );
    let started = turn_of(&scene, ship);
    scene.ticks(40);
    assert!(on_a_rail(&scene, ship), "it keeps its rail while it turns");
    let now = turn_of(&scene, ship);
    for axis in 0..3 {
        assert!(
            (now.spin[axis] - spin[axis]).abs() < 1e-9,
            "the spin of a cube is what it was: {:?} then {:?}",
            spin,
            now.spin
        );
    }
    let turned_by = angle_between(started.facing, now.facing);
    let expected = length(spin) * 40.0 * SECONDS_PER_TICK;
    assert!(
        (turned_by - expected).abs() < 1e-6,
        "40 ticks at {} radians a second turn it by {expected}, and it turned by {turned_by}",
        length(spin)
    );
}

/// The path is the rail's: a ship that turns on a rail has its middle where a ship that does not turn has it, to well under a hundredth of a voxel, and goes at the speed the rail
/// gives it, bit for bit.
#[test]
fn a_ship_that_turns_on_a_rail_goes_where_one_that_does_not_turn_goes() {
    let facing = Orientation::from_yaw_pitch(0.4, 0.2);
    let size = [2, 3, 5];
    let (mut turning, turning_ship) =
        a_ship_on_a_rail(DISTANCE, 1.0, size, facing, [0.2, 0.3, 0.1]);
    let (mut still, still_ship) = a_ship_on_a_rail(DISTANCE, 1.0, size, facing, [0.0; 3]);
    for second in 1..=5 {
        turning.ticks(20);
        still.ticks(20);
        assert!(
            on_a_rail(&turning, turning_ship) && on_a_rail(&still, still_ship),
            "second {second}: both are on rails"
        );
        let middle = size.map(|edge| edge as f64 / 2.0);
        let apart = length(common::minus(
            middle_of(&turning, turning_ship, middle),
            middle_of(&still, still_ship, middle),
        ));
        assert!(
            apart < 1e-2,
            "second {second}: the middles are {apart} voxels apart"
        );
        let (turning_speed, still_speed) = (
            turning
                .rig
                .server
                .world()
                .grids()
                .get(turning_ship)
                .unwrap()
                .linear_velocity,
            still
                .rig
                .server
                .world()
                .grids()
                .get(still_ship)
                .unwrap()
                .linear_velocity,
        );
        assert_eq!(
            turning_speed.map(f64::to_bits),
            still_speed.map(f64::to_bits),
            "second {second}: the rail gives the same speed"
        );
    }
    assert!(
        angle_between(facing, turn_of(&turning, turning_ship).facing) > 0.5,
        "the ship turned, or the test says nothing"
    );
    assert_eq!(
        bits_of(&turn_of(&still, still_ship))[..4],
        facing.components().map(f64::to_bits),
        "the one that does not turn faces as it did"
    );
}

/// **The turn is the same bits wherever the ship is and however fast it goes** (the rail frame holds the ship at rest at the rail's point, so its numbers are its own): a box
/// that is not a cube, spun about no axis of its own so that it wobbles, is turned just so at 70 km, at 400 km and at a speed a quarter again as high.
#[test]
fn the_turn_of_a_ship_on_a_rail_is_the_same_bits_wherever_it_is_and_however_fast_it_goes() {
    let facing = Orientation::from_yaw_pitch(0.4, 0.2);
    let size = [2, 3, 5];
    let spin = [0.2, 0.3, 0.1];
    let mut results = Vec::new();
    for (distance, factor) in [(DISTANCE, 1.0), (400_000.0, 1.0), (DISTANCE, 1.25)] {
        let (mut scene, ship) = a_ship_on_a_rail(distance, factor, size, facing, spin);
        scene.ticks(100);
        assert!(on_a_rail(&scene, ship), "{distance} at {factor}: a rail");
        results.push((distance, factor, turn_of(&scene, ship)));
    }
    let (_, _, first) = &results[0];
    let wobbled = length(common::minus(first.spin, spin));
    assert!(
        wobbled > 1e-3,
        "the spin of the box changed by {wobbled}, or it does not wobble and the test says nothing"
    );
    for (distance, factor, turn) in &results[1..] {
        assert_eq!(
            bits_of(turn),
            bits_of(first),
            "at {distance} voxels and {factor} times the speed of a circle the turn is not that at 70 km and at the speed of a circle"
        );
    }
}

/// **The turn is Rapier's**: a ship that wobbles keeps its angular momentum and its energy as well as Rapier's own step does. That step is explicit and of the first order, so a body
/// spun about no axis of its own loses or gains a little of its angular momentum: **measured on a bare `BodyWorld` with the same box and spin, 1.1e-3 of it over 100 ticks with
/// one substep and 2.8e-4 with four, halving as the step does** (the drift question 5 of the design left alone; a ship spun about an axis of its own has none), and 3.8e-4 here.
#[test]
fn a_ship_that_wobbles_on_a_rail_keeps_its_angular_momentum_and_its_energy_to_the_steps_own_error()
{
    let facing = Orientation::from_yaw_pitch(0.4, 0.2);
    let size = [2, 3, 5];
    let (mut scene, ship) = a_ship_on_a_rail(DISTANCE, 1.0, size, facing, [0.2, 0.3, 0.1]);
    scene.ticks(1);
    let started = turn_of(&scene, ship);
    let (momentum, energy) = momentum_and_energy(size, &started);
    for _ in 0..4 {
        scene.ticks(100);
        assert!(
            angle_between(started.facing, turn_of(&scene, ship).facing) > 0.5,
            "the ship turned, or the test says nothing"
        );
        let (now, now_energy) = momentum_and_energy(size, &turn_of(&scene, ship));
        let drift = length(common::minus(now, momentum)) / length(momentum);
        assert!(
            drift < 2e-3,
            "the angular momentum changed by {drift} of itself"
        );
        assert!(
            (now_energy / energy - 1.0).abs() < 2e-3,
            "the energy changed from {energy} to {now_energy}"
        );
    }
}

/// **The turn is Rapier's, the same as a free ship's**: the box that wobbles on a rail turns, to rounding (1e-14 radians over 80 ticks, measured), as the same box does in the
/// ordinary frame of the solver as a free ship at rest (the planet pulls it, which turns nothing): the rail frame takes nothing from the turn and gives it nothing.
#[test]
fn a_ship_that_wobbles_on_a_rail_turns_as_a_free_ship_does() {
    let facing = Orientation::from_yaw_pitch(0.4, 0.2);
    let size = [2, 3, 5];
    let spin = [0.2, 0.3, 0.1];
    let (mut on_a_rail, riding) = a_ship_on_a_rail(DISTANCE, 1.0, size, facing, spin);
    // The twin: the same ship, made in the same way, at rest and turning (it falls: a ship at rest has no orbit, so no rail takes it).
    let mut free = Scene::in_a_new_world();
    let middle = size.map(|edge| edge as f64 / 2.0);
    make_ship(
        &mut free,
        GridPose::new(
            WorldPosition::from_voxels([-middle[0], DISTANCE - middle[1], -middle[2]]),
            Orientation::IDENTITY,
        ),
        false,
        size,
        |_| true,
    );
    let twin = ship_id(1);
    free.run(move |world| {
        let position = world.data().grids().get(twin).unwrap().pose.position;
        world
            .set_grid_pose(twin, GridPose::new(position, facing))
            .unwrap();
        world.set_grid_velocity(twin, [0.0; 3], spin).unwrap();
    });
    // (80 ticks: the free ship falls, and at about 42 metres a second the solver cuts its tick into four steps and not three, which changes the last digits of what it turns. The ship on
    // a rail is at rest in its frame, so its steps are what they were at the start, whatever speed the rail gives it.)
    on_a_rail.ticks(80);
    free.ticks(80);
    assert!(on_a_rail_is(&on_a_rail, riding) && !on_a_rail_is(&free, twin));
    let (riding_turn, free_turn) = (turn_of(&on_a_rail, riding), turn_of(&free, twin));
    assert!(
        angle_between(facing, riding_turn.facing) > 0.5,
        "the ship turned, or the test says nothing"
    );
    let apart_in_facing = angle_between(riding_turn.facing, free_turn.facing);
    let apart_in_spin = length(common::minus(riding_turn.spin, free_turn.spin));
    assert!(
        apart_in_facing < 1e-12 && apart_in_spin < 1e-12,
        "the two turn differently: {apart_in_facing} radians apart in facing and {apart_in_spin} in spin"
    );
}

/// A ship that turns slowly is not put to sleep and stopped (the ship audit's PHY-2 was a free ship at 0.002 radians a second, dead in half a second): on a rail it goes on.
#[test]
fn a_ship_that_turns_slowly_on_a_rail_goes_on_turning() {
    let spin = [0.0, 0.002, 0.0];
    let (mut scene, ship) = a_ship_on_a_rail(DISTANCE, 1.0, [3, 3, 3], Orientation::IDENTITY, spin);
    let started = turn_of(&scene, ship);
    scene.ticks(100);
    let turn = turn_of(&scene, ship);
    for axis in 0..3 {
        assert!(
            (turn.spin[axis] - spin[axis]).abs() < 1e-12,
            "{:?} became {:?}",
            spin,
            turn.spin
        );
    }
    let turned_by = angle_between(started.facing, turn.facing);
    let expected = 0.002 * 100.0 * SECONDS_PER_TICK;
    assert!(
        (turned_by - expected).abs() < 1e-9,
        "it turned by {turned_by} and should have by {expected}"
    );
}

/// A frozen ship does not turn (the owner's answer 4 of step 4c-2: a ship no player is near is held by the bodies, and comes back not turning), and its rail goes on: it is where
/// the clock says, at the speed its rail gives it.
#[test]
fn a_frozen_ship_on_a_rail_stops_turning_and_goes_on_round() {
    let (mut scene, ship) = a_ship_on_a_rail(
        DISTANCE,
        1.0,
        [3, 3, 3],
        Orientation::IDENTITY,
        [0.0, 0.3, 0.1],
    );
    scene.ticks(10);
    let middle = [1.5; 3];
    let before = middle_of(&scene, ship, middle);
    scene.run(move |world| world.set_grid_frozen(ship, true).unwrap());
    let frozen_at = turn_of(&scene, ship);
    assert_eq!(frozen_at.spin, [0.0; 3], "freezing takes the turn");
    scene.ticks(10);
    let now = turn_of(&scene, ship);
    assert_eq!(
        bits_of(&now),
        bits_of(&frozen_at),
        "a frozen ship does not turn"
    );
    assert!(on_a_rail(&scene, ship), "and keeps its rail");
    assert!(
        length(common::minus(middle_of(&scene, ship, middle), before)) > 380.0,
        "which goes on: 11 ticks at about 38 voxels a tick"
    );
    scene.run(move |world| world.set_grid_frozen(ship, false).unwrap());
    scene.ticks(5);
    assert_eq!(
        turn_of(&scene, ship).spin,
        [0.0; 3],
        "it comes back not turning"
    );
}

/// A push takes a ship off its rail, as it did, and the turn goes on: the ship keeps the speed the rail gave it and the spin it had.
#[test]
fn a_push_takes_a_turning_ship_off_its_rail_and_the_turn_goes_on() {
    let spin = [0.0, 0.3, 0.1];
    let (mut scene, ship) = a_ship_on_a_rail(DISTANCE, 1.0, [3, 3, 3], Orientation::IDENTITY, spin);
    scene.ticks(30);
    assert!(on_a_rail(&scene, ship));
    let before = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship)
        .unwrap()
        .linear_velocity;
    scene.run(move |world| {
        world.apply_grid_force(ship, [0.0, 1.0, 0.0], None).unwrap();
    });
    // (Ten ticks: a ship that is left alone for a second goes on a rail again.)
    scene.ticks(10);
    assert!(
        !on_a_rail(&scene, ship),
        "a push takes the ship off its rail"
    );
    let found = scene.rig.server.world().grids().get(ship).unwrap();
    let speed = length(found.linear_velocity);
    assert!(
        (speed - length(before)).abs() < 0.01 * length(before),
        "the ship went at {} and goes at {speed}",
        length(before)
    );
    for axis in 0..3 {
        assert!(
            (found.angular_velocity[axis] - spin[axis]).abs() < 1e-6,
            "the turn went on: {:?} became {:?}",
            spin,
            found.angular_velocity
        );
    }
}

/// A ship on a rail that turns is saved and loaded as it is: a world that goes on and one that was saved and loaded are the same bit for bit after twenty ticks more.
#[test]
fn a_turning_ship_on_a_rail_that_is_saved_and_loaded_goes_on_as_if_it_had_not_been() {
    let facing = Orientation::from_yaw_pitch(0.4, 0.2);
    let spin = [0.2, 0.3, 0.1];
    let build = || a_ship_on_a_rail(DISTANCE, 1.0, [2, 3, 5], facing, spin);
    let (mut continued, ship) = build();
    continued.ticks(30);
    let (saved, _) = build();
    let mut saved = saved;
    saved.ticks(30);
    assert_eq!(
        bits_of(&turn_of(&continued, ship)),
        bits_of(&turn_of(&saved, ship)),
        "the two worlds are the same before the save"
    );
    let mut loaded = saved.restarted();
    continued.ticks(20);
    loaded.ticks(20);
    assert!(
        on_a_rail(&loaded, ship),
        "the ship is on its rail after it was loaded"
    );
    assert!(
        angle_between(facing, turn_of(&loaded, ship).facing) > 0.5,
        "the ship turned, or the test says nothing"
    );
    assert_eq!(
        bits_of(&turn_of(&loaded, ship)),
        bits_of(&turn_of(&continued, ship)),
        "the turn of the ship that was saved and loaded is that of the one that was not"
    );
    let middle = [1.0, 1.5, 2.5];
    let apart = length(common::minus(
        middle_of(&loaded, ship, middle),
        middle_of(&continued, ship, middle),
    ));
    assert!(apart < 1e-2, "the middles are {apart} voxels apart");
}

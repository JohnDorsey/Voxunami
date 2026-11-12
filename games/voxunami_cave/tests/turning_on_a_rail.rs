//! A ship that turns rides a rail (step 4c-2 of `docs/decisions/joints-and-cars.md`; the owner's answer 6 of the ship audit: rotation never keeps a ship off a rail).
//!
//! A rail holds the centre of mass of a ship and nothing else; **the turn is the bodies' work**, in a frame that goes with the rail's point: the ship is a free body there, at
//! rest at the point, with no pull on it, and what it does is turn as a free body turns (Rapier's own gyroscopic step). Each tick the rail puts the centre of mass where the
//! clock says and the bodies write back how the ship faces and how fast it turns, so a ship on a rail goes on turning, and its path is the one it would have if it did not.
//!
//! The ships here are put on their rails directly (`install_a_rail`: set turning first, the rail last, since moving or speeding a ship takes a coasting rail from it), so
//! that what they do is what a ship does on a rail from its first tick, and ships at different places and speeds can be compared bit for bit.

mod common;

use common::rails::{
    TICKS_PER_SECOND, install_a_rail, put_a_ship_on_a_rail, put_on_its_rail_in, set_going_in,
};
use common::{Scene, circular_speed, length, make_ship, ship_id};
use server::GridView;
use voxel_space::{GridId, GridPose, Orientation, VoxelCoordinate, WorldPosition};
use voxunami_cave::content::CaveContent;

/// The distance from the middle of the planet at which a ship is clear of its ground and may go on a rail (see `orbit.rs`).
const DISTANCE: f64 = 70_000.0;
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
    let ship = put_a_ship_on_a_rail(
        &mut scene,
        distance,
        speed_factor,
        size,
        |_| true,
        facing,
        spin,
    );
    (scene, ship)
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

/// The turn from one facing to another, in radians (whichever way round).
fn angle_between(first: Orientation, second: Orientation) -> f64 {
    let turn = second.compose(first.inverse()).components();
    2.0 * libm::atan2(length([turn[0], turn[1], turn[2]]), turn[3].abs())
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A vector turned by an orientation (the vector part `q` of a unit quaternion `(q, w)`: `v + 2 w (q x v) + 2 q x (q x v)`).
fn turned(by: Orientation, vector: [f64; 3]) -> [f64; 3] {
    let [x, y, z, w] = by.components();
    let q = [x, y, z];
    let first = cross(q, vector);
    let second = cross(q, first);
    [0, 1, 2].map(|axis| vector[axis] + 2.0 * w * first[axis] + 2.0 * second[axis])
}

/// The voxels of a box of this size from the ship's origin.
fn box_of(size: [i128; 3]) -> Vec<[i128; 3]> {
    let mut voxels = Vec::new();
    for x in 0..size[0] {
        for y in 0..size[1] {
            for z in 0..size[2] {
                voxels.push([x, y, z]);
            }
        }
    }
    voxels
}

/// The centre of mass, in the ship's own coordinates, of stone in these voxels (they weigh alike: the mass of one is 1), and the tensor of inertia about it in the ship's own axes: a
/// voxel is a unit cube, which has 1/6 on each axis about its own middle, and the rest is the term of the parallel axis (the square of the arm on the diagonal, less the arm times itself).
fn centre_and_tensor(voxels: &[[i128; 3]]) -> ([f64; 3], [[f64; 3]; 3]) {
    let middles: Vec<[f64; 3]> = voxels
        .iter()
        .map(|voxel| voxel.map(|part| part as f64 + 0.5))
        .collect();
    let count = middles.len() as f64;
    let centre =
        [0, 1, 2].map(|axis| middles.iter().map(|middle| middle[axis]).sum::<f64>() / count);
    let mut tensor = [[0.0; 3]; 3];
    for middle in &middles {
        let arm = [0, 1, 2].map(|axis| middle[axis] - centre[axis]);
        let square: f64 = arm.iter().map(|part| part * part).sum();
        for (row, line) in tensor.iter_mut().enumerate() {
            for (column, entry) in line.iter_mut().enumerate() {
                let own = if row == column {
                    1.0 / 6.0 + square
                } else {
                    0.0
                };
                *entry += own - arm[row] * arm[column];
            }
        }
    }
    (centre, tensor)
}

/// The angular momentum (in the world's axes) and the energy of a body of this tensor of inertia (in its own axes) that turns as `turn` says.
fn momentum_and_energy_in(tensor: &[[f64; 3]; 3], turn: &Turn) -> ([f64; 3], f64) {
    let own_spin = turned(turn.facing.inverse(), turn.spin);
    let own_momentum =
        tensor.map(|line| (0..3).map(|axis| line[axis] * own_spin[axis]).sum::<f64>());
    let energy = 0.5
        * (0..3)
            .map(|axis| own_momentum[axis] * own_spin[axis])
            .sum::<f64>();
    (turned(turn.facing, own_momentum), energy)
}

/// The angular momentum (in the world's axes) and the energy of a box of stone of unit voxel mass that turns as `turn` says.
fn momentum_and_energy(size: [i128; 3], turn: &Turn) -> ([f64; 3], f64) {
    momentum_and_energy_in(&centre_and_tensor(&box_of(size)).1, turn)
}

/// The weight of the ship, in the unit the server has (kilograms), and where its centre of mass is in its own coordinates, as the server has them.
fn weight_and_centre(scene: &Scene, ship: GridId) -> (f64, [f64; 3]) {
    let ship = scene.rig.server.world().grids().get(ship).unwrap();
    (ship.mass, ship.centre_of_mass)
}

/// The velocity on record of a ship: its centre of mass's, in the world's axes.
fn velocity_of(scene: &Scene, ship: GridId) -> [f64; 3] {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship)
        .unwrap()
        .linear_velocity
}

/// Places stone in the voxels `placed` of a ship and breaks the blocks in the voxels `broken`, in the tick it takes (what a player's edits come to in the server).
fn edit(scene: &mut Scene, ship: GridId, placed: Vec<[i128; 3]>, broken: Vec<[i128; 3]>) {
    let content = CaveContent::new();
    scene.run(move |world| {
        let mut view = world.grid(ship).unwrap();
        for at in placed {
            let at = VoxelCoordinate::new(at[0], at[1], at[2]);
            view.set(content.block_channel(), at, content.blocks.stone)
                .unwrap();
        }
        for at in broken {
            let at = VoxelCoordinate::new(at[0], at[1], at[2]);
            view.set(content.block_channel(), at, 0).unwrap();
        }
    });
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
    let (mut rider, riding) = a_ship_on_a_rail(DISTANCE, 1.0, size, facing, spin);
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
    rider.ticks(80);
    free.ticks(80);
    assert!(on_a_rail(&rider, riding) && !on_a_rail(&free, twin));
    let (riding_turn, free_turn) = (turn_of(&rider, riding), turn_of(&free, twin));
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
    let (mut saved, _) = build();
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

// ---- What a ship that turns on a rail meets: edits to its blocks, a cut, a blow, a neighbour, and many of its kind (found untested by `ship-rail-frames-review.md`, 8.3) ----

/// A box of stone 2 by 3 by 5: not a cube, so that a spin about no axis of its own makes it wobble.
const BOX: [i128; 3] = [2, 3, 5];
const WOBBLE: [f64; 3] = [0.2, 0.3, 0.1];

fn tilted() -> Orientation {
    Orientation::from_yaw_pitch(0.4, 0.2)
}

/// The tensor of inertia that `centre_and_tensor` works out is that of the box formula (the mass of a box of unit voxels is its volume, and an axis through its middle has a twelfth of
/// the mass times the squares of the other two edges): the test of the helper the tests of the edits below measure with.
#[test]
fn the_tensor_that_the_tests_use_is_the_one_of_a_box() {
    let (centre, tensor) = centre_and_tensor(&box_of(BOX));
    let [a, b, c] = BOX.map(|edge| edge as f64);
    let volume = a * b * c;
    assert_eq!(centre, [1.0, 1.5, 2.5]);
    let expected = [
        volume * (b * b + c * c) / 12.0,
        volume * (a * a + c * c) / 12.0,
        volume * (a * a + b * b) / 12.0,
    ];
    for (row, line) in tensor.iter().enumerate() {
        for (column, entry) in line.iter().enumerate() {
            let wanted = if row == column { expected[row] } else { 0.0 };
            assert!(
                (entry - wanted).abs() < 1e-9,
                "[{row}][{column}] is {entry} and should be {wanted}"
            );
        }
    }
}

/// Two worlds with the same box on the same rail, turning the same way, 30 ticks old, and then in the first stone is placed in the voxels `placed` and the blocks in the voxels
/// `broken` are broken (in the tick that takes, in which the other goes on). The first, the second, and the ship (it is number 1 in both).
fn edited_and_untouched(placed: &[[i128; 3]], broken: &[[i128; 3]]) -> (Scene, Scene, GridId) {
    let (mut edited, ship) = a_ship_on_a_rail(DISTANCE, 1.0, BOX, tilted(), WOBBLE);
    let (mut untouched, _) = a_ship_on_a_rail(DISTANCE, 1.0, BOX, tilted(), WOBBLE);
    edited.ticks(30);
    untouched.ticks(30);
    edit(&mut edited, ship, placed.to_vec(), broken.to_vec());
    untouched.ticks(1);
    (edited, untouched, ship)
}

/// An edit of the box: the voxels where stone is placed and those where a block is broken.
struct Edit {
    name: &'static str,
    placed: Vec<[i128; 3]>,
    broken: Vec<[i128; 3]>,
}

/// The edits tried: stone placed on a side (four blocks in a bar) and a block broken from a corner.
fn edits() -> [Edit; 2] {
    [
        Edit {
            name: "four blocks placed",
            placed: (2..6).map(|x| [x, 1, 2]).collect(),
            broken: vec![],
        },
        Edit {
            name: "a corner broken",
            placed: vec![],
            broken: vec![[0, 0, 0]],
        },
    ]
}

/// **A block placed on a ship that turns on a rail, or broken from it, changes how it is weighed and nothing else**: it stays on its rail, its centre of mass goes on along the path of
/// the rail (that of a twin that was not touched), its turn goes on from where it was, and from then on it turns as a body of the new shape does (its angular momentum and its energy,
/// worked out with the new tensor, are kept as well as the step keeps them).
#[test]
fn blocks_placed_on_or_broken_from_a_ship_that_turns_on_a_rail_change_how_it_is_weighed_and_nothing_else()
 {
    for Edit {
        name,
        placed,
        broken,
    } in edits()
    {
        let (mut edited, mut untouched, ship) = edited_and_untouched(&placed, &broken);
        let after: Vec<[i128; 3]> = box_of(BOX)
            .into_iter()
            .filter(|voxel| !broken.contains(voxel))
            .chain(placed.iter().copied())
            .collect();
        // Weighed: the weight is that of the blocks it has now, and the centre of mass is theirs.
        let (weight, centre) = weight_and_centre(&edited, ship);
        let (old_weight, old_centre) = weight_and_centre(&untouched, ship);
        let (expected_centre, tensor) = centre_and_tensor(&after);
        let expected_weight = old_weight * after.len() as f64 / box_of(BOX).len() as f64;
        assert!(
            (weight / expected_weight - 1.0).abs() < 1e-9,
            "{name}: weighs {weight} and should weigh {expected_weight}"
        );
        assert!(
            length(common::minus(centre, expected_centre)) < 1e-6,
            "{name}: the centre of mass is {centre:?} and should be {expected_centre:?}"
        );
        assert_eq!(old_centre, [1.0, 1.5, 2.5], "{name}: the twin is as it was");
        // It is on its rail, and its turn went on from where it was: the same as the twin's, to what the new weighing makes of one tick (measured: 6e-5 radians and 2e-3 radians a
        // second for the bar, 4e-6 and 2e-4 for the corner; a turn that was lost or begun again would be 0.4 radians a second off).
        assert!(on_a_rail(&edited, ship), "{name}: on its rail");
        let (turn, twin_turn) = (turn_of(&edited, ship), turn_of(&untouched, ship));
        let facing_apart = angle_between(turn.facing, twin_turn.facing);
        let spin_apart = length(common::minus(turn.spin, twin_turn.spin));
        assert!(
            facing_apart < 1e-3 && spin_apart < 5e-3,
            "{name}: the turn is {facing_apart} radians and {spin_apart} radians a second from the twin's"
        );
        // Along the path of the rail: the centre of mass is where the twin's is (each in its own ship), tick after tick. (Measured: 7e-15 voxels, which is no distance at all at this
        // size of number: the rail puts the centre, and what the edit changed is where the origin is from it.)
        let mut most_apart: f64 = 0.0;
        for tick in 0..60 {
            edited.ticks(1);
            untouched.ticks(1);
            assert!(
                on_a_rail(&edited, ship),
                "{name}: on its rail at tick {tick}"
            );
            let (_, centre) = weight_and_centre(&edited, ship);
            let apart = length(common::minus(
                middle_of(&edited, ship, centre),
                middle_of(&untouched, ship, old_centre),
            ));
            most_apart = most_apart.max(apart);
        }
        assert!(
            most_apart < 1e-9,
            "{name}: the centre of mass left the path of the rail by {most_apart} voxels"
        );
        // It turns as a body of its new shape does. (Measured: the angular momentum drifts by 1e-4 to 4e-4 of itself, and the energy by 6e-5 at most, in 300 ticks, as in the box above.
        // A body that is weighed once and not again, so that an edit does not reach it, was measured drifting by 0.5 of itself for the bar: the new tensor is not what it turns by.)
        let (momentum, energy) = momentum_and_energy_in(&tensor, &turn_of(&edited, ship));
        for _ in 0..3 {
            edited.ticks(100);
            assert!(on_a_rail(&edited, ship), "{name}: still on its rail");
            let (now, now_energy) = momentum_and_energy_in(&tensor, &turn_of(&edited, ship));
            let drift = length(common::minus(now, momentum)) / length(momentum);
            assert!(
                drift < 2e-3,
                "{name}: the angular momentum changed by {drift} of itself"
            );
            assert!(
                (now_energy / energy - 1.0).abs() < 2e-3,
                "{name}: the energy changed from {energy} to {now_energy}"
            );
        }
    }
}

/// **The step, as the owner kept it (2026-11-10; `ship-centre-of-mass-edit-options.md`, "what happens now")**: a rail holds the centre of mass, so when an edit moves the centre in the
/// ship the centre stays on its path and **every block of the ship moves by the same shift, turned with the ship, in the tick of the edit**. This is today's behaviour and the owner's choice for
/// now: whoever builds option A (fit the rail at the edit) changes this test on purpose.
#[test]
fn an_edit_steps_the_whole_ship_by_the_shift_of_its_centre_of_mass_in_the_tick_of_the_edit() {
    for Edit {
        name,
        placed,
        broken,
    } in edits()
    {
        let (edited, untouched, ship) = edited_and_untouched(&placed, &broken);
        let ((_, new_centre), (_, old_centre)) = (
            weight_and_centre(&edited, ship),
            weight_and_centre(&untouched, ship),
        );
        let shift = common::minus(new_centre, old_centre);
        assert!(
            length(shift) > 0.05,
            "{name}: the centre moved by {shift:?}"
        );
        let origin = |scene: &Scene| {
            scene
                .rig
                .server
                .world()
                .grids()
                .get(ship)
                .unwrap()
                .pose
                .position
        };
        let stepped = origin(&untouched)
            .displacement_to(origin(&edited))
            .in_voxels();
        // (The origin is the rail's point less the facing applied to the centre of mass: it moves by minus the shift, turned.)
        let expected = turned(turn_of(&edited, ship).facing, shift).map(|part| -part);
        // (Measured: right to 1.4e-4 voxels for a shift of 0.35, and to 1.4e-5 for one of 0.08; the rest is the turn of the twin, which has had a tick of the old weighing.)
        let wrong = length(common::minus(stepped, expected));
        assert!(
            wrong < 1e-3,
            "{name}: the origin of the ship stepped by {stepped:?} and should have by {expected:?}"
        );
    }
}

/// The ships of a scene (the planets are not).
fn ships_in(scene: &Scene) -> Vec<GridId> {
    scene
        .rig
        .server
        .world()
        .grids()
        .iter()
        .filter(|(_, ship)| ship.terrain.is_none())
        .map(|(id, _)| id)
        .collect()
}

/// **A ship that turns on a rail and is cut in two by an edit gives each part the speed its own centre had** (the speed of the ship's centre of mass, which is the rail's, and what the
/// turn adds there: omega crossed with the arm), and the turn it had; the parts are no one's rail's to begin with, and the frame of the rider is gone with it.
#[test]
fn a_ship_that_turns_on_a_rail_and_is_cut_in_two_gives_each_part_the_speed_its_own_centre_had() {
    // Two blocks of 3 by 3 by 3 with a bar one block thick between them along x (from x = 3 to x = 8).
    let size = [12, 3, 3];
    let solid =
        |voxel: [i128; 3]| voxel[0] < 3 || voxel[0] >= 9 || (voxel[1] == 1 && voxel[2] == 1);
    let spin = [0.1, 0.3, 0.2];
    let (mut scene, frames) = Scene::reporting_in_a_new_world();
    let ship = put_a_ship_on_a_rail(&mut scene, DISTANCE, 1.0, size, solid, tilted(), spin);
    scene.ticks(30);
    assert!(on_a_rail(&scene, ship));
    assert_eq!(
        frames().frames,
        1,
        "the ship that turns is in a frame of its own"
    );
    let whole: Vec<[i128; 3]> = box_of(size).into_iter().filter(|v| solid(*v)).collect();
    let state_of = |scene: &Scene| {
        let found = scene.rig.server.world().grids().get(ship).unwrap();
        (
            found.pose.orientation,
            found.angular_velocity,
            found.linear_velocity,
        )
    };
    // What the cut sees of the ship: how it stood at the end of the tick before the one in which it is cut.
    let mut seen = state_of(&scene);
    // The bar is cut in the middle. (The look at the blocks beside the cut is done with a little work in each tick: it may take more than the one.)
    edit(&mut scene, ship, vec![], vec![[6, 1, 1]]);
    let mut parts = ships_in(&scene);
    for _ in 0..10 {
        if parts.len() == 2 {
            break;
        }
        seen = state_of(&scene);
        scene.ticks(1);
        parts = ships_in(&scene);
    }
    let (seen_facing, seen_spin, seen_velocity) = seen;
    assert_eq!(parts.len(), 2, "the ship is two ships: {parts:?}");
    assert!(
        !parts.contains(&ship),
        "neither keeps the number of the ship"
    );
    // The left part is the one whose centre is the lower along x.
    parts.sort_by(|first, second| {
        weight_and_centre(&scene, *first).1[0].total_cmp(&weight_and_centre(&scene, *second).1[0])
    });
    let (left, right) = (parts[0], parts[1]);
    let left_blocks: Vec<[i128; 3]> = whole.iter().copied().filter(|v| v[0] < 6).collect();
    let right_blocks: Vec<[i128; 3]> = whole.iter().copied().filter(|v| v[0] > 6).collect();
    let mut velocities = Vec::new();
    for (part, blocks) in [(left, &left_blocks), (right, &right_blocks)] {
        let (_, centre) = weight_and_centre(&scene, part);
        let (expected_centre, _) = centre_and_tensor(blocks);
        assert!(
            length(common::minus(centre, expected_centre)) < 1e-6,
            "the centre of mass of a part is {centre:?} and should be {expected_centre:?}"
        );
        let found = scene.rig.server.world().grids().get(part).unwrap();
        assert!(found.rail.is_none(), "a part is on no rail");
        assert!(
            length(common::minus(found.angular_velocity, seen_spin)) < 1e-2,
            "a part turns as the ship did: {:?} and {seen_spin:?}",
            found.angular_velocity
        );
        velocities.push((found.linear_velocity, centre, blocks.len() as f64));
    }
    // One moves relative to the other as the turn of the ship says: omega crossed with the arm between their centres, turned with the ship. (The pull is the same on both, to
    // a hundredth: the difference is the turn.)
    let arm = turned(seen_facing, common::minus(velocities[0].1, velocities[1].1));
    let expected = cross(seen_spin, arm);
    let relative = common::minus(velocities[0].0, velocities[1].0);
    let wrong = length(common::minus(relative, expected));
    // (Measured: the two differ by 6e-5 metres a second, of 3.2 in all.)
    assert!(
        wrong < 1e-3,
        "the parts move apart at {relative:?} and the turn says {expected:?}"
    );
    // And together they go as the ship did: the weighted mean of their speeds is the speed of the ship's centre of mass, the rail's, to two ticks of the pull (measured 0.83, which is two
    // times 0.41 metres a second: the rail's own speed turns by that much in the tick the cut is made, before the cut, and the parts have had the pull of that tick).
    let weight = velocities[0].2 + velocities[1].2;
    let mean = [0, 1, 2].map(|axis| {
        (velocities[0].0[axis] * velocities[0].2 + velocities[1].0[axis] * velocities[1].2) / weight
    });
    let off = length(common::minus(mean, seen_velocity));
    assert!(
        off < 1.0,
        "the parts go at {mean:?} and the ship went at {seen_velocity:?}"
    );
    scene.ticks(40);
    assert_eq!(ships_in(&scene).len(), 2);
    assert_eq!(
        frames().frames,
        1,
        "the parts are in one frame, and the rider's is gone"
    );
}

/// **A blow off the centre sets a ship on a rail tumbling, and the rail takes it again, still tumbling**: the blow pushes the ship and turns it (as much as a push and a torque of that size
/// should, worked out here from the weight and the tensor), takes it off its rail, and after its second of quiet it is on a rail again, with the turn it had. Nothing jumps where the rail takes
/// it (not its path, not its turn), and from the blow on, its angular momentum and its energy are kept as well as the step keeps them. Once from rest and once from a turn.
#[test]
fn a_blow_off_the_centre_sets_a_ship_on_a_rail_tumbling_and_it_is_put_on_a_rail_again_still_tumbling()
 {
    let (centre, tensor) = centre_and_tensor(&box_of(BOX));
    for (name, facing, spin) in [
        ("from rest", Orientation::IDENTITY, [0.0; 3]),
        ("from a turn", tilted(), WOBBLE),
    ] {
        let (mut scene, ship) = a_ship_on_a_rail(DISTANCE, 1.0, BOX, facing, spin);
        scene.ticks(30);
        assert!(on_a_rail(&scene, ship), "{name}: a rail to begin with");
        let (weight, _) = weight_and_centre(&scene, ship);
        let (speed, started) = (velocity_of(&scene, ship), turn_of(&scene, ship));
        // The blow is along z (the rail goes in a plane that has no z in it, so nothing else changes that speed), at a place of the ship 1 voxel off its centre along x and 1.5 along y.
        let force = [0.0, 0.0, 250_000.0];
        let at = [centre[0] + 1.0, centre[1] + 1.5, centre[2]];
        scene.run(move |world| world.apply_grid_force(ship, force, Some(at)).unwrap());
        assert!(
            !on_a_rail(&scene, ship),
            "{name}: a blow takes the ship off its rail"
        );
        let (blown, sped) = (turn_of(&scene, ship), velocity_of(&scene, ship));
        let gained = sped[2] - speed[2];
        let expected_gain = force[2] * SECONDS_PER_TICK / weight;
        // The push: a twentieth of a second of the force on the weight of the ship. The turn: the same on the tensor (a voxel of stone weighs the weight over the number of voxels).
        let voxel_weight = weight / box_of(BOX).len() as f64;
        let torque = cross(turned(started.facing, common::minus(at, centre)), force);
        let own_torque = turned(started.facing.inverse(), torque);
        let own_change = [0, 1, 2]
            .map(|axis| own_torque[axis] * SECONDS_PER_TICK / (voxel_weight * tensor[axis][axis]));
        let expected_change = turned(started.facing, own_change);
        let change = common::minus(blown.spin, started.spin);
        // (Measured: the push is right to 1e-15 of 0.42; from rest the spin is right to 4e-4 radians a second of 0.28, the rest being the wobble of that tick; from a turn it is 1 percent off, since the ship turns in the tick of the blow.)
        assert!(
            (gained - expected_gain).abs() < 0.01 * expected_gain,
            "{name}: the push gave {gained} and should have given {expected_gain}"
        );
        assert!(
            length(common::minus(change, expected_change)) < 0.05 * length(expected_change),
            "{name}: the spin changed by {change:?} and should have by {expected_change:?}"
        );
        // From the blow to a rail again, tick by tick.
        let mut path = vec![middle_of(&scene, ship, centre)];
        let mut turns = vec![blown];
        let mut railed_at = None;
        for tick in 1..=60 {
            scene.ticks(1);
            path.push(middle_of(&scene, ship, centre));
            turns.push(turn_of(&scene, ship));
            if on_a_rail(&scene, ship) {
                railed_at = Some(tick);
                break;
            }
        }
        // (Measured: 20 ticks, the second of quiet.)
        railed_at
            .unwrap_or_else(|| panic!("{name}: the ship was not put on a rail again in 60 ticks"));
        for _ in 0..100 {
            scene.ticks(1);
            path.push(middle_of(&scene, ship, centre));
            turns.push(turn_of(&scene, ship));
        }
        assert!(on_a_rail(&scene, ship), "{name}: it keeps the rail");
        // Where it was put on the rail nothing jumps: not the path of its centre (a kink is the change of its step from one tick to the next; a circle of this size has 0.0207 of it,
        // the speed squared over the radius times the tick squared, and 0.0207 is the most that was measured, in the free flight, at the rail and after it), and not the turn (each tick turns it by its spin for a tick, give or take the wobble in
        // that tick: measured 1e-4 and 3e-4 of itself at worst).
        let kink = common::greatest_kink(&path[1..]);
        assert!(
            kink < 0.03,
            "{name}: the path of the centre has a kink of {kink}"
        );
        let mut worst: f64 = 0.0;
        for pair in turns.windows(2) {
            let turned_by = angle_between(pair[0].facing, pair[1].facing);
            let expected = length(pair[0].spin) * SECONDS_PER_TICK;
            worst = worst.max((turned_by / expected - 1.0).abs());
        }
        assert!(
            worst < 2e-3,
            "{name}: a tick turned the ship by {worst} of what its spin says too much or too little"
        );
        let (momentum, energy) = momentum_and_energy_in(&tensor, &turns[0]);
        assert!(
            length(change) > 0.05,
            "{name}: the blow set the ship turning, or the test says nothing"
        );
        let mut drift: f64 = 0.0;
        let mut energy_change: f64 = 0.0;
        for turn in &turns {
            let (now, now_energy) = momentum_and_energy_in(&tensor, turn);
            drift = drift.max(length(common::minus(now, momentum)) / length(momentum));
            energy_change = energy_change.max((now_energy / energy - 1.0).abs());
        }
        // (Measured over the free flight, the hand-on and 100 ticks on the rail: the momentum drifts by 2e-4 and 6e-4, the energy by 1e-5 and 8e-4. The free ship's tick is cut into more steps than the
        // rider's (`steps_for_this_tick` goes by the speed of the body), so this is two sizes of step.)
        assert!(
            drift < 2e-3,
            "{name}: the angular momentum changed by {drift} of itself"
        );
        assert!(
            energy_change < 2e-3,
            "{name}: the energy changed by {energy_change} of itself"
        );
    }
}

/// **A rider is taken off its rail by a ship that comes near, and meets it as a body in the same frame.** A rider is alone in its frame, so nothing there could meet it: the world takes the rail
/// from a ship when something is within a margin of it, and the ship is the solver's again, in the frame of whatever is near. Ship A (a cube, turning, on a rail) has ship B put 40 voxels
/// ahead of it, going 20 metres a second slower. A loses its rail in that tick and keeps its turn; the two then meet, and B does not go through A. (A ship that is slower falls to a lower orbit, and the slower it is the longer it has to fall
/// before they meet: at 5 metres a second B was measured passing 6.5 voxels from A without touching it, so the test would have said nothing.)
#[test]
fn a_ship_that_turns_on_a_rail_is_taken_off_it_by_a_ship_that_comes_near_and_the_two_meet_as_bodies()
 {
    let mut scene = Scene::in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    let (a, b) = (ship_id(1), ship_id(2));
    let spin = [0.0, 0.3, 0.1];
    // B starts 300 voxels ahead of A, going the same way at the same speed, so that A is alone (the margin to go on a rail is twice the margin to stay on one).
    for ahead in [0.0, 300.0] {
        make_ship(
            &mut scene,
            GridPose::new(
                WorldPosition::from_voxels([-1.5 + ahead, DISTANCE - 1.5, -1.5]),
                Orientation::IDENTITY,
            ),
            false,
            [3, 3, 3],
            |_| true,
        );
    }
    scene.run(move |world| {
        world
            .set_grid_velocity(b, [speed, 0.0, 0.0], [0.0; 3])
            .unwrap()
    });
    install_a_rail(&mut scene, a, speed, tilted(), spin);
    scene.ticks(30);
    assert!(on_a_rail(&scene, a), "A is on its rail");
    assert!(on_a_rail(&scene, b), "so is B, which does not turn");
    // B is put 40 voxels ahead of A along A's way, going 20 metres a second slower than A (which takes B's rail from it).
    scene.run(move |world| {
        let ship = world.data().grids().get(a).unwrap();
        let (place, velocity) = (ship.pose.position, ship.linear_velocity);
        let along = velocity.map(|part| part / length(velocity));
        world
            .set_grid_pose(
                b,
                GridPose::new(
                    place.translated(along.map(|part| 40.0 * part)),
                    Orientation::IDENTITY,
                ),
            )
            .unwrap();
        world
            .set_grid_velocity(
                b,
                [0, 1, 2].map(|axis| velocity[axis] - 20.0 * along[axis]),
                [0.0; 3],
            )
            .unwrap();
    });
    scene.ticks(1);
    assert!(!on_a_rail(&scene, a), "B is near: A is taken off its rail");
    let found = turn_of(&scene, a);
    assert!(
        length(common::minus(found.spin, spin)) < 1e-9,
        "A turns as it did: {:?}",
        found.spin
    );
    // The two come together at 20 metres a second: 37 voxels between them, so they meet in 37 ticks. A tick at a time to where they meet, and well past it. Their middles are 3 voxels apart or more when two cubes
    // of 3 touch, whatever way they are turned.
    let mut closest = f64::MAX;
    for _ in 0..100 {
        scene.ticks(1);
        let apart = common::minus(
            middle_of(&scene, a, [1.5; 3]),
            middle_of(&scene, b, [1.5; 3]),
        );
        closest = closest.min(length(apart));
    }
    let last_spin = turn_of(&scene, a).spin;
    // (Measured: the middles came to 4.19 voxels, which fits a corner of A (up to 2.6 from its middle) against a face of B (1.5); A's spin became (0.9, 1.2, -3.1) from (0, 0.3, 0.1); B and A closed at
    // 7.5 metres a second at the end, from 20.)
    assert!(
        length(common::minus(last_spin, spin)) > 0.5,
        "they never touched: A turns as it did, {last_spin:?}"
    );
    assert!(
        closest > 2.9,
        "B went through A (the middles were {closest} voxels apart at the closest)"
    );
    assert!(
        length(common::minus(
            velocity_of(&scene, b),
            velocity_of(&scene, a)
        )) < 15.0,
        "after they met they no longer closed at 20 metres a second (7.5 measured)"
    );
}

/// The kinds of ship the many riders are: a size, and how it turns (a kind each tilts differently).
fn kind_of_rider(index: usize) -> ([i128; 3], Orientation, [f64; 3]) {
    let kinds: [([i128; 3], [f64; 3]); 4] = [
        ([2, 3, 5], [0.2, 0.3, 0.1]),
        ([3, 3, 3], [0.0, 0.3, 0.1]),
        ([1, 4, 6], [0.3, -0.2, 0.15]),
        ([2, 2, 7], [-0.1, 0.1, 0.4]),
    ];
    let (size, spin) = kinds[index % kinds.len()];
    (
        size,
        Orientation::from_yaw_pitch(0.4 + 0.1 * (index % kinds.len()) as f64, 0.2),
        spin,
    )
}

/// **Many ships that turn on rails, in one world, each turn exactly as one alone does**, and each has a frame of its own (a rider is alone in its frame: nothing in the frame of another can
/// touch it). Forty of four kinds, at distances 400 voxels apart from the planet, all put on their rails in one tick; after 60 ticks the turn of each is, bit for bit, that of a ship of its kind
/// in a world of its own.
#[test]
fn many_ships_that_turn_on_rails_each_turn_as_one_alone_does_each_in_a_frame_of_its_own() {
    const RIDERS: usize = 40;
    let alone: Vec<Vec<u64>> = (0..4)
        .map(|index| {
            let (size, facing, spin) = kind_of_rider(index);
            let (mut scene, ship) = a_ship_on_a_rail(DISTANCE, 1.0, size, facing, spin);
            scene.ticks(60);
            bits_of(&turn_of(&scene, ship))
        })
        .collect();
    let (mut scene, frames) = Scene::reporting_in_a_new_world();
    let distance_of = |rider: usize| DISTANCE + 400.0 * rider as f64;
    for rider in 0..RIDERS {
        let (size, _, _) = kind_of_rider(rider);
        let middle = size.map(|edge| edge as f64 / 2.0);
        make_ship(
            &mut scene,
            GridPose::new(
                WorldPosition::from_voxels([
                    -middle[0],
                    distance_of(rider) - middle[1],
                    -middle[2],
                ]),
                Orientation::IDENTITY,
            ),
            false,
            size,
            |_| true,
        );
    }
    let speeds: Vec<f64> = (0..RIDERS)
        .map(|rider| circular_speed(&scene, 0, distance_of(rider)))
        .collect();
    scene.run(move |world| {
        for (rider, speed) in speeds.iter().enumerate() {
            let (_, facing, spin) = kind_of_rider(rider);
            set_going_in(world, ship_id(1 + rider as u64), *speed, facing, spin);
        }
        for rider in 0..RIDERS {
            put_on_its_rail_in(world, ship_id(1 + rider as u64));
        }
    });
    scene.ticks(60);
    assert_eq!(
        frames().frames,
        RIDERS as u64,
        "every rider has a frame of its own, and nothing else has one"
    );
    for rider in 0..RIDERS {
        let ship = ship_id(1 + rider as u64);
        assert!(on_a_rail(&scene, ship), "rider {rider} is on its rail");
        assert_eq!(
            bits_of(&turn_of(&scene, ship)),
            alone[rider % 4],
            "rider {rider} (kind {}) does not turn as one of its kind alone does",
            rider % 4
        );
    }
}

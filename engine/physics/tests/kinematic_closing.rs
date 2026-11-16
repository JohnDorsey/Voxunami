//! A body that is in the way of a fast one is in the way whichever of the two the frame says is the one that moves (step 4c-5-3 of `docs/decisions/joints-and-cars.md`).
//!
//! The world cuts a tick into more solver steps when a dynamic body is fast, so that it does not cross more than a step's worth of ground in one of them. A frame that goes
//! with a group in free fall has the group at rest in it and the ground of a planet **comes at it** as a kinematic body, at the speed the group closes on the ground: nothing
//! in the frame is fast but that body, and it used to count for nothing.

mod block_bodies;

use block_bodies::{Block, add_block, block_mass, free_space, velocity};
use physics::{BodyId, BodyKind, BodyState, BodyWorld};
use voxel_space::{BrickCoordinate, Orientation, WorldPosition};

/// A slab of ground, one brick of stone (32 voxels thick), its top at height 0, going at `velocity`, of this kind.
fn add_slab(world: &mut BodyWorld, kind: BodyKind, velocity: [f64; 3]) {
    let state = BodyState {
        position: WorldPosition::from_voxels([0.0, -32.0, 0.0]),
        orientation: Orientation::IDENTITY,
        linear_velocity: velocity,
        angular_velocity: [0.0; 3],
    };
    world.add_body(BodyId(1), state, kind).unwrap();
    world
        .set_brick(BodyId(1), BrickCoordinate::ORIGIN, &mut Block(32))
        .unwrap();
    world.set_mass(BodyId(1), block_mass(32)).unwrap();
}

/// A cube of two voxels, 3 voxels over the slab, going `against` the slab at `speed` (down, with the ground standing still; at rest, with the ground coming up at it), and what
/// each of the two is doing after `ticks` ticks: the cube's height over the slab's top, and its speed against the slab along y.
fn meet(slab_moves: bool, speed: f64, ticks: usize) -> (f64, f64) {
    let mut world = free_space(3);
    if slab_moves {
        add_slab(&mut world, BodyKind::Kinematic, [0.0, speed, 0.0]);
        add_block(&mut world, 2, 2, [10.0, 3.0, 10.0], [0.0; 3]);
    } else {
        add_slab(&mut world, BodyKind::Fixed, [0.0; 3]);
        add_block(&mut world, 2, 2, [10.0, 3.0, 10.0], [0.0, -speed, 0.0]);
    }
    for _ in 0..ticks {
        world.step();
    }
    let slab = world.state(BodyId(1)).unwrap();
    let cube = world.state(BodyId(2)).unwrap();
    let height = cube.position.relative_to(slab.position)[1] - 32.0;
    (height, velocity(&world, 2)[1] - slab.linear_velocity[1])
}

#[test]
fn a_slab_that_comes_at_a_resting_cube_is_met_by_it_as_a_cube_that_goes_at_the_slab_is() {
    // The cube lands on the slab either way: it ends on the top of it (the height over the top is nothing, to a few thousandths of a voxel) and goes with it (no speed against it).
    // (Ran, before the world counted the speed of a slab: the cube that goes at the slab lands at every speed to 1,200 voxels a second, and a slab that comes at it passed through it
    // from 300 on, to 447 voxels below the top at 300, 897 at 600, 1,797 at 1,200, with the speed of the cube against the slab minus all of the slab's.)
    for speed in [50.0, 150.0, 300.0, 600.0, 1200.0] {
        for slab_moves in [false, true] {
            let (height, against) = meet(slab_moves, speed, 30);
            assert!(
                height.abs() < 0.05 && against.abs() < 0.05,
                "at {speed} voxels a second, {}: the cube is {height} voxels over the top of the slab and goes {against} against it",
                if slab_moves {
                    "the slab comes at the cube"
                } else {
                    "the cube goes at the slab"
                }
            );
        }
    }
}

#[test]
fn a_slab_that_comes_at_a_cube_cuts_the_tick_into_the_steps_that_the_closing_speed_asks_for() {
    // The steps of a tick are as many as it takes for the fastest thing to cross 0.8 voxels in none of them: ceil(speed * 0.05 / 0.8), from 3 to 24. The cube is at rest, so
    // what is fast is the slab, which the cube closes on.
    for (speed, steps) in [(30.0, 3), (300.0, 19), (600.0, 24), (1200.0, 24)] {
        let mut world = free_space(3);
        add_slab(&mut world, BodyKind::Kinematic, [0.0, speed, 0.0]);
        add_block(&mut world, 2, 2, [10.0, 3.0, 10.0], [0.0; 3]);
        assert_eq!(
            world.steps_this_tick(),
            steps,
            "a slab that comes at {speed} voxels a second"
        );
    }
}

#[test]
fn what_counts_is_the_speed_the_two_close_at_and_the_speed_of_the_slab_where_the_cube_is() {
    // A slab coming up at 300 and a cube going down at 300 close at 600: more than either goes at (the cube alone would ask for 19 steps), so 24.
    let mut world = free_space(3);
    add_slab(&mut world, BodyKind::Kinematic, [0.0, 300.0, 0.0]);
    add_block(&mut world, 2, 2, [10.0, 3.0, 10.0], [0.0, -300.0, 0.0]);
    assert_eq!(world.steps_this_tick(), 24, "closing at 600");
    // A cube that turns adds the speed of its far corner to the speed it closes at: the same slab (156 voxels a second, 10 steps) asks for more steps when the cube turns at 5
    // radians a second (a reach of 3.5 voxels or more: 17 voxels a second, more than one step's worth of 16).
    let steps_of = |turn: f64| {
        let mut world = free_space(3);
        add_slab(&mut world, BodyKind::Kinematic, [0.0, 156.0, 0.0]);
        add_block(&mut world, 2, 2, [10.0, 3.0, 10.0], [0.0; 3]);
        let mut state = world.state(BodyId(2)).unwrap();
        state.angular_velocity = [0.0, turn, 0.0];
        world.set_state(BodyId(2), state).unwrap();
        world.steps_this_tick()
    };
    assert_eq!(steps_of(0.0), 10, "the slab alone");
    assert!(
        steps_of(5.0) > 10,
        "a cube that turns closes on the slab faster than one that does not"
    );
    // A slab that turns, 10 radians a second about its middle, and a cube at rest over its corner, 20 voxels from the middle of it: the ground under the cube goes at 200 voxels a
    // second (13 steps), though the slab goes nowhere.
    let mut world = free_space(3);
    let state = BodyState {
        position: WorldPosition::from_voxels([0.0, -32.0, 0.0]),
        orientation: Orientation::IDENTITY,
        linear_velocity: [0.0; 3],
        angular_velocity: [0.0, 10.0, 0.0],
    };
    world
        .add_body(BodyId(1), state, BodyKind::Kinematic)
        .unwrap();
    world
        .set_brick(BodyId(1), BrickCoordinate::ORIGIN, &mut Block(32))
        .unwrap();
    world.set_mass(BodyId(1), block_mass(32)).unwrap();
    add_block(&mut world, 2, 2, [30.0, 3.0, 30.0], [0.0; 3]);
    let steps = world.steps_this_tick();
    assert!(
        (12..=14).contains(&steps),
        "the ground under the cube goes at 200 voxels a second: {steps} steps"
    );
}

#[test]
fn a_slab_that_carries_the_cube_and_a_slab_with_no_ground_made_leave_the_steps_as_the_settings_say()
{
    // A slab at 30 voxels a second and a cube on it going the same way: they close on each other at nothing, and the tick is cut as the settings say (as it was).
    let mut world = free_space(3);
    add_slab(&mut world, BodyKind::Kinematic, [0.0, 30.0, 0.0]);
    add_block(&mut world, 2, 2, [10.0, 0.0, 10.0], [0.0, 30.0, 0.0]);
    assert_eq!(world.steps_this_tick(), 3, "a cube that goes with the slab");
    // A slab that has no bricks (a planet whose ground is not made yet, because nothing is near it) can touch nothing, however fast it goes.
    let mut world = free_space(3);
    let state = BodyState {
        position: WorldPosition::from_voxels([0.0, -32.0, 0.0]),
        orientation: Orientation::IDENTITY,
        linear_velocity: [0.0, 1200.0, 0.0],
        angular_velocity: [0.0; 3],
    };
    world
        .add_body(BodyId(1), state, BodyKind::Kinematic)
        .unwrap();
    add_block(&mut world, 2, 2, [10.0, 3.0, 10.0], [0.0; 3]);
    assert_eq!(world.steps_this_tick(), 3, "a slab with no ground made");
}

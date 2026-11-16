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
fn the_steps_a_world_has_taken_are_counted_as_the_ticks_cut_them() {
    // (What a cost counts, for a test of what the ground of a planet costs a body: a count of steps does not depend on how fast a machine is.) A cube at rest: the settings' 3
    // steps a tick. The same cube with a slab coming at it at 300 voxels a second: 19 in the first tick (the second asks for what the cube has become, so the count is of the first).
    let mut world = free_space(3);
    add_block(&mut world, 2, 2, [10.0, 3.0, 10.0], [0.0; 3]);
    assert_eq!(world.steps_taken(), 0, "nothing stepped yet");
    world.step();
    world.step();
    assert_eq!(world.steps_taken(), 6, "two ticks of 3 steps");
    let mut world = free_space(3);
    add_slab(&mut world, BodyKind::Kinematic, [0.0, 300.0, 0.0]);
    add_block(&mut world, 2, 2, [10.0, 3.0, 10.0], [0.0; 3]);
    let asked = world.steps_this_tick();
    world.step();
    assert_eq!(asked, 19, "the premise");
    assert_eq!(
        world.steps_taken(),
        u64::from(asked),
        "the steps of the tick"
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

#[test]
fn how_fast_a_body_goes_at_a_place_is_its_speed_and_its_turn_about_its_centre_of_mass() {
    // A slab that goes 7 voxels a second along x and turns 10 radians a second about the vertical axis through its centre of mass (16, -16, 16): the place (30, 3, 30) is 14 voxels
    // along x and z of the axis, where the turn goes (10 * 14, 0, -10 * 14) = (140, 0, -140), and the speed is added.
    let mut world = free_space(3);
    let state = BodyState {
        position: WorldPosition::from_voxels([0.0, -32.0, 0.0]),
        orientation: Orientation::IDENTITY,
        linear_velocity: [7.0, 0.0, 0.0],
        angular_velocity: [0.0, 10.0, 0.0],
    };
    world
        .add_body(BodyId(1), state, BodyKind::Kinematic)
        .unwrap();
    world
        .set_brick(BodyId(1), BrickCoordinate::ORIGIN, &mut Block(32))
        .unwrap();
    world.set_mass(BodyId(1), block_mass(32)).unwrap();
    let there = world
        .velocity_at(BodyId(1), WorldPosition::from_voxels([30.0, 3.0, 30.0]))
        .unwrap();
    for (axis, expected) in [147.0, 0.0, -140.0].into_iter().enumerate() {
        assert!(
            (there[axis] - expected).abs() < 1e-9,
            "axis {axis}: {there:?}"
        );
    }
    // On the axis there is only the speed. (And a body that is not there has none.)
    let on_the_axis = world
        .velocity_at(BodyId(1), WorldPosition::from_voxels([16.0, 3.0, 16.0]))
        .unwrap();
    assert!((on_the_axis[0] - 7.0).abs() < 1e-9 && on_the_axis[2].abs() < 1e-9);
    assert!(
        world
            .velocity_at(BodyId(9), WorldPosition::from_voxels([0.0; 3]))
            .is_none()
    );
}

#[test]
fn two_bodies_touch_when_the_last_step_had_a_contact_between_them_and_not_before_or_apart() {
    // A cube comes down at 10 voxels a second on a slab of ground, 3 voxels over it, with a second cube far above it (and a third body that is not there).
    let mut world = free_space(3);
    add_slab(&mut world, BodyKind::Fixed, [0.0; 3]);
    add_block(&mut world, 2, 2, [10.0, 3.0, 10.0], [0.0, -10.0, 0.0]);
    add_block(&mut world, 3, 2, [20.0, 500.0, 10.0], [0.0; 3]);
    assert!(
        !world.touches(BodyId(2), BodyId(1)),
        "before it has come down"
    );
    for _ in 0..30 {
        world.step();
    }
    assert!(
        world.touches(BodyId(2), BodyId(1)) && world.touches(BodyId(1), BodyId(2)),
        "the cube lies on the slab: each touches the other"
    );
    assert!(
        !world.touches(BodyId(3), BodyId(1)),
        "a cube far above does not"
    );
    assert!(!world.touches(BodyId(2), BodyId(3)));
    assert!(!world.touches(BodyId(2), BodyId(9)) && !world.touches(BodyId(9), BodyId(2)));
}

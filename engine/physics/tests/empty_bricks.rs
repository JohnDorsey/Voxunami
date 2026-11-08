//! A brick that a source says is empty changes nothing where a body had nothing, and takes everything it had where it had something.
//!
//! Found by timing a rocket at 6 km/s over a planet (`docs/terrain-collider-options.md`): the server asks for the colliders of about 1,900 bricks a tick, almost all of them air, and
//! `BodyWorld::set_brick` went through all 32,768 voxel places of each (19 microseconds) to find that there was nothing to do: 35 of the 50 milliseconds of a tick. A brick that is empty,
//! over a body that has no voxels or boxes there, is now left at once. The cost cannot be asserted here (what is simulated must not depend on a clock); what these tests hold is what
//! the shortcut must not change: the bits of the simulation, a brick that did have something, and the errors, in the order they come.

use std::collections::HashMap;
use std::sync::Arc;

use block_models::ShapeBox;
use physics::{
    BodyError, BodyId, BodyKind, BodyMass, BodySettings, BodyState, BodyWorld, BrickHint,
    Collision, CollisionSource, MassMoments,
};
use voxel_space::{BrickCoordinate, Orientation, VoxelCoordinate, WorldPosition};

const FLOOR: BodyId = BodyId(1);
const FALLER: BodyId = BodyId(2);

/// A source that says the brick is all air, as the sources of a game do for a brick that has one value, and counts what it is asked.
#[derive(Default)]
struct Air {
    voxels_read: usize,
    hints_asked: usize,
}

impl CollisionSource for Air {
    fn collision_at(&mut self, _: VoxelCoordinate) -> Collision {
        self.voxels_read += 1;
        Collision::Empty
    }

    fn brick_hint(&mut self, _: BrickCoordinate) -> BrickHint {
        self.hints_asked += 1;
        BrickHint::Empty
    }
}

/// The voxels of a brick that has no hint, so every voxel is asked.
struct Voxels(HashMap<VoxelCoordinate, Collision>);

impl CollisionSource for Voxels {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        self.0.get(&voxel).cloned().unwrap_or(Collision::Empty)
    }
}

/// A floor of 8 by 8 voxels, one voxel thick: whole cubes, or a slab that fills the lower half of each, which is a box and not a voxel of the body's voxel collider.
fn floor(whole_cubes: bool) -> Voxels {
    let mut voxels = HashMap::new();
    for x in 0..8 {
        for z in 0..8 {
            let collision = if whole_cubes {
                Collision::Full
            } else {
                Collision::Partial(Arc::from(vec![ShapeBox {
                    min: [0, 0, 0],
                    max: [8, 4, 8],
                }]))
            };
            voxels.insert(VoxelCoordinate::new(x, 0, z), collision);
        }
    }
    Voxels(voxels)
}

/// The mass of a block of `size` voxels from the origin of its body.
fn block_mass(size: [i128; 3]) -> BodyMass {
    let voxels = (0..size[0]).flat_map(|x| {
        (0..size[1])
            .flat_map(move |y| (0..size[2]).map(move |z| MassMoments::of_voxel([x, y, z], 1.0)))
    });
    MassMoments::sum(voxels)
        .properties()
        .expect("a block has a mass")
}

fn at_rest(place: [f64; 3]) -> BodyState {
    BodyState {
        position: WorldPosition::from_voxels(place),
        orientation: Orientation::IDENTITY,
        linear_velocity: [0.0; 3],
        angular_velocity: [0.0; 3],
    }
}

fn height_of(world: &BodyWorld, id: BodyId) -> f64 {
    let position = world.state(id).unwrap().position;
    position.brick().y() as f64 * 32.0 + position.offset()[1]
}

fn run(world: &mut BodyWorld, ticks: usize) {
    for _ in 0..ticks {
        world.step();
    }
}

/// A fixed floor, and a cube of 2 voxels let go over it, at rest on it after 80 ticks.
fn a_cube_resting_on(mut floor: Voxels) -> BodyWorld {
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings::default());
    world
        .add_body(FLOOR, at_rest([0.0; 3]), BodyKind::Fixed)
        .unwrap();
    world
        .set_brick(FLOOR, BrickCoordinate::ORIGIN, &mut floor)
        .unwrap();
    world.set_mass(FLOOR, block_mass([8, 1, 8])).unwrap();
    world
        .add_body(FALLER, at_rest([3.0, 3.0, 3.0]), BodyKind::Dynamic)
        .unwrap();
    let mut cube = Voxels(HashMap::new());
    for x in 0..2 {
        for y in 0..2 {
            for z in 0..2 {
                cube.0
                    .insert(VoxelCoordinate::new(x, y, z), Collision::Full);
            }
        }
    }
    world
        .set_brick(FALLER, BrickCoordinate::ORIGIN, &mut cube)
        .unwrap();
    world.set_mass(FALLER, block_mass([2, 2, 2])).unwrap();
    run(&mut world, 80);
    world
}

#[test]
fn an_empty_brick_where_a_body_had_nothing_is_not_read_and_changes_no_bit_of_the_simulation() {
    let (mut with, mut without) = (
        a_cube_resting_on(floor(true)),
        a_cube_resting_on(floor(true)),
    );
    let mut air = Air::default();
    // Bricks beside the floor's, and above and below, where the floor has nothing.
    for brick in [
        BrickCoordinate::new(1, 0, 0),
        BrickCoordinate::new(0, 1, 0),
        BrickCoordinate::new(0, -1, 0),
        BrickCoordinate::new(-3, 7, 2),
    ] {
        with.set_brick(FLOOR, brick, &mut air).unwrap();
        with.remove_brick(FALLER, brick).unwrap();
    }
    assert_eq!(
        (air.hints_asked, air.voxels_read),
        (4, 0),
        "an empty brick is asked about once and its voxels not at all"
    );
    assert_eq!(with.state_hash(), without.state_hash());
    run(&mut with, 40);
    run(&mut without, 40);
    assert_eq!(with.state_hash(), without.state_hash());
    // (And the cube still stands on the floor it was on.)
    assert!((height_of(&with, FALLER) - 1.0).abs() < 0.05);
}

#[test]
fn an_empty_brick_where_a_body_had_whole_cubes_takes_them_and_what_rests_on_them_falls() {
    let mut world = a_cube_resting_on(floor(true));
    assert!((height_of(&world, FALLER) - 1.0).abs() < 0.05);
    world
        .set_brick(FLOOR, BrickCoordinate::ORIGIN, &mut Air::default())
        .unwrap();
    run(&mut world, 40);
    // (Free fall at 5 for two seconds is 10 voxels.)
    assert!(
        height_of(&world, FALLER) < -8.0,
        "{}",
        height_of(&world, FALLER)
    );
}

#[test]
fn an_empty_brick_where_a_body_had_only_boxes_takes_them_and_what_rests_on_them_falls() {
    // A slab is a box of its own and not a voxel of the voxel collider, so the body has nothing filled in the brick, only a collider of boxes.
    let mut world = a_cube_resting_on(floor(false));
    assert!(
        (height_of(&world, FALLER) - 0.5).abs() < 0.05,
        "{}",
        height_of(&world, FALLER)
    );
    world
        .set_brick(FLOOR, BrickCoordinate::ORIGIN, &mut Air::default())
        .unwrap();
    run(&mut world, 40);
    assert!(
        height_of(&world, FALLER) < -8.0,
        "{}",
        height_of(&world, FALLER)
    );
}

#[test]
fn an_empty_brick_is_still_refused_for_a_body_that_is_not_there_or_a_brick_that_is_too_far() {
    let mut world = a_cube_resting_on(floor(true));
    assert_eq!(
        world.set_brick(BodyId(9), BrickCoordinate::ORIGIN, &mut Air::default()),
        Err(BodyError::UnknownBody(BodyId(9)))
    );
    let mut air = Air::default();
    // A brick 2^27 bricks along is 2^32 voxels: beyond what the coordinates of a collider hold.
    assert_eq!(
        world.set_brick(FLOOR, BrickCoordinate::new(1 << 27, 0, 0), &mut air),
        Err(BodyError::BrickOutOfRange)
    );
    assert_eq!(
        air.hints_asked, 0,
        "a brick out of range is refused before it is asked about"
    );
}

//! The colliders of a body can be taken out of one world and given to the same body in another, so that a ship that goes from one frame of the solver to the next does not have
//! every brick of it looked at again (`docs/history/build-log.md`, "Joints, step 4c-0": the two scans of a brick were all of the 3 ms (900 blocks) to 48 ms (101,614 blocks) of the
//! tick in which a ship entered a frame, and the collider scan was four fifths of that).
//!
//! What a carry must not change is what the body does: a body that was given the colliders of another must move, bit for bit, as a body whose bricks were made from the same voxels, and
//! must be edited, brick by brick, as that one is. (What the carry saves cannot be asserted here: nothing that is simulated may depend on a clock. The plugin's test counts the bricks.)

use std::collections::HashMap;
use std::sync::Arc;

use block_models::ShapeBox;
use physics::{
    BodyColliders, BodyError, BodyId, BodyKind, BodyMass, BodySettings, BodyState, BodyWorld,
    Collision, CollisionSource, MassMoments,
};
use voxel_space::{BrickCoordinate, Orientation, VoxelCoordinate, WorldPosition};

const FLOOR: BodyId = BodyId(1);
const HULL: BodyId = BodyId(7);

/// The voxels of a body that a source answers for.
struct Voxels(HashMap<VoxelCoordinate, Collision>);

impl CollisionSource for Voxels {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        self.0.get(&voxel).cloned().unwrap_or(Collision::Empty)
    }
}

fn slab() -> Collision {
    Collision::Partial(Arc::from(vec![ShapeBox {
        min: [0, 0, 0],
        max: [8, 4, 8],
    }]))
}

/// A deck of whole cubes 6 by 2 by 6, a slab along one side of it, and a second brick of cubes (a third of a ship, in two bricks, with a voxel collider and a box compound).
fn hull() -> Voxels {
    let mut voxels = HashMap::new();
    for x in 0..6 {
        for y in 0..2 {
            for z in 0..6 {
                voxels.insert(VoxelCoordinate::new(x, y, z), Collision::Full);
            }
        }
    }
    for z in 0..6 {
        voxels.insert(VoxelCoordinate::new(6, 0, z), slab());
    }
    for x in 32..36 {
        for y in 0..2 {
            for z in 0..4 {
                voxels.insert(VoxelCoordinate::new(x, y, z), Collision::Full);
            }
        }
    }
    Voxels(voxels)
}

/// The same, edited: the half of the underside of the deck that is at the low x gone (cubes that were set and are not any more: the hull comes down on the other half), a cube of
/// the top turned into a slab, and the slab along the side made a box of another size.
fn edited_hull() -> Voxels {
    let mut edited = hull();
    for x in 0..3 {
        for z in 0..6 {
            edited.0.remove(&VoxelCoordinate::new(x, 0, z));
        }
    }
    edited.0.insert(VoxelCoordinate::new(3, 1, 3), slab());
    for z in 0..6 {
        edited.0.insert(
            VoxelCoordinate::new(6, 0, z),
            Collision::Partial(Arc::from(vec![ShapeBox {
                min: [0, 0, 0],
                max: [4, 8, 8],
            }])),
        );
    }
    edited
}

fn bricks() -> [BrickCoordinate; 2] {
    [BrickCoordinate::ORIGIN, BrickCoordinate::new(1, 0, 0)]
}

fn block_mass() -> BodyMass {
    let voxels = (0..6).flat_map(|x| {
        (0..2).flat_map(move |y| (0..6).map(move |z| MassMoments::of_voxel([x, y, z], 1.0)))
    });
    MassMoments::sum(voxels)
        .properties()
        .expect("a deck has a mass")
}

fn state(place: [f64; 3], linear: [f64; 3], angular: [f64; 3]) -> BodyState {
    BodyState {
        position: WorldPosition::from_voxels(place),
        orientation: Orientation::IDENTITY,
        linear_velocity: linear,
        angular_velocity: angular,
    }
}

/// A fixed floor of whole cubes 40 by 1 by 40, with its top at height 0.
fn world_with_a_floor() -> BodyWorld {
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings::default());
    world
        .add_body(
            FLOOR,
            state([0.0, -1.0, 0.0], [0.0; 3], [0.0; 3]),
            BodyKind::Fixed,
        )
        .unwrap();
    let mut cubes = HashMap::new();
    for x in 0..32 {
        for z in 0..32 {
            cubes.insert(VoxelCoordinate::new(x, 0, z), Collision::Full);
        }
    }
    let mut floor = Voxels(cubes);
    world
        .set_brick(FLOOR, BrickCoordinate::ORIGIN, &mut floor)
        .unwrap();
    world
        .set_mass(
            FLOOR,
            MassMoments::sum((0..4).map(|x| MassMoments::of_voxel([x, 0, 0], 1.0)))
                .properties()
                .unwrap(),
        )
        .unwrap();
    world
}

/// Where the hull starts: above the floor, going across it and turning, so that it comes down on an edge and a corner.
fn the_start() -> BodyState {
    state([4.0, 5.0, 4.0], [1.5, 0.0, 0.5], [0.4, 0.3, 0.2])
}

/// A world with a floor and the hull, its colliders made from the voxels of `source`.
fn made_from(mut source: Voxels) -> BodyWorld {
    let mut world = world_with_a_floor();
    world
        .add_body(HULL, the_start(), BodyKind::Dynamic)
        .unwrap();
    for brick in bricks() {
        world.set_brick(HULL, brick, &mut source).unwrap();
    }
    world.set_mass(HULL, block_mass()).unwrap();
    world
}

/// A world with a floor and the hull, its colliders the ones taken out of `donor`'s body.
fn carried_from(donor: &mut BodyWorld) -> BodyWorld {
    let colliders = donor.take_colliders(HULL).unwrap();
    let mut world = world_with_a_floor();
    world
        .add_body(HULL, the_start(), BodyKind::Dynamic)
        .unwrap();
    world.give_colliders(HULL, colliders).unwrap();
    world.set_mass(HULL, block_mass()).unwrap();
    world
}

fn run(world: &mut BodyWorld, ticks: usize) {
    for _ in 0..ticks {
        world.step();
    }
}

fn hull_state(world: &BodyWorld) -> BodyState {
    world.state(HULL).unwrap()
}

#[test]
fn a_body_given_the_colliders_taken_from_another_world_moves_as_one_whose_bricks_were_made_from_the_voxels()
 {
    let mut donor = made_from(hull());
    let mut made = made_from(hull());
    let mut carried = carried_from(&mut donor);
    // The control: a hull with no colliders at all falls through the floor, so the test would see a carry that carried nothing.
    let mut bare = world_with_a_floor();
    bare.add_body(HULL, the_start(), BodyKind::Dynamic).unwrap();
    bare.set_mass(HULL, block_mass()).unwrap();
    for tick in 1..=120 {
        run(&mut made, 1);
        run(&mut carried, 1);
        run(&mut bare, 1);
        assert_eq!(
            carried.state_hash(),
            made.state_hash(),
            "tick {tick}: the carried hull is not where the made one is: {:?} against {:?}",
            hull_state(&carried),
            hull_state(&made)
        );
    }
    assert_eq!(hull_state(&carried), hull_state(&made), "bit for bit");
    // It came down on the floor and did not go through it: the floor's top is at height 0, so a hull at rest on it has its origin (the underside of its deck) within a hair of it.
    let landed = hull_state(&made).position;
    let height = landed.brick().y() as f64 * 32.0 + landed.offset()[1];
    assert!(
        height.abs() < 0.01,
        "the made hull rests on the floor, at height {height}"
    );
    assert_ne!(
        bare.state_hash(),
        made.state_hash(),
        "a hull with no colliders does not do what one with them does"
    );
}

#[test]
fn a_world_the_colliders_were_taken_from_no_longer_has_them() {
    let mut donor = made_from(hull());
    assert!(
        donor.bricks_near(HULL, 1.0).is_some(),
        "the hull has colliders before"
    );
    let kept = donor.take_colliders(HULL).unwrap();
    assert_eq!(
        donor.bricks_near(HULL, 1.0),
        None,
        "and none after: a body with no blocks has no bricks"
    );
    // The body is still there, and can be given colliders again (the same ones, back).
    assert!(donor.state(HULL).is_some());
    donor.give_colliders(HULL, kept).unwrap();
    assert!(donor.bricks_near(HULL, 1.0).is_some());
}

#[test]
fn a_carried_body_is_edited_brick_by_brick_as_one_made_there_is() {
    let mut donor = made_from(hull());
    let mut made = made_from(hull());
    let mut carried = carried_from(&mut donor);
    run(&mut made, 5);
    run(&mut carried, 5);
    // The edit takes cubes away (which only a world that remembers which voxels were set can do), makes another a box, and makes the box compound of the side another one.
    let mut edit = edited_hull();
    for brick in bricks() {
        made.set_brick(HULL, brick, &mut edit).unwrap();
        carried.set_brick(HULL, brick, &mut edit).unwrap();
    }
    for tick in 1..=120 {
        run(&mut made, 1);
        run(&mut carried, 1);
        assert_eq!(
            carried.state_hash(),
            made.state_hash(),
            "tick {tick} after the edit: {:?} against {:?}",
            hull_state(&carried),
            hull_state(&made)
        );
    }
    // And the edit of one that was not carried gives a different hull from the unedited, so the edit was seen.
    let mut unedited = made_from(hull());
    run(&mut unedited, 125);
    assert_ne!(unedited.state_hash(), made.state_hash());
}

#[test]
fn a_body_with_no_colliders_carries_none_and_gives_none() {
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings::default());
    world
        .add_body(HULL, the_start(), BodyKind::Dynamic)
        .unwrap();
    let nothing = world.take_colliders(HULL).unwrap();
    let mut other = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings::default());
    other
        .add_body(HULL, the_start(), BodyKind::Dynamic)
        .unwrap();
    other.give_colliders(HULL, nothing).unwrap();
    assert_eq!(world.bricks_near(HULL, 1.0), None);
    assert_eq!(other.bricks_near(HULL, 1.0), None);
}

#[test]
fn colliders_given_to_a_body_are_all_it_has_and_what_it_had_is_gone() {
    let mut donor = made_from(hull());
    let mut made = made_from(hull());
    // A body that has a brick of its own, far from the hull's, is given the hull's colliders.
    let colliders = donor.take_colliders(HULL).unwrap();
    let mut world = world_with_a_floor();
    world
        .add_body(HULL, the_start(), BodyKind::Dynamic)
        .unwrap();
    let mut stray = Voxels(HashMap::new());
    stray
        .0
        .insert(VoxelCoordinate::new(3 * 32, 0, 0), Collision::Full);
    world
        .set_brick(HULL, BrickCoordinate::new(3, 0, 0), &mut stray)
        .unwrap();
    world.give_colliders(HULL, colliders).unwrap();
    world.set_mass(HULL, block_mass()).unwrap();
    assert_eq!(
        format!("{:?}", world.bricks_near(HULL, 1.0)),
        format!("{:?}", made.bricks_near(HULL, 1.0)),
        "the stray brick is gone: the reach of the hull is the reach of the made one"
    );
    run(&mut world, 60);
    run(&mut made, 60);
    assert_eq!(world.state_hash(), made.state_hash());
}

#[test]
fn a_body_that_is_not_there_has_no_colliders_to_take_or_to_be_given() {
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings::default());
    assert_eq!(
        world.take_colliders(HULL).err(),
        Some(BodyError::UnknownBody(HULL))
    );
    assert_eq!(
        world.give_colliders(HULL, BodyColliders::default()).err(),
        Some(BodyError::UnknownBody(HULL))
    );
}

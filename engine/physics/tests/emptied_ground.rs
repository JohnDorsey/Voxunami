//! A body that is a ground (a planet on a rail: a kinematic body) keeps its voxels in one collider, and the world takes bricks of it away where nothing is near and makes them
//! where something comes. **When the last brick goes the collider has no voxel and no bounds** (its box is not a number), and Rapier takes a collider like that out of play for
//! good at the end of the step (its "quarantine": the collider is disabled, and the voxels that are put in it later are never met). A thing that was thrown at the ground of a
//! planet and bounced away, so that the ground round it was let go, came back and went through the ground (found 2026-11-17: `docs/history/build-log.md`, "The bounce,
//! investigated"; 5 of 9 throws in the game).
//!
//! The world therefore takes the collider out when the last voxel goes (`BodyWorld::set_brick`), and makes it again with the next brick that has a voxel.

mod block_bodies;

use block_bodies::{add_block, block_mass};
use physics::{
    BodyId, BodyKind, BodySettings, BodyState, BodyWorld, BrickHint, Collision, CollisionSource,
};
use std::collections::BTreeSet;
use voxel_space::{BrickCoordinate, Orientation, VoxelCoordinate, WorldPosition};

const GROUND: BodyId = BodyId(100);
const THING: u64 = 1;

/// Every voxel below the height 0 is a whole cube.
struct Floor;

impl CollisionSource for Floor {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        if voxel.y() < 0 {
            Collision::Full
        } else {
            Collision::Empty
        }
    }

    fn brick_hint(&mut self, brick: BrickCoordinate) -> BrickHint {
        if brick.y() < 0 {
            BrickHint::Solid
        } else {
            BrickHint::Empty
        }
    }
}

fn game_settings() -> BodySettings {
    BodySettings {
        gravity: [0.0, -8.0, 0.0],
        ..BodySettings::default()
    }
}

/// A world with a ground that goes at no speed, as a planet on a rail that stands still does: a kinematic body, which Rapier advances (and so checks) every step.
fn world_with_a_ground() -> BodyWorld {
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, game_settings());
    let at_rest = BodyState {
        position: WorldPosition::from_voxels([0.0; 3]),
        orientation: Orientation::IDENTITY,
        linear_velocity: [0.0; 3],
        angular_velocity: [0.0; 3],
    };
    world
        .add_body(GROUND, at_rest, BodyKind::Kinematic)
        .unwrap();
    world
}

fn make_floor_under(world: &mut BodyWorld, bricks: impl IntoIterator<Item = (i128, i128)>) {
    for (x, z) in bricks {
        world
            .set_brick(GROUND, BrickCoordinate::new(x, -1, z), &mut Floor)
            .unwrap();
    }
}

fn height_of(world: &BodyWorld, id: u64) -> f64 {
    world.state(BodyId(id)).unwrap().position.in_voxels()[1]
}

#[test]
fn a_ground_that_lost_all_its_bricks_has_colliders_again_when_bricks_are_made_far_from_where_they_were()
 {
    let mut world = world_with_a_ground();
    // The ground is made where something is, and let go brick by brick as it goes (a tick for each).
    let first_place = [
        (-1, -1),
        (0, -1),
        (1, -1),
        (-1, 0),
        (0, 0),
        (1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
    ];
    make_floor_under(&mut world, first_place);
    world.step();
    for (x, z) in first_place {
        world
            .remove_brick(GROUND, BrickCoordinate::new(x, -1, z))
            .unwrap();
        world.step();
    }
    for _ in 0..3 {
        world.step();
    }
    // It comes down again, 300 voxels from there, where the ground is made again.
    let second_place = first_place.map(|(x, z)| (x + 10, z));
    make_floor_under(&mut world, second_place);
    add_block(&mut world, THING, 2, [330.0, 12.0, 10.0], [0.0; 3]);
    world.set_mass(BodyId(THING), block_mass(2)).unwrap();
    for _ in 0..120 {
        world.step();
    }
    let height = height_of(&world, THING);
    assert!(
        (-0.5..1.5).contains(&height),
        "the cube is at {height}: it fell on the ground that was made again, 300 voxels from where it had been, and went through it (the ground's collider was out of play, and the ground's place in the broad phase was where it had been)"
    );
}

/// What the plugin does for the ground of a planet: each tick the bricks within the margin of the thing are made and those farther than the margin and the keeping are let go
/// (the margins are the plugin's for a thing going at `speed`: 2 voxels and a tenth of a second, 8 voxels and a quarter of a second, and 16 voxels more to keep).
fn drop_over_ground_that_is_kept(speed: f64, spin: f64, ticks: usize) -> f64 {
    let mut world = world_with_a_ground();
    let state = BodyState {
        position: WorldPosition::from_voxels([0.0, 400.0, 0.0]),
        orientation: Orientation::IDENTITY,
        linear_velocity: [0.0, -speed, 0.0],
        angular_velocity: [0.0, 0.0, spin],
    };
    block_bodies::add_block(&mut world, THING, 6, [0.0, 400.0, 0.0], [0.0, -speed, 0.0]);
    world
        .set_state(BodyId(THING), state)
        .expect("the cube can be given its spin");
    let mut made: BTreeSet<(i128, i128, i128)> = BTreeSet::new();
    let mut lowest = f64::INFINITY;
    for _ in 0..ticks {
        let now = world.state(BodyId(THING)).unwrap().linear_velocity;
        let fast = now.iter().map(|part| part * part).sum::<f64>().sqrt();
        let needed = world
            .bricks_near_in(BodyId(THING), 2.0 + 0.1 * fast, GROUND)
            .unwrap();
        let keeping = world
            .bricks_near_in(BodyId(THING), 8.0 + 0.25 * fast + 16.0, GROUND)
            .unwrap();
        let to_drop: Vec<_> = made
            .iter()
            .copied()
            .filter(|&(x, y, z)| !keeping.contains(BrickCoordinate::new(x, y, z)))
            .collect();
        for (x, y, z) in to_drop {
            world
                .remove_brick(GROUND, BrickCoordinate::new(x, y, z))
                .unwrap();
            made.remove(&(x, y, z));
        }
        for x in needed.min().x()..=needed.max().x() {
            for y in needed.min().y()..=needed.max().y() {
                for z in needed.min().z()..=needed.max().z() {
                    if made.insert((x, y, z)) {
                        world
                            .set_brick(GROUND, BrickCoordinate::new(x, y, z), &mut Floor)
                            .unwrap();
                    }
                }
            }
        }
        world.step();
        lowest = lowest.min(height_of(&world, THING));
    }
    lowest
}

#[test]
fn a_cube_thrown_at_ground_that_is_made_as_it_comes_and_let_go_as_it_goes_does_not_go_through_it() {
    // (A cube of 6 that spins as it comes: it lands on a corner, bounces off and comes down again, the ground round it let go in between. Before the world took the collider
    // out when the last voxel went, 150 voxels a second with 3 radians a second went 85 voxels under the ground, and 300 with 22 went 132.)
    let mut wrong = Vec::new();
    for (speed, spin) in [(150.0, 3.0), (300.0, 22.0), (67.0, 3.0), (300.0, 0.0)] {
        let lowest = drop_over_ground_that_is_kept(speed, spin, 200);
        if lowest < -2.0 {
            wrong.push(format!(
                "at {speed} voxels a second with a spin of {spin} the cube went down to {lowest}"
            ));
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

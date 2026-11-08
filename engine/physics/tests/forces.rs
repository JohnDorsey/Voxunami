//! A force on a ship at a place (an engine) turns it about its centre of mass as the mass was last set, in the tick it was set as in every other.
//!
//! Found by flying a ship out from a planet: every time the ship was taken out of one frame of the solver and put in another (twice as far from the frame's origin as a ship
//! may be, 32 km), its body was made, given its mass and pushed by its engines in the one tick, and Rapier turned the push about the *origin* of the ship and not about its
//! centre of mass (the centre it knows is only brought up to date by the step): a ship whose engines push it straight through its centre turned at 0.6 radians a second for
//! that tick, and the pilot's seat took 9 ticks to stop it.

use physics::{BodyId, BodyKind, BodyMass, BodySettings, BodyState, BodyWorld, MassMoments};
use voxel_space::{BrickCoordinate, Orientation, WorldPosition};

const SHIP: BodyId = BodyId(1);
/// The mass of a voxel, in kilograms.
const DENSITY: f64 = 5.0;

fn free_space() -> BodyWorld {
    BodyWorld::new(
        BrickCoordinate::ORIGIN,
        BodySettings {
            gravity: [0.0; 3],
            ..BodySettings::default()
        },
    )
}

/// The mass of a block of voxels `size` voxels along each axis from the origin of the ship: its centre of mass is the middle of the block, not the origin.
fn block_mass(size: [i128; 3]) -> BodyMass {
    let voxels = (0..size[0]).flat_map(|x| {
        (0..size[1])
            .flat_map(move |y| (0..size[2]).map(move |z| MassMoments::of_voxel([x, y, z], DENSITY)))
    });
    MassMoments::sum(voxels)
        .properties()
        .expect("a block has a mass")
}

fn at_rest() -> BodyState {
    BodyState {
        position: WorldPosition::from_voxels([100.0, 100.0, 100.0]),
        orientation: Orientation::IDENTITY,
        linear_velocity: [0.0; 3],
        angular_velocity: [0.0; 3],
    }
}

fn length(vector: [f64; 3]) -> f64 {
    vector.iter().map(|part| part * part).sum::<f64>().sqrt()
}

#[test]
fn a_force_through_the_centre_of_mass_does_not_turn_a_ship_in_the_tick_its_body_is_made() {
    let mut world = free_space();
    world.add_body(SHIP, at_rest(), BodyKind::Dynamic).unwrap();
    // A block 8 by 2 by 2 from the origin of the ship: the centre of mass is at 4, 1, 1.
    let mass = block_mass([8, 2, 2]);
    world.set_mass(SHIP, mass).unwrap();
    // Two engines, one each side of the centre, push alike: through the centre of mass, up. (The tick the body is made in is the first to push it.)
    world
        .apply_force(SHIP, [0.0, 100.0, 0.0], Some([1.0, 1.0, 1.0]))
        .unwrap();
    world
        .apply_force(SHIP, [0.0, 100.0, 0.0], Some([7.0, 1.0, 1.0]))
        .unwrap();
    world.step();
    let state = world.state(SHIP).unwrap();
    assert!(
        length(state.angular_velocity) < 1e-9,
        "turning at {:?} radians a second",
        state.angular_velocity
    );
    // 200 newtons on the whole mass (32 voxels of 5 kilograms) for a twentieth of a second.
    assert_eq!(mass.mass, 160.0);
    assert!((state.linear_velocity[1] - 200.0 / mass.mass / 20.0).abs() < 1e-9);
}

#[test]
fn a_force_off_the_centre_of_mass_turns_a_ship_by_its_lever_and_its_inertia_in_the_tick_its_body_is_made()
 {
    let mut world = free_space();
    world.add_body(SHIP, at_rest(), BodyKind::Dynamic).unwrap();
    // A block 2 by 2 by 2: the centre of mass is at 1, 1, 1, and the inertia about any axis through it is 40 * (4 + 4) / 12 for a mass of 40 (as the density is 5).
    world.set_mass(SHIP, block_mass([2, 2, 2])).unwrap();
    // One voxel along x from the centre of mass, pushed up: a torque of 100 about z, for the one tick.
    world
        .apply_force(SHIP, [0.0, 100.0, 0.0], Some([2.0, 1.0, 1.0]))
        .unwrap();
    world.step();
    let expected = 100.0 / (40.0 * 8.0 / 12.0) / 20.0;
    let turning = world.state(SHIP).unwrap().angular_velocity;
    assert!(
        (turning[2] - expected).abs() < 1e-6 * expected
            && turning[0].abs() < 1e-9
            && turning[1].abs() < 1e-9,
        "{turning:?} against {expected} about z"
    );
    // The push was for that tick alone: a tick later it turns as fast as it did, and no faster.
    world.step();
    let later = world.state(SHIP).unwrap().angular_velocity;
    assert!((later[2] - expected).abs() < 1e-6 * expected, "{later:?}");
}

#[test]
fn a_ship_whose_mass_changes_while_it_is_pushed_is_pushed_about_the_new_centre_at_once() {
    let mut world = free_space();
    world.add_body(SHIP, at_rest(), BodyKind::Dynamic).unwrap();
    world.set_mass(SHIP, block_mass([2, 2, 2])).unwrap();
    world
        .apply_force(SHIP, [0.0, 100.0, 0.0], Some([1.0, 1.0, 1.0]))
        .unwrap();
    world.step();
    assert!(length(world.state(SHIP).unwrap().angular_velocity) < 1e-9);
    // A wing is built on: the centre of mass moves to 4, 1, 1, and the engine under it pushes through that, in the tick the blocks are placed in.
    world.set_mass(SHIP, block_mass([8, 2, 2])).unwrap();
    world
        .apply_force(SHIP, [0.0, 100.0, 0.0], Some([4.0, 1.0, 1.0]))
        .unwrap();
    world.step();
    let turning = world.state(SHIP).unwrap().angular_velocity;
    assert!(
        length(turning) < 1e-9,
        "turning at {turning:?} radians a second"
    );
}

//! A body that turns very slowly goes on turning: the turn of a ship on a rail is for good (step 4c-2). Rapier's own rule would put a body that turns at 0.002 radians a second to
//! sleep within two seconds and zero its turn (the ship audit's PHY-2); the rule of this world is that a body sleeps only while it touches something, and a body told with
//! `BodyWorld::set_can_sleep` that it may not (made for the cars of a mechanism) does not sleep even then. A rider touches nothing, so it is covered twice.

use physics::{BodyId, BodyKind, BodySettings, BodyState, BodyWorld};
use voxel_space::{BrickCoordinate, Orientation, WorldPosition};

const SHIP: BodyId = BodyId(1);
const SPIN: [f64; 3] = [0.0, 0.002, 0.0];

fn a_slow_turn_in_free_space() -> BodyWorld {
    let mut world = BodyWorld::new(
        BrickCoordinate::ORIGIN,
        BodySettings {
            gravity: [0.0; 3],
            ..BodySettings::default()
        },
    );
    world
        .add_body(
            SHIP,
            BodyState {
                position: WorldPosition::from_voxels([100.0, 100.0, 100.0]),
                orientation: Orientation::IDENTITY,
                linear_velocity: [0.0; 3],
                angular_velocity: SPIN,
            },
            BodyKind::Dynamic,
        )
        .unwrap();
    world
}

fn turn_after(world: &mut BodyWorld, steps: usize) -> [f64; 3] {
    for _ in 0..steps {
        world.step();
    }
    world.state(SHIP).unwrap().angular_velocity
}

#[test]
fn a_body_that_turns_very_slowly_and_touches_nothing_goes_on_turning() {
    // Rapier's own rule put it to sleep and zeroed its turn within 200 steps (the ship audit's PHY-2); a body sleeps only while it touches something
    // (`sleep_only_in_contact.rs`), which is what keeps a rider's turn.
    let mut world = a_slow_turn_in_free_space();
    let turn = turn_after(&mut world, 200);
    for axis in 0..3 {
        assert!(
            (turn[axis] - SPIN[axis]).abs() < 1e-12,
            "{SPIN:?} became {turn:?}"
        );
    }
    assert!(!world.is_asleep(SHIP));
}

#[test]
fn a_body_that_may_not_sleep_goes_on_turning() {
    let mut world = a_slow_turn_in_free_space();
    world.set_can_sleep(SHIP, false).unwrap();
    let turn = turn_after(&mut world, 200);
    for axis in 0..3 {
        // (Not to the bit: the solver's own rounding is 1e-15 of it.)
        assert!(
            (turn[axis] - SPIN[axis]).abs() < 1e-12,
            "{SPIN:?} became {turn:?}"
        );
    }
    assert!(!world.is_asleep(SHIP));
}

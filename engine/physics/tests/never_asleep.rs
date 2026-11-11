//! A body that is told it may not sleep (`BodyWorld::set_can_sleep`, made for the cars of a mechanism) goes on turning however slowly: step 4c-2 uses it for a ship on a rail,
//! which turns in a frame of its own, and a body that turns at 0.002 radians a second is put to sleep by Rapier within two seconds, which zeroes its turn (the ship audit's PHY-2).

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
fn a_body_that_turns_very_slowly_is_put_to_sleep_and_stops_as_the_audit_found() {
    let mut world = a_slow_turn_in_free_space();
    assert_eq!(turn_after(&mut world, 200), [0.0; 3]);
    assert!(world.is_asleep(SHIP));
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

#[test]
fn a_body_that_may_not_sleep_and_is_asleep_wakes_and_one_that_may_again_sleeps() {
    let mut world = a_slow_turn_in_free_space();
    turn_after(&mut world, 200);
    assert!(world.is_asleep(SHIP), "it went to sleep");
    world.set_can_sleep(SHIP, false).unwrap();
    assert!(!world.is_asleep(SHIP), "it is awake when it may not sleep");
    world.set_can_sleep(SHIP, true).unwrap();
    turn_after(&mut world, 200);
    assert!(world.is_asleep(SHIP), "and it sleeps again when it may");
}
